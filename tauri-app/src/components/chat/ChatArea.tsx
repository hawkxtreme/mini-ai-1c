import { useRef, useEffect, useState, useMemo } from 'react';
import { useChat, ToolCall, ChatMessage } from '../../contexts/ChatContext';
import { useProfiles } from '../../contexts/ProfileContext';
import { useSettings } from '../../contexts/SettingsContext';
import { useConfigurator } from '../../contexts/ConfiguratorContext';
import { parseConfiguratorTitle, ConfiguratorTitleContext } from '../../utils/configurator';
import { MarkdownRenderer, cleanDiffArtifacts } from '../MarkdownRenderer';
import { Loader2, Square, ArrowUp, Settings, ChevronDown, ChevronRight, Monitor, RefreshCw, FileText, MousePointerClick, Brain, BrainCircuit, Check, X, Terminal, Pencil, Play, Send, User, HardHat, Mic, MoreHorizontal } from 'lucide-react';
import { useVoiceInput } from '../../voice/useVoiceInput';
import logo from '../../assets/logo.png';
import ToolCallBlock from './ToolCallBlock';
import { MessageActions } from './MessageActions';
import { applyDiff, applyDiffWithDiagnostics, formatDiffErrorMessage, hasDiffBlocks, extractDisplayCode, stripCodeBlocks, parseDiffBlocks, hasApplicableDiffBlocks } from '../../utils/diffViewer';
import { FileDiff, Plus, Minus, Edit2, PanelRight } from 'lucide-react';
import { CommandMenu } from './CommandMenu';
import { ContextChips } from './ContextChips';
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

