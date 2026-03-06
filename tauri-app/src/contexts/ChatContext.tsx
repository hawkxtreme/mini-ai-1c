import React, { createContext, useContext, useEffect, useState, useCallback, useRef } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import * as api from '../api';

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
}

// Helper to generate unique IDs
const generateId = () => Math.random().toString(36).substring(2, 15);

interface ChatContextType {
    messages: ChatMessage[];
    isLoading: boolean;
    chatStatus: string;
    currentIteration: number;
    sendMessage: (content: string, codeContext?: string, diagnostics?: string[], displayContent?: string) => Promise<void>;
    stopChat: () => Promise<void>;
    clearChat: () => void;
    editAndRerun: (messageIndex: number, newContent: string, codeContext?: string, diagnostics?: string[], displayContent?: string) => Promise<void>;
    addSystemMessage: (content: string) => void;
}

const ChatContext = createContext<ChatContextType | undefined>(undefined);

export function ChatProvider({ children }: { children: React.ReactNode }) {
    const [messages, setMessages] = useState<ChatMessage[]>([]);
    const [isLoading, setIsLoading] = useState(false);
    const [chatStatus, setChatStatus] = useState('');
    const [currentIteration, setCurrentIteration] = useState(0);
    // Маппинг index→id для tool-call-progress (сбрасывается при новом запросе)
    const currentBatchToolIds = useRef<string[]>([]);

    useEffect(() => {
        let isMounted = true;
        let unlistenFns: UnlistenFn[] = [];

        const setupListeners = async () => {
            try {
                const results = await Promise.all([
                    listen<string>('chat-chunk', (event) => {
                        setMessages(prev => {
                            // Ищем последнее assistant-сообщение, не пересекая границу хода (user-сообщение)
                            let lastAssistantIdx = -1;
                            for (let i = prev.length - 1; i >= 0; i--) {
                                if (prev[i].role === 'user') break;
                                if (prev[i].role === 'assistant') { lastAssistantIdx = i; break; }
                            }

                            if (lastAssistantIdx !== -1) {
                                const last = prev[lastAssistantIdx];
                                const newParts = [...(last.parts || [])];
                                const lastPart = newParts[newParts.length - 1];
                                if (lastPart && lastPart.type === 'text') {
                                    newParts[newParts.length - 1] = { ...lastPart, content: (lastPart.content || '') + event.payload };
                                } else {
                                    newParts.push({ type: 'text', content: event.payload });
                                }
                                return [
                                    ...prev.slice(0, lastAssistantIdx),
                                    { ...last, content: last.content + event.payload, parts: newParts },
                                    ...prev.slice(lastAssistantIdx + 1)
                                ];
                            }
                            return [...prev, { id: generateId(), role: 'assistant', content: event.payload, parts: [{ type: 'text', content: event.payload }], timestamp: Date.now() }];
                        });
                    }),
                    listen<string>('chat-thinking-chunk', (event) => {
                        setMessages(prev => {
                            // Ищем последнее assistant-сообщение, не пересекая границу хода (user-сообщение)
                            let lastAssistantIdx = -1;
                            for (let i = prev.length - 1; i >= 0; i--) {
                                if (prev[i].role === 'user') break;
                                if (prev[i].role === 'assistant') { lastAssistantIdx = i; break; }
                            }

                            if (lastAssistantIdx !== -1) {
                                const last = prev[lastAssistantIdx];
                                const newParts = [...(last.parts || [])];
                                const lastPart = newParts[newParts.length - 1];
                                if (lastPart && lastPart.type === 'thinking') {
                                    newParts[newParts.length - 1] = { ...lastPart, content: (lastPart.content || '') + event.payload };
                                } else {
                                    newParts.push({ type: 'thinking', content: event.payload });
                                }
                                return [
                                    ...prev.slice(0, lastAssistantIdx),
                                    { ...last, thinking: (last.thinking || '') + event.payload, parts: newParts },
                                    ...prev.slice(lastAssistantIdx + 1)
                                ];
                            }
                            return [...prev, { id: generateId(), role: 'assistant', content: '', thinking: event.payload, parts: [{ type: 'thinking', content: event.payload }], timestamp: Date.now() }];
                        });
                    }),
                    listen<{ index: number, id: string, name: string }>('tool-call-started', (event) => {
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
                        setMessages(prev => {
                            // Ищем assistant-сообщение с нужным tool call по ID
                            let targetIdx = -1;
                            for (let i = prev.length - 1; i >= 0; i--) {
                                if (prev[i].role === 'assistant' && prev[i].toolCalls?.some(tc => tc.id === event.payload.id)) {
                                    targetIdx = i; break;
                                }
                            }
                            if (targetIdx === -1) return prev;

                            const last = prev[targetIdx];
                            const toolCalls = last.toolCalls!.map(tc => {
                                if (tc.id === event.payload.id) {
                                    return { ...tc, status: event.payload.status, result: event.payload.result };
                                }
                                return tc;
                            });

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
                    listen<string>('chat-status', (event) => {
                        setChatStatus(event.payload);
                    }),
                    listen<number>('chat-iteration', (event) => {
                        setCurrentIteration(event.payload);
                    }),
                    // New iteration started (e.g. planning → execution phase transition)
                    // Create a fresh assistant message block so Шаг 2 doesn't append to Шаг 1
                    listen('chat-new-iteration', () => {
                        setMessages(prev => [
                            ...prev,
                            {
                                id: generateId(),
                                role: 'assistant',
                                content: '',
                                parts: [],
                                timestamp: Date.now()
                            }
                        ]);
                    }),
                    listen('chat-done', () => {
                        setIsLoading(false);
                        setChatStatus('');
                        setCurrentIteration(0);
                        setMessages(prev => {
                            // Reset any lingering pending/executing tool calls (stream died before tool-call-completed)
                            const withFixedTools = prev.map(msg =>
                                msg.toolCalls?.some(tc => tc.status === 'pending' || tc.status === 'executing')
                                    ? { ...msg, toolCalls: msg.toolCalls!.map(tc => tc.status === 'pending' || tc.status === 'executing' ? { ...tc, status: 'error' as const } : tc) }
                                    : msg
                            );
                            // Remove trailing empty assistant messages (created by chat-new-iteration
                            // but not filled if model produced no output in that phase)
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
        };
    }, []);

    const sendMessage = useCallback(async (content: string, codeContext?: string, diagnostics?: string[], displayContent?: string) => {
        if (!content.trim() || isLoading) return;

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
        if (codeContext) {
            contextPayload += `\n\n=== CURRENT CODE CONTEXT ===\n\`\`\`bsl\n${codeContext}\n\`\`\`\n`;
            if (diagnostics && diagnostics.length > 0) {
                contextPayload += `\n=== BSL DIAGNOSTICS (for context only) ===\n${diagnostics.join('\n')}\n`;
            }
        }

        try {
            // Construct message history (system messages are UI-only, not sent to backend)
            // IMPORTANT: tool_calls, tool_call_id and name must be preserved for LLM to
            // correctly understand the history of tool usage in subsequent turns.
            const payloadMessages: api.ChatMessage[] = messages
                .filter(m => m.role !== 'system')
                .map(m => {
                    const msg: api.ChatMessage = {
                        role: m.role as 'user' | 'assistant' | 'tool',
                        content: m.content || ''
                    };
                    // Preserve tool call history for proper LLM context
                    if (m.toolCalls && m.toolCalls.length > 0 && m.role === 'assistant') {
                        msg.tool_calls = m.toolCalls.map(tc => ({
                            id: tc.id,
                            type: 'function',
                            function: {
                                name: tc.name,
                                arguments: tc.arguments || '{}'
                            }
                        }));
                    }
                    // For tool role messages, restore tool_call_id and name
                    // This info is stored in the message content via the backend loop,
                    // but for history reconstruction, we rely on UI state if available.
                    return msg;
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
    }, []);

    const addSystemMessage = useCallback((content: string) => {
        setMessages(prev => [
            ...prev,
            { id: generateId(), role: 'system', content, parts: [{ type: 'text', content }], timestamp: Date.now() }
        ]);
    }, []);

    // Edit message and rerun from that point
    const editAndRerun = useCallback(async (messageIndex: number, newContent: string, codeContext?: string, diagnostics?: string[], displayContent?: string) => {
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
        if (codeContext) {
            contextPayload += `\n\n=== CURRENT CODE CONTEXT ===\n\`\`\`bsl\n${codeContext}\n\`\`\`\n`;
            if (diagnostics && diagnostics.length > 0) {
                contextPayload += `\n=== BSL DIAGNOSTICS (for context only) ===\n${diagnostics.join('\n')}\n`;
            }
        }

        try {
            // Construct message history from truncated + edited (filter system/UI messages)
            const payloadMessages: api.ChatMessage[] = [...truncatedMessages, editedMessage]
                .filter(m => m.role !== 'system')
                .map(m => {
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
                    }
                    return msg;
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

    return (
        <ChatContext.Provider value={{
            messages,
            isLoading,
            chatStatus,
            currentIteration,
            sendMessage,
            stopChat,
            clearChat,
            editAndRerun,
            addSystemMessage
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
