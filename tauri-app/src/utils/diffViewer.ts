/**
 * Утилита для применения изменений в формате SEARCH/REPLACE к исходному коду.
 * Позволяет реконструировать полный текст модуля из чанка изменений.
 */
import { diffLines } from 'diff';

// ─── Типы ──────────────────────────────────────────────────────────────────────

/** Результат применения одного блока изменений */
export type DiffApplyStatus =
    | 'applied_exact'      // Точное совпадение, применено
    | 'applied_trimmed'    // Совпадение без концевых пробелов, применено
    | 'applied_loose'      // Совпадение без учёта отступов, применено с восстановлением
    | 'applied_fuzzy'      // Нечёткое совпадение, применено с предупреждением
    | 'failed_not_found'   // Блок не найден в исходном коде
    | 'failed_ambiguous'   // Найдено несколько совпадений
    | 'skipped';           // Пропущен (отфильтрован selectedIndices)

export interface DiffBlock {
    search: string;
    replace: string;
    lineStart?: number;
    status?: 'pending' | 'confirmed' | 'rejected';
    applyStatus?: DiffApplyStatus;
    applyError?: string;   // Человекочитаемая причина неудачи
    appliedAt?: number;    // Номер строки (1-based), где применён
    index?: number;
    stats?: {
        added: number;
        removed: number;
        modified: number;
    };
}

/** Итог применения всех блоков */
export interface DiffApplyResult {
    code: string;
    blocks: DiffBlock[];
    /** Кол-во блоков, которые не удалось применить */
    failedCount: number;
    /** Кол-во блоков, применённых нечётко (с предупреждением) */
    fuzzyCount: number;
}

// ─── Вспомогательные функции ───────────────────────────────────────────────────

/** Вычисляет схожесть двух строк: 0 = разные, 1 = идентичны */
function stringSimilarity(a: string, b: string): number {
    if (a === b) return 1;
    if (!a || !b) return 0;
    const maxLen = Math.max(a.length, b.length);
    let matches = 0;
    const shorter = a.length < b.length ? a : b;
    const longer = a.length < b.length ? b : a;
    for (let i = 0; i < shorter.length; i++) {
        if (shorter[i] === longer[i]) matches++;
    }
    return matches / maxLen;
}

/** Считает схожесть двух блоков строк как среднее по строкам */
function blockSimilarity(aLines: string[], bLines: string[]): number {
    if (aLines.length !== bLines.length) return 0;
    const sims = aLines.map((l, i) => stringSimilarity(l.trim(), bLines[i].trim()));
    return sims.reduce((s, v) => s + v, 0) / sims.length;
}

/** Восстанавливает отступы в заменяемом тексте по образцу первой строки оригинала */
function restoreIndent(originalFirstLine: string, replaceText: string): string {
    const indent = originalFirstLine.match(/^\s*/)?.[0] ?? '';
    if (!indent) return replaceText;
    return replaceText.split('\n')
        .map((line, idx) => {
            if (idx === 0 && !line.startsWith(indent)) return indent + line.trimStart();
            if (idx > 0 && line.trim() && !line.startsWith(indent)) return indent + line.trimStart();
            return line;
        })
        .join('\n');
}

/** Критичность схожести для fuzzy-принятия */
const FUZZY_THRESHOLD = 0.85;

// ─── Создание блока ────────────────────────────────────────────────────────────

function createBlock(searchLines: string[], replaceLines: string[], index: number): DiffBlock {
    let search = searchLines.join('\n');
    let replace = replaceLines.join('\n');

    let lineStart: number | undefined;
    const lineMatch = search.match(/^:(строка|line):(\d+|EOF)\s*-+\s*\n/i);
    if (lineMatch) {
        search = search.substring(lineMatch[0].length);
        if (lineMatch[2] !== 'EOF') lineStart = parseInt(lineMatch[2], 10);
    }

    const dLines = diffLines(search.trim(), replace.trim(), { ignoreWhitespace: false });
    let added = 0, removed = 0;
    dLines.forEach(part => {
        const count = part.value.split('\n').filter(l => l.length > 0).length;
        if (part.added) added += count;
        else if (part.removed) removed += count;
    });
    const modified = Math.min(added, removed);

    return {
        search,
        replace,
        lineStart,
        status: 'pending',
        index,
        stats: { added: added - modified, removed: removed - modified, modified }
    };
}

// ─── Парсинг ───────────────────────────────────────────────────────────────────

/**
 * Парсит текст сообщения на блоки изменений с поддержкой незавершенных блоков.
 */
