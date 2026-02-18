use serde::{Deserialize, Serialize};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use crate::settings::{McpServerConfig, McpTransport, AppSettings};
use tokio::process::{Command, Child};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use std::collections::{HashMap, HashSet, VecDeque};
use lazy_static::lazy_static;
use notify::{Watcher, RecursiveMode, RecommendedWatcher, Config};
use std::thread;
use async_trait::async_trait;
use tauri::Manager;

#[async_trait]
pub trait InternalMcpHandler: Send + Sync {
    async fn list_tools(&self) -> Vec<McpTool>;
    async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, String>;
    fn is_alive(&self) -> bool { true }
}

#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Value,
    id: u64,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    result: Option<Value>,
    error: Option<JsonRpcError>,
    id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpServerStatus {
    pub id: String,
    pub name: String,
    pub status: String,
    pub transport: String,
}

// Global manager to hold persistent sessions
lazy_static! {
    static ref MCP_MANAGER: McpManager = McpManager::new();
}

pub struct McpManager {
    // Store both config and session to check for changes
    sessions: Arc<Mutex<HashMap<String, (McpServerConfig, Arc<McpSession>)>>>,
    internal_handlers: Arc<Mutex<HashMap<String, Arc<dyn InternalMcpHandler>>>>,
    app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
}

impl McpManager {
    fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            internal_handlers: Arc::new(Mutex::new(HashMap::new())),
            app_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn register_internal_handler(id: &str, handler: Arc<dyn InternalMcpHandler>) {
        let mut handlers = MCP_MANAGER.internal_handlers.lock().await;
        handlers.insert(id.to_string(), handler);
    }

    pub async fn get_client(config: McpServerConfig) -> Result<Arc<McpSession>, String> {
        let mut sessions = MCP_MANAGER.sessions.lock().await;

        // For Internal transport, we look up the registered handler
        if config.transport == McpTransport::Internal {
            crate::app_log!("[DEBUG] get_client for Internal: {}", config.id);
            if let Some((_, session)) = sessions.get(&config.id) {
                 if session.is_alive().await {
                     return Ok(session.clone());
                 }
            }
            
            let handlers = MCP_MANAGER.internal_handlers.lock().await;
            if let Some(handler) = handlers.get(&config.id) {
                let session = Arc::new(McpSession::new_internal(config.clone(), handler.clone()));
                sessions.insert(config.id.clone(), (config, session.clone()));
                return Ok(session);
            } else {
                return Err(format!("Internal handler not found for {}", config.id));
            }
        }

        // For HTTP transport, we don't need persistence in the same way, but let's unify interface
        if config.transport == McpTransport::Http {
            return Ok(Arc::new(McpSession::new_http(config)));
        }

        // For Stdio, we check if we have a running session AND if config matches
        if let Some((stored_config, session)) = sessions.get(&config.id) {
            if session.is_alive().await && stored_config == &config {
                return Ok(session.clone());
            }
            // If config changed or dead, we'll replace it below
        }

        // Create new session
        let settings = crate::settings::load_settings();
        let session = Arc::new(McpSession::new_stdio(config.clone(), settings.debug_mcp).await?);
        sessions.insert(config.id.clone(), (config.clone(), session.clone()));
        Ok(session)
    }

