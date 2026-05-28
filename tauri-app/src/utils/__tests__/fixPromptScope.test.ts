import test from 'node:test';
import assert from 'node:assert/strict';

import {
    applySelectiveFixScopeInstructions,
    SELECTIVE_FIX_SCOPE_MARKER,
} from '../fixPromptScope';

test('adds strict scope instructions when only part of diagnostics is selected for fix command', () => {
    const prompt = applySelectiveFixScopeInstructions(
        'Исправь ошибки в этом коде.\n- Line 83: ...\n- Line 195: ...',
        {
            commandId: 'fix',
            totalDiagnosticsCount: 8,
            selectedDiagnosticsCount: 2,
            diagnosticsText: '- Line 83: Ключевое слово "новый" написано не канонически (warning)\n- Line 195: Ключевое слово "новый" написано не канонически (warning)',
            code: 'Процедура Тест()\n    Запрос = новый Запрос;\nКонецПроцедуры',
        },
    );

    assert.match(prompt, new RegExp(SELECTIVE_FIX_SCOPE_MARKER));
    assert.match(prompt, /ТОЛЬКО явно выбранные пользователем диагностики/i);
    assert.match(prompt, /НЕ вызывай `check_bsl_syntax` до внесения правок/i);
    assert.match(prompt, /После внесения правок можно использовать `check_bsl_syntax` только для самопроверки/i);
    assert.match(prompt, /Выбранные диагностики для исправления:/i);
    assert.match(prompt, /```bsl/);
});

test('keeps prompt unchanged when all diagnostics remain selected', () => {
    const basePrompt = 'Исправь ошибки в этом коде.';
    const prompt = applySelectiveFixScopeInstructions(basePrompt, {
        commandId: 'fix',
        totalDiagnosticsCount: 2,
        selectedDiagnosticsCount: 2,
    });

    assert.equal(prompt, basePrompt);
});
