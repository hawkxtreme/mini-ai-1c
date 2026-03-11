use std::fs;
use std::io::Read; // Wait, actually I might not need Read if I use fs::read
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ignore::WalkBuilder;
use rayon::prelude::*;
use rusqlite::{params, Connection};

use crate::parser::bsl_ast;

/// Robustly read a file to string, handling UTF-8 (with BOM) and Windows-1251 fallback.
pub fn read_file_to_string_lossy(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Ok(String::new());
    }

    // Check for UTF-8 BOM
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(bytes[3..].to_vec())
            .map_err(|e| format!("Invalid UTF-8 after BOM: {}", e));
    }

    // Attempt UTF-8
    match String::from_utf8(bytes.clone()) {
        Ok(s) => Ok(s),
        Err(_) => {
            // Fallback to Windows-1251 manual decoding
            Ok(decode_windows_1251(&bytes))
        }
    }
}

fn decode_windows_1251(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            0..=127 => s.push(b as char),
            0x80 => s.push('\u{0402}'), // Ђ
            0x81 => s.push('\u{0403}'), // Ѓ
            0x82 => s.push('\u{201A}'), // ,
            0x83 => s.push('\u{0453}'), // ѓ
            0x84 => s.push('\u{201E}'), // ,,
            0x85 => s.push('\u{2026}'), // ...
            0x86 => s.push('\u{2020}'), // †
            0x87 => s.push('\u{2021}'), // ‡
            0x88 => s.push('\u{20AC}'), // €
            0x89 => s.push('\u{2030}'), // ‰
            0x8A => s.push('\u{0409}'), // Љ
            0x8B => s.push('\u{2039}'), // <
            0x8C => s.push('\u{040A}'), // Њ
            0x8D => s.push('\u{040B}'), // Ћ
            0x8E => s.push('\u{040C}'), // Ќ
            0x8F => s.push('\u{040F}'), // Џ
            0x90 => s.push('\u{0452}'), // ђ
            0x91 => s.push('\u{2018}'), // '
            0x92 => s.push('\u{2019}'), // '
            0x93 => s.push('\u{201C}'), // "
            0x94 => s.push('\u{201D}'), // "
            0x95 => s.push('\u{2022}'), // .
            0x96 => s.push('\u{2013}'), // -
            0x97 => s.push('\u{2014}'), // --
            0x98 => s.push('\u{0000}'), // undefined
            0x99 => s.push('\u{2122}'), // (TM)
            0x9A => s.push('\u{0459}'), // љ
            0x9B => s.push('\u{203A}'), // >
            0x9C => s.push('\u{045A}'), // њ
            0x9D => s.push('\u{045B}'), // ћ
            0x9E => s.push('\u{045C}'), // ќ
            0x9F => s.push('\u{045F}'), // џ
            0xA0 => s.push('\u{00A0}'), // NBSP
            0xA1 => s.push('\u{040E}'), // Ў
            0xA2 => s.push('\u{045E}'), // ў
            0xA3 => s.push('\u{0408}'), // Ј
            0xA4 => s.push('\u{00A4}'), // ¤
            0xA5 => s.push('\u{0490}'), // Ґ
            0xA6 => s.push('\u{00A6}'), // |
            0xA7 => s.push('\u{00A7}'), // §
            0xA8 => s.push('\u{0401}'), // Ё
            0xA9 => s.push('\u{00A9}'), // (C)
            0xAA => s.push('\u{0404}'), // Є
            0xAB => s.push('\u{00AB}'), // <<
            0xAC => s.push('\u{00AC}'), // -
            0xAD => s.push('\u{00AD}'), // soft hyphen
            0xAE => s.push('\u{00AE}'), // (R)
            0xAF => s.push('\u{0407}'), // Ї
            0xB0 => s.push('\u{00B0}'), // °
            0xB1 => s.push('\u{00B1}'), // +-
            0xB2 => s.push('\u{0406}'), // І
            0xB3 => s.push('\u{0456}'), // і
            0xB4 => s.push('\u{0491}'), // ґ
            0xB5 => s.push('\u{00B5}'), // mu
            0xB6 => s.push('\u{00B6}'), // paragraph
            0xB7 => s.push('\u{00B7}'), // .
            0xB8 => s.push('\u{0451}'), // ё
            0xB9 => s.push('\u{2116}'), // No.
            0xBA => s.push('\u{0454}'), // є
            0xBB => s.push('\u{00BB}'), // >>
            0xBC => s.push('\u{0458}'), // ј
            0xBD => s.push('\u{0405}'), // Ѕ
            0xBE => s.push('\u{0455}'), // ѕ
            0xBF => s.push('\u{0407}'), // Ї
            0xC0..=0xFF => s.push((0x0410 + (b as u32 - 0xC0)) as u8 as char), // mapping for A-Ya, a-ya
            // Wait, CP1251 mapping: 0xC0-0xDF is А-Я (0x0410-0x042F)
            // 0xE0-0xFF is а-я (0x0430-0x044F)
            // Correct simple math for 0xC0-0xFF:
            // return std::char::from_u32(0x0410 + (b - 0xC0) as u32).unwrap_or('?')
        }
    }
    // Re-doing the range mapping carefully:
    let mut s2 = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            0..=127 => s2.push(b as char),
            0xC0..=0xFF => s2.push(std::char::from_u32(0x0410 + (b as u32 - 0xC0)).unwrap_or('?')),
            0xA1 => s2.push('\u{040E}'), 0xA2 => s2.push('\u{045E}'), 0xA8 => s2.push('\u{0401}'), 0xB8 => s2.push('\u{0451}'),
            _ => {
                // Simplified: use basic mapping for others or just push as is if it fits
                s2.push(decode_one_cp1251(b));
            }
        }
    }
    s2
}

