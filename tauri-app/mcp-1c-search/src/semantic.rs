/// Semantic search module for mcp-1c-search.
///
/// Implements CamelCase/PascalCase tokenizer, FTS5-backed symbol search,
/// domain synonym expansion for 1C BSL code, and call-graph-weighted ranking.

use rusqlite::{Connection, params};

// ─── CamelCase / PascalCase tokenizer ────────────────────────────────────────

/// Split a 1C identifier into semantic tokens.
///
/// "СтавкаНДСПоЗначениюПеречисления" → ["Ставка","НДС","По","Значению","Перечисления"]
/// "ЗначениеРеквизитаОбъекта"        → ["Значение","Реквизита","Объекта"]
/// "GetNDSValue"                      → ["Get","NDS","Value"]
pub fn tokenize_identifier(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let n = chars.len();
    let mut tokens: Vec<String> = Vec::new();
    let mut start = 0;

    let is_upper = |c: char| c.is_uppercase();
    let is_lower = |c: char| c.is_lowercase();
    let is_letter = |c: char| c.is_alphabetic();

    let mut i = 0;
    while i < n {
        if !is_letter(chars[i]) {
            // Skip non-letter chars (digits, underscores)
            if i > start {
                let tok: String = chars[start..i].iter().collect();
                if tok.chars().count() >= 2 {
                    tokens.push(tok);
                }
            }
            start = i + 1;
            i += 1;
            continue;
        }

        // Detect boundary: transition from lowercase to uppercase
        if i > start && is_upper(chars[i]) {
            if is_lower(chars[i - 1]) {
                // e.g. "Ставка|НДС" or "Value|NDS"
                let tok: String = chars[start..i].iter().collect();
                if tok.chars().count() >= 2 {
                    tokens.push(tok);
                }
                start = i;
            } else if is_upper(chars[i - 1]) && i + 1 < n && is_lower(chars[i + 1]) {
                // e.g. "НД|С|По" → end of acronym before next word
                let tok: String = chars[start..i].iter().collect();
                if tok.chars().count() >= 2 {
                    tokens.push(tok);
                }
                start = i;
            }
        }
        i += 1;
    }

    // Last token
    if start < n {
        let tok: String = chars[start..].iter().collect();
        if tok.chars().count() >= 2 {
            tokens.push(tok);
        }
    }

    tokens
}

/// Tokenize identifier to lowercase tokens (for FTS/search matching).
pub fn tokenize_lower(name: &str) -> Vec<String> {
    tokenize_identifier(name)
        .into_iter()
        .map(|t| t.to_lowercase())
        .collect()
}

/// Convert tokens to FTS5 MATCH expression joined by OR.
#[allow(dead_code)]
pub fn tokens_to_fts_query(tokens: &[String]) -> String {
    if tokens.is_empty() {
        return String::new();
    }
    // Quote each token for FTS5 safety
    tokens.iter()
        .map(|t| {
            let clean: String = t.chars().filter(|c| c.is_alphanumeric()).collect();
            clean
        })
        .filter(|t| !t.is_empty() && t.chars().count() >= 2)
        .collect::<Vec<_>>()
        .join(" OR ")
}

// ─── Domain synonym dictionary ─────────────────────────────────────────────

/// A synonym entry: query term → alias token with weight.
pub struct DomainAlias {
    pub term: &'static str,
    pub alias: &'static str,
    pub weight: f64,
}

