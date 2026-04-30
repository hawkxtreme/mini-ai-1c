//! 1С:Напарник direct HTTP client (code.1c.ai)
//!
//! Реализует прямое общение с API code.1c.ai без MCP-прослойки.
//! Поддерживает: SSE-стриминг, reasoning_content, server-side tool calls round-trip.

use futures::StreamExt;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::Emitter;

use super::models::{ApiMessage, ToolInfo};
use super::prompts::{get_system_prompt, has_code_context};
use super::tools::get_available_tools;
use crate::llm_profiles::get_active_profile;
use crate::settings::{load_settings, McpServerConfig, McpTransport};

const BASE_URL: &str = "https://code.1c.ai";
const BUILTIN_NAPARNIK_SERVER_ID: &str = "builtin-1c-naparnik";
const NAPARNIK_MCP_BRIDGE_TOOL: &str = "Read";
const MAX_NAPARNIK_TOOL_RESULT_CHARS: usize = 8000;

// ─── Session State ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct OneCSession {
    conversation_id: String,
    last_message_uuid: Option<String>,
}

lazy_static! {
    /// profile_id → сессия (conversation_id + last assistant uuid)
    static ref SESSIONS: Mutex<HashMap<String, OneCSession>> = Mutex::new(HashMap::new());
}

pub fn clear_naparnik_session(profile_id: &str) {
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.remove(profile_id);
        crate::app_log!("[Naparnik] Session cleared for profile: {}", profile_id);
    }
}

fn get_session(profile_id: &str) -> Option<OneCSession> {
    SESSIONS.lock().ok()?.get(profile_id).cloned()
}

fn save_session(profile_id: &str, session: OneCSession) {
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.insert(profile_id.to_string(), session);
    }
}

fn update_last_uuid(profile_id: &str, uuid: &str) {
    if let Ok(mut sessions) = SESSIONS.lock() {
        if let Some(s) = sessions.get_mut(profile_id) {
            s.last_message_uuid = Some(uuid.to_string());
        }
    }
}

// ─── API Structures ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct CreateConversationRequest {
    is_chat: bool,
    programming_language: String,
    skill_name: String,
    ui_language: String,
}

#[derive(Serialize)]
struct MessageRequest {
    role: String,
    content: MessageContent,
    parent_uuid: Option<String>,
}

#[derive(Serialize)]
struct MessageContent {
    content: MessageContentInner,
    tools: Vec<Value>,
}

#[derive(Serialize)]
struct MessageContentInner {
    instruction: String,
}

#[derive(Deserialize, Debug)]
struct SseChunk {
    uuid: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    content_delta: Option<ContentDelta>,
    #[serde(default)]
    finished: bool,
    #[serde(default)]
    #[allow(dead_code)]
    render_info: Option<Value>,
}

#[derive(Deserialize, Debug)]
struct ContentDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[derive(Serialize)]
struct ToolResultRequest {
    role: String,
    parent_uuid: String,
    content: Vec<Value>,
}

// ─── HTTP Helpers ─────────────────────────────────────────────────────────────

fn build_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

fn build_headers(token: &str) -> reqwest::header::HeaderMap {
    use reqwest::header::*;
    let mut h = HeaderMap::new();
    h.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    h.insert(ORIGIN, HeaderValue::from_static(BASE_URL));
    h.insert(
        REFERER,
        HeaderValue::from_str(&format!("{}/chat//", BASE_URL))
            .unwrap_or(HeaderValue::from_static("")),
    );
    h.insert(
        USER_AGENT,
        HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
    );
    if let Ok(v) = HeaderValue::from_str(token) {
        h.insert(AUTHORIZATION, v);
    }
    h
}

fn sanitize_tool_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}

fn filter_naparnik_tools(tools_info: &[ToolInfo]) -> Vec<ToolInfo> {
    tools_info
        .iter()
        .filter(|info| info.server_id != BUILTIN_NAPARNIK_SERVER_ID)
        .cloned()
        .collect()
}

