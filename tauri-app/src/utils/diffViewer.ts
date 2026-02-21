/**
 * Утилита для применения изменений в формате SEARCH/REPLACE к исходному коду.
 * Позволяет реконструировать полный текст модуля из чанка изменений.
 */
import { diffLines } from 'diff';


interface DiffBlock {
    search: string;
    replace: string;
    lineStart?: number; // Optional hint
    status?: 'pending' | 'confirmed' | 'rejected'; // Статус для UI
    index?: number; // Уникальный индекс в рамках сообщения
    stats?: {
        added: number;
        removed: number;
        modified: number;
    };
}

/**
 * Удаляет блоки кода Markdown (```...```), чтобы их содержимое не парсилось как Diff.
 * Это необходимо для случаев, когда ИИ приводит SEARCH/REPLACE просто как пример кода.
 */
function stripMarkdownCodeBlocks(content: string): string {
    return content.replace(/```[\s\S]*?```/g, '');
}

/**
 * Парсит текст сообщения на блоки изменений
 */
export function parseDiffBlocks(content: string): DiffBlock[] {
    const cleanContent = stripMarkdownCodeBlocks(content);
    const blocks: DiffBlock[] = [];
    const regex = /<<<<<<< SEARCH\s*([\s\S]*?)=======\s*([\s\S]*?)>>>>>>> REPLACE/g;

    let match;
    let index = 0;
    while ((match = regex.exec(cleanContent)) !== null) {
        let search = match[1];
        const replace = match[2];

        // Попытка извлечь метку строки из блока search (:строка:123 или :line:123)
        let lineStart: number | undefined;
        const lineMatch = search.match(/^:(строка|line):(\d+|EOF)\s*-+\s*\n/i);

        if (lineMatch) {
            search = search.substring(lineMatch[0].length);
            if (lineMatch[2] !== 'EOF') {
                lineStart = parseInt(lineMatch[2], 10);
            }
        }

        const searchTrim = search.trim();
        const replaceTrim = replace.replace(/^\n/, '').replace(/\n$/, '');

        // Если блок поиска пустой - это приведет к вставке в начало файла (дублированию кода). 
        // Если ИИ не указал искомый код, мы не сможем его найти. Пропускаем.
        if (!searchTrim) {
            console.warn('Пустой блок SEARCH найден, игнорируем во избежание дублирования');
            continue;
        }

        // Расчет чистой статистики с использованием diffLines
        const dLines = diffLines(searchTrim, replaceTrim, { ignoreWhitespace: false });
        let added = 0;
        let removed = 0;
        let unchanged = 0;

        dLines.forEach(part => {
            const lines = part.value.split('\n').slice(0, part.value.endsWith('\n') ? -1 : undefined).length;
            if (part.added) added += lines;
            else if (part.removed) removed += lines;
            else unchanged += lines;
        });

        // "Modified" считаем как пересечение удаленных и добавленных (где строка была заменена)
        let modified = Math.min(added, removed);
        added -= modified;
        removed -= modified;


        blocks.push({
            search: searchTrim, // Важно: trim для надежности поиска, но может быть опасно для отступов
            replace: replaceTrim, // Чистим лишние переносы от формата
            lineStart,
            status: 'pending',
            index: index++,
            stats: { added, removed, modified }
        });
    }

    return blocks;
}

/**
 * Применяет изменения к исходному коду.
 * Стратегия:
 * 1. Ищет точное совпадение блока SEARCH.
 * 2. Если не найдено -> возвращает как есть с предупреждением.
 * @param originalCode Исходный код
 * @param diffContent Строка с диффами (или массив блоков)
 * @param selectedIndices Массив индексов блоков, которые нужно применить. Если не передан - применяются все.
 */
