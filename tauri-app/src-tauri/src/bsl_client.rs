//! BSL Language Server client
//! Communicates with BSL LS via WebSocket using JSON-RPC

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::process::{Command as StdCommand, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;
use tokio::process::{Child, Command as AsyncCommand};
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

use crate::mcp_client::{InternalMcpHandler, McpClient, McpTool};
use crate::settings::load_settings;
use async_trait::async_trait;
use serde_json::json;
use std::sync::{Arc, Mutex as StdMutex};
use std::collections::HashMap;
use tokio::sync::{oneshot, mpsc};
use tauri::{AppHandle, Emitter};

fn native_server_args(port: u16) -> Vec<String> {
    vec![
        "websocket".to_string(),
        "--mcp".to_string(),
        format!("--server.port={port}"),
    ]
}

const BSL_TOMCAT_BUFFER_SIZE_JVM_OPT: &str =
    "-Dorg.apache.tomcat.websocket.DEFAULT_BUFFER_SIZE=1048576";

fn native_server_envs() -> Vec<(String, String)> {
    let java_tool_options = match std::env::var("JAVA_TOOL_OPTIONS") {
        Ok(existing) if !existing.trim().is_empty() => {
            if existing.contains("org.apache.tomcat.websocket.DEFAULT_BUFFER_SIZE") {
                existing
            } else {
                format!("{existing} {BSL_TOMCAT_BUFFER_SIZE_JVM_OPT}")
            }
        }
        _ => BSL_TOMCAT_BUFFER_SIZE_JVM_OPT.to_string(),
    };

    vec![("JAVA_TOOL_OPTIONS".to_string(), java_tool_options)]
}

fn request_timeout_for(method: &str) -> Duration {
    if method == "textDocument/diagnostic" {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(15)
    }
}

fn should_reuse_existing_listener(mcp_required: bool) -> bool {
    !mcp_required
}

fn official_mcp_endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

const BSL_UPSTREAM_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(1200);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServerLaunchSpec {
    program: String,
    args: Vec<String>,
    envs: Vec<(String, String)>,
    mcp_enabled: bool,
}

fn server_launch_spec(
    settings: &crate::settings::BSLServerSettings,
    port: u16,
) -> Result<ServerLaunchSpec, String> {
    if native_launcher_available(settings) {
        return Ok(ServerLaunchSpec {
            program: settings.executable_path.clone(),
            args: native_server_args(port),
            envs: native_server_envs(),
            mcp_enabled: true,
        });
    }

    if settings.jar_path.trim().is_empty() {
        return Err(
            "BSL Language Server is not installed: native launcher and legacy JAR are missing"
                .to_string(),
        );
    }

    Ok(ServerLaunchSpec {
        program: settings.java_path.clone(),
        args: vec![
            BSL_TOMCAT_BUFFER_SIZE_JVM_OPT.to_string(),
            "-Xmx256m".to_string(),
            "-XX:+UseSerialGC".to_string(),
            "-jar".to_string(),
            settings.jar_path.clone(),
            "websocket".to_string(),
            format!("--server.port={port}"),
        ],
        envs: Vec::new(),
        mcp_enabled: false,
    })
}

fn native_launcher_available(settings: &crate::settings::BSLServerSettings) -> bool {
    let executable_path = settings.executable_path.trim();
    !executable_path.is_empty() && std::path::Path::new(executable_path).is_file()
}

fn resolve_workspace_path(configured_path: &str, fallback: &std::path::Path) -> std::path::PathBuf {
    let configured_path = configured_path.trim();
    if configured_path.is_empty() {
        fallback.to_path_buf()
    } else {
        std::path::PathBuf::from(configured_path)
    }
}

fn temporary_document_uri(workspace: &std::path::Path, prefix: &str, sequence: u128) -> String {
    let safe_prefix: String = prefix
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .collect();
    let filename = format!(".mini-ai-1c-{safe_prefix}-{sequence}.bsl");
    let path = workspace.join(filename);
    Url::from_file_path(&path)
        .map(|url| url.to_string())
        .unwrap_or_else(|_| format!("file:///{}", path.to_string_lossy().replace('\\', "/")))
}

fn official_mcp_config(port: u16) -> crate::settings::McpServerConfig {
    crate::settings::McpServerConfig {
        id: "bsl-ls-official".to_string(),
        name: "BSL Language Server".to_string(),
        enabled: true,
        transport: crate::settings::McpTransport::Http,
        url: Some(official_mcp_endpoint(port)),
        ..Default::default()
    }
}

fn merge_bsl_tools(mut internal: Vec<McpTool>, upstream: Vec<McpTool>) -> Vec<McpTool> {
    for tool in upstream {
        if !internal.iter().any(|existing| existing.name == tool.name) {
            internal.push(tool);
        }
    }
    internal
}

/// JSON-RPC request
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<i32>,
    method: String,
    params: serde_json::Value,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<i32>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
    // For notifications (like publishDiagnostics)
    method: Option<String>,
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDocumentContentChangeEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
    #[serde(rename = "rangeLength", skip_serializing_if = "Option::is_none")]
    pub range_length: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct DocumentState {
    pub uri: String,
    pub text: String,
    pub version: i32,
}

/// LSP Diagnostic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Option<i32>,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    Disconnected,
    Connecting,
    Ready,
}

/// BSL Language Server client
pub struct BSLClient {
    ws_tx: Option<mpsc::Sender<String>>,
    server_process: Option<Child>,
    request_id: AtomicI32,
    capabilities: Option<serde_json::Value>,
    workspace_root: Option<String>,
    actual_port: Option<u16>,
    mcp_enabled: bool,
    
    app_handle: Option<AppHandle>,
    documents: Arc<StdMutex<HashMap<String, DocumentState>>>,
    pending_requests: Arc<StdMutex<HashMap<i32, oneshot::Sender<JsonRpcResponse>>>>,
    pending_diagnostics: Arc<StdMutex<HashMap<String, oneshot::Sender<Vec<Diagnostic>>>>>,

    state_tx: tokio::sync::watch::Sender<ClientState>,
    state_rx: tokio::sync::watch::Receiver<ClientState>,
}

impl BSLClient {
    pub fn new() -> Self {
        let (state_tx, state_rx) = tokio::sync::watch::channel(ClientState::Disconnected);
        Self {
            ws_tx: None,
            server_process: None,
            request_id: AtomicI32::new(1),
            capabilities: None,
            workspace_root: None,
            actual_port: None,
            mcp_enabled: false,
            app_handle: None,
            documents: Arc::new(StdMutex::new(HashMap::new())),
            pending_requests: Arc::new(StdMutex::new(HashMap::new())),
            pending_diagnostics: Arc::new(StdMutex::new(HashMap::new())),
            state_tx,
            state_rx,
        }
    }

