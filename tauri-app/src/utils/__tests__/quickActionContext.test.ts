import test from 'node:test';
import assert from 'node:assert/strict';

import {
    resolveCaptureFromEditorContext,
    shouldSyncQuickActionToClickTarget,
    type QuickActionEditorContextSnapshot,
} from '../quickActionContext';

function makeContext(
    overrides: Partial<QuickActionEditorContextSnapshot> = {},
): QuickActionEditorContextSnapshot {
    return {
        available: true,
        has_selection: false,
        selection_text: '',
        current_method_name: 'ПринятьПакетОбмена',
        current_method_text:
            'Процедура ПринятьПакетОбмена(УзелОбмена, ДанныеОбмена)\nКонецПроцедуры',
        module_text:
            'Процедура ПринятьПакетОбмена(УзелОбмена, ДанныеОбмена)\nКонецПроцедуры\n\nПроцедура Следующая()\nКонецПроцедуры',
        caret_line: 12,
        method_start_line: 10,
        primary_runtime_id: 'runtime-1',
        ...overrides,
    };
}

test('describe always stays on current method even when a line is selected', () => {
    const capture = resolveCaptureFromEditorContext(
        makeContext({
            has_selection: true,
            selection_text: '\tУстановитьПривилегированныйРежим(Истина);',
        }),
        'describe',
    );

    assert.ok(capture);
    assert.equal(capture.scope, 'current_method');
    assert.match(capture.promptCode, /^Процедура ПринятьПакетОбмена/m);
});

test('describe does not sync caret to right-click point', () => {
    assert.equal(shouldSyncQuickActionToClickTarget('describe', false), false);
    assert.equal(shouldSyncQuickActionToClickTarget('describe', true), false);
});

test('fix still respects explicit selection when it exists', () => {
    const capture = resolveCaptureFromEditorContext(
        makeContext({
            has_selection: true,
            selection_text: 'Сообщить("Тест");',
        }),
        'fix',
    );

    assert.ok(capture);
    assert.equal(capture.scope, 'selection');
    assert.equal(capture.promptCode, 'Сообщить("Тест");');
});

test('non-describe action syncs caret only when there is no selection', () => {
    assert.equal(shouldSyncQuickActionToClickTarget('fix', false), true);
    assert.equal(shouldSyncQuickActionToClickTarget('fix', true), false);
});
