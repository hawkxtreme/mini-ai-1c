import test from 'node:test';
import assert from 'node:assert/strict';
import { normalizeProxyPortInput } from '../proxySettings';

test('normalizeProxyPortInput accepts blank and valid TCP port values', () => {
    assert.equal(normalizeProxyPortInput(''), null);
    assert.equal(normalizeProxyPortInput('  '), null);
    assert.equal(normalizeProxyPortInput('1'), 1);
    assert.equal(normalizeProxyPortInput('1080'), 1080);
    assert.equal(normalizeProxyPortInput('65535'), 65535);
});

test('normalizeProxyPortInput rejects invalid or out-of-range values', () => {
    assert.equal(normalizeProxyPortInput('0'), undefined);
    assert.equal(normalizeProxyPortInput('65536'), undefined);
    assert.equal(normalizeProxyPortInput('70000'), undefined);
    assert.equal(normalizeProxyPortInput('1.5'), undefined);
    assert.equal(normalizeProxyPortInput('1e3'), undefined);
    assert.equal(normalizeProxyPortInput('abc'), undefined);
});
