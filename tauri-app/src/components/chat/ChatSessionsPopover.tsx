import { Download, MessageSquarePlus, Search, Trash2 } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { ChatSession } from '../../hooks/useChatSessions';
import { useSettings } from '../../contexts/SettingsContext';

interface Props {
    sessions: ChatSession[];
    activeId: string | null;
    isOpen: boolean;
    onClose: () => void;
    onSwitch: (id: string) => void;
    onNew: () => void;
    onDelete: (id: string) => void;
    onExportSession: (session: ChatSession) => void | Promise<void>;
}

function formatRelativeTime(ts: number): string {
    const diffMs = Math.max(0, Date.now() - ts);
    const diffMinutes = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffMinutes < 1) return 'сейчас';
    if (diffMinutes < 60) return `${diffMinutes}м`;
    if (diffHours < 24) return `${diffHours}ч`;
    return `${Math.max(1, diffDays)}д`;
}

export function ChatSessionsPopover({
    sessions,
    activeId,
    isOpen,
    onClose,
    onSwitch,
    onNew,
    onDelete,
    onExportSession,
}: Props) {
    const [query, setQuery] = useState('');
    const searchRef = useRef<HTMLInputElement | null>(null);
    const { settings } = useSettings();
    const isLightTheme = settings?.theme === 'light';

    useEffect(() => {
        if (!isOpen) {
            setQuery('');
            return;
        }

        const focusTimer = window.setTimeout(() => {
            searchRef.current?.focus();
        }, 0);

        const handleKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                onClose();
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => {
            window.clearTimeout(focusTimer);
            window.removeEventListener('keydown', handleKeyDown);
        };
    }, [isOpen, onClose]);

    const filteredSessions = useMemo(() => {
        const normalizedQuery = query.trim().toLowerCase();
        if (!normalizedQuery) {
            return sessions;
        }

        return sessions.filter((session) =>
            session.title.toLowerCase().includes(normalizedQuery),
        );
    }, [query, sessions]);

    const newChatButtonClassName = isLightTheme
        ? 'inline-flex items-center gap-2 rounded-xl border border-blue-300 bg-blue-50 px-3 py-2 text-xs font-semibold text-blue-700 shadow-sm transition-colors hover:bg-blue-100'
        : 'inline-flex items-center gap-2 rounded-xl border border-blue-500/30 bg-blue-500/10 px-3 py-2 text-xs font-medium text-blue-200 transition-colors hover:bg-blue-500/20';

    if (!isOpen) {
        return null;
    }

    return (
        <div
            data-testid="chat-history-popover"
            className="absolute right-0 top-full z-50 mt-2 w-[24rem] max-w-[calc(100vw-1rem)] overflow-hidden rounded-2xl border border-zinc-700 bg-zinc-900/95 shadow-2xl backdrop-blur-xl"
        >
            <div className="border-b border-zinc-800/80 px-3 py-3">
                <div className="mb-3 flex items-center justify-between gap-3">
                    <div>
                        <div className="text-xs font-semibold uppercase tracking-[0.18em] text-zinc-500">
                            История чатов
                        </div>
                        <div className="mt-1 text-sm text-zinc-300">
                            {sessions.length === 0 ? 'Нет сохранённых диалогов' : `${sessions.length} сохранённых чатов`}
                        </div>
                    </div>
                    <button
                        data-testid="chat-history-new"
                        onClick={() => {
                            onNew();
                            onClose();
                        }}
                        className={newChatButtonClassName}
                        title="Новый чат"
                    >
                        <MessageSquarePlus className="h-3.5 w-3.5" />
                        Новый чат
                    </button>
                </div>

                <label className="relative block">
                    <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-zinc-500" />
                    <input
                        ref={searchRef}
                        data-testid="chat-history-search"
                        type="text"
                        value={query}
                        onChange={(event) => setQuery(event.target.value)}
                        placeholder="Поиск по истории..."
                        className="w-full rounded-xl border border-zinc-800 bg-zinc-950/70 py-2.5 pl-9 pr-3 text-sm text-zinc-200 outline-none transition-colors placeholder:text-zinc-600 focus:border-zinc-700 focus:bg-zinc-950"
                    />
                </label>
            </div>

            <div className="max-h-[22rem] overflow-y-auto px-2 py-2">
                {filteredSessions.length === 0 && (
                    <div className="rounded-xl border border-dashed border-zinc-800 px-4 py-8 text-center text-sm text-zinc-500">
                        {query.trim() ? 'Ничего не найдено' : 'Создайте первый чат'}
                    </div>
                )}

                {filteredSessions.map((session) => {
                    const isActive = session.id === activeId;
                    const canExportSession = session.messages.some(
                        (message) => (message.role === 'user' || message.role === 'assistant') && message.variant == null
                    );

                    return (
                        <div
                            key={session.id}
                            role="button"
                            tabIndex={0}
                            data-testid={`chat-history-item-${session.id}`}
                            onClick={() => {
                                onSwitch(session.id);
                                onClose();
                            }}
                            onKeyDown={(event) => {
                                if (event.key === 'Enter' || event.key === ' ') {
                                    event.preventDefault();
                                    onSwitch(session.id);
                                    onClose();
                                }
                            }}
                            className={`group mb-1 flex w-full items-start gap-3 rounded-xl px-3 py-3 text-left transition-colors ${
                                isActive
                                    ? 'bg-sky-500/15 ring-1 ring-inset ring-sky-500/20'
                                    : 'hover:bg-zinc-800/70'
                            }`}
                        >
                            <div className="min-w-0 flex-1">
                                <div
                                    className={`truncate text-sm font-medium ${
                                        isActive ? 'text-zinc-100' : 'text-zinc-300'
                                    }`}
                                >
                                    {session.title}
                                </div>
                                <div className="mt-1 text-xs text-zinc-500">
                                    {formatRelativeTime(session.updatedAt)}
                                </div>
                            </div>

                            {canExportSession && (
                                <button
                                    type="button"
                                    data-testid={`chat-history-export-${session.id}`}
                                    onClick={(event) => {
                                        event.stopPropagation();
                                        void onExportSession(session);
                                    }}
                                    className={`mt-0.5 rounded-md p-1 text-zinc-500 transition-all hover:bg-zinc-800 hover:text-sky-300 ${
                                        isActive ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
                                    }`}
                                    title="Экспортировать чат"
                                >
                                    <Download className="h-3.5 w-3.5" />
                                </button>
                            )}

                            <button
                                type="button"
                                data-testid={`chat-history-delete-${session.id}`}
                                onClick={(event) => {
                                    event.stopPropagation();
                                    onDelete(session.id);
                                }}
                                className="mt-0.5 rounded-md p-1 text-zinc-600 opacity-0 transition-all hover:bg-zinc-800 hover:text-red-300 group-hover:opacity-100"
                                title="Удалить чат"
                            >
                                <Trash2 className="h-3.5 w-3.5" />
                            </button>
                        </div>
                    );
                })}
            </div>
        </div>
    );
}
