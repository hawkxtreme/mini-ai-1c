import test from 'node:test';
import assert from 'node:assert/strict';

import { createLatencyStats } from '../performanceDiagnostics';

test('latency stats tracks p50 p95 max and sample count', () => {
    const stats = createLatencyStats(10);

    for (const value of [10, 20, 30, 40, 50]) {
        stats.record(value);
    }

    assert.deepEqual(stats.snapshot(), {
        count: 5,
        p50: 30,
        p95: 50,
        max: 50,
    });
});

test('latency stats keeps only the newest samples', () => {
    const stats = createLatencyStats(3);

    for (const value of [10, 20, 30, 40]) {
        stats.record(value);
    }

    assert.deepEqual(stats.snapshot(), {
        count: 3,
        p50: 30,
        p95: 40,
        max: 40,
    });
});
