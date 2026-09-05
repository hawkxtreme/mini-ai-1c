import test from 'node:test';
import assert from 'node:assert/strict';
import {
    getProfileReasoningLevel,
    formatModelLabel,
    buildModelDetailsLines,
    formatProfileModelLabel,
    formatProfileTooltip,
    formatProfileSummary,
    resolveAssistantMessageMetadata,
} from '../profileHelpers';

test('getProfileReasoningLevel returns reasoning_effort when set and not none', () => {
    assert.equal(getProfileReasoningLevel({ reasoning_effort: 'medium' }), 'medium');
    assert.equal(getProfileReasoningLevel({ reasoning_effort: 'high' }), 'high');
    assert.equal(getProfileReasoningLevel({ reasoning_effort: 'none' }), null);
    assert.equal(getProfileReasoningLevel({ reasoning_effort: undefined }), null);
    assert.equal(getProfileReasoningLevel(null), null);
    assert.equal(getProfileReasoningLevel(undefined), null);
});

test('getProfileReasoningLevel returns thinking when enable_thinking is true', () => {
    assert.equal(getProfileReasoningLevel({ enable_thinking: true }), 'thinking');
    assert.equal(getProfileReasoningLevel({ enable_thinking: false }), null);
    // If both reasoning_effort and enable_thinking exist, reasoning_effort takes precedence
    assert.equal(getProfileReasoningLevel({ reasoning_effort: 'low', enable_thinking: true }), 'low');
});

test('formatProfileModelLabel formats standard model without reasoning', () => {
    assert.equal(formatProfileModelLabel({ name: 'Default', model: 'gpt-4o' }), 'gpt-4o');
});

test('formatProfileModelLabel formats model with reasoning effort', () => {
    assert.equal(
        formatProfileModelLabel({ name: 'Codex', model: 'o3-mini', reasoning_effort: 'medium' }),
        'o3-mini (medium)'
    );
});

test('formatProfileModelLabel formats model with thinking enabled', () => {
    assert.equal(
        formatProfileModelLabel({ name: 'Qwen', model: 'qwen-max', enable_thinking: true }),
        'qwen-max (thinking)'
    );
});

test('formatProfileModelLabel ignores reasoning_effort if set to none', () => {
    assert.equal(
        formatProfileModelLabel({ name: 'Default', model: 'gpt-4o', reasoning_effort: 'none' }),
        'gpt-4o'
    );
});

test('formatProfileModelLabel falls back to profile name when model is empty', () => {
    assert.equal(
        formatProfileModelLabel({ name: 'Custom Assistant', model: '', reasoning_effort: 'high' }),
        'Custom Assistant (high)'
    );
    assert.equal(
        formatProfileModelLabel({ name: 'My Profile', model: '   ' }),
        'My Profile'
    );
});

test('formatProfileModelLabel falls back to default placeholder when profile is missing', () => {
    assert.equal(formatProfileModelLabel(null), 'Модель');
    assert.equal(formatProfileModelLabel(undefined), 'Модель');
    assert.equal(formatProfileModelLabel({ name: '', model: '' }), 'Модель');
});

test('formatProfileTooltip generates multi-line details with reasoning and preset', () => {
    const tooltip = formatProfileTooltip(
        {
            name: 'Codex Fast',
            provider: 'CodexCli',
            model: 'o3-mini',
            reasoning_effort: 'medium',
        },
        'project'
    );

    assert.equal(
        tooltip,
        'Профиль: Codex Fast\nПровайдер: CodexCli\nМодель: o3-mini\nРассуждения: medium\nРежим: СВОЙ (разработка)'
    );
});

test('formatProfileTooltip handles maintenance mode preset and missing profile', () => {
    const tooltipMaint = formatProfileTooltip(
        {
            name: 'Qwen',
            provider: 'QwenCli',
            model: 'qwen-max',
            enable_thinking: true,
        },
        'maintenance'
    );
    assert.ok(tooltipMaint.includes('Рассуждения: thinking'));
    assert.ok(tooltipMaint.includes('Режим: ЧУЖОЙ (исправление)'));

    const tooltipNull = formatProfileTooltip(null, null);
    assert.equal(tooltipNull, 'Профиль не выбран');
});

test('formatProfileSummary builds combined provider and model with reasoning', () => {
    assert.equal(
        formatProfileSummary({
            provider: 'CodexCli',
            model: 'o3-mini',
            reasoning_effort: 'high',
        }),
        'CodexCli • o3-mini (high)'
    );

    assert.equal(
        formatProfileSummary({
            provider: 'QwenCli',
            model: 'qwen-max',
            enable_thinking: true,
        }),
        'QwenCli • qwen-max (thinking)'
    );

    assert.equal(
        formatProfileSummary({
            provider: 'OpenAI',
            model: 'gpt-4o',
        }),
        'OpenAI • gpt-4o'
    );
});

test('formatModelLabel formats with reasoning, fallbacks, and edge cases', () => {
    assert.equal(formatModelLabel('o3-mini', 'high'), 'o3-mini (high)');
    assert.equal(formatModelLabel('o3-mini', 'none'), 'o3-mini');
    assert.equal(formatModelLabel('', 'low', 'Fallback'), 'Fallback (low)');
    assert.equal(formatModelLabel(null, null, null), null);
    // Cases previously covered by formatMessageModelLabel wrapper:
    assert.equal(formatModelLabel('o3-mini', 'medium'), 'o3-mini (medium)');
    assert.equal(formatModelLabel('qwen-max', 'thinking'), 'qwen-max (thinking)');
    assert.equal(formatModelLabel('gpt-4o', 'none'), 'gpt-4o');
    assert.equal(formatModelLabel('gpt-4o', undefined), 'gpt-4o');
    assert.equal(formatModelLabel('gpt-4o', null), 'gpt-4o');
    assert.equal(formatModelLabel('', 'high', 'Fallback Profile'), 'Fallback Profile (high)');
    assert.equal(formatModelLabel(undefined, undefined, 'Fallback Profile'), 'Fallback Profile');
    assert.equal(formatModelLabel('', '', ''), null);
    assert.equal(formatModelLabel(null, null, null), null);
    assert.equal(formatModelLabel(undefined, undefined, undefined), null);
});

