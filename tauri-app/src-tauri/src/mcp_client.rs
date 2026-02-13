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
}

impl McpManager {
    fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn get_client(config: McpServerConfig) -> Result<Arc<McpSession>, String> {
        let mut sessions = MCP_MANAGER.sessions.lock().await;

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

    pub async fn reconfigure(new_settings: AppSettings) {
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
    }

    pub async fn get_statuses() -> Vec<McpServerStatus> {
        let sessions = MCP_MANAGER.sessions.lock().await;
        let mut statuses = Vec::new();

        for (id, (config, session)) in sessions.iter() {
            let is_alive = session.is_alive().await;
            statuses.push(McpServerStatus {
                id: id.clone(),
                name: config.name.clone(),
                status: if is_alive { "connected".to_string() } else { "disconnected".to_string() },
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

pub fn start_settings_watcher() {
    thread::spawn(|| {
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
                        tauri::async_runtime::spawn(async {
                            let settings = crate::settings::load_settings();
                            McpManager::reconfigure(settings).await;
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
        match tokio::time::timeout(Duration::from_secs(10), self.session.list_tools()).await {
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

    async fn new_stdio(config: McpServerConfig, debug_all: bool) -> Result<Self, String> {
        let command = config.command.ok_or("Command is missing")?;
        let args = config.args.unwrap_or_default();

        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&command);
            c
        } else {
            Command::new(&command)
        };

        if let Some(env) = &config.env {
            cmd.envs(env);
        }

        // Pass global debug flag
        if debug_all {
            cmd.env("ONEC_AI_DEBUG", "true");
        }

        let mut child = cmd
            .args(args)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
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
                             println!("[MCP][STDERR] {}", line); // Print to stdout for debugging
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
        }
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
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
        }
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>, String> {
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

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, String> {
        self.request("tools/call", json!({
            "name": name,
            "arguments": arguments
        })).await
    }
}
