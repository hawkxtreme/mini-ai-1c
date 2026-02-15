/**
 * Parses the 1C Configurator window title to extract only the configuration name.
 * Expected format: "Object - Configurator - ConfigName"
 */
export function parseConfiguratorTitle(title: string): string {
    if (!title) return "Конфигуратор";

    // Split by " - " which is the standard separator in 1C windows
    const parts = title.split(' - ');

    if (parts.length >= 3) {
        // Typical structure: [Object/File] - [Configurator] - [BaseName]
        // We want the last part which is usually the database/configuration name
        return parts[parts.length - 1].trim();
    }

    // Fallback for other title formats (e.g. just "Configurator - BaseName")
    if (parts.length === 2) {
        return parts[1].trim();
    }

    // Secondary fallback for file paths if parts split didn't yield much
    if (title.includes('\\')) {
        const pathParts = title.split('\\');
        return pathParts[pathParts.length - 1] || title;
    }

    return title;
}