export function parseDiffBlocks(content: string): DiffBlock[] {
    // Normalize CRLF → LF so that the regex \n? correctly strips the newline
    // after <search>/<replace> tags (otherwise the \r becomes the first char
    // of the captured content, creating a spurious leading empty line)
    content = content.replace(/\r\n/g, '\n');

    const blocks: DiffBlock[] = [];
    let index = 0;

    // Парсим XML-формат (<diff><search>...</search><replace>...</replace></diff>)
    const xmlRegex = /<diff(?:\s+[^>]*)?\>\s*<search(?:\s+[^>]*)?\>\n?([\s\S]*?)\n?[ \t]*<\/search>\s*<replace(?:\s+[^>]*)?\>\n?([\s\S]*?)\n?[ \t]*<\/replace>\s*<\/diff>/g;
    let xmlMatch;
    while ((xmlMatch = xmlRegex.exec(content)) !== null) {
        blocks.push(createBlock(xmlMatch[1].split('\n'), xmlMatch[2].split('\n'), index++));
    }

    // Парсим SEARCH/REPLACE формат (legacy)
    const legacyContent = content.replace(xmlRegex, '');
    const lines = legacyContent.split('\n');
    let mode: 'none' | 'search' | 'replace' = 'none';
    let searchLines: string[] = [];
    let replaceLines: string[] = [];

    for (const line of lines) {
        const trimmed = line.trim();

        if (trimmed.startsWith('<<<<<<< SEARCH')) {
            if (mode === 'replace' && (searchLines.length > 0 || replaceLines.length > 0)) {
                blocks.push(createBlock(searchLines, replaceLines, index++));
            }
            mode = 'search'; searchLines = []; replaceLines = [];
            continue;
        }
        if (trimmed === '=======') {
            if (mode === 'search') mode = 'replace';
            continue;
        }
        if (trimmed.startsWith('>>>>>>> REPLACE')) {
            if (mode === 'replace') {
                blocks.push(createBlock(searchLines, replaceLines, index++));
                mode = 'none'; searchLines = []; replaceLines = [];
            }
            continue;
        }

        if (mode === 'search') searchLines.push(line);
        else if (mode === 'replace') replaceLines.push(line);
    }

    if (mode === 'replace' && (searchLines.length > 0 || replaceLines.length > 0)) {
        blocks.push(createBlock(searchLines, replaceLines, index++));
    }

    return blocks;
}

// ─── Применение одного блока ───────────────────────────────────────────────────

/**
 * Пытается применить один блок к коду, используя все доступные стратегии.
 * Возвращает изменённый код и обновлённый блок со статусом.
 */
