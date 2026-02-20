/**
 * Утилита для парсинга и применения изменений в формате Search/Replace
 * Используется для применения diff-изменений от AI к коду BSL
 */

export interface CodeChange {
    /** Точное содержимое для поиска и замены */
    search: string;
    /** Новый код для замены */
    replace: string;
    /** Номер строки начала поиска (опционально) */
    lineStart?: number;
}

export interface DiffParseResult {
    /** Массив найденных изменений */
    changes: CodeChange[];
    /** Были ли ошибки при парсинге */
    hasErrors: boolean;
    /** Сообщения об ошибках */
    errors: string[];
}

/**
 * Парсит содержимое ответа AI и извлекает diff-блоки в формате Search/Replace
 * 
 * Формат блока:
 * <<<<<<< SEARCH
 * :строка:[номер_строки]
 * -------
 * [точный код для поиска]
 * =======
 * [новый код]
 * >>>>>>> REPLACE
 * 
 * @param content - Текст ответа от AI
 * @returns Объект с массивом изменений и информацией об ошибках
 */
export function parseDiffBlocks(content: string): DiffParseResult {
    const changes: CodeChange[] = [];
    const errors: string[] = [];
    
    // Регулярное выражение для поиска diff-блоков
    // Поддерживает как русский (:строка:), так и английский (:line:) формат
    const regex = /<<<<<<< SEARCH\n:строка:(\d+|EOF)\n-------\n([\s\S]*?)\n=======\n([\s\S]*?)\n>>>>>>> REPLACE/g;
    const regexEn = /<<<<<<< SEARCH\n:line:(\d+|EOF)\n-------\n([\s\S]*?)\n=======\n([\s\S]*?)\n>>>>>>> REPLACE/g;
    
    let match;
    let blockIndex = 0;
    
    // Парсим блоки в русском формате
    while ((match = regex.exec(content)) !== null) {
        blockIndex++;
        const lineStartStr = match[1];
        const search = match[2];
        const replace = match[3];
        
        if (!search.trim()) {
            errors.push(`Блок #${blockIndex}: пустой блок SEARCH`);
            continue;
        }
        
        const lineStart = lineStartStr === 'EOF' ? Infinity : parseInt(lineStartStr, 10);
        
        if (lineStart !== Infinity && (isNaN(lineStart) || lineStart < 1)) {
            errors.push(`Блок #${blockIndex}: некорректный номер строки "${lineStartStr}"`);
            continue;
        }
        
        changes.push({
            lineStart,
            search: search.trim(),
            replace: replace.trim()
        });
    }
    
    // Парсим блоки в английском формате
    while ((match = regexEn.exec(content)) !== null) {
        blockIndex++;
        const lineStartStr = match[1];
        const search = match[2];
        const replace = match[3];
        
        if (!search.trim()) {
            errors.push(`Block #${blockIndex}: empty SEARCH block`);
            continue;
        }
        
        const lineStart = lineStartStr === 'EOF' ? Infinity : parseInt(lineStartStr, 10);
        
        if (lineStart !== Infinity && (isNaN(lineStart) || lineStart < 1)) {
            errors.push(`Block #${blockIndex}: invalid line number "${lineStartStr}"`);
            continue;
        }
        
        changes.push({
            lineStart,
            search: search.trim(),
            replace: replace.trim()
        });
    }
    
    return {
        changes,
        hasErrors: errors.length > 0,
        errors
    };
}

/**
 * Проверяет, содержит ли ответ AI diff-блоки
 * 
 * @param content - Текст ответа от AI
 * @returns true если содержатся diff-блоки
 */
export function hasDiffBlocks(content: string): boolean {
    const regex = /<<<<<<< SEARCH\n:строка:(\d+|EOF)\n-------/;
    const regexEn = /<<<<<<< SEARCH\n:line:(\d+|EOF)\n-------/;
    return regex.test(content) || regexEn.test(content);
}

/**
 * Применяет diff-изменения к оригинальному коду
 * 
 * @param originalCode - Исходный код
 * @param changes - Массив изменений
 * @returns Объект с результатом применения и информацией об ошибках
 */
