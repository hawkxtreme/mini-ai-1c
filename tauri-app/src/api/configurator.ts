import { invoke } from '@tauri-apps/api/core';

export interface WindowInfo {
    hwnd: number;
    title: string;
}

/**
 * Find 1C Configurator windows matching the pattern
 */
export async function findConfiguratorWindows(pattern: string): Promise<WindowInfo[]> {
    return await invoke<WindowInfo[]>('find_configurator_windows_cmd', { pattern });
}

/**
 * Get code from specific Configurator window
 * @param hwnd Window handle
 * @param useSelectAll If true, sends Ctrl+A before Copy
 */
export async function getCodeFromConfigurator(hwnd: number, useSelectAll: boolean = false): Promise<string> {
    return await invoke<string>('get_code_from_configurator', { hwnd, useSelectAll });
}

/**
 * Get active fragment (selection or current line)
 */
export async function getActiveFragment(hwnd: number): Promise<string> {
    return await invoke<string>('get_active_fragment_cmd', { hwnd });
}

/**
 * Paste code to specific Configurator window
 * @param hwnd Window handle
 * @param code Code to paste
 * @param useSelectAll If true, sends Ctrl+A before Paste (replacing everything)
 */
export async function pasteCodeToConfigurator(hwnd: number, code: string, useSelectAll: boolean = false): Promise<void> {
    return await invoke('paste_code_to_configurator', { hwnd, code, useSelectAll });
}

/**
 * Undo last code change in specific Configurator window
 */
export async function undoLastChange(hwnd: number): Promise<void> {
    return await invoke('undo_last_change', { hwnd });
}
