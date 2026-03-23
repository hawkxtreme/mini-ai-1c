use crate::mcp_client::{McpClient, McpServerStatus, McpTool};
use crate::settings::{load_settings, McpServerConfig};
use futures::future::join_all;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Struct returned to frontend with aggregated tool metadata
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpToolInfo {
    pub server_name: String,
    pub tool_name: String,
    pub description: Option<String>,
    pub input_schema: Option<Value>,
    pub is_enabled: bool,
}

lazy_static! {
    static ref MCP_TOOLS_CACHE: Mutex<Option<(String, Vec<McpToolInfo>, Instant)>> = Mutex::new(None);
}
const MCP_TOOLS_CACHE_TTL_SECS: u64 = 300;
const MCP_TOOLS_REQUEST_TIMEOUT_SECS: u64 = 8;
const INTERNAL_BSL_SERVER_ID: &str = "bsl-ls";

fn unavailable_tool(server_name: String, message: String) -> Vec<McpToolInfo> {
    vec![McpToolInfo {
        server_name,
        tool_name: "__server_unavailable__".to_string(),
        description: Some(message),
        input_schema: None,
        is_enabled: false,
    }]
}

async fn collect_server_tools(config: McpServerConfig) -> Vec<McpToolInfo> {
    let server_name = config.name.clone();
    let timeout = Duration::from_secs(MCP_TOOLS_REQUEST_TIMEOUT_SECS);

    match tokio::time::timeout(timeout, async move {
        let client = McpClient::new(config).await?;
        client.list_tools().await
    })
    .await
    {
        Ok(Ok(tools)) => tools
            .into_iter()
            .map(|tool| McpToolInfo {
                server_name: server_name.clone(),
                tool_name: tool.name,
                description: Some(tool.description),
                input_schema: Some(tool.input_schema),
                is_enabled: true,
            })
            .collect(),
        Ok(Err(error)) => unavailable_tool(server_name, format!("Failed to list tools: {}", error)),
        Err(_) => unavailable_tool(
            server_name,
            format!(
                "Timed out while loading tools after {}s",
                MCP_TOOLS_REQUEST_TIMEOUT_SECS
            ),
        ),
    }
}

fn get_tool_identity(tool: &McpToolInfo) -> String {
    format!("{}::{}", tool.server_name, tool.tool_name)
}

fn dedupe_tools(tools: Vec<McpToolInfo>) -> Vec<McpToolInfo> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(tools.len());

    for tool in tools {
        if seen.insert(get_tool_identity(&tool)) {
            deduped.push(tool);
        }
    }

    deduped
}

/// Get available MCP tools from a specific server
#[tauri::command]
pub async fn get_mcp_tools(server_id: String) -> Result<Vec<McpTool>, String> {
    let settings = load_settings();
    let config = settings
        .mcp_servers
        .iter()
        .find(|s| s.id == server_id)
        .cloned()
        .or_else(|| {
            if server_id == "bsl-ls" {
                Some(crate::settings::McpServerConfig {
                    id: "bsl-ls".to_string(),
                    name: "BSL Language Server".to_string(),
                    enabled: settings.bsl_server.enabled,
                    transport: crate::settings::McpTransport::Internal,
                    ..Default::default()
                })
            } else {
                None
            }
        })
        .ok_or_else(|| format!("MCP server with ID '{}' not found", server_id))?;

    let client = McpClient::new(config).await?;
    client.list_tools().await
}

/// List tools across all enabled MCP servers (cached)
#[tauri::command]
pub async fn list_mcp_tools(force_refresh: Option<bool>) -> Result<Vec<McpToolInfo>, String> {
    let force = force_refresh.unwrap_or(false);
    let settings = load_settings();
    let mut configs: Vec<McpServerConfig> = settings
        .mcp_servers
        .iter()
        .filter(|server| server.enabled)
        .cloned()
        .collect();

    // Include internal BSL LS only when it isn't already represented in the configured MCP list.
    if settings.bsl_server.enabled
        && !configs.iter().any(|config| config.id == INTERNAL_BSL_SERVER_ID)
    {
        configs.push(crate::settings::McpServerConfig {
            id: INTERNAL_BSL_SERVER_ID.to_string(),
            name: "BSL Language Server".to_string(),
            enabled: settings.bsl_server.enabled,
            transport: crate::settings::McpTransport::Internal,
            ..Default::default()
        });
    }

    let cache_key = serde_json::to_string(&configs)
        .unwrap_or_else(|_| format!("fallback:{}:{}", configs.len(), settings.bsl_server.enabled));

    // Check cache
    if !force {
        if let Ok(cache_lock) = MCP_TOOLS_CACHE.lock() {
            if let Some((cached_key, cached, ts)) = &*cache_lock {
                if cached_key == &cache_key && ts.elapsed().as_secs() < MCP_TOOLS_CACHE_TTL_SECS {
                    return Ok(cached.clone());
                }
            }
        }
    }

    let result = dedupe_tools(
        join_all(configs.into_iter().map(collect_server_tools))
        .await
        .into_iter()
        .flatten()
        .collect(),
    );

    // Update cache
    if let Ok(mut cache_lock) = MCP_TOOLS_CACHE.lock() {
        *cache_lock = Some((cache_key, result.clone(), Instant::now()));
    }

    Ok(result)
}