export function applyDiffChanges(
    originalCode: string, 
    changes: CodeChange[]
): { result: string; success: boolean; appliedCount: number; failedChanges: number; errors: string[] } {
    if (!changes.length) {
        return {
            result: originalCode,
            success: true,
            appliedCount: 0,
            failedChanges: 0,
            errors: []
        };
    }
    
    let result = originalCode;
    let appliedCount = 0;
    let failedChanges = 0;
    const errors: string[] = [];
    
    // Сортируем изменения по убыванию номера строки для корректного применения
    // (чтобы изменения в начале файла не смещали позиции изменений в конце)
    const sortedChanges = [...changes].sort((a, b) => 
        (b.lineStart || 0) - (a.lineStart || 0)
    );
    
    for (const change of sortedChanges) {
        if (change.lineStart === Infinity) {
            // Добавление в конец файла
            if (change.replace) {
                // Добавляем перевод строки если нужно
                const separator = result.endsWith('\n') ? '' : '\n';
                result = result + separator + change.replace;
                appliedCount++;
            }
        } else {
            // Поиск и замена
            // Проверяем наличие искомого текста
            if (!result.includes(change.search)) {
                failedChanges++;
                errors.push(
                    `Не найден текст для замены (строка ${change.lineStart}): "${change.search.substring(0, 50)}..."`
                );
                continue;
            }
            
            // Безопасная замена - только первое вхождение
            const index = result.indexOf(change.search);
            if (index !== -1) {
                result = result.substring(0, index) + change.replace + result.substring(index + change.search.length);
                appliedCount++;
            }
        }
    }
    
    return {
        result,
        success: failedChanges === 0,
        appliedCount,
        failedChanges,
        errors
    };
}

/**
 * Извлекает код из ответа AI (из кодовых блоков или как есть)
 * 
 * @param content - Текст ответа от AI
 * @param language - Язык кода для извлечения (по умолчанию 'bsl')
 * @returns Извлечённый код или null
 */
export function extractCodeFromResponse(content: string, language: string = 'bsl'): string | null {
    // Ищем кодовый блок с указанным языком
    const codeBlockRegex = new RegExp(`\`\`\`${language}\\s*\\n([\\s\\S]*?)\n\`\`\``, 'i');
    const match = content.match(codeBlockRegex);
    
    if (match && match[1]) {
        return match[1].trim();
    }
    
    // Если кодовый блок не найден, проверяем наличие diff-блоков
    if (hasDiffBlocks(content)) {
        return content; // Возвращаем как есть для последующего парсинга
    }
    
    return null;
}

/**
 * Определяет тип ответа AI (полный код или diff)
 * 
 * @param content - Текст ответа от AI
 * @returns 'full' если полный код, 'diff' если diff-блоки, 'unknown' если не определено
 */
export function detectResponseType(content: string): 'full' | 'diff' | 'unknown' {
    if (hasDiffBlocks(content)) {
        return 'diff';
    }
    
    // Ищем кодовый блок BSL
    const bslCodeBlock = /```bsl\s*\n[\s\S]*?\n```/i.test(content);
    if (bslCodeBlock) {
        return 'full';
    }
    
    return 'unknown';
}

/**
 * Безопасно применяет изменения к коду с валидацией
 * 
 * @param originalCode - Исходный код
 * @param aiResponse - Ответ AI
 * @returns Объект с результатом и детальной информацией
 */
export function safelyApplyChanges(
    originalCode: string,
    aiResponse: string
): {
    result: string;
    type: 'full' | 'diff' | 'unknown';
    success: boolean;
    message: string;
    appliedCount?: number;
    errors: string[];
} {
    const responseType = detectResponseType(aiResponse);
    
    if (responseType === 'diff') {
        const parseResult = parseDiffBlocks(aiResponse);
        
        if (parseResult.hasErrors) {
            return {
                result: originalCode,
                type: 'diff',
                success: false,
                message: `Ошибки парсинга diff-блоков: ${parseResult.errors.join('; ')}`,
                errors: parseResult.errors
            };
        }
        
        if (parseResult.changes.length === 0) {
            return {
                result: originalCode,
                type: 'diff',
                success: false,
                message: 'Diff-блоки не найдены в ответе',
                errors: ['No diff blocks found']
            };
        }
        
        const applyResult = applyDiffChanges(originalCode, parseResult.changes);
        
        return {
            result: applyResult.result,
            type: 'diff',
            success: applyResult.success,
            message: applyResult.success 
                ? `Успешно применено ${applyResult.appliedCount} изменений`
                : `Применено ${applyResult.appliedCount} изменений, ${applyResult.failedChanges} ошибок`,
            appliedCount: applyResult.appliedCount,
            errors: applyResult.errors
        };
    }
    
    if (responseType === 'full') {
        const code = extractCodeFromResponse(aiResponse, 'bsl');
        
        if (code) {
            return {
                result: code,
                type: 'full',
                success: true,
                message: 'Получен полный код',
                errors: []
            };
        }
    }
    
    return {
        result: originalCode,
        type: 'unknown',
        success: false,
        message: 'Не удалось определить формат ответа',
        errors: ['Unknown response format']
    };
}

export default {
    parseDiffBlocks,
    hasDiffBlocks,
    applyDiffChanges,
    extractCodeFromResponse,
    detectResponseType,
    safelyApplyChanges
};
