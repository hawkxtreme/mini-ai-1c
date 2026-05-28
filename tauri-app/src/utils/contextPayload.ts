import type { ChatToolCall } from '../api/chat';

export interface PayloadChatMessage {
    role: 'user' | 'assistant' | 'tool' | 'system';
    content: string;
    tool_calls?: ChatToolCall[];
    tool_call_id?: string;
    name?: string;
}

export interface ClampPayloadResult<T extends PayloadChatMessage> {
    messages: T[];
    wasClamped: boolean;
}

const TOKEN_CHAR_RATIO = 4;
const SAFETY_TOKENS = 256;
const MIN_CURRENT_MESSAGE_CHARS = 1200;

export function estimateChatMessageTokens(messages: PayloadChatMessage[]): number {
    return messages.reduce((sum, message) => {
        const contentChars = message.content?.length ?? 0;
        const toolCallChars = message.tool_calls?.reduce((toolSum, toolCall) => {
            return toolSum + toolCall.function.name.length + toolCall.function.arguments.length + 10;
        }, 0) ?? 0;

        return sum + Math.ceil((contentChars + toolCallChars) / TOKEN_CHAR_RATIO);
    }, 0);
}

function clampTextMiddle(text: string, maxChars: number): string {
    if (text.length <= maxChars) {
        return text;
    }

    const marker = '\n\n[Контекст текущего сообщения усечен автоматически: исходный блок слишком большой для окна модели. Сохранены начало и конец.]\n\n';
    const availableChars = Math.max(0, maxChars - marker.length);

    if (availableChars <= 0) {
        return marker.trim();
    }

    const headChars = Math.ceil(availableChars * 0.65);
    const tailChars = availableChars - headChars;

    return `${text.slice(0, headChars)}${marker}${tailChars > 0 ? text.slice(-tailChars) : ''}`;
}

function removeOldestNonSystemBeforeCurrent<T extends PayloadChatMessage>(
    messages: T[],
    currentIndex: number,
    maxTokens: number
): T[] {
    let result = messages;

    while (estimateChatMessageTokens(result) > maxTokens && result.length > 1) {
        const removableIndex = result.findIndex((message, index) => {
            return index !== currentIndex && message.role !== 'system';
        });

        if (removableIndex === -1) {
            break;
        }

        result = result.filter((_, index) => index !== removableIndex);
        currentIndex = removableIndex < currentIndex ? currentIndex - 1 : currentIndex;
    }

    return result;
}

export function clampPayloadToBudget<T extends PayloadChatMessage>(
    messages: T[],
    currentMessage: T,
    maxTokens: number
): ClampPayloadResult<T> {
    if (maxTokens <= 0 || estimateChatMessageTokens(messages) <= maxTokens) {
        return { messages, wasClamped: false };
    }

    const currentIndex = messages.lastIndexOf(currentMessage);
    if (currentIndex === -1) {
        return { messages, wasClamped: false };
    }

    const otherMessages = messages.filter((_, index) => index !== currentIndex);
    const otherTokens = estimateChatMessageTokens(otherMessages);
    const currentBudgetTokens = Math.max(
        Math.min(maxTokens - otherTokens - SAFETY_TOKENS, maxTokens - SAFETY_TOKENS),
        Math.ceil(MIN_CURRENT_MESSAGE_CHARS / TOKEN_CHAR_RATIO)
    );
    const maxCurrentChars = Math.max(MIN_CURRENT_MESSAGE_CHARS, currentBudgetTokens * TOKEN_CHAR_RATIO);

    let result = messages.map((message, index) => {
        if (index !== currentIndex) {
            return message;
        }

        return {
            ...message,
            content: clampTextMiddle(message.content ?? '', maxCurrentChars),
        };
    }) as T[];

    result = removeOldestNonSystemBeforeCurrent(result, currentIndex, maxTokens);

    return {
        messages: result,
        wasClamped: true,
    };
}

