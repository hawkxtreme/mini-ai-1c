use std::path::PathBuf;
use serde_json::{json, Value};
use crate::search;

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
    ]
}

pub async fn call_tool(
    name: &str,
    args: &Value,
    config_path: &Option<PathBuf>,
) -> Result<Value, String> {
    match name {
        "search_code" => handle_search_code(args, config_path).await,
        "get_file_context" => handle_get_file_context(args, config_path).await,
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

    let results = tokio::task::spawn_blocking(move || {
        search::search_code(&root_clone, &query_owned, use_regex, limit)
    })
    .await
    .map_err(|e| format!("Ошибка выполнения поиска: {}", e))?;

    if results.is_empty() {
        return Ok(json!({
            "content": [{ "type": "text", "text": format!("По запросу \"{}\" ничего не найдено.", query) }]
        }));
    }

    let mut text = format!("Найдено {} результат(ов) по запросу \"{}\":\n\n", results.len(), query);
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