    pub async fn reconfigure(new_settings: AppSettings, app_handle: &tauri::AppHandle) {
        println!("Reconfiguring MCP servers...");
        let mut sessions = MCP_MANAGER.sessions.lock().await;

        let new_server_ids: HashSet<String> = new_settings.mcp_servers.iter()
            .map(|s| s.id.clone())
            .collect();

        // 1. Remove servers that are no longer in settings
        sessions.retain(|id, _| new_server_ids.contains(id));

        // 2. Update or Create servers
        for config in new_settings.mcp_servers {
            if !config.enabled {
                // If disabled, remove if exists
                sessions.remove(&config.id);
                continue;
            }

            let needs_restart = if let Some((stored_config, session)) = sessions.get(&config.id) {
                stored_config != &config || !session.is_alive().await
            } else {
                true 
            };

            if needs_restart {
                println!("Restarting/Starting MCP server: {}", config.name);
                // Remove old session if exists to ensure cleanup (drop will kill child)
                sessions.remove(&config.id);

                if config.transport == McpTransport::Internal {
                    let handlers = MCP_MANAGER.internal_handlers.lock().await;
                    if let Some(handler) = handlers.get(&config.id) {
                        let session = Arc::new(McpSession::new_internal(config.clone(), handler.clone()));
                        sessions.insert(config.id.clone(), (config, session));
                    }
                    continue;
                }

                match McpSession::new_stdio(config.clone(), new_settings.debug_mcp).await {
                    Ok(session) => {
                        let session = Arc::new(session);
                        println!("Started MCP server: {}", config.id);
                        sessions.insert(config.id.clone(), (config, session));
                    }
                    Err(e) => {
                        eprintln!("Failed to start MCP server {}: {}", config.name, e);
                    }
                }
            }
        }

        // 3. Handle BSL Server (Virtual)
        let bsl_client_state = app_handle.state::<Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>();
        let bsl_client = bsl_client_state.inner().clone();
        
        let mut bsl = bsl_client.lock().await;
        if new_settings.bsl_server.enabled {
            let jar_exists = std::path::Path::new(&new_settings.bsl_server.jar_path).exists();
            if jar_exists && !bsl.is_connected() {
                println!("Restarting/Starting BSL LS because it was enabled and not connected");
                let _ = bsl.start_server();
                let _ = bsl.connect().await;
            }
        } else {
            bsl.stop();
        }
    }

    pub async fn get_statuses() -> Vec<McpServerStatus> {
        let sessions = MCP_MANAGER.sessions.lock().await;
        let mut statuses = Vec::new();

        // Load settings to get the full list of servers, including those not running
        let settings = crate::settings::load_settings();

        let mut all_configs = settings.mcp_servers.clone();
        
        // Add virtual BSL server
        all_configs.push(crate::settings::McpServerConfig {
            id: "bsl-ls".to_string(),
            name: "BSL Language Server".to_string(),
            enabled: settings.bsl_server.enabled,
            transport: crate::settings::McpTransport::Internal,
            ..Default::default()
        });

        for config in all_configs {
             let status = if !config.enabled {
                 "disabled"
             } else if let Some((_, session)) = sessions.get(&config.id) {
                 if session.is_alive().await {
                     "connected"
                 } else {
                     "stopped"
                 }
             } else if config.transport == McpTransport::Internal {
                 let handlers = MCP_MANAGER.internal_handlers.lock().await;
                 if handlers.contains_key(&config.id) {
                     "connected"
                 } else {
                     "stopped"
                 }
             } else {
                 "stopped" // Enabled but not in sessions (failed to start or never started)
             };

             println!("[DEBUG] MCP Server status for {}: {}", config.id, status);
             statuses.push(McpServerStatus {
                id: config.id.clone(),
                name: config.name.clone(),
                status: status.to_string(),
                transport: format!("{:?}", config.transport).to_lowercase(),
            });
        }
        
        statuses
    }

    pub async fn get_logs(server_id: &str) -> Vec<String> {
        let sessions = MCP_MANAGER.sessions.lock().await;
        if let Some((_, session)) = sessions.get(server_id) {
            let logs = session.logs.lock().await;
            logs.iter().cloned().collect()
        } else {
            Vec::new()
        }
    }
}

pub fn start_settings_watcher(app_handle: tauri::AppHandle) {
    // Store app_handle in manager for path resolution
    {
        let handle_inner = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            let mut h = MCP_MANAGER.app_handle.lock().await;
            *h = Some(handle_inner);
        });
    }

    let _app_handle_for_watcher = app_handle.clone();
    thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        
        // Use RecommendedWatcher
        let mut watcher = RecommendedWatcher::new(tx, Config::default()).unwrap();
        
        // Watch the parent directory because atomic writes (rename) might change inode
        let config_dir = crate::settings::get_settings_dir();

        if let Err(e) = watcher.watch(&config_dir, RecursiveMode::NonRecursive) {
             eprintln!("Failed to watch settings dir: {}", e);
             return;
        }

        println!("Started watching settings at {:?}", config_dir);

        for res in rx {
            match res {
                Ok(event) => {
                    // Check if settings.json was modified
                    let interesting = event.paths.iter().any(|p| {
                        p.file_name().and_then(|n| n.to_str()).map(|s| s == "settings.json").unwrap_or(false)
                    });

                    if interesting {
                        // Debounce? or just reload. 
                        // It's better to wait a bit to ensure write is complete.
                        thread::sleep(Duration::from_millis(100));
                        
                        // Run async reconfigure in tauri runtime
                        let app_handle_clone = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let settings = crate::settings::load_settings();
                            McpManager::reconfigure(settings, &app_handle_clone).await;
                        });
                    }
                },
                Err(e) => eprintln!("Watch error: {:?}", e),
            }
        }
    });
}