export function applyDiff(originalCode: string, diffContent: string | DiffBlock[], selectedIndices?: number[]): string {
    if (!originalCode) return typeof diffContent === 'string' ? diffContent : originalCode;

    const blocks = typeof diffContent === 'string' ? parseDiffBlocks(diffContent) : diffContent;
    if (blocks.length === 0) return originalCode;

    let result = originalCode;

    for (let i = 0; i < blocks.length; i++) {
        // Если указан фильтр и текущий блок не выбран - пропускаем
        if (selectedIndices && !selectedIndices.includes(blocks[i].index !== undefined ? blocks[i].index! : i)) {
            continue;
        }

        const block = blocks[i];

        // 1. Точный поиск
        // Нормализуем окончания строк для кросс-платформенности
        const normalizedOriginal = result.replace(/\r\n/g, '\n');
        const normalizedSearch = block.search.replace(/\r\n/g, '\n');

        // Пробуем найти блок
        if (normalizedOriginal.includes(normalizedSearch)) {
            result = normalizedOriginal.replace(normalizedSearch, block.replace);
            continue;
        }

        // Если не нашли - выводим варнинг
        console.warn('Не удалось найти блок для замены (индекс ' + i + '):', block.search);
    }

    return result;
}

/**
 * Проверяет, содержит ли сообщение блоки diff
 */
export function hasDiffBlocks(content: string): boolean {
    const cleanContent = stripMarkdownCodeBlocks(content);
    return /<<<<<<< SEARCH/.test(cleanContent);
}

/**
 * Проверяет, можно ли применить хотя бы один diff-блок к исходному коду.
 * Полезно для фильтрации "примеров кода", которые ИИ пишет текстом.
 */
export function hasApplicableDiffBlocks(originalCode: string, content: string): boolean {
    if (!originalCode) return false;
    const blocks = parseDiffBlocks(content);
    if (blocks.length === 0) return false;

    const normalizedOriginal = originalCode.replace(/\r\n/g, '\n');
    return blocks.some(block => {
        const normalizedSearch = block.search.replace(/\r\n/g, '\n');
        return normalizedSearch && normalizedOriginal.includes(normalizedSearch);
    });
}

/**
 * Очищает сообщение от технических блоков diff для отображения (если нужно скрыть)
 */
export function cleanDiffArtifacts(content: string): string {
    return content.replace(/<<<<<<< SEARCH[\s\S]*?>>>>>>> REPLACE/g, '').trim();
}

/**
 * Обрабатывает ответ ИИ с diff-блоками:
 * 1. Извлекает пояснительный текст (всё, что не является diff-блоком).
 * 2. Применяет изменения к исходному коду.
 * 3. Возвращает отформатированный Markdown: пояснения + полный код.
 */
export function processDiffResponse(originalCode: string, response: string): string {
    // 1. Извлекаем пояснения (удаляем diff-блоки)
    const explanation = cleanDiffArtifacts(response);

    // 2. Применяем изменения к коду
    const modifiedCode = applyDiff(originalCode, response);

    // 3. Формируем итоговый ответ
    let result = '';

    if (explanation) {
        result += explanation + '\n\n';
    }

    // Если код изменился или был передан, добавляем его в блок bsl
    if (modifiedCode) {
        // Добавляем заголовок, если есть пояснения, чтобы разделить контекст
        if (explanation) {
            result += '### Полный код модуля:\n';
        }
        result += '```bsl\n' + modifiedCode + '\n```';
    }

    return result;
}

/**
 * Извлекает "чистый" код для отображения в редакторе.
 * Если есть diff-блоки -> применяет их к контексту.
 * Если есть просто блоки кода -> возвращает их содержимое.
 */
export function extractDisplayCode(originalCode: string, response: string): string | null {
    // 1. Если есть diff-блоки, применяем их
    if (hasDiffBlocks(response)) {
        return applyDiff(originalCode, response);
    }

    // 2. Иначе ищем блоки кода ```bsl или ```1c
    const codeBlockRegex = /```(?:bsl|1c)([\s\S]*?)```/i;
    const match = response.match(codeBlockRegex);
    if (match) {
        return match[1].trim();
    }

    return null;
}

/**
 * Удаляет все блоки кода и diff-блоки из сообщения, оставляя только текст.
 */
export function stripCodeBlocks(content: string): string {
    // 1. Удаляем Diff-блоки
    let stripped = content.replace(/<<<<<<< SEARCH[\s\S]*?>>>>>>> REPLACE/g, '');

    // 2. Удаляем блоки кода
    stripped = stripped.replace(/```(?:bsl|1c)([\s\S]*?)```/gi, '');

    // 3. Чистим лишние переносы
    return stripped.trim();
}
