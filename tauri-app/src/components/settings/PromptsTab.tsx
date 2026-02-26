import React, { useState, useEffect } from 'react';
import { Save, Plus, Trash2, FileText, ChevronDown, ChevronUp, Code, Shield, Zap, Sparkles, User, HardHat, Edit2, Terminal } from 'lucide-react';
import {
    AppSettings,
    CustomPromptsSettings,
    PromptTemplate,
    CodeGenerationSettings,
    PromptBehaviorPreset,
    DEFAULT_CUSTOM_PROMPTS,
    DEFAULT_CODE_GENERATION
} from '../../types/settings';

interface PromptsTabProps {
    settings: AppSettings;
    onSettingsChange: (settings: AppSettings) => void;
    onSave: () => void;
    saving: boolean;
}

function TokenCode({ code, colorClass = 'text-blue-400/80 bg-blue-400/5' }: { code: string, colorClass?: string }) {
    const [copied, setCopied] = useState(false);

    const handleCopy = () => {
        navigator.clipboard.writeText(code);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
    };

    return (
        <code
            onClick={handleCopy}
            className={`text-[9px] ${copied ? 'text-green-500 bg-green-500/10 font-bold' : colorClass} px-1.5 py-0.5 rounded cursor-pointer hover:bg-white/5 transition-all active:scale-95 select-none border border-transparent hover:border-white/10`}
            title="Нажмите, чтобы скопировать"
        >
            {copied ? 'Скопировано!' : code}
        </code>
    );
}

