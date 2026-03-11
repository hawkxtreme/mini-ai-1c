use std::path::{Path, PathBuf};
use serde_json::{json, Value};
use crate::search;
use crate::index;

/// Maps a 1C object type to its plural folder name in the config dump.
fn object_type_to_folder(obj_type: &str) -> Option<&'static str> {
    match obj_type {
        "Catalog"                    => Some("Catalogs"),
        "Document"                   => Some("Documents"),
        "CommonModule"               => Some("CommonModules"),
        "InformationRegister"        => Some("InformationRegisters"),
        "AccumulationRegister"       => Some("AccumulationRegisters"),
        "AccountingRegister"         => Some("AccountingRegisters"),
        "CalculationRegister"        => Some("CalculationRegisters"),
        "ExchangePlan"               => Some("ExchangePlans"),
        "BusinessProcess"            => Some("BusinessProcesses"),
        "Task"                       => Some("Tasks"),
        "ChartOfCharacteristicTypes" => Some("ChartsOfCharacteristicTypes"),
        "ChartOfAccounts"            => Some("ChartsOfAccounts"),
        "ChartOfCalculationTypes"    => Some("ChartsOfCalculationTypes"),
        "DataProcessor"              => Some("DataProcessors"),
        "Report"                     => Some("Reports"),
        "Enum"                       => Some("Enums"),
        "Constant"                   => Some("Constants"),
        "DocumentJournal"            => Some("DocumentJournals"),
        "FilterCriterion"            => Some("FilterCriteria"),
        "ScheduledJob"               => Some("ScheduledJobs"),
        "WebService"                 => Some("WebServices"),
        "HTTPService"                => Some("HTTPServices"),
        "CommonForm"                 => Some("CommonForms"),
        "CommonTemplate"             => Some("CommonTemplates"),
        "CommonAttribute"            => Some("CommonAttributes"),
        "CommonCommand"              => Some("CommonCommands"),
        "Role"                       => Some("Roles"),
        "Subsystem"                  => Some("Subsystems"),
        "Language"                   => Some("Languages"),
        _ => None,
    }
}

/// Resolve a `scope` string to a relative sub-path within the config root.
///
/// Accepts two forms:
///   1. `"CommonModule.МодульИмя"` → `CommonModules/МодульИмя`
///   2. `"CommonModules/МодульИмя"` → `CommonModules/МодульИмя` (raw path, used as-is)
///
/// Returns `None` if the type is unknown.
fn resolve_scope(scope: &str) -> Option<PathBuf> {
    // Form 1: "Type.Name" — contains exactly one dot and first part is a known type
    if let Some(dot) = scope.find('.') {
        let type_part = &scope[..dot];
        let name_part = &scope[dot + 1..];
        if !name_part.is_empty() {
            if let Some(folder) = object_type_to_folder(type_part) {
                return Some(Path::new(folder).join(name_part));
            }
        }
    }
    // Form 2: raw relative path (forward or back slashes)
    if !scope.is_empty() {
        return Some(PathBuf::from(scope.replace('\\', "/")));
    }
    None
}

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
                    },
                    "scope": {
                        "type": "string",
                        "description": "Ограничить поиск конкретным объектом 1С. Форматы: 'CommonModule.МодульИмя', 'Catalog.СправочникИмя', 'Document.ДокументИмя' и т.д. Можно также передать относительный путь: 'CommonModules/МодульИмя'. Если не указан — поиск по всей конфигурации."
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
        json!({
            "name": "list_objects",
            "description": "Список объектов конфигурации 1С (справочники, документы, общие модули и т.д.). Требует предварительной индексации метаданных.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": {
                        "type": "string",
                        "description": "Фильтр по типу объекта: Catalog, Document, CommonModule, InformationRegister, AccumulationRegister, Report, DataProcessor и т.д. Если не указан — возвращает все типы."
                    },
                    "name_filter": {
                        "type": "string",
                        "description": "Фильтр по части имени объекта (регистронезависимый). Например: 'файл' найдёт РаботаСФайлами, ФайлыСервер и т.д."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Максимум результатов (по умолчанию 100, максимум 500)",
                        "default": 100
                    }
                }
            }
        }),
        json!({
            "name": "get_object_structure",
            "description": "Получить полную структуру объекта конфигурации 1С: реквизиты, табличные части, формы, команды, модули.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "object": {
                        "type": "string",
                        "description": "Имя объекта или полный идентификатор (например: РеализацияТоваров или Document.РеализацияТоваров)"
                    }
                },
                "required": ["object"]
            }
        }),
        json!({
            "name": "find_references",
            "description": "Найти все вхождения символа (процедуры, функции, переменной) в коде конфигурации. Показывает где и как используется символ.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Имя символа для поиска"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Максимум результатов (по умолчанию 50)",
                        "default": 50
                    }
                },
                "required": ["symbol"]
            }
        }),
        json!({
            "name": "impact_analysis",
            "description": "Анализ влияния: показывает какие модули и файлы используют данный объект или символ. Помогает понять последствия изменений.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "object": {
                        "type": "string",
                        "description": "Имя объекта или символа для анализа влияния (например: РеализацияТоваров, НачислитьНДС)"
                    }
                },
                "required": ["object"]
            }
        }),
        json!({
            "name": "get_function_context",
            "description": "Граф вызовов функции или процедуры: что она вызывает и кто её вызывает. Помогает понять зависимости и контекст использования.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "function_name": {
                        "type": "string",
                        "description": "Имя функции или процедуры (например: ПровестиДокумент, РассчитатьСумму)"
                    }
                },
                "required": ["function_name"]
            }
        }),
        json!({
            "name": "get_module_functions",
            "description": "Список всех процедур и функций модуля BSL. Полезно для ориентации в крупном модуле без поиска по тексту.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "module_path": {
                        "type": "string",
                        "description": "Путь к модулю или его имя. Форматы: 'CommonModule.МодульИмя', 'CommonModules/МодульИмя/Module.bsl', просто 'МодульИмя'"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Максимум результатов (по умолчанию 200)",
                        "default": 200
                    }
                },
                "required": ["module_path"]
            }
        }),
        json!({
            "name": "smart_find",
            "description": "Умный поиск функции/процедуры по имени: находит символ в индексе (1 мс) и возвращает полный код за один вызов. Используй ВМЕСТО search_code когда знаешь имя функции.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Имя функции или процедуры (полное или частичное)"
                    },
                    "include_code": {
                        "type": "boolean",
                        "description": "Включить полный код лучшего совпадения (по умолчанию true)",
                        "default": true
                    }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "find_function_in_object",
            "description": "Найти функцию/процедуру внутри конкретного объекта 1С (справочник, документ, общий модуль). Возвращает список функций объекта + код лучшего совпадения по подсказке.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "object": {
                        "type": "string",
                        "description": "Имя объекта или полный идентификатор: 'Catalog.СтавкиНДС', 'CommonModule.УчетНДС', 'Document.ЗаказПокупателя'"
                    },
                    "function_hint": {
                        "type": "string",
                        "description": "Ключевое слово для фильтрации функций (регистронезависимый поиск по имени). Если не указано — возвращает все функции объекта."
                    }
                },
                "required": ["object"]
            }
        }),
        json!({
            "name": "stats",
            "description": "Статистика символьного индекса конфигурации 1С: количество символов, файлов, объектов, рёбер графа вызовов.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "benchmark",
            "description": "Замер производительности всех инструментов поиска: min/avg/p95/max latency в мс. Используется для публичного бенчмарка и сравнения до/после оптимизаций.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "iterations": {
                        "type": "number",
                        "description": "Количество итераций каждого инструмента (по умолчанию 20, max 100)"
                    }
                }
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
    let start = std::time::Instant::now();
    let result = match name {
        "search_code" => handle_search_code(args, config_path, db_path).await,
        "get_file_context" => handle_get_file_context(args, config_path).await,
        "find_symbol" => handle_find_symbol(args, db_path).await,
        "get_symbol_context" => handle_get_symbol_context(args, config_path, db_path).await,
        "list_objects" => handle_list_objects(args, db_path).await,
        "get_object_structure" => handle_get_object_structure(args, db_path, config_path).await,
        "find_references" => handle_find_references(args, config_path).await,
        "impact_analysis" => handle_impact_analysis(args, config_path, db_path).await,
        "get_function_context" => handle_get_function_context(args, db_path).await,
        "get_module_functions" => handle_get_module_functions(args, db_path).await,
        "smart_find" => handle_smart_find(args, config_path, db_path).await,
        "find_function_in_object" => handle_find_function_in_object(args, config_path, db_path).await,
        "stats" => handle_stats(db_path).await,
        "sync_index" => handle_sync_index(config_path, db_path).await,
        "benchmark" => handle_benchmark(args, config_path, db_path).await,
        _ => Err(format!("Неизвестный инструмент: {}", name)),
    };
    eprintln!("[PERF] {} in {}ms", name, start.elapsed().as_millis());
    result
}

