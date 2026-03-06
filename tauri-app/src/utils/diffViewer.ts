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
    | 'applied_ws'         // Совпадение без всех пробелов (whitespace-ignored), применено
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

/**
 * Расстояние Левенштейна между двумя строками (две строки DP, O(m*n)).
 * Для больших строк усекает до MAX_LEV_LEN для производительности.
 */
function levenshteinDistance(a: string, b: string): number {
    if (a === b) return 0;
    if (!a) return b.length;
    if (!b) return a.length;

    // We compute full distance. For performance, we assume it's only called when strings are somewhat similar
    const m = a.length, n = b.length;
    let prev = Array.from({ length: n + 1 }, (_, i) => i);
    let curr = new Array(n + 1).fill(0);

    for (let i = 1; i <= m; i++) {
        curr[0] = i;
        for (let j = 1; j <= n; j++) {
            curr[j] = a[i - 1] === b[j - 1]
                ? prev[j - 1]
                : 1 + Math.min(prev[j], curr[j - 1], prev[j - 1]);
        }
        [prev, curr] = [curr, prev];
    }
    return prev[n];
}

/** Вычисляет схожесть двух строк через расстояние Левенштейна: 0 = разные, 1 = идентичны */
function stringSimilarity(a: string, b: string): number {
    const maxLen = Math.max(a.length, b.length);
    if (maxLen === 0) return 1;
    return 1 - levenshteinDistance(a, b) / maxLen;
}

function blockSimilarity(aLines: string[], bLines: string[]): number {
    const aTrimmed = aLines.map(l => l.trim());
    const bTrimmed = bLines.map(l => l.trim());
    const m = aTrimmed.length, n = bTrimmed.length;
    if (m === 0 && n === 0) return 1;
    if (m === 0 || n === 0) return 0;

    // Quick heuristic: count matching lines (ignoring order)
    let matchLines = 0;
    const bSet = new Set(bTrimmed.filter(l => l.length > 0));
    let aNonEmpty = 0;
    for (const line of aTrimmed) {
        if (line.length > 0) {
            aNonEmpty++;
            if (bSet.has(line)) matchLines++;
        }
    }
    const maxNonEmpty = Math.max(aNonEmpty, bSet.size);
    // If less than 40% of lines match exactly, skip full Levenshtein
    if (maxNonEmpty > 0 && matchLines / maxNonEmpty < 0.4) {
        return 0;
    }

    const aText = aTrimmed.join('\n');
    const bText = bTrimmed.join('\n');
    return stringSimilarity(aText, bText);
}

/**
 * Восстанавливает относительные отступы в replace-тексте по образцу оригинала.
 * Сохраняет относительное вложение всех строк (как в Roo-Code).
 */
