//! Settings management module for Mini AI 1C Agent
//! Persists application settings to JSON file

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

// Helper functions for defaults
fn default_true() -> bool { true }

fn default_addition_marker() -> String {
    "// Доработка START (Добавление) - {datetime}\n{newCode}\n// Доработка END".to_string()
}

fn default_modification_marker() -> String {
    "// Доработка START (Изменение) - {datetime}\n{newCode}\n// Доработка END".to_string()
}

fn default_deletion_marker() -> String {
    "// Доработка (Удаление) - {datetime}\n// {oldCode}".to_string()
}

fn default_max_iterations() -> Option<u32> {
    Some(7)
}

/// Быстрые команды (Slash Commands)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommand {
    pub id: String,
    pub command: String,
    pub name: String,
    pub description: String,
    pub template: String,
    pub is_enabled: bool,
    pub is_system: bool,
}

fn default_slash_commands() -> Vec<SlashCommand> {
    vec![
        SlashCommand {
            id: "fix".to_string(),
            command: "исправить".to_string(),
            name: "Исправить".to_string(),
            description: "Исправить ошибки BSL и логические ошибки".to_string(),
            template: "Исправь ошибки в этом коде. Обрати внимание на следующие диагностики:\n{diagnostics}\n\nКод для исправления:\n```bsl\n{code}\n```".to_string(),
            is_enabled: true,
            is_system: true,
        },
        SlashCommand {
            id: "refactor".to_string(),
            command: "рефакторинг".to_string(),
            name: "Рефакторинг".to_string(),
            description: "Улучшить структуру и читаемость кода".to_string(),
            template: "Проведи рефакторинг этого кода, улучши его структуру и читаемость, соблюдая стандарты 1С:\n```bsl\n{code}\n```".to_string(),
            is_enabled: true,
            is_system: true,
        },
        SlashCommand {
            id: "desc".to_string(),
            command: "описание".to_string(),
            name: "Описание".to_string(),
            description: "Сгенерировать описание процедуры/функции".to_string(),
            template: "Сгенерируй стандартную шапку описания для этой процедуры/функции в формате 1С (только комментарии //, без тегов <Описание>):\n```bsl\n{code}\n```".to_string(),
            is_enabled: true,
            is_system: true,
        },
        SlashCommand {
            id: "explain".to_string(),
            command: "объясни".to_string(),
            name: "Объясни".to_string(),
            description: "Подробно объяснить работу кода".to_string(),
            template: "Подробно объясни, как работает этот фрагмент кода:\n```bsl\n{code}\n```".to_string(),
            is_enabled: true,
            is_system: true,
        },
        SlashCommand {
            id: "review".to_string(),
            command: "ревью".to_string(),
            name: "Ревью".to_string(),
            description: "Провести код-ревью".to_string(),
            template: "Проведи подробное код-ревью этого фрагмента. Найди потенциальные баги, узкие места и предложи улучшения:\n```bsl\n{code}\n```".to_string(),
            is_enabled: true,
            is_system: true,
        },
        SlashCommand {
            id: "standards".to_string(),
            command: "стандарты".to_string(),
            name: "Стандарты".to_string(),
            description: "Проверить на соответствие стандартам 1С".to_string(),
            template: "Проверь этот код на соответствие официальным стандартам разработки 1С и БСП:\n```bsl\n{code}\n```".to_string(),
            is_enabled: true,
            is_system: true,
        },
        SlashCommand {
            id: "its".to_string(),
            command: "итс".to_string(),
            name: "1С:ИТС".to_string(),
            description: "Поиск информации в ИТС через Напарника".to_string(),
            template: "Используй инструменты MCP сервера \"Напарник\" (1C:Naparnik), чтобы найти ответ на мой вопрос в информационной системе 1С:ИТС. Мой вопрос: {query}".to_string(),
            is_enabled: true,
            is_system: true,
        },
        SlashCommand {
            id: "search-1c".to_string(),
            command: "найти".to_string(),
            name: "1С:Найти".to_string(),
            description: "Поиск кода в конфигурации 1С".to_string(),
            template: "Выполни поиск в конфигурации 1С по запросу: \"{query}\".\n\nИнструкции:\n1. Если запрос содержит имя процедуры или функции — используй find_symbol для точного поиска по символьному индексу.\n2. Если ищешь текст, переменную или фрагмент кода — используй search_code.\n3. Если в запросе упоминается конкретный объект (\"в модуле X\", \"в справочнике Y\") — передай scope в search_code.\n4. Для найденных символов — вызови get_symbol_context чтобы показать полный код.\nПокажи результаты с объяснением.".to_string(),
            is_enabled: true,
            is_system: true,
        },
        SlashCommand {
            id: "refs-1c".to_string(),
            command: "где".to_string(),
            name: "1С:Где используется".to_string(),
            description: "Найти все места использования символа в конфигурации".to_string(),
            template: "Найди все места использования \"{query}\" в конфигурации 1С.\nИспользуй инструмент find_references для поиска всех вхождений.\nПокажи результаты, сгруппированные по модулям, с краткой аннотацией к каждому месту использования.".to_string(),
            is_enabled: true,
            is_system: true,
        },
        SlashCommand {
            id: "struct-1c".to_string(),
            command: "объект".to_string(),
            name: "1С:Структура объекта".to_string(),
            description: "Показать структуру объекта конфигурации (реквизиты, ТЧ, формы)".to_string(),
            template: "Покажи структуру объекта конфигурации 1С: \"{query}\".\n1. Используй get_object_structure для получения реквизитов, табличных частей, форм и модулей.\n2. Если объект не найден — используй list_objects с name_filter для поиска похожих объектов.\n3. Опиши структуру понятно для разработчика.".to_string(),
            is_enabled: true,
            is_system: true,
        },
    ]
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
    pub headers: Option<std::collections::HashMap<String, String>>,
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
            headers: None,
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
    pub debug_mode: bool,
    #[serde(default)]
    pub onboarding_completed: bool,
    /// Настройки пользовательских промптов
    #[serde(default)]
    pub custom_prompts: CustomPromptsSettings,
    /// Настройки генерации кода
    #[serde(default)]
    pub code_generation: CodeGenerationSettings,
    /// Быстрые команды
    #[serde(default = "default_slash_commands")]
    pub slash_commands: Vec<SlashCommand>,
    
    /// Максимальное количество итераций агента
    #[serde(default = "default_max_iterations")]
    pub max_agent_iterations: Option<u32>,

    /// Тема оформления (light / dark)
    #[serde(default)]
    pub theme: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PromptBehaviorPreset {
    Project,
    Maintenance,
}

impl Default for PromptBehaviorPreset {
    fn default() -> Self {
        Self::Project
    }
}

// LabelingStyle больше не нужен, он зашит в пресет

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

    /// Пресет поведения
    #[serde(default)]
    pub behavior_preset: PromptBehaviorPreset,
    
    /// Маркировать изменения
    #[serde(default = "default_true")]
    pub mark_changes: bool,
    
    /// Шаблон маркера для добавления (Maintenance)
    #[serde(default = "default_addition_marker")]
    pub addition_marker_template: String,

    /// Шаблон маркера для изменения (Maintenance)
    #[serde(default = "default_modification_marker")]
    pub modification_marker_template: String,

    /// Шаблон маркера для удаления (Maintenance)
    #[serde(default = "default_deletion_marker")]
    pub deletion_marker_template: String,
}

