// ─── Полный парсер заголовка Конфигуратора ───────────────────────────────────

/**
 * Контекст исполнения BSL-кода.
 * Определяет, какие директивы компилятора применимы.
 */
export type ExecutionContext =
    | 'managed_form'   // Форма / МодульФормы — &НаСервере, &НаКлиенте, &НаСервереБезКонтекста
    | 'server'         // МодульОбъекта, МодульМенеджера, МодульНабораЗаписей — только серверный
    | 'common_module'  // Общий модуль — контекст зависит от свойств модуля
    | 'client'         // Клиентский модуль (МодульПриложения и др.)
    | 'unknown';

export interface ConfiguratorTitleContext {
    raw_title: string;
    config_name?: string;
    object_type?: string;
    object_name?: string;
    module_type?: string;
    execution_context?: ExecutionContext;
    read_only?: boolean;
    is_external_file?: boolean;
    confidence: number;
}

// ─── Типы объектов ────────────────────────────────────────────────────────────
// Полный список из структуры конфигурации 1С (папки src/cf/*)
// Двусловные и трёхсловные — проверять перед однословными

const OBJECT_TYPES_MULTI = [
    // Регистры
    'Регистр сведений',
    'Регистр накопления',
    'Регистр бухгалтерии',
    'Регистр расчета',
    // Планы
    'План видов характеристик',
    'План видов расчета',
    'План счетов',
    'План обмена',
    // Общие объекты
    'Общий модуль',
    'Общая форма',
    'Общая команда',
    'Общий макет',
    'Общая картинка',
    // Прочие составные
    'Журнал документов',
    'Бизнес-процесс',
    'Хранилище настроек',
    'Регламентное задание',
    'Параметр сеанса',
    'Подписка на события',
    'Функциональная опция',
    'Критерий отбора',
    'Определяемый тип',
    'Web-сервис',
    'XDTO пакет',
];

const OBJECT_TYPES_SINGLE = [
    'Документ',
    'Справочник',
    'Перечисление',
    'Обработка',
    'Отчет',
    'Задача',
    'Константа',
    'Подсистема',
    'Роль',
    'Язык',
    'Стиль',
    'Последовательность',
    'Интерфейс',
];

// ─── Типы модулей ─────────────────────────────────────────────────────────────
// Маппинг: строка из заголовка → внутреннее имя

const MODULE_TYPE_MAP: Record<string, string> = {
    'Модуль менеджера': 'МодульМенеджера',
    'Модуль объекта': 'МодульОбъекта',
    'Модуль формы': 'МодульФормы',
    'Модуль набора записей': 'МодульНабораЗаписей',
    'Модуль приложения': 'МодульПриложения',
    'Модуль внешнего соединения': 'МодульВнешнегоСоединения',
    'Модуль сеанса': 'МодульСеанса',
    'Модуль': 'Модуль',
    'Форма': 'Форма',
};

// ─── Контекст исполнения ──────────────────────────────────────────────────────

const EXECUTION_CONTEXT_DESCRIPTIONS: Record<ExecutionContext, string> = {
    managed_form:  'Управляемая форма — доступны директивы &НаСервере, &НаКлиенте, &НаСервереБезКонтекста, &НаКлиентеНаСервереБезКонтекста',
    server:        'Серверный модуль — директивы компилятора не используются, весь код выполняется на сервере',
    common_module: 'Общий модуль — контекст исполнения определяется настройками модуля (Сервер/Клиент/ВызовСервера)',
    client:        'Клиентский модуль — весь код выполняется на клиенте',
    unknown:       '',
};

/**
 * Определяет контекст исполнения BSL-кода по типу модуля.
 *
 * managed_form  → &НаСервере, &НаКлиенте, &НаСервереБезКонтекста
 * server        → только серверный код, директивы не нужны
 * common_module → зависит от настроек модуля (Сервер/Клиент/ВызовСервера)
 * client        → только клиентский код
 */
function resolveExecutionContext(moduleType: string | undefined): ExecutionContext {
    if (!moduleType) return 'unknown';
    switch (moduleType) {
        case 'Форма':
        case 'МодульФормы':
            return 'managed_form';
        case 'МодульОбъекта':
        case 'МодульМенеджера':
        case 'МодульНабораЗаписей':
        case 'МодульВнешнегоСоединения':
        case 'МодульСеанса':
            return 'server';
        case 'Модуль':
            return 'common_module';
        case 'МодульПриложения':
            return 'client';
        default:
            return 'unknown';
    }
}

/**
 * Полный парсер заголовка окна 1С Конфигуратора.
 *
 * Реальный формат (разделитель " - "):
 *   {ObjectType} {ObjectName}: {ModuleType} [{flags}] - Конфигуратор - {ConfigName}
 *   {FilePath}: {ModuleType} - Конфигуратор - {ConfigName}
 *   Конфигуратор - {ConfigName}
 *
 * Также поддерживает em-dash "—" как альтернативный разделитель.
 */
