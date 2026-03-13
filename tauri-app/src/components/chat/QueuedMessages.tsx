import React, { useState } from 'react';
import { X, Clock, Pencil, Check } from 'lucide-react';
import { QueuedMessage } from '../../services/MessageQueueService';

interface QueuedMessagesProps {
    queue: QueuedMessage[];
    onRemove: (id: string) => void;
    onUpdate: (id: string, content: string) => void;
    onClearAll: () => void;
}

export function QueuedMessages({ queue, onRemove, onUpdate, onClearAll }: QueuedMessagesProps) {
    const [editingId, setEditingId] = useState<string | null>(null);
    const [editValue, setEditValue] = useState('');

    if (queue.length === 0) return null;

    const startEdit = (msg: QueuedMessage) => {
        setEditingId(msg.id);
        setEditValue(msg.displayContent || msg.content);
    };

    const commitEdit = (id: string) => {
        if (editValue.trim()) {
            onUpdate(id, editValue.trim());
        }
        setEditingId(null);
    };

    const cancelEdit = () => {
        setEditingId(null);
    };

    return (
        <div className="mx-2 mb-1.5 rounded-lg border border-amber-500/20 bg-amber-500/5 overflow-hidden animate-in fade-in slide-in-from-bottom-1 duration-200">
            {/* Заголовок */}
            <div className="flex items-center justify-between px-2.5 py-1.5 border-b border-amber-500/15">
                <div className="flex items-center gap-1.5">
                    <Clock className="w-3 h-3 text-amber-400/60" />
                    <span className="text-[11px] font-medium text-amber-400/70">
                        В очереди ({queue.length})
                    </span>
                </div>
                <button
                    onClick={onClearAll}
                    className="text-[10px] text-zinc-500 hover:text-zinc-300 transition-colors"
                    title="Очистить очередь"
                >
                    очистить всё
                </button>
            </div>

            {/* Список сообщений */}
            <div className="flex flex-col gap-0.5 p-1.5" role="list">
                {queue.map((msg, idx) => (
                    <div
                        key={msg.id}
                        role="listitem"
                        className="flex items-center gap-1.5 rounded-md px-2 py-1 bg-zinc-900/60 group"
                    >
                        {/* Номер */}
                        <span className="text-[10px] text-zinc-600 flex-shrink-0 w-3 text-center">
                            {idx + 1}
                        </span>

                        {/* Текст / редактор */}
                        {editingId === msg.id ? (
                            <input
                                autoFocus
                                value={editValue}
                                onChange={e => setEditValue(e.target.value)}
                                onKeyDown={e => {
                                    if (e.key === 'Enter') commitEdit(msg.id);
                                    if (e.key === 'Escape') cancelEdit();
                                }}
                                onBlur={() => commitEdit(msg.id)}
                                className="flex-1 bg-transparent text-xs text-zinc-200 outline-none border-b border-amber-500/40 pb-0.5 min-w-0"
                                aria-label="Редактировать сообщение в очереди"
                            />
                        ) : (
                            <span
                                className="flex-1 text-xs text-zinc-400 truncate min-w-0 cursor-default"
                                title={msg.displayContent || msg.content}
                                aria-label={`Сообщение в очереди: ${msg.displayContent || msg.content}`}
                            >
                                {msg.displayContent || msg.content}
                            </span>
                        )}

                        {/* Кнопки действий */}
                        {editingId === msg.id ? (
                            <button
                                onClick={() => commitEdit(msg.id)}
                                className="flex-shrink-0 text-emerald-400/70 hover:text-emerald-400 transition-colors"
                                title="Сохранить"
                            >
                                <Check className="w-3 h-3" />
                            </button>
                        ) : (
                            <button
                                onClick={() => startEdit(msg)}
                                className="flex-shrink-0 text-zinc-600 hover:text-zinc-400 transition-colors opacity-0 group-hover:opacity-100"
                                title="Редактировать"
                                aria-label="Редактировать сообщение"
                            >
                                <Pencil className="w-3 h-3" />
                            </button>
                        )}

                        <button
                            onClick={() => onRemove(msg.id)}
                            className="flex-shrink-0 text-zinc-600 hover:text-red-400 transition-colors"
                            title="Удалить из очереди"
                            aria-label="Удалить из очереди"
                        >
                            <X className="w-3 h-3" />
                        </button>
                    </div>
                ))}
            </div>
        </div>
    );
}
