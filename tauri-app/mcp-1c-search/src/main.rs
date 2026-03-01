use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::{json, Value};

mod search;
mod tools;
mod parser;
mod index;

use crate::search::count_files_and_size;

#[tokio::main]
async fn main() {
    let config_path_str = std::env::var("ONEC_CONFIG_PATH").unwrap_or_default();
    let config_path: Option<PathBuf> = if !config_path_str.is_empty() {
        let p = PathBuf::from(&config_path_str);
        if p.exists() && p.is_dir() {
            Some(p)
        } else {
            None
        }
    } else {
        None
    };

    // Derive db_path for symbol index (always Some when config_path is Some)
    let db_path: Option<PathBuf> = config_path.as_ref().map(|p| index::get_db_path(p));

    // Report status via stderr — parsed by mcp_client.rs
    let (file_count, size_mb) = match &config_path {
        Some(path) => {
            let (count, size_mb) = count_files_and_size(path);
            eprintln!("SEARCH_STATUS:ready:{}:{:.2}", count, size_mb);
            (count, size_mb)
        }
        None => {
            if config_path_str.is_empty() {
                eprintln!("SEARCH_STATUS:unavailable:Путь к конфигурации не задан");
            } else {
                eprintln!("SEARCH_STATUS:unavailable:Директория не найдена: {}", config_path_str);
            }
            (0, 0.0)
        }
    };

    // Spawn background symbol indexing if config path is set and index is not ready
    if let (Some(root), Some(db)) = (&config_path, &db_path) {
        if !index::index_exists(db) {
            let root_clone = root.clone();
            let db_clone = db.clone();
            // Detached background task — JoinHandle intentionally dropped
            let _ = tokio::task::spawn_blocking(move || {
                match index::build_index(&root_clone, &db_clone) {
                    Ok(_) => {
                        // Re-emit ready after indexing to update UI status
                        eprintln!("SEARCH_STATUS:ready:{}:{:.2}", file_count, size_mb);
                    }
                    Err(e) => {
                        eprintln!("SEARCH_STATUS:unavailable:Ошибка индексации: {}", e);
                    }
                }
            });
        }
    }

    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF — client disconnected
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let request: Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("[1c-search] JSON parse error: {}", e);
                        continue;
                    }
                };

                // Notifications have no "id" — no response needed per JSON-RPC spec
                let id = match request.get("id") {
                    Some(id) => id.clone(),
                    None => continue,
                };

                let method = request["method"].as_str().unwrap_or("");
                let params = request.get("params").cloned().unwrap_or(json!({}));

                let result = handle_method(method, &params, &config_path, &db_path).await;

                let response = match result {
                    Ok(res) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": res
                    }),
                    Err(msg) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32603,
                            "message": msg
                        }
                    }),
                };

                let resp_str = serde_json::to_string(&response).unwrap_or_default();
                let _ = stdout.write_all(resp_str.as_bytes()).await;
                let _ = stdout.write_all(b"\n").await;
                let _ = stdout.flush().await;
            }
            Err(e) => {
                eprintln!("[1c-search] Read error: {}", e);
                break;
            }
        }
    }
}

async fn handle_method(
    method: &str,
    params: &Value,
    config_path: &Option<PathBuf>,
    db_path: &Option<PathBuf>,
) -> Result<Value, String> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "1c-search", "version": "0.1.0" }
        })),
        "tools/list" => Ok(json!({ "tools": tools::list_tools() })),
        "tools/call" => {
            let tool_name = params["name"].as_str().unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            tools::call_tool(tool_name, &arguments, config_path, db_path).await
        }
        "ping" => Ok(json!({})),
        _ => Err(format!("Method not found: {}", method)),
    }
}