export function parseConfiguratorTitleFull(title: string): ConfiguratorTitleContext {
    if (!title) return { raw_title: '', confidence: 0 };

    const result: ConfiguratorTitleContext = { raw_title: title, confidence: 0 };

    // Найти " - Конфигуратор - " или " — Конфигуратор — " (регистронезависимо)
    const confSepRegex = /\s[—\-]\s*Конфигуратор\s*[—\-]\s+(.+)$/;
    const confMatch = title.match(confSepRegex);

    if (!confMatch) {
        // Простой вариант: "Конфигуратор - ConfigName"
        const simple = title.match(/^Конфигуратор\s*[—\-]\s*(.+)$/);
        if (simple) {
            result.config_name = simple[1].trim();
            result.confidence = 0.5;
        }
        return result;
    }

    result.config_name = confMatch[1].trim();
    result.confidence = 0.5;

    // Левая часть до " - Конфигуратор - "
    const leftPart = title.substring(0, confMatch.index).trim();
    if (!leftPart) return result;

    // Разделить по ": " (двоеточие + пробел).
    // Для путей типа "U:\folder\file.epf: Форма" — разделитель ":\\" не совпадает.
    const colonIdx = leftPart.lastIndexOf(': ');
    if (colonIdx === -1) return result;

    const objectInfo = leftPart.substring(0, colonIdx).trim();
    let modulePart = leftPart.substring(colonIdx + 2).trim();

    // Флаг [Только для чтения]
    if (modulePart.includes('[Только для чтения]')) {
        result.read_only = true;
        modulePart = modulePart.replace('[Только для чтения]', '').trim();
    }

    // Маппинг типа модуля и контекст исполнения
    result.module_type = MODULE_TYPE_MAP[modulePart] ?? (modulePart || undefined);
    result.execution_context = resolveExecutionContext(result.module_type);

    // Путь к внешнему файлу (Windows-путь с drive letter)
    if (/^[A-Za-z]:\\/.test(objectInfo)) {
        result.is_external_file = true;
        const parts = objectInfo.split('\\');
        const filename = parts[parts.length - 1] ?? '';
        result.object_name = filename.replace(/\.(epf|erf|cf|cfe)$/i, '');
        result.object_type = /\.epf$/i.test(filename) ? 'ВнешняяОбработка'
            : /\.erf$/i.test(filename) ? 'ВнешнийОтчет'
            : 'ВнешнийФайл';
        result.confidence = 0.8;
        return result;
    }

    // Парсинг типа объекта: сначала двусловные, потом однословные
    for (const objType of OBJECT_TYPES_MULTI) {
        if (objectInfo.startsWith(objType + ' ')) {
            result.object_type = objType;
            result.object_name = objectInfo.substring(objType.length + 1).trim();
            result.confidence = 0.95;
            return result;
        }
    }
    for (const objType of OBJECT_TYPES_SINGLE) {
        if (objectInfo.startsWith(objType + ' ')) {
            result.object_type = objType;
            result.object_name = objectInfo.substring(objType.length + 1).trim();
            result.confidence = 0.95;
            return result;
        }
    }

    // Не распознан тип — сохраняем как имя
    result.object_name = objectInfo;
    result.confidence = 0.65;
    return result;
}

/**
 * Форматирует контекст Конфигуратора для передачи в LLM.
 * Вставляется перед кодом в промпт.
 */
export function formatConfiguratorContextForLLM(ctx: ConfiguratorTitleContext): string {
    const lines: string[] = [
        'SOURCE: 1C CONFIGURATOR',
        '',
        'RAW WINDOW TITLE:',
        ctx.raw_title,
        '',
    ];

    const hasParsed = ctx.config_name || ctx.object_type || ctx.object_name || ctx.module_type;
    if (hasParsed) {
        lines.push('PARSED CONTEXT:');
        if (ctx.object_type) lines.push(`Тип объекта: ${ctx.object_type}`);
        if (ctx.object_name) lines.push(`Имя объекта: ${ctx.object_name}`);
        if (ctx.module_type) lines.push(`Тип модуля: ${ctx.module_type}`);
        if (ctx.config_name) lines.push(`Конфигурация: ${ctx.config_name}`);
        if (ctx.read_only) lines.push('Режим: Только для чтения');

        // Контекст исполнения — важен для ИИ при генерации кода
        if (ctx.execution_context) {
            const ctxDesc = EXECUTION_CONTEXT_DESCRIPTIONS[ctx.execution_context];
            if (ctxDesc) lines.push(`Контекст исполнения: ${ctxDesc}`);
        }

        lines.push('');
        lines.push(`CONFIDENCE: ${ctx.confidence.toFixed(2)}`);
        lines.push('');
    }

    return lines.join('\n');
}

// ─── Простой парсер (для UI-кнопки) ──────────────────────────────────────────

/**
 * Parses the 1C Configurator window title to extract only the configuration name.
 * Expected format: "Object - Configurator - ConfigName"
 */
export function parseConfiguratorTitle(title: string): string {
    if (!title) return "Конфигуратор";

    const parts = title.split(' - ');

    if (parts.length >= 3) {
        // Типичная структура: [Object/File] - [Configurator] - [BaseName]
        const baseName = parts[parts.length - 1].trim();

        // Убираем лишние суффиксы типа " (1С:Предприятие)"
        return baseName
            .replace(/\s*\(.*?\)\s*$/, '')
            .replace(/\s*\[.*?\]\s*$/, '')
            .trim();
    }

    if (parts.length === 2) {
        return parts[1].trim();
    }

    // Для файловых путей берем последний сегмент
    if (title.includes('\\')) {
        const pathParts = title.split('\\');
        const lastPart = pathParts[pathParts.length - 1] || title;
        // Убираем расширение .1CD если есть
        return lastPart.replace(/\.1CD$/i, '');
    }

    // Обрезаем если слишком длинное
    if (title.length > 25) {
        return title.substring(0, 22) + '...';
    }

    return title;
}

/**
 * Возвращает сокращенное имя для UI с tooltip
 */
export function getShortConfigName(title: string, maxLength = 15): string {
    const parsed = parseConfiguratorTitle(title);
    if (parsed.length <= maxLength) return parsed;
    return parsed.substring(0, maxLength - 2) + '..';
}
