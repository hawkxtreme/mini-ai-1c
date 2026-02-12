import { useRef, useEffect, useState } from 'react';
import { useChat } from '../../contexts/ChatContext';
import { useProfiles } from '../../contexts/ProfileContext';
import { useConfigurator } from '../../contexts/ConfiguratorContext';
import { MarkdownRenderer } from '../MarkdownRenderer';
import { Loader2, Square, ArrowUp, Settings, ChevronDown, Monitor, RefreshCw, FileText, MousePointerClick } from 'lucide-react';
import logo from '../../assets/logo.png';

interface ChatAreaProps {
    modifiedCode: string;
    onApplyCode: (code: string) => void;
    onCodeLoaded: (code: string, isSelection: boolean) => void;
    diagnostics: any[];
}

export function ChatArea({ modifiedCode, onApplyCode, onCodeLoaded, diagnostics }: ChatAreaProps) {
    const { messages, isLoading, chatStatus, sendMessage, stopChat } = useChat();
    const { profiles, activeProfileId, setActiveProfile } = useProfiles();
    const { detectedWindows, selectedHwnd, refreshWindows, selectWindow, getActiveConfiguratorTitle, getCode } = useConfigurator();

    const [input, setInput] = useState('');
    const [showModelDropdown, setShowModelDropdown] = useState(false);
    const [showConfigDropdown, setShowConfigDropdown] = useState(false); // Duplicated from Header logic? Ideally Config/Code logic should be in one place (e.g. Header or ChatInput)
    // Refactoring note: Original App.tsx had Config/Code dropdowns in the Input Area. I'll keep them there.

    const [showGetCodeDropdown, setShowGetCodeDropdown] = useState(false);

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
    }, [messages]);

    const handleSendMessage = () => {
        if (!input.trim() || isLoading) return;

        // Pass code context and diagnostics if available (basic simple strings for now)
        // Diagnostics type mapping might be needed if complex
        const diagStrings = diagnostics.map(d => `- Line ${d.line + 1}: ${d.message} (${d.severity})`);

        sendMessage(input, modifiedCode, diagStrings);
        setInput('');
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            handleSendMessage();
        }
    };

    return (
        <div className="flex flex-col flex-1 min-w-[400px] transition-all duration-300">
            {/* Messages List */}
            <div className="flex-1 overflow-y-auto bg-[#09090b]">
                {messages.length === 0 && (
                    <div className="flex-1 flex flex-col items-center justify-center p-8 max-w-3xl mx-auto w-full h-full">
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
                                    desc: "Получите код модуля или выделенный фрагмент из Конфигуратора для разбора.",
                                    icon: <FileText className="w-5 h-5 text-blue-400" />,
                                },
                                {
                                    title: "Генерация кода",
                                    desc: "Опишите задачу, и AI предложит решение в формате BSL с возможностью вставки.",
                                    icon: <RefreshCw className="w-5 h-5 text-purple-400" />,
                                },
                                {
                                    title: "Проверка BSL LS",
                                    desc: "Интеграция с BSL Language Server для поиска ошибок и предупреждений.",
                                    icon: <Monitor className="w-5 h-5 text-green-400" />,
                                },
                                {
                                    title: "Профили LLM",
                                    desc: "Настройте DeepSeek, OpenAI или локальные LLM через Ollama.",
                                    icon: <Settings className="w-5 h-5 text-orange-400" />,
                                }
                            ].map((step, i) => (
                                <div key={i} className="p-5 rounded-2xl bg-zinc-900/50 border border-zinc-800/50 hover:border-zinc-700/50 transition-all hover:bg-zinc-850 group cursor-default">
                                    <div className="flex items-start gap-4">
                                        <div className="p-2.5 rounded-xl bg-zinc-800/50 group-hover:bg-zinc-800 transition-colors">
                                            {step.icon}
                                        </div>
                                        <div className="space-y-1">
                                            <h3 className="text-sm font-semibold text-zinc-200">{step.title}</h3>
                                            <p className="text-xs text-zinc-500 leading-relaxed">{step.desc}</p>
                                        </div>
                                    </div>
                                </div>
                            ))}
                        </div>

                        <div className="mt-12 flex flex-col items-center gap-6 pb-2">
                            <a
                                href="https://t.me/hawkxtreme"
                                target="_blank"
                                rel="noopener noreferrer"
                                className="group flex items-center gap-3 px-4 py-2 rounded-2xl bg-zinc-900/10 border border-zinc-800/30 hover:bg-zinc-800/30 hover:border-zinc-700/50 transition-all duration-300"
                            >
                                <div className="p-1.5 rounded-lg bg-blue-500/10 group-hover:bg-blue-500/20 transition-colors">
                                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-blue-400">
                                        <path d="m22 2-7 20-4-9-9-4Z" /><path d="M22 2 11 13" />
                                    </svg>
                                </div>
                                <div className="flex flex-col items-start leading-tight">
                                    <span className="text-[10px] text-zinc-600 uppercase tracking-wider font-semibold">Feedback & Support</span>
                                    <span className="text-xs text-zinc-400 group-hover:text-blue-400 transition-colors">@hawkxtreme</span>
                                </div>
                            </a>
                        </div>
                    </div>
                )}

                <div className={`flex flex-col pb-4 gap-2 px-4 w-full pt-4`}>
                    {messages.map((msg, i) => (
                        <div key={i} className={`flex w-full ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}>
                            <div className={`w-full max-w-full p-4 rounded-xl border text-[13px] leading-relaxed ${msg.role === 'user' ? 'bg-[#1b1b1f] border-zinc-800 text-zinc-300' : 'bg-zinc-900/40 border-zinc-800/50 text-zinc-300'}`}>
                                <div className="min-w-0">
                                    {msg.role === 'assistant' ? (
                                        <MarkdownRenderer content={msg.content} isStreaming={isLoading && i === messages.length - 1} onApplyCode={onApplyCode} />
                                    ) : (
                                        <pre className="whitespace-pre-wrap font-sans" style={{ fontFamily: 'Inter, sans-serif' }}>{msg.content}</pre>
                                    )}
                                </div>
                            </div>
                        </div>
                    ))}
                    {isLoading && (
                        <div className="w-full px-4 mb-4">
                            <div className="p-6 rounded-xl border border-[#27272a] bg-transparent flex items-center gap-3">
                                <Loader2 className="w-4 h-4 animate-spin text-blue-400" />
                                <span className="text-zinc-400 text-sm">{chatStatus || 'Thinking...'}</span>
                            </div>
                        </div>
                    )}
                </div>
                <div ref={messagesEndRef} />
            </div>

            {/* Input Area */}
            <div className="p-4 bg-[#09090b] border-t border-[#27272a] w-full">
                {messages.length === 0 && (
                    <div className="max-w-4xl mx-auto mb-3 flex justify-center">
                        <div className="flex items-center gap-2 text-[11px] text-zinc-500 opacity-80 hover:opacity-100 transition-opacity">
                            <ChevronDown className="w-3.5 h-3.5 animate-bounce" />
                            <span>Начните с подключения к окну Конфигуратора снизу</span>
                        </div>
                    </div>
                )}
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

                    <div ref={dropdownRef} className="px-3 pb-2 pt-0 flex items-end gap-2 pointer-events-auto flex-nowrap w-full">
                        <div className="flex items-center gap-1 flex-1 flex-shrink-0">
                            {/* Model Selector */}
                            <div className="relative flex-shrink-0">
                                <button
                                    onClick={() => setShowModelDropdown(!showModelDropdown)}
                                    className={`flex-shrink-0 flex items-center gap-1.5 text-[12px] font-medium px-2.5 py-1.5 rounded-md transition-all border border-transparent ${showModelDropdown ? 'bg-zinc-800 text-zinc-200 border-zinc-700' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
                                >
                                    <ChevronDown className={`w-3 h-3 text-zinc-500 transition-transform ${showModelDropdown ? 'rotate-180' : ''}`} />
                                    <span className="hidden sm:inline whitespace-nowrap">
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
                            <div className="relative flex-shrink-0">
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
                                >
                                    <Monitor className="w-3.5 h-3.5" />
                                    <span className="truncate max-w-[80px] hidden sm:inline">{getActiveConfiguratorTitle()}</span>
                                </button>
                                {showConfigDropdown && (
                                    <div className="absolute bottom-full left-0 mb-2 w-64 bg-[#1f1f23] border border-[#27272a] rounded-lg shadow-2xl z-30 ring-1 ring-black/20 p-1">
                                        {detectedWindows.map(w => (
                                            <button key={w.hwnd} onClick={() => { selectWindow(w.hwnd); setShowConfigDropdown(false); }} className={`w-full text-left px-3 py-2 rounded-md text-xs truncate ${selectedHwnd === w.hwnd ? 'bg-blue-500/10 text-blue-400' : 'text-zinc-400 hover:bg-[#27272a]'}`}>
                                                {w.title}
                                            </button>
                                        ))}
                                    </div>
                                )}
                            </div>

                            {/* Get Code Button */}
                            <div className="relative flex-shrink-0">
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
                                    <div className="absolute bottom-full left-0 mb-2 w-40 bg-[#1f1f23] border border-[#27272a] rounded-lg shadow-2xl z-30 ring-1 ring-black/20 p-1 flex flex-col">
                                        <button onClick={async () => { const code = await getCode(true); onCodeLoaded(code, false); setShowGetCodeDropdown(false); }} className="flex items-center gap-2 px-3 py-2 text-xs text-zinc-400 hover:text-white hover:bg-[#27272a] transition-colors text-left rounded-md">
                                            <FileText className="w-3.5 h-3.5" /> Модуль целиком
                                        </button>
                                        <button onClick={async () => { const code = await getCode(false); onCodeLoaded(code, true); setShowGetCodeDropdown(false); }} className="flex items-center gap-2 px-3 py-2 text-xs text-zinc-400 hover:text-white hover:bg-[#27272a] transition-colors text-left rounded-md">
                                            <MousePointerClick className="w-3.5 h-3.5" /> Выделенный фрагмент
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
