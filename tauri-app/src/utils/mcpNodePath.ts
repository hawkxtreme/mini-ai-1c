export const DEFAULT_NODE_PATH = 'node';

export function normalizeNodePath(nodePath: string | null | undefined): string {
    const trimmed = nodePath?.trim() ?? '';
    return trimmed.length > 0 ? trimmed : DEFAULT_NODE_PATH;
}

export function getNodePathInputValue(nodePath: string | null | undefined): string {
    const trimmed = nodePath?.trim() ?? '';
    return trimmed.length > 0 && trimmed !== DEFAULT_NODE_PATH ? trimmed : '';
}

export function getNodePathPreview(
    nodePath: string | null | undefined,
    detectedNodePath: string | null | undefined,
): string {
    const customNodePath = getNodePathInputValue(nodePath);
    if (customNodePath) return customNodePath;

    const detected = detectedNodePath?.trim() ?? '';
    return detected.length > 0 ? detected : DEFAULT_NODE_PATH;
}

export function isBuiltinNodeLauncher(command: string | null | undefined, nodePath: string | null | undefined): boolean {
    const trimmed = command?.trim() ?? '';
    if (!trimmed) return false;

    const configuredNode = normalizeNodePath(nodePath);
    const normalizedCommand = trimmed.replace(/\\/g, '/').toLowerCase();

    return trimmed === configuredNode
        || normalizedCommand === 'node'
        || normalizedCommand === 'node.exe'
        || normalizedCommand === 'npx'
        || normalizedCommand === 'npx.cmd'
        || normalizedCommand.endsWith('/node')
        || normalizedCommand.endsWith('/node.exe')
        || normalizedCommand.includes('tsx');
}