async fn handle_search_code(
    args: &Value,
    config_path: &Option<PathBuf>,
    db_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let root = config_path
        .as_ref()
        .ok_or("Конфигурация не настроена. Укажите путь в настройках MCP сервера.")?;

    let query = args["query"].as_str().ok_or("Параметр 'query' обязателен")?;
    if query.trim().is_empty() {
        return Err("Параметр 'query' не может быть пустым".to_string());
    }

    let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize;
    let use_regex = args["regex"].as_bool().unwrap_or(false);

    // Resolve scope → relative sub-path within config root
    let sub_path: Option<PathBuf> = args["scope"].as_str().and_then(|s| {
        let s = s.trim();
        if s.is_empty() {
            None
        } else {
            match resolve_scope(s) {
                Some(p) => Some(p),
                None => {
                    eprintln!("[1c-search] Unknown scope '{}', searching full config", s);
                    None
                }
            }
        }
    });

    // Early check: if scope resolves to a path that doesn't exist, return informative message
    if let Some(ref sp) = sub_path {
        let full_scope_path = root.join(sp);
        if !full_scope_path.exists() {
            // Check if the parent (type folder) is non-empty — if parent is also empty,
            // it means this object type wasn't exported in this configuration dump.
            let scope_str = args["scope"].as_str().unwrap_or("");
            let parent_empty = sp.parent()
                .map(|p| {
                    let parent_full = root.join(p);
                    !parent_full.is_dir()
                        || std::fs::read_dir(&parent_full)
                            .map(|mut rd| rd.next().is_none())
                            .unwrap_or(true)
                })
                .unwrap_or(true);

            let msg = if parent_empty {
                format!(
                    "Исходные файлы для объектов данного типа в выгрузке конфигурации отсутствуют (область «{}» не найдена).\n\
                     Попробуйте поиск без параметра `scope` — например: `search_code query=\"{}\" `.",
                    scope_str, query
                )
            } else {
                format!(
                    "Объект «{}» не найден в выгрузке конфигурации.\n\
                     Проверьте правильность имени или используйте `list_objects` для просмотра доступных объектов.",
                    scope_str
                )
            };
            return Ok(json!({ "content": [{ "type": "text", "text": msg }] }));
        }
    }

    let root_clone = root.clone();
    let db_clone = db_path.clone();
    let query_owned = query.to_string();
    let query_lower = query.to_lowercase();
    // Index-guided conditions:
    // - full-config search only (no scope)
    // - not a regex
    // - no spaces in query (symbol names never contain spaces — hint would return 0 results)
    let use_index_hint = sub_path.is_none() && !use_regex && !query.contains(' ');
    let sub_path_clone = sub_path.clone();

    let start_time = std::time::Instant::now();

    let (results, timed_out) = tokio::task::spawn_blocking(move || -> (Vec<search::SearchResult>, bool) {
        // Phase 1: SQLite-guided search
        // Query the symbols table for files that DECLARE symbols matching the query.
        // These files are the most likely to also contain usages — grep only them first.
        // If limit is reached → return without touching the rest of the 25K-file config.
        //
        // Smart hint for qualified names: "Справочники.СтавкиНДС" → hint on "ставкиндс"
        // Symbol names never include the "Справочники." prefix, so stripping it gives hits.
        if use_index_hint {
            if let Some(db) = db_clone.as_deref() {
                let hint_query = if query_lower.contains('.') {
                    // Use only the last segment after the final dot
                    query_lower.rsplit('.').next().unwrap_or(&query_lower).to_string()
                } else {
                    query_lower.clone()
                };
                let hot_files = index::find_files_by_symbol_query(db, &hint_query, 100);
                if !hot_files.is_empty() {
                    let hot = search::search_code_in_file_set(
                        &root_clone, &hot_files, &query_owned, false, limit,
                    );
                    // Return index-guided results regardless of count.
                    // A full-config fallback scan on cold HDD takes minutes — unacceptable.
                    // If the symbol exists in the index, hint files are the most relevant.
                    // When hot is empty (symbol truly not in hint files), fall through below.
                    if !hot.is_empty() {
                        eprintln!(
                            "[1c-search] index-guided: {} results from {} hint files",
                            hot.len(), hot_files.len()
                        );
                        return (hot, false);
                    }
                }
            }
        }
        // Phase 2: no index hint (regex, scoped, spaces in query, or no db)
        // BSL-first two-pass streaming scan — capped at 8s to avoid multi-minute waits on HDD.
        search::search_code(&root_clone, sub_path_clone.as_deref(), &query_owned, use_regex, limit, Some(8_000))
    })
    .await
    .map_err(|e| format!("Ошибка выполнения поиска: {}", e))?;

    let elapsed = start_time.elapsed().as_millis();

    let scope_label = args["scope"].as_str()
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!(" в «{}»", s))
        .unwrap_or_default();

    if results.is_empty() {
        let timeout_note = if timed_out {
            " Поиск прерван по таймауту (8с) — попробуйте уточнить запрос через параметр `scope`."
        } else {
            ""
        };
        return Ok(json!({
            "content": [{ "type": "text", "text": format!(
                "По запросу \"{}\"{}  ничего не найдено. ({}мс){}",
                query, scope_label, elapsed, timeout_note
            )}]
        }));
    }

    let mut text = format!(
        "Найдено {} результат(ов) по запросу \"{}\"{} ({}мс):\n\n",
        results.len(), query, scope_label, elapsed
    );
    for r in &results {
        let ext = r.file.rsplit('.').next().unwrap_or("bsl");
        // Enrich with containing function from symbol index
        let containing = db_path.as_ref()
            .and_then(|db| index::find_symbol_at_line(db, &r.file, r.line))
            .map(|sym| format!(" _(в {}_)", sym.name))
            .unwrap_or_default();
        text.push_str(&format!(
            "**{}:{}**{}\n```{}\n{}\n```\n\n",
            r.file, r.line, containing, ext, r.snippet.trim()
        ));
    }
    if timed_out {
        text.push_str(&format!(
            "\n⚠️ *Поиск ограничен по времени (8с) — показаны первые {} результатов. Для полного поиска используйте параметр `scope`.*",
            results.len()
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
        let file_path = root_clone.join(file_normalized.replace('/', std::path::MAIN_SEPARATOR_STR));

        // Try to find the enclosing symbol in the index
        if let Some(sym) = index::find_symbol_at_line(&db_clone, &file_normalized, line) {
            let content = std::fs::read_to_string(&file_path)
                .map_err(|e| format!("Ошибка чтения файла {}: {}", sym.file, e))?;

            let lines: Vec<&str> = content.lines().collect();
            let start = (sym.start_line as usize).saturating_sub(1);
            let end = (sym.end_line as usize).min(lines.len());

            if start < lines.len() {
                let body = lines[start..end].join("\n");
                let export_mark = if sym.is_export { " Экспорт" } else { "" };
                return Ok::<String, String>(format!(
                    "**{}** ({}{}) — `{}` строки {}-{}\n\n```bsl\n{}\n```",
                    sym.name, sym.kind, export_mark, sym.file, sym.start_line, sym.end_line, body
                ));
            }
        }

        // Fallback: symbol not found in index (top-level code, form modules, etc.)
        // Return a context window around the requested line
        match search::get_file_context(&file_path, line as usize, 40) {
            Ok(ctx) => Ok(format!(
                "⚠️ Символ в индексе не найден — возможно, это код вне процедуры/функции.\nПоказан контекст файла:\n\n```bsl\n{}\n```",
                ctx
            )),
            Err(e) => Err(format!(
                "Символ не найден в строке {} файла {}, и файл не читается: {}",
                line, file_normalized, e
            )),
        }
    })
    .await
    .map_err(|e| format!("Ошибка выполнения: {}", e))??;

    Ok(json!({ "content": [{ "type": "text", "text": result }] }))
}

