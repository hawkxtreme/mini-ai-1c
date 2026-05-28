import test from 'node:test';
import assert from 'node:assert/strict';
import {
    estimateChatSessionStats,
    formatByteSize,
    formatChatSessionStats,
} from '../chatSessionStats';
import type { ChatMessage } from '../../contexts/ChatContext';

function message(partial: Partial<ChatMessage>): ChatMessage {
    return {
        id: partial.id ?? Math.random().toString(36).slice(2),
        role: partial.role ?? 'user',
        content: partial.content ?? '',
        timestamp: partial.timestamp ?? 1,
        ...partial,
    };
}

test('estimateChatSessionStats counts visible chat lines, bytes and approximate tokens', () => {
    const messages: ChatMessage[] = [
        message({ role: 'user', content: 'Первая строка\nВторая строка' }),
        message({ role: 'assistant', content: 'Ответ ассистента' }),
    ];

    const stats = estimateChatSessionStats(messages);

    assert.equal(stats.messageCount, 2);
    assert.equal(stats.lineCount, 3);
    assert.equal(
        stats.byteCount,
        new TextEncoder().encode('Первая строка\nВторая строка\nОтвет ассистента').length,
    );
    assert.equal(
        stats.tokenEstimate,
        Math.ceil('Первая строка\nВторая строка\nОтвет ассистента'.length / 4),
    );
});

test('estimateChatSessionStats ignores technical and variant messages', () => {
    const messages: ChatMessage[] = [
        message({ role: 'system', content: 'system text' }),
        message({ role: 'tool', content: 'tool result' }),
        message({ role: 'assistant', variant: 'warning', content: 'warning text' }),
        message({ role: 'user', content: 'видимый текст' }),
    ];

    const stats = estimateChatSessionStats(messages);

    assert.equal(stats.messageCount, 1);
    assert.equal(stats.lineCount, 1);
    assert.equal(stats.tokenEstimate, Math.ceil('видимый текст'.length / 4));
});

test('formatByteSize uses compact Russian byte units', () => {
    assert.equal(formatByteSize(0), '0 Б');
    assert.equal(formatByteSize(512), '512 Б');
    assert.equal(formatByteSize(1536), '1,5 КБ');
    assert.equal(formatByteSize(12 * 1024), '12 КБ');
});

test('formatChatSessionStats produces a compact history label', () => {
    const messages: ChatMessage[] = [
        message({ role: 'user', content: 'строка 1\nстрока 2' }),
        message({ role: 'assistant', content: 'ответ' }),
    ];

    assert.match(formatChatSessionStats(messages), /^3 строки · .+ · ~\d+ ток\.$/);
});