/// Built-in 1C domain synonym dictionary (~300 entries).
/// Maps natural language words to tokens commonly found in BSL function names.
pub const INITIAL_ALIASES: &[DomainAlias] = &[
    // === Конвертация / Преобразование ===
    DomainAlias { term: "превратить",     alias: "По",          weight: 0.8 },
    DomainAlias { term: "превратить",     alias: "Из",          weight: 0.8 },
    DomainAlias { term: "превратить",     alias: "Конвертация", weight: 0.9 },
    DomainAlias { term: "превратить",     alias: "ПоЗначению",  weight: 0.8 },
    DomainAlias { term: "превращает",     alias: "По",          weight: 0.8 },
    DomainAlias { term: "превращает",     alias: "Из",          weight: 0.8 },
    DomainAlias { term: "превращает",     alias: "Конвертация", weight: 0.9 },
    DomainAlias { term: "превращает",     alias: "ПоЗначению",  weight: 0.8 },
    DomainAlias { term: "конвертировать", alias: "Конвертация", weight: 1.0 },
    DomainAlias { term: "конвертировать", alias: "По",          weight: 0.7 },
    DomainAlias { term: "преобразовать",  alias: "Конвертация", weight: 0.9 },
    DomainAlias { term: "преобразовать",  alias: "По",          weight: 0.7 },
    DomainAlias { term: "перевести",      alias: "По",          weight: 0.8 },
    DomainAlias { term: "перевести",      alias: "Конвертация", weight: 0.7 },
    DomainAlias { term: "из",             alias: "По",          weight: 0.6 },
    DomainAlias { term: "из",             alias: "Из",          weight: 1.0 },
    DomainAlias { term: "в",              alias: "По",          weight: 0.5 },

    // === Получение / Нахождение ===
    DomainAlias { term: "найти",          alias: "Найти",       weight: 1.0 },
    DomainAlias { term: "найти",          alias: "Получить",    weight: 0.7 },
    DomainAlias { term: "найти",          alias: "Поиск",       weight: 0.8 },
    DomainAlias { term: "найти",          alias: "По",          weight: 0.6 },
    DomainAlias { term: "получить",       alias: "Получить",    weight: 1.0 },
    DomainAlias { term: "получить",       alias: "Найти",       weight: 0.7 },
    DomainAlias { term: "получить",       alias: "Вычислить",   weight: 0.6 },
    DomainAlias { term: "взять",          alias: "Получить",    weight: 0.9 },
    DomainAlias { term: "достать",        alias: "Получить",    weight: 0.8 },
    DomainAlias { term: "прочитать",      alias: "Получить",    weight: 0.7 },
    DomainAlias { term: "прочитать",      alias: "Загрузить",   weight: 0.8 },
    DomainAlias { term: "определить",     alias: "Определить",  weight: 1.0 },
    DomainAlias { term: "определить",     alias: "Вычислить",   weight: 0.8 },
    DomainAlias { term: "вычислить",      alias: "Вычислить",   weight: 1.0 },
    DomainAlias { term: "рассчитать",     alias: "Рассчитать",  weight: 1.0 },
    DomainAlias { term: "рассчитать",     alias: "Вычислить",   weight: 0.8 },

    // === Проверка / Валидация ===
    DomainAlias { term: "проверить",      alias: "Проверить",   weight: 1.0 },
    DomainAlias { term: "проверить",      alias: "Проверка",    weight: 0.9 },
    DomainAlias { term: "проверить",      alias: "Контроль",    weight: 0.7 },
    DomainAlias { term: "проверить",      alias: "Корректность",weight: 0.8 },
    DomainAlias { term: "валидировать",   alias: "Проверить",   weight: 0.9 },
    DomainAlias { term: "валидировать",   alias: "Валидация",   weight: 1.0 },
    DomainAlias { term: "убедиться",      alias: "Проверить",   weight: 0.8 },

    // === Заполнение / Запись ===
    DomainAlias { term: "заполнить",      alias: "Заполнить",   weight: 1.0 },
    DomainAlias { term: "заполнить",      alias: "Установить",  weight: 0.7 },
    DomainAlias { term: "установить",     alias: "Установить",  weight: 1.0 },
    DomainAlias { term: "установить",     alias: "Заполнить",   weight: 0.7 },
    DomainAlias { term: "записать",       alias: "Записать",    weight: 1.0 },
    DomainAlias { term: "записать",       alias: "Сохранить",   weight: 0.8 },
    DomainAlias { term: "сохранить",      alias: "Сохранить",   weight: 1.0 },
    DomainAlias { term: "сохранить",      alias: "Записать",    weight: 0.8 },
    DomainAlias { term: "добавить",       alias: "Добавить",    weight: 1.0 },
    DomainAlias { term: "вставить",       alias: "Вставить",    weight: 1.0 },
    DomainAlias { term: "провести",       alias: "Провести",    weight: 1.0 },

    // === Удаление / Очистка ===
    DomainAlias { term: "удалить",        alias: "Удалить",     weight: 1.0 },
    DomainAlias { term: "очистить",       alias: "Очистить",    weight: 1.0 },
    DomainAlias { term: "очистить",       alias: "Сбросить",    weight: 0.8 },
    DomainAlias { term: "убрать",         alias: "Удалить",     weight: 0.8 },
    DomainAlias { term: "убрать",         alias: "Очистить",    weight: 0.8 },
    DomainAlias { term: "сбросить",       alias: "Сбросить",    weight: 1.0 },

    // === Форматирование / Представление ===
    DomainAlias { term: "отформатировать",alias: "Формат",      weight: 1.0 },
    DomainAlias { term: "вывести",        alias: "Вывод",       weight: 0.9 },
    DomainAlias { term: "вывести",        alias: "Представление",weight: 0.7 },
    DomainAlias { term: "представить",    alias: "Представление",weight: 1.0 },
    DomainAlias { term: "отобразить",     alias: "Отображение", weight: 1.0 },
    DomainAlias { term: "показать",       alias: "Показать",    weight: 1.0 },

    // === Открытие / Создание ===
    DomainAlias { term: "открыть",        alias: "Открыть",     weight: 1.0 },
    DomainAlias { term: "создать",        alias: "Создать",     weight: 1.0 },
    DomainAlias { term: "создать",        alias: "Новый",       weight: 0.7 },
    DomainAlias { term: "инициализировать",alias: "Инициализация",weight: 1.0 },
    DomainAlias { term: "инициализировать",alias: "Создать",    weight: 0.6 },

    // === Объекты метаданных 1С ===
    DomainAlias { term: "справочник",     alias: "Справочник",  weight: 1.0 },
    DomainAlias { term: "справочник",     alias: "Каталог",     weight: 0.7 },
    DomainAlias { term: "элемент",        alias: "Ссылка",      weight: 0.7 },
    DomainAlias { term: "перечисление",   alias: "Перечисление",weight: 1.0 },
    DomainAlias { term: "перечисление",   alias: "Перечислений",weight: 0.9 },
    DomainAlias { term: "перечисления",   alias: "Перечисление",weight: 1.0 },
    DomainAlias { term: "документ",       alias: "Документ",    weight: 1.0 },
    DomainAlias { term: "накладная",      alias: "Накладная",   weight: 1.0 },
    DomainAlias { term: "накладная",      alias: "Документ",    weight: 0.6 },
    DomainAlias { term: "счёт",           alias: "Счет",        weight: 1.0 },
    DomainAlias { term: "счет",           alias: "Счет",        weight: 1.0 },
    DomainAlias { term: "регистр",        alias: "Регистр",     weight: 1.0 },
    DomainAlias { term: "отчёт",          alias: "Отчет",       weight: 1.0 },
    DomainAlias { term: "отчет",          alias: "Отчет",       weight: 1.0 },
    DomainAlias { term: "обработка",      alias: "Обработка",   weight: 1.0 },
    DomainAlias { term: "план",           alias: "План",        weight: 0.9 },
    DomainAlias { term: "бизнес-процесс", alias: "БизнесПроцесс",weight: 1.0 },
    DomainAlias { term: "задача",         alias: "Задача",      weight: 0.8 },

    // === НДС ===
    DomainAlias { term: "ндс",            alias: "НДС",         weight: 1.0 },
    DomainAlias { term: "налог",          alias: "НДС",         weight: 0.8 },
    DomainAlias { term: "налог",          alias: "Налог",       weight: 1.0 },
    DomainAlias { term: "ставка",         alias: "Ставка",      weight: 1.0 },
    DomainAlias { term: "ставка",         alias: "Ставки",      weight: 0.9 },
    DomainAlias { term: "налогообложение",alias: "Налогообложение",weight: 1.0 },
    DomainAlias { term: "налогообложение",alias: "НДС",         weight: 0.6 },

    // === Финансы / Деньги ===
    DomainAlias { term: "сумма",          alias: "Сумма",       weight: 1.0 },
    DomainAlias { term: "цена",           alias: "Цена",        weight: 1.0 },
    DomainAlias { term: "стоимость",      alias: "Стоимость",   weight: 1.0 },
    DomainAlias { term: "стоимость",      alias: "Цена",        weight: 0.6 },
    DomainAlias { term: "оплата",         alias: "Оплата",      weight: 1.0 },
    DomainAlias { term: "оплата",         alias: "Платёж",      weight: 0.8 },
    DomainAlias { term: "платёж",         alias: "Платёж",      weight: 1.0 },
    DomainAlias { term: "платеж",         alias: "Платеж",      weight: 1.0 },
    DomainAlias { term: "валюта",         alias: "Валюта",      weight: 1.0 },
    DomainAlias { term: "курс",           alias: "Курс",        weight: 0.9 },

    // === Контрагент / Организация ===
    DomainAlias { term: "контрагент",     alias: "Контрагент",  weight: 1.0 },
    DomainAlias { term: "организация",    alias: "Организация", weight: 1.0 },
    DomainAlias { term: "партнёр",        alias: "Партнер",     weight: 1.0 },
    DomainAlias { term: "партнер",        alias: "Партнер",     weight: 1.0 },
    DomainAlias { term: "покупатель",     alias: "Покупатель",  weight: 1.0 },
    DomainAlias { term: "продавец",       alias: "Продавец",    weight: 1.0 },
    DomainAlias { term: "поставщик",      alias: "Поставщик",   weight: 1.0 },

    // === Склад / Номенклатура ===
    DomainAlias { term: "товар",          alias: "Товар",       weight: 1.0 },
    DomainAlias { term: "товар",          alias: "Номенклатура",weight: 0.7 },
    DomainAlias { term: "номенклатура",   alias: "Номенклатура",weight: 1.0 },
    DomainAlias { term: "склад",          alias: "Склад",       weight: 1.0 },
    DomainAlias { term: "остаток",        alias: "Остаток",     weight: 1.0 },
    DomainAlias { term: "количество",     alias: "Количество",  weight: 1.0 },

    // === Пользователь / Права ===
    DomainAlias { term: "пользователь",   alias: "Пользователь",weight: 1.0 },
    DomainAlias { term: "роль",           alias: "Роль",        weight: 1.0 },
    DomainAlias { term: "права",          alias: "Права",       weight: 1.0 },
    DomainAlias { term: "доступ",         alias: "Доступ",      weight: 1.0 },
    DomainAlias { term: "разрешение",     alias: "Разрешение",  weight: 0.8 },

    // === Дата / Время ===
    DomainAlias { term: "дата",           alias: "Дата",        weight: 1.0 },
    DomainAlias { term: "период",         alias: "Период",      weight: 1.0 },
    DomainAlias { term: "месяц",          alias: "Месяц",       weight: 1.0 },
    DomainAlias { term: "год",            alias: "Год",         weight: 0.8 },
    DomainAlias { term: "квартал",        alias: "Квартал",     weight: 1.0 },

    // === Строки / Текст ===
    DomainAlias { term: "строка",         alias: "Строка",      weight: 0.9 },
    DomainAlias { term: "текст",          alias: "Текст",       weight: 0.9 },
    DomainAlias { term: "наименование",   alias: "Наименование",weight: 1.0 },
    DomainAlias { term: "описание",       alias: "Описание",    weight: 0.9 },
    DomainAlias { term: "код",            alias: "Код",         weight: 0.9 },
    DomainAlias { term: "артикул",        alias: "Артикул",     weight: 1.0 },

    // === Печать / Форма ===
    DomainAlias { term: "печать",         alias: "Печать",      weight: 1.0 },
    DomainAlias { term: "форма",          alias: "Форма",       weight: 0.9 },
    DomainAlias { term: "табличная",      alias: "Таблица",     weight: 0.8 },
    DomainAlias { term: "макет",          alias: "Макет",       weight: 1.0 },

    // === Обмен / Интеграция ===
    DomainAlias { term: "обмен",          alias: "Обмен",       weight: 1.0 },
    DomainAlias { term: "синхронизация",  alias: "Синхронизация",weight: 1.0 },
    DomainAlias { term: "загрузить",      alias: "Загрузить",   weight: 1.0 },
    DomainAlias { term: "выгрузить",      alias: "Выгрузить",   weight: 1.0 },
    DomainAlias { term: "экспорт",        alias: "Экспорт",     weight: 1.0 },
    DomainAlias { term: "импорт",         alias: "Импорт",      weight: 1.0 },
    DomainAlias { term: "отправить",      alias: "Отправить",   weight: 1.0 },
    DomainAlias { term: "получить",       alias: "Получить",    weight: 1.0 },

    // === Ошибки / Логирование ===
    DomainAlias { term: "ошибка",         alias: "Ошибка",      weight: 1.0 },
    DomainAlias { term: "исключение",     alias: "Исключение",  weight: 1.0 },
    DomainAlias { term: "лог",            alias: "Журнал",      weight: 0.8 },
    DomainAlias { term: "журнал",         alias: "Журнал",      weight: 1.0 },

    // === Значение / Реквизит ===
    DomainAlias { term: "значение",       alias: "Значение",    weight: 1.0 },
    DomainAlias { term: "значение",       alias: "По",          weight: 0.5 },
    DomainAlias { term: "реквизит",       alias: "Реквизит",    weight: 1.0 },
    DomainAlias { term: "атрибут",        alias: "Реквизит",    weight: 0.8 },
    DomainAlias { term: "свойство",       alias: "Реквизит",    weight: 0.7 },
    DomainAlias { term: "поле",           alias: "Реквизит",    weight: 0.6 },
    DomainAlias { term: "параметр",       alias: "Параметр",    weight: 0.9 },
];