fn build_naparnik_tools(tools_info: &[ToolInfo]) -> Vec<Value> {
    if tools_info.is_empty() {
        return Vec::new();
    }

    let tool_names: Vec<Value> = tools_info
        .iter()
        .map(|info| Value::String(info.tool.function.name.clone()))
        .collect();

    vec![serde_json::json!({
        "name": NAPARNIK_MCP_BRIDGE_TOOL,
        "description": "Mini AI 1C MCP bridge. Execute one of the local MCP tools listed in the system instructions. Pass the exact MCP tool name in `tool_name` and the original tool arguments in `arguments`.",
        "parameters": {
            "type": "object",
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Exact local MCP tool name to execute.",
                    "enum": tool_names
                },
                "arguments": {
                    "type": "object",
                    "description": "JSON arguments for the selected MCP tool.",
                    "additionalProperties": true
                }
            },
            "required": ["tool_name"]
        },
    })]
}

fn build_local_tool_routes(tools_info: &[ToolInfo]) -> HashMap<String, String> {
    tools_info
        .iter()
        .map(|info| (info.tool.function.name.clone(), info.server_id.clone()))
        .collect()
}

fn build_naparnik_diff_instruction(has_code_context: bool) -> &'static str {
    if has_code_context {
        r#"

[NAPARNIK TEXT DIFF MODE]
Mini AI 1C applies code edits locally after the user reviews them. Naparnik must return edit text; it must NOT call `apply_diff`, `replace_in_file`, or any other native apply/edit tool.
This section overrides generic XML diff instructions for Naparnik: when editing existing BSL code, use ONLY this SEARCH/REPLACE block format:
  <<<<<<< SEARCH
exact original complete lines
  =======
replacement complete lines
  >>>>>>> REPLACE
Rules:
- Use SEARCH/REPLACE only when the user asks to change existing code.
- Do not wrap SEARCH/REPLACE blocks in Markdown fences.
- The SEARCH part must be an exact copy of complete original lines, including indentation.
- Include enough original context to make each SEARCH block unique, but keep blocks small.
- The REPLACE part must contain the full replacement for the SEARCH part.
- Use tab characters for BSL indentation in replacement code.
- When returning SEARCH/REPLACE blocks, do not also include a full modified ```bsl code block in the same response.
- If the user asks a question or explanation only, answer normally and do not return diff blocks.
- If the original code is empty or you cannot produce an exact SEARCH block, return a full ```bsl code block instead and briefly explain that a text diff was not safe.
[/NAPARNIK TEXT DIFF MODE]"#
    } else {
        r#"

[NAPARNIK TEXT DIFF MODE]
No editable source code is loaded in Mini AI 1C. Do NOT use SEARCH/REPLACE diff blocks and do NOT call `apply_diff`.
For new BSL code, return the complete code in a ```bsl block.
[/NAPARNIK TEXT DIFF MODE]"#
    }
}

fn build_naparnik_instruction(
    system_prompt: &str,
    user_instruction: &str,
    tools_info: &[ToolInfo],
    has_code_context: bool,
) -> String {
    let diff_instruction = build_naparnik_diff_instruction(has_code_context);
    let bridge_instruction = if tools_info.is_empty() {
        String::new()
    } else {
        let tool_names = tools_info
            .iter()
            .map(|info| format!("`{}`", info.tool.function.name))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            r#"

[NAPARNIK MCP BRIDGE]
Naparnik exposes local Mini AI 1C MCP tools through a single client-side tool named `{bridge_tool}`.
Do NOT call MCP tool names directly as native Naparnik tools: direct names like `search_1c_help` will be reported by Naparnik as unknown even when the local MCP server is available.
To use any MCP tool, call `{bridge_tool}` with this JSON shape:
{{"tool_name":"exact_mcp_tool_name","arguments":{{...original MCP arguments...}}}}
Available exact MCP tool names: {tool_names}
If a `{bridge_tool}` result contains "Mini AI 1C MCP tool `<name>` result", treat that MCP tool call as successful. Never conclude that a local MCP server is unavailable only because its exact MCP tool name is not a native Naparnik tool.
[/NAPARNIK MCP BRIDGE]"#,
            bridge_tool = NAPARNIK_MCP_BRIDGE_TOOL,
            tool_names = tool_names
        )
    };

    format!(
        "[SYSTEM INSTRUCTIONS FROM MINI AI 1C]\n{}{}{}\n[/SYSTEM INSTRUCTIONS]\n\n[USER REQUEST]\n{}\n[/USER REQUEST]",
        system_prompt, diff_instruction, bridge_instruction, user_instruction
    )
}

fn all_mcp_configs() -> Vec<McpServerConfig> {
    let settings = load_settings();
    let mut configs = settings.mcp_servers.clone();

    if !configs.iter().any(|c| c.id == "bsl-ls") {
        configs.push(McpServerConfig {
            id: "bsl-ls".to_string(),
            name: "BSL Language Server".to_string(),
            enabled: settings.bsl_server.enabled,
            transport: McpTransport::Internal,
            ..Default::default()
        });
    }

    configs
}

fn truncate_tool_result(result: String) -> String {
    if result.len() <= MAX_NAPARNIK_TOOL_RESULT_CHARS {
        return result;
    }

    let boundary = (0..=MAX_NAPARNIK_TOOL_RESULT_CHARS)
        .rev()
        .find(|idx| result.is_char_boundary(*idx))
        .unwrap_or(MAX_NAPARNIK_TOOL_RESULT_CHARS);
    format!(
        "{}\n\n[Result truncated to {} chars]",
        &result[..boundary],
        MAX_NAPARNIK_TOOL_RESULT_CHARS
    )
}

fn build_naparnik_tool_result_content(
    local_name: &str,
    result: String,
    has_code_context: bool,
) -> String {
    let mut content = format!("Mini AI 1C MCP tool `{}` result:\n{}", local_name, result);

    if has_code_context {
        content.push_str(
            r#"

[NAPARNIK TEXT DIFF REMINDER]
If the next answer edits the existing BSL code, return only SEARCH/REPLACE blocks that Mini AI 1C can apply locally.
Do not return a full modified ```bsl code block when a safe SEARCH/REPLACE block can be produced.
[/NAPARNIK TEXT DIFF REMINDER]"#,
        );
    }

    content
}