    /// Ensure BSL LS is Ready
    pub async fn ensure_ready(&self) -> Result<(), String> {
        let mut rx = self.state_rx.clone();
        let timeout = tokio::time::Duration::from_secs(10);
        
        let result = tokio::time::timeout(timeout, async {
            while *rx.borrow_and_update() != ClientState::Ready {
                rx.changed().await.unwrap();
            }
        }).await;
        
        result.map_err(|_| "Timeout waiting for BSL LS to become Ready".to_string())
    }

    pub fn set_app_handle(&mut self, app_handle: AppHandle) {
        self.app_handle = Some(app_handle);
    }

    fn set_state(&self, new_state: ClientState) {
        let _ = self.state_tx.send(new_state);
        if let Some(app) = &self.app_handle {
            let payload = match new_state {
                ClientState::Disconnected => "disconnected",
                ClientState::Connecting => "connecting",
                ClientState::Ready => "ready",
            };
            let _ = app.emit("bsl-ls-state", payload);
        }
    }

    /// Check if a port has an active listener (someone is already listening on it).
    /// Uses connect() instead of bind() — reliable on Windows across multiple user sessions.
    fn is_port_listening(port: u16) -> bool {
        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            std::time::Duration::from_millis(50),
        )
        .is_ok()
    }

    /// Find a free TCP port starting from the preferred port.
    /// Uses connect() to check occupation — correctly handles Windows SO_REUSEADDR behavior.
    fn find_available_port(preferred: u16) -> u16 {
        let mut port = preferred;
        while port < preferred + 100 {
            if !Self::is_port_listening(port) {
                return port;
            }
            port += 1;
        }
        preferred // Fallback to preferred if none found in range
    }

    pub fn is_connected(&self) -> bool {
        self.ws_tx.is_some()
    }

    fn official_mcp_port(&self) -> Option<u16> {
        if self.mcp_enabled {
            Some(
                self.actual_port
                    .unwrap_or_else(|| load_settings().bsl_server.websocket_port),
            )
        } else {
            None
        }
    }

    pub fn is_official_mcp_available(&self) -> bool {
        self.is_connected() && self.official_mcp_port().is_some()
    }

    pub fn active_port(&self) -> Option<u16> {
        self.actual_port
    }

    /// Start the BSL Language Server
    pub fn start_server(&mut self) -> Result<(), String> {
        // Guard: already running in this process instance
        if self.server_process.is_some() {
            crate::app_log!("[BSL LS] Already running in this instance, skipping start");
            return Ok(());
        }

        let settings = load_settings();

        if !settings.bsl_server.enabled {
            return Err("BSL LS is disabled in settings".to_string());
        }

        let preferred_port = settings.bsl_server.websocket_port;
        let mcp_required = native_launcher_available(&settings.bsl_server);
        self.mcp_enabled = mcp_required;

        // Check if BSL LS is already listening on the preferred port
        // (e.g. started by another app instance or another user session on this machine).
        // In that case reuse it instead of spawning a duplicate Java process.
        if Self::is_port_listening(preferred_port) && should_reuse_existing_listener(mcp_required) {
            crate::app_log!(
                "[BSL LS] Port {} already has a listener — reusing existing server",
                preferred_port
            );
            self.actual_port = Some(preferred_port);
            return Ok(());
        }
        if Self::is_port_listening(preferred_port) && mcp_required {
            crate::app_log!(
                "[BSL LS] Port {} is occupied by an unverified listener; starting MCP-capable server on a free port",
                preferred_port
            );
        }

        // Find a truly free port (skips any occupied ports)
        let port = Self::find_available_port(preferred_port);
        self.actual_port = Some(port);

        crate::app_log!(
            "[BSL LS] Starting on port {} (preferred was {})",
            port,
            preferred_port
        );

        let launch = server_launch_spec(&settings.bsl_server, port)?;
        self.mcp_enabled = launch.mcp_enabled;
        let mut cmd = AsyncCommand::new(&launch.program);
        cmd.args(&launch.args)
            .envs(launch.envs.clone())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start BSL LS: {}", e))?;

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // Task to read stdout
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                crate::app_log!("[BSL LS][STDOUT] {}", line);
            }
        });

        // Task to read stderr
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                crate::app_log!("[BSL LS][STDERR] {}", line);
            }
        });

        self.server_process = Some(child);
        
        // Wait for server to actually start listening
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if Self::is_port_listening(port) {
                crate::app_log!("[BSL LS] Port {} is now listening", port);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        crate::app_log!("BSL LS process spawned");
        Ok(())
    }

    /// Connect to the BSL Language Server
    pub async fn connect(&mut self) -> Result<(), String> {
        self.set_state(ClientState::Connecting);
        let port = self
            .actual_port
            .unwrap_or_else(|| load_settings().bsl_server.websocket_port);
        let url = format!("ws://127.0.0.1:{}/lsp", port);

        crate::app_log!("[BSL LS] Attempting to connect to {}", url);

        let mut retries = 0;
        let max_retries = 30; // 15 seconds total

        loop {
            // Add timeout to connect_async to prevent hang during handshake (common in terminal servers)
            let connect_timeout =
                tokio::time::timeout(tokio::time::Duration::from_secs(3), connect_async(&url))
                    .await;

            match connect_timeout {
                Ok(Ok((ws_stream, _))) => {
                    crate::app_log!("[BSL LS] WebSocket connected successfully to {}", url);
                    
                    let (mut write, mut read) = ws_stream.split();
                    let (tx, mut rx) = mpsc::channel::<String>(100);
                    self.ws_tx = Some(tx.clone());

                    let pending_reqs = self.pending_requests.clone();
                    let pending_diags = self.pending_diagnostics.clone();
                    let app_handle = self.app_handle.clone();

                    tokio::spawn(async move {
                        while let Some(msg) = rx.recv().await {
                            if write.send(Message::Text(msg)).await.is_err() {
                                break;
                            }
                        }
                    });

                    let loop_tx = tx.clone();
                    tokio::spawn(async move {
                        while let Some(Ok(Message::Text(text))) = read.next().await {
                            if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&text) {
                                if response.id.is_some() && response.method.is_none() {
                                    let id = response.id.unwrap();
                                    let req_tx = {
                                        let mut reqs = pending_reqs.lock().unwrap();
                                        reqs.remove(&id)
                                    };
                                    if let Some(req_tx) = req_tx {
                                        let _ = req_tx.send(response);
                                    }
                                } else if let Some(method) = &response.method {
                                    if method == "textDocument/publishDiagnostics" {
                                        if let Some(params) = response.params {
                                            if let Some(uri) = params.get("uri").and_then(|u| u.as_str()) {
                                                let items = params.get("diagnostics").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                                                let diagnostics: Vec<Diagnostic> = items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect();
                                                
                                                let diag_tx = {
                                                    let mut diags = pending_diags.lock().unwrap();
                                                    diags.remove(uri)
                                                };
                                                
                                                if let Some(diag_tx) = diag_tx {
                                                    let _ = diag_tx.send(diagnostics);
                                                } else if let Some(app) = &app_handle {
                                                    let flat: Vec<serde_json::Value> = diagnostics.iter().map(|d| {
                                                        serde_json::json!({
                                                            "line": d.range.start.line,
                                                            "character": d.range.start.character,
                                                            "message": d.message,
                                                            "severity": match d.severity {
                                                                Some(1) => "error",
                                                                Some(2) => "warning",
                                                                Some(3) => "info",
                                                                _ => "hint",
                                                            }
                                                        })
                                                    }).collect();
                                                    let payload = serde_json::json!({
                                                        "uri": uri,
                                                        "diagnostics": flat
                                                    });
                                                    let _ = app.emit("bsl-diagnostics", payload);
                                                }
                                            }
                                        }
                                    } else {
                                        if let Some(id) = response.id {
                                            let _ = Self::handle_server_request_async(&loop_tx, method, id, &response.params).await;
                                        } else if method == "window/logMessage" {
                                            let _ = Self::handle_server_request_async(&loop_tx, method, 0, &response.params).await;
                                        }
                                    }
                                }
                            }
                        }
                    });

                    break;
                }
                Ok(Err(e)) => {
                    retries += 1;
                    if retries >= max_retries {
                        crate::app_log!(
                            "[BSL LS] Connection FAILED after {} attempts. Last error: {}",
                            max_retries,
                            e
                        );
                        return Err(format!(
                            "Failed to connect to BSL LS after {} attempts: {}",
                            max_retries, e
                        ));
                    }
                    if retries % 5 == 0 {
                        crate::app_log!(
                            "[BSL LS] connection attempt {}/{}... (error: {})",
                            retries,
                            max_retries,
                            e
                        );
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
                Err(_) => {
                    retries += 1;
                    crate::app_log!(
                        "[BSL LS] Connection HANDSHAKE TIMEOUT (3s) at {}/{}",
                        retries,
                        max_retries
                    );
                    if retries >= max_retries {
                        return Err(format!(
                            "Failed to connect to BSL LS (Handshake Timeout) after {} attempts",
                            max_retries
                        ));
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }

        crate::app_log!("[BSL LS] Initializing LSP handshake...");
        let client_capabilities = serde_json::json!({
            "workspace": {
                "configuration": true,
                "workspaceFolders": true,
                "didChangeConfiguration": { "dynamicRegistration": true }
            },
            "textDocument": {
                "synchronization": {
                    "dynamicRegistration": true,
                    "willSave": false,
                    "willSaveWaitUntil": false,
                    "didSave": false
                },
                "diagnostic": { "dynamicRegistration": true },
                "formatting": { "dynamicRegistration": true },
                "publishDiagnostics": {
                    "relatedInformation": true,
                    "tagSupport": { "valueSet": [1, 2] },
                    "versionSupport": true
                }
            }
        });

        // Register either the user-selected BSL project or the app's persistent fallback workspace.
        let settings = load_settings();
        let fallback_workspace = crate::settings::get_settings_dir().join("bsl-workspace");
        let workspace_path =
            resolve_workspace_path(&settings.bsl_server.workspace_path, &fallback_workspace);
        let uses_fallback_workspace = settings.bsl_server.workspace_path.trim().is_empty();
        if uses_fallback_workspace {
            if let Err(error) = std::fs::create_dir_all(&workspace_path) {
                self.invalidate_connection();
                return Err(format!(
                    "Failed to create BSL workspace '{}': {error}",
                    workspace_path.display()
                ));
            }
        } else if !workspace_path.is_dir() {
            self.invalidate_connection();
            return Err(format!(
                "Configured BSL workspace does not exist or is not a directory: {}",
                workspace_path.display()
            ));
        }
        let root_dir = workspace_path.to_string_lossy().replace('\\', "/");
        self.workspace_root = Some(root_dir.clone());

        // Do not write configuration files into a user project. The default belongs only
        // to the app-managed fallback workspace.
        let config_path = workspace_path.join(".bsl-language-server.json");
        if uses_fallback_workspace && !config_path.exists() {
            let config = serde_json::json!({
                "language": "ru",
                "diagnostics": {
                    "parameters": {
                        "EmptyLines": { "maxCount": 1 }
                    }
                }
            });
            let _ = std::fs::write(
                &config_path,
                serde_json::to_string_pretty(&config).unwrap_or_default(),
            );
        }

        // Properly format file URI using url crate (critical for UNC and spaces)
        let root_path = std::fs::canonicalize(&workspace_path).unwrap_or(workspace_path.clone());
        let root_uri = Url::from_file_path(&root_path)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| {
                if root_dir.starts_with('/') {
                    format!("file://{}", root_dir)
                } else {
                    format!("file:///{}", root_dir)
                }
            });

        crate::app_log!("[BSL LS] Using rootUri: {}", root_uri);

        let initialize_result = match self
            .send_request(
                "initialize",
                serde_json::json!({
                    "processId": std::process::id(),
                    "rootUri": root_uri,
                    "workspaceFolders": [{
                        "uri": root_uri,
                        "name": "BSL Workspace"
                    }],
                    "capabilities": client_capabilities,
                    "trace": "verbose"
                }),
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                self.invalidate_connection();
                return Err(error);
            }
        };

        // Store server capabilities
        self.capabilities = initialize_result.get("capabilities").cloned();
        crate::app_log!(
            "[BSL LS] Initialized. Server capabilities: {:?}",
            self.capabilities.as_ref().map(|c| c.to_string())
        );

        // Send initialized notification
        let _ = self.send_notification("initialized", serde_json::json!({})).await;

        self.set_state(ClientState::Ready);

        // Automatically open all documents from cache
        let docs = {
            let docs_guard = self.documents.lock().unwrap();
            docs_guard.values().cloned().collect::<Vec<_>>()
        };
        for doc in docs {
            let _ = self.bsl_did_open(doc.uri, doc.text, doc.version).await;
        }

        Ok(())
    }

    /// Send a JSON-RPC response to a server-initiated request
    async fn send_response_raw(
        tx: &mpsc::Sender<String>,
        id: i32,
        result: serde_json::Value,
    ) -> Result<(), String> {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        });
        let msg = serde_json::to_string(&response).map_err(|e| e.to_string())?;
        crate::app_log!("[BSL LS] >>> Sending response for id={}: {}", id, msg);
        tx.send(msg).await.map_err(|e| e.to_string())
    }

    /// Handle server-initiated requests
    async fn handle_server_request_async(
        tx: &mpsc::Sender<String>,
        method: &str,
        id: i32,
        _params: &Option<serde_json::Value>,
    ) {
        crate::app_log!("[BSL LS] Server requested: {} (id={})", method, id);
        match method {
            "workspace/configuration" => {
                // Return default configuration
                let config = serde_json::json!([{
                    "bsl": {
                        "language": "ru",
                        "diagnostics": {
                            "parameters": {
                                "EmptyLines": { "maxCount": 1 }
                            }
                        }
                    }
                }]);
                let _ = Self::send_response_raw(tx, id, config).await;
            }
            "client/registerCapability" => {
                let _ = Self::send_response_raw(tx, id, serde_json::json!({})).await;
            }
            "window/logMessage" => {
                if let Some(params) = _params {
                    let msg = params.get("message").and_then(|v| v.as_str()).unwrap_or("");
                    crate::app_log!("[BSL LS][server] {}", msg);
                }
            }
            "window/showMessageRequest" => {
                // Auto-accept error reporting and other prompts to avoid UI hangs
                // For "Agree to send error report", take the first option (usually "Yes")
                if let Some(params) = _params {
                    let msg = params.get("message").and_then(|v| v.as_str()).unwrap_or("");
                    crate::app_log!("[BSL LS] Auto-responding to showMessageRequest: {}", msg);

                    let actions = params.get("actions").and_then(|v| v.as_array());
                    let result = if let Some(first_action) = actions.and_then(|a| a.first()) {
                        first_action
                            .get("title")
                            .cloned()
                            .unwrap_or(serde_json::json!("Да"))
                    } else {
                        serde_json::json!("Да")
                    };
                    let _ = Self::send_response_raw(tx, id, serde_json::json!({ "title": result }))
                        .await;
                }
            }
            _ => {
                crate::app_log!("[BSL LS] Warning: Unhandled server request: {}", method);
                let _ = Self::send_response_raw(tx, id, serde_json::Value::Null).await;
            }
        }
    }

    /// Send JSON-RPC request with timeout
    async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let tx = self.ws_tx.as_ref().ok_or("Not connected")?;

        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: method.to_string(),
            params,
        };

        let msg = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        
        let (resp_tx, resp_rx) = oneshot::channel();
        {
            let mut reqs = self.pending_requests.lock().unwrap();
            reqs.insert(id, resp_tx);
        }

        crate::app_log!("[BSL LS] >>> Request {}: {}", method, msg);
        tx.send(msg)
        .await
        .map_err(|e| format!("WebSocket error: {e}"))?;

        // Wait for response with overall timeout
        let request_timeout = request_timeout_for(method);
        match tokio::time::timeout(request_timeout, resp_rx).await {
            Ok(Ok(response)) => {
                if let Some(error) = response.error {
                    crate::app_log!("[BSL LS] LSP error response: {:?}", error);
                    return Err(format!("LSP error {}: {}", error.code, error.message));
                }
                Ok(response.result.unwrap_or(serde_json::Value::Null))
            }
            Ok(Err(_)) => Err("Response channel closed".to_string()),
            Err(_) => {
                let mut reqs = self.pending_requests.lock().unwrap();
                reqs.remove(&id);
                crate::app_log!(
                    "[BSL LS] TIMEOUT ({:?}) waiting for response to '{}' request",
                    request_timeout,
                    method,
                );
                Err(format!(
                    "Timeout waiting for BSL LS response to '{}'",
                     method
                ))
            }
        }
    }

    /// Send JSON-RPC notification
    async fn send_notification(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), String> {
        let tx = self.ws_tx.as_ref().ok_or("Not connected")?;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.to_string(),
            params,
        };

        let msg = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        crate::app_log!("[BSL LS] >>> Notification {}: {}", method, msg);
        tx.send(msg)
        .await
        .map_err(|e| format!("WebSocket error: {e}"))?;

        Ok(())
    }

    async fn fetch_pull_diagnostics(&self, uri: &str) {
        let supports_pull_diagnostics = self
            .capabilities
            .as_ref()
            .and_then(|c| c.get("diagnosticProvider"))
            .is_some();

        if !supports_pull_diagnostics {
            return;
        }

        crate::app_log!("[BSL LS] Using pull-model diagnostics");
        let result = self
            .send_request(
                "textDocument/diagnostic",
                serde_json::json!({
                    "textDocument": {
                        "uri": uri
                    }
                }),
            )
            .await;

        match result {
            Ok(result) => {
                let diagnostics: Vec<Diagnostic> = result
                    .get("items")
                    .and_then(|v| v.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|v| serde_json::from_value(v.clone()).ok())
                            .collect()
                    })
                    .unwrap_or_default();

                crate::app_log!("[BSL LS] Returned {} diagnostics for {}", diagnostics.len(), uri);

                let diag_tx = {
                    let mut diags = self.pending_diagnostics.lock().unwrap();
                    diags.remove(uri)
                };

                if let Some(diag_tx) = diag_tx {
                    let _ = diag_tx.send(diagnostics);
                } else if let Some(app) = &self.app_handle {
                    let flat: Vec<serde_json::Value> = diagnostics
                        .iter()
                        .map(|d| {
                            serde_json::json!({
                                "line": d.range.start.line,
                                "character": d.range.start.character,
                                "message": d.message,
                                "severity": match d.severity {
                                    Some(1) => "error",
                                    Some(2) => "warning",
                                    Some(3) => "info",
                                    _ => "hint",
                                }
                            })
                        })
                        .collect();

                    let payload = serde_json::json!({
                        "uri": uri,
                        "diagnostics": flat
                    });

                    let _ = app.emit("bsl-diagnostics", payload);
                }
            }
            Err(e) => {
                crate::app_log!("[BSL LS] Error fetching pull diagnostics for {}: {}", uri, e);
                let diag_tx = {
                    let mut diags = self.pending_diagnostics.lock().unwrap();
                    diags.remove(uri)
                };
                if let Some(diag_tx) = diag_tx {
                    let _ = diag_tx.send(vec![]);
                }
            }
        }
    }

    pub async fn bsl_did_open(&self, uri: String, text: String, version: i32) -> Result<(), String> {
        self.ensure_ready().await?;
        {
            let mut docs = self.documents.lock().unwrap();
            docs.insert(uri.clone(), DocumentState {
                uri: uri.clone(),
                text: text.clone(),
                version,
            });
        }
        self.send_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": &uri,
                    "languageId": "bsl",
                    "version": version,
                    "text": text
                }
            }),
        )
        .await?;

        self.fetch_pull_diagnostics(&uri).await;
        Ok(())
    }

    pub fn update_document_text(doc: &mut DocumentState, text: String) -> i32 {
        doc.version += 1;
        doc.text = text;
        doc.version
    }

    pub async fn bsl_did_change(&self, uri: String, text: String, _frontend_version: i32) -> Result<(), String> {
        self.ensure_ready().await?;

        let new_version = {
            let mut docs = self.documents.lock().unwrap();
            if let Some(doc) = docs.get_mut(&uri) {
                Self::update_document_text(doc, text.clone())
            } else {
                docs.insert(uri.clone(), DocumentState {
                    uri: uri.clone(),
                    text: text.clone(),
                    version: _frontend_version,
                });
                _frontend_version
            }
        };

        // Full-sync: send a single change event with only the full text (no range).
        let change = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text,
        };

        self.send_notification(
            "textDocument/didChange",
            serde_json::json!({
                "textDocument": {
                    "uri": &uri,
                    "version": new_version
                },
                "contentChanges": [change]
            }),
        )
        .await?;

        self.fetch_pull_diagnostics(&uri).await;
        Ok(())
    }

    pub async fn bsl_did_close(&self, uri: String) -> Result<(), String> {
        {
            let mut docs = self.documents.lock().unwrap();
            docs.remove(&uri);
        }
        self.send_notification(
            "textDocument/didClose",
            serde_json::json!({
                "textDocument": {
                    "uri": uri
                }
            }),
        )
        .await
    }

    /// Analyze code and return diagnostics
    pub async fn analyze_code(&self, code: &str, suffix: &str) -> Result<Vec<Diagnostic>, String> {
        self.ensure_ready().await?;
        let fallback_workspace = crate::settings::get_settings_dir().join("bsl-workspace");
        let workspace = self
            .workspace_root
            .as_deref()
            .map(std::path::Path::new)
            .unwrap_or(&fallback_workspace);
        let sequence = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let uri = temporary_document_uri(workspace, suffix, sequence as u128);

        let (tx, rx) = oneshot::channel();
        {
            let mut diags = self.pending_diagnostics.lock().unwrap();
            diags.insert(uri.clone(), tx);
        }

        crate::app_log!("[BSL LS] Starting analysis for URI: {}", uri);

        // Send didOpen notification
        if let Err(e) = self.send_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": &uri,
                    "languageId": "bsl",
                    "version": 1,
                    "text": code
                }
            }),
        )
        .await {
            let mut diags = self.pending_diagnostics.lock().unwrap();
            diags.remove(&uri);
            return Err(e);
        }

        self.fetch_pull_diagnostics(&uri).await;

        let result = match tokio::time::timeout(tokio::time::Duration::from_secs(10), rx).await {
            Ok(Ok(diags)) => Ok(diags),
            _ => {
                let mut diags = self.pending_diagnostics.lock().unwrap();
                diags.remove(&uri);
                Ok(vec![])
            }
        };

        // Always close the temporary document, including request timeout/error paths.
        let _ = self.send_notification(
            "textDocument/didClose",
            serde_json::json!({
                "textDocument": {
                    "uri": &uri
                }
            }),
        ).await;

        result
    }

    /// Format code
    pub async fn format_code(&self, code: &str, suffix: &str) -> Result<String, String> {
        // Guard check
        let can_format = self
            .capabilities
            .as_ref()
            .and_then(|c| c.get("documentFormattingProvider"))
            .and_then(|v| v.as_bool().or_else(|| v.as_object().map(|_| true)))
            .unwrap_or(false);

        if !can_format {
            return Err("BSL LS does not support formatting for this document".to_string());
        }

        self.ensure_ready().await?;
        let fallback_workspace = crate::settings::get_settings_dir().join("bsl-workspace");
        let workspace = self
            .workspace_root
            .as_deref()
            .map(std::path::Path::new)
            .unwrap_or(&fallback_workspace);
        let sequence = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let uri = temporary_document_uri(workspace, suffix, sequence as u128);

        // Open document
        self.send_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": &uri,
                    "languageId": "bsl",
                    "version": 1,
                    "text": code
                }
            }),
        )
        .await?;
        
        // Request formatting
        let result = self
            .send_request(
                "textDocument/formatting",
                serde_json::json!({
                    "textDocument": {
                        "uri": &uri
                    },
                    "options": {
                        "tabSize": 4,
                        "insertSpaces": false
                    }
                }),
            )
            .await?;

        // Close document
        self.send_notification(
            "textDocument/didClose",
            serde_json::json!({
                "textDocument": {
                    "uri": &uri
                }
            }),
        )
        .await?;

        // Apply edits
        if let Some(edits) = result.as_array() {
            if let Some(edit) = edits.first() {
                if let Some(new_text) = edit.get("newText").and_then(|v| v.as_str()) {
                    return Ok(new_text.to_string());
                }
            }
        }
        
        // No edits, return original
        Ok(code.to_string())
    }

    /// Go to Definition
    #[allow(dead_code)]
    pub async fn goto_definition(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<crate::bsl_client::Location>, String> {
        // Build params
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri
            },
            "position": {
                "line": line,
                "character": character
            }
        });

        // Send request
        let result = self.send_request("textDocument/definition", params).await?;

        // Parse result (Location | Location[] | LocationLink[] | null)
        if result.is_null() {
            return Ok(None);
        }

        // Case 1: Single Location
        if let Ok(location) = serde_json::from_value::<crate::bsl_client::Location>(result.clone())
        {
            return Ok(Some(location));
        }

        // Case 2: Array of Locations (take first)
        if let Ok(locations) =
            serde_json::from_value::<Vec<crate::bsl_client::Location>>(result.clone())
        {
            if let Some(first) = locations.first() {
                return Ok(Some(first.clone()));
            }
        }

        // Case 3: Array of LocationLinks (take first)
        // Structure: targetUri, targetRange, targetSelectionRange
        if let Some(links) = result.as_array() {
            if let Some(first_link) = links.first() {
                // Try to extract uri/range manually as it differs from Location
                if let Some(target_uri) = first_link.get("targetUri").and_then(|v| v.as_str()) {
                    if let Some(target_range) = first_link.get("targetSelectionRange") {
                        // Use selection range for precision
                        if let Ok(range) =
                            serde_json::from_value::<crate::bsl_client::Range>(target_range.clone())
                        {
                            return Ok(Some(crate::bsl_client::Location {
                                uri: target_uri.to_string(),
                                range,
                            }));
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    /// Resolve definition and return source code
    #[allow(dead_code)]
    pub async fn resolve_definition(
        &self,
        code: &str,
        line: u32,
        character: u32,
    ) -> Result<String, String> {
        let uri = "file:///temp_definition.bsl";

        // 1. Open document
        self.send_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "bsl", // "bsl" (1c)
                    "version": 1,
                    "text": code
                }
            }),
        )
        .await?;

        // 2. Request definition
        let location_opt = self.goto_definition(uri, line, character).await?;

        // 3. Close document
        self.send_notification(
            "textDocument/didClose",
            serde_json::json!({
                "textDocument": {
                    "uri": uri
                }
            }),
        )
        .await?;

        // 4. Process result
        if let Some(location) = location_opt {
            let target_uri = location.uri;

            // Clean up URI (file:///...)
            let path_str = if target_uri.starts_with("file:///") {
                // Windows: file:///c:/... -> c:/...
                // Unix: file:///usr/... -> /usr/...
                if cfg!(windows) {
                    &target_uri[8..]
                } else {
                    &target_uri[7..]
                }
            } else if target_uri.starts_with("file://") {
                &target_uri[7..]
            } else {
                &target_uri
            };

            let path_decoded = urlencoding::decode(path_str).map_err(|e| e.to_string())?;
            let path = std::path::Path::new(path_decoded.as_ref());

            if path.exists() {
                let content = tokio::fs::read_to_string(path)
                    .await
                    .map_err(|e| format!("Failed to read file: {}", e))?;

                // Extract range? Or return whole method?
                // Usually we want the whole method. BSL LS returns range of the Name.
                // We can try to heuristic parsing or just return the whole file if it's small,
                // OR better: return a snippet around the definition.
                // For BSL, often it points to "Procedure MyProc()".
                // Let's return the whole file for now, or maybe 50 lines?
                // Ideally we want the Function body.

                // Simple heuristic: read +- 50 lines?
                // No, let's just return the content and let the UI/AI decide.
                // Actually, for "Context" we want the function body.
                // Let's return the whole file content and let the frontend slice it?
                // Or just return the whole file content.
                return Ok(content);
            } else {
                return Err(format!("File not found: {}", path.display()));
            }
        }

        Err("Definition not found".to_string())
    }

    fn invalidate_connection(&mut self) {
        self.ws_tx = None;
        self.capabilities = None;
        self.workspace_root = None;
        self.set_state(ClientState::Disconnected);
    }

    /// Stop the server and clear LSP session state.
    pub fn stop(&mut self) {
        let owns_process = self.server_process.is_some();
        let ws_tx = self.ws_tx.take();
        self.capabilities = None;
        self.workspace_root = None;
        self.actual_port = None;
        self.mcp_enabled = false;
        self.set_state(ClientState::Disconnected);

        if owns_process {
            if let Some(tx) = ws_tx {
                tokio::spawn(async move {
                    let exit_notif = JsonRpcRequest {
                        jsonrpc: "2.0".to_string(),
                        id: None,
                        method: "exit".to_string(),
                        params: serde_json::json!({}),
                    };
                    if let Ok(msg) = serde_json::to_string(&exit_notif) {
                        let _ = tx.send(msg).await;
                    }
                });
            }
        }

        if let Some(mut child) = self.server_process.take() {
            if let Err(error) = child.start_kill() {
                crate::app_log!(
                    force: true,
                    "[BSL LS] Failed to terminate owned process: {}",
                    error
                );
            }
        }
    }

    /// Check if Java is installed and retrieve version
    pub fn check_java(java_path: &str) -> String {
        let mut cmd = StdCommand::new(java_path);
        cmd.arg("-version");

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }

        match cmd.output() {
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("version") {
                    stderr.lines().next().unwrap_or("Java found").to_string()
                } else {
                    "Java found (version unknown)".to_string()
                }
            }
            Err(_) => "Not found".to_string(),
        }
    }

    /// Check if BSL LS is installed (JAR exists)
    pub fn check_install(jar_path: &str) -> bool {
        std::path::Path::new(jar_path).exists()
    }
}

