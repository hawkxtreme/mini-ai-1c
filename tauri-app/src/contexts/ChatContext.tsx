import React, { createContext, useContext, useEffect, useState, useCallback } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import * as api from '../api';

export interface ToolCall {
    id: string;
    name: string;
    arguments: string;
    status: 'pending' | 'executing' | 'done' | 'error' | 'rejected';
}

export interface BSLDiagnostic {
    line: number;
    character: number;
    message: string;
    severity: 'error' | 'warning' | 'info' | 'hint';
}

export interface ChatMessage {
    id: string;
    role: 'user' | 'assistant' | 'tool';
    content: string;
    thinking?: string;
    toolCalls?: ToolCall[];
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
    sendMessage: (content: string, codeContext?: string, diagnostics?: string[]) => Promise<void>;
    stopChat: () => Promise<void>;
    clearChat: () => void;
    editAndRerun: (messageIndex: number, newContent: string, codeContext?: string, diagnostics?: string[]) => Promise<void>;
}

const ChatContext = createContext<ChatContextType | undefined>(undefined);

export function ChatProvider({ children }: { children: React.ReactNode }) {
    const [messages, setMessages] = useState<ChatMessage[]>([]);
    const [isLoading, setIsLoading] = useState(false);
    const [chatStatus, setChatStatus] = useState('');
    const [currentIteration, setCurrentIteration] = useState(0);

    useEffect(() => {
        let isMounted = true;
        let unlistenFns: UnlistenFn[] = [];

        const setupListeners = async () => {
            try {
                const results = await Promise.all([
                    listen<string>('chat-chunk', (event) => {
                        setMessages(prev => {
                            const last = prev[prev.length - 1];
                            if (last && last.role === 'assistant') {
                                return [...prev.slice(0, -1), { ...last, content: last.content + event.payload }];
                            }
                            return [...prev, { id: generateId(), role: 'assistant', content: event.payload, timestamp: Date.now() }];
                        });
                    }),
                    listen<string>('chat-thinking-chunk', (event) => {
                        setMessages(prev => {
                            const last = prev[prev.length - 1];
                            if (last && last.role === 'assistant') {
                                return [...prev.slice(0, -1), { ...last, thinking: (last.thinking || '') + event.payload }];
                            }
                            return [...prev, { id: generateId(), role: 'assistant', content: '', thinking: event.payload, timestamp: Date.now() }];
                        });
                    }),
                    listen<{ index: number, id: string, name: string }>('tool-call-started', (event) => {
                        setMessages(prev => {
                            const last = prev[prev.length - 1];
                            if (!last || last.role !== 'assistant') return prev;

                            const toolCalls = [...(last.toolCalls || [])];
                            toolCalls[event.payload.index] = {
                                id: event.payload.id,
                                name: event.payload.name,
                                arguments: '',
                                status: 'pending'
                            };

                            return [...prev.slice(0, -1), { ...last, toolCalls }];
                        });
                    }),
                    listen<{ index: number, arguments: string }>('tool-call-progress', (event) => {
                        setMessages(prev => {
                            const last = prev[prev.length - 1];
                            if (!last || last.role !== 'assistant' || !last.toolCalls) return prev;

                            const toolCalls = [...last.toolCalls];
                            if (toolCalls[event.payload.index]) {
                                toolCalls[event.payload.index] = {
                                    ...toolCalls[event.payload.index],
                                    arguments: toolCalls[event.payload.index].arguments + event.payload.arguments
                                };
                            }

                            return [...prev.slice(0, -1), { ...last, toolCalls }];
                        });
                    }),
                    listen<{ id: string, status: 'done' | 'error', result: string }>('tool-call-completed', (event) => {
                        setMessages(prev => {
                            const last = prev[prev.length - 1];
                            if (!last || last.role !== 'assistant' || !last.toolCalls) return prev;

                            // Find tool call by ID
                            const toolCalls = last.toolCalls.map(tc => {
                                if (tc.id === event.payload.id) {
                                    return { ...tc, status: event.payload.status };
                                }
                                return tc;
                            });

                            return [...prev.slice(0, -1), { ...last, toolCalls }];
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
                    listen('chat-done', () => {
                        setIsLoading(false);
                        setChatStatus('');
                        setCurrentIteration(0);
                        setMessages(prev => {
                            const last = prev[prev.length - 1];
                            if (last && last.role === 'assistant') {
                                api.saveMessage('assistant', last.content);
                            }
                            return prev;
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

    const sendMessage = useCallback(async (content: string, codeContext?: string, diagnostics?: string[]) => {
        if (!content.trim() || isLoading) return;

        // 1. UI: Show clean user message
        const userMessage: ChatMessage = { id: generateId(), role: 'user', content, timestamp: Date.now() };
        setMessages(prev => [...prev, userMessage]);
        setIsLoading(true);

        // Save clean message to history
        await api.saveMessage('user', content);

        // 2. Backend: Prepare payload
        let contextPayload = content;
        if (codeContext) {
            contextPayload += `\n\n=== CURRENT CODE CONTEXT ===\n\`\`\`bsl\n${codeContext}\n\`\`\`\n`;
            if (diagnostics && diagnostics.length > 0) {
                contextPayload += `\n=== BSL DIAGNOSTICS (for context only) ===\n${diagnostics.join('\n')}\n`;
            }
        }

        try {
            // Construct message history including the new simplified payload logic
            const payloadMessages: api.ChatMessage[] = messages.map(m => ({
                role: m.role,
                content: m.content || ''
            }));
            payloadMessages.push({ role: 'user', content: contextPayload });

            await api.streamChat(payloadMessages);
        } catch (err) {
            const errorMsg = `❌ Ошибка: ${err} `;
            setMessages(prev => [...prev, { id: generateId(), role: 'assistant', content: errorMsg, timestamp: Date.now() }]);
            await api.saveMessage('assistant', errorMsg);
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

    // Edit message and rerun from that point
    const editAndRerun = useCallback(async (messageIndex: number, newContent: string, codeContext?: string, diagnostics?: string[]) => {
        if (!newContent.trim() || isLoading) return;

        // 1. Truncate messages to the edited message
        const truncatedMessages = messages.slice(0, messageIndex);

        // 2. Update the edited message with new content
        const editedMessage: ChatMessage = {
            ...messages[messageIndex],
            content: newContent,
            timestamp: Date.now()
        };

        // 3. Set messages to truncated + edited
        setMessages([...truncatedMessages, editedMessage]);
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
            // Construct message history from truncated + edited
            const payloadMessages: api.ChatMessage[] = [...truncatedMessages, editedMessage].map(m => ({
                role: m.role,
                content: m.content || ''
            }));
            payloadMessages.push({ role: 'user', content: contextPayload });

            await api.streamChat(payloadMessages);
        } catch (err) {
            const errorMsg = `❌ Ошибка: ${err} `;
            setMessages(prev => [...prev, { id: generateId(), role: 'assistant', content: errorMsg, timestamp: Date.now() }]);
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
            editAndRerun
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
