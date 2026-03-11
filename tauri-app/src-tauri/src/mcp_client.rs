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
    #[serde(rename = "jsonrpc")]
    _jsonrpc: String,
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
    // 1С:Справка — прогресс индексации
    pub index_progress: u32,       // 0-100 (%)
    pub index_message: String,     // Текущее сообщение прогресса
    pub help_status: String,       // "unavailable" | "indexing" | "ready" | ""
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

        if let Some((stored_config, session)) = sessions.get(&config.id) {
            // For Stdio and Internal, check if config matches or session is alive
            // For HTTP, we also reuse the session if URL is the same
            if session.is_alive().await && stored_config == &config {
                return Ok(session.clone());
            }
        }

        // Create new session
        let session = match config.transport {
            McpTransport::Internal => {
                let handlers = MCP_MANAGER.internal_handlers.lock().await;
                if let Some(handler) = handlers.get(&config.id) {
                    Arc::new(McpSession::new_internal(config.clone(), handler.clone()))
                } else {
                    return Err(format!("Internal handler not found for {}", config.id));
                }
            },
            McpTransport::Http => Arc::new(McpSession::new_http(config.clone())),
            McpTransport::Stdio => {
                let settings = crate::settings::load_settings();
                crate::logger::set_debug_mode(settings.debug_mode);
                Arc::new(McpSession::new_stdio(config.clone(), settings.debug_mode).await?)
            }
        };

        sessions.insert(config.id.clone(), (config, session.clone()));
        Ok(session)
    }

    pub async fn reconfigure(new_settings: AppSettings, app_handle: &tauri::AppHandle) {
        crate::logger::set_debug_mode(new_settings.debug_mode);
        crate::ai::clear_mcp_cache();
        crate::app_log!("Reconfiguring MCP servers...");
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
                crate::app_log!("Restarting/Starting MCP server: {}", config.name);
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

                match McpSession::new_stdio(config.clone(), new_settings.debug_mode).await {
                    Ok(session) => {
                        let session = Arc::new(session);
                        crate::app_log!("Started MCP server: {}", config.id);
                        sessions.insert(config.id.clone(), (config, session));
                    }
                    Err(e) => {
                        crate::app_log!(force: true, "Failed to start MCP server {}: {}", config.name, e);
                    }
                }
            }
        }

        // 3. Handle BSL Server (Virtual)
        // Optimization: only lock and restart if enabled status changed or not connected
        if new_settings.bsl_server.enabled {
             let bsl_client_state = app_handle.state::<Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>();
             // Try lock with timeout to avoid hanging if BSL is currently busy analyzing large file
             let bsl_client = bsl_client_state.inner();
             let bsl_lock_future = bsl_client.lock();
             if let Ok(mut bsl) = tokio::time::timeout(Duration::from_millis(100), bsl_lock_future).await {
                 let jar_exists = std::path::Path::new(&new_settings.bsl_server.jar_path).exists();
                 if jar_exists && !bsl.is_connected() {
                     crate::app_log!("[MCP] Restarting/Starting BSL LS because it was enabled and not connected");
                     let _ = bsl.start_server();
                     let _ = bsl.connect().await;
                 }
             };
        } else {
             // If disabled, we still need to stop it
             let bsl_client_state = app_handle.state::<Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>();
             let bsl_client = bsl_client_state.inner();
             if let Ok(mut bsl) = tokio::time::timeout(Duration::from_millis(500), bsl_client.lock()).await {
                 bsl.stop();
             };
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

             crate::app_log!("[DEBUG] MCP Server status for {}: {}", config.id, status);
             
             // Извлекаем прогресс индексации для 1С:Справка и 1С:Поиск
             let (index_progress, index_message, help_status_str) = if config.id == "builtin-1c-help" || config.id == "builtin-1c-search" {
                 if let Some((_, session)) = sessions.get(&config.id) {
                     let progress = *session.help_progress.lock().await;
                     let message = session.help_message.lock().await.clone();
                     let hs = session.help_status.lock().await.clone();
                     (progress, message, hs)
                 } else {
                     (0, String::new(), String::new())
                 }
             } else {
                 (0, String::new(), String::new())
             };
             
             statuses.push(McpServerStatus {
                id: config.id.clone(),
                name: config.name.clone(),
                status: status.to_string(),
                transport: format!("{:?}", config.transport).to_lowercase(),
                index_progress,
                index_message,
                help_status: help_status_str,
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
             crate::app_log!(force: true, "Failed to watch settings dir: {}", e);
             return;
        }

        crate::app_log!("Started watching settings at {:?}", config_dir);

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
                Err(e) => crate::app_log!(force: true, "Watch error: {:?}", e),
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
        // builtin-1c-search processes requests sequentially; a heavy find_references
        // may block the queue for tens of seconds, so match the call_tool timeout
        let timeout_secs = if self.session.config.id == "builtin-1c-search" { 120 } else { 60 };
        match tokio::time::timeout(Duration::from_secs(timeout_secs), self.session.list_tools()).await {
            Ok(res) => res,
            Err(_) => Err("Timeout listing tools".to_string()),
        }
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let timeout_secs = if self.session.config.id == "builtin-1c-search" { 120 } else { 30 };
        match tokio::time::timeout(Duration::from_secs(timeout_secs), self.session.call_tool(name, arguments)).await {
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
        extra_headers: std::collections::HashMap<String, String>,
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
    pub config: McpServerConfig,
    transport: TransportImpl,
    next_id: std::sync::atomic::AtomicU64,
    logs: Arc<Mutex<VecDeque<String>>>,
    // Для 1С:Справка — статус индексации из stderr
    pub help_status: Arc<tokio::sync::Mutex<String>>,
    pub help_progress: Arc<tokio::sync::Mutex<u32>>,
    pub help_message: Arc<tokio::sync::Mutex<String>>,
}

impl McpSession {
    fn new_http(config: McpServerConfig) -> Self {
        let extra_headers = config.headers.clone().unwrap_or_default();
        Self {
            config: config.clone(),
            transport: TransportImpl::Http {
                client: Client::builder()
                    .timeout(Duration::from_secs(30))
                    .build()
                    .unwrap_or_default(),
                url: config.url.unwrap_or_default(),
                login: config.login,
                password: config.password,
                extra_headers,
            },
            next_id: std::sync::atomic::AtomicU64::new(1),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            help_status: Arc::new(tokio::sync::Mutex::new(String::new())),
            help_progress: Arc::new(tokio::sync::Mutex::new(0)),
            help_message: Arc::new(tokio::sync::Mutex::new(String::new())),
        }
    }

    fn new_internal(config: McpServerConfig, handler: Arc<dyn InternalMcpHandler>) -> Self {
        Self {
            config,
            transport: TransportImpl::Internal {
                handler,
            },
            next_id: std::sync::atomic::AtomicU64::new(1),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            help_status: Arc::new(tokio::sync::Mutex::new(String::new())),
            help_progress: Arc::new(tokio::sync::Mutex::new(0)),
            help_message: Arc::new(tokio::sync::Mutex::new(String::new())),
        }
    }

    async fn new_stdio(config: McpServerConfig, debug_all: bool) -> Result<Self, String> {
        let server_id_for_logs = config.id.clone();
        let mut command = config.command.clone().ok_or("Command is missing")?;
        let mut args = config.args.clone().unwrap_or_default();

        // Path resolution for production (Tauri Resources & Embedded)
        let app_handle_opt = MCP_MANAGER.app_handle.lock().await;

        if let Some(app_handle) = app_handle_opt.as_ref() {
            let cmd_lower = command.to_lowercase();
            let is_stdio_node_launcher = cmd_lower == "npx" || cmd_lower == "npx.cmd" || cmd_lower == "node" || cmd_lower.contains("tsx");
            
            if is_stdio_node_launcher {
                crate::app_log!("[MCP] Resolving resources for command '{}' with args {:?}", command, args);

                // Embedded resources for "True Portability"
                let embedded_servers = [
                    ("1c-help.cjs", include_bytes!("../mcp-servers/1c-help.cjs") as &[u8]),
                    ("1c-metadata.cjs", include_bytes!("../mcp-servers/1c-metadata.cjs") as &[u8]),
                    ("1c-naparnik.cjs", include_bytes!("../mcp-servers/1c-naparnik.cjs") as &[u8]),
                ];

                for arg in args.iter_mut() {
                    if arg.contains("mcp-servers") && (arg.ends_with(".ts") || arg.ends_with(".js") || arg.ends_with(".cjs")) {
                        let filename = std::path::Path::new(&*arg)
                            .file_name()
                            .and_then(|f| f.to_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| arg.to_string());
                        
                        let js_filename = filename.replace(".ts", ".cjs").replace(".js", ".cjs");
                        let mut resolved = false;

                        // Phase 1: Try to find embedded resource and extract it to AppData
                        if let Some((_, bytes)) = embedded_servers.iter().find(|(name, _)| *name == js_filename) {
                            let mcp_dir = crate::settings::get_settings_dir().join("mcp-servers");
                            let _ = std::fs::create_dir_all(&mcp_dir);
                            let target_path = mcp_dir.join(&js_filename);
                            
                            // Only write if not exists or different size (basic cache)
                            let current_size = std::fs::metadata(&target_path).map(|m| m.len()).unwrap_or(0);
                            if current_size != bytes.len() as u64 {
                                crate::app_log!("[MCP] Extracting embedded server to: {:?}", target_path);
                                if let Err(e) = std::fs::write(&target_path, bytes) {
                                    crate::app_log!("[ERROR] Failed to extract embedded MCP: {}", e);
                                }
                            }

                            if target_path.exists() {
                                let path_str = target_path.to_string_lossy().to_string();
                                *arg = path_str.strip_prefix(r"\\?\").unwrap_or(&path_str).to_string();
                                crate::app_log!("[MCP] Using embedded/extracted resource: {}", arg);
                                resolved = true;
                            }
                        }

                        // Phase 2: Fallback to standard Tauri resource resolution (MSI case)
                        if !resolved {
                            let js_subpath = format!("mcp-servers/{}", js_filename);
                            if let Ok(path) = app_handle.path().resolve(&js_subpath, tauri::path::BaseDirectory::Resource) {
                                if path.exists() {
                                    let path_str = path.to_string_lossy().to_string();
                                    *arg = path_str.strip_prefix(r"\\?\").unwrap_or(&path_str).to_string();
                                    crate::app_log!("[MCP] Resolved to MSI resource: {}", arg);
                                    resolved = true;
                                }
                            }
                        }
                        
                        // Phase 3: Last resort - check next to EXE
                        if !resolved {
                             if let Ok(exe_path) = std::env::current_exe() {
                                 if let Some(exe_dir) = exe_path.parent() {
                                     let local_path = exe_dir.join("mcp-servers").join(&js_filename);
                                     if local_path.exists() {
                                         let path_str = local_path.to_string_lossy().to_string();
                                         *arg = path_str.strip_prefix(r"\\?\").unwrap_or(&path_str).to_string();
                                         crate::app_log!("[MCP] Resolved to EXE-relative resource: {}", arg);
                                         resolved = true;
                                     }
                                 }
                             }
                        }

                        if !resolved {
                             crate::app_log!("[WARN] Could not resolve MCP resource '{}' via any method", js_filename);
                        }
                    }
                }
            }

            // .exe binary resolution
            let is_stdio_exe = cmd_lower.ends_with(".exe") && !is_stdio_node_launcher;
            if is_stdio_exe {
                let exe_filename = command.clone();
                let exe_subpath = format!("mcp-servers/{}", exe_filename);
                let mut exe_resolved = false;

                // Phase 1: Embedded binary (True Portability — same approach as .cjs servers)
                let embedded_exe_servers: &[(&str, &[u8])] = &[
                    ("mcp-1c-search.exe", include_bytes!("../mcp-servers/mcp-1c-search.exe")),
                ];
                if let Some((_, bytes)) = embedded_exe_servers.iter().find(|(name, _)| *name == exe_filename) {
                    let mcp_dir = crate::settings::get_settings_dir().join("mcp-servers");
                    let _ = std::fs::create_dir_all(&mcp_dir);
                    let target_path = mcp_dir.join(&exe_filename);
                    let current_size = std::fs::metadata(&target_path).map(|m| m.len()).unwrap_or(0);
                    if current_size != bytes.len() as u64 {
                        crate::app_log!("[MCP] Extracting embedded .exe to: {:?}", target_path);
                        if let Err(e) = std::fs::write(&target_path, bytes) {
                            crate::app_log!("[ERROR] Failed to extract embedded .exe: {}", e);
                        }
                    }
                    if target_path.exists() {
                        let path_str = target_path.to_string_lossy().to_string();
                        command = path_str.strip_prefix(r"\\?\").unwrap_or(&path_str).to_string();
                        crate::app_log!("[MCP] Using embedded/extracted .exe: {}", command);
                        exe_resolved = true;
                    }
                }

                // Phase 2: Tauri resource (MSI/NSIS bundle)
                if !exe_resolved {
                    if let Ok(path) = app_handle.path().resolve(&exe_subpath, tauri::path::BaseDirectory::Resource) {
                        if path.exists() {
                            let path_str = path.to_string_lossy().to_string();
                            command = path_str.strip_prefix(r"\\?\").unwrap_or(&path_str).to_string();
                            crate::app_log!("[MCP] Resolved .exe to resource: {}", command);
                            exe_resolved = true;
                        }
                    }
                }

                // Phase 3: Next to main EXE
                if !exe_resolved {
                    if let Ok(current_exe) = std::env::current_exe() {
                        if let Some(exe_dir) = current_exe.parent() {
                            let local = exe_dir.join("mcp-servers").join(&exe_filename);
                            if local.exists() {
                                let path_str = local.to_string_lossy().to_string();
                                command = path_str.strip_prefix(r"\\?\").unwrap_or(&path_str).to_string();
                                crate::app_log!("[MCP] Resolved .exe EXE-relative: {}", command);
                                exe_resolved = true;
                            }
                        }
                    }
                }

                // Phase 4: Dev mode fallback (src-tauri/mcp-servers)
                if !exe_resolved {
                    let dev_path = std::path::PathBuf::from("src-tauri/mcp-servers").join(&exe_filename);
                    if dev_path.exists() {
                        if let Ok(abs) = std::fs::canonicalize(&dev_path) {
                            let path_str = abs.to_string_lossy().to_string();
                            command = path_str.strip_prefix(r"\\?\").unwrap_or(&path_str).to_string();
                            crate::app_log!("[MCP] Resolved .exe Dev-relative: {}", command);
                            exe_resolved = true;
                        }
                    } else {
                        // try just mcp-servers (if cwd is already src-tauri)
                        let dev_path2 = std::path::PathBuf::from("mcp-servers").join(&exe_filename);
                        if dev_path2.exists() {
                             if let Ok(abs) = std::fs::canonicalize(&dev_path2) {
                                let path_str = abs.to_string_lossy().to_string();
                                command = path_str.strip_prefix(r"\\?\").unwrap_or(&path_str).to_string();
                                crate::app_log!("[MCP] Resolved .exe Dev-relative: {}", command);
                                exe_resolved = true;
                            }
                        }
                    }
                }

                if !exe_resolved {
                    crate::app_log!("[WARN] Could not resolve .exe '{}' — ensure mcp-1c-search is built", exe_filename);
                }
            }
        }

        #[allow(unused_mut)]
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
                 let has_ts_or_js = args.iter().any(|a| a.ends_with(".ts") || a.ends_with(".js") || a.ends_with(".cjs"));
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
                         if arg.ends_with(".ts") || arg.ends_with(".js") {
                             new_args.push(arg.replace(".ts", ".cjs").replace(".js", ".cjs"));
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

        // Assign child to Windows Job Object so it's killed when Mini AI 1C exits
        // (even on crash). JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE does this at kernel level.
        if let Some(pid) = child.id() {
            crate::job_guard::assign_to_job(pid);
        }

        let mut stdin = child.stdin.take().ok_or("Failed to open stdin")?;
        let stdout = child.stdout.take().ok_or("Failed to open stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to open stderr")?;

        let (tx, mut rx) = mpsc::channel::<JsonRpcRequest>(32);
        let pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>> = 
            Arc::new(Mutex::new(HashMap::new()));

        let logs = Arc::new(Mutex::new(VecDeque::with_capacity(100)));
        let logs_writer = logs.clone();
        let help_status = Arc::new(tokio::sync::Mutex::new(String::new()));
        let help_progress = Arc::new(tokio::sync::Mutex::new(0u32));
        let help_message = Arc::new(tokio::sync::Mutex::new(String::new()));
        let help_status_writer = help_status.clone();
        let help_progress_writer = help_progress.clone();
        let help_message_writer = help_message.clone();
        let is_help_server = config.id == "builtin-1c-help";
        let is_search_server = config.id == "builtin-1c-search";

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
                                 crate::app_log!("[MCP][{}] STDOUT RAW: {}", server_id_for_logs, line);
                                 let trimmed = line.trim();
                                 if !trimmed.starts_with('{') {
                                     continue; 
                                 }
                                 
                                 match serde_json::from_str::<JsonRpcResponse>(trimmed) {
                                     Ok(response) => {
                                         if let Some(id) = response.id {
                                             crate::app_log!("[MCP][{}] Parsed response for id: {}", server_id_for_logs, id);
                                             let mut pending = pending_requests_reader.lock().await;
                                             if let Some(sender) = pending.remove(&id) {
                                                 let result = if let Some(err) = response.error {
                                                     Err(format!("MCP Error {}: {}", err.code, err.message))
                                                 } else {
                                                     Ok(response.result.unwrap_or(Value::Null))
                                                 };
                                                 let _ = sender.send(result);
                                             }
                                         } else {
                                              crate::app_log!("[MCP][{}] Received notification or response without ID: {}", server_id_for_logs, trimmed);
                                         }
                                     },
                                     Err(e) => {
                                         crate::app_log!("[MCP][{}] Failed to parse JSON-RPC: {}. Line: {}", server_id_for_logs, e, trimmed);
                                     }
                                 }
                             }
                             _ => {
                                 crate::app_log!("[MCP][{}] STDOUT EOF or Error", server_id_for_logs);
                                 break;
                             }
                         }
                      }
                     stderr_res = stderr_reader.next_line() => {
                         // Consume stderr to prevent buffer fill
                         if let Ok(Some(line)) = stderr_res {
                             crate::app_log!("[MCP][{}][STDERR] {}", server_id_for_logs, line);
                             // Парсим HELP_STATUS строки от 1С:Справка сервера
                             if is_help_server && line.starts_with("HELP_STATUS:") {
                                 let parts: Vec<&str> = line.trim_start_matches("HELP_STATUS:").splitn(4, ':').collect();
                                 if !parts.is_empty() {
                                     let state = parts[0];
                                     *help_status_writer.lock().await = state.to_string();
                                     match state {
                                         "indexing" => {
                                             if parts.len() >= 3 {
                                                 let progress: u32 = parts[1].parse().unwrap_or(0);
                                                 *help_progress_writer.lock().await = progress;
                                                 let msg = parts.get(3).unwrap_or(&"").to_string();
                                                 *help_message_writer.lock().await = msg;
                                             }
                                         }
                                         "ready" => {
                                             *help_progress_writer.lock().await = 100;
                                             let version = parts.get(1).unwrap_or(&"");
                                             let count = parts.get(2).unwrap_or(&"0");
                                             *help_message_writer.lock().await =
                                                 format!("Готово: {} тем (платформа {})", count, version);
                                         }
                                         "unavailable" => {
                                             *help_progress_writer.lock().await = 0;
                                             let reason = parts.get(1).unwrap_or(&"Платформа 1С не найдена");
                                             *help_message_writer.lock().await = reason.to_string();
                                         }
                                         _ => {}
                                     }
                                 }
                             }
                             // Парсим SEARCH_STATUS строки от 1С:Поиск (mcp-1c-search)
                             // Format: SEARCH_STATUS:{state}:{sym_count}:{db_size_mb}:{built_at_unix}
                             if is_search_server && line.starts_with("SEARCH_STATUS:") {
                                 let parts: Vec<&str> = line.trim_start_matches("SEARCH_STATUS:").splitn(5, ':').collect();
                                 if !parts.is_empty() {
                                     let state = parts[0];
                                     *help_status_writer.lock().await = state.to_string();
                                     match state {
                                         "ready" => {
                                             *help_progress_writer.lock().await = 100;
                                             let sym_count = parts.get(1).unwrap_or(&"").trim();
                                             let db_size   = parts.get(2).unwrap_or(&"").trim();
                                             let built_at_unix: u64 = parts.get(3).unwrap_or(&"0").trim().parse().unwrap_or(0);

                                             // Format timestamp as ДД.ММ.ГГГГ ЧЧ:ММ (UTC+3 Moscow)
                                             let date_str = if built_at_unix > 0 {
                                                 let msk = built_at_unix as i64 + 3 * 3600;
                                                 let days = msk / 86400;
                                                 let h = (msk % 86400) / 3600;
                                                 let m = (msk % 3600) / 60;
                                                 let z = days + 719468;
                                                 let era = if z >= 0 { z } else { z - 146096 } / 146097;
                                                 let doe = z - era * 146097;
                                                 let yoe = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
                                                 let y = yoe + era * 400;
                                                 let doy = doe - (365*yoe + yoe/4 - yoe/100);
                                                 let mp = (5*doy + 2) / 153;
                                                 let d = doy - (153*mp + 2)/5 + 1;
                                                 let mo = if mp < 10 { mp + 3 } else { mp - 9 };
                                                 let y = if mo <= 2 { y + 1 } else { y };
                                                 format!("{:02}.{:02}.{} {:02}:{:02}", d, mo, y, h, m)
                                             } else {
                                                 String::new()
                                             };

                                             *help_message_writer.lock().await = match (sym_count, db_size, date_str.as_str()) {
                                                 ("", _, _) | ("0", _, _) => "Готово".to_string(),
                                                 (c, s, "") if s.is_empty() || s == "0.00" => format!("{} символов", c),
                                                 (c, s, dt) if dt.is_empty() => format!("{} символов • {} МБ", c, s),
                                                 (c, s, dt) if s.is_empty() || s == "0.00" => format!("{} символов • {}", c, dt),
                                                 (c, s, dt) => format!("{} символов • {} МБ • {}", c, s, dt),
                                             };
                                         }
                                         "unavailable" => {
                                             *help_progress_writer.lock().await = 0;
                                             let reason = parts.get(1).unwrap_or(&"Путь не задан");
                                             *help_message_writer.lock().await = reason.to_string();
                                         }
                                         "indexing" | "syncing" => {
                                             if let Some(pct_str) = parts.get(1) {
                                                 if let Ok(pct) = pct_str.parse::<u32>() {
                                                     *help_progress_writer.lock().await = pct;
                                                 }
                                             }
                                             if let Some(msg) = parts.get(2) {
                                                 *help_message_writer.lock().await = msg.to_string();
                                             }
                                         }
                                         _ => {}
                                     }
                                 }
                             }
                             let mut logs = logs_writer.lock().await;
                             if logs.len() >= 100 {
                                 logs.pop_front();
                             }
                             logs.push_back(line);
                         } else {
                             // EOF on stderr
                         }
                     }
                }
            }
        });

        Ok(Self {
            config,
            transport: TransportImpl::Stdio {
                tx,
                pending_requests,
                _child: Arc::new(Mutex::new(child)),
            },
            next_id: std::sync::atomic::AtomicU64::new(1),
            logs,
            help_status,
            help_progress,
            help_message,
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
            TransportImpl::Http { client, url, login, password, extra_headers } => {
                let mut rb = client.post(url)
                    .header("Accept", "application/json, text/event-stream")
                    .header("Content-Type", "application/json")
                    .json(&req);
                if let Some(l) = login {
                    if !l.is_empty() {
                       rb = rb.basic_auth(l, password.as_deref());
                    }
                }
                for (k, v) in extra_headers {
                    rb = rb.header(k.as_str(), v.as_str());
                }
                let resp = rb.send().await.map_err(|e| e.to_string())?;
                let content_type = resp.headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();

                let rpc_res: JsonRpcResponse = if content_type.contains("text/event-stream") {
                    // Parse SSE: find first "data: {...}" line with a result or error
                    let body = resp.text().await.map_err(|e| e.to_string())?;
                    let mut found: Option<JsonRpcResponse> = None;
                    for line in body.lines() {
                        let line = line.trim();
                        if let Some(data) = line.strip_prefix("data:") {
                            let data = data.trim();
                            if data.is_empty() || data == "[DONE]" {
                                continue;
                            }
                            if let Ok(parsed) = serde_json::from_str::<JsonRpcResponse>(data) {
                                if parsed.result.is_some() || parsed.error.is_some() {
                                    found = Some(parsed);
                                    break;
                                }
                            }
                        }
                    }
                    found.ok_or_else(|| "No JSON-RPC response found in SSE stream".to_string())?
                } else {
                    resp.json().await.map_err(|e| e.to_string())?
                };

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

                crate::app_log!("[MCP][{}] >>> Sending: {}", self.config.id, serde_json::to_string(&req).unwrap_or_default());
                tx.send(req).await.map_err(|_| "Failed to send request to MCP process".to_string())?;

                // builtin-1c-search may do ripgrep over large 5GB+ configs → need longer timeout
                let timeout_secs = if self.config.id == "builtin-1c-search" { 120 } else { 30 };
                match tokio::time::timeout(Duration::from_secs(timeout_secs), auth_rx).await {
                    Ok(Ok(result)) => {
                        crate::app_log!("[MCP][{}] <<< Received result for id {}", self.config.id, id);
                        result
                    },
                    Ok(Err(_)) => {
                        crate::app_log!("[MCP][{}][ERROR] Response channel closed for id {}", self.config.id, id);
                        Err("Channel closed".to_string())
                    },
                    Err(_) => {
                        let mut pending = pending_requests.lock().await;
                        pending.remove(&id);
                        crate::app_log!("[MCP][{}][ERROR] Request timed out for id {}", self.config.id, id);
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

