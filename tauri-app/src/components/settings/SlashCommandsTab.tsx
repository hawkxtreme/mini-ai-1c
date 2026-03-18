import React, { useState } from 'react';
import { Save, Plus, Trash2, Command, Shield, Edit2, RotateCcw, ChevronDown, ChevronUp, Terminal, TextCursorInput } from 'lucide-react';
import {
    AppSettings,
    SlashCommand,
    DEFAULT_SLASH_COMMANDS
} from '../../types/settings';

function TokenCode({ code, colorClass = 'text-blue-400/80 bg-blue-400/5', onSelect }: { code: string, colorClass?: string, onSelect?: (code: string) => void }) {
    const [copied, setCopied] = useState(false);

    const handleClick = () => {
        if (onSelect) {
            onSelect(code);
        } else {
            navigator.clipboard.writeText(code);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        }
    };

    return (
        <code
            onClick={handleClick}
            className={`text-[10px] ${copied ? 'text-green-500 bg-green-500/10 font-bold' : colorClass} px-2 py-0.5 rounded cursor-pointer hover:bg-white/5 transition-all active:scale-95 select-none border border-transparent hover:border-white/10`}
            title={onSelect ? "Нажмите, чтобы вставить" : "Нажмите, чтобы скопировать"}
        >
            {copied ? 'Скопировано!' : code}
        </code>
    );
}

interface SlashCommandsTabProps {
    settings: AppSettings;
    onSettingsChange: (settings: AppSettings) => void;
    onSave: () => void;
    saving: boolean;
}

