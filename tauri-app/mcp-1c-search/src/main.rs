use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::{json, Value};

mod search;
mod tools;
mod parser;
mod index;
mod metadata;
mod semantic;
mod workspace;

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
    let workspace = workspace::SearchWorkspace::from_env();

    // Report status via stderr — parsed by mcp_client.rs
    // IMPORTANT: Do NOT call count_files_and_size() here synchronously.
    // On large configs (5GB+, 100k+ files) it blocks the async main for 30+ seconds,
    // preventing the JSON-RPC event loop from starting and causing tools/list timeouts.
    match workspace.primary_root() {
        Some(root) if workspace::root_exists(root) => {
            // Emit preliminary ready immediately so the event loop can start.
            // Background task below will emit an updated status with actual counts.
            eprintln!("SEARCH_STATUS:ready:0:0.00");
            let root_label = if workspace.roots.len() > 1 {
                format!("Инициализация рабочей области: {} источн.", workspace.roots.len())
            } else {
                "Инициализация...".to_string()
            };
            emit_search_status_json("bootstrapping", 0, &root_label, 0, 0.0, 0);
        }
        None => {
            eprintln!("SEARCH_STATUS:unavailable:Путь к конфигурации не задан");
            emit_search_status_json("unavailable", 0, "Путь к конфигурации не задан", 0, 0.0, 0);
        }
        Some(root) => {
            eprintln!(
                "SEARCH_STATUS:unavailable:Директория не найдена: {}",
                root.path.to_string_lossy()
            );
            emit_search_status_json(
                "unavailable",
                0,
                &format!("Директория не найдена: {}", root.path.to_string_lossy()),
                0,
                0.0,
                0,
            );
        }
    }

    // Background: build or sync symbol index, then emit accurate status.
    // Runs in spawn_blocking so it doesn't block the async event loop.
    let index_roots: Vec<_> = workspace
        .roots
        .iter()
        .filter(|root| workspace::root_exists(root))
        .cloned()
        .collect();
    if !index_roots.is_empty() {
        // Detached background task — JoinHandle intentionally dropped
        let _ = tokio::task::spawn_blocking(move || {
            let total_roots = index_roots.len().max(1);
            let mut total_symbols = 0usize;
            let mut total_size = 0.0f64;
            let mut last_built_at = 0u64;
            let mut degraded_messages = Vec::new();

            for (idx, root) in index_roots.iter().enumerate() {
                let root_progress_base = (idx as u32 * 100 / total_roots as u32).min(99);
                let db_for_index = workspace::root_db_path(root);
                let source_label = format!("{} ({})", root.name, root.kind);

                emit_search_status_json(
                    "schema_init",
                    root_progress_base,
                    &format!("Инициализация схемы индекса: {}", source_label),
                    total_symbols,
                    total_size,
                    last_built_at,
                );

                if let Err(e) = index::ensure_schema(&db_for_index) {
                    eprintln!("[1c-search][{}] Schema init failed: {}", root.id, e);
                    degraded_messages.push(format!("{}: {}", source_label, e));
                    continue;
                }

                index::migrate_if_needed(&db_for_index);
                index::migrate_semantic_fts_if_needed(&db_for_index);

                let needs_metadata = !index::metadata_exists(&db_for_index)
                    || (index::metadata_exists(&db_for_index) && !index::metadata_has_items(&db_for_index));
                if needs_metadata {
                    emit_search_status_json(
                        "metadata_indexing",
                        root_progress_base + 1,
                        &format!("Индексация метаданных: {}", source_label),
                        total_symbols,
                        total_size,
                        last_built_at,
                    );
                    match metadata::build_metadata(&root.path, &db_for_index) {
                        Ok(n) => eprintln!("[1c-search][{}] Metadata indexed: {} objects", root.id, n),
                        Err(e) => eprintln!("[1c-search][{}] Metadata skipped: {}", root.id, e),
                    }
                }

                let root_result = if index::index_exists(&db_for_index) {
                    eprintln!("[1c-search][{}] Index found — running incremental sync...", root.id);
                    emit_search_status_json(
                        "syncing_index",
                        root_progress_base + 2,
                        &format!("Синхронизация индекса: {}", source_label),
                        total_symbols,
                        total_size,
                        last_built_at,
                    );
                    index::sync_index(&root.path, &db_for_index).map(|stats| stats.total_symbols)
                } else {
                    eprintln!("[1c-search][{}] No index found — starting full build...", root.id);
                    emit_search_status_json(
                        "building_index",
                        root_progress_base + 2,
                        &format!("Первичная индексация: {}", source_label),
                        total_symbols,
                        total_size,
                        last_built_at,
                    );
                    index::build_index(&root.path, &db_for_index)
                };

                match root_result {
                    Ok(sym_count) => {
                        total_symbols += sym_count;
                        total_size += db_size_mb(&db_for_index);
                        last_built_at = last_built_at.max(index::get_built_at(&db_for_index).unwrap_or(0));
                    }
                    Err(e) => {
                        eprintln!("[1c-search][{}] Index error: {}", root.id, e);
                        degraded_messages.push(format!("{}: {}", source_label, e));
                        let existing = index::symbol_count(&db_for_index);
                        if existing > 0 {
                            total_symbols += existing;
                            total_size += db_size_mb(&db_for_index);
                            last_built_at = last_built_at.max(index::get_built_at(&db_for_index).unwrap_or(0));
                        }
                    }
                }
            }

            if degraded_messages.is_empty() {
                eprintln!(
                    "SEARCH_STATUS:ready:{}:{:.2}:{}",
                    total_symbols, total_size, last_built_at
                );
                emit_search_status_json(
                    "ready",
                    100,
                    &format!("Индекс готов: {} источн.", total_roots),
                    total_symbols,
                    total_size,
                    last_built_at,
                );
            } else if total_symbols > 0 {
                emit_search_status_json(
                    "degraded",
                    100,
                    &format!("Часть индексов недоступна: {}", degraded_messages.join("; ")),
                    total_symbols,
                    total_size,
                    last_built_at,
                );
            } else {
                eprintln!("SEARCH_STATUS:unavailable:{}", degraded_messages.join("; "));
                emit_search_status_json(
                    "unavailable",
                    0,
                    &format!("Индексация не выполнена: {}", degraded_messages.join("; ")),
                    0,
                    0.0,
                    0,
                );
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
                let workspace_task = workspace.clone();
                let stdout_task = Arc::clone(&stdout);

                tokio::spawn(async move {
                    let result = handle_method(&method, &params, &workspace_task).await;

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
    workspace: &workspace::SearchWorkspace,
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
            tools::call_tool(tool_name, &arguments, workspace).await
        }
        "ping" => Ok(json!({})),
        _ => Err(format!("Method not found: {}", method)),
    }
}
