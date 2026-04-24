//! OpenAI Codex Responses API client
//!
//! Implements streaming completion via the Responses API (`POST /v1/responses`).
//! Translates between our `ApiMessage` format (OpenAI Chat Completions compatible)
//! and the Codex Responses API format, then parses SSE events back.
//!
//! Reference: https://github.com/router-for-me/CLIProxyAPI (translator/codex/openai/)

use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Emitter;

use super::models::{ApiMessage, Tool, ToolCall, ToolCallFunction};
use crate::llm_profiles::{
    get_active_profile, normalize_codex_reasoning_effort, DEFAULT_CODEX_REASONING_EFFORT,
    DEFAULT_CODEX_STREAM_TIMEOUT_SECS,
};

const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const CODEX_RESPONSES_ENDPOINT: &str = "/responses";
const CODEX_USER_AGENT: &str = "codex-cli/0.1.0 (Windows NT 10.0; x86_64) vscode/1.111.0";
const DEFAULT_CODEX_INSTRUCTIONS: &str = "You are Codex, a coding assistant running inside Mini AI 1C.\n\
IMPORTANT: Always output raw characters — never use HTML entities. \
Write `<`, `>`, `&`, `\"`, `'` literally. \
Do NOT write `&lt;`, `&gt;`, `&amp;`, `&quot;`, `&#39;` or any other HTML escape sequences. \
This applies to all code, diffs, BSL/1C code, and explanations.";
/// Codex requires tool names ≤ 64 characters
const MAX_TOOL_NAME_LEN: usize = 64;
const CODEX_HTTP_TIMEOUT_SECS: u64 = 120;

fn resolve_codex_stream_timeout_secs(configured_timeout_secs: Option<u32>) -> u32 {
    configured_timeout_secs.unwrap_or(DEFAULT_CODEX_STREAM_TIMEOUT_SECS)
}

/// Decode HTML entities that Codex sometimes emits (e.g. `&amp;` → `&`).
fn unescape_html(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
}

// ─── Request types ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct CodexRequest {
    model: String,
    instructions: String,
    input: Vec<CodexInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<CodexTool>>,
    stream: bool,
    reasoning: CodexReasoning,
    include: Vec<String>,
    store: bool,
}

#[derive(Serialize)]
struct CodexReasoning {
    effort: String,
    summary: String,
}

/// A single item in the `input` array
#[derive(Serialize)]
#[serde(untagged)]
enum CodexInputItem {
    Message(CodexMessage),
    FunctionCall(CodexFunctionCall),
    FunctionCallOutput(CodexFunctionCallOutput),
}

#[derive(Serialize)]
struct CodexMessage {
    role: String,
    content: Value, // string or array of content parts
}

#[derive(Serialize)]
struct CodexFunctionCall {
    r#type: String, // "function_call"
    call_id: String,
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct CodexFunctionCallOutput {
    r#type: String, // "function_call_output"
    call_id: String,
    output: String,
}

#[derive(Serialize)]
struct CodexTool {
    r#type: String, // "function"
    name: String,
    description: String,
    parameters: Value,
}

// ─── SSE Event types ─────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct SseEvent {
    r#type: String,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    item: Option<Value>,
    #[serde(default)]
    call_id: Option<String>,
}

// ─── Message translation ─────────────────────────────────────────────────────

/// Sanitize tool name: keep only alphanumeric, '_', '-'; truncate to 64 chars.
fn sanitize_tool_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if sanitized.len() > MAX_TOOL_NAME_LEN {
        sanitized[..MAX_TOOL_NAME_LEN].to_string()
    } else {
        sanitized
    }
}

