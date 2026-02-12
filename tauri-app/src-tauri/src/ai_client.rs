//! AI Client for streaming chat responses
//! Supports OpenAI-compatible APIs with SSE streaming

use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::llm_profiles::{get_active_profile, LLMProvider};

/// Chat message for API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    pub content: String,
}

/// Request body for OpenAI-compatible API
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ApiMessage>,
    stream: bool,
    temperature: f32,
    max_tokens: u32,
}

/// Streaming chunk from OpenAI API
#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

/// System prompt for 1C assistant
const SYSTEM_PROMPT: &str = r#"Ты - AI-ассистент для разработки на платформе 1С:Предприятие.

Твои возможности:
- Анализ и рефакторинг кода на языке BSL (1С)
- Объяснение логики кода
- Поиск ошибок и предложение исправлений
- Написание нового кода по описанию
- Форматирование и улучшение читаемости кода

ПРАВИЛА ГЕНЕРАЦИИ КОДА (SCOPE CONTROL):
1. Если тебе передан контекст кода (CURRENT CODE CONTEXT), фокусируйся строго на запрашиваемом фрагменте или месте вставки.
2. ЗАПРЕЩЕНО изменять сигнатуры существующих функций/процедур, если об этом не просили.
3. ПРАВИЛО ТИШИНЫ: Тебе категорически ЗАПРЕЩЕНО исправлять старый код в частях модуля, которые ты не должен изменять. 
4. ПОМЕТКА ДЕЙСТВИЙ (ОБЯЗАТЕЛЬНО): Начинай блок кода ```bsl СТРОГО с комментария-тега на первой строке ВНУТРИ блока:
   - `// ACTION: REPLACE [Имя]` - если ты заменяешь существующую функцию/процедуру полностью.
   - `// ACTION: ADD` - если ты добавляешь новый код (в конец модуля или в указанную область).
   - `// ACTION: FULL_MODULE` - если ты возвращаешь весь модуль целиком (используй редко).
   Пример:
   ```bsl
   // ACTION: REPLACE МояФункция
   Функция МояФункция()
       ...
   КонецФункции
   ```
5. Если в контексте указано "Focus: Конец Модуля", ты должен генерировать только новый код с тегом `// ACTION: ADD`.
6. Если задача касается только одной функции, верни ТОЛЬКО эту исправленную функцию (вместе с ее сигнатурой и КонецФункции) с тегом `// ACTION: REPLACE` ВНУТРИ блока.

Используй русский язык в ответах. Форматируй код СТРОГО в блоках ```bsl...```. Сопроводительный текст пиши ДО или ПОСЛЕ блоков кода.
"#;

/// Stream chat completion from OpenAI-compatible API
/// Returns the full accumulated response text
pub async fn stream_chat_completion(
    messages: Vec<ApiMessage>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let profile = get_active_profile().ok_or("No active LLM profile")?;
    
    let api_key = profile.get_api_key();
    if api_key.is_empty() {
        return Err("API key not configured for this profile".to_string());
    }
    
    // Build base URL
    let base_url = profile.base_url.clone().unwrap_or_else(|| {
        match profile.provider {
            LLMProvider::OpenAI => "https://api.openai.com/v1".to_string(),
            LLMProvider::Anthropic => "https://api.anthropic.com/v1".to_string(),
            LLMProvider::OpenRouter => "https://openrouter.ai/api/v1".to_string(),
            LLMProvider::Google => "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
            LLMProvider::DeepSeek => "https://api.deepseek.com".to_string(),
            LLMProvider::Groq => "https://api.groq.com/openai/v1".to_string(),
            LLMProvider::Mistral => "https://api.mistral.ai/v1".to_string(),
            LLMProvider::XAI => "https://api.x.ai/v1".to_string(),
            LLMProvider::Perplexity => "https://api.perplexity.ai".to_string(),
            LLMProvider::Custom => String::new(),
        }
    });
    
    let url = format!("{}/chat/completions", base_url);
    
    // Build messages with system prompt
    let mut api_messages = vec![ApiMessage {
        role: "system".to_string(),
        content: SYSTEM_PROMPT.to_string(),
    }];
    api_messages.extend(messages);
    
    // Build request
    let request_body = ChatRequest {
        model: profile.model.clone(),
        messages: api_messages,
        stream: true,
        temperature: profile.temperature,
        max_tokens: profile.max_tokens,
    };
    
    // Build headers
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_key))
            .map_err(|e| e.to_string())?,
    );
    
    // For OpenRouter, add extra headers
    if matches!(profile.provider, LLMProvider::OpenRouter) {
        headers.insert(
            "HTTP-Referer",
            HeaderValue::from_static("https://mini-ai-1c.local"),
        );
        headers.insert(
            "X-Title",
            HeaderValue::from_static("Mini AI 1C Agent"),
        );
    }
    
    // Make streaming request
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .headers(headers)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    
    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, error_body));
    }
    
    // Stream response
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut full_response = String::new();
    
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        buffer.push_str(&chunk_str);
        
        // Process complete SSE events
        while let Some(pos) = buffer.find("\n\n") {
            let event = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();
            
            for line in event.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        return Ok(full_response);
                    }
                    
                    if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                        if let Some(choice) = chunk.choices.first() {
                            if let Some(content) = &choice.delta.content {
                                full_response.push_str(content);
                                let _ = app_handle.emit("chat-chunk", content.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(full_response)
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
    
    // Also try ```1c just in case
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
    
    // Also try ``` without prefix
    start_pos = 0;
    while let Some(start) = text[start_pos..].find("```") {
        let block_start = start_pos + start + 3;
        // Skip if it was already handled (bsl or 1c)
        let is_prefixed = text[start_pos..start_pos + start + 6].contains("```bsl") || 
                          text[start_pos..start_pos + start + 5].contains("```1c");
        
        if let Some(end) = text[block_start..].find("```") {
             let code = &text[block_start..block_start + end];
             // If this block wasn't already captured, and it's not empty, add it
             let clean_code = code.trim().to_string();
             if !clean_code.is_empty() && !blocks.contains(&clean_code) {
                 // For generic block, if it starts with a language name on the first line, strip it
                 if clean_code.starts_with("bsl\n") || clean_code.starts_with("1c\n") {
                      blocks.push(clean_code.lines().skip(1).collect::<Vec<_>>().join("\n").trim().to_string());
                 } else {
                      blocks.push(clean_code);
                 }
             }
             start_pos = block_start + end + 3;
        } else {
            break;
        }
    }
    
    blocks
}


/// Fetch models from provider
pub async fn fetch_models(profile: &crate::llm_profiles::LLMProfile) -> Result<Vec<String>, String> {
    let api_key = profile.get_api_key();
    if api_key.is_empty() {
        return Err("API key not configured".to_string());
    }

    let base_url = profile.get_base_url();
    // Heuristic: append /models if not present, strip /v1 if needed? 
    // Most /v1 base_urls need /models appended.
    let url = if base_url.ends_with("/chat/completions") {
        base_url.replace("/chat/completions", "/models")
    } else {
        format!("{}/models", base_url.trim_end_matches('/'))
    };

    let client = reqwest::Client::new();
    let mut builder = client.get(&url);

    builder = builder
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {}", api_key));

    // Special handling for OpenRouter
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
    
    // Parse OpenAI format: { "data": [ { "id": "..." } ] }
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
    // Simply try to fetch models as a connection test
    match fetch_models(profile).await {
        Ok(models) => Ok(format!("Success! Found {} models.", models.len())),
        Err(e) => Err(format!("Connection failed: {}", e)),
    }
}
