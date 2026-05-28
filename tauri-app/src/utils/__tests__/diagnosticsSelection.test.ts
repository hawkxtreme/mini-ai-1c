import test from 'node:test';
import assert from 'node:assert/strict';

import {
    getDiagnosticSelectionKey,
    resolveEffectiveSelectedDiagnostics,
    type DiagnosticSelectionItem,
} from '../diagnosticsSelection';

const diagnostics: DiagnosticSelectionItem[] = [
    { line: 1, message: 'Функция не содержит "Возврат"', severity: 'error' },
    { line: 9, message: 'Добавьте ветвь Иначе', severity: 'warn' },
    { line: 12, message: 'Не следует использовать устаревший метод "Сообщить"', severity: 'warn' },
];

test('preserves explicit subset while diagnostics list is unchanged', () => {
    const result = resolveEffectiveSelectedDiagnostics(diagnostics, diagnostics.slice(0, 2));

    assert.equal(result.selectionWasExplicit, true);
    assert.equal(result.selectionWasStale, false);
    assert.deepEqual(result.effectiveDiagnostics, diagnostics.slice(0, 2));
});

test('falls back to current diagnostics when selected subset becomes stale after diagnostics refresh', () => {
    const staleSelection: DiagnosticSelectionItem[] = [
        { line: 40, message: 'Старая диагностика, уже исправлена', severity: 'warn' },
        diagnostics[1],
    ];

    const result = resolveEffectiveSelectedDiagnostics(diagnostics, staleSelection);

    assert.equal(result.selectionWasExplicit, false);
    assert.equal(result.selectionWasStale, true);
    assert.deepEqual(result.effectiveDiagnostics, diagnostics);
});

test('keeps explicit empty selection for exclude-all scenario', () => {
    const result = resolveEffectiveSelectedDiagnostics(diagnostics, []);

    assert.equal(result.selectionWasExplicit, true);
    assert.equal(result.selectionWasStale, false);
    assert.deepEqual(result.effectiveDiagnostics, []);
});

test('diagnostic selection key is stable for same line severity and message', () => {
    assert.equal(
        getDiagnosticSelectionKey({ line: 12, message: 'Magic number', severity: 'warn' }),
        getDiagnosticSelectionKey({ line: 12, message: 'Magic number', severity: 'warn' }),
    );
});