fn tool_call_name(tool_call: &Value) -> Option<String> {
    tool_call
        .get("function")
        .and_then(|f| f.get("name"))
        .and_then(|v| v.as_str())
        .or_else(|| tool_call.get("name").and_then(|v| v.as_str()))
        .map(str::to_string)
}

fn tool_call_id(tool_call: &Value) -> String {
    tool_call
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn tool_call_arguments(tool_call: &Value) -> Value {
    let arguments = tool_call
        .get("function")
        .and_then(|f| f.get("arguments"))
        .or_else(|| tool_call.get("arguments"));
    let Some(arguments) = arguments else {
        return serde_json::json!({});
    };

    normalize_tool_arguments_value(arguments)
}

fn normalize_tool_arguments_value(arguments: &Value) -> Value {
    match arguments {
        Value::String(raw) => serde_json::from_str(raw).unwrap_or_else(|_| serde_json::json!({})),
        Value::Object(_) => arguments.clone(),
        _ => serde_json::json!({}),
    }
}

fn bridge_tool_request(arguments: &Value) -> Option<(String, Value)> {
    let obj = arguments.as_object()?;
    let tool_name = obj
        .get("tool_name")
        .or_else(|| obj.get("name"))
        .or_else(|| obj.get("tool"))
        .and_then(Value::as_str)?
        .to_string();
    let tool_arguments = obj
        .get("arguments")
        .or_else(|| obj.get("args"))
        .map(normalize_tool_arguments_value)
        .unwrap_or_else(|| serde_json::json!({}));

    Some((tool_name, tool_arguments))
}

fn tool_call_display_name(tool_call: &Value) -> String {
    let name = tool_call_name(tool_call).unwrap_or_else(|| "?".to_string());
    if name == NAPARNIK_MCP_BRIDGE_TOOL {
        if let Some((local_name, _)) = bridge_tool_request(&tool_call_arguments(tool_call)) {
            return local_name;
        }
    }

    name
}

async fn execute_local_mcp_tool(
    tool_name: &str,
    arguments: Value,
    local_tool_routes: &HashMap<String, String>,
) -> Result<String, String> {
    let server_id = local_tool_routes
        .get(tool_name)
        .ok_or_else(|| format!("Tool '{}' is not a local MCP tool", tool_name))?;

    let config = all_mcp_configs()
        .into_iter()
        .find(|config| config.id == *server_id && config.enabled)
        .ok_or_else(|| format!("MCP server '{}' is not enabled", server_id))?;

    let client = crate::mcp_client::McpClient::new(config.clone()).await?;
    let tools = client.list_tools().await?;
    let target_tool = tools
        .into_iter()
        .find(|tool| sanitize_tool_name(&tool.name) == tool_name)
        .ok_or_else(|| format!("Tool '{}' not found on server '{}'", tool_name, server_id))?;

    client
        .call_tool(&target_tool.name, arguments)
        .await
        .map(|result| truncate_tool_result(result.to_string()))
}

async fn create_conversation(
    client: &reqwest::Client,
    token: &str,
) -> Result<(String, Option<String>), String> {
    let url = format!("{}/chat_api/v1/conversations/", BASE_URL);
    let body = CreateConversationRequest {
        is_chat: true,
        programming_language: "1C (BSL)".to_string(),
        skill_name: "custom".to_string(),
        ui_language: "russian".to_string(),
    };

    let mut headers = build_headers(token);
    headers.insert("Session-Id", reqwest::header::HeaderValue::from_static(""));

    let resp = client
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Naparnik: failed to create conversation: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Naparnik: conversation create error {}: {}",
            status, text
        ));
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Naparnik: parse error: {}", e))?;
    let uuid = data["uuid"]
        .as_str()
        .ok_or("Naparnik: no uuid in response")?
        .to_string();
    let root_msg_uuid = data["root_message_uuid"].as_str().map(|s| s.to_string());

    crate::app_log!(
        "[Naparnik] Created conversation: {} (root_msg: {:?})",
        uuid,
        root_msg_uuid
    );
    Ok((uuid, root_msg_uuid))
}

