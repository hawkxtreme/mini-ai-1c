export function normalizeProxyPortInput(value: string): number | null | undefined {
    const trimmed = value.trim();
    if (trimmed === '') return null;
    if (!/^\d+$/.test(trimmed)) return undefined;

    const port = Number(trimmed);
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
        return undefined;
    }

    return port;
}
