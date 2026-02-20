//! Tauri commands for IPC with frontend

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::llm_profiles::{self, LLMProfile, ProfileStore};
use crate::settings::{self, AppSettings};
use tokio_tungstenite::connect_async;
use std::time::Duration;
use tauri::{Runtime, AppHandle, Manager, Emitter};


/// Chat message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Get application settings
#[tauri::command]
pub fn get_settings() -> AppSettings {
    crate::app_log!("[DEBUG] get_settings called");
    settings::load_settings()
}

/// Save application settings
#[tauri::command]
pub fn save_settings(new_settings: AppSettings) -> Result<(), String> {
    settings::save_settings(&new_settings)
}

/// Mark onboarding as completed
#[tauri::command]
pub fn complete_onboarding() -> Result<(), String> {
    let mut settings = settings::load_settings();
    settings.onboarding_completed = true;
    settings::save_settings(&settings)
}

#[tauri::command]
pub fn reset_onboarding() -> Result<(), String> {
    let mut settings = settings::load_settings();
    settings.onboarding_completed = false;
    settings::save_settings(&settings)
}

/// Restart the application
#[tauri::command]
pub fn restart_app_cmd(app_handle: tauri::AppHandle) {
    app_handle.restart();
}

/// Check if Java is installed and available in PATH
#[tauri::command]
pub fn check_java_cmd() -> bool {
    use std::process::Command;
    
    // Try verification by running java -version
    #[cfg(target_os = "windows")]
    let output = Command::new("cmd")
        .args(["/C", "java -version"])
        .output();
        
    #[cfg(not(target_os = "windows"))]
    let output = Command::new("java")
        .arg("-version")
        .output();

    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Get all LLM profiles
#[tauri::command]
pub fn get_profiles() -> ProfileStore {
    llm_profiles::load_profiles()
}

/// Save profile
#[tauri::command]
pub fn save_profile(mut profile: LLMProfile, api_key: Option<String>) -> Result<(), String> {
    if let Some(key) = api_key {
        profile.set_api_key(&key);
    }

    let mut store = llm_profiles::load_profiles();

    // Update or add profile
    if let Some(pos) = store.profiles.iter().position(|p| p.id == profile.id) {
        store.profiles[pos] = profile;
    } else {
        store.profiles.push(profile);
    }

    llm_profiles::save_profiles(&store)
}

/// Delete a profile
#[tauri::command]
pub fn delete_profile(profile_id: String) -> Result<(), String> {
    let mut store = llm_profiles::load_profiles();

    // Check if profile exists
    if !store.profiles.iter().any(|p| p.id == profile_id) {
        return Err("Профиль не найден".to_string());
    }

    // Don't allow deleting the last profile
    if store.profiles.len() <= 1 {
        return Err("Нельзя удалить последний профиль".to_string());
    }

    // Remove the profile
    store.profiles.retain(|p| p.id != profile_id);

    // If we deleted the active profile, pick the first available one
    if store.active_profile_id == profile_id {
        if let Some(first) = store.profiles.first() {
            store.active_profile_id = first.id.clone();
        }
    }

    llm_profiles::save_profiles(&store)
}

/// Set active profile
#[tauri::command]
pub fn set_active_profile(profile_id: String) -> Result<(), String> {
    let mut store = llm_profiles::load_profiles();

    if !store.profiles.iter().any(|p| p.id == profile_id) {
        return Err("Profile not found".to_string());
    }

    store.active_profile_id = profile_id;
    llm_profiles::save_profiles(&store)
}

/// State for managing active chat task
#[derive(Default)]
pub struct ChatState {
    pub abort_handle: tokio::sync::Mutex<Option<tokio::task::AbortHandle>>,
    pub approval_tx: tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<bool>>>,
}

/// Stop the current chat generation
#[tauri::command]
pub async fn stop_chat(
    state: tauri::State<'_, ChatState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let mut handle_guard = state.abort_handle.lock().await;
    if let Some(handle) = handle_guard.take() {
        handle.abort();
        use tauri::Emitter;
        let _ = app_handle.emit("chat-status", "Generation stopped by user");
    }
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
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>,
    chat_state: tauri::State<'_, ChatState>,
) -> Result<(), String> {
    use crate::ai_client::{extract_bsl_code, stream_chat_completion, ApiMessage};
    use tauri::{Emitter, Manager};
    
    // Create channel for tool approval
    let (tx, mut rx) = tokio::sync::mpsc::channel::<bool>(1);
    {
        let mut guard = chat_state.approval_tx.lock().await;
        *guard = Some(tx);
    }

    // 1. Initial status
    let _ = app_handle.emit("chat-status", "Думаю...");

    // Convert to API messages
    let mut api_messages: Vec<ApiMessage> = messages
        .into_iter()
        .map(|m| ApiMessage {
            role: m.role,
            content: Some(m.content),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        })
        .collect();

    // Spawn the work into a cancellable task
    let task_app_handle = app_handle.clone();
    
    let join_handle = tokio::spawn(async move {
        // 1. Initial status
        let _ = task_app_handle.emit("chat-status", "Думаю...");
        
        let bsl_state = task_app_handle.state::<Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>();
        let settings = crate::settings::load_settings();

        let mut current_iteration = 0;
        const MAX_ITERATIONS: u32 = 7;

        loop {
            current_iteration += 1;
            let _ = task_app_handle.emit("chat-iteration", current_iteration);

            if current_iteration > MAX_ITERATIONS {
                let _ = task_app_handle.emit("chat-chunk", "\n\n**[Система] Достигнут лимит итераций диалога (7).** Пожалуйста, уточните запрос или продолжите в новом сообщении.");
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
                    println!("[AI][LOOP] Tool calls rejected by user");
                    
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
                println!("[AI][LOOP] Processing {} tool calls (Approved)", tool_calls.len());

                for tool_call in tool_calls {
                    let tool_name = &tool_call.function.name;
                    let _ = task_app_handle.emit("chat-status", format!("Вызов MCP: {}...", tool_name));
                    // Deserialize arguments safely
                    let arguments: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
                        .unwrap_or(serde_json::json!({}));
                    
                    println!("[AI][TOOL] Executing: {} with args: {}", tool_name, arguments);

                    let mut tool_result = "Error: Tool not found".to_string();
                    let mut found = false;

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
                        if !config.enabled { continue; }
                        
                        if let Ok(client) = crate::mcp_client::McpClient::new(config.clone()).await {
                            if let Ok(tools) = client.list_tools().await {
                                // Find tool by sanitized name
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
                                    found = true;
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

            // 2. If no tool calls, check for BSL blocks
            let full_text = assistant_msg.content.as_deref().unwrap_or("");
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
                                    // FILTER: Ignore stylistic errors to prevent "Legacy Code" noise
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
                                    .filter(|d| d.severity == Some(1)) // Only Errors
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

            // Emit BSL validation results for UI
            let _ = task_app_handle.emit("bsl-validation-result", &ui_diagnostics);

            if all_errors.is_empty() {
                break;
            }

            // 3. Request fix if errors found
            // DISABLE AUTO-FIX LOOP to prevent AI from messing up legacy code
            /* 
            // SOFTENED PROMPT: Allow AI to ignore legacy errors
            let fix_prompt = format!(
                "В сгенерированном коде обнаружены потенциальные ошибки:\n\n{}\n\nВАЖНО: Исправляй эти ошибки ТОЛЬКО если они находятся в ТВОЕМ НОВОМ коде. Если это ошибки в старом коде (Legacy) или 'Новый Шрифт' — ИГНОРИРУЙ ЭТО СООБЩЕНИЕ и просто верни код как был. НЕ ПЕРЕПИСЫВАЙ РАБОЧИЙ КОД.",
                all_errors.join("\n\n")
            );

            api_messages.push(ApiMessage {
                role: "user".to_string(),
                content: Some(fix_prompt),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });

            let _ = task_app_handle.emit("chat-chunk", "\n\n---\n*Обнаружены ошибки. Исправляю...*\n\n".to_string());
            */
            // Break loop immediately after first generation if we are not fixing errors
            break;
        }

        let _ = task_app_handle.emit("chat-status", ""); // Clear status
        let _ = task_app_handle.emit("chat-done", ());
        Ok(())
    });

    // Store the abort handle
    let abort_handle = join_handle.abort_handle();
    {
        let mut guard = chat_state.abort_handle.lock().await;
        *guard = Some(abort_handle);
    }

    // Wait for task to finish
    match join_handle.await {
        Ok(res) => res, // Forward task result
        Err(e) => {
            if e.is_cancelled() {
                 // Clean up status if cancelled
                 let _ = app_handle.emit("chat-status", "");
                 Err("Cancelled".to_string())
            } else {
                 Err(format!("Task panic: {}", e))
            }
        }
    }
}


/// BSL analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BSLDiagnostic {
    pub line: u32,
    pub character: u32,
    pub message: String,
    pub severity: String,
}

/// Analyze BSL code
#[tauri::command]
pub async fn analyze_bsl(
    code: String,
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>
) -> Result<Vec<BSLDiagnostic>, String> {
    crate::app_log!("[BSL] Requesting analysis of {} chars", code.len());
    let mut client = state.inner().lock().await;
    
    // Ensure connected
    if !client.is_connected() {
        if let Err(_) = client.connect().await {
             // Try to connect but don't fail hard if LS just started
        }
    }

    // Use unique URI to ensure fresh analysis each time
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let uri = format!("file:///temp_{}.bsl", timestamp);

    let diagnostics = client.analyze_code(&code, &uri).await?;
    
    let result: Vec<BSLDiagnostic> = diagnostics.iter().map(|d| BSLDiagnostic {
        line: d.range.start.line,
        character: d.range.start.character,
        message: d.message.clone(),
        severity: match d.severity {
            Some(1) => "error".to_string(),
            Some(2) => "warning".to_string(),
            Some(3) => "info".to_string(),
            _ => "hint".to_string(),
        },
    }).collect();
    
    Ok(result)
}

/// Format BSL code
#[tauri::command]
pub async fn format_bsl(
    code: String, 
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>
) -> Result<String, String> {
    crate::app_log!("[BSL] Requesting format of {} chars", code.len());
    let mut client = state.inner().lock().await;
    
    // Ensure connected
    if !client.is_connected() {
        let _ = client.connect().await;
    }
    
    
    client.format_code(&code, "file:///temp.bsl").await
}

// get_definition_context_cmd removed

// ============== Configurator Integration ==============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
}

/// Find 1C Configurator windows
#[tauri::command]
pub fn find_configurator_windows_cmd(pattern: String) -> Vec<WindowInfo> {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::find_configurator_windows(&pattern)
            .into_iter()
            .map(|w| WindowInfo { hwnd: w.hwnd, title: w.title })
            .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Check if there is an active selection in the window
#[tauri::command]
pub fn check_selection_state(hwnd: isize) -> bool {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::is_selection_active(hwnd)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        false
    }
}

/// Get code from 1C Configurator window
/// Get code from 1C Configurator window
#[tauri::command]
pub fn get_code_from_configurator(hwnd: isize, use_select_all: Option<bool>) -> Result<String, String> {
    crate::app_log!("[1C] get_code (HWND: {}, select_all: {:?})", hwnd, use_select_all);
    #[cfg(windows)]
    {
        use crate::configurator;
        // Default to false if not provided
        configurator::get_selected_code(hwnd, use_select_all.unwrap_or(false))
    }
    #[cfg(not(windows))]
    {
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Get active fragment from 1C Configurator window
#[tauri::command]
pub fn get_active_fragment_cmd(hwnd: isize) -> Result<String, String> {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::get_active_fragment(hwnd)
    }
    #[cfg(not(windows))]
    {
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Paste code to 1C Configurator window with conflict detection
#[tauri::command]
pub async fn paste_code_to_configurator<R: Runtime>(
    app_handle: AppHandle<R>,
    hwnd: isize,
    code: String,
    use_select_all: Option<bool>,
    original_content: Option<String>,
) -> Result<(), String> {
    crate::app_log!("[1C] paste_code (HWND: {}, len: {})", hwnd, code.len());
    #[cfg(windows)]
    {
        use crate::configurator;
        use crate::history_manager;
        
        let select_all = use_select_all.unwrap_or(false);
        
        // 1. Read current code for conflict detection & snapshot
        if let Ok(current_code) = configurator::get_selected_code(hwnd, select_all) {
            // 2. Conflict detection: compare hash of current code vs original
            if let Some(ref original) = original_content {
                let original_hash = configurator::calculate_content_hash(original);
                let current_hash = configurator::calculate_content_hash(&current_code);
                
                if original_hash != current_hash {
                    return Err("CONFLICT: Код в Конфигураторе был изменён с момента последнего чтения. Получите код заново перед применением.".to_string());
                }
            }
            
            // 3. Save snapshot for undo
            history_manager::save_snapshot(hwnd, current_code).await;
        }
        
        // 4. Paste code (with selection restoration built into configurator::paste_code)
        let result = configurator::paste_code(hwnd, &code, select_all);
        
        if result.is_ok() {
            // Эмитим событие сброса диффа для всех слушателей (Problem #4)
            let _ = app_handle.emit("RESET_DIFF", code);
        }
        
        result
    }
    #[cfg(not(windows))]
    {
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Undo last code change in 1C Configurator
#[tauri::command]
pub async fn undo_last_change(hwnd: isize) -> Result<(), String> {
    crate::app_log!("[1C] undo_last_change (HWND: {})", hwnd);
    #[cfg(windows)]
    {
        use crate::configurator;
        use crate::history_manager;
        
        if let Some(snapshot) = history_manager::pop_snapshot(hwnd).await {
            // Restore code (usually requires select all if we want to replace back)
            configurator::paste_code(hwnd, &snapshot.original_code, true)
        } else {
            Err("No history for this window".to_string())
        }
    }
    #[cfg(not(windows))]
    {
        Err("Configurator integration is only available on Windows".to_string())
    }
}

// ============== Chat History ==============

use crate::chat_history::{self, ChatSession};

/// Get all chat sessions
#[tauri::command]
pub fn get_chat_sessions() -> Vec<ChatSession> {
    chat_history::get_sessions()
}

/// Get active chat session
#[tauri::command]
pub fn get_active_chat() -> ChatSession {
    chat_history::get_active_session()
}

/// Create new chat
#[tauri::command]
pub fn create_chat() -> ChatSession {
    chat_history::create_new_session()
}

/// Switch to chat session
#[tauri::command]
pub fn switch_chat(session_id: String) -> Result<ChatSession, String> {
    chat_history::set_active_session(&session_id)
}

/// Delete chat session
#[tauri::command]
pub fn delete_chat(session_id: String) -> Result<(), String> {
    chat_history::delete_session(&session_id)
}

/// Save message to active chat
#[tauri::command]
pub fn save_chat_message(role: String, content: String) -> Result<(), String> {
    chat_history::save_message(&role, &content)
}

// Hotkeys removed

// ============== LLM Utilities ==============

/// Fetch models for a profile
#[tauri::command]
pub async fn fetch_models_cmd(profile_id: String) -> Result<Vec<String>, String> {
    let store = llm_profiles::load_profiles();
    let profile = store.profiles.iter().find(|p| p.id == profile_id)
        .ok_or("Profile not found")?;
    
    crate::ai_client::fetch_models(profile).await
}

/// Test connection for a profile
#[tauri::command]
pub async fn test_llm_connection_cmd(profile_id: String) -> Result<String, String> {
    let store = llm_profiles::load_profiles();
    let profile = store.profiles.iter().find(|p| p.id == profile_id)
        .ok_or("Profile not found")?;
    
    crate::ai_client::test_connection(profile).await
}

/// Fetch models from a specific provider using API and Registry
#[tauri::command]
pub async fn fetch_models_from_provider(
    provider_id: String,
    base_url: String,
    api_key: String
) -> Result<Vec<crate::llm::providers::Model>, String> {
    use crate::llm::providers;
    
    // 1. Fetch from API
    let api_models = providers::fetch_models_from_api(&provider_id, &base_url, &api_key).await?;

    if api_models.is_empty() {
         return Err("Provider returned empty model list".to_string());
    }

    // 2. Fetch Registry
    let registry = providers::fetch_registry().await
        .unwrap_or_else(|e| {
             println!("Failed to fetch registry: {}", e);
             providers::RegistryData { providers: std::collections::HashMap::new() }
        });

    // 3. Merge
    let merged = providers::merge_models(api_models, &registry, &provider_id);
    
    Ok(merged)
}

/// Fetch models for an existing profile (using stored key)
#[tauri::command]
pub async fn fetch_models_for_profile(profile_id: String) -> Result<Vec<crate::llm::providers::Model>, String> {
    use crate::llm::providers;
    
    let store = llm_profiles::load_profiles();
    let profile = store.profiles.iter().find(|p| p.id == profile_id)
        .ok_or("Profile not found")?;

    let api_key = profile.get_api_key();

    let base_url = profile.get_base_url(); // Logic to get URL or default
    // Note: profile.get_base_url() might return default if None.
    // We should use the same defaults as in LLMSettings or rely on `ai_client` defaults?
    // `profile.get_base_url()` in `llm_profiles.rs` might handles it.
    // Let's assume it returns a valid base URL string.
    
    // 1. Fetch from API
    let api_models = providers::fetch_models_from_api(&profile.provider.to_string(), &base_url, &api_key).await?;

    if api_models.is_empty() {
         return Err("Provider returned empty model list".to_string());
    }

    // 2. Fetch Registry
    let registry = providers::fetch_registry().await
        .unwrap_or_else(|e| {
             println!("Failed to fetch registry: {}", e);
             providers::RegistryData { providers: std::collections::HashMap::new() }
        });

    // 3. Merge
    let merged = providers::merge_models(api_models, &registry, &profile.provider.to_string());
    
    Ok(merged)
}

// ============== BSL Utilities ==============

#[derive(Debug, Serialize)]
pub struct BslStatus {
    pub installed: bool,
    pub java_info: String,
    pub connected: bool,
}

/// Check BSL LS status
#[tauri::command]
pub async fn check_bsl_status_cmd(
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>
) -> Result<BslStatus, String> {
    use crate::bsl_client::BSLClient;
    println!("[DEBUG] check_bsl_status_cmd called");
    let settings = settings::load_settings();
    
    let installed = BSLClient::check_install(&settings.bsl_server.jar_path);
    let java_info = BSLClient::check_java(&settings.bsl_server.java_path);
    
    // Use try_lock to avoid blocking the UI thread if another task (like connect) is holding the lock
    let connected = if let Ok(client) = state.inner().try_lock() {
        client.is_connected()
    } else {
        println!("[DEBUG] check_bsl_status_cmd: state is locked, returning connected=false");
        false
    };
    
    let status = BslStatus {
        installed,
        java_info,
        connected,
    };
    println!("[DEBUG] check_bsl_status_cmd result: {:?}", status);
    Ok(status)
}

/// Install (download) BSL Language Server
#[tauri::command]
pub async fn install_bsl_ls_cmd(app: tauri::AppHandle) -> Result<String, String> {
    crate::bsl_installer::download_bsl_ls(app).await
}

/// Reconnect BSL Language Server (stop and restart)
#[tauri::command]
pub async fn reconnect_bsl_ls_cmd(
    state: tauri::State<'_, Arc<tokio::sync::Mutex<crate::bsl_client::BSLClient>>>
) -> Result<(), String> {
    {
        let mut client = state.inner().lock().await;
        // Stop current server if running
        client.stop();
        // Start server again
        client.start_server()?;
    }
    
    // Wait a bit for server to start in background
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    
    // Try to connect
    let mut client = state.inner().lock().await;
    client.connect().await?;
    
    Ok(())
}

/// Diagnose BSL LS launch issues
#[derive(Serialize, Deserialize, Debug)]
pub struct BslDiagnosticItem {
    pub status: String, // "ok", "warn", "error"
    pub title: String,
    pub message: String,
    pub suggestion: Option<String>,
}

#[tauri::command]
pub async fn diagnose_bsl_ls_cmd() -> Vec<BslDiagnosticItem> {
    let settings = settings::load_settings();
    let mut report = Vec::new();

    // 1. Check Java version
    let mut java_cmd = std::process::Command::new(&settings.bsl_server.java_path);
    java_cmd.arg("-version");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        java_cmd.creation_flags(0x08000000);
    }

    match java_cmd.output() {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let version_line = stderr.lines().next().unwrap_or("unknown").to_string();
            
            // Parse Java major version
            let java_version = parse_java_major_version(&stderr);
            if let Some(ver) = java_version {
                if ver < 17 {
                    report.push(BslDiagnosticItem {
                        status: "error".to_string(),
                        title: "Несовместимая версия Java".to_string(),
                        message: format!("Найдена Java {}, но требуется версия 17 или выше.", ver),
                        suggestion: Some("Установите Java 17+ (например, Eclipse Temurin) или winget install EclipseAdoptium.Temurin.17.JDK".to_string()),
                    });
                } else {
                    report.push(BslDiagnosticItem {
                        status: "ok".to_string(),
                        title: "Java Runtime".to_string(),
                        message: format!("Найдена совместимая версия: {}", version_line),
                        suggestion: None,
                    });
                }
            } else {
                report.push(BslDiagnosticItem {
                    status: "warn".to_string(),
                    title: "Версия Java".to_string(),
                    message: format!("Java найдена ({}), но не удалось определить мажорную версию.", version_line),
                    suggestion: Some("Убедитесь, что у вас установлена Java 17 или выше.".to_string()),
                });
            }
        }
        Err(e) => {
            report.push(BslDiagnosticItem {
                status: "error".to_string(),
                title: "Java не найдена".to_string(),
                message: format!("Ошибка при поиске Java по пути '{}': {}", settings.bsl_server.java_path, e),
                suggestion: Some("Установите Java 17+ и укажите корректный путь в настройках.".to_string()),
            });
        }
    }

    // 2. Check JAR
    let jar_path_str = &settings.bsl_server.jar_path;
    let jar_path = std::path::Path::new(jar_path_str);
    if jar_path.exists() {
        if let Ok(meta) = std::fs::metadata(jar_path) {
            let size_mb = meta.len() as f64 / 1024.0 / 1024.0;
            if size_mb < 1.0 {
                report.push(BslDiagnosticItem {
                    status: "error".to_string(),
                    title: "JAR файл поврежден".to_string(),
                    message: format!("Файл найден, но его размер ({:.2} МБ) слишком мал.", size_mb),
                    suggestion: Some("Удалите файл и нажмите 'Download' в настройках BSL Server.".to_string()),
                });
            } else {
                report.push(BslDiagnosticItem {
                    status: "ok".to_string(),
                    title: "BSL Server JAR".to_string(),
                    message: format!("Файл найден и готов к работе ({:.1} МБ).", size_mb),
                    suggestion: None,
                });

                // 3. Try to run JAR with --help to verify execution
                let mut test_cmd = std::process::Command::new(&settings.bsl_server.java_path);
                test_cmd.args(["-jar", jar_path_str, "--help"]);
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    test_cmd.creation_flags(0x08000000);
                }

                match test_cmd.output() {
                    Ok(output) => {
                        if output.status.success() {
                            report.push(BslDiagnosticItem {
                                status: "ok".to_string(),
                                title: "Запуск сервера".to_string(),
                                message: "Тестовый запуск JAR прошел успешно.".to_string(),
                                suggestion: None,
                            });
                        } else {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            let error_msg = if stderr.contains("UnsupportedClassVersionError") {
                                "Несовместимая версия Java при попытке запуска JAR.".to_string()
                            } else {
                                format!("Сервер не запустился (код: {}).", output.status)
                            };
                            
                            report.push(BslDiagnosticItem {
                                status: "error".to_string(),
                                title: "Ошибка запуска JAR".to_string(),
                                message: error_msg,
                                suggestion: Some("Проверьте версию Java (требуется 17+) или целостность JAR-файла.".to_string()),
                            });
                        }
                    }
                    Err(e) => {
                        report.push(BslDiagnosticItem {
                            status: "error".to_string(),
                            title: "Ошибка выполнения".to_string(),
                            message: format!("Не удалось запустить процесс: {}", e),
                            suggestion: Some("Убедитесь, что Java установлена и путь к ней корректен.".to_string()),
                        });
                    }
                }
            }
        }
    } else {
        report.push(BslDiagnosticItem {
            status: "error".to_string(),
            title: "JAR файл не найден".to_string(),
            message: format!("По пути '{}' ничего не найдено.", jar_path_str),
            suggestion: Some("Нажмите 'Download' в настройках BSL Server для загрузки.".to_string()),
        });
    }

    // 4. Check port availability and respond
    let port = settings.bsl_server.websocket_port;
    let url = format!("http://127.0.0.1:{}", port);
    
    match std::net::TcpListener::bind(format!("127.0.0.1:{}", port)) {
        Ok(_) => {
            report.push(BslDiagnosticItem {
                status: "warn".to_string(),
                title: "Сетевой порт".to_string(),
                message: format!("Порт {} свободен. Это значит, что сервер BSL сейчас НЕ запущен.", port),
                suggestion: Some("Попробуйте нажать 'Reconnect' или 'Save Settings' для запуска сервера.".to_string()),
            });
        },
        Err(_) => {
            // Port is busy, let's check if it's our server
            report.push(BslDiagnosticItem {
                status: "ok".to_string(),
                title: "Сетевой порт".to_string(),
                message: format!("Порт {} занят (сервер запущен).", port),
                suggestion: None,
            });

            // Try to connect via HTTP to check if it's alive
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap_or_default();
            
            match client.get(&url).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    report.push(BslDiagnosticItem {
                        status: "ok".to_string(),
                        title: "HTTP ответ".to_string(),
                        message: format!("Сервер ответил по HTTP (статус: {}).", status),
                        suggestion: None,
                    });
                },
                Err(e) => {
                    report.push(BslDiagnosticItem {
                        status: "error".to_string(),
                        title: "Ошибка HTTP".to_string(),
                        message: format!("Порт занят, но сервер не отвечает на HTTP запрос: {}", e),
                        suggestion: Some("Возможно, порт занят другим приложением или сервер завис.".to_string()),
                    });
                }
            }

            // Try WebSocket handshake
            let ws_url = format!("ws://127.0.0.1:{}/lsp", port);
            match tokio::time::timeout(Duration::from_secs(3), connect_async(&ws_url)).await {
                Ok(Ok(_)) => {
                    report.push(BslDiagnosticItem {
                        status: "ok".to_string(),
                        title: "WebSocket соединение".to_string(),
                        message: "WebSocket рукопожатие прошло успешно.".to_string(),
                        suggestion: None,
                    });
                },
                Ok(Err(e)) => {
                    report.push(BslDiagnosticItem {
                        status: "error".to_string(),
                        title: "Ошибка WebSocket".to_string(),
                        message: format!("Не удалось установить WebSocket соединение: {}", e),
                        suggestion: Some("Проверьте настройки брандмауэра или антивируса. Также убедитесь, что URL '/lsp' корректен.".to_string()),
                    });
                },
                Err(_) => {
                    report.push(BslDiagnosticItem {
                        status: "error".to_string(),
                        title: "Таймаут WebSocket".to_string(),
                        message: "Превышено время ожидания WebSocket рукопожатия (3 сек).".to_string(),
                        suggestion: Some("Это часто случается на перегруженных системах или при блокировке сетевого трафика. Попробуйте перезапустить приложение.".to_string()),
                    });
                }
            }
        }
    }

    report
}

/// Parse major Java version from `java -version` output
fn parse_java_major_version(version_output: &str) -> Option<u32> {
    // Patterns: "11.0.20", "17.0.8", "1.8.0_381"
    for line in version_output.lines() {
        if let Some(start) = line.find('"') {
            if let Some(end) = line[start+1..].find('"') {
                let ver_str = &line[start+1..start+1+end];
                // Handle "1.8.0_xxx" format (Java 8)
                if ver_str.starts_with("1.") {
                    return ver_str.split('.').nth(1)?.parse().ok();
                }
                // Handle "11.0.20", "17.0.8" format
                return ver_str.split('.').next()?.parse().ok();
            }
        }
    }
    None
}

// scenario_test_cmd removed
// ============== Universal MCP Client ==============

use crate::mcp_client::{McpClient, McpTool, McpServerStatus};
use crate::settings::{load_settings, McpServerConfig};

/// Get available MCP tools from a specific server
#[tauri::command]
pub async fn get_mcp_tools(server_id: String) -> Result<Vec<McpTool>, String> {
    let settings = load_settings();
    let config = settings.mcp_servers.iter()
        .find(|s| s.id == server_id)
        .cloned()
        .or_else(|| {
            if server_id == "bsl-ls" {
                Some(crate::settings::McpServerConfig {
                    id: "bsl-ls".to_string(),
                    name: "BSL Language Server".to_string(),
                    enabled: settings.bsl_server.enabled,
                    transport: crate::settings::McpTransport::Internal,
                    ..Default::default()
                })
            } else {
                None
            }
        })
        .ok_or_else(|| format!("MCP server with ID '{}' not found", server_id))?;

    let client = McpClient::new(config).await?;
    client.list_tools().await
}

/// Get status of all MCP servers
#[tauri::command]
pub async fn get_mcp_server_statuses() -> Result<Vec<McpServerStatus>, String> {
    Ok(crate::mcp_client::McpManager::get_statuses().await)
}

/// Get logs of a specific MCP server
#[tauri::command]
pub async fn get_mcp_server_logs(server_id: String) -> Result<Vec<String>, String> {
    Ok(crate::mcp_client::McpManager::get_logs(&server_id).await)
}

/// Save all debug logs to a file
#[tauri::command]
pub async fn save_debug_logs(app_handle: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_dialog::DialogExt;
    
    let logs = crate::logger::get_all_logs();
    
    let file_path = app_handle.dialog()
        .file()
        .add_filter("Text", &["txt"])
        .set_file_name("mini-ai-1c-logs.txt")
        .blocking_save_file();
        
    if let Some(path) = file_path {
        std::fs::write(path.to_string(), logs)
            .map_err(|e| format!("Failed to write logs: {}", e))?;
        crate::app_log!("Logs saved successfully to {}", path.to_string());
    }
    
    Ok(())
}

/// Call an MCP tool on a specific server
#[tauri::command]
pub async fn call_mcp_tool(server_id: String, name: String, arguments: serde_json::Value) -> Result<serde_json::Value, String> {
    let settings = load_settings();
    let config = settings.mcp_servers.iter()
        .find(|s| s.id == server_id)
        .cloned()
        .or_else(|| {
            if server_id == "bsl-ls" {
                Some(crate::settings::McpServerConfig {
                    id: "bsl-ls".to_string(),
                    name: "BSL Language Server".to_string(),
                    enabled: settings.bsl_server.enabled,
                    transport: crate::settings::McpTransport::Internal,
                    ..Default::default()
                })
            } else {
                None
            }
        })
        .ok_or_else(|| format!("MCP server with ID '{}' not found", server_id))?;

    let client = McpClient::new(config).await?;
    client.call_tool(&name, arguments).await
}

/// Test connection to an MCP server
#[tauri::command]
pub async fn test_mcp_connection(config: McpServerConfig) -> Result<String, String> {
    let client = McpClient::new(config).await?;
    match client.list_tools().await {
        Ok(tools) => Ok(format!("Подключено! ({})", tools.len())),
        Err(e) => Err(format!("Ошибка: {}", e)),
    }
}