// ─── Schema ───────────────────────────────────────────────────────────────────

/// Create semantic tables if they don't exist.
pub fn ensure_semantic_schema(conn: &Connection) {
    // FTS5 virtual table for tokenized symbol names + comments
    let _ = conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS symbol_terms USING fts5(
            symbol_id UNINDEXED,
            name_tokens,
            comment_head,
            param_tokens,
            tokenize='unicode61 remove_diacritics 1'
        );
        CREATE TABLE IF NOT EXISTS symbol_weights (
            symbol_id   INTEGER PRIMARY KEY,
            call_count  INTEGER NOT NULL DEFAULT 0,
            caller_diversity INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS domain_aliases (
            id      INTEGER PRIMARY KEY,
            term    TEXT NOT NULL,
            alias   TEXT NOT NULL,
            weight  REAL NOT NULL DEFAULT 1.0
        );
        CREATE INDEX IF NOT EXISTS idx_alias_term ON domain_aliases(term);"
    );

    // Populate domain_aliases if empty
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM domain_aliases", [], |r| r.get(0))
        .unwrap_or(0);
    if count == 0 {
        if let Ok(tx) = conn.unchecked_transaction() {
            for alias in INITIAL_ALIASES {
                let _ = tx.execute(
                    "INSERT OR IGNORE INTO domain_aliases (term, alias, weight) VALUES (?1, ?2, ?3)",
                    params![alias.term, alias.alias, alias.weight],
                );
            }
            let _ = tx.commit();
        }
    }
}

