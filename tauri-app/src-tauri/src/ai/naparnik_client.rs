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

use crate::llm_profiles::get_active_profile;
use super::models::ApiMessage;

const BASE_URL: &str = "https://code.1c.ai";

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

#[derive(Deserialize)]
struct CreateConversationResponse {
    #[allow(dead_code)]
    uuid: String,
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
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    h.insert(ORIGIN, HeaderValue::from_static(BASE_URL));
    h.insert(REFERER, HeaderValue::from_str(&format!("{}/chat//", BASE_URL)).unwrap_or(HeaderValue::from_static("")));
    h.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"));
    if let Ok(v) = HeaderValue::from_str(token) {
        h.insert(AUTHORIZATION, v);
    }
    h
}

async fn create_conversation(client: &reqwest::Client, token: &str) -> Result<(String, Option<String>), String> {
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
        return Err(format!("Naparnik: conversation create error {}: {}", status, text));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| format!("Naparnik: parse error: {}", e))?;
    let uuid = data["uuid"].as_str().ok_or("Naparnik: no uuid in response")?.to_string();
    let root_msg_uuid = data["root_message_uuid"].as_str().map(|s| s.to_string());

    crate::app_log!("[Naparnik] Created conversation: {} (root_msg: {:?})", uuid, root_msg_uuid);
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
        return Err("1С:Напарник: токен не задан. Укажите токен code.1c.ai в настройках профиля.".to_string());
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
    let instruction = messages.iter().rev()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.as_deref())
        .unwrap_or("")
        .to_string();

    if instruction.is_empty() {
        return Err("Naparnik: empty user message".to_string());
    }

    let _ = app_handle.emit("chat-status", "Отправляю запрос Напарнику...");

    let full_content = run_message_loop(
        &client,
        &token,
        &profile_id,
        &session.conversation_id,
        session.last_message_uuid.clone(),
        instruction,
        &app_handle,
    ).await?;

    Ok(ApiMessage {
        role: "assistant".to_string(),
        content: if full_content.is_empty() { None } else { Some(full_content) },
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
    app_handle: &tauri::AppHandle,
) -> Result<String, String> {
    let url = format!("{}/chat_api/v1/conversations/{}/messages", BASE_URL, conversation_id);

    // First payload: user message
    let mut payload: Value = serde_json::to_value(MessageRequest {
        role: "user".to_string(),
        content: MessageContent {
            content: MessageContentInner { instruction },
            tools: vec![],
        },
        parent_uuid: initial_parent_uuid,
    }).map_err(|e| e.to_string())?;

    let mut assistant_segments: Vec<String> = Vec::new();
    let mut is_first_round = true;

    loop {
        let response = client
            .post(&url)
            .headers({
                let mut h = build_headers(token);
                h.insert(reqwest::header::ACCEPT, reqwest::header::HeaderValue::from_static("text/event-stream"));
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

        let tool_calls_to_send = process_sse_stream(
            response,
            profile_id,
            &mut assistant_segments,
            app_handle,
        ).await?;

        if tool_calls_to_send.is_empty() {
            break;
        }

        // Server-side tools executed — send tool result to continue
        let last_uuid = get_session(profile_id)
            .and_then(|s| s.last_message_uuid)
            .unwrap_or_default();

        let items: Vec<Value> = tool_calls_to_send.iter().map(|tc| {
            let tc_id = tc["id"].as_str().unwrap_or("").to_string();
            serde_json::json!({
                "status": "accepted",
                "tool_call_id": tc_id,
                "content": null
            })
        }).collect();

        let tool_result_req = ToolResultRequest {
            role: "tool".to_string(),
            parent_uuid: last_uuid,
            content: items,
        };

        payload = serde_json::to_value(tool_result_req).map_err(|e| e.to_string())?;
        let _ = app_handle.emit("chat-status", "Обработка инструментов Напарника...");
    }

    let full_text = assistant_segments.iter()
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
        let chunk_result = match tokio::time::timeout(
            std::time::Duration::from_secs(60),
            stream.next(),
        ).await {
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
                let data = if let Some(d) = line.strip_prefix("data: ") { d } else { continue };
                if data == "[DONE]" { break 'outer; }

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
                                .replace("```1\u{0421}", "```bsl")        // Cyrillic С plain
                                .replace("```1C (BSL)", "```bsl")         // Latin C + (BSL)
                                .replace("```1C\n", "```bsl\n")           // Latin C + newline
                                .replace("```1C\r\n", "```bsl\r\n")       // Latin C + CRLF
                                .replace("```1c (BSL)", "```bsl");        // lowercase + (BSL)
                            let _ = app_handle.emit("chat-chunk", normalized);
                        }
                    }
                }

                // cumulative content (non-delta format)
                if let Some(content_val) = &chunk.content {
                    if let Some(text) = content_val.get("content").and_then(|v| v.as_str()) {
                        if !text.is_empty() && text != accumulated_text {
                            // Only emit the new delta portion
                            if text.len() > accumulated_text.len() && text.starts_with(&accumulated_text as &str) {
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
                        if let Some(tc_arr) = content_val.get("tool_calls").and_then(|v| v.as_array()) {
                            if !tc_arr.is_empty() {
                                tool_calls_pending = tc_arr.clone();

                                // Emit tool-call-started events for UI display (read-only)
                                for (idx, tc) in tc_arr.iter().enumerate() {
                                    let name = tc["function"]["name"].as_str().unwrap_or("?");
                                    let _ = app_handle.emit("tool-call-started", serde_json::json!({
                                        "index": idx,
                                        "id": tc["id"].as_str().unwrap_or(""),
                                        "name": name,
                                        "naparnik": true
                                    }));
                                    let _ = app_handle.emit("tool-call-completed", serde_json::json!({
                                        "id": tc["id"].as_str().unwrap_or(""),
                                        "status": "naparnik",
                                        "result": ""
                                    }));
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
