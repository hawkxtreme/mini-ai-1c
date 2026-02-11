import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Plus, Save, RefreshCw, Trash2, Check, ExternalLink } from 'lucide-react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

export interface LLMProfile {
    id: string;
    name: string;
    provider: string;
    model: string;
    api_key_encrypted: string;
    base_url: string | null;
    max_tokens: number;
    temperature: number;
    context_window_override?: number; // New field for manual override
}

export interface ProfileStore {
    profiles: LLMProfile[];
    active_profile_id: string;
}

interface LLMSettingsProps {
    profiles: ProfileStore;
    onUpdate: (store: ProfileStore) => void;
}

const PROVIDERS = [
    { value: 'OpenAI', label: 'OpenAI', defaultModel: 'gpt-4o', defaultUrl: 'https://api.openai.com' },
    { value: 'Anthropic', label: 'Anthropic', defaultModel: 'claude-3-5-sonnet-latest', defaultUrl: 'https://api.anthropic.com' },
    { value: 'Google', label: 'Google Gemini', defaultModel: 'gemini-1.5-pro', defaultUrl: 'https://generativelanguage.googleapis.com/v1beta/openai' },
    { value: 'DeepSeek', label: 'DeepSeek', defaultModel: 'deepseek-chat', defaultUrl: 'https://api.deepseek.com' },
    { value: 'Groq', label: 'Groq', defaultModel: 'llama-3.3-70b-versatile', defaultUrl: 'https://api.groq.com/openai/v1' },
    { value: 'Mistral', label: 'Mistral AI', defaultModel: 'mistral-large-latest', defaultUrl: 'https://api.mistral.ai/v1' },
    { value: 'XAI', label: 'xAI (Grok)', defaultModel: 'grok-beta', defaultUrl: 'https://api.x.ai/v1' },
    { value: 'Perplexity', label: 'Perplexity', defaultModel: 'sonar-reasoning', defaultUrl: 'https://api.perplexity.ai' },
    { value: 'OpenRouter', label: 'OpenRouter', defaultModel: 'google/gemini-2.0-flash-001', defaultUrl: 'https://openrouter.ai/api/v1' },
    { value: 'Ollama', label: 'Ollama (Local)', defaultModel: 'llama3', defaultUrl: 'http://localhost:11434' },
    { value: 'Custom', label: 'Custom / Other', defaultModel: '', defaultUrl: '' },
];

