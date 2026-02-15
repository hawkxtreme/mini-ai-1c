//! Tauri commands for IPC with frontend

use serde::{Deserialize, Serialize};

use crate::llm_profiles::{self, LLMProfile, ProfileStore};
use crate::settings::{self, AppSettings};


/// Chat message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Get application settings
#[tauri::command]
pub fn get_settings() -> AppSettings {
    settings::load_settings()
}

/// Save application settings
#[tauri::command]
pub fn save_settings(new_settings: AppSettings) -> Result<(), String> {
    settings::save_settings(&new_settings)
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
    if profile_id == "default" {
        return Err("Cannot delete default profile".to_string());
    }

    let mut store = llm_profiles::load_profiles();
    store.profiles.retain(|p| p.id != profile_id);

    if store.active_profile_id == profile_id {
        store.active_profile_id = "default".to_string();
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
    _state: tauri::State<'_, tokio::sync::Mutex<crate::bsl_client::BSLClient>>,
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
        
        let bsl_state = task_app_handle.state::<tokio::sync::Mutex<crate::bsl_client::BSLClient>>();
        let settings = crate::settings::load_settings();

        let mut current_iteration = 0;
        const MAX_ITERATIONS: u32 = 25;

        loop {
            if current_iteration >= MAX_ITERATIONS {
                let _ = task_app_handle.emit("chat-chunk", "\n[System] Conversation iteration limit reached.");
                break;
            }
            current_iteration += 1;

            // Stream chat completion
            let response_msg = stream_chat_completion(api_messages.clone(), task_app_handle.clone()).await;
            
            let assistant_msg = match response_msg {
                Ok(m) => m,
                Err(e) => {
                    let _ = task_app_handle.emit("chat-chunk", format!("\nError: {}", e));
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

                    for config in &settings.mcp_servers {
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
            let fix_prompt = format!(
                "В сгенерированном коде обнаружены ошибки:\n\n{}\n\nПожалуйста, исправь их.",
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
    state: tauri::State<'_, tokio::sync::Mutex<crate::bsl_client::BSLClient>>
) -> Result<Vec<BSLDiagnostic>, String> {
    let mut client = state.lock().await;
    
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
    state: tauri::State<'_, tokio::sync::Mutex<crate::bsl_client::BSLClient>>
) -> Result<String, String> {
    let mut client = state.lock().await;
    
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
pub async fn paste_code_to_configurator(
    hwnd: isize,
    code: String,
    use_select_all: Option<bool>,
    original_content: Option<String>,
) -> Result<(), String> {
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
        configurator::paste_code(hwnd, &code, select_all)
    }
    #[cfg(not(windows))]
    {
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Undo last code change in 1C Configurator
#[tauri::command]
pub async fn undo_last_change(hwnd: isize) -> Result<(), String> {
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
    state: tauri::State<'_, tokio::sync::Mutex<crate::bsl_client::BSLClient>>
) -> Result<BslStatus, String> {
    use crate::bsl_client::BSLClient;
    let settings = settings::load_settings();
    
    let installed = BSLClient::check_install(&settings.bsl_server.jar_path);
    let java_info = BSLClient::check_java(&settings.bsl_server.java_path);
    
    let client = state.lock().await;
    let connected = client.is_connected();
    
    Ok(BslStatus {
        installed,
        java_info,
        connected,
    })
}

/// Install (download) BSL Language Server
#[tauri::command]
pub async fn install_bsl_ls_cmd(app: tauri::AppHandle) -> Result<String, String> {
    crate::bsl_installer::download_bsl_ls(app).await
}

/// Reconnect BSL Language Server (stop and restart)
#[tauri::command]
pub async fn reconnect_bsl_ls_cmd(
    state: tauri::State<'_, tokio::sync::Mutex<crate::bsl_client::BSLClient>>
) -> Result<(), String> {
    let mut client = state.lock().await;
    
    // Stop current server if running
    client.stop();
    
    // Start server again
    client.start_server()?;
    
    // Drop lock to allow connection
    drop(client);
    
    // Wait a bit for server to start
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    
    // Try to connect
    let mut client = state.lock().await;
    client.connect().await?;
    
    Ok(())
}

/// Diagnose BSL LS launch issues
#[tauri::command]
pub async fn diagnose_bsl_ls_cmd() -> String {
    let settings = settings::load_settings();
    let mut report = String::new();

    report.push_str(&format!("Java Path: {}\n", settings.bsl_server.java_path));
    report.push_str(&format!("JAR Path: {}\n", settings.bsl_server.jar_path));

    let jar_exists = std::path::Path::new(&settings.bsl_server.jar_path).exists();
    report.push_str(&format!("JAR Exists: {}\n", jar_exists));

    if !jar_exists {
        return report;
    }

    report.push_str("Attempting to spawn process...\n");
    
    let mut cmd = std::process::Command::new(&settings.bsl_server.java_path);
    cmd.args([
        "-jar",
        &settings.bsl_server.jar_path,
        "--help"
    ]);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // NO_WINDOW
    }

    match cmd.output() {
        Ok(output) => {
            report.push_str(&format!("Exit Status: {}\n", output.status));
            report.push_str(&format!("Stdout: {}\n", String::from_utf8_lossy(&output.stdout)));
            report.push_str(&format!("Stderr: {}\n", String::from_utf8_lossy(&output.stderr)));
        }
        Err(e) => {
            report.push_str(&format!("Failed to execute: {}\n", e));
        }
    }

    report
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

/// Call an MCP tool on a specific server
#[tauri::command]
pub async fn call_mcp_tool(server_id: String, name: String, arguments: serde_json::Value) -> Result<serde_json::Value, String> {
    let settings = load_settings();
    let config = settings.mcp_servers.iter()
        .find(|s| s.id == server_id)
        .cloned()
        .ok_or_else(|| format!("MCP server with ID '{}' not found", server_id))?;

    let client = McpClient::new(config).await?;
    client.call_tool(&name, arguments).await
}

/// Test connection to an MCP server
#[tauri::command]
pub async fn test_mcp_connection(config: McpServerConfig) -> Result<String, String> {
    let client = McpClient::new(config).await?;
    match client.list_tools().await {
        Ok(tools) => Ok(format!("Подключено успешно! Доступно инструментов: {}.", tools.len())),
        Err(e) => Err(format!("Ошибка подключения: {}", e)),
    }
}