function applyBlock(code: string, block: DiffBlock): { code: string; block: DiffBlock } {
    const nSearch = block.search.replace(/\r\n/g, '\n');
    const nReplace = block.replace.replace(/\r\n/g, '\n');
    const originalLines = code.split('\n');
    const searchLines = nSearch.split('\n');
    // Убираем хвостовые пустые строки из SEARCH (ИИ часто ставит пустую строку перед </search>)
    while (searchLines.length > 0 && searchLines[searchLines.length - 1].trim() === '') {
        searchLines.pop();
    }

    // ── Стратегия 1: Точное совпадение ────────────────────────────────────────
    if (code.includes(nSearch)) {
        // Проверяем, единственное ли совпадение
        const occurrences = code.split(nSearch).length - 1;
        if (occurrences > 1) {
            return {
                code,
                block: {
                    ...block,
                    applyStatus: 'failed_ambiguous',
                    applyError: `Найдено ${occurrences} идентичных вхождения. Уточните контекст.`
                }
            };
        }
        const lineIdx = code.substring(0, code.indexOf(nSearch)).split('\n').length;
        return {
            code: code.replace(nSearch, nReplace),
            block: { ...block, applyStatus: 'applied_exact', appliedAt: lineIdx }
        };
    }

    // ── Стратегия 2: Без концевых пробелов ────────────────────────────────────
    for (let i = 0; i <= originalLines.length - searchLines.length; i++) {
        let match = true;
        for (let j = 0; j < searchLines.length; j++) {
            if (originalLines[i + j].trimEnd() !== searchLines[j].trimEnd()) { match = false; break; }
        }
        if (match) {
            let finalReplace = nReplace;
            // Если замена однострочная и без отступа — восстанавливаем
            if (searchLines.length === 1 && !nReplace.startsWith(' ') && !nReplace.startsWith('\t')) {
                finalReplace = restoreIndent(originalLines[i], nReplace);
            }
            const result = [...originalLines.slice(0, i), finalReplace, ...originalLines.slice(i + searchLines.length)].join('\n');
            return { code: result, block: { ...block, applyStatus: 'applied_trimmed', appliedAt: i + 1 } };
        }
    }

    // ── Стратегия 3: Без учёта отступов (loose) ───────────────────────────────
    const norm = (l: string) => l.trim();
    const looseSearch = searchLines.map(norm);
    const looseOriginal = originalLines.map(norm);

    for (let i = 0; i <= looseOriginal.length - searchLines.length; i++) {
        let match = true;
        for (let j = 0; j < searchLines.length; j++) {
            if (looseOriginal[i + j] !== looseSearch[j]) { match = false; break; }
        }
        if (match) {
            const finalReplace = restoreIndent(originalLines[i], nReplace);
            const result = [...originalLines.slice(0, i), finalReplace, ...originalLines.slice(i + searchLines.length)].join('\n');
            console.log(`[applyDiff] loose-match на строке ${i + 1}, отступ восстановлен`);
            return { code: result, block: { ...block, applyStatus: 'applied_loose', appliedAt: i + 1 } };
        }
    }

    // ── Стратегия 4: Fuzzy matching ───────────────────────────────────────────
    // Ищем окно с наибольшей схожестью
    let bestScore = 0;
    let bestIdx = -1;

    for (let i = 0; i <= originalLines.length - searchLines.length; i++) {
        const windowLines = originalLines.slice(i, i + searchLines.length);
        const score = blockSimilarity(windowLines, searchLines);
        if (score > bestScore) {
            bestScore = score;
            bestIdx = i;
        }
    }

    if (bestScore >= FUZZY_THRESHOLD && bestIdx >= 0) {
        const finalReplace = restoreIndent(originalLines[bestIdx], nReplace);
        const result = [...originalLines.slice(0, bestIdx), finalReplace, ...originalLines.slice(bestIdx + searchLines.length)].join('\n');
        console.warn(`[applyDiff] fuzzy-match на строке ${bestIdx + 1}, схожесть ${(bestScore * 100).toFixed(0)}%`);
        return {
            code: result,
            block: {
                ...block,
                applyStatus: 'applied_fuzzy',
                appliedAt: bestIdx + 1,
                applyError: `Применено через нечёткое совпадение (схожесть ${(bestScore * 100).toFixed(0)}%). Проверьте результат.`
            }
        };
    }

    // ── Провал: блок не найден ─────────────────────────────────────────────────
    const searchPreview = searchLines[0]?.trim().substring(0, 60) ?? '';
    return {
        code,
        block: {
            ...block,
            applyStatus: 'failed_not_found',
            applyError: `Блок не найден в исходном коде. Начало поиска: "${searchPreview}..."`
        }
    };
}

// ─── Публичное API ─────────────────────────────────────────────────────────────

/**
 * Применяет изменения к коду и возвращает подробный результат с диагностикой.
 */
export function applyDiffWithDiagnostics(
    originalCode: string,
    diffContent: string | DiffBlock[],
    selectedIndices?: number[]
): DiffApplyResult {
    const blocks = typeof diffContent === 'string' ? parseDiffBlocks(diffContent) : [...diffContent];
    const useCRLF = originalCode.includes('\r\n');
    let code = originalCode.replace(/\r\n/g, '\n');
    const resultBlocks: DiffBlock[] = [];
    let failedCount = 0;
    let fuzzyCount = 0;

    for (let i = 0; i < blocks.length; i++) {
        const block = blocks[i];
        const effectiveIndex = block.index ?? i;

        if (selectedIndices && !selectedIndices.includes(effectiveIndex)) {
            resultBlocks.push({ ...block, applyStatus: 'skipped' });
            continue;
        }

        const result = applyBlock(code, block);
        code = result.code;
        resultBlocks.push(result.block);

        if (result.block.applyStatus === 'failed_not_found' || result.block.applyStatus === 'failed_ambiguous') {
            failedCount++;
        } else if (result.block.applyStatus === 'applied_fuzzy') {
            fuzzyCount++;
        }
    }

    // Убираем тройные пустые строки
    code = code.replace(/\n{3,}/g, '\n\n');
    if (useCRLF) code = code.replace(/\n/g, '\r\n');

    return { code, blocks: resultBlocks, failedCount, fuzzyCount };
}

/**
 * Упрощённый вариант (обратная совместимость) — возвращает только строку кода.
 */
export function applyDiff(originalCode: string, diffContent: string | DiffBlock[], selectedIndices?: number[]): string {
    if (!originalCode) return typeof diffContent === 'string' ? diffContent : originalCode;
    const result = applyDiffWithDiagnostics(originalCode, diffContent, selectedIndices);
    return result.code;
}

