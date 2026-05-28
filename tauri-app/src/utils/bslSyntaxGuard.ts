import type { BslDiagnostic } from '../api/bsl';
import { applyDiffWithDiagnostics, type DiffBlock } from './diffViewer';

const PARSE_ERROR_MARKERS = [
    'ошибка разбора',
    'parseerror',
    'expected one of',
    'ожидался один из следующих токенов',
];

function normalizeDiagnosticMessage(message: string): string {
    return message.trim().replace(/\s+/g, ' ').toLowerCase();
}

function getParseErrorKey(diagnostic: BslDiagnostic): string {
    return `${diagnostic.line}:${diagnostic.character}:${normalizeDiagnosticMessage(diagnostic.message)}`;
}

export function isParseErrorDiagnostic(
    diagnostic: Pick<BslDiagnostic, 'message' | 'severity'> | null | undefined,
): boolean {
    if (!diagnostic || diagnostic.severity !== 'error') {
        return false;
    }

    const normalizedMessage = normalizeDiagnosticMessage(diagnostic.message || '');
    return PARSE_ERROR_MARKERS.some((marker) => normalizedMessage.includes(marker));
}

export function extractParseErrorDiagnostics(
    diagnostics: BslDiagnostic[] | null | undefined,
): BslDiagnostic[] {
    return (diagnostics || []).filter((diagnostic) => isParseErrorDiagnostic(diagnostic));
}

export function findFirstIntroducedParseError(
    previousDiagnostics: BslDiagnostic[] | null | undefined,
    nextDiagnostics: BslDiagnostic[] | null | undefined,
): BslDiagnostic | null {
    const previousParseErrorKeys = new Set(
        extractParseErrorDiagnostics(previousDiagnostics).map((diagnostic) => getParseErrorKey(diagnostic)),
    );

    return extractParseErrorDiagnostics(nextDiagnostics).find(
        (diagnostic) => !previousParseErrorKeys.has(getParseErrorKey(diagnostic)),
    ) ?? null;
}

export function formatIntroducedParseErrorMessage(diagnostic: BslDiagnostic): string {
    return `Применение отменено: diff приводит к синтаксической ошибке BSL на строке ${diagnostic.line + 1}. ${diagnostic.message}`;
}

export interface SyntaxSafeDiffFallbackResult {
    code: string;
    blocks: DiffBlock[];
    totalBlocks: number;
    appliedBlockCount: number;
    skippedValidationCount: number;
    skippedValidationMessages: string[];
}

function normalizeValidationMessage(message: string): string {
    return message.replace(/^Применение отменено:\s*/i, '').trim();
}

export function isRecoverableSyntaxValidationMessage(message: string | null | undefined): boolean {
    if (!message) {
        return false;
    }

    return normalizeValidationMessage(message).toLowerCase().includes('синтаксической ошибке bsl');
}

export async function salvageSyntaxSafeDiffBlocks(
    originalCode: string,
    blocks: DiffBlock[],
    validateCandidate: (baseCode: string, candidateCode: string) => Promise<string | null>,
): Promise<SyntaxSafeDiffFallbackResult> {
    let code = originalCode;
    let appliedBlockCount = 0;
    const resultBlocks: DiffBlock[] = [];
    const skippedValidationMessages: string[] = [];

    for (const block of blocks) {
        const singleResult = applyDiffWithDiagnostics(code, [block]);
        const singleBlock = singleResult.blocks[0] ?? { ...block, applyStatus: 'failed_not_found' as const };

        if (singleResult.failedCount > 0 || singleResult.fuzzyCount > 0) {
            resultBlocks.push(singleBlock);
            continue;
        }

        const validationError = await validateCandidate(code, singleResult.code);
        if (validationError) {
            skippedValidationMessages.push(validationError);
            resultBlocks.push({
                ...singleBlock,
                applyStatus: 'skipped_validation',
                applyError: validationError,
            });
            continue;
        }

        code = singleResult.code;
        appliedBlockCount++;
        resultBlocks.push(singleBlock);
    }

    return {
        code,
        blocks: resultBlocks,
        totalBlocks: blocks.length,
        appliedBlockCount,
        skippedValidationCount: skippedValidationMessages.length,
        skippedValidationMessages,
    };
}

export function formatSyntaxSafeFallbackMessage(result: SyntaxSafeDiffFallbackResult): string | null {
    if (result.skippedValidationCount === 0) {
        return null;
    }

    const firstDetail = normalizeValidationMessage(result.skippedValidationMessages[0] ?? '');
    if (result.appliedBlockCount > 0) {
        return `Частично применено: ${result.appliedBlockCount} из ${result.totalBlocks} diff-блоков. Пропущено ${result.skippedValidationCount} из-за синтаксических ошибок BSL.${firstDetail ? ` ${firstDetail}` : ''}`;
    }

    return `Применение отменено: ни один diff-блок не прошёл синтаксическую проверку BSL.${firstDetail ? ` ${firstDetail}` : ''}`;
}
