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
    headers?: Record<string, string> | null;
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
    theme?: 'light' | 'dark';
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
        template: 'Исправь ошибки в этом коде. Если доступен инструмент check_bsl_syntax — СНАЧАЛА вызови его для получения актуального анализа, затем исправь все найденные ошибки. Также обрати внимание на следующие диагностики:\n{diagnostics}\n\nКод для исправления:\n```bsl\n{code}\n```',
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
        template: 'Добавь стандартную шапку описания перед объявлением этой процедуры/функции в формате 1С (только комментарии //, без тегов <Описание>). В <search> — только строка объявления Функция/Процедура, в <replace> — шапка описания + та же строка объявления:\n```bsl\n{code}\n```',
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
        template: 'Используй инструмент `ask_1c_ai` (MCP сервер "Напарник" / 1C:Naparnik) для поиска ответа в информационной системе 1С:ИТС. Вызови этот инструмент напрямую с моим вопросом. Если инструмент `ask_1c_ai` недоступен — сообщи об этом явно. Мой вопрос: {query}',
        is_enabled: true,
        is_system: true
    },
    {
        id: 'search-1c',
        command: 'найти',
        name: '1С:Найти',
        description: 'Поиск кода в конфигурации 1С',
        template: 'Выполни поиск в конфигурации 1С по запросу: "{query}".\n\nИнструкции:\n1. Если запрос содержит имя процедуры или функции — используй find_symbol для точного поиска по символьному индексу.\n2. Если ищешь текст, переменную или фрагмент кода — используй search_code.\n3. Если в запросе упоминается конкретный объект ("в модуле X", "в справочнике Y") — передай scope в search_code.\n4. Для найденных символов — вызови get_symbol_context чтобы показать полный код.\nПокажи результаты с объяснением.',
        is_enabled: true,
        is_system: true
    },
    {
        id: 'refs-1c',
        command: 'где',
        name: '1С:Где используется',
        description: 'Найти все места использования символа в конфигурации',
        template: 'Найди все места использования "{query}" в конфигурации 1С.\nИспользуй инструмент find_references для поиска всех вхождений.\nПокажи результаты, сгруппированные по модулям, с краткой аннотацией к каждому месту использования.',
        is_enabled: true,
        is_system: true
    },
    {
        id: 'struct-1c',
        command: 'объект',
        name: '1С:Структура объекта',
        description: 'Показать структуру объекта конфигурации (реквизиты, ТЧ, формы)',
        template: 'Покажи структуру объекта конфигурации 1С: "{query}".\n1. Используй get_object_structure для получения реквизитов, табличных частей, форм и модулей.\n2. Если объект не найден — используй list_objects с name_filter для поиска похожих объектов.\n3. Опиши структуру понятно для разработчика.',
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