/// Resolve a BSL file path (relative or absolute) to an absolute path string.
fn resolve_bsl_file_path(file: &str, config_root: Option<&str>) -> String {
    let p = std::path::Path::new(file);
    if p.is_absolute() {
        return file.to_string();
    }
    if let Some(root) = config_root {
        let joined =
            std::path::Path::new(root).join(file.replace('/', std::path::MAIN_SEPARATOR_STR));
        return joined.to_string_lossy().to_string();
    }
    file.to_string()
}

/// Convert an absolute file path to a file:// URI (Windows-safe).
fn path_to_file_uri(abs_path: &str) -> String {
    let normalized = abs_path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{}", normalized)
    } else {
        format!("file:///{}", normalized)
    }
}

/// Convert a file:// URI back to an absolute path string.
fn uri_to_abs_path(uri: &str) -> String {
    let s = uri
        .trim_start_matches("file:///")
        .trim_start_matches("file://");
    // On Windows, restore drive letter path
    if cfg!(windows) && s.len() > 1 && s.chars().nth(1) == Some(':') {
        s.replace('/', "\\")
    } else if cfg!(windows) {
        format!("\\\\{}", s.replace('/', "\\"))
    } else {
        format!("/{}", s)
    }
}

/// Convert a file:// URI to a display path (relative to config_root when possible).
fn uri_to_display_path(uri: &str, config_root: Option<&str>) -> String {
    let abs = uri_to_abs_path(uri);
    if let Some(root) = config_root {
        let root_norm = root.replace('\\', "/");
        let abs_norm = abs.replace('\\', "/");
        if let Some(rel) = abs_norm.strip_prefix(&root_norm) {
            return rel.trim_start_matches('/').to_string();
        }
    }
    abs
}

