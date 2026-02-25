import { useRef, useEffect, useState, useMemo } from 'react';
import { useChat, ToolCall } from '../../contexts/ChatContext';
import { useProfiles } from '../../contexts/ProfileContext';
import { useSettings } from '../../contexts/SettingsContext';
import { useConfigurator } from '../../contexts/ConfiguratorContext';
import { parseConfiguratorTitle } from '../../utils/configurator';
import { MarkdownRenderer } from '../MarkdownRenderer';
import { Loader2, Square, ArrowUp, Settings, ChevronDown, Monitor, RefreshCw, FileText, MousePointerClick, Brain, Check, X, Terminal, Pencil, Play, Send, User, HardHat, Mic } from 'lucide-react';
import { useVoiceInput } from '../../voice/useVoiceInput';
import logo from '../../assets/logo.png';
import ToolCallBlock from './ToolCallBlock';
import { MessageActions } from './MessageActions';
import { applyDiff, hasDiffBlocks, extractDisplayCode, stripCodeBlocks, parseDiffBlocks, hasApplicableDiffBlocks } from '../../utils/diffViewer';
import { FileDiff, Plus, Minus, Edit2, PanelRight } from 'lucide-react';
import { CommandMenu } from './CommandMenu';
import { DEFAULT_SLASH_COMMANDS, SlashCommand, CliStatus } from '../../types/settings';
import { cliProvidersApi } from '../../api/cli_providers';
import { QwenAuthModal } from '../settings/QwenAuthModal';

interface ChatAreaProps {
    originalCode?: string;
    modifiedCode?: string;
    onApplyCode?: (code: string) => void;
    onCommitCode?: (code: string) => void;
    onCodeLoaded?: (code: string, isSelection: boolean) => void;
    diagnostics?: any[];
    onOpenSettings?: (tab?: string) => void;
    onActiveDiffChange?: (content: string) => void;
    activeDiffContent?: string;
}

function DiffSummaryBanner({ content, onApply, onReject, disabled }: { content: string, onApply?: () => void, onReject?: () => void, disabled?: boolean }) {
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
        <div className="flex items-center gap-3 bg-zinc-900/40 border border-zinc-800/80 rounded-lg px-2 py-1 mt-2 w-fit ml-auto shadow-sm">
            <div className="flex items-center gap-2 text-[10px] font-mono leading-none">
                <span className="text-emerald-500">+{stats.added}</span>
                <span className="text-red-500">-{stats.removed}</span>
                {stats.modified > 0 && <span className="text-blue-400">~{stats.modified}</span>}
            </div>
            <div className="w-[1px] h-3 bg-zinc-800" />
            <div className="flex items-center gap-2">
                {onApply && (
                    <button
                        onClick={disabled ? undefined : onApply}
                        disabled={disabled}
                        className={`px-2 py-0.5 rounded text-[11px] font-semibold transition-all ${disabled ? 'bg-zinc-800 text-zinc-600 cursor-not-allowed' : 'bg-zinc-700 text-zinc-200 hover:bg-zinc-600 hover:text-white active:scale-95'}`}
                    >
                        Принять
                    </button>
                )}
                {onReject && (
                    <button
                        onClick={disabled ? undefined : onReject}
                        disabled={disabled}
                        className={`px-2 py-0.5 rounded text-[11px] font-semibold transition-all border ${disabled ? 'text-zinc-600 border-transparent cursor-not-allowed' : 'text-zinc-500 hover:text-zinc-300 active:scale-95 border-transparent hover:border-zinc-800'}`}
                    >
                        Отменить
                    </button>
                )}
            </div>
            <FileDiff className="w-3.5 h-3.5 text-zinc-700" />
        </div>
    );
}

