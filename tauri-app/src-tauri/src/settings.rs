//! Settings management module for Mini AI 1C Agent
//! Persists application settings to JSON file

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Settings for 1C Configurator integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfiguratorSettings {
    pub window_title_pattern: String,
    pub selected_window_hwnd: Option<isize>,
}

impl Default for ConfiguratorSettings {
    fn default() -> Self {
        Self {
            window_title_pattern: "Конфигуратор".to_string(),
            selected_window_hwnd: None,
        }
    }
}

/// Settings for BSL Language Server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BSLServerSettings {
    pub jar_path: String,
    pub auto_download: bool,
    pub websocket_port: u16,
    pub java_path: String,
    pub enabled: bool,
}

impl Default for BSLServerSettings {
    fn default() -> Self {
        Self {
            jar_path: String::new(),
            auto_download: true,
            websocket_port: 8025,
            java_path: "java".to_string(),
            enabled: true,
        }
    }
}

/// UI-related settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UISettings {
    pub theme: String,
    pub minimize_to_tray: bool,
    pub start_minimized: bool,
    pub window_width: u32,
    pub window_height: u32,
    pub window_x: i32,
    pub window_y: i32,
}

impl Default for UISettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            minimize_to_tray: true,
            start_minimized: false,
            window_width: 700,
            window_height: 800,
            window_x: 100,
            window_y: 100,
        }
    }
}

/// Main application settings container
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub configurator: ConfiguratorSettings,
    pub bsl_server: BSLServerSettings,
    pub ui: UISettings,
    pub active_llm_profile: String,
    #[serde(default)]
    pub llm: LLMGlobalSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LLMGlobalSettings {
    pub active_provider_id: String,
    pub providers: std::collections::HashMap<String, ProviderSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSettings {
    pub enabled: bool,
    pub api_key: Option<String>, // TODO: Encrypt this
    pub base_url: Option<String>,
    pub active_model_id: Option<String>,
    pub models: std::collections::HashMap<String, ModelSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSettings {
    pub context_window: Option<u32>, // Override
    pub cost_in: Option<f64>,
    pub cost_out: Option<f64>,
}

/// Get the settings directory path
pub fn get_settings_dir() -> PathBuf {
    let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config_dir.join("MiniAI1C")
}

/// Get the settings file path
pub fn get_settings_file() -> PathBuf {
    get_settings_dir().join("settings.json")
}

/// Load settings from file
pub fn load_settings() -> AppSettings {
    let path = get_settings_file();
    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(content) => {
                serde_json::from_str(&content).unwrap_or_default()
            }
            Err(_) => AppSettings::default(),
        }
    } else {
        AppSettings::default()
    }
}

/// Save settings to file
pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let dir = get_settings_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    
    let path = get_settings_file();
    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| e.to_string())?;
    
    fs::write(path, content).map_err(|e| e.to_string())
}
