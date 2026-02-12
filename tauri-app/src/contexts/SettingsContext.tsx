import React, { createContext, useContext, useEffect, useState } from 'react';
import * as api from '../api';

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

interface SettingsContextType {
    settings: AppSettings | null;
    loadSettings: () => Promise<void>;
    updateSettings: (newSettings: AppSettings) => Promise<void>;
}

const SettingsContext = createContext<SettingsContextType | undefined>(undefined);

export function SettingsProvider({ children }: { children: React.ReactNode }) {
    const [settings, setSettings] = useState<AppSettings | null>(null);

    const loadSettings = async () => {
        try {
            const data = await api.getSettings();
            setSettings(data);
        } catch (e) {
            console.error("Failed to load settings:", e);
        }
    };

    const updateSettings = async (newSettings: AppSettings) => {
        try {
            await api.saveSettings(newSettings);
            setSettings(newSettings);
        } catch (e) {
            console.error("Failed to save settings:", e);
            throw e;
        }
    };

    useEffect(() => {
        loadSettings();
    }, []);

    return (
        <SettingsContext.Provider value={{ settings, loadSettings, updateSettings }}>
            {children}
        </SettingsContext.Provider>
    );
}

export function useSettings() {
    const context = useContext(SettingsContext);
    if (context === undefined) {
        throw new Error('useSettings must be used within a SettingsProvider');
    }
    return context;
}
