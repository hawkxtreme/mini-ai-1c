use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::settings::{AppSettings, McpServerConfig, McpTransport};
use async_trait::async_trait;
use lazy_static::lazy_static;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

static RECONFIGURE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Format a Unix timestamp as "ДД.ММ.ГГГГ ЧЧ:ММ" in UTC+3 (Moscow time).
fn format_unix_msk(unix: u64) -> String {
    if unix == 0 { return String::new(); }
    let msk = unix as i64 + 3 * 3600;
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
}

pub(crate) const BUILTIN_1C_SEARCH_SERVER_ID: &str = "builtin-1c-search";

pub(crate) fn builtin_search_unavailable_reason(config: &McpServerConfig) -> Option<String> {
    if config.id != BUILTIN_1C_SEARCH_SERVER_ID {
        return None;
    }

    let config_path = config
        .env
        .as_ref()
        .and_then(|env| env.get("ONEC_CONFIG_PATH"))
        .map(|value| value.trim())
        .unwrap_or("");

    if config_path.is_empty() {
        return Some("Путь к выгрузке конфигурации 1С не задан".to_string());
    }

    let path = std::path::Path::new(config_path);
    if !path.exists() {
        return Some(format!(
            "Путь к выгрузке конфигурации 1С не найден: {}",
            config_path
        ));
    }

    if !path.is_dir() {
        return Some(format!(
            "Путь к выгрузке конфигурации 1С должен указывать на директорию: {}",
            config_path
        ));
    }

    None
}

#[async_trait]
pub trait InternalMcpHandler: Send + Sync {
    async fn list_tools(&self) -> Vec<McpTool>;
    async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, String>;
    fn is_alive(&self) -> bool {
        true
    }
}

#[derive(Clone, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
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

struct HttpRpcResponse {
    status: reqwest::StatusCode,
    body: String,
    rpc_response: Option<JsonRpcResponse>,
    session_id: Option<String>,
    final_url: String,
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
    pub index_progress: u32,   // 0-100 (%)
    pub index_message: String, // Текущее сообщение прогресса
    pub help_status: String,   // "unavailable" | "indexing" | "ready" | ""
    pub last_checked: i64,     // unix timestamp of last health check, 0 = never
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
            }
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

        let new_server_ids: HashSet<String> = new_settings
            .mcp_servers
            .iter()
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
                        let session =
                            Arc::new(McpSession::new_internal(config.clone(), handler.clone()));
                        sessions.insert(config.id.clone(), (config, session));
                    }
                    continue;
                }

                if config.transport == McpTransport::Http {
                    let session = Arc::new(McpSession::new_http(config.clone()));
                    crate::app_log!("Started HTTP MCP session: {}", config.id);
                    sessions.insert(config.id.clone(), (config, session));
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
            let bsl_client_state =
                app_handle.state::<Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>();
            // Try lock with timeout to avoid hanging if BSL is currently busy analyzing large file
            let bsl_client = bsl_client_state.inner();
            let bsl_lock_future = bsl_client.lock();
            if let Ok(mut bsl) =
                tokio::time::timeout(Duration::from_millis(3000), bsl_lock_future).await
            {
                let jar_exists = std::path::Path::new(&new_settings.bsl_server.jar_path).exists();
                if jar_exists && !bsl.is_connected() {
                    crate::app_log!(
                        "[MCP] Restarting/Starting BSL LS because it was enabled and not connected"
                    );
                    let _ = bsl.start_server();
                    let _ = bsl.connect().await;
                }
            };
        } else {
            // If disabled, we still need to stop it
            let bsl_client_state =
                app_handle.state::<Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>();
            let bsl_client = bsl_client_state.inner();
            if let Ok(mut bsl) =
                tokio::time::timeout(Duration::from_millis(500), bsl_client.lock()).await
            {
                bsl.stop();
            };
        }
    }

    pub async fn get_statuses() -> Vec<McpServerStatus> {
        let sessions = MCP_MANAGER.sessions.lock().await;
        let mut statuses = Vec::new();
        let mut probe_candidates: Vec<(Arc<McpSession>, Option<HttpHealthState>, i64)> = Vec::new();

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
            let (base_status, last_checked) = if !config.enabled {
                ("disabled".to_string(), 0)
            } else if let Some((_, session)) = sessions.get(&config.id) {
                let status = session.get_status_string().await;
                let last_checked = session.get_last_checked().await;
                if config.transport == McpTransport::Http {
                    let health = session.get_health().await;
                    probe_candidates.push((session.clone(), health, last_checked));
                }
                (status, last_checked)
            } else if config.transport == McpTransport::Internal {
                let handlers = MCP_MANAGER.internal_handlers.lock().await;
                if handlers.contains_key(&config.id) {
                    ("connected".to_string(), 0)
                } else {
                    ("stopped".to_string(), 0)
                }
            } else {
                ("stopped".to_string(), 0) // Enabled but not in sessions (failed to start or never started)
            };

            crate::app_log!(
                "[DEBUG] MCP Server status for {}: {}",
                config.id,
                base_status
            );

            // Извлекаем прогресс индексации для 1С:Справка и 1С:Поиск
            let (index_progress, index_message, help_status_str) =
                if config.id == "builtin-1c-help" || config.id == BUILTIN_1C_SEARCH_SERVER_ID {
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

            let (index_progress, index_message, help_status_str) =
                if config.id == BUILTIN_1C_SEARCH_SERVER_ID && help_status_str.is_empty() {
                    if let Some(reason) = builtin_search_unavailable_reason(&config) {
                        (0, reason, "unavailable".to_string())
                    } else {
                        (index_progress, index_message, help_status_str)
                    }
                } else {
                    (index_progress, index_message, help_status_str)
                };

            let status = if config.enabled
                && config.id == BUILTIN_1C_SEARCH_SERVER_ID
                && help_status_str == "unavailable"
            {
                "error".to_string()
            } else {
                base_status
            };

            statuses.push(McpServerStatus {
                id: config.id.clone(),
                name: config.name.clone(),
                status,
                transport: format!("{:?}", config.transport).to_lowercase(),
                index_progress,
                index_message,
                help_status: help_status_str,
                last_checked,
            });
        }

        drop(sessions);

        const PROBE_CONNECTED_SECS: i64 = 30;
        const PROBE_FAILED_SECS: i64 = 120;
        let now = now_unix();

        for (session, health, last_checked) in probe_candidates {
            let needs_probe = match health {
                Some(HttpHealthState::Connected) => now - last_checked >= PROBE_CONNECTED_SECS,
                Some(HttpHealthState::Offline) | Some(HttpHealthState::Error) => {
                    now - last_checked >= PROBE_FAILED_SECS
                }
                _ => false,
            };

            if !needs_probe || !session.try_begin_http_probe(now).await {
                continue;
            }

            tokio::spawn(async move {
                let reset_init = matches!(
                    session.get_health().await,
                    Some(HttpHealthState::Offline) | Some(HttpHealthState::Error)
                );
                if reset_init {
                    session.reset_http_state().await;
                }
                let _ = session.list_tools().await;
                session.finish_http_probe();
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
        let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
            Ok(w) => w,
            Err(e) => {
                crate::app_log!(force: true, "Failed to create file watcher: {}", e);
                return;
            }
        };

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
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|s| s == "settings.json")
                            .unwrap_or(false)
                    });

                    if interesting {
                        // Debounce: wait a bit to ensure write is complete.
                        thread::sleep(Duration::from_millis(100));

                        // Дедупликация: если reconfigure уже выполняется — пропускаем
                        if RECONFIGURE_IN_FLIGHT.swap(true, Ordering::SeqCst) {
                            continue;
                        }

                        // Run async reconfigure in tauri runtime
                        let app_handle_clone = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let settings = crate::settings::load_settings();
                            McpManager::reconfigure(settings, &app_handle_clone).await;
                            RECONFIGURE_IN_FLIGHT.store(false, Ordering::SeqCst);
                        });
                    }
                }
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
        let timeout_secs = if self.session.config.id == BUILTIN_1C_SEARCH_SERVER_ID {
            120
        } else {
            60
        };
        match tokio::time::timeout(Duration::from_secs(timeout_secs), self.session.list_tools())
            .await
        {
            Ok(res) => res,
            Err(_) => Err("Timeout listing tools".to_string()),
        }
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let timeout_secs = if self.session.config.id == BUILTIN_1C_SEARCH_SERVER_ID
            || self.session.config.id == "builtin-1c-naparnik"
        {
            120
        } else {
            30
        };
        match tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            self.session.call_tool(name, arguments),
        )
        .await
        {
            Ok(res) => res,
            Err(_) => Err(format!("Timeout executing tool '{}'", name)),
        }
    }

    pub async fn get_help_state(&self) -> (String, String) {
        (
            self.session.help_status.lock().await.clone(),
            self.session.help_message.lock().await.clone(),
        )
    }
}

