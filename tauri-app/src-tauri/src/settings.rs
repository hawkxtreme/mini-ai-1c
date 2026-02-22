//! Settings management module for Mini AI 1C Agent
//! Persists application settings to JSON file

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// Helper functions for defaults
fn default_true() -> bool { true }

fn default_change_marker() -> String {
    "// [ИЗМЕНЕНО AI] - {date}".to_string()
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    Http,
    Stdio,
    Internal,
}

/// Configuration for an MCP server (HTTP or Stdio)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub transport: McpTransport,
    // HTTP specific
    pub url: Option<String>,
    pub login: Option<String>,
    pub password: Option<String>,
    // Stdio specific
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "New MCP Server".to_string(),
            enabled: false,
            transport: McpTransport::Http,
            url: Some("http://localhost/mcp".to_string()),
            login: None,
            password: None,
            command: None,
            args: None,
            env: None,
        }
    }
}


/// Main application settings container
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub configurator: ConfiguratorSettings,
    pub bsl_server: BSLServerSettings,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    pub active_llm_profile: String,
    pub llm: LLMGlobalSettings,
    #[serde(default)]
    pub debug_mcp: bool,
    #[serde(default)]
    pub onboarding_completed: bool,
    /// Настройки пользовательских промптов
    #[serde(default)]
    pub custom_prompts: CustomPromptsSettings,
    /// Настройки генерации кода
    #[serde(default)]
    pub code_generation: CodeGenerationSettings,
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

/// Режим генерации кода
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CodeGenerationMode {
    /// Всегда полный код
    Full,
    /// Только изменения в формате Search/Replace
    Diff,
    /// Автовыбор по размеру модуля
    Auto,
}

impl Default for CodeGenerationMode {
    fn default() -> Self {
        Self::Diff
    }
}

/// Настройки генерации кода
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenerationSettings {
    /// Режим генерации
    #[serde(default)]
    pub mode: CodeGenerationMode,
    
    /// Сохранять copyright-комментарии
    #[serde(default = "default_true")]
    pub preserve_copyright: bool,
    
    /// Маркировать изменения
    #[serde(default = "default_true")]
    pub mark_changes: bool,
    
    /// Шаблон маркера изменения
    #[serde(default = "default_change_marker")]
    pub change_marker_template: String,
}

impl Default for CodeGenerationSettings {
    fn default() -> Self {
        Self {
            mode: CodeGenerationMode::Full,
            preserve_copyright: true,
            mark_changes: true,
            change_marker_template: default_change_marker(),
        }
    }
}

/// Настройки маркеров изменений
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeMarkersSettings {
    /// Добавлять маркер изменения
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Шаблон маркера (поддерживает {date}, {reason}, {author})
    #[serde(default = "default_change_marker")]
    pub template: String,
}

impl Default for ChangeMarkersSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            template: default_change_marker(),
        }
    }
}

/// Шаблон промпта
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub content: String,
    #[serde(default)]
    pub enabled: bool,
}

/// Настройки пользовательских промптов
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPromptsSettings {
    /// Префикс, добавляемый к system prompt
    #[serde(default)]
    pub system_prefix: String,
    
    /// Инструкции при изменении кода
    #[serde(default)]
    pub on_code_change: String,
    
    /// Инструкции при генерации нового кода
    #[serde(default)]
    pub on_code_generate: String,
    
    /// Шаблоны комментариев для изменений
    #[serde(default)]
    pub change_markers: ChangeMarkersSettings,
    
    /// Пользовательские шаблоны промптов
    #[serde(default)]
    pub templates: Vec<PromptTemplate>,
}

impl Default for CustomPromptsSettings {
    fn default() -> Self {
        Self {
            system_prefix: String::new(),
            on_code_change: String::new(),
            on_code_generate: String::new(),
            change_markers: ChangeMarkersSettings::default(),
            templates: vec![
                PromptTemplate {
                    id: "bsl-standards".to_string(),
                    name: "Стандарты 1С".to_string(),
                    description: "Соблюдать стандарты разработки 1С и БСП".to_string(),
                    content: "Соблюдай стандарты разработки 1С и Библиотеки Стандартных Подсистем (БСП).".to_string(),
                    enabled: false,
                },
                PromptTemplate {
                    id: "wrap-changes".to_string(),
                    name: "Оборачивать изменения".to_string(),
                    description: "Оборачивать изменения в комментарии доработки".to_string(),
                    content: r#"Все изменения оборачивай в комментарии:
// Доработка START
// Дата: {date}
<измененный код>
// Доработка END"#.to_string(),
                    enabled: false,
                },
            ],
        }
    }
}

/// Get the settings directory path
pub fn get_settings_dir() -> PathBuf {
    // Use data_local_dir instead of config_dir to avoid UNC paths on terminal servers
    // data_local_dir points to %LOCALAPPDATA% which is always local, not roaming
    let config_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    config_dir.join("MiniAI1C")
}

/// Get the settings file path
pub fn get_settings_file() -> PathBuf {
    get_settings_dir().join("settings.json")
}

/// Load settings from file
pub fn load_settings() -> AppSettings {
    let path = get_settings_file();
    let mut settings = if path.exists() {
        match fs::read_to_string(&path) {
            Ok(content) => {
                serde_json::from_str(&content).unwrap_or_default()
            }
            Err(_) => AppSettings::default(),
        }
    } else {
        AppSettings::default()
    };

    // Migration: Force high-performance node launcher for built-in MCP servers
    let mut modified = false;
    for server in settings.mcp_servers.iter_mut() {
        if server.id == "builtin-1c-naparnik" || server.id == "builtin-1c-metadata" {
            let current_cmd = server.command.as_deref().unwrap_or("");
            if current_cmd != "node" {
                crate::app_log!("[SETTINGS] Migrating builtin server '{}' from '{}' to 'node' launcher", server.id, current_cmd);
                server.command = Some("node".to_string());
                modified = true;
            }
            
            if let Some(args) = &mut server.args {
                let original_args = args.clone();
                // Filter out tsx/npx specific artifacts
                args.retain(|a| a != "tsx" && a != "--yes" && !a.contains("node_modules"));
                
                for arg in args.iter_mut() {
                    // Fix paths: we want 'mcp-servers/name.cjs' relative to src-tauri
                    if arg.contains("mcp-servers") {
                        *arg = arg.replace("src-tauri/", "").replace("src/mcp-servers/", "mcp-servers/");
                    }
                    if arg.ends_with(".ts") || arg.ends_with(".js") {
                        *arg = arg.replace(".ts", ".cjs").replace(".js", ".cjs");
                    }
                }
                if args != &original_args {
                    crate::app_log!("[SETTINGS] Migrated builtin server '{}' args to: {:?}", server.id, args);
                    modified = true; 
                }
            }
        } else {
            // Generic migration for other servers if they have node_modules in command
            if let Some(cmd) = &server.command {
                if cmd.contains("node_modules") {
                    crate::app_log!("[DEBUG] Migrating stale command '{}' to 'npx' for MCP server '{}'", cmd, server.id);
                    server.command = Some("npx".to_string());
                    modified = true;
                }
            }
        }
    }

    // Migration: Force 'Diff' mode over 'Full' if detected (to fix AI interaction issues)
    if settings.code_generation.mode == CodeGenerationMode::Full {
        crate::app_log!("[SETTINGS] Migrating deprecated 'Full' mode to 'Diff'");
        settings.code_generation.mode = CodeGenerationMode::Diff;
        modified = true;
    }

    if modified {
        let _ = save_settings(&settings);
    }

    settings
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
