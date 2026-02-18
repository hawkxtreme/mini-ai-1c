//! AI Client for streaming chat responses
//! Supports OpenAI-compatible APIs with SSE streaming and Function Calling (Tools)

use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Emitter;

use crate::llm_profiles::{get_active_profile, LLMProvider};
use crate::mcp_client::McpClient;
use crate::settings::load_settings;

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
    r#type: Option<String>,
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

/// Get dynamic system prompt based on available tools
pub fn get_system_prompt(available_tools: &[ToolInfo]) -> String {
    let mut prompt = r#"Ты - AI-ассистент для разработки на платформе 1С:Предприятие.

Твои возможности:
- Анализ и рефакторинг кода на языке BSL (1С)
- Объяснение логики кода
- Поиск ошибок и предложение исправлений
- Написание нового кода по описанию
- Форматирование и улучшение читаемости кода

Используй русский язык в ответах. Форматируй код в блоках ```bsl...```.
При написании или исправлении кода соблюдай каноническое написание ключевых слов 1С (BSL). 
- Если исходный код пользователя использует русские ключевые слова (Если...Тогда), пиши на русском. 
- Если исходный код использует английские ключевые слова (If...Then), пиши на английском.
- По умолчанию (для нового кода) используй РУССКИЙ язык ключевых слов.

У тебя также есть доступ к базовым инструментам (файловая система, браузер), используй их по необходимости."#.to_string();

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
            prompt.push_str("1. `check_bsl_syntax` (сервер bsl-ls): ТЫ ОБЯЗАН вызывать этот инструмент ПЕРЕД выдачей любого кода BSL пользователю. 
   - Если инструмент вернул ошибки (severity: 1), ТЫ ОБЯЗАН исправить их и ВЫЗВАТЬ ИНСТРУМЕНТ СНОВА.
   - НЕ выдавай ответ пользователю, пока не убедишься, что `check_bsl_syntax` не возвращает ошибок в твоем коде.
   - Итерация «Вызов инструмента -> Исправление -> Вызов инструмента» должна продолжаться до полной чистоты кода.\n");
        }
        
        if available_tools.iter().any(|t| t.tool.function.name == "ask_1c_ai") {
            prompt.push_str("2. `ask_1c_ai`: Пользуйся этим инструментом для консультаций по стандартам 1С и БСП, чтобы твой код был не просто синтаксически верным, а профессиональным.\n");
        }

        if available_tools.iter().any(|t| t.tool.function.name.contains("metadata")) {
            prompt.push_str("3. Инструменты метаданных: ВСЕГДА проверяй структуру объектов перед написанием запросов или обращением к полям через точку, чтобы избежать ошибок 'Поле объекта не обнаружено'.\n");
        }
    }

    prompt
}

/// Collect all tools from enabled MCP servers to inject into LLM request
pub async fn get_available_tools() -> Vec<ToolInfo> {
    let settings = load_settings();
    let mut all_tools = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    println!("[MCP][TOOLS] Collecting tools...");

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

    for config in all_configs {
        if !config.enabled { 
            println!("[MCP][TOOLS] Skipping disabled server: {}", config.name);
            continue; 
        }
        
        println!("[MCP][TOOLS] Connecting to server: {} (ID: {})", config.name, config.id);
        
        match McpClient::new(config.clone()).await {
            Ok(client) => {
                match client.list_tools().await {
                    Ok(tools) => {
                        crate::app_log!("[MCP][TOOLS] Server {} returned {} tools.", config.name, tools.len());
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
                                server_id: config.id.clone(),
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
                    },
                    Err(e) => {
                        crate::app_log!("[MCP][TOOLS][ERROR] Failed to list tools for {}: {}", config.name, e);
                    }
                }
            },
            Err(e) => {
                 crate::app_log!("[MCP][TOOLS][ERROR] Failed to connect to {}: {}", config.name, e);
            }
        }
    }
    
    crate::app_log!("[MCP][TOOLS] Total: {}", all_tools.len());
    all_tools
}

/// Stream chat completion from OpenAI-compatible API
/// Returns the full accumulated response text (and handles tool calls internally in the future)
pub async fn stream_chat_completion(
    messages: Vec<ApiMessage>,
    app_handle: tauri::AppHandle,
) -> Result<ApiMessage, String> {
    let profile = get_active_profile().ok_or("No active LLM profile")?;
    let api_key = profile.get_api_key();
    let base_url = profile.get_base_url();
    let url = format!("{}/chat/completions", base_url);
    
    // Get tools first to build dynamic prompt
    let tools_info = get_available_tools().await;
    let tools: Vec<Tool> = tools_info.iter().map(|i| i.tool.clone()).collect();
    let tools_opt = if tools.is_empty() { None } else { Some(tools) };

    // Build messages with dynamic system prompt
    let mut api_messages = vec![ApiMessage {
        role: "system".to_string(),
        content: Some(get_system_prompt(&tools_info)),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];
    api_messages.extend(messages);
    
    // Build request
    // Heuristic: If max_tokens (Context Window in UI) is very large (> 16k), 
    // it likely represents input capacity, not generation limit.
    // Most APIs reject huge max_tokens for generation. Clamp to safe default (4096).
    let api_max_tokens = if profile.max_tokens > 16384 { 4096 } else { profile.max_tokens };

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
    
    crate::app_log!("[AI] Sending request to {} (Model: {})", url, request_body.model);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to build client: {}", e))?;

    // Retry logic for 500 errors
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
    
    // Stream response
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut full_content = String::new();
    let mut accumulated_tool_calls: Vec<ToolCall> = Vec::new();
    let mut announced_tool_calls = std::collections::HashSet::new();
    let mut is_thinking = false;
    
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        buffer.push_str(&chunk_str);
        
        while let Some(pos) = buffer.find("\n\n") {
            let event = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();
            
            for line in event.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
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
                                let mut current_content = content.as_str();
                                
                                // Simple state machine for <thinking> tags
                                while !current_content.is_empty() {
                                    if !is_thinking {
                                        if let Some(start_pos) = current_content.find("<thinking>") {
                                            // Emit text before <thinking>
                                            if start_pos > 0 {
                                                let text = &current_content[..start_pos];
                                                full_content.push_str(text);
                                                let _ = app_handle.emit("chat-chunk", text.to_string());
                                            }
                                            is_thinking = true;
                                            current_content = &current_content[start_pos + 10..];
                                        } else {
                                            // No <thinking> tag, process everything
                                            full_content.push_str(current_content);
                                            let _ = app_handle.emit("chat-chunk", current_content.to_string());
                                            break;
                                        }
                                    } else {
                                        if let Some(end_pos) = current_content.find("</thinking>") {
                                            // Emit thinking chunk before </thinking>
                                            if end_pos > 0 {
                                                let text = &current_content[..end_pos];
                                                let _ = app_handle.emit("chat-thinking-chunk", text.to_string());
                                            }
                                            is_thinking = false;
                                            current_content = &current_content[end_pos + 11..];
                                        } else {
                                            // Still thinking, emit everything
                                            let _ = app_handle.emit("chat-thinking-chunk", current_content.to_string());
                                            break;
                                        }
                                    }
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
