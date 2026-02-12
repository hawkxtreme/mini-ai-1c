import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { X, Save, Cpu, RefreshCw, CheckCircle, Monitor, FileCode, Download } from 'lucide-react';
import { LLMSettings, ProfileStore } from './settings/LLMSettings';

interface WindowInfo {
    hwnd: number;
    title: string;
}

interface BslStatus {
    installed: boolean;
    java_info: string;
    connected: boolean;
}

interface AppSettings {
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
    ui: {
        theme: string;
        minimize_to_tray: boolean;
        start_minimized: boolean;
    };
}

interface SettingsPanelProps {
    isOpen: boolean;
    onClose: () => void;
}

export function SettingsPanel({ isOpen, onClose }: SettingsPanelProps) {
    const [tab, setTab] = useState<'llm' | 'configurator' | 'bsl' | 'ui'>('llm');
    const [profiles, setProfiles] = useState<ProfileStore | null>(null);
    const [settings, setSettings] = useState<AppSettings | null>(null);
    const [saving, setSaving] = useState(false);

    // Configurator state
    const [detectedWindows, setDetectedWindows] = useState<WindowInfo[]>([]);
    const [testCaptureResult, setTestCaptureResult] = useState<string | null>(null);

    // BSL state
    const [bslStatus, setBslStatus] = useState<BslStatus | null>(null);
    const [downloading, setDownloading] = useState(false);
    const [downloadProgress, setDownloadProgress] = useState<number>(0);

    useEffect(() => {
        if (isOpen) {
            refreshAll();
        }
    }, [isOpen]);

    const refreshAll = () => {
        invoke<ProfileStore>('get_profiles').then(setProfiles);
        invoke<AppSettings>('get_settings').then(setSettings);
        refreshBslStatus();
    };

    const refreshBslStatus = () => {
        invoke<BslStatus>('check_bsl_status_cmd').then(setBslStatus);
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

    // --- Configurator Actions ---
    const refreshWindows = async () => {
        if (!settings) return;
        const windows = await invoke<WindowInfo[]>('find_configurator_windows_cmd', { pattern: settings.configurator.window_title_pattern });
        setDetectedWindows(windows);
    };

    // Auto-refresh Configurator windows
    useEffect(() => {
        let interval: any;
        if (tab === 'configurator' && isOpen) {
            refreshWindows();
            interval = setInterval(refreshWindows, 3000);
        }
        return () => {
            if (interval) clearInterval(interval);
        };
    }, [tab, isOpen, settings?.configurator.window_title_pattern]);

    const testCapture = async (hwnd: number) => {
        try {
            const code = await invoke<string>('get_code_from_configurator', { hwnd });
            setTestCaptureResult(code.substring(0, 200) + (code.length > 200 ? '...' : ''));
        } catch (e) {
            setTestCaptureResult(`Error: ${e}`);
        }
    };

    // --- BSL Actions ---
    const browseJar = async () => {
        const file = await open({
            multiple: false,
            filters: [{ name: 'JAR Files', extensions: ['jar'] }]
        });
        if (file && settings) {
            setSettings({
                ...settings,
                bsl_server: { ...settings.bsl_server, jar_path: file as string }
            });
        }
    };

    // --- BSL Download ---
    const handleDownloadBslLs = async () => {
        setDownloading(true);
        setDownloadProgress(0);

        // Listen for progress events
        const unlisten = await listen<{ percent: number }>('bsl-download-progress', (event) => {
            setDownloadProgress(event.payload.percent);
        });

        try {
            console.log('[Settings] Starting BSL LS download...');
            const path = await invoke<string>('install_bsl_ls_cmd');
            console.log('[Settings] BSL LS downloaded to:', path);

            if (settings) {
                setSettings({
                    ...settings,
                    bsl_server: { ...settings.bsl_server, jar_path: path }
                });
            }

            // Reconnect BSL LS
            console.log('[Settings] Reconnecting BSL LS...');
            try {
                await invoke('reconnect_bsl_ls_cmd');
            } catch (e) {
                console.warn('[Settings] Reconnect failed:', e);
            }

            setTimeout(refreshBslStatus, 2000);
            alert('BSL LS installed successfully!');
        } catch (e) {
            console.error('[Settings] BSL download error:', e);
            alert('Error downloading BSL LS: ' + e);
        }

        unlisten();
        setDownloading(false);
        setDownloadProgress(0);
    };

    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
            <div className="bg-zinc-900 border border-zinc-700 rounded-xl w-[95vw] max-w-4xl h-[85vh] overflow-hidden flex flex-col shadow-2xl">
                {/* Header */}
                <div data-tauri-drag-region className="flex items-center justify-between px-6 py-4 border-b border-zinc-800 bg-zinc-900 select-none">
                    <h2 className="text-xl font-bold text-zinc-100 pointer-events-none">Settings</h2>
                    <button onClick={onClose} className="p-1 hover:bg-zinc-800 rounded transition">
                        <X className="w-5 h-5 text-zinc-400" />
                    </button>
                </div>

                {/* Tabs */}
                <div className="flex border-b border-zinc-800 bg-zinc-900/50 overflow-x-auto scrollbar-hide no-scrollbar">
                    {[
                        { id: 'llm', label: 'LLM Profiles', icon: Cpu },
                        { id: 'configurator', label: 'Configurator', icon: Monitor },
                        { id: 'bsl', label: 'BSL Server', icon: FileCode },
                    ].map((t) => (
                        <button
                            key={t.id}
                            onClick={() => setTab(t.id as any)}
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
                    {/* LLM Tab */}
                    {tab === 'llm' && profiles && (
                        <div className="w-full h-full">
                            <LLMSettings profiles={profiles} onUpdate={setProfiles} />
                        </div>
                    )}

                    {/* Configurator Tab */}
                    {tab === 'configurator' && settings && (
                        <div className="p-8 w-full h-full overflow-y-auto">
                            <div className="max-w-2xl mx-auto space-y-8">
                                <section>
                                    <h3 className="text-lg font-medium mb-4 flex items-center gap-2">
                                        <Monitor className="w-5 h-5 text-blue-500" />
                                        Window Detection
                                    </h3>
                                    <div className="bg-zinc-800/50 border border-zinc-700 rounded-xl p-5 space-y-4">
                                        <div>
                                            <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">Title Pattern</label>
                                            <input
                                                type="text"
                                                value={settings.configurator.window_title_pattern}
                                                onChange={(e) => setSettings({
                                                    ...settings,
                                                    configurator: { ...settings.configurator, window_title_pattern: e.target.value }
                                                })}
                                                placeholder="e.g. Configurator"
                                                className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                            />
                                        </div>

                                        <div className="mt-4">
                                            <div className="flex items-center justify-between mb-2">
                                                <label className="text-xs text-zinc-500 uppercase font-semibold">Detected Windows</label>
                                                <button onClick={refreshWindows} className="text-xs bg-zinc-700 hover:bg-zinc-600 px-2 py-1 rounded flex items-center gap-1">
                                                    <RefreshCw className="w-3 h-3" /> Refresh
                                                </button>
                                            </div>
                                            <div className="bg-zinc-900 border border-zinc-700 rounded-lg h-32 overflow-y-auto">
                                                {detectedWindows.length === 0 ? (
                                                    <div className="p-4 text-center text-zinc-500 text-sm italic">No windows detected</div>
                                                ) : (
                                                    detectedWindows.map(w => (
                                                        <div key={w.hwnd} className="p-2 border-b border-zinc-800 text-sm hover:bg-zinc-800 flex justify-between items-center group">
                                                            <span className="truncate">{w.title}</span>
                                                            <button onClick={() => testCapture(w.hwnd)} className="opacity-0 group-hover:opacity-100 text-xs bg-blue-600 px-2 py-0.5 rounded text-white">Test</button>
                                                        </div>
                                                    ))
                                                )}
                                            </div>
                                        </div>

                                        {testCaptureResult && (
                                            <div className="mt-2 p-3 bg-zinc-900 rounded border border-zinc-700 text-xs font-mono max-h-32 overflow-y-auto whitespace-pre-wrap text-zinc-300">
                                                {testCaptureResult}
                                            </div>
                                        )}
                                    </div>
                                </section>
                            </div>
                        </div>
                    )}

                    {/* BSL Tab */}
                    {tab === 'bsl' && settings && (
                        <div className="p-8 w-full overflow-y-auto">
                            <div className="max-w-2xl mx-auto space-y-8">
                                <section>
                                    <h3 className="text-lg font-medium mb-4 flex items-center gap-2">
                                        <FileCode className="w-5 h-5 text-blue-500" />
                                        BSL Language Server
                                    </h3>
                                    <div className="bg-zinc-800/50 border border-zinc-700 rounded-xl p-5 space-y-4">
                                        <div className="flex items-center gap-2 mb-4">
                                            <input
                                                type="checkbox"
                                                checked={settings.bsl_server.enabled}
                                                onChange={(e) => setSettings({
                                                    ...settings,
                                                    bsl_server: { ...settings.bsl_server, enabled: e.target.checked }
                                                })}
                                                className="rounded bg-zinc-700 border-zinc-600 text-blue-500 focus:ring-blue-500"
                                            />
                                            <span className="font-medium">Enable BSL Language Server</span>
                                        </div>

                                        <div>
                                            <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">JAR Path</label>
                                            <div className="flex gap-2">
                                                <input
                                                    type="text"
                                                    value={settings.bsl_server.jar_path}
                                                    onChange={(e) => setSettings({
                                                        ...settings,
                                                        bsl_server: { ...settings.bsl_server, jar_path: e.target.value }
                                                    })}
                                                    className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                                />
                                                <button onClick={browseJar} className="px-3 py-2 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-lg text-sm">Browse</button>
                                                <button
                                                    onClick={handleDownloadBslLs}
                                                    disabled={downloading}
                                                    className="px-3 py-2 bg-green-600 hover:bg-green-700 disabled:opacity-50 border border-green-700 rounded-lg text-sm text-white flex items-center gap-1"
                                                >
                                                    <Download className="w-3 h-3" />
                                                    {downloading ? 'Downloading...' : 'Download'}
                                                </button>
                                            </div>
                                            {downloading && (
                                                <div className="mt-2 space-y-1">
                                                    <div className="flex justify-between text-xs text-zinc-400">
                                                        <span>Downloading BSL Language Server...</span>
                                                        <span>{downloadProgress}%</span>
                                                    </div>
                                                    <div className="w-full h-1.5 bg-zinc-700 rounded-full overflow-hidden">
                                                        <div
                                                            className="h-full bg-green-500 transition-all duration-300"
                                                            style={{ width: `${downloadProgress}%` }}
                                                        />
                                                    </div>
                                                </div>
                                            )}
                                        </div>

                                        <div>
                                            <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">Java Path</label>
                                            <input
                                                type="text"
                                                value={settings.bsl_server.java_path}
                                                onChange={(e) => setSettings({
                                                    ...settings,
                                                    bsl_server: { ...settings.bsl_server, java_path: e.target.value }
                                                })}
                                                className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                            />
                                        </div>

                                        <div>
                                            <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">WebSocket Port</label>
                                            <input
                                                type="number"
                                                value={settings.bsl_server.websocket_port}
                                                onChange={(e) => setSettings({
                                                    ...settings,
                                                    bsl_server: { ...settings.bsl_server, websocket_port: parseInt(e.target.value) || 8025 }
                                                })}
                                                className="w-32 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                            />
                                        </div>
                                    </div>
                                </section>

                                <section>
                                    <h3 className="text-lg font-medium mb-4">Status</h3>
                                    <div className="bg-zinc-800/50 border border-zinc-700 rounded-xl p-5">
                                        <div className="space-y-2 text-sm">
                                            <div className="flex justify-between border-b border-zinc-800 pb-2">
                                                <span className="text-zinc-400">Java Runtime:</span>
                                                <span className={bslStatus?.java_info.includes('found') ? 'text-green-400' : 'text-red-400'}>
                                                    {bslStatus?.java_info || 'Checking...'}
                                                </span>
                                            </div>
                                            <div className="flex justify-between pt-2">
                                                <span className="text-zinc-400">BSL Server JAR:</span>
                                                <span className={bslStatus?.installed ? 'text-green-400' : 'text-red-400'}>
                                                    {bslStatus?.installed ? 'Installed' : 'Not Found'}
                                                </span>
                                            </div>
                                            <div className="flex justify-between pt-2">
                                                <span className="text-zinc-400">LSP Connection:</span>
                                                <span className={bslStatus?.connected ? 'text-green-400' : 'text-red-400'}>
                                                    {bslStatus?.connected ? 'Connected' : 'Disconnected'}
                                                </span>
                                            </div>
                                        </div>
                                    </div>
                                </section>
                            </div>
                        </div>
                    )}
                </div>

                {/* Footer */}
                <div className="p-4 border-t border-zinc-800 bg-zinc-900 flex justify-end gap-3 z-10 relative">
                    {/* Save Button is only for Settings Tabs, LLM has its own */}
                    {tab !== 'llm' && (
                        <button
                            onClick={handleSaveSettings}
                            disabled={saving}
                            className="flex items-center gap-2 px-6 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition disabled:opacity-50"
                        >
                            <Save className="w-4 h-4" /> Save Settings
                        </button>
                    )}
                </div>
            </div >
        </div >
    );
}
