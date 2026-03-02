use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ignore::WalkBuilder;
use rayon::prelude::*;
use rusqlite::{params, Connection};

use crate::parser::bsl_ast;

pub struct SymbolMatch {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub is_export: bool,
}

/// Derive database path from config root.
/// Stored in AppData\com.mini-ai-1c\search-index\{hash}.db
pub fn get_db_path(config_root: &Path) -> PathBuf {
    let path_str = config_root.to_string_lossy();
    let hash = fnv_hash(&path_str);
    if let Some(data_dir) = dirs::data_dir() {
        let dir = data_dir.join("com.mini-ai-1c").join("search-index");
        let _ = fs::create_dir_all(&dir);
        dir.join(format!("{:016x}.db", hash))
    } else {
        config_root.join(".mcp-index").join("symbols.db")
    }
}

fn fnv_hash(s: &str) -> u64 {
    let mut hash: u64 = 14695981039346656037;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(1099511628211);
        hash ^= byte as u64;
    }
    hash
}

/// Initialize the database schema (creates all tables if they don't exist).
/// Safe to call multiple times — all statements use CREATE IF NOT EXISTS.
pub fn ensure_schema(db_path: &Path) -> Result<(), String> {
    init_db(db_path).map(|_| ()).map_err(|e| e.to_string())
}

fn init_db(db_path: &Path) -> Result<Connection, rusqlite::Error> {
    if let Some(parent) = db_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS symbols (
             id INTEGER PRIMARY KEY,
             name TEXT NOT NULL,
             name_lower TEXT NOT NULL,
             kind TEXT NOT NULL,
             file TEXT NOT NULL,
             start_line INTEGER NOT NULL,
             end_line INTEGER NOT NULL,
             is_export INTEGER NOT NULL DEFAULT 0
         );
         CREATE INDEX IF NOT EXISTS idx_name_lower ON symbols(name_lower);
         CREATE INDEX IF NOT EXISTS idx_file ON symbols(file);
         CREATE TABLE IF NOT EXISTS meta (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS objects (
             id INTEGER PRIMARY KEY,
             obj_type TEXT NOT NULL,
             name TEXT NOT NULL,
             name_lower TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_obj_name ON objects(name_lower);
         CREATE INDEX IF NOT EXISTS idx_obj_type ON objects(obj_type);
         CREATE TABLE IF NOT EXISTS object_items (
             id INTEGER PRIMARY KEY,
             object_id INTEGER NOT NULL,
             item_type TEXT NOT NULL,
             item_name TEXT NOT NULL,
             parent_section TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_items_obj ON object_items(object_id);
         CREATE TABLE IF NOT EXISTS indexed_files (
             filepath TEXT PRIMARY KEY,
             modified_at INTEGER NOT NULL
         );",
    )?;
    Ok(conn)
}

/// Check if index exists and has data.
pub fn index_exists(db_path: &Path) -> bool {
    if !db_path.exists() {
        return false;
    }
    if let Ok(conn) = Connection::open(db_path) {
        if let Ok(count) = conn.query_row(
            "SELECT COUNT(*) FROM symbols",
            [],
            |r| r.get::<_, i64>(0),
        ) {
            return count > 0;
        }
    }
    false
}

/// Get unix timestamp when index was last built (from meta table).
pub fn get_built_at(db_path: &Path) -> Option<u64> {
    let conn = Connection::open(db_path).ok()?;
    conn.query_row(
        "SELECT value FROM meta WHERE key = 'built_at'",
        [],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| s.parse::<u64>().ok())
}

/// Get the number of indexed symbols.
pub fn symbol_count(db_path: &Path) -> usize {
    if let Ok(conn) = Connection::open(db_path) {
        if let Ok(count) = conn.query_row(
            "SELECT COUNT(*) FROM symbols",
            [],
            |r| r.get::<_, i64>(0),
        ) {
            return count as usize;
        }
    }
    0
}

/// Extracted symbol data collected during parallel parse phase.
struct ParsedFile {
    rel_path: String,
    mtime: u64,
    symbols: Vec<crate::parser::bsl_ast::BslSymbol>,
}

/// Get unix mtime of a file in seconds, 0 if unavailable.
fn file_mtime(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Load the `indexed_files` table into a HashMap<rel_path, mtime>.
fn load_indexed_mtimes(conn: &Connection) -> std::collections::HashMap<String, u64> {
    let mut map = std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT filepath, modified_at FROM indexed_files") {
        let _ = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        }).map(|rows| {
            for row in rows.flatten() {
                map.insert(row.0, row.1);
            }
        });
    }
    map
}

