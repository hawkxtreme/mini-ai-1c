import test from 'node:test';
import assert from 'node:assert/strict';

import { buildDescribePrompt } from '../quickActionPrompts';

test('describe prompt follows std453 for functions without an explicit description section', () => {
    const prompt = buildDescribePrompt(
        'Функция СформироватьПакетОбмена(УзелОбмена)\n\tВозврат УзелОбмена;\nКонецФункции',
    );

    assert.match(prompt, /Это функция, секция `\/\/ Возвращаемое значение:` обязательна\./);
    assert.match(prompt, /- `\/\/ Параметры:`/);
    assert.match(prompt, /- `\/\/ Возвращаемое значение:`/);
    assert.doesNotMatch(prompt, /- `\/\/ Описание:`/);
    assert.match(
        prompt,
        /Шаблон результата для этого метода:\n\/\/ Формирует краткое и предметное описание назначения метода\./,
    );
    assert.doesNotMatch(
        prompt,
        /Шаблон результата для этого метода:[\s\S]*\/\/ Описание:/,
    );
    assert.match(prompt, /Не добавляй заголовок `\/\/ Описание:`\./);
});

test('describe prompt keeps parameters and skips return section for procedures', () => {
    const prompt = buildDescribePrompt(
        'Процедура ПринятьПакетОбмена(УзелОбмена, ДанныеОбмена) Экспорт\nКонецПроцедуры',
    );

    assert.match(prompt, /Это процедура, секцию `\/\/ Возвращаемое значение:` не добавляй\./);
    assert.match(prompt, /Параметры метода: УзелОбмена, ДанныеОбмена\./);
    assert.match(
        prompt,
        /\/\/ Параметры:\n\/\/  УзелОбмена - Тип - Описание параметра\.\n\/\/  ДанныеОбмена - Тип - Описание параметра\./,
    );
    assert.doesNotMatch(
        prompt,
        /Шаблон результата для этого метода:[\s\S]*\/\/ Описание:/,
    );
    assert.doesNotMatch(prompt, /- `\/\/ Возвращаемое значение:`/);
});

test('describe prompt skips parameter section for parameterless procedures', () => {
    const prompt = buildDescribePrompt(
        'Процедура ОбновитьКэш() Экспорт\nКонецПроцедуры',
    );

    assert.match(prompt, /У метода нет параметров, секцию `\/\/ Параметры:` не добавляй\./);
    assert.doesNotMatch(prompt, /- `\/\/ Параметры:`/);
    assert.match(prompt, /Ожидаемые секции для этого метода:\n- Именованные секции не требуются\./);
    assert.match(
        prompt,
        /Шаблон результата для этого метода:\n\/\/ Формирует краткое и предметное описание назначения метода\./,
    );
    assert.doesNotMatch(
        prompt,
        /Шаблон результата для этого метода:[\s\S]*\/\/ Параметры:/,
    );
    assert.doesNotMatch(
        prompt,
        /Шаблон результата для этого метода:[\s\S]*\/\/ Описание:/,
    );
});
