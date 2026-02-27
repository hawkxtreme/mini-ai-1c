//! AI Client for streaming chat responses
//! Supports OpenAI-compatible APIs with SSE streaming and Function Calling (Tools)

use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Emitter;

use crate::llm_profiles::{get_active_profile, LLMProvider};
use crate::mcp_client::McpClient;
use crate::settings::{load_settings, PromptBehaviorPreset};
use std::sync::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    static ref TOOLS_CACHE: Mutex<Option<(std::time::Instant, Vec<ToolInfo>)>> = Mutex::new(None);
}

/// Chat message for API (OpenAI compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub r#type: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Request body for OpenAI-compatible API
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ApiMessage>,
    stream: bool,
    temperature: f32,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
}

/// Streaming chunk from OpenAI API
#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: Option<usize>,
    id: Option<String>,
    _type: Option<String>,
    function: Option<ToolCallFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct ToolCallFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

/// System prompt for 1C assistant
/// Extended tool info for internal prompt generation
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub tool: Tool,
    pub server_id: String,
}

/// Константа с правилами сохранения кода
const CODE_PRESERVATION_RULES: &str = r#"
=== КРИТИЧЕСКИ ВАЖНЫЕ ПРАВИЛА СОХРАНЕНИЯ КОДА ===

1. ПОЛНОЕ СОХРАНЕНИЕ: При редактировании кода ВСЕГДА возвращай ПОЛНЫЙ текст модуля.
   - НИКОГДА не пропускай существующие процедуры и функции
   - НИКОГДА не удаляй комментарии в начале модуля (включая copyright)
   - НИКОГДА не обрезай код в конце модуля

2. ФОРМАТ ОТВЕТА С КОДОМ:
   Если пользователь просит изменить существующий код:
   - Верни ВЕСЬ модуль целиком с внесенными изменениями
   - Не пиши "... остальной код без изменений"
   - Не используй сокращения вида "// ... предыдущий код"

3. ИЗМЕНЕНИЯ ВЫДЕЛЯЙ КОММЕНТАРИЯМИ:
   // [ИЗМЕНЕНО AI] - дата: <дата>
   // Причина: <описание изменения>

ПРИМЕР ПРАВИЛЬНОГО ОТВЕТА:
```bsl
// Copyright (c) Company... (сохранено!)
#Область ОписаниеПеременных
// ... весь код области (сохранено!)
#КонецОбласти

#Область ПрограммныйИнтерфейс
// [ИЗМЕНЕНО] Добавлена новая функция
Функция МояНоваяФункция()
    // новый код
КонецФункции

// Существующая функция (сохранена без изменений!)
Процедура СуществующаяПроцедура()
    // ...
КонецПроцедуры
#КонецОбласти
```
"#;

/// Константа с инструкциями для diff-формата (Search/Replace)
/// Константа с инструкциями для diff-формата (Search/Replace)
const DIFF_FORMAT_INSTRUCTIONS: &str = r#"
IMPORTANT: You are an expert 1C Developer.
Your goal is to make **Targeted Edits** using strictly XML-based diff format.

[RULES]
1. OUTPUT_FORMAT: You MUST ONLY output your modifications using the following XML structure for EVERY change:
<diff>
  <search>
[Exact content to be replaced, including indentation]
  </search>
  <replace>
[New content to replace with]
  </replace>
</diff>

2. SEARCH_BLOCK_RULES (CRITICAL):
   - The `<search>` block must contain **COMPLETE LINES** of code. Do not start/end in the middle of a line.
   - It must match the original file **EXACTLY** (character-for-character, space-for-space).
   - It must include enough context (2-3 lines before/after) to be unique.
   - To ADD code, search for the line before the insertion point and include it in both `<search>` and `<replace>`.

3. STRICT_MODIFICATION_RULES:
   - Modiffy ONLY the lines you are actively requested to change.
   - PRESERVE the original logic, variable names, and comments of unmodified code.
   - Do NOT fix typos in variable names unless explicitly requested.

4. BLOCK_SPLITTING_RULES:
   - Break large changes into a series of SMALLER `<diff>` blocks that each change a distinct small portion.
   - DO NOT include long runs (e.g. 5+ lines) of unchanging lines in `<search>` blocks.

5. RESPONSE_STRUCTURE:
   - Respond ONLY with a brief text explanation and the `<diff>` blocks.
   - NEVER start a diff block without `<diff><search>`.
   - Ignore the format of previous answers in this chat. For the CURRENT task, you MUST wrap the result in the `<diff>` block.

6. EOF_RULE_COMPLETING_CODE:
   - If the code ends abruptly, you MUST complete it logically within the replace block.
[/RULES]
"#;

const TWO_STEP_PLANNING_RULES: &str = r#"
=== TWO-STEP PLANNING AND LANGUAGE RULES ===

[RULES]
1. AUTOMATIC_PLANNING:
   - For COMPLEX tasks (multiple steps), you MUST start your response with a `<think>` tag.
   - For SIMPLE tasks, you MAY skip the `<think>` tag and reply directly.

2. LANGUAGE:
   - The `<think>` BLOCK MUST BE IN ENGLISH for better reasoning.
   - The FINAL RESPONSE (AFTER `</think>` OR DIRECTLY) MUST BE IN THE USER'S LANGUAGE.
   - If the user writes in Russian — answer in Russian.

3. THINKING_CONTENT:
   - Analyze the goal inside `<think>`.
   - Do NOT include final code inside `<think>`.
[/RULES]
"#;

/// Helper to detect target language based on message content
fn detect_target_lang(messages: &[ApiMessage]) -> String {
    // 1. Check for Cyrillic in the last user message
    for msg in messages.iter().rev() {
        if msg.role == "user" {
            let clean_text: String = if let Some(content) = &msg.content {
                content.lines()
                    .filter(|l| !l.trim().starts_with('/'))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                "".to_string()
            };
            
            if clean_text.chars().any(|c| c >= '\u{0400}' && c <= '\u{04FF}') {
                return "Russian".to_string();
            }
            break;
        }
    }
    "Russian".to_string() // Default to Russian (system language)
}