async fn handle_list_objects(args: &Value, db_path: &Option<PathBuf>) -> Result<Value, String> {
    let db = db_path
        .as_ref()
        .ok_or("Индекс не готов. Укажите путь к конфигурации в настройках MCP сервера.")?;

    let type_filter = args["type"].as_str().map(|s| s.to_string());
    let name_filter = args["name_filter"].as_str().map(|s| s.to_string());
    let limit = args["limit"].as_u64().unwrap_or(100).clamp(1, 500) as usize;
    let db_clone = db.clone();

    let objects = tokio::task::spawn_blocking(move || {
        index::list_objects(&db_clone, type_filter.as_deref(), name_filter.as_deref(), limit)
    })
    .await
    .map_err(|e| format!("Ошибка выполнения: {}", e))??;

    if objects.is_empty() {
        let hint = if args["type"].is_string() {
            "Проверьте правильность типа объекта (Catalog, Document, CommonModule и т.д.) или запустите переиндексацию."
        } else {
            "Метаданные не проиндексированы. Убедитесь, что в директории конфигурации есть Configuration.xml и индексация завершена."
        };
        return Ok(json!({
            "content": [{ "type": "text", "text": format!("Объекты не найдены. {}", hint) }]
        }));
    }

    let mut by_type: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for obj in &objects {
        by_type.entry(obj.obj_type.clone()).or_default().push(obj.name.clone());
    }

    let mut text = format!("**Объекты конфигурации** ({} шт.):\n\n", objects.len());
    for (obj_type, names) in &by_type {
        text.push_str(&format!("### {} ({})\n", obj_type, names.len()));
        for name in names {
            text.push_str(&format!("- {}\n", name));
        }
        text.push('\n');
    }
    if objects.len() >= limit {
        text.push_str(&format!(
            "\n*Показано {} объектов. Используйте параметр `type` для фильтрации.*",
            limit
        ));
    }

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

async fn handle_get_object_structure(
    args: &Value,
    db_path: &Option<PathBuf>,
    config_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let object_name = args["object"].as_str().ok_or("Параметр 'object' обязателен")?;
    if object_name.trim().is_empty() {
        return Err("Параметр 'object' не может быть пустым".to_string());
    }

    // Try SQLite index first (if available)
    let details = if let Some(db) = db_path.as_ref() {
        let db_clone = db.clone();
        let name_owned = object_name.to_string();
        tokio::task::spawn_blocking(move || index::get_object_details(&db_clone, &name_owned))
            .await
            .map_err(|e| format!("Ошибка выполнения: {}", e))?
    } else {
        None
    };

    match details {
        Some(d) => {
            let mut text = format!("## {}.{}\n\n", d.obj_type, d.name);

            if !d.attributes.is_empty() {
                text.push_str(&format!("### Реквизиты ({})\n", d.attributes.len()));
                for attr in &d.attributes { text.push_str(&format!("- {}\n", attr)); }
                text.push('\n');
            }
            if !d.tabular_sections.is_empty() {
                text.push_str(&format!("### Табличные части ({})\n", d.tabular_sections.len()));
                for (section, attrs) in &d.tabular_sections {
                    if attrs.is_empty() {
                        text.push_str(&format!("- **{}**\n", section));
                    } else {
                        text.push_str(&format!("- **{}**: {}\n", section, attrs.join(", ")));
                    }
                }
                text.push('\n');
            }
            if !d.forms.is_empty() {
                text.push_str(&format!("### Формы ({})\n", d.forms.len()));
                for form in &d.forms { text.push_str(&format!("- {}\n", form)); }
                text.push('\n');
            }
            if !d.commands.is_empty() {
                text.push_str(&format!("### Команды ({})\n", d.commands.len()));
                for cmd in &d.commands { text.push_str(&format!("- {}\n", cmd)); }
                text.push('\n');
            }
            if !d.modules.is_empty() {
                text.push_str(&format!("### Модули ({})\n", d.modules.len()));
                for m in &d.modules { text.push_str(&format!("- {}\n", m)); }
                text.push('\n');
            }
            if d.attributes.is_empty()
                && d.tabular_sections.is_empty()
                && d.forms.is_empty()
                && d.commands.is_empty()
                && d.modules.is_empty()
            {
                if let Some(fallback) = scan_object_folder_fallback(&d.obj_type, &d.name, config_path) {
                    text.push_str("*Структурные данные получены из файловой системы.*\n\n");
                    text.push_str(&fallback);
                } else {
                    // Check if any objects of this type have source files in the dump
                    let type_folder_has_files = config_path.as_ref()
                        .and_then(|root| object_type_to_folder(d.obj_type.as_str()).map(|f| root.join(f)))
                        .map(|p| p.is_dir() && std::fs::read_dir(&p).map(|mut rd| rd.next().is_some()).unwrap_or(false))
                        .unwrap_or(false);

                    if type_folder_has_files {
                        text.push_str("*Папка объекта не найдена в файловой структуре выгрузки.*\n");
                        text.push_str("Используйте `search_code` или `list_objects` для работы с этим объектом.\n");
                    } else {
                        text.push_str(&format!(
                            "*Объект **{}.{}** присутствует в конфигурации.*\n\n",
                            d.obj_type, d.name
                        ));
                        text.push_str(&format!(
                            "*Исходные файлы объектов типа {} в данной выгрузке конфигурации не экспортированы.*\n",
                            d.obj_type
                        ));
                        text.push_str("Для поиска связанного кода используйте `search_code` без параметра `scope`.\n");
                    }
                }
            }

            Ok(json!({ "content": [{ "type": "text", "text": text }] }))
        }
        None => {
            // Object not in index — try to resolve via filesystem directly
            // Supports both "Type.Name" and plain "Name" forms
            let (explicit_type, plain_name) = if let Some(dot) = object_name.find('.') {
                let t = &object_name[..dot];
                let n = &object_name[dot + 1..];
                if object_type_to_folder(t).is_some() {
                    (Some(t.to_string()), n.to_string())
                } else {
                    (None, object_name.to_string())
                }
            } else {
                (None, object_name.to_string())
            };

            if let Some(root) = config_path.as_ref() {
                // If type is explicit, try only that folder; otherwise try all known types
                let types_to_try: Vec<(&str, &'static str)> = if let Some(ref t) = explicit_type {
                    if let Some(folder) = object_type_to_folder(t.as_str()) {
                        vec![(t.as_str(), folder)]
                    } else {
                        vec![]
                    }
                } else {
                    // Try all known types - find first matching folder
                    vec![
                        ("CommonModule", "CommonModules"),
                        ("Catalog", "Catalogs"),
                        ("Document", "Documents"),
                        ("DataProcessor", "DataProcessors"),
                        ("Report", "Reports"),
                        ("InformationRegister", "InformationRegisters"),
                        ("AccumulationRegister", "AccumulationRegisters"),
                        ("AccountingRegister", "AccountingRegisters"),
                        ("ExchangePlan", "ExchangePlans"),
                        ("Enum", "Enums"),
                        ("BusinessProcess", "BusinessProcesses"),
                        ("Task", "Tasks"),
                        ("ChartOfCharacteristicTypes", "ChartsOfCharacteristicTypes"),
                        ("ChartOfAccounts", "ChartsOfAccounts"),
                        ("ChartOfCalculationTypes", "ChartsOfCalculationTypes"),
                        ("CommonForm", "CommonForms"),
                        ("CommonCommand", "CommonCommands"),
                        ("ScheduledJob", "ScheduledJobs"),
                        ("Constant", "Constants"),
                        ("DocumentJournal", "DocumentJournals"),
                        ("Role", "Roles"),
                        ("Subsystem", "Subsystems"),
                    ]
                };

                let plain_name_lower = plain_name.to_lowercase();
                for (obj_type, folder) in &types_to_try {
                    let parent = root.join(folder);
                    if !parent.is_dir() { continue; }

                    // Try exact match first, then case-insensitive
                    let obj_dir = {
                        let exact = parent.join(&plain_name);
                        if exact.is_dir() {
                            Some(exact)
                        } else {
                            std::fs::read_dir(&parent).ok()
                                .and_then(|rd| {
                                    rd.flatten()
                                        .find(|e| e.file_name().to_string_lossy().to_lowercase() == plain_name_lower)
                                        .map(|e| e.path())
                                })
                        }
                    };

                    if let Some(dir) = obj_dir {
                        let actual_name = dir.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| plain_name.clone());

                        let mut text = format!(
                            "## {}.{}\n*Объект найден в файловой системе (не в индексе — запустите переиндексацию для полных данных).*\n\n",
                            obj_type, actual_name
                        );

                        if let Some(fallback) = scan_object_folder_fallback(obj_type, &actual_name, config_path) {
                            text.push_str(&fallback);
                        } else {
                            text.push_str("*Папка объекта пуста.*\n");
                        }

                        return Ok(json!({ "content": [{ "type": "text", "text": text }] }));
                    }
                }

                // Nothing found even in filesystem
                return Ok(json!({
                    "content": [{
                        "type": "text",
                        "text": format!(
                            "Объект \"{}\" не найден ни в индексе, ни в файловой системе конфигурации.\n\
                             Попробуйте:\n\
                             1. `list_objects` — список проиндексированных объектов\n\
                             2. `sync_index` — переиндексировать конфигурацию\n\
                             3. Проверьте правильность имени объекта (без пространства имён)",
                            object_name
                        )
                    }]
                }));
            }

            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!(
                        "Объект \"{}\" не найден в индексе метаданных.\n\
                         Попробуйте list_objects для просмотра доступных объектов.",
                        object_name
                    )
                }]
            }))
        }
    }
}


