import React, { createContext, useContext, useState, useEffect, useCallback, useMemo } from 'react';
import * as api from '../api';
import { useSettings } from './SettingsContext';
import { parseConfiguratorTitleFull, ConfiguratorTitleContext } from '../utils/configurator';
import {
    areConfiguratorBindingsEqual,
    bindConfiguratorWindow,
    getConfiguratorBindingDisplayTitle,
    resolveConfiguratorBinding,
    type ConfiguratorBindingStatus,
    type ConfiguratorWindowBinding,
} from '../utils/configuratorBinding';

export interface WindowInfo {
    hwnd: number;
    title: string;
    process_id: number;
}

interface ConfiguratorContextType {
    detectedWindows: WindowInfo[];
    selectedHwnd: number | null;
    bindingStatus: ConfiguratorBindingStatus;
    bindingMessage: string | null;
    refreshWindows: () => Promise<void>;
    selectWindow: (window: WindowInfo) => Promise<void>;
    getCode: (useSelectAll: boolean) => Promise<string>;
    pasteCode: (
        code: string,
        useSelectAll: boolean,
        originalContent?: string,
        options?: api.ConfiguratorPasteOptions,
    ) => Promise<void>;
    checkSelection: () => Promise<boolean>;
    snapToConfigurator: () => Promise<void>;
    activeConfigTitle: string;
    parsedTitleContext: ConfiguratorTitleContext | null;
}

const ConfiguratorContext = createContext<ConfiguratorContextType | undefined>(undefined);