/// Ensure BSL client is connected, starting server if needed.
async fn ensure_bsl_connected(client: &mut BSLClient) -> Result<(), String> {
    if !client.is_connected() {
        if let Err(e) = client.connect().await {
            if client.server_process.is_none() {
                client.start_server()?;
            }
            client.connect().await.map_err(|e2| {
                format!(
                    "BSL LS не запущен или недоступен: {}\nДоп. ошибка: {}",
                    e, e2
                )
            })?;
        }
    }
    Ok(())
}

pub struct BSLMcpHandler {
    client: Arc<Mutex<BSLClient>>,
}

impl BSLMcpHandler {
    pub fn new(client: Arc<Mutex<BSLClient>>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl InternalMcpHandler for BSLMcpHandler {
    async fn list_tools(&self) -> Vec<McpTool> {
        let internal_tools = vec![
            McpTool {
                name: "check_bsl_syntax".to_string(),
                description: "Проверяет BSL код (1С) на наличие синтаксических ошибок и предупреждений с использованием BSL Language Server.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "code": {
                            "type": "string",
                            "description": "Исходный код на языке BSL для анализа."
                        }
                    },
                    "required": ["code"]
                }),
            },
            McpTool {
                name: "goto_definition".to_string(),
                description: "Семантический переход к определению символа BSL (процедуры, функции, переменной) по позиции в файле. Быстрее и точнее чем text search для навигации по коду.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": "Абсолютный путь к BSL файлу или путь относительно корня конфигурации."
                        },
                        "line": {
                            "type": "integer",
                            "description": "Номер строки (0-based, LSP convention)."
                        },
                        "character": {
                            "type": "integer",
                            "description": "Позиция символа в строке (0-based)."
                        },
                        "config_root": {
                            "type": "string",
                            "description": "Корневой путь конфигурации для резолва относительных путей."
                        }
                    },
                    "required": ["file", "line", "character"]
                }),
            },
            McpTool {
                name: "resolve_definition_context".to_string(),
                description: "Переходит к определению символа BSL и возвращает контекст кода вокруг определения. Объединяет goto_definition + get_file_context в один вызов.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": "Абсолютный путь к BSL файлу или путь относительно корня конфигурации."
                        },
                        "line": {
                            "type": "integer",
                            "description": "Номер строки (0-based, LSP convention)."
                        },
                        "character": {
                            "type": "integer",
                            "description": "Позиция символа в строке (0-based)."
                        },
                        "radius": {
                            "type": "integer",
                            "description": "Количество строк контекста вокруг определения (по умолчанию 30).",
                            "default": 30
                        },
                        "config_root": {
                            "type": "string",
                            "description": "Корневой путь конфигурации для резолва относительных путей."
                        }
                    },
                    "required": ["file", "line", "character"]
                }),
            },
        ];

        let official_port = {
            let client = self.client.lock().await;
            client.official_mcp_port()
        };
        let Some(port) = official_port else {
            return internal_tools;
        };

        let upstream_result = tokio::time::timeout(BSL_UPSTREAM_DISCOVERY_TIMEOUT, async {
            let client = McpClient::new(official_mcp_config(port)).await?;
            client.list_tools().await
        })
        .await;

        match upstream_result {
            Ok(Ok(upstream_tools)) => merge_bsl_tools(internal_tools, upstream_tools),
            Ok(Err(error)) => {
                crate::app_log!(
                    "[BSL MCP] Official tools are temporarily unavailable: {}",
                    error
                );
                internal_tools
            }
            Err(_) => {
                crate::app_log!(
                    "[BSL MCP] Official tools discovery exceeded {} ms; returning internal tools",
                    BSL_UPSTREAM_DISCOVERY_TIMEOUT.as_millis()
                );
                internal_tools
            }
        }
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match name {
            "check_bsl_syntax" => {
                let code = arguments
                    .get("code")
                    .and_then(|v| v.as_str())
                    .ok_or("Параметр 'code' обязателен для check_bsl_syntax")?;

                let mut client = self.client.lock().await;

                // Ensure server is started and connected
                ensure_bsl_connected(&mut client).await?;
                
                let diagnostics = client.analyze_code(code, "mcp-check-syntax").await?;
                
                let flat: Vec<serde_json::Value> = diagnostics.iter().map(|d| {
                    serde_json::json!({
                        "line": d.range.start.line,
                        "character": d.range.start.character,
                        "message": d.message,
                        "severity": match d.severity {
                            Some(1) => "error",
                            Some(2) => "warning",
                            Some(3) => "info",
                            _ => "hint",
                        }
                    })
                }).collect();

                Ok(json!({
                    "success": flat.is_empty(),
                    "diagnostics": flat,
                    "count": diagnostics.len()
                }))
            }

            "goto_definition" => {
                let file = arguments["file"]
                    .as_str()
                    .ok_or("Параметр 'file' обязателен")?;
                let line = arguments["line"]
                    .as_u64()
                    .ok_or("Параметр 'line' обязателен")? as u32;
                let character = arguments["character"]
                    .as_u64()
                    .ok_or("Параметр 'character' обязателен")?
                    as u32;
                let config_root = arguments["config_root"].as_str();

                // Resolve to absolute path and convert to file:// URI
                let abs_path = resolve_bsl_file_path(file, config_root);
                let uri = path_to_file_uri(&abs_path);

                let mut client = self.client.lock().await;
                ensure_bsl_connected(&mut client).await?;

                match client.goto_definition(&uri, line, character).await? {
                    Some(location) => {
                        let target_file = uri_to_display_path(&location.uri, config_root);
                        Ok(json!({
                            "found": true,
                            "target_file": target_file,
                            "target_uri": location.uri,
                            "target_range": {
                                "start": { "line": location.range.start.line, "character": location.range.start.character },
                                "end":   { "line": location.range.end.line,   "character": location.range.end.character }
                            }
                        }))
                    }
                    None => Ok(json!({
                        "found": false,
                        "message": "Определение не найдено. BSL LS не смог разрешить символ по указанной позиции."
                    })),
                }
            }

            "resolve_definition_context" => {
                let file = arguments["file"]
                    .as_str()
                    .ok_or("Параметр 'file' обязателен")?;
                let line = arguments["line"]
                    .as_u64()
                    .ok_or("Параметр 'line' обязателен")? as u32;
                let character = arguments["character"]
                    .as_u64()
                    .ok_or("Параметр 'character' обязателен")?
                    as u32;
                let radius = arguments["radius"].as_u64().unwrap_or(30) as usize;
                let config_root = arguments["config_root"].as_str();

                let abs_path = resolve_bsl_file_path(file, config_root);
                let uri = path_to_file_uri(&abs_path);

                let mut client = self.client.lock().await;
                ensure_bsl_connected(&mut client).await?;

                let location_opt = client.goto_definition(&uri, line, character).await?;
                let location = match location_opt {
                    Some(l) => l,
                    None => {
                        return Ok(json!({
                            "found": false,
                            "message": "Определение не найдено."
                        }))
                    }
                };

                let target_display = uri_to_display_path(&location.uri, config_root);
                let target_abs = uri_to_abs_path(&location.uri);
                let def_line = location.range.start.line as usize + 1; // convert to 1-based for context

                // Read context around definition
                let context = if std::path::Path::new(&target_abs).is_file() {
                    use std::io::{BufRead, BufReader};
                    let f = std::fs::File::open(&target_abs).ok();
                    f.map(|file| {
                        let lines: Vec<String> = BufReader::new(file)
                            .lines()
                            .map(|l| l.unwrap_or_default())
                            .collect();
                        let total = lines.len();
                        let idx = (def_line.saturating_sub(1)).min(total.saturating_sub(1));
                        let start = idx.saturating_sub(radius);
                        let end = (idx + radius + 1).min(total);
                        let mut out = format!("// {}:{}\n", target_display, def_line);
                        for (i, ln) in lines[start..end].iter().enumerate() {
                            let num = start + i + 1;
                            let marker = if num == def_line { "→" } else { " " };
                            out.push_str(&format!("{} {:4} | {}\n", marker, num, ln));
                        }
                        out
                    })
                    .unwrap_or_default()
                } else {
                    String::new()
                };

                Ok(json!({
                    "found": true,
                    "target_file": target_display,
                    "target_uri": location.uri,
                    "target_line": def_line,
                    "target_range": {
                        "start": { "line": location.range.start.line, "character": location.range.start.character },
                        "end":   { "line": location.range.end.line,   "character": location.range.end.character }
                    },
                    "context": context
                }))
            }

            _ => {
                let port = {
                    let mut client = self.client.lock().await;
                    ensure_bsl_connected(&mut client).await?;
                    client.official_mcp_port().ok_or_else(|| {
                        format!(
                            "Official BSL MCP tool '{}' requires BSL Language Server 1.x",
                            name
                        )
                    })?
                };

                let client = McpClient::new(official_mcp_config(port)).await?;
                client.call_tool(name, arguments).await
            }
        }
    }

    fn is_alive(&self) -> bool {
        // Run checks for Java and JAR
        let settings = load_settings();

        // 1. Check if enabled
        if !settings.bsl_server.enabled {
            return false;
        }

        let native_installed = !settings.bsl_server.executable_path.trim().is_empty()
            && std::path::Path::new(&settings.bsl_server.executable_path).is_file();
        if native_installed {
            return true;
        }

        BSLClient::check_install(&settings.bsl_server.jar_path)
            && BSLClient::check_java(&settings.bsl_server.java_path) != "Not found"
    }
}

