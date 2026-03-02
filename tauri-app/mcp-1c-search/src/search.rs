use ignore::WalkBuilder;
use regex::Regex;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub struct SearchResult {
    pub file: String,
    pub line: u32,
    pub snippet: String,
}

/// Compile a search pattern: literal case-insensitive or regex.
fn compile_pattern(query: &str, use_regex: bool) -> Option<Regex> {
    if use_regex {
        match Regex::new(query) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("[1c-search] Invalid regex '{}': {}", query, e);
                None
            }
        }
    } else {
        match Regex::new(&format!("(?i){}", regex::escape(query))) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("[1c-search] Regex build error: {}", e);
                None
            }
        }
    }
}

/// Search for `query` in .bsl and .xml files under `root` (or `root/sub_path` if given).
/// `use_regex` — treat query as regex; otherwise literal case-insensitive.
/// `sub_path` — optional relative sub-directory to restrict the search scope.
///
/// BSL-first, two-pass streaming:
/// - Pass 1: streams `.bsl` files, stops as soon as `limit` results are found.
/// - Pass 2: streams `.xml` files — only entered if Pass 1 didn't fill the limit.
///
/// Critical: does NOT collect all file paths upfront. On large configs (25K+ files)
/// on a cold HDD, collecting all metadata first would take 5-10 minutes.
/// Two-pass streaming means we stop reading as soon as we have enough results.
pub fn search_code(
    root: &Path,
    sub_path: Option<&Path>,
    query: &str,
    use_regex: bool,
    limit: usize,
) -> Vec<SearchResult> {
    let pattern = match compile_pattern(query, use_regex) {
        Some(p) => p,
        None => return vec![],
    };

    let search_root = match sub_path {
        Some(sub) => {
            let p = root.join(sub);
            if !p.exists() {
                eprintln!("[1c-search] Scope path not found: {}", p.display());
                return vec![];
            }
            p
        }
        None => root.to_path_buf(),
    };

    let mut results = Vec::new();

    // Pass 1: BSL only — streaming, early exit at limit
    'bsl: for entry in WalkBuilder::new(&search_root)
        .standard_filters(true)
        .follow_links(false)
        .build()
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("bsl") {
            continue;
        }
        for r in search_file(path, &pattern, root) {
            results.push(r);
            if results.len() >= limit {
                break 'bsl;
            }
        }
    }

    // Pass 2: XML only — skipped entirely if BSL already filled the limit
    if results.len() < limit {
        'xml: for entry in WalkBuilder::new(&search_root)
            .standard_filters(true)
            .follow_links(false)
            .build()
            .flatten()
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("xml") {
                continue;
            }
            for r in search_file(path, &pattern, root) {
                results.push(r);
                if results.len() >= limit {
                    break 'xml;
                }
            }
        }
    }

    results
}

/// Search for `query` only in the specified set of files (given as relative paths from `root`).
/// Used by index-guided search: SQLite provides candidate files, we grep only those.
pub fn search_code_in_file_set(
    root: &Path,
    rel_files: &[String],
    query: &str,
    use_regex: bool,
    limit: usize,
) -> Vec<SearchResult> {
    let pattern = match compile_pattern(query, use_regex) {
        Some(p) => p,
        None => return vec![],
    };

    let mut results = Vec::new();
    for rel_file in rel_files {
        let abs_path = root.join(rel_file);
        if !abs_path.is_file() {
            continue;
        }
        for r in search_file(&abs_path, &pattern, root) {
            results.push(r);
            if results.len() >= limit {
                return results;
            }
        }
    }
    results
}

fn search_file(path: &Path, pattern: &Regex, root: &Path) -> Vec<SearchResult> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let rel_path = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));

    let reader = BufReader::new(file);
    let mut results = Vec::new();

    for (idx, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if pattern.is_match(&line) {
            results.push(SearchResult {
                file: rel_path.clone(),
                line: (idx + 1) as u32,
                snippet: line,
            });
        }
    }

    results
}