fn decode_one_cp1251(b: u8) -> char {
    match b {
        0x80 => '\u{0402}', 0x81 => '\u{0403}', 0x82 => '\u{201A}', 0x83 => '\u{0453}',
        0x84 => '\u{201E}', 0x85 => '\u{2026}', 0x86 => '\u{2020}', 0x87 => '\u{2021}',
        0x88 => '\u{20AC}', 0x89 => '\u{2030}', 0x8A => '\u{0409}', 0x8B => '\u{2039}',
        0x8C => '\u{040A}', 0x8D => '\u{040B}', 0x8E => '\u{040C}', 0x8F => '\u{040F}',
        0x90 => '\u{0452}', 0x91 => '\u{2018}', 0x92 => '\u{2019}', 0x93 => '\u{201C}',
        0x94 => '\u{201D}', 0x95 => '\u{2022}', 0x96 => '\u{2013}', 0x97 => '\u{2014}',
        0x99 => '\u{2122}', 0x9A => '\u{0459}', 0x9B => '\u{203A}', 0x9C => '\u{045A}',
        0x9D => '\u{045B}', 0x9E => '\u{045C}', 0x9F => '\u{045F}', 0xA0 => '\u{00A0}',
        0xA3 => '\u{0408}', 0xA4 => '\u{00A4}', 0xA5 => '\u{0490}', 0xA6 => '\u{00A6}',
        0xA7 => '\u{00A7}', 0xA9 => '\u{00A9}', 0xAA => '\u{0404}', 0xAB => '\u{00AB}',
        0xAC => '\u{00AC}', 0xAD => '\u{00AD}', 0xAE => '\u{00AE}', 0xAF => '\u{0407}',
        0xB0 => '\u{00B0}', 0xB1 => '\u{00B1}', 0xB2 => '\u{0406}', 0xB3 => '\u{0456}',
        0xB4 => '\u{0491}', 0xB5 => '\u{00B5}', 0xB6 => '\u{00B6}', 0xB7 => '\u{00B7}',
        0xB9 => '\u{2116}', 0xBA => '\u{0454}', 0xBB => '\u{00BB}', 0xBC => '\u{0458}',
        0xBD => '\u{0405}', 0xBE => '\u{0455}', 0xBF => '\u{0407}',
        _ => '?',
    }
}

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
         );
         CREATE TABLE IF NOT EXISTS calls (
             id INTEGER PRIMARY KEY,
             caller_file TEXT NOT NULL,
             caller_name TEXT NOT NULL,
             caller_name_lower TEXT NOT NULL,
             callee_name TEXT NOT NULL,
             callee_name_lower TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_calls_caller ON calls(caller_name_lower);
         CREATE INDEX IF NOT EXISTS idx_calls_callee ON calls(callee_name_lower);",
    )?;
    Ok(conn)
}

