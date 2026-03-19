import React, { createContext, useContext, useEffect, useState, useCallback, useRef } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import * as api from '../api';
import { ConfiguratorTitleContext, formatConfiguratorContextForLLM } from '../utils/configurator';
import { messageQueueService, QueuedMessage } from '../services/MessageQueueService';

export interface ToolCall {
    id: string;
    name: string;
    arguments: string;
    status: 'pending' | 'executing' | 'done' | 'error' | 'rejected';
    result?: string;
}

export interface BSLDiagnostic {
    line: number;
    character: number;
    message: string;
    severity: 'error' | 'warning' | 'info' | 'hint';
}

export interface MessagePart {
    type: 'text' | 'thinking' | 'tool';
    content?: string;
    toolCallId?: string;
}

export interface ChatMessage {
    id: string;
    role: 'user' | 'assistant' | 'tool' | 'system';
    content: string;
    displayContent?: string;
    thinking?: string;
    toolCalls?: ToolCall[];
    parts?: MessagePart[];
    diagnostics?: BSLDiagnostic[];
    timestamp: number;
    variant?: 'warning' | 'info';
}

// Helper to generate unique IDs
const generateId = () => Math.random().toString(36).substring(2, 15);

interface ChatContextType {
    messages: ChatMessage[];
    isLoading: boolean;
    chatStatus: string;
    currentIteration: number;
    messageQueue: QueuedMessage[];
    sendMessage: (content: string, codeContext?: string, diagnostics?: string[], displayContent?: string, configuratorCtx?: ConfiguratorTitleContext | null) => Promise<void>;
    stopChat: () => Promise<void>;
    clearChat: () => void;
    editAndRerun: (messageIndex: number, newContent: string, codeContext?: string, diagnostics?: string[], displayContent?: string, configuratorCtx?: ConfiguratorTitleContext | null) => Promise<void>;
    addSystemMessage: (content: string, variant?: 'warning' | 'info') => void;
    removeSystemMessage: (content: string) => void;
    injectMessage: (message: ChatMessage) => void;
    removeQueuedMessage: (id: string) => void;
    updateQueuedMessage: (id: string, content: string) => void;
    clearQueue: () => void;
}

const ChatContext = createContext<ChatContextType | undefined>(undefined);