/// Return `radius` lines above and below `target_line` (1-based).
pub fn get_file_context(path: &Path, target_line: usize, radius: usize) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("Не удалось открыть файл: {}", e))?;
    let lines: Vec<String> = BufReader::new(file)
        .lines()
        .map(|l| l.unwrap_or_default())
        .collect();

    let total = lines.len();
    if total == 0 {
        return Err("Файл пуст".to_string());
    }

    let target_idx = target_line.saturating_sub(1);
    if target_idx >= total {
        return Err(format!(
            "Строка {} не найдена (файл содержит {} строк)",
            target_line, total
        ));
    }

    let start = target_idx.saturating_sub(radius);
    let end = (target_idx + radius + 1).min(total);

    let mut out = format!("// {}:{}\n", path.display(), target_line);
    for (i, content) in lines[start..end].iter().enumerate() {
        let num = start + i + 1;
        let marker = if num == target_line { "→" } else { " " };
        out.push_str(&format!("{} {:4} | {}\n", marker, num, content));
    }

    Ok(out)
}

/// Per-file match summary used by impact_analysis.
pub struct FileHits {
    pub file: String,
    pub count: usize,
    pub examples: Vec<(u32, String)>, // (line_no, snippet)
}

/// Scan all `.bsl`/`.xml` files under `root` and return a per-file hit summary.
///
/// Unlike `search_code` which collects individual line matches up to a fixed total,
/// this function stops after finding `max_files` files that contain at least one match.
/// It collects up to `examples_per_file` example lines per file.
/// Results are sorted by match count descending.
///
/// This is the correct approach for impact_analysis: for widely-used symbols that
/// appear in hundreds of files, the old limit=500 approach had to scan ALL files;
/// this approach stops after `max_files` matched files, which is O(matched_files)
/// instead of O(total_files).
pub fn search_files_summary(
    root: &Path,
    query: &str,
    use_regex: bool,
    max_files: usize,
    examples_per_file: usize,
) -> Vec<FileHits> {
    let pattern = if use_regex {
        match Regex::new(query) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[1c-search] Invalid regex '{}': {}", query, e);
                return vec![];
            }
        }
    } else {
        match Regex::new(&format!("(?i){}", regex::escape(query))) {
            Ok(r) => r,
            Err(_) => return vec![],
        }
    };

    let walker = WalkBuilder::new(root)
        .standard_filters(true)
        .follow_links(false)
        .build();

    let mut results: Vec<FileHits> = Vec::new();

    for entry in walker {
        if results.len() >= max_files {
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "bsl" && ext != "xml" {
            continue;
        }

        let rel_path = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));

        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };

        let reader = BufReader::new(file);
        let mut count = 0usize;
        let mut examples: Vec<(u32, String)> = Vec::new();

        for (idx, line) in reader.lines().enumerate() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            if pattern.is_match(&line) {
                count += 1;
                if examples.len() < examples_per_file {
                    examples.push(((idx + 1) as u32, line));
                }
            }
        }

        if count > 0 {
            results.push(FileHits { file: rel_path, count, examples });
        }
    }

    results.sort_by(|a, b| b.count.cmp(&a.count));
    results
}

/// Считает кол-во конфигурационных файлов (`.bsl`, `.xml`) и возвращает `(count, size_in_mb)`.
pub fn count_files_and_size(root: &Path) -> (usize, f64) {
    let mut count = 0;
    let mut size_bytes = 0;

    let walker = WalkBuilder::new(root)
        .standard_filters(true)
        .follow_links(false)
        .build();

    for entry in walker.into_iter().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext == "bsl" || ext == "xml" {
            count += 1;
            if let Ok(m) = entry.metadata() {
                size_bytes += m.len();
            }
        }
    }

    let size_mb = (size_bytes as f64) / 1024.0 / 1024.0;
    (count, size_mb)
}
