use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::{json, Value};

mod config;
mod search;
mod tools;
mod parser;
mod index;
mod metadata;

use config::ConfigEntry;

/// Returns SQLite DB file size in MB (0.0 if not found).
pub fn db_size_mb(path: &std::path::Path) -> f64 {
    std::fs::metadata(path)
        .map(|m| m.len() as f64 / 1024.0 / 1024.0)
        .unwrap_or(0.0)
}

/// Emit combined SEARCH_STATUS after all configs finish indexing.
/// g = (total_symbols, total_db_size_mb, max_built_at, finished_count, error_count)
fn emit_combined_status(g: &(u64, f64, u64, usize, usize)) {
    let (total_sym, total_mb, built_at, finished, errors) = g;
    if *finished == 0 || (*errors == *finished) {
        eprintln!("SEARCH_STATUS:unavailable:Ошибка индексации всех конфигураций");
    } else {
        eprintln!(
            "SEARCH_STATUS:ready:{}:{:.2}:{}",
            total_sym, total_mb, built_at
        );
    }
}

#[tokio::main]
async fn main() {
    // ── Parse configs from env ───────────────────────────────────────────────
    let configs: Vec<ConfigEntry> = {
        let raw = std::env::var("ONEC_CONFIGS").unwrap_or_default();
        if raw.trim().is_empty() {
            // Fallback to legacy single-config env var
            let config_path_str = std::env::var("ONEC_CONFIG_PATH").unwrap_or_default();
            if config_path_str.is_empty() {
                vec![]
            } else {
                vec![ConfigEntry {
                    id: "default".to_string(),
                    path: config_path_str,
                    role: "main".to_string(),
                    extends: None,
                    name: None,
                    onec_uuid: None,
                    alias: None,
                }]
            }
        } else {
            match serde_json::from_str::<Vec<ConfigEntry>>(&raw) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[1c-search] Failed to parse ONEC_CONFIGS: {}", e);
                    vec![]
                }
            }
        }
    };

    // ── Validate paths and build (entry, db_path) list ───────────────────────
    let mut valid_configs: Vec<(ConfigEntry, PathBuf)> = Vec::new();

    for entry in &configs {
        if entry.path.is_empty() {
            continue;
        }
        let p = PathBuf::from(&entry.path);
        if p.exists() && p.is_dir() {
            let db = index::get_db_path(&p);
            valid_configs.push((entry.clone(), db));
        } else {
            eprintln!("[1c-search] Config '{}' path not found: {}", entry.id, entry.path);
        }
    }

    // ── Report preliminary status ────────────────────────────────────────────
    if valid_configs.is_empty() {
        if configs.is_empty() {
            eprintln!("SEARCH_STATUS:unavailable:Путь к конфигурации не задан");
        } else {
            eprintln!("SEARCH_STATUS:unavailable:Директория не найдена: {}", configs.first().map(|c| c.path.as_str()).unwrap_or(""));
        }
        // No configs to index — nothing more to do.
    }

    // ── Background indexing for all configs ──────────────────────────────────
    // Shared aggregator: (total_symbols, total_db_size_mb, max_built_at, finished_count, error_count)
    let total_configs = valid_configs.len();
    let aggregator: Arc<std::sync::Mutex<(u64, f64, u64, usize, usize)>> =
        Arc::new(std::sync::Mutex::new((0, 0.0, 0, 0, 0)));

    for (entry, db_for_index) in &valid_configs {
        let root = PathBuf::from(&entry.path);
        let db = db_for_index.clone();
        let entry_id = entry.id.clone();
        let agg = Arc::clone(&aggregator);

        let _ = tokio::task::spawn_blocking(move || {
            if let Err(e) = index::ensure_schema(&db) {
                eprintln!("[1c-search] Schema init failed ({}): {}", entry_id, e);
                let mut g = agg.lock().unwrap();
                g.3 += 1; // finished
                g.4 += 1; // error
                if g.3 == total_configs {
                    emit_combined_status(&g);
                }
                return;
            }

            index::migrate_if_needed(&db);

            let needs_metadata = !index::metadata_exists(&db)
                || (index::metadata_exists(&db) && !index::metadata_has_items(&db));
            if needs_metadata {
                match metadata::build_metadata(&root, &db) {
                    Ok((n, cfg_name, cfg_uuid)) => {
                        eprintln!("[1c-search] Metadata indexed ({}): {} objects", entry_id, n);
                        index::save_config_identity(&db, cfg_name.as_deref(), cfg_uuid.as_deref());
                    }
                    Err(e) => eprintln!("[1c-search] Metadata skipped ({}): {}", entry_id, e),
                }
            }

            let (sym_count, built_at, had_error) = if index::index_exists(&db) {
                eprintln!("[1c-search] Index found ({}) — running incremental sync...", entry_id);
                match index::sync_index(&root, &db) {
                    Ok(stats) => {
                        eprintln!(
                            "[1c-search] Sync done ({}): +{} ~{} -{} total={}",
                            entry_id, stats.added, stats.updated, stats.removed, stats.total_symbols
                        );
                        let built_at = index::get_built_at(&db).unwrap_or(0);
                        (stats.total_symbols as u64, built_at, false)
                    }
                    Err(e) => {
                        eprintln!("[1c-search] Sync error ({}): {}", entry_id, e);
                        (0, 0, true)
                    }
                }
            } else {
                eprintln!("[1c-search] No index found ({}) — starting full build...", entry_id);
                match index::build_index(&root, &db) {
                    Ok(sym_count) => {
                        let built_at = index::get_built_at(&db).unwrap_or(0);
                        (sym_count as u64, built_at, false)
                    }
                    Err(e) => {
                        eprintln!("[1c-search] Build error ({}): {}", entry_id, e);
                        (0, 0, true)
                    }
                }
            };

            let size = db_size_mb(&db);
            {
                let mut g = agg.lock().unwrap();
                g.0 += sym_count;
                g.1 += size;
                if built_at > g.2 { g.2 = built_at; }
                g.3 += 1; // finished
                if had_error { g.4 += 1; }
                if g.3 == total_configs {
                    emit_combined_status(&g);
                }
            }
        });
    }

    // ── JSON-RPC event loop ──────────────────────────────────────────────────
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let stdout = Arc::new(tokio::sync::Mutex::new(stdout));
    let configs_arc: Arc<Vec<(ConfigEntry, PathBuf)>> = Arc::new(valid_configs);

    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
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

                let id = match request.get("id") {
                    Some(id) => id.clone(),
                    None => continue,
                };

                let method = request["method"].as_str().unwrap_or("").to_string();
                let params = request.get("params").cloned().unwrap_or(json!({}));

                let stdout_task = Arc::clone(&stdout);
                let configs_task = Arc::clone(&configs_arc);

                tokio::spawn(async move {
                    let result = handle_method(&method, &params, &configs_task).await;

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
    configs: &[(ConfigEntry, PathBuf)],
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
            tools::call_tool(tool_name, &arguments, configs).await
        }
        "ping" => Ok(json!({})),
        _ => Err(format!("Method not found: {}", method)),
    }
}
