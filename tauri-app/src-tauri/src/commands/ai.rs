use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use crate::ai::{extract_bsl_code, stream_chat_completion, ApiMessage};

/// Simplified tool call structure from frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendToolCall {
    pub id: String,
    pub r#type: String,
    pub function: FrontendToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendToolCallFunction {
    pub name: String,
    pub arguments: String,
}

/// Chat message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<FrontendToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// State for managing active chat task
#[derive(Default)]
pub struct ChatState {
    pub abort_handle: tokio::sync::Mutex<Option<tokio::task::AbortHandle>>,
    pub approval_tx: tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<bool>>>,
    /// Channel for injecting user messages mid-loop (interrupt)
    pub interrupt_tx: tokio::sync::Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>,
}

use super::bsl::BSLDiagnostic;

/// Maximum tool calls per iteration to prevent context explosion.
/// Excess calls are SILENTLY DROPPED from history (no error messages that confuse the model).
const MAX_PARALLEL_TOOL_CALLS: usize = 5;

/// Context token threshold — when exceeded, old tool-result rounds are pruned.
/// System prompt ≈ 5000t. Total input threshold = 7000 + 5000 = ~12000t,
/// safely below the ~13000t hallucination threshold observed in testing.
const CONTEXT_PRUNE_THRESHOLD: usize = 7000;

/// Maximum chars per tool result to prevent a single large response from blowing up context.
/// 8000 chars ≈ 2000 tokens per tool result.
const MAX_TOOL_RESULT_CHARS: usize = 8000;

/// Estimates token count for a slice of messages (chars / 4 approximation).
fn estimate_tokens(messages: &[ApiMessage]) -> usize {
    messages.iter().map(|m| {
        let content_len = m.content.as_deref().map(|c| c.len()).unwrap_or(0);
        let tc_len = m.tool_calls.as_ref().map(|tc| {
            tc.iter().map(|t| t.function.arguments.len() + t.function.name.len() + 10).sum::<usize>()
        }).unwrap_or(0);
        (content_len + tc_len) / 4
    }).sum()
}

/// Prunes old tool-call rounds from the context to keep it under `max_tokens`.
///
/// A "round" = one assistant message with tool_calls + all following tool messages.
/// Rounds are removed oldest-first. The most recent round is always preserved.
/// User messages and system messages are never removed.
fn prune_tool_context(messages: &mut Vec<ApiMessage>, max_tokens: usize) {
    if estimate_tokens(messages) <= max_tokens {
        return;
    }

    // Find all tool rounds: (start_idx, end_idx) inclusive
    // Each round starts with assistant+tool_calls, ends before next non-tool message
    let mut rounds: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role == "assistant" && messages[i].tool_calls.is_some() {
            let start = i;
            let mut end = i;
            let mut j = i + 1;
            while j < messages.len() && messages[j].role == "tool" {
                end = j;
                j += 1;
            }
            if end > start {
                rounds.push((start, end));
            }
            i = j;
        } else {
            i += 1;
        }
    }

    // Always keep the most recent round; prune from oldest
    if rounds.len() < 2 {
        return;
    }
    let prunable_count = rounds.len() - 1;
    let mut removed_total = 0usize;

    for idx in 0..prunable_count {
        let (start, end) = rounds[idx];
        let actual_start = start.saturating_sub(removed_total);
        let actual_end = end.saturating_sub(removed_total);
        if actual_end >= messages.len() { break; }
        let count = actual_end - actual_start + 1;
        messages.drain(actual_start..=actual_end);
        removed_total += count;
        crate::app_log!("[AI][PRUNE] Removed tool round (was [{},{}]), {} msgs pruned total. Tokens now ~{}t",
            start, end, removed_total, estimate_tokens(messages));
        if estimate_tokens(messages) <= max_tokens {
            break;
        }
    }
}

/// Stop the current chat generation
#[tauri::command]
pub async fn stop_chat(
    state: tauri::State<'_, ChatState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    // Release the approval channel first to unblock approve_tool waiters
    {
        let mut tx_guard = state.approval_tx.lock().await;
        if let Some(tx) = tx_guard.take() {
            // Send reject to unblock any pending rx.recv() in the streaming loop
            let _ = tx.send(false).await;
        }
    }
    let mut handle_guard = state.abort_handle.lock().await;
    if let Some(handle) = handle_guard.take() {
        handle.abort();
    }
    // Always emit chat-done so the frontend isLoading state is reset
    let _ = app_handle.emit("chat-status", "");
    let _ = app_handle.emit("chat-done", ());
    Ok(())
}


