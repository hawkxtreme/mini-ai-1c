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
 * @param originalContent Original content for conflict detection
 */
export async function pasteCodeToConfigurator(
    hwnd: number,
    code: string,
    useSelectAll: boolean = false,
    originalContent?: string
): Promise<void> {
    return await invoke('paste_code_to_configurator', {
        hwnd,
        code,
        useSelectAll,
        originalContent: originalContent ?? null,
    });
}

/**
 * Undo last code change in specific Configurator window
 */
export async function undoLastChange(hwnd: number): Promise<void> {
    return await invoke('undo_last_change', { hwnd });
}

/**
 * Check if there is an active selection in the window
 */
export async function checkSelectionState(hwnd: number): Promise<boolean> {
    return await invoke<boolean>('check_selection_state', { hwnd });
}

/**
 * Align active Configurator window and AI window
 */
export async function alignWithConfigurator(hwnd: number): Promise<void> {
    return await invoke('align_with_configurator', { hwnd });
}
