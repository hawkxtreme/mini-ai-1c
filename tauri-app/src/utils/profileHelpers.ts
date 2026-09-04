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
 * Generates the compact model label for the chat input button (e.g. "o3-mini (medium)" or "gpt-4o").
 */
export function formatProfileModelLabel(
    profile?: Pick<LLMProfile, 'name' | 'model' | 'reasoning_effort' | 'enable_thinking'> | null
): string {
    if (!profile) return 'Модель';
    const rawModel = (profile.model || '').trim();
    const displayName = rawModel || (profile.name || '').trim() || 'Модель';
    const reasoning = getProfileReasoningLevel(profile);
    if (reasoning) {
        return `${displayName} (${reasoning})`;
    }
    return displayName;
}

/**
 * Generates the detailed tooltip for the chat profile button.
 */
export function formatProfileTooltip(
    profile?: Pick<LLMProfile, 'name' | 'provider' | 'model' | 'reasoning_effort' | 'enable_thinking'> | null,
    behaviorPreset?: string | null
): string {
    const lines: string[] = [];
    if (profile) {
        if (profile.name) {
            lines.push(`Профиль: ${profile.name}`);
        }
        if (profile.provider) {
            lines.push(`Провайдер: ${profile.provider}`);
        }
        if (profile.model) {
            lines.push(`Модель: ${profile.model}`);
        }
        const reasoning = getProfileReasoningLevel(profile);
        if (reasoning) {
            lines.push(`Рассуждения: ${reasoning}`);
        }
    } else {
        lines.push('Профиль не выбран');
    }

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
    if (profile.model) {
        const reasoning = getProfileReasoningLevel(profile);
        parts.push(reasoning ? `${profile.model} (${reasoning})` : profile.model);
    }
    return parts.join(' • ');
}