pub struct McpClient {
    session: Arc<McpSession>,
}

impl McpClient {
    pub async fn new(config: McpServerConfig) -> Result<Self, String> {
       let session = McpManager::get_client(config).await?;
       Ok(Self { session })
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>, String> {
        match tokio::time::timeout(Duration::from_secs(60), self.session.list_tools()).await {
            Ok(res) => res,
            Err(_) => Err("Timeout listing tools".to_string()),
        }
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, String> {
        match tokio::time::timeout(Duration::from_secs(30), self.session.call_tool(name, arguments)).await {
            Ok(res) => res,
            Err(_) => Err(format!("Timeout executing tool '{}'", name)),
        }
    }
}

enum TransportImpl {
    Http {
        client: Client,
        url: String,
        login: Option<String>,
        password: Option<String>,
    },
    Stdio {
        tx: mpsc::Sender<JsonRpcRequest>,
        pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>,
        // We keep the child here just to keep the process alive
        _child: Arc<Mutex<Child>>, 
    },
    Internal {
        handler: Arc<dyn InternalMcpHandler>,
    }
}

pub struct McpSession {
    transport: TransportImpl,
    next_id: std::sync::atomic::AtomicU64,
    logs: Arc<Mutex<VecDeque<String>>>,
}

impl McpSession {
    fn new_http(config: McpServerConfig) -> Self {
        Self {
            transport: TransportImpl::Http {
                client: Client::builder()
                    .timeout(Duration::from_secs(30))
                    .build()
                    .unwrap_or_default(),
                url: config.url.unwrap_or_default(),
                login: config.login,
                password: config.password,
            },
            next_id: std::sync::atomic::AtomicU64::new(1),
            logs: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    fn new_internal(_config: McpServerConfig, handler: Arc<dyn InternalMcpHandler>) -> Self {
        Self {
            transport: TransportImpl::Internal {
                handler,
            },
            next_id: std::sync::atomic::AtomicU64::new(1),
            logs: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    async fn new_stdio(config: McpServerConfig, debug_all: bool) -> Result<Self, String> {
        let server_id_for_logs = config.id.clone();
        let command = config.command.ok_or("Command is missing")?;
        let mut args = config.args.unwrap_or_default();

        // Path resolution for production (Tauri Resources)
        // Only resolve as resources if we are NOT in debug mode,
        // because in debug mode we want to use the local files via npx/tsx.
        #[cfg(not(debug_assertions))]
        {
            let cmd_lower = command.to_lowercase();
            let is_stdio_node_launcher = cmd_lower == "npx" || cmd_lower == "npx.cmd" || cmd_lower == "node" || cmd_lower.contains("tsx");
            
            if is_stdio_node_launcher {
                let app_handle_opt = MCP_MANAGER.app_handle.lock().await;
                if let Some(app_handle) = app_handle_opt.as_ref() {
                    crate::app_log!("[MCP] Resolving resources for command '{}' with args {:?}", command, args);
                    for arg in args.iter_mut() {
                        if arg.contains("mcp-servers") && (arg.ends_with(".ts") || arg.ends_with(".js")) {
                            let filename = std::path::Path::new(&*arg)
                                .file_name()
                                .and_then(|f| f.to_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| arg.to_string());
                            
                            // Priority 1: .js version (for production)
                            let js_filename = filename.replace(".ts", ".js");
                            let js_subpath = format!("mcp-servers/{}", js_filename);
                            
                            // Priority 2: original filename
                            let orig_subpath = format!("mcp-servers/{}", filename);

                            let mut resolved = false;
                            
                            // Try JS first in any case if it exists in resources
                            if let Ok(path) = app_handle.path().resolve(&js_subpath, tauri::path::BaseDirectory::Resource) {
                                if path.exists() {
                                    let path_str = path.to_string_lossy().to_string();
                                    // Normalize for Node.js on Windows (remove \\?\ prefix)
                                    *arg = path_str.strip_prefix(r"\\?\").unwrap_or(&path_str).to_string();
                                    crate::app_log!("[MCP] Resolved to JS resource: {}", arg);
                                    resolved = true;
                                }
                            }
                            
                            if !resolved {
                                if let Ok(path) = app_handle.path().resolve(&orig_subpath, tauri::path::BaseDirectory::Resource) {
                                    if path.exists() {
                                        let path_str = path.to_string_lossy().to_string();
                                        *arg = path_str.strip_prefix(r"\\?\").unwrap_or(&path_str).to_string();
                                        crate::app_log!("[MCP] Resolved to original resource: {}", arg);
                                        resolved = true;
                                    }
                                }
                            }

                            if !resolved {
                                 crate::app_log!("[WARN] Could not resolve MCP resource '{}' in 'mcp-servers/' folder", filename);
                            }
                        }
                    }
                }
            }
        }

        let (mut command, mut args) = if cfg!(windows) {
            // On Windows, if command is 'npx' or 'npm', we might need .cmd
            // Also avoid wrapping in cmd /C unless absolutely necessary, to keep PID correct.
            let cmd_lower = command.to_lowercase();
            if cmd_lower == "npx" || cmd_lower == "npm" {
                (format!("{}.cmd", command), args)
            } else {
                (command, args)
            }
        } else {
            (command, args)
        };

        #[cfg(not(debug_assertions))]
        {
            let cmd_lower = command.to_lowercase();
            let is_tsx_launcher = cmd_lower.contains("npx") || cmd_lower.contains("tsx");
            
            if is_tsx_launcher {
                 let has_ts_or_js = args.iter().any(|a| a.ends_with(".ts") || a.ends_with(".js"));
                 if has_ts_or_js {
                     crate::app_log!("[MCP] Production mode detected. Switching launcher to node for portability.");
                     command = "node".to_string();
                     // Filter out npx specific flags and switch .ts to .js
                     let mut new_args = Vec::new();
                     for arg in args {
                         if arg == "--yes" || arg == "tsx" || arg.contains("node_modules") {
                             continue;
                         }
                         // Since we already resolved absolute paths above, we just pass them to node
                         if arg.ends_with(".ts") {
                             new_args.push(arg.replace(".ts", ".js"));
                         } else {
                             new_args.push(arg);
                         }
                     }
                     args = new_args;
                 }
            }
        }

        crate::app_log!("[MCP] Spawning server process: {} {:?}", command, args);

        let mut cmd = Command::new(&command);
        
        if let Some(env) = &config.env {
            cmd.envs(env);
        }

        // Pass global debug flag
        if debug_all {
            cmd.env("ONEC_AI_DEBUG", "true");
        }

        cmd.args(args)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Hide console window on Windows
        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        let mut child = cmd.spawn()
            .map_err(|e| format!("Failed to spawn {}: {}", command, e))?;

        let mut stdin = child.stdin.take().ok_or("Failed to open stdin")?;
        let stdout = child.stdout.take().ok_or("Failed to open stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to open stderr")?;

        let (tx, mut rx) = mpsc::channel::<JsonRpcRequest>(32);
        let pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>> = 
            Arc::new(Mutex::new(HashMap::new()));

        let logs = Arc::new(Mutex::new(VecDeque::with_capacity(100)));
        let logs_writer = logs.clone();

        // Writer task
        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                if let Ok(json) = serde_json::to_string(&req) {
                    if let Err(_) = stdin.write_all(format!("{}\n", json).as_bytes()).await {
                        break;
                    }
                    if let Err(_) = stdin.flush().await {
                        break;
                    }
                }
            }
        });

        // Reader task
        let pending_requests_reader = pending_requests.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            let mut stderr_reader = BufReader::new(stderr).lines();

            loop {
                tokio::select! {
                     line_res = reader.next_line() => {
                        match line_res {
                            Ok(Some(line)) => {
                                if !line.trim().starts_with('{') {
                                    // println!("MCP Stderr/Log: {}", line);
                                    continue; 
                                }
                                
                                if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&line) {
                                    if let Some(id) = response.id {
                                        let mut pending = pending_requests_reader.lock().await;
                                        if let Some(sender) = pending.remove(&id) {
                                            let result = if let Some(err) = response.error {
                                                Err(format!("MCP Error {}: {}", err.code, err.message))
                                            } else {
                                                Ok(response.result.unwrap_or(Value::Null))
                                            };
                                            let _ = sender.send(result);
                                        }
                                    }
                                }
                            }
                            _ => break, // Exit loop on EOF or error
                        }
                     }
                     stderr_res = stderr_reader.next_line() => {
                         // Consume stderr to prevent buffer fill
                         if let Ok(Some(line)) = stderr_res {
                             crate::app_log!("[MCP][{}][STDERR] {}", server_id_for_logs, line);
                             let mut logs = logs_writer.lock().await;
                             if logs.len() >= 100 {
                                 logs.pop_front();
                             }
                             logs.push_back(line);
                         } else {
                             // logs?
                         }
                     }
                }
            }
        });

        Ok(Self {
            transport: TransportImpl::Stdio {
                tx,
                pending_requests,
                _child: Arc::new(Mutex::new(child)),
            },
            next_id: std::sync::atomic::AtomicU64::new(1),
            logs,
        })
    }

    async fn is_alive(&self) -> bool {
        match &self.transport {
            TransportImpl::Http { .. } => true,
            TransportImpl::Stdio { _child, .. } => {
                // Check if child has exited
                let mut child = _child.lock().await;
                child.try_wait().map(|s| s.is_none()).unwrap_or(false)
            }
            TransportImpl::Internal { handler } => handler.is_alive(),
        }
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: params.clone(),
            id,
        };

        match &self.transport {
            TransportImpl::Http { client, url, login, password } => {
                let mut rb = client.post(url).json(&req);
                if let Some(l) = login {
                    if !l.is_empty() {
                       rb = rb.basic_auth(l, password.as_deref());
                    }
                }
                let resp = rb.send().await.map_err(|e| e.to_string())?;
                let rpc_res: JsonRpcResponse = resp.json().await.map_err(|e| e.to_string())?;
                if let Some(err) = rpc_res.error {
                    Err(format!("MCP Error {}: {}", err.code, err.message))
                } else {
                    Ok(rpc_res.result.unwrap_or(Value::Null))
                }
            }
            TransportImpl::Stdio { tx, pending_requests, .. } => {
                let (auth_tx, auth_rx) = oneshot::channel();
                {
                    let mut pending = pending_requests.lock().await;
                    pending.insert(id, auth_tx);
                }

                tx.send(req).await.map_err(|_| "Failed to send request to MCP process".to_string())?;

                match tokio::time::timeout(Duration::from_secs(30), auth_rx).await {
                    Ok(res) => res.map_err(|_| "Channel closed".to_string())?,
                    Err(_) => {
                        let mut pending = pending_requests.lock().await;
                        pending.remove(&id);
                        Err("Timeout waiting for MCP response".to_string())
                    }
                }
            }
            TransportImpl::Internal { handler } => {
                handler.call_tool(method, params.clone()).await
            }
        }
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>, String> {
        match &self.transport {
            TransportImpl::Internal { handler } => Ok(handler.list_tools().await),
            _ => {
                let result = self.request("tools/list", json!({})).await?;
                if let Some(tools_arr) = result.get("tools").and_then(|v| v.as_array()) {
                        let tools = tools_arr.iter().filter_map(|v| {
                        Some(McpTool {
                            name: v.get("name")?.as_str()?.to_string(),
                            description: v.get("description").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                            input_schema: v.get("inputSchema")?.clone(),
                        })
                    }).collect();
                    Ok(tools)
                } else {
                    Ok(Vec::new())
                }
            }
        }
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, String> {
        crate::app_log!("[DEBUG] McpSession::call_tool: {}", name);
        match &self.transport {
            TransportImpl::Internal { handler } => {
                crate::app_log!("[DEBUG] McpSession::call_tool handling Internal for {}", name);
                handler.call_tool(name, arguments).await
            }
            _ => {
                self.request("tools/call", json!({
                    "name": name,
                    "arguments": arguments
                })).await
            }
        }
    }
}
