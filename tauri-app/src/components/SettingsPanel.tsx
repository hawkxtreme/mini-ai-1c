import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { X, Save, Cpu, RefreshCw, CheckCircle, Monitor, FileCode, Download, Database, Bug, AlertCircle, Info, ExternalLink, AlertTriangle, Terminal } from 'lucide-react';
import { LLMSettings } from './settings/LLMSettings';
import { MCPSettings } from './settings/MCPSettings';
import { useProfiles, ProfileStore } from '../contexts/ProfileContext';
import { parseConfiguratorTitle } from '../utils/configurator';

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

interface BslDiagnosticItem {
    status: 'ok' | 'warn' | 'error';
    title: string;
    message: string;
    suggestion?: string;
}

interface SettingsPanelProps {
    isOpen: boolean;
    onClose: () => void;
    initialTab?: 'configurator' | 'llm' | 'bsl' | 'mcp' | 'debug';
}

export function SettingsPanel({ isOpen, onClose, initialTab }: SettingsPanelProps) {
    const [tab, setTab] = useState<'llm' | 'configurator' | 'bsl' | 'mcp' | 'debug'>('llm');

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
        // invoke<ProfileStore>('get_profiles').then(setProfiles); // Removed, using context
        invoke<AppSettings>('get_settings').then(setSettings);
        refreshBslStatus();
    };

    const refreshBslStatus = () => {
        console.log('[DEBUG] refreshBslStatus called');
        invoke<BslStatus>('check_bsl_status_cmd')
            .then((status) => {
                console.log('[DEBUG] refreshBslStatus success:', status);
                setBslStatus(status);
            })
            .catch((err) => {
                console.error('[DEBUG] refreshBslStatus error:', err);
            });
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

    // Auto-refresh BSL status
    useEffect(() => {
        let interval: any;
        if (tab === 'bsl' && isOpen) {
            refreshBslStatus();
            interval = setInterval(refreshBslStatus, 5000);
        }
        return () => {
            if (interval) clearInterval(interval);
        };
    }, [tab, isOpen]);

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
        try {
            const file = await open({
                multiple: false,
                filters: [{ name: 'JAR Files', extensions: ['jar'] }],
                directory: false
            });

            // open() returns string | string[] | null
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

    // --- BSL Download ---
    const handleDownloadBslLs = async () => {
        setDownloading(true);

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
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-2 sm:p-4">
            <div className="bg-zinc-900 border border-zinc-700 rounded-xl w-full max-w-4xl h-full sm:h-[85vh] overflow-hidden flex flex-col shadow-2xl">
                {/* Header */}
                <div data-tauri-drag-region className="flex items-center justify-between px-4 sm:px-6 py-3 sm:py-4 border-b border-zinc-800 bg-zinc-900 select-none">
                    <h2 className="text-lg sm:text-xl font-bold text-zinc-100 pointer-events-none">Settings</h2>
                    <button onClick={onClose} className="p-1.5 hover:bg-zinc-800 rounded transition">
                        <X className="w-5 h-5 text-zinc-400" />
                    </button>
                </div>

                {/* Tabs */}
                <div className="flex border-b border-zinc-800 bg-zinc-900/50 overflow-x-auto scrollbar-hide no-scrollbar">
                    {[
                        { id: 'llm', label: 'LLM Profiles', icon: Cpu },
                        { id: 'configurator', label: 'Configurator', icon: Monitor },
                        { id: 'bsl', label: 'BSL Server', icon: FileCode },
                        { id: 'mcp', label: 'MCP Servers', icon: Database },
                        { id: 'debug', label: 'Debug', icon: Bug },
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
                    {tab === 'llm' && (
                        <div className="w-full h-full">
                            <LLMSettings
                                profiles={{ profiles, active_profile_id: activeProfileId }}
                                onUpdate={loadProfiles}
                            />
                        </div>
                    )}

                    {/* Configurator Tab */}
                    {tab === 'configurator' && settings && (
                        <div className="p-4 sm:p-8 w-full h-full overflow-y-auto">
                            <div className="max-w-2xl mx-auto space-y-6 sm:space-y-8">
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
                                                            <span className="truncate" title={w.title}>{parseConfiguratorTitle(w.title)}</span>
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
                        <div className="p-4 sm:p-8 w-full h-full overflow-y-auto">
                            <div className="max-w-2xl mx-auto space-y-6 sm:space-y-8">
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
                                                    <div className="flex justify-between text-[10px] text-zinc-500 font-bold uppercase tracking-wider">
                                                        <span className="flex items-center gap-1">
                                                            <RefreshCw className="w-3 h-3 animate-spin" />
                                                            Загрузка сервера...
                                                        </span>
                                                        <span>{downloadProgress}%</span>
                                                    </div>
                                                    <div className="w-full h-1.5 bg-zinc-800 border border-zinc-700 rounded-full overflow-hidden">
                                                        <div
                                                            className="h-full bg-gradient-to-r from-blue-600 to-blue-400 transition-all duration-300"
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
                                    <h3 className="text-lg font-medium mb-4 flex items-center gap-2">
                                        <RefreshCw className={`w-5 h-5 ${bslStatus?.connected ? 'text-green-400' : 'text-zinc-500'}`} />
                                        Состояние системы
                                    </h3>

                                    <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 mb-6">
                                        {/* Java Runtime Card */}
                                        <div className="bg-zinc-800/40 border border-zinc-700/50 rounded-xl p-4 flex flex-col items-center text-center">
                                            <div className={`p-2 rounded-full mb-3 ${bslStatus?.java_info.includes('version') ? 'bg-green-500/10 text-green-400' : 'bg-red-500/10 text-red-400'}`}>
                                                <Cpu className="w-5 h-5" />
                                            </div>
                                            <div className="text-xs text-zinc-500 font-medium uppercase mb-1">Java Runtime</div>
                                            <div className="text-sm font-semibold truncate w-full" title={bslStatus?.java_info}>
                                                {bslStatus?.java_info.includes('version') ? 'Установлена' : 'Не найдена'}
                                            </div>
                                        </div>

                                        {/* BSL JAR Card */}
                                        <div className="bg-zinc-800/40 border border-zinc-700/50 rounded-xl p-4 flex flex-col items-center text-center">
                                            <div className={`p-2 rounded-full mb-3 ${bslStatus?.installed ? 'bg-green-500/10 text-green-400' : 'bg-red-500/10 text-red-400'}`}>
                                                <FileCode className="w-5 h-5" />
                                            </div>
                                            <div className="text-xs text-zinc-500 font-medium uppercase mb-1">BSL Server</div>
                                            <div className="text-sm font-semibold">
                                                {bslStatus?.installed ? 'Готов' : 'Отсутствует'}
                                            </div>
                                        </div>

                                        {/* Connection Card */}
                                        <div className="bg-zinc-800/40 border border-zinc-700/50 rounded-xl p-4 flex flex-col items-center text-center">
                                            <div className={`p-2 rounded-full mb-3 ${bslStatus?.connected ? 'bg-green-500/10 text-green-400' : 'bg-red-500/10 text-red-400'}`}>
                                                <RefreshCw className={`w-5 h-5 ${bslStatus?.connected ? 'animate-spin-slow' : ''}`} />
                                            </div>
                                            <div className="text-xs text-zinc-500 font-medium uppercase mb-1">LSP Статус</div>
                                            <div className="text-sm font-semibold">
                                                {bslStatus?.connected ? 'Online' : 'Offline'}
                                            </div>
                                        </div>
                                    </div>

                                    {/* Diagnose button */}
                                    <div className="flex gap-3">
                                        <button
                                            onClick={async () => {
                                                setDiagnosing(true);
                                                setDiagReport(null);
                                                try {
                                                    const report = await invoke<BslDiagnosticItem[]>('diagnose_bsl_ls_cmd');
                                                    setDiagReport(report);
                                                } catch (e) {
                                                    // Fallback for unexpected errors
                                                    setDiagReport([{
                                                        status: 'error',
                                                        title: 'Системная ошибка',
                                                        message: String(e)
                                                    }]);
                                                }
                                                setDiagnosing(false);
                                            }}
                                            disabled={diagnosing}
                                            className="flex-1 flex items-center justify-center gap-2 px-4 py-2.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-xl text-sm font-medium transition-all hover:scale-[1.02] active:scale-[0.98] disabled:opacity-50"
                                        >
                                            <Terminal className={`w-4 h-4 ${diagnosing ? 'animate-pulse' : ''}`} />
                                            {diagnosing ? 'Выполняется диагностика...' : 'Запустить диагностику'}
                                        </button>

                                        <button
                                            onClick={refreshBslStatus}
                                            className="p-2.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-xl transition-all"
                                            title="Обновить статус"
                                        >
                                            <RefreshCw className="w-4 h-4" />
                                        </button>
                                    </div>

                                    {/* Diagnostic report */}
                                    {diagReport && (
                                        <div className="mt-6 space-y-3 animate-in fade-in slide-in-from-top-4 duration-300">
                                            <div className="flex items-center justify-between">
                                                <h4 className="text-sm font-semibold text-zinc-400 uppercase tracking-wider">Результаты диагностики</h4>
                                                <button onClick={() => setDiagReport(null)} className="text-xs text-zinc-500 hover:text-zinc-300 transition-colors">Очистить</button>
                                            </div>

                                            <div className="space-y-3">
                                                {diagReport.map((item, idx) => (
                                                    <div
                                                        key={idx}
                                                        className={`p-4 rounded-xl border flex gap-4 ${item.status === 'ok' ? 'bg-green-500/5 border-green-500/20' :
                                                            item.status === 'warn' ? 'bg-amber-500/5 border-amber-500/20' :
                                                                'bg-red-500/5 border-red-500/20'
                                                            }`}
                                                    >
                                                        <div className={`shrink-0 p-2 h-fit rounded-lg ${item.status === 'ok' ? 'bg-green-500/10 text-green-400' :
                                                            item.status === 'warn' ? 'bg-amber-500/10 text-amber-400' :
                                                                'bg-red-500/10 text-red-400'
                                                            }`}>
                                                            {item.status === 'ok' ? <CheckCircle className="w-5 h-5" /> :
                                                                item.status === 'warn' ? <AlertTriangle className="w-5 h-5" /> :
                                                                    <AlertCircle className="w-5 h-5" />}
                                                        </div>
                                                        <div className="flex-1 space-y-1">
                                                            <div className="font-semibold text-sm">{item.title}</div>
                                                            <div className="text-sm text-zinc-400 leading-relaxed">{item.message}</div>
                                                            {item.suggestion && (
                                                                <div className="mt-2 text-xs flex items-start gap-2 text-zinc-300 bg-white/5 p-2 rounded-lg">
                                                                    <Info className="w-4 h-4 shrink-0 mt-0.5 text-blue-400" />
                                                                    <div>{item.suggestion}</div>
                                                                </div>
                                                            )}
                                                        </div>
                                                    </div>
                                                ))}
                                            </div>
                                        </div>
                                    )}
                                </section>
                            </div>
                        </div>
                    )}

                    {/* MCP Tab */}
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

                    {/* Debug Tab */}
                    {tab === 'debug' && settings && (
                        <div className="p-4 sm:p-8 w-full h-full overflow-y-auto">
                            <div className="max-w-2xl mx-auto space-y-6 sm:space-y-8">
                                <section>
                                    <h3 className="text-lg font-medium mb-4 flex items-center gap-2">
                                        <Bug className="w-5 h-5 text-red-500" />
                                        Advanced Debugging
                                    </h3>
                                    <div className="bg-zinc-800/50 border border-zinc-700 rounded-xl p-5 space-y-4">
                                        <div className="flex items-center justify-between">
                                            <div>
                                                <div className="font-medium text-zinc-200">Reset Onboarding</div>
                                                <div className="text-xs text-zinc-500">Reset the "first run" flag to show the wizard again on next restart.</div>
                                            </div>
                                            <div className="flex gap-2">
                                                {!showResetConfirm ? (
                                                    <button
                                                        onClick={() => setShowResetConfirm(true)}
                                                        className="px-3 py-1 bg-red-900/40 text-red-300 border border-red-800/50 rounded-lg text-xs hover:bg-red-800/60 transition-colors"
                                                    >
                                                        Reset Onboarding
                                                    </button>
                                                ) : (
                                                    <div className="flex items-center gap-2 bg-red-950/40 border border-red-900/50 rounded-lg p-1 animate-in fade-in zoom-in-95 duration-200">
                                                        <span className="text-[10px] uppercase font-bold text-red-400 px-2">Are you sure?</span>
                                                        <button
                                                            onClick={async () => {
                                                                await invoke('reset_onboarding');
                                                                window.location.reload();
                                                            }}
                                                            className="px-3 py-1 bg-red-600 text-white rounded-md text-xs font-bold hover:bg-red-500 transition-colors"
                                                        >
                                                            YES, RESET
                                                        </button>
                                                        <button
                                                            onClick={() => setShowResetConfirm(false)}
                                                            className="px-3 py-1 bg-zinc-800 text-zinc-300 rounded-md text-xs hover:bg-zinc-700 transition-colors"
                                                        >
                                                            NO
                                                        </button>
                                                    </div>
                                                )}
                                                <button
                                                    onClick={() => window.location.reload()}
                                                    className="px-3 py-1 bg-zinc-800 border border-zinc-700 rounded-lg text-xs hover:bg-zinc-700 transition-colors"
                                                >
                                                    Reload App
                                                </button>
                                            </div>
                                        </div>

                                        <div className="flex items-center justify-between pt-4 border-t border-zinc-700">
                                            <div>
                                                <div className="font-medium">MCP Verbose Logging</div>
                                                <div className="text-xs text-zinc-500">Log all SSE events and tool payloads to terminal. Can impact performance.</div>
                                            </div>
                                            <div className="relative inline-flex h-6 w-11 items-center rounded-full bg-zinc-700 cursor-pointer transition-colors"
                                                onClick={() => setSettings({ ...settings, debug_mcp: !settings.debug_mcp })}
                                            >
                                                <span className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${settings.debug_mcp ? 'translate-x-6 bg-blue-500' : 'translate-x-1'}`} />
                                            </div>
                                        </div>

                                        <div className="flex items-center justify-between pt-4 border-t border-zinc-700">
                                            <div>
                                                <div className="font-medium text-zinc-200">System Logs</div>
                                                <div className="text-xs text-zinc-500">Экспорт всех логов приложения и серверов (BSL LS, MCP) в текстовый файл.</div>
                                            </div>
                                            <button
                                                onClick={async () => {
                                                    try {
                                                        await invoke('save_debug_logs');
                                                    } catch (e) {
                                                        console.error('Failed to save logs:', e);
                                                    }
                                                }}
                                                className="flex items-center gap-2 px-3 py-1 bg-zinc-700 text-zinc-200 border border-zinc-600 rounded-lg text-xs hover:bg-zinc-600 transition-colors"
                                            >
                                                <Save className="w-4 h-4" />
                                                Save Logs
                                            </button>
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
