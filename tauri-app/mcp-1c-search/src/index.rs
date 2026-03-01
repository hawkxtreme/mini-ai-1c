use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
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

/// Build the symbol index by scanning all .bsl files under root.
/// Reports progress via stderr: SEARCH_STATUS:indexing:{pct}:{message}
pub fn build_index(root: &Path, db_path: &Path) -> Result<usize, String> {
    eprintln!("SEARCH_STATUS:indexing:0:Сканирование файлов...");

    // Collect all .bsl files first to know total count
    let bsl_files: Vec<_> = WalkBuilder::new(root)
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
        .collect();

    let total_files = bsl_files.len();
    if total_files == 0 {
        return Err("В директории не найдено BSL файлов".to_string());
    }

    let conn = init_db(db_path).map_err(|e| format!("Ошибка БД: {}", e))?;
    conn.execute("DELETE FROM symbols", [])
        .map_err(|e| e.to_string())?;

    let mut total_symbols = 0usize;
    let mut buf = String::new();
    let mut last_pct: u32 = 0;

    for (idx, entry) in bsl_files.iter().enumerate() {
        let path = entry.path();
        buf.clear();

        let mut file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        // Skip files that can't be read as UTF-8
        if file.read_to_string(&mut buf).is_err() {
            continue;
        }

        let rel_path = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));

        let symbols = bsl_ast::extract_symbols(&buf);
        for sym in &symbols {
            let _ = conn.execute(
                "INSERT INTO symbols (name, name_lower, kind, file, start_line, end_line, is_export)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    sym.name,
                    sym.name.to_lowercase(),
                    sym.kind,
                    rel_path,
                    sym.start_line,
                    sym.end_line,
                    sym.is_export as i32
                ],
            );
            total_symbols += 1;
        }

        let pct = ((idx + 1) * 100 / total_files) as u32;
        if pct != last_pct && (pct % 5 == 0 || pct >= 100) {
            eprintln!(
                "SEARCH_STATUS:indexing:{}:Проиндексировано {}/{} файлов",
                pct,
                idx + 1,
                total_files
            );
            last_pct = pct;
        }
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
