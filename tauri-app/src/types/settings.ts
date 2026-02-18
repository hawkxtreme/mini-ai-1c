export interface WindowInfo {
    hwnd: number;
    title: string;
}

export interface BslStatus {
    installed: boolean;
    java_info: string;
    connected: boolean;
}

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
    mcp_servers: {
        id: string;
        name: string;
        enabled: boolean;
        transport: 'http' | 'stdio' | 'internal';
        url?: string | null;
        login?: string | null;
        password?: string | null;
        command?: string | null;
        args?: string[] | null;
    }[];
    active_llm_profile: string;
    debug_mcp: boolean;
}

export interface BslDiagnosticItem {
    status: 'ok' | 'warn' | 'error';
    title: string;
    message: string;
    suggestion?: string;
}