/**
 * Возвращает список блоков, которые не удалось применить.
 */
export function getDiffDiagnostics(result: DiffApplyResult): DiffBlock[] {
    return result.blocks.filter(b =>
        b.applyStatus === 'failed_not_found' ||
        b.applyStatus === 'failed_ambiguous' ||
        b.applyStatus === 'applied_fuzzy'
    );
}

/**
 * Формирует читаемое сообщение об ошибках применения для отображения в чате.
 */
export function formatDiffErrorMessage(result: DiffApplyResult): string | null {
    if (result.failedCount === 0 && result.fuzzyCount === 0) return null;

    const lines: string[] = [];

    if (result.failedCount > 0) {
        lines.push(`⚠️ **${result.failedCount} из ${result.blocks.length} блоков изменений не применены:**`);
        result.blocks
            .filter(b => b.applyStatus === 'failed_not_found' || b.applyStatus === 'failed_ambiguous')
            .forEach((b, i) => {
                const preview = b.search.trim().split('\n')[0].substring(0, 70);
                lines.push(`  ${i + 1}. ${b.applyError ?? 'Неизвестная ошибка'} \`${preview}\``);
            });
    }

    if (result.fuzzyCount > 0) {
        lines.push(`⚡ **${result.fuzzyCount} блок(а/ов) применены приблизительно (проверьте результат).**`);
    }

    return lines.join('\n');
}

// ─── Вспомогательные экспорты (обратная совместимость) ────────────────────────

/** Проверяет, содержит ли сообщение блоки diff */
export function hasDiffBlocks(content: string): boolean {
    return /<<<<<<< SEARCH/.test(content) || /<diff>/.test(content);
}

/** Проверяет, можно ли применить хотя бы один дифф-блок к исходному коду */
export function hasApplicableDiffBlocks(originalCode: string, content: string): boolean {
    if (!originalCode) return false;
    const blocks = parseDiffBlocks(content);
    if (blocks.length === 0) return false;

    const test = originalCode.replace(/\r\n/g, '\n');
    return blocks.some(block => {
        const ns = block.search.replace(/\r\n/g, '\n');
        if (test.includes(ns)) return true;
        const looseS = ns.split('\n').map(l => l.trim()).join('\n');
        const looseO = test.split('\n').map(l => l.trim()).join('\n');
        return looseO.includes(looseS);
    });
}

/** Очищает сообщение от технических блоков diff */
export function cleanDiffArtifacts(content: string): string {
    let cleaned = content.replace(/<<<<<<< SEARCH[\s\S]*?>>>>>>> REPLACE/g, '');
    cleaned = cleaned.replace(/<<<<<<< SEARCH[\s\S]*?=======[\s\S]*?(?:\n|$)/g, '');
    cleaned = cleaned.replace(/<diff>[\s\S]*?<\/diff>/g, '');
    cleaned = cleaned.replace(/<diff>[\s\S]*?(?:\n|$)/g, '');
    return cleaned.trim();
}

/** Обрабатывает ответ ИИ с diff-блоками: применяет изменения и возвращает Markdown */
export function processDiffResponse(originalCode: string, response: string): string {
    const explanation = cleanDiffArtifacts(response);
    const modifiedCode = applyDiff(originalCode, response);
    let result = '';
    if (explanation) result += explanation + '\n\n';
    if (modifiedCode) {
        if (explanation) result += '### Полный код модуля:\n';
        result += '```bsl\n' + modifiedCode + '\n```';
    }
    return result;
}

/** Извлекает код для отображения в редакторе */
export function extractDisplayCode(originalCode: string, response: string): string | null {
    if (hasDiffBlocks(response)) return applyDiff(originalCode, response);
    const match = response.match(/```(?:bsl|1c)([\s\S]*?)```/i);
    return match ? match[1].trim() : null;
}

/** Удаляет все блоки кода и diff-блоки, оставляя только текст */
export function stripCodeBlocks(content: string): string {
    let s = content.replace(/<<<<<<< SEARCH[\s\S]*?>>>>>>> REPLACE/g, '');
    s = s.replace(/<<<<<<< SEARCH[\s\S]*?=======[\s\S]*?(?:\n|$)/g, '');
    s = s.replace(/<diff>[\s\S]*?<\/diff>/g, '');
    s = s.replace(/<diff>[\s\S]*?(?:\n|$)/g, '');
    s = s.replace(/```(?:bsl|1c)([\s\S]*?)```/gi, '');
    return s.trim();
}
