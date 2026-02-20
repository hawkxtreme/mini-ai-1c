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

// Настройки генерации кода
export interface CodeGenerationSettings {
    mode: CodeGenerationMode;
    preserve_copyright: boolean;
    mark_changes: boolean;
    change_marker_template: string;
}

// Настройки маркеров изменений
export interface ChangeMarkersSettings {
    enabled: boolean;
    template: string;
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
    change_markers: ChangeMarkersSettings;
    templates: PromptTemplate[];
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
    mcp_servers: {
        id: string;
        name: string;
        enabled: boolean;
        transport: 'http' | 'stdio' | 'internal';
        url?: string | null;
        login?: string | null;
        password?: string | null;
        command?: string | null;
        args?: string[] | null;
    }[];
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
export const DEFAULT_CHANGE_MARKER_TEMPLATE = "// [ИЗМЕНЕНО AI] - {date}";

export const DEFAULT_CUSTOM_PROMPTS: CustomPromptsSettings = {
    system_prefix: "",
    on_code_change: "",
    on_code_generate: "",
    change_markers: {
        enabled: true,
        template: DEFAULT_CHANGE_MARKER_TEMPLATE
    },
    templates: [
        {
            id: "bsl-standards",
            name: "Стандарты 1С",
            description: "Соблюдать стандарты разработки 1С и БСП",
            content: "Соблюдай стандарты разработки 1С и Библиотеки Стандартных Подсистем (БСП).",
            enabled: false
        },
        {
            id: "wrap-changes",
            name: "Оборачивать изменения",
            description: "Оборачивать изменения в комментарии доработки",
            content: `Все изменения оборачивай в комментарии:
// Доработка START
// Дата: {date}
<измененный код>
// Доработка END`,
            enabled: false
        }
    ]
};

export const DEFAULT_CODE_GENERATION: CodeGenerationSettings = {
    mode: "full",
    preserve_copyright: true,
    mark_changes: true,
    change_marker_template: DEFAULT_CHANGE_MARKER_TEMPLATE
};
