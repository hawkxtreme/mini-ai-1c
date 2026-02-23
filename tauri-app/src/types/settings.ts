export interface WindowInfo {
    hwnd: number;
    title: string;
}

export interface BslStatus {
    installed: boolean;
    java_info: string;
    connected: boolean;
}

// Режим генерации кода
export type CodeGenerationMode = 'full' | 'diff' | 'auto';

// Пресеты поведения промптов
export type PromptBehaviorPreset = 'project' | 'maintenance';

// Стиль маркировки больше не нужен как отдельный тип, он зашит в пресет

// Настройки генерации кода
export interface CodeGenerationSettings {
    mode: CodeGenerationMode;
    behavior_preset: PromptBehaviorPreset;
    mark_changes: boolean;
    addition_marker_template: string;
    modification_marker_template: string;
    deletion_marker_template: string;
}


// Шаблон промпта
export interface PromptTemplate {
    id: string;
    name: string;
    description: string;
    content: string;
    enabled: boolean;
}

// Настройки пользовательских промптов
export interface CustomPromptsSettings {
    system_prefix: string;
    on_code_change: string;
    on_code_generate: string;
    templates: PromptTemplate[];
}

export interface McpServerConfig {
    id: string;
    name: string;
    enabled: boolean;
    transport: 'http' | 'stdio' | 'internal';
    url?: string | null;
    login?: string | null;
    password?: string | null;
    command?: string | null;
    args?: string[] | null;
    env?: Record<string, string> | null;
}

export interface AppSettings {
    configurator: {
        window_title_pattern: string;
        selected_window_hwnd: number | null;
    };
    bsl_server: {
        jar_path: string;
        websocket_port: number;
        enabled: boolean;
        java_path: string;
        auto_download: boolean;
    };
    mcp_servers: McpServerConfig[];
    active_llm_profile: string;
    debug_mcp: boolean;
    onboarding_completed?: boolean;
    custom_prompts: CustomPromptsSettings;
    code_generation: CodeGenerationSettings;
}

export interface BslDiagnosticItem {
    status: 'ok' | 'warn' | 'error';
    title: string;
    message: string;
    suggestion?: string;
}

// Значения по умолчанию для новых настроек
export const DEFAULT_ADDITION_MARKER_TEMPLATE = "// Доработка START (Добавление) - {datetime}\n{newCode}\n// Доработка END";
export const DEFAULT_MODIFICATION_MARKER_TEMPLATE = "// Доработка START (Изменение) - {datetime}\n{newCode}\n// Доработка END";
export const DEFAULT_DELETION_MARKER_TEMPLATE = "// Доработка (Удаление) - {datetime}\n// {oldCode}";

export const DEFAULT_CUSTOM_PROMPTS: CustomPromptsSettings = {
    system_prefix: "",
    on_code_change: "",
    on_code_generate: "",
    templates: [
        {
            id: "bsl-standards",
            name: "Стандарты 1С",
            description: "Соблюдать стандарты разработки 1С и БСП",
            content: "Соблюдай стандарты разработки 1С и Библиотеки Стандартных Подсистем (БСП).",
            enabled: false
        }
    ]
};

export const DEFAULT_CODE_GENERATION: CodeGenerationSettings = {
    mode: "diff",
    behavior_preset: "project",
    mark_changes: true,
    addition_marker_template: DEFAULT_ADDITION_MARKER_TEMPLATE,
    modification_marker_template: DEFAULT_MODIFICATION_MARKER_TEMPLATE,
    deletion_marker_template: DEFAULT_DELETION_MARKER_TEMPLATE
};
