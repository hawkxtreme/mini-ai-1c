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
const SYSTEM_PROMPT: &str = r#"Ты - AI-ассистент для разработки на платформе 1С:Предприятие.

Твои возможности:
- Анализ и рефакторинг кода на языке BSL (1С)
- Объяснение логики кода
- Поиск ошибок и предложение исправлений
- Написание нового кода по описанию
- Форматирование и улучшение читаемости кода

ВАЖНО: У тебя есть доступ к специализированному MCP серверу "1C:Напарник" (1С.ai), который предоставляет экспертные знания.
Используй следующие инструменты для ответов на вопросы по 1С:
1. `ask_1c_ai` - для любых вопросов по платформе 1С, синтаксису, стандартным библиотекам (БСП) и лучшим практикам. Если вопрос касается "как сделать в 1С" или "как работает метод X", используй этот инструмент.
2. `explain_1c_syntax` - для детального объяснения конкретных конструкций языка или объектов метаданных.
3. `check_1c_code` - ОБЯЗАТЕЛЬНО используй этот инструмент для проверки любого написанного или анализируемого тобой кода 1С перед выдачей ответа пользователю. Это поможет избежать синтаксических ошибок.

Используй русский язык в ответах. Форматируй код в блоках ```bsl...```.
У тебя также есть доступ к другим инструментам (файловая система, браузер), используй их по необходимости."#;

/// Collect all tools from enabled MCP servers to inject into LLM request
pub async fn get_available_tools() -> Vec<Tool> {
    let settings = load_settings();
    let mut all_tools = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    println!("[MCP][TOOLS] Collecting tools...");

    for config in settings.mcp_servers {
        if !config.enabled { 
            println!("[MCP][TOOLS] Skipping disabled server: {}", config.name);
            continue; 
        }
        
        println!("[MCP][TOOLS] Connecting to server: {} (ID: {})", config.name, config.id);
        
        match McpClient::new(config.clone()).await {
            Ok(client) => {
                match client.list_tools().await {
                    Ok(tools) => {
                        println!("[MCP][TOOLS] Server {} returned {} tools.", config.name, tools.len());
                        for tool in tools {
                            // 1. Sanitize Name (only alphanumeric, underscore, hyphen)
                            let name = tool.name.chars()
                                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                                .collect::<String>();
                            
                            if name.is_empty() { 
                                println!("[MCP][TOOLS][WARN] Tool name became empty after sanitization: {}", tool.name);
                                continue; 
                            }

                            // 2. Ensure unique name
                            if seen_names.contains(&name) {
                                println!("[MCP][TOOLS][WARN] Duplicate tool name '{}'. Skipping.", name);
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

                            println!("[MCP][TOOLS]   + Registered: {}", name);
                            all_tools.push(Tool {
                                r#type: "function".to_string(),
                                function: ToolFunction {
                                    name,
                                    description: tool.description,
                                    parameters,
                                },
                            });
                        }
                    },
                    Err(e) => {
                        println!("[MCP][TOOLS][ERROR] Failed to list tools for {}: {}", config.name, e);
                    }
                }
            },
            Err(e) => {
                 println!("[MCP][TOOLS][ERROR] Failed to connect to {}: {}", config.name, e);
            }
        }
    }
    
    println!("[MCP][TOOLS] Total: {}", all_tools.len());
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
    
    // Build messages with system prompt
    let mut api_messages = vec![ApiMessage {
        role: "system".to_string(),
        content: Some(SYSTEM_PROMPT.to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];
    api_messages.extend(messages);
    
    // Get tools
    let tools = get_available_tools().await;
    let tools_opt = if tools.is_empty() { None } else { Some(tools) };

    // Build request
    let request_body = ChatRequest {
        model: profile.model.clone(),
        messages: api_messages,
        stream: true,
        temperature: profile.temperature,
        max_tokens: profile.max_tokens,
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
    
    // Make streaming request
    println!("[AI] Sending request to {} (Model: {})", url, request_body.model);
    println!("[AI] Tools count: {}", request_body.tools.as_ref().map(|t| t.len()).unwrap_or(0));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to build client: {}", e))?;
        
    let response = client
        .post(&url)
        .headers(headers)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    
    println!("[AI] Response received. Status: {}", response.status());

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();
        println!("[AI] API Error: {} - {}", status, error_body);
        return Err(format!("API error {}: {}", status, error_body));
    }
    
    // Stream response
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut full_content = String::new();
    let mut accumulated_tool_calls: Vec<ToolCall> = Vec::new();
    
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
                            // 1. Handle content
                            if let Some(content) = &choice.delta.content {
                                full_content.push_str(content);
                                let _ = app_handle.emit("chat-chunk", content.clone());
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
                                        if let Some(name) = &f.name { tc.function.name.push_str(name); }
                                        if let Some(args) = &f.arguments { tc.function.arguments.push_str(args); }
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
