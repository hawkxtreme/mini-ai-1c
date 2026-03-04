use crate::llm::cli_providers::{self as cli, qwen::QwenCliProvider};

#[tauri::command]
pub async fn cli_auth_start(provider: String) -> Result<cli::CliAuthInitResponse, String> {
    crate::app_log!(force: true, "[DEBUG] cli_auth_start called for: {}", provider);
    match provider.as_str() {
        "qwen" => QwenCliProvider::auth_start().await,
        _ => Err(format!("Unsupported provider: {}", provider)),
    }
}

#[tauri::command]
pub async fn cli_auth_poll(provider: String, device_code: String, code_verifier: Option<String>) -> Result<cli::CliAuthStatus, String> {
    crate::app_log!(force: true, "[DEBUG] cli_auth_poll called for: {}, device_code: {}", provider, device_code);
    match provider.as_str() {
        "qwen" => QwenCliProvider::auth_poll(&device_code, code_verifier.as_deref()).await,
        _ => Err(format!("Unsupported provider: {}", provider)),
    }
}

#[tauri::command]
pub async fn cli_save_token(profile_id: String, provider: String, access_token: String, refresh_token: Option<String>, expires_at: u64, resource_url: Option<String>) -> Result<(), String> {
    crate::app_log!(force: true, "[DEBUG] cli_save_token called for profile: {}, provider: {}", profile_id, provider);
    match provider.as_str() {
        "qwen" => QwenCliProvider::save_token(&profile_id, &access_token, refresh_token.as_deref(), expires_at, resource_url.as_deref()),
        _ => Err(format!("Unsupported provider: {}", provider)),
    }
}

#[tauri::command]
pub async fn cli_logout(profile_id: String, provider: String) -> Result<(), String> {
    crate::app_log!(force: true, "[DEBUG] cli_logout called for profile: {}, provider: {}", profile_id, provider);
    match provider.as_str() {
        "qwen" => QwenCliProvider::logout(&profile_id),
        _ => Err(format!("Unsupported provider: {}", provider)),
    }
}

#[tauri::command]
pub async fn cli_get_status(profile_id: String, provider: String) -> Result<cli::CliStatus, String> {
    crate::app_log!(force: true, "[DEBUG] cli_get_status called for profile: {}, provider: {}", profile_id, provider);
    match provider.as_str() {
        "qwen" => QwenCliProvider::get_status(&profile_id).await,
        _ => Err(format!("Unsupported provider: {}", provider)),
    }
}

#[tauri::command]
pub async fn cli_refresh_usage(profile_id: String, provider: String) -> Result<cli::CliUsage, String> {
    crate::app_log!(force: true, "[DEBUG] cli_refresh_usage called for profile: {}, provider: {}", profile_id, provider);
    match provider.as_str() {
        "qwen" => QwenCliProvider::fetch_usage_from_api(&profile_id).await,
        _ => Err(format!("Unsupported provider for refresh: {}", provider)),
    }
}
