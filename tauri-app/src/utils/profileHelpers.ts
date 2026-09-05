import type { LLMProfile } from '../api/profiles';

/**
 * Признак облачного профиля Ollama (ollama.com).
 *
 * Принимает профиль с явным provider='OllamaCloud' (новые профили) ИЛИ
 * legacy-вариант: provider='Ollama' с base_url, указывающим на ollama.com.
 * Это нужно чтобы не сломать существующие профили, созданные через
 * Custom/Ollama до выделения провайдера.
 */
export function isOllamaCloudProfile(p: Pick<LLMProfile, 'provider' | 'base_url'>): boolean {
    if (p.provider === 'OllamaCloud') return true;
    if (p.provider === 'Ollama' && (p.base_url || '').includes('ollama.com')) return true;
    return false;
}

export type ProfileGroup = 'standard' | 'cli' | 'naparnik' | 'ollama-cloud';

export function getProfileGroup(p: Pick<LLMProfile, 'provider' | 'base_url'>): ProfileGroup {
    if (p.provider === 'OneCNaparnik') return 'naparnik';
    if (p.provider === 'QwenCli' || p.provider === 'CodexCli') return 'cli';
    if (isOllamaCloudProfile(p)) return 'ollama-cloud';
    return 'standard';
}

/**
 * Returns reasoning level string ('low', 'medium', 'high', 'thinking', etc.) or null if not applicable.
 */
export function getProfileReasoningLevel(
    profile?: Pick<LLMProfile, 'reasoning_effort' | 'enable_thinking'> | null
): string | null {
    if (!profile) return null;
    if (profile.reasoning_effort && profile.reasoning_effort !== 'none') {
        return profile.reasoning_effort;
    }
    if (profile.enable_thinking) {
        return 'thinking';
    }
    return null;
}

/**
 * Base helper: formats a model name with an optional reasoning level.
 * Returns null if both model and fallback name are empty.
 */
export function formatModelLabel(
    model?: string | null,
    reasoningLevel?: string | null,
    fallbackName?: string | null
): string | null {
    const rawModel = (model || '').trim();
    const displayName = rawModel || (fallbackName || '').trim();
    if (!displayName) return null;
    const reasoning = (reasoningLevel || '').trim();
    if (reasoning && reasoning !== 'none') {
        return `${displayName} (${reasoning})`;
    }
    return displayName;
}

export interface ModelMetadataDetails {
    model?: string | null;
    reasoningLevel?: string | null;
    provider?: string | null;
    profileName?: string | null;
}

export interface AssistantMessageMetadata {
    model?: string;
    reasoningLevel?: string;
    provider?: string;
    profileName?: string;
}

/**
 * Resolves metadata fields to attach to an assistant response message from the active/requested LLM profile.
 */
export function resolveAssistantMessageMetadata(
    profile?: LLMProfile | null
): AssistantMessageMetadata {
    if (!profile) return {};
    const modelName = profile.model?.trim() || profile.name?.trim();
    const reasoningLevel = getProfileReasoningLevel(profile);
    const provider = profile.provider?.trim();
    const profileName = profile.name?.trim();

    return {
        ...(modelName ? { model: modelName } : {}),
        ...(reasoningLevel ? { reasoningLevel } : {}),
        ...(provider ? { provider } : {}),
        ...(profileName ? { profileName } : {}),
    };
}

/**
 * Base helper: builds formatted metadata lines (Profile, Provider, Model, Reasoning).
 */
export function buildModelDetailsLines(info?: ModelMetadataDetails | null): string[] {
    if (!info) return [];
    const lines: string[] = [];
    if (info.profileName?.trim()) {
        lines.push(`Профиль: ${info.profileName.trim()}`);
    }
    if (info.provider?.trim()) {
        lines.push(`Провайдер: ${info.provider.trim()}`);
    }
    if (info.model?.trim()) {
        lines.push(`Модель: ${info.model.trim()}`);
    }
    const reasoning = info.reasoningLevel?.trim();
    if (reasoning && reasoning !== 'none') {
        lines.push(`Рассуждения: ${reasoning}`);
    }
    return lines;
}

/**
 * Generates the compact model label for the chat input button (e.g. "o3-mini (medium)" or "gpt-4o").
 */
export function formatProfileModelLabel(
    profile?: Pick<LLMProfile, 'name' | 'model' | 'reasoning_effort' | 'enable_thinking'> | null
): string {
    return (
        formatModelLabel(
            profile?.model,
            getProfileReasoningLevel(profile),
            profile?.name
        ) || 'Модель'
    );
}

/**
 * Generates the detailed tooltip for the chat profile button.
 */
export function formatProfileTooltip(
    profile?: Pick<LLMProfile, 'name' | 'provider' | 'model' | 'reasoning_effort' | 'enable_thinking'> | null,
    behaviorPreset?: string | null
): string {
    const lines: string[] = profile
        ? buildModelDetailsLines({
            profileName: profile.name,
            provider: profile.provider,
            model: profile.model,
            reasoningLevel: getProfileReasoningLevel(profile),
        })
        : ['Профиль не выбран'];

    if (behaviorPreset) {
        const modeLabel = behaviorPreset === 'maintenance'
            ? 'ЧУЖОЙ (исправление)'
            : behaviorPreset === 'project'
                ? 'СВОЙ (разработка)'
                : behaviorPreset;
        lines.push(`Режим: ${modeLabel}`);
    }

    return lines.join('\n');
}
/**
 * Generates formatted summary for dropdown profile items (e.g. "CodexCli • gpt-4o (high)").
 */
export function formatProfileSummary(
    profile: Pick<LLMProfile, 'provider' | 'model' | 'reasoning_effort' | 'enable_thinking'>
): string {
    const parts: string[] = [];
    if (profile.provider) {
        parts.push(profile.provider);
    }
    const modelLabel = formatModelLabel(profile.model, getProfileReasoningLevel(profile));
    if (modelLabel) {
        parts.push(modelLabel);
    }
    return parts.join(' • ');
}
