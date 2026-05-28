export interface ChatScrollMetrics {
    scrollTop: number;
    scrollHeight: number;
    clientHeight: number;
}

export const DEFAULT_CHAT_BOTTOM_THRESHOLD = 160;

export function getChatMaxScrollTop(metrics: ChatScrollMetrics): number {
    return Math.max(0, metrics.scrollHeight - metrics.clientHeight);
}

export function getChatBottomDistance(metrics: ChatScrollMetrics): number {
    return Math.max(0, getChatMaxScrollTop(metrics) - metrics.scrollTop);
}

export function isChatNearBottom(
    metrics: ChatScrollMetrics,
    threshold = DEFAULT_CHAT_BOTTOM_THRESHOLD,
): boolean {
    return getChatBottomDistance(metrics) <= threshold;
}

export function getStreamingAutoScrollTop(
    metrics: ChatScrollMetrics,
    threshold = DEFAULT_CHAT_BOTTOM_THRESHOLD,
): number | null {
    if (!isChatNearBottom(metrics, threshold)) {
        return null;
    }

    return getChatMaxScrollTop(metrics);
}
