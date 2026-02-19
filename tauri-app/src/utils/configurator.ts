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