impl Drop for BSLClient {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_server_starts_combined_lsp_and_mcp_mode() {
        assert_eq!(
            native_server_args(8025),
            vec![
                "websocket".to_string(),
                "--mcp".to_string(),
                "--server.port=8025".to_string(),
            ]
        );
    }

    #[test]
    fn native_server_envs_configures_tomcat_buffer_size() {
        let envs = native_server_envs();
        let java_tool_opts = envs
            .iter()
            .find(|(k, _)| k == "JAVA_TOOL_OPTIONS")
            .map(|(_, v)| v.as_str());
        assert!(java_tool_opts.is_some());
        assert!(java_tool_opts
            .unwrap()
            .contains("-Dorg.apache.tomcat.websocket.DEFAULT_BUFFER_SIZE=1048576"));
    }

    #[test]
    fn native_runtime_does_not_reuse_unknown_listener_without_mcp() {
        assert!(!should_reuse_existing_listener(true));
        assert!(should_reuse_existing_listener(false));
    }

    #[test]
    fn pull_diagnostics_has_extended_timeout() {
        assert_eq!(
            request_timeout_for("textDocument/diagnostic"),
            Duration::from_secs(60)
        );
        assert_eq!(
            request_timeout_for("textDocument/hover"),
            Duration::from_secs(15)
        );
    }

