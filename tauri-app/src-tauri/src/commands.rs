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

/// Stream chat response using AI client with automatic BSL correction
#[tauri::command]
pub async fn stream_chat(
    messages: Vec<ChatMessage>,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, tokio::sync::Mutex<crate::bsl_client::BSLClient>>,
    chat_state: tauri::State<'_, ChatState>,
) -> Result<(), String> {
    use crate::ai_client::{extract_bsl_code, stream_chat_completion, ApiMessage};
    use crate::bsl_client::BSLClient;
    use tauri::{Emitter, Manager};

    // 1. Initial status
    let _ = app_handle.emit("chat-status", "Thinking...");

    // Convert to API messages
    let mut api_messages: Vec<ApiMessage> = messages
        .into_iter()
        .map(|m| ApiMessage {
            role: m.role,
            content: m.content,
        })
        .collect();

    // Spawn the work into a cancellable task
    let task_app_handle = app_handle.clone();
    
    let join_handle = tokio::spawn(async move {
        // 1. Initial status
        let _ = task_app_handle.emit("chat-status", "Thinking...");
        
        // We need to access BSL Client state inside the task
        // We can get it from app_handle
        let bsl_state = task_app_handle.state::<tokio::sync::Mutex<crate::bsl_client::BSLClient>>();

        let mut current_iteration = 0;
        const MAX_FIX_ATTEMPTS: u32 = 2;

        loop {
            // Stream chat completion
            let full_response = stream_chat_completion(api_messages.clone(), task_app_handle.clone()).await;
            
            // Handle cancellation or error from stream_chat_completion
            let full_response = match full_response {
                Ok(r) => r,
                Err(e) => {
                    let _ = task_app_handle.emit("chat-chunk", format!("\nError: {}", e));
                    return Err(e);
                }
            };
            
            // Add response to history for potential next round
            api_messages.push(ApiMessage {
                role: "assistant".to_string(),
                content: full_response.clone(),
            });

            // 2. Extract and Validate BSL Code
            let bsl_blocks = extract_bsl_code(&full_response);
            if bsl_blocks.is_empty() {
                 // No code to validate, we are done
                 break;
            }

            let _ = task_app_handle.emit("chat-status", "Validating BSL code...");
            
            // 30 second timeout for validation
            let validation_result = tokio::time::timeout(
                tokio::time::Duration::from_secs(30),
                async {
                    // Explicit type annotation to fix inference error and ensure correct locking
                    let mut client: tokio::sync::MutexGuard<crate::bsl_client::BSLClient> = bsl_state.lock().await;
                    if !client.is_connected() {
                        let _ = client.connect().await;
                    }

                    let mut all_errors: Vec<String> = Vec::new();
                    for (idx, code) in bsl_blocks.iter().enumerate() {
                        let uri = format!("file:///iteration_{}_{}.bsl", current_iteration, idx);
                        match client.analyze_code(code, &uri).await {
                            Ok(diagnostics) => {
                                // Explicit type annotation
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
                            Err(e) => {
                                println!("[AutoFix] Check failed for block {}: {}", idx + 1, e);
                            }
                        }
                    }
                    all_errors
                }
            ).await;

            let all_errors = match validation_result {
                Ok(errors) => errors,
                Err(_) => {
                    let _ = task_app_handle.emit("chat-status", "Ошибка проверки кода: Таймаут (30с)");
                    let _ = task_app_handle.emit("chat-chunk", "\n\n> [!WARNING]\n> Проверка кода BSL заняла слишком много времени и была прервана.\n\n".to_string());
                    break; // Break the auto-fix loop on timeout
                }
            };

            // 3. Decide whether to fix or end
            if all_errors.is_empty() || current_iteration >= MAX_FIX_ATTEMPTS {
                break;
            }

            // We have errors and attempts left
            current_iteration += 1;
            let _ = task_app_handle.emit("chat-status", format!("Fixing errors (Attempt {}/{})...", current_iteration, MAX_FIX_ATTEMPTS));

            let fix_prompt = format!(
                "В сгенерированном коде обнаружены ошибки:\n\n{}\n\nПожалуйста, исправь эти ошибки и предоставь корректный код.",
                all_errors.join("\n\n")
            );

            api_messages.push(ApiMessage {
                role: "user".to_string(),
                content: fix_prompt,
            });

            // Let the user know we are re-generating
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

/// Paste code to 1C Configurator window
#[tauri::command]
pub fn paste_code_to_configurator(hwnd: isize, code: String, use_select_all: Option<bool>) -> Result<(), String> {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::paste_code(hwnd, &code, use_select_all.unwrap_or(false))
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
