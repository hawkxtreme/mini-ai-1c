import { invoke } from '@tauri-apps/api/core';

export interface LatencyStatsSnapshot {
    count: number;
    p50: number;
    p95: number;
    max: number;
}

export interface LatencyStats {
    record(value: number): void;
    snapshot(): LatencyStatsSnapshot;
}

interface LongTaskRecord {
    startedAt: number;
    duration: number;
    name?: string;
}

interface EventLoopLagRecord {
    at: number;
    lagMs: number;
}

declare global {
    interface Window {
        __MINI_AI_PERF__?: {
            recordInputLatency: (source: string, durationMs: number) => void;
            markInputLatency: (source: string) => void;
            snapshot: () => unknown;
            flushToLog: (reason?: string) => Promise<void>;
        };
    }
}

const INPUT_SAMPLE_LIMIT = 120;
const LONG_TASK_LIMIT = 60;
const EVENT_LOOP_LAG_LIMIT = 60;
const LOG_LATENCY_THRESHOLD_MS = 120;
const LOG_EVENT_LOOP_LAG_THRESHOLD_MS = 250;

export function createLatencyStats(limit = INPUT_SAMPLE_LIMIT): LatencyStats {
    const samples: number[] = [];

    const percentile = (sorted: number[], ratio: number) => {
        if (sorted.length === 0) return 0;
        const index = Math.min(sorted.length - 1, Math.ceil(sorted.length * ratio) - 1);
        return Math.round(sorted[index]);
    };

    return {
        record(value) {
            if (!Number.isFinite(value) || value < 0) return;
            samples.push(value);
            if (samples.length > limit) {
                samples.splice(0, samples.length - limit);
            }
        },
        snapshot() {
            const sorted = [...samples].sort((a, b) => a - b);
            return {
                count: samples.length,
                p50: percentile(sorted, 0.5),
                p95: percentile(sorted, 0.95),
                max: sorted.length ? Math.round(sorted[sorted.length - 1]) : 0,
            };
        },
    };
}

const inputStats = new Map<string, LatencyStats>();
const longTasks: LongTaskRecord[] = [];
const eventLoopLags: EventLoopLagRecord[] = [];
let installed = false;
let lastHeartbeat = 0;

function getStats(source: string) {
    let stats = inputStats.get(source);
    if (!stats) {
        stats = createLatencyStats();
        inputStats.set(source, stats);
    }
    return stats;
}

function pushLimited<T>(target: T[], value: T, limit: number) {
    target.push(value);
    if (target.length > limit) {
        target.splice(0, target.length - limit);
    }
}

function writePerfLog(message: string) {
    invoke('write_frontend_log', { message }).catch(() => {
        // Best-effort diagnostics must never affect the UI path.
    });
}

export function recordInputLatency(source: string, durationMs: number) {
    getStats(source).record(durationMs);
    if (durationMs >= LOG_LATENCY_THRESHOLD_MS) {
        writePerfLog(`[PERF][INPUT] source=${source} duration_ms=${Math.round(durationMs)}`);
    }
}

export function markInputLatency(source: string) {
    if (typeof window === 'undefined' || typeof performance === 'undefined') return;

    const startedAt = performance.now();
    window.requestAnimationFrame(() => {
        recordInputLatency(source, performance.now() - startedAt);
    });
}

export function getPerformanceDiagnosticsSnapshot() {
    const inputLatency: Record<string, LatencyStatsSnapshot> = {};
    for (const [source, stats] of inputStats) {
        inputLatency[source] = stats.snapshot();
    }

    return {
        createdAt: new Date().toISOString(),
        inputLatency,
        longTasks: [...longTasks],
        eventLoopLags: [...eventLoopLags],
    };
}

export async function flushPerformanceDiagnosticsToLog(reason = 'manual') {
    const snapshot = getPerformanceDiagnosticsSnapshot();
    await invoke('write_frontend_log', {
        message: `[PERF][SNAPSHOT][${reason}] ${JSON.stringify(snapshot)}`,
    });
}

export function installPerformanceDiagnostics() {
    if (installed || typeof window === 'undefined' || typeof performance === 'undefined') return;
    installed = true;

    window.__MINI_AI_PERF__ = {
        recordInputLatency,
        markInputLatency,
        snapshot: getPerformanceDiagnosticsSnapshot,
        flushToLog: flushPerformanceDiagnosticsToLog,
    };

    if ('PerformanceObserver' in window) {
        try {
            const observer = new PerformanceObserver((list) => {
                for (const entry of list.getEntries()) {
                    pushLimited(longTasks, {
                        startedAt: Math.round(entry.startTime),
                        duration: Math.round(entry.duration),
                        name: entry.name,
                    }, LONG_TASK_LIMIT);
                }
            });
            observer.observe({ entryTypes: ['longtask'] });
        } catch {
            // WebView2 can miss longtask support. The heartbeat below still catches event-loop stalls.
        }
    }

    lastHeartbeat = performance.now();
    window.setInterval(() => {
        const now = performance.now();
        const lagMs = now - lastHeartbeat - 1000;
        lastHeartbeat = now;

        if (lagMs >= LOG_EVENT_LOOP_LAG_THRESHOLD_MS) {
            const roundedLag = Math.round(lagMs);
            pushLimited(eventLoopLags, {
                at: Date.now(),
                lagMs: roundedLag,
            }, EVENT_LOOP_LAG_LIMIT);
            writePerfLog(`[PERF][EVENT_LOOP_LAG] lag_ms=${roundedLag}`);
        }
    }, 1000);
}
