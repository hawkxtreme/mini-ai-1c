import { useRef, useEffect, useState, useMemo } from 'react';
import { useChat, ToolCall } from '../../contexts/ChatContext';
import { useProfiles } from '../../contexts/ProfileContext';
import { useConfigurator } from '../../contexts/ConfiguratorContext';
import { parseConfiguratorTitle } from '../../utils/configurator';
import { MarkdownRenderer } from '../MarkdownRenderer';
import { Loader2, Square, ArrowUp, Settings, ChevronDown, Monitor, RefreshCw, FileText, MousePointerClick, Brain, Check, X, Terminal, Pencil, Play, Send } from 'lucide-react';
import logo from '../../assets/logo.png';
import ToolCallBlock from './ToolCallBlock';
import { MessageActions } from './MessageActions';
import { applyDiff, hasDiffBlocks, extractDisplayCode, stripCodeBlocks, parseDiffBlocks } from '../../utils/diffViewer';
import { FileDiff, Plus, Minus, Edit2, PanelRight } from 'lucide-react';

interface ChatAreaProps {
    modifiedCode: string;
    onApplyCode: (code: string) => void;
    onCodeLoaded: (code: string, isSelection: boolean) => void;
    diagnostics: any[];
    onOpenSettings: (tab: string) => void;
    onActiveDiffChange?: (content: string) => void;
}

function DiffSummaryBanner({ content, onApply }: { content: string, onApply?: () => void }) {
    const blocks = useMemo(() => parseDiffBlocks(content), [content]);
    const stats = useMemo(() => {
        let added = 0;
        let removed = 0;
        let modified = 0;

        blocks.forEach(b => {
            if (b.stats) {
                added += b.stats.added;
                removed += b.stats.removed;
                modified += b.stats.modified;
            }
        });

        return { added, removed, modified };
    }, [blocks]);

    return (
        <div className="flex items-center justify-between bg-blue-500/10 border border-blue-500/20 rounded-xl px-4 py-3 text-sm mt-4">
            <div className="flex items-center gap-4">
                <div className="flex items-center gap-2 text-zinc-300 font-medium">
                    <FileDiff className="w-5 h-5 text-blue-400" />
                    <span>Применено блоков: {blocks.length}</span>
                </div>
                <div className="flex items-center gap-3 text-xs font-mono bg-zinc-900/50 py-1.5 px-3 rounded-lg border border-zinc-800">
                    <div className="flex items-center gap-1.5 text-emerald-400" title="Добавлено строк">
                        <Plus className="w-3.5 h-3.5" />
                        <span>{stats.added}</span>
                    </div>
                    <div className="flex items-center gap-1.5 text-blue-400" title="Изменено строк">
                        <Edit2 className="w-3.5 h-3.5" />
                        <span>{stats.modified}</span>
                    </div>
                    <div className="flex items-center gap-1.5 text-red-500" title="Удалено строк">
                        <Minus className="w-3.5 h-3.5" />
                        <span>{stats.removed}</span>
                    </div>
                </div>
            </div>
            {onApply && (
                <button
                    onClick={onApply}
                    className="flex items-center gap-2 px-3 py-1.5 bg-blue-600 hover:bg-blue-500 text-white rounded-lg transition-all text-xs font-semibold shadow-lg shadow-blue-900/20"
                >
                    <PanelRight className="w-4 h-4" />
                    В редактор
                </button>
            )}
        </div>
    );
}

