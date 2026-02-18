//! BSL Language Server client
//! Communicates with BSL LS via WebSocket using JSON-RPC

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command as AsyncCommand};
use std::process::{Command as StdCommand, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use std::time::Duration;

use crate::settings::load_settings;
use crate::mcp_client::{InternalMcpHandler, McpTool};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// BSL Language Server client
pub struct BSLClient {
    ws: Option<Mutex<WebSocketStream<MaybeTlsStream<TcpStream>>>>,
    server_process: Option<Child>,
    request_id: AtomicI32,
    capabilities: Option<serde_json::Value>,
    workspace_root: Option<String>,
    actual_port: Option<u16>,
}

impl BSLClient {
    pub fn new() -> Self {
        Self {
            ws: None,
            server_process: None,
            request_id: AtomicI32::new(1),
            capabilities: None,
            workspace_root: None,
            actual_port: None,
        }
    }

    /// Find an available TCP port starting from the preferred port
    fn find_available_port(preferred: u16) -> u16 {
        let mut port = preferred;
        while port < preferred + 100 {
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                return port;
            }
            port += 1;
        }
        preferred // Fallback to preferred if none found in range
    }

    pub fn is_connected(&self) -> bool {
        self.ws.is_some()
    }

    /// Start the BSL Language Server
    pub fn start_server(&mut self) -> Result<(), String> {
        let settings = load_settings();
        
        if !settings.bsl_server.enabled {
            return Err("BSL LS is disabled in settings".to_string());
        }
        
        let jar_path = &settings.bsl_server.jar_path;
        if jar_path.is_empty() {
            return Err("BSL LS JAR path not configured".to_string());
        }
        
        // Dynamic port search for terminal server compatibility
        let preferred_port = settings.bsl_server.websocket_port;
        let port = Self::find_available_port(preferred_port);
        self.actual_port = Some(port);

        crate::app_log!("[BSL LS] Starting on port {} (preferred was {})", port, preferred_port);
        
        let mut cmd = AsyncCommand::new(&settings.bsl_server.java_path);
        cmd.args([
                // Increase WebSocket message buffer from 8KB default to 1MB
                "-Dorg.apache.tomcat.websocket.DEFAULT_BUFFER_SIZE=1048576",
                // Minimal memory footprint for terminal server (256MB heap + Serial GC)
                "-Xmx256m",
                "-XX:+UseSerialGC",
                "-jar",
                jar_path,
                "websocket",
                &format!("--server.port={}", port),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000);
        }

        let mut child = cmd.spawn()
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
        crate::app_log!("BSL LS process spawned");
        Ok(())
    }

    /// Connect to the BSL Language Server
    pub async fn connect(&mut self) -> Result<(), String> {
        let port = self.actual_port.unwrap_or_else(|| {
            load_settings().bsl_server.websocket_port
        });
        let url = format!("ws://127.0.0.1:{}/lsp", port);
        
        crate::app_log!("[BSL LS] Attempting to connect to {}", url);
        
        let mut retries = 0;
        let max_retries = 30; // 15 seconds total
        
        loop {
            // Add timeout to connect_async to prevent hang during handshake (common in terminal servers)
            let connect_timeout = tokio::time::timeout(
                tokio::time::Duration::from_secs(3),
                connect_async(&url)
            ).await;

            match connect_timeout {
                Ok(Ok((ws_stream, _))) => {
                    crate::app_log!("[BSL LS] WebSocket connected successfully to {}", url);
                    self.ws = Some(Mutex::new(ws_stream));
                    break;
                }
                Ok(Err(e)) => {
                    retries += 1;
                    if retries >= max_retries {
                         crate::app_log!("[BSL LS] Connection FAILED after {} attempts. Last error: {}", max_retries, e);
                         return Err(format!("Failed to connect to BSL LS after {} attempts: {}", max_retries, e));
                    }
                    if retries % 5 == 0 {
                        crate::app_log!("[BSL LS] connection attempt {}/{}... (error: {})", retries, max_retries, e);
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
                Err(_) => {
                    retries += 1;
                    crate::app_log!("[BSL LS] Connection HANDSHAKE TIMEOUT (3s) at {}/{}", retries, max_retries);
                    if retries >= max_retries {
                        return Err(format!("Failed to connect to BSL LS (Handshake Timeout) after {} attempts", max_retries));
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
                    "willSave": true,
                    "willSaveWaitUntil": false,
                    "didSave": true
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

        // Setup persistent workspace for BSL LS
        let app_data = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        let workspace_path = std::path::PathBuf::from(app_data).join("MiniAI1C").join("bsl-workspace");
        std::fs::create_dir_all(&workspace_path).unwrap_or_default();
        let root_dir = workspace_path.to_string_lossy().replace('\\', "/");
        self.workspace_root = Some(root_dir.clone());

        // Create default bsl-ls.json if it doesn't exist
        let config_path = workspace_path.join(".bsl-language-server.json");
        if !config_path.exists() {
            let config = serde_json::json!({
                "language": "ru",
                "diagnostics": {
                    "parameters": {
                        "EmptyLines": { "maxCount": 1 }
                    }
                }
            });
            let _ = std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap_or_default());
        }

        let root_uri = format!("file:///{}", root_dir.trim_start_matches('/'));
        
        let initialize_result = self.send_request("initialize", serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "workspaceFolders": [{
                "uri": root_uri,
                "name": "BSL Workspace"
            }],
            "capabilities": client_capabilities,
            "trace": "verbose"
        })).await?;
        
        // Store server capabilities
        self.capabilities = initialize_result.get("capabilities").cloned();
        crate::app_log!("[BSL LS] Initialized. Server capabilities: {:?}", self.capabilities.as_ref().map(|c| c.to_string()));

        // Notify initialized and pump server messages for 1 second
        {
            let ws_ref = self.ws.as_ref().ok_or("Not connected")?;
            let mut ws = ws_ref.lock().await;

            let init_notif = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: None,
                method: "initialized".to_string(),
                params: serde_json::json!({}),
            };
            if let Ok(msg) = serde_json::to_string(&init_notif) {
                ws.send(Message::Text(msg)).await.map_err(|e| e.to_string())?;
                crate::app_log!("[BSL LS] Sent 'initialized' notification");
            }

            // Quick drain for server-initiated requests (configuration, etc.)
            let drain_duration = tokio::time::Duration::from_millis(800);
            let drain_timeout = tokio::time::sleep(drain_duration);
            tokio::pin!(drain_timeout);
            loop {
                tokio::select! {
                    msg = ws.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                crate::app_log!("[BSL LS] <<< Initial drain msg: {}", text);
                                if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&text) {
                                    if resp.method.is_some() && resp.id.is_some() {
                                        let method = resp.method.as_ref().unwrap();
                                        let id = resp.id.unwrap();
                                        crate::app_log!("[BSL LS] Handling server request during drain: {} id={}", method, id);
                                        Self::handle_server_request(&mut ws, method, id, &resp.params).await;
                                        continue;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ = &mut drain_timeout => break,
                }
            }
        }
        
        Ok(())
    }

    /// Send a JSON-RPC response to a server-initiated request
    async fn send_response_raw(ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>, id: i32, result: serde_json::Value) -> Result<(), String> {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        });
        let msg = serde_json::to_string(&response).map_err(|e| e.to_string())?;
        crate::app_log!("[BSL LS] >>> Sending response for id={}: {}", id, msg);
        ws.send(Message::Text(msg)).await.map_err(|e| e.to_string())
    }

    /// Handle server-initiated requests
    async fn handle_server_request(ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>, method: &str, id: i32, _params: &Option<serde_json::Value>) {
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
                let _ = Self::send_response_raw(ws, id, config).await;
            }
            "client/registerCapability" => {
                let _ = Self::send_response_raw(ws, id, serde_json::json!({})).await;
            }
            _ => {
                crate::app_log!("[BSL LS] Warning: Unhandled server request: {}", method);
                let _ = Self::send_response_raw(ws, id, serde_json::Value::Null).await;
            }
        }
    }

    /// Send JSON-RPC request with timeout
    async fn send_request(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        let ws = self.ws.as_ref().ok_or("Not connected")?;
        let mut ws = ws.lock().await;
        
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: method.to_string(),
            params,
        };
        
        let msg = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        crate::app_log!("[BSL LS] >>> Request {}: {}", method, msg);
        ws.send(Message::Text(msg))
            .await
            .map_err(|e| e.to_string())?;
        
        // Wait for response with overall timeout
        let request_timeout = Duration::from_secs(15);
        let start = std::time::Instant::now();
        
        while start.elapsed() < request_timeout {
            let next_msg_timeout = tokio::time::timeout(Duration::from_secs(5), ws.next());
            
            match next_msg_timeout.await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    crate::app_log!("[BSL LS] <<< Message: {}", text);
                    if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&text) {
                        // Server request
                        if response.method.is_some() && response.id.is_some() {
                            let method = response.method.as_ref().unwrap();
                            let srv_id = response.id.unwrap();
                            Self::handle_server_request(&mut ws, method, srv_id, &response.params).await;
                            continue;
                        }

                        // Response for our request
                        if response.id == Some(id) {
                            if let Some(error) = response.error {
                                crate::app_log!("[BSL LS] LSP error response: {:?}", error);
                                return Err(format!("LSP error {}: {}", error.code, error.message));
                            }
                            return Ok(response.result.unwrap_or(serde_json::Value::Null));
                        }
                    }
                }
                Ok(Some(Err(e))) => {
                    crate::app_log!("[BSL LS] WebSocket error: {}", e);
                    return Err(e.to_string());
                }
                Ok(None) => {
                    crate::app_log!("[BSL LS] WebSocket closed while waiting for response");
                    return Err("Connection closed".to_string());
                }
                Err(_) => {
                    // next_msg_timeout triggered
                    crate::app_log!("[BSL LS] No message for 5s (total elapsed: {:?})", start.elapsed());
                }
                _ => {}
            }
        }
        
        crate::app_log!("[BSL LS] TIMEOUT (15s) waiting for response to '{}' request", method);
        Err(format!("Timeout waiting for BSL LS response to '{}'", method))
    }

    /// Send JSON-RPC notification
    async fn send_notification(&self, method: &str, params: serde_json::Value) -> Result<(), String> {
        let ws = self.ws.as_ref().ok_or("Not connected")?;
        let mut ws = ws.lock().await;
        
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.to_string(),
            params,
        };
        
        let msg = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        ws.send(Message::Text(msg))
            .await
            .map_err(|e| e.to_string())?;
            
        Ok(())
    }

    /// Analyze code and return diagnostics
    pub async fn analyze_code(&self, code: &str, uri: &str) -> Result<Vec<Diagnostic>, String> {
        println!("[BSL LS] Starting analysis for URI: {}", uri);

        // Send didOpen notification
        self.send_notification("textDocument/didOpen", serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": "bsl",
                "version": 1,
                "text": code
            }
        })).await?;
        
        // Try Pull-Model Diagnostics (LSP 3.17+)
        let supports_pull_diagnostics = self.capabilities.as_ref()
            .and_then(|c| c.get("diagnosticProvider"))
            .is_some();

        if supports_pull_diagnostics {
            println!("[BSL LS] Using pull-model diagnostics");
            let result = self.send_request("textDocument/diagnostic", serde_json::json!({
                "textDocument": {
                    "uri": uri
                }
            })).await?;

            // Close document
            self.send_notification("textDocument/didClose", serde_json::json!({
                "textDocument": {
                    "uri": uri
                }
            })).await?;

            if let Some(items) = result.get("items").and_then(|v| v.as_array()) {
                println!("[BSL LS] Pull diagnostics raw: {:?}", items);
                let diagnostics: Vec<Diagnostic> = items
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect();
                println!("[BSL LS] Parsed diagnostics count: {}", diagnostics.len());
                return Ok(diagnostics);
            } else {
                println!("[BSL LS] Pull diagnostics 'items' field missing or not array");
            }
        }

        // Fallback or parallel: Listen for publishDiagnostics
        println!("[BSL LS] Falling back to publishDiagnostics listener");
        let ws = self.ws.as_ref().ok_or("Not connected")?;
        let mut ws = ws.lock().await;

        // Wait up to 5 seconds for diagnostics (increased from 2s)
        let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(5));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                msg = ws.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&text) {
                                // Server request
                                if response.method.is_some() && response.id.is_some() {
                                    let method = response.method.as_ref().unwrap();
                                    let srv_id = response.id.unwrap();
                                    Self::handle_server_request(&mut ws, method, srv_id, &response.params).await;
                                    continue;
                                }

                                // Check if it is publishDiagnostics
                                if let Some(method) = &response.method {
                                    if method == "textDocument/publishDiagnostics" {
                                        println!("[BSL LS] Received publishDiagnostics");
                                        if let Some(params) = response.params {
                                            // Ensure it's for our URI
                                            if let Some(diag_uri) = params.get("uri").and_then(|u| u.as_str()) {
                                                println!("[BSL LS] Diagnostics URI: {}, Expected: {}", diag_uri, uri);
                                                
                                                // Normalize check: BSL LS might add drive letter
                                                let filename = uri.split('/').last().unwrap_or(uri);
                                                
                                                if diag_uri == uri || diag_uri.ends_with(filename) {
                                                    let items = params.get("diagnostics")
                                                        .and_then(|v| v.as_array())
                                                        .cloned()
                                                        .unwrap_or_default();
                                                    
                                                    println!("[BSL LS] Found {} diagnostics", items.len());

                                                    let diagnostics: Vec<Diagnostic> = items
                                                        .into_iter()
                                                        .filter_map(|v| serde_json::from_value(v).ok())
                                                        .collect();
                                                    
                                                    // Close document
                                                    let close_req = JsonRpcRequest {
                                                        jsonrpc: "2.0".to_string(),
                                                        id: None,
                                                        method: "textDocument/didClose".to_string(),
                                                        params: serde_json::json!({
                                                            "textDocument": { "uri": uri }
                                                        }),
                                                    };
                                                    if let Ok(msg) = serde_json::to_string(&close_req) {
                                                         let _ = ws.send(Message::Text(msg)).await;
                                                         println!("[BSL LS] Sent didClose (manual)");
                                                    }

                                                    return Ok(diagnostics);
                                                }
                                            }
                                        }
                                    } else if method == "window/logMessage" {
                                        if let Some(params) = &response.params {
                                            let msg_text = params.get("message").and_then(|m| m.as_str()).unwrap_or("");
                                            println!("[BSL LS][server] {}", msg_text);
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            println!("[BSL LS] Error reading message: {}", e);
                            return Err(e.to_string());
                        }
                        None => {
                            println!("[BSL LS] Connection closed by server");
                            return Err("Connection closed".to_string());
                        }
                        _ => {
                            // Ignore other messages (Ping/Pong/Binary)
                        }
                    }
                }
                _ = &mut timeout => {
                    println!("[BSL LS] Timeout waiting for diagnostics");
                    // Close document even on timeout (manual send)
                    let close_req = JsonRpcRequest {
                        jsonrpc: "2.0".to_string(),
                        id: None,
                        method: "textDocument/didClose".to_string(),
                        params: serde_json::json!({
                            "textDocument": {
                                "uri": uri
                            }
                        }),
                    };
                    if let Ok(msg) = serde_json::to_string(&close_req) {
                            let _ = ws.send(Message::Text(msg)).await;
                    }
                    
                    return Ok(Vec::new());
                }
            }
        }
    }

    /// Format code
    pub async fn format_code(&self, code: &str, uri: &str) -> Result<String, String> {
        // Guard check
        let can_format = self.capabilities.as_ref()
            .and_then(|c| c.get("documentFormattingProvider"))
            .and_then(|v| v.as_bool().or_else(|| v.as_object().map(|_| true)))
            .unwrap_or(false);

        if !can_format {
            return Err("BSL LS does not support formatting for this document".to_string());
        }

        // Open document
        self.send_notification("textDocument/didOpen", serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": "bsl",
                "version": 1,
                "text": code
            }
        })).await?;
        
        // Request formatting
        let result = self.send_request("textDocument/formatting", serde_json::json!({
            "textDocument": {
                "uri": uri
            },
            "options": {
                "tabSize": 4,
                "insertSpaces": true
            }
        })).await?;
        
        // Close document
        self.send_notification("textDocument/didClose", serde_json::json!({
            "textDocument": {
                "uri": uri
            }
        })).await?;
        
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
    pub async fn goto_definition(&self, uri: &str, line: u32, character: u32) -> Result<Option<crate::bsl_client::Location>, String> {
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
        if let Ok(location) = serde_json::from_value::<crate::bsl_client::Location>(result.clone()) {
            return Ok(Some(location));
        }

        // Case 2: Array of Locations (take first)
        if let Ok(locations) = serde_json::from_value::<Vec<crate::bsl_client::Location>>(result.clone()) {
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
                    if let Some(target_range) = first_link.get("targetSelectionRange") { // Use selection range for precision
                         if let Ok(range) = serde_json::from_value::<crate::bsl_client::Range>(target_range.clone()) {
                             return Ok(Some(crate::bsl_client::Location {
                                 uri: target_uri.to_string(),
                                 range
                             }));
                         }
                    }
                }
            }
        }

        Ok(None)
    }

    /// Resolve definition and return source code
    pub async fn resolve_definition(&self, code: &str, line: u32, character: u32) -> Result<String, String> {
        let uri = "file:///temp_definition.bsl";

        // 1. Open document
        self.send_notification("textDocument/didOpen", serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": "bsl", // "bsl" (1c)
                "version": 1,
                "text": code
            }
        })).await?;

        // 2. Request definition
        let location_opt = self.goto_definition(uri, line, character).await?;

        // 3. Close document
        self.send_notification("textDocument/didClose", serde_json::json!({
            "textDocument": {
                "uri": uri
            }
        })).await?;

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
                 let content = tokio::fs::read_to_string(path).await
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

    /// Stop the server
    pub fn stop(&mut self) {
        if let Some(mut child) = self.server_process.take() {
             // Try to send exit notification if WS is still alive
             if let Some(ws_mutex) = self.ws.take() {
                 tokio::spawn(async move {
                     let mut ws = ws_mutex.lock().await;
                     let exit_notif = JsonRpcRequest {
                         jsonrpc: "2.0".to_string(),
                         id: None,
                         method: "exit".to_string(),
                         params: serde_json::json!({}),
                     };
                     if let Ok(msg) = serde_json::to_string(&exit_notif) {
                         let _ = ws.send(Message::Text(msg)).await;
                     }
                     // Give it a tiny bit of time to breathe
                     tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                 });
             }

            let _ = child.kill();
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
            },
            Err(_) => "Not found".to_string(),
        }
    }

    /// Check if BSL LS is installed (JAR exists)
    pub fn check_install(jar_path: &str) -> bool {
        std::path::Path::new(jar_path).exists()
    }
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
        vec![
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
            }
        ]
    }

    async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> Result<serde_json::Value, String> {
        match name {
            "check_bsl_syntax" => {
                let code = arguments.get("code")
                    .and_then(|v| v.as_str())
                    .ok_or("Параметр 'code' обязателен для check_bsl_syntax")?;
                
                let mut client = self.client.lock().await;
                
                // Ensure server is started and connected
                if !client.is_connected() {
                    // Try to connect if server is likely running
                    if let Err(e) = client.connect().await {
                         // If connection fails, check if server needs to be started
                         if client.server_process.is_none() {
                             client.start_server()?;
                         }
                         client.connect().await.map_err(|e2| format!("BSL LS не запущен или недоступен: {}\nДоп. ошибка: {}", e, e2))?;
                    }
                }

                let uri = "file:///mcp_check_syntax.bsl";
                let diagnostics = client.analyze_code(code, uri).await?;
                
                Ok(json!({
                    "diagnostics": diagnostics,
                    "count": diagnostics.len()
                }))
            }
            _ => Err(format!("Неизвестный инструмент BSL: {}", name)),
        }
    }

    fn is_alive(&self) -> bool {
        // Run checks for Java and JAR
        let settings = load_settings();
        
        // 1. Check if enabled
        if !settings.bsl_server.enabled {
            return false;
        }

        // 2. Check JAR
        if !BSLClient::check_install(&settings.bsl_server.jar_path) {
            return false;
        }

        // 3. Check Java
        let java_ver = BSLClient::check_java(&settings.bsl_server.java_path);
        if java_ver == "Not found" {
            return false;
        }

        true 
    }
}

impl Drop for BSLClient {
    fn drop(&mut self) {
        self.stop();
    }
}
