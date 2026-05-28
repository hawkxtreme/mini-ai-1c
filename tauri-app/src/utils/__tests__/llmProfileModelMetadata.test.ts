import test from 'node:test';
import assert from 'node:assert/strict';

import {
    applyFetchedModelMetadata,
    applySelectedModelMetadata,
} from '../llmProfileModelMetadata';

test('selecting a model preserves user configured max tokens', () => {
    const profile = {
        id: 'profile_1',
        name: 'LM Studio',
        provider: 'LMStudio',
        model: 'old-model',
        api_key_encrypted: 'set',
        base_url: 'http://localhost:1234/v1',
        max_tokens: 4096,
        context_window_override: 32768,
        temperature: 0.7,
    };

    const updated = applySelectedModelMetadata(profile, {
        id: 'new-model',
        context_window: 131072,
    });

    assert.equal(updated.model, 'new-model');
    assert.equal(updated.max_tokens, 4096);
    assert.equal(updated.context_window_override, 131072);
});

test('refreshing metadata preserves user configured max tokens', () => {
    const profile = {
        id: 'profile_1',
        name: 'Ollama',
        provider: 'Ollama',
        model: 'qwen3',
        api_key_encrypted: '',
        base_url: 'http://localhost:11434',
        max_tokens: 2048,
        context_window_override: 8192,
        temperature: 0.7,
    };

    const updated = applyFetchedModelMetadata(profile, {
        id: 'qwen3',
        context_window: 65536,
    });

    assert.equal(updated.max_tokens, 2048);
    assert.equal(updated.context_window_override, 65536);
});

test('selecting a model with syncMaxTokens updates max tokens to context window', () => {
    const profile = {
        id: 'profile_1',
        name: 'OpenAI',
        provider: 'OpenAI',
        model: 'gpt-4o-mini',
        api_key_encrypted: 'set',
        base_url: 'https://api.openai.com/v1',
        max_tokens: 4096,
        context_window_override: 16384,
        temperature: 0.7,
    };

    const updated = applySelectedModelMetadata(
        profile,
        {
            id: 'gpt-4o',
            context_window: 128000,
        },
        { syncMaxTokens: true },
    );

    assert.equal(updated.model, 'gpt-4o');
    assert.equal(updated.max_tokens, 128000);
    assert.equal(updated.context_window_override, 128000);
});

test('syncMaxTokens does nothing when context_window is missing', () => {
    const profile = {
        id: 'profile_1',
        name: 'LM Studio',
        provider: 'LMStudio',
        model: 'old-model',
        api_key_encrypted: '',
        base_url: 'http://localhost:1234/v1',
        max_tokens: 4096,
        context_window_override: 32768,
        temperature: 0.7,
    };

    const updated = applySelectedModelMetadata(
        profile,
        { id: 'new-model' },
        { syncMaxTokens: true },
    );

    assert.equal(updated.model, 'new-model');
    assert.equal(updated.max_tokens, 4096);
    assert.equal(updated.context_window_override, 32768);
});

