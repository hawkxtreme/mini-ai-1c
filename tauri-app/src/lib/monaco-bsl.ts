import { loader } from '@monaco-editor/react';

export const registerBSL = (monaco: any) => {
    // Check if already registered
    if (monaco.languages.getLanguages().some((lang: any) => lang.id === 'bsl')) {
        return;
    }

    monaco.languages.register({ id: 'bsl' });

    monaco.languages.setLanguageConfiguration('bsl', {
        comments: {
            lineComment: '//',
        },
        brackets: [
            ['[', ']'],
            ['(', ')'],
        ],
        autoClosingPairs: [
            { open: '[', close: ']' },
            { open: '(', close: ')' },
            { open: '"', close: '"', notIn: ['string'] },
        ],
        surroundingPairs: [
            { open: '[', close: ']' },
            { open: '(', close: ')' },
            { open: '"', close: '"' },
        ]
    });

    monaco.languages.registerFoldingRangeProvider('bsl', {
        provideFoldingRanges: function (model: any, context: any, token: any) {
            const ranges: any[] = [];
            const lines = model.getLinesContent();
            const stack: { type: string, line: number }[] = [];

            const startPatterns = [
                { pattern: /^\s*(#?Область|Region)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'region' },
                { pattern: /^\s*(Процедура|Procedure)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'procedure' },
                { pattern: /^\s*(Функция|Function)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'function' },
                { pattern: /^\s*(Если|If)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'if' },
                { pattern: /^\s*(Для|For)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'for' },
                { pattern: /^\s*(Пока|While)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'while' },
                { pattern: /^\s*(Попытка|Try)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'try' }
            ];

            const endPatterns = [
                { pattern: /^\s*(#?КонецОбласти|EndRegion)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'region' },
                { pattern: /^\s*(КонецПроцедуры|EndProcedure)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'procedure' },
                { pattern: /^\s*(КонецФункции|EndFunction)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'function' },
                { pattern: /^\s*(КонецЕсли|EndIf)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'if' },
                { pattern: /^\s*(КонецЦикла|EndDo)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'for' },
                { pattern: /^\s*(КонецЦикла|EndDo)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'while' },
                { pattern: /^\s*(КонецПопытки|EndTry)(?![a-zA-Zа-яА-Я0-9_])/i, type: 'try' }
            ];

            let commentStart = -1;

            for (let i = 0; i < lines.length; i++) {
                const line = lines[i];
                const lineNumber = i + 1;

                // Comment block detection
                if (line.trim().startsWith('//')) {
                    if (commentStart === -1) {
                        commentStart = lineNumber;
                    }
                } else {
                    if (commentStart !== -1) {
                        if (lineNumber - 1 > commentStart) {
                            ranges.push({
                                start: commentStart,
                                end: lineNumber - 1,
                                kind: 1 // monaco.languages.FoldingRangeKind.Comment
                            });
                        }
                        commentStart = -1;
                    }
                }

                // Keyword block detection
                let foundStart = false;
                for (const p of startPatterns) {
                    if (p.pattern.test(line)) {
                        stack.push({ type: p.type, line: lineNumber });
                        foundStart = true;
                        break;
                    }
                }

                if (!foundStart) {
                    for (const p of endPatterns) {
                        if (p.pattern.test(line)) {
                            // Find matching start
                            for (let j = stack.length - 1; j >= 0; j--) {
                                if (stack[j].type === p.type) {
                                    const start = stack.splice(j, 1)[0];
                                    if (lineNumber > start.line) {
                                        ranges.push({
                                            start: start.line,
                                            end: lineNumber,
                                            kind: p.type === 'region' ? 3 : undefined // 3 is monaco.languages.FoldingRangeKind.Region
                                        });
                                    }
                                    break;
                                }
                            }
                            break;
                        }
                    }
                }
            }

            // Finalize any trailing comment block
            if (commentStart !== -1 && lines.length > commentStart) {
                ranges.push({
                    start: commentStart,
                    end: lines.length,
                    kind: 1 // Comment
                });
            }

            return ranges;
        }
    });

    monaco.languages.setMonarchTokensProvider('bsl', {
        defaultToken: '',
        tokenPostfix: '.bsl',
        ignoreCase: true,

        keywords: [
            'Процедура', 'Procedure', 'КонецПроцедуры', 'EndProcedure',
            'Функция', 'Function', 'КонецФункции', 'EndFunction',
            'Если', 'If', 'Тогда', 'Then', 'Иначе', 'Else', 'ИначеЕсли', 'ElsIf', 'КонецЕсли', 'EndIf',
            'Для', 'For', 'Каждого', 'Each', 'Из', 'In', 'Цикл', 'Do', 'КонецЦикла', 'EndDo',
            'Пока', 'While', 'Перем', 'Var', 'Возврат', 'Return', 'Попытка', 'Try',
            'Исключение', 'Except', 'КонецПопытки', 'EndTry', 'Прервать', 'Break',
            'Продолжить', 'Continue', 'Новый', 'New', 'Экспорт', 'Export',
            'Ложь', 'False', 'Истина', 'True', 'Неопределено', 'Undefined', 'Null'
        ],

        operators: [
            '=', '<>', '<', '<=', '>', '>=', '+', '-', '*', '/', '%',
            'И', 'And', 'ИЛИ', 'Or', 'НЕ', 'Not'
        ],

        symbols: /[=><!~?:&|+\-*/^%]+/,

        tokenizer: {
            root: [
                // Identifiers and keywords
                [/[a-zA-Zа-яА-Я_][a-zA-Zа-яА-Я0-9_]*/, {
                    cases: {
                        '@keywords': 'keyword',
                        '@default': 'identifier'
                    }
                }],

                // Whitespace
                { include: '@whitespace' },

                // Delimiters
                [/[()\[\]]/, '@brackets'],
                [/[<>={}\/]|(!=)|(<=)|(>=)|(&&)|(\|\|)/, {
                    cases: {
                        '@operators': 'operator',
                        '@default': ''
                    }
                }],

                // Numbers
                [/\d*\.\d+([eE][\-+]?\d+)?/, 'number.float'],
                [/\d+/, 'number'],

                // Delimiter: after number because of .\d floats
                [/[;,.]/, 'delimiter'],

                // Strings
                [/"([^"\\]|\\.)*$/, 'string.invalid'], // non-teminated string
                [/"/, { token: 'string.quote', bracket: '@open', next: '@string' }],

                // Preprocessor
                [/^\s*#.*/, 'metatag'],
            ],

            string: [
                [/[^\\"]+/, 'string'],
                [/\\./, 'string.escape'],
                [/"/, { token: 'string.quote', bracket: '@close', next: '@pop' }],
            ],

            whitespace: [
                [/[ \t\r\n]+/, 'white'],
                [/\/\/.*$/, 'comment'],
            ],
        },
    });
};