export function LLMSettings({ profiles, onUpdate }: LLMSettingsProps) {
    const [editingId, setEditingId] = useState<string | null>(null);
    const [editForm, setEditForm] = useState<LLMProfile | null>(null);
    const [newApiKey, setNewApiKey] = useState('');
    const [modelList, setModelList] = useState<any[]>([]);
    const [loadingModels, setLoadingModels] = useState(false);
    const [connectionTest, setConnectionTest] = useState<string | null>(null);

    // Select profile to edit
    useEffect(() => {
        if (editingId) {
            const p = profiles.profiles.find(p => p.id === editingId);
            if (p) {
                setEditForm({ ...p });
                setNewApiKey('');
                setModelList([]);
                setConnectionTest(null);
            }
        }
    }, [editingId, profiles]);

    const handleSave = async () => {
        if (!editForm) return;

        try {
            await invoke('save_profile', {
                profile: editForm,
                apiKey: newApiKey || null
            });
            const updated = await invoke<ProfileStore>('get_profiles');
            onUpdate(updated);
            alert('Profile saved!');
        } catch (e) {
            alert('Failed to save: ' + e);
        }
    };

    const handleDelete = async (id: string) => {
        if (!confirm('Delete this profile?')) return;
        try {
            await invoke('delete_profile', { profileId: id });
            const updated = await invoke<ProfileStore>('get_profiles');
            onUpdate(updated);
            if (editingId === id) setEditingId(null);
        } catch (e) {
            alert('Error: ' + e);
        }
    };

    const handleCreate = () => {
        const id = `profile_${Date.now()}`;
        const newProfile: LLMProfile = {
            id,
            name: 'New Profile',
            provider: 'OpenAI',
            model: 'gpt-4o-mini',
            api_key_encrypted: '',
            base_url: null,
            max_tokens: 4096,
            temperature: 0.7
        };
        // We can't save immediately without valid data, but we need it in the list to edit.
        // Option: Add to local state 'profiles' temporarily?
        // Better: Save basic profile immediately or handle "Draft" state.
        // Let's use a "Draft" approach or just mock it in the list.
        // For simplicity, let's just append to local list via parent update or effectively save it.
        // Actually, let's just save it.
        invoke('save_profile', { profile: newProfile, apiKey: null }).then(() => {
            invoke<ProfileStore>('get_profiles').then(onUpdate).then(() => setEditingId(id));
        });
    };

    const handleFetchModels = async () => {
        if (!editForm) return;
        setLoadingModels(true);
        try {
            if (newApiKey) {
                // Use explicit credentials from form
                const res = await invoke<any[]>('fetch_models_from_provider', {
                    providerId: editForm.provider,
                    baseUrl: editForm.base_url || PROVIDERS.find(p => p.value === editForm.provider)?.defaultUrl || '',
                    apiKey: newApiKey
                });
                const sorted = [...res].sort((a, b) => a.id.localeCompare(b.id));
                setModelList(sorted);
            } else if (editForm.api_key_encrypted) {
                // Use stored credentials
                // First save any pending changes (except key)
                await invoke('save_profile', { profile: editForm, apiKey: null });

                const res = await invoke<any[]>('fetch_models_for_profile', {
                    profileId: editForm.id
                });
                const sorted = [...res].sort((a, b) => a.id.localeCompare(b.id));
                setModelList(sorted);
            } else {
                alert("Please enter API Key first.");
            }

        } catch (e) {
            alert("Error fetching: " + e);
        }
        setLoadingModels(false);
    };

    const handleSetActive = async (id: string) => {
        await invoke('set_active_profile', { profileId: id });
        const updated = await invoke<ProfileStore>('get_profiles');
        onUpdate(updated);
    };

    return (
        <div className="flex h-full w-full">
            {/* Sidebar List */}
            <div className="w-1/3 border-r border-zinc-800 bg-zinc-900/30 overflow-y-auto p-3">
                <div className="space-y-2">
                    {profiles.profiles.map(p => (
                        <div
                            key={p.id}
                            onClick={() => setEditingId(p.id)}
                            className={`p-3 rounded-lg border cursor-pointer transition-all ${editingId === p.id
                                ? 'border-blue-500 bg-blue-500/10'
                                : 'border-zinc-800 bg-zinc-800 hover:border-zinc-600'
                                }`}
                        >
                            <div className="flex justify-between items-center mb-1">
                                <span className="font-medium text-zinc-200">{p.name}</span>
                                {profiles.active_profile_id === p.id && <Check className="w-4 h-4 text-green-500" />}
                            </div>
                            <div className="text-xs text-zinc-500 truncate">{p.provider} • {p.model}</div>
                        </div>
                    ))}
                    <button
                        onClick={handleCreate}
                        className="w-full py-2 flex items-center justify-center gap-2 border border-dashed border-zinc-700 rounded-lg text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200 transition"
                    >
                        <Plus className="w-4 h-4" /> New Profile
                    </button>
                </div>
            </div>

            {/* Main Form */}
            <div className="flex-1 p-6 bg-zinc-900 overflow-y-auto">
                {editForm ? (
                    <div className="space-y-6 max-w-xl">
                        <div className="flex justify-between items-center pb-4 border-b border-zinc-800">
                            <h3 className="text-lg font-semibold">Edit Profile</h3>
                            <div className="flex gap-2">
                                {profiles.active_profile_id !== editForm.id && (
                                    <button onClick={() => handleSetActive(editForm.id)} className="text-xs bg-zinc-800 hover:bg-zinc-700 px-3 py-1.5 rounded border border-zinc-700">Set Active</button>
                                )}
                                <button onClick={() => handleDelete(editForm.id)} className="p-1.5 text-red-400 hover:bg-red-500/10 rounded"><Trash2 className="w-4 h-4" /></button>
                            </div>
                        </div>

                        {/* Name & Provider */}
                        <div className="grid grid-cols-2 gap-4">
                            <div>
                                <label className="text-xs text-zinc-500 uppercase font-bold px-1">Profile Name</label>
                                <input
                                    className="w-full mt-1 bg-zinc-950 border border-zinc-800 rounded-md px-3 py-2 text-sm focus:border-blue-500 outline-none"
                                    value={editForm.name}
                                    onChange={e => setEditForm({ ...editForm, name: e.target.value })}
                                />
                            </div>
                            <div>
                                <label className="text-xs text-zinc-500 uppercase font-bold px-1">Provider</label>
                                <Select value={editForm.provider} onValueChange={v => {
                                    const def = PROVIDERS.find(p => p.value === v);
                                    setEditForm({
                                        ...editForm,
                                        provider: v,
                                        base_url: def?.defaultUrl || '',
                                        model: def?.defaultModel || ''
                                    });
                                }}>
                                    <SelectTrigger className="mt-1 bg-zinc-950 border-zinc-800"><SelectValue /></SelectTrigger>
                                    <SelectContent>
                                        {PROVIDERS.map(p => <SelectItem key={p.value} value={p.value}>{p.label}</SelectItem>)}
                                    </SelectContent>
                                </Select>
                            </div>
                        </div>

                        {/* Base URL (Swapped here) */}
                        <div>
                            <label className="text-xs text-zinc-500 uppercase font-bold px-1">Base URL</label>
                            <input
                                className="w-full mt-1 bg-zinc-950 border border-zinc-800 rounded-md px-3 py-2 text-sm focus:border-blue-500 outline-none font-mono text-zinc-400"
                                value={editForm.base_url || ''}
                                onChange={e => setEditForm({ ...editForm, base_url: e.target.value })}
                            />
                        </div>

                        {/* API Key (Swapped here) */}
                        <div>
                            <label className="text-xs text-zinc-500 uppercase font-bold px-1">API Key</label>
                            <input
                                type="password"
                                className="w-full mt-1 bg-zinc-950 border border-zinc-800 rounded-md px-3 py-2 text-sm focus:border-blue-500 outline-none placeholder-zinc-700"
                                placeholder={editForm.api_key_encrypted ? "•••••••••••• (Encrypted)" : "sk-..."}
                                value={newApiKey}
                                onChange={e => setNewApiKey(e.target.value)}
                            />
                        </div>

                        {/* Model Selection */}
                        <div className="p-4 bg-zinc-950/50 rounded-lg border border-zinc-800 space-y-4">
                            <div className="flex justify-between items-end">
                                <label className="text-xs text-zinc-500 uppercase font-bold px-1">Model ID</label>
                                <button
                                    onClick={handleFetchModels}
                                    disabled={loadingModels || (!newApiKey && !editForm.api_key_encrypted)}
                                    title={!newApiKey && !editForm.api_key_encrypted ? "Enter API Key to fetch models" : "Fetch models from API"}
                                    className="text-xs flex items-center gap-1 text-blue-400 hover:text-blue-300 disabled:opacity-50 disabled:cursor-not-allowed"
                                >
                                    <RefreshCw className={`w-3 h-3 ${loadingModels ? 'animate-spin' : ''}`} />
                                    {loadingModels ? 'Fetching...' : 'Fetch from API'}
                                </button>
                            </div>

                            <div className="relative">
                                {modelList.length > 0 ? (
                                    <Select
                                        value={editForm.model}
                                        onValueChange={v => {
                                            const m = modelList.find(m => m.id === v);
                                            console.log(`Selecting model: ${v}, context: ${m?.context_window}`);
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
                                        <SelectTrigger className="w-full bg-zinc-900 border-zinc-700">
                                            <SelectValue placeholder="Select a model" />
                                        </SelectTrigger>
                                        <SelectContent className="max-h-60 w-[var(--radix-select-trigger-width)]">
                                            {modelList.map((m: any) => (
                                                <SelectItem key={m.id} value={m.id}>
                                                    <div className="flex items-center justify-between gap-4 w-full pr-6">
                                                        <span className="truncate text-sm font-medium">{m.id}</span>
                                                        <span className="text-[10px] text-zinc-500 font-mono flex-shrink-0 opacity-70">
                                                            {m.context_window ? `${Math.round(m.context_window / 1024)}k` : ''}
                                                        </span>
                                                    </div>
                                                </SelectItem>
                                            ))}
                                        </SelectContent>
                                    </Select>
                                ) : (
                                    <input
                                        className="w-full bg-zinc-900 border border-zinc-700 rounded-md px-3 py-2 text-sm focus:border-blue-500 outline-none"
                                        value={editForm.model}
                                        onChange={e => setEditForm({ ...editForm, model: e.target.value })}
                                        placeholder="gpt-4, claude-3, etc."
                                    />
                                )}
                            </div>

                            {/* Model Details (Characteristics) */}
                            {(() => {
                                const selectedModel = modelList.find(m => m.id === editForm.model);
                                if (!selectedModel) return null;
                                return (
                                    <div className="mt-2 p-3 bg-zinc-900/50 border border-zinc-800 rounded-lg space-y-2">
                                        <div className="flex justify-between text-[11px]">
                                            <span className="text-zinc-500 uppercase font-bold">Max Tokens (Context)</span>
                                            <span className="text-zinc-300 font-mono">
                                                {selectedModel.context_window === 4096 ?
                                                    <span className="text-zinc-500">Default (4,096)</span> :
                                                    selectedModel.context_window?.toLocaleString() || 'Unknown'
                                                }
                                            </span>
                                        </div>
                                        {(selectedModel.cost_in !== undefined || selectedModel.cost_out !== undefined) && (
                                            <div className="flex justify-between text-[11px]">
                                                <span className="text-zinc-500 uppercase font-bold">Cost (per 1M tokens)</span>
                                                <span className="text-green-500 font-mono">
                                                    ${selectedModel.cost_in?.toFixed(2)} / ${selectedModel.cost_out?.toFixed(2)}
                                                </span>
                                            </div>
                                        )}
                                        {selectedModel.description && (
                                            <p className="text-[10px] text-zinc-600 italic leading-tight pt-1 border-t border-zinc-800/50">
                                                {selectedModel.description}
                                            </p>
                                        )}
                                    </div>
                                );
                            })()}

                            <div className="grid grid-cols-2 gap-4 pt-2">
                                <div>
                                    <label className="text-xs text-zinc-500 uppercase font-bold px-1">Context Window</label>
                                    <input
                                        type="number"
                                        className="w-full mt-1 bg-zinc-900 border border-zinc-700 rounded-md px-3 py-2 text-sm"
                                        value={editForm.max_tokens}
                                        onChange={e => setEditForm({ ...editForm, max_tokens: parseInt(e.target.value) || 0 })}
                                    />
                                </div>
                                <div>
                                    <label className="text-xs text-zinc-500 uppercase font-bold px-1">Temperature</label>
                                    <input
                                        type="number" step="0.1" min="0" max="2"
                                        className="w-full mt-1 bg-zinc-900 border border-zinc-700 rounded-md px-3 py-2 text-sm"
                                        value={editForm.temperature}
                                        onChange={e => setEditForm({ ...editForm, temperature: parseFloat(e.target.value) || 0.7 })}
                                    />
                                </div>
                            </div>
                        </div>

                        {/* Save Button */}
                        <div className="pt-4">
                            <button
                                onClick={handleSave}
                                className="w-full py-2 bg-blue-600 hover:bg-blue-500 text-white rounded-md font-medium flex items-center justify-center gap-2"
                            >
                                <Save className="w-4 h-4" /> Save Profile
                            </button>
                        </div>

                    </div>
                ) : (
                    <div className="h-full flex items-center justify-center text-zinc-500">
                        <p>Select or create a profile</p>
                    </div>
                )}
            </div>
        </div>
    );
}
