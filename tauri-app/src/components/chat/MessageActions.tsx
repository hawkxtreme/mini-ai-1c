import { useState } from 'react';
import { Copy, Check, Clock, Pencil } from 'lucide-react';

interface MessageActionsProps {
    content: string;
    timestamp: number;
    isUser?: boolean;
    onEdit?: () => void;
}

export function MessageActions({ content, timestamp, isUser = false, onEdit }: MessageActionsProps) {
    const [copied, setCopied] = useState(false);

    const handleCopy = async () => {
        try {
            await navigator.clipboard.writeText(content);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        } catch (err) {
            console.error('Failed to copy:', err);
        }
    };

    const formatTime = (ts: number) => {
        const date = new Date(ts);
        const now = new Date();
        const diffMs = now.getTime() - date.getTime();
        const diffMins = Math.floor(diffMs / 60000);

        // Относительное время для недавних
        if (diffMins < 1) {
            return 'только что';
        } else if (diffMins < 5) {
            return `${diffMins} мин назад`;
        }

        // Полное время
        return date.toLocaleTimeString('ru-RU', {
            hour: '2-digit',
            minute: '2-digit'
        });
    };

    const fullTime = new Date(timestamp).toLocaleString('ru-RU', {
        day: '2-digit',
        month: '2-digit',
        year: 'numeric',
        hour: '2-digit',
        minute: '2-digit'
    });

    return (
        <div className="flex items-center gap-2 transition-opacity duration-200">
            {/* Timestamp */}
            <span
                className="text-[10px] text-zinc-600 flex items-center gap-1"
                title={fullTime}
            >
                <Clock size={10} className="text-zinc-500" />
                {formatTime(timestamp)}
            </span>

            {/* Edit button (only for user messages) */}
            {isUser && onEdit && (
                <button
                    onClick={onEdit}
                    className="p-1 rounded hover:bg-zinc-800 transition-colors"
                    title="Редактировать"
                >
                    <Pencil size={12} className="text-zinc-500 hover:text-zinc-300" />
                </button>
            )}

            {/* Copy button */}
            <button
                onClick={handleCopy}
                className="p-1 rounded hover:bg-zinc-800 transition-colors"
                title={copied ? 'Скопировано!' : 'Копировать'}
            >
                {copied ? (
                    <Check size={12} className="text-green-400" />
                ) : (
                    <Copy size={12} className="text-zinc-500 hover:text-zinc-300" />
                )}
            </button>
        </div>
    );
}
