export const SELECTIVE_FIX_SCOPE_MARKER = '=== SELECTIVE BSL FIX SCOPE ===';

interface SelectiveFixScopeOptions {
    commandId?: string;
    totalDiagnosticsCount: number;
    selectedDiagnosticsCount?: number | null;
    diagnosticsText?: string;
    code?: string;
    queryPart?: string;
}

function isSelectiveFixScope(options: SelectiveFixScopeOptions): boolean {
    if (options.commandId !== 'fix') {
        return false;
    }

    if (options.selectedDiagnosticsCount === null || options.selectedDiagnosticsCount === undefined) {
        return false;
    }

    return options.selectedDiagnosticsCount > 0
        && options.totalDiagnosticsCount > options.selectedDiagnosticsCount;
}

export function applySelectiveFixScopeInstructions(
    basePrompt: string,
    options: SelectiveFixScopeOptions,
): string {
    if (!isSelectiveFixScope(options)) {
        return basePrompt;
    }

    const diagnosticsText = options.diagnosticsText?.trim() || '- Диагностики не переданы';
    const code = options.code ?? '';
    const extraWish = options.queryPart?.trim();

    const lines = [
        SELECTIVE_FIX_SCOPE_MARKER,
        'Исправь ТОЛЬКО явно выбранные пользователем диагностики из списка ниже.',
        'НЕ исправляй другие ошибки, предупреждения и замечания, даже если заметишь их в коде или получишь их из дополнительного анализа.',
        'НЕ вызывай `check_bsl_syntax` до внесения правок, потому что это расширит объём работ сверх выбранного списка.',
        'После внесения правок можно использовать `check_bsl_syntax` только для самопроверки изменённых строк.',
        SELECTIVE_FIX_SCOPE_MARKER,
        '',
        'Выбранные диагностики для исправления:',
        diagnosticsText,
    ];

    if (extraWish) {
        lines.push('', `Дополнительное пожелание пользователя: ${extraWish}`);
    }

    lines.push('', 'Код для исправления:', '```bsl', code, '```');

    return lines.join('\n');
}
