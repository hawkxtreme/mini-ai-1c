import test from 'node:test';
import assert from 'node:assert/strict';

import type { BslDiagnostic } from '../../api/bsl';
import {
    extractParseErrorDiagnostics,
    findFirstIntroducedParseError,
    formatIntroducedParseErrorMessage,
    formatSyntaxSafeFallbackMessage,
    isParseErrorDiagnostic,
    isRecoverableSyntaxValidationMessage,
    salvageSyntaxSafeDiffBlocks,
} from '../bslSyntaxGuard';

function makeDiagnostic(overrides: Partial<BslDiagnostic>): BslDiagnostic {
    return {
        line: 0,
        character: 0,
        message: '',
        severity: 'hint',
        ...overrides,
    };
}

test('detects parser errors by message markers instead of generic severity', () => {
    assert.equal(isParseErrorDiagnostic(makeDiagnostic({
        severity: 'error',
        message: 'Ошибка разбора исходного кода. Ожидался один из следующих токенов: ENDPROCEDURE_KEYWORD',
    })), true);

    assert.equal(isParseErrorDiagnostic(makeDiagnostic({
        severity: 'error',
        message: 'Замените конструктор Шрифт на получение элемента стиля',
    })), false);
});

test('extractParseErrorDiagnostics keeps only syntax parser failures', () => {
    const diagnostics = [
        makeDiagnostic({ severity: 'error', message: 'Ошибка разбора исходного кода. Ожидался токен ENDIF_KEYWORD' }),
        makeDiagnostic({ severity: 'warning', message: 'Длина строки 129 превышает максимально допустимую 120' }),
        makeDiagnostic({ severity: 'error', message: 'Замените конструктор Шрифт на получение элемента стиля' }),
    ];

    assert.deepEqual(extractParseErrorDiagnostics(diagnostics), [diagnostics[0]]);
});

test('findFirstIntroducedParseError returns a new parse error introduced by candidate code', () => {
    const previousDiagnostics = [
        makeDiagnostic({ line: 12, character: 4, severity: 'warning', message: 'Неиспользуемая переменная' }),
    ];
    const candidateDiagnostics = [
        ...previousDiagnostics,
        makeDiagnostic({
            line: 188,
            character: 1,
            severity: 'error',
            message: 'Ошибка разбора исходного кода. Ожидался один из следующих токенов: ENDPROCEDURE_KEYWORD',
        }),
    ];

    assert.deepEqual(findFirstIntroducedParseError(previousDiagnostics, candidateDiagnostics), candidateDiagnostics[1]);
});

test('findFirstIntroducedParseError ignores the same existing parse error in both versions', () => {
    const existingParseError = makeDiagnostic({
        line: 40,
        character: 2,
        severity: 'error',
        message: 'Ошибка разбора исходного кода. Ожидался токен ENDIF_KEYWORD',
    });

    assert.equal(
        findFirstIntroducedParseError([existingParseError], [existingParseError]),
        null,
    );
});

test('formatIntroducedParseErrorMessage reports a 1-based line number', () => {
    const diagnostic = makeDiagnostic({
        line: 188,
        severity: 'error',
        message: 'Ошибка разбора исходного кода.',
    });

    assert.equal(
        formatIntroducedParseErrorMessage(diagnostic),
        'Применение отменено: diff приводит к синтаксической ошибке BSL на строке 189. Ошибка разбора исходного кода.',
    );
});

test('isRecoverableSyntaxValidationMessage distinguishes parse-error validation from generic validator failure', () => {
    assert.equal(
        isRecoverableSyntaxValidationMessage('Применение отменено: diff приводит к синтаксической ошибке BSL на строке 9. Ошибка разбора исходного кода.'),
        true,
    );
    assert.equal(
        isRecoverableSyntaxValidationMessage('Применение отменено: не удалось проверить синтаксис BSL перед применением. timeout'),
        false,
    );
});

test('salvageSyntaxSafeDiffBlocks keeps valid blocks and skips the block that introduces a parse error', async () => {
    const originalCode = [
        'Процедура Тест()',
        '\tСообщить("one");',
        '\tСообщить("two");',
        '\tСообщить("three");',
        'КонецПроцедуры',
    ].join('\n');

    const result = await salvageSyntaxSafeDiffBlocks(
        originalCode,
        [
            { search: '\tСообщить("one");', replace: '\tСообщить("ONE");' },
            { search: '\tСообщить("two");', replace: '\tBAD();' },
            { search: '\tСообщить("three");', replace: '\tСообщить("THREE");' },
        ],
        async (_baseCode, candidateCode) => {
            if (candidateCode.includes('BAD();')) {
                return 'Применение отменено: diff приводит к синтаксической ошибке BSL на строке 3. Ошибка разбора исходного кода.';
            }
            return null;
        },
    );

    assert.equal(result.appliedBlockCount, 2);
    assert.equal(result.skippedValidationCount, 1);
    assert.equal(result.blocks[1].applyStatus, 'skipped_validation');
    assert.equal(result.blocks[1].applyError, 'Применение отменено: diff приводит к синтаксической ошибке BSL на строке 3. Ошибка разбора исходного кода.');
    assert.equal(result.code, [
        'Процедура Тест()',
        '\tСообщить("ONE");',
        '\tСообщить("two");',
        '\tСообщить("THREE");',
        'КонецПроцедуры',
    ].join('\n'));
});

test('formatSyntaxSafeFallbackMessage reports partial application summary', () => {
    assert.equal(
        formatSyntaxSafeFallbackMessage({
            code: '',
            blocks: [],
            totalBlocks: 7,
            appliedBlockCount: 5,
            skippedValidationCount: 2,
            skippedValidationMessages: [
                'Применение отменено: diff приводит к синтаксической ошибке BSL на строке 9. Ошибка разбора исходного кода.',
            ],
        }),
        'Частично применено: 5 из 7 diff-блоков. Пропущено 2 из-за синтаксических ошибок BSL. diff приводит к синтаксической ошибке BSL на строке 9. Ошибка разбора исходного кода.',
    );
});
