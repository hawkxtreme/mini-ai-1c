import React, { createContext, useContext, useEffect, useState } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import * as api from '../api';

export interface ChatMessage {
    role: 'user' | 'assistant';
    content: string;
}

interface ChatContextType {
    messages: ChatMessage[];
    isLoading: boolean;
    chatStatus: string;
    sendMessage: (content: string, codeContext?: string, diagnostics?: string[]) => Promise<void>;
    stopChat: () => Promise<void>;
    clearChat: () => void;
}

const ChatContext = createContext<ChatContextType | undefined>(undefined);

export function ChatProvider({ children }: { children: React.ReactNode }) {
    const [messages, setMessages] = useState<ChatMessage[]>([]);
    const [isLoading, setIsLoading] = useState(false);
    const [chatStatus, setChatStatus] = useState('');

    useEffect(() => {
        let unlistenChunk: Promise<UnlistenFn>;
        let unlistenStatus: Promise<UnlistenFn>;
        let unlistenDone: Promise<UnlistenFn>;

        const setupListeners = async () => {
            unlistenChunk = listen<string>('chat-chunk', (event) => {
                setMessages(prev => {
                    const last = prev[prev.length - 1];
                    if (last && last.role === 'assistant') {
                        return [...prev.slice(0, -1), { ...last, content: last.content + event.payload }];
                    }
                    return [...prev, { role: 'assistant', content: event.payload }];
                });
            });

            unlistenStatus = listen<string>('chat-status', (event) => {
                setChatStatus(event.payload);
            });

            unlistenDone = listen('chat-done', () => {
                setIsLoading(false);
                setChatStatus('');
                setMessages(prev => {
                    const last = prev[prev.length - 1];
                    if (last && last.role === 'assistant') {
                        api.saveMessage('assistant', last.content);
                    }
                    return prev;
                });
            });
        };

        setupListeners();

        return () => {
            unlistenChunk?.then(fn => fn());
            unlistenStatus?.then(fn => fn());
            unlistenDone?.then(fn => fn());
        };
    }, []);

    const sendMessage = async (content: string, codeContext?: string, diagnostics?: string[]) => {
        if (!content.trim() || isLoading) return;

        // 1. UI: Show clean user message
        const userMessage: ChatMessage = { role: 'user', content };
        setMessages(prev => [...prev, userMessage]);
        setIsLoading(true);

        // Save clean message to history
        await api.saveMessage('user', content);

        // 2. Backend: Prepare payload
        let contextPayload = content;
        if (codeContext) {
            contextPayload += `\n\n=== CURRENT CODE CONTEXT ===\n\`\`\`bsl\n${codeContext}\n\`\`\`\n`;
            if (diagnostics && diagnostics.length > 0) {
                contextPayload += `\n=== DETECTED ERRORS ===\n${diagnostics.join('\n')}\n`;
                contextPayload += `\nPlease fix these errors in the code.`;
            }
        }

        try {
            // Construct message history including the new simplified payload logic
            const payloadMessages = messages.map(m => ({ role: m.role, content: m.content }));
            payloadMessages.push({ role: 'user', content: contextPayload });

            await api.streamChat(payloadMessages);
        } catch (err) {
            const errorMsg = `❌ Ошибка: ${err} `;
            setMessages(prev => [...prev, { role: 'assistant', content: errorMsg }]);
            await api.saveMessage('assistant', errorMsg);
            setIsLoading(false);
        }
    };

    const stopChat = async () => {
        try {
            await api.stopChat();
            setIsLoading(false);
            setChatStatus('Stopped');
        } catch (e) {
            console.error("Failed to stop chat:", e);
        }
    };

    const clearChat = () => {
        setMessages([]);
        setChatStatus('');
        setIsLoading(false);
    };

    return (
        <ChatContext.Provider value={{ messages, isLoading, chatStatus, sendMessage, stopChat, clearChat }}>
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