/// Convert our `Vec<ApiMessage>` to Codex `instructions + input[]` payload.
///
/// Mapping:
/// - `role: "system"` / `role: "developer"` → top-level `instructions`
/// - `role: "user"` → `role: "user"` message
/// - `role: "assistant"` with content → `role: "assistant"` message
/// - `role: "assistant"` with tool_calls → one `function_call` item per call
/// - `role: "tool"` → `function_call_output` item
fn messages_to_codex_payload(messages: &[ApiMessage]) -> (String, Vec<CodexInputItem>) {
    let mut instructions_parts = Vec::new();
    let mut input = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "system" | "developer" => {
                if let Some(content) = &msg.content {
                    if !content.is_empty() {
                        instructions_parts.push(content.clone());
                    }
                }
            }

            "user" => {
                let content = msg.content.clone().unwrap_or_default();
                input.push(CodexInputItem::Message(CodexMessage {
                    role: "user".to_string(),
                    content: Value::String(content),
                }));
            }

            "assistant" => {
                // Content (text) part
                if let Some(content) = &msg.content {
                    if !content.is_empty() {
                        input.push(CodexInputItem::Message(CodexMessage {
                            role: "assistant".to_string(),
                            content: Value::String(content.clone()),
                        }));
                    }
                }
                // Tool calls → individual function_call items
                if let Some(tool_calls) = &msg.tool_calls {
                    for tc in tool_calls {
                        input.push(CodexInputItem::FunctionCall(CodexFunctionCall {
                            r#type: "function_call".to_string(),
                            call_id: tc.id.clone(),
                            name: sanitize_tool_name(&tc.function.name),
                            arguments: tc.function.arguments.clone(),
                        }));
                    }
                }
            }

            "tool" => {
                // Tool result
                if let Some(call_id) = &msg.tool_call_id {
                    let output = msg.content.clone().unwrap_or_default();
                    input.push(CodexInputItem::FunctionCallOutput(
                        CodexFunctionCallOutput {
                            r#type: "function_call_output".to_string(),
                            call_id: call_id.clone(),
                            output,
                        },
                    ));
                }
            }

            _ => {} // ignore unknown roles
        }
    }

    let instructions = if instructions_parts.is_empty() {
        DEFAULT_CODEX_INSTRUCTIONS.to_string()
    } else {
        instructions_parts.join("\n\n")
    };

    (instructions, input)
}

/// Convert our Tool definitions to Codex format (same schema, just ensure name sanitization).
fn tools_to_codex(tools: &[Tool]) -> Vec<CodexTool> {
    let mut result = Vec::new();
    let mut name_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    for tool in tools {
        let base_name = sanitize_tool_name(&tool.function.name);
        let count = name_counts.entry(base_name.clone()).or_insert(0);
        let final_name = if *count == 0 {
            base_name.clone()
        } else {
            // Ensure uniqueness with suffix, keeping within 64 chars
            let suffix = format!("_{}", count);
            let trimmed_len = MAX_TOOL_NAME_LEN.saturating_sub(suffix.len());
            format!(
                "{}{}",
                &base_name[..base_name.len().min(trimmed_len)],
                suffix
            )
        };
        *count += 1;

        result.push(CodexTool {
            r#type: "function".to_string(),
            name: final_name,
            description: tool.function.description.clone(),
            parameters: tool.function.parameters.clone(),
        });
    }

    result
}

// ─── HTTP helpers ─────────────────────────────────────────────────────────

fn build_headers(access_token: &str, account_id: Option<&str>) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", access_token))
            .map_err(|e| format!("Invalid auth token: {}", e))?,
    );
    headers.insert("User-Agent", HeaderValue::from_static(CODEX_USER_AGENT));
    headers.insert("Originator", HeaderValue::from_static("codex-cli"));
    if let Some(aid) = account_id {
        if let Ok(val) = HeaderValue::from_str(aid) {
            headers.insert("ChatGPT-Account-Id", val);
        }
    }
    Ok(headers)
}

fn resolve_codex_reasoning_effort(profile: &crate::llm_profiles::LLMProfile) -> String {
    normalize_codex_reasoning_effort(profile.reasoning_effort.as_deref())
        .unwrap_or_else(|| DEFAULT_CODEX_REASONING_EFFORT.to_string())
}

fn resolve_codex_model(profile: &crate::llm_profiles::LLMProfile) -> String {
    if profile.model.trim().is_empty()
        || matches!(
            profile.model.as_str(),
            "codex-cli" | "codex-mini-latest" | "o4-mini" | "o3" | "gpt-5-3" | "gpt-5-3-instant"
        )
    {
        "gpt-5.4".to_string()
    } else {
        profile.model.clone()
    }
}

fn safe_api_error_summary(status: reqwest::StatusCode, body: &str) -> String {
    format!("status={} body_len={}", status.as_u16(), body.len())
}