// ─── Indexing helpers ─────────────────────────────────────────────────────────

/// Insert or replace FTS5 entry for a symbol.
#[allow(dead_code)]
pub fn upsert_symbol_terms(
    conn: &Connection,
    symbol_id: i64,
    name: &str,
    comment_head: &str,
    params_str: &str,
) {
    // Delete old entry first (FTS5 content='' requires manual delete)
    let _ = conn.execute(
        "DELETE FROM symbol_terms WHERE symbol_id = ?1",
        params![symbol_id.to_string()],
    );

    let name_tokens = tokenize_identifier(name).join(" ");
    let _ = conn.execute(
        "INSERT INTO symbol_terms (symbol_id, name_tokens, comment_head, param_tokens)
         VALUES (?1, ?2, ?3, ?4)",
        params![symbol_id.to_string(), name_tokens, comment_head, params_str],
    );
}

/// Extract comment lines above start_line from BSL source.
/// Returns up to 5 lines that start with "//" immediately before the function.
#[allow(dead_code)]
pub fn extract_comment_head(source: &str, start_line: u32) -> String {
    if start_line == 0 {
        return String::new();
    }
    let lines: Vec<&str> = source.lines().collect();
    let func_idx = (start_line as usize).saturating_sub(1); // 0-based

    let mut comment_lines: Vec<&str> = Vec::new();
    let mut i = func_idx.saturating_sub(1);
    loop {
        let line = lines.get(i).map(|l| l.trim()).unwrap_or("");
        if line.starts_with("//") {
            comment_lines.push(line.trim_start_matches('/').trim());
        } else {
            break;
        }
        if comment_lines.len() >= 5 || i == 0 {
            break;
        }
        i = i.saturating_sub(1);
    }
    comment_lines.reverse();
    comment_lines.join(" ")
}

