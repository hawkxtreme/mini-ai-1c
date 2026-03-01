use std::path::PathBuf;
use serde_json::{json, Value};
use crate::search;
use crate::index;

pub fn list_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "search_code",
            "description": "Быстрый поиск по исходному коду конфигурации 1С (BSL и XML файлы). Возвращает совпадения с файлом и номером строки.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Поисковый запрос — имя процедуры, функции или любой текст"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Максимум результатов (по умолчанию 20, максимум 100)",
                        "default": 20
                    },
                    "regex": {
                        "type": "boolean",
                        "description": "Использовать регулярное выражение (по умолчанию false — регистронезависимый литеральный поиск)",
                        "default": false
                    }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "get_file_context",
            "description": "Получить контекст кода из файла конфигурации 1С вокруг указанной строки.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "Путь к файлу относительно корня конфигурации (например: CommonModules/ОбщийМодуль/Module.bsl)"
                    },
                    "line": {
                        "type": "integer",
                        "description": "Номер строки (1-based)"
                    },
                    "radius": {
                        "type": "integer",
                        "description": "Строк контекста выше и ниже (по умолчанию 40)",
                        "default": 40
                    }
                },
                "required": ["file", "line"]
            }
        }),
        json!({
            "name": "find_symbol",
            "description": "Найти процедуру или функцию по имени в символьном индексе конфигурации 1С. Возвращает файл и номера строк определения. Используйте get_symbol_context для получения полного тела.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Имя процедуры или функции (полное или частичное)"
                    },
                    "exact": {
                        "type": "boolean",
                        "description": "Точное совпадение имени (по умолчанию false — поиск по подстроке)",
                        "default": false
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Максимум результатов (по умолчанию 20)",
                        "default": 20
                    }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "get_symbol_context",
            "description": "Получить полный код процедуры или функции по файлу и строке. Возвращает полное тело символа от начала до конца определения.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "Путь к файлу относительно корня конфигурации (из результатов find_symbol)"
                    },
                    "line": {
                        "type": "integer",
                        "description": "Номер строки внутри процедуры/функции (start_line из find_symbol)"
                    }
                },
                "required": ["file", "line"]
            }
        }),
    ]
}

pub async fn call_tool(
    name: &str,
    args: &Value,
    config_path: &Option<PathBuf>,
    db_path: &Option<PathBuf>,
) -> Result<Value, String> {
    match name {
        "search_code" => handle_search_code(args, config_path).await,
        "get_file_context" => handle_get_file_context(args, config_path).await,
        "find_symbol" => handle_find_symbol(args, db_path).await,
        "get_symbol_context" => handle_get_symbol_context(args, config_path, db_path).await,
        _ => Err(format!("Неизвестный инструмент: {}", name)),
    }
}

