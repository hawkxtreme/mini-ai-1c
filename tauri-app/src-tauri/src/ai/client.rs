use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use tauri::Emitter;

use crate::llm_profiles::{get_active_profile, LLMProvider};
use super::models::*;
use super::prompts::*;
use super::tools::*;

/// Stream chat completion from OpenAI-compatible API
/// Returns the full accumulated response text
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
    
    let api_max_tokens = if matches!(profile.provider, LLMProvider::QwenCli) {
        65536u32
    } else if profile.max_tokens > 16384 {
        4096
    } else {
        profile.max_tokens
    };

    let thinking_enabled = matches!(profile.provider, LLMProvider::QwenCli) && profile.enable_thinking.unwrap_or(false);

    let effective_temperature = if thinking_enabled {
        1.0
    } else {
        profile.temperature
    };

    // Dynamic thinking budget: estimate total input tokens (chars / 4),
    // then allocate ~30% for thinking, clamped to [8192, 32768].
    // This prevents the model from "thinking without space" in long conversations.
    let dynamic_thinking_budget: Option<u32> = if thinking_enabled {
        let total_chars: usize = api_messages.iter()
            .map(|m| m.content.as_deref().map(|c| c.len()).unwrap_or(0))
            .sum();
        let estimated_tokens = (total_chars / 4) as u32;
        let budget = (estimated_tokens * 30 / 100).max(8192).min(32768);
        crate::app_log!("[AI] Thinking budget: {}t (input ~{}t)", budget, estimated_tokens);
        Some(budget)
    } else {
        None
    };

    let request_body = ChatRequest {
        model: profile.model.clone(),
        messages: api_messages,
        stream: true,
        temperature: effective_temperature,
        max_tokens: api_max_tokens,
        tools: tools_opt,
        enable_thinking: if thinking_enabled { Some(true) } else { None },
        thinking_budget_tokens: dynamic_thinking_budget,
    };
    
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

    if matches!(profile.provider, LLMProvider::QwenCli) {
        let hdrs = response.headers();
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
            let _ = crate::llm::cli_providers::qwen::QwenCliProvider::save_usage(&profile.id, used, limit, reset);
        }
    }

    let mut stream = response.bytes_stream();
    let mut byte_buffer = Vec::new();
    let mut full_content = String::new();
    let mut content_search_temp = String::new();
    let mut accumulated_tool_calls: Vec<ToolCall> = Vec::new();
    let mut announced_tool_calls = std::collections::HashSet::new();
    let mut is_thinking = false;
    let mut is_qwen_fn = false;
    let mut qwen_fn_buf = String::new();
    let mut has_switched_to_executing = false;
    let mut first_token_received = false;
    let start_gen_time = std::time::Instant::now();
    
    loop {
        let chunk_result = match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            stream.next()
        ).await {
            Err(_) => return Err("Stream timeout: no data from API for 30s".to_string()),
            Ok(None) => break,
            Ok(Some(r)) => r,
        };
        if !first_token_received {
            first_token_received = true;
            let ttft = start_gen_time.elapsed().as_millis();
            crate::app_log!("[AI][TIMER] TTFT (Time to First Token): {} ms", ttft);
        }
        let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;
        byte_buffer.extend_from_slice(&chunk);
        
        while let Some(pos) = byte_buffer.windows(2).position(|w| w == b"\n\n") {
            let event_bytes = byte_buffer.drain(..pos + 2).collect::<Vec<u8>>();
            let event_str = String::from_utf8_lossy(&event_bytes);
            
            for line in event_str.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
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
                            // Handle Qwen3 native reasoning_content field (enable_thinking=true)
                            if let Some(reasoning) = &choice.delta.reasoning_content {
                                if !reasoning.is_empty() {
                                    if !is_thinking {
                                        is_thinking = true;
                                        let _ = app_handle.emit("chat-status", "Размышляю...");
                                    }
                                    let _ = app_handle.emit("chat-thinking-chunk", reasoning.clone());
                                }
                            } else if is_thinking && choice.delta.content.as_deref().map(|c| !c.is_empty()).unwrap_or(false) {
                                // Thinking phase ended, text phase started
                                is_thinking = false;
                                has_switched_to_executing = true;
                                let _ = app_handle.emit("chat-status", "Выполнение...");
                            }

                            if let Some(content) = &choice.delta.content {
                                if !has_switched_to_executing && !is_thinking && !content.trim().is_empty() {
                                    let _ = app_handle.emit("chat-status", "Выполнение...");
                                    has_switched_to_executing = true;
                                }

                                content_search_temp.push_str(content);
                                
                                loop {
                                    if is_qwen_fn { break; }
                                    
                                    if !is_thinking {
                                        // Detect <tool_call>JSON</tool_call> (Qwen/other model XML format)
                                        if let Some(tc_start) = content_search_temp.find("<tool_call>") {
                                            if tc_start > 0 {
                                                let text = content_search_temp[..tc_start].to_string();
                                                full_content.push_str(&text);
                                                let _ = app_handle.emit("chat-chunk", text);
                                            }
                                            is_qwen_fn = true;
                                            // buffer includes the opening tag so we can detect </tool_call>
                                            qwen_fn_buf = content_search_temp[tc_start..].to_string();
                                            content_search_temp.clear();
                                            break;
                                        }

                                        if let Some(fn_start) = content_search_temp.find("<function=") {
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
                                            if start_pos > 0 {
                                                let text = content_search_temp[..start_pos].to_string();
                                                full_content.push_str(&text);
                                                let _ = app_handle.emit("chat-chunk", text);
                                            }
                                            is_thinking = true;
                                            let _ = app_handle.emit("chat-status", "Планирование (EN)...");
                                            content_search_temp = content_search_temp[start_pos + 10..].to_string();
                                        } else if let Some(last_lt) = content_search_temp.rfind('<') {
                                            let after_lt = content_search_temp[last_lt..].chars().nth(1);
                                            let is_potential_tag = matches!(after_lt, Some(c) if c.is_alphabetic() || c == '/' || c == '?');
                                            let potential_tag_len = content_search_temp.len() - last_lt;
                                            
                                            if is_potential_tag && potential_tag_len < 15 {
                                                if last_lt > 0 {
                                                    // Strip stray </tool_call> tags that leaked into text content
                                                    let raw = &content_search_temp[..last_lt];
                                                    let text = raw.replace("</tool_call>", "").replace("<tool_call>", "");
                                                    if !text.is_empty() {
                                                        full_content.push_str(&text);
                                                        let _ = app_handle.emit("chat-chunk", text);
                                                    }
                                                    content_search_temp = content_search_temp[last_lt..].to_string();
                                                }
                                                break;
                                            } else {
                                                let text = content_search_temp.replace("</tool_call>", "").replace("<tool_call>", "");
                                                if !text.is_empty() {
                                                    full_content.push_str(&text);
                                                    let _ = app_handle.emit("chat-chunk", text);
                                                }
                                                content_search_temp.clear();
                                                break;
                                            }
                                        } else {
                                            let text = content_search_temp.replace("</tool_call>", "").replace("<tool_call>", "");
                                            if !text.is_empty() {
                                                full_content.push_str(&text);
                                                let _ = app_handle.emit("chat-chunk", text);
                                            }
                                            content_search_temp.clear();
                                            break;
                                        }
                                    } else {
                                        if let Some(end_pos) = content_search_temp.find("</thinking>") {
                                            if end_pos > 0 {
                                                let text = content_search_temp[..end_pos].to_string();
                                                let _ = app_handle.emit("chat-thinking-chunk", text);
                                            }
                                            is_thinking = false;
                                            has_switched_to_executing = true;
                                            let _ = app_handle.emit("chat-status", "Выполнение...");
                                            content_search_temp = content_search_temp[end_pos + 11..].to_string();
                                        } else if let Some(last_lt) = content_search_temp.rfind('<') {
                                            let potential_tag_len = content_search_temp.len() - last_lt;
                                            if potential_tag_len < 15 {
                                                if last_lt > 0 {
                                                    let text = content_search_temp[..last_lt].to_string();
                                                    let _ = app_handle.emit("chat-thinking-chunk", text);
                                                    content_search_temp = content_search_temp[last_lt..].to_string();
                                                }
                                                break;
                                            } else {
                                                let _ = app_handle.emit("chat-thinking-chunk", content_search_temp.clone());
                                                content_search_temp.clear();
                                                break;
                                            }
                                        } else {
                                            let _ = app_handle.emit("chat-thinking-chunk", content_search_temp.clone());
                                            content_search_temp.clear();
                                            break;
                                        }
                                    }
                                }
                            }
                            
                            if is_qwen_fn {
                                qwen_fn_buf.push_str(&content_search_temp);
                                content_search_temp.clear();
                                // Handle <tool_call>JSON</tool_call> format (Qwen/other models)
                                if qwen_fn_buf.starts_with("<tool_call>") {
                                    if let Some(end_pos) = qwen_fn_buf.find("</tool_call>") {
                                        let json_content = qwen_fn_buf[11..end_pos].trim().to_string();
                                        let remainder = qwen_fn_buf[end_pos + 12..].to_string();
                                        qwen_fn_buf.clear();
                                        is_qwen_fn = false;
                                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_content) {
                                            let fn_name = parsed.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                                            let args = parsed.get("arguments")
                                                .map(|a| if a.is_object() { a.to_string() } else { a.as_str().unwrap_or("{}").to_string() })
                                                .unwrap_or_default();
                                            if !fn_name.is_empty() {
                                                let tc_idx = accumulated_tool_calls.len();
                                                let tc = ToolCall {
                                                    id: format!("tc_xml_{}", tc_idx),
                                                    r#type: "function".to_string(),
                                                    function: ToolCallFunction { name: fn_name.clone(), arguments: args },
                                                };
                                                let _ = app_handle.emit("tool-call-started", serde_json::json!({
                                                    "index": tc_idx, "id": tc.id, "name": fn_name
                                                }));
                                                accumulated_tool_calls.push(tc);
                                            }
                                        }
                                        content_search_temp = remainder;
                                    }
                                // Handle <function=name>...</function> format (Qwen inline)
                                } else if let Some(end_pos) = qwen_fn_buf.find("</function>") {
                                    let full_block = qwen_fn_buf[..end_pos + "</function>".len()].to_string();
                                    let remainder = qwen_fn_buf[end_pos + "</function>".len()..].to_string();
                                    qwen_fn_buf.clear();
                                    is_qwen_fn = false;
                                    if let Some(fn_name_end) = full_block[10..].find('>') {
                                        let fn_name = full_block[10..10 + fn_name_end].to_string();
                                        let mut args_map = serde_json::Map::new();
                                        let body = &full_block[10 + fn_name_end + 1..];
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
                                    content_search_temp = remainder;
                                }
                            }
                            
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
                                    // ID приходит только в первом delta — записываем только если ещё не установлен
                                    if let Some(id) = &tc_delta.id { if tc.id.is_empty() { tc.id.push_str(id); } }
                                    if let Some(f) = &tc_delta.function {
                                        if let Some(name) = &f.name {
                                            tc.function.name.push_str(name);
                                        }
                                        if let Some(args) = &f.arguments { 
                                            tc.function.arguments.push_str(args);
                                            let _ = app_handle.emit("tool-call-progress", serde_json::json!({
                                                "index": idx,
                                                "arguments": args
                                            }));
                                        }
                                    }

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
