import test from 'node:test';
import assert from 'node:assert/strict';

import {
    createDiffRenderSummaryCache,
    createTextFingerprint,
    createTextFingerprintCache,
} from '../diffRenderCache';

test('diff render summary cache reuses a computed value for the same base and content keys', () => {
    const cache = createDiffRenderSummaryCache<string>(4);
    let computeCount = 0;

    const first = cache.get('base-a', 'message-1', () => {
        computeCount += 1;
        return 'computed';
    });
    const second = cache.get('base-a', 'message-1', () => {
        computeCount += 1;
        return 'recomputed';
    });

    assert.equal(first, 'computed');
    assert.equal(second, 'computed');
    assert.equal(computeCount, 1);
});

test('diff render summary cache separates entries by base key', () => {
    const cache = createDiffRenderSummaryCache<string>(4);
    let computeCount = 0;

    const first = cache.get('base-a', 'message-1', () => {
        computeCount += 1;
        return 'for-a';
    });
    const second = cache.get('base-b', 'message-1', () => {
        computeCount += 1;
        return 'for-b';
    });

    assert.equal(first, 'for-a');
    assert.equal(second, 'for-b');
    assert.equal(computeCount, 2);
});

test('diff render summary cache evicts least recently used entries', () => {
    const cache = createDiffRenderSummaryCache<string>(2);

    cache.get('base', 'a', () => 'a1');
    cache.get('base', 'b', () => 'b1');
    cache.get('base', 'a', () => 'a2');
    cache.get('base', 'c', () => 'c1');

    assert.equal(cache.get('base', 'a', () => 'a2'), 'a1');
    assert.equal(cache.get('base', 'b', () => 'b2'), 'b2');
});

test('text fingerprint includes content changes with equal length', () => {
    assert.notEqual(createTextFingerprint('abc'), createTextFingerprint('abd'));
    assert.equal(createTextFingerprint('abc'), createTextFingerprint('abc'));
});

test('text fingerprint cache reuses fingerprints for repeated text', () => {
    const cache = createTextFingerprintCache(2);

    const first = cache.get('module text');
    const second = cache.get('module text');

    assert.equal(first, second);
    assert.equal(cache.size(), 1);
});
