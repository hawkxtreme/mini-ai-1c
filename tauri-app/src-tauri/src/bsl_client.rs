//! BSL Language Server client
//! Communicates with BSL LS via WebSocket using JSON-RPC

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::settings::load_settings;

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
}

impl BSLClient {
    pub fn new() -> Self {
        Self {
            ws: None,
            server_process: None,
            request_id: AtomicI32::new(1),
            capabilities: None,
        }
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
        
        let port = settings.bsl_server.websocket_port;
        
        let mut cmd = Command::new(&settings.bsl_server.java_path);
        cmd.args([
                "-jar",
                jar_path,
                "websocket",
                &format!("--server.port={}", port),
            ])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }

        let child = cmd.spawn()
            .map_err(|e| format!("Failed to start BSL LS: {}", e))?;
        
        self.server_process = Some(child);
        Ok(())
    }

    /// Connect to the BSL Language Server
    pub async fn connect(&mut self) -> Result<(), String> {
        let settings = load_settings();
        let port = settings.bsl_server.websocket_port;
        let url = format!("ws://127.0.0.1:{}/lsp", port);
        
        let mut retries = 0;
        let max_retries = 20; // 10 seconds total
        
        loop {
            match connect_async(&url).await {
                Ok((ws_stream, _)) => {
                    self.ws = Some(Mutex::new(ws_stream));
                    break;
                }
                Err(e) => {
                    retries += 1;
                    if retries >= max_retries {
                         return Err(format!("Failed to connect to BSL LS after {} attempts: {}", max_retries, e));
                    }
                    println!("BSL LS connection attempt {}/{} failed, retrying in 500ms...", retries, max_retries);
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
        
        // Initialize LSP with proper capabilities
        let client_capabilities = serde_json::json!({
            "textDocument": {
                "synchronization": {
                    "dynamicRegistration": true,
                    "willSave": true,
                    "willSaveWaitUntil": false,
                    "didSave": true
                },
                "diagnostic": {
                    "dynamicRegistration": true
                },
                "formatting": {
                    "dynamicRegistration": true
                },
                "publishDiagnostics": {
                    "relatedInformation": true,
                    "tagSupport": {
                        "valueSet": [1, 2]
                    },
                    "versionSupport": true
                }
            },
            "workspace": {
                "configuration": true,
                "didChangeConfiguration": {
                    "dynamicRegistration": true
                }
            }
        });

        let initialize_result = self.send_request("initialize", serde_json::json!({
            "processId": std::process::id(),
            "rootUri": null,
            "capabilities": client_capabilities,
            "trace": "verbose"
        })).await?;
        
        // Store server capabilities
        self.capabilities = initialize_result.get("capabilities").cloned();
        println!("[BSL LS] Initialized. Server capabilities: {:?}", self.capabilities.as_ref().map(|c| c.to_string()));

        // Notify initialized
        self.send_notification("initialized", serde_json::json!({})).await?;
        
        Ok(())
    }

    /// Send JSON-RPC request
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
        ws.send(Message::Text(msg))
            .await
            .map_err(|e| e.to_string())?;
        
        // Wait for response
        while let Some(msg) = ws.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&text) {
                        if response.id == Some(id) {
                            if let Some(error) = response.error {
                                return Err(format!("LSP error {}: {}", error.code, error.message));
                            }
                            return Ok(response.result.unwrap_or(serde_json::Value::Null));
                        }
                    }
                }
                Err(e) => return Err(e.to_string()),
                _ => {}
            }
        }
        
        Err("No response from BSL LS".to_string())
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
                            // println!("[BSL LS] Received: {}", text); // excessive?
                            if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&text) {
                                // Check if it is publishDiagnostics
                                if let Some(method) = &response.method {
                                    if method == "textDocument/publishDiagnostics" {
                                        println!("[BSL LS] Received publishDiagnostics");
                                        if let Some(params) = response.params {
                                            // Ensure it's for our URI
                                            if let Some(diag_uri) = params.get("uri").and_then(|u| u.as_str()) {
                                                println!("[BSL LS] Diagnostics URI: {}, Expected: {}", diag_uri, uri);
                                                
                                                // Normalize check: BSL LS might add drive letter (e.g. file:///D:/temp.bsl)
                                                // We check if diag_uri ends with the filename we sent
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
                                                    // (We need to unlock ws to call send_notification, so we can't use self.send_notification here easily because we hold the lock)
                                                    // We'll just construct the close message manually to reuse the stream
                                                    
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
                                                         println!("[BSL LS] Sent didClose (manual)");
                                                    }

                                                    return Ok(diagnostics);
                                                } else {
                                                    println!("[BSL LS] URI mismatch, ignoring");
                                                }
                                            }
                                        }
                                    } else {
                                         println!("[BSL LS] Received notification/request method: {}", method);
                                    }
                                } else if let Some(id) = response.id {
                                     println!("[BSL LS] Received response for id: {:?}, result: {:?}", id, response.result);
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
            let _ = child.kill();
        }
    }

    /// Check if Java is installed and retrieve version
    pub fn check_java(java_path: &str) -> String {
        let mut cmd = Command::new(java_path);
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

impl Drop for BSLClient {
    fn drop(&mut self) {
        self.stop();
    }
}
