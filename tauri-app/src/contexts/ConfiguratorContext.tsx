import React, { createContext, useContext, useState, useEffect, useCallback, useMemo } from 'react';
import * as api from '../api';
import { useSettings } from './SettingsContext';
import { parseConfiguratorTitle } from '../utils/configurator';

export interface WindowInfo {
    hwnd: number;
    title: string;
}

interface ConfiguratorContextType {
    detectedWindows: WindowInfo[];
    selectedHwnd: number | null;
    refreshWindows: () => Promise<void>;
    selectWindow: (hwnd: number) => Promise<void>;
    getCode: (useSelectAll: boolean) => Promise<string>;
    pasteCode: (code: string, useSelectAll: boolean, originalContent?: string) => Promise<void>;
    checkSelection: () => Promise<boolean>;
    snapToConfigurator: () => Promise<void>;
    activeConfigTitle: string;
}

const ConfiguratorContext = createContext<ConfiguratorContextType | undefined>(undefined);

export function ConfiguratorProvider({ children }: { children: React.ReactNode }) {
    const { settings, updateSettings } = useSettings();
    const [detectedWindows, setDetectedWindows] = useState<WindowInfo[]>([]);

    const selectedHwnd = settings?.configurator.selected_window_hwnd || null;
    const pattern = settings?.configurator.window_title_pattern || 'Конфигуратор';

    const refreshWindows = useCallback(async () => {
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
    }, [pattern, selectedHwnd, settings, updateSettings]);

    // Initial refresh when settings are loaded
    useEffect(() => {
        if (settings) {
            refreshWindows();
        }
    }, [settings?.configurator.window_title_pattern]); // Refresh if pattern changes

    const selectWindow = useCallback(async (hwnd: number) => {
        if (!settings) return;
        const newSettings = {
            ...settings,
            configurator: { ...settings.configurator, selected_window_hwnd: hwnd }
        };
        await updateSettings(newSettings);
    }, [settings, updateSettings]);

    const activeConfigTitle = useMemo(() => {
        if (!selectedHwnd) return "Конфигуратор";
        const win = detectedWindows.find(w => w.hwnd === selectedHwnd);
        if (!win) return "Конфигуратор";
        return parseConfiguratorTitle(win.title);
    }, [selectedHwnd, detectedWindows]);

    const getCode = useCallback(async (useSelectAll: boolean): Promise<string> => {
        let targetHwnd = selectedHwnd;

        // Auto-select if not selected but only one found
        if (!targetHwnd) {
            const windows = await api.findConfiguratorWindows(pattern);
            if (windows.length > 0) targetHwnd = windows[0].hwnd;
        }

        if (!targetHwnd) throw new Error("No Configurator window selected");

        return await api.getCodeFromConfigurator(targetHwnd, useSelectAll);
    }, [selectedHwnd, pattern]);

    const pasteCode = useCallback(async (code: string, useSelectAll: boolean, originalContent?: string) => {
        let targetHwnd = selectedHwnd;

        if (!targetHwnd) {
            const windows = await api.findConfiguratorWindows(pattern);
            if (windows.length > 0) targetHwnd = windows[0].hwnd;
        }

        if (!targetHwnd) throw new Error("No Configurator window selected");

        await api.pasteCodeToConfigurator(targetHwnd, code, useSelectAll, originalContent);
    }, [selectedHwnd, pattern]);

    const checkSelection = useCallback(async (): Promise<boolean> => {
        let targetHwnd = selectedHwnd;
        if (!targetHwnd) {
            const windows = await api.findConfiguratorWindows(pattern);
            if (windows.length > 0) targetHwnd = windows[0].hwnd;
        }
        if (!targetHwnd) return false;
        return await api.checkSelectionState(targetHwnd);
    }, [selectedHwnd, pattern]);

    const snapToConfigurator = useCallback(async () => {
        let targetHwnd = selectedHwnd;
        if (!targetHwnd) {
            const windows = await api.findConfiguratorWindows(pattern);
            if (windows.length > 0) targetHwnd = windows[0].hwnd;
        }

        if (!targetHwnd) {
            console.warn("No Configurator window found for snapping");
            return;
        }

        try {
            await api.alignWithConfigurator(targetHwnd);
        } catch (e) {
            console.error("Failed to snap window", e);
        }
    }, [selectedHwnd, pattern]);

    const contextValue = useMemo(() => ({
        detectedWindows,
        selectedHwnd,
        refreshWindows,
        selectWindow,
        getCode,
        pasteCode,
        checkSelection,
        snapToConfigurator,
        activeConfigTitle
    }), [detectedWindows, selectedHwnd, refreshWindows, selectWindow, getCode, pasteCode, checkSelection, snapToConfigurator, activeConfigTitle]);

    return (
        <ConfiguratorContext.Provider value={contextValue}>
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
