import { useState } from 'react';
import { Pencil, X, Play } from 'lucide-react';

interface EditableMessageProps {
    content: string;
    isUser: boolean;
    onEdit: (newContent: string) => void;
}

export function EditableMessage({ content, isUser, onEdit }: EditableMessageProps) {
    const [isEditing, setIsEditing] = useState(false);
    const [editText, setEditText] = useState(content);

    const handleSave = () => {
        if (editText.trim() && editText !== content) {
            onEdit(editText);
        }
        setIsEditing(false);
    };

    const handleCancel = () => {
        setEditText(content);
        setIsEditing(false);
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
            handleSave();
        } else if (e.key === 'Escape') {
            handleCancel();
        }
    };

    if (isEditing) {
        return (
            <div className="w-full">
                <textarea
                    value={editText}
                    onChange={(e) => setEditText(e.target.value)}
                    onKeyDown={handleKeyDown}
                    className="w-full bg-zinc-800 border border-zinc-700 rounded-lg p-3 text-zinc-300 text-[13px] font-sans resize-none focus:outline-none focus:border-blue-500/50 transition-colors"
                    rows={Math.min(10, Math.max(3, editText.split('\n').length))}
                    autoFocus
                />
                <div className="flex justify-end gap-2 mt-2">
                    <button
                        onClick={handleCancel}
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[11px] font-medium bg-zinc-800 text-zinc-400 hover:text-white hover:bg-zinc-700 transition-all"
                    >
                        <X size={14} />
                        Отмена
                    </button>
                    <button
                        onClick={handleSave}
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[11px] font-medium bg-blue-600 text-white hover:bg-blue-500 transition-all"
                    >
                        <Play size={14} />
                        Сохранить и перезапустить
                    </button>
                </div>
            </div>
        );
    }

    return null;
}

// Edit button component to be shown on hover
export function EditButton({ onClick }: { onClick: () => void }) {
    return (
        <button
            onClick={onClick}
            className="p-1 rounded hover:bg-zinc-800 transition-colors opacity-0 group-hover:opacity-100"
            title="Редактировать"
        >
            <Pencil size={12} className="text-zinc-500 hover:text-zinc-300" />
        </button>
    );
}
