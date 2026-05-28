import test from 'node:test';
import assert from 'node:assert/strict';
import { getStreamingAutoScrollTop, isChatNearBottom } from '../chatAutoScroll';

test('isChatNearBottom treats small distance from bottom as pinned to latest message', () => {
    assert.equal(
        isChatNearBottom({ scrollTop: 895, scrollHeight: 1200, clientHeight: 220 }, 100),
        true,
    );
});

test('isChatNearBottom detects when the user intentionally scrolled away from latest messages', () => {
    assert.equal(
        isChatNearBottom({ scrollTop: 620, scrollHeight: 1200, clientHeight: 220 }, 100),
        false,
    );
});

test('getStreamingAutoScrollTop keeps streaming content pinned to the current bottom', () => {
    assert.equal(
        getStreamingAutoScrollTop({ scrollTop: 900, scrollHeight: 1240, clientHeight: 220 }),
        1020,
    );
});

test('getStreamingAutoScrollTop does not force scrolling when the user is reading older messages', () => {
    assert.equal(
        getStreamingAutoScrollTop({ scrollTop: 620, scrollHeight: 1240, clientHeight: 220 }),
        null,
    );
});
