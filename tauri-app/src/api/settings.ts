import { invoke } from '@tauri-apps/api/core';

export interface AppSettings {
    configurator: {
        window_title_pattern: string;
        selected_window_hwnd: number | null;
    };
    bsl_server: {
        jar_path: string;
        websocket_port: number;
        enabled: boolean;
        java_path: string;
        auto_download: boolean;
    };
}

/**
 * Get application settings
 */
export async function getSettings(): Promise<AppSettings> {
    return await invoke<AppSettings>('get_settings');
}

/**
 * Save application settings
 */
export async function saveSettings(newSettings: AppSettings): Promise<void> {
    return await invoke('save_settings', { newSettings });
}
