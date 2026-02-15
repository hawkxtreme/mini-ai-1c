import { invoke } from '@tauri-apps/api/core';

export interface LLMProfile {
    id: string;
    name: string;
    provider: string;
    model: string;
    api_key_encrypted: string;
    base_url: string | null;
    max_tokens: number;
    temperature: number;
    context_window_override?: number;
}

export interface ProfileStore {
    profiles: LLMProfile[];
    active_profile_id: string;
}

/**
 * Get all LLM profiles
 */
export async function getProfiles(): Promise<ProfileStore> {
    return await invoke<ProfileStore>('get_profiles');
}

/**
 * Save a profile
 * @param profile The profile data
 * @param apiKey Optional API key to update
 */
export async function saveProfile(profile: LLMProfile, apiKey?: string): Promise<void> {
    return await invoke('save_profile', { profile, apiKey });
}

/**
 * Delete a profile
 */
export async function deleteProfile(profileId: string): Promise<void> {
    return await invoke('delete_profile', { profileId });
}

/**
 * Set active profile
 */
export async function setActiveProfile(profileId: string): Promise<void> {
    return await invoke('set_active_profile', { profileId });
}

/**
 * Fetch available models for a specific profile
 */
export async function fetchModelsForProfile(profileId: string): Promise<string[]> {
    // Note: The backend command returns Vec<Model> struct, but frontend mapping might differ. 
    // Let's check commands.rs again. 
    // commands.rs: fetch_models_for_profile -> Vec<crate::llm::providers::Model>
    // Model struct has id, name, etc.
    // However, the `fetch_models_cmd` (old one) returns Vec<String>.
    // Let's stick to the consistent `fetch_models_cmd` if it's used, or `fetch_models_for_profile`.
    // In App.tsx: invoke('fetch_models_cmd', { profileId }) -> returns Strings? 
    // Checking App.tsx (it wasn't fully visible but I saw `fetch_models_cmd` in `commands.rs`)
    // commands.rs: pub async fn fetch_models_cmd(profile_id: String) -> Result<Vec<String>, String>
    return await invoke<string[]>('fetch_models_cmd', { profileId });
}

/**
 * Test connection for a profile
 */
export async function testConnection(profileId: string): Promise<string> {
    return await invoke<string>('test_llm_connection_cmd', { profileId });
}
