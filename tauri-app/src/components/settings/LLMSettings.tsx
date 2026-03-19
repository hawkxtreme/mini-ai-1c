import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Plus, Save, RefreshCw, Trash2, Check, LogIn, LogOut, Info, X, ExternalLink } from 'lucide-react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cliProvidersApi } from '../../api/cli_providers';
import { QwenAuthModal } from './QwenAuthModal';
import { CliStatus } from '../../types/settings';

import { LLMProfile, ProfileStore } from '../../contexts/ProfileContext';

interface LLMSettingsProps {
    profiles: ProfileStore;
    onUpdate: () => void;
}

const PROVIDERS = [
    { value: 'OpenAI', label: 'OpenAI', defaultModel: 'gpt-4o', defaultUrl: 'https://api.openai.com/v1', type: 'standard' },
    { value: 'Anthropic', label: 'Anthropic', defaultModel: 'claude-3-5-sonnet-latest', defaultUrl: 'https://api.anthropic.com/v1', type: 'standard' },
    { value: 'Google', label: 'Google Gemini', defaultModel: 'gemini-1.5-pro', defaultUrl: 'https://generativelanguage.googleapis.com/v1beta/openai', type: 'standard' },
    { value: 'DeepSeek', label: 'DeepSeek', defaultModel: 'deepseek-chat', defaultUrl: 'https://api.deepseek.com/v1', type: 'standard' },
    { value: 'Groq', label: 'Groq', defaultModel: 'llama-3.3-70b-versatile', defaultUrl: 'https://api.groq.com/openai/v1', type: 'standard' },
    { value: 'Mistral', label: 'Mistral AI', defaultModel: 'mistral-large-latest', defaultUrl: 'https://api.mistral.ai/v1', type: 'standard' },
    { value: 'XAI', label: 'xAI (Grok)', defaultModel: 'grok-beta', defaultUrl: 'https://api.x.ai/v1', type: 'standard' },
    { value: 'Perplexity', label: 'Perplexity', defaultModel: 'sonar-reasoning', defaultUrl: 'https://api.perplexity.ai', type: 'standard' },
    { value: 'ZAI', label: 'Z.ai (Zhipu)', defaultModel: 'glm-5', defaultUrl: 'https://api.z.ai/api/coding/paas/v4', type: 'standard' },
    { value: 'OpenRouter', label: 'OpenRouter', defaultModel: 'google/gemini-2.0-flash-001', defaultUrl: 'https://openrouter.ai/api/v1', type: 'standard' },
    { value: 'Ollama', label: 'Ollama (Local)', defaultModel: 'llama3', defaultUrl: 'http://localhost:11434/v1', type: 'standard' },
    { value: 'LMStudio', label: 'LM Studio (Local)', defaultModel: '', defaultUrl: 'http://localhost:1234/v1', type: 'standard' },
    { value: 'QwenCli', label: 'Qwen Code (CLI)', defaultModel: 'coder-model', defaultUrl: 'https://portal.qwen.ai/v1', type: 'cli' },
    { value: 'Custom', label: 'Custom / Other', defaultModel: '', defaultUrl: '', type: 'standard' },
    { value: 'OneCNaparnik', label: '1С:Напарник', defaultModel: 'naparnik', defaultUrl: 'https://code.1c.ai', type: 'naparnik' },
];