/// Statistics returned from `sync_index`.
pub struct SyncStats {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub total_symbols: usize,
}

/// Incremental sync: only re-parse files that are new or have changed mtime.
/// Also removes symbols for deleted files.
/// Returns statistics of what changed.
pub fn sync_index(root: &Path, db_path: &Path) -> Result<SyncStats, String> {
    eprintln!("SEARCH_STATUS:syncing:0:Сравнение файлов...");

    let conn = init_db(db_path).map_err(|e| format!("Ошибка БД: {}", e))?;
    let indexed_mtimes = load_indexed_mtimes(&conn);

    // Scan filesystem — collect all current .bsl files with their mtime
    let root_owned = root.to_path_buf();
    let all_disk_files: Vec<(String, u64, PathBuf)> = WalkBuilder::new(root)
        .standard_filters(true)
        .follow_links(false)
        .build()
        .into_iter()
        .flatten()
        .filter(|e| {
            e.path().is_file()
                && e.path().extension().and_then(|x| x.to_str()) == Some("bsl")
        })
        .filter_map(|e| {
            let path = e.into_path();
            let rel = path
                .strip_prefix(&root_owned)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
            let mtime = file_mtime(&path);
            Some((rel, mtime, path))
        })
        .collect();

    let disk_set: std::collections::HashSet<String> =
        all_disk_files.iter().map(|(r, _, _)| r.clone()).collect();

    // Detect deleted files
    let deleted: Vec<String> = indexed_mtimes
        .keys()
        .filter(|k| !disk_set.contains(*k))
        .cloned()
        .collect();

    // Detect new / changed files
    let to_parse: Vec<(String, u64, PathBuf)> = all_disk_files
        .into_iter()
        .filter(|(rel, mtime, _)| {
            match indexed_mtimes.get(rel) {
                None => true,               // new file
                Some(&old) => *mtime > old, // changed file
            }
        })
        .collect();

    let added = to_parse.iter().filter(|(r, _, _)| !indexed_mtimes.contains_key(r)).count();
    let updated = to_parse.len() - added;

    eprintln!(
        "SEARCH_STATUS:syncing:10:+{}новых  ~{}изм  -{}удал",
        added, updated, deleted.len()
    );

    if deleted.is_empty() && to_parse.is_empty() {
        // Update built_at timestamp so UI knows we explicitly checked just now
        let now_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        if let Ok(conn) = Connection::open(db_path) {
            let _ = conn.execute(
                "INSERT INTO meta (key, value) VALUES ('built_at', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                [now_unix.to_string()],
            );
        }

        // Nothing to do — index is up-to-date
        let total_symbols = symbol_count(db_path);
        eprintln!("SEARCH_STATUS:syncing:100:Индекс актуален");
        return Ok(SyncStats { added: 0, updated: 0, removed: 0, total_symbols });
    }

    // ── Parallel parse of new/changed files ──────────────────────────────────
    let total_to_parse = to_parse.len();
    let processed = Arc::new(AtomicUsize::new(0));

    let parsed: Vec<ParsedFile> = to_parse
        .par_iter()
        .filter_map(|(rel_path, mtime, path)| {
            let mut buf = String::new();
            let mut file = fs::File::open(path).ok()?;
            file.read_to_string(&mut buf).ok()?;
            let symbols = bsl_ast::extract_symbols(&buf);
            let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
            if total_to_parse > 0 && done % (total_to_parse / 10).max(1) == 0 {
                let pct = done * 80 / total_to_parse + 10;
                eprintln!("SEARCH_STATUS:syncing:{}:Парсинг {}/{}", pct, done, total_to_parse);
            }
            Some(ParsedFile { rel_path: rel_path.clone(), mtime: *mtime, symbols })
        })
        .collect();

    // ── Serial phase: apply changes to SQLite ─────────────────────────────────
    eprintln!("SEARCH_STATUS:syncing:90:Запись изменений...");
    {
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

        // Remove deleted files' symbols
        for rel in &deleted {
            let _ = tx.execute("DELETE FROM symbols WHERE file = ?1", params![rel]);
            let _ = tx.execute("DELETE FROM indexed_files WHERE filepath = ?1", params![rel]);
        }

        // Replace symbols for changed/new files
        for pf in &parsed {
            let _ = tx.execute("DELETE FROM symbols WHERE file = ?1", params![pf.rel_path]);
            for sym in &pf.symbols {
                let _ = tx.execute(
                    "INSERT INTO symbols (name, name_lower, kind, file, start_line, end_line, is_export)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        sym.name,
                        sym.name.to_lowercase(),
                        sym.kind,
                        pf.rel_path,
                        sym.start_line,
                        sym.end_line,
                        sym.is_export as i32
                    ],
                );
            }
            let _ = tx.execute(
                "INSERT OR REPLACE INTO indexed_files (filepath, modified_at) VALUES (?1, ?2)",
                params![pf.rel_path, pf.mtime as i64],
            );
        }

        tx.commit().map_err(|e| e.to_string())?;
    }

    // Update built_at timestamp
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('built_at', ?1)",
        params![ts.to_string()],
    );

    let total_symbols = symbol_count(db_path);
    Ok(SyncStats {
        added,
        updated,
        removed: deleted.len(),
        total_symbols,
    })
}

