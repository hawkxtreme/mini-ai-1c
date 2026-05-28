import test from 'node:test';
import assert from 'node:assert/strict';
import {
    getNodePathInputValue,
    getNodePathPreview,
    normalizeNodePath,
    isBuiltinNodeLauncher,
} from '../mcpNodePath';

test('normalizeNodePath falls back to node for blank values', () => {
    assert.equal(normalizeNodePath(''), 'node');
    assert.equal(normalizeNodePath('   '), 'node');
    assert.equal(normalizeNodePath(null), 'node');
});

test('isBuiltinNodeLauncher accepts a configured portable node executable', () => {
    const portableNode = String.raw`C:\portable\node\node.exe`;

    assert.equal(isBuiltinNodeLauncher(portableNode, portableNode), true);
    assert.equal(isBuiltinNodeLauncher('node', portableNode), true);
    assert.equal(isBuiltinNodeLauncher('npx', portableNode), true);
    assert.equal(isBuiltinNodeLauncher(String.raw`C:\tools\other.exe`, portableNode), false);
});

test('getNodePathInputValue keeps default node empty for an unset custom path', () => {
    assert.equal(getNodePathInputValue(''), '');
    assert.equal(getNodePathInputValue(null), '');
    assert.equal(getNodePathInputValue('node'), '');
    assert.equal(getNodePathInputValue('  node  '), '');
    assert.equal(
        getNodePathInputValue(String.raw`D:\tools\node\node.exe`),
        String.raw`D:\tools\node\node.exe`,
    );
});

test('getNodePathPreview shows detected node path when no custom path is selected', () => {
    assert.equal(
        getNodePathPreview('node', String.raw`C:\Program Files\nodejs\node.exe`),
        String.raw`C:\Program Files\nodejs\node.exe`,
    );
    assert.equal(getNodePathPreview('', null), 'node');
    assert.equal(
        getNodePathPreview(String.raw`D:\portable\node.exe`, String.raw`C:\Program Files\nodejs\node.exe`),
        String.raw`D:\portable\node.exe`,
    );
});