    #[test]
    fn official_mcp_uses_combined_server_endpoint() {
        assert_eq!(official_mcp_endpoint(8025), "http://127.0.0.1:8025/mcp");
    }

    #[test]
    fn native_launch_spec_uses_executable_without_external_java() {
        let mut settings = crate::settings::BSLServerSettings::default();
        settings.executable_path = std::env::current_exe()
            .expect("test executable path")
            .to_string_lossy()
            .to_string();
        settings.java_path = "missing-java".to_string();

        let spec = server_launch_spec(&settings, 8123).expect("native launch spec");

        assert_eq!(spec.program, settings.executable_path);
        assert_eq!(spec.args, native_server_args(8123));
        assert!(spec.envs.iter().any(|(k, v)| k == "JAVA_TOOL_OPTIONS"
            && v.contains("-Dorg.apache.tomcat.websocket.DEFAULT_BUFFER_SIZE=1048576")));
        assert!(spec.mcp_enabled);
    }

    #[test]
    fn legacy_launch_spec_keeps_java_jar_compatibility() {
        let mut settings = crate::settings::BSLServerSettings::default();
        settings.jar_path = r"C:\MiniAI1C\bsl-language-server-0.28.5-exec.jar".to_string();
        settings.java_path = r"C:\Java\bin\java.exe".to_string();

        let spec = server_launch_spec(&settings, 8025).expect("legacy launch spec");

        assert_eq!(spec.program, settings.java_path);
        assert!(spec
            .args
            .windows(2)
            .any(|args| args == ["-jar", settings.jar_path.as_str()]));
        assert!(!spec.args.iter().any(|arg| arg == "--mcp"));
        assert!(!spec.mcp_enabled);
    }

