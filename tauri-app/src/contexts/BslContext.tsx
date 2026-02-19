import React, { createContext, useContext, useEffect, useState, useCallback, useMemo } from 'react';
import * as api from '../api';
import { emit } from '@tauri-apps/api/event';

export interface BslStatus {
    installed: boolean;
    java_info: string;
    connected: boolean;
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

    const checkStatus = async () => {
        try {
            const data = await api.checkBslStatus();
            setStatus(data);
        } catch (e) {
            console.error("Failed to check Bsl Status", e);
        }
    };

    useEffect(() => {
        checkStatus();
        const interval = setInterval(checkStatus, 5000); // Polling every 5s
        return () => clearInterval(interval);
    }, []);

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
        resetDiffBase
    }), [status, analyzeCode, formatCode, resetDiffBase]);

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
