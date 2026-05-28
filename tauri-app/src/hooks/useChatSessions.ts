import { useState, useEffect, useCallback } from 'react';
import { ChatMessage } from '../contexts/ChatContext';

export interface ChatSession {
    id: string;
    title: string;
    createdAt: number;
    updatedAt: number;
    messages: ChatMessage[];
}

const STORAGE_KEY = 'chat_sessions';
const ACTIVE_KEY = 'active_session_id';
const MAX_SESSIONS = 50;

function generateId(): string {
    return Date.now().toString(36) + Math.random().toString(36).slice(2);
}

function createEmptySession(id: string = generateId()): ChatSession {
    const now = Date.now();
    return {
        id,
        title: 'Новый чат',
        createdAt: now,
        updatedAt: now,
        messages: [],
    };
}

function getTitle(messages: ChatMessage[]): string {
    const first = messages.find(m => m.role === 'user');
    if (!first) return 'Новый чат';
    const text = first.displayContent || first.content;
    return text.slice(0, 40) + (text.length > 40 ? '…' : '');
}

function loadSessions(): ChatSession[] {
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        return raw ? JSON.parse(raw) : [];
    } catch { return []; }
}

function saveSessions(sessions: ChatSession[]): void {
    try { localStorage.setItem(STORAGE_KEY, JSON.stringify(sessions)); } catch {}
}

export function useChatSessions() {
    const [sessions, setSessions] = useState<ChatSession[]>(() => loadSessions());
    const [activeId, setActiveId] = useState<string | null>(
        () => localStorage.getItem(ACTIVE_KEY)
    );

    useEffect(() => { saveSessions(sessions); }, [sessions]);
    useEffect(() => {
        if (activeId) localStorage.setItem(ACTIVE_KEY, activeId);
        else localStorage.removeItem(ACTIVE_KEY);
    }, [activeId]);

    const activeSession = sessions.find(s => s.id === activeId) ?? null;

    const createSession = useCallback((initialMessages: ChatMessage[] = []): string => {
        const now = Date.now();
        const id = generateId();
        const newSession: ChatSession = {
            id,
            title: getTitle(initialMessages),
            createdAt: now,
            updatedAt: now,
            messages: initialMessages,
        };
        setSessions(prev => {
            const updated = [newSession, ...prev];
            return updated.length > MAX_SESSIONS ? updated.slice(0, MAX_SESSIONS) : updated;
        });
        setActiveId(newSession.id);
        return newSession.id;
    }, []);

    const switchSession = useCallback((id: string) => {
        setActiveId(id);
    }, []);

    const startDraft = useCallback(() => {
        setActiveId(null);
    }, []);

    const deleteSession = useCallback((id: string) => {
        setSessions(prev => {
            const remaining = prev.filter(s => s.id !== id);
            setActiveId(currentActiveId => {
                if (currentActiveId !== id) return currentActiveId;
                return remaining[0]?.id ?? null;
            });
            return remaining;
        });
    }, []);

    const updateSessionMessages = useCallback((id: string | null, messages: ChatMessage[]) => {
        if (!id) return;
        setSessions(prev => prev.map(s => {
            if (s.id !== id) return s;
            return {
                ...s,
                messages,
                title: messages.length > 0 ? getTitle(messages) : s.title,
                updatedAt: Date.now(),
            };
        }));
    }, []);

    return {
        sessions,
        activeId,
        activeSession,
        createSession,
        switchSession,
        startDraft,
        deleteSession,
        updateSessionMessages,
    };
}