export function ChatProvider({ children }: { children: React.ReactNode }) {
    const [messages, setMessages] = useState<ChatMessage[]>([]);
    const [isLoading, setIsLoading] = useState(false);
    const [chatStatus, setChatStatus] = useState('');
    const [currentIteration, setCurrentIteration] = useState(0);
    const [messageQueue, setMessageQueue] = useState<QueuedMessage[]>([]);
    // Маппинг index→id для tool-call-progress (сбрасывается при новом запросе)
    const currentBatchToolIds = useRef<string[]>([]);
    // Батчинг чанков: буферизуем токены и применяем setMessages не чаще 1 раза в кадр (~30fps)
    const chunkBuffer = useRef('');
    const thinkingBuffer = useRef('');
    const flushRafId = useRef<number | null>(null);

    const flushChunkBuffer = useCallback(() => {
        flushRafId.current = null;
        const text = chunkBuffer.current;
        const thinking = thinkingBuffer.current;
        if (!text && !thinking) return;
        chunkBuffer.current = '';
        thinkingBuffer.current = '';
        setMessages(prev => {
            let result = prev;
            if (text) {
                let lastAssistantIdx = -1;
                for (let i = result.length - 1; i >= 0; i--) {
                    if (result[i].role === 'user') break;
                    if (result[i].role === 'assistant') { lastAssistantIdx = i; break; }
                }
                if (lastAssistantIdx !== -1) {
                    const last = result[lastAssistantIdx];
                    const newParts = [...(last.parts || [])];
                    const lastPart = newParts[newParts.length - 1];
                    if (lastPart && lastPart.type === 'text') {
                        newParts[newParts.length - 1] = { ...lastPart, content: (lastPart.content || '') + text };
                    } else {
                        newParts.push({ type: 'text', content: text });
                    }
                    result = [...result.slice(0, lastAssistantIdx), { ...last, content: last.content + text, parts: newParts }, ...result.slice(lastAssistantIdx + 1)];
                } else {
                    result = [...result, { id: generateId(), role: 'assistant', content: text, parts: [{ type: 'text', content: text }], timestamp: Date.now() }];
                }
            }
            if (thinking) {
                let lastAssistantIdx = -1;
                for (let i = result.length - 1; i >= 0; i--) {
                    if (result[i].role === 'user') break;
                    if (result[i].role === 'assistant') { lastAssistantIdx = i; break; }
                }
                if (lastAssistantIdx !== -1) {
                    const last = result[lastAssistantIdx];
                    const newParts = [...(last.parts || [])];
                    // Find the last thinking part anywhere (not just last element) — handles interleaved reasoning/content
                    let lastThinkingIdx = -1;
                    for (let i = newParts.length - 1; i >= 0; i--) {
                        if (newParts[i].type === 'thinking') { lastThinkingIdx = i; break; }
                    }
                    if (lastThinkingIdx !== -1) {
                        newParts[lastThinkingIdx] = { ...newParts[lastThinkingIdx], content: (newParts[lastThinkingIdx].content || '') + thinking };
                    } else {
                        newParts.unshift({ type: 'thinking', content: thinking });
                    }
                    result = [...result.slice(0, lastAssistantIdx), { ...last, thinking: (last.thinking || '') + thinking, parts: newParts }, ...result.slice(lastAssistantIdx + 1)];
                } else {
                    result = [...result, { id: generateId(), role: 'assistant', content: '', thinking, parts: [{ type: 'thinking', content: thinking }], timestamp: Date.now() }];
                }
            }
            return result;
        });
    }, []);

    const scheduleFlush = useCallback(() => {
        if (flushRafId.current === null) {
            flushRafId.current = requestAnimationFrame(flushChunkBuffer);
        }
    }, [flushChunkBuffer]);

    const flushNow = useCallback(() => {
        if (flushRafId.current !== null) {
            cancelAnimationFrame(flushRafId.current);
            flushRafId.current = null;
        }
        flushChunkBuffer();
    }, [flushChunkBuffer]);

    // Подписка на изменения очереди
    useEffect(() => {
        return messageQueueService.subscribe(setMessageQueue);
    }, []);

    useEffect(() => {
        let isMounted = true;
        let unlistenFns: UnlistenFn[] = [];

        const setupListeners = async () => {
            try {
                const results = await Promise.all([
                    listen<string>('chat-chunk', (event) => {
                        chunkBuffer.current += event.payload;
                        scheduleFlush();
                    }),
                    listen<string>('chat-thinking-chunk', (event) => {
                        thinkingBuffer.current += event.payload;
                        scheduleFlush();
                    }),
                    listen<{ index: number, id: string, name: string }>('tool-call-started', (event) => {
                        flushNow();
                        setMessages(prev => {
                            const newToolCall = {
                                id: event.payload.id,
                                name: event.payload.name,
                                arguments: '',
                                status: 'pending' as const
                            };

                            // Ищем последнее assistant-сообщение, не пересекая границу хода (user-сообщение)
                            let lastAssistantIdx = -1;
                            for (let i = prev.length - 1; i >= 0; i--) {
                                if (prev[i].role === 'user') break;
                                if (prev[i].role === 'assistant') { lastAssistantIdx = i; break; }
                            }

                            if (lastAssistantIdx === -1) {
                                return [...prev, {
                                    id: generateId(),
                                    role: 'assistant',
                                    content: '',
                                    timestamp: Date.now(),
                                    toolCalls: [newToolCall],
                                    parts: [{ type: 'tool' as const, toolCallId: event.payload.id }]
                                }];
                            }

                            // Сохраняем ID в ref для tool-call-progress
                            currentBatchToolIds.current[event.payload.index] = event.payload.id;

                            const last = prev[lastAssistantIdx];
                            // Push вместо index-assign — не перезаписываем tool calls из предыдущих итераций
                            const toolCalls = [...(last.toolCalls || []), newToolCall];
                            const newParts = [...(last.parts || []), { type: 'tool' as const, toolCallId: event.payload.id }];

                            return [
                                ...prev.slice(0, lastAssistantIdx),
                                { ...last, toolCalls, parts: newParts },
                                ...prev.slice(lastAssistantIdx + 1)
                            ];
                        });
                    }),
                    listen<{ index: number, arguments: string }>('tool-call-progress', (event) => {
                        setMessages(prev => {
                            // Ищем последнее assistant-сообщение с toolCalls, не пересекая границу хода
                            let lastAssistantIdx = -1;
                            for (let i = prev.length - 1; i >= 0; i--) {
                                if (prev[i].role === 'user') break;
                                if (prev[i].role === 'assistant' && prev[i].toolCalls) { lastAssistantIdx = i; break; }
                            }
                            if (lastAssistantIdx === -1) return prev;

                            const last = prev[lastAssistantIdx];
                            const toolCalls = [...last.toolCalls!];
                            // Ищем по ID из ref (индекс — позиция в текущей итерации, не в массиве)
                            const toolId = currentBatchToolIds.current[event.payload.index];
                            const tcIdx = toolId ? toolCalls.findIndex(tc => tc.id === toolId) : -1;
                            if (tcIdx !== -1) {
                                toolCalls[tcIdx] = {
                                    ...toolCalls[tcIdx],
                                    arguments: toolCalls[tcIdx].arguments + event.payload.arguments
                                };
                            }

                            return [
                                ...prev.slice(0, lastAssistantIdx),
                                { ...last, toolCalls },
                                ...prev.slice(lastAssistantIdx + 1)
                            ];
                        });
                    }),
                    listen<{ id: string, status: 'done' | 'error', result: string }>('tool-call-completed', (event) => {
                        flushNow();
                        setMessages(prev => {
                            // Ищем assistant-сообщение с нужным tool call по ID
                            let targetIdx = -1;
                            for (let i = prev.length - 1; i >= 0; i--) {
                                if (prev[i].role === 'assistant' && prev[i].toolCalls?.some(tc => tc.id === event.payload.id)) {
                                    targetIdx = i; break;
                                }
                            }
                            // Fallback: если ID не совпал (пустой ID при анонсировании) — ищем pending
                            if (targetIdx === -1) {
                                for (let i = prev.length - 1; i >= 0; i--) {
                                    if (prev[i].role === 'assistant' && prev[i].toolCalls?.some(tc => tc.status === 'pending' || tc.status === 'executing')) {
                                        targetIdx = i; break;
                                    }
                                }
                            }
                            if (targetIdx === -1) return prev;

                            const last = prev[targetIdx];
                            let matched = false;
                            const toolCalls = last.toolCalls!.map(tc => {
                                if (tc.id === event.payload.id) {
                                    matched = true;
                                    return { ...tc, status: event.payload.status, result: event.payload.result };
                                }
                                return tc;
                            });
                            // Если не нашли по ID — обновляем первый pending
                            if (!matched) {
                                let found = false;
                                return [
                                    ...prev.slice(0, targetIdx),
                                    { ...last, toolCalls: last.toolCalls!.map(tc => {
                                        if (!found && (tc.status === 'pending' || tc.status === 'executing')) {
                                            found = true;
                                            return { ...tc, id: event.payload.id, status: event.payload.status, result: event.payload.result };
                                        }
                                        return tc;
                                    })},
                                    ...prev.slice(targetIdx + 1)
                                ];
                            }

                            return [
                                ...prev.slice(0, targetIdx),
                                { ...last, toolCalls },
                                ...prev.slice(targetIdx + 1)
                            ];
                        });
                    }),
                    listen<any>('waiting-for-approval', async () => {
                        // Auto-approve tools
                        try {
                            await api.approveTool();
                        } catch (e) {
                            console.error("Failed to auto-approve tool:", e);
                        }
                    }),
                    listen<BSLDiagnostic[]>('bsl-validation-result', (event) => {
                        setMessages(prev => {
                            const last = prev[prev.length - 1];
                            if (last && last.role === 'assistant') {
                                return [...prev.slice(0, -1), { ...last, diagnostics: event.payload }];
                            }
                            return prev;
                        });
                    }),
                    // chat-interrupt-injected: Rust подтвердил приём — сбрасываем итерацию
                    listen<string>('chat-interrupt-injected', () => {
                        flushNow();
                        currentBatchToolIds.current = [];
                        setCurrentIteration(0);
                    }),
                    listen<string>('chat-status', (event) => {
                        setChatStatus(event.payload);
                    }),
                    listen<number>('chat-iteration', (event) => {
                        setCurrentIteration(event.payload);
                    }),

                    listen('chat-done', () => {
                        flushNow();
                        setIsLoading(false);
                        setChatStatus('');
                        setCurrentIteration(0);
                        setMessages(prev => {
                            // Reset any lingering pending/executing tool calls
                            const withFixedTools = prev.map(msg =>
                                msg.toolCalls?.some(tc => tc.status === 'pending' || tc.status === 'executing')
                                    ? { ...msg, toolCalls: msg.toolCalls!.map(tc => tc.status === 'pending' || tc.status === 'executing' ? { ...tc, status: 'error' as const } : tc) }
                                    : msg
                            );
                            // Remove trailing empty assistant messages (no content, no parts, no tool calls)
                            const filtered = [...withFixedTools];
                            while (
                                filtered.length > 0 &&
                                filtered[filtered.length - 1].role === 'assistant' &&
                                !filtered[filtered.length - 1].content &&
                                (!filtered[filtered.length - 1].parts || filtered[filtered.length - 1].parts!.length === 0) &&
                                !filtered[filtered.length - 1].toolCalls?.length
                            ) {
                                filtered.pop();
                            }
                            return filtered;
                        });
                    })

                ]);

                if (!isMounted) {
                    results.forEach(fn => fn());
                    return;
                }

                unlistenFns = results;
            } catch (error) {
                console.error("Failed to setup chat listeners:", error);
            }
        };

        setupListeners();

        return () => {
            isMounted = false;
            unlistenFns.forEach(fn => fn());
            unlistenFns = [];
            if (flushRafId.current !== null) {
                cancelAnimationFrame(flushRafId.current);
                flushRafId.current = null;
            }
        };
    }, []);

    const sendMessage = useCallback(async (content: string, codeContext?: string, diagnostics?: string[], displayContent?: string, configuratorCtx?: ConfiguratorTitleContext | null) => {
        if (!content.trim()) return;

        // Если идёт генерация — пробуем инжектировать в активный agentic loop.
        // interruptChat возвращает true если loop принял сообщение (между итерациями tool calls).
        // Если false (нет активного loop / pure-text streaming) — кладём в очередь.
        if (isLoading) {
            const injected = await api.interruptChat(content);
            if (injected) {
                // Оптимистично добавляем user-сообщение в UI
                const interruptMsg: ChatMessage = {
                    id: generateId(),
                    role: 'user',
                    content,
                    displayContent,
                    parts: [{ type: 'text', content: displayContent || content }],
                    timestamp: Date.now()
                };
                setMessages(prev => [...prev, interruptMsg]);
                currentBatchToolIds.current = [];
            } else {
                // Нет активного loop — очередь (отправится после завершения текущего ответа)
                messageQueueService.enqueue({ content, displayContent, codeContext, diagnostics, configuratorCtx });
            }
            return;
        }

        // 1. UI: Show clean user message (original slash command if available)
        const userMessage: ChatMessage = {
            id: generateId(),
            role: 'user',
            content,
            displayContent,
            parts: [{ type: 'text', content: displayContent || content }],
            timestamp: Date.now()
        };
        setMessages(prev => [...prev, userMessage]);
        currentBatchToolIds.current = [];
        setIsLoading(true);

        // 2. Backend: Prepare payload
        let contextPayload = content;
        if (configuratorCtx && codeContext) {
            // Структурированный блок: SOURCE + PARSED CONTEXT + код
            contextPayload += '\n\n' + formatConfiguratorContextForLLM(configuratorCtx);
            contextPayload += `SELECTED CODE:\n\`\`\`bsl\n${codeContext}\n\`\`\`\n`;
            if (diagnostics && diagnostics.length > 0) {
                contextPayload += `\n=== BSL DIAGNOSTICS (for context only) ===\n${diagnostics.join('\n')}\n`;
            }
        } else if (configuratorCtx && !codeContext) {
            // Только контекст источника без кода (например, slash-команда со встроенным кодом)
            contextPayload += '\n\n' + formatConfiguratorContextForLLM(configuratorCtx);
        } else if (codeContext) {
            // Старый формат без контекста конфигуратора
            contextPayload += `\n\n=== CURRENT CODE CONTEXT ===\n\`\`\`bsl\n${codeContext}\n\`\`\`\n`;
            if (diagnostics && diagnostics.length > 0) {
                contextPayload += `\n=== BSL DIAGNOSTICS (for context only) ===\n${diagnostics.join('\n')}\n`;
            }
        }

        try {
            // Construct message history (system messages are UI-only, not sent to backend)
            // IMPORTANT: tool_calls + tool results must be preserved for multi-turn tool use.
            // Assistant messages with tool_calls are expanded to include synthetic tool result
            // messages so the LLM gets a valid conversation history.
            const payloadMessages: api.ChatMessage[] = messages
                .filter(m => m.role !== 'system')
                .flatMap(m => {
                    const msg: api.ChatMessage = {
                        role: m.role as 'user' | 'assistant' | 'tool',
                        content: m.content || ''
                    };
                    if (m.toolCalls && m.toolCalls.length > 0 && m.role === 'assistant') {
                        msg.tool_calls = m.toolCalls.map(tc => ({
                            id: tc.id,
                            type: 'function',
                            function: {
                                name: tc.name,
                                arguments: tc.arguments || '{}'
                            }
                        }));
                        // Inject tool result messages so the API history is valid
                        const toolResults: api.ChatMessage[] = m.toolCalls
                            .filter(tc => tc.result !== undefined && tc.id)
                            .map(tc => ({
                                role: 'tool' as const,
                                content: tc.result || '',
                                tool_call_id: tc.id,
                                name: tc.name
                            }));
                        return [msg, ...toolResults];
                    }
                    return [msg];
                });
            payloadMessages.push({ role: 'user', content: contextPayload });

            await api.streamChat(payloadMessages);
        } catch (err) {
            setMessages(prev => {
                // Reset any pending/executing tool calls to 'error' (stream died mid-tool-call)
                const withFixedTools = prev.map(msg =>
                    msg.toolCalls?.some(tc => tc.status === 'pending' || tc.status === 'executing')
                        ? { ...msg, toolCalls: msg.toolCalls!.map(tc => tc.status === 'pending' || tc.status === 'executing' ? { ...tc, status: 'error' as const } : tc) }
                        : msg
                );
                const last = withFixedTools[withFixedTools.length - 1];
                if (last && last.role === 'assistant') {
                    // Append error to the existing assistant message
                    const errorStr = `\n\n❌ **Ошибка:** ${err}`;
                    const newParts = [...(last.parts || [])];
                    const lastPart = newParts[newParts.length - 1];
                    if (lastPart && lastPart.type === 'text') {
                        newParts[newParts.length - 1] = { ...lastPart, content: (lastPart.content || '') + errorStr };
                    } else {
                        newParts.push({ type: 'text', content: errorStr });
                    }
                    return [
                        ...withFixedTools.slice(0, -1),
                        { ...last, content: last.content + errorStr, parts: newParts }
                    ];
                }
                // Fallback: create a new message
                const errorStr = `❌ Ошибка: ${err}`;
                return [...withFixedTools, { id: generateId(), role: 'assistant', content: errorStr, parts: [{ type: 'text', content: errorStr }], timestamp: Date.now() }];
            });
            setIsLoading(false);
        }
    }, [isLoading, messages]);

    // Дренирование очереди: срабатывает когда isLoading переходит false
    // useEffect гарантирует что sendMessage уже видит isLoading=false
    const prevIsLoadingRef = useRef(false);
    useEffect(() => {
        if (prevIsLoadingRef.current && !isLoading && !messageQueueService.isEmpty) {
            const next = messageQueueService.dequeue();
            if (next) {
                sendMessage(next.content, next.codeContext, next.diagnostics, next.displayContent, next.configuratorCtx);
            }
        }
        prevIsLoadingRef.current = isLoading;
    }, [isLoading, sendMessage]);

    const stopChat = useCallback(async () => {
        try {
            await api.stopChat();
            setIsLoading(false);
            setChatStatus('Stopped');
        } catch (e) {
            console.error("Failed to stop chat:", e);
        }
    }, []);

    const clearChat = useCallback(() => {
        setMessages([]);
        setChatStatus('');
        setIsLoading(false);
        // Reset Naparnik conversation session if provider is OneCNaparnik
        api.clearNaparnikSession().catch(() => {/* non-critical */});
    }, []);

    const addSystemMessage = useCallback((content: string, variant?: 'warning' | 'info') => {
        setMessages(prev => [
            ...prev,
            { id: generateId(), role: 'system', content, parts: [{ type: 'text', content }], timestamp: Date.now(), variant: variant ?? 'warning' }
        ]);
    }, []);

    const removeSystemMessage = useCallback((content: string) => {
        setMessages(prev => prev.filter(m => !(m.role === 'system' && m.content === content)));
    }, []);

    const injectMessage = useCallback((message: ChatMessage) => {
        setMessages(prev => [...prev, { id: generateId(), ...message }]);
    }, []);

    // Edit message and rerun from that point
    const editAndRerun = useCallback(async (messageIndex: number, newContent: string, codeContext?: string, diagnostics?: string[], displayContent?: string, configuratorCtx?: ConfiguratorTitleContext | null) => {
        if (!newContent.trim() || isLoading) return;

        // 1. Truncate messages to the edited message
        const truncatedMessages = messages.slice(0, messageIndex);

        // 2. Update the edited message with new content
        const editedMessage: ChatMessage = {
            ...messages[messageIndex],
            content: newContent,
            displayContent,
            parts: [{ type: 'text', content: displayContent || newContent }],
            timestamp: Date.now()
        };

        // 3. Set messages to truncated + edited
        setMessages([...truncatedMessages, editedMessage]);
        currentBatchToolIds.current = [];
        setIsLoading(true);

        // 4. Prepare payload
        let contextPayload = newContent;
        if (configuratorCtx && codeContext) {
            contextPayload += '\n\n' + formatConfiguratorContextForLLM(configuratorCtx);
            contextPayload += `SELECTED CODE:\n\`\`\`bsl\n${codeContext}\n\`\`\`\n`;
            if (diagnostics && diagnostics.length > 0) {
                contextPayload += `\n=== BSL DIAGNOSTICS (for context only) ===\n${diagnostics.join('\n')}\n`;
            }
        } else if (codeContext) {
            contextPayload += `\n\n=== CURRENT CODE CONTEXT ===\n\`\`\`bsl\n${codeContext}\n\`\`\`\n`;
            if (diagnostics && diagnostics.length > 0) {
                contextPayload += `\n=== BSL DIAGNOSTICS (for context only) ===\n${diagnostics.join('\n')}\n`;
            }
        }

        try {
            // Construct message history from truncated + edited (filter system/UI messages)
            const payloadMessages: api.ChatMessage[] = [...truncatedMessages, editedMessage]
                .filter(m => m.role !== 'system')
                .flatMap(m => {
                    const msg: api.ChatMessage = {
                        role: m.role as 'user' | 'assistant' | 'tool',
                        content: m.content || ''
                    };
                    if (m.toolCalls && m.toolCalls.length > 0 && m.role === 'assistant') {
                        msg.tool_calls = m.toolCalls.map(tc => ({
                            id: tc.id,
                            type: 'function',
                            function: {
                                name: tc.name,
                                arguments: tc.arguments || '{}'
                            }
                        }));
                        const toolResults: api.ChatMessage[] = m.toolCalls
                            .filter(tc => tc.result !== undefined && tc.id)
                            .map(tc => ({
                                role: 'tool' as const,
                                content: tc.result || '',
                                tool_call_id: tc.id,
                                name: tc.name
                            }));
                        return [msg, ...toolResults];
                    }
                    return [msg];
                });
            payloadMessages.push({ role: 'user', content: contextPayload });

            await api.streamChat(payloadMessages);
        } catch (err) {
            setMessages(prev => {
                // Reset any pending/executing tool calls to 'error' (stream died mid-tool-call)
                const withFixedTools = prev.map(msg =>
                    msg.toolCalls?.some(tc => tc.status === 'pending' || tc.status === 'executing')
                        ? { ...msg, toolCalls: msg.toolCalls!.map(tc => tc.status === 'pending' || tc.status === 'executing' ? { ...tc, status: 'error' as const } : tc) }
                        : msg
                );
                const last = withFixedTools[withFixedTools.length - 1];
                if (last && last.role === 'assistant') {
                    // Append error to the existing assistant message
                    const errorStr = `\n\n❌ **Ошибка:** ${err}`;
                    const newParts = [...(last.parts || [])];
                    const lastPart = newParts[newParts.length - 1];
                    if (lastPart && lastPart.type === 'text') {
                        newParts[newParts.length - 1] = { ...lastPart, content: (lastPart.content || '') + errorStr };
                    } else {
                        newParts.push({ type: 'text', content: errorStr });
                    }
                    return [
                        ...withFixedTools.slice(0, -1),
                        { ...last, content: last.content + errorStr, parts: newParts }
                    ];
                }
                const errorMsg = `❌ Ошибка: ${err} `;
                return [...withFixedTools, { id: generateId(), role: 'assistant', content: errorMsg, parts: [{ type: 'text', content: errorMsg }], timestamp: Date.now() }];
            });
            setIsLoading(false);
        }
    }, [isLoading, messages]);

    const removeQueuedMessage = useCallback((id: string) => {
        messageQueueService.remove(id);
    }, []);

    const updateQueuedMessage = useCallback((id: string, content: string) => {
        messageQueueService.update(id, content);
    }, []);

    const clearQueue = useCallback(() => {
        messageQueueService.clear();
    }, []);

    return (
        <ChatContext.Provider value={{
            messages,
            isLoading,
            chatStatus,
            currentIteration,
            messageQueue,
            sendMessage,
            stopChat,
            clearChat,
            editAndRerun,
            addSystemMessage,
            removeSystemMessage,
            injectMessage,
            removeQueuedMessage,
            updateQueuedMessage,
            clearQueue,
        }}>
            {children}
        </ChatContext.Provider>
    );
}

export function useChat() {
    const context = useContext(ChatContext);
    if (context === undefined) {
        throw new Error('useChat must be used within a ChatProvider');
    }
    return context;
}
