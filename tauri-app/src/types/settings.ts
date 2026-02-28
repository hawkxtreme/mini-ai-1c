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

// CLI Auth Types
export interface CliAuthInitResponse {
    device_code: string;
    user_code: string;
    verification_url: string;
    expires_in: number;
    poll_interval: number;
    code_verifier?: string;
}

export type CliAuthStatus =
    | { status: 'Pending' }
    | { status: 'Authorized'; data: { access_token: string; refresh_token: string | null; expires_at: number; resource_url: string | null } }
    | { status: 'Expired' }
    | { status: 'SlowDown' }
    | { status: 'Error'; data: string };

export interface CliUsage {
    requests_used: number;
    requests_limit: number;
    resets_at?: string;
}

export interface CliStatus {
    is_authenticated: boolean;
    auth_expires_at?: string;
    usage?: CliUsage;
}

// Пресеты поведения промптов
export type PromptBehaviorPreset = 'project' | 'maintenance' | 'cli';

export type CliProviderType = 'qwen' | 'gemini' | 'codex' | 'claude';

export interface CliProviderUsage {
    requests_used: number;
    requests_limit: number;
    resets_at?: string;
}

export interface CliProviderInfo {
    provider: CliProviderType;
    is_authenticated: boolean;
    auth_expires_at?: string;
    usage?: CliProviderUsage;
}

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

export interface SlashCommand {
    id: string;
    command: string;
    name: string;
    description: string;
    template: string;
    is_enabled: boolean;
    is_system: boolean;
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
    debug_mode: boolean;
    onboarding_completed?: boolean;
    custom_prompts: CustomPromptsSettings;
    code_generation: CodeGenerationSettings;
    slash_commands: SlashCommand[];
    max_agent_iterations?: number | null;
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

export const DEFAULT_SLASH_COMMANDS: SlashCommand[] = [
    {
        id: 'fix',
        command: 'исправить',
        name: 'Исправить',
        description: 'Исправить ошибки BSL и логические ошибки',
        template: 'Исправь ошибки в этом коде. Обрати внимание на следующие диагностики:\n{diagnostics}\n\nКод для исправления:\n```bsl\n{code}\n```',
        is_enabled: true,
        is_system: true
    },
    {
        id: 'refactor',
        command: 'рефакторинг',
        name: 'Рефакторинг',
        description: 'Улучшить структуру и читаемость кода',
        template: 'Проведи рефакторинг этого кода, улучши его структуру и читаемость, соблюдая стандарты 1С:\n```bsl\n{code}\n```',
        is_enabled: true,
        is_system: true
    },
    {
        id: 'desc',
        command: 'описание',
        name: 'Описание',
        description: 'Сгенерировать описание процедуры/функции',
        template: 'Сгенерируй стандартную шапку описания для этой процедуры/функции в формате 1С (только комментарии //, без тегов <Описание>):\n```bsl\n{code}\n```',
        is_enabled: true,
        is_system: true
    },
    {
        id: 'explain',
        command: 'объясни',
        name: 'Объясни',
        description: 'Подробно объяснить работу кода',
        template: 'Подробно объясни, как работает этот фрагмент кода:\n```bsl\n{code}\n```',
        is_enabled: true,
        is_system: true
    },
    {
        id: 'review',
        command: 'ревью',
        name: 'Ревью',
        description: 'Провести код-ревью',
        template: 'Проведи подробное код-ревью этого фрагмента. Найди потенциальные баги, узкие места и предложи улучшения:\n```bsl\n{code}\n```',
        is_enabled: true,
        is_system: true
    },
    {
        id: 'standards',
        command: 'стандарты',
        name: 'Стандарты',
        description: 'Проверить на соответствие стандартам 1С',
        template: 'Проверь этот код на соответствие официальным стандартам разработки 1С и БСП:\n```bsl\n{code}\n```',
        is_enabled: true,
        is_system: true
    },
    {
        id: 'its',
        command: 'итс',
        name: '1С:ИТС',
        description: 'Поиск информации в ИТС через Напарника',
        template: 'Используй инструменты MCP сервера "Напарник" (1C:Naparnik), чтобы найти ответ на мой вопрос в информационной системе 1С:ИТС. Мой вопрос: {query}',
        is_enabled: true,
        is_system: true
    }
];

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