async fn handle_search_code(args: &Value, config_path: &Option<PathBuf>) -> Result<Value, String> {
    let root = config_path
        .as_ref()
        .ok_or("Конфигурация не настроена. Укажите путь в настройках MCP сервера.")?;

    let query = args["query"].as_str().ok_or("Параметр 'query' обязателен")?;
    if query.trim().is_empty() {
        return Err("Параметр 'query' не может быть пустым".to_string());
    }

    let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize;
    let use_regex = args["regex"].as_bool().unwrap_or(false);

    let root_clone = root.clone();
    let query_owned = query.to_string();

    let start_time = std::time::Instant::now();

    let results = tokio::task::spawn_blocking(move || {
        search::search_code(&root_clone, &query_owned, use_regex, limit)
    })
    .await
    .map_err(|e| format!("Ошибка выполнения поиска: {}", e))?;

    let elapsed = start_time.elapsed().as_millis();

    if results.is_empty() {
        return Ok(json!({
            "content": [{ "type": "text", "text": format!("По запросу \"{}\" ничего не найдено. (Поиск занял: {}мс)", query, elapsed) }]
        }));
    }

    let mut text = format!("Найдено {} результат(ов) по запросу \"{}\" (Поиск занял: {}мс):\n\n", results.len(), query, elapsed);
    for r in &results {
        let ext = r.file.rsplit('.').next().unwrap_or("bsl");
        text.push_str(&format!(
            "**{}:{}**\n```{}\n{}\n```\n\n",
            r.file, r.line, ext, r.snippet.trim()
        ));
    }

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

async fn handle_get_file_context(
    args: &Value,
    config_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let file_str = args["file"].as_str().ok_or("Параметр 'file' обязателен")?;
    let line = args["line"].as_u64().ok_or("Параметр 'line' обязателен")? as usize;
    let radius = args["radius"].as_u64().unwrap_or(40).clamp(1, 200) as usize;

    let file_path = {
        let p = std::path::Path::new(file_str);
        if p.is_absolute() {
            p.to_path_buf()
        } else if let Some(root) = config_path {
            root.join(file_str)
        } else {
            p.to_path_buf()
        }
    };

    let result = tokio::task::spawn_blocking(move || {
        search::get_file_context(&file_path, line, radius)
    })
    .await
    .map_err(|e| format!("Ошибка чтения файла: {}", e))??;

    Ok(json!({ "content": [{ "type": "text", "text": result }] }))
}

async fn handle_find_symbol(args: &Value, db_path: &Option<PathBuf>) -> Result<Value, String> {
    let db = db_path
        .as_ref()
        .ok_or("Индекс символов не готов. Убедитесь, что указан путь к конфигурации и индексация завершена.")?;

    let query = args["query"].as_str().ok_or("Параметр 'query' обязателен")?;
    if query.trim().is_empty() {
        return Err("Параметр 'query' не может быть пустым".to_string());
    }

    let exact = args["exact"].as_bool().unwrap_or(false);
    let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize;

    let db_clone = db.clone();
    let query_owned = query.to_string();

    let results = tokio::task::spawn_blocking(move || {
        index::find_symbols(&db_clone, &query_owned, exact, limit)
    })
    .await
    .map_err(|e| format!("Ошибка поиска: {}", e))??;

    if results.is_empty() {
        let hint = if exact {
            "Попробуйте поиск без флага exact для поиска по подстроке."
        } else {
            "Проверьте написание имени."
        };
        return Ok(json!({
            "content": [{ "type": "text", "text": format!("Символ \"{}\" не найден в индексе. {}", query, hint) }]
        }));
    }

    let mut text = format!("Найдено {} символ(ов) по запросу \"{}\":\n\n", results.len(), query);
    for r in &results {
        let export_mark = if r.is_export { " Экспорт" } else { "" };
        text.push_str(&format!(
            "**{}** ({}{}) — `{}` строки {}-{}\n",
            r.name, r.kind, export_mark, r.file, r.start_line, r.end_line
        ));
    }
    text.push_str("\nИспользуйте get_symbol_context для получения полного кода.");

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

async fn handle_get_symbol_context(
    args: &Value,
    config_path: &Option<PathBuf>,
    db_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let root = config_path
        .as_ref()
        .ok_or("Конфигурация не настроена.")?;
    let db = db_path
        .as_ref()
        .ok_or("Индекс символов не готов.")?;

    let file_str = args["file"].as_str().ok_or("Параметр 'file' обязателен")?;
    let line = args["line"].as_u64().ok_or("Параметр 'line' обязателен")? as u32;

    let db_clone = db.clone();
    let root_clone = root.clone();
    let file_owned = file_str.to_string();

    let result = tokio::task::spawn_blocking(move || {
        // Normalize path separators to forward slash (stored in index as /)
        let file_normalized = file_owned.replace('\\', "/");

        // Find enclosing symbol at given line
        let sym = index::find_symbol_at_line(&db_clone, &file_normalized, line)
            .ok_or_else(|| format!(
                "Символ не найден в строке {} файла {}. Используйте find_symbol для поиска по имени.",
                line, file_normalized
            ))?;

        // Read the source file
        let file_path = root_clone.join(file_normalized.replace('/', std::path::MAIN_SEPARATOR_STR));
        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| format!("Ошибка чтения файла {}: {}", sym.file, e))?;

        let lines: Vec<&str> = content.lines().collect();
        let start = (sym.start_line as usize).saturating_sub(1);
        let end = (sym.end_line as usize).min(lines.len());

        if start >= lines.len() {
            return Err(format!("Строка {} выходит за пределы файла (всего {} строк)", sym.start_line, lines.len()));
        }

        let body = lines[start..end].join("\n");
        let export_mark = if sym.is_export { " Экспорт" } else { "" };

        Ok::<String, String>(format!(
            "**{}** ({}{}) — `{}` строки {}-{}\n\n```bsl\n{}\n```",
            sym.name, sym.kind, export_mark, sym.file, sym.start_line, sym.end_line, body
        ))
    })
    .await
    .map_err(|e| format!("Ошибка выполнения: {}", e))??;

    Ok(json!({ "content": [{ "type": "text", "text": result }] }))
}
