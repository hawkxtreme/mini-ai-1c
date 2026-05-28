type BslMethodKind = 'procedure' | 'function' | 'unknown';

interface BslMethodSignatureInfo {
    kind: BslMethodKind;
    name: string | null;
    parameters: string[];
}

function parseBslMethodSignature(code: string): BslMethodSignatureInfo {
    const signatureMatch = code.match(
        /^\s*(Процедура|Функция|Procedure|Function)\s+([A-Za-zА-Яа-я_][\wА-Яа-я]*)\s*(?:\(([^)]*)\))?/im,
    );

    if (!signatureMatch) {
        return {
            kind: 'unknown',
            name: null,
            parameters: [],
        };
    }

    const [, rawKind, rawName, rawParams] = signatureMatch;
    const kind: BslMethodKind =
        /^(Функция|Function)$/i.test(rawKind) ? 'function' : 'procedure';
    const parameters = (rawParams ?? '')
        .split(',')
        .map((item) => item.trim())
        .filter(Boolean)
        .map((item) => item.replace(/\s*=.*$/u, '').trim())
        .filter(Boolean);

    return {
        kind,
        name: rawName?.trim() ?? null,
        parameters,
    };
}

function buildExpectedSections(info: BslMethodSignatureInfo): string[] {
    const sections: string[] = [];

    if (info.parameters.length > 0) {
        sections.push('- `// Параметры:`');
    }

    if (info.kind === 'function') {
        sections.push('- `// Возвращаемое значение:`');
    }

    return sections;
}

function formatExpectedSections(info: BslMethodSignatureInfo): string {
    const sections = buildExpectedSections(info);
    return sections.length > 0 ? sections.join('\n') : '- Именованные секции не требуются.';
}

function buildResultSkeleton(info: BslMethodSignatureInfo): string {
    const lines = ['// Формирует краткое и предметное описание назначения метода.'];

    if (info.parameters.length > 0) {
        lines.push('//', '// Параметры:');
        for (const parameter of info.parameters) {
            lines.push(`//  ${parameter} - Тип - Описание параметра.`);
        }
    }

    if (info.kind === 'function') {
        lines.push('//', '// Возвращаемое значение:', '//  Тип - Описание возвращаемого значения.');
    }

    return lines.join('\n');
}

export function buildDescribePrompt(code: string): string {
    const methodInfo = parseBslMethodSignature(code);
    const expectedSections = formatExpectedSections(methodInfo);
    const methodLine =
        methodInfo.name && methodInfo.kind !== 'unknown'
            ? `Метод: ${methodInfo.kind === 'function' ? 'Функция' : 'Процедура'} ${methodInfo.name}.`
            : 'Тип метода не удалось определить по сигнатуре, ориентируйся по коду.';
    const parametersLine =
        methodInfo.parameters.length > 0
            ? `Параметры метода: ${methodInfo.parameters.join(', ')}.`
            : 'У метода нет параметров, секцию `// Параметры:` не добавляй.';
    const returnValueLine =
        methodInfo.kind === 'function'
            ? 'Это функция, секция `// Возвращаемое значение:` обязательна.'
            : 'Это процедура, секцию `// Возвращаемое значение:` не добавляй.';

    return `Ты — опытный 1С-разработчик. Сгенерируй комментарий к процедуре или функции строго по стандарту 1С «Описание процедур и функций» (#std453).

${methodLine}
${parametersLine}
${returnValueLine}

Требования к результату:
- Верни только блок комментария перед объявлением метода.
- Каждая строка результата должна начинаться с //.
- Не добавляй заголовок \`// Описание:\`.
- Первая непустая строка комментария должна сразу содержать текст назначения метода.
- Между секциями оставляй пустую строку в виде отдельной строки \`//\`.
- Если комментарий не помещается в одну строку, переноси его по словам на следующую строку, и каждая перенесённая строка тоже должна начинаться с //.
- Ни одна строка готового комментария не должна превышать 120 символов.
- Описание должно начинаться с глагола и объяснять назначение метода так, чтобы был понятен сценарий использования без чтения реализации.
- Не начинай описание со слов "Процедура", "Функция" и не повторяй имя метода, если это не добавляет смысла.
- Не пиши тавтологии и очевидные фразы, которые просто повторяют название метода.
- Для каждого параметра используй формат:
//  ИмяПараметра - Тип1, Тип2 - Осмысленное описание параметра.
- Тип параметра обязателен. Если точный тип неочевиден, определи наиболее вероятный по коду.
- Для простого возвращаемого значения используй формат:
//  Тип - Описание.
- Для составного возвращаемого значения каждый вариант пиши с новой строки:
//  - Тип - Описание.
- Не добавляй секцию "Пример".
- Не возвращай код метода, сигнатуру метода, markdown или пояснения вне комментария.

Ожидаемые секции для этого метода:
${expectedSections}

Шаблон результата для этого метода:
${buildResultSkeleton(methodInfo)}

Код:
${code}

Верни только готовый комментарий по стандарту 1С.`;
}
