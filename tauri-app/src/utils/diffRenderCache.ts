export interface DiffRenderSummaryCache<T> {
    get(baseKey: string, contentKey: string, compute: () => T): T;
    clear(): void;
}

export interface TextFingerprintCache {
    get(text: string): string;
    clear(): void;
    size(): number;
}

interface CacheEntry<T> {
    value: T;
    lastUsed: number;
}

const FNV_OFFSET = 0x811c9dc5;
const FNV_PRIME = 0x01000193;

export function createTextFingerprint(text: string): string {
    let hash = FNV_OFFSET;
    for (let i = 0; i < text.length; i += 1) {
        hash ^= text.charCodeAt(i);
        hash = Math.imul(hash, FNV_PRIME);
    }
    return `${text.length}:${(hash >>> 0).toString(36)}`;
}

export function createDiffRenderSummaryCache<T>(maxEntries = 120): DiffRenderSummaryCache<T> {
    const entries = new Map<string, CacheEntry<T>>();
    let tick = 0;

    const evictIfNeeded = () => {
        if (entries.size <= maxEntries) return;

        let oldestKey: string | null = null;
        let oldestUsed = Number.POSITIVE_INFINITY;
        for (const [key, entry] of entries) {
            if (entry.lastUsed < oldestUsed) {
                oldestUsed = entry.lastUsed;
                oldestKey = key;
            }
        }

        if (oldestKey !== null) {
            entries.delete(oldestKey);
        }
    };

    return {
        get(baseKey, contentKey, compute) {
            const key = `${baseKey}\u0000${contentKey}`;
            const existing = entries.get(key);
            tick += 1;

            if (existing) {
                existing.lastUsed = tick;
                return existing.value;
            }

            const value = compute();
            entries.set(key, { value, lastUsed: tick });
            evictIfNeeded();
            return value;
        },
        clear() {
            entries.clear();
        },
    };
}

export function createTextFingerprintCache(maxEntries = 300): TextFingerprintCache {
    const entries = new Map<string, CacheEntry<string>>();
    let tick = 0;

    const evictIfNeeded = () => {
        if (entries.size <= maxEntries) return;

        let oldestKey: string | null = null;
        let oldestUsed = Number.POSITIVE_INFINITY;
        for (const [key, entry] of entries) {
            if (entry.lastUsed < oldestUsed) {
                oldestUsed = entry.lastUsed;
                oldestKey = key;
            }
        }

        if (oldestKey !== null) {
            entries.delete(oldestKey);
        }
    };

    return {
        get(text) {
            const existing = entries.get(text);
            tick += 1;

            if (existing) {
                existing.lastUsed = tick;
                return existing.value;
            }

            const fingerprint = createTextFingerprint(text);
            entries.set(text, { value: fingerprint, lastUsed: tick });
            evictIfNeeded();
            return fingerprint;
        },
        clear() {
            entries.clear();
        },
        size() {
            return entries.size;
        },
    };
}