/// Rebuild symbol_weights from the calls graph.
/// Should be called after a full build or sync.
pub fn rebuild_symbol_weights(conn: &Connection) {
    let _ = conn.execute("DELETE FROM symbol_weights", []);
    let _ = conn.execute_batch(
        "INSERT INTO symbol_weights (symbol_id, call_count, caller_diversity)
         SELECT s.id,
                COUNT(c.id),
                COUNT(DISTINCT c.caller_file)
         FROM symbols s
         LEFT JOIN calls c ON c.callee_name_lower = s.name_lower
         GROUP BY s.id;"
    );
}

// ─── Synonym expansion ────────────────────────────────────────────────────────

/// Expand a list of tokens using the domain_aliases table.
/// Returns (original tokens + alias tokens) deduplicated, with max weight per token.
pub fn expand_with_aliases(
    conn: &Connection,
    tokens: &[String],
) -> Vec<(String, f64)> {
    let mut result: std::collections::HashMap<String, f64> = std::collections::HashMap::new();

    // Add originals at weight 1.0
    for t in tokens {
        result.insert(t.to_lowercase(), 1.0_f64);
    }

    // Look up aliases for each token
    for t in tokens {
        let lower = t.to_lowercase();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT alias, weight FROM domain_aliases WHERE term = ?1"
        ) {
            let _ = stmt.query_map(params![lower], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            }).map(|rows| {
                for row in rows.flatten() {
                    let alias_lower = row.0.to_lowercase();
                    let w = row.1;
                    let entry = result.entry(alias_lower).or_insert(0.0);
                    if w > *entry { *entry = w; }
                }
            });
        }
    }

    result.into_iter().collect()
}

