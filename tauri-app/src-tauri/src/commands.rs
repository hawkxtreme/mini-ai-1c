//! Tauri commands for IPC with frontend

use serde::{Deserialize, Serialize};

use crate::ai_client::ApiMessage;
use crate::bsl_client::{self, SymbolInfo, Range};
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

/// Stream chat response using AI client with automatic BSL correction
#[tauri::command]
pub async fn stream_chat(
    messages: Vec<ChatMessage>,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, tokio::sync::Mutex<crate::bsl_client::BSLClient>>,
    original_code: Option<String>,
    target_scope: Option<String>,
) -> Result<(), String> {
    use crate::ai_client::{extract_bsl_code, stream_chat_completion, ApiMessage};
    use crate::bsl_client::BSLClient;
    use tauri::Emitter;

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

    let mut current_iteration = 0;
    const MAX_FIX_ATTEMPTS: u32 = 3;
    let mut last_bsl_blocks = Vec::new();


    loop {
        // Stream chat completion
        let full_response = stream_chat_completion(api_messages.clone(), app_handle.clone()).await?;
        
        // Add response to history for potential next round
        api_messages.push(ApiMessage {
            role: "assistant".to_string(),
            content: full_response.clone(),
        });

        // 2. Extract and Validate BSL Code
        last_bsl_blocks = extract_bsl_code(&full_response);
        if last_bsl_blocks.is_empty() {
             // No code to validate, we are done
             break;
        }

        let _ = app_handle.emit("chat-status", "Validating BSL code...");

        // 2.1 Scope Verification
        let mut scope_warnings = Vec::new();
        if let Some(orig) = &original_code {
            for block in &last_bsl_blocks {
                match verify_block_scope(block, orig, &state, target_scope.as_deref()).await {
                    Ok(mut w) => {
                        if !w.is_empty() {
                            println!("[AutoFix] Scope warnings detected: {:?}", w);
                        }
                        scope_warnings.append(&mut w);
                    },
                    Err(e) => println!("[AutoFix] Scope verification failed: {}", e),
                }
            }
        }
        
        // 2.2 Syntax Validation (LSP Diagnostics)
        // 30 second timeout for validation
        let validation_result = tokio::time::timeout(
            tokio::time::Duration::from_secs(30),
            async {
                let mut client = state.lock().await;
                if !client.is_connected() {
                    let _ = client.connect().await;
                }

                let mut all_errors = Vec::new();
                for (idx, code) in last_bsl_blocks.iter().enumerate() {
                    let uri = format!("file:///iteration_{}_{}.bsl", current_iteration, idx);
                    match client.analyze_code(code, &uri).await {
                        Ok(diagnostics) => {
                            let errors: Vec<_> = diagnostics.into_iter()
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

        let mut all_errors = match validation_result {
            Ok(errors) => errors,
            Err(_) => {
                let _ = app_handle.emit("chat-status", "Ошибка проверки кода: Таймаут (30с)");
                let _ = app_handle.emit("chat-chunk", "\n\n> [!WARNING]\n> Проверка кода BSL заняла слишком много времени и была прервана.\n\n".to_string());
                break; // Break the auto-fix loop on timeout
            }
        };

        // Add scope warnings to errors if any
        if !scope_warnings.is_empty() {
            let warnings_text = format!("Нарушение Scope Control:\n{}", scope_warnings.join("\n"));
            all_errors.push(warnings_text);
        }

        // 3. Decide whether to fix or end
        if all_errors.is_empty() || current_iteration >= MAX_FIX_ATTEMPTS {
            break;
        }

        // We have errors and attempts left
        current_iteration += 1;
        let _ = app_handle.emit("chat-status", format!("Исправление ошибок (попытка {}/{})", current_iteration, MAX_FIX_ATTEMPTS));

        let fix_prompt = format!(
            "В твоем коде обнаружены ошибки или нарушения правил:\n{}\n\nПожалуйста, исправь их.\nКРИТИЧЕСКОЕ ПРАВИЛО: НЕ РЕФАКТОРИ СУЩЕСТВУЮЩИЙ КОД! Если ты изменил что-то помимо целевой задачи (например, заменил ПолучитьФорму на ОткрытьФорму), ВЕРНИ КАК БЫЛО. Твой ответ должен содержать ТОЛЬКО запрошенные изменения.",
            all_errors.join("\n")
        );

        api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: fix_prompt,
        });

        // Let the user know we are re-generating
        let _ = app_handle.emit("chat-chunk", "\n\n---\n*Обнаружены ошибки. Исправляю...*\n\n".to_string());
    }

    // FINAL SMART MERGE: Ensure no legacy refactoring was missed
    if let Some(orig) = original_code.as_deref() {
        if !last_bsl_blocks.is_empty() {
            // Merge the last (most corrected) block
            let merged = apply_smart_merge(orig, &last_bsl_blocks[0], target_scope.as_deref(), &state).await;
            
            // If the merged code is different from the original, we send it as the final block
            if merged != orig {
                // Wrap in BSL block for Frontend
                let final_md = format!("\n\n> [!TIP]\n> Изменения автоматически очищены от нецелевого рефакторинга.\n\n```bsl\n{}\n```", merged);
                let _ = app_handle.emit("chat-chunk", final_md);
            }
        }
    }

    let _ = app_handle.emit("chat-status", ""); // Clear status
    let _ = app_handle.emit("chat-done", ());
    Ok(())
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

/// Analyze BSL code structure (symbols)
#[tauri::command]
pub async fn analyze_bsl_structure(
    code: String,
    state: tauri::State<'_, tokio::sync::Mutex<crate::bsl_client::BSLClient>>
) -> Result<Vec<SymbolInfo>, String> {
    let client = state.lock().await;
    
    // Ensure connected
    if !client.is_connected() {
        // We don't want to connect here if it takes too long, 
        // but get_symbols needs a connection.
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let uri = format!("file:///structure_{}.bsl", timestamp);

    client.get_symbols(&code, &uri).await
}

/// Get the active scope (function/procedure) at a certain position
#[tauri::command]
pub async fn get_active_scope(
    code: String,
    line: u32,
    character: u32,
    state: tauri::State<'_, tokio::sync::Mutex<crate::bsl_client::BSLClient>>
) -> Result<Option<SymbolInfo>, String> {
    let client = state.lock().await;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let uri = format!("file:///scope_{}.bsl", timestamp);

    let symbols = client.get_symbols(&code, &uri).await?;
    Ok(find_symbol_at_pos(&symbols, line, character).cloned())
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
    if api_key.is_empty() {
        return Err("API key not found in profile".to_string());
    }

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

// ============== Scope Control Helpers ==============

/// Find the symbol that contains the given position
fn find_symbol_at_pos(symbols: &[SymbolInfo], line: u32, character: u32) -> Option<&SymbolInfo> {
    for sym in symbols {
        // Range check
        if line >= sym.range.start.line && line <= sym.range.end.line {
             // If we are on boundary lines, check character
             if line == sym.range.start.line && character < sym.range.start.character {
                 continue;
             }
             if line == sym.range.end.line && character > sym.range.end.character {
                 continue;
             }
             
             // If it has children, check them first (more specific)
             if let Some(children) = &sym.children {
                 if let Some(child) = find_symbol_at_pos(children, line, character) {
                     return Some(child);
                 }
             }
             
             return Some(sym);
        }
    }
    None
}

/// Extract code within a range
fn extract_range(code: &str, range: &Range, line_ending: &str) -> String {
    let lines: Vec<&str> = code.lines().collect();
    let mut result = Vec::new();
    
    // BSL Specific: Include annotations (&AtClient, etc.) that might be just above the reported range
    let mut start_line = range.start.line as usize;
    while start_line > 0 {
        if let Some(prev_line) = lines.get(start_line - 1) {
            let trimmed = prev_line.trim();
            if trimmed.starts_with('&') || trimmed.starts_with("//") && trimmed.contains("ACTION:") {
                start_line -= 1;
            } else if trimmed.is_empty() && start_line > (range.start.line as usize).saturating_sub(2) {
                // Allow one empty line if it might be part of the header
                start_line -= 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    for i in start_line ..= range.end.line as usize {
        if let Some(line) = lines.get(i) {
            let s_char = if i == start_line { 
                if i < range.start.line as usize { 0 } else { range.start.character as usize }
            } else { 0 };
            
            let e_char = if i == range.end.line as usize { 
                std::cmp::min(range.end.character as usize, line.len()) 
            } else { 
                line.len() 
            };
            
            if s_char < line.len() {
                result.push(&line[s_char..e_char]);
            } else {
                result.push("");
            }
        }
    }
    
    result.join(line_ending)
}

/// Verify if a generated block respects the scope of the original code
async fn verify_block_scope(
    block: &str,
    original_code: &str,
    state: &tauri::State<'_, tokio::sync::Mutex<crate::bsl_client::BSLClient>>,
    target_scope: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    
    let ai_symbols = {
        let client = state.lock().await;
        client.get_symbols(block, "file:///ai_verify.bsl").await?
    };
    
    if ai_symbols.is_empty() {
        return Ok(Vec::new()); 
    }

    let orig_symbols = {
        let client = state.lock().await;
        client.get_symbols(original_code, "file:///orig_verify.bsl").await?
    };
    
    for ai_sym in &ai_symbols {
        let matched = orig_symbols.iter().find(|s| s.name.to_lowercase() == ai_sym.name.to_lowercase());
        
        match matched {
            Some(orig_sym) => {
                // Check if this function is allowed to be modified
                let is_target = target_scope.map(|t| t.to_lowercase() == ai_sym.name.to_lowercase()).unwrap_or(false);
                
                if !is_target {
                    // Extract body from both AI block and Original code
                    let ai_body_extracted = extract_range(block, &ai_sym.range, "\n");
                    let orig_body = extract_range(original_code, &orig_sym.range, "\n");
                    
                    // Normalize for comparison (trim each line)
                    let norm_ai = ai_body_extracted.lines().map(|l| l.trim()).collect::<Vec<_>>().join("\n");
                    let norm_orig = orig_body.lines().map(|l| l.trim()).collect::<Vec<_>>().join("\n");

                    if norm_ai != norm_orig {
                        println!("[ScopeControl] Violation in '{}'.", ai_sym.name);
                        warnings.push(format!("Изменение в нецелевой функции '{}'. Тело функции было изменено без запроса. Верни оригинальный код этой функции.", ai_sym.name));
                    }
                } else {
                    // Check signature consistency for target
                    let ai_lines: Vec<&str> = block.lines().collect();
                    let ai_sig = ai_lines.get(ai_sym.range.start.line as usize).unwrap_or(&"").trim().to_lowercase();
                    
                    let orig_lines: Vec<&str> = original_code.lines().collect();
                    let orig_sig = orig_lines.get(orig_sym.range.start.line as usize).unwrap_or(&"").trim().to_lowercase();
                    
                    let clean_ai = ai_sig.split("//").next().unwrap_or("").trim();
                    let clean_orig = orig_sig.split("//").next().unwrap_or("").trim();

                    if clean_ai != clean_orig && !clean_ai.is_empty() && !clean_orig.is_empty() {
                        // If it's the target, we allow body changes, but sig changes are suspicious
                         println!("[ScopeControl] Sig change in target '{}'.", ai_sym.name);
                    }
                }
            }
            None => {
                // New function added
                if let Some(target) = target_scope {
                    warnings.push(format!("Обнаружена новая функция '{}', хотя фокус был на '{}'. Не добавляй лишнего.", ai_sym.name, target));
                }
                // If no target_scope, we allow new functions (module mode)
            }
        }
    }
    
    Ok(warnings)
}

/// Physically merge AI changes into original code, discarding unsolicited refactorings
async fn apply_smart_merge(
    original: &str,
    ai_block: &str,
    target_scope: Option<&str>,
    state: &tauri::State<'_, tokio::sync::Mutex<crate::bsl_client::BSLClient>>,
) -> String {
    // 0. Extract clean BSL if wrapped in markdown
    let clean_ai = if ai_block.contains("```") {
        crate::ai_client::extract_bsl_code(ai_block).first().cloned().unwrap_or_else(|| ai_block.to_string())
    } else {
        ai_block.to_string()
    };

    let ai_symbols = {
        let client = state.lock().await;
        client.get_symbols(&clean_ai, "file:///ai_merge.bsl").await.unwrap_or_default()
    };
    
    if ai_symbols.is_empty() {
        println!("[SmartMerge] No symbols found in AI block. Returning original to prevent deletion.");
        return original.to_string(); 
    }

    let orig_symbols = {
        let client = state.lock().await;
        client.get_symbols(original, "file:///orig_merge.bsl").await.unwrap_or_default()
    };

    apply_smart_merge_logic(original, &clean_ai, target_scope, &ai_symbols, &orig_symbols)
}

/// Pure logic for merging code snippets
fn apply_smart_merge_logic(
    original: &str,
    clean_ai: &str,
    target_scope: Option<&str>,
    ai_symbols: &[SymbolInfo],
    orig_symbols: &[SymbolInfo],
) -> String {
    // 0. Detect Line Endings of original
    let line_ending = if original.contains("\r\n") { "\r\n" } else { "\n" };
    
    println!("[SmartMerge] START. Target Scope: {:?}, LineEnding: {:?}", target_scope, if line_ending == "\r\n" { "CRLF" } else { "LF" });
    let original_lines: Vec<String> = original.lines().map(|l| l.to_string()).collect();

    // 1. Detect Explicit Actions from AI
    let mut explicit_replace: Option<String> = None;
    let mut explicit_add = false;
    let mut explicit_full = false;

    for line in clean_ai.lines().take(15) { 
        let clean = line.trim();
        if clean.starts_with("// ACTION: REPLACE") {
            explicit_replace = Some(clean.replace("// ACTION: REPLACE", "").trim().to_string());
        } else if clean.starts_with("// ACTION: ADD") {
            explicit_add = true;
        } else if clean.starts_with("// ACTION: FULL_MODULE") {
            explicit_full = true;
        }
    }

    if explicit_full {
        println!("[SmartMerge] AI requested FULL_MODULE update.");
        return clean_ai.to_string();
    }

    // 2. Logic for REPLACE
    let final_target = explicit_replace.clone().or_else(|| target_scope.map(|s| s.to_string()));
    
    if let Some(target) = final_target {
        if let Some(ai_sym) = ai_symbols.iter().find(|s| s.name.to_lowercase() == target.to_lowercase()) {
            if let Some(orig_sym) = orig_symbols.iter().find(|s| s.name.to_lowercase() == target.to_lowercase()) {
                println!("[SmartMerge] Found original symbol: {}. Replacing.", orig_sym.name);
                
                let new_body = extract_range(clean_ai, &ai_sym.range, line_ending);
                
                let mut new_full_code = Vec::new();
                for i in 0..orig_sym.range.start.line as usize {
                    if let Some(line) = original_lines.get(i) {
                        new_full_code.push(line.clone());
                    }
                }
                
                new_full_code.push(new_body);
                
                for i in (orig_sym.range.end.line as usize + 1)..original_lines.len() {
                    if let Some(line) = original_lines.get(i) {
                        new_full_code.push(line.clone());
                    }
                }
                
                let result = new_full_code.join(line_ending);
                // Safety check: if result is too small compared to original, something went wrong
                if result.len() < original.len() / 2 && original.len() > 100 {
                     println!("[SmartMerge] SAFETY. Result too small. Returning original.");
                     return original.to_string();
                }
                return result;
            } else if explicit_replace.is_some() || explicit_add {
                explicit_add = true;
            }
        }
    }
    
    // 3. Logic for ADD (or catch-all additions)
    if explicit_add || target_scope.is_none() {
        println!("[SmartMerge] Processing as ADDITIONS.");
        let mut additions = Vec::new();
        for ai_sym in ai_symbols {
            let exists = orig_symbols.iter().any(|s| s.name.to_lowercase() == ai_sym.name.to_lowercase());
            if !exists {
                println!("[SmartMerge] Found NEW symbol in AI response: {}", ai_sym.name);
                additions.push(extract_range(clean_ai, &ai_sym.range, line_ending));
            }
        }

        if !additions.is_empty() {
            let mut final_code = original.trim_end().to_string();
            // Use original line ending
            final_code.push_str(line_ending);
            final_code.push_str(line_ending);
            final_code.push_str(&additions.join(&format!("{}{}", line_ending, line_ending)));
            return final_code;
        }
    }

    // 4. Final Fallback: full module if snippet is large enough
    if ai_symbols.len() > orig_symbols.len() / 2 && clean_ai.len() > original.len() * 8 / 10 {
         println!("[SmartMerge] Fallback to clean AI block.");
         return clean_ai.to_string();
    }

    println!("[SmartMerge] No changes applied. Returning original.");
    original.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bsl_client::{SymbolInfo, Range, Position};

    fn mock_range(start_line: u32, end_line: u32) -> Range {
        Range {
            start: Position { line: start_line, character: 0 },
            end: Position { line: end_line, character: 100 }, // Dummy character end
        }
    }

    fn mock_symbol(name: &str, start: u32, end: u32) -> SymbolInfo {
        SymbolInfo {
            name: name.to_string(),
            kind: 12,
            range: mock_range(start, end),
            selection_range: mock_range(start, end),
            children: None,
        }
    }

    #[test]
    fn test_line_ending_preservation() {
        let original = "Proc1()\r\nEndProc";
        let ai = "Proc1()\nNewBody\nEndProc";
        let ai_symbols = vec![mock_symbol("Proc1", 0, 2)];
        let orig_symbols = vec![mock_symbol("Proc1", 0, 1)];
        
        let result = apply_smart_merge_logic(original, ai, Some("Proc1"), &ai_symbols, &orig_symbols);
        assert!(result.contains("\r\n"), "Should preserve CRLF");
        assert!(result.contains("NewBody"), "Should apply change");
    }

    #[test]
    fn test_annotation_preservation() {
        let original = "&AtClient\r\nProc1()\r\nEndProc";
        let ai = "Proc1()\r\nNewBody\r\nEndProc";
        // AI range starts at Proc1, missing &AtClient
        let ai_symbols = vec![mock_symbol("Proc1", 0, 2)]; 
        let orig_symbols = vec![mock_symbol("Proc1", 1, 2)]; // Orig starts at line 1
        
        let result = apply_smart_merge_logic(original, ai, Some("Proc1"), &ai_symbols, &orig_symbols);
        assert!(result.contains("&AtClient"), "Should not lose annotation. Result: {:?}", result);
    }

    #[test]
    fn test_safety_abort_on_deletion() {
        let original = "Module\n".repeat(100) + "Процедура Proc1()\nКонецПроцедуры";
        let ai = "Tiny";
        let ai_symbols = vec![mock_symbol("Tiny", 0, 0)];
        let orig_symbols = vec![mock_symbol("Proc1", 100, 101)];
        
        // Target scope should prevent deletion or trigger fallback/abort
        let result = apply_smart_merge_logic(&original, ai, Some("Proc1"), &ai_symbols, &orig_symbols);
        assert_eq!(result, original, "Should abort merge if result is too small. Result len: {}", result.len());
    }
}
