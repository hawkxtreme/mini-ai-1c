import test from 'node:test';
import assert from 'node:assert/strict';

import {
    applyDiffWithDiagnostics,
    formatDiffErrorMessage,
    getApplicableDiffContent,
    hasApplicableDiffBlocks,
    hasApplicableDiffContent,
    hasBlockingIncompleteDiffBlocks,
    hasDiffBlocks,
    hasIncompleteDiffBlocks,
    parseDiffBlocks,
} from '../diffViewer';

test('hasApplicableDiffBlocks returns false for a no-op SEARCH/REPLACE block', () => {
    const originalCode = [
        'Procedure Demo()',
        '\tMessage("ok");',
        'EndProcedure',
    ].join('\n');

    const diffContent = [
        '<<<<<<< SEARCH',
        '\tMessage("ok");',
        '=======',
        '\tMessage("ok");',
        '>>>>>>> REPLACE',
    ].join('\n');

    assert.equal(hasApplicableDiffBlocks(originalCode, diffContent), false);
});

test('hasApplicableDiffBlocks returns true when SEARCH/REPLACE changes the code', () => {
    const originalCode = [
        'Procedure Demo()',
        '\tMessage("old");',
        'EndProcedure',
    ].join('\n');

    const diffContent = [
        '<<<<<<< SEARCH',
        '\tMessage("old");',
        '=======',
        '\tMessage("new");',
        '>>>>>>> REPLACE',
    ].join('\n');

    assert.equal(hasApplicableDiffBlocks(originalCode, diffContent), true);
});

test('getApplicableDiffContent converts a full BSL code block into an applyable replacement diff', () => {
    const originalCode = [
        'Procedure Demo()',
        '\tMessage("old");',
        'EndProcedure',
    ].join('\n');

    const response = [
        'Вот исправленный код:',
        '',
        '```bsl',
        'Procedure Demo()',
        '\tMessage("new");',
        'EndProcedure',
        '```',
    ].join('\n');

    const diffContent = getApplicableDiffContent(originalCode, response);

    assert.ok(diffContent);
    assert.equal(hasApplicableDiffContent(originalCode, response), true);

    const result = applyDiffWithDiagnostics(originalCode, diffContent);

    assert.equal(result.failedCount, 0);
    assert.equal(result.code, [
        'Procedure Demo()',
        '\tMessage("new");',
        'EndProcedure',
    ].join('\n'));
});

test('getApplicableDiffContent ignores short BSL snippets that are not full replacements', () => {
    const originalCode = [
        'Procedure Demo()',
        '\tMessage("old");',
        '\tMessage("second");',
        '\tMessage("third");',
        'EndProcedure',
    ].join('\n');

    const response = [
        'Например:',
        '',
        '```bsl',
        '\tMessage("new");',
        '```',
    ].join('\n');

    assert.equal(getApplicableDiffContent(originalCode, response), null);
    assert.equal(hasApplicableDiffContent(originalCode, response), false);
});

test('hasApplicableDiffBlocks returns true for a non-empty replacement on an empty base code', () => {
    const diffContent = [
        '<<<<<<< SEARCH',
        '=======',
        'Procedure Demo()',
        '\tMessage("new");',
        'EndProcedure',
        '>>>>>>> REPLACE',
    ].join('\n');

    assert.equal(hasApplicableDiffBlocks('', diffContent), true);
});

test('parseDiffBlocks ignores an incomplete trailing XML diff block', () => {
    const diffContent = [
        '<diff>',
        '<search>',
        '\tMessage("old");',
        '</search>',
        '<replace>',
        '\tMessage("new");',
        '</replace>',
        '</diff>',
        '',
        '<diff>',
        '<search>',
        '\tMessage("second old");',
        '</search>',
        '<replace>',
        '\tMessage("second new");',
    ].join('\n');

    const blocks = parseDiffBlocks(diffContent);

    assert.equal(blocks.length, 1);
    assert.equal(blocks[0].search, '\tMessage("old");');
    assert.equal(blocks[0].replace, '\tMessage("new");');
    assert.equal(hasIncompleteDiffBlocks(diffContent), true);
    assert.equal(hasBlockingIncompleteDiffBlocks(diffContent), false);
});

