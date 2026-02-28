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

/// Search for `query` in all .bsl and .xml files under `root`.
/// `use_regex` — treat query as regex; otherwise literal case-insensitive.
pub fn search_code(root: &Path, query: &str, use_regex: bool, limit: usize) -> Vec<SearchResult> {
    let pattern = if use_regex {
        match Regex::new(query) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[1c-search] Invalid regex '{}': {}", query, e);
                return vec![];
            }
        }
    } else {
        // Literal search, case-insensitive via (?i) flag
        match Regex::new(&format!("(?i){}", regex::escape(query))) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[1c-search] Regex build error: {}", e);
                return vec![];
            }
        }
    };

    let mut results = Vec::new();

    let walker = WalkBuilder::new(root)
        .standard_filters(true) // respects .gitignore, skips hidden
        .follow_links(false)
        .build();

    'outer: for entry in walker {
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

        for result in search_file(path, &pattern, root) {
            results.push(result);
            if results.len() >= limit {
                break 'outer;
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