function buildCopyContent(msg: ChatMessage): string {
    if (msg.role !== 'assistant' || !msg.parts) {
        return msg.content;
    }
    const sections: string[] = [];
    const merged = msg.parts.reduce<{ type: string; content?: string; toolCallId?: string }[]>((acc, part) => {
        if (part.type === 'text' && acc.length > 0 && acc[acc.length - 1].type === 'text') {
            acc[acc.length - 1] = { ...acc[acc.length - 1], content: (acc[acc.length - 1].content || '') + (part.content || '') };
        } else {
            acc.push({ ...part });
        }
        return acc;
    }, []);
    for (const part of merged) {
        if (part.type === 'text' && part.content?.trim()) {
            sections.push(part.content.trim());
        } else if (part.type === 'tool' && part.toolCallId) {
            const tc = msg.toolCalls?.find(t => t.id === part.toolCallId);
            if (tc && (tc.status === 'done' || tc.status === 'error')) {
                const lines: string[] = [`[Tool: ${tc.name}]`];
                if (tc.arguments?.trim()) {
                    lines.push(`Аргументы: ${tc.arguments}`);
                }
                if (tc.result?.trim()) {
                    lines.push(`Результат: ${tc.result}`);
                }
                sections.push(lines.join('\n'));
            }
        }
    }
    return sections.join('\n\n') || msg.content;
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
    const { messages, isLoading, chatStatus, currentIteration, sendMessage, stopChat, editAndRerun, addSystemMessage } = useChat();
    const { profiles, activeProfileId, setActiveProfile } = useProfiles();
    const { settings, updateSettings } = useSettings();
    const { detectedWindows, selectedHwnd, refreshWindows, selectWindow, activeConfigTitle, getCode, parsedTitleContext } = useConfigurator();

    const [appliedDiffMessages, setAppliedDiffMessages] = useState<Set<string>>(new Set());
    const [dismissedDiffMessages, setDismissedDiffMessages] = useState<Set<string>>(new Set());
    const [diffActions, setDiffActions] = useState<Map<string, 'accepted' | 'rejected'>>(new Map());
    const [input, setInput] = useState('');
    const [showModelDropdown, setShowModelDropdown] = useState(false);
    const [showConfigDropdown, setShowConfigDropdown] = useState(false);
    const [isAuthModalOpen, setIsAuthModalOpen] = useState(false);
    const [cliStatuses, setCliStatuses] = useState<Record<string, CliStatus>>({});
    const [showGetCodeDropdown, setShowGetCodeDropdown] = useState(false);
    const [expandedThinking, setExpandedThinking] = useState<Record<string, boolean>>({});
    const [contextCode, setContextCode] = useState('');
    const [isContextSelection, setIsContextSelection] = useState(false);
    const [configuratorTitleCtx, setConfiguratorTitleCtx] = useState<ConfiguratorTitleContext | null>(null);
    const [editingIndex, setEditingIndex] = useState<number | null>(null);
    const [editText, setEditText] = useState('');
    const [showVoiceHint, setShowVoiceHint] = useState(false);

    // Slash Commands state
    const [showCommands, setShowCommands] = useState(false);
    const [commandFilter, setCommandFilter] = useState('');
    const availableCommands = useMemo(() => {
        const saved = settings?.slash_commands || DEFAULT_SLASH_COMMANDS;
        // Системные команды всегда используют актуальный шаблон из дефолтов
        const synced = saved.map(cmd => {
            if (cmd.is_system) {
                const def = DEFAULT_SLASH_COMMANDS.find(d => d.id === cmd.id);
                if (def) return { ...cmd, template: def.template };
            }
            return cmd;
        });
        return synced.filter(c => c.is_enabled);
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
        const activeProfile = profiles.find(p => p.id === activeProfileId);
        if (activeProfile?.provider !== 'QwenCli') return;

        try {
            const status = await cliProvidersApi.getStatus(activeProfile.id, 'qwen');
            setCliStatuses(prev => ({ ...prev, [activeProfile.id]: status }));
        } catch (err) {
            console.error(`Failed to fetch CLI status for profile ${activeProfile.id}:`, err);
        }
    };

    // Consolidated CLI status effect
    useEffect(() => {
        fetchCliStatuses();
    }, [activeProfileId]);

    // Update status when generation completed
    const prevIsLoadingRef = useRef(false);
    useEffect(() => {
        if (prevIsLoadingRef.current && !isLoading) {
            fetchCliStatuses();
        }
        prevIsLoadingRef.current = isLoading;
    }, [isLoading]);

    // Periodic check ONLY for QwenCli
    const cliStatusIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
    useEffect(() => {
        if (cliStatusIntervalRef.current !== null) {
            clearInterval(cliStatusIntervalRef.current);
            cliStatusIntervalRef.current = null;
        }
        const activeProfile = profiles.find(p => p.id === activeProfileId);
        if (activeProfile?.provider !== 'QwenCli') return;

        cliStatusIntervalRef.current = setInterval(() => {
            fetchCliStatuses();
        }, 60_000);
        return () => {
            if (cliStatusIntervalRef.current !== null) {
                clearInterval(cliStatusIntervalRef.current);
                cliStatusIntervalRef.current = null;
            }
        };
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
    const autoScrollRaf = useRef<number | null>(null);
    const isLoadingRef = useRef(isLoading);

    // Синхронизируем ref с состоянием isLoading
    useEffect(() => {
        isLoadingRef.current = isLoading;
    }, [isLoading]);

    // Функция tick в ref — доступна из handleScroll без замыкания
    const tickFnRef = useRef<(() => void) | null>(null);

    // Обработчик скролла — отслеживаем ручную прокрутку вверх
    const handleScroll = () => {
        if (scrollRef.current) {
            const { scrollTop, scrollHeight, clientHeight } = scrollRef.current;
            const isAtBottom = scrollHeight - scrollTop <= clientHeight + 100;
            wasAtBottom.current = isAtBottom;
            // Пользователь прокрутил вверх — останавливаем автоскролл
            if (!isAtBottom && autoScrollRaf.current) {
                cancelAnimationFrame(autoScrollRaf.current);
                autoScrollRaf.current = null;
            }
            // Пользователь вернулся вниз — возобновляем RAF если идёт стриминг
            if (isAtBottom && isLoadingRef.current && !autoScrollRaf.current && tickFnRef.current) {
                autoScrollRaf.current = requestAnimationFrame(tickFnRef.current);
            }
        }
    };

    // RAF-цикл плавной прокрутки во время стриминга
    useEffect(() => {
        if (!isLoading) {
            if (autoScrollRaf.current) {
                cancelAnimationFrame(autoScrollRaf.current);
                autoScrollRaf.current = null;
            }
            tickFnRef.current = null;
            // Финальная плавная прокрутка после завершения генерации
            if (wasAtBottom.current && scrollRef.current) {
                scrollRef.current.scrollTo({ top: scrollRef.current.scrollHeight, behavior: 'smooth' });
            }
            return;
        }

        // При старте генерации всегда следуем за контентом
        wasAtBottom.current = true;

        const tick = () => {
            const el = scrollRef.current;
            if (!el || !wasAtBottom.current) {
                autoScrollRaf.current = null;
                return;
            }
            const maxScroll = el.scrollHeight - el.clientHeight;
            const diff = maxScroll - el.scrollTop;
            if (diff > 1) {
                el.scrollTop += Math.ceil(Math.max(3, diff * 0.2));
            }
            autoScrollRaf.current = requestAnimationFrame(tick);
        };

        // Сохраняем tick в ref чтобы handleScroll мог его перезапустить
        tickFnRef.current = tick;

        if (!autoScrollRaf.current) {
            autoScrollRaf.current = requestAnimationFrame(tick);
        }

        return () => {
            if (autoScrollRaf.current) {
                cancelAnimationFrame(autoScrollRaf.current);
                autoScrollRaf.current = null;
            }
        };
    }, [isLoading]);

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
        if (messages.length === 0) {
            setContextCode('');
            setIsContextSelection(false);
            setConfiguratorTitleCtx(null);
        }
    }, [messages.length]);

    useEffect(() => {
        // Показываем предпросмотр диффов в боковой панели — НЕ применяем автоматически.
        // Срабатывает сразу (в том числе во время стриминга), чтобы DiffEditor обновлялся в реальном времени.
        if (messages.length === 0) return;
        const lastMsg = messages[messages.length - 1];
        if (lastMsg.role !== 'assistant' || !hasDiffBlocks(lastMsg.content)) return;

        const msgKey = lastMsg.id || String(messages.length - 1);

        // Если пользователь уже принял/отклонил изменения через баннер — не перебиваем его выбор
        if (diffActions.has(msgKey)) return;

        // Если превью уже было показано, а затем явно очищено (например, через боковую панель) —
        // не восстанавливаем его снова и помечаем как обработанное, чтобы баннер в чате
        // переключился на badge "Изменения приняты" вместо кнопок "Принять / Отменить".
        if (!activeDiffContent && appliedDiffMessages.has(msgKey)) {
            if (!diffActions.has(msgKey)) {
                setDiffActions(prev => new Map(prev).set(msgKey, 'accepted'));
            }
            return;
        }

        // Показываем diff-превью только ПОСЛЕ завершения стриминга
        if (!isLoading) {
            if (onActiveDiffChange) {
                onActiveDiffChange(lastMsg.content);
            }
            // Фиксируем как "показанное"
            if (!appliedDiffMessages.has(msgKey)) {
                setAppliedDiffMessages(prev => new Set(prev).add(msgKey));
            }
        }
    }, [messages, isLoading, onActiveDiffChange, appliedDiffMessages, diffActions, activeDiffContent]);

    const handleSendMessage = async (textOverride?: string) => {
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

                // Проверка для команд поиска по конфигурации
                if (['search-1c', 'refs-1c', 'struct-1c'].includes(foundCmd.id)) {
                    const searchServer = settings?.mcp_servers.find(s => s.id === 'builtin-1c-search');
                    if (!searchServer || !searchServer.enabled) {
                        alert('Для использования этой команды необходимо включить MCP сервер "1С:Поиск по конфигурации" в настройках и указать путь к выгрузке конфигурации.');
                        return;
                    }
                }

                let expanded = foundCmd.template;

                // Если шаблон требует {code}, а у нас нет contextCode, 
                // пытаемся автоматически получить выделенный текст из активного окна Конфигуратора
                let activeCode = contextCode || modifiedCode || '';
                if (expanded.includes('{code}') && !contextCode && selectedHwnd) {
                    try {
                        const fetchedCode = await getCode(true); // Запрашиваем только выделение
                        if (fetchedCode && fetchedCode.trim().length > 0) {
                            activeCode = fetchedCode;
                        } else {
                            // Если нет выделения, запрашиваем весь текст
                            const fullCode = await getCode(false);
                            if (fullCode && fullCode.trim().length > 0) {
                                activeCode = fullCode;
                            }
                        }
                    } catch (err) {
                        console.error('Failed to auto-fetch code context for slash command:', err);
                    }
                }

                const diagStringsText = (diagnostics || []).map((d: any) => `- Line ${d.line + 1}: ${d.message} (${d.severity})`).join('\n');
                expanded = expanded.replace('{diagnostics}', diagStringsText || 'Ошибок не обнаружено');
                expanded = expanded.replace('{code}', activeCode);
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

        sendMessage(textToSend, finalContext, diagStrings, displayContent, configuratorTitleCtx);
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

    const toggleThinking = (key: string) => {
        setExpandedThinking(prev => ({ ...prev, [key]: !prev[key] }));
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
        // Захватываем контекст заголовка конфигуратора в момент загрузки кода
        setConfiguratorTitleCtx(parsedTitleContext);
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
        setConfiguratorTitleCtx(null);
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
            editAndRerun(index, editText, contextCode || modifiedCode, diagStrings, undefined, configuratorTitleCtx);
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
    // Индекс последнего assistant-сообщения с diff-блоками.
    // Баннер "Принять/Отменить" показываем ТОЛЬКО там — иначе при chat-new-iteration
    // два сообщения показывают кнопки одновременно.
    const lastDiffMsgIndex = useMemo(() => {
        for (let i = messages.length - 1; i >= 0; i--) {
            if (messages[i].role === 'assistant' && hasDiffBlocks(messages[i].content)) {
                return i;
            }
        }
        return -1;
    }, [messages]);

    return (
        <div id="chat-area" className="flex flex-col flex-1 min-w-[300px] transition-all duration-300">
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
                            {/* Системное сообщение (уведомление об ошибках применения диффов) */}
                            {(msg.role as string) === 'system' ? (
                                <div className="w-full max-w-full rounded-xl border border-amber-700/40 bg-amber-950/30 px-4 py-3 text-[13px] text-amber-300/90 shadow-sm">
                                    <div className="flex items-start gap-2">
                                        <svg className="w-4 h-4 text-amber-400 flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L3.34 16.5c-.77.833.192 2.5 1.732 2.5z" />
                                        </svg>
                                        <div className="flex-1 whitespace-pre-wrap leading-relaxed">
                                            {msg.content}
                                        </div>
                                    </div>
                                </div>
                            ) : (
                                <div className={`p-4 rounded-xl border text-[13px] leading-relaxed group ${msg.role === 'user' ? 'bg-[#1b1b1f] border-zinc-800/80 text-zinc-300 max-w-[90%]' : 'bg-zinc-900/40 border-zinc-800/50 text-zinc-300 shadow-sm w-full max-w-full'}`}>
                                    <div className="min-w-0 flex flex-col gap-3">
                                        {/* Message Header with Actions */}
                                        <div className="flex items-start justify-end gap-2 mb-2">
                                            {/* Actions */}
                                            <MessageActions
                                                content={buildCopyContent(msg)}
                                                timestamp={msg.timestamp}
                                                isUser={msg.role === 'user'}
                                                onEdit={msg.role === 'user' ? () => handleStartEdit(i, msg.content) : undefined}
                                            />
                                        </div>

                                        <div className="min-w-0 flex flex-col gap-3">
                                            {msg.role === 'assistant' && msg.parts ? (
                                                <>
                                                    {/* Объединяем соседние text-части чтобы tool call не разбивал слова */}
                                                    {msg.parts.reduce<{ type: string; content?: string; toolCallId?: string; origIdx: number }[]>((acc, part, idx) => {
                                                        if (part.type === 'text' && acc.length > 0 && acc[acc.length - 1].type === 'text') {
                                                            acc[acc.length - 1] = { ...acc[acc.length - 1], content: (acc[acc.length - 1].content || '') + (part.content || '') };
                                                        } else {
                                                            acc.push({ ...part, origIdx: idx });
                                                        }
                                                        return acc;
                                                    }, []).map((part, partIdx) => {
                                                        if (part.type === 'thinking') {
                                                            const thinkingKey = `${i}-${partIdx}`;
                                                            const isThinkingStreaming = isLoading && i === messages.length - 1;
                                                            const isExpanded = expandedThinking[thinkingKey] ?? false;
                                                            return (
                                                                <div key={partIdx} className="my-1 mb-2">
                                                                    <button
                                                                        onClick={() => toggleThinking(thinkingKey)}
                                                                        className="flex items-center gap-2 text-[11px] text-white/40 hover:text-white/60 uppercase tracking-widest font-semibold transition-colors group mb-1.5"
                                                                    >
                                                                        <BrainCircuit className="w-3.5 h-3.5" />
                                                                        <span>{isThinkingStreaming && chatStatus ? chatStatus : 'Размышления'}</span>
                                                                        <ChevronRight className={`w-3.5 h-3.5 transition-transform ${isExpanded ? 'rotate-90' : ''}`} />
                                                                    </button>
                                                                    {isExpanded && (
                                                                        <div className="text-[12px] italic text-white/40 leading-relaxed border-l-2 border-white/10 pl-3 py-1 my-2 animate-in fade-in slide-in-from-top-1 whitespace-pre-wrap">
                                                                            {part.content}
                                                                        </div>
                                                                    )}
                                                                </div>
                                                            );
                                                        } else if (part.type === 'tool') {
                                                            const tc = msg.toolCalls?.find(t => t.id === part.toolCallId);
                                                            if (!tc) return null;
                                                            return (
                                                                <div key={partIdx} className="flex flex-col gap-0.5 mb-2 mt-1 -ml-1">
                                                                    <ToolCallBlock toolCall={tc} />
                                                                </div>
                                                            );
                                                        } else {
                                                            // text
                                                            const currentOriginalCode = contextCode || modifiedCode || "";
                                                            const cleanedContent = cleanDiffArtifacts(part.content || '', currentOriginalCode);
                                                            if (cleanedContent.trim().length === 0) return null;

                                                            return (
                                                                <div key={partIdx} className="min-w-0">
                                                                    <MarkdownRenderer
                                                                        content={part.content || ''}
                                                                        isStreaming={isLoading && i === messages.length - 1 && (part as any).origIdx === msg.parts!.length - 1}
                                                                        onApplyCode={onApplyCode}
                                                                        originalCode={currentOriginalCode}
                                                                    />
                                                                </div>
                                                            );
                                                        }
                                                    })}
                                                    {/* Статус выполнения — внутри пузыря, после всех parts */}
                                                    {isLoading && i === messages.length - 1 && (
                                                        <div className="flex items-center gap-2 mt-1 pt-2 border-t border-zinc-800/40">
                                                            <Loader2 className="w-3.5 h-3.5 animate-spin text-blue-400 flex-shrink-0" />
                                                            <span className="text-zinc-400 text-xs">{chatStatus || 'Выполнение...'}</span>
                                                            {currentIteration > 1 && (
                                                                <span className="text-[10px] bg-zinc-800 text-zinc-500 px-1.5 py-0.5 rounded-full border border-zinc-700 font-mono ml-1">
                                                                    Шаг {currentIteration}
                                                                </span>
                                                            )}
                                                        </div>
                                                    )}
                                                </>
                                            ) : (
                                                // Fallback for older messages or user messages
                                                <>
                                                    {/* Thinking Section */}
                                                    {msg.thinking && (
                                                        <div className="my-1 mb-3">
                                                            <button
                                                                onClick={() => toggleThinking(String(i))}
                                                                className="flex items-center gap-2 text-[11px] text-white/40 hover:text-white/60 uppercase tracking-widest font-semibold transition-colors group mb-1.5"
                                                            >
                                                                <BrainCircuit className="w-3.5 h-3.5" />
                                                                <span>{msg.thinking && isLoading && i === messages.length - 1 && chatStatus ? chatStatus : 'Размышления'}</span>
                                                                <ChevronRight className={`w-3.5 h-3.5 transition-transform ${expandedThinking[String(i)] ? 'rotate-90' : ''}`} />
                                                            </button>
                                                            {expandedThinking[String(i)] && (
                                                                <div className="text-[12px] italic text-white/40 leading-relaxed border-l-2 border-white/10 pl-3 py-1 my-2 animate-in fade-in slide-in-from-top-1 whitespace-pre-wrap">
                                                                    {msg.thinking}
                                                                </div>
                                                            )}
                                                        </div>
                                                    )}

                                                    {/* Tool Calls */}
                                                    {msg.toolCalls && msg.toolCalls.length > 0 && (
                                                        <div className="flex flex-col gap-0.5 mb-2 mt-1 -ml-1">
                                                            {msg.toolCalls.map((tc, idx) => (
                                                                <ToolCallBlock key={idx} toolCall={tc} />
                                                            ))}
                                                        </div>
                                                    )}

                                                    {/* Content */}
                                                    {(() => {
                                                        const currentOriginalCode = contextCode || modifiedCode || "";
                                                        const cleanedContent = cleanDiffArtifacts(msg.content || '', currentOriginalCode);
                                                        const hasVisibleContent = cleanedContent.trim().length > 0;

                                                        if (msg.role !== 'assistant' || !hasVisibleContent) return null;

                                                        return (
                                                            <div className="min-w-0">
                                                                <MarkdownRenderer
                                                                    content={msg.content}
                                                                    isStreaming={isLoading && i === messages.length - 1}
                                                                    onApplyCode={onApplyCode}
                                                                    originalCode={currentOriginalCode}
                                                                />
                                                            </div>
                                                        );
                                                    })()}
                                                </>
                                            )}

                                            {msg.role === 'assistant' ? (
                                                <>

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

                                                        const currentOriginalCode = contextCode || modifiedCode || "";
                                                        const hasContext = currentOriginalCode.trim().length > 0;
                                                        const shouldShowBanner = hasContext &&
                                                            i === lastDiffMsgIndex &&
                                                            !isLoading &&
                                                            hasDiffBlocks(msg.content) &&
                                                            parseDiffBlocks(msg.content).length > 0 &&
                                                            !dismissedDiffMessages.has(msgKey);

                                                        if (!shouldShowBanner) return null;

                                                        return (
                                                            <DiffSummaryBanner
                                                                content={msg.content}
                                                                onApply={() => {
                                                                    // Применяем дифф только сейчас — по явному подтверждению пользователя
                                                                    const baseCode = originalCode || modifiedCode || "";
                                                                    const diffResult = applyDiffWithDiagnostics(baseCode, msg.content);
                                                                    if (onApplyCode) {
                                                                        onApplyCode(diffResult.code);
                                                                    }
                                                                    if (diffResult.failedCount > 0 || diffResult.fuzzyCount > 0) {
                                                                        const errorMsg = formatDiffErrorMessage(diffResult);
                                                                        if (errorMsg) addSystemMessage(errorMsg);
                                                                    }
                                                                    if (onActiveDiffChange) onActiveDiffChange('');
                                                                    setDiffActions(prev => new Map(prev).set(msgKey, 'accepted'));
                                                                }}
                                                                onReject={() => {
                                                                    // Просто сбрасываем превью — код в редакторе не тронут
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
                            )}
                        </div>
                    ))}
                    {/* Индикатор ожидания первого ответа (пока нет assistant-сообщения) */}
                    {isLoading && (messages.length === 0 || messages[messages.length - 1].role === 'user') && (
                        <div className="w-full px-0">
                            <div className="p-4 rounded-xl border border-zinc-800/50 bg-zinc-900/40 flex items-center gap-3">
                                <Loader2 className="w-4 h-4 animate-spin text-blue-400" />
                                <span className="text-zinc-400 text-xs">{chatStatus || 'Выполнение...'}</span>
                                {currentIteration > 1 && (
                                    <span className="text-[10px] bg-zinc-800 text-zinc-500 px-1.5 py-0.5 rounded-full border border-zinc-700 font-mono">
                                        Шаг {currentIteration}
                                    </span>
                                )}
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

                    <ContextChips
                        codeContext={contextCode || modifiedCode}
                        isSelection={isContextSelection}
                        diagnostics={diagnostics}
                        configuratorCtx={configuratorTitleCtx}
                        onRemoveCode={handleRemoveCodeContext}
                    />
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

                    <div ref={dropdownRef} className="px-3 pb-2 pt-0 flex items-end gap-2 pointer-events-auto flex-nowrap w-full">
                        <div className="flex items-center gap-1.5 flex-1 min-w-0">
                            {/* Кнопка [+] (Опции) */}
                            <div className="relative">
                                <button
                                    onClick={() => setShowModelDropdown(!showModelDropdown)}
                                    className="h-8 w-12 flex items-center justify-center gap-1 rounded-xl bg-zinc-900 border border-zinc-800 text-zinc-300 hover:bg-zinc-800 transition-all active:scale-95 flex-shrink-0"
                                    title="Настройки профиля и генерации"
                                >
                                    {(() => {
                                        const behavior = settings?.code_generation?.behavior_preset;
                                        if (behavior === 'maintenance') return <HardHat className="w-4 h-4 text-orange-400" />;
                                        if (behavior === 'project') return <User className="w-4 h-4 text-blue-400" />;
                                        return <Brain className="w-4 h-4 text-blue-400" />;
                                    })()}
                                    <MoreHorizontal className="w-3.5 h-3.5 text-zinc-500" />
                                </button>

                                {showModelDropdown && (
                                    <div className="absolute bottom-full left-0 mb-2 w-64 bg-[#09090b] border border-[#27272a] rounded-xl shadow-2xl z-50 overflow-hidden py-1 animate-in slide-in-from-bottom-2 duration-200">
                                        {/* Behavior Preset Toggle (Перенесено в меню) */}
                                        {settings?.code_generation && (
                                            <>
                                                <div className="px-3 py-1.5 border-b border-[#27272a] mb-1">
                                                    <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider">Режим генерации</span>
                                                </div>
                                                <div className="px-3 py-1 flex gap-2">
                                                    <button
                                                        onClick={() => {
                                                            updateSettings({
                                                                ...settings,
                                                                code_generation: {
                                                                    ...settings.code_generation,
                                                                    behavior_preset: 'project'
                                                                }
                                                            });
                                                        }}
                                                        className={`flex-1 flex items-center justify-center gap-1.5 p-2 rounded-md text-[11px] font-bold transition-all ${settings.code_generation.behavior_preset === 'project'
                                                            ? 'bg-blue-500/15 text-blue-400 border border-blue-500/30 shadow-sm'
                                                            : 'bg-zinc-800/50 text-zinc-500 hover:bg-zinc-800'
                                                            }`}
                                                    >
                                                        <User className="w-3.5 h-3.5" /> СВОЙ
                                                    </button>
                                                    <button
                                                        onClick={() => {
                                                            updateSettings({
                                                                ...settings,
                                                                code_generation: {
                                                                    ...settings.code_generation,
                                                                    behavior_preset: 'maintenance'
                                                                }
                                                            });
                                                        }}
                                                        className={`flex-1 flex items-center justify-center gap-1.5 p-2 rounded-md text-[11px] font-bold transition-all ${settings.code_generation.behavior_preset === 'maintenance'
                                                            ? 'bg-orange-500/15 text-orange-400 border border-orange-500/30 shadow-sm'
                                                            : 'bg-zinc-800/50 text-zinc-500 hover:bg-zinc-800'
                                                            }`}
                                                    >
                                                        <HardHat className="w-3.5 h-3.5" /> ЧУЖОЙ
                                                    </button>
                                                </div>
                                            </>
                                        )}

                                        <div className="px-3 py-1.5 border-b border-[#27272a] mb-1 mt-1">
                                            <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider">Ваши профили</span>
                                        </div>
                                        <div className="max-h-[250px] overflow-y-auto custom-scrollbar">
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
                                            {profiles.filter(p => p.provider === 'QwenCli').length > 0 && (
                                                <>
                                                    <div className="px-3 py-1.5 border-b border-[#27272a] mt-1 mb-1 sticky top-0 bg-[#09090b] z-10">
                                                        <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider">CLI Провайдеры (Free)</span>
                                                    </div>
                                                    {profiles.filter(p => p.provider === 'QwenCli').map(p => {
                                                        const status = cliStatuses[p.id];
                                                        const isAuthenticated = status?.is_authenticated;
                                                        return (
                                                            <div
                                                                key={p.id}
                                                                className={`group px-3 py-2 flex items-center justify-between cursor-pointer transition-colors ${activeProfileId === p.id ? 'bg-amber-500/10' : 'hover:bg-zinc-800/50'}`}
                                                                onClick={() => {
                                                                    if (!isAuthenticated) {
                                                                        setActiveProfile(p.id);
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
                                                                                {status.usage.requests_used}{status.usage.requests_limit > 0 ? `/${status.usage.requests_limit}` : ''}
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

                            {/* Объединенный Конфигуратор & Код */}
                            <div className="relative flex-shrink-0" id="tour-get-code">
                                <button onClick={() => {
                                    const next = !showConfigDropdown;
                                    setShowConfigDropdown(next);
                                    if (next) {
                                        setShowModelDropdown(false);
                                        refreshWindows();
                                    }
                                }}
                                    className={`flex-shrink-0 flex items-center gap-1.5 text-[12px] font-medium px-2.5 h-8 rounded-xl transition-all border border-transparent ${showConfigDropdown ? 'bg-zinc-800 text-zinc-200 border-zinc-700' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
                                    title="Выбор Конфигуратора и работа с кодом"
                                >
                                    <Monitor className="w-4 h-4 text-emerald-400" />
                                    <span className="hidden sm:inline max-w-[150px] truncate block">{activeConfigTitle || 'Конфигуратор'}</span>
                                    <ChevronDown className={`w-3 h-3 transition-transform duration-200 ml-1 ${showConfigDropdown ? 'rotate-180' : ''}`} />
                                </button>

                                {showConfigDropdown && (
                                    <div className="absolute bottom-full left-0 mb-2 w-72 bg-[#1f1f23] border border-[#27272a] rounded-xl shadow-2xl z-30 ring-1 ring-black/20 flex flex-col overflow-hidden animate-in slide-in-from-bottom-2 duration-200">

                                        {/* Секция выбора окон */}
                                        <div className="px-3 py-2 border-b border-[#27272a] bg-[#09090b]">
                                            <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider flex items-center gap-1.5"><Monitor className="w-3 h-3" /> Окна конфигуратора</span>
                                        </div>
                                        <div className="max-h-[200px] overflow-y-auto custom-scrollbar p-1">
                                            {detectedWindows.length > 0 ? detectedWindows.map(w => (
                                                <button key={w.hwnd} onClick={() => { selectWindow(w.hwnd); }}
                                                    className={`w-full text-left px-3 py-2 rounded-md text-[13px] truncate transition-colors ${selectedHwnd === w.hwnd ? 'bg-emerald-500/10 text-emerald-400 font-medium' : 'text-zinc-400 hover:bg-[#27272a] hover:text-zinc-200'}`}
                                                    title={w.title}
                                                >
                                                    {parseConfiguratorTitle(w.title)}
                                                </button>
                                            )) : (
                                                <div className="px-3 py-4 text-center text-[12px] text-zinc-500">
                                                    Окна не найдены
                                                </div>
                                            )}
                                        </div>

                                        {/* Секция действий с кодом */}
                                        <div className="border-t border-[#27272a] bg-[#18181b] p-1 flex flex-col gap-0.5">
                                            <button onClick={() => { handleLoadCode(true); setShowConfigDropdown(false); }} disabled={!selectedHwnd} className="flex items-center gap-2 px-3 py-2.5 text-[12px] font-medium text-zinc-300 hover:text-white hover:bg-emerald-500/20 hover:border-emerald-500/20 border border-transparent transition-all rounded-md disabled:opacity-50 disabled:cursor-not-allowed">
                                                <FileText className="w-4 h-4 text-emerald-400 flex-shrink-0" />
                                                <span>Получить модуль целиком</span>
                                            </button>
                                            <button onClick={() => { handleLoadCode(false); setShowConfigDropdown(false); }} disabled={!selectedHwnd} className="flex items-center gap-2 px-3 py-2.5 text-[12px] font-medium text-zinc-300 hover:text-white hover:bg-emerald-500/20 hover:border-emerald-500/20 border border-transparent transition-all rounded-md disabled:opacity-50 disabled:cursor-not-allowed">
                                                <MousePointerClick className="w-4 h-4 text-emerald-400 flex-shrink-0" />
                                                <span>Получить выделенный фрагмент</span>
                                            </button>
                                        </div>

                                    </div>
                                )}
                            </div>
                        </div>

                        {/* Правый блок кнопок (зафиксирован) */}
                        <div className="flex items-center gap-1.5 flex-shrink-0">
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
                        const currentProfile = profiles.find(p => p.id === activeProfileId);
                        if (!currentProfile) return;
                        try {
                            await cliProvidersApi.saveToken(currentProfile.id, 'qwen', access_token, refresh_token, expires_at, resource_url);
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