/// Approve the pending tool call
#[tauri::command]
pub async fn approve_tool(
    state: tauri::State<'_, ChatState>,
) -> Result<(), String> {
    let guard = state.approval_tx.lock().await;
    if let Some(tx) = &*guard {
        let _ = tx.send(true).await;
        Ok(())
    } else {
        Err("No pending tool call to approve".to_string())
    }
}

/// Reject the pending tool call
#[tauri::command]
pub async fn reject_tool(
    state: tauri::State<'_, ChatState>,
) -> Result<(), String> {
    let guard = state.approval_tx.lock().await;
    if let Some(tx) = &*guard {
        let _ = tx.send(false).await;
        Ok(())
    } else {
        Err("No pending tool call to reject".to_string())
    }
}

/// Inject a user message into the active agentic loop without aborting it.
/// Returns true if the message was accepted (active loop exists), false otherwise.
/// When false the frontend should fall back to the message queue.
#[tauri::command]
pub async fn interrupt_chat(
    message: String,
    state: tauri::State<'_, ChatState>,
) -> Result<bool, String> {
    let guard = state.interrupt_tx.lock().await;
    if let Some(tx) = &*guard {
        Ok(tx.send(message).is_ok())
    } else {
        Ok(false)
    }
}

/// Stream chat response using AI client with automatic BSL correction
#[tauri::command]
pub async fn stream_chat(
    messages: Vec<ChatMessage>,
    app_handle: AppHandle,
    _state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>,
    chat_state: tauri::State<'_, ChatState>,
) -> Result<(), String> {
    // Create channel for tool approval
    let (tx, mut rx) = tokio::sync::mpsc::channel::<bool>(1);
    {
        let mut guard = chat_state.approval_tx.lock().await;
        *guard = Some(tx);
    }

    // Create channel for mid-loop interrupt messages
    let (interrupt_tx, mut interrupt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    {
        let mut guard = chat_state.interrupt_tx.lock().await;
        *guard = Some(interrupt_tx);
    }

    // Convert to API messages
    let mut api_messages: Vec<ApiMessage> = messages
        .into_iter()
        .map(|m| {
            // Convert frontend tool_calls to backend ToolCall format
            let tool_calls = m.tool_calls.map(|tcs| {
                tcs.into_iter().map(|tc| crate::ai::models::ToolCall {
                    id: tc.id,
                    r#type: tc.r#type,
                    function: crate::ai::models::ToolCallFunction {
                        name: tc.function.name,
                        arguments: tc.function.arguments,
                    },
                }).collect::<Vec<_>>()
            });

            ApiMessage {
                role: m.role,
                content: if m.content.is_empty() && tool_calls.is_some() {
                    // assistant message with tool_calls may have empty content (valid)
                    None
                } else {
                    Some(m.content)
                },
                tool_calls,
                tool_call_id: m.tool_call_id,
                name: m.name,
            }
        })
        .collect();

    // Spawn the work into a cancellable task
    let task_app_handle = app_handle.clone();
    
    let join_handle = tokio::spawn(async move {
        // 1. Initial status
        let _ = task_app_handle.emit("chat-status", "Инициализация...");
        
        let bsl_state = task_app_handle.state::<Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>();
        let settings = crate::settings::load_settings();

        let max_iterations = settings.max_agent_iterations.unwrap_or(u32::MAX);
        let mut current_iteration = 0;
        // Guard: ask AI to write text response only once (when it returns thinking-only with no text)
        let mut asked_for_text_response = false;

        loop {
            current_iteration += 1;
            let _ = task_app_handle.emit("chat-iteration", current_iteration);

            if current_iteration > max_iterations {
                let _ = task_app_handle.emit("chat-chunk", &format!("\n\n**[Система] Достигнут лимит итераций диалога ({}).** Пожалуйста, уточните запрос или продолжите в новом сообщении.", max_iterations));
                break;
            }

            // Prune old tool rounds to keep context under threshold
            prune_tool_context(&mut api_messages, CONTEXT_PRUNE_THRESHOLD);

            // Stream chat completion
            let response_msg = stream_chat_completion(api_messages.clone(), task_app_handle.clone()).await;
            
            let assistant_msg = match response_msg {
                Ok(m) => m,
                Err(e) => {
                    return Err(e);
                }
            };
            
            // Add assistant response to history, truncating excess tool calls.
            // We modify the stored version so tool_call_ids match exactly what we'll execute.
            // Excess tool calls are dropped silently (no error messages that confuse the model).
            let assistant_msg_to_push = {
                let mut m = assistant_msg.clone();
                if let Some(tc) = &mut m.tool_calls {
                    if tc.len() > MAX_PARALLEL_TOOL_CALLS {
                        crate::app_log!("[AI][LOOP] Truncating tool_calls in history: {} → {}",
                            tc.len(), MAX_PARALLEL_TOOL_CALLS);
                        tc.truncate(MAX_PARALLEL_TOOL_CALLS);
                    }
                }
                m
            };
            api_messages.push(assistant_msg_to_push);

            // 1. Check for tool calls (use original to get full count for UI)
            if let Some(tool_calls) = &assistant_msg.tool_calls {
                let tool_calls_limited: Vec<_> = tool_calls.iter()
                    .take(MAX_PARALLEL_TOOL_CALLS)
                    .collect();
                let _ = task_app_handle.emit("chat-status", "Ожидаю подтверждения...");
                let _ = task_app_handle.emit("waiting-for-approval", serde_json::json!({
                    "count": tool_calls_limited.len()
                }));
                
                // Wait for approval signal
                let approved = rx.recv().await.unwrap_or(false);
                
                if !approved {
                    let _ = task_app_handle.emit("chat-status", "Действие отклонено пользователем");
                    crate::app_log!("[AI][LOOP] Tool calls rejected by user");
                    for tool_call in &tool_calls_limited {
                        api_messages.push(ApiMessage {
                            role: "tool".to_string(),
                            content: Some("Error: Action rejected by user".to_string()),
                            tool_call_id: Some(tool_call.id.clone()),
                            tool_calls: None,
                            name: Some(tool_call.function.name.clone()),
                        });
                    }
                    continue;
                }

                let _ = task_app_handle.emit("chat-status", "Вызов MCP...");
                crate::app_log!("[AI][LOOP] Processing {} tool calls (Approved)", tool_calls_limited.len());

                for tool_call in &tool_calls_limited {
                    let tool_name = &tool_call.function.name;
                    let _ = task_app_handle.emit("chat-status", format!("Вызов MCP: {}...", tool_name));
                    let arguments: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
                        .unwrap_or(serde_json::json!({}));
                    
                    crate::app_log!("[AI][TOOL] Executing: {} with args: {}", tool_name, arguments);

                    let mut tool_result = "Error: Tool not found".to_string();
                    let mut all_configs = settings.mcp_servers.clone();
                    
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
                        if !config.enabled { continue; }
                        
                        if let Ok(client) = crate::mcp_client::McpClient::new(config.clone()).await {
                            if let Ok(tools) = client.list_tools().await {
                                let target_tool = tools.into_iter().find(|t| {
                                    let sanitized = t.name.chars()
                                        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                                        .collect::<String>();
                                    sanitized == *tool_name
                                });

                                if let Some(t) = target_tool {
                                    match client.call_tool(&t.name, arguments.clone()).await {
                                        Ok(res) => {
                                            tool_result = res.to_string();
                                            let _ = task_app_handle.emit("tool-call-completed", serde_json::json!({
                                                "id": tool_call.id,
                                                "status": "done",
                                                "result": tool_result
                                            }));
                                        },
                                        Err(e) => {
                                            tool_result = format!("Error calling tool: {}", e);
                                            let _ = task_app_handle.emit("tool-call-completed", serde_json::json!({
                                                "id": tool_call.id,
                                                "status": "error",
                                                "result": tool_result
                                            }));
                                        },
                                    }
                                    break;
                                }
                            }
                        }
                    }
                    
                    // Truncate large tool results to prevent context explosion
                    if tool_result.len() > MAX_TOOL_RESULT_CHARS {
                        // Find last valid UTF-8 char boundary at or before the byte limit
                        let boundary = (0..=MAX_TOOL_RESULT_CHARS).rev()
                            .find(|&i| tool_result.is_char_boundary(i))
                            .unwrap_or(0);
                        tool_result.truncate(boundary);
                        tool_result.push_str("\n... [результат усечён]");
                        crate::app_log!("[AI][TOOL] Result truncated to {}b for {}", boundary, tool_name);
                    }
                    api_messages.push(ApiMessage {
                        role: "tool".to_string(),
                        content: Some(tool_result),
                        tool_call_id: Some(tool_call.id.clone()),
                        tool_calls: None,
                        name: Some(tool_name.clone()),
                    });
                }

                // Check for interrupt message after all tool calls finish
                if let Ok(interrupt_msg) = interrupt_rx.try_recv() {
                    crate::app_log!("[AI][INTERRUPT] Injecting user message mid-loop");
                    let _ = task_app_handle.emit("chat-interrupt-injected", &interrupt_msg);
                    let wrapped = format!(
                        "[СТОП. ПОЛЬЗОВАТЕЛЬ ПРЕРВАЛ ТЕКУЩУЮ ЗАДАЧУ]\n\n{}\n\n[Немедленно прекрати текущую задачу. Ответь пользователю на его сообщение выше.]",
                        interrupt_msg
                    );
                    api_messages.push(ApiMessage {
                        role: "user".to_string(),
                        content: Some(wrapped),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                }

                continue;
            }

            // 2. If no tool calls — check for empty response (thinking-only, TTFT=0)
            let full_text = assistant_msg.content.as_deref().unwrap_or("");

            if full_text.is_empty() {
                if !asked_for_text_response {
                    asked_for_text_response = true;
                    let _ = task_app_handle.emit("chat-status", "Запрашиваю текстовый ответ...");
                    api_messages.push(ApiMessage {
                        role: "user".to_string(),
                        content: Some("Напиши свой ответ текстом.".to_string()),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                    continue;
                } else {
                    // Model returned empty response twice — likely context too large
                    crate::app_log!("[AI] Model returned empty response twice (context ~{}t). Emitting fallback.",
                        api_messages.iter().map(|m| m.content.as_deref().unwrap_or("").len() / 4).sum::<usize>());
                    let _ = task_app_handle.emit("chat-chunk",
                        "\n\n> **[Система]** Модель не смогла сформировать ответ (вероятно, контекст диалога слишком велик). Попробуйте начать новый чат или сократить историю.");
                    break;
                }
            }
            // Check for BSL blocks
            let bsl_blocks = extract_bsl_code(full_text);

            if bsl_blocks.is_empty() {
                if let Ok(interrupt_msg) = interrupt_rx.try_recv() {
                    crate::app_log!("[AI][INTERRUPT] Injecting user message after text response");
                    let _ = task_app_handle.emit("chat-interrupt-injected", &interrupt_msg);
                    let wrapped = format!(
                        "[СТОП. ПОЛЬЗОВАТЕЛЬ ПРЕРВАЛ ТЕКУЩУЮ ЗАДАЧУ]\n\n{}\n\n[Немедленно прекрати текущую задачу. Ответь пользователю на его сообщение выше.]",
                        interrupt_msg
                    );
                    api_messages.push(ApiMessage {
                        role: "user".to_string(),
                        content: Some(wrapped),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                    continue;
                }
                break;
            }

            let _ = task_app_handle.emit("chat-status", "Проверка BSL кода...");
            
            let validation_result = tokio::time::timeout(
                tokio::time::Duration::from_secs(30),
                async {
                    // Проверяем подключение один раз до цикла
                    {
                        let mut client = bsl_state.lock().await;
                        if !client.is_connected() {
                            let _ = client.connect().await;
                        }
                    } // lock освобождён

                    let mut all_errors: Vec<String> = Vec::new();
                    let mut ui_diagnostics: Vec<BSLDiagnostic> = Vec::new();

                    for (idx, code) in bsl_blocks.iter().enumerate() {
                        let uri = format!("file:///iteration_{}_{}.bsl", current_iteration, idx);
                        // Захватываем и освобождаем lock на каждой итерации
                        let result = {
                            let mut client = bsl_state.lock().await;
                            client.analyze_code(code, &uri).await
                        };
                        match result {
                            Ok(diagnostics) => {
                                for d in &diagnostics {
                                    let msg_lower = d.message.to_lowercase();
                                    if msg_lower.contains("каноническ") ||
                                       msg_lower.contains("пробел") ||
                                       msg_lower.contains("canonical") ||
                                       msg_lower.contains("comments") {
                                        continue;
                                    }

                                    ui_diagnostics.push(BSLDiagnostic {
                                        line: d.range.start.line,
                                        character: d.range.start.character,
                                        message: d.message.clone(),
                                        severity: match d.severity {
                                            Some(1) => "error".to_string(),
                                            Some(2) => "warning".to_string(),
                                            _ => "info".to_string(),
                                        },
                                    });
                                }

                                let errors: Vec<crate::bsl_client::Diagnostic> = diagnostics.into_iter()
                                    .filter(|d| d.severity == Some(1))
                                    .collect();

                                if !errors.is_empty() {
                                    let error_str = errors.iter()
                                        .map(|e| format!("- Line {}: {}", e.range.start.line + 1, e.message))
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    all_errors.push(format!("Block {}:\n{}", idx + 1, error_str));
                                }
                            }
                            Err(_) => {}
                        }
                    }
                    (all_errors, ui_diagnostics)
                }
            ).await;

            let (all_errors, ui_diagnostics) = match validation_result {
                Ok(res) => res,
                Err(_) => {
                    let _ = task_app_handle.emit("chat-status", "Ошибка проверки кода: Таймаут (30с)");
                    break;
                }
            };

            let _ = task_app_handle.emit("bsl-validation-result", &ui_diagnostics);

            if all_errors.is_empty() {
                if let Ok(interrupt_msg) = interrupt_rx.try_recv() {
                    crate::app_log!("[AI][INTERRUPT] Injecting user message after BSL-clean response");
                    let _ = task_app_handle.emit("chat-interrupt-injected", &interrupt_msg);
                    let wrapped = format!(
                        "[СТОП. ПОЛЬЗОВАТЕЛЬ ПРЕРВАЛ ТЕКУЩУЮ ЗАДАЧУ]\n\n{}\n\n[Немедленно прекрати текущую задачу. Ответь пользователю на его сообщение выше.]",
                        interrupt_msg
                    );
                    api_messages.push(ApiMessage {
                        role: "user".to_string(),
                        content: Some(wrapped),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                    continue;
                }
                break;
            }
            // BSL errors present — check interrupt before retrying
            if let Ok(interrupt_msg) = interrupt_rx.try_recv() {
                crate::app_log!("[AI][INTERRUPT] Injecting user message (BSL errors path)");
                let _ = task_app_handle.emit("chat-interrupt-injected", &interrupt_msg);
                let wrapped = format!(
                    "[СТОП. ПОЛЬЗОВАТЕЛЬ ПРЕРВАЛ ТЕКУЩУЮ ЗАДАЧУ]\n\n{}\n\n[Немедленно прекрати текущую задачу. Ответь пользователю на его сообщение выше.]",
                    interrupt_msg
                );
                api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: Some(wrapped),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
                continue;
            }
            break;
        }

        // Clear interrupt channel on loop exit
        {
            // interrupt_rx is dropped here (local), interrupt_tx in ChatState will
            // return SendError on next interrupt_chat call — frontend falls back to queue.
        }

        let _ = task_app_handle.emit("chat-status", "");
        let _ = task_app_handle.emit("chat-done", ());
        Ok(())
    });

    // Store the abort handle
    let abort_handle = join_handle.abort_handle();
    {
        let mut guard = chat_state.abort_handle.lock().await;
        *guard = Some(abort_handle);
    }

    let result = match join_handle.await {
        Ok(res) => res,
        Err(e) => {
            if e.is_cancelled() {
                 let _ = app_handle.emit("chat-status", "");
                 Err("Cancelled".to_string())
            } else {
                 Err(format!("Task panic: {}", e))
            }
        }
    };

    // Clear interrupt channel — subsequent interrupt_chat calls will return false
    {
        let mut guard = chat_state.interrupt_tx.lock().await;
        *guard = None;
    }

    result
}
