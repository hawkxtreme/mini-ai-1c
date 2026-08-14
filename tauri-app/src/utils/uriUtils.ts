export function normalizeUri(uri: string): string {
    if (!uri) return uri;
    let decoded = uri;
    try {
        decoded = decodeURIComponent(uri);
    } catch {
        // ignore
    }
    decoded = decoded.replace(/\\/g, '/');
    return decoded.replace(/^file:\/\/\/([a-zA-Z]):/, (_, drive) => `file:///${drive.toLowerCase()}:`);
}