    #[test]
    fn missing_native_launcher_falls_back_to_legacy_jar() {
        let mut settings = crate::settings::BSLServerSettings::default();
        settings.executable_path =
            r"C:\missing\bsl-language-server\bsl-language-server.exe".to_string();
        settings.jar_path = r"C:\MiniAI1C\bsl-language-server-0.28.5-exec.jar".to_string();
        settings.java_path = r"C:\Java\bin\java.exe".to_string();

        let spec = server_launch_spec(&settings, 8025).expect("legacy launch spec");

        assert_eq!(spec.program, settings.java_path);
        assert!(!spec.mcp_enabled);
        assert!(spec
            .args
            .windows(2)
            .any(|args| args == ["-jar", settings.jar_path.as_str()]));
    }

    #[test]
    fn official_mcp_discovery_finishes_before_chat_discovery_budget() {
        assert!(BSL_UPSTREAM_DISCOVERY_TIMEOUT < Duration::from_secs(2));
    }

    #[test]
    fn configured_workspace_overrides_internal_fallback() {
        let configured = std::path::Path::new(r"C:\Projects\DemoConfiguration");
        let fallback = std::path::Path::new(r"C:\MiniAI1C\bsl-workspace");

        assert_eq!(
            resolve_workspace_path(configured.to_string_lossy().as_ref(), fallback),
            configured
        );
        assert_eq!(resolve_workspace_path("", fallback), fallback);
    }