export function ConfiguratorProvider({ children }: { children: React.ReactNode }) {
    const { settings, updateSettings } = useSettings();
    const [detectedWindows, setDetectedWindows] = useState<WindowInfo[]>([]);

    const base = settings?.configurator.window_title_pattern || 'Конфигуратор|1C:Enterprise';
    const extras = settings?.configurator.extra_window_title_patterns ?? [];
    const pattern = extras.length > 0 ? `${base}|${extras.join('|')}` : base;
    const currentBinding = useMemo<ConfiguratorWindowBinding>(() => ({
        selected_window_hwnd: settings?.configurator.selected_window_hwnd ?? null,
        selected_window_pid: settings?.configurator.selected_window_pid ?? null,
        selected_window_title: settings?.configurator.selected_window_title ?? null,
        selected_config_name: settings?.configurator.selected_config_name ?? null,
    }), [
        settings?.configurator.selected_window_hwnd,
        settings?.configurator.selected_window_pid,
        settings?.configurator.selected_window_title,
        settings?.configurator.selected_config_name,
    ]);

    const bindingResolution = useMemo(
        () => resolveConfiguratorBinding(currentBinding, detectedWindows),
        [currentBinding, detectedWindows],
    );

    const selectedHwnd = bindingResolution.activeWindow?.hwnd ?? null;

    const persistBinding = useCallback(async (nextBinding: ConfiguratorWindowBinding) => {
        if (!settings) return;
        if (areConfiguratorBindingsEqual(currentBinding, nextBinding)) return;

        await updateSettings({
            ...settings,
            configurator: {
                ...settings.configurator,
                selected_window_hwnd: nextBinding.selected_window_hwnd ?? null,
                selected_window_pid: nextBinding.selected_window_pid ?? null,
                selected_window_title: nextBinding.selected_window_title ?? null,
                selected_config_name: nextBinding.selected_config_name ?? null,
            },
        });
    }, [currentBinding, settings, updateSettings]);

    const buildBindingMessage = useCallback((status: ConfiguratorBindingStatus, candidateCount: number) => {
        const displayTitle = getConfiguratorBindingDisplayTitle(currentBinding, bindingResolution.activeWindow);
        switch (status) {
            case 'resolved':
                return `Выбран Конфигуратор: ${displayTitle}`;
            case 'rebound':
                return `Привязка к Конфигуратору восстановлена: ${displayTitle}`;
            case 'missing':
                return `Выбранный Конфигуратор недоступен: ${displayTitle}. Откройте его снова или выберите окно заново.`;
            case 'ambiguous':
                return `Найдено несколько окон для выбранного Конфигуратора (${candidateCount}). Выберите нужное окно вручную.`;
            case 'unselected':
            default:
                return null;
        }
    }, [bindingResolution.activeWindow, currentBinding]);

    const resolveActiveWindowOrThrow = useCallback(async (): Promise<WindowInfo> => {
        const windows = await api.findConfiguratorWindows(pattern);
        setDetectedWindows(windows);

        const resolution = resolveConfiguratorBinding(currentBinding, windows);
        if (!areConfiguratorBindingsEqual(currentBinding, resolution.nextBinding)) {
            await persistBinding(resolution.nextBinding);
        }

        if (resolution.activeWindow) {
            return resolution.activeWindow;
        }

        const displayTitle = getConfiguratorBindingDisplayTitle(currentBinding);
        switch (resolution.status) {
            case 'ambiguous':
                throw new Error(`Найдено несколько окон для выбранного Конфигуратора "${displayTitle}". Выберите нужное окно вручную.`);
            case 'missing':
                throw new Error(`Выбранный Конфигуратор "${displayTitle}" сейчас недоступен. Откройте его снова или выберите окно заново.`);
            case 'unselected':
            default:
                throw new Error('Сначала выберите окно Конфигуратора.');
        }
    }, [currentBinding, pattern, persistBinding]);

    const refreshWindows = useCallback(async () => {
        try {
            const windows = await api.findConfiguratorWindows(pattern);
            setDetectedWindows(windows);
            const resolution = resolveConfiguratorBinding(currentBinding, windows);
            if (!areConfiguratorBindingsEqual(currentBinding, resolution.nextBinding)) {
                await persistBinding(resolution.nextBinding);
            }
            // Auto-select the single available window when nothing is bound
            if (resolution.status === 'unselected' && windows.length === 1) {
                await persistBinding(bindConfiguratorWindow(windows[0]));
            }
        } catch (e) {
            console.error("Failed to find windows", e);
        }
    }, [currentBinding, pattern, persistBinding]);

    // Initial refresh when settings are loaded, and periodic re-scan every 30s.
    // This ensures newly opened Configurator windows appear without manual refresh.
    useEffect(() => {
        if (!settings) return;
        void refreshWindows();
        const interval = setInterval(() => {
            void refreshWindows();
        }, 30_000);
        return () => clearInterval(interval);
    }, [settings, refreshWindows]);

    const selectWindow = useCallback(async (window: WindowInfo) => {
        if (!settings) return;
        const nextBinding = bindConfiguratorWindow(window);
        const newSettings = {
            ...settings,
            configurator: {
                ...settings.configurator,
                selected_window_hwnd: nextBinding.selected_window_hwnd,
                selected_window_pid: nextBinding.selected_window_pid ?? null,
                selected_window_title: nextBinding.selected_window_title ?? null,
                selected_config_name: nextBinding.selected_config_name ?? null,
            }
        };
        await updateSettings(newSettings);
    }, [settings, updateSettings]);

    const activeConfigTitle = useMemo(() => {
        return getConfiguratorBindingDisplayTitle(currentBinding, bindingResolution.activeWindow);
    }, [bindingResolution.activeWindow, currentBinding]);

    const parsedTitleContext = useMemo<ConfiguratorTitleContext | null>(() => {
        if (bindingResolution.activeWindow) {
            return parseConfiguratorTitleFull(bindingResolution.activeWindow.title);
        }

        if (currentBinding.selected_window_title) {
            return parseConfiguratorTitleFull(currentBinding.selected_window_title);
        }

        if (currentBinding.selected_config_name) {
            return {
                raw_title: currentBinding.selected_config_name,
                config_name: currentBinding.selected_config_name,
                confidence: 0.2,
            };
        }

        return null;
    }, [bindingResolution.activeWindow, currentBinding.selected_config_name, currentBinding.selected_window_title]);

    const bindingStatus = bindingResolution.status;
    const bindingMessage = useMemo(
        () => buildBindingMessage(bindingResolution.status, bindingResolution.candidates.length),
        [bindingResolution.candidates.length, bindingResolution.status, buildBindingMessage],
    );

    const getCode = useCallback(async (useSelectAll: boolean): Promise<string> => {
        const targetWindow = await resolveActiveWindowOrThrow();
        return await api.getCodeFromConfigurator(targetWindow.hwnd, useSelectAll);
    }, [resolveActiveWindowOrThrow]);

    const pasteCode = useCallback(async (
        code: string,
        useSelectAll: boolean,
        originalContent?: string,
        options?: api.ConfiguratorPasteOptions,
    ) => {
        const targetWindow = await resolveActiveWindowOrThrow();
        await api.pasteCodeToConfigurator(targetWindow.hwnd, code, useSelectAll, originalContent, options);
    }, [resolveActiveWindowOrThrow]);

    const checkSelection = useCallback(async (): Promise<boolean> => {
        try {
            const targetWindow = await resolveActiveWindowOrThrow();
            return await api.checkSelectionState(targetWindow.hwnd);
        } catch {
            return false;
        }
    }, [resolveActiveWindowOrThrow]);

    const snapToConfigurator = useCallback(async () => {
        try {
            const targetWindow = await resolveActiveWindowOrThrow();
            await api.alignWithConfigurator(targetWindow.hwnd);
        } catch (e) {
            console.warn("No Configurator window found for snapping", e);
            return;
        }
    }, [resolveActiveWindowOrThrow]);

    const contextValue = useMemo(() => ({
        detectedWindows,
        selectedHwnd,
        bindingStatus,
        bindingMessage,
        refreshWindows,
        selectWindow,
        getCode,
        pasteCode,
        checkSelection,
        snapToConfigurator,
        activeConfigTitle,
        parsedTitleContext,
    }), [detectedWindows, selectedHwnd, bindingStatus, bindingMessage, refreshWindows, selectWindow, getCode, pasteCode, checkSelection, snapToConfigurator, activeConfigTitle, parsedTitleContext]);

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
