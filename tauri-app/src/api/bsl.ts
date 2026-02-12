import { invoke } from '@tauri-apps/api/core';

export interface BslStatus {
    installed: boolean;
    java_info: string;
    connected: boolean;
}

export interface BslDiagnostic {
    line: number;
    character: number;
    message: string;
    severity: string;
}

/**
 * Check BSL LS status
 */
export async function checkBslStatus(): Promise<BslStatus> {
    return await invoke<BslStatus>('check_bsl_status_cmd');
}

/**
 * Install BSL Language Server
 */
export async function installBslLs(): Promise<string> {
    return await invoke<string>('install_bsl_ls_cmd');
}

/**
 * Reconnect BSL Language Server
 */
export async function reconnectBslLs(): Promise<void> {
    return await invoke('reconnect_bsl_ls_cmd');
}

/**
 * Analyze BSL code
 */
export async function analyzeBsl(code: string): Promise<BslDiagnostic[]> {
    return await invoke<BslDiagnostic[]>('analyze_bsl', { code });
}

/**
 * Format BSL code
 */
export async function formatBsl(code: string): Promise<string> {
    return await invoke<string>('format_bsl', { code });
}

/**
 * Diagnose BSL LS launch issues
 */
export async function diagnoseBslLs(): Promise<string> {
    return await invoke<string>('diagnose_bsl_ls_cmd');
}
