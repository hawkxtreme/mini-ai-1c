use crate::mcp_client::{McpClient, McpTool, McpServerStatus};
use crate::settings::{load_settings, McpServerConfig};

/// Get available MCP tools from a specific server
#[tauri::command]
pub async fn get_mcp_tools(server_id: String) -> Result<Vec<McpTool>, String> {
    let settings = load_settings();
    let config = settings.mcp_servers.iter()
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
    
    let file_path = app_handle.dialog()
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
pub async fn call_mcp_tool(server_id: String, name: String, arguments: serde_json::Value) -> Result<serde_json::Value, String> {
    let settings = load_settings();
    let config = settings.mcp_servers.iter()
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
        std::fs::remove_file(&db)
            .map_err(|e| format!("Не удалось удалить файл индекса: {}", e))?;
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