// ─── Main Streaming Function ──────────────────────────────────────────────────

/// Main entry point: called from ai/client.rs when provider == OneCNaparnik
pub async fn stream_naparnik_completion(
    messages: Vec<ApiMessage>,
    app_handle: tauri::AppHandle,
) -> Result<ApiMessage, String> {
    let profile = get_active_profile().ok_or("No active LLM profile")?;
    let token = profile.get_api_key();
    if token.is_empty() {
        return Err(
            "1С:Напарник: токен не задан. Укажите токен code.1c.ai в настройках профиля."
                .to_string(),
        );
    }

    let profile_id = profile.id.clone();
    let client = build_client()?;

    // Ensure active session
    let session = match get_session(&profile_id) {
        Some(s) => s,
        None => {
            let _ = app_handle.emit("chat-status", "Создаю сессию Напарника...");
            let (conv_id, root_uuid) = create_conversation(&client, &token).await?;
            let s = OneCSession {
                conversation_id: conv_id,
                last_message_uuid: root_uuid,
            };
            save_session(&profile_id, s.clone());
            s
        }
    };

    // Extract last user message text
    let instruction = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.as_deref())
        .unwrap_or("")
        .to_string();

    if instruction.is_empty() {
        return Err("Naparnik: empty user message".to_string());
    }

    let _ = app_handle.emit("chat-status", "Отправляю запрос Напарнику...");

    let all_tools_info = get_available_tools().await;
    let naparnik_tools_info = filter_naparnik_tools(&all_tools_info);
    let naparnik_tools = build_naparnik_tools(&naparnik_tools_info);
    let local_tool_routes = build_local_tool_routes(&naparnik_tools_info);
    let system_prompt = get_system_prompt(&naparnik_tools_info, &messages);
    let has_code_context = has_code_context(&messages);
    let instruction = build_naparnik_instruction(
        &system_prompt,
        &instruction,
        &naparnik_tools_info,
        has_code_context,
    );

    let full_content = run_message_loop(
        &client,
        &token,
        &profile_id,
        &session.conversation_id,
        session.last_message_uuid.clone(),
        instruction,
        naparnik_tools,
        local_tool_routes,
        has_code_context,
        &app_handle,
    )
    .await?;

    Ok(ApiMessage {
        role: "assistant".to_string(),
        content: if full_content.is_empty() {
            None
        } else {
            Some(full_content)
        },
        tool_calls: None,
        tool_call_id: None,
        name: None,
    })
}