enum TransportImpl {
    Http {
        client: Client,
        url: String,
        effective_url: Arc<tokio::sync::Mutex<Option<String>>>,
        login: Option<String>,
        password: Option<String>,
        extra_headers: std::collections::HashMap<String, String>,
        // None = not initialized yet, Some(None) = direct HTTP flow works without session,
        // Some(Some(id)) = initialized session
        http_state: Arc<tokio::sync::Mutex<Option<Option<String>>>>,
        health: Arc<tokio::sync::Mutex<HttpHealthState>>,
        last_checked: Arc<tokio::sync::Mutex<i64>>,
        probe_in_flight: Arc<AtomicBool>,
    },
    Stdio {
        tx: mpsc::Sender<JsonRpcRequest>,
        pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>,
        initialized: Arc<AtomicBool>,
        init_lock: Arc<tokio::sync::Mutex<()>>,
        // We keep the child here just to keep the process alive
        _child: Arc<Mutex<Child>>,
    },
    Internal {
        handler: Arc<dyn InternalMcpHandler>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpHealthState {
    Unknown,
    Connected,
    Offline,
    Error,
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
                effective_url: Arc::new(tokio::sync::Mutex::new(None)),
                login: config.login,
                password: config.password,
                extra_headers,
                http_state: Arc::new(tokio::sync::Mutex::new(None)),
                health: Arc::new(tokio::sync::Mutex::new(HttpHealthState::Unknown)),
                last_checked: Arc::new(tokio::sync::Mutex::new(0)),
                probe_in_flight: Arc::new(AtomicBool::new(false)),
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
            transport: TransportImpl::Internal { handler },
            next_id: std::sync::atomic::AtomicU64::new(1),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            help_status: Arc::new(tokio::sync::Mutex::new(String::new())),
            help_progress: Arc::new(tokio::sync::Mutex::new(0)),
            help_message: Arc::new(tokio::sync::Mutex::new(String::new())),
        }
    }