fn codex_api_error_message(status: reqwest::StatusCode, body: &str) -> String {
    match status.as_u16() {
        401 => "Codex: токен недействителен. Переавторизуйтесь в настройках профиля.".to_string(),
        429 => "Codex: превышен лимит запросов. Попробуйте позже.".to_string(),
        _ => serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("Codex API error {} (подробности скрыты)", status.as_u16())),
    }
}

fn build_codex_request(
    profile: &crate::llm_profiles::LLMProfile,
    messages: &[ApiMessage],
    tools: Option<Vec<CodexTool>>,
    stream: bool,
) -> CodexRequest {
    let (instructions, input) = messages_to_codex_payload(messages);

    CodexRequest {
        model: resolve_codex_model(profile),
        instructions,
        input,
        tools,
        stream,
        reasoning: CodexReasoning {
            effort: resolve_codex_reasoning_effort(profile),
            summary: "concise".to_string(),
        },
        include: vec!["reasoning.encrypted_content".to_string()],
        store: false,
    }
}

async fn resolve_codex_access_token(profile_id: &str) -> Result<(String, Option<String>), String> {
    let (access_token, refresh_token, expires_at, account_id) =
        crate::llm::cli_providers::codex::CodexCliProvider::get_token(profile_id)?
            .ok_or("Codex CLI: требуется авторизация. Откройте настройки профиля и нажмите 'Войти через браузер'.")?;

    let access_token = if chrono::Utc::now().timestamp() as u64 + 60 > expires_at {
        if let Some(rt) = refresh_token.as_deref() {
            crate::app_log!(force: true, "[Codex] Token expired, attempting refresh...");
            match crate::llm::cli_providers::codex::CodexCliProvider::refresh_access_token(
                profile_id, rt,
            )
            .await
            {
                Ok(()) => {
                    crate::llm::cli_providers::codex::CodexCliProvider::get_token(profile_id)?
                        .map(|(at, _, _, _)| at)
                        .unwrap_or(access_token)
                }
                Err(e) => {
                    crate::app_log!(force: true, "[Codex] Token refresh failed: {}", e);
                    access_token
                }
            }
        } else {
            access_token
        }
    } else {
        access_token
    };

    Ok((access_token, account_id))
}

async fn send_codex_request(
    request_body: &CodexRequest,
    access_token: &str,
    account_id: Option<&str>,
) -> Result<reqwest::Response, String> {
    let headers = build_headers(access_token, account_id)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(CODEX_HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("HTTP client build error: {}", e))?;

    client
        .post(format!("{}{}", CODEX_BASE_URL, CODEX_RESPONSES_ENDPOINT))
        .headers(headers)
        .json(request_body)
        .send()
        .await
        .map_err(|e| format!("Codex: ошибка сети: {}", e))
}