/// Sends a message and handles server-side tool_calls round-trips.
/// Returns the final accumulated text after all rounds complete.
async fn run_message_loop(
    client: &reqwest::Client,
    token: &str,
    profile_id: &str,
    conversation_id: &str,
    initial_parent_uuid: Option<String>,
    instruction: String,
    naparnik_tools: Vec<Value>,
    local_tool_routes: HashMap<String, String>,
    has_code_context: bool,
    app_handle: &tauri::AppHandle,
) -> Result<String, String> {
    let url = format!(
        "{}/chat_api/v1/conversations/{}/messages",
        BASE_URL, conversation_id
    );

    // First payload: user message
    let mut payload: Value = serde_json::to_value(MessageRequest {
        role: "user".to_string(),
        content: MessageContent {
            content: MessageContentInner { instruction },
            tools: naparnik_tools,
        },
        parent_uuid: initial_parent_uuid,
    })
    .map_err(|e| e.to_string())?;

    let mut assistant_segments: Vec<String> = Vec::new();
    let mut is_first_round = true;

    loop {
        let response = client
            .post(&url)
            .headers({
                let mut h = build_headers(token);
                h.insert(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_static("text/event-stream"),
                );
                h
            })
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Naparnik: send error: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("Naparnik: API error {}: {}", status, text));
        }

        if is_first_round {
            let _ = app_handle.emit("chat-status", "Выполнение...");
            is_first_round = false;
        }

        let tool_calls_to_send =
            process_sse_stream(response, profile_id, &mut assistant_segments, app_handle).await?;

        if tool_calls_to_send.is_empty() {
            break;
        }

        // Server-side tools executed — send tool result to continue
        let last_uuid = get_session(profile_id)
            .and_then(|s| s.last_message_uuid)
            .unwrap_or_default();

        let mut items: Vec<Value> = Vec::with_capacity(tool_calls_to_send.len());
        for tool_call in &tool_calls_to_send {
            let tc_id = tool_call_id(tool_call);
            let name = tool_call_name(tool_call).unwrap_or_default();
            let raw_arguments = tool_call_arguments(tool_call);
            let bridge_request = if name == NAPARNIK_MCP_BRIDGE_TOOL {
                bridge_tool_request(&raw_arguments)
            } else {
                None
            };
            let local_name = bridge_request
                .as_ref()
                .map(|(tool_name, _)| tool_name.clone())
                .unwrap_or_else(|| name.clone());
            let local_arguments = bridge_request
                .map(|(_, arguments)| arguments)
                .unwrap_or(raw_arguments);

            if local_tool_routes.contains_key(&local_name) {
                let _ = app_handle.emit(
                    "chat-status",
                    format!("Р’С‹Р·РѕРІ MCP РґР»СЏ РќР°РїР°СЂРЅРёРєР°: {}...", name),
                );
                let result =
                    match execute_local_mcp_tool(&local_name, local_arguments, &local_tool_routes)
                        .await
                    {
                        Ok(result) => {
                            let _ = app_handle.emit(
                                "tool-call-completed",
                                serde_json::json!({
                                    "id": tc_id.clone(),
                                    "status": "done",
                                    "name": local_name.clone(),
                                    "result": result.clone()
                                }),
                            );
                            result
                        }
                        Err(error) => {
                            let result =
                                format!("Error calling local MCP tool '{}': {}", local_name, error);
                            let _ = app_handle.emit(
                                "tool-call-completed",
                                serde_json::json!({
                                    "id": tc_id.clone(),
                                    "status": "error",
                                    "name": local_name.clone(),
                                    "result": result.clone()
                                }),
                            );
                            result
                        }
                    };

                items.push(serde_json::json!({
                    "status": "ok",
                    "tool_call_id": tc_id,
                    "name": name,
                    "content": build_naparnik_tool_result_content(
                        &local_name,
                        result,
                        has_code_context
                    )
                }));
            } else {
                items.push(serde_json::json!({
                    "status": "accepted",
                    "tool_call_id": tc_id,
                    "name": name,
                    "content": null,
                    "details": { "auto_call": true }
                }));
            }
        }

        let tool_result_req = ToolResultRequest {
            role: "tool".to_string(),
            parent_uuid: last_uuid,
            content: items,
        };

        payload = serde_json::to_value(tool_result_req).map_err(|e| e.to_string())?;
        let _ = app_handle.emit("chat-status", "Обработка инструментов Напарника...");
    }

    let full_text = assistant_segments
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(full_text)
}