/// Full (re)build: clear all symbols and re-index everything in parallel.
/// Also fills `indexed_files` with mtime for each file.
/// Use `sync_index` for incremental updates after initial build.
pub fn build_index(root: &Path, db_path: &Path) -> Result<usize, String> {
    eprintln!("SEARCH_STATUS:indexing:0:Сканирование файлов...");

    // Collect all .bsl file paths first to know total count
    let bsl_paths: Vec<(PathBuf, u64)> = WalkBuilder::new(root)
        .standard_filters(true)
        .follow_links(false)
        .build()
        .into_iter()
        .flatten()
        .filter(|e| {
            e.path().is_file()
                && e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    == Some("bsl")
        })
        .map(|e| {
            let path = e.into_path();
            let mtime = file_mtime(&path);
            (path, mtime)
        })
        .collect();

    let total_files = bsl_paths.len();
    if total_files == 0 {
        return Err("В директории не найдено BSL файлов".to_string());
    }

    eprintln!("SEARCH_STATUS:indexing:5:Парсинг {} файлов...", total_files);

    // ── Parallel phase: read + parse (CPU-bound, rayon thread pool) ──────────
    let processed = Arc::new(AtomicUsize::new(0));
    let root_owned = root.to_path_buf();

    let parsed_files: Vec<ParsedFile> = bsl_paths
        .par_iter()
        .filter_map(|(path, mtime)| {
            let mut buf = String::new();
            let mut file = fs::File::open(path).ok()?;
            file.read_to_string(&mut buf).ok()?;

            let rel_path = path
                .strip_prefix(&root_owned)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));

            let symbols = bsl_ast::extract_symbols(&buf);

            // Progress reporting ~every 10%
            let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
            let pct = done * 90 / total_files + 5; // range 5..95
            if done % (total_files / 10).max(1) == 0 {
                eprintln!(
                    "SEARCH_STATUS:indexing:{}:Парсинг {}/{} файлов",
                    pct, done, total_files
                );
            }

            Some(ParsedFile { rel_path, mtime: *mtime, symbols })
        })
        .collect();

    // ── Serial phase: batch INSERT into SQLite ────────────────────────────────
    eprintln!("SEARCH_STATUS:indexing:95:Запись в индекс...");

    let conn = init_db(db_path).map_err(|e| format!("Ошибка БД: {}", e))?;
    conn.execute("DELETE FROM symbols", []).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM indexed_files", []).map_err(|e| e.to_string())?;

    let mut total_symbols = 0usize;
    {
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        for pf in &parsed_files {
            for sym in &pf.symbols {
                let _ = tx.execute(
                    "INSERT INTO symbols (name, name_lower, kind, file, start_line, end_line, is_export)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        sym.name,
                        sym.name.to_lowercase(),
                        sym.kind,
                        pf.rel_path,
                        sym.start_line,
                        sym.end_line,
                        sym.is_export as i32
                    ],
                );
                total_symbols += 1;
            }
            // Record mtime for incremental sync
            let _ = tx.execute(
                "INSERT OR REPLACE INTO indexed_files (filepath, modified_at) VALUES (?1, ?2)",
                params![pf.rel_path, pf.mtime as i64],
            );
        }
        tx.commit().map_err(|e| e.to_string())?;
    }

    // Save build timestamp
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('built_at', ?1)",
        params![ts.to_string()],
    );

    Ok(total_symbols)
}