function restoreIndent(
    originalMatchedLines: string[],
    searchLines: string[],
    replaceText: string,
    precedingIndent?: string
): string {
    const replaceLines = replaceText.split('\n');
    let matchedBaseIndent = '';
    let searchBaseIndent = '';

    // Ищем первую непустую строку для определения базового отступа оригинала
    for (const line of originalMatchedLines) {
        if (line.trim()) {
            matchedBaseIndent = (line.match(/^[\t ]*/) ?? [''])[0];
            break;
        }
    }

    // Ищем первую непустую строку для определения базового отступа поиска
    for (const line of searchLines) {
        if (line.trim()) {
            searchBaseIndent = (line.match(/^[\t ]*/) ?? [''])[0];
            break;
        }
    }
    const searchBaseLevel = searchBaseIndent.length;

    // Ищем первую непустую строку для определения базового отступа замены
    let replaceBaseIndent = '';
    for (const line of replaceLines) {
        if (line.trim()) {
            replaceBaseIndent = (line.match(/^[\t ]*/) ?? [''])[0];
            break;
        }
    }
    const replaceBaseLevel = replaceBaseIndent.length;

    // Эвристика: если AI предлагает замену с МЕНЬШИМ отступом чем в поиске (replaceBaseLevel < searchBaseLevel)
    // ИЛИ если замена плоская (replaceBaseLevel === 0), а в оригинале был отступ,
    // и это начало топ-левел конструкции 1С — сбрасываем отступ.
    let effectiveBaseIndent = matchedBaseIndent;

    const firstReplaceLine = replaceLines.find(l => l.trim().length > 0) || '';
    const trimmedFirst = firstReplaceLine.trim();
    const isTopLevelPattern = /^(Процедура|Функция|\/\/|&|#|Если|Для|Пока|Попытка|Перем|КонецПроцедуры|КонецФункции)/i.test(trimmedFirst);

    // Агрессивная де-индентация: 
    // Если AI предлагает замену (плоскую или с тем же отступом), но мы видим, что это 
    // топ-левел конструкция (Процедура, КонецПроцедуры и т.д.), мы СБРАСЫВАЕМ отступ в 0,
    // если это поможет выровнять код по левому краю или по precedingIndent.
    if (isTopLevelPattern) {
        effectiveBaseIndent = '';
    } else if (replaceBaseLevel < searchBaseLevel || (replaceBaseLevel === 0 && searchBaseLevel === 0)) {
        if (matchedBaseIndent.length > (precedingIndent?.length ?? 0)) {
            effectiveBaseIndent = precedingIndent ?? '';
        }
    }

    return replaceLines.map(line => {
        if (!line.trim()) return line;

        const currentIndentStr = (line.match(/^[\t ]*/) ?? [''])[0];
        const currentIndentLevel = currentIndentStr.replace(/ {4}/g, '\t').length;

        // Как отступ этой строки соотносится с базовым отступом блока в REPLACE?
        const relativeLevel = currentIndentLevel - replaceBaseLevel;

        let resIndent = effectiveBaseIndent;
        if (relativeLevel > 0) {
            resIndent += '\t'.repeat(relativeLevel);
        } else if (relativeLevel < 0) {
            // Если относительный уровень отрицательный, нам нужно УМЕНЬШИТЬ отступ оригинала
            const absRelative = Math.abs(relativeLevel);
            // Пытаемся удалить табы с конца базового отступа
            for (let k = 0; k < absRelative; k++) {
                if (resIndent.endsWith('\t') || resIndent.endsWith(' ')) {
                    resIndent = resIndent.slice(0, -1);
                }
            }
        }

        return resIndent + line.trim();
    }).join('\n');
}

/** Старый вариант restoreIndent для однострочных замен без контекста */
function restoreIndentSimple(originalFirstLine: string, replaceText: string): string {
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

/**
 * Стратегия whitespace-ignored: удаляет все пробельные символы и ищет совпадение.
 * Возвращает символьные позиции в оригинальном тексте или null.
 * Адаптировано из Continue.dev findSearchMatch.ts.
 */
function findWhitespaceIgnored(
    code: string,
    nSearch: string
): { startChar: number; endChar: number } | null {
    const strip = (s: string) => s.replace(/\s/g, '');
    const strippedCode = strip(code);
    const strippedSearch = strip(nSearch);
    if (!strippedSearch) return null;

    const idx = strippedCode.indexOf(strippedSearch);
    if (idx === -1) return null;

    // Строим маппинг: stripped_index → original_index
    const strippedToOrig: number[] = [];
    for (let i = 0; i < code.length; i++) {
        if (!/\s/.test(code[i])) strippedToOrig.push(i);
    }

    const endStrippedIdx = idx + strippedSearch.length - 1;
    if (idx >= strippedToOrig.length || endStrippedIdx >= strippedToOrig.length) return null;

    const startChar = strippedToOrig[idx];
    const endChar = strippedToOrig[endStrippedIdx] + 1;
    return { startChar, endChar };
}

/**
 * Стратегия dot-dot-dots: обрабатывает `...` как "пропустить строки" в SEARCH-блоке.
 * Адаптировано из Aider editblock_coder.py.
 */
function tryDotDotDots(code: string, search: string, replace: string): string | null {
    const dotLineRe = /^[ \t]*\.\.\.[ \t]*$/m;
    if (!dotLineRe.test(search)) return null;

    const splitDots = (s: string) => s.split(/^[ \t]*\.\.\.[ \t]*\r?\n?/m);
    const searchParts = splitDots(search).filter(p => p.trim());
    const replaceParts = splitDots(replace).filter(p => p.trim());

    if (searchParts.length <= 1) return null;
    if (searchParts.length !== replaceParts.length) return null;

    let result = code;
    for (let k = 0; k < searchParts.length; k++) {
        const sp = searchParts[k];
        const rp = replaceParts[k];
        const occurrences = result.split(sp).length - 1;
        if (occurrences !== 1) return null; // неоднозначно
        result = result.replace(sp, rp);
    }
    return result;
}

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
    // Normalize CRLF → LF
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
    // Поддерживаем 5-9 символов chevron и лишний > в маркерах (Claude Sonnet 4 иногда добавляет)
    const legacyContent = content.replace(xmlRegex, '');
    const lines = legacyContent.split('\n');
    let mode: 'none' | 'search' | 'replace' = 'none';
    let searchLines: string[] = [];
    let replaceLines: string[] = [];

    for (const line of lines) {
        const trimmed = line.trim();

        if (/^<{5,9} SEARCH>?\s*$/.test(trimmed)) {
            if (mode === 'replace' && (searchLines.length > 0 || replaceLines.length > 0)) {
                blocks.push(createBlock(searchLines, replaceLines, index++));
            }
            mode = 'search'; searchLines = []; replaceLines = [];
            continue;
        }
        if (/^={5,9}\s*$/.test(trimmed)) {
            if (mode === 'search') mode = 'replace';
            continue;
        }
        if (/^>{5,9} REPLACE>?\s*$/.test(trimmed)) {
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
function applyBlock(
    code: string,
    block: DiffBlock,
    usedLineRanges: Array<[number, number]> = []
): { code: string; block: DiffBlock; appliedRange?: [number, number] } {
    const nSearch = block.search.replace(/\r\n/g, '\n');
    let replaceLines = block.replace.replace(/\r\n/g, '\n').split('\n');
    if (replaceLines.length > 0 && replaceLines[replaceLines.length - 1] === '') {
        replaceLines.pop();
    }
    const nReplace = replaceLines.join('\n');

    // ── Пустой SEARCH = вставить в начало файла ────────────────────────────────
    if (nSearch.trim() === '') {
        return {
            code: nReplace + (code ? '\n' + code : ''),
            block: { ...block, applyStatus: 'applied_exact', appliedAt: 1 }
        };
    }

    const originalLines = code.split('\n');
    const searchLines = nSearch.split('\n');
    // Убираем хвостовые пустые строки из SEARCH (ИИ часто ставит пустую строку перед </search>)
    let poppedCount = 0;
    while (searchLines.length > 0 && searchLines[searchLines.length - 1].trim() === '') {
        searchLines.pop();
        poppedCount++;
    }

    // Симметрично убираем такие же пустые строки из REPLACE, чтобы не вставлять лишние \n
    while (poppedCount > 0 && replaceLines.length > 0 && replaceLines[replaceLines.length - 1].trim() === '') {
        replaceLines.pop();
        poppedCount--;
    }

    // Пересчитываем nReplace для корректных подстановок
    const finalNReplace = replaceLines.join('\n');

    if (searchLines.length === 0) {
        return {
            code: finalNReplace + (code ? '\n' + code : ''),
            block: { ...block, applyStatus: 'applied_exact', appliedAt: 1 }
        };
    }

    // ── Стратегия 0: Dot-dot-dots (`...` в SEARCH) ────────────────────────────
    const dotsResult = tryDotDotDots(code, nSearch, finalNReplace);
    if (dotsResult !== null) {
        const lineIdx = code.substring(0, code.indexOf(searchLines[0])).split('\n').length;
        return { code: dotsResult, block: { ...block, applyStatus: 'applied_exact', appliedAt: lineIdx } };
    }

    // ── Стратегия 1: Точное совпадение ────────────────────────────────────────
    const cleanSearch = searchLines.join('\n');
    if (code.includes(cleanSearch)) {
        const occurrences = code.split(cleanSearch).length - 1;
        if (occurrences > 1) {
            // Disambiguation by replace pattern:
            // - replace starts with search → "append after last" pattern → use LAST occurrence
            // - replace ends with search   → "prepend before first" pattern → use FIRST occurrence
            const replaceStartsWithSearch = finalNReplace.startsWith(cleanSearch + '\n') || finalNReplace === cleanSearch;
            const replaceEndsWithSearch   = finalNReplace.endsWith('\n' + cleanSearch);

            let targetIdx: number;
            if (replaceStartsWithSearch) {
                targetIdx = code.lastIndexOf(cleanSearch);
            } else if (replaceEndsWithSearch) {
                targetIdx = code.indexOf(cleanSearch);
            } else {
                return {
                    code,
                    block: {
                        ...block,
                        applyStatus: 'failed_ambiguous',
                        applyError: `Найдено ${occurrences} идентичных вхождения. Уточните контекст.`
                    }
                };
            }

            const before = code.substring(0, targetIdx);
            const after  = code.substring(targetIdx + cleanSearch.length);
            const cleanReplaceForInsert = finalNReplace.replace(/\r/g, '');
            const newCode = before + cleanReplaceForInsert + after;
            const firstLineIdx = before.split('\n').length;
            const replacedLinesCount = finalNReplace.split('\n').length;

            return {
                code: newCode,
                block: { ...block, applyStatus: 'applied_exact', appliedAt: firstLineIdx },
                appliedRange: [firstLineIdx - 1, firstLineIdx - 1 + replacedLinesCount]
            };
        }
        const firstLineIdx = code.substring(0, code.indexOf(cleanSearch)).split('\n').length;
        const replacedLinesCount = finalNReplace.split('\n').length;
        // Убираем возможные артефакты \r при вставке
        const cleanReplaceForInsert = finalNReplace.replace(/\r/g, '');
        const newCode = code.replace(cleanSearch, cleanReplaceForInsert);

        return {
            code: newCode,
            block: { ...block, applyStatus: 'applied_exact', appliedAt: firstLineIdx },
            appliedRange: [firstLineIdx - 1, firstLineIdx - 1 + replacedLinesCount]
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
            if (searchLines.length === 1 && !nReplace.startsWith(' ') && !nReplace.startsWith('\t')) {
                finalReplace = restoreIndentSimple(originalLines[i], nReplace);
            }
            const result = [...originalLines.slice(0, i), finalReplace, ...originalLines.slice(i + searchLines.length)].join('\n');
            return {
                code: result,
                block: { ...block, applyStatus: 'applied_trimmed', appliedAt: i + 1 },
                appliedRange: [i, i + finalReplace.split('\n').length]
            };
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
            const precedingLine = i > 0 ? (originalLines.slice(0, i).reverse().find(l => l.trim().length > 0) || '') : '';
            const precedingIndent = (precedingLine.match(/^[\t ]*/) ?? [''])[0];
            const finalReplace = restoreIndent(originalLines.slice(i, i + searchLines.length), searchLines, nReplace, precedingIndent);
            const result = [...originalLines.slice(0, i), finalReplace, ...originalLines.slice(i + searchLines.length)].join('\n');
            return {
                code: result,
                block: { ...block, applyStatus: 'applied_loose', appliedAt: i + 1 },
                appliedRange: [i, i + finalReplace.split('\n').length]
            };
        }
    }


    // ── Стратегия 3.5: Whitespace-ignored ─────────────────────────────────────
    // Удаляем ВСЕ пробельные символы и маппим позицию обратно (из Continue.dev)
    const wsResult = findWhitespaceIgnored(code, cleanSearch);
    if (wsResult) {
        const before = code.substring(0, wsResult.startChar);
        const after = code.substring(wsResult.endChar);
        const newCode = before + nReplace + after;
        const lineIdx = before.split('\n').length;
        return { code: newCode, block: { ...block, applyStatus: 'applied_ws', appliedAt: lineIdx } };
    }

    // ── Стратегия 4: Fuzzy matching (переменное окно ±15%, Левенштейн) ────────
    // Адаптировано из Aider (переменное окно) + Roo-Code (Левенштейн)
    const scale = 0.15;
    const minLen = Math.max(1, Math.floor(searchLines.length * (1 - scale)));
    const maxLen = Math.ceil(searchLines.length * (1 + scale));

    let bestScore = 0;
    let bestIdx = -1;
    let bestLen = searchLines.length;

    for (let len = minLen; len <= maxLen; len++) {
        for (let i = 0; i <= originalLines.length - len; i++) {
            // Пропускаем окна, пересекающиеся с уже применёнными диапазонами
            const overlaps = usedLineRanges.some(([start, end]) =>
                i < end && (i + len) > start
            );
            if (overlaps) continue;

            const windowLines = originalLines.slice(i, i + len);
            const score = blockSimilarity(windowLines, searchLines);

            if (score > bestScore) {
                bestScore = score;
                bestIdx = i;
                bestLen = len;
            }
        }
    }

    if (bestScore >= FUZZY_THRESHOLD && bestIdx >= 0) {
        const matchedLines = originalLines.slice(bestIdx, bestIdx + bestLen);
        const finalReplace = restoreIndent(matchedLines, searchLines, nReplace);
        const result = [...originalLines.slice(0, bestIdx), finalReplace, ...originalLines.slice(bestIdx + bestLen)].join('\n');
        return {
            code: result,
            block: {
                ...block,
                applyStatus: 'applied_fuzzy',
                appliedAt: bestIdx + 1,
                applyError: `Применено через нечёткое совпадение (схожесть ${(bestScore * 100).toFixed(0)}%). Проверьте результат.`
            },
            appliedRange: [bestIdx, bestIdx + finalReplace.split('\n').length]
        };
    }

    // ── Провал: блок не найден ─────────────────────────────────────────────────
    const searchPreview = searchLines[0]?.trim().substring(0, 60) ?? '';
    // Подсказка: ищем наиболее похожую строку (как в Aider "did you mean")
    let hint = '';
    if (searchLines[0]?.trim()) {
        let hintBest = 0;
        let hintLine = '';
        for (const line of originalLines) {
            const s = stringSimilarity(line.trim(), searchLines[0].trim());
            if (s > hintBest && s > 0.5) { hintBest = s; hintLine = line.trim(); }
        }
        if (hintLine) hint = ` Похожая строка: "${hintLine.substring(0, 50)}"`;
    }

    return {
        code,
        block: {
            ...block,
            applyStatus: 'failed_not_found',
            applyError: `Блок не найден в исходном коде. Начало поиска: "${searchPreview}"${hint}`
        }
    };
}

// ─── Объединение перекрывающихся диффов ────────────────────────────────────────

/**
 * Обнаруживает и объединяет диффы с перекрывающимися SEARCH-блоками.
 * Когда несколько диффов модифицируют одну и ту же строку, их нужно объединить
 * в каскадную цепочку: результат первого становится SEARCH для второго.
 */
function mergeOverlappingBlocks(blocks: DiffBlock[]): DiffBlock[] {
    if (blocks.length <= 1) return blocks;

    // Группируем диффы по их SEARCH-блоку (нормализованному)
    const searchToIndices = new Map<string, number[]>();

    const normalizeSearch = (s: string): string => {
        // Нормализуем для сравнения: убираем пробелы в начале/конце строк
        return s.replace(/\r\n/g, '\n')
            .split('\n')
            .map(l => l.trim())
            .filter(l => l.length > 0)
            .join('\n')
            .toLowerCase();
    };

    blocks.forEach((block, idx) => {
        const key = normalizeSearch(block.search);
        if (!searchToIndices.has(key)) {
            searchToIndices.set(key, []);
        }
        searchToIndices.get(key)!.push(idx);
    });

    // Если нет дублирующихся SEARCH-блоков, возвращаем как есть
    const hasOverlaps = Array.from(searchToIndices.values()).some(indices => indices.length > 1);
    if (!hasOverlaps) return blocks;

    // Строим цепочки зависимых диффов
    // Для каждой группы с одинаковым SEARCH - объединяем в каскад
    const processedIndices = new Set<number>();
    const result: DiffBlock[] = [];

    for (let i = 0; i < blocks.length; i++) {
        if (processedIndices.has(i)) continue;

        const block = blocks[i];
        const key = normalizeSearch(block.search);
        const groupIndices = searchToIndices.get(key) || [i];

        if (groupIndices.length === 1) {
            // Одиночный дифф - добавляем как есть
            result.push(block);
            processedIndices.add(i);
        } else {
            // Несколько диффов с одинаковым SEARCH - объединяем в каскад
            // Сортируем по индексу для сохранения порядка
            const sortedIndices = [...groupIndices].sort((a, b) => a - b);

            let currentSearch = block.search;
            let currentReplace = block.replace;
            let combinedStats = block.stats ? { ...block.stats } : { added: 0, removed: 0, modified: 0 };

            // Применяем каждый последующий дифф к результату предыдущего
            for (let j = 1; j < sortedIndices.length; j++) {
                const nextIdx = sortedIndices[j];
                const nextBlock = blocks[nextIdx];
                processedIndices.add(nextIdx);

                // Каскад: SEARCH следующего должен найтись в REPLACE текущего
                // Если не находится - пробуем применить к текущему SEARCH (оригиналу)
                let tempReplace = currentReplace.replace(currentSearch, nextBlock.replace);

                // Если замена не произошла, значит следующий дифф модифицирует оригинальный SEARCH
                // В этом случае его REPLACE нужно применить к текущему REPLACE
                if (tempReplace === currentReplace) {
                    // Пробуем найти что именно меняет следующий дифф
                    // и применить это изменение к текущему результату
                    const searchDiff = findSearchDifference(currentSearch, nextBlock.search, nextBlock.replace);
                    if (searchDiff) {
                        tempReplace = applyPartialChange(currentReplace, searchDiff);
                    }
                }

                currentSearch = currentReplace; // Для следующего в цепочке
                currentReplace = tempReplace;

                // Обновляем статистику
                if (nextBlock.stats) {
                    combinedStats.added += nextBlock.stats.added;
                    combinedStats.removed += nextBlock.stats.removed;
                    combinedStats.modified += nextBlock.stats.modified;
                }
            }

            // Создаём объединённый дифф
            result.push({
                ...block,
                search: block.search, // Оригинальный SEARCH
                replace: currentReplace, // Итоговый REPLACE после всех каскадов
                stats: combinedStats,
                applyError: undefined // Сбрасываем ошибки
            });

            processedIndices.add(i);
        }
    }

    // Удаляем блоки где search === replace (нет реальных изменений)
    return result.filter(block => block.search !== block.replace);
}

/**
 * Находит разницу между оригинальным SEARCH и модифицированным SEARCH/REPLACE.
 * Возвращает паттерн для частичного применения.
 */
function findSearchDifference(
    originalSearch: string,
    modifiedSearch: string,
    modifiedReplace: string
): { searchFragment: string; replaceFragment: string } | null {
    // Если SEARCH одинаковый - возвращаем как есть
    if (originalSearch === modifiedSearch) {
        return { searchFragment: modifiedSearch, replaceFragment: modifiedReplace };
    }

    // Ищем общую часть и различия
    const origLines = originalSearch.split('\n');
    const modSearchLines = modifiedSearch.split('\n');
    const modReplaceLines = modifiedReplace.split('\n');

    // Простой случай: одинаковое кол-во строк, различия в некоторых строках
    if (origLines.length === modSearchLines.length && origLines.length === modReplaceLines.length) {
        const searchFragments: string[] = [];
        const replaceFragments: string[] = [];
        let hasDiff = false;

        for (let i = 0; i < origLines.length; i++) {
            if (origLines[i] !== modSearchLines[i] || modSearchLines[i] !== modReplaceLines[i]) {
                searchFragments.push(modSearchLines[i]);
                replaceFragments.push(modReplaceLines[i]);
                hasDiff = true;
            }
        }

        if (hasDiff) {
            return {
                searchFragment: searchFragments.join('\n'),
                replaceFragment: replaceFragments.join('\n')
            };
        }
    }

    return null;
}

/**
 * Применяет частичное изменение к тексту.
 * Заменяет searchFragment на replaceFragment.
 */
function applyPartialChange(text: string, diff: { searchFragment: string; replaceFragment: string }): string {
    const { searchFragment, replaceFragment } = diff;

    // Если фрагмент найден - заменяем
    if (text.includes(searchFragment)) {
        return text.replace(searchFragment, replaceFragment);
    }

    // Пробуем побайтовое сравнение для поиска близких совпадений
    const textLines = text.split('\n');
    const searchLines = searchFragment.split('\n');
    const replaceLines = replaceFragment.split('\n');

    if (searchLines.length === 1 && replaceLines.length === 1) {
        // Однострочное изменение - ищем похожую строку
        for (let i = 0; i < textLines.length; i++) {
            const similarity = stringSimilarity(textLines[i].trim(), searchLines[0].trim());
            if (similarity > 0.7) {
                // Сохраняем отступ оригинала
                const indent = textLines[i].match(/^[\t ]*/)?.[0] || '';
                textLines[i] = indent + replaceLines[0].trimStart();
                return textLines.join('\n');
            }
        }
    }

    return text;
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
    let blocks = typeof diffContent === 'string' ? parseDiffBlocks(diffContent) : [...diffContent];

    // Объединяем перекрывающиеся диффы перед применением
    blocks = mergeOverlappingBlocks(blocks);

    const useCRLF = originalCode.includes('\r\n');
    let code = originalCode.replace(/\r\n/g, '\n');
    const resultBlocks: DiffBlock[] = [];
    let failedCount = 0;
    let fuzzyCount = 0;

    // Накапливаем занятые диапазоны строк для защиты от fuzzy-match на уже изменённых регионах
    const usedLineRanges: Array<[number, number]> = [];

    for (let i = 0; i < blocks.length; i++) {
        const block = blocks[i];
        const effectiveIndex = block.index ?? i;

        if (selectedIndices && !selectedIndices.includes(effectiveIndex)) {
            resultBlocks.push({ ...block, applyStatus: 'skipped' });
            continue;
        }

        const result = applyBlock(code, block, usedLineRanges);

        code = result.code;
        resultBlocks.push(result.block);

        // Регистрируем диапазон применённого блока
        if (result.appliedRange) {
            usedLineRanges.push(result.appliedRange);
        }

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