test('buildModelDetailsLines formats metadata lines and joins for tooltips', () => {
    const lines = buildModelDetailsLines({
        profileName: 'My Profile',
        provider: 'OpenAI',
        model: 'o3-mini',
        reasoningLevel: 'high',
    });
    assert.deepEqual(lines, [
        'Профиль: My Profile',
        'Провайдер: OpenAI',
        'Модель: o3-mini',
        'Рассуждения: high',
    ]);
    assert.deepEqual(buildModelDetailsLines(null), []);
    assert.deepEqual(buildModelDetailsLines({}), []);
    // Cases previously covered by formatMessageModelTooltip wrapper (join('\n')):
    assert.equal(
        buildModelDetailsLines({
            profileName: 'OpenAI Fast',
            provider: 'OpenAI',
            model: 'o3-mini',
            reasoningLevel: 'medium',
        }).join('\n'),
        'Профиль: OpenAI Fast\nПровайдер: OpenAI\nМодель: o3-mini\nРассуждения: medium'
    );
    assert.equal(
        buildModelDetailsLines({ model: 'gpt-4o' }).join('\n'),
        'Модель: gpt-4o'
    );
    assert.equal(
        buildModelDetailsLines({ model: 'gpt-4o', reasoningLevel: 'none' }).join('\n'),
        'Модель: gpt-4o'
    );
});

test('resolveAssistantMessageMetadata resolves metadata from LLM profile correctly', () => {
    assert.deepEqual(resolveAssistantMessageMetadata(null), {});
    assert.deepEqual(resolveAssistantMessageMetadata(undefined), {});

    const standardProfile = {
        id: 'p1',
        name: 'GPT-4o Main',
        provider: 'OpenAI',
        model: 'gpt-4o',
        api_key_encrypted: '',
        base_url: null,
        max_tokens: 4096,
        temperature: 0.7,
    };
    assert.deepEqual(resolveAssistantMessageMetadata(standardProfile), {
        model: 'gpt-4o',
        provider: 'OpenAI',
        profileName: 'GPT-4o Main',
    });

    const reasoningProfile = {
        id: 'p2',
        name: 'Codex Fast',
        provider: 'CodexCli',
        model: 'o3-mini',
        reasoning_effort: 'high' as const,
        api_key_encrypted: '',
        base_url: null,
        max_tokens: 4096,
        temperature: 0.2,
    };
    assert.deepEqual(resolveAssistantMessageMetadata(reasoningProfile), {
        model: 'o3-mini',
        reasoningLevel: 'high',
        provider: 'CodexCli',
        profileName: 'Codex Fast',
    });

    const thinkingProfile = {
        id: 'p3',
        name: 'Qwen Reason',
        provider: 'QwenCli',
        model: 'qwen-max',
        enable_thinking: true,
        api_key_encrypted: '',
        base_url: null,
        max_tokens: 4096,
        temperature: 0.1,
    };
    assert.deepEqual(resolveAssistantMessageMetadata(thinkingProfile), {
        model: 'qwen-max',
        reasoningLevel: 'thinking',
        provider: 'QwenCli',
        profileName: 'Qwen Reason',
    });

    const fallbackProfile = {
        id: 'p4',
        name: 'Custom Assistant',
        provider: 'Ollama',
        model: '',
        api_key_encrypted: '',
        base_url: 'http://localhost:11434',
        max_tokens: 2048,
        temperature: 0.5,
    };
    assert.deepEqual(resolveAssistantMessageMetadata(fallbackProfile), {
        model: 'Custom Assistant',
        provider: 'Ollama',
        profileName: 'Custom Assistant',
    });
});

test('profile switching produces distinct and updated assistant metadata', () => {
    const profileA = {
        id: 'p_a',
        name: 'Model A',
        provider: 'OpenAI',
        model: 'gpt-4o',
        api_key_encrypted: '',
        base_url: null,
        max_tokens: 4096,
        temperature: 0.7,
    };
    const profileB = {
        id: 'p_b',
        name: 'Model B',
        provider: 'Anthropic',
        model: 'claude-3-7-sonnet',
        enable_thinking: true,
        api_key_encrypted: '',
        base_url: null,
        max_tokens: 4096,
        temperature: 0.5,
    };

    const metadataA = resolveAssistantMessageMetadata(profileA);
    const metadataB = resolveAssistantMessageMetadata(profileB);

    assert.equal(metadataA.model, 'gpt-4o');
    assert.equal(metadataA.provider, 'OpenAI');
    assert.equal(metadataA.reasoningLevel, undefined);

    assert.equal(metadataB.model, 'claude-3-7-sonnet');
    assert.equal(metadataB.provider, 'Anthropic');
    assert.equal(metadataB.reasoningLevel, 'thinking');

    // Verify formatModelLabel and tooltips for both
    assert.equal(formatModelLabel(metadataA.model, metadataA.reasoningLevel, metadataA.profileName), 'gpt-4o');
    assert.equal(formatModelLabel(metadataB.model, metadataB.reasoningLevel, metadataB.profileName), 'claude-3-7-sonnet (thinking)');
});