test('hasApplicableDiffBlocks returns false for an incomplete legacy SEARCH/REPLACE block', () => {
    const originalCode = [
        'Procedure Demo()',
        '\tMessage("old");',
        'EndProcedure',
    ].join('\n');

    const diffContent = [
        '<<<<<<< SEARCH',
        '\tMessage("old");',
        '=======',
        '\tMessage("new");',
    ].join('\n');

    assert.equal(hasIncompleteDiffBlocks(diffContent), true);
    assert.equal(hasBlockingIncompleteDiffBlocks(diffContent), true);
    assert.equal(hasApplicableDiffBlocks(originalCode, diffContent), false);

    const result = applyDiffWithDiagnostics(originalCode, diffContent);
    assert.equal(result.code, originalCode);
    assert.equal(result.blocks.length, 0);
});

test('applyDiffWithDiagnostics keeps only complete blocks when the response ends with a truncated tail', () => {
    const originalCode = [
        'Procedure Demo()',
        '\tMessage("first old");',
        '\tMessage("second old");',
        'EndProcedure',
    ].join('\n');

    const diffContent = [
        '<diff>',
        '<search>',
        '\tMessage("first old");',
        '</search>',
        '<replace>',
        '\tMessage("first new");',
        '</replace>',
        '</diff>',
        '',
        '<diff>',
        '<search>',
        '\tMessage("second old");',
        '</search>',
        '<replace>',
        '\tMessage("second new");',
    ].join('\n');

    const result = applyDiffWithDiagnostics(originalCode, diffContent);

    assert.equal(result.code, [
        'Procedure Demo()',
        '\tMessage("first new");',
        '\tMessage("second old");',
        'EndProcedure',
    ].join('\n'));
    assert.equal(result.failedCount, 0);
    assert.equal(result.fuzzyCount, 0);
    assert.equal(hasApplicableDiffBlocks(originalCode, diffContent), true);
    assert.equal(hasIncompleteDiffBlocks(diffContent), true);
    assert.equal(hasBlockingIncompleteDiffBlocks(diffContent), false);
});

test('applyDiffWithDiagnostics decodes HTML entities inside XML diff blocks', () => {
    const originalCode = [
        'Procedure Demo()',
        '\tIf Value < 9 Then',
        '\t\tMessage("small");',
        '\tEndIf;',
        'EndProcedure',
    ].join('\n');

    const diffContent = [
        '<diff>',
        '<search>',
        '\tIf Value &lt; 9 Then',
        '\t\tMessage("small");',
        '\tEndIf;',
        '</search>',
        '<replace>',
        '\tIf Value &lt; MaxValue Then',
        '\t\tMessage("small");',
        '\tEndIf;',
        '</replace>',
        '</diff>',
    ].join('\n');

    const result = applyDiffWithDiagnostics(originalCode, diffContent);

    assert.equal(result.failedCount, 0);
    assert.equal(result.fuzzyCount, 0);
    assert.match(result.code, /Value < MaxValue/);
    assert.doesNotMatch(result.code, /&lt;/);
});

test('hasDiffBlocks recognizes escaped XML diff tags', () => {
    const escapedDiff = [
        '&lt;diff&gt;',
        '&lt;search&gt;',
        'old',
        '&lt;/search&gt;',
        '&lt;replace&gt;',
        'new',
        '&lt;/replace&gt;',
        '&lt;/diff&gt;',
    ].join('\n');

    assert.equal(hasDiffBlocks(escapedDiff), true);
});

test('formatDiffErrorMessage returns plain text without markdown emphasis markers', () => {
    const result = applyDiffWithDiagnostics(
        ['Procedure Demo()', '\tMessage("old");', 'EndProcedure'].join('\n'),
        [
            '<diff>',
            '<search>',
            '\tMessage("older");',
            '</search>',
            '<replace>',
            '\tMessage("new");',
            '</replace>',
            '</diff>',
        ].join('\n'),
    );

    const message = formatDiffErrorMessage(result);

    assert.ok(message);
    assert.doesNotMatch(message, /\*\*/);
});