/// Scan the object's folder in the config dump to collect forms, modules, templates, commands.
fn scan_object_folder_fallback(
    obj_type: &str,
    obj_name: &str,
    config_path: &Option<std::path::PathBuf>,
) -> Option<String> {
    let root = config_path.as_ref()?;
    let folder_type = object_type_to_folder(obj_type)?;

    // Try exact name match and case-insensitive match
    let obj_dir = root.join(folder_type).join(obj_name);
    let obj_dir = if obj_dir.is_dir() {
        obj_dir
    } else {
        // case-insensitive scan
        let parent = root.join(folder_type);
        let lower = obj_name.to_lowercase();
        std::fs::read_dir(&parent).ok()?
            .flatten()
            .find(|e| e.file_name().to_string_lossy().to_lowercase() == lower)
            .map(|e| e.path())?
    };

    let mut forms: Vec<String> = Vec::new();
    let mut modules: Vec<String> = Vec::new();
    let mut templates: Vec<String> = Vec::new();
    let mut commands: Vec<String> = Vec::new();
    let mut has_module = false;

    if let Ok(entries) = std::fs::read_dir(&obj_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            if path.is_dir() {
                match name.as_str() {
                    "Forms"     => { if let Ok(es) = std::fs::read_dir(&path) { for e in es.flatten() { let n = e.file_name().to_string_lossy().to_string(); if !n.starts_with('.') { forms.push(n); } } } }
                    "Templates" => { if let Ok(es) = std::fs::read_dir(&path) { for e in es.flatten() { let n = e.file_name().to_string_lossy().to_string(); if !n.starts_with('.') { templates.push(n); } } } }
                    "Commands"  => { if let Ok(es) = std::fs::read_dir(&path) { for e in es.flatten() { let n = e.file_name().to_string_lossy().to_string(); if !n.starts_with('.') { commands.push(n); } } } }
                    "Ext"       => { if let Ok(es) = std::fs::read_dir(&path) { for e in es.flatten() { let n = e.file_name().to_string_lossy().to_string(); if !n.starts_with('.') { modules.push(n); } } } }
                    _ => {}
                }
            } else if name == "Module.bsl" {
                has_module = true;
            }
        }
    }

    let mut out = String::new();

    // Check for Module.bsl (CommonModule or object module)
    if has_module {
        let rel = format!("{}/{}/Module.bsl", folder_type, obj_name);
        out.push_str(&format!("### Модуль\n- [Module.bsl]({rel})\n\n"));
    }
    if !modules.is_empty() {
        out.push_str(&format!("### Модули ({})\n", modules.len()));
        for m in &modules { out.push_str(&format!("- {m}\n")); }
        out.push('\n');
    }
    if !forms.is_empty() {
        out.push_str(&format!("### Формы ({})\n", forms.len()));
        for f in &forms { out.push_str(&format!("- {f}\n")); }
        out.push('\n');
    }
    if !commands.is_empty() {
        out.push_str(&format!("### Команды ({})\n", commands.len()));
        for c in &commands { out.push_str(&format!("- {c}\n")); }
        out.push('\n');
    }
    if !templates.is_empty() {
        out.push_str(&format!("### Макеты ({})\n", templates.len()));
        for t in &templates { out.push_str(&format!("- {t}\n")); }
        out.push('\n');
    }

    if out.is_empty() {
        out.push_str("*Структура объекта не определена. BSL-код доступен через `search_code`.*\n");
    } else {
        out.push_str(&format!("\n*Данные получены из файловой структуры `{folder_type}/{obj_name}/`.*\n"));
    }

    Some(out)
}

