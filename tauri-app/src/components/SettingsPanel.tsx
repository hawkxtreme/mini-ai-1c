import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { X, Save, Cpu, Monitor, FileCode, Database, Bug, MessageSquare } from 'lucide-react';

import { LLMSettings } from './settings/LLMSettings';
import { MCPSettings } from './settings/MCPSettings';
import { ConfiguratorTab } from './settings/ConfiguratorTab';
import { BslTab } from './settings/BslTab';
import { DebugTab } from './settings/DebugTab';
import { PromptsTab } from './settings/PromptsTab';

import { useProfiles } from '../contexts/ProfileContext';
import { WindowInfo, BslStatus, AppSettings, BslDiagnosticItem } from '../types/settings';

interface SettingsPanelProps {
    isOpen: boolean;
    onClose: () => void;
    initialTab?: 'configurator' | 'llm' | 'bsl' | 'mcp' | 'debug' | 'prompts';
}

export function SettingsPanel({ isOpen, onClose, initialTab }: SettingsPanelProps) {
    const [tab, setTab] = useState<'llm' | 'configurator' | 'bsl' | 'mcp' | 'debug' | 'prompts'>('llm');

    useEffect(() => {
        if (isOpen && initialTab) {
            setTab(initialTab);
        }
    }, [isOpen, initialTab]);

    const { profiles, activeProfileId, loadProfiles } = useProfiles();
    const [settings, setSettings] = useState<AppSettings | null>(null);
    const [saving, setSaving] = useState(false);

    // Configurator state
    const [detectedWindows, setDetectedWindows] = useState<WindowInfo[]>([]);
    const [testCaptureResult, setTestCaptureResult] = useState<string | null>(null);

    // BSL state
    const [bslStatus, setBslStatus] = useState<BslStatus | null>(null);
    const [downloading, setDownloading] = useState(false);
    const [downloadProgress, setDownloadProgress] = useState<number>(0);
    const [diagnosing, setDiagnosing] = useState(false);
    const [diagReport, setDiagReport] = useState<BslDiagnosticItem[] | null>(null);
    const [showResetConfirm, setShowResetConfirm] = useState(false);

    useEffect(() => {
        if (isOpen) {
            refreshAll();
        }
    }, [isOpen]);

    const refreshAll = () => {
        invoke<AppSettings>('get_settings').then(setSettings);
        refreshBslStatus();
    };

    const refreshBslStatus = () => {
        invoke<BslStatus>('check_bsl_status_cmd')
            .then(setBslStatus)
            .catch((err) => console.error('[Settings] BSL status error:', err));
    };

    const handleSaveSettings = async () => {
        if (!settings) return;
        setSaving(true);
        try {
            await invoke('save_settings', { newSettings: settings });
        } catch (err) {
            console.error('Failed to save settings:', err);
        }
        setSaving(false);
    };

    const refreshWindows = async () => {
        if (!settings) return;
        const windows = await invoke<WindowInfo[]>('find_configurator_windows_cmd', {
            pattern: settings.configurator.window_title_pattern
        });
        setDetectedWindows(windows);
    };

    // Auto-refresh loops
    useEffect(() => {
        let interval: any;
        if (tab === 'configurator' && isOpen) {
            refreshWindows();
            interval = setInterval(refreshWindows, 3000);
        }
        return () => interval && clearInterval(interval);
    }, [tab, isOpen, settings?.configurator.window_title_pattern]);

    useEffect(() => {
        let interval: any;
        if (tab === 'bsl' && isOpen) {
            refreshBslStatus();
            interval = setInterval(refreshBslStatus, 5000);
        }
        return () => interval && clearInterval(interval);
    }, [tab, isOpen]);

    const testCapture = async (hwnd: number) => {
        try {
            const code = await invoke<string>('get_code_from_configurator', { hwnd });
            setTestCaptureResult(code.substring(0, 200) + (code.length > 200 ? '...' : ''));
        } catch (e) {
            setTestCaptureResult(`Error: ${e}`);
        }
    };

    const browseJar = async () => {
        try {
            const file = await open({
                multiple: false,
                filters: [{ name: 'JAR Files', extensions: ['jar'] }],
                directory: false
            });
            if (file && typeof file === 'string' && settings) {
                setSettings({
                    ...settings,
                    bsl_server: { ...settings.bsl_server, jar_path: file }
                });
            }
        } catch (error) {
            console.error('Failed to open file dialog:', error);
        }
    };

    const handleDownloadBslLs = async () => {
        setDownloading(true);
        const unlisten = await listen<{ percent: number }>('bsl-download-progress', (event) => {
            setDownloadProgress(event.payload.percent);
        });

        try {
            const path = await invoke<string>('install_bsl_ls_cmd');
            if (settings) {
                setSettings({
                    ...settings,
                    bsl_server: { ...settings.bsl_server, jar_path: path }
                });
            }
            try { await invoke('reconnect_bsl_ls_cmd'); } catch (e) { console.warn(e); }
            setTimeout(refreshBslStatus, 2000);
            alert('BSL LS installed successfully!');
        } catch (e) {
            alert('Error downloading BSL LS: ' + e);
        }
        unlisten();
        setDownloading(false);
        setDownloadProgress(0);
    };

    const runDiagnostics = async () => {
        setDiagnosing(true);
        setDiagReport(null);
        try {
            const report = await invoke<BslDiagnosticItem[]>('diagnose_bsl_ls_cmd');
            setDiagReport(report);
        } catch (e) {
            setDiagReport([{ status: 'error', title: 'Системная ошибка', message: String(e) }]);
        }
        setDiagnosing(false);
    };

    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-2 sm:p-4 animate-in fade-in duration-200">
            <div className="bg-zinc-900 border border-zinc-700 rounded-xl w-full max-w-4xl h-full sm:h-[85vh] overflow-hidden flex flex-col shadow-2xl">
                {/* Header */}
                <div data-tauri-drag-region className="flex items-center justify-between px-4 sm:px-6 py-3 sm:py-4 border-b border-zinc-800 bg-zinc-900 select-none">
                    <h2 className="text-lg sm:text-xl font-bold text-zinc-100 pointer-events-none">Settings</h2>
                    <button onClick={onClose} className="p-1.5 hover:bg-zinc-800 rounded transition text-zinc-400 hover:text-zinc-200">
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* Tabs */}
                <div className="flex border-b border-zinc-800 bg-zinc-900/50 overflow-x-auto scrollbar-hide no-scrollbar">
                    {[
                        { id: 'llm' as const, label: 'LLM Profiles', icon: Cpu },
                        // { id: 'prompts' as const, label: 'Промпты', icon: MessageSquare },
                        { id: 'configurator' as const, label: 'Configurator', icon: Monitor },
                        { id: 'bsl' as const, label: 'BSL Server', icon: FileCode },
                        { id: 'mcp' as const, label: 'MCP Servers', icon: Database },
                        { id: 'debug' as const, label: 'Debug', icon: Bug },
                    ].map((t) => (
                        <button
                            key={t.id}
                            onClick={() => setTab(t.id)}
                            className={`flex items-center gap-2 px-4 sm:px-6 py-3 sm:py-4 text-xs sm:text-sm font-medium transition-colors border-b-2 whitespace-nowrap ${tab === t.id
                                ? 'border-blue-500 text-blue-400 bg-zinc-800/50'
                                : 'border-transparent text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/30'
                                }`}
                        >
                            <t.icon className="w-4 h-4" />
                            {t.label}
                        </button>
                    ))}
                </div>

                {/* Content */}
                <div className="flex-1 overflow-hidden flex relative">
                    {tab === 'llm' && (
                        <div className="w-full h-full">
                            <LLMSettings
                                profiles={{ profiles, active_profile_id: activeProfileId }}
                                onUpdate={loadProfiles}
                            />
                        </div>
                    )}

                    {tab === 'prompts' && settings && (
                        <div className="p-4 sm:p-8 w-full h-full overflow-y-auto scrollbar-thin">
                            <div className="max-w-2xl mx-auto">
                                <PromptsTab
                                    settings={settings}
                                    onSettingsChange={setSettings}
                                    onSave={handleSaveSettings}
                                    saving={saving}
                                />
                            </div>
                        </div>
                    )}

                    {tab === 'configurator' && settings && (
                        <ConfiguratorTab
                            settings={settings}
                            setSettings={setSettings}
                            detectedWindows={detectedWindows}
                            refreshWindows={refreshWindows}
                            testCapture={testCapture}
                            testCaptureResult={testCaptureResult}
                        />
                    )}

                    {tab === 'bsl' && settings && (
                        <BslTab
                            settings={settings}
                            setSettings={setSettings}
                            bslStatus={bslStatus}
                            refreshBslStatus={refreshBslStatus}
                            browseJar={browseJar}
                            handleDownloadBslLs={handleDownloadBslLs}
                            downloading={downloading}
                            downloadProgress={downloadProgress}
                            diagnosing={diagnosing}
                            diagReport={diagReport}
                            setDiagReport={setDiagReport}
                            runDiagnostics={runDiagnostics}
                        />
                    )}

                    {tab === 'mcp' && settings && (
                        <div className="p-4 sm:p-8 w-full h-full overflow-y-auto scrollbar-thin">
                            <div className="max-w-2xl mx-auto">
                                <MCPSettings
                                    servers={settings.mcp_servers}
                                    onUpdate={(mcpServers) => setSettings({ ...settings, mcp_servers: mcpServers })}
                                />
                            </div>
                        </div>
                    )}

                    {tab === 'debug' && settings && (
                        <DebugTab
                            settings={settings}
                            setSettings={setSettings}
                            showResetConfirm={showResetConfirm}
                            setShowResetConfirm={setShowResetConfirm}
                            resetOnboarding={async () => {
                                await invoke('reset_onboarding');
                                window.location.reload();
                            }}
                            saveDebugLogs={async () => {
                                try { await invoke('save_debug_logs'); } catch (e) { console.error(e); }
                            }}
                        />
                    )}
                </div>

                {/* Footer */}
                <div className="p-4 border-t border-zinc-800 bg-zinc-900 flex justify-end gap-3 z-10 relative">
                    {tab !== 'llm' && tab !== 'prompts' && (
                        <button
                            onClick={handleSaveSettings}
                            disabled={saving}
                            className="flex items-center gap-2 px-6 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition-all disabled:opacity-50 active:scale-95 shadow-lg shadow-blue-900/20"
                        >
                            <Save className="w-4 h-4" /> Save Settings
                        </button>
                    )}
                </div>
            </div >
        </div >
    );
}