impl Default for CodeGenerationSettings {
    fn default() -> Self {
        Self {
            mode: CodeGenerationMode::Diff,
            behavior_preset: PromptBehaviorPreset::Project,
            mark_changes: true,
            addition_marker_template: default_addition_marker(),
            modification_marker_template: default_modification_marker(),
            deletion_marker_template: default_deletion_marker(),
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
            templates: vec![
                PromptTemplate {
                    id: "bsl-standards".to_string(),
                    name: "Стандарты 1С".to_string(),
                    description: "Соблюдать стандарты разработки 1С и БСП".to_string(),
                    content: "Соблюдай стандарты разработки 1С и Библиотеки Стандартных Подсистем (БСП).".to_string(),
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

    let mut modified = false;
    
    // Migration: debug_mcp -> debug_mode
    let path = get_settings_file();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&content) {
                if let Some(old_val) = map.get("debug_mcp") {
                    if !map.contains_key("debug_mode") {
                        if let Some(b) = old_val.as_bool() {
                            crate::app_log!("[SETTINGS] Migrating 'debug_mcp' ({}) to 'debug_mode'", b);
                            settings.debug_mode = b;
                            modified = true;
                        }
                    }
                }
            }
        }
    }
    
    // Migration: Force high-performance node launcher for built-in MCP servers
    for server in settings.mcp_servers.iter_mut() {
        if server.id == "builtin-1c-naparnik" || server.id == "builtin-1c-metadata" || server.id == "builtin-1c-help" {
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
        } else if server.id == "builtin-1c-search" {
            // 1С:Поиск — Rust binary, command must stay as mcp-1c-search.exe (NOT node)
            let current_cmd = server.command.as_deref().unwrap_or("");
            if current_cmd != "mcp-1c-search.exe" && !current_cmd.ends_with("mcp-1c-search.exe") {
                crate::app_log!("[SETTINGS] Migrating builtin-1c-search command to 'mcp-1c-search.exe'");
                server.command = Some("mcp-1c-search.exe".to_string());
                server.args = None;
                modified = true;
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

    // Migration: ensure default slash commands exist
    if settings.slash_commands.is_empty() {
        settings.slash_commands = default_slash_commands();
        modified = true;
    } else {
        // Inject new system commands that may be missing in existing settings
        let new_system_ids = ["search-1c", "refs-1c", "struct-1c"];
        let existing_ids: std::collections::HashSet<String> = settings.slash_commands.iter()
            .map(|c| c.id.clone())
            .collect();
        let to_add: Vec<SlashCommand> = default_slash_commands()
            .into_iter()
            .filter(|cmd| new_system_ids.contains(&cmd.id.as_str()) && !existing_ids.contains(&cmd.id))
            .collect();
        if !to_add.is_empty() {
            settings.slash_commands.extend(to_add);
            modified = true;
        }
    }

    if modified {
        let _ = save_settings(&settings);
    }

    crate::logger::set_debug_mode(settings.debug_mode);
    settings
}

/// Save settings to file
pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let dir = get_settings_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    
    let path = get_settings_file();
    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| e.to_string())?;
    
    crate::logger::set_debug_mode(settings.debug_mode);
    fs::write(path, content).map_err(|e| e.to_string())
}
