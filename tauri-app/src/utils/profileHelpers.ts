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