async fn handle_find_references(
    args: &Value,
    config_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let root = config_path
        .as_ref()
        .ok_or("Конфигурация не настроена. Укажите путь в настройках MCP сервера.")?;

    let symbol = args["symbol"].as_str().ok_or("Параметр 'symbol' обязателен")?;
    if symbol.trim().is_empty() {
        return Err("Параметр 'symbol' не может быть пустым".to_string());
    }

    let limit = args["limit"].as_u64().unwrap_or(50).clamp(1, 200) as usize;
    let root_clone = root.clone();
    let symbol_owned = symbol.to_string();

    let start = std::time::Instant::now();
    let (results, timed_out) = tokio::task::spawn_blocking(move || {
        search::search_code(&root_clone, None, &symbol_owned, false, limit, Some(8_000))
    })
    .await
    .map_err(|e| format!("Ошибка поиска: {}", e))?;
    let elapsed = start.elapsed().as_millis();

    if results.is_empty() {
        let note = if timed_out {
            " Поиск прерван по таймауту (8с) — символ мог не встретиться в первых просмотренных файлах."
        } else { "" };
        return Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("Символ \"{}\" не найден в коде конфигурации. ({}мс){}", symbol, elapsed, note)
            }]
        }));
    }

    // Group by file preserving insertion order
    let mut file_order: Vec<String> = Vec::new();
    let mut by_file: std::collections::HashMap<String, Vec<(u32, String)>> =
        std::collections::HashMap::new();
    for r in &results {
        if !by_file.contains_key(&r.file) {
            file_order.push(r.file.clone());
        }
        by_file
            .entry(r.file.clone())
            .or_default()
            .push((r.line, r.snippet.trim().to_string()));
    }

    let mut text = format!(
        "**Ссылки на \"{}\"** — {} вхождений в {} файлах ({}мс):\n\n",
        symbol, results.len(), file_order.len(), elapsed
    );
    for file in &file_order {
        let lines = &by_file[file];
        let ext = file.rsplit('.').next().unwrap_or("bsl");
        text.push_str(&format!("**{}** ({} вхожд.)\n", file, lines.len()));
        for (line_no, snippet) in lines.iter().take(5) {
            text.push_str(&format!(
                "  ```{}\n  // строка {}\n  {}\n  ```\n",
                ext, line_no, snippet
            ));
        }
        if lines.len() > 5 {
            text.push_str(&format!("  *...ещё {} вхождений*\n", lines.len() - 5));
        }
        text.push('\n');
    }
    if timed_out {
        text.push_str(&format!(
            "\n⚠️ *Поиск ограничен по времени (8с) — показаны первые {} результатов. Для полного поиска уточните область через `scope`.*",
            results.len()
        ));
    } else if results.len() >= limit {
        text.push_str(&format!(
            "*Показано {} результатов. Увеличьте `limit` для большего количества.*",
            limit
        ));
    }

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

async fn handle_impact_analysis(
    args: &Value,
    config_path: &Option<PathBuf>,
    db_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let root = config_path
        .as_ref()
        .ok_or("Конфигурация не настроена. Укажите путь в настройках MCP сервера.")?;

    let object_name = args["object"].as_str().ok_or("Параметр 'object' обязателен")?;
    if object_name.trim().is_empty() {
        return Err("Параметр 'object' не может быть пустым".to_string());
    }

    // Strip "Type." prefix for text search
    let search_term = if let Some(dot) = object_name.find('.') {
        object_name[dot + 1..].to_string()
    } else {
        object_name.to_string()
    };

    let root_clone = root.clone();
    let db_clone = db_path.clone();
    let search_term_clone = search_term.clone();
    let object_name_owned = object_name.to_string();

    // Use search_files_summary instead of search_code:
    // - stops after MAX_FILES files with matches (not 500 individual line matches)
    // - for widely-used symbols this is drastically faster: O(matched_files) vs O(all_files)
    // - collects 3 example lines per file inline, no second pass needed
    const MAX_FILES: usize = 50;
    const EXAMPLES_PER_FILE: usize = 3;

    let (details, hits, timed_out): (Option<index::ObjectDetails>, Vec<search::FileHits>, bool) =
        tokio::task::spawn_blocking(move || {
            let details = db_clone
                .as_deref()
                .and_then(|db| index::get_object_details(db, &object_name_owned));
            let (hits, timed_out) = search::search_files_summary(
                &root_clone,
                &search_term_clone,
                false,
                MAX_FILES,
                EXAMPLES_PER_FILE,
                Some(8_000),
            );
            (details, hits, timed_out)
        })
        .await
        .map_err(|e| format!("Ошибка выполнения: {}", e))?;

    let mut text = format!("## Анализ влияния: {}\n\n", object_name);

    if let Some(d) = &details {
        text.push_str(&format!("**Тип**: {}\n", d.obj_type));
        if !d.attributes.is_empty() {
            text.push_str(&format!("**Реквизитов**: {}\n", d.attributes.len()));
        }
        if !d.tabular_sections.is_empty() {
            text.push_str(&format!("**Табличных частей**: {}\n", d.tabular_sections.len()));
        }
        text.push('\n');
    }

    if hits.is_empty() {
        text.push_str(&format!(
            "Ссылок на \"{}\" в коде конфигурации не найдено.\n",
            search_term
        ));
    } else {
        let total_count: usize = hits.iter().map(|h| h.count).sum();
        text.push_str(&format!(
            "**{} вхождений в {} файлах:**\n\n",
            total_count, hits.len()
        ));
        for h in hits.iter().take(20) {
            text.push_str(&format!("- `{}` — {} вхождений\n", h.file, h.count));
        }
        if hits.len() > 20 {
            text.push_str(&format!("- *...ещё {} файлов*\n", hits.len() - 20));
        }

        text.push_str("\n**Примеры использования:**\n");
        let mut example_count = 0;
        'outer: for h in &hits {
            for (line, snippet) in &h.examples {
                let ext = h.file.rsplit('.').next().unwrap_or("bsl");
                text.push_str(&format!(
                    "```{}\n// {}:{}\n{}\n```\n",
                    ext, h.file, line, snippet.trim()
                ));
                example_count += 1;
                if example_count >= 5 {
                    break 'outer;
                }
            }
        }
        if timed_out {
            text.push_str(&format!(
                "\n⚠️ *Поиск ограничен по времени (8с) — показаны {} файлов из найденных. Объект используется шире.*",
                hits.len()
            ));
        } else if hits.len() >= MAX_FILES {
            text.push_str(
                "\n*Поиск ограничен первыми 50 файлами. Объект широко используется в конфигурации.*",
            );
        }
    }

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

