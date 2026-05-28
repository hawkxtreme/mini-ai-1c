import test from 'node:test';
import assert from 'node:assert/strict';

import {
    bindConfiguratorWindow,
    getConfiguratorBindingDisplayTitle,
    resolveConfiguratorBinding,
    type ConfiguratorWindowDescriptor,
} from '../configuratorBinding';

function makeWindow(overrides: Partial<ConfiguratorWindowDescriptor> = {}): ConfiguratorWindowDescriptor {
    return {
        hwnd: 1001,
        process_id: 501,
        title: 'Общий модуль ГлобальныйПоискСервер: Модуль - Конфигуратор - DemoBase',
        ...overrides,
    };
}

test('keeps exact hwnd when selected window is still present', () => {
    const selectedWindow = makeWindow();
    const binding = bindConfiguratorWindow(selectedWindow);

    const result = resolveConfiguratorBinding(binding, [
        selectedWindow,
        makeWindow({ hwnd: 1002, process_id: 777, title: 'Конфигуратор - AnotherBase' }),
    ]);

    assert.equal(result.status, 'resolved');
    assert.equal(result.matchedBy, 'hwnd');
    assert.equal(result.activeWindow?.hwnd, 1001);
    assert.equal(result.nextBinding.selected_window_pid, 501);
    assert.equal(result.nextBinding.selected_config_name, 'DemoBase');
});

test('rebinds to a new hwnd inside the same process', () => {
    const binding = bindConfiguratorWindow(makeWindow({ hwnd: 1001, process_id: 501 }));

    const result = resolveConfiguratorBinding(binding, [
        makeWindow({ hwnd: 2001, process_id: 501, title: 'Документ ЗаказКлиента: Модуль объекта - Конфигуратор - DemoBase' }),
    ]);

    assert.equal(result.status, 'rebound');
    assert.equal(result.matchedBy, 'process_id');
    assert.equal(result.activeWindow?.hwnd, 2001);
    assert.equal(result.nextBinding.selected_window_hwnd, 2001);
    assert.equal(result.nextBinding.selected_window_pid, 501);
});

test('rebinds by config name after configurator restart when match is unique', () => {
    const binding = bindConfiguratorWindow(makeWindow({
        hwnd: 1001,
        process_id: 501,
        title: 'Общий модуль Старый: Модуль - Конфигуратор - DemoBase',
    }));

    const result = resolveConfiguratorBinding(binding, [
        makeWindow({
            hwnd: 3001,
            process_id: 999,
            title: 'Справочник Номенклатура: Модуль менеджера - Конфигуратор - DemoBase',
        }),
    ]);

    assert.equal(result.status, 'rebound');
    assert.equal(result.matchedBy, 'config_name');
    assert.equal(result.activeWindow?.hwnd, 3001);
    assert.equal(result.nextBinding.selected_window_pid, 999);
});

test('falls back to unique title match for legacy binding without config name', () => {
    const result = resolveConfiguratorBinding(
        {
            selected_window_hwnd: 555,
            selected_window_pid: null,
            selected_window_title: 'Конфигуратор - LegacyBase',
            selected_config_name: null,
        },
        [makeWindow({ hwnd: 777, process_id: 123, title: 'Конфигуратор - LegacyBase' })],
    );

    assert.equal(result.status, 'rebound');
    assert.equal(result.matchedBy, 'title');
    assert.equal(result.activeWindow?.hwnd, 777);
    assert.equal(result.nextBinding.selected_config_name, 'LegacyBase');
});

test('returns ambiguous instead of silently switching to one of many candidates', () => {
    const binding = bindConfiguratorWindow(makeWindow({
        hwnd: 1001,
        process_id: 501,
        title: 'Общий модуль Старый: Модуль - Конфигуратор - DemoBase',
    }));

    const result = resolveConfiguratorBinding(binding, [
        makeWindow({ hwnd: 4001, process_id: 900, title: 'Общий модуль A: Модуль - Конфигуратор - DemoBase' }),
        makeWindow({ hwnd: 4002, process_id: 901, title: 'Общий модуль B: Модуль - Конфигуратор - DemoBase' }),
    ]);

    assert.equal(result.status, 'ambiguous');
    assert.equal(result.activeWindow, null);
    assert.equal(result.nextBinding.selected_window_hwnd, null);
    assert.equal(result.candidates.length, 2);
});

test('returns missing and clears active hwnd when no candidate is found', () => {
    const binding = bindConfiguratorWindow(makeWindow({
        hwnd: 1001,
        process_id: 501,
        title: 'Общий модуль Старый: Модуль - Конфигуратор - DemoBase',
    }));

    const result = resolveConfiguratorBinding(binding, [
        makeWindow({ hwnd: 5001, process_id: 700, title: 'Конфигуратор - AnotherBase' }),
    ]);

    assert.equal(result.status, 'missing');
    assert.equal(result.activeWindow, null);
    assert.equal(result.nextBinding.selected_window_hwnd, null);
    assert.equal(result.nextBinding.selected_config_name, 'DemoBase');
});

test('display title uses active window first and falls back to stored binding', () => {
    const binding = bindConfiguratorWindow(makeWindow({
        title: 'Общий модуль ГлобальныйПоискСервер: Модуль - Конфигуратор - DemoBase',
    }));

    assert.equal(
        getConfiguratorBindingDisplayTitle(binding, makeWindow({ title: 'Документ ЗаказКлиента: Модуль объекта - Конфигуратор - DemoBase' })),
        'DemoBase',
    );
    assert.equal(getConfiguratorBindingDisplayTitle(binding, null), 'DemoBase');
});

// ─── English Configurator window title support (issue #110) ──────────────────

test('EN: resolves window with English "Configurator" title', () => {
    const selectedWindow = makeWindow({
        title: 'Common module GlobalSearch: Module - Configurator - DemoBase',
    });
    const binding = bindConfiguratorWindow(selectedWindow);

    const result = resolveConfiguratorBinding(binding, [selectedWindow]);

    assert.equal(result.status, 'resolved');
    assert.equal(result.matchedBy, 'hwnd');
});

test('EN: extracts config_name from English title format', () => {
    const binding = bindConfiguratorWindow(makeWindow({
        title: 'Document SalesOrder: Object Module - Configurator - DemoBase',
    }));

    assert.equal(getConfiguratorBindingDisplayTitle(binding, null), 'DemoBase');
});

test('EN: simple "Configurator - ConfigName" title', () => {
    const binding = bindConfiguratorWindow(makeWindow({
        title: 'Configurator - ProductionBase',
    }));

    assert.equal(getConfiguratorBindingDisplayTitle(binding, null), 'ProductionBase');
});

test('EN: rebinds by config_name when hwnd changes (English title)', () => {
    const original = makeWindow({
        hwnd: 1001,
        process_id: 501,
        title: 'Common module OldModule: Module - Configurator - DemoBase',
    });
    const binding = bindConfiguratorWindow(original);

    // hwnd changed but same config — status is 'rebound'
    const result = resolveConfiguratorBinding(binding, [
        makeWindow({ hwnd: 9001, process_id: 502, title: 'Common module NewModule: Module - Configurator - DemoBase' }),
    ]);

    assert.equal(result.status, 'rebound');
    assert.equal(result.nextBinding.selected_config_name, 'DemoBase');
});