pub async fn quick_codex_invoke(prompt: String) -> Result<String, String> {
    let profile = get_active_profile().ok_or("Нет активного LLM профиля")?;
    let (access_token, account_id) = resolve_codex_access_token(&profile.id).await?;
    let stream_timeout_secs = resolve_codex_stream_timeout_secs(profile.stream_timeout_secs);
    let messages = vec![ApiMessage {
        role: "user".to_string(),
        content: Some(prompt),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];
    let request_body = build_codex_request(&profile, &messages, None, true);

    crate::app_log!(
        force: true,
        "[Codex][QuickAction] Sending request to {}{} (model={}, effort={})",
        CODEX_BASE_URL,
        CODEX_RESPONSES_ENDPOINT,
        &request_body.model,
        &request_body.reasoning.effort
    );

    let response = send_codex_request(&request_body, &access_token, account_id.as_deref()).await?;
    let status = response.status();

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        crate::app_log!(
            force: true,
            "[Codex][QuickAction] API error: {}",
            safe_api_error_summary(status, &body)
        );
        return Err(codex_api_error_message(status, &body));
    }

    let mut stream = response.bytes_stream();
    let mut byte_buffer = Vec::new();
    let mut full_content = String::new();

    'stream_loop: loop {
        let chunk_result = match tokio::time::timeout(
            std::time::Duration::from_secs(stream_timeout_secs as u64),
            stream.next(),
        )
        .await
        {
            Err(_) => {
                return Err(format!(
                    "Codex: таймаут потока ({} сек без данных)",
                    stream_timeout_secs
                ))
            }
            Ok(None) => break 'stream_loop,
            Ok(Some(r)) => r,
        };

        let chunk = chunk_result.map_err(|e| format!("Codex stream error: {}", e))?;
        byte_buffer.extend_from_slice(&chunk);

        while let Some(pos) = byte_buffer.windows(2).position(|w| w == b"\n\n") {
            let event_bytes = byte_buffer.drain(..pos + 2).collect::<Vec<u8>>();
            let event_str = String::from_utf8_lossy(&event_bytes);

            let mut event_type = String::new();
            let mut event_data = String::new();

            for line in event_str.lines() {
                if let Some(t) = line.strip_prefix("event: ") {
                    event_type = t.trim().to_string();
                } else if let Some(d) = line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:")) {
                    event_data = d.trim().to_string();
                }
            }

            if event_data.is_empty() || event_data == "[DONE]" {
                continue;
            }

            let evt: SseEvent = match serde_json::from_str(&event_data) {
                Ok(e) => e,
                Err(_) => {
                    if event_type.is_empty() {
                        continue;
                    }
                    SseEvent {
                        r#type: event_type.clone(),
                        delta: None,
                        item: None,
                        call_id: None,
                    }
                }
            };

            match evt.r#type.as_str() {
                "response.output_text.delta" => {
                    if let Some(delta) = &evt.delta {
                        if !delta.is_empty() {
                            full_content.push_str(delta);
                        }
                    }
                }
                "response.completed" => break 'stream_loop,
                "error" => {
                    let err_msg = serde_json::from_str::<Value>(&event_data)
                        .ok()
                        .and_then(|v| v["message"].as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| format!("Codex stream error: {}", event_data));
                    return Err(err_msg);
                }
                _ => {}
            }
        }
    }

    Ok(full_content)
}

// ─── Main streaming function ──────────────────────────────────────────────

