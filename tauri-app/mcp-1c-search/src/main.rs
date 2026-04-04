use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::{json, Value};

mod search;
mod tools;
mod parser;
mod index;
mod metadata;

/// Returns SQLite DB file size in MB (0.0 if not found).
pub fn db_size_mb(path: &std::path::Path) -> f64 {
    std::fs::metadata(path)
        .map(|m| m.len() as f64 / 1024.0 / 1024.0)
        .unwrap_or(0.0)
}

fn emit_search_status_json(
    state: &str,
    progress: u32,
    message: &str,
    sym_count: usize,
    db_size_mb: f64,
    built_at_unix: u64,
) {
    eprintln!(
        "SEARCH_STATUS_JSON:{}",
        json!({
            "state": state,
            "progress": progress,
            "message": message,
            "sym_count": sym_count,
            "db_size_mb": db_size_mb,
            "built_at_unix": built_at_unix
        })
    );
}

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
    // IMPORTANT: Do NOT call count_files_and_size() here synchronously.
    // On large configs (5GB+, 100k+ files) it blocks the async main for 30+ seconds,
    // preventing the JSON-RPC event loop from starting and causing tools/list timeouts.
    match &config_path {
        Some(_) => {
            // Emit preliminary ready immediately so the event loop can start.
            // Background task below will emit an updated status with actual counts.
            eprintln!("SEARCH_STATUS:ready:0:0.00");
            emit_search_status_json("bootstrapping", 0, "Инициализация...", 0, 0.0, 0);
        }
        None => {
            if config_path_str.is_empty() {
                eprintln!("SEARCH_STATUS:unavailable:Путь к конфигурации не задан");
                emit_search_status_json("unavailable", 0, "Путь к конфигурации не задан", 0, 0.0, 0);
            } else {
                eprintln!("SEARCH_STATUS:unavailable:Директория не найдена: {}", config_path_str);
                emit_search_status_json(
                    "unavailable",
                    0,
                    &format!("Директория не найдена: {}", config_path_str),
                    0,
                    0.0,
                    0,
                );
            }
        }
    }

    // Background: build or sync symbol index, then emit accurate status.
    // Runs in spawn_blocking so it doesn't block the async event loop.
    if let (Some(root), Some(db)) = (config_path.clone(), db_path.clone()) {
        let db_for_index = db.clone();
        // Detached background task — JoinHandle intentionally dropped
        let _ = tokio::task::spawn_blocking(move || {
            emit_search_status_json("schema_init", 1, "Инициализация схемы индекса...", 0, 0.0, 0);
            // Ensure DB schema exists before anything else
            if let Err(e) = index::ensure_schema(&db_for_index) {
                eprintln!("[1c-search] Schema init failed: {}", e);
                emit_search_status_json(
                    "unavailable",
                    0,
                    &format!("Ошибка инициализации схемы: {}", e),
                    index::symbol_count(&db_for_index),
                    db_size_mb(&db_for_index),
                    index::get_built_at(&db_for_index).unwrap_or(0),
                );
                return;
            }

            // Migrate: if calls table is empty but symbols exist, reset indexed_files
            // so the next sync will re-parse all files and populate the call graph.
            index::migrate_if_needed(&db_for_index);

            // Build metadata if missing, or if objects exist but have no attributes
            // (happens when ConfigDumpInfo.xml was absent on first run — per-object XMLs will be parsed now)
            let needs_metadata = !index::metadata_exists(&db_for_index)
                || (index::metadata_exists(&db_for_index) && !index::metadata_has_items(&db_for_index));
            if needs_metadata {
                emit_search_status_json(
                    "metadata_indexing",
                    3,
                    "Индексация метаданных...",
                    index::symbol_count(&db_for_index),
                    db_size_mb(&db_for_index),
                    index::get_built_at(&db_for_index).unwrap_or(0),
                );
                match metadata::build_metadata(&root, &db_for_index) {
                    Ok(n) => eprintln!("[1c-search] Metadata indexed: {} objects", n),
                    Err(e) => eprintln!("[1c-search] Metadata skipped: {}", e),
                }
            }

            if index::index_exists(&db_for_index) {
                // ─── Incremental sync (mtime-based) ─────────────────────────
                eprintln!("[1c-search] Index found — running incremental sync...");
                emit_search_status_json(
                    "syncing_index",
                    5,
                    "Синхронизация индекса...",
                    index::symbol_count(&db_for_index),
                    db_size_mb(&db_for_index),
                    index::get_built_at(&db_for_index).unwrap_or(0),
                );
                match index::sync_index(&root, &db_for_index) {
                    Ok(stats) => {
                        let size = db_size_mb(&db_for_index);
                        let built_at = index::get_built_at(&db_for_index).unwrap_or(0);
                        eprintln!(
                            "[1c-search] Sync done: +{} ~{} -{} total={}",
                            stats.added, stats.updated, stats.removed, stats.total_symbols
                        );
                        eprintln!(
                            "SEARCH_STATUS:ready:{}:{:.2}:{}",
                            stats.total_symbols, size, built_at
                        );
                        emit_search_status_json("ready", 100, "Индекс готов", stats.total_symbols, size, built_at);
                    }
                    Err(e) => {
                        eprintln!("[1c-search] Sync error: {}", e);
                        let sym_count = index::symbol_count(&db_for_index);
                        let size = db_size_mb(&db_for_index);
                        let built_at = index::get_built_at(&db_for_index).unwrap_or(0);
                        if sym_count > 0 {
                            emit_search_status_json(
                                "degraded",
                                100,
                                &format!(
                                    "Синхронизация завершилась с ошибкой, используется существующий индекс: {}",
                                    e
                                ),
                                sym_count,
                                size,
                                built_at,
                            );
                        } else {
                            eprintln!("SEARCH_STATUS:unavailable:Ошибка синхронизации: {}", e);
                            emit_search_status_json(
                                "unavailable",
                                0,
                                &format!("Ошибка синхронизации: {}", e),
                                0,
                                size,
                                built_at,
                            );
                        }
                    }
                }
            } else {
                // ─── Full build ─────────────────────────────────────────────
                eprintln!("[1c-search] No index found — starting full build...");
                emit_search_status_json("building_index", 0, "Первичная индексация...", 0, 0.0, 0);
                match index::build_index(&root, &db_for_index) {
                    Ok(sym_count) => {
                        let size = db_size_mb(&db_for_index);
                        let built_at = index::get_built_at(&db_for_index).unwrap_or(0);
                        eprintln!(
                            "SEARCH_STATUS:ready:{}:{:.2}:{}",
                            sym_count, size, built_at
                        );
                        emit_search_status_json("ready", 100, "Индекс построен", sym_count, size, built_at);
                    }
                    Err(e) => {
                        eprintln!("SEARCH_STATUS:unavailable:Ошибка индексации: {}", e);
                        emit_search_status_json("unavailable", 0, &format!("Ошибка индексации: {}", e), 0, 0.0, 0);
                    }
                }
            }
        });
    }

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    // Wrap stdout in Arc<Mutex<>> so concurrent tasks can write responses safely
    let stdout = Arc::new(tokio::sync::Mutex::new(stdout));

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

                let method = request["method"].as_str().unwrap_or("").to_string();
                let params = request.get("params").cloned().unwrap_or(json!({}));

                // Spawn each request as an independent async task so that
                // heavy tools (find_references, search_code on large configs)
                // don't block subsequent tools/list or initialize responses.
                let config_path_task = config_path.clone();
                let db_path_task = db_path.clone();
                let stdout_task = Arc::clone(&stdout);

                tokio::spawn(async move {
                    let result = handle_method(&method, &params, &config_path_task, &db_path_task).await;

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
                    let mut out = stdout_task.lock().await;
                    let _ = out.write_all(resp_str.as_bytes()).await;
                    let _ = out.write_all(b"\n").await;
                    let _ = out.flush().await;
                });
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