export function ChatArea({ modifiedCode, onApplyCode, onCodeLoaded, diagnostics, onOpenSettings, onActiveDiffChange }: ChatAreaProps) {
    const { messages, isLoading, chatStatus, currentIteration, sendMessage, stopChat, editAndRerun } = useChat();
    const { profiles, activeProfileId, setActiveProfile } = useProfiles();
    const { detectedWindows, selectedHwnd, refreshWindows, selectWindow, activeConfigTitle, getCode } = useConfigurator();

    const [appliedDiffMessages, setAppliedDiffMessages] = useState<Set<string>>(new Set());
    const [input, setInput] = useState('');
    const [showModelDropdown, setShowModelDropdown] = useState(false);
    const [showConfigDropdown, setShowConfigDropdown] = useState(false);
    const [showGetCodeDropdown, setShowGetCodeDropdown] = useState(false);
    const [expandedThinking, setExpandedThinking] = useState<Record<number, boolean>>({});
    const [contextCode, setContextCode] = useState('');
    const [isContextSelection, setIsContextSelection] = useState(false);
    const [editingIndex, setEditingIndex] = useState<number | null>(null);
    const [editText, setEditText] = useState('');

    const messagesEndRef = useRef<HTMLDivElement>(null);
    const inputRef = useRef<HTMLTextAreaElement>(null);
    const dropdownRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
                setShowModelDropdown(false);
                setShowConfigDropdown(false);
                setShowGetCodeDropdown(false);
            }
        };
        document.addEventListener('mousedown', handleClickOutside);
        return () => document.removeEventListener('mousedown', handleClickOutside);
    }, []);

    useEffect(() => {
        messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });

        // Автоматически применяем дифф-блоки последнего ответа ассистента
        if (!isLoading && messages.length > 0) {
            const lastMsg = messages[messages.length - 1];
            if (lastMsg.role === 'assistant' && hasDiffBlocks(lastMsg.content)) {
                if (onActiveDiffChange) {
                    onActiveDiffChange(lastMsg.content);
                }

                const msgKey = lastMsg.id || String(messages.length - 1);
                if (!appliedDiffMessages.has(msgKey)) {
                    console.log("[ChatArea] Auto-applying diffs for message", msgKey);
                    const newCode = applyDiff(contextCode || modifiedCode, lastMsg.content);
                    if (newCode !== (contextCode || modifiedCode)) {
                        onApplyCode(newCode);
                    }
                    setAppliedDiffMessages(prev => new Set(prev).add(msgKey));
                }
            }
        }
    }, [messages, isLoading, onActiveDiffChange, contextCode, modifiedCode, onApplyCode, appliedDiffMessages]);

    const handleSendMessage = () => {
        if (!input.trim() || isLoading) return;
        const diagStrings = diagnostics.map(d => `- Line ${d.line + 1}: ${d.message} (${d.severity})`);
        sendMessage(input, contextCode || modifiedCode, diagStrings);
        setInput('');
        // Clear context after sending
        setContextCode('');
        setIsContextSelection(false);
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            handleSendMessage();
        }
    };

    const toggleThinking = (index: number) => {
        setExpandedThinking(prev => ({ ...prev, [index]: !prev[index] }));
    };

    const handleLoadCode = async (isSelection: boolean) => {
        const code = await getCode(isSelection);
        setContextCode(code);
        setIsContextSelection(isSelection);
        onCodeLoaded(code, isSelection);
        if (onActiveDiffChange) {
            onActiveDiffChange('');
        }
        setShowGetCodeDropdown(false);
    };

    const handleRemoveCodeContext = () => {
        setContextCode('');
        setIsContextSelection(false);
    };

    const handleStartEdit = (index: number, content: string) => {
        setEditingIndex(index);
        setEditText(content);
    };

    const handleCancelEdit = () => {
        setEditingIndex(null);
        setEditText('');
    };

    const handleSaveEdit = (index: number) => {
        if (editText.trim()) {
            const diagStrings = diagnostics.map(d => `- Line ${d.line + 1}: ${d.message} (${d.severity})`);
            editAndRerun(index, editText, contextCode || modifiedCode, diagStrings);
            setEditingIndex(null);
            setEditText('');
        }
    };

    const handleEditKeyDown = (e: React.KeyboardEvent, index: number) => {
        if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
            handleSaveEdit(index);
        } else if (e.key === 'Escape') {
            handleCancelEdit();
        }
    };

    return (
        <div id="chat-area" className="flex flex-col flex-1 min-w-[400px] transition-all duration-300">
            {/* Messages List */}
            <div className={`flex-1 ${messages.length === 0 ? 'overflow-hidden' : 'overflow-y-auto scrollbar-thin scrollbar-thumb-white/10'} bg-[#09090b]`}>
                {messages.length === 0 && (
                    <div className="flex-1 flex flex-col items-center justify-center p-4 max-w-3xl mx-auto w-full h-full">
                        <div className="relative mb-10 group">
                            <div className="absolute inset-0 bg-blue-500/20 blur-3xl rounded-full group-hover:bg-blue-500/30 transition-all duration-700 animate-pulse"></div>
                            <div className="relative bg-zinc-900 p-6 rounded-3xl border border-zinc-800 shadow-2xl transform group-hover:scale-105 transition-transform duration-500">
                                <img src={logo} alt="Mini AI 1C" className="w-16 h-16 grayscale opacity-80" />
                            </div>
                        </div>

                        <div className="text-center space-y-3 mb-12">
                            <h2 className="text-3xl font-bold text-white tracking-tight">Mini AI 1C Assistant</h2>
                            <p className="text-zinc-500 text-lg max-w-md mx-auto">Интеллектуальный помощник для разработчиков 1С:Предприятие</p>
                        </div>

                        <div className="grid grid-cols-1 md:grid-cols-2 gap-4 w-full">
                            {[
                                {
                                    title: "Анализ кода",
                                    desc: "Получить код модуля или выделенный фрагмент из Конфигуратора для разбора.",
                                    icon: <FileText className="w-5 h-5 text-blue-400" />,
                                    onClick: () => handleLoadCode(true)
                                },
                                {
                                    title: "Генерация кода",
                                    desc: "Опишите задачу, и AI предложит решение в формате BSL с возможностью вставки.",
                                    icon: <RefreshCw className="w-5 h-5 text-purple-400" />,
                                    onClick: () => {
                                        setInput("Напиши процедуру для...");
                                        inputRef.current?.focus();
                                    }
                                },
                                {
                                    title: "Проверка BSL LS",
                                    desc: "Интеграция с BSL Language Server для поиска ошибок и предупреждений.",
                                    icon: <Monitor className="w-5 h-5 text-green-400" />,
                                    onClick: () => onOpenSettings('bsl')
                                },
                                {
                                    title: "Серверы MCP",
                                    desc: "Предустановленные инструменты: 1C:Метаданные, 1C:Напарник.",
                                    icon: <Settings className="w-5 h-5 text-orange-400" />,
                                    onClick: () => onOpenSettings('mcp')
                                }
                            ].map((step, i) => (
                                <div
                                    key={i}
                                    onClick={step.onClick}
                                    className="p-5 rounded-2xl bg-zinc-900/50 border border-zinc-800/50 hover:border-blue-500/30 transition-all hover:bg-zinc-800/80 group cursor-pointer active:scale-[0.98]"
                                >
                                    <div className="flex items-start gap-4">
                                        <div className="p-2.5 rounded-xl bg-zinc-800/50 group-hover:bg-blue-500/10 transition-colors">{step.icon}</div>
                                        <div className="space-y-1">
                                            <h3 className="text-sm font-semibold text-zinc-200 group-hover:text-blue-400 transition-colors">{step.title}</h3>
                                            <p className="text-xs text-zinc-500 leading-relaxed group-hover:text-zinc-400 transition-colors">{step.desc}</p>
                                        </div>
                                    </div>
                                </div>
                            ))}
                        </div>

                        <div className="mt-12 flex flex-col items-center gap-6 pb-2">
                            <a href="https://t.me/hawkxtreme" target="_blank" rel="noopener noreferrer" className="group flex items-center gap-3 px-4 py-2 rounded-2xl bg-zinc-900/10 border border-zinc-800/30 hover:bg-zinc-800/30 hover:border-zinc-700/50 transition-all duration-300">
                                <div className="p-1.5 rounded-lg bg-blue-500/10 group-hover:bg-blue-500/20 transition-colors">
                                    <Send className="w-3.5 h-3.5 text-blue-400" />
                                </div>
                                <div className="flex flex-col items-start leading-tight">
                                    <span className="text-[10px] text-zinc-600 uppercase tracking-wider font-semibold">Feedback & Support</span>
                                    <span className="text-xs text-zinc-400 group-hover:text-blue-400 transition-colors">@hawkxtreme</span>
                                </div>
                            </a>
                        </div>
                    </div>
                )}

                <div className={`flex flex-col pb-4 gap-4 px-4 w-full pt-4`}>
                    {messages.map((msg, i) => (
                        <div key={msg.id || i} className={`flex w-full ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}>
                            <div className={`p-4 rounded-xl border text-[13px] leading-relaxed group ${msg.role === 'user' ? 'bg-[#1b1b1f] border-zinc-800/80 text-zinc-300 max-w-[90%]' : 'bg-zinc-900/40 border-zinc-800/50 text-zinc-300 shadow-sm w-full max-w-full'}`}>
                                <div className="min-w-0 flex flex-col gap-3">
                                    {/* Message Header with Actions */}
                                    <div className="flex items-start justify-between gap-2">
                                        <div className="flex-1 min-w-0">
                                            {/* Thinking Section */}
                                            {msg.thinking && (
                                                <div className="border-l-2 border-white/10 pl-3">
                                                    <button
                                                        onClick={() => toggleThinking(i)}
                                                        className="flex items-center gap-2 text-[11px] text-white/40 hover:text-white/60 uppercase tracking-tighter mb-1 transition-colors group"
                                                    >
                                                        <Brain size={12} className={expandedThinking[i] ? 'text-blue-400' : ''} />
                                                        Думаю
                                                        <ChevronDown size={12} className={`transition-transform ${expandedThinking[i] ? 'rotate-180' : ''}`} />
                                                    </button>
                                                    {expandedThinking[i] && (
                                                        <div className="text-[12px] italic text-white/40 leading-snug animate-in fade-in slide-in-from-top-1">
                                                            {msg.thinking}
                                                        </div>
                                                    )}
                                                </div>
                                            )}
                                        </div>
                                        {/* Actions */}
                                        <MessageActions
                                            content={msg.content}
                                            timestamp={msg.timestamp}
                                            isUser={msg.role === 'user'}
                                            onEdit={msg.role === 'user' ? () => handleStartEdit(i, msg.content) : undefined}
                                        />
                                    </div>

                                    {/* Tool Calls */}
                                    {msg.toolCalls && msg.toolCalls.length > 0 && (
                                        <div className="flex flex-col gap-1">
                                            {msg.toolCalls.map((tc, idx) => (
                                                <ToolCallBlock key={idx} toolCall={tc} />
                                            ))}
                                        </div>
                                    )}

                                    {/* Content */}
                                    <div className="min-w-0">
                                        {msg.role === 'assistant' ? (
                                            <>
                                                <MarkdownRenderer
                                                    content={msg.content}
                                                    isStreaming={isLoading && i === messages.length - 1}
                                                    onApplyCode={onApplyCode}
                                                    originalCode={contextCode || modifiedCode}
                                                />
                                                {hasDiffBlocks(msg.content) && (
                                                    <DiffSummaryBanner
                                                        content={msg.content}
                                                        onApply={() => {
                                                            const newCode = applyDiff(contextCode || modifiedCode, msg.content);
                                                            onApplyCode(newCode);
                                                            if (onActiveDiffChange) onActiveDiffChange(msg.content);
                                                        }}
                                                    />
                                                )}
                                            </>
                                        ) : editingIndex === i ? (

                                            <div className="w-full">
                                                <textarea
                                                    value={editText}
                                                    onChange={(e) => setEditText(e.target.value)}
                                                    onKeyDown={(e) => handleEditKeyDown(e, i)}
                                                    className="w-full bg-zinc-800 border border-zinc-700 rounded-lg p-3 text-zinc-300 text-[13px] font-sans resize-none focus:outline-none focus:border-blue-500/50 transition-colors"
                                                    rows={Math.min(10, Math.max(3, editText.split('\n').length))}
                                                    autoFocus
                                                />
                                                <div className="flex justify-end gap-2 mt-2">
                                                    <button
                                                        onClick={handleCancelEdit}
                                                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[11px] font-medium bg-zinc-800 text-zinc-400 hover:text-white hover:bg-zinc-700 transition-all"
                                                    >
                                                        <X size={14} />
                                                        Отмена
                                                    </button>
                                                    <button
                                                        onClick={() => handleSaveEdit(i)}
                                                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[11px] font-medium bg-blue-600 text-white hover:bg-blue-500 transition-all"
                                                    >
                                                        <Play size={14} />
                                                        Сохранить и перезапустить
                                                    </button>
                                                </div>
                                            </div>
                                        ) : (
                                            <pre className="whitespace-pre-wrap font-sans break-words break-all overflow-hidden" style={{ fontFamily: 'Inter, sans-serif', overflowWrap: 'anywhere' }}>{msg.content}</pre>
                                        )}
                                    </div>

                                    {/* BSL Diagnostics */}
                                    {msg.diagnostics && msg.diagnostics.length > 0 && (
                                        <div className="mt-2 p-2 rounded bg-red-500/10 border border-red-500/20">
                                            <div className="flex items-center gap-2 text-[11px] font-bold text-red-400 uppercase mb-1">
                                                <Terminal size={12} /> Ошибки проверки кода
                                            </div>
                                            {msg.diagnostics.map((d, di) => (
                                                <div key={di} className="text-[11px] text-red-300/80 font-mono">
                                                    Строка {d.line + 1}: {d.message}
                                                </div>
                                            ))}
                                        </div>
                                    )}
                                </div>
                            </div>
                        </div>
                    ))}
                    {isLoading && (
                        <div className="w-full px-0">
                            <div className="p-4 rounded-xl border border-zinc-800/50 bg-transparent flex items-center gap-3">
                                <Loader2 className="w-4 h-4 animate-spin text-blue-400" />
                                <div className="flex items-center gap-2">
                                    <span className="text-zinc-300 text-xs font-medium">{chatStatus || 'Думаю...'}</span>
                                    {currentIteration > 1 && (
                                        <span className="text-[10px] bg-zinc-800 text-zinc-500 px-1.5 py-0.5 rounded-full border border-zinc-700 font-mono">
                                            Шаг {currentIteration}
                                        </span>
                                    )}
                                </div>
                            </div>
                        </div>
                    )}
                </div>
                <div ref={messagesEndRef} />
            </div>

            {/* Input Area */}
            <div className="px-6 pb-6 pt-4 bg-[#09090b] border-t border-[#27272a] shadow-2xl z-10">
                {/* Context Stats Overlay */}
                <div className="max-w-4xl mx-auto mb-3 flex items-center justify-between px-1">
                    {messages.length === 0 ? (
                        <div className="flex items-center gap-2 text-[11px] text-zinc-600 italic transition-all duration-500">
                            <ChevronDown className="w-3.5 h-3.5 animate-bounce" />
                            <span>{selectedHwnd ? 'Окно выбрано' : 'Выберите окно Конфигуратора снизу'}</span>
                        </div>
                    ) : (
                        <div className="flex items-center gap-3">
                            {/* Actions removed */}
                        </div>
                    )}

                    <div className="flex items-center gap-2 text-[10px] text-zinc-600 font-mono">
                        {modifiedCode.length > 0 && `CONTEXT: ${modifiedCode.length} chars`}
                    </div>
                </div>
                <div className="relative bg-[#18181b] border border-[#27272a] rounded-xl focus-within:ring-1 focus-within:ring-blue-500/50 transition-all min-h-[120px] flex flex-col max-w-4xl mx-auto">

                    <textarea
                        ref={inputRef}
                        value={input}
                        onChange={(e) => setInput(e.target.value)}
                        onKeyDown={handleKeyDown}
                        placeholder="Опишите задачу или вставьте код..."
                        className="w-full h-full bg-transparent text-zinc-300 px-4 py-3 resize-none focus:outline-none placeholder-zinc-600 text-[13px] font-sans leading-relaxed flex-1"
                        style={{ fontFamily: 'Inter, sans-serif' }}
                    />

                    <div ref={dropdownRef} className="px-3 pb-2 pt-0 flex items-end gap-2 pointer-events-auto flex-wrap w-full">
                        <div className="flex items-center gap-1.5 flex-1 min-w-0">
                            {/* Model Selector */}
                            <div className="relative flex-shrink-1 min-w-0 max-w-[140px]">
                                <button
                                    onClick={() => {
                                        const next = !showModelDropdown;
                                        setShowModelDropdown(next);
                                        if (next) {
                                            setShowConfigDropdown(false);
                                            setShowGetCodeDropdown(false);
                                        }
                                    }}
                                    className={`w-full flex items-center gap-1.5 text-[12px] font-medium px-2 py-1.5 rounded-md transition-all border border-transparent ${showModelDropdown ? 'bg-zinc-800 text-zinc-200 border-zinc-700' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
                                >
                                    <ChevronDown className={`w-3 h-3 text-zinc-500 flex-shrink-0 transition-transform ${showModelDropdown ? 'rotate-180' : ''}`} />
                                    <span className="truncate block">
                                        {profiles.find(p => p.id === activeProfileId)?.name || 'Agent'}
                                    </span>
                                </button>
                                {showModelDropdown && (
                                    <div className="absolute bottom-full left-0 mb-2 w-56 bg-[#1f1f23] border border-[#27272a] rounded-lg shadow-2xl z-30 ring-1 ring-black/20 p-1">
                                        {profiles.map(p => (
                                            <button key={p.id} onClick={() => { setActiveProfile(p.id); setShowModelDropdown(false); }} className={`w-full text-left px-3 py-2 rounded-md text-[13px] flex items-center justify-between ${p.id === activeProfileId ? 'bg-blue-500/10 text-blue-400' : 'text-zinc-400 hover:bg-[#27272a]'}`}>
                                                {p.name}
                                            </button>
                                        ))}
                                    </div>
                                )}
                            </div>

                            {/* Configurator Selector */}
                            <div className="relative flex-shrink-0" id="configurator-selector">
                                <button onClick={() => {
                                    const next = !showConfigDropdown;
                                    setShowConfigDropdown(next);
                                    if (next) {
                                        setShowModelDropdown(false);
                                        setShowGetCodeDropdown(false);
                                        refreshWindows();
                                    }
                                }}
                                    className={`flex-shrink-0 flex items-center gap-1.5 text-[12px] font-medium px-2.5 py-1.5 rounded-md transition-all border border-transparent ${showConfigDropdown ? 'bg-zinc-800 text-zinc-200 border-zinc-700' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
                                    title={activeConfigTitle}
                                >
                                    <Monitor className="w-3.5 h-3.5 flex-shrink-0" />
                                    <span className="sm:inline max-w-[120px] truncate block">{activeConfigTitle}</span>
                                </button>
                                {showConfigDropdown && (
                                    <div className="absolute bottom-full left-0 mb-2 w-64 bg-[#1f1f23] border border-[#27272a] rounded-lg shadow-2xl z-30 ring-1 ring-black/20 p-1">
                                        {detectedWindows.map(w => (
                                            <button key={w.hwnd} onClick={() => { selectWindow(w.hwnd); setShowConfigDropdown(false); }}
                                                className={`w-full text-left px-3 py-2 rounded-md text-[13px] truncate ${selectedHwnd === w.hwnd ? 'bg-blue-500/10 text-blue-400' : 'text-zinc-400 hover:bg-[#27272a]'}`}
                                                title={w.title}
                                            >
                                                {parseConfiguratorTitle(w.title)}
                                            </button>
                                        ))}
                                    </div>
                                )}
                            </div>

                            {/* Get Code Button */}
                            <div className="relative flex-shrink-0" id="tour-get-code">
                                <button onClick={() => {
                                    const next = !showGetCodeDropdown;
                                    setShowGetCodeDropdown(next);
                                    if (next) {
                                        setShowModelDropdown(false);
                                        setShowConfigDropdown(false);
                                    }
                                }}
                                    className={`flex-shrink-0 flex items-center gap-1.5 text-[12px] font-medium px-2.5 py-1.5 rounded-md transition-all border border-transparent ${showGetCodeDropdown ? 'bg-zinc-800 text-zinc-200 border-zinc-700' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
                                >
                                    <FileText className="w-3.5 h-3.5" />
                                    <span className="hidden sm:inline">Получить код</span>
                                </button>
                                {showGetCodeDropdown && (
                                    <div className="absolute bottom-full right-0 mb-2 w-max min-w-[180px] bg-[#1f1f23] border border-[#27272a] rounded-lg shadow-2xl z-30 ring-1 ring-black/20 p-1 flex flex-col">
                                        <button onClick={() => handleLoadCode(true)} className="flex items-center gap-2 px-3 py-2 text-[13px] text-zinc-400 hover:text-white hover:bg-[#27272a] transition-colors text-left rounded-md whitespace-nowrap">
                                            <FileText className="w-3.5 h-3.5 flex-shrink-0" />
                                            <span>Модуль целиком</span>
                                        </button>
                                        <button onClick={() => handleLoadCode(false)} className="flex items-center gap-2 px-3 py-2 text-[13px] text-zinc-400 hover:text-white hover:bg-[#27272a] transition-colors text-left rounded-md whitespace-nowrap">
                                            <MousePointerClick className="w-3.5 h-3.5 flex-shrink-0" />
                                            <span>Выделенный фрагмент</span>
                                        </button>
                                    </div>
                                )}
                            </div>
                        </div>

                        <button onClick={isLoading ? stopChat : handleSendMessage} disabled={!isLoading && !input.trim()} className={`p-2 rounded-lg transition-colors flex-shrink-0 ${isLoading ? 'bg-red-500/10 text-red-400' : input.trim() ? 'bg-blue-600 text-white' : 'bg-[#27272a] text-zinc-600'}`}>
                            {isLoading ? <Square className="w-4 h-4 fill-current" /> : <ArrowUp className="w-4 h-4" strokeWidth={2.5} />}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
}