/// Main entry point: called from ai/client.rs when provider == CodexCli
pub async fn stream_codex_completion(
    messages: Vec<ApiMessage>,
    app_handle: tauri::AppHandle,
) -> Result<ApiMessage, String> {
    let profile = get_active_profile().ok_or("No active LLM profile")?;
    let profile_id = profile.id.clone();

    // Get OAuth token & auto-refresh
    let (access_token, refresh_token, expires_at, account_id) =
        crate::llm::cli_providers::codex::CodexCliProvider::get_token(&profile_id)?
            .ok_or("Codex CLI: требуется авторизация. Откройте настройки профиля и нажмите 'Войти через браузер'.")?;

    let access_token = if chrono::Utc::now().timestamp() as u64 + 60 > expires_at {
        if let Some(rt) = refresh_token.as_deref() {
            crate::app_log!(force: true, "[Codex] Token expired, attempting refresh...");
            let _ = app_handle.emit("chat-status", "Обновляю токен Codex...");
            match crate::llm::cli_providers::codex::CodexCliProvider::refresh_access_token(
                &profile_id,
                rt,
            )
            .await
            {
                Ok(()) => {
                    crate::llm::cli_providers::codex::CodexCliProvider::get_token(&profile_id)?
                        .map(|(at, _, _, _)| at)
                        .unwrap_or(access_token)
                }
                Err(e) => {
                    crate::app_log!(force: true, "[Codex] Token refresh failed: {}", e);
                    access_token // try with expired token, API will reject
                }
            }
        } else {
            access_token
        }
    } else {
        access_token
    };

    // Collect available tools
    let tool_infos = super::tools::get_available_tools().await;
    let tools: Vec<Tool> = tool_infos.iter().map(|ti| ti.tool.clone()).collect();
    let codex_tools = if tools.is_empty() {
        None
    } else {
        Some(tools_to_codex(&tools))
    };

    // Build model name
    let model = if profile.model.trim().is_empty()
        || matches!(
            profile.model.as_str(),
            "codex-cli" | "codex-mini-latest" | "o4-mini" | "o3" | "gpt-5-3" | "gpt-5-3-instant"
        ) {
        "gpt-5.4".to_string()
    } else {
        profile.model.clone()
    };
    let reasoning_effort = resolve_codex_reasoning_effort(&profile);

    // Build request
    let (instructions, input) = messages_to_codex_payload(&messages);
    let request_body = CodexRequest {
        model,
        instructions,
        input,
        tools: codex_tools,
        stream: true,
        reasoning: CodexReasoning {
            effort: reasoning_effort.clone(),
            summary: "concise".to_string(),
        },
        include: vec!["reasoning.encrypted_content".to_string()],
        store: false,
    };

    let headers = build_headers(&access_token, account_id.as_deref())?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client build error: {}", e))?;

    let url = format!("{}{}", CODEX_BASE_URL, CODEX_RESPONSES_ENDPOINT);

    crate::app_log!(
        force: true,
        "[Codex] Sending request to {} (model={}, effort={}, has_account_id={})",
        url,
        &request_body.model,
        reasoning_effort,
        account_id.is_some()
    );
    let _ = app_handle.emit("chat-status", "Отправляю запрос Codex...");

    let response = client
        .post(&url)
        .headers(headers)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Codex: ошибка сети: {}", e))?;

    let status = response.status();

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        crate::app_log!(
            force: true,
            "[Codex] API error: {}",
            safe_api_error_summary(status, &body)
        );

        // Human-readable errors
        let message = match status.as_u16() {
            401 => {
                "Codex: токен недействителен. Переавторизуйтесь в настройках профиля.".to_string()
            }
            429 => "Codex: превышен лимит запросов. Попробуйте позже.".to_string(),
            _ => {
                // Try to parse OpenAI error format
                serde_json::from_str::<Value>(&body)
                    .ok()
                    .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| {
                        format!("Codex API error {} (подробности скрыты)", status.as_u16())
                    })
            }
        };
        return Err(message);
    }

    // Parse SSE stream
    let mut stream = response.bytes_stream();
    let mut byte_buffer = Vec::new();
    let mut full_content = String::new();
    let mut accumulated_tool_calls: Vec<ToolCall> = Vec::new();

    // Tracking state for streaming tool calls
    // call_id → (name, accumulated_arguments)
    let mut pending_calls: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    let mut announced_calls: std::collections::HashSet<String> = std::collections::HashSet::new();

    let _ = app_handle.emit("chat-status", "Получаю ответ Codex...");
    let stream_timeout_secs = resolve_codex_stream_timeout_secs(profile.stream_timeout_secs);

    'stream_loop: loop {
        let chunk_result = match tokio::time::timeout(
            std::time::Duration::from_secs(stream_timeout_secs as u64),
            stream.next(),
        )
        .await
        {
            Err(_) => {
                return Err(format!(
                    "Codex: таймаут потока ({} сек без данных)",
                    stream_timeout_secs
                ))
            }
            Ok(None) => break 'stream_loop,
            Ok(Some(r)) => r,
        };

        let chunk = chunk_result.map_err(|e| format!("Codex stream error: {}", e))?;
        byte_buffer.extend_from_slice(&chunk);

        // Process complete SSE events (separated by \n\n)
        while let Some(pos) = byte_buffer.windows(2).position(|w| w == b"\n\n") {
            let event_bytes = byte_buffer.drain(..pos + 2).collect::<Vec<u8>>();
            let event_str = String::from_utf8_lossy(&event_bytes);

            let mut event_type = String::new();
            let mut event_data = String::new();

            for line in event_str.lines() {
                if let Some(t) = line.strip_prefix("event: ") {
                    event_type = t.trim().to_string();
                } else if let Some(d) = line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:")) {
                    event_data = d.trim().to_string();
                }
            }

            if event_data.is_empty() || event_data == "[DONE]" {
                continue;
            }

            let evt: SseEvent = match serde_json::from_str(&event_data) {
                Ok(e) => e,
                Err(_) => {
                    // Use event: header as type fallback
                    if event_type.is_empty() {
                        continue;
                    }
                    SseEvent {
                        r#type: event_type.clone(),
                        delta: None,
                        item: None,
                        call_id: None,
                    }
                }
            };

            match evt.r#type.as_str() {
                "response.output_text.delta" => {
                    if let Some(delta) = &evt.delta {
                        if !delta.is_empty() {
                            let clean = unescape_html(delta);
                            full_content.push_str(&clean);
                            let _ = app_handle.emit("chat-chunk", clean);
                        }
                    }
                }

                "response.reasoning_summary_text.delta" => {
                    if let Some(delta) = &evt.delta {
                        if !delta.is_empty() {
                            let _ = app_handle.emit("chat-thinking-chunk", delta.clone());
                        }
                    }
                }

                "response.output_item.added" => {
                    // New output item — check if it's a function_call
                    if let Some(item) = &evt.item {
                        let item_type = item["type"].as_str().unwrap_or("");
                        if item_type == "function_call" {
                            let call_id = item["call_id"].as_str().unwrap_or("").to_string();
                            let name = item["name"].as_str().unwrap_or("").to_string();
                            crate::app_log!(
                                "[Codex] Function call started: {} (id={})",
                                name,
                                call_id
                            );
                            pending_calls
                                .entry(call_id)
                                .or_insert((name, String::new()));
                        }
                    }
                }

                "response.function_call_arguments.delta" => {
                    if let Some(call_id) = &evt.call_id {
                        if let Some(delta) = &evt.delta {
                            let is_new = !announced_calls.contains(call_id.as_str());
                            if let Some((name, args)) = pending_calls.get_mut(call_id) {
                                args.push_str(delta);
                                // Announce tool call start on first arg delta
                                if is_new {
                                    let call_idx = accumulated_tool_calls.len();
                                    let emit_name = name.clone();
                                    let emit_id = call_id.clone();
                                    announced_calls.insert(emit_id.clone());
                                    let _ = app_handle.emit(
                                        "tool-call-started",
                                        serde_json::json!({
                                            "index": call_idx,
                                            "id": emit_id,
                                            "name": emit_name
                                        }),
                                    );
                                }
                            }
                        }
                    }
                }

                "response.function_call_arguments.done" | "response.output_item.done" => {
                    // Check if it's a completed function_call
                    if let Some(item) = &evt.item {
                        let item_type = item["type"].as_str().unwrap_or("");
                        if item_type == "function_call" {
                            let call_id = item["call_id"].as_str().unwrap_or("").to_string();
                            let name = item["name"].as_str().unwrap_or("").to_string();
                            let arguments = item["arguments"].as_str().unwrap_or("{}").to_string();

                            if !call_id.is_empty() && !name.is_empty() {
                                accumulated_tool_calls.push(ToolCall {
                                    id: call_id.clone(),
                                    r#type: "function".to_string(),
                                    function: ToolCallFunction {
                                        name: name.clone(),
                                        arguments: arguments.clone(),
                                    },
                                });
                                pending_calls.remove(&call_id);
                                crate::app_log!(
                                    "[Codex] Function call completed: {} args_len={}",
                                    name,
                                    arguments.len()
                                );
                            }
                        }
                    } else if evt.r#type == "response.function_call_arguments.done" {
                        // Alternative: arguments done event without item
                        if let Some(call_id) = &evt.call_id {
                            if let Some((name, args)) = pending_calls.remove(call_id) {
                                let arguments = if args.is_empty() {
                                    "{}".to_string()
                                } else {
                                    args
                                };
                                accumulated_tool_calls.push(ToolCall {
                                    id: call_id.clone(),
                                    r#type: "function".to_string(),
                                    function: ToolCallFunction {
                                        name: name.clone(),
                                        arguments: arguments.clone(),
                                    },
                                });
                                crate::app_log!(
                                    "[Codex] Function call completed (args.done): {} args_len={}",
                                    name,
                                    arguments.len()
                                );
                            }
                        }
                    }
                }

                "response.completed" => {
                    // Flush any remaining pending calls (shouldn't normally happen)
                    for (call_id, (name, args)) in pending_calls.drain() {
                        let arguments = if args.is_empty() {
                            "{}".to_string()
                        } else {
                            args
                        };
                        accumulated_tool_calls.push(ToolCall {
                            id: call_id,
                            r#type: "function".to_string(),
                            function: ToolCallFunction { name, arguments },
                        });
                    }

                    crate::app_log!(
                        "[Codex] response.completed — content_chars={} tool_calls={}",
                        full_content.len(),
                        accumulated_tool_calls.len()
                    );
                    break 'stream_loop;
                }

                "error" => {
                    let err_msg = serde_json::from_str::<Value>(&event_data)
                        .ok()
                        .and_then(|v| v["message"].as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| format!("Codex stream error: {}", event_data));
                    return Err(err_msg);
                }

                _ => {
                    // Ignore: response.created, response.content_part.added, etc.
                }
            }
        }
    }

    crate::app_log!(
        "[Codex] Stream complete: content_chars={} tool_calls={}",
        full_content.len(),
        accumulated_tool_calls.len()
    );

    Ok(ApiMessage {
        role: "assistant".to_string(),
        content: if full_content.is_empty() {
            None
        } else {
            Some(full_content)
        },
        tool_calls: if accumulated_tool_calls.is_empty() {
            None
        } else {
            Some(accumulated_tool_calls)
        },
        tool_call_id: None,
        name: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        build_codex_request, messages_to_codex_payload, resolve_codex_model,
        resolve_codex_stream_timeout_secs, CodexInputItem, DEFAULT_CODEX_INSTRUCTIONS,
    };
    use crate::ai::models::ApiMessage;
    use crate::llm_profiles::{
        LLMProfile, LLMProvider, DEFAULT_CODEX_REASONING_EFFORT, DEFAULT_CODEX_STREAM_TIMEOUT_SECS,
    };
    use serde_json::Value;

    #[test]
    fn messages_to_codex_payload_promotes_system_messages_to_instructions() {
        let messages = vec![
            ApiMessage {
                role: "system".to_string(),
                content: Some("system instructions".to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ApiMessage {
                role: "developer".to_string(),
                content: Some("developer instructions".to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ApiMessage {
                role: "user".to_string(),
                content: Some("user request".to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ];

        let (instructions, input) = messages_to_codex_payload(&messages);

        assert!(instructions.contains("system instructions"));
        assert!(instructions.contains("developer instructions"));
        assert_eq!(input.len(), 1);
        assert!(matches!(&input[0], CodexInputItem::Message(_)));

        if let CodexInputItem::Message(message) = &input[0] {
            assert_eq!(message.role, "user");
            assert_eq!(message.content, Value::String("user request".to_string()));
        }
    }

    #[test]
    fn messages_to_codex_payload_uses_default_instructions_without_system_messages() {
        let messages = vec![ApiMessage {
            role: "user".to_string(),
            content: Some("hello".to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }];

        let (instructions, input) = messages_to_codex_payload(&messages);

        assert_eq!(instructions, DEFAULT_CODEX_INSTRUCTIONS);
        assert_eq!(input.len(), 1);
        assert!(matches!(&input[0], CodexInputItem::Message(_)));
    }

    #[test]
    fn resolve_codex_stream_timeout_uses_codex_default_when_profile_timeout_missing() {
        assert_eq!(
            resolve_codex_stream_timeout_secs(None),
            DEFAULT_CODEX_STREAM_TIMEOUT_SECS
        );
    }

    #[test]
    fn resolve_codex_stream_timeout_preserves_profile_timeout_when_present() {
        assert_eq!(resolve_codex_stream_timeout_secs(Some(180)), 180);
    }

    #[test]
    fn resolve_codex_model_maps_legacy_aliases_to_default_model() {
        let mut profile = LLMProfile::default_profile();
        profile.provider = LLMProvider::CodexCli;
        profile.model = "codex-mini-latest".to_string();

        assert_eq!(resolve_codex_model(&profile), "gpt-5.4");
    }

    #[test]
    fn build_codex_request_uses_responses_api_shape_for_quick_invokes() {
        let mut profile = LLMProfile::default_profile();
        profile.provider = LLMProvider::CodexCli;
        profile.model = "codex-mini-latest".to_string();

        let messages = vec![ApiMessage {
            role: "user".to_string(),
            content: Some("describe this method".to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }];

        let request = build_codex_request(&profile, &messages, None, true);

        assert_eq!(request.model, "gpt-5.4");
        assert_eq!(request.reasoning.effort, DEFAULT_CODEX_REASONING_EFFORT);
        assert!(request.stream);
        assert!(request.tools.is_none());
        assert_eq!(request.input.len(), 1);
    }
}
