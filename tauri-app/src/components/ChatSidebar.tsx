import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Plus, MessageSquare, Trash2, X } from 'lucide-react';

interface ChatSession {
    id: string;
    title: string;
    created_at: string;
    updated_at: string;
    messages: Array<{ role: string; content: string; timestamp: string }>;
}

interface ChatSidebarProps {
    isOpen: boolean;
    onClose: () => void;
    onSelectSession: (session: ChatSession) => void;
    activeSessionId?: string;
}

export function ChatSidebar({ isOpen, onClose, onSelectSession, activeSessionId }: ChatSidebarProps) {
    const [sessions, setSessions] = useState<ChatSession[]>([]);

    useEffect(() => {
        if (isOpen) {
            loadSessions();
        }
    }, [isOpen]);

    const loadSessions = async () => {
        const result = await invoke<ChatSession[]>('get_chat_sessions');
        setSessions(result);
    };

    const createNewChat = async () => {
        const session = await invoke<ChatSession>('create_chat');
        setSessions(prev => [session, ...prev]);
        onSelectSession(session);
    };

    const deleteSession = async (id: string, e: React.MouseEvent) => {
        e.stopPropagation();
        await invoke('delete_chat', { sessionId: id });
        setSessions(prev => prev.filter(s => s.id !== id));
    };

    const selectSession = async (session: ChatSession) => {
        const fullSession = await invoke<ChatSession>('switch_chat', { sessionId: session.id });
        onSelectSession(fullSession);
    };

    const formatDate = (dateStr: string) => {
        const date = new Date(dateStr);
        const now = new Date();
        const diff = now.getTime() - date.getTime();
        const days = Math.floor(diff / (1000 * 60 * 60 * 24));

        if (days === 0) return 'Сегодня';
        if (days === 1) return 'Вчера';
        if (days < 7) return `${days} дн. назад`;
        return date.toLocaleDateString('ru-RU');
    };

    if (!isOpen) return null;

    return (
        <div className="fixed top-10 bottom-0 left-0 w-72 bg-zinc-900 border-r border-zinc-800 flex flex-col z-40 shadow-xl">
            {/* Header */}
            <div className="flex items-center justify-between px-4 py-3 border-b border-zinc-800">
                <h2 className="font-semibold">История чатов</h2>
                <button onClick={onClose} className="p-1 hover:bg-zinc-800 rounded">
                    <X className="w-5 h-5" />
                </button>
            </div>

            {/* New Chat Button */}
            <div className="p-2">
                <button
                    onClick={createNewChat}
                    className="flex items-center justify-center gap-2 w-full px-4 py-2 bg-blue-600 hover:bg-blue-700 rounded-lg"
                >
                    <Plus className="w-4 h-4" /> Новый чат
                </button>
            </div>

            {/* Sessions List */}
            <div className="flex-1 overflow-y-auto">
                {sessions.map(session => (
                    <div
                        key={session.id}
                        onClick={() => selectSession(session)}
                        className={`group flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-zinc-800 border-b border-zinc-800/50 ${session.id === activeSessionId ? 'bg-zinc-800' : ''
                            }`}
                    >
                        <MessageSquare className="w-4 h-4 text-zinc-500 flex-shrink-0" />
                        <div className="flex-1 min-w-0">
                            <div className="text-sm truncate">{session.title || 'Новый чат'}</div>
                            <div className="text-xs text-zinc-500">{formatDate(session.updated_at)}</div>
                        </div>
                        <button
                            onClick={(e) => deleteSession(session.id, e)}
                            className="p-1 opacity-0 group-hover:opacity-100 hover:bg-red-500/20 rounded text-red-400"
                        >
                            <Trash2 className="w-4 h-4" />
                        </button>
                    </div>
                ))}
                {sessions.length === 0 && (
                    <div className="text-center text-zinc-500 py-8 text-sm">
                        Нет сохранённых чатов
                    </div>
                )}
            </div>
        </div>
    );
}