/// Get status of all MCP servers
#[tauri::command]
pub async fn get_mcp_server_statuses() -> Result<Vec<McpServerStatus>, String> {
    Ok(crate::mcp_client::McpManager::get_statuses().await)
}

/// Get logs of a specific MCP server
#[tauri::command]
pub async fn get_mcp_server_logs(server_id: String) -> Result<Vec<String>, String> {
    Ok(crate::mcp_client::McpManager::get_logs(&server_id).await)
}

/// Save all debug logs to a file
#[tauri::command]
pub async fn save_debug_logs(app_handle: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_dialog::DialogExt;

    let logs = crate::logger::get_all_logs();

    let file_path = app_handle
        .dialog()
        .file()
        .add_filter("Text", &["txt"])
        .set_file_name("mini-ai-1c-logs.txt")
        .blocking_save_file();

    if let Some(path) = file_path {
        std::fs::write(path.to_string(), logs)
            .map_err(|e| format!("Failed to write logs: {}", e))?;
        crate::app_log!("Logs saved successfully to {}", path.to_string());
    }

    Ok(())
}

/// Call an MCP tool on a specific server
#[tauri::command]
pub async fn call_mcp_tool(
    server_id: String,
    name: String,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let settings = load_settings();
    let config = settings
        .mcp_servers
        .iter()
        .find(|s| s.id == server_id)
        .cloned()
        .or_else(|| {
            if server_id == "bsl-ls" {
                Some(crate::settings::McpServerConfig {
                    id: "bsl-ls".to_string(),
                    name: "BSL Language Server".to_string(),
                    enabled: settings.bsl_server.enabled,
                    transport: crate::settings::McpTransport::Internal,
                    ..Default::default()
                })
            } else {
                None
            }
        })
        .ok_or_else(|| format!("MCP server with ID '{}' not found", server_id))?;

    let client = McpClient::new(config).await?;
    client.call_tool(&name, arguments).await
}

/// Test connection to an MCP server
#[tauri::command]
pub async fn test_mcp_connection(config: McpServerConfig) -> Result<String, String> {
    let client = McpClient::new(config).await?;
    match client.list_tools().await {
        Ok(tools) => Ok(format!("Подключено! ({})", tools.len())),
        Err(e) => Err(format!("Ошибка: {}", e)),
    }
}

/// Delete the SQLite search index .db file for a given config path.
#[tauri::command]
pub async fn delete_search_index(config_path: String) -> Result<(), String> {
    let db = search_index_db_path(&config_path);
    if db.exists() {
        std::fs::remove_file(&db).map_err(|e| format!("Не удалось удалить файл индекса: {}", e))?;
    }
    Ok(())
}

/// Open the search-index directory in the system file explorer.
#[tauri::command]
pub async fn open_search_index_dir(app_handle: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = dirs::data_dir()
        .ok_or("Не удалось определить директорию данных")?
        .join("com.mini-ai-1c")
        .join("search-index");
    std::fs::create_dir_all(&dir).ok();
    app_handle
        .opener()
        .open_path(dir.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| format!("Не удалось открыть папку: {}", e))
}

/// Compute the db path for a given config path (mirrors mcp-1c-search::index::get_db_path).
fn search_index_db_path(config_path: &str) -> std::path::PathBuf {
    let hash = fnv_hash_path(config_path);
    if let Some(data_dir) = dirs::data_dir() {
        data_dir
            .join("com.mini-ai-1c")
            .join("search-index")
            .join(format!("{:016x}.db", hash))
    } else {
        std::path::PathBuf::from(config_path)
            .join(".mcp-index")
            .join("symbols.db")
    }
}

/// FNV-1 hash — must match the implementation in mcp-1c-search/src/index.rs.
fn fnv_hash_path(s: &str) -> u64 {
    let mut hash: u64 = 14695981039346656037;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(1099511628211);
        hash ^= byte as u64;
    }
    hash
}