/// If the calls table is empty but symbols exist, this DB was indexed before call extraction
/// was added. Reset indexed_files so the next sync re-parses all files.
pub fn migrate_if_needed(db_path: &Path) {
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let sym_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
        .unwrap_or(0);
    let calls_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM calls", [], |r| r.get(0))
        .unwrap_or(0);
    if sym_count > 0 && calls_count == 0 {
        eprintln!("[1c-search] Migrating: resetting indexed_files to rebuild call graph...");
        let _ = conn.execute("DELETE FROM indexed_files", []);
    }
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

/// Save symbol/file/object/calls counts to meta table for fast stats retrieval.
fn save_stats_to_meta(conn: &Connection) {
    let sym: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap_or(0);
    let files: i64 = conn.query_row("SELECT COUNT(*) FROM indexed_files", [], |r| r.get(0)).unwrap_or(0);
    let objs: i64 = conn.query_row("SELECT COUNT(*) FROM objects", [], |r| r.get(0)).unwrap_or(0);
    let calls: i64 = conn.query_row("SELECT COUNT(*) FROM calls", [], |r| r.get(0)).unwrap_or(0);
    let _ = conn.execute_batch(&format!(
        "INSERT OR REPLACE INTO meta(key,value) VALUES ('stat_symbols','{sym}');
         INSERT OR REPLACE INTO meta(key,value) VALUES ('stat_files','{files}');
         INSERT OR REPLACE INTO meta(key,value) VALUES ('stat_objects','{objs}');
         INSERT OR REPLACE INTO meta(key,value) VALUES ('stat_calls','{calls}');"
    ));
}

/// Extracted symbol data collected during parallel parse phase.
struct ParsedFile {
    rel_path: String,
    mtime: u64,
    symbols: Vec<crate::parser::bsl_ast::BslSymbol>,
    /// true = brand new file, never indexed before (skip DELETE)
    is_new: bool,
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
            let buf = read_file_to_string_lossy(path).ok()?;
            let symbols = bsl_ast::extract_symbols(&buf);
            let is_new = !indexed_mtimes.contains_key(rel_path);
            let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
            if total_to_parse > 0 && done % (total_to_parse / 10).max(1) == 0 {
                let pct = done * 80 / total_to_parse + 10;
                eprintln!("SEARCH_STATUS:syncing:{}:Парсинг {}/{}", pct, done, total_to_parse);
            }
            Some(ParsedFile { rel_path: rel_path.clone(), mtime: *mtime, symbols, is_new })
        })
        .collect();

    // ── Serial phase: apply changes to SQLite ─────────────────────────────────
    eprintln!("SEARCH_STATUS:syncing:90:Запись изменений...");

    // Maximum write speed: memory journal (stays in RAM, no disk sync),
    // large page cache, drop all indexes (rebuild once at the end).
    let _ = conn.execute_batch(
        "PRAGMA journal_mode=MEMORY;
         PRAGMA synchronous=OFF;
         PRAGMA cache_size=-262144;
         PRAGMA temp_store=MEMORY;
         DROP INDEX IF EXISTS idx_name_lower;
         DROP INDEX IF EXISTS idx_file;
         DROP INDEX IF EXISTS idx_calls_caller;
         DROP INDEX IF EXISTS idx_calls_callee;"
    );

    let total_parsed = parsed.len();

    // Single transaction — with journal_mode=MEMORY there is no WAL growth.
    // Statements are dropped in inner block so tx can be committed (borrow rules).
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        // Remove deleted files
        if !deleted.is_empty() {
            let mut del_sym  = tx.prepare("DELETE FROM symbols WHERE file = ?1").map_err(|e| e.to_string())?;
            let mut del_call = tx.prepare("DELETE FROM calls WHERE caller_file = ?1").map_err(|e| e.to_string())?;
            let mut del_file = tx.prepare("DELETE FROM indexed_files WHERE filepath = ?1").map_err(|e| e.to_string())?;
            for rel in &deleted {
                let _ = del_sym.execute([rel]);
                let _ = del_call.execute([rel]);
                let _ = del_file.execute([rel]);
            }
        }

        // Prepare all INSERT/DELETE statements ONCE and reuse — avoids SQL re-compilation per row
        let mut del_sym  = tx.prepare("DELETE FROM symbols WHERE file = ?1").map_err(|e| e.to_string())?;
        let mut del_call = tx.prepare("DELETE FROM calls WHERE caller_file = ?1").map_err(|e| e.to_string())?;
        let mut ins_sym  = tx.prepare(
            "INSERT INTO symbols (name, name_lower, kind, file, start_line, end_line, is_export)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
        ).map_err(|e| e.to_string())?;
        let mut ins_call = tx.prepare(
            "INSERT INTO calls (caller_file, caller_name, caller_name_lower, callee_name, callee_name_lower)
             VALUES (?1, ?2, ?3, ?4, ?5)"
        ).map_err(|e| e.to_string())?;
        let mut ins_file = tx.prepare(
            "INSERT OR REPLACE INTO indexed_files (filepath, modified_at) VALUES (?1, ?2)"
        ).map_err(|e| e.to_string())?;

        for (i, pf) in parsed.iter().enumerate() {
            if i > 0 && i % 5000 == 0 {
                let pct = 90 + (i * 8 / total_parsed.max(1));
                eprintln!("SEARCH_STATUS:syncing:{}:Запись {}/{}...", pct, i, total_parsed);
            }
            // Only DELETE existing data for changed files — skip for brand new files
            if !pf.is_new {
                let _ = del_sym.execute([&pf.rel_path]);
                let _ = del_call.execute([&pf.rel_path]);
            }
            for sym in &pf.symbols {
                let name_lower = sym.name.to_lowercase();
                let _ = ins_sym.execute(params![
                    sym.name, name_lower, sym.kind,
                    pf.rel_path, sym.start_line, sym.end_line, sym.is_export as i32
                ]);
                for callee in &sym.calls {
                    let _ = ins_call.execute(params![
                        pf.rel_path, sym.name, name_lower,
                        callee, callee.to_lowercase()
                    ]);
                }
            }
            let _ = ins_file.execute(params![pf.rel_path, pf.mtime as i64]);
        }
        // statements dropped here — tx borrow ends
    }
    tx.commit().map_err(|e| e.to_string())?;

    // Recreate indexes and switch back to WAL
    eprintln!("SEARCH_STATUS:syncing:98:Создание индексов...");
    let _ = conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_name_lower ON symbols(name_lower);
         CREATE INDEX IF NOT EXISTS idx_file ON symbols(file);
         CREATE INDEX IF NOT EXISTS idx_calls_caller ON calls(caller_name_lower);
         CREATE INDEX IF NOT EXISTS idx_calls_callee ON calls(callee_name_lower);
         PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;"
    );

    // Update built_at timestamp
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('built_at', ?1)",
        params![ts.to_string()],
    );

    // Cache counts so stats tool is O(1) instead of COUNT(*)
    save_stats_to_meta(&conn);

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
            let buf = read_file_to_string_lossy(path).ok()?;

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

            Some(ParsedFile { rel_path, mtime: *mtime, symbols, is_new: true })
        })
        .collect();

    // ── Serial phase: batch INSERT into SQLite ────────────────────────────────
    eprintln!("SEARCH_STATUS:indexing:95:Запись в индекс...");

    let conn = init_db(db_path).map_err(|e| format!("Ошибка БД: {}", e))?;

    // Maximum write speed: memory journal, no sync, large cache, no indexes during insert
    let _ = conn.execute_batch(
        "PRAGMA journal_mode=MEMORY;
         PRAGMA synchronous=OFF;
         PRAGMA cache_size=-262144;
         PRAGMA temp_store=MEMORY;
         DELETE FROM symbols;
         DELETE FROM indexed_files;
         DELETE FROM calls;
         DROP INDEX IF EXISTS idx_name_lower;
         DROP INDEX IF EXISTS idx_file;
         DROP INDEX IF EXISTS idx_calls_caller;
         DROP INDEX IF EXISTS idx_calls_callee;"
    );

    let mut total_symbols = 0usize;
    let total_parsed = parsed_files.len();

    // Single transaction + prepared statements reused for every row.
    // Statements dropped in inner block so tx can be committed.
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        let mut ins_sym = tx.prepare(
            "INSERT INTO symbols (name, name_lower, kind, file, start_line, end_line, is_export)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
        ).map_err(|e| e.to_string())?;
        let mut ins_call = tx.prepare(
            "INSERT INTO calls (caller_file, caller_name, caller_name_lower, callee_name, callee_name_lower)
             VALUES (?1, ?2, ?3, ?4, ?5)"
        ).map_err(|e| e.to_string())?;
        let mut ins_file = tx.prepare(
            "INSERT OR REPLACE INTO indexed_files (filepath, modified_at) VALUES (?1, ?2)"
        ).map_err(|e| e.to_string())?;

        for (i, pf) in parsed_files.iter().enumerate() {
            if i > 0 && i % 5000 == 0 {
                eprintln!("SEARCH_STATUS:indexing:95:Запись {}/{}...", i, total_parsed);
            }
            for sym in &pf.symbols {
                let name_lower = sym.name.to_lowercase();
                let _ = ins_sym.execute(params![
                    sym.name, name_lower, sym.kind,
                    pf.rel_path, sym.start_line, sym.end_line, sym.is_export as i32
                ]);
                for callee in &sym.calls {
                    let _ = ins_call.execute(params![
                        pf.rel_path, sym.name, name_lower,
                        callee, callee.to_lowercase()
                    ]);
                }
                total_symbols += 1;
            }
            let _ = ins_file.execute(params![pf.rel_path, pf.mtime as i64]);
        }
        // statements dropped here
    }
    tx.commit().map_err(|e| e.to_string())?;

    // Recreate indexes and switch back to WAL
    eprintln!("SEARCH_STATUS:indexing:98:Создание индексов...");
    let _ = conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_name_lower ON symbols(name_lower);
         CREATE INDEX IF NOT EXISTS idx_file ON symbols(file);
         CREATE INDEX IF NOT EXISTS idx_calls_caller ON calls(caller_name_lower);
         CREATE INDEX IF NOT EXISTS idx_calls_callee ON calls(callee_name_lower);
         PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;"
    );

    // Save build timestamp
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('built_at', ?1)",
        params![ts.to_string()],
    );

    // Cache counts so stats tool is O(1) instead of COUNT(*)
    save_stats_to_meta(&conn);

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

    if exact {
        let mut stmt = conn
            .prepare(
                "SELECT name, kind, file, start_line, end_line, is_export \
                 FROM symbols WHERE name_lower = ?1 LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;
        return collect_symbol_rows(&mut stmt, &query_lower, limit);
    }

    // Two-phase substring search to avoid slow full-table scans:
    //
    // Phase 1: prefix match  — `name_lower LIKE 'query%'`
    //   Uses the idx_name_lower B-tree index → O(log n + k), fast on cold HDD.
    //   Covers the common case: user types the start of a function name.
    //
    // Phase 2: mid-string match — `name_lower LIKE '%query%'`
    //   Full table scan, O(n). Only runs when Phase 1 produced fewer than `limit` results,
    //   and only fetches the remaining gap (deduplicating Phase 1 results).
    //   On cold HDD this is slow (10-12s for 642K rows), but it's the minority case.

    let prefix_pattern = format!("{}%", query_lower);
    let mut stmt = conn
        .prepare(
            "SELECT name, kind, file, start_line, end_line, is_export \
             FROM symbols WHERE name_lower LIKE ?1 LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let mut results = collect_symbol_rows(&mut stmt, &prefix_pattern, limit)?;

    if results.len() < limit {
        // Phase 2: find mid-string matches not already returned by prefix search
        let mid_pattern = format!("%{}%", query_lower);
        let remaining = limit - results.len();
        let prefix_set: std::collections::HashSet<String> =
            results.iter().map(|r| r.name.to_lowercase()).collect();
        let mut stmt2 = conn
            .prepare(
                "SELECT name, kind, file, start_line, end_line, is_export \
                 FROM symbols WHERE name_lower LIKE ?1 AND name_lower NOT LIKE ?2 LIMIT ?3",
            )
            .map_err(|e| e.to_string())?;
        let extra = stmt2
            .query_map(
                params![mid_pattern, prefix_pattern, remaining as i64],
                symbol_row_mapper,
            )
            .map_err(|e| e.to_string())?;
        for row in extra.flatten() {
            if !prefix_set.contains(&row.name.to_lowercase()) {
                results.push(row);
            }
        }
    }

    Ok(results)
}

fn symbol_row_mapper(row: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolMatch> {
    Ok(SymbolMatch {
        name: row.get(0)?,
        kind: row.get(1)?,
        file: row.get(2)?,
        start_line: row.get::<_, u32>(3)?,
        end_line: row.get::<_, u32>(4)?,
        is_export: row.get::<_, i32>(5)? != 0,
    })
}

fn collect_symbol_rows(
    stmt: &mut rusqlite::Statement<'_>,
    pattern: &str,
    limit: usize,
) -> Result<Vec<SymbolMatch>, String> {
    let rows = stmt
        .query_map(params![pattern, limit as i64], symbol_row_mapper)
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

/// Return distinct file paths that declare symbols matching `query_lower` (LIKE %query%).
/// Used by index-guided search in `search_code`: instead of scanning all 25K files,
/// the caller first greps only these "hot" files where the symbol is likely to appear.
pub fn find_files_by_symbol_query(db_path: &Path, query_lower: &str, limit: usize) -> Vec<String> {
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let pattern = format!("%{}%", query_lower);
    let mut stmt = match conn.prepare(
        "SELECT DISTINCT file FROM symbols WHERE name_lower LIKE ?1 LIMIT ?2",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map(params![pattern, limit as i64], |row| row.get(0))
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
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

/// Returns true if metadata exists AND has at least one attribute/tabular section.
/// Used to detect stale metadata (objects indexed but no attributes — ConfigDumpInfo.xml was absent).
pub fn metadata_has_items(db_path: &Path) -> bool {
    if let Ok(conn) = Connection::open(db_path) {
        if let Ok(count) = conn.query_row(
            "SELECT COUNT(*) FROM object_items",
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

// ─── Call graph queries ────────────────────────────────────────────────────

pub struct CallerInfo {
    pub name: String,
    pub file: String,
    pub start_line: u32,
}

pub struct FunctionContext {
    pub function: SymbolMatch,
    pub calls: Vec<String>,
    pub called_by: Vec<CallerInfo>,
}

/// Get call graph context for a function: what it calls and who calls it.
pub fn get_function_context(db_path: &Path, function_name: &str) -> Option<FunctionContext> {
    let conn = Connection::open(db_path).ok()?;
    let name_lower = function_name.to_lowercase();

    // Find the function symbol (exact first, then prefix)
    let function = conn.query_row(
        "SELECT name, kind, file, start_line, end_line, is_export \
         FROM symbols WHERE name_lower = ?1 LIMIT 1",
        params![name_lower],
        |row| Ok(SymbolMatch {
            name: row.get(0)?,
            kind: row.get(1)?,
            file: row.get(2)?,
            start_line: row.get::<_, u32>(3)?,
            end_line: row.get::<_, u32>(4)?,
            is_export: row.get::<_, i32>(5)? != 0,
        }),
    ).or_else(|_| conn.query_row(
        "SELECT name, kind, file, start_line, end_line, is_export \
         FROM symbols WHERE name_lower LIKE ?1 LIMIT 1",
        params![format!("{}%", name_lower)],
        |row| Ok(SymbolMatch {
            name: row.get(0)?,
            kind: row.get(1)?,
            file: row.get(2)?,
            start_line: row.get::<_, u32>(3)?,
            end_line: row.get::<_, u32>(4)?,
            is_export: row.get::<_, i32>(5)? != 0,
        }),
    )).ok()?;

    let resolved_name_lower = function.name.to_lowercase();

    // What does this function call?
    let mut calls_stmt = conn.prepare(
        "SELECT DISTINCT callee_name FROM calls WHERE caller_name_lower = ?1 ORDER BY callee_name"
    ).ok()?;
    let calls: Vec<String> = calls_stmt
        .query_map(params![resolved_name_lower], |row| row.get(0))
        .ok()?
        .flatten()
        .collect();

    // Who calls this function? (limit 50 to avoid huge responses)
    let mut callers_stmt = conn.prepare(
        "SELECT DISTINCT c.caller_name, c.caller_file, s.start_line \
         FROM calls c \
         LEFT JOIN symbols s ON s.name_lower = c.caller_name_lower AND s.file = c.caller_file \
         WHERE c.callee_name_lower = ?1 \
         ORDER BY c.caller_file, c.caller_name \
         LIMIT 50"
    ).ok()?;
    let called_by: Vec<CallerInfo> = callers_stmt
        .query_map(params![resolved_name_lower], |row| {
            Ok(CallerInfo {
                name: row.get(0)?,
                file: row.get(1)?,
                start_line: row.get::<_, Option<u32>>(2)?.unwrap_or(0),
            })
        })
        .ok()?
        .flatten()
        .collect();

    Some(FunctionContext { function, calls, called_by })
}

/// List all functions in a module matching the given path substring.
pub fn get_module_functions(db_path: &Path, module_path: &str, limit: usize) -> Vec<SymbolMatch> {
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let pattern = format!("%{}%", module_path.replace('\\', "/"));
    let mut stmt = match conn.prepare(
        "SELECT name, kind, file, start_line, end_line, is_export \
         FROM symbols WHERE file LIKE ?1 \
         ORDER BY start_line LIMIT ?2"
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map(params![pattern, limit as i64], symbol_row_mapper)
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
}

// ─── Stats ────────────────────────────────────────────────────────────────

pub struct IndexStats {
    pub symbol_count: usize,
    pub file_count: usize,
    pub object_count: usize,
    pub calls_count: usize,
    pub built_at: Option<u64>,
    pub db_size_mb: f64,
}

pub fn get_index_stats(db_path: &Path) -> IndexStats {
    let db_size_mb = std::fs::metadata(db_path)
        .map(|m| m.len() as f64 / 1024.0 / 1024.0)
        .unwrap_or(0.0);

    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return IndexStats { symbol_count: 0, file_count: 0, object_count: 0, calls_count: 0, built_at: None, db_size_mb },
    };

    // Try reading cached counts from meta table (written at build/sync time) — O(1)
    let read_meta_int = |key: &str| -> Option<usize> {
        conn.query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [key],
            |r| r.get::<_, String>(0),
        ).ok().and_then(|s| s.parse::<usize>().ok())
    };
    // If any cached value is missing, compute all 4 via COUNT and save to meta (one-time migration).
    let needs_migration = read_meta_int("stat_symbols").is_none();
    let symbol_count = if needs_migration {
        conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    } else {
        read_meta_int("stat_symbols").unwrap_or(0)
    };
    let file_count = if needs_migration {
        conn.query_row("SELECT COUNT(*) FROM indexed_files", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    } else {
        read_meta_int("stat_files").unwrap_or(0)
    };
    let object_count = if needs_migration {
        conn.query_row("SELECT COUNT(*) FROM objects", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    } else {
        read_meta_int("stat_objects").unwrap_or(0)
    };
    let calls_count = if needs_migration {
        conn.query_row("SELECT COUNT(*) FROM calls", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    } else {
        read_meta_int("stat_calls").unwrap_or(0)
    };
    let built_at = conn.query_row(
        "SELECT value FROM meta WHERE key = 'built_at'", [],
        |r| r.get::<_, String>(0),
    ).ok().and_then(|s| s.parse::<u64>().ok());

    // One-time migration: write computed counts to meta for future O(1) reads
    if needs_migration {
        let _ = conn.execute_batch(&format!(
            "INSERT OR REPLACE INTO meta(key,value) VALUES ('stat_symbols','{symbol_count}');
             INSERT OR REPLACE INTO meta(key,value) VALUES ('stat_files','{file_count}');
             INSERT OR REPLACE INTO meta(key,value) VALUES ('stat_objects','{object_count}');
             INSERT OR REPLACE INTO meta(key,value) VALUES ('stat_calls','{calls_count}');"
        ));
    }

    IndexStats { symbol_count, file_count, object_count, calls_count, built_at, db_size_mb }
}

/// Get full structure of an object by name (case-insensitive).
pub fn get_object_details(db_path: &Path, name_query: &str) -> Option<ObjectDetails> {
    let conn = Connection::open(db_path).ok()?;
    
    // Handle qualified names like "Catalog.Agent" or "CommonModule.РаботаСФайлами"
    let (obj_type_filter, name_filter) = if let Some(dot_pos) = name_query.find('.') {
        let t = &name_query[..dot_pos];
        let n = &name_query[dot_pos + 1..];
        (Some(t.to_string()), n.to_string())
    } else {
        (None, name_query.to_string())
    };

    let name_lower = name_filter.to_lowercase();

    // Find the object — try exact match first
    let query = if let Some(ref _t) = obj_type_filter {
        "SELECT obj_type, name, id FROM objects WHERE name_lower = ?1 AND obj_type LIKE ?2 LIMIT 1"
    } else {
        "SELECT obj_type, name, id FROM objects WHERE name_lower = ?1 LIMIT 1"
    };

    let res = if let Some(ref t) = obj_type_filter {
        conn.query_row(query, params![name_lower, t], 
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))
    } else {
        conn.query_row(query, params![name_lower], 
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))
    };

    let (obj_type, obj_name, obj_id) = res.or_else(|_| {
        let like_pattern = format!("%{}%", name_lower);
        if let Some(ref t) = obj_type_filter {
            conn.query_row(
                "SELECT obj_type, name, id FROM objects WHERE name_lower LIKE ?1 AND obj_type LIKE ?2 LIMIT 1",
                params![like_pattern, t],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            )
        } else {
            conn.query_row(
                "SELECT obj_type, name, id FROM objects WHERE name_lower LIKE ?1 LIMIT 1",
                params![like_pattern],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            )
        }
    }).ok()?;

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