    fn normalize_spawn_path(path: &std::path::Path) -> String {
        let path_str = path.to_string_lossy().to_string();
        path_str
            .strip_prefix(r"\\?\")
            .unwrap_or(&path_str)
            .to_string()
    }

    #[cfg_attr(debug_assertions, allow(dead_code))]
    fn write_embedded_mcp_resource_to_dir(
        base_dir: &std::path::Path,
        filename: &str,
        bytes: &[u8],
    ) -> Result<std::path::PathBuf, String> {
        let mcp_dir = base_dir.join("mcp-servers");
        std::fs::create_dir_all(&mcp_dir).map_err(|e| {
            format!(
                "Failed to create portable MCP dir '{}': {}",
                mcp_dir.display(),
                e
            )
        })?;

        let target_path = mcp_dir.join(filename);
        let needs_write = match std::fs::read(&target_path) {
            Ok(existing) => existing != bytes,
            Err(_) => true,
        };

        if needs_write {
            std::fs::write(&target_path, bytes).map_err(|e| {
                format!(
                    "Failed to write embedded MCP resource '{}' to '{}': {}",
                    filename,
                    target_path.display(),
                    e
                )
            })?;
        }

        Ok(target_path)
    }

    #[cfg(not(debug_assertions))]
    fn embedded_mcp_resource_bytes(filename: &str) -> Option<&'static [u8]> {
        match filename {
            "1c-help.cjs" => Some(include_bytes!("../mcp-servers/1c-help.cjs")),
            "1c-metadata.cjs" => Some(include_bytes!("../mcp-servers/1c-metadata.cjs")),
            "1c-naparnik.cjs" => Some(include_bytes!("../mcp-servers/1c-naparnik.cjs")),
            "mcp-1c-search.exe" => Some(include_bytes!("../mcp-servers/mcp-1c-search.exe")),
            _ => None,
        }
    }

    #[cfg(not(debug_assertions))]
    fn ensure_portable_embedded_mcp(filename: &str) -> Option<String> {
        let bytes = Self::embedded_mcp_resource_bytes(filename)?;
        let exe_path = std::env::current_exe().ok()?;
        let exe_dir = exe_path.parent()?;

        match Self::write_embedded_mcp_resource_to_dir(exe_dir, filename, bytes) {
            Ok(path) => Some(Self::normalize_spawn_path(&path)),
            Err(error) => {
                crate::app_log!("[WARN] {}", error);
                None
            }
        }
    }

    fn trim_http_body(body: &str) -> String {
        const MAX_CHARS: usize = 240;

        let trimmed = body.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let shortened: String = trimmed.chars().take(MAX_CHARS).collect();
        if trimmed.chars().count() > MAX_CHARS {
            format!("{}...", shortened)
        } else {
            shortened
        }
    }

    fn normalize_http_url_for_compare(url: &str) -> String {
        url.trim().to_string()
    }

    fn should_update_effective_http_url(requested_url: &str, final_url: &str) -> bool {
        let requested = Self::normalize_http_url_for_compare(requested_url);
        let final_normalized = Self::normalize_http_url_for_compare(final_url);
        !final_url.trim().is_empty() && requested != final_normalized
    }

    fn should_retry_stdio_with_initialize(error: &str) -> bool {
        let lower = error.to_lowercase();
        lower.contains("session initialization")
            || lower.contains("call initialize first")
            || lower.contains("not initialized")
            || (lower.contains("initialize") && lower.contains("session"))
    }

    fn parse_http_rpc_response(content_type: &str, body: &str) -> Option<JsonRpcResponse> {
        if content_type.contains("text/event-stream") {
            for line in body.lines() {
                let line = line.trim();
                if let Some(data) = line.strip_prefix("data:") {
                    let data = data.trim();
                    if data.is_empty() || data == "[DONE]" {
                        continue;
                    }

                    if let Ok(parsed) = serde_json::from_str::<JsonRpcResponse>(data) {
                        if parsed.result.is_some() || parsed.error.is_some() {
                            return Some(parsed);
                        }
                    }
                }
            }

            None
        } else {
            serde_json::from_str::<JsonRpcResponse>(body).ok()
        }
    }

    async fn send_http_payload(
        client: &Client,
        url: &str,
        login: &Option<String>,
        password: &Option<String>,
        extra_headers: &HashMap<String, String>,
        payload: &Value,
        session_id: Option<&str>,
        expect_rpc_response: bool,
    ) -> Result<HttpRpcResponse, String> {
        let mut rb = client
            .post(url)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .json(payload);

        if let Some(l) = login {
            if !l.is_empty() {
                rb = rb.basic_auth(l, password.as_deref());
            }
        }

        for (k, v) in extra_headers {
            rb = rb.header(k.as_str(), v.as_str());
        }

        if let Some(session_id) = session_id {
            rb = rb.header("Mcp-Session-Id", session_id);
        }

        let response = rb.send().await.map_err(|e| e.to_string())?;
        let status = response.status();
        let final_url = response.url().to_string();
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = response.text().await.map_err(|e| e.to_string())?;
        let rpc_response = if expect_rpc_response {
            Self::parse_http_rpc_response(&content_type, &body)
        } else {
            None
        };

        Ok(HttpRpcResponse {
            status,
            body,
            rpc_response,
            session_id,
            final_url,
        })
    }

    fn describe_http_response(response: &HttpRpcResponse) -> String {
        if let Some(rpc_response) = &response.rpc_response {
            if let Some(err) = &rpc_response.error {
                return format!("MCP Error {}: {}", err.code, err.message);
            }
        }

        let body = Self::trim_http_body(&response.body);
        if body.is_empty() {
            format!("HTTP {}", response.status.as_u16())
        } else {
            format!("HTTP {}: {}", response.status.as_u16(), body)
        }
    }

    fn extract_http_result(response: &HttpRpcResponse) -> Result<Value, String> {
        if let Some(rpc_response) = &response.rpc_response {
            if let Some(err) = &rpc_response.error {
                Err(format!("MCP Error {}: {}", err.code, err.message))
            } else {
                Ok(rpc_response.result.clone().unwrap_or(Value::Null))
            }
        } else if response.status.is_success() {
            let body = Self::trim_http_body(&response.body);
            if body.is_empty() {
                Err("Failed to parse JSON-RPC response".to_string())
            } else {
                Err(format!("Failed to parse JSON-RPC response: {}", body))
            }
        } else {
            Err(Self::describe_http_response(response))
        }
    }