    #[test]
    fn temporary_analysis_document_is_created_inside_registered_workspace() {
        let workspace = std::path::Path::new(r"C:\Projects\Demo Configuration");

        let uri = temporary_document_uri(workspace, "check", 42);

        assert_eq!(
            uri,
            "file:///C:/Projects/Demo%20Configuration/.mini-ai-1c-check-42.bsl"
        );
    }

    #[test]
    fn official_mcp_config_targets_local_bsl_server() {
        let config = official_mcp_config(8025);

        assert_eq!(config.id, "bsl-ls-official");
        assert_eq!(config.url.as_deref(), Some("http://127.0.0.1:8025/mcp"));
        assert_eq!(config.transport, crate::settings::McpTransport::Http);
    }

    #[test]
    fn upstream_mcp_tools_are_merged_without_duplicate_names() {
        let internal = vec![McpTool {
            name: "check_bsl_syntax".to_string(),
            description: "internal".to_string(),
            input_schema: json!({"type": "object"}),
        }];
        let upstream = vec![
            McpTool {
                name: "analyze_file".to_string(),
                description: "upstream".to_string(),
                input_schema: json!({"type": "object"}),
            },
            McpTool {
                name: "check_bsl_syntax".to_string(),
                description: "duplicate".to_string(),
                input_schema: json!({"type": "object"}),
            },
        ];

        let tools = merge_bsl_tools(internal, upstream);

        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].description, "internal");
        assert_eq!(tools[1].name, "analyze_file");
    }

    #[test]
    fn document_update_sets_text_and_increments_version() {
        let mut doc = DocumentState {
            uri: "test".to_string(),
            version: 1,
            text: "old text".to_string(),
        };

        BSLClient::update_document_text(&mut doc, "new text".to_string());
        assert_eq!(doc.version, 2);
        assert_eq!(doc.text, "new text");
    }

    #[tokio::test]
    async fn pending_diagnostics_sender_is_fulfilled_when_channel_resolves() {
        let client = BSLClient::new();
        let uri = "file:///workspace/test.bsl";
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = client.pending_diagnostics.lock().unwrap();
            pending.insert(uri.to_string(), tx);
        }

        let test_diag = Diagnostic {
            range: Range {
                start: Position { line: 5, character: 2 },
                end: Position { line: 5, character: 10 },
            },
            severity: Some(1),
            message: "Syntax error test".to_string(),
            source: Some("bsl-language-server".to_string()),
        };

        // Simulate resolution
        let diag_tx = {
            let mut pending = client.pending_diagnostics.lock().unwrap();
            pending.remove(uri)
        };
        assert!(diag_tx.is_some());
        diag_tx.unwrap().send(vec![test_diag.clone()]).unwrap();

        let received = rx.await.expect("channel should resolve");
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].message, "Syntax error test");
        assert_eq!(received[0].range.start.line, 5);
    }
}