export function PromptsTab({ settings, onSettingsChange, onSave, saving }: PromptsTabProps) {
    const [localSettings, setLocalSettings] = useState<CustomPromptsSettings>(
        settings.custom_prompts || DEFAULT_CUSTOM_PROMPTS
    );
    const [codeGenSettings, setCodeGenSettings] = useState<CodeGenerationSettings>(
        settings.code_generation || DEFAULT_CODE_GENERATION
    );
    const [expandedTemplate, setExpandedTemplate] = useState<string | null>(null);
    const [showAdvanced, setShowAdvanced] = useState(false);
    const [showMarkers, setShowMarkers] = useState(false);

    useEffect(() => {
        setLocalSettings(settings.custom_prompts || DEFAULT_CUSTOM_PROMPTS);
        setCodeGenSettings(settings.code_generation || DEFAULT_CODE_GENERATION);
    }, [settings.custom_prompts, settings.code_generation]);

    const updateLocalSettings = (updates: Partial<CustomPromptsSettings>) => {
        const newSettings = { ...localSettings, ...updates };
        setLocalSettings(newSettings);
        onSettingsChange({ ...settings, custom_prompts: newSettings });
    };

    const updateCodeGenSettings = (updates: Partial<CodeGenerationSettings>) => {
        const newSettings = { ...codeGenSettings, ...updates };
        setCodeGenSettings(newSettings);
        onSettingsChange({ ...settings, code_generation: newSettings });
    };

    const updateTemplate = (index: number, updates: Partial<PromptTemplate>) => {
        const newTemplates = [...localSettings.templates];
        newTemplates[index] = { ...newTemplates[index], ...updates };
        updateLocalSettings({ templates: newTemplates });
    };

    const addTemplate = () => {
        const newTemplate: PromptTemplate = {
            id: `custom-${Date.now()}`,
            name: 'Новое правило',
            description: 'Краткое описание',
            content: '',
            enabled: true
        };
        updateLocalSettings({ templates: [...localSettings.templates, newTemplate] });
        setExpandedTemplate(newTemplate.id);
    };

    const removeTemplate = (index: number) => {
        const newTemplates = localSettings.templates.filter((_, i) => i !== index);
        updateLocalSettings({ templates: newTemplates });
    };

    const profileDescriptions: Record<PromptBehaviorPreset, { title: string; desc: string; icon: any; color: string; badge: string }> = {
        project: {
            title: 'Свой код',
            desc: 'Для новой разработки в своих модулях. Чистый код, стандарты 1С/БСП, свободный рефакторинг.',
            icon: User,
            color: 'text-blue-400',
            badge: 'Моя разработка'
        },
        maintenance: {
            title: 'Чужой код',
            desc: 'Работа с типовыми, Legacy или чужими модулями. Жесткая изоляция правок комментариями, запрет рефакторинга.',
            icon: HardHat,
            color: 'text-orange-400',
            badge: 'Поддержка / Внедрение'
        },
        cli: {
            title: 'CLI Ассистент',
            desc: 'Оптимизирован для работы через внешние CLI-провайдеры (Qwen, DeepSeek). Экономный расход токенов и фокус на конкретных изменениях.',
            icon: Terminal,
            color: 'text-amber-400',
            badge: 'Free / CLI'
        }
    };

    return (
        <div className="space-y-8 pb-24">
            {/* 1. Выбор сценария */}
            <div className="space-y-4">
                <div className="flex flex-col gap-1">
                    <h2 className="text-lg font-bold text-zinc-100 flex items-center gap-2">
                        <Sparkles className="w-5 h-5 text-blue-400" />
                        Сценарий работы
                    </h2>
                    <p className="text-xs text-zinc-500">Выберите контекст вашей текущей задачи для оптимальной стратегии ИИ.</p>
                </div>

                <div className="grid grid-cols-1 gap-4">
                    {(Object.keys(profileDescriptions) as PromptBehaviorPreset[]).map((preset) => {
                        const Icon = profileDescriptions[preset].icon;
                        const active = codeGenSettings.behavior_preset === preset;
                        return (
                            <React.Fragment key={preset}>
                                <button
                                    key={preset}
                                    onClick={() => updateCodeGenSettings({ behavior_preset: preset })}
                                    className={`p-5 rounded-2xl border-2 text-left transition-all relative overflow-hidden group flex items-center gap-5 ${active ? 'border-zinc-500 bg-zinc-800/80 shadow-lg' : 'border-zinc-800 bg-zinc-900/40 hover:border-zinc-700'
                                        }`}
                                >
                                    <div className={`p-4 rounded-xl ${active ? 'bg-zinc-700/50' : 'bg-zinc-800/50'} transition-all`}>
                                        <Icon className={`w-8 h-8 ${active ? profileDescriptions[preset].color : 'text-zinc-600'}`} />
                                    </div>
                                    <div className="flex-1 space-y-1">
                                        <div className="flex items-center gap-3">
                                            <div className={`text-base font-bold ${active ? 'text-zinc-100' : 'text-zinc-400'}`}>
                                                {profileDescriptions[preset].title}
                                            </div>
                                            <span className={`text-[9px] px-1.5 py-0.5 rounded uppercase font-bold tracking-wider ${active ? 'bg-zinc-700 text-zinc-300' : 'bg-zinc-800 text-zinc-600'
                                                }`}>
                                                {profileDescriptions[preset].badge}
                                            </span>
                                        </div>
                                        <div className={`text-xs ${active ? 'text-zinc-400' : 'text-zinc-600'} transition-colors leading-relaxed`}>
                                            {profileDescriptions[preset].desc}
                                        </div>
                                    </div>
                                    {active && (
                                        <div className="absolute right-6 top-1/2 -translate-y-1/2">
                                            <div className="w-2 h-2 rounded-full bg-blue-500 shadow-[0_0_10px_rgba(59,130,246,0.5)]" />
                                        </div>
                                    )}
                                </button>

                                {preset === 'maintenance' && active && (
                                    <div className="mt-2 ml-14 animate-in slide-in-from-top-2 duration-300">
                                        <button
                                            onClick={(e) => {
                                                e.stopPropagation();
                                                setShowMarkers(!showMarkers);
                                            }}
                                            className="flex items-center gap-2 text-[11px] font-bold text-zinc-500 hover:text-zinc-300 transition-colors uppercase tracking-wider mb-3 px-2 py-1 bg-zinc-800/50 rounded-lg border border-zinc-700/50"
                                        >
                                            <Edit2 className="w-3 h-3" />
                                            Настройка маркеров
                                            {showMarkers ? <ChevronUp className="w-3 h-3" /> : <ChevronDown className="w-3 h-3" />}
                                        </button>

                                        {showMarkers && (
                                            <div className="space-y-6 pb-6 pr-4">
                                                <div className="flex items-center gap-3 px-2">
                                                    <label className="relative inline-flex items-center cursor-pointer">
                                                        <input
                                                            type="checkbox"
                                                            checked={codeGenSettings.mark_changes}
                                                            onChange={(e) => updateCodeGenSettings({ mark_changes: e.target.checked })}
                                                            className="sr-only peer"
                                                        />
                                                        <div className="w-8 h-4.5 bg-zinc-700 rounded-full peer peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-3.5 after:w-3.5 after:transition-all peer-checked:bg-blue-600"></div>
                                                    </label>
                                                    <span className="text-[11px] text-zinc-400 font-medium">Маркировать изменения комментариями</span>
                                                </div>

                                                <div className="grid grid-cols-1 gap-5 pl-2">
                                                    {[
                                                        {
                                                            id: 'addition',
                                                            label: 'Добавление',
                                                            icon: <Plus className="w-3 h-3 text-green-500" />,
                                                            value: codeGenSettings.addition_marker_template,
                                                            field: 'addition_marker_template',
                                                        },
                                                        {
                                                            id: 'modification',
                                                            label: 'Изменение',
                                                            icon: <Edit2 className="w-3 h-3 text-blue-500" />,
                                                            value: codeGenSettings.modification_marker_template,
                                                            field: 'modification_marker_template',
                                                        },
                                                        {
                                                            id: 'deletion',
                                                            label: 'Удаление',
                                                            icon: <Trash2 className="w-3 h-3 text-red-500" />,
                                                            value: codeGenSettings.deletion_marker_template,
                                                            field: 'deletion_marker_template',
                                                        }
                                                    ].map((marker) => (
                                                        <div key={marker.id} className="space-y-2">
                                                            <label className="text-[9px] font-bold text-zinc-600 uppercase tracking-widest flex items-center gap-2 px-1">
                                                                {marker.icon}
                                                                {marker.label}
                                                            </label>
                                                            <textarea
                                                                rows={5}
                                                                value={marker.value}
                                                                onChange={(e) => updateCodeGenSettings({ [marker.field]: e.target.value })}
                                                                className="w-full bg-zinc-900 border border-zinc-700/50 rounded-xl p-3 text-zinc-300 text-[11px] focus:border-blue-500 outline-none font-mono resize-none leading-relaxed transition-all shadow-inner"
                                                                placeholder={`Введите шаблон... (${marker.id === 'modification' ? '{datetime}, {oldCode}, {newCode}' : marker.id === 'deletion' ? '{datetime}, {oldCode}' : '{datetime}, {newCode}'})`}
                                                            />
                                                            <div className="flex flex-wrap gap-2 px-1 items-center">
                                                                <span className="text-[9px] text-zinc-500">Доступно (клик для копирования):</span>
                                                                <TokenCode code="{date}" />
                                                                <TokenCode code="{datetime}" />
                                                                {marker.id !== 'deletion' && (
                                                                    <TokenCode code="{newCode}" colorClass="text-green-400/80 bg-green-400/5" />
                                                                )}
                                                                {(marker.id === 'modification' || marker.id === 'deletion') && (
                                                                    <TokenCode code="{oldCode}" colorClass="text-orange-400/80 bg-orange-400/5" />
                                                                )}
                                                            </div>
                                                        </div>
                                                    ))}
                                                </div>
                                            </div>
                                        )}
                                    </div>
                                )}
                            </React.Fragment>
                        );
                    })}
                </div>
            </div>

            {/* 2. Библиотека знаний (Правила) */}
            <div className="space-y-4">
                <div className="flex items-center justify-between">
                    <h2 className="text-sm font-bold text-zinc-100 flex items-center gap-2">
                        <FileText className="w-4 h-4 text-zinc-500" />
                        Дополнительные правила
                    </h2>
                    <button
                        onClick={addTemplate}
                        className="text-[11px] text-blue-400 hover:text-blue-300 transition-all font-bold px-3 py-1 bg-blue-400/5 rounded-lg border border-blue-400/20"
                    >
                        + Добавить правило
                    </button>
                </div>

                <div className="space-y-2">
                    {localSettings.templates.map((template, idx) => (
                        <div
                            key={template.id}
                            className={`rounded-xl border transition-all ${template.enabled ? 'border-zinc-700 bg-zinc-800/40' : 'border-zinc-800 bg-zinc-900/20 opacity-60 hover:opacity-100'
                                }`}
                        >
                            <div className="flex items-center gap-4 p-4">
                                <label className="relative inline-flex items-center cursor-pointer">
                                    <input
                                        type="checkbox"
                                        checked={template.enabled}
                                        onChange={(e) => updateTemplate(idx, { enabled: e.target.checked })}
                                        className="sr-only peer"
                                    />
                                    <div className="w-8 h-4.5 bg-zinc-700 rounded-full peer peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-3.5 after:w-3.5 after:transition-all peer-checked:bg-blue-600"></div>
                                </label>
                                <div
                                    className="flex-1 cursor-pointer select-none"
                                    onClick={() => setExpandedTemplate(expandedTemplate === template.id ? null : template.id)}
                                >
                                    <div className={`text-sm font-bold ${template.enabled ? 'text-zinc-200' : 'text-zinc-500'}`}>
                                        {template.name}
                                    </div>
                                    <div className="text-[10px] text-zinc-500 truncate max-w-[300px]">
                                        {template.description}
                                    </div>
                                </div>
                                <div className="flex items-center gap-2">
                                    <button
                                        onClick={() => setExpandedTemplate(expandedTemplate === template.id ? null : template.id)}
                                        className="p-2 text-zinc-600 hover:text-zinc-300 transition-colors"
                                    >
                                        {expandedTemplate === template.id ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
                                    </button>
                                    <button
                                        onClick={() => removeTemplate(idx)}
                                        className="p-2 text-zinc-600 hover:text-red-400 transition-colors"
                                    >
                                        <Trash2 className="w-4 h-4" />
                                    </button>
                                </div>
                            </div>

                            {expandedTemplate === template.id && (
                                <div className="px-4 pb-4 space-y-4 animate-in slide-in-from-top-1 duration-200">
                                    <div className="grid grid-cols-2 gap-4">
                                        <div className="space-y-1">
                                            <span className="text-[9px] uppercase font-bold text-zinc-600 ml-1">Название</span>
                                            <input
                                                type="text"
                                                value={template.name}
                                                onChange={(e) => updateTemplate(idx, { name: e.target.value })}
                                                className="w-full bg-zinc-900 border border-zinc-700 rounded-lg p-2 text-zinc-300 text-xs focus:border-blue-500 outline-none"
                                            />
                                        </div>
                                        <div className="space-y-1">
                                            <span className="text-[9px] uppercase font-bold text-zinc-600 ml-1">Описание</span>
                                            <input
                                                type="text"
                                                value={template.description}
                                                onChange={(e) => updateTemplate(idx, { description: e.target.value })}
                                                className="w-full bg-zinc-900 border border-zinc-700 rounded-lg p-2 text-zinc-300 text-xs focus:border-blue-500 outline-none"
                                            />
                                        </div>
                                    </div>
                                    <div className="space-y-1">
                                        <span className="text-[9px] uppercase font-bold text-zinc-600 ml-1">Инструкция для ИИ</span>
                                        <textarea
                                            value={template.content}
                                            onChange={(e) => updateTemplate(idx, { content: e.target.value })}
                                            className="w-full h-32 bg-zinc-900 border border-zinc-700 rounded-lg p-3 text-zinc-300 text-xs resize-none focus:border-blue-500 outline-none font-mono leading-relaxed"
                                        />
                                    </div>
                                </div>
                            )}
                        </div>
                    ))}
                </div>
            </div>

            {/* 3. Экспертные настройки */}
            <div className="pt-4 mt-8 border-t border-zinc-800/50">
                <button
                    onClick={() => setShowAdvanced(!showAdvanced)}
                    className="flex items-center gap-2 text-zinc-600 hover:text-zinc-400 transition-colors"
                >
                    <Zap className="w-3.5 h-3.5" />
                    <span className="text-[10px] font-bold uppercase tracking-[0.2em]">Экспертные настройки</span>
                    {showAdvanced ? <ChevronUp className="w-3.5 h-3.5" /> : <ChevronDown className="w-3.5 h-3.5" />}
                </button>

                {showAdvanced && (
                    <div className="mt-6 space-y-6 animate-in slide-in-from-top-2 duration-300">
                        <div className="space-y-2 pb-10">
                            <h3 className="text-[10px] font-bold text-zinc-500 uppercase tracking-widest flex items-center gap-2">
                                <Shield className="w-3.5 h-3.5" />
                                Глобальная роль (System Prefix)
                            </h3>
                            <textarea
                                value={localSettings.system_prefix}
                                onChange={(e) => updateLocalSettings({ system_prefix: e.target.value })}
                                className="w-full h-24 bg-zinc-800/30 border border-zinc-700 rounded-xl p-3 text-zinc-300 text-[11px] resize-none focus:border-blue-500 outline-none leading-relaxed"
                                placeholder="Опишите общую роль ИИ. Перекрывает стандартные промпты."
                            />
                        </div>
                    </div>
                )}
            </div>


        </div>
    );
}
