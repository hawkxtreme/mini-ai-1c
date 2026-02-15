import { AlertTriangle, Copy, MousePointer, X } from 'lucide-react';

interface ConflictDialogProps {
    isOpen: boolean;
    onClose: () => void;
    onApplyToAll: () => void;
    onApplyToSelection: () => void;
    selectionActive?: boolean;
}

export function ConflictDialog({
    isOpen,
    onClose,
    onApplyToAll,
    onApplyToSelection,
    selectionActive = true
}: ConflictDialogProps) {
    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 z-[100] flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm animate-in fade-in duration-200">
            <div className="w-full max-w-md bg-[#18181b] border border-[#27272a] rounded-xl shadow-2xl overflow-hidden animate-in zoom-in-95 duration-200">
                {/* Header */}
                <div className="p-4 border-b border-[#27272a] flex items-center justify-between bg-zinc-900/50">
                    <div className="flex items-center gap-2 text-yellow-500">
                        <AlertTriangle className="w-5 h-5" />
                        <h3 className="text-sm font-semibold text-zinc-100">Конфликт изменений</h3>
                    </div>
                    <button
                        onClick={onClose}
                        className="p-1 hover:bg-zinc-800 rounded-md text-zinc-500 hover:text-zinc-300 transition-colors"
                    >
                        <X className="w-4 h-4" />
                    </button>
                </div>

                {/* Content */}
                <div className="p-6 space-y-4">
                    <p className="text-sm text-zinc-400 leading-relaxed">
                        Код в Конфигураторе 1С был изменён с момента последнего чтения. Как вы хотите применить изменения?
                    </p>

                    <div className="grid gap-3">
                        <button
                            onClick={onApplyToAll}
                            className="flex items-start gap-3 p-3 rounded-lg border border-[#27272a] bg-zinc-900/50 hover:bg-zinc-800 transition-all group text-left"
                        >
                            <div className="p-2 rounded-md bg-blue-500/10 text-blue-500 group-hover:bg-blue-500/20 transition-colors">
                                <Copy className="w-4 h-4" />
                            </div>
                            <div>
                                <div className="text-xs font-medium text-zinc-200">Применить ко всей области</div>
                                <div className="text-[10px] text-zinc-500 mt-0.5">Полностью заменить текущее содержимое (через Ctrl+A)</div>
                            </div>
                        </button>

                        <button
                            onClick={onApplyToSelection}
                            className="flex items-start gap-3 p-3 rounded-lg border border-[#27272a] bg-zinc-900/50 hover:bg-zinc-800 transition-all group text-left"
                        >
                            <div className="p-2 rounded-md bg-zinc-800 text-zinc-400 group-hover:bg-zinc-700 transition-colors">
                                <MousePointer className="w-4 h-4" />
                            </div>
                            <div>
                                <div className="text-xs font-medium text-zinc-200">Вставить в текущее выделение</div>
                                <div className="text-[10px] text-zinc-500 mt-0.5">Вставить код в позицию курсора без проверки конфликта</div>
                                {!selectionActive && (
                                    <div className="text-[10px] text-red-500 mt-1 flex items-center gap-1 font-medium">
                                        <AlertTriangle className="w-3 h-3" />
                                        Выделение не обнаружено. Код будет вставлен в позицию курсора!
                                    </div>
                                )}
                            </div>
                        </button>
                    </div>
                </div>

                {/* Footer */}
                <div className="p-3 border-t border-[#27272a] bg-zinc-900/50 flex justify-end">
                    <button
                        onClick={onClose}
                        className="px-4 py-1.5 text-xs font-medium text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded transition-colors"
                    >
                        Отмена
                    </button>
                </div>
            </div>
        </div>
    );
}
