import test from 'node:test';
import assert from 'node:assert/strict';
import { buildPayloadMessages, ChatMessage } from '../ChatContext';

test('buildPayloadMessages excludes internal tool calls from assistant message payload', () => {
    const messages: ChatMessage[] = [
        {
            id: 'user-1',
            role: 'user',
            content: 'Please check this code',
            timestamp: 1000,
        },
        {
            id: 'asst-1',
            role: 'assistant',
            content: 'I noticed an error in your code.',
            timestamp: 1001,
            toolCalls: [
                {
                    id: 'auto_check_bsl_syntax_1',
                    name: 'check_bsl_syntax',
                    arguments: '{"code":"test"}',
                    status: 'done',
                    result: '{"success":false,"diagnostics":[]}',
                    internal: true,
                }
            ]
        }
    ];

    const payload = buildPayloadMessages(messages, 'user-1', 'Please check this code');

    // The user message is included
    assert.equal(payload.length, 2);
    assert.equal(payload[0].role, 'user');
    assert.equal(payload[0].content, 'Please check this code');

    // The assistant message is included as text only, without tool_calls
    assert.equal(payload[1].role, 'assistant');
    assert.equal(payload[1].content, 'I noticed an error in your code.');
    assert.equal(payload[1].tool_calls, undefined);
});

test('buildPayloadMessages omits assistant message completely if it only had internal tool calls and no text', () => {
    const messages: ChatMessage[] = [
        {
            id: 'user-1',
            role: 'user',
            content: 'Hello',
            timestamp: 1000,
        },
        {
            id: 'asst-1',
            role: 'assistant',
            content: '',
            timestamp: 1001,
            toolCalls: [
                {
                    id: 'auto_check_bsl_syntax_1',
                    name: 'check_bsl_syntax',
                    arguments: '{"code":"test"}',
                    status: 'done',
                    result: '{"success":true}',
                    internal: true,
                }
            ]
        }
    ];

    const payload = buildPayloadMessages(messages, 'user-1', 'Hello');

    // Only user message remains
    assert.equal(payload.length, 1);
    assert.equal(payload[0].role, 'user');
});

test('buildPayloadMessages keeps non-internal tool calls and thought_signature', () => {
    const messages: ChatMessage[] = [
        {
            id: 'user-1',
            role: 'user',
            content: 'Find something',
            timestamp: 1000,
        },
        {
            id: 'asst-1',
            role: 'assistant',
            content: '',
            timestamp: 1001,
            toolCalls: [
                {
                    id: 'internal_call_1',
                    name: 'check_bsl_syntax',
                    arguments: '{"code":"test"}',
                    status: 'done',
                    result: '{"success":true}',
                    internal: true,
                },
                {
                    id: 'call_external_1',
                    name: 'search_code',
                    arguments: '{"query":"find me"}',
                    status: 'done',
                    result: '{"matches":[]}',
                    thought_signature: 'sig_abc_123',
                }
            ]
        }
    ];

    const payload = buildPayloadMessages(messages, 'user-1', 'Find something');

    // Expected: user message, assistant message with only external tool call, tool result message
    assert.equal(payload.length, 3);
    assert.equal(payload[0].role, 'user');
    assert.equal(payload[1].role, 'assistant');
    assert.equal(payload[1].tool_calls?.length, 1);
    assert.equal(payload[1].tool_calls?.[0].id, 'call_external_1');
    assert.equal(payload[1].tool_calls?.[0].extra_content?.google?.thought_signature, 'sig_abc_123');

    assert.equal(payload[2].role, 'tool');
    assert.equal(payload[2].tool_call_id, 'call_external_1');
    assert.equal(payload[2].content, '{"matches":[]}');
});