/// Проверяет наличие BSL-кода в контексте диалога.
/// Если кода нет — инструкции дифф-формата не включаются в system prompt.
fn has_code_context(messages: &[ApiMessage]) -> bool {
    for msg in messages {
        if let Some(content) = &msg.content {
            // Явные BSL-блоки кода
            if content.contains("```bsl") || content.contains("```1c") {
                return true;
            }
            // Характерные BSL-ключевые слова (≥2 разных → значимый фрагмент кода)
            let bsl_markers = [
                "КонецФункции",
                "КонецПроцедуры",
                "КонецЕсли",
                "Функция ",
                "Процедура ",
            ];
            let count = bsl_markers.iter().filter(|&&m| content.contains(m)).count();
            if count >= 2 {
                return true;
            }
        }
    }
    false
}

/// Get dynamic system prompt based on available tools
pub fn get_system_prompt(available_tools: &[ToolInfo], messages: &[ApiMessage]) -> String {
    let settings = load_settings();
    let custom = &settings.custom_prompts;
    let code_gen = &settings.code_generation;
    
    let mut prompt = String::new();
    let target_lang = detect_target_lang(messages);
    
    // 1. Применяем пресет поведения (Behavior Preset)
    match code_gen.behavior_preset {
        PromptBehaviorPreset::Project => {
            prompt.push_str("Ты - эксперт-разработчик 1С. Твоя задача - писать чистый, поддерживаемый код, следуя стандартам 1С и БСП. Можешь исправлять ошибки и предлагать оптимальные решения в рамках запроса.\n\n");
        },
        PromptBehaviorPreset::Maintenance => {
            prompt.push_str("Ты - специалист по поддержке 1С. Твоя ГЛАВНАЯ задача - вносить точечные изменения в существующий (возможно, чужой или типовой) код. НИКОГДА не проводи рефакторинг и не меняй логику, которую не просили затронуть.\n\n");
            prompt.push_str("КРИТИЧЕСКОЕ ПРАВИЛО: Все свои изменения (добавление, изменение или удаление кода) ты обязан изолировать комментариями. НИКОГДА не удаляй существующие комментарии и копирайты.\n\n");
        },
    }
    
    // 2. Определяем формат ответа в зависимости от наличия BSL-кода в диалоге.
    // Если кода нет (например, описание отправлено хоткеем как текст без явной загрузки)
    // — не включаем DIFF_FORMAT_INSTRUCTIONS, модель отвечает свободным текстом.
    let has_code = has_code_context(messages);
    let code_rules = if has_code { DIFF_FORMAT_INSTRUCTIONS } else { "" };
    let planning_rules = TWO_STEP_PLANNING_RULES;

    // Инструкции режима редактирования — зависят от наличия кода в контексте
    let edit_mode_instructions = if has_code {
        r#"РЕЖИМ ОТВЕТА НА ВОПРОСЫ (СТРОГИЙ ПРИОРИТЕТ):
- Если запрос пользователя является ВОПРОСОМ (содержит слова: "что делает", "объясни", "как работает", "расскажи", "зачем", "почему", "что такое", "как используется") — ОТВЕЧАЙ ТОЛЬКО ТЕКСТОМ.
- В режиме вопроса ЗАПРЕЩЕНО использовать блоки SEARCH/REPLACE.
- В режиме вопроса ЗАПРЕЩЕНО вносить ЛЮБЫЕ изменения в код, даже "очевидные улучшения" или исправления.
- Изменения кода (SEARCH/REPLACE) — если запрос содержит явное действие: "исправь", "добавь", "измени", "перепиши", "удали", "создай", "реализуй", "оптимизируй", **"допиши"**, **"заверши"**, "дополни".
- ПУСТОЙ МОДУЛЬ: Если исходный код BSL пуст или содержит только маркер/комментарии, а пользователь просит "добавить", "создать" или "написать" — генерируй ПОЛНЫЙ текст модуля с нуля в блоке ```bsl. Не пытайся использовать SEARCH/REPLACE для абсолютно пустого файла.

**КРИТИЧЕСКИ ВАЖНО**: Если тебе предоставлен исходный код (контекст) и запрошено изменение — используй SEARCH/REPLACE. НЕ форматируй изменённый код в ```bsl``` блоки вместо SEARCH/REPLACE."#
    } else {
        r#"РЕЖИМ ОТВЕТА (КОНТЕКСТ КОДА ОТСУТСТВУЕТ):
- В текущем диалоге нет загруженного файла для редактирования.
- Отвечай ТОЛЬКО текстом или блоком ```bsl при генерации нового кода с нуля.
- ЗАПРЕЩЕНО использовать формат SEARCH/REPLACE — он не применим без исходного кода."#
    };

    prompt.push_str(&format!(
        r#"Ты - AI-ассистент для разработки на платформе 1С:Предприятие.

{}

=== ЯЗЫК ОТВЕТА (КРИТИЧЕСКИ ВАЖНО) ===
- ALWAYS respond in **{}** language. This is MANDATORY and MUST NOT be violated under any circumstances.
- You MAY think inside `<thinking>` in any language (English is preferred for efficiency).
- But the FINAL ANSWER (outside `<thinking>`) MUST ALWAYS be in {} — NEVER in English or any other language.
- If the user writes in Russian — answer in Russian. If in another language — answer in Russian anyway.

{}
Твоя ГЛАВНАЯ ЦЕЛЬ: Выполнять запросы пользователя МАКСИМАЛЬНО ТОЧНО, НЕ ВНОСЯ НИКАКИХ ЛИШНИХ ИЗМЕНЕНИЙ.

Твои задачи:
1. Выполнять конкретные запросы по коду (добавить комментарий, изменить условие и т.д.).
2. Объяснять логику кода.
3. Искать ошибки ТОЛЬКО если об этом просили.

ГЛАВНАЯ ДИРЕКТИВА (STRICT COMPLIANCE):
- Вноси изменения ТОЛЬКО в строгом соответствии с запросом пользователя.
- ЗАПРЕЩАЕТСЯ любой самопроизвольный рефакторинг, оптимизация алгоритмов или удаление комментариев.
- ЗАПРЕЩЕНО изменять код за пределами запрашиваемых модификаций.
- НЕ исправляй опечатки в переменных, если об этом не просили, так как это нарушит ссылки в других модулях.

{}

ФИНАЛЬНОЕ НАПОМИНАНИЕ: твой ответ НА РУССКОМ ЯЗЫКЕ!

=== ФОРМАТ ДОКУМЕНТАЦИИ (КРИТИЧЕСКИ ВАЖНО) ===
- При генерации описаний (шапок) процедур и функций используй ТОЛЬКО стандартный формат комментариев 1С (символы //).
- КАТЕГОРИЧЕСКИ ЗАПРЕЩЕНО использовать любые XML-подобные теги, такие как `<ОписаниеФункции>`, `<Параметры>`, `<ВозвращаемоеЗначение>` и т.д.
- ШАБЛОН ОПИСАНИЯ:
// Рассчитывает...
//
// Параметры:
//   ИмяПараметра - Тип - Описание
//
// Возвращаемое значение:
//   Тип - Описание"#,
        planning_rules, target_lang, target_lang, code_rules, edit_mode_instructions
    ));

    // 3. Инструкции для маркировки (только если включено или в режиме Maintenance)
    if code_gen.mark_changes || code_gen.behavior_preset == PromptBehaviorPreset::Maintenance {
        let now = chrono::Local::now();
        let date_str = now.format("%Y-%m-%d").to_string();
        let datetime_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
        
        let addition_marker = code_gen.addition_marker_template
            .replace("{datetime}", &datetime_str)
            .replace("{date}", &date_str);
        let modification_marker = code_gen.modification_marker_template
            .replace("{datetime}", &datetime_str)
            .replace("{date}", &date_str);
        let deletion_marker = code_gen.deletion_marker_template
            .replace("{datetime}", &datetime_str)
            .replace("{date}", &date_str);
        
        match code_gen.behavior_preset {
            PromptBehaviorPreset::Maintenance => {
                prompt.push_str("\n\n=== ПРАВИЛА ИЗОЛЯЦИИ ИЗМЕНЕНИЙ (MAINTENANCE) ===\n");
                prompt.push_str("Ты обязан маркировать свои правки согласно стандартам 1С:\n");
                prompt.push_str(&format!(
                    "1. ДОБАВЛЕНИЕ НОВОГО КОДА: {}\n",
                    if addition_marker.contains("{newCode}") {
                        addition_marker.replace("{newCode}", "<твой новый код>")
                    } else {
                        format!("Оборачивай в:\n{}\n<твой код>\n// Доработка END", addition_marker)
                    }
                ));
                prompt.push_str(&format!(
                    "2. ИЗМЕНЕНИЕ СУЩЕСТВУЮЩЕГО КОДА: {}\n",
                    if modification_marker.contains("{newCode}") {
                        modification_marker.replace("{newCode}", "<твой новый исправленный код>")
                    } else {
                        format!("Оборачивай в:\n{}\n<твой код>\n// Доработка END", modification_marker)
                    }
                ));
                if modification_marker.contains("{oldCode}") {
                    prompt.push_str("ВАЖНО: В шаблоне изменения ты обязан заменить {oldCode} на исходный текст кода, который ты исправляешь или удаляешь.\n");
                }
                prompt.push_str(&format!(
                    "3. УДАЛЕНИЕ КОДА: {}\n",
                    if deletion_marker.contains("{oldCode}") {
                        deletion_marker.replace("{oldCode}", "<закомментированный старый код>")
                    } else {
                        format!("{} (ниже следует закомментированный код)", deletion_marker)
                    }
                ));
                if addition_marker.contains("{newCode}") || modification_marker.contains("{newCode}") {
                    prompt.push_str("ВАЖНО: Если шаблон содержит {newCode}, ты ОБЯЗАН вставить свой код ровно на место этого токена.\n");
                }
                if deletion_marker.contains("{oldCode}") {
                    prompt.push_str("ВАЖНО: Если шаблон удаления содержит {oldCode}, ты ОБЯЗАН заменить его на закомментированный текст удаляемого кода.\n");
                }
                prompt.push_str("НИКОГДА не удаляй код бесследно. Всегда изолируй изменения или комментируй удаляемое.\n");
            },
            PromptBehaviorPreset::Project => {
                prompt.push_str("\n\n=== ПРАВИЛА МАРКИРОВКИ ИЗМЕНЕНИЙ ===\n");
                prompt.push_str("При необходимости маркировки используй комментарий в конце измененных строк или отдельной строкой выше.\n");
            }
        }
    }

    // 4. Глобальный префикс (имеет высший приоритет, если задан)
    if !custom.system_prefix.is_empty() {
        prompt.push_str("\n\n=== ПОЛЬЗОВАТЕЛЬСКИЕ ГЛОБАЛЬНЫЕ НАСТРОЙКИ (OVERRIDE) ===\n");
        prompt.push_str(&custom.system_prefix);
    }

    if !custom.on_code_change.is_empty() {
        prompt.push_str("\n\n=== ПОЛЬЗОВАТЕЛЬСКИЕ ИНСТРУКЦИИ ДЛЯ ИЗМЕНЕНИЯ КОДА ===\n");
        prompt.push_str(&custom.on_code_change);
    }
    
    // 4. Инструкции для генерации нового кода
    if !custom.on_code_generate.is_empty() {
        prompt.push_str("\n\n=== ПОЛЬЗОВАТЕЛЬСКИЕ ИНСТРУКЦИИ ДЛЯ ГЕНЕРАЦИИ КОДА ===\n");
        prompt.push_str(&custom.on_code_generate);
    }
    
    // 5. Активные шаблоны
    let active_templates: Vec<_> = custom.templates.iter()
        .filter(|t| t.enabled)
        .collect();
    
    if !active_templates.is_empty() {
        prompt.push_str("\n\n=== АКТИВНЫЕ ШАБЛОНЫ ===\n");
        for template in active_templates {
            prompt.push_str(&format!("- {}\n{}\n", template.name, template.content));
        }
    }
    
    // 6. MCP инструменты
    if !available_tools.is_empty() {
        prompt.push_str("\n\nВАЖНО: Тебе доступны следующие специализированные инструменты MCP:\n");
        for info in available_tools {
            let tool = &info.tool;
            let desc = if tool.function.description.is_empty() {
                "(описание отсутствует)"
            } else {
                &tool.function.description
            };
            prompt.push_str(&format!("- `{}` (сервер: {}): {}\n", tool.function.name, info.server_id, desc));
        }

        prompt.push_str("\nКРИТИЧЕСКИЕ ПРАВИЛА ИСПОЛЬЗОВАНИЯ ИНСТРУМЕНТОВ:\n");
        
        if available_tools.iter().any(|t| t.tool.function.name == "check_bsl_syntax") {
            prompt.push_str("1. `check_bsl_syntax` (сервер bsl-ls): Используй для анализа и самопроверки.\n");
            prompt.push_str("\n");
            prompt.push_str("   РЕЖИМ А — Самопроверка (ИИ проверяет свои собственные изменения):\n");
            prompt.push_str("   - Зона ответственности: ТОЛЬКО строки, которые ты сам добавил или изменил.\n");
            prompt.push_str("   - ЗАПРЕТ: не трогай ошибки в окружающем Legacy-коде, даже в той же функции.\n");
            prompt.push_str("   - 'Cognitive Complexity', 'Magic Number' в старом коде — ИГНОРИРУЙ.\n");
            prompt.push_str("   - Исправляй ТОЛЬКО критические синтаксические ошибки (забытая скобка и т.п.).\n");
            prompt.push_str("\n");
            prompt.push_str("   РЕЖИМ Б — Выполнение явного запроса пользователя:\n");
            prompt.push_str("   - Если пользователь ЯВНО просит исправить ошибки, добавить описание, устранить предупреждения — ВЫПОЛНЯЙ.\n");
            prompt.push_str("   - Примеры явных запросов: 'исправь ошибки bsl', 'добавь описание параметров', 'устрани предупреждения'.\n");
            prompt.push_str("   - В этом режиме исправляй ВСЕ указанные пользователем проблемы, включая Legacy-код.\n");
            prompt.push_str("   - НЕ отказывайся со ссылкой на правила Legacy — пользователь осознанно просит изменения.\n");
        }
        
        if available_tools.iter().any(|t| t.tool.function.name == "ask_1c_ai") {
            prompt.push_str("2. `ask_1c_ai`: Пользуйся этим инструментом для консультаций по стандартам 1С и БСП, чтобы твой код был не просто синтаксически верным, а профессиональным.\n");
        }

        if available_tools.iter().any(|t| t.tool.function.name.contains("metadata")) {
            prompt.push_str("3. Инструменты метаданных: ВСЕГДА проверяй структуру объектов перед написанием запросов или обращению к полям через точку, чтобы избежать ошибок 'Поле объекта не обнаружено'.\n");
        }
    }

    crate::app_log!("[DEBUG][PROMPT] Final System Prompt:\n{}", prompt);
    prompt
}

/// Collect all tools from enabled MCP servers to inject into LLM request
pub async fn get_available_tools() -> Vec<ToolInfo> {
    let settings = load_settings();
    let mut all_tools = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    crate::app_log!("[MCP][TOOLS] Collecting tools...");
    
    // Check cache first
    {
        if let Ok(cache) = TOOLS_CACHE.lock() {
            if let Some((time, tools)) = &*cache {
                if time.elapsed().as_secs() < 120 { // 2 minute cache
                    let duration = time.elapsed().as_millis();
                    crate::app_log!("[MCP][CACHE] Using cached tools ({} items, {} ms ago)", tools.len(), duration);
                    return tools.clone();
                }
            }
        }
    }

    let start_time = std::time::Instant::now();

    let mut all_configs = settings.mcp_servers.clone();
    
    // Add virtual BSL server only if not already present
    if !all_configs.iter().any(|c| c.id == "bsl-ls") {
        all_configs.push(crate::settings::McpServerConfig {
            id: "bsl-ls".to_string(),
            name: "BSL Language Server".to_string(),
            enabled: settings.bsl_server.enabled,
            transport: crate::settings::McpTransport::Internal,
            ..Default::default()
        });
    }

    let enabled_configs: Vec<_> = all_configs.into_iter().filter(|c| c.enabled).collect();
    let mut futures = Vec::new();

    for config in enabled_configs {
        futures.push(async move {
            let server_name = config.name.clone();
            let server_id = config.id.clone();
            let start = std::time::Instant::now();
            crate::app_log!("[MCP][TOOLS] Connecting to server: {} (ID: {})", server_name, server_id);
            
            match McpClient::new(config).await {
                Ok(client) => {
                    match client.list_tools().await {
                        Ok(tools) => {
                            let duration = start.elapsed().as_millis();
                            crate::app_log!("[MCP][TOOLS] Server {} returned {} tools in {} ms.", server_name, tools.len(), duration);
                            Ok((server_id, tools))
                        },
                        Err(e) => {
                            crate::app_log!("[MCP][TOOLS][ERROR] Failed to list tools for {}: {}", server_name, e);
                            Err(e)
                        }
                    }
                },
                Err(e) => {
                    crate::app_log!("[MCP][TOOLS][ERROR] Failed to connect to {}: {}", server_name, e);
                    Err(e)
                }
            }
        });
    }

    let results = futures::future::join_all(futures).await;

    for res in results {
        if let Ok((server_id, tools)) = res {
            for tool in tools {
                            // 1. Sanitize Name (only alphanumeric, underscore, hyphen)
                            let name = tool.name.chars()
                                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                                .collect::<String>();
                            
                            if name.is_empty() { 
                                crate::app_log!("[MCP][TOOLS][WARN] Tool name became empty after sanitization: {}", tool.name);
                                continue; 
                            }

                            // 2. Ensure unique name
                            if seen_names.contains(&name) {
                                crate::app_log!("[MCP][TOOLS][WARN] Duplicate tool name '{}'. Skipping.", name);
                                continue;
                            }
                            seen_names.insert(name.clone());

                            // 3. Sanitize Schema (Gemini/OpenAI strictly require root type "object")
                            let mut parameters = tool.input_schema.clone();
                            if !parameters.is_object() {
                                parameters = serde_json::json!({
                                    "type": "object",
                                    "properties": {}
                                });
                            } else {
                                let obj = parameters.as_object_mut().unwrap();
                                if !obj.contains_key("type") {
                                    obj.insert("type".to_string(), serde_json::json!("object"));
                                }
                                if !obj.contains_key("properties") {
                                     obj.insert("properties".to_string(), serde_json::json!({}));
                                }
                            }

                            crate::app_log!("[MCP][TOOLS]   + Registered: {}", name);
                            all_tools.push(ToolInfo {
                                server_id: server_id.clone(),
                                tool: Tool {
                                    r#type: "function".to_string(),
                                    function: ToolFunction {
                                        name,
                                        description: tool.description,
                                        parameters,
                                    },
                                },
                            });
                        }
                    }
                }
    
    let total_duration = start_time.elapsed().as_millis();
    crate::app_log!("[MCP][TOOLS] Total collection time: {} ms. Total tools: {}", total_duration, all_tools.len());
    
    // Update cache
    if let Ok(mut cache) = TOOLS_CACHE.lock() {
        *cache = Some((std::time::Instant::now(), all_tools.clone()));
    }

    all_tools
}

/// Force clear the MCP tools cache
pub fn clear_mcp_cache() {
    if let Ok(mut cache) = TOOLS_CACHE.lock() {
        *cache = None;
        crate::app_log!("[MCP][CACHE] Cache cleared.");
    }
}

/// Stream chat completion from OpenAI-compatible API
/// Returns the full accumulated response text (and handles tool calls internally in the future)
pub async fn stream_chat_completion(
    messages: Vec<ApiMessage>,
    app_handle: tauri::AppHandle,
) -> Result<ApiMessage, String> {
    let profile = get_active_profile().ok_or("No active LLM profile")?;
    let (api_key, url) = if matches!(profile.provider, LLMProvider::QwenCli) {
        let token_info = crate::llm::cli_providers::qwen::QwenCliProvider::get_token(&profile.id)?;
        let (access_token, refresh_token, expires_at, resource_url) = token_info.ok_or("Qwen CLI: Требуется авторизация")?;

        // Auto-refresh if token expired (or expires within next 60 seconds)
        let (access_token, resource_url) = if chrono::Utc::now().timestamp() as u64 + 60 > expires_at {
            if let Some(rt) = refresh_token.as_deref() {
                crate::app_log!(force: true, "[Qwen] Token expired/near-expiry, attempting refresh...");
                match crate::llm::cli_providers::qwen::QwenCliProvider::refresh_access_token(&profile.id, rt).await {
                    Ok(()) => {
                        let new_info = crate::llm::cli_providers::qwen::QwenCliProvider::get_token(&profile.id)?
                            .ok_or("Qwen CLI: Токен не найден после обновления")?;
                        crate::app_log!(force: true, "[Qwen] Token refreshed successfully");
                        (new_info.0, new_info.3)
                    }
                    Err(e) => {
                        crate::app_log!(force: true, "[Qwen] Token refresh failed: {}", e);
                        return Err("Qwen CLI: Токен истек и не удалось обновить. Требуется повторная авторизация".to_string());
                    }
                }
            } else {
                return Err("Qwen CLI: Токен истек. Требуется повторная авторизация".to_string());
            }
        } else {
            (access_token, resource_url)
        };

        // Use resource_url from token if available and not a commercial DashScope endpoint,
        // else fallback to portal.qwen.ai (free tier)
        let base = if let Some(ru) = resource_url.as_deref().filter(|s| {
            !s.is_empty() && !s.contains("dashscope") && !s.contains("aliyun")
        }) {
            format!("https://{}/v1", ru)
        } else {
            "https://portal.qwen.ai/v1".to_string()
        };
        (access_token, format!("{}/chat/completions", base))
    } else {
        let api_key = profile.get_api_key();
        let base_url = profile.get_base_url();
        (api_key, format!("{}/chat/completions", base_url))
    };
    
    // Get tools first to build dynamic prompt
    let tools_info = get_available_tools().await;
    let tools: Vec<Tool> = tools_info.iter().map(|i| i.tool.clone()).collect();
    let tools_opt = if tools.is_empty() { None } else { Some(tools) };

    // Build messages with dynamic system prompt
    let mut api_messages = vec![ApiMessage {
        role: "system".to_string(),
        content: Some(get_system_prompt(&tools_info, &messages)),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];
    api_messages.extend(messages);
    
    // Build request
    // Heuristic: If max_tokens (Context Window in UI) is very large (> 16k), 
    // it likely represents input capacity, not generation limit.
    // Most APIs reject huge max_tokens for generation. Clamp to safe default (4096).
    let _api_max_tokens = if profile.max_tokens > 16384 { 4096 } else { profile.max_tokens };

    // Build request
    let api_max_tokens = if profile.max_tokens > 16384 { 4096 } else { profile.max_tokens };

    let request_body = ChatRequest {
        model: profile.model.clone(),
        messages: api_messages,
        stream: true,
        temperature: profile.temperature,
        max_tokens: api_max_tokens,
        tools: tools_opt,
    };
    
    // Build headers
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    
    if !api_key.is_empty() {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key))
                .map_err(|e| e.to_string())?,
        );
    }
    
    if matches!(profile.provider, LLMProvider::OpenRouter) {
        headers.insert("HTTP-Referer", HeaderValue::from_static("https://mini-ai-1c.local"));
        headers.insert("X-Title", HeaderValue::from_static("Mini AI 1C Agent"));
    }

    if matches!(profile.provider, LLMProvider::QwenCli) {
        headers.insert("User-Agent", HeaderValue::from_static("QwenCode/0.10.3 (darwin; arm64)"));
        headers.insert("X-Dashscope-Useragent", HeaderValue::from_static("QwenCode/0.10.3 (darwin; arm64)"));
        headers.insert("X-Dashscope-Authtype", HeaderValue::from_static("qwen-oauth"));
        headers.insert("X-Dashscope-Cachecontrol", HeaderValue::from_static("enable"));
        headers.insert("X-Stainless-Runtime", HeaderValue::from_static("node"));
        headers.insert("X-Stainless-Runtime-Version", HeaderValue::from_static("v22.17.0"));
        headers.insert("X-Stainless-Lang", HeaderValue::from_static("js"));
        headers.insert("X-Stainless-Package-Version", HeaderValue::from_static("5.11.0"));
        headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("cors"));
    }
    
    crate::app_log!("[AI] Sending request to {} (Model: {})", url, request_body.model);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to build client: {}", e))?;

    // Retry logic for 500 errors and Qwen 429 rate-limits
    let mut attempt = 0;
    let max_retries = 3;
    let response = loop {
        attempt += 1;
        let res = client
            .post(&url)
            .headers(headers.clone())
            .json(&request_body)
            .send()
            .await;

        match res {
            Ok(r) if r.status().is_success() => break r,
            Ok(r) if r.status().as_u16() == 500 && attempt < max_retries => {
                crate::app_log!("[AI][RETRY] Attempt {} failed with 500. Retrying in 2s...", attempt);
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
            Ok(r) if r.status().as_u16() == 429 && matches!(profile.provider, LLMProvider::QwenCli) && attempt < max_retries => {
                let retry_after = r.headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(5);
                crate::app_log!("[AI][RETRY] Qwen 429 rate-limit (attempt {}), waiting {}s...", attempt, retry_after);
                tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
                continue;
            }
            Ok(r) => {
                let status = r.status();
                let error_body = r.text().await.unwrap_or_default();
                crate::app_log!("[AI] API Error (Attempt {}): {} - {}", attempt, status, error_body);
                return Err(format!("API error {}: {}", status, error_body));
            }
            Err(e) if attempt < max_retries => {
                crate::app_log!("[AI][RETRY] Request failed (Attempt {}): {}. Retrying in 2s...", attempt, e);
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
            Err(e) => return Err(format!("Request failed after {} attempts: {}", attempt, e)),
        }
    };
    
    crate::app_log!("[AI] Response received. Status: {}", response.status());

    // For QwenCli — parse rate-limit headers and cache usage in keyring
    if matches!(profile.provider, LLMProvider::QwenCli) {
        let hdrs = response.headers();
        crate::app_log!(force: true, "[DEBUG] Qwen response headers: {:?}", hdrs);
        // Common patterns: x-ratelimit-limit-requests, x-ratelimit-remaining-requests,
        // x-ratelimit-reset-requests (or -tokens variants)
        let limit = hdrs.get("x-ratelimit-limit-requests")
            .or_else(|| hdrs.get("x-ratelimit-requests-limit"))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());
        let remaining = hdrs.get("x-ratelimit-remaining-requests")
            .or_else(|| hdrs.get("x-ratelimit-requests-remaining"))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());
        let reset = hdrs.get("x-ratelimit-reset-requests")
            .or_else(|| hdrs.get("x-ratelimit-requests-reset"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if let (Some(limit), Some(remaining)) = (limit, remaining) {
            let used = limit.saturating_sub(remaining);
            crate::app_log!(force: true, "[DEBUG] Qwen rate-limit: used={}/{}, reset={:?}", used, limit, reset);
            let _ = crate::llm::cli_providers::qwen::QwenCliProvider::save_usage(&profile.id, used, limit, reset);
        }
    }

    // Stream response
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut full_content = String::new();
    let mut content_search_temp = String::new();
    let mut accumulated_tool_calls: Vec<ToolCall> = Vec::new();
    let mut announced_tool_calls = std::collections::HashSet::new();
    let mut is_thinking = false;
    let mut is_qwen_fn = false;           // Qwen XML tool call mode: <function=name>...</function>
    let mut qwen_fn_buf = String::new();  // Буфер Qwen XML tool call
    let mut has_switched_to_executing = false;
    let mut first_token_received = false;
    let start_gen_time = std::time::Instant::now();
    
    while let Some(chunk_result) = stream.next().await {
        if !first_token_received {
            first_token_received = true;
            let ttft = start_gen_time.elapsed().as_millis();
            crate::app_log!("[AI][TIMER] TTFT (Time to First Token): {} ms", ttft);
        }
        let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        buffer.push_str(&chunk_str);
        
        while let Some(pos) = buffer.find("\n\n") {
            let event = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();
            
            for line in event.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        // Flush any remaining content in buffers
                        if !content_search_temp.is_empty() {
                            if is_thinking {
                                let _ = app_handle.emit("chat-thinking-chunk", content_search_temp.clone());
                            } else if !is_qwen_fn {
                                full_content.push_str(&content_search_temp);
                                let _ = app_handle.emit("chat-chunk", content_search_temp.clone());
                            }
                            content_search_temp.clear();
                        }
                        // If we have an incomplete Qwen function, add it to full content so it's not lost
                        if is_qwen_fn && !qwen_fn_buf.is_empty() {
                            full_content.push_str(&qwen_fn_buf);
                            let _ = app_handle.emit("chat-chunk", qwen_fn_buf.clone());
                            qwen_fn_buf.clear();
                        }

                        crate::app_log!("[AI][DIAG] [DONE] received. full_content.len()={}, tool_calls={}, qwen_fn_buf_len={}",
                            full_content.len(), accumulated_tool_calls.len(), qwen_fn_buf.len());
                        if !full_content.is_empty() {
                            let preview: String = full_content.chars().take(300).collect();
                            crate::app_log!("[AI][DIAG] content preview: {:?}", preview);
                        }
                        if matches!(profile.provider, LLMProvider::QwenCli) {
                            crate::llm::cli_providers::qwen::QwenCliProvider::increment_request_count(&profile.id);
                        }
                        return Ok(ApiMessage {
                            role: "assistant".to_string(),
                            content: if full_content.is_empty() { None } else { Some(full_content) },
                            tool_calls: if accumulated_tool_calls.is_empty() { None } else { Some(accumulated_tool_calls) },
                            tool_call_id: None,
                            name: None,
                        });
                    }
                    
                    if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                        if let Some(choice) = chunk.choices.first() {
                            // 1. Handle content & thinking tags
                            if let Some(content) = &choice.delta.content {
                                // 0. Switch status if this is the first real content and we are not thinking
                                if !has_switched_to_executing && !is_thinking && !content.trim().is_empty() {
                                    let _ = app_handle.emit("chat-status", "Выполнение...");
                                    has_switched_to_executing = true;
                                }

                                // Add to persistent search buffer
                                content_search_temp.push_str(content);
                                
                                // Process the search buffer
                                loop {
                                    if !is_thinking && !is_qwen_fn {
                                        // --- Qwen XML function call detection ---
                                        if let Some(fn_start) = content_search_temp.find("<function=") {
                                            // Emit everything before the tag as regular text
                                            if fn_start > 0 {
                                                let text = content_search_temp[..fn_start].to_string();
                                                full_content.push_str(&text);
                                                let _ = app_handle.emit("chat-chunk", text);
                                            }
                                            is_qwen_fn = true;
                                            qwen_fn_buf = content_search_temp[fn_start..].to_string();
                                            content_search_temp.clear();
                                            break;
                                        }
                                        
                                        if let Some(start_pos) = content_search_temp.find("<thinking>") {
                                            // 1. Text before <thinking> goes to regular chat
                                            if start_pos > 0 {
                                                let text = content_search_temp[..start_pos].to_string();
                                                full_content.push_str(&text);
                                                let _ = app_handle.emit("chat-chunk", text);
                                            }
                                            
                                            // 2. Start thinking
                                            is_thinking = true;
                                            let _ = app_handle.emit("chat-status", "Планирование (EN)...");
                                            
                                            // 3. Remove processed part including tag
                                            content_search_temp = content_search_temp[start_pos + 10..].to_string();
                                        } else if let Some(last_lt) = content_search_temp.rfind('<') {
                                            // Potential tag start - but only hold back if it COULD be an XML tag.
                                            // XML tags start with '<' followed by a letter, slash, or '?'.
                                            // Diff markers like '<<<<<<< SEARCH' start with multiple '<'.
                                            // Do NOT hold back for diff markers or operator sequences.
                                            let after_lt = content_search_temp[last_lt..].chars().nth(1);
                                            let is_potential_tag = matches!(after_lt, Some(c) if c.is_alphabetic() || c == '/' || c == '?');
                                            
                                            if is_potential_tag {
                                                // Hold back everything from `<` onwards, emit the rest.
                                                if last_lt > 0 {
                                                    let text = content_search_temp[..last_lt].to_string();
                                                    full_content.push_str(&text);
                                                    let _ = app_handle.emit("chat-chunk", text);
                                                    content_search_temp = content_search_temp[last_lt..].to_string();
                                                }
                                                break;
                                            } else {
                                                // Not a potential tag (e.g., diff marker '<<<'), emit everything.
                                                full_content.push_str(&content_search_temp);
                                                let _ = app_handle.emit("chat-chunk", content_search_temp.clone());
                                                content_search_temp.clear();
                                                break;
                                            }

                                        } else {
                                            // No '<' found, emit everything
                                            full_content.push_str(&content_search_temp);
                                            let _ = app_handle.emit("chat-chunk", content_search_temp.clone());
                                            content_search_temp.clear();
                                            break;
                                        }
                                    } else {
                                        if let Some(end_pos) = content_search_temp.find("</thinking>") {
                                            // 1. Text before </thinking> goes to thinking channel
                                            if end_pos > 0 {
                                                let text = content_search_temp[..end_pos].to_string();
                                                let _ = app_handle.emit("chat-thinking-chunk", text);
                                            }
                                            
                                            // 2. Stop thinking
                                            is_thinking = false;
                                            has_switched_to_executing = true;
                                            let _ = app_handle.emit("chat-status", "Выполнение...");
                                            
                                            // 3. Remove processed part including tag
                                            content_search_temp = content_search_temp[end_pos + 11..].to_string();
                                        } else if let Some(last_lt) = content_search_temp.rfind('<') {
                                            // Potential end-tag start found.
                                            if last_lt > 0 {
                                                let text = content_search_temp[..last_lt].to_string();
                                                let _ = app_handle.emit("chat-thinking-chunk", text);
                                                content_search_temp = content_search_temp[last_lt..].to_string();
                                            }
                                            break;
                                        } else {
                                            // No '<' found, emit everything to thinking
                                            let _ = app_handle.emit("chat-thinking-chunk", content_search_temp.clone());
                                            content_search_temp.clear();
                                            break;
                                        }
                                    }
                                }
                            }
                            
                            // 1b. Handle Qwen XML tool calls accumulation
                            if is_qwen_fn {
                                qwen_fn_buf.push_str(&content_search_temp);
                                content_search_temp.clear();
                                if let Some(end_pos) = qwen_fn_buf.find("</function>") {
                                    let full_block = qwen_fn_buf[..end_pos + "</function>".len()].to_string();
                                    let remainder = qwen_fn_buf[end_pos + "</function>".len()..].to_string();
                                    qwen_fn_buf.clear();
                                    is_qwen_fn = false;
                                    // Parse: <function=name>\n<parameter=p>\nVALUE\n</parameter>\n</function>
                                    if let Some(fn_name_end) = full_block[10..].find('>') {
                                        let fn_name = full_block[10..10 + fn_name_end].to_string();
                                        let mut args_map = serde_json::Map::new();
                                        let body = &full_block[10 + fn_name_end + 1..];
                                        // Find all <parameter=X>VALUE</parameter>
                                        let mut pos = 0;
                                        while let Some(p_start) = body[pos..].find("<parameter=") {
                                            let abs = pos + p_start;
                                            if let Some(close_gt) = body[abs..].find('>') {
                                                let p_name = body[abs + 11..abs + close_gt].to_string();
                                                let v_start = abs + close_gt + 1;
                                                if let Some(v_end) = body[v_start..].find("</parameter>") {
                                                    let value = body[v_start..v_start + v_end].trim().to_string();
                                                    args_map.insert(p_name, serde_json::Value::String(value));
                                                    pos = v_start + v_end + 12;
                                                } else { break; }
                                            } else { break; }
                                        }
                                        let args_json = serde_json::to_string(&args_map).unwrap_or("{}".to_string());
                                        let tc_idx = accumulated_tool_calls.len();
                                        let tc = ToolCall {
                                            id: format!("qwen_fn_{}", tc_idx),
                                            r#type: "function".to_string(),
                                            function: ToolCallFunction { name: fn_name.clone(), arguments: args_json },
                                        };
                                        let _ = app_handle.emit("tool-call-started", serde_json::json!({
                                            "index": tc_idx, "id": tc.id, "name": fn_name
                                        }));
                                        accumulated_tool_calls.push(tc);
                                    }
                                    // Continue with remainder
                                    content_search_temp = remainder;
                                }
                            }
                            
                            // 2. Handle tool calls
                            if let Some(tool_calls) = &choice.delta.tool_calls {
                                for tc_delta in tool_calls {
                                    let idx = tc_delta.index.unwrap_or(0);
                                    
                                    while accumulated_tool_calls.len() <= idx {
                                        accumulated_tool_calls.push(ToolCall {
                                            id: String::new(),
                                            r#type: "function".to_string(),
                                            function: ToolCallFunction { name: String::new(), arguments: String::new() },
                                        });
                                    }
                                    
                                    let tc = &mut accumulated_tool_calls[idx];
                                    if let Some(id) = &tc_delta.id { tc.id.push_str(id); }
                                    if let Some(f) = &tc_delta.function {
                                        if let Some(name) = &f.name { 
                                            tc.function.name.push_str(name); 
                                        }
                                        if let Some(args) = &f.arguments { 
                                            tc.function.arguments.push_str(args);
                                            // Emit progress
                                            let _ = app_handle.emit("tool-call-progress", serde_json::json!({
                                                "index": idx,
                                                "arguments": args
                                            }));
                                        }
                                    }

                                    // Emit "started" event when we have an ID or name
                                    if !announced_tool_calls.contains(&idx) && (!tc.id.is_empty() || !tc.function.name.is_empty()) {
                                        let _ = app_handle.emit("tool-call-started", serde_json::json!({
                                            "index": idx,
                                            "id": tc.id,
                                            "name": tc.function.name
                                        }));
                                        announced_tool_calls.insert(idx);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // FINAL FLUSH: ensure everything from buffers is in full_content if stream ended unexpectedly
    if !content_search_temp.is_empty() {
        if is_thinking {
            let _ = app_handle.emit("chat-thinking-chunk", content_search_temp.clone());
        } else if !is_qwen_fn {
            full_content.push_str(&content_search_temp);
            let _ = app_handle.emit("chat-chunk", content_search_temp.clone());
        }
        content_search_temp.clear();
    }
    if is_qwen_fn && !qwen_fn_buf.is_empty() {
        full_content.push_str(&qwen_fn_buf);
        let _ = app_handle.emit("chat-chunk", qwen_fn_buf.clone());
        qwen_fn_buf.clear();
    }
    
    let total_gen_duration = start_gen_time.elapsed().as_millis();
    crate::app_log!("[AI][TIMER] Total generation time: {} ms", total_gen_duration);
    crate::app_log!("[AI][DIAG] full_content len={}, tool_calls={}, qwen_fn_buf_len={}",
        full_content.len(), accumulated_tool_calls.len(), qwen_fn_buf.len());
    if !full_content.is_empty() {
        let preview: String = full_content.chars().take(300).collect();
        crate::app_log!("[AI][DIAG] content preview: {:?}", preview);
    }

    Ok(ApiMessage {
        role: "assistant".to_string(),
        content: if full_content.is_empty() { None } else { Some(full_content) },
        tool_calls: if accumulated_tool_calls.is_empty() { None } else { Some(accumulated_tool_calls) },
        tool_call_id: None,
        name: None,
    })
}

/// Helper to extract BSL code blocks from text
pub fn extract_bsl_code(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut start_pos = 0;
    
    while let Some(start) = text[start_pos..].find("```bsl") {
        let actual_start = start_pos + start + 6;
        if let Some(end) = text[actual_start..].find("```") {
            let code = &text[actual_start..actual_start + end];
            blocks.push(code.trim().to_string());
            start_pos = actual_start + end + 3;
        } else {
            break;
        }
    }
    
    start_pos = 0;
    while let Some(start) = text[start_pos..].find("```1c") {
        let actual_start = start_pos + start + 5;
        if let Some(end) = text[actual_start..].find("```") {
            let code = &text[actual_start..actual_start + end];
            blocks.push(code.trim().to_string());
            start_pos = actual_start + end + 3;
        } else {
            break;
        }
    }
    
    blocks
}

/// Fetch models from provider
pub async fn fetch_models(profile: &crate::llm_profiles::LLMProfile) -> Result<Vec<String>, String> {
    let api_key = profile.get_api_key();
    let base_url = profile.get_base_url();
    let url = if base_url.ends_with("/chat/completions") {
        base_url.replace("/chat/completions", "/models")
    } else {
        format!("{}/models", base_url.trim_end_matches('/'))
    };

    let client = reqwest::Client::new();
    let mut builder = client.get(&url);
    builder = builder.header(CONTENT_TYPE, "application/json");

    if !api_key.is_empty() {
        builder = builder.header(AUTHORIZATION, format!("Bearer {}", api_key));
    }

    if matches!(profile.provider, LLMProvider::OpenRouter) {
        builder = builder
            .header("HTTP-Referer", "https://mini-ai-1c.local")
            .header("X-Title", "Mini AI 1C Agent");
    }

    let response = builder.send().await.map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch models: {}", response.status()));
    }

    let data: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let mut models = Vec::new();
    if let Some(list) = data.get("data").and_then(|d| d.as_array()) {
        for item in list {
            if let Some(id) = item.get("id").and_then(|id| id.as_str()) {
                models.push(id.to_string());
            }
        }
    }
    
    models.sort();
    Ok(models)
}

/// Test connection
pub async fn test_connection(profile: &crate::llm_profiles::LLMProfile) -> Result<String, String> {
    match fetch_models(profile).await {
        Ok(models) => Ok(format!("Success! Found {} models.", models.len())),
        Err(e) => Err(format!("Connection failed: {}", e)),
    }
}
