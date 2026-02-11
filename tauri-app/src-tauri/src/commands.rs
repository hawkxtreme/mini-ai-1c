//! Tauri commands for IPC with frontend

use serde::{Deserialize, Serialize};

use crate::ai_client::{stream_chat_completion, ApiMessage};
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

/// Stream chat response using AI client
#[tauri::command]
pub async fn stream_chat(
    messages: Vec<ChatMessage>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Convert to API messages
    let api_messages: Vec<ApiMessage> = messages
        .into_iter()
        .map(|m| ApiMessage {
            role: m.role,
            content: m.content,
        })
        .collect();

    // Stream chat completion
    stream_chat_completion(api_messages, app_handle).await
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
