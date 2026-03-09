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
}

use super::bsl::BSLDiagnostic;

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


            // Stream chat completion
            let response_msg = stream_chat_completion(api_messages.clone(), task_app_handle.clone()).await;
            
            let assistant_msg = match response_msg {
                Ok(m) => m,
                Err(e) => {
                    return Err(e);
                }
            };
            
            // Add assistant response to history
            api_messages.push(assistant_msg.clone());

            // 1. Check for tool calls
            if let Some(tool_calls) = &assistant_msg.tool_calls {
                let _ = task_app_handle.emit("chat-status", "Ожидаю подтверждения...");
                let _ = task_app_handle.emit("waiting-for-approval", serde_json::json!({
                    "count": tool_calls.len()
                }));
                
                // Wait for approval signal
                let approved = rx.recv().await.unwrap_or(false);
                
                if !approved {
                    let _ = task_app_handle.emit("chat-status", "Действие отклонено пользователем");
                    crate::app_log!("[AI][LOOP] Tool calls rejected by user");
                    
                    for tool_call in tool_calls {
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
                crate::app_log!("[AI][LOOP] Processing {} tool calls (Approved)", tool_calls.len());

                for tool_call in tool_calls {
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
                    
                    api_messages.push(ApiMessage {
                        role: "tool".to_string(),
                        content: Some(tool_result),
                        tool_call_id: Some(tool_call.id.clone()),
                        tool_calls: None,
                        name: Some(tool_name.clone()),
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
                    break; // Already asked once, nothing to show
                }
            }
            // Check for BSL blocks
            let bsl_blocks = extract_bsl_code(full_text);

            if bsl_blocks.is_empty() {
                break;
            }

            let _ = task_app_handle.emit("chat-status", "Проверка BSL кода...");
            
            let validation_result = tokio::time::timeout(
                tokio::time::Duration::from_secs(30),
                async {
                    let mut client: tokio::sync::MutexGuard<crate::bsl_client::BSLClient> = bsl_state.lock().await;
                    if !client.is_connected() {
                        let _ = client.connect().await;
                    }

                    let mut all_errors: Vec<String> = Vec::new();
                    let mut ui_diagnostics: Vec<BSLDiagnostic> = Vec::new();

                    for (idx, code) in bsl_blocks.iter().enumerate() {
                        let uri = format!("file:///iteration_{}_{}.bsl", current_iteration, idx);
                        match client.analyze_code(code, &uri).await {
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
                break;
            }
            break;
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

    match join_handle.await {
        Ok(res) => res,
        Err(e) => {
            if e.is_cancelled() {
                 let _ = app_handle.emit("chat-status", "");
                 Err("Cancelled".to_string())
            } else {
                 Err(format!("Task panic: {}", e))
            }
        }
    }
}