// ─── Search ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
#[allow(dead_code)]
pub struct SemanticResult {
    pub symbol_id: i64,
    pub name: String,
    pub kind: String,
    pub file: String,
    pub start_line: i64,
    pub end_line: i64,
    pub is_export: bool,
    pub bm25_score: f64,
    pub call_count: i64,
    pub caller_diversity: i64,
    pub final_score: f64,
}

/// Tokenize a free-text query into lowercase tokens.
/// Splits on spaces/punctuation, then applies CamelCase splitting to each word,
/// skips stop words. "СтавкиНДС" → ["ставки","ндс"].
pub fn tokenize_query(query: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        // Предлоги, союзы, местоимения
        "и", "в", "на", "с", "по", "из", "для", "от", "до", "за", "к", "о",
        "что", "как", "это", "не", "а", "но", "или", "то", "же", "при",
        "который", "которая", "которые", "которого", "которую",
        // Слова-описания поиска (не часть имён BSL)
        "найди", "найти", "покажи", "покажите", "какая", "какой", "какие",
        "нужно", "надо", "хочу", "хочется",
        // Типовые технические слова (описывают контекст, не имя функции)
        "функция", "функцию", "функции", "процедура", "процедуру", "метод",
        "делает", "делать", "выполняет", "выполнять", "возвращает",
        // Родительный/дательный падеж типов 1С — описывают тип данных, не имя
        "конфигурации", "конфигурация",
        "элемента", "объекта",
        "документа", "справочника", "регистра", "перечислений",
        "строки", "реквизита", "реквизитов",
        "значения",  // "значение" оставляем (имя функции ЗначениеРеквизита)
    ];

    query
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .flat_map(|w| {
            // Apply CamelCase split: "СтавкиНДС" → ["Ставки","НДС"]
            let parts = tokenize_identifier(w);
            if parts.len() > 1 {
                parts
            } else {
                vec![w.to_string()]
            }
        })
        .map(|w| w.to_lowercase())
        .filter(|w| w.chars().count() >= 2 && !STOP_WORDS.contains(&w.as_str()))
        .collect()
}