/// Query the index for symbols matching the query.
pub fn find_symbols(
    db_path: &Path,
    query: &str,
    exact: bool,
    limit: usize,
) -> Result<Vec<SymbolMatch>, String> {
    let conn = Connection::open(db_path).map_err(|e| format!("Ошибка БД: {}", e))?;
    let query_lower = query.to_lowercase();

    let (sql, pattern) = if exact {
        (
            "SELECT name, kind, file, start_line, end_line, is_export \
             FROM symbols WHERE name_lower = ?1 LIMIT ?2",
            query_lower,
        )
    } else {
        (
            "SELECT name, kind, file, start_line, end_line, is_export \
             FROM symbols WHERE name_lower LIKE ?1 LIMIT ?2",
            format!("%{}%", query_lower),
        )
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![pattern, limit as i64], |row| {
            Ok(SymbolMatch {
                name: row.get(0)?,
                kind: row.get(1)?,
                file: row.get(2)?,
                start_line: row.get::<_, u32>(3)?,
                end_line: row.get::<_, u32>(4)?,
                is_export: row.get::<_, i32>(5)? != 0,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for row in rows {
        if let Ok(r) = row {
            results.push(r);
        }
    }
    Ok(results)
}

/// Find which symbol (if any) contains the given line in the given file.
pub fn find_symbol_at_line(
    db_path: &Path,
    file: &str,
    line: u32,
) -> Option<SymbolMatch> {
    let conn = Connection::open(db_path).ok()?;
    conn.query_row(
        "SELECT name, kind, file, start_line, end_line, is_export \
         FROM symbols WHERE file = ?1 AND start_line <= ?2 AND end_line >= ?2 \
         LIMIT 1",
        params![file, line],
        |row| {
            Ok(SymbolMatch {
                name: row.get(0)?,
                kind: row.get(1)?,
                file: row.get(2)?,
                start_line: row.get::<_, u32>(3)?,
                end_line: row.get::<_, u32>(4)?,
                is_export: row.get::<_, i32>(5)? != 0,
            })
        },
    )
    .ok()
}

// ─── Metadata graph queries ────────────────────────────────────────────────

pub struct ObjectInfo {
    pub obj_type: String,
    pub name: String,
}

pub struct ObjectDetails {
    pub obj_type: String,
    pub name: String,
    pub attributes: Vec<String>,
    pub tabular_sections: Vec<(String, Vec<String>)>, // (section_name, [attr_names])
    pub forms: Vec<String>,
    pub commands: Vec<String>,
    pub modules: Vec<String>,
}

/// Check if metadata (objects table) has been built.
pub fn metadata_exists(db_path: &Path) -> bool {
    if let Ok(conn) = Connection::open(db_path) {
        if let Ok(count) = conn.query_row(
            "SELECT COUNT(*) FROM objects",
            [],
            |r| r.get::<_, i64>(0),
        ) {
            return count > 0;
        }
    }
    false
}

/// List all objects, optionally filtered by type and/or name substring.
pub fn list_objects(
    db_path: &Path,
    obj_type_filter: Option<&str>,
    name_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<ObjectInfo>, String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    // Build query dynamically based on filters
    let name_pattern = name_filter.map(|n| format!("%{}%", n.to_lowercase()));

    let (sql, boxed_params): (&str, Vec<Box<dyn rusqlite::ToSql>>) = match (obj_type_filter, name_pattern.as_deref()) {
        (Some(t), Some(n)) => (
            "SELECT obj_type, name FROM objects WHERE obj_type = ?1 AND name_lower LIKE ?2 ORDER BY name LIMIT ?3",
            vec![Box::new(t.to_string()), Box::new(n.to_string()), Box::new(limit as i64)],
        ),
        (Some(t), None) => (
            "SELECT obj_type, name FROM objects WHERE obj_type = ?1 ORDER BY name LIMIT ?2",
            vec![Box::new(t.to_string()), Box::new(limit as i64)],
        ),
        (None, Some(n)) => (
            "SELECT obj_type, name FROM objects WHERE name_lower LIKE ?1 ORDER BY obj_type, name LIMIT ?2",
            vec![Box::new(n.to_string()), Box::new(limit as i64)],
        ),
        (None, None) => (
            "SELECT obj_type, name FROM objects ORDER BY obj_type, name LIMIT ?1",
            vec![Box::new(limit as i64)],
        ),
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = boxed_params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(ObjectInfo {
                obj_type: row.get(0)?,
                name: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for row in rows.flatten() {
        result.push(row);
    }
    Ok(result)
}

/// Get full structure of an object by name (case-insensitive).
pub fn get_object_details(db_path: &Path, name_query: &str) -> Option<ObjectDetails> {
    let conn = Connection::open(db_path).ok()?;
    let name_lower = name_query.to_lowercase();

    // Find the object — try exact match first, then partial
    let (obj_type, obj_name, obj_id) = conn.query_row(
        "SELECT obj_type, name, id FROM objects WHERE name_lower = ?1 LIMIT 1",
        params![name_lower],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)),
    ).or_else(|_| conn.query_row(
        "SELECT obj_type, name, id FROM objects WHERE name_lower LIKE ?1 LIMIT 1",
        params![format!("%{}%", name_lower)],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)),
    )).ok()?;

    // Fetch all children
    let mut stmt = conn.prepare(
        "SELECT item_type, item_name, parent_section FROM object_items WHERE object_id = ?1 ORDER BY item_type, parent_section, item_name"
    ).ok()?;
    let children: Vec<(String, String, Option<String>)> = stmt
        .query_map(params![obj_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))
        })
        .ok()?
        .flatten()
        .collect();

    let mut attributes = Vec::new();
    let mut forms = Vec::new();
    let mut commands = Vec::new();
    let mut modules = Vec::new();
    let mut tab_section_attrs: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut tab_section_names: Vec<String> = Vec::new();

    for (item_type, item_name, parent) in children {
        match item_type.as_str() {
            "Attribute" => {
                if let Some(sec) = parent {
                    tab_section_attrs.entry(sec).or_default().push(item_name);
                } else {
                    attributes.push(item_name);
                }
            }
            "TabularSection" => {
                if !tab_section_names.contains(&item_name) {
                    tab_section_names.push(item_name);
                }
            }
            "Form" => forms.push(item_name),
            "Command" => commands.push(item_name),
            t if t.ends_with("Module") => modules.push(item_name),
            _ => {}
        }
    }

    let tabular_sections = tab_section_names
        .into_iter()
        .map(|sec| {
            let attrs = tab_section_attrs.remove(&sec).unwrap_or_default();
            (sec, attrs)
        })
        .collect();

    Some(ObjectDetails {
        obj_type,
        name: obj_name,
        attributes,
        tabular_sections,
        forms,
        commands,
        modules,
    })
}
