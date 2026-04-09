use crate::{
    llm_profiles::{self, LLMProfile, ProfileStore},
    settings::{self, AppSettings},
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

const SETTINGS_EXPORT_FORMAT_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsTransferStatus {
    Saved,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportSettingsResult {
    pub status: SettingsTransferStatus,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsExportBundle {
    pub format_version: u32,
    pub settings: AppSettings,
    pub llm_profiles: ProfileStore,
}

impl ExportSettingsResult {
    fn saved(path: String) -> Self {
        Self {
            status: SettingsTransferStatus::Saved,
            path: Some(path),
        }
    }

    fn cancelled() -> Self {
        Self {
            status: SettingsTransferStatus::Cancelled,
            path: None,
        }
    }
}

fn sanitize_settings_for_export(mut safe_settings: AppSettings) -> AppSettings {
    settings::clear_runtime_only_settings(&mut safe_settings);

    // Clear sensitive fields from mcp_servers.
    for server in safe_settings.mcp_servers.iter_mut() {
        server.login = None;
        server.password = None;
        server.headers = None;
        server.env = None;
    }

    // Clear provider API keys from global settings.
    for provider in safe_settings.llm.providers.values_mut() {
        provider.api_key = None;
    }

    safe_settings
}

fn sanitize_profiles_for_export(mut safe_profiles: ProfileStore) -> ProfileStore {
    for profile in &mut safe_profiles.profiles {
        profile.api_key_encrypted.clear();
    }

    safe_profiles
}

fn sync_active_profile(settings: &mut AppSettings, profiles: &ProfileStore) {
    if profiles
        .profiles
        .iter()
        .any(|profile| profile.id == profiles.active_profile_id)
    {
        settings.active_llm_profile = profiles.active_profile_id.clone();
    } else if let Some(first_profile) = profiles.profiles.first() {
        settings.active_llm_profile = first_profile.id.clone();
    }
}

fn build_export_bundle() -> SettingsExportBundle {
    let safe_profiles = sanitize_profiles_for_export(llm_profiles::load_profiles());
    let mut safe_settings = sanitize_settings_for_export(settings::load_settings());
    sync_active_profile(&mut safe_settings, &safe_profiles);

    SettingsExportBundle {
        format_version: SETTINGS_EXPORT_FORMAT_VERSION,
        settings: safe_settings,
        llm_profiles: safe_profiles,
    }
}

fn export_settings_json() -> Result<String, String> {
    serde_json::to_string_pretty(&build_export_bundle()).map_err(|e| e.to_string())
}

fn restore_sensitive_settings(mut imported: AppSettings, current: &AppSettings) -> AppSettings {
    for server in imported.mcp_servers.iter_mut() {
        if let Some(current_server) = current.mcp_servers.iter().find(|s| s.id == server.id) {
            server.login = current_server.login.clone();
            server.password = current_server.password.clone();
            server.headers = current_server.headers.clone();
            server.env = current_server.env.clone();
        }
    }

    for (provider_id, provider) in imported.llm.providers.iter_mut() {
        if let Some(current_provider) = current.llm.providers.get(provider_id) {
            provider.api_key = current_provider.api_key.clone();
        } else {
            provider.api_key = None;
        }
    }

    imported
}

fn restore_profile_secrets(
    mut imported_store: ProfileStore,
    current_store: &ProfileStore,
) -> ProfileStore {
    for profile in &mut imported_store.profiles {
        if let Some(current_profile) = current_store
            .profiles
            .iter()
            .find(|existing| existing.id == profile.id)
        {
            profile.api_key_encrypted = current_profile.api_key_encrypted.clone();
        } else {
            profile.api_key_encrypted.clear();
        }
    }

    if imported_store.profiles.is_empty() {
        imported_store.profiles.push(LLMProfile::default_profile());
        imported_store.active_profile_id = imported_store.profiles[0].id.clone();
        return imported_store;
    }

    if !imported_store
        .profiles
        .iter()
        .any(|profile| profile.id == imported_store.active_profile_id)
    {
        imported_store.active_profile_id = imported_store.profiles[0].id.clone();
    }

    imported_store
}

fn parse_imported_settings(json_data: &str) -> Result<(AppSettings, ProfileStore), String> {
    let current_settings = settings::load_settings();
    let current_profiles = llm_profiles::load_profiles();

    if let Ok(mut bundle) = serde_json::from_str::<SettingsExportBundle>(json_data) {
        bundle.settings = restore_sensitive_settings(bundle.settings, &current_settings);
        bundle.llm_profiles = restore_profile_secrets(bundle.llm_profiles, &current_profiles);
        sync_active_profile(&mut bundle.settings, &bundle.llm_profiles);

        return Ok((bundle.settings, bundle.llm_profiles));
    }

    let mut imported: AppSettings =
        serde_json::from_str(json_data).map_err(|e| format!("Ошибка парсинга JSON: {}", e))?;

    imported = restore_sensitive_settings(imported, &current_settings);
    sync_active_profile(&mut imported, &current_profiles);

    Ok((imported, current_profiles))
}

fn read_import_settings_file(file_path: &str) -> Result<String, String> {
    fs::read_to_string(file_path)
        .map_err(|e| format!("Не удалось прочитать файл настроек '{}': {}", file_path, e))
}

fn default_chat_export_file_name() -> String {
    format!("chat-{}.md", Local::now().format("%Y-%m-%d-%H%M"))
}

fn sanitize_chat_export_stem(raw_stem: &str) -> String {
    let mut sanitized = String::with_capacity(raw_stem.len());
    let mut previous_was_space = false;

    for ch in raw_stem.chars() {
        let is_invalid = ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*');
        if is_invalid || ch.is_whitespace() {
            if !previous_was_space && !sanitized.is_empty() {
                sanitized.push(' ');
            }
            previous_was_space = true;
            continue;
        }

        sanitized.push(ch);
        previous_was_space = false;
    }

    let trimmed = sanitized.trim_matches(|ch: char| ch == ' ' || ch == '.');
    let truncated = trimmed.chars().take(80).collect::<String>();
    truncated.trim_matches(|ch: char| ch == ' ' || ch == '.').to_string()
}

fn build_chat_export_file_name(suggested_file_name: Option<String>) -> String {
    let fallback_name = default_chat_export_file_name();
    let Some(raw_name) = suggested_file_name else {
        return fallback_name;
    };

    let trimmed = raw_name.trim();
    if trimmed.is_empty() {
        return fallback_name;
    }

    let (raw_stem, extension) = match trimmed.rsplit_once('.') {
        Some((stem, ext)) => {
            let normalized_extension = ext.trim().to_ascii_lowercase();
            if matches!(normalized_extension.as_str(), "md" | "txt") {
                if stem.trim().is_empty() {
                    return fallback_name;
                }
                (stem, normalized_extension)
            } else {
                (trimmed, "md".to_string())
            }
        }
        _ => (trimmed, "md".to_string()),
    };

    let sanitized_stem = sanitize_chat_export_stem(raw_stem);
    if sanitized_stem.is_empty() {
        return fallback_name;
    }

    format!("{}.{}", sanitized_stem, extension)
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
    settings::save_settings(&new_settings)?;

    #[cfg(windows)]
    {
        crate::configurator::set_rdp_mode(new_settings.configurator.rdp_mode);
        crate::mouse_hook::set_editor_bridge_enabled(
            new_settings.configurator.editor_bridge_enabled,
        );
    }

    Ok(())
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
pub fn restart_app_cmd(app_handle: AppHandle) {
    app_handle.restart();
}

/// Check if Node.js is installed and return its version string, or None if not found
#[tauri::command]
pub fn check_node_version_cmd() -> Option<String> {
    use std::process::Command;

    #[cfg(target_os = "windows")]
    let output = Command::new("cmd").args(["/C", "node --version"]).output();

    #[cfg(not(target_os = "windows"))]
    let output = Command::new("node").arg("--version").output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8(o.stdout)
            .ok()
            .map(|s| s.trim().to_string()),
        _ => None,
    }
}

/// Export settings to a user-selected JSON file without sensitive data.
#[tauri::command]
pub fn export_settings(app_handle: AppHandle) -> Result<ExportSettingsResult, String> {
    let json_data = export_settings_json()?;
    let file_name = format!("mini-ai-1c-config-{}.json", Local::now().format("%Y%m%d"));

    let Some(file_path) = app_handle
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name(&file_name)
        .blocking_save_file()
    else {
        return Ok(ExportSettingsResult::cancelled());
    };

    let path = file_path
        .into_path()
        .map_err(|e| format!("Не удалось определить путь сохранения: {}", e))?;

    fs::write(&path, json_data)
        .map_err(|e| format!("Не удалось сохранить экспорт настроек: {}", e))?;

    Ok(ExportSettingsResult::saved(path.display().to_string()))
}

/// Export chat dialog to a user-selected Markdown file.
#[tauri::command]
pub fn export_chat(
    app_handle: AppHandle,
    content: String,
    suggested_file_name: Option<String>,
) -> Result<ExportSettingsResult, String> {
    let file_name = build_chat_export_file_name(suggested_file_name);

    let Some(file_path) = app_handle
        .dialog()
        .file()
        .add_filter("Markdown", &["md"])
        .add_filter("Text", &["txt"])
        .set_file_name(&file_name)
        .blocking_save_file()
    else {
        return Ok(ExportSettingsResult::cancelled());
    };

    let path = file_path
        .into_path()
        .map_err(|e| format!("Не удалось определить путь сохранения: {}", e))?;

    fs::write(&path, content)
        .map_err(|e| format!("Не удалось сохранить диалог: {}", e))?;

    Ok(ExportSettingsResult::saved(path.display().to_string()))
}

/// Import settings from JSON string, preserving credentials from current settings
#[tauri::command]
pub fn import_settings(json_data: String) -> Result<(), String> {
    let (imported_settings, imported_profiles) = parse_imported_settings(&json_data)?;
    settings::save_settings(&imported_settings)?;
    llm_profiles::save_profiles(&imported_profiles)
}

/// Validate a settings file before import.
#[tauri::command]
pub fn validate_import_settings_file(file_path: String) -> Result<(), String> {
    let json_data = read_import_settings_file(&file_path)?;
    let _ = parse_imported_settings(&json_data)?;
    Ok(())
}

/// Import settings from a user-selected file.
#[tauri::command]
pub fn import_settings_from_file(file_path: String) -> Result<(), String> {
    let json_data = read_import_settings_file(&file_path)?;
    let (imported_settings, imported_profiles) = parse_imported_settings(&json_data)?;
    settings::save_settings(&imported_settings)?;
    llm_profiles::save_profiles(&imported_profiles)
}

/// Check if Java is installed and available in PATH
#[tauri::command]
pub fn check_java_cmd() -> bool {
    use std::process::Command;

    // Try verification by running java -version
    #[cfg(target_os = "windows")]
    let output = Command::new("cmd").args(["/C", "java -version"]).output();

    #[cfg(not(target_os = "windows"))]
    let output = Command::new("java").arg("-version").output();

    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_chat_export_file_name, build_export_bundle, restore_profile_secrets,
        restore_sensitive_settings, sanitize_profiles_for_export, sanitize_settings_for_export,
        SettingsExportBundle, SETTINGS_EXPORT_FORMAT_VERSION,
    };
    use crate::llm_profiles::{LLMProfile, LLMProvider, ProfileStore};
    use crate::settings::{
        AppSettings, McpServerConfig, McpTransport, ModelSettings, ProviderSettings,
    };
    use std::collections::HashMap;

    fn settings_with_sensitive_data() -> AppSettings {
        let mut provider_models = HashMap::new();
        provider_models.insert(
            "gpt-test".to_string(),
            ModelSettings {
                context_window: Some(128_000),
                cost_in: Some(1.0),
                cost_out: Some(2.0),
            },
        );

        let mut providers = HashMap::new();
        providers.insert(
            "openai".to_string(),
            ProviderSettings {
                enabled: true,
                api_key: Some("super-secret-key".to_string()),
                base_url: Some("https://api.example.com".to_string()),
                active_model_id: Some("gpt-test".to_string()),
                models: provider_models,
            },
        );

        AppSettings {
            active_llm_profile: "profile-123".to_string(),
            llm: crate::settings::LLMGlobalSettings {
                active_provider_id: "openai".to_string(),
                providers,
            },
            mcp_servers: vec![McpServerConfig {
                id: "mcp-1".to_string(),
                name: "Protected MCP".to_string(),
                enabled: true,
                transport: McpTransport::Http,
                url: Some("http://localhost:3000/mcp".to_string()),
                login: Some("admin".to_string()),
                password: Some("hunter2".to_string()),
                headers: Some(HashMap::from([(
                    "Authorization".to_string(),
                    "Bearer test".to_string(),
                )])),
                command: None,
                args: None,
                env: Some(HashMap::from([("TOKEN".to_string(), "secret".to_string())])),
            }],
            ..AppSettings::default()
        }
    }

    fn profile_store_with_sensitive_data() -> ProfileStore {
        ProfileStore {
            profiles: vec![
                LLMProfile {
                    id: "profile-1".to_string(),
                    name: "Profile One".to_string(),
                    provider: LLMProvider::OpenAI,
                    model: "gpt-5".to_string(),
                    api_key_encrypted: "encrypted-secret".to_string(),
                    base_url: Some("https://api.example.com/v1".to_string()),
                    max_tokens: 8000,
                    temperature: 0.2,
                    context_window_override: Some(128_000),
                    reasoning_effort: None,
                    enable_thinking: Some(true),
                    disable_streaming: Some(false),
                    stream_timeout_secs: Some(60),
                    context_compress_strategy: "summarize".to_string(),
                    max_context_messages: Some(50),
                },
                LLMProfile {
                    id: "profile-2".to_string(),
                    name: "Profile Two".to_string(),
                    provider: LLMProvider::QwenCli,
                    model: "qwen3-coder".to_string(),
                    api_key_encrypted: "encrypted-qwen-secret".to_string(),
                    base_url: Some("https://chat.qwen.ai/api/v1".to_string()),
                    max_tokens: 4096,
                    temperature: 0.1,
                    context_window_override: None,
                    reasoning_effort: None,
                    enable_thinking: Some(false),
                    disable_streaming: Some(true),
                    stream_timeout_secs: Some(30),
                    context_compress_strategy: "disabled".to_string(),
                    max_context_messages: None,
                },
            ],
            active_profile_id: "profile-2".to_string(),
        }
    }

    #[test]
    fn export_sanitizer_removes_sensitive_data() {
        let sanitized = sanitize_settings_for_export(settings_with_sensitive_data());
        let server = &sanitized.mcp_servers[0];
        let provider = sanitized.llm.providers.get("openai").unwrap();

        assert_eq!(sanitized.active_llm_profile, "profile-123");
        assert_eq!(server.login, None);
        assert_eq!(server.password, None);
        assert_eq!(server.headers, None);
        assert_eq!(server.env, None);
        assert_eq!(provider.api_key, None);
    }

    #[test]
    fn export_sanitizer_removes_transient_configurator_binding() {
        let mut settings = settings_with_sensitive_data();
        settings.configurator.selected_window_hwnd = Some(265280);
        settings.configurator.selected_window_pid = Some(12892);
        settings.configurator.selected_window_title = Some(
            "Общий модуль ГлобальныйПоискСервер: Модуль - Конфигуратор - Демонстрационное приложение"
                .to_string(),
        );
        settings.configurator.selected_config_name =
            Some("Демонстрационное приложение".to_string());

        let sanitized = sanitize_settings_for_export(settings);

        assert_eq!(sanitized.configurator.selected_window_hwnd, None);
        assert_eq!(sanitized.configurator.selected_window_pid, None);
        assert_eq!(sanitized.configurator.selected_window_title, None);
        assert_eq!(sanitized.configurator.selected_config_name, None);
    }

    #[test]
    fn profile_export_sanitizer_removes_only_profile_secrets() {
        let sanitized = sanitize_profiles_for_export(profile_store_with_sensitive_data());
        let profile = &sanitized.profiles[0];
        let active_profile = &sanitized.profiles[1];

        assert_eq!(profile.api_key_encrypted, "");
        assert_eq!(profile.name, "Profile One");
        assert_eq!(profile.model, "gpt-5");
        assert_eq!(
            profile.base_url.as_deref(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(profile.temperature, 0.2);
        assert_eq!(profile.context_window_override, Some(128_000));
        assert_eq!(active_profile.id, "profile-2");
        assert_eq!(sanitized.active_profile_id, "profile-2");
    }

    #[test]
    fn restore_sensitive_settings_preserves_current_secrets() {
        let mut current = AppSettings::default();
        current.active_llm_profile = "existing-profile".to_string();
        current.mcp_servers = vec![McpServerConfig {
            id: "mcp-1".to_string(),
            name: "Protected MCP".to_string(),
            enabled: true,
            transport: McpTransport::Http,
            url: Some("http://localhost:3000/mcp".to_string()),
            login: Some("current-login".to_string()),
            password: Some("current-password".to_string()),
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer current".to_string(),
            )])),
            command: None,
            args: None,
            env: Some(HashMap::from([(
                "TOKEN".to_string(),
                "current-secret".to_string(),
            )])),
        }];
        current.llm.providers.insert(
            "openai".to_string(),
            ProviderSettings {
                enabled: true,
                api_key: Some("current-api-key".to_string()),
                base_url: Some("https://api.example.com".to_string()),
                active_model_id: Some("gpt-test".to_string()),
                models: HashMap::new(),
            },
        );

        let imported = restore_sensitive_settings(
            sanitize_settings_for_export(settings_with_sensitive_data()),
            &current,
        );
        let server = &imported.mcp_servers[0];
        let provider = imported.llm.providers.get("openai").unwrap();

        assert_eq!(imported.active_llm_profile, "profile-123");
        assert_eq!(server.login.as_deref(), Some("current-login"));
        assert_eq!(server.password.as_deref(), Some("current-password"));
        assert_eq!(
            server
                .headers
                .as_ref()
                .and_then(|headers| headers.get("Authorization")),
            Some(&"Bearer current".to_string())
        );
        assert_eq!(
            server.env.as_ref().and_then(|env| env.get("TOKEN")),
            Some(&"current-secret".to_string())
        );
        assert_eq!(provider.api_key.as_deref(), Some("current-api-key"));
    }

    #[test]
    fn restore_profile_secrets_preserves_existing_profile_keys() {
        let imported_store = sanitize_profiles_for_export(profile_store_with_sensitive_data());
        let current_store = ProfileStore {
            profiles: vec![LLMProfile {
                api_key_encrypted: "current-profile-key".to_string(),
                ..profile_store_with_sensitive_data().profiles[0].clone()
            }],
            active_profile_id: "profile-1".to_string(),
        };

        let restored = restore_profile_secrets(imported_store, &current_store);
        let restored_profile = restored
            .profiles
            .iter()
            .find(|profile| profile.id == "profile-1")
            .unwrap();
        let untouched_profile = restored
            .profiles
            .iter()
            .find(|profile| profile.id == "profile-2")
            .unwrap();

        assert_eq!(restored_profile.api_key_encrypted, "current-profile-key");
        assert_eq!(untouched_profile.api_key_encrypted, "");
        assert_eq!(restored.active_profile_id, "profile-2");
    }

    #[test]
    fn export_bundle_contains_settings_and_profiles() {
        let bundle = build_export_bundle();

        assert_eq!(bundle.format_version, SETTINGS_EXPORT_FORMAT_VERSION);
        assert!(!bundle.llm_profiles.profiles.is_empty());
    }

    #[test]
    fn export_bundle_serializes_with_llm_profiles_section() {
        let mut settings = settings_with_sensitive_data();
        settings.configurator.selected_window_hwnd = Some(265280);
        settings.configurator.selected_window_pid = Some(12892);
        settings.configurator.selected_window_title = Some(
            "Общий модуль ГлобальныйПоискСервер: Модуль - Конфигуратор - Демонстрационное приложение"
                .to_string(),
        );
        settings.configurator.selected_config_name =
            Some("Демонстрационное приложение".to_string());

        let bundle = SettingsExportBundle {
            format_version: SETTINGS_EXPORT_FORMAT_VERSION,
            settings: sanitize_settings_for_export(settings),
            llm_profiles: sanitize_profiles_for_export(profile_store_with_sensitive_data()),
        };

        let json = serde_json::to_string_pretty(&bundle).unwrap();

        assert!(json.contains("\"format_version\""));
        assert!(json.contains("\"settings\""));
        assert!(json.contains("\"llm_profiles\""));
        assert!(json.contains("\"profiles\""));
        assert!(!json.contains("encrypted-secret"));
        assert!(json.contains("\"Profile One\""));
        assert!(json.contains("\"gpt-5\""));
        assert!(!json.contains("selected_window_hwnd"));
        assert!(!json.contains("selected_window_pid"));
        assert!(!json.contains("selected_window_title"));
        assert!(!json.contains("selected_config_name"));
        assert!(!json.contains("window_title_pattern"));
    }

    #[test]
    fn chat_export_file_name_uses_suggested_name_and_sanitizes_it() {
        let file_name =
            build_chat_export_file_name(Some("  тест: связи / demo? 2026-04-09 12-30.md  ".to_string()));

        assert_eq!(file_name, "тест связи demo 2026-04-09 12-30.md");
    }

    #[test]
    fn chat_export_file_name_adds_md_extension_when_missing() {
        let file_name = build_chat_export_file_name(Some("Отчет по чату".to_string()));

        assert_eq!(file_name, "Отчет по чату.md");
    }

    #[test]
    fn chat_export_file_name_falls_back_to_default_for_empty_input() {
        let file_name = build_chat_export_file_name(Some("   .md   ".to_string()));

        assert!(file_name.starts_with("chat-"));
        assert!(file_name.ends_with(".md"));
    }
}
