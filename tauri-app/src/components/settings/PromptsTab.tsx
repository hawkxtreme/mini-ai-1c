import React, { useState, useEffect } from 'react';
import { Save, Plus, Trash2, Info, FileText, ChevronDown, ChevronUp, Code } from 'lucide-react';
import {
    AppSettings,
    CustomPromptsSettings,
    PromptTemplate,
    CodeGenerationSettings,
    CodeGenerationMode,
    DEFAULT_CUSTOM_PROMPTS,
    DEFAULT_CODE_GENERATION
} from '../../types/settings';

interface PromptsTabProps {
    settings: AppSettings;
    onSettingsChange: (settings: AppSettings) => void;
    onSave: () => void;
    saving: boolean;
}

export function PromptsTab({ settings, onSettingsChange, onSave, saving }: PromptsTabProps) {
    // Инициализация с дефолтными значениями если поле отсутствует
    const [localSettings, setLocalSettings] = useState<CustomPromptsSettings>(
        settings.custom_prompts || DEFAULT_CUSTOM_PROMPTS
    );
    const [codeGenSettings, setCodeGenSettings] = useState<CodeGenerationSettings>(
        settings.code_generation || DEFAULT_CODE_GENERATION
    );
    const [expandedTemplate, setExpandedTemplate] = useState<string | null>(null);

    // Синхронизация с пропсами
    useEffect(() => {
        setLocalSettings(settings.custom_prompts || DEFAULT_CUSTOM_PROMPTS);
        setCodeGenSettings(settings.code_generation || DEFAULT_CODE_GENERATION);
    }, [settings.custom_prompts, settings.code_generation]);

    // Обновление локального состояния и проброс вверх
    const updateLocalSettings = (updates: Partial<CustomPromptsSettings>) => {
        const newSettings = { ...localSettings, ...updates };
        setLocalSettings(newSettings);
        onSettingsChange({
            ...settings,
            custom_prompts: newSettings
        });
    };

    // Обновление настроек генерации кода
    const updateCodeGenSettings = (updates: Partial<CodeGenerationSettings>) => {
        const newSettings = { ...codeGenSettings, ...updates };
        setCodeGenSettings(newSettings);
        onSettingsChange({
            ...settings,
            code_generation: newSettings
        });
    };

    // Обновление шаблона по индексу
    const updateTemplate = (index: number, updates: Partial<PromptTemplate>) => {
        const newTemplates = [...localSettings.templates];
        newTemplates[index] = { ...newTemplates[index], ...updates };
        updateLocalSettings({ templates: newTemplates });
    };

    // Добавление нового шаблона
    const addTemplate = () => {
        const newTemplate: PromptTemplate = {
            id: `custom-${Date.now()}`,
            name: 'Новый шаблон',
            description: 'Описание шаблона',
            content: '',
            enabled: false
        };
        updateLocalSettings({
            templates: [...localSettings.templates, newTemplate]
        });
        setExpandedTemplate(newTemplate.id);
    };

    // Удаление шаблона
    const removeTemplate = (index: number) => {
        const newTemplates = localSettings.templates.filter((_, i) => i !== index);
        updateLocalSettings({ templates: newTemplates });
    };

    // Описание режимов генерации
    const modeDescriptions: Record<CodeGenerationMode, { title: string; desc: string; icon: string }> = {
        full: {
            title: 'Полный код',
            desc: 'ИИ возвращает полный текст модуля с изменениями',
            icon: '📄'
        },
        diff: {
            title: 'Diff (Search/Replace)',
            desc: 'ИИ возвращает только изменения в формате Search/Replace блоков',
            icon: '🔧'
        },
        auto: {
            title: 'Авто',
            desc: 'Автоматический выбор режима по размеру модуля',
            icon: '⚡'
        }
    };

    const [showAdvanced, setShowAdvanced] = useState(false);

    return (
        <div className="space-y-6">
            {/* Секция 1: Основные настройки генерации */}
            <div className="space-y-4 p-4 bg-zinc-800/50 rounded-lg border border-zinc-700">
                <div className="flex items-center gap-2 mb-2">
                    <Code className="w-4 h-4 text-blue-400" />
                    <h3 className="text-sm font-semibold text-zinc-200">Режим работы ИИ</h3>
                </div>

                <div className="grid grid-cols-3 gap-3">
                    {(Object.keys(modeDescriptions) as CodeGenerationMode[]).map((mode) => (
                        <button
                            key={mode}
                            onClick={() => updateCodeGenSettings({ mode })}
                            className={`p-3 rounded-lg border text-left transition-all ${codeGenSettings.mode === mode
                                    ? 'border-blue-500 bg-blue-500/10'
                                    : 'border-zinc-700 bg-zinc-800/50 hover:border-zinc-600'
                                }`}
                        >
                            <div className="text-lg mb-1">{modeDescriptions[mode].icon}</div>
                            <div className={`text-sm font-medium ${codeGenSettings.mode === mode ? 'text-blue-400' : 'text-zinc-300'
                                }`}>
                                {modeDescriptions[mode].title}
                            </div>
                            <div className="text-xs text-zinc-500 mt-1">
                                {modeDescriptions[mode].desc}
                            </div>
                        </button>
                    ))}
                </div>

                <div className="pt-2 space-y-3 border-t border-zinc-700/50">
                    {/* Mark Changes (Главный переключатель) */}
                    <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                            <span className="text-sm text-zinc-300">Подписывать изменения ИИ</span>
                            <span title="Добавлять поясняющий комментарий к коду, который изменил ассистент">
                                <Info className="w-3.5 h-3.5 text-zinc-500 cursor-help" />
                            </span>
                        </div>
                        <label className="relative inline-flex items-center cursor-pointer">
                            <input
                                type="checkbox"
                                checked={codeGenSettings.mark_changes}
                                onChange={(e) => updateCodeGenSettings({ mark_changes: e.target.checked })}
                                className="sr-only peer"
                            />
                            <div className="w-9 h-5 bg-zinc-600 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-blue-600"></div>
                        </label>
                    </div>

                    {codeGenSettings.mark_changes && (
                        <div className="space-y-2 animate-in fade-in slide-in-from-top-1 duration-200">
                            <input
                                type="text"
                                value={codeGenSettings.change_marker_template}
                                onChange={(e) => updateCodeGenSettings({ change_marker_template: e.target.value })}
                                className="w-full bg-zinc-900 border border-zinc-700 rounded p-2 text-zinc-300 text-sm focus:border-blue-500 focus:outline-none"
                                placeholder="// [ИЗМЕНЕНО AI] - {date}"
                            />
                            <p className="text-[10px] text-zinc-500">
                                Переменные: <code className="text-zinc-400">{'{date}'}</code>, <code className="text-zinc-400">{'{author}'}</code>
                            </p>
                        </div>
                    )}
                </div>
            </div>

            {/* Секция 2: Шаблоны промптов */}
            <div className="space-y-3">
                <div className="flex items-center justify-between">
                    <label className="text-sm font-medium text-zinc-300 flex items-center gap-2">
                        <FileText className="w-4 h-4 text-green-400" />
                        Библиотека знаний (Шаблоны)
                    </label>
                    <button
                        onClick={addTemplate}
                        className="flex items-center gap-1 text-xs text-blue-400 hover:text-blue-300 transition-colors"
                    >
                        <Plus className="w-4 h-4" />
                        Добавить свои правила
                    </button>
                </div>

                <div className="space-y-2">
                    {localSettings.templates.map((template, idx) => (
                        <div
                            key={template.id}
                            className="bg-zinc-800/50 rounded-lg border border-zinc-700 overflow-hidden"
                        >
                            <div className="flex items-center gap-3 p-3">
                                <label className="relative inline-flex items-center cursor-pointer">
                                    <input
                                        type="checkbox"
                                        checked={template.enabled}
                                        onChange={(e) => updateTemplate(idx, { enabled: e.target.checked })}
                                        className="sr-only peer"
                                    />
                                    <div className="w-9 h-5 bg-zinc-600 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-green-600"></div>
                                </label>
                                <div
                                    className="flex-1 cursor-pointer"
                                    onClick={() => setExpandedTemplate(
                                        expandedTemplate === template.id ? null : template.id
                                    )}
                                >
                                    <div className="text-sm font-medium text-zinc-300">{template.name}</div>
                                    <div className="text-xs text-zinc-500">{template.description}</div>
                                </div>
                                <button
                                    onClick={() => removeTemplate(idx)}
                                    className="p-1 text-zinc-500 hover:text-red-400 transition-colors"
                                >
                                    <Trash2 className="w-4 h-4" />
                                </button>
                            </div>

                            {expandedTemplate === template.id && (
                                <div className="p-3 pt-0 space-y-3 border-t border-zinc-700 animate-in zoom-in-95 duration-200">
                                    <div className="grid grid-cols-2 gap-3 pt-3">
                                        <div>
                                            <input
                                                type="text"
                                                value={template.name}
                                                onChange={(e) => updateTemplate(idx, { name: e.target.value })}
                                                className="w-full bg-zinc-900 border border-zinc-700 rounded p-2 text-zinc-300 text-sm focus:border-blue-500 focus:outline-none"
                                            />
                                        </div>
                                        <div>
                                            <input
                                                type="text"
                                                value={template.description}
                                                onChange={(e) => updateTemplate(idx, { description: e.target.value })}
                                                className="w-full bg-zinc-900 border border-zinc-700 rounded p-2 text-zinc-300 text-sm focus:border-blue-500 focus:outline-none"
                                            />
                                        </div>
                                    </div>
                                    <textarea
                                        value={template.content}
                                        onChange={(e) => updateTemplate(idx, { content: e.target.value })}
                                        className="w-full h-24 bg-zinc-900 border border-zinc-700 rounded p-2 text-zinc-300 text-sm resize-none focus:border-blue-500 focus:outline-none font-mono"
                                        placeholder="Инструкции для ИИ..."
                                    />
                                </div>
                            )}
                        </div>
                    ))}
                </div>
            </div>

            {/* Секция 3: Продвинутые настройки (Для экспертов) */}
            <div className="border border-zinc-700 rounded-lg overflow-hidden">
                <button
                    onClick={() => setShowAdvanced(!showAdvanced)}
                    className="w-full flex items-center justify-between p-3 bg-zinc-800/30 hover:bg-zinc-800/50 transition-colors text-zinc-400"
                >
                    <span className="text-xs font-medium uppercase tracking-wider">Настройки для экспертов</span>
                    {showAdvanced ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
                </button>

                {showAdvanced && (
                    <div className="p-4 space-y-5 bg-zinc-900/40 border-t border-zinc-700 animate-in slide-in-from-top-2 duration-300">
                        <div className="space-y-2">
                            <label className="text-xs font-medium text-zinc-400">Глобальная роль ИИ (System Prefix)</label>
                            <textarea
                                value={localSettings.system_prefix}
                                onChange={(e) => updateLocalSettings({ system_prefix: e.target.value })}
                                className="w-full h-20 bg-zinc-800 border border-zinc-700 rounded-lg p-2 text-zinc-300 text-sm resize-none focus:border-blue-500 focus:outline-none"
                                placeholder="Опишите общую роль ассистента..."
                            />
                        </div>

                        <div className="space-y-2">
                            <label className="text-xs font-medium text-zinc-400">Правила правки кода</label>
                            <textarea
                                value={localSettings.on_code_change}
                                onChange={(e) => updateLocalSettings({ on_code_change: e.target.value })}
                                className="w-full h-20 bg-zinc-800 border border-zinc-700 rounded-lg p-2 text-zinc-300 text-sm resize-none focus:border-blue-500 focus:outline-none"
                                placeholder="Например: Не удаляй комментарии, используй БСП..."
                            />
                        </div>

                        <div className="space-y-2">
                            <label className="text-xs font-medium text-zinc-400">Правила написания нового кода</label>
                            <textarea
                                value={localSettings.on_code_generate}
                                onChange={(e) => updateLocalSettings({ on_code_generate: e.target.value })}
                                className="w-full h-20 bg-zinc-800 border border-zinc-700 rounded-lg p-2 text-zinc-300 text-sm resize-none focus:border-blue-500 focus:outline-none"
                                placeholder="Например: Всегда добавляй 'Экспорт' к новым процедурам..."
                            />
                        </div>

                        <div className="flex items-center justify-between pt-2">
                            <span className="text-xs text-zinc-500 italic">Опция "Сохранять copyright"</span>
                            <label className="relative inline-flex items-center cursor-pointer">
                                <input
                                    type="checkbox"
                                    checked={codeGenSettings.preserve_copyright}
                                    onChange={(e) => updateCodeGenSettings({ preserve_copyright: e.target.checked })}
                                    className="sr-only peer"
                                />
                                <div className="w-7 h-4 bg-zinc-600 rounded-full peer peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-3 after:w-3 after:transition-all peer-checked:bg-blue-600"></div>
                            </label>
                        </div>
                    </div>
                )}
            </div>

            {/* Save Button */}
            <div className="flex justify-end pt-4 border-t border-zinc-700">
                <button
                    onClick={onSave}
                    disabled={saving}
                    className="flex items-center gap-2 px-6 py-2 bg-blue-600 hover:bg-blue-500 disabled:bg-zinc-600 text-white font-medium rounded-lg transition-all shadow-lg shadow-blue-900/20 active:scale-95"
                >
                    <Save className="w-4 h-4" />
                    {saving ? 'Сохранение...' : 'Применить изменения'}
                </button>
            </div>
        </div>
    );
}