/// Reads SSE stream, emits chat events, returns tool_calls list if server wants round-trip.
async fn process_sse_stream(
    response: reqwest::Response,
    profile_id: &str,
    assistant_segments: &mut Vec<String>,
    app_handle: &tauri::AppHandle,
) -> Result<Vec<Value>, String> {
    let mut stream = response.bytes_stream();
    let mut byte_buffer = Vec::<u8>::new();
    let mut accumulated_text = String::new();
    let mut is_thinking = false;
    let mut tool_calls_pending: Vec<Value> = Vec::new();

    'outer: loop {
        let chunk_result =
            match tokio::time::timeout(std::time::Duration::from_secs(60), stream.next()).await {
                Err(_) => return Err("Naparnik: stream timeout (60s)".to_string()),
                Ok(None) => break,
                Ok(Some(r)) => r,
            };

        let chunk = chunk_result.map_err(|e| format!("Naparnik: stream error: {}", e))?;
        byte_buffer.extend_from_slice(&chunk);

        // Process complete SSE events (delimited by \n\n)
        while let Some(pos) = byte_buffer.windows(2).position(|w| w == b"\n\n") {
            let event_bytes = byte_buffer.drain(..pos + 2).collect::<Vec<u8>>();
            let event_str = String::from_utf8_lossy(&event_bytes);

            for line in event_str.lines() {
                let data = if let Some(d) = line
                    .strip_prefix("data: ")
                    .or_else(|| line.strip_prefix("data:"))
                {
                    d
                } else {
                    continue;
                };
                if data == "[DONE]" {
                    break 'outer;
                }

                let chunk: SseChunk = match serde_json::from_str(data) {
                    Ok(c) => c,
                    Err(e) => {
                        crate::app_log!("[Naparnik] SSE parse error: {} | data: {:.100}", e, data);
                        continue;
                    }
                };

                let role = chunk.role.as_deref().unwrap_or("");

                // Skip user echo and tool echo (handled elsewhere)
                if (role == "user" || role == "tool") && chunk.finished {
                    continue;
                }

                // reasoning_content
                if let Some(delta) = &chunk.content_delta {
                    if let Some(reasoning) = &delta.reasoning_content {
                        if !reasoning.is_empty() {
                            if !is_thinking {
                                is_thinking = true;
                                let _ = app_handle.emit("chat-status", "Размышляю...");
                            }
                            let _ = app_handle.emit("chat-thinking-chunk", reasoning.clone());
                        }
                    }

                    // text delta
                    if let Some(text) = &delta.content {
                        if !text.is_empty() {
                            if is_thinking {
                                is_thinking = false;
                                let _ = app_handle.emit("chat-status", "Выполнение...");
                            }
                            accumulated_text.push_str(text);
                            let normalized = text
                                .replace("```1\u{0421} (BSL)", "```bsl") // Cyrillic С + (BSL)
                                .replace("```1\u{0421}", "```bsl") // Cyrillic С plain
                                .replace("```1C (BSL)", "```bsl") // Latin C + (BSL)
                                .replace("```1C\n", "```bsl\n") // Latin C + newline
                                .replace("```1C\r\n", "```bsl\r\n") // Latin C + CRLF
                                .replace("```1c (BSL)", "```bsl"); // lowercase + (BSL)
                            let _ = app_handle.emit("chat-chunk", normalized);
                        }
                    }
                }

                // cumulative content (non-delta format)
                if let Some(content_val) = &chunk.content {
                    if let Some(text) = content_val.get("content").and_then(|v| v.as_str()) {
                        if !text.is_empty() && text != accumulated_text {
                            // Only emit the new delta portion
                            if text.len() > accumulated_text.len()
                                && text.starts_with(&accumulated_text as &str)
                            {
                                let new_part = &text[accumulated_text.len()..];
                                if !new_part.is_empty() {
                                    let normalized = new_part
                                        .replace("```1\u{0421} (BSL)", "```bsl")
                                        .replace("```1\u{0421}", "```bsl")
                                        .replace("```1C (BSL)", "```bsl")
                                        .replace("```1C\n", "```bsl\n")
                                        .replace("```1C\r\n", "```bsl\r\n")
                                        .replace("```1c (BSL)", "```bsl");
                                    let _ = app_handle.emit("chat-chunk", normalized);
                                }
                            }
                            accumulated_text = text.to_string();
                        }
                    }
                }

                // Final assistant chunk
                if chunk.finished && role == "assistant" {
                    update_last_uuid(profile_id, &chunk.uuid);

                    // Collect server-side tool_calls if present
                    if let Some(content_val) = &chunk.content {
                        if let Some(tc_arr) =
                            content_val.get("tool_calls").and_then(|v| v.as_array())
                        {
                            if !tc_arr.is_empty() {
                                tool_calls_pending = tc_arr.clone();

                                // Emit tool-call-started events for UI display (read-only)
                                for (idx, tc) in tc_arr.iter().enumerate() {
                                    let name = tool_call_display_name(tc);
                                    let _ = app_handle.emit(
                                        "tool-call-started",
                                        serde_json::json!({
                                            "index": idx,
                                            "id": tc["id"].as_str().unwrap_or(""),
                                            "name": name,
                                            "naparnik": true
                                        }),
                                    );
                                    let _ = app_handle.emit(
                                        "tool-call-completed",
                                        serde_json::json!({
                                            "id": tc["id"].as_str().unwrap_or(""),
                                            "status": "naparnik",
                                            "result": ""
                                        }),
                                    );
                                }

                                // Save current text segment before tool round
                                if !accumulated_text.is_empty() {
                                    assistant_segments.push(accumulated_text.clone());
                                    accumulated_text.clear();
                                }

                                break 'outer;
                            }
                        }
                    }

                    // No tool calls — save segment and finish
                    if !accumulated_text.is_empty() {
                        assistant_segments.push(accumulated_text.clone());
                        accumulated_text.clear();
                    }
                    break 'outer;
                }
            }
        }
    }

    // Flush any remaining text
    if !accumulated_text.is_empty() {
        assistant_segments.push(accumulated_text);
    }

    Ok(tool_calls_pending)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naparnik_diff_instruction_uses_text_search_replace_when_code_is_loaded() {
        let instruction =
            build_naparnik_instruction("BASE SYSTEM", "Измени текст сообщения", &[], true);

        assert!(instruction.contains("[NAPARNIK TEXT DIFF MODE]"));
        assert!(instruction.contains("<<<<<<< SEARCH"));
        assert!(instruction.contains(">>>>>>> REPLACE"));
        assert!(instruction.contains("must NOT call `apply_diff`"));
        assert!(instruction.contains("Do not wrap SEARCH/REPLACE blocks in Markdown fences"));
        assert!(instruction.contains("do not also include a full modified ```bsl code block"));
        assert!(instruction.contains("BASE SYSTEM"));
        assert!(instruction.contains("Измени текст сообщения"));
    }

    #[test]
    fn naparnik_diff_instruction_disables_diff_without_code_context() {
        let instruction =
            build_naparnik_instruction("BASE SYSTEM", "Напиши новую функцию", &[], false);

        assert!(instruction.contains("No editable source code is loaded"));
        assert!(instruction.contains("Do NOT use SEARCH/REPLACE diff blocks"));
        assert!(instruction.contains("return the complete code in a ```bsl block"));
        assert!(!instruction.contains("exact original complete lines"));
    }

    #[test]
    fn naparnik_tool_result_reinforces_text_diff_after_mcp_when_code_is_loaded() {
        let content = build_naparnik_tool_result_content(
            "mcp__syntax-checker__validate",
            "[]".to_string(),
            true,
        );

        assert!(content.contains("Mini AI 1C MCP tool `mcp__syntax-checker__validate` result"));
        assert!(content.contains("[NAPARNIK TEXT DIFF REMINDER]"));
        assert!(content.contains("SEARCH/REPLACE"));
        assert!(content.contains("Do not return a full modified ```bsl code block"));
    }

    #[test]
    fn naparnik_tool_result_does_not_reinforce_text_diff_without_code_context() {
        let content = build_naparnik_tool_result_content(
            "mcp__syntax-checker__validate",
            "[]".to_string(),
            false,
        );

        assert!(content.contains("Mini AI 1C MCP tool `mcp__syntax-checker__validate` result"));
        assert!(!content.contains("[NAPARNIK TEXT DIFF REMINDER]"));
    }
}
