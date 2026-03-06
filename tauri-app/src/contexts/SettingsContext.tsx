import React, { createContext, useContext, useEffect, useState } from 'react';
import * as api from '../api';

import { AppSettings } from '../types/settings';

interface SettingsContextType {
    settings: AppSettings | null;
    loadSettings: () => Promise<void>;
    updateSettings: (newSettings: AppSettings) => Promise<void>;
}

const SettingsContext = createContext<SettingsContextType | undefined>(undefined);

export function SettingsProvider({ children }: { children: React.ReactNode }) {
    const [settings, setSettings] = useState<AppSettings | null>(null);

    const loadSettings = React.useCallback(async () => {
        try {
            const data = await api.getSettings();
            setSettings(data);
        } catch (e) {
            console.error("Failed to load settings:", e);
        }
    }, []);

    const updateSettings = React.useCallback(async (newSettings: AppSettings) => {
        try {
            await api.saveSettings(newSettings);
            setSettings(newSettings);
        } catch (e) {
            console.error("Failed to save settings:", e);
            throw e;
        }
    }, []);

    useEffect(() => {
        loadSettings();
    }, [loadSettings]);

    const value = React.useMemo(() => ({
        settings,
        loadSettings,
        updateSettings
    }), [settings, loadSettings, updateSettings]);

    return (
        <SettingsContext.Provider value={value}>
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