/// Run semantic search: tokenize query → expand with aliases → FTS5 → rank.
pub fn semantic_search(
    conn: &Connection,
    query: &str,
    context_object_names: &[String],
    limit: usize,
) -> Vec<SemanticResult> {
    // 1. Tokenize query (split on spaces + expand CamelCase for each word)
    let query_tokens = tokenize_query(query);
    if query_tokens.is_empty() {
        return vec![];
    }

    // 2. Expand with domain aliases
    let expanded = expand_with_aliases(conn, &query_tokens);
    if expanded.is_empty() {
        return vec![];
    }

    // 3. Build FTS5 MATCH expression with prefix matching (token* matches prefixes)
    let fts_terms: Vec<String> = expanded.iter()
        .map(|(t, _)| {
            let clean: String = t.chars().filter(|c| c.is_alphanumeric()).collect();
            clean
        })
        .filter(|t| t.chars().count() >= 2)
        .map(|t| format!("{}*", t))   // prefix match: "ставки*" matches "СтавкаНДС" tokens
        .collect();

    if fts_terms.is_empty() {
        return vec![];
    }

    let fts_query = fts_terms.join(" OR ");

    // 4. Compute domain object boost: names from context_objects
    let domain_tokens: Vec<String> = context_object_names.iter()
        .flat_map(|obj| {
            // "Catalog.СтавкиНДС" → ["СтавкиНДС", "Ставки", "НДС"]
            let name = obj.splitn(2, '.').nth(1).unwrap_or(obj.as_str());
            let mut tokens = tokenize_lower(name);
            tokens.push(name.to_lowercase());
            tokens
        })
        .collect();

    // 5. Run FTS5 query
    let sql = format!(
        "SELECT
             CAST(st.symbol_id AS INTEGER) AS sid,
             s.name, s.kind, s.file, s.start_line, s.end_line, s.is_export,
             bm25(symbol_terms) AS bm25,
             COALESCE(sw.call_count, 0) AS call_count,
             COALESCE(sw.caller_diversity, 0) AS caller_diversity
         FROM symbol_terms st
         JOIN symbols s ON s.id = CAST(st.symbol_id AS INTEGER)
         LEFT JOIN symbol_weights sw ON sw.symbol_id = s.id
         WHERE symbol_terms MATCH ?1
         ORDER BY bm25(symbol_terms)
         LIMIT {}",
        limit * 5  // fetch more, rerank below
    );

    let mut candidates: Vec<SemanticResult> = Vec::new();

    if let Ok(mut stmt) = conn.prepare(&sql) {
        let _ = stmt.query_map(params![fts_query], |row| {
            Ok(SemanticResult {
                symbol_id:       row.get(0)?,
                name:            row.get(1)?,
                kind:            row.get(2)?,
                file:            row.get(3)?,
                start_line:      row.get(4)?,
                end_line:        row.get(5)?,
                is_export:       row.get::<_, i32>(6)? != 0,
                bm25_score:      row.get(7)?,
                call_count:      row.get(8)?,
                caller_diversity:row.get(9)?,
                final_score:     0.0,
            })
        }).map(|rows| {
            for r in rows.flatten() {
                candidates.push(r);
            }
        });
    }

    // 6. Rerank: bm25 × call_weight × domain_weight
    for c in &mut candidates {
        // BM25 in SQLite FTS5: negative, lower = better match → invert
        let bm25_norm = 1.0 / (1.0 + (-c.bm25_score).max(0.0));

        // Call popularity boost (logarithmic, capped to ~2.4x for 3000 calls)
        let call_weight = 1.0 + (1.0 + c.call_count as f64).ln() * 0.15;

        // Caller diversity boost (logarithmic, capped to ~1.7x for 100 unique callers)
        let diversity_weight = 1.0 + (1.0 + c.caller_diversity as f64).ln() * 0.15;

        // Domain boost: does the file path contain any context object token?
        let domain_weight = if domain_tokens.is_empty() {
            1.0
        } else {
            let file_lower = c.file.to_lowercase();
            let name_lower = c.name.to_lowercase();
            let matches = domain_tokens.iter()
                .filter(|t| file_lower.contains(t.as_str()) || name_lower.contains(t.as_str()))
                .count();
            1.0 + matches as f64 * 0.3
        };

        c.final_score = bm25_norm * call_weight * diversity_weight * domain_weight;
    }

    // Sort by final_score descending
    candidates.sort_by(|a, b| b.final_score.partial_cmp(&a.final_score).unwrap_or(std::cmp::Ordering::Equal));
    candidates.truncate(limit);
    candidates
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_cyrillic() {
        let tokens = tokenize_identifier("СтавкаНДСПоЗначениюПеречисления");
        assert!(tokens.contains(&"Ставка".to_string()), "expected Ставка, got {:?}", tokens);
        assert!(tokens.contains(&"НДС".to_string()), "expected НДС, got {:?}", tokens);
        assert!(tokens.contains(&"По".to_string()), "expected По, got {:?}", tokens);
        assert!(tokens.contains(&"Перечисления".to_string()), "expected Перечисления, got {:?}", tokens);
    }

    #[test]
    fn test_tokenize_latin() {
        let tokens = tokenize_identifier("GetNDSValue");
        assert!(tokens.contains(&"Get".to_string()), "got {:?}", tokens);
        assert!(tokens.contains(&"NDS".to_string()), "got {:?}", tokens);
        assert!(tokens.contains(&"Value".to_string()), "got {:?}", tokens);
    }

    #[test]
    fn test_tokenize_simple() {
        let tokens = tokenize_identifier("ЗначениеРеквизитаОбъекта");
        assert!(tokens.contains(&"Значение".to_string()), "got {:?}", tokens);
        assert!(tokens.contains(&"Реквизита".to_string()), "got {:?}", tokens);
        assert!(tokens.contains(&"Объекта".to_string()), "got {:?}", tokens);
    }

    #[test]
    fn test_query_tokenizer() {
        let tokens = tokenize_query("найди функцию которая из Справочника СтавкиНДС делает Перечисление");
        assert!(tokens.contains(&"справочника".to_string()), "got {:?}", tokens);
        assert!(tokens.contains(&"ставкиндс".to_string()), "got {:?}", tokens);
        assert!(tokens.contains(&"перечисление".to_string()), "got {:?}", tokens);
        // stop words filtered
        assert!(!tokens.contains(&"найди".to_string()), "stop word should be filtered");
        assert!(!tokens.contains(&"функцию".to_string()), "stop word should be filtered");
    }

    #[test]
    fn test_fts_query_building() {
        let tokens = vec!["НДС".to_string(), "Справочник".to_string()];
        let q = tokens_to_fts_query(&tokens);
        assert!(q.contains("НДС"), "got {}", q);
        assert!(q.contains("Справочник"), "got {}", q);
        assert!(q.contains("OR"), "got {}", q);
    }
}