export function LLMSettings({ profiles, onUpdate }: LLMSettingsProps) {
    const [editingId, setEditingId] = useState<string | null>(null);
    const [editForm, setEditForm] = useState<LLMProfile | null>(null);
    const [newApiKey, setNewApiKey] = useState('');
    const [modelList, setModelList] = useState<any[]>([]);
    const [loadingModels, setLoadingModels] = useState(false);
    const [connectionTest, setConnectionTest] = useState<string | null>(null);
    const [isSaving, setIsSaving] = useState(false);
    const [showSaved, setShowSaved] = useState(false);
    const [isAuthModalOpen, setIsAuthModalOpen] = useState(false);
    const [cliStatus, setCliStatus] = useState<CliStatus | null>(null);
    const [loadingStatus, setLoadingStatus] = useState(false);

    // Track which profile was previously active to detect real profile switches
    const prevEditingIdRef = useRef<string | null>(null);

    // Select profile to edit
    useEffect(() => {
        if (editingId) {
            const p = profiles.profiles.find(p => p.id === editingId);
            if (p) {
                const isNewProfile = prevEditingIdRef.current !== editingId;
                prevEditingIdRef.current = editingId;

                setEditForm(prev => (prev?.id === editingId ? prev : { ...p }));
                setNewApiKey('');
                setConnectionTest(null);

                // Only reset model list when switching to a different profile
                if (isNewProfile) {
                    setModelList([]);
                }

                // Fetch CLI status if it's a CLI provider
                if (p.provider === 'QwenCli') {
                    fetchCliStatus(p.id, 'qwen');
                } else {
                    setCliStatus(null);
                }

                // Auto-fetch models for CLI providers since the Fetch button is hidden
                // NOTE: handleFetchModels() reads editForm which is stale here (async state update),
                // so we invoke directly with the freshly found profile `p` to avoid race condition.
                if (PROVIDERS.find(prov => prov.value === p.provider)?.type === 'cli') {
                    setLoadingModels(true);
                    invoke<any[]>('fetch_models_from_provider', {
                        providerId: p.provider,
                        baseUrl: p.base_url || PROVIDERS.find(prov => prov.value === p.provider)?.defaultUrl || '',
                        apiKey: ''
                    }).then(res => {
                        const sorted = [...res].sort((a, b) => a.id.localeCompare(b.id));
                        setModelList(sorted);
                    }).catch(e => {
                        console.error('[LLMSettings] Failed to auto-fetch CLI models:', e);
                    }).finally(() => {
                        setLoadingModels(false);
                    });
                }
            }
        }
    }, [editingId, profiles]);

    const fetchCliStatus = async (profileId: string, provider: string, force = false) => {
        setLoadingStatus(true);
        try {
            if (force) {
                const usage = await cliProvidersApi.refreshUsage(profileId, provider);
                setCliStatus(prev => prev ? { ...prev, usage } : null);
            } else {
                const status = await cliProvidersApi.getStatus(profileId, provider);
                setCliStatus(status);
            }
        } catch (e) {
            console.error('Failed to fetch CLI status:', e);
        } finally {
            setLoadingStatus(false);
        }
    };

    const handleSave = async () => {
        if (!editForm) return;

        setIsSaving(true);
        setShowSaved(false);
        try {
            await invoke('save_profile', {
                profile: editForm,
                apiKey: newApiKey || null
            });
            if (newApiKey) {
                setEditForm(prev => prev ? { ...prev, api_key_encrypted: 'set' } : null);
                setNewApiKey('');
            }
            onUpdate();
            setShowSaved(true);
            setTimeout(() => setShowSaved(false), 3000);
        } catch (e) {
            alert('Failed to save: ' + e);
        } finally {
            setIsSaving(false);
        }
    };

    const handleDelete = async (id: string) => {
        try {
            await invoke('delete_profile', { profileId: id });
            onUpdate();
            if (editingId === id) {
                setEditingId(null);
                setEditForm(null);
                setModelList([]);
                setNewApiKey('');
                setConnectionTest(null);
                setCliStatus(null);
            }
        } catch (e) {
            alert('Error: ' + e);
        }
    };

    const handleCreate = (providerValue: string = 'OpenAI') => {
        const id = `profile_${Date.now()}`;
        const provider = PROVIDERS.find(p => p.value === providerValue) || PROVIDERS[0];

        const newProfile: LLMProfile = {
            id,
            name: 'New Profile',
            provider: provider.value,
            model: provider.defaultModel,
            api_key_encrypted: '',
            base_url: provider.defaultUrl,
            max_tokens: 4096,
            temperature: providerValue === 'QwenCli' ? 0.1 : 0.7
        };
        invoke('save_profile', { profile: newProfile, apiKey: null }).then(() => {
            onUpdate();
            setEditingId(id);
        });
    };

    const handleFetchModels = async () => {
        if (!editForm) return;
        setLoadingModels(true);
        try {
            let res: any[] = [];
            if (newApiKey) {
                res = await invoke<any[]>('fetch_models_from_provider', {
                    providerId: editForm.provider,
                    baseUrl: editForm.base_url || PROVIDERS.find(p => p.value === editForm.provider)?.defaultUrl || '',
                    apiKey: newApiKey
                });
            } else if (editForm.api_key_encrypted) {
                await invoke('save_profile', { profile: editForm, apiKey: null });
                res = await invoke<any[]>('fetch_models_for_profile', { profileId: editForm.id });
            } else {
                res = await invoke<any[]>('fetch_models_from_provider', {
                    providerId: editForm.provider,
                    baseUrl: editForm.base_url || PROVIDERS.find(p => p.value === editForm.provider)?.defaultUrl || '',
                    apiKey: ''
                });
            }

            const sortedModels = [...res].sort((a, b) => a.id.localeCompare(b.id));
            setModelList(sortedModels);

            // Sync metadata for the current model if it's already selected
            if (editForm.model) {
                const currentModel = sortedModels.find(m => m.id === editForm.model);
                if (currentModel) {
                    setEditForm(prev => prev ? ({
                        ...prev,
                        max_tokens: currentModel.context_window || prev.max_tokens,
                        context_window_override: currentModel.context_window
                    }) : null);
                }
            }
        } catch (e) {
            alert("Error fetching: " + e);
        }
        setLoadingModels(false);
    };

    const handleSetActive = async (id: string) => {
        await invoke('set_active_profile', { profileId: id });
        onUpdate();
    };

    return (
        <div className="flex h-full w-full">
            {/* Sidebar List */}
            <div className="w-24 sm:w-1/3 border-r border-zinc-800 bg-zinc-900/30 overflow-y-auto p-2 sm:p-3">
                <div className="space-y-6">
                    {/* Standard Profiles Group */}
                    <div className="space-y-2">
                        <div className="px-1 flex items-center gap-2 opacity-50">
                            <span className="text-[10px] uppercase font-black tracking-widest text-zinc-400">LLM Ассистенты</span>
                            <div className="h-[1px] flex-1 bg-zinc-800"></div>
                        </div>
                        <div className="space-y-1.5">
                            {profiles.profiles.filter(p => p.provider !== 'QwenCli').map(p => (
                                <div
                                    key={p.id}
                                    onClick={() => setEditingId(p.id)}
                                    className={`p-2 sm:p-3 rounded-lg border cursor-pointer transition-all ${editingId === p.id
                                        ? 'border-blue-500 bg-blue-500/10'
                                        : 'border-zinc-800 bg-zinc-800 hover:border-zinc-600'
                                        }`}
                                >
                                    <div className="flex justify-between items-center mb-0.5">
                                        <span className="font-medium text-xs sm:text-sm text-zinc-200 truncate pr-1">{p.name}</span>
                                        {profiles.active_profile_id === p.id && <Check className="w-3 h-3 text-green-500 flex-shrink-0" />}
                                    </div>
                                    <div className="text-[10px] text-zinc-500 truncate">{p.provider} • {p.model}</div>
                                </div>
                            ))}
                        </div>
                        <div className="space-y-1.5 pt-1">
                            <button
                                onClick={() => handleCreate('OpenAI')}
                                className="w-full py-2 flex items-center justify-center gap-2 border border-dashed border-zinc-700 rounded-lg text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300 transition text-[10px] font-medium"
                            >
                                <Plus className="w-3 h-3" /> Новый ассистент
                            </button>
                        </div>
                    </div>

                    {/* CLI Providers Group */}
                    <div className="space-y-2">
                        <div className="px-1 flex items-center gap-2 opacity-50">
                            <span className="text-[10px] uppercase font-black tracking-widest text-zinc-400">CLI Провайдеры</span>
                            <div className="h-[1px] flex-1 bg-zinc-800"></div>
                        </div>
                        <div className="space-y-1.5">
                            {profiles.profiles.filter(p => p.provider === 'QwenCli').map(p => (
                                <div
                                    key={p.id}
                                    onClick={() => setEditingId(p.id)}
                                    className={`p-2 sm:p-3 rounded-lg border cursor-pointer transition-all ${editingId === p.id
                                        ? 'border-blue-400 bg-blue-400/10'
                                        : 'border-zinc-800 bg-zinc-800 hover:border-zinc-600'
                                        }`}
                                >
                                    <div className="flex justify-between items-center mb-0.5">
                                        <span className="font-medium text-xs sm:text-sm text-zinc-200 truncate pr-1">{p.name}</span>
                                        {profiles.active_profile_id === p.id && <Check className="w-3 h-3 text-blue-400 flex-shrink-0" />}
                                    </div>
                                    <div className="text-[10px] text-zinc-500 truncate">{p.provider} • {p.model}</div>
                                </div>
                            ))}
                        </div>
                        <div className="space-y-1.5 pt-1">
                            <button
                                onClick={() => handleCreate('QwenCli')}
                                className="w-full py-2 flex items-center justify-center gap-2 border border-dashed border-zinc-700 rounded-lg text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300 transition text-[10px] font-medium"
                            >
                                <Plus className="w-3 h-3" /> Новый CLI провайдер
                            </button>
                        </div>
                    </div>

                    {/* 1С:Напарник Group */}
                    <div className="space-y-2">
                        <div className="px-1 flex items-center gap-2 opacity-50">
                            <span className="text-[10px] uppercase font-black tracking-widest text-zinc-400">1С:Напарник</span>
                            <div className="h-[1px] flex-1 bg-zinc-800"></div>
                        </div>
                        <div className="space-y-1.5">
                            {profiles.profiles.filter(p => p.provider === 'OneCNaparnik').map(p => (
                                <div
                                    key={p.id}
                                    onClick={() => setEditingId(p.id)}
                                    className={`p-2 sm:p-3 rounded-lg border cursor-pointer transition-all ${editingId === p.id
                                        ? 'border-orange-400 bg-orange-400/10'
                                        : 'border-zinc-800 bg-zinc-800 hover:border-zinc-600'
                                        }`}
                                >
                                    <div className="flex justify-between items-center mb-0.5">
                                        <span className="font-medium text-xs sm:text-sm text-zinc-200 truncate pr-1">{p.name}</span>
                                        {profiles.active_profile_id === p.id && <Check className="w-3 h-3 text-orange-400 flex-shrink-0" />}
                                    </div>
                                    <div className="text-[10px] text-zinc-500 truncate">code.1c.ai</div>
                                </div>
                            ))}
                        </div>
                        <div className="space-y-1.5 pt-1">
                            <button
                                onClick={() => handleCreate('OneCNaparnik')}
                                className="w-full py-2 flex items-center justify-center gap-2 border border-dashed border-zinc-700 rounded-lg text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300 transition text-[10px] font-medium"
                            >
                                <Plus className="w-3 h-3" /> Добавить Напарника
                            </button>
                        </div>
                    </div>
                </div>
            </div>

            {/* Main Form */}
            <div className="flex-1 p-4 sm:p-6 bg-zinc-900 overflow-y-auto">
                {editForm ? (
                    <div className="space-y-6 max-w-xl">
                        <div className="flex justify-between items-center pb-4 border-b border-zinc-800">
                            <h3 className="text-lg font-semibold text-zinc-100">Edit Profile</h3>
                            <div className="flex gap-2">
                                {profiles.active_profile_id !== editForm.id && (
                                    <button onClick={() => handleSetActive(editForm.id)} className="text-xs bg-zinc-800 hover:bg-zinc-700 text-zinc-300 px-3 py-1.5 rounded border border-zinc-700 transition-colors">Set Active</button>
                                )}
                                <button onClick={() => handleDelete(editForm.id)} className="p-1.5 text-red-400 hover:bg-red-500/10 rounded transition-colors"><Trash2 className="w-4 h-4" /></button>
                            </div>
                        </div>

                        {/* Name & Provider */}
                        <div className="flex flex-wrap gap-4">
                            <div className="flex-1 min-w-[150px]">
                                <label className="text-xs text-zinc-500 uppercase font-bold px-1">Profile Name</label>
                                <input
                                    className="w-full mt-1 bg-zinc-950 border border-zinc-800 rounded-md px-3 h-9 text-sm focus:border-blue-500 outline-none text-zinc-200"
                                    value={editForm.name}
                                    onChange={e => setEditForm({ ...editForm, name: e.target.value })}
                                />
                            </div>
                            <div className="flex-1 min-w-[150px]">
                                <label className="text-xs text-zinc-500 uppercase font-bold px-1">Provider</label>
                                <Select value={editForm.provider} onValueChange={v => {
                                    setEditForm(prev => {
                                        if (!prev) return null;
                                        const def = PROVIDERS.find(p => p.value === v);
                                        return {
                                            ...prev,
                                            provider: v,
                                            base_url: def?.defaultUrl || '',
                                            model: def?.defaultModel || ''
                                        };
                                    });
                                }}>
                                    <SelectTrigger className="w-full mt-1 bg-zinc-950 border border-zinc-800 h-9 px-3 rounded-md focus:ring-1 focus:ring-blue-500 shadow-none transition-all outline-none">
                                        <SelectValue />
                                    </SelectTrigger>
                                    <SelectContent>
                                        {editForm.provider === 'OneCNaparnik' ? (
                                            <>
                                                <div className="px-2 py-1.5 text-[10px] font-bold text-zinc-500 uppercase tracking-wider">1С:Напарник</div>
                                                {PROVIDERS.filter(p => p.type === 'naparnik').map(p => <SelectItem key={p.value} value={p.value} className="text-xs">{p.label}</SelectItem>)}
                                            </>
                                        ) : PROVIDERS.find(p => p.value === editForm.provider)?.type === 'cli' ? (
                                            <>
                                                <div className="px-2 py-1.5 text-[10px] font-bold text-zinc-500 uppercase tracking-wider">CLI Провайдеры</div>
                                                {PROVIDERS.filter(p => p.type === 'cli').map(p => <SelectItem key={p.value} value={p.value} className="text-xs">{p.label}</SelectItem>)}
                                            </>
                                        ) : (
                                            <>
                                                <div className="px-2 py-1.5 text-[10px] font-bold text-zinc-500 uppercase tracking-wider">LLM Ассистенты</div>
                                                {PROVIDERS.filter(p => p.type === 'standard').map(p => <SelectItem key={p.value} value={p.value} className="text-xs">{p.label}</SelectItem>)}
                                            </>
                                        )}
                                    </SelectContent>
                                </Select>
                            </div>
                        </div>

                        {/* API Key / CLI Auth Section */}
                        <div className="space-y-4">
                            {editForm.provider === 'QwenCli' && (
                                <div className="p-4 bg-zinc-950/50 rounded-lg border border-zinc-800 space-y-4">
                                    <div className="flex items-center justify-between">
                                        <div className="flex items-center gap-2">
                                            <label className="text-xs text-zinc-500 uppercase font-bold">Authentication</label>
                                            {loadingStatus && <RefreshCw className="w-3 h-3 animate-spin text-zinc-500" />}
                                        </div>
                                        {cliStatus?.is_authenticated ? (
                                            <span className="flex items-center gap-1.5 text-[10px] bg-green-500/10 text-green-500 px-2 py-0.5 rounded-full border border-green-500/20 font-medium whitespace-nowrap">
                                                <Check className="w-3 h-3" /> Logged In
                                            </span>
                                        ) : (
                                            <span className="flex items-center gap-1.5 text-[10px] bg-red-500/10 text-red-500 px-2 py-0.5 rounded-full border border-red-500/20 font-medium whitespace-nowrap">
                                                <X className="w-3 h-3" /> Logged Out
                                            </span>
                                        )}
                                    </div>

                                    <div className="space-y-4">
                                        {cliStatus?.is_authenticated ? (
                                            <>
                                                {cliStatus.usage ? (
                                                    <div className="p-3 bg-zinc-900 border border-zinc-800 rounded-lg">
                                                        <div className="flex justify-between items-center mb-2">
                                                            <div className="flex items-center gap-2">
                                                                <span className="text-xs text-zinc-400 font-medium">Daily Limit</span>
                                                                <button
                                                                    onClick={() => fetchCliStatus(editForm.id, 'qwen', true)}
                                                                    disabled={loadingStatus}
                                                                    className="p-1 hover:bg-zinc-800 rounded transition-colors"
                                                                    title="Refresh limits"
                                                                >
                                                                    <RefreshCw className={`w-3 h-3 ${loadingStatus ? 'animate-spin' : ''} text-zinc-500`} />
                                                                </button>
                                                            </div>
                                                            <span className="text-xs text-zinc-200 font-mono">
                                                                {cliStatus.usage.requests_used} / {cliStatus.usage.requests_limit > 0 ? cliStatus.usage.requests_limit : '?'}
                                                            </span>
                                                        </div>
                                                        <div className="w-full h-1.5 bg-zinc-800 rounded-full overflow-hidden">
                                                            <div
                                                                className={`h-full transition-all duration-500 rounded-full ${cliStatus.usage.requests_limit > 0 && (cliStatus.usage.requests_used / cliStatus.usage.requests_limit) > 0.8 ? 'bg-amber-500' : 'bg-blue-500'}`}
                                                                style={{ width: cliStatus.usage.requests_limit > 0 ? `${Math.min(100, (cliStatus.usage.requests_used / cliStatus.usage.requests_limit) * 100)}%` : '0%' }}
                                                            />
                                                        </div>
                                                        {cliStatus.usage.resets_at && (
                                                            <p className="text-[10px] text-zinc-500 mt-2 flex items-center gap-1">
                                                                <Info className="w-3 h-3" />
                                                                Resets at: {new Date(cliStatus.usage.resets_at).toLocaleString()}
                                                            </p>
                                                        )}
                                                    </div>
                                                ) : (
                                                    <button
                                                        onClick={() => fetchCliStatus(editForm.id, 'qwen', true)}
                                                        disabled={loadingStatus}
                                                        className="w-full h-9 flex items-center justify-center gap-2 bg-zinc-900 hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 rounded-lg border border-zinc-800 text-xs font-medium transition-all disabled:opacity-50"
                                                    >
                                                        <RefreshCw className={`w-3 h-3 ${loadingStatus ? 'animate-spin' : ''}`} />
                                                        {loadingStatus ? 'Loading limits...' : 'Load usage limits'}
                                                    </button>
                                                )}

                                                <button
                                                    onClick={async () => {
                                                        await cliProvidersApi.logout(editForm.id, 'qwen');
                                                        fetchCliStatus(editForm.id, 'qwen');
                                                    }}
                                                    className="w-full h-10 flex items-center justify-center gap-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-200 rounded-lg border border-zinc-700 text-sm font-medium transition-all"
                                                >
                                                    <LogOut className="w-4 h-4" /> Logout from Qwen
                                                </button>
                                            </>
                                        ) : (
                                            <button
                                                onClick={() => setIsAuthModalOpen(true)}
                                                className="w-full h-12 flex items-center justify-center gap-2 bg-blue-600 hover:bg-blue-500 text-white rounded-lg shadow-lg shadow-blue-900/10 text-sm font-bold transition-all active:scale-[0.98]"
                                            >
                                                <LogIn className="w-5 h-5" /> Login to Qwen Account
                                            </button>
                                        )}
                                    </div>
                                    <p className="text-[10px] text-zinc-500 leading-relaxed px-1">
                                        Qwen Code CLI integration uses official OAuth Device Flow.
                                        Tokens are stored securely in your system's Keychain.
                                    </p>
                                </div>
                            )}

                            {editForm.provider === 'OneCNaparnik' && (
                                <div className="p-4 bg-zinc-950/50 rounded-lg border border-zinc-800 space-y-3">
                                    <label className="text-xs text-zinc-500 uppercase font-bold">Токен code.1c.ai</label>
                                    <input
                                        type="password"
                                        className="w-full bg-zinc-950 border border-zinc-800 rounded-md px-3 h-9 text-sm focus:border-orange-500 outline-none placeholder-zinc-700 text-zinc-200"
                                        placeholder={editForm.api_key_encrypted ? "•••••••••••• (сохранён)" : "Вставьте токен..."}
                                        value={newApiKey}
                                        onChange={e => setNewApiKey(e.target.value)}
                                    />
                                    <p className="text-[10px] text-zinc-500 leading-relaxed flex items-start gap-1.5">
                                        <Info className="w-3 h-3 shrink-0 mt-0.5" />
                                        <span>
                                            Получить токен:{' '}
                                            <button
                                                type="button"
                                                onClick={() => openUrl('https://code.1c.ai')}
                                                className="text-orange-400 hover:text-orange-300 inline-flex items-center gap-0.5 transition-colors"
                                            >
                                                code.1c.ai <ExternalLink className="w-2.5 h-2.5" />
                                            </button>
                                            {' '}→ Профиль → API токен.
                                            Токен хранится зашифрованным в системном keychain.
                                        </span>
                                    </p>
                                </div>
                            )}

                        {editForm.provider !== 'QwenCli' && editForm.provider !== 'OneCNaparnik' && (
                                <div>
                                    <label className="text-xs text-zinc-500 uppercase font-bold px-1">API Key</label>
                                    <input
                                        type="password"
                                        className="w-full mt-1 bg-zinc-950 border border-zinc-800 rounded-md px-3 h-9 text-sm focus:border-blue-500 outline-none placeholder-zinc-700 text-zinc-200"
                                        placeholder={editForm.api_key_encrypted ? "•••••••••••• (Encrypted)" : "sk-..."}
                                        value={newApiKey}
                                        onChange={e => setNewApiKey(e.target.value)}
                                    />
                                </div>
                            )}
                        </div>

                        {PROVIDERS.find(p => p.value === editForm.provider)?.type !== 'cli' && editForm.provider !== 'OneCNaparnik' && (
                            <div>
                                <label className="text-xs text-zinc-500 uppercase font-bold px-1">Base URL</label>
                                <input
                                    className="w-full mt-1 bg-zinc-950 border border-zinc-800 rounded-md px-3 h-9 text-sm focus:border-blue-500 outline-none font-mono text-zinc-400"
                                    value={editForm.base_url || ''}
                                    onChange={e => setEditForm({ ...editForm, base_url: e.target.value })}
                                />
                            </div>
                        )}

                        {/* Model Selection — hidden for OneCNaparnik */}
                        {editForm.provider !== 'OneCNaparnik' && <div className="p-4 bg-zinc-950/50 rounded-lg border border-zinc-800 space-y-4">
                            <div className="flex justify-between items-end">
                                <label className="text-xs text-zinc-500 uppercase font-bold px-1">Model ID</label>
                                {PROVIDERS.find(p => p.value === editForm.provider)?.type !== 'cli' && (
                                    <button
                                        onClick={handleFetchModels}
                                        disabled={loadingModels}
                                        className="text-xs flex items-center gap-1 text-blue-400 hover:text-blue-300 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                                    >
                                        <RefreshCw className={`w-3 h-3 ${loadingModels ? 'animate-spin' : ''}`} />
                                        {loadingModels ? 'Fetching...' : 'Fetch from API'}
                                    </button>
                                )}
                            </div>

                            <div className="relative">
                                {modelList.length > 0 ? (
                                    <Select
                                        value={editForm.model}
                                        onValueChange={v => {
                                            const m = modelList.find(m => m.id === v);
                                            setEditForm(prev => {
                                                if (!prev) return prev;
                                                return {
                                                    ...prev,
                                                    model: v,
                                                    max_tokens: m?.context_window || prev.max_tokens,
                                                    context_window_override: m?.context_window
                                                };
                                            });
                                        }}
                                    >
                                        <SelectTrigger className="w-full bg-zinc-900 border-zinc-700 h-9 px-3">
                                            <SelectValue placeholder="Select a model" />
                                        </SelectTrigger>
                                        <SelectContent className="max-h-60">
                                            {modelList.map((m: any) => (
                                                <SelectItem key={m.id} value={m.id}>
                                                    <div className="flex items-center justify-between gap-4 w-full pr-2">
                                                        <span className="truncate text-sm font-medium">{m.id}</span>
                                                        <span className="text-[10px] text-zinc-500 font-mono flex-shrink-0">
                                                            {m.context_window ? `${Math.round(m.context_window / 1024)}k` : ''}
                                                        </span>
                                                    </div>
                                                </SelectItem>
                                            ))}
                                        </SelectContent>
                                    </Select>
                                ) : (
                                    <input
                                        className="w-full bg-zinc-900 border border-zinc-700 rounded-md px-3 h-9 text-sm focus:border-blue-500 outline-none text-zinc-200"
                                        value={editForm.model}
                                        onChange={e => setEditForm(prev => prev ? ({ ...prev, model: e.target.value }) : null)}
                                        placeholder="gpt-4, qwen-2.5-coder, etc."
                                    />
                                )}
                            </div>

                            <div className="flex flex-wrap gap-4 pt-2">
                                <div className="flex-1 min-w-[120px]">
                                    <label className="text-xs text-zinc-500 uppercase font-bold px-1">
                                        Max tokens
                                        {editForm.provider === 'QwenCli' && (
                                            <span className="ml-1 text-zinc-600 normal-case font-normal">(фиксировано 65536)</span>
                                        )}
                                    </label>
                                    <input
                                        type="number"
                                        className="w-full mt-1 bg-zinc-900 border border-zinc-700 rounded-md px-3 h-9 text-sm text-zinc-200"
                                        value={editForm.max_tokens}
                                        onChange={e => setEditForm({ ...editForm, max_tokens: parseInt(e.target.value) || 0 })}
                                    />
                                </div>
                                <div className="flex-1 min-w-[120px]">
                                    <label className="text-xs text-zinc-500 uppercase font-bold px-1 whitespace-nowrap overflow-hidden text-ellipsis">
                                        Temperature
                                        {editForm.provider === 'QwenCli' && editForm.enable_thinking && (
                                            <span className="ml-1 text-amber-600 normal-case font-normal">(Thinking → 1.0)</span>
                                        )}
                                    </label>
                                    <input
                                        type="number" step="0.1" min="0" max="2"
                                        className="w-full mt-1 bg-zinc-900 border border-zinc-700 rounded-md px-3 h-9 text-sm text-zinc-200"
                                        value={editForm.temperature}
                                        onChange={e => setEditForm({ ...editForm, temperature: parseFloat(e.target.value) || 0.7 })}
                                    />
                                </div>
                            </div>

                            {/* Thinking mode toggle — Qwen CLI only */}
                            {editForm.provider === 'QwenCli' && (
                                <div className="flex items-center justify-between pt-3 px-1">
                                    <div>
                                        <span className="text-xs text-zinc-400 font-medium">Режим размышлений</span>
                                        <p className="text-[10px] text-zinc-600 mt-0.5">
                                            enable_thinking · budget 8192 токенов · temp → 1.0 (возврат к настройке при генерации)
                                        </p>
                                    </div>
                                    <button
                                        type="button"
                                        onClick={() => setEditForm({ ...editForm, enable_thinking: !editForm.enable_thinking })}
                                        className={`relative w-9 h-5 rounded-full transition-colors focus:outline-none ${editForm.enable_thinking ? 'bg-blue-500' : 'bg-zinc-700'}`}
                                    >
                                        <span className={`absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform ${editForm.enable_thinking ? 'translate-x-4' : 'translate-x-0'}`} />
                                    </button>
                                </div>
                            )}
                        </div>}

                        {/* Save Button */}
                        <div className="pt-4 pb-10">
                            <button
                                onClick={handleSave}
                                disabled={isSaving}
                                className={`w-full py-3 ${showSaved ? 'bg-green-600 hover:bg-green-500' : 'bg-blue-600 hover:bg-blue-500'} text-white rounded-xl font-bold text-sm flex items-center justify-center gap-2 transition-all disabled:opacity-50 active:scale-[0.98] shadow-lg`}
                            >
                                {isSaving ? (
                                    <>
                                        <RefreshCw className="w-4 h-4 animate-spin" />
                                        Saving...
                                    </>
                                ) : showSaved ? (
                                    <>
                                        <Check className="w-4 h-4" />
                                        Saved!
                                    </>
                                ) : (
                                    <>
                                        <Save className="w-4 h-4" />
                                        Save Profile
                                    </>
                                )}
                            </button>
                        </div>
                    </div>
                ) : (
                    <div className="h-full flex flex-col items-center justify-center text-zinc-500 gap-4">
                        <div className="p-4 bg-zinc-800/20 rounded-full border border-zinc-800/50">
                            <Plus className="w-8 h-8 opacity-20" />
                        </div>
                        <p className="text-sm">Select or create an LLM profile</p>
                    </div>
                )}
            </div>

            <QwenAuthModal
                isOpen={isAuthModalOpen}
                onClose={() => setIsAuthModalOpen(false)}
                onSuccess={async (access_token, refresh_token, expires_at, resource_url) => {
                    console.log('[DEBUG] LLMSettings: Qwen Auth Success, saving token...');
                    if (!editForm) return;
                    try {
                        await cliProvidersApi.saveToken(editForm.id, 'qwen', access_token, refresh_token, expires_at, resource_url);
                        console.log('[DEBUG] LLMSettings: Token saved successfully');
                        await fetchCliStatus(editForm.id, 'qwen');
                    } catch (err) {
                        console.error('[DEBUG] LLMSettings: Failed to save token:', err);
                    }
                }}
            />
        </div >
    );
}
