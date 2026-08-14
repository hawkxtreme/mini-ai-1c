import React, { createContext, useContext, useEffect, useState, useCallback, useMemo } from 'react';
import * as api from '../api';
import { emit, listen } from '@tauri-apps/api/event';

export interface BslStatus {
    installed: boolean;
    java_info: string;
    runtime_info: string;
    server_version: string;
    server_path: string;
    workspace_path: string;
    active_port: number;
    connected: boolean;
    mcp_available: boolean;
}

interface BslContextType {
    status: BslStatus | null;
    checkStatus: () => Promise<void>;
    analyzeCode: (code: string) => Promise<api.BslDiagnostic[]>;
    formatCode: (code: string) => Promise<string>;
    resetDiffBase: (code: string) => Promise<void>;
}

const BslContext = createContext<BslContextType | undefined>(undefined);

export function BslProvider({ children }: { children: React.ReactNode }) {
    const [status, setStatus] = useState<BslStatus | null>(null);

    const statusRef = React.useRef<BslStatus | null>(null);
    statusRef.current = status;
    const lastCheckTimeRef = React.useRef(0);

    const checkStatus = useCallback(async () => {
        const now = Date.now();
        // Пропускаем проверку если уже подключены и прошло меньше 10с
        if (statusRef.current?.connected && now - lastCheckTimeRef.current < 10000) {
            return;
        }
        try {
            const data = await api.checkBslStatus();
            setStatus(data);
            lastCheckTimeRef.current = Date.now();
        } catch (e) {
            console.error("Failed to check Bsl Status", e);
            // При ошибке сбрасываем статус
            setStatus(prev => prev ? { ...prev, connected: false } : null);
        }
    }, []);

    useEffect(() => {
        checkStatus();
        const interval = setInterval(checkStatus, 15000); // Polling every 15s instead of 5s

        // Reactive: listen for backend state change events
        const unlistenPromise = listen<string>('bsl-ls-state', () => {
            // Reset throttle so the next checkStatus actually fires
            lastCheckTimeRef.current = 0;
            checkStatus();
        });

        return () => {
            clearInterval(interval);
            unlistenPromise.then(fn => fn());
        };
    }, [checkStatus]);

    const analyzeCode = useCallback(async (code: string) => {
        return await api.analyzeBsl(code);
    }, []);

    const formatCode = useCallback(async (code: string) => {
        return await api.formatBsl(code);
    }, []);

    const resetDiffBase = useCallback(async (code: string) => {
        await emit('RESET_DIFF', code);
    }, []);

    const contextValue = useMemo(() => ({
        status,
        checkStatus,
        analyzeCode,
        formatCode,
        resetDiffBase,
    }), [status, checkStatus, analyzeCode, formatCode, resetDiffBase]);

    return (
        <BslContext.Provider value={contextValue}>
            {children}
        </BslContext.Provider>
    );
}

export function useBsl() {
    const context = useContext(BslContext);
    if (context === undefined) {
        throw new Error('useBsl must be used within a BslProvider');
    }
    return context;
}
