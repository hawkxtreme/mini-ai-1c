import type { ChatMessage } from '../contexts/ChatContext';

export interface ChatSessionStats {
    messageCount: number;
    lineCount: number;
    byteCount: number;
    tokenEstimate: number;
}

const TOKEN_CHAR_RATIO = 4;
const BYTE_UNITS = ['Б', 'КБ', 'МБ'];

function getMessageText(message: ChatMessage): string {
    return message.displayContent ?? message.content ?? '';
}

function countTextLines(text: string): number {
    if (!text) return 0;
    return text.split(/\r\n|\r|\n/).length;
}

function formatCompactNumber(value: number): string {
    if (value < 1000) return String(value);
    if (value < 10_000) return `${(value / 1000).toFixed(1).replace('.', ',')} тыс.`;
    return `${Math.round(value / 1000)} тыс.`;
}

function formatLineCount(value: number): string {
    if (value >= 1000) return `${formatCompactNumber(value)} строк`;

    const mod10 = value % 10;
    const mod100 = value % 100;
    const unit = mod10 === 1 && mod100 !== 11
        ? 'строка'
        : mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)
            ? 'строки'
            : 'строк';

    return `${value} ${unit}`;
}

export function estimateChatSessionStats(messages: ChatMessage[]): ChatSessionStats {
    const textMessages = messages.filter((message) =>
        (message.role === 'user' || message.role === 'assistant') && message.variant == null
    );
    const combinedText = textMessages.map(getMessageText).filter(Boolean).join('\n');
    const charCount = combinedText.length;

    return {
        messageCount: textMessages.length,
        lineCount: textMessages.reduce((sum, message) => sum + countTextLines(getMessageText(message)), 0),
        byteCount: new TextEncoder().encode(combinedText).length,
        tokenEstimate: Math.ceil(charCount / TOKEN_CHAR_RATIO),
    };
}

export function formatByteSize(byteCount: number): string {
    if (byteCount <= 0) return '0 Б';

    let value = byteCount;
    let unitIndex = 0;
    while (value >= 1024 && unitIndex < BYTE_UNITS.length - 1) {
        value /= 1024;
        unitIndex += 1;
    }

    const formatted = value >= 10 || unitIndex === 0
        ? Math.round(value).toString()
        : value.toFixed(1).replace('.', ',');

    return `${formatted} ${BYTE_UNITS[unitIndex]}`;
}

export function formatChatSessionStats(messages: ChatMessage[]): string {
    const stats = estimateChatSessionStats(messages);
    return `${formatLineCount(stats.lineCount)} · ${formatByteSize(stats.byteCount)} · ~${formatCompactNumber(stats.tokenEstimate)} ток.`;
}