async fn handle_sync_index(
    config_path: &Option<PathBuf>,
    db_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let root = config_path
        .as_ref()
        .ok_or("Конфигурация не настроена. Укажите путь в настройках MCP сервера.")?;
    let db = db_path
        .as_ref()
        .ok_or("Нет пути к базе данных индекса")?;

    let root = root.clone();
    let db = db.clone();

    let stats = tokio::task::spawn_blocking(move || index::sync_index(&root, &db))
        .await
        .map_err(|e| format!("Паника spawn_blocking: {}", e))?
        .map_err(|e| format!("Ошибка синхронизации: {}", e))?;

    let db_for_index = db_path.as_ref().unwrap();
    let size = crate::db_size_mb(db_for_index);
    // Use current time directly — avoids SQLite WAL caching issues when reading back built_at
    let built_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    eprintln!("SEARCH_STATUS:ready:{}:{:.2}:{}", stats.total_symbols, size, built_at);

    let text = if stats.added == 0 && stats.updated == 0 && stats.removed == 0 {
        "✅ Индекс актуален. Изменённых BSL файлов не обнаружено.".to_string()
    } else {
        format!(
            "✅ Синхронизация завершена:\n- Новых файлов: {}\n- Изменённых: {}\n- Удалённых: {}\n- Итого символов в индексе: {}",
            stats.added, stats.updated, stats.removed, stats.total_symbols
        )
    };

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

// ─── get_function_context ────────────────────────────────────────────────────

async fn handle_get_function_context(
    args: &Value,
    db_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let function_name = args["function_name"].as_str().ok_or("Параметр 'function_name' обязателен")?;
    let db = db_path.as_ref().ok_or("Индекс символов не настроен")?;

    let ctx = index::get_function_context(db, function_name)
        .ok_or_else(|| format!("Функция '{}' не найдена в индексе", function_name))?;

    let kind_label = if ctx.function.kind == "function" { "Функция" } else { "Процедура" };
    let export_label = if ctx.function.is_export { " Экспорт" } else { "" };

    let mut text = format!(
        "## {}{} ({}, {}:{})\n\n",
        ctx.function.name, export_label, kind_label,
        ctx.function.file, ctx.function.start_line
    );

    if ctx.calls.is_empty() {
        text.push_str("**Вызывает:** *(нет вызовов в индексе)*\n\n");
    } else {
        text.push_str(&format!("**Вызывает ({}):**\n", ctx.calls.len()));
        for callee in &ctx.calls {
            text.push_str(&format!("- {}\n", callee));
        }
        text.push('\n');
    }

    if ctx.called_by.is_empty() {
        text.push_str("**Вызывается из:** *(нет вызывающих в индексе)*\n");
    } else {
        text.push_str(&format!("**Вызывается из ({}):**\n", ctx.called_by.len()));
        for caller in &ctx.called_by {
            if caller.start_line > 0 {
                text.push_str(&format!("- {} ({}:{})\n", caller.name, caller.file, caller.start_line));
            } else {
                text.push_str(&format!("- {} ({})\n", caller.name, caller.file));
            }
        }
    }

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

// ─── get_module_functions ────────────────────────────────────────────────────

async fn handle_get_module_functions(
    args: &Value,
    db_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let module_path = args["module_path"].as_str().ok_or("Параметр 'module_path' обязателен")?;
    let limit = args["limit"].as_u64().unwrap_or(200).clamp(1, 500) as usize;
    let db = db_path.as_ref().ok_or("Индекс символов не настроен")?;

    // Resolve "CommonModule.МодульИмя" → "CommonModules/МодульИмя"
    let resolved = if let Some(dot) = module_path.find('.') {
        let type_part = &module_path[..dot];
        let name_part = &module_path[dot + 1..];
        if let Some(folder) = object_type_to_folder(type_part) {
            format!("{}/{}", folder, name_part)
        } else {
            module_path.to_string()
        }
    } else {
        module_path.to_string()
    };

    let symbols = index::get_module_functions(db, &resolved, limit);

    if symbols.is_empty() {
        return Ok(json!({ "content": [{ "type": "text", "text": format!(
            "Модуль «{}» не найден в индексе или не содержит функций.", module_path
        )}] }));
    }

    let first_file = &symbols[0].file;
    let mut text = format!("## Функции модуля `{}`\n\n", first_file);

    let total = symbols.len();
    for sym in &symbols {
        let kind = if sym.kind == "function" { "Функция" } else { "Процедура" };
        let export = if sym.is_export { " Экспорт" } else { "" };
        text.push_str(&format!(
            "- **{}**{} — {} (строка {})\n",
            sym.name, export, kind, sym.start_line
        ));
    }

    if total == limit {
        text.push_str(&format!("\n*Показано первых {} — уточните путь для фильтрации.*", limit));
    }

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

// ─── stats ───────────────────────────────────────────────────────────────────

async fn handle_stats(db_path: &Option<PathBuf>) -> Result<Value, String> {
    let db = db_path.as_ref().ok_or("Индекс символов не настроен")?;
    let s = index::get_index_stats(db);

    let built_at_str = s.built_at
        .map(|ts| {
            // Simple UTC date from Unix timestamp
            let secs = ts;
            let days_total = secs / 86400;
            let time_of_day = secs % 86400;
            let hh = time_of_day / 3600;
            let mm = (time_of_day % 3600) / 60;
            // Approximate date (good enough for display)
            let mut y = 1970u64;
            let mut d = days_total;
            loop {
                let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
                let days_in_year = if leap { 366 } else { 365 };
                if d < days_in_year { break; }
                d -= days_in_year;
                y += 1;
            }
            let month_days = [31u64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            let mut m = 1u64;
            for &md in &month_days {
                if d < md { break; }
                d -= md;
                m += 1;
            }
            format!("{}-{:02}-{:02} {:02}:{:02} UTC", y, m, d + 1, hh, mm)
        })
        .unwrap_or_else(|| "неизвестно".to_string());

    let text = format!(
        "## Статистика индекса\n\
        - Символов (функции/процедуры): {}\n\
        - Проиндексировано файлов: {}\n\
        - Объектов метаданных: {}\n\
        - Рёбер графа вызовов: {}\n\
        - Размер БД: {:.1} МБ\n\
        - Построен: {}",
        s.symbol_count, s.file_count, s.object_count,
        s.calls_count, s.db_size_mb, built_at_str
    );

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

// ─── smart_find ──────────────────────────────────────────────────────────────

async fn handle_smart_find(
    args: &Value,
    config_path: &Option<PathBuf>,
    db_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let query = args["query"].as_str().ok_or("Параметр 'query' обязателен")?;
    if query.trim().is_empty() {
        return Err("Параметр 'query' не может быть пустым".to_string());
    }
    let with_code = args["include_code"].as_bool().unwrap_or(true);

    let db = db_path.as_ref().ok_or("Индекс символов не готов. Убедитесь, что указан путь к конфигурации и индексация завершена.")?;

    let db_clone = db.clone();
    let query_owned = query.to_string();

    // Step 1: find by exact name, fallback to substring
    let results = tokio::task::spawn_blocking(move || {
        let exact = index::find_symbols(&db_clone, &query_owned, true, 5)?;
        if !exact.is_empty() {
            return Ok(exact);
        }
        index::find_symbols(&db_clone, &query_owned, false, 10)
    })
    .await
    .map_err(|e| format!("Ошибка поиска: {}", e))??;

    if results.is_empty() {
        // Fallback to search_code if config available
        if config_path.is_some() {
            let fallback = handle_search_code(
                &json!({"query": query, "limit": 10}),
                config_path,
                db_path,
            )
            .await?;
            let fallback_text = fallback["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .to_string();
            return Ok(json!({ "content": [{ "type": "text", "text": format!(
                "Символ \"{}\" не найден в индексе. Результаты текстового поиска:\n\n{}",
                query, fallback_text
            )}]}));
        }
        return Ok(json!({ "content": [{ "type": "text", "text": format!(
            "Символ \"{}\" не найден в индексе. Проверьте написание имени.", query
        )}]}));
    }

    // Build result text
    let mut text = format!("Найдено {} символ(ов) по запросу \"{}\":\n\n", results.len(), query);
    for r in &results {
        let export_mark = if r.is_export { " Экспорт" } else { "" };
        text.push_str(&format!(
            "- **{}** ({}{}) — `{}` строки {}-{}\n",
            r.name, r.kind, export_mark, r.file, r.start_line, r.end_line
        ));
    }

    // Step 2: append code of best match (prefer export, then first)
    if with_code {
        if let Some(root) = config_path {
            let best = results.iter().find(|r| r.is_export).unwrap_or(&results[0]);
            let file_path = root.join(best.file.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Ok(content) = std::fs::read_to_string(&file_path) {
                let lines: Vec<&str> = content.lines().collect();
                let start = (best.start_line as usize).saturating_sub(1);
                let end = (best.end_line as usize).min(lines.len());
                if start < lines.len() {
                    let body = lines[start..end].join("\n");
                    let export_mark = if best.is_export { " Экспорт" } else { "" };
                    text.push_str(&format!(
                        "\n\n---\n**Код: {}{}** (`{}` строки {}-{}):\n\n```bsl\n{}\n```",
                        best.name, export_mark, best.file,
                        best.start_line, best.end_line, body
                    ));
                }
            }
        }
    }

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

// ─── find_function_in_object ─────────────────────────────────────────────────

async fn handle_find_function_in_object(
    args: &Value,
    config_path: &Option<PathBuf>,
    db_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let object = args["object"].as_str().ok_or("Параметр 'object' обязателен")?;
    let function_hint = args["function_hint"].as_str().unwrap_or("").to_lowercase();
    let db = db_path.as_ref().ok_or("Индекс символов не готов")?;

    // Resolve "Catalog.СтавкиНДС" → "Catalogs/СтавкиНДС"
    let path_prefix = if let Some(dot) = object.find('.') {
        let type_part = &object[..dot];
        let name_part = &object[dot + 1..];
        if let Some(folder) = object_type_to_folder(type_part) {
            format!("{}/{}", folder, name_part)
        } else {
            object.to_string()
        }
    } else {
        object.to_string()
    };

    let symbols = index::get_module_functions(db, &path_prefix, 500);

    if symbols.is_empty() {
        return Ok(json!({ "content": [{ "type": "text", "text": format!(
            "Объект «{}» не найден в индексе или не содержит функций. Проверьте имя или запустите переиндексацию.", object
        )}]}));
    }

    // Filter by hint
    let matched: Vec<&index::SymbolMatch> = if function_hint.is_empty() {
        symbols.iter().collect()
    } else {
        symbols.iter().filter(|s| s.name.to_lowercase().contains(&function_hint)).collect()
    };

    let mut text = format!("## Функции объекта `{}`\n\n", object);
    text.push_str(&format!(
        "Всего функций: {} | Совпадений по «{}»: {}\n\n",
        symbols.len(),
        if function_hint.is_empty() { "все" } else { &function_hint },
        matched.len()
    ));

    if matched.is_empty() {
        text.push_str("Функций, соответствующих подсказке, не найдено.\n\n**Первые 30 функций объекта:**\n");
        for s in symbols.iter().take(30) {
            let export = if s.is_export { " Экспорт" } else { "" };
            text.push_str(&format!("- **{}**{} (строка {})\n", s.name, export, s.start_line));
        }
    } else {
        for s in &matched {
            let export = if s.is_export { " Экспорт" } else { "" };
            text.push_str(&format!("- **{}**{} (строка {})\n", s.name, export, s.start_line));
        }

        // Code of best match (prefer export)
        let best = matched.iter().find(|s| s.is_export).copied().unwrap_or(matched[0]);
        if let Some(root) = config_path {
            let file_path = root.join(best.file.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Ok(content) = std::fs::read_to_string(&file_path) {
                let lines: Vec<&str> = content.lines().collect();
                let start = (best.start_line as usize).saturating_sub(1);
                let end = (best.end_line as usize).min(lines.len());
                if start < lines.len() {
                    let body = lines[start..end].join("\n");
                    let export_mark = if best.is_export { " Экспорт" } else { "" };
                    text.push_str(&format!(
                        "\n\n---\n**Код: {}{}** (`{}`):\n\n```bsl\n{}\n```",
                        best.name, export_mark, best.file, body
                    ));
                }
            }
        }
    }

    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

// ─── benchmark ───────────────────────────────────────────────────────────────

async fn handle_benchmark(
    args: &Value,
    config_path: &Option<PathBuf>,
    db_path: &Option<PathBuf>,
) -> Result<Value, String> {
    let db = db_path.as_ref().ok_or("Индекс не настроен")?;
    let n = (args["iterations"].as_u64().unwrap_or(20) as usize).min(100).max(3);

    // ── Sample data from the index for realistic queries ──────────────────────
    let (sample_symbol, sample_prefix, sample_file) = {
        let conn = rusqlite::Connection::open(db).map_err(|e| e.to_string())?;
        let sym: String = conn
            .query_row(
                "SELECT name FROM symbols WHERE kind='function' AND LENGTH(name) > 4 ORDER BY RANDOM() LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "ОбщийМодуль".to_string());
        let prefix: String = sym.chars().take(4).collect();
        let file: String = conn
            .query_row("SELECT filepath FROM indexed_files ORDER BY RANDOM() LIMIT 1", [], |r| r.get(0))
            .unwrap_or_default();
        (sym, prefix, file)
    };

    // ── Timing helper ─────────────────────────────────────────────────────────
    let calc_stats = |mut times: Vec<u128>| -> serde_json::Value {
        times.sort_unstable();
        let len = times.len();
        let min = times[0];
        let max = times[len - 1];
        let avg = times.iter().sum::<u128>() / len as u128;
        let p95_idx = ((len as f64 * 0.95) as usize).min(len - 1);
        let p95 = times[p95_idx];
        json!({ "min_ms": min, "avg_ms": avg, "p95_ms": p95, "max_ms": max, "n": len })
    };

    let mut results: Vec<serde_json::Value> = vec![];

    // ── 1. find_symbol — точный (SQLite PK lookup) ────────────────────────────
    {
        let mut times = Vec::with_capacity(n);
        for _ in 0..n {
            let t = std::time::Instant::now();
            let _ = handle_find_symbol(&json!({"query": sample_symbol, "exact": true, "limit": 10}), db_path).await;
            times.push(t.elapsed().as_millis());
        }
        let mut s = calc_stats(times);
        s["tool"] = json!("find_symbol (точный)");
        s["description"] = json!("SQLite WHERE name_lower = ?");
        results.push(s);
    }

    // ── 2. find_symbol — prefix LIKE ─────────────────────────────────────────
    {
        let mut times = Vec::with_capacity(n);
        for _ in 0..n {
            let t = std::time::Instant::now();
            let _ = handle_find_symbol(&json!({"query": sample_prefix, "exact": false, "limit": 20}), db_path).await;
            times.push(t.elapsed().as_millis());
        }
        let mut s = calc_stats(times);
        s["tool"] = json!("find_symbol (prefix)");
        s["description"] = json!("SQLite WHERE name_lower LIKE ?%");
        results.push(s);
    }

    // ── 3. get_function_context ───────────────────────────────────────────────
    {
        let mut times = Vec::with_capacity(n);
        for _ in 0..n {
            let t = std::time::Instant::now();
            let _ = handle_get_function_context(&json!({"function_name": sample_symbol}), db_path).await;
            times.push(t.elapsed().as_millis());
        }
        let mut s = calc_stats(times);
        s["tool"] = json!("get_function_context");
        s["description"] = json!("SQLite + чтение диапазона строк файла");
        results.push(s);
    }

    // ── 4. get_module_functions ───────────────────────────────────────────────
    if !sample_file.is_empty() {
        let mut times = Vec::with_capacity(n);
        for _ in 0..n {
            let t = std::time::Instant::now();
            let _ = handle_get_module_functions(&json!({"file": sample_file}), db_path).await;
            times.push(t.elapsed().as_millis());
        }
        let mut s = calc_stats(times);
        s["tool"] = json!("get_module_functions");
        s["description"] = json!("SQLite WHERE file = ?");
        results.push(s);
    }

    // ── 5. list_objects ───────────────────────────────────────────────────────
    {
        let mut times = Vec::with_capacity(n);
        for _ in 0..n {
            let t = std::time::Instant::now();
            let _ = handle_list_objects(&json!({}), db_path).await;
            times.push(t.elapsed().as_millis());
        }
        let mut s = calc_stats(times);
        s["tool"] = json!("list_objects");
        s["description"] = json!("SQLite GROUP BY из таблицы метаданных");
        results.push(s);
    }

    // ── 6. stats ──────────────────────────────────────────────────────────────
    {
        let mut times = Vec::with_capacity(n);
        for _ in 0..n {
            let t = std::time::Instant::now();
            let _ = handle_stats(db_path).await;
            times.push(t.elapsed().as_millis());
        }
        let mut s = calc_stats(times);
        s["tool"] = json!("stats");
        s["description"] = json!("SQLite COUNT агрегаты");
        results.push(s);
    }

    // ── 7. get_file_context — baseline: чтение файла с диска ─────────────────
    if config_path.is_some() && !sample_file.is_empty() {
        let n_slow = n.min(10);
        let mut times = Vec::with_capacity(n_slow);
        for _ in 0..n_slow {
            let t = std::time::Instant::now();
            let _ = handle_get_file_context(&json!({"file": sample_file, "line": 1}), config_path).await;
            times.push(t.elapsed().as_millis());
        }
        let mut s = calc_stats(times);
        s["tool"] = json!("get_file_context");
        s["description"] = json!("Чтение файла с диска (baseline)");
        results.push(s);
    }

    // ── 8. search_code — ripgrep, 1 прогрев + 1 замер (тяжёлая операция) ──────
    // Не включаем в цикл N итераций — одиночный ripgrep-скан уже репрезентативен.
    if config_path.is_some() {
        // Прогрев (ОС кэширует файлы)
        let _ = handle_search_code(&json!({"query": &sample_prefix, "max_results": 10}), config_path, db_path).await;
        let t = std::time::Instant::now();
        let _ = handle_search_code(&json!({"query": &sample_prefix, "max_results": 10}), config_path, db_path).await;
        let elapsed = t.elapsed().as_millis();
        results.push(json!({
            "tool": "search_code",
            "description": "ripgrep по всем BSL/XML файлам (1 замер после прогрева)",
            "min_ms": elapsed, "avg_ms": elapsed, "p95_ms": elapsed, "max_ms": elapsed, "n": 1
        }));
    }

    let db_size_mb = crate::db_size_mb(db);
    let symbol_count = index::symbol_count(db);

    Ok(json!({
        "iterations": n,
        "sample_symbol": sample_symbol,
        "sample_file": sample_file,
        "db_size_mb": db_size_mb,
        "symbol_count": symbol_count,
        "results": results
    }))
}
