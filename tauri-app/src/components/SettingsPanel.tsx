import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { X, Plus, Save, Key, Cpu, RefreshCw, CheckCircle, Monitor, FileCode } from 'lucide-react';



interface LLMProfile {
    id: string;
    name: string;
    provider: string;
    model: string;
    api_key_encrypted: string;
    base_url: string | null;
    max_tokens: number;
    temperature: number;
}

interface ProfileStore {
    profiles: LLMProfile[];
    active_profile_id: string;
}

interface WindowInfo {
    hwnd: number;
    title: string;
}

interface BslStatus {
    installed: boolean;
    java_info: string;
}

interface AppSettings {
    configurator: {
        window_title_pattern: string;
        selected_window_hwnd: number | null;
        capture_on_hotkey: boolean;
        hotkey: string;
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

const PROVIDERS = [
    { value: 'OpenAI', label: 'OpenAI', defaultModel: 'gpt-4o-mini' },
    { value: 'Anthropic', label: 'Anthropic', defaultModel: 'claude-3-5-sonnet-latest' },
    { value: 'OpenRouter', label: 'OpenRouter', defaultModel: 'google/gemini-2.0-flash-001' },
    { value: 'Google', label: 'Google AI', defaultModel: 'gemini-1.5-flash' },
    { value: 'Custom', label: 'Custom', defaultModel: '' },
];

export function SettingsPanel({ isOpen, onClose }: SettingsPanelProps) {
    const [tab, setTab] = useState<'llm' | 'configurator' | 'bsl' | 'ui'>('llm');
    const [profiles, setProfiles] = useState<ProfileStore | null>(null);
    const [settings, setSettings] = useState<AppSettings | null>(null);
    const [editingProfile, setEditingProfile] = useState<LLMProfile | null>(null);
    const [newApiKey, setNewApiKey] = useState('');
    const [saving, setSaving] = useState(false);

    // Configurator state
    const [detectedWindows, setDetectedWindows] = useState<WindowInfo[]>([]);
    const [testCaptureResult, setTestCaptureResult] = useState<string | null>(null);

    // BSL state
    const [bslStatus, setBslStatus] = useState<BslStatus | null>(null);

    // LLM state
    const [modelList, setModelList] = useState<string[]>([]);
    const [connectionTestResult, setConnectionTestResult] = useState<string | null>(null);

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

    const handleSaveProfile = async () => {
        if (!editingProfile) return;
        setSaving(true);
        try {
            await invoke('save_profile', {
                profile: editingProfile,
                apiKey: newApiKey || null,
            });
            const updated = await invoke<ProfileStore>('get_profiles');
            setProfiles(updated);
            setEditingProfile(null);
            setNewApiKey('');
        } catch (err) {
            console.error('Failed to save profile:', err);
        }
        setSaving(false);
    };

    const handleDeleteProfile = async (id: string) => {
        if (!confirm('Are you sure you want to delete this profile?')) return;
        try {
            await invoke('delete_profile', { profileId: id });
            const updated = await invoke<ProfileStore>('get_profiles');
            setProfiles(updated);
            if (editingProfile?.id === id) setEditingProfile(null);
        } catch (err) {
            console.error('Failed to delete profile:', err);
        }
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

    const createNewProfile = () => {
        const id = `profile_${Date.now()}`;
        setEditingProfile({
            id,
            name: 'New Profile',
            provider: 'OpenAI',
            model: 'gpt-4o-mini',
            api_key_encrypted: '',
            base_url: null,
            max_tokens: 4096,
            temperature: 0.7,
        });
        setNewApiKey('');
    };

    // --- LLM Actions ---
    const fetchModels = async () => {
        if (!editingProfile) return;
        // Temporarily save to ensure we use current credentials (handled by backend ideally, but we pass ID)
        // Wait, backend fetch_models_cmd takes ID. So we must save first? 
        // Or we should update profile in memory. 
        // For now let's just save.
        await handleSaveProfile();
        // Re-select editing profile
        if (profiles) {
            const p = profiles.profiles.find(p => p.id === editingProfile.id);
            if (p) setEditingProfile(p);
        }

        try {
            const models = await invoke<string[]>('fetch_models_cmd', { profileId: editingProfile.id });
            setModelList(models);
            alert(`Found ${models.length} models`);
        } catch (e) {
            alert(`Error fetching models: ${e}`);
        }
    };

    const testConnection = async () => {
        if (!editingProfile) return;
        await handleSaveProfile();
        try {
            const result = await invoke<string>('test_llm_connection_cmd', { profileId: editingProfile.id });
            setConnectionTestResult(result);
        } catch (e) {
            setConnectionTestResult(`Error: ${e}`);
        }
    };

    const setDefaultProfile = async () => {
        if (!editingProfile) return;
        await invoke('set_active_profile', { profileId: editingProfile.id });
        refreshAll();
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

    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
            <div className="bg-zinc-900 border border-zinc-700 rounded-xl w-full max-w-4xl h-[85vh] overflow-hidden flex flex-col shadow-2xl">
                {/* Header */}
                <div data-tauri-drag-region className="flex items-center justify-between px-6 py-4 border-b border-zinc-800 bg-zinc-900 select-none">
                    <h2 className="text-xl font-bold text-zinc-100 pointer-events-none">Settings</h2>
                    <button onClick={onClose} className="p-1 hover:bg-zinc-800 rounded transition">
                        <X className="w-5 h-5 text-zinc-400" />
                    </button>
                </div>

                {/* Tabs */}
                <div className="flex border-b border-zinc-800 bg-zinc-900/50">
                    {[
                        { id: 'llm', label: 'LLM Profiles', icon: Cpu },
                        { id: 'configurator', label: 'Configurator', icon: Monitor },
                        { id: 'bsl', label: 'BSL Server', icon: FileCode },
                        { id: 'ui', label: 'Interface', icon: CheckCircle },
                    ].map((t) => (
                        <button
                            key={t.id}
                            onClick={() => setTab(t.id as any)}
                            className={`flex items-center gap-2 px-6 py-4 text-sm font-medium transition-colors border-b-2 ${tab === t.id
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
                        <div className="flex w-full h-full">
                            {/* List */}
                            <div className="w-1/3 border-r border-zinc-800 overflow-y-auto p-4 bg-zinc-900/30">
                                <div className="space-y-2">
                                    {profiles.profiles.map((p) => (
                                        <div
                                            key={p.id}
                                            onClick={() => { setEditingProfile(p); setNewApiKey(''); setConnectionTestResult(null); }}
                                            className={`p-3 rounded-lg border cursor-pointer transition-all ${editingProfile?.id === p.id
                                                ? 'border-blue-500 bg-blue-500/10'
                                                : 'border-zinc-800 bg-zinc-800 hover:border-zinc-600'
                                                }`}
                                        >
                                            <div className="flex items-center justify-between mb-1">
                                                <div className="font-medium text-zinc-200">{p.name}</div>
                                                {p.id === profiles.active_profile_id && (
                                                    <span className="text-xs px-2 py-0.5 bg-green-500/20 text-green-400 rounded-full">Active</span>
                                                )}
                                            </div>
                                            <div className="text-xs text-zinc-500 truncate">{p.provider} / {p.model}</div>
                                        </div>
                                    ))}
                                    <button
                                        onClick={createNewProfile}
                                        className="w-full py-3 flex items-center justify-center gap-2 border border-dashed border-zinc-700 rounded-lg hover:border-zinc-500 hover:bg-zinc-800 transition text-zinc-400"
                                    >
                                        <Plus className="w-4 h-4" /> Add Profile
                                    </button>
                                </div>
                            </div>

                            {/* Editor */}
                            <div className="flex-1 overflow-y-auto p-6 bg-zinc-900">
                                {editingProfile ? (
                                    <div className="space-y-6 max-w-lg">
                                        <div className="flex items-center justify-between">
                                            <h3 className="text-lg font-medium">Edit Profile</h3>
                                            <div className="flex gap-2">
                                                {editingProfile.id !== profiles.active_profile_id && (
                                                    <button onClick={setDefaultProfile} className="text-xs px-3 py-1 bg-zinc-800 hover:bg-zinc-700 rounded border border-zinc-700">Set Active</button>
                                                )}
                                                {editingProfile.id !== 'default' && (
                                                    <button onClick={() => handleDeleteProfile(editingProfile.id)} className="text-xs px-3 py-1 bg-red-500/10 text-red-400 hover:bg-red-500/20 rounded border border-red-500/20">Delete</button>
                                                )}
                                            </div>
                                        </div>

                                        <div className="space-y-4">
                                            <div className="grid grid-cols-2 gap-4">
                                                <div>
                                                    <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">Name</label>
                                                    <input
                                                        type="text"
                                                        value={editingProfile.name}
                                                        onChange={(e) => setEditingProfile({ ...editingProfile, name: e.target.value })}
                                                        className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                                    />
                                                </div>
                                                <div>
                                                    <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">Provider</label>
                                                    <select
                                                        value={editingProfile.provider}
                                                        onChange={(e) => {
                                                            const provider = PROVIDERS.find(p => p.value === e.target.value);
                                                            setEditingProfile({
                                                                ...editingProfile,
                                                                provider: e.target.value,
                                                                model: provider?.defaultModel || '',
                                                            });
                                                        }}
                                                        className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                                    >
                                                        {PROVIDERS.map((p) => (
                                                            <option key={p.value} value={p.value}>{p.label}</option>
                                                        ))}
                                                    </select>
                                                </div>
                                            </div>

                                            <div>
                                                <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">Model</label>
                                                <div className="flex gap-2">
                                                    <input
                                                        type="text"
                                                        value={editingProfile.model}
                                                        onChange={(e) => setEditingProfile({ ...editingProfile, model: e.target.value })}
                                                        className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                                    />
                                                    <button onClick={fetchModels} className="px-3 py-2 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-lg text-sm">Load</button>
                                                </div>
                                                {modelList.length > 0 && (
                                                    <select
                                                        className="w-full mt-2 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm"
                                                        onChange={(e) => setEditingProfile({ ...editingProfile, model: e.target.value })}
                                                        value={editingProfile.model}
                                                    >
                                                        <option value="">Select from loaded...</option>
                                                        {modelList.map(m => <option key={m} value={m}>{m}</option>)}
                                                    </select>
                                                )}
                                            </div>

                                            <div>
                                                <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">API Key</label>
                                                <input
                                                    type="password"
                                                    value={newApiKey}
                                                    onChange={(e) => setNewApiKey(e.target.value)}
                                                    placeholder={editingProfile.api_key_encrypted ? '•••••••• (Encrypted)' : 'Enter API Key'}
                                                    className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none placeholder-zinc-600"
                                                />
                                            </div>

                                            <div>
                                                <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">Base URL (Optional)</label>
                                                <input
                                                    type="text"
                                                    value={editingProfile.base_url || ''}
                                                    onChange={(e) => setEditingProfile({ ...editingProfile, base_url: e.target.value || null })}
                                                    placeholder="https://api.openai.com/v1"
                                                    className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                                />
                                            </div>

                                            <div className="grid grid-cols-2 gap-4">
                                                <div>
                                                    <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">Temperature</label>
                                                    <input
                                                        type="number"
                                                        step="0.1"
                                                        min="0"
                                                        max="2"
                                                        value={editingProfile.temperature}
                                                        onChange={(e) => setEditingProfile({ ...editingProfile, temperature: parseFloat(e.target.value) || 0.7 })}
                                                        className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                                    />
                                                </div>
                                                <div>
                                                    <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">Max Tokens</label>
                                                    <input
                                                        type="number"
                                                        value={editingProfile.max_tokens}
                                                        onChange={(e) => setEditingProfile({ ...editingProfile, max_tokens: parseInt(e.target.value) || 4096 })}
                                                        className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                                    />
                                                </div>
                                            </div>

                                            <div className="pt-4 flex gap-3">
                                                <button
                                                    onClick={handleSaveProfile}
                                                    disabled={saving}
                                                    className="flex-1 bg-blue-600 hover:bg-blue-700 text-white font-medium py-2 rounded-lg transition disabled:opacity-50"
                                                >
                                                    {saving ? 'Saving...' : 'Save Changes'}
                                                </button>
                                                <button
                                                    onClick={testConnection}
                                                    className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-lg text-sm font-medium transition"
                                                >
                                                    Test Connection
                                                </button>
                                            </div>

                                            {connectionTestResult && (
                                                <div className={`p-3 rounded-lg text-sm border ${connectionTestResult.startsWith('Success') ? 'bg-green-500/10 border-green-500/30 text-green-400' : 'bg-red-500/10 border-red-500/30 text-red-400'}`}>
                                                    {connectionTestResult}
                                                </div>
                                            )}
                                        </div>
                                    </div>
                                ) : (
                                    <div className="h-full flex flex-col items-center justify-center text-zinc-500">
                                        <Cpu className="w-12 h-12 mb-4 opacity-50" />
                                        <p>Select a profile to edit or create a new one</p>
                                    </div>
                                )}
                            </div>
                        </div>
                    )}

                    {/* Configurator Tab */}
                    {tab === 'configurator' && settings && (
                        <div className="p-8 w-full overflow-y-auto">
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

                                <section>
                                    <h3 className="text-lg font-medium mb-4 flex items-center gap-2">
                                        <Key className="w-5 h-5 text-blue-500" />
                                        Capture Settings
                                    </h3>
                                    <div className="bg-zinc-800/50 border border-zinc-700 rounded-xl p-5 space-y-4">
                                        <div className="flex items-center gap-2">
                                            <input
                                                type="checkbox"
                                                checked={settings.configurator.capture_on_hotkey}
                                                onChange={(e) => setSettings({
                                                    ...settings,
                                                    configurator: { ...settings.configurator, capture_on_hotkey: e.target.checked }
                                                })}
                                                className="rounded bg-zinc-700 border-zinc-600 text-blue-500 focus:ring-blue-500"
                                            />
                                            <span className="text-sm">Capture on Hotkey</span>
                                        </div>
                                        <div>
                                            <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">Hotkey</label>
                                            <input
                                                type="text"
                                                value={settings.configurator.hotkey}
                                                onChange={(e) => setSettings({
                                                    ...settings,
                                                    configurator: { ...settings.configurator, hotkey: e.target.value }
                                                })}
                                                className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none"
                                            />
                                        </div>
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
                                            </div>
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
                                        </div>
                                    </div>
                                </section>
                            </div>
                        </div>
                    )}

                    {/* UI Tab */}
                    {tab === 'ui' && settings && (
                        <div className="p-8 w-full overflow-y-auto">
                            <div className="max-w-xl mx-auto">
                                <section>
                                    <h3 className="text-lg font-medium mb-4">Behavior</h3>
                                    <div className="bg-zinc-800/50 border border-zinc-700 rounded-xl p-5 space-y-4">
                                        <div className="flex items-center gap-2">
                                            <input
                                                type="checkbox"
                                                checked={settings.ui.minimize_to_tray}
                                                onChange={(e) => setSettings({
                                                    ...settings,
                                                    ui: { ...settings.ui, minimize_to_tray: e.target.checked }
                                                })}
                                                className="rounded bg-zinc-700 border-zinc-600 text-blue-500 focus:ring-blue-500"
                                            />
                                            <span className="text-sm">Minimize to system tray</span>
                                        </div>
                                        <div className="flex items-center gap-2">
                                            <input
                                                type="checkbox"
                                                checked={settings.ui.start_minimized}
                                                onChange={(e) => setSettings({
                                                    ...settings,
                                                    ui: { ...settings.ui, start_minimized: e.target.checked }
                                                })}
                                                className="rounded bg-zinc-700 border-zinc-600 text-blue-500 focus:ring-blue-500"
                                            />
                                            <span className="text-sm">Start minimized</span>
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
            </div>
        </div>
    );
}
