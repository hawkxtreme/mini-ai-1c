import test from 'node:test';
import assert from 'node:assert/strict';
import { clampPayloadToBudget, estimateChatMessageTokens, type PayloadChatMessage } from '../contextPayload';

test('clampPayloadToBudget trims oversized current user payload even when history is small', () => {
    const hugeCode = 'Процедура Большая()\n' + 'Сообщить("x");\n'.repeat(12000) + 'КонецПроцедуры';
    const current: PayloadChatMessage = {
        role: 'user',
        content: `исправь\n\n=== CURRENT CODE CONTEXT ===\n\`\`\`bsl\n${hugeCode}\n\`\`\``,
    };
    const messages: PayloadChatMessage[] = [
        { role: 'user', content: 'короткий вопрос' },
        { role: 'assistant', content: 'короткий ответ' },
        current,
    ];

    const result = clampPayloadToBudget(messages, current, 8000);

    assert.equal(result.wasClamped, true);
    assert.ok(estimateChatMessageTokens(result.messages) <= 8000);
    const lastMessage = result.messages[result.messages.length - 1];
    assert.match(lastMessage?.content ?? '', /CURRENT CODE CONTEXT/);
    assert.match(lastMessage?.content ?? '', /усечен/);
});