export function SlashCommandsTab({ settings, onSettingsChange, onSave, saving }: SlashCommandsTabProps) {
    const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());

    const slashCommands = settings.slash_commands || DEFAULT_SLASH_COMMANDS;

    const toggleExpand = (id: string) => {
        const newExpanded = new Set(expandedIds);
        if (newExpanded.has(id)) {
            newExpanded.delete(id);
        } else {
            newExpanded.add(id);
        }
        setExpandedIds(newExpanded);
    };

    const updateCommand = (index: number, updates: Partial<SlashCommand>) => {
        const newCommands = [...slashCommands];
        newCommands[index] = { ...newCommands[index], ...updates };
        onSettingsChange({
            ...settings,
            slash_commands: newCommands
        });
    };

    const addCommand = () => {
        const id = `custom-${Date.now()}`;
        const newCommand: SlashCommand = {
            id,
            command: 'новая',
            name: 'Новая команда',
            description: 'Описание команды',
            template: 'Шаблон текста...\n\nКод:\n```bsl\n{code}\n```',
            is_enabled: true,
            is_system: false
        };
        onSettingsChange({
            ...settings,
            slash_commands: [...slashCommands, newCommand]
        });
        setExpandedIds(new Set([...expandedIds, id]));
    };

    const removeCommand = (index: number) => {
        const newCommands = slashCommands.filter((_, i) => i !== index);
        onSettingsChange({
            ...settings,
            slash_commands: newCommands
        });
    };

    const resetToDefaults = () => {
        onSettingsChange({
            ...settings,
            slash_commands: DEFAULT_SLASH_COMMANDS
        });
    };

    return (
        <div className="space-y-8 pb-24">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                    <div className="w-10 h-10 rounded-xl bg-blue-500/10 flex items-center justify-center border border-blue-500/20">
                        <Terminal className="text-blue-400" size={20} />
                    </div>
                    <div>
                        <h2 className="text-sm font-bold text-zinc-100 flex items-center gap-2">Быстрые команды</h2>
                        <p className="text-[11px] text-zinc-500">Настройка команд, вызываемых через "/" в чате</p>
                    </div>
                </div>
                <button
                    onClick={addCommand}
                    className="text-[11px] text-blue-400 hover:text-blue-300 transition-all font-bold px-3 py-1 bg-blue-400/5 rounded-lg border border-blue-400/20"
                >
                    + Добавить команду
                </button>
            </div>

            <div className="space-y-2">
                {slashCommands.map((cmd, index) => (
                    <div
                        key={cmd.id}
                        className={`group border rounded-xl transition-all duration-200 overflow-hidden ${expandedIds.has(cmd.id)
                            ? 'bg-white/[0.03] border-white/[0.08] shadow-xl'
                            : 'bg-white/[0.01] border-white/[0.03] hover:border-white/[0.08] hover:bg-white/[0.02]'
                            }`}
                    >
                        {/* Summary / Header of card */}
                        <div
                            className="px-4 py-3 flex items-center gap-4 cursor-pointer select-none"
                            onClick={() => toggleExpand(cmd.id)}
                        >
                            <div className={`w-8 h-8 rounded-lg flex items-center justify-center transition-colors ${cmd.is_enabled ? 'bg-blue-600/20 text-blue-400' : 'bg-zinc-800 text-zinc-600'}`}>
                                <Command size={16} />
                            </div>

                            <div className="flex-1 min-w-0">
                                <div className="flex items-center gap-2">
                                    <span className={`text-[13px] font-bold ${cmd.is_enabled ? 'text-zinc-100' : 'text-zinc-500'}`}>
                                        /{cmd.command}
                                    </span>
                                    {cmd.is_system && (
                                        <span className="text-[9px] px-1.5 py-0.5 rounded bg-zinc-800 text-zinc-500 border border-zinc-700/50 uppercase font-mono">
                                            System
                                        </span>
                                    )}
                                    {!cmd.is_enabled && (
                                        <span className="text-[9px] px-1.5 py-0.5 rounded bg-red-900/20 text-red-400 border border-red-900/30 uppercase">
                                            Disabled
                                        </span>
                                    )}
                                </div>
                                <div className="text-[11px] text-zinc-500 truncate">{cmd.name} — {cmd.description}</div>
                            </div>

                            <div className="flex items-center gap-2 shrink-0" onClick={e => e.stopPropagation()}>
                                <div
                                    onClick={() => updateCommand(index, { is_enabled: !cmd.is_enabled })}
                                    className={`w-8 h-4 rounded-full relative transition-colors duration-200 cursor-pointer ${cmd.is_enabled ? 'bg-blue-600' : 'bg-[#71717a]'}`}
                                >
                                    <div className={`absolute top-0.5 w-3 h-3 bg-white rounded-full transition-all duration-200 shadow-sm ${cmd.is_enabled ? 'left-[15px]' : 'left-0.5'}`} />
                                </div>
                                {!cmd.is_system && (
                                    <button
                                        onClick={() => removeCommand(index)}
                                        className="p-1.5 text-zinc-500 hover:text-red-400 hover:bg-red-400/10 rounded-lg transition-all"
                                    >
                                        <Trash2 size={14} />
                                    </button>
                                )}
                                {expandedIds.has(cmd.id) ? <ChevronUp size={16} className="text-zinc-600" /> : <ChevronDown size={16} className="text-zinc-600" />}
                            </div>
                        </div>

                        {/* Expanded Content */}
                        {expandedIds.has(cmd.id) && (
                            <div className="px-5 pb-5 pt-1 space-y-4 animate-in fade-in slide-in-from-top-2 duration-300">
                                <div className="grid grid-cols-2 gap-4">
                                    <div className="space-y-1.5">
                                        <label className="text-[11px] font-medium text-zinc-500 uppercase tracking-wider px-1 flex items-center gap-1.5">
                                            <TextCursorInput size={12} />
                                            Команда (без /)
                                        </label>
                                        <input
                                            type="text"
                                            value={cmd.command}
                                            onChange={e => updateCommand(index, { command: e.target.value })}
                                            className="w-full bg-[#1e1e21] border border-white/[0.05] rounded-lg px-3 py-2 text-sm text-zinc-200 focus:outline-none focus:border-blue-500/50 transition-colors"
                                            placeholder="например: исправить"
                                        />
                                    </div>
                                    <div className="space-y-1.5">
                                        <label className="text-[11px] font-medium text-zinc-500 uppercase tracking-wider px-1 flex items-center gap-1.5">
                                            <Edit2 size={12} />
                                            Понятное название
                                        </label>
                                        <input
                                            type="text"
                                            value={cmd.name}
                                            onChange={e => updateCommand(index, { name: e.target.value })}
                                            className="w-full bg-[#1e1e21] border border-white/[0.05] rounded-lg px-3 py-2 text-sm text-zinc-200 focus:outline-none focus:border-blue-500/50 transition-colors"
                                        />
                                    </div>
                                </div>

                                <div className="space-y-1.5">
                                    <label className="text-[11px] font-medium text-zinc-500 uppercase tracking-wider px-1">Краткое описание</label>
                                    <input
                                        type="text"
                                        value={cmd.description}
                                        onChange={e => updateCommand(index, { description: e.target.value })}
                                        className="w-full bg-[#1e1e21] border border-white/[0.05] rounded-lg px-3 py-2 text-sm text-zinc-200 focus:outline-none focus:border-blue-500/50 transition-colors"
                                    />
                                </div>

                                <div className="space-y-2">
                                    <div className="flex items-center justify-between px-1">
                                        <label className="text-[11px] font-medium text-zinc-500 uppercase tracking-wider">Шаблон промпта</label>
                                    </div>
                                    <textarea
                                        value={cmd.template}
                                        onChange={e => updateCommand(index, { template: e.target.value })}
                                        className="w-full h-32 bg-[#1e1e21] border border-white/[0.05] rounded-lg px-3 py-2 text-sm text-zinc-200 focus:outline-none focus:border-blue-500/50 transition-colors resize-none font-mono leading-relaxed shadow-inner"
                                        placeholder="Введите текст промпта с использованием плейсхолдеров..."
                                    />
                                    <div className="flex flex-wrap gap-x-4 gap-y-2 px-1 pt-1">
                                        <div className="flex items-center gap-2">
                                            <span className="text-[10px] text-zinc-600">Код:</span>
                                            <TokenCode
                                                code="{code}"
                                                onSelect={(c) => updateCommand(index, { template: cmd.template + c })}
                                            />
                                        </div>
                                        <div className="flex items-center gap-2">
                                            <span className="text-[10px] text-zinc-600">Ошибки:</span>
                                            <TokenCode
                                                code="{diagnostics}"
                                                colorClass="text-red-400/80 bg-red-400/5"
                                                onSelect={(c) => updateCommand(index, { template: cmd.template + c })}
                                            />
                                        </div>
                                        <div className="flex items-center gap-2">
                                            <span className="text-[10px] text-zinc-600">Вопрос:</span>
                                            <TokenCode
                                                code="{query}"
                                                colorClass="text-green-400/80 bg-green-400/5"
                                                onSelect={(c) => updateCommand(index, { template: cmd.template + c })}
                                            />
                                        </div>
                                    </div>
                                    <div className="px-1 pt-1 space-y-1">
                                        <p className="text-[10px] text-zinc-500 leading-tight">
                                            <span className="text-zinc-400 font-medium">{`{code}`}</span> — выделенный фрагмент или весь код модуля.
                                        </p>
                                        <p className="text-[10px] text-zinc-500 leading-tight">
                                            <span className="text-red-400/70 font-medium">{`{diagnostics}`}</span> — список ошибок из BSL LS (для исправлений).
                                        </p>
                                        <p className="text-[10px] text-zinc-500 leading-tight">
                                            <span className="text-green-400/70 font-medium">{`{query}`}</span> — текст, введенный после команды (напр. <span className="italic">/итс вопрос</span>).
                                        </p>
                                    </div>
                                </div>
                            </div>
                        )}
                    </div>
                ))}
            </div>

        </div>
    );
}