export function ChatArea({
    originalCode,
    modifiedCode,
    onApplyCode,
    onCommitCode,
    onCodeLoaded,
    diagnostics,
    onOpenSettings,
    onActiveDiffChange,
    activeDiffContent
}: ChatAreaProps) {
    const { messages, isLoading, chatStatus, currentIteration, sendMessage, stopChat, editAndRerun } = useChat();
    const { profiles, activeProfileId, setActiveProfile } = useProfiles();
    const { settings, updateSettings } = useSettings();
    const { detectedWindows, selectedHwnd, refreshWindows, selectWindow, activeConfigTitle, getCode } = useConfigurator();

    const [appliedDiffMessages, setAppliedDiffMessages] = useState<Set<string>>(new Set());
    const [dismissedDiffMessages, setDismissedDiffMessages] = useState<Set<string>>(new Set());
    const [diffActions, setDiffActions] = useState<Map<string, 'accepted' | 'rejected'>>(new Map());
    const [input, setInput] = useState('');
    const [showModelDropdown, setShowModelDropdown] = useState(false);
    const [showConfigDropdown, setShowConfigDropdown] = useState(false);
    const [isAuthModalOpen, setIsAuthModalOpen] = useState(false);
    const [cliStatuses, setCliStatuses] = useState<Record<string, CliStatus>>({});
    const [showGetCodeDropdown, setShowGetCodeDropdown] = useState(false);
    const [expandedThinking, setExpandedThinking] = useState<Record<number, boolean>>({});
    const [contextCode, setContextCode] = useState('');
    const [isContextSelection, setIsContextSelection] = useState(false);
    const [editingIndex, setEditingIndex] = useState<number | null>(null);
    const [editText, setEditText] = useState('');
    const [showVoiceHint, setShowVoiceHint] = useState(false);

    // Slash Commands state
    const [showCommands, setShowCommands] = useState(false);
    const [commandFilter, setCommandFilter] = useState('');
    const availableCommands = useMemo(() => {
        const cmds = settings?.slash_commands || DEFAULT_SLASH_COMMANDS;
        return cmds.filter(c => c.is_enabled);
    }, [settings?.slash_commands]);

    const filteredCommands = useMemo(() => {
        if (!commandFilter) return availableCommands;
        const filter = commandFilter.toLowerCase();
        return availableCommands.filter(c =>
            c.command.toLowerCase().includes(filter) ||
            c.name.toLowerCase().includes(filter)
        );
    }, [availableCommands, commandFilter]);

    const { isRecording, toggleRecording, isSupported, error: voiceError, permissionState } = useVoiceInput((text) => {
        setInput(prev => prev + (prev ? ' ' : '') + text);
    }, selectedHwnd);

    // Show hint if permission is denied or pending on first click
    useEffect(() => {
        if (voiceError === 'not-allowed') {
            setShowVoiceHint(true);
            const timer = setTimeout(() => setShowVoiceHint(false), 8000);
            return () => clearTimeout(timer);
        }
    }, [voiceError]);

    // Automatically stop recording when AI starts responding
    useEffect(() => {
        if (isLoading && isRecording) {
            toggleRecording();
        }
    }, [isLoading, isRecording, toggleRecording]);

    // CLI Statuses
    const fetchCliStatuses = async () => {
        try {
            const status = await cliProvidersApi.getStatus('qwen');
            setCliStatuses(prev => ({ ...prev, qwen: status }));
        } catch (err) {
            console.error('Failed to fetch CLI status:', err);
        }
    };

    useEffect(() => {
        fetchCliStatuses();
    }, []);

    // Обновляем лимиты когда активный профиль переключается на QwenCli
    useEffect(() => {
        const activeProfile = profiles.find(p => p.id === activeProfileId);
        if (activeProfile?.provider === 'QwenCli') {
            fetchCliStatuses();
        }
    }, [activeProfileId]);

    // Периодическое обновление лимитов Qwen каждые 60 секунд
    useEffect(() => {
        const interval = setInterval(() => {
            const activeProfile = profiles.find(p => p.id === activeProfileId);
            if (activeProfile?.provider === 'QwenCli') {
                fetchCliStatuses();
            }
        }, 60_000);
        return () => clearInterval(interval);
    }, [activeProfileId, profiles]);


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
            // Закрываем меню команд при клике вне
            if (showCommands) {
                setShowCommands(false);
            }
        };
        document.addEventListener('mousedown', handleClickOutside);
        return () => document.removeEventListener('mousedown', handleClickOutside);
    }, [showCommands]);

    const scrollRef = useRef<HTMLDivElement>(null);
    const wasAtBottom = useRef(true);

    // Обработчик скролла для отслеживания ручной прокрутки вверх
    const handleScroll = () => {
        if (scrollRef.current) {
            const { scrollTop, scrollHeight, clientHeight } = scrollRef.current;
            // Проверяем, находится ли пользователь внизу (с допуском 100px для надежности)
            const isAtBottom = scrollHeight - scrollTop <= clientHeight + 100;
            wasAtBottom.current = isAtBottom;
        }
    };

    useEffect(() => {
        // Автопрокрутка только во время стриминга или при получении нового сообщения
        if (!isLoading && messages.length > 0 && messages[messages.length - 1].role !== 'user') return;

        const scrollToBottom = (behavior: ScrollBehavior = 'smooth') => {
            if (wasAtBottom.current && scrollRef.current) {
                scrollRef.current.scrollTo({
                    top: scrollRef.current.scrollHeight,
                    behavior
                });
            }
        };

        // При стриминге используем smooth, но для первого скролла можно auto
        scrollToBottom('smooth');

        // Дополнительный скролл для компенсации динамического контента (Markdown)
        const timer = setTimeout(() => scrollToBottom('smooth'), 100);
        return () => clearTimeout(timer);
    }, [messages, isLoading]);

    // Блок ДУМАЮ по умолчанию свёрнут — пользователь разворачивает вручную.
    // (Авторасширение во время стриминга отключено)

    // Прокрутка вниз при отправке нового сообщения пользователем (всегда плавно)
    useEffect(() => {
        if (messages.length > 0 && messages[messages.length - 1].role === 'user') {
            wasAtBottom.current = true;
            scrollRef.current?.scrollTo({
                top: scrollRef.current.scrollHeight,
                behavior: 'smooth'
            });
        }
    }, [messages.length]);

    useEffect(() => {

        // Автоматически применяем дифф-блоки последнего ответа ассистента
        if (!isLoading && messages.length > 0) {
            const lastMsg = messages[messages.length - 1];
            const currentOriginal = contextCode || modifiedCode || "";

            // Проверяем: есть ли маркеры SEARCH
            console.log(`[ChatArea:diag] isLoading=${isLoading}, role=${lastMsg.role}, contextLen=${currentOriginal.length}, msgContentLen=${(lastMsg.content || '').length}`);
            console.log(`[ChatArea:diag] msgContent(100)="${(lastMsg.content || '').substring(0, 100).replace(/\n/g, '↵')}"`);
            console.log(`[ChatArea:diag] hasDiffBlocks=${lastMsg.role === 'assistant' && hasDiffBlocks(lastMsg.content)}`);
            if (lastMsg.role === 'assistant' && hasDiffBlocks(lastMsg.content)) {
                const msgKey = lastMsg.id || String(messages.length - 1);

                if (!appliedDiffMessages.has(msgKey)) {
                    // Устанавливаем активный дифф ТОЛЬКО при первой встрече этого сообщения
                    if (onActiveDiffChange) {
                        onActiveDiffChange(lastMsg.content);
                    }

                    console.log("[ChatArea] Diff markers found in message", msgKey);

                    const isApplicable = hasApplicableDiffBlocks(currentOriginal, lastMsg.content);
                    if (isApplicable) {
                        console.log("[ChatArea] Auto-applying diffs...");
                        const newCode = applyDiff(currentOriginal, lastMsg.content);
                        if (newCode !== currentOriginal) {
                            if (onApplyCode) {
                                onApplyCode(newCode);
                            }
                        }
                        setAppliedDiffMessages(prev => new Set(prev).add(msgKey));
                    } else {
                        console.warn("[ChatArea] Diff markers found but blocks are not applicable to current code. Check indentation/tabs or EOF.");
                        // Даже если не применилось автоматически, мы помечаем как "обработанное", 
                        // чтобы не спамить ворнингами при каждом рендере, 
                        // но пользователь увидит ошибку в консоли если что.
                        setAppliedDiffMessages(prev => new Set(prev).add(msgKey));
                    }
                } else {
                    // Синхронизируем activeDiffContent по мере докачки сообщения, 
                    // если этот дифф-блок является текущим активным
                    if (onActiveDiffChange && activeDiffContent && lastMsg.content.includes(activeDiffContent)) {
                        onActiveDiffChange(lastMsg.content);
                    }
                }
            }
        }
    }, [messages, isLoading, onActiveDiffChange, contextCode, modifiedCode, onApplyCode, appliedDiffMessages, activeDiffContent, onCodeLoaded]);

    const handleSendMessage = (textOverride?: string) => {
        let textToSend = textOverride || input;

        // Автоматическое расширение слеш-команд
        let displayContent: string | undefined = undefined;
        let isSlashCommand = false;

        if (!textOverride && textToSend.startsWith('/')) {
            const firstSpace = textToSend.indexOf(' ');
            const cmdPart = firstSpace === -1 ? textToSend.substring(1) : textToSend.substring(1, firstSpace);
            const queryPart = firstSpace === -1 ? '' : textToSend.substring(firstSpace + 1).trim();

            const foundCmd = availableCommands.find(c => c.command.toLowerCase() === cmdPart.toLowerCase());
            if (foundCmd) {
                isSlashCommand = true;
                displayContent = textToSend; // Сохраняем "/итс вопрос" для отображения

                // Проверка для /итс
                if (foundCmd.id === 'its') {
                    const naparnik = settings?.mcp_servers.find(s => s.id === 'builtin-1c-naparnik');
                    if (!naparnik || !naparnik.enabled) {
                        alert('Для использования команды /итс необходимо включить MCP сервер "Напарник" в настройках.');
                        return;
                    }
                }

                let expanded = foundCmd.template;
                const diagStringsText = (diagnostics || []).map((d: any) => `- Line ${d.line + 1}: ${d.message} (${d.severity})`).join('\n');
                expanded = expanded.replace('{diagnostics}', diagStringsText || 'Ошибок не обнаружено');
                expanded = expanded.replace('{code}', contextCode || modifiedCode || '');
                expanded = expanded.replace('{query}', queryPart);
                textToSend = expanded;
            }
        }

        if (!textToSend.trim() || isLoading) return;

        // ... rest of the logic ...

        const diagStrings = (diagnostics || []).map((d: any) => `- Line ${d.line + 1}: ${d.message} (${d.severity})`);

        // Если это расширенная слеш-команда, мы НЕ передаем contextCode повторно, 
        // так как он уже вставлен в expanded-шаблон через {code}
        const finalContext = isSlashCommand ? undefined : (contextCode || modifiedCode);

        sendMessage(textToSend, finalContext, diagStrings, displayContent);
        setInput('');
        // Clear context after sending
        setContextCode('');
        setIsContextSelection(false);
    };

    const handleSelectCommand = (cmd: SlashCommand) => {
        // Находим позицию слеша, чтобы заменить его на саму команду или шаблон
        const lastSlashIndex = input.lastIndexOf('/');
        if (lastSlashIndex === -1) return;

        const beforeSlash = input.substring(0, lastSlashIndex);
        const afterSlash = input.substring(lastSlashIndex + 1);

        // Извлекаем query (все что после первого пробела в 'afterSlash', если он есть)
        const firstSpaceInAfter = afterSlash.indexOf(' ');
        const queryPart = firstSpaceInAfter === -1 ? '' : afterSlash.substring(firstSpaceInAfter + 1).trim();

        // Вместо немедленной отправки, подставляем команду в поле ввода
        // Если команда системная и сложная (как /исправить), оставляем как есть,
        // но для удобства пользователя мы просто вставляем "/команда "
        const newValue = `${beforeSlash}/${cmd.command} ${queryPart}`.trim() + ' ';
        setInput(newValue);

        setShowCommands(false);
        setCommandFilter('');

        // Устанавливаем фокус обратно в textarea (через ref, если он есть)
        // Но так как input привязан к состоянию, пользователь просто продолжит ввод
    };

    // Expose testing hooks
    useEffect(() => {
        (window as any).__MINI_AI_TEST__ = {
            setBaselineCode: (code: string) => {
                setContextCode(code);
                if (onCodeLoaded) {
                    onCodeLoaded(code, true);
                }
                console.log("[TEST] Baseline code set and propagated, length:", code.length);
            },
            sendMessage: (text: string) => {
                setInput(text);
                console.log("[TEST] Triggering sendMessage with:", text);
                handleSendMessage(text);
            },
            injectAssistantMessage: (content: string) => {
                console.log("[TEST] injectAssistantMessage called");
            }
        };
        return () => { delete (window as any).__MINI_AI_TEST__; };
    }, [handleSendMessage, onCodeLoaded]);

    const handleInputChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
        const value = e.target.value;
        const cursorPosition = e.target.selectionStart;
        setInput(value);

        // Логика открытия меню команд
        if (value && cursorPosition > 0) {
            const textBeforeCursor = value.substring(0, cursorPosition);
            const lastSlashIndex = textBeforeCursor.lastIndexOf('/');

            if (lastSlashIndex !== -1) {
                // Проверяем, что перед слешем либо начало строки, либо пробел
                const charBeforeSlash = lastSlashIndex > 0 ? textBeforeCursor[lastSlashIndex - 1] : '';
                if (charBeforeSlash === '' || charBeforeSlash === ' ' || charBeforeSlash === '\n') {
                    const filterText = textBeforeCursor.substring(lastSlashIndex + 1);
                    // Фильтр не должен содержать пробелов (команда заканчивается пробелом)
                    if (!filterText.includes(' ')) {
                        setShowCommands(true);
                        setCommandFilter(filterText);
                        return;
                    }
                }
            }
        }

        if (showCommands) {
            setShowCommands(false);
        }
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (showCommands) {
            // В меню команд перехватываем стрелки и Enter
            if (e.key === 'ArrowUp' || e.key === 'ArrowDown' || e.key === 'Enter' || e.key === 'Escape') {
                return;
            }
        }

        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            handleSendMessage();
        }
    };

    const toggleThinking = (index: number) => {
        setExpandedThinking(prev => ({ ...prev, [index]: !prev[index] }));
    };

    const handleLoadCode = async (isSelection: boolean) => {
        let code = await getCode(isSelection);

        // Safeguard: Filter out any internal markers that might have leaked
        if (code.includes('___1C_AI_MARKER_')) {
            console.warn("[ChatArea] Clipboard marker detected in loaded code. Filtering.");
            code = code.replace(/___1C_AI_MARKER_.*?___/g, '').trim();
        }

        setContextCode(code);
        setIsContextSelection(isSelection);
        if (onCodeLoaded) {
            onCodeLoaded(code, isSelection);
        }
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
            const diagStrings = (diagnostics || []).map((d: any) => `- Line ${d.line + 1}: ${d.message} (${d.severity})`);
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
            <div
                ref={scrollRef}
                onScroll={handleScroll}
                className={`flex-1 ${messages.length === 0 ? 'overflow-hidden' : 'overflow-y-auto scrollbar-thin scrollbar-thumb-white/10'} bg-[#09090b]`}
            >
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
                                    onClick: () => { if (onOpenSettings) onOpenSettings('bsl'); }
                                },
                                {
                                    title: "Серверы MCP",
                                    desc: "Предустановленные инструменты: 1C:Метаданные, 1C:Напарник.",
                                    icon: <Settings className="w-5 h-5 text-orange-400" />,
                                    onClick: () => { if (onOpenSettings) onOpenSettings('mcp'); }
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
                                <div className="flex flex-col items-center leading-tight">
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
                                                        {msg.thinking && isLoading && i === messages.length - 1 && chatStatus ? chatStatus : 'Думаю'}
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
                                                    originalCode={contextCode || modifiedCode || ""}
                                                />
                                                {(() => {
                                                    const msgKey = msg.id || String(i);
                                                    const action = diffActions.get(msgKey);

                                                    // Если действие уже совершено — показываем badge
                                                    if (action) {
                                                        return (
                                                            <div className={`flex items-center gap-1.5 mt-2 w-fit ml-auto px-2.5 py-1 rounded-full text-[11px] font-medium ${action === 'accepted'
                                                                ? 'bg-emerald-500/10 text-emerald-400 border border-emerald-500/20'
                                                                : 'bg-zinc-800/60 text-zinc-500 border border-zinc-700/40'
                                                                }`}>
                                                                {action === 'accepted' ? (
                                                                    <>
                                                                        <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.5} d="M5 13l4 4L19 7" /></svg>
                                                                        Изменения приняты
                                                                    </>
                                                                ) : (
                                                                    <>
                                                                        <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.5} d="M6 18L18 6M6 6l12 12" /></svg>
                                                                        Изменения отклонены
                                                                    </>
                                                                )}
                                                            </div>
                                                        );
                                                    }

                                                    const isDiffActive = activeDiffContent && (activeDiffContent === msg.content || msg.content.includes(activeDiffContent.substring(0, 50)));
                                                    const shouldShowBanner = hasDiffBlocks(msg.content) &&
                                                        parseDiffBlocks(msg.content).length > 0 &&
                                                        !dismissedDiffMessages.has(msgKey) &&
                                                        isDiffActive;

                                                    if (!shouldShowBanner) return null;

                                                    return (
                                                        <DiffSummaryBanner
                                                            content={msg.content}
                                                            onApply={() => {
                                                                // modifiedCode уже содержит применённый дифф (auto-apply из useEffect).
                                                                // Просто фиксируем его как новый бейзлайн — НЕ применяем дифф повторно.
                                                                if (onCommitCode) {
                                                                    onCommitCode(modifiedCode || '');
                                                                } else if (onApplyCode) {
                                                                    onApplyCode(modifiedCode || '');
                                                                }
                                                                if (onActiveDiffChange) onActiveDiffChange('');
                                                                setDiffActions(prev => new Map(prev).set(msgKey, 'accepted'));
                                                            }}
                                                            onReject={() => {
                                                                if (originalCode) {
                                                                    if (onCommitCode) {
                                                                        onCommitCode(originalCode);
                                                                    } else if (onApplyCode) {
                                                                        onApplyCode(originalCode);
                                                                    }
                                                                }
                                                                if (onActiveDiffChange) onActiveDiffChange('');
                                                                setDiffActions(prev => new Map(prev).set(msgKey, 'rejected'));
                                                            }}
                                                        />
                                                    );
                                                })()}
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
                                            <pre className="whitespace-pre-wrap font-sans break-words break-all overflow-hidden" style={{ fontFamily: 'Inter, sans-serif', overflowWrap: 'anywhere' }}>{msg.displayContent || msg.content}</pre>
                                        )}
                                    </div>


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
                        {modifiedCode && modifiedCode.length > 0 && `CONTEXT: ${modifiedCode.length} chars`}
                    </div>
                </div>
                <div className="relative bg-[#18181b] border border-[#27272a] rounded-xl focus-within:ring-1 focus-within:ring-blue-500/50 transition-all min-h-[120px] flex flex-col max-w-4xl mx-auto">

                    <textarea
                        ref={inputRef}
                        value={input}
                        onChange={handleInputChange}
                        onKeyDown={handleKeyDown}
                        placeholder="Опишите задачу, вставьте код или введите / для команд..."
                        className="w-full h-full bg-transparent text-zinc-300 px-4 py-3 resize-none focus:outline-none placeholder-zinc-600 text-[13px] font-sans leading-relaxed flex-1"
                        style={{ fontFamily: 'Inter, sans-serif' }}
                    />

                    {showCommands && filteredCommands.length > 0 && (
                        <CommandMenu
                            commands={filteredCommands}
                            onSelect={handleSelectCommand}
                            onClose={() => setShowCommands(false)}
                            anchorRect={inputRef.current?.getBoundingClientRect() || null}
                        />
                    )}

                    <div ref={dropdownRef} className="px-3 pb-2 pt-0 flex items-end gap-2 pointer-events-auto flex-wrap w-full">
                        <div className="flex items-center gap-1.5 flex-1 min-w-0">
                            <div className="relative">
                                <button
                                    onClick={() => setShowModelDropdown(!showModelDropdown)}
                                    className="h-8 flex items-center gap-1.5 px-3 rounded-full bg-zinc-900 border border-zinc-800 text-zinc-300 hover:bg-zinc-800 transition-all text-[11px] font-medium active:scale-95"
                                >
                                    {(() => {
                                        const activeProfile = profiles.find(p => p.id === activeProfileId);
                                        const isQwen = activeProfile?.provider === 'QwenCli';
                                        const qwenStatus = cliStatuses['qwen'];
                                        return (
                                            <>
                                                <Brain className={`w-3.5 h-3.5 ${isQwen ? 'text-amber-400' : 'text-blue-400'}`} />
                                                {activeProfile?.name || 'Выберите профиль'}
                                                {isQwen && qwenStatus?.is_authenticated && qwenStatus.usage && (
                                                    <span className={`text-[9px] font-mono px-1.5 py-0.5 rounded-full border ${
                                                        qwenStatus.usage.requests_used / qwenStatus.usage.requests_limit > 0.8
                                                            ? 'bg-amber-500/15 text-amber-400 border-amber-500/30'
                                                            : 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30'
                                                    }`}>
                                                        {qwenStatus.usage.requests_used}/{qwenStatus.usage.requests_limit}
                                                    </span>
                                                )}
                                                {isQwen && qwenStatus?.is_authenticated && !qwenStatus.usage && (
                                                    <span className="text-[9px] bg-emerald-500/15 text-emerald-400 border border-emerald-500/30 px-1.5 py-0.5 rounded-full">
                                                        free
                                                    </span>
                                                )}
                                                {isQwen && qwenStatus && !qwenStatus.is_authenticated && (
                                                    <span className="text-[9px] bg-red-500/15 text-red-400 border border-red-500/30 px-1.5 py-0.5 rounded-full">
                                                        Войти
                                                    </span>
                                                )}
                                            </>
                                        );
                                    })()}
                                    <ChevronDown className={`w-3 h-3 transition-transform duration-200 ${showModelDropdown ? 'rotate-180' : ''}`} />
                                </button>

                                {showModelDropdown && (
                                    <div className="absolute bottom-full left-0 mb-2 w-64 bg-[#09090b] border border-[#27272a] rounded-xl shadow-2xl z-50 overflow-hidden py-1 animate-in slide-in-from-bottom-2 duration-200">
                                        <div className="px-3 py-2 border-b border-[#27272a] mb-1">
                                            <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider">Ваши профили</span>
                                        </div>
                                        <div className="max-h-[350px] overflow-y-auto custom-scrollbar">
                                            {/* Standard Assistants Section */}
                                            {profiles.filter(p => p.provider !== 'QwenCli').length > 0 && (
                                                <>
                                                    <div className="px-3 py-1.5 border-b border-[#27272a] mb-1 sticky top-0 bg-[#09090b] z-10">
                                                        <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider">Стандартные ассистенты</span>
                                                    </div>
                                                    {profiles.filter(p => p.provider !== 'QwenCli').map(p => (
                                                        <div
                                                            key={p.id}
                                                            className={`group px-3 py-2 flex items-center justify-between cursor-pointer transition-colors ${activeProfileId === p.id ? 'bg-blue-500/10' : 'hover:bg-zinc-800/50'}`}
                                                            onClick={() => {
                                                                setActiveProfile(p.id);
                                                                setShowModelDropdown(false);
                                                            }}
                                                        >
                                                            <div className="flex flex-col gap-0.5 min-w-0">
                                                                <span className={`text-[12px] font-semibold truncate ${activeProfileId === p.id ? 'text-blue-400' : 'text-zinc-200'}`}>{p.name}</span>
                                                                <span className="text-[10px] text-zinc-500 truncate">{p.provider} • {p.model}</span>
                                                            </div>
                                                            {activeProfileId === p.id && <Check className="w-3.5 h-3.5 text-blue-500 flex-shrink-0" />}
                                                        </div>
                                                    ))}
                                                </>
                                            )}

                                            {/* CLI Providers Section */}
                                            {profiles.filter(p => p.provider === 'QwenCli').length > 0 && (
                                                <>
                                                    <div className="px-3 py-1.5 border-b border-[#27272a] mt-1 mb-1 sticky top-0 bg-[#09090b] z-10">
                                                        <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider">CLI Провайдеры (Free)</span>
                                                    </div>
                                                    {profiles.filter(p => p.provider === 'QwenCli').map(p => {
                                                        const status = cliStatuses['qwen'];
                                                        const isAuthenticated = status?.is_authenticated;

                                                        return (
                                                            <div
                                                                key={p.id}
                                                                className={`group px-3 py-2 flex items-center justify-between cursor-pointer transition-colors ${activeProfileId === p.id ? 'bg-amber-500/10' : 'hover:bg-zinc-800/50'}`}
                                                                onClick={() => {
                                                                    if (!isAuthenticated) {
                                                                        setIsAuthModalOpen(true);
                                                                    } else {
                                                                        setActiveProfile(p.id);
                                                                        setShowModelDropdown(false);
                                                                    }
                                                                }}
                                                            >
                                                                <div className="flex flex-col gap-0.5 min-w-0">
                                                                    <div className="flex items-center gap-1.5">
                                                                        <span className={`text-[12px] font-semibold truncate ${activeProfileId === p.id ? 'text-amber-400' : 'text-zinc-200'}`}>{p.name}</span>
                                                                        {!isAuthenticated && <span className="text-[9px] bg-red-500/20 text-red-500 px-1 rounded border border-red-500/20">Login required</span>}
                                                                    </div>
                                                                    <div className="flex items-center gap-2">
                                                                        <span className="text-[10px] text-zinc-500 truncate">{p.model}</span>
                                                                        {isAuthenticated && status?.usage && (
                                                                            <span className="text-[9px] text-zinc-600 font-mono">
                                                                                {status.usage.requests_used}/{status.usage.requests_limit}
                                                                            </span>
                                                                        )}
                                                                    </div>
                                                                </div>
                                                                <div className="flex items-center gap-2">
                                                                    {activeProfileId === p.id && <Check className="w-3.5 h-3.5 text-amber-500 flex-shrink-0" />}
                                                                    <Terminal className="w-3 h-3 text-zinc-700" />
                                                                </div>
                                                            </div>
                                                        );
                                                    })}
                                                </>
                                            )}
                                        </div>
                                        <div className="p-2 border-t border-[#27272a] mt-1">
                                            <button
                                                onClick={(e) => {
                                                    e.stopPropagation();
                                                    onOpenSettings?.('llm');
                                                    setShowModelDropdown(false);
                                                }}
                                                className="w-full py-1.5 px-3 flex items-center justify-center gap-2 text-[11px] text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800/50 rounded-lg transition-all"
                                            >
                                                <Settings className="w-3.5 h-3.5" /> Настроить профили
                                            </button>
                                        </div>
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
                                        refreshWindows();
                                    }
                                }}
                                    className={`flex-shrink-0 flex items-center gap-1.5 text-[12px] font-medium px-2.5 h-8 rounded-md transition-all border border-transparent ${showConfigDropdown ? 'bg-zinc-800 text-zinc-200 border-zinc-700' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
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
                                    className={`flex-shrink-0 flex items-center gap-1.5 text-[12px] font-medium px-2 h-8 rounded-md transition-all border border-transparent ${showGetCodeDropdown ? 'bg-zinc-800 text-zinc-200 border-zinc-700' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
                                >
                                    <FileText className="w-3.5 h-3.5" />
                                    <span className="hidden xl:inline">Код</span>
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

                            {/* Behavior Preset Toggle */}
                            {settings?.code_generation && (
                                <div className="flex items-center bg-zinc-800/30 border border-zinc-700/50 rounded-lg p-0.5 h-8 flex-shrink-0">
                                    <button
                                        onClick={() => {
                                            if (settings) {
                                                updateSettings({
                                                    ...settings,
                                                    code_generation: {
                                                        ...settings.code_generation,
                                                        behavior_preset: 'project'
                                                    }
                                                });
                                            }
                                        }}
                                        className={`flex items-center gap-1 px-2.5 h-full rounded-md text-[10px] font-bold transition-all ${settings.code_generation.behavior_preset === 'project'
                                            ? 'bg-blue-500/15 text-blue-400 shadow-sm'
                                            : 'text-zinc-600 hover:text-zinc-400'
                                            }`}
                                        title="Свой код: Чистая разработка, стандарты 1С"
                                    >
                                        <User className="w-3 h-3" />
                                        <span className="hidden sm:inline">СВОЙ</span>
                                    </button>
                                    <button
                                        onClick={() => {
                                            if (settings) {
                                                updateSettings({
                                                    ...settings,
                                                    code_generation: {
                                                        ...settings.code_generation,
                                                        behavior_preset: 'maintenance'
                                                    }
                                                });
                                            }
                                        }}
                                        className={`flex items-center gap-1 px-2.5 h-full rounded-md text-[10px] font-bold transition-all ${settings.code_generation.behavior_preset === 'maintenance'
                                            ? 'bg-orange-500/15 text-orange-400 shadow-sm'
                                            : 'text-zinc-600 hover:text-zinc-400'
                                            }`}
                                        title="Чужой код: Изоляция правок, запрет рефакторинга"
                                    >
                                        <HardHat className="w-3 h-3" />
                                        <span className="hidden sm:inline">ЧУЖОЙ</span>
                                    </button>
                                </div>
                            )}
                        </div>

                        <div className="flex items-center gap-1">
                            {isSupported && (
                                <div className="relative">
                                    {showVoiceHint && (
                                        <div className="absolute bottom-full right-0 mb-4 w-64 p-3 bg-blue-600 text-white text-xs rounded-xl shadow-2xl animate-in fade-in slide-in-from-bottom-2 duration-300 z-50">
                                            <div className="font-bold mb-1 flex items-center gap-2">
                                                <Mic className="w-3 h-3" />
                                                Нужно разрешение
                                            </div>
                                            Нажмите "Разрешить" в появившемся окне браузера в верхнем левом углу для доступа к микрофону.
                                            <div className="absolute top-full right-4 w-3 h-3 bg-blue-600 rotate-45 -translate-y-1.5" />
                                        </div>
                                    )}
                                    <button
                                        onClick={async () => {
                                            if (isLoading) return;
                                            const wasRecording = isRecording;
                                            await toggleRecording();

                                            // Show hint ONLY if we are starting AND permission is NOT granted
                                            // 'unknown' is also treated as "maybe show" to be safe, but only if user hasn't seen it
                                            if (!wasRecording && (permissionState === 'prompt' || permissionState === 'unknown')) {
                                                setShowVoiceHint(true);
                                                setTimeout(() => setShowVoiceHint(false), 5000);
                                            }
                                        }}
                                        disabled={isLoading}
                                        className={`w-8 h-8 flex items-center justify-center rounded-lg transition-all ${isLoading ? 'opacity-20 cursor-not-allowed' : ''} ${isRecording ? 'bg-red-500 text-white shadow-[0_0_10px_rgba(239,68,68,0.5)]' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
                                        title={isLoading ? 'Голосовой ввод недоступен во время генерации' : (isRecording ? 'Остановить запись' : 'Голосовой ввод')}
                                    >
                                        <Mic className={`w-4 h-4 ${isRecording ? 'animate-pulse' : ''}`} />
                                    </button>
                                </div>
                            )}

                            <button onClick={isLoading ? stopChat : () => handleSendMessage()} disabled={!isLoading && !input.trim()} className={`w-8 h-8 flex items-center justify-center rounded-lg transition-colors flex-shrink-0 ${isLoading ? 'bg-red-500/10 text-red-400' : input.trim() ? 'bg-blue-600 text-white' : 'bg-[#27272a] text-zinc-600'}`}>
                                {isLoading ? <Square className="w-4 h-4 fill-current" /> : <ArrowUp className="w-4 h-4" strokeWidth={2.5} />}
                            </button>
                        </div>
                    </div>
                </div>
            </div>

            {isAuthModalOpen && (
                <QwenAuthModal
                    isOpen={isAuthModalOpen}
                    onClose={() => {
                        setIsAuthModalOpen(false);
                        fetchCliStatuses();
                    }}
                    onSuccess={async (access_token, refresh_token, expires_at, resource_url) => {
                        console.log('[DEBUG] ChatArea: Qwen Auth Success, saving token...');
                        try {
                            await cliProvidersApi.saveToken('qwen', access_token, refresh_token, expires_at, resource_url);
                            console.log('[DEBUG] ChatArea: Token saved successfully');
                            await fetchCliStatuses();
                        } catch (err) {
                            console.error('[DEBUG] ChatArea: Failed to save token:', err);
                        }
                    }}
                />
            )}
        </div >
    );
}
