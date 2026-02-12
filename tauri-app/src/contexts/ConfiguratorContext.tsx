import React, { createContext, useContext, useState, useEffect } from 'react';
import * as api from '../api';
import { useSettings } from './SettingsContext';

export interface WindowInfo {
    hwnd: number;
    title: string;
}

interface ConfiguratorContextType {
    detectedWindows: WindowInfo[];
    selectedHwnd: number | null;
    refreshWindows: () => Promise<void>;
    selectWindow: (hwnd: number) => Promise<void>;
    getActiveConfiguratorTitle: () => string;
    getCode: (useSelectAll: boolean) => Promise<string>;
    pasteCode: (code: string, useSelectAll: boolean) => Promise<void>;
}

const ConfiguratorContext = createContext<ConfiguratorContextType | undefined>(undefined);

export function ConfiguratorProvider({ children }: { children: React.ReactNode }) {
    const { settings, updateSettings } = useSettings();
    const [detectedWindows, setDetectedWindows] = useState<WindowInfo[]>([]);

    const selectedHwnd = settings?.configurator.selected_window_hwnd || null;
    const pattern = settings?.configurator.window_title_pattern || 'Конфигуратор';

    const refreshWindows = async () => {
        try {
            const windows = await api.findConfiguratorWindows(pattern);
            setDetectedWindows(windows);

            // Auto-cleanup stale selection
            if (selectedHwnd) {
                const stillExists = windows.some(w => w.hwnd === selectedHwnd);
                if (!stillExists && settings) {
                    const newSet = { ...settings, configurator: { ...settings.configurator, selected_window_hwnd: null } };
                    updateSettings(newSet);
                }
            }
        } catch (e) {
            console.error("Failed to find windows", e);
        }
    };

    // Initial refresh when settings are loaded
    useEffect(() => {
        if (settings) {
            refreshWindows();
        }
    }, [settings?.configurator.window_title_pattern]); // Refresh if pattern changes

    const selectWindow = async (hwnd: number) => {
        if (!settings) return;
        const newSettings = {
            ...settings,
            configurator: { ...settings.configurator, selected_window_hwnd: hwnd }
        };
        await updateSettings(newSettings);
    };

    const getActiveConfiguratorTitle = () => {
        if (!selectedHwnd) return "Конфигуратор";
        const win = detectedWindows.find(w => w.hwnd === selectedHwnd);
        return win ? win.title : `ID: ${selectedHwnd} (Не найден)`;
    };

    const getCode = async (useSelectAll: boolean): Promise<string> => {
        let targetHwnd = selectedHwnd;

        // Auto-select if not selected but only one found
        if (!targetHwnd) {
            const windows = await api.findConfiguratorWindows(pattern);
            if (windows.length > 0) targetHwnd = windows[0].hwnd;
        }

        if (!targetHwnd) throw new Error("No Configurator window selected");

        return await api.getCodeFromConfigurator(targetHwnd, useSelectAll);
    };

    const pasteCode = async (code: string, useSelectAll: boolean) => {
        let targetHwnd = selectedHwnd;

        if (!targetHwnd) {
            const windows = await api.findConfiguratorWindows(pattern);
            if (windows.length > 0) targetHwnd = windows[0].hwnd;
        }

        if (!targetHwnd) throw new Error("No Configurator window selected");

        await api.pasteCodeToConfigurator(targetHwnd, code, useSelectAll);
    };

    return (
        <ConfiguratorContext.Provider value={{
            detectedWindows,
            selectedHwnd,
            refreshWindows,
            selectWindow,
            getActiveConfiguratorTitle,
            getCode,
            pasteCode
        }}>
            {children}
        </ConfiguratorContext.Provider>
    );
}

export function useConfigurator() {
    const context = useContext(ConfiguratorContext);
    if (context === undefined) {
        throw new Error('useConfigurator must be used within a ConfiguratorProvider');
    }
    return context;
}