    fn should_retry_with_initialize(response: &HttpRpcResponse) -> bool {
        let rpc_error_text = response
            .rpc_response
            .as_ref()
            .and_then(|rpc| rpc.error.as_ref())
            .map(|err| err.message.to_lowercase())
            .unwrap_or_default();
        let body_text = response.body.to_lowercase();

        let rpc_requires_initialize = rpc_error_text.contains("initialize")
            || rpc_error_text.contains("initialized")
            || rpc_error_text.contains("mcp-session-id")
            || (rpc_error_text.contains("session") && rpc_error_text.contains("mcp"));

        if rpc_requires_initialize {
            return true;
        }

        let body_requires_initialize = body_text.contains("streamable http")
            || body_text.contains("mcp-session-id")
            || body_text.contains("initialize")
            || body_text.contains("initialized")
            || (body_text.contains("session") && body_text.contains("mcp"));

        body_requires_initialize
            && matches!(
                response.status.as_u16(),
                400 | 404 | 405 | 409 | 412 | 415 | 422 | 428
            )
    }

    async fn initialize_http_session(
        client: &Client,
        url: &str,
        login: &Option<String>,
        password: &Option<String>,
        extra_headers: &HashMap<String, String>,
    ) -> Result<(Option<String>, String), String> {
        let init_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "mini-ai-1c", "version": "1.0" }
            }
        });

        let init_response = Self::send_http_payload(
            client,
            url,
            login,
            password,
            extra_headers,
            &init_payload,
            None,
            true,
        )
        .await?;

        if !init_response.status.is_success() {
            return Err(format!(
                "HTTP MCP initialize failed: {}",
                Self::describe_http_response(&init_response)
            ));
        }

        if let Some(rpc_response) = &init_response.rpc_response {
            if let Some(err) = &rpc_response.error {
                return Err(format!(
                    "HTTP MCP initialize failed: MCP Error {}: {}",
                    err.code, err.message
                ));
            }
        }

        let session_id = init_response.session_id.clone();
        let effective_url = init_response.final_url.clone();
        let initialized_notification =
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}});

        match Self::send_http_payload(
            client,
            &effective_url,
            login,
            password,
            extra_headers,
            &initialized_notification,
            session_id.as_deref(),
            false,
        )
        .await
        {
            Ok(response) if !response.status.is_success() => {
                crate::app_log!(
                    "[MCP][HTTP] initialized notification returned HTTP {} for {}",
                    response.status.as_u16(),
                    effective_url
                );
            }
            Err(error) => {
                crate::app_log!(
                    "[MCP][HTTP] initialized notification failed for {}: {}",
                    effective_url,
                    error
                );
            }
            _ => {}
        }

        crate::app_log!(
            "[MCP][HTTP] Session initialized for {}{}",
            effective_url,
            session_id
                .as_ref()
                .map(|sid| format!(", id={}", sid))
                .unwrap_or_default()
        );

        Ok((session_id, effective_url))
    }

    async fn new_stdio(config: McpServerConfig, debug_all: bool) -> Result<Self, String> {
        let server_id_for_logs = config.id.clone();
        let mut command = config.command.clone().ok_or("Command is missing")?;
        let mut args = config.args.clone().unwrap_or_default();

        // Path resolution for production (Tauri Resources & Embedded)
        let app_handle_opt = MCP_MANAGER.app_handle.lock().await;

        if let Some(app_handle) = app_handle_opt.as_ref() {
            let cmd_lower = command.to_lowercase();
            let is_stdio_node_launcher = cmd_lower == "npx"
                || cmd_lower == "npx.cmd"
                || cmd_lower == "node"
                || cmd_lower.contains("tsx");

            if is_stdio_node_launcher {
                crate::app_log!(
                    "[MCP] Resolving resources for command '{}' with args {:?}",
                    command,
                    args
                );

                for arg in args.iter_mut() {
                    if arg.contains("mcp-servers")
                        && (arg.ends_with(".ts") || arg.ends_with(".js") || arg.ends_with(".cjs"))
                    {
                        let filename = std::path::Path::new(&*arg)
                            .file_name()
                            .and_then(|f| f.to_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| arg.to_string());

                        let js_filename = filename.replace(".ts", ".cjs").replace(".js", ".cjs");
                        let mut resolved = false;

                        // In bundled builds, prefer packaged resources to avoid self-extracting
                        // helper scripts into user-writable directories.
                        if !resolved {
                            let js_subpath = format!("mcp-servers/{}", js_filename);
                            if let Ok(path) = app_handle
                                .path()
                                .resolve(&js_subpath, tauri::path::BaseDirectory::Resource)
                            {
                                if path.exists() {
                                    let path_str = path.to_string_lossy().to_string();
                                    *arg = path_str
                                        .strip_prefix(r"\\?\")
                                        .unwrap_or(&path_str)
                                        .to_string();
                                    crate::app_log!("[MCP] Resolved to MSI resource: {}", arg);
                                    resolved = true;
                                }
                            }
                        }

                        // Next to the main executable (portable/dev launch).
                        if !resolved {
                            if let Ok(exe_path) = std::env::current_exe() {
                                if let Some(exe_dir) = exe_path.parent() {
                                    let local_path = exe_dir.join("mcp-servers").join(&js_filename);
                                    if local_path.exists() {
                                        *arg = Self::normalize_spawn_path(&local_path);
                                        crate::app_log!(
                                            "[MCP] Resolved to EXE-relative resource: {}",
                                            arg
                                        );
                                        resolved = true;
                                    }
                                }
                            }
                        }

                        #[cfg(not(debug_assertions))]
                        if !resolved {
                            if let Some(path_str) = Self::ensure_portable_embedded_mcp(&js_filename)
                            {
                                *arg = path_str;
                                crate::app_log!(
                                    "[MCP] Resolved to embedded portable resource: {}",
                                    arg
                                );
                                resolved = true;
                            }
                        }

                        // Dev mode fallback when cwd is the project root or src-tauri.
                        if !resolved {
                            let dev_path = std::path::PathBuf::from("src-tauri/mcp-servers")
                                .join(&js_filename);
                            if dev_path.exists() {
                                if let Ok(abs) = std::fs::canonicalize(&dev_path) {
                                    let path_str = abs.to_string_lossy().to_string();
                                    *arg = path_str
                                        .strip_prefix(r"\\?\")
                                        .unwrap_or(&path_str)
                                        .to_string();
                                    crate::app_log!(
                                        "[MCP] Resolved to Dev-relative resource: {}",
                                        arg
                                    );
                                    resolved = true;
                                }
                            } else {
                                let dev_path2 =
                                    std::path::PathBuf::from("mcp-servers").join(&js_filename);
                                if dev_path2.exists() {
                                    if let Ok(abs) = std::fs::canonicalize(&dev_path2) {
                                        let path_str = abs.to_string_lossy().to_string();
                                        *arg = path_str
                                            .strip_prefix(r"\\?\")
                                            .unwrap_or(&path_str)
                                            .to_string();
                                        crate::app_log!(
                                            "[MCP] Resolved to Dev-relative resource: {}",
                                            arg
                                        );
                                        resolved = true;
                                    }
                                }
                            }
                        }

                        if !resolved {
                            crate::app_log!(
                                "[WARN] Could not resolve MCP resource '{}' via any method",
                                js_filename
                            );
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

                // In bundled builds, run packaged helper binaries directly instead of
                // writing embedded bytes to disk on first launch.
                if !exe_resolved {
                    if let Ok(path) = app_handle
                        .path()
                        .resolve(&exe_subpath, tauri::path::BaseDirectory::Resource)
                    {
                        if path.exists() {
                            let path_str = path.to_string_lossy().to_string();
                            command = path_str
                                .strip_prefix(r"\\?\")
                                .unwrap_or(&path_str)
                                .to_string();
                            crate::app_log!("[MCP] Resolved .exe to resource: {}", command);
                            exe_resolved = true;
                        }
                    }
                }

                // Next to main EXE for portable/dev launches.
                if !exe_resolved {
                    if let Ok(current_exe) = std::env::current_exe() {
                        if let Some(exe_dir) = current_exe.parent() {
                            let local = exe_dir.join("mcp-servers").join(&exe_filename);
                            if local.exists() {
                                command = Self::normalize_spawn_path(&local);
                                crate::app_log!("[MCP] Resolved .exe EXE-relative: {}", command);
                                exe_resolved = true;
                            }
                        }
                    }
                }

                #[cfg(not(debug_assertions))]
                if !exe_resolved {
                    if let Some(path_str) = Self::ensure_portable_embedded_mcp(&exe_filename) {
                        command = path_str;
                        crate::app_log!(
                            "[MCP] Resolved .exe via embedded portable fallback: {}",
                            command
                        );
                        exe_resolved = true;
                    }
                }

                // Dev mode fallback (src-tauri/mcp-servers)
                if !exe_resolved {
                    let dev_path =
                        std::path::PathBuf::from("src-tauri/mcp-servers").join(&exe_filename);
                    if dev_path.exists() {
                        if let Ok(abs) = std::fs::canonicalize(&dev_path) {
                            let path_str = abs.to_string_lossy().to_string();
                            command = path_str
                                .strip_prefix(r"\\?\")
                                .unwrap_or(&path_str)
                                .to_string();
                            crate::app_log!("[MCP] Resolved .exe Dev-relative: {}", command);
                            exe_resolved = true;
                        }
                    } else {
                        // try just mcp-servers (if cwd is already src-tauri)
                        let dev_path2 = std::path::PathBuf::from("mcp-servers").join(&exe_filename);
                        if dev_path2.exists() {
                            if let Ok(abs) = std::fs::canonicalize(&dev_path2) {
                                let path_str = abs.to_string_lossy().to_string();
                                command = path_str
                                    .strip_prefix(r"\\?\")
                                    .unwrap_or(&path_str)
                                    .to_string();
                                crate::app_log!("[MCP] Resolved .exe Dev-relative: {}", command);
                                exe_resolved = true;
                            }
                        }
                    }
                }

                if !exe_resolved {
                    crate::app_log!(
                        "[WARN] Could not resolve .exe '{}' — ensure mcp-1c-search is built",
                        exe_filename
                    );
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
                let has_ts_or_js = args
                    .iter()
                    .any(|a| a.ends_with(".ts") || a.ends_with(".js") || a.ends_with(".cjs"));
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

        let mut child = cmd
            .spawn()
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
        let initialized = Arc::new(AtomicBool::new(false));
        let init_lock = Arc::new(tokio::sync::Mutex::new(()));

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
        let pending_for_writer_drain = pending_requests.clone();
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
            // Процесс умер — немедленно уведомляем все ожидающие запросы
            let mut pending = pending_for_writer_drain.lock().await;
            for (_, sender) in pending.drain() {
                let _ = sender.send(Err("MCP server process died (stdin closed)".to_string()));
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
                             // New JSON format (preferred): SEARCH_STATUS_JSON:{...}
                             // Legacy colon format (fallback): SEARCH_STATUS:{state}:{sym_count}:{db_size_mb}:{built_at_unix}
                             if is_search_server && line.starts_with("SEARCH_STATUS_JSON:") {
                                 let json_str = line.trim_start_matches("SEARCH_STATUS_JSON:");
                                 if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                                     let state = v["state"].as_str().unwrap_or("").to_string();
                                     let progress = v["progress"].as_u64().unwrap_or(0) as u32;
                                     let message = v["message"].as_str().unwrap_or("").to_string();
                                     let sym_count = v["sym_count"].as_u64().unwrap_or(0);
                                     let db_size_mb = v["db_size_mb"].as_f64().unwrap_or(0.0);
                                     let built_at_unix = v["built_at_unix"].as_u64().unwrap_or(0);

                                     *help_status_writer.lock().await = state.clone();
                                     *help_progress_writer.lock().await = progress;

                                     let display_msg = match state.as_str() {
                                         "ready" if sym_count > 0 => {
                                             let date_str = format_unix_msk(built_at_unix);
                                             match (db_size_mb > 0.1, !date_str.is_empty()) {
                                                 (true, true)  => format!("{} символов • {:.1} МБ • {}", sym_count, db_size_mb, date_str),
                                                 (true, false) => format!("{} символов • {:.1} МБ", sym_count, db_size_mb),
                                                 (false, true) => format!("{} символов • {}", sym_count, date_str),
                                                 _             => format!("{} символов", sym_count),
                                             }
                                         }
                                         "ready" => message.clone(),
                                         _ => message.clone(),
                                     };
                                     *help_message_writer.lock().await = display_msg;
                                 }
                             } else if is_search_server && line.starts_with("SEARCH_STATUS:") {
                                 // Legacy format fallback
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
                                             let date_str = format_unix_msk(built_at_unix);

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
                initialized,
                init_lock,
                _child: Arc::new(Mutex::new(child)),
            },
            next_id: std::sync::atomic::AtomicU64::new(1),
            logs,
            help_status,
            help_progress,
            help_message,
        })
    }

    fn stdio_timeout_secs(&self) -> u64 {
        if self.config.id == "builtin-1c-search" || self.config.id == "builtin-1c-naparnik" {
            120
        } else {
            30
        }
    }

    async fn send_stdio_request_message(
        &self,
        tx: &mpsc::Sender<JsonRpcRequest>,
        pending_requests: &Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>,
        req: JsonRpcRequest,
    ) -> Result<Value, String> {
        let id = req
            .id
            .ok_or_else(|| "JSON-RPC request id is missing".to_string())?;
        let (auth_tx, auth_rx) = oneshot::channel();
        {
            let mut pending = pending_requests.lock().await;
            pending.insert(id, auth_tx);
        }

        crate::app_log!(
            "[MCP][{}] >>> Sending: {}",
            self.config.id,
            serde_json::to_string(&req).unwrap_or_default()
        );
        tx.send(req)
            .await
            .map_err(|_| "Failed to send request to MCP process".to_string())?;

        match tokio::time::timeout(Duration::from_secs(self.stdio_timeout_secs()), auth_rx).await {
            Ok(Ok(result)) => {
                crate::app_log!(
                    "[MCP][{}] <<< Received result for id {}",
                    self.config.id,
                    id
                );
                result
            }
            Ok(Err(_)) => {
                crate::app_log!(
                    "[MCP][{}][ERROR] Response channel closed for id {}",
                    self.config.id,
                    id
                );
                Err("Channel closed".to_string())
            }
            Err(_) => {
                let mut pending = pending_requests.lock().await;
                pending.remove(&id);
                crate::app_log!(
                    "[MCP][{}][ERROR] Request timed out for id {}",
                    self.config.id,
                    id
                );
                Err("Timeout waiting for MCP response".to_string())
            }
        }
    }

    async fn send_stdio_notification_message(
        &self,
        tx: &mpsc::Sender<JsonRpcRequest>,
        notification: JsonRpcRequest,
    ) -> Result<(), String> {
        crate::app_log!(
            "[MCP][{}] >>> Notification: {}",
            self.config.id,
            serde_json::to_string(&notification).unwrap_or_default()
        );
        tx.send(notification)
            .await
            .map_err(|_| "Failed to send notification to MCP process".to_string())
    }

    async fn initialize_stdio_session(
        &self,
        tx: &mpsc::Sender<JsonRpcRequest>,
        pending_requests: &Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>,
        initialized: &Arc<AtomicBool>,
        init_lock: &Arc<tokio::sync::Mutex<()>>,
    ) -> Result<(), String> {
        if initialized.load(Ordering::SeqCst) {
            return Ok(());
        }

        let _guard = init_lock.lock().await;
        if initialized.load(Ordering::SeqCst) {
            return Ok(());
        }

        crate::app_log!(
            "[MCP][{}] Starting stdio initialize handshake",
            self.config.id
        );

        let init_id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let init_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "initialize".to_string(),
            params: json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "mini-ai-1c", "version": "1.0" }
            }),
            id: Some(init_id),
        };
        let _ = self
            .send_stdio_request_message(tx, pending_requests, init_req)
            .await?;

        self.send_stdio_notification_message(
            tx,
            JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: "notifications/initialized".to_string(),
                params: json!({}),
                id: None,
            },
        )
        .await?;

        initialized.store(true, Ordering::SeqCst);
        crate::app_log!(
            "[MCP][{}] Stdio initialize handshake completed",
            self.config.id
        );
        Ok(())
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
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: params.clone(),
            id: Some(id),
        };
        let req_payload = serde_json::to_value(&req).map_err(|e| e.to_string())?;

        match &self.transport {
            TransportImpl::Http {
                client,
                url,
                effective_url,
                login,
                password,
                extra_headers,
                http_state,
                health,
                last_checked,
                ..
            } => {
                let known_state = {
                    let state = http_state.lock().await;
                    state.clone()
                };
                let current_session_id = known_state.as_ref().and_then(|state| state.clone());
                let current_url = {
                    let effective = effective_url.lock().await;
                    effective.clone().unwrap_or_else(|| url.clone())
                };

                let initial_response = match Self::send_http_payload(
                    client,
                    &current_url,
                    login,
                    password,
                    extra_headers,
                    &req_payload,
                    current_session_id.as_deref(),
                    true,
                )
                .await
                {
                    Ok(response) => response,
                    Err(error) => {
                        *health.lock().await = HttpHealthState::Offline;
                        *last_checked.lock().await = now_unix();
                        return Err(error);
                    }
                };

                if Self::should_update_effective_http_url(&current_url, &initial_response.final_url)
                {
                    let mut effective = effective_url.lock().await;
                    *effective = Some(initial_response.final_url.clone());
                }

                match Self::extract_http_result(&initial_response) {
                    Ok(result) => {
                        if known_state.is_none() {
                            let mut state = http_state.lock().await;
                            if state.is_none() {
                                *state = Some(None);
                            }
                        }
                        *health.lock().await = HttpHealthState::Connected;
                        *last_checked.lock().await = now_unix();
                        Ok(result)
                    }
                    Err(initial_error) => {
                        if !Self::should_retry_with_initialize(&initial_response) {
                            let next_health = if initial_response
                                .rpc_response
                                .as_ref()
                                .and_then(|response| response.error.as_ref())
                                .is_some()
                            {
                                HttpHealthState::Connected
                            } else {
                                HttpHealthState::Error
                            };
                            *health.lock().await = next_health;
                            *last_checked.lock().await = now_unix();
                            return Err(initial_error);
                        }

                        if current_session_id.is_some() {
                            crate::app_log!(
                                "[MCP][HTTP] Refreshing MCP session for {}",
                                current_url
                            );
                        } else {
                            crate::app_log!(
                                "[MCP][HTTP] Falling back to initialize handshake for {}",
                                current_url
                            );
                        }

                        let (new_session_id, initialized_url) = match Self::initialize_http_session(
                            client,
                            &initial_response.final_url,
                            login,
                            password,
                            extra_headers,
                        )
                        .await
                        {
                            Ok(session_id) => session_id,
                            Err(error) => {
                                *health.lock().await = HttpHealthState::Error;
                                *last_checked.lock().await = now_unix();
                                return Err(error);
                            }
                        };

                        {
                            let mut state = http_state.lock().await;
                            *state = Some(new_session_id.clone());
                        }
                        {
                            let mut effective = effective_url.lock().await;
                            *effective = Some(initialized_url.clone());
                        }

                        let retry_response = match Self::send_http_payload(
                            client,
                            &initialized_url,
                            login,
                            password,
                            extra_headers,
                            &req_payload,
                            new_session_id.as_deref(),
                            true,
                        )
                        .await
                        {
                            Ok(response) => response,
                            Err(error) => {
                                *health.lock().await = HttpHealthState::Offline;
                                *last_checked.lock().await = now_unix();
                                return Err(error);
                            }
                        };

                        if Self::should_update_effective_http_url(
                            &initialized_url,
                            &retry_response.final_url,
                        ) {
                            let mut effective = effective_url.lock().await;
                            *effective = Some(retry_response.final_url.clone());
                        }

                        match Self::extract_http_result(&retry_response) {
                            Ok(result) => {
                                *health.lock().await = HttpHealthState::Connected;
                                *last_checked.lock().await = now_unix();
                                Ok(result)
                            }
                            Err(error) => {
                                let next_health = if retry_response
                                    .rpc_response
                                    .as_ref()
                                    .and_then(|response| response.error.as_ref())
                                    .is_some()
                                {
                                    HttpHealthState::Connected
                                } else {
                                    HttpHealthState::Error
                                };
                                *health.lock().await = next_health;
                                *last_checked.lock().await = now_unix();
                                Err(error)
                            }
                        }
                    }
                }
            }
            TransportImpl::Stdio {
                tx,
                pending_requests,
                initialized,
                init_lock,
                ..
            } => {
                let first_attempt = self
                    .send_stdio_request_message(tx, pending_requests, req.clone())
                    .await;
                match first_attempt {
                    Ok(result) => Ok(result),
                    Err(error) if Self::should_retry_stdio_with_initialize(&error) => {
                        initialized.store(false, Ordering::SeqCst);
                        self.initialize_stdio_session(tx, pending_requests, initialized, init_lock)
                            .await?;
                        self.send_stdio_request_message(tx, pending_requests, req)
                            .await
                    }
                    Err(error) => Err(error),
                }
            }
            TransportImpl::Internal { handler } => handler.call_tool(method, params.clone()).await,
        }
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>, String> {
        match &self.transport {
            TransportImpl::Internal { handler } => Ok(handler.list_tools().await),
            _ => {
                let result = self.request("tools/list", json!({})).await?;
                if let Some(tools_arr) = result.get("tools").and_then(|v| v.as_array()) {
                    let tools = tools_arr
                        .iter()
                        .filter_map(|v| {
                            Some(McpTool {
                                name: v.get("name")?.as_str()?.to_string(),
                                description: v
                                    .get("description")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                input_schema: v.get("inputSchema")?.clone(),
                            })
                        })
                        .collect();
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
                crate::app_log!(
                    "[DEBUG] McpSession::call_tool handling Internal for {}",
                    name
                );
                handler.call_tool(name, arguments).await
            }
            _ => {
                self.request(
                    "tools/call",
                    json!({
                        "name": name,
                        "arguments": arguments
                    }),
                )
                .await
            }
        }
    }

    async fn get_status_string(&self) -> String {
        match &self.transport {
            TransportImpl::Http { health, .. } => match *health.lock().await {
                HttpHealthState::Unknown => "unknown",
                HttpHealthState::Connected => "connected",
                HttpHealthState::Offline => "offline",
                HttpHealthState::Error => "error",
            }
            .to_string(),
            _ => {
                if self.is_alive().await {
                    "connected".to_string()
                } else {
                    "stopped".to_string()
                }
            }
        }
    }

    async fn get_last_checked(&self) -> i64 {
        match &self.transport {
            TransportImpl::Http { last_checked, .. } => *last_checked.lock().await,
            _ => 0,
        }
    }

    async fn get_health(&self) -> Option<HttpHealthState> {
        match &self.transport {
            TransportImpl::Http { health, .. } => Some(*health.lock().await),
            _ => None,
        }
    }

    async fn reset_http_state(&self) {
        if let TransportImpl::Http {
            http_state,
            effective_url,
            ..
        } = &self.transport
        {
            *http_state.lock().await = None;
            *effective_url.lock().await = None;
        }
    }

    async fn try_begin_http_probe(&self, now: i64) -> bool {
        match &self.transport {
            TransportImpl::Http {
                last_checked,
                probe_in_flight,
                ..
            } => {
                if probe_in_flight
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    return false;
                }
                *last_checked.lock().await = now;
                true
            }
            _ => false,
        }
    }

    fn finish_http_probe(&self) {
        if let TransportImpl::Http {
            probe_in_flight, ..
        } = &self.transport
        {
            probe_in_flight.store(false, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn detects_initialize_requirement_from_rpc_error() {
        let response = HttpRpcResponse {
            status: reqwest::StatusCode::OK,
            body: String::new(),
            rpc_response: Some(JsonRpcResponse {
                _jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32000,
                    message: "Session not initialized. Call initialize first.".to_string(),
                }),
                id: Some(1),
            }),
            session_id: None,
            final_url: "http://localhost/mcp".to_string(),
        };

        assert!(McpSession::should_retry_with_initialize(&response));
    }

    #[test]
    fn detects_initialize_requirement_from_http_body() {
        let response = HttpRpcResponse {
            status: reqwest::StatusCode::BAD_REQUEST,
            body: "MCP Streamable HTTP session missing. Send initialize first.".to_string(),
            rpc_response: None,
            session_id: None,
            final_url: "http://localhost/mcp/".to_string(),
        };

        assert!(McpSession::should_retry_with_initialize(&response));
    }

    #[test]
    fn does_not_retry_on_unrelated_http_error() {
        let response = HttpRpcResponse {
            status: reqwest::StatusCode::UNAUTHORIZED,
            body: "Unauthorized".to_string(),
            rpc_response: None,
            session_id: None,
            final_url: "http://localhost/mcp".to_string(),
        };

        assert!(!McpSession::should_retry_with_initialize(&response));
    }

    #[test]
    fn keeps_redirected_http_url_with_trailing_slash() {
        assert!(McpSession::should_update_effective_http_url(
            "http://localhost:8004/mcp",
            "http://localhost:8004/mcp/"
        ));
    }

    #[test]
    fn detects_builtin_search_path_validation_errors() {
        let missing_path = McpServerConfig {
            id: BUILTIN_1C_SEARCH_SERVER_ID.to_string(),
            ..Default::default()
        };
        assert_eq!(
            builtin_search_unavailable_reason(&missing_path),
            Some("Путь к выгрузке конфигурации 1С не задан".to_string())
        );

        let invalid_path = McpServerConfig {
            id: BUILTIN_1C_SEARCH_SERVER_ID.to_string(),
            env: Some(HashMap::from([(
                "ONEC_CONFIG_PATH".to_string(),
                "Z:\\definitely-missing-mini-ai-1c".to_string(),
            )])),
            ..Default::default()
        };
        assert!(builtin_search_unavailable_reason(&invalid_path)
            .expect("expected invalid path error")
            .contains("не найден"));
    }

    #[test]
    fn parses_sse_json_rpc_payload() {
        let parsed = McpSession::parse_http_rpc_response(
            "text/event-stream",
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n",
        )
        .expect("expected parsed SSE payload");

        let result = parsed.result.expect("expected result object");
        assert_eq!(result.get("ok"), Some(&Value::Bool(true)));
    }

    #[test]
    fn writes_embedded_resource_into_mcp_servers_subdir() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("expected monotonic clock")
            .as_nanos();
        let base_dir = std::env::temp_dir().join(format!("mini-ai-1c-mcp-test-{unique}"));
        let bytes = b"console.log('portable');";

        let path =
            McpSession::write_embedded_mcp_resource_to_dir(&base_dir, "test-tool.cjs", bytes)
                .expect("expected embedded resource write to succeed");

        assert_eq!(path, base_dir.join("mcp-servers").join("test-tool.cjs"));
        assert_eq!(
            std::fs::read(&path).expect("expected written file to exist"),
            bytes
        );

        let _ = std::fs::remove_dir_all(&base_dir);
    }

    #[test]
    fn rewrites_embedded_resource_when_contents_change() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("expected monotonic clock")
            .as_nanos();
        let base_dir = std::env::temp_dir().join(format!("mini-ai-1c-mcp-test-{unique}"));
        let path = base_dir.join("mcp-servers").join("test-tool.cjs");

        std::fs::create_dir_all(path.parent().expect("expected parent dir"))
            .expect("expected test dir creation");
        std::fs::write(&path, b"old").expect("expected seed file");

        McpSession::write_embedded_mcp_resource_to_dir(&base_dir, "test-tool.cjs", b"new")
            .expect("expected embedded resource rewrite to succeed");

        assert_eq!(
            std::fs::read(&path).expect("expected rewritten file to exist"),
            b"new"
        );

        let _ = std::fs::remove_dir_all(&base_dir);
    }
}
