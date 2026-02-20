import { useState, useMemo, useEffect } from 'react';
import { parseDiffBlocks, applyDiff, hasDiffBlocks } from '../../utils/diffViewer';
import { BslDiffEditor } from '../ui/BslDiffEditor';
import { Check, X, FileDiff, Send, Plus, Minus, Edit2 } from 'lucide-react';

interface DiffViewerProps {
    originalCode: string;
    diffContent: string;
    onApply: (newCode: string) => void;
    onCancel?: () => void;
}

export function DiffViewer({ originalCode, diffContent, onApply, onCancel }: DiffViewerProps) {
    const defaultBlocks = useMemo(() => parseDiffBlocks(diffContent), [diffContent]);

    // Состояние блоков (для кнопок Confirm/Reject)
    const [blocks, setBlocks] = useState(defaultBlocks);

    // Сбрасываем стейт при новом контенте
    useEffect(() => {
        setBlocks(parseDiffBlocks(diffContent));
    }, [diffContent]);

    const stats = useMemo(() => {
        let added = 0;
        let removed = 0;
        let modified = 0;

        blocks.forEach(b => {
            if (b.status === 'rejected') return;
            // Простейший подсчет количества строк, игнорируем пустые
            const sLines = b.search.split('\n').filter(l => l.trim() !== '');
            const rLines = b.replace.split('\n').filter(l => l.trim() !== '');

            const sCount = sLines.length;
            const rCount = rLines.length;

            modified += Math.min(sCount, rCount);
            if (rCount > sCount) added += (rCount - sCount);
            if (sCount > rCount) removed += (sCount - rCount);
        });

        return { added, removed, modified };
    }, [blocks]);

    if (blocks.length === 0) {
        return <div className="p-4 text-zinc-500 text-sm">Не найдено блоков изменений.</div>;
    }

    const handleConfirm = (index: number) => {
        setBlocks(prev => prev.map((b, i) => i === index ? { ...b, status: 'confirmed' } : b));
    };

    const handleReject = (index: number) => {
        setBlocks(prev => prev.map((b, i) => i === index ? { ...b, status: 'rejected' } : b));
    };

    const handleApplyAll = () => {
        // Применяем все confirmed или pending (считаем pending за подтвержденные по умолчанию, если нажали Apply All)
        const indicesToApply = blocks
            .map((b, i) => (b.status !== 'rejected' ? i : -1))
            .filter(i => i !== -1);

        const newCode = applyDiff(originalCode, blocks, indicesToApply);
        onApply(newCode);
    };

    const handleRejectAll = () => {
        if (onCancel) {
            onCancel();
        } else {
            setBlocks(prev => prev.map(b => ({ ...b, status: 'rejected' })));
        }
    };

    const pendingCount = blocks.filter(b => b.status === 'pending').length;
    let currentPreviewCode = originalCode;

    return (
        <div className="flex flex-col gap-4 w-full animate-in fade-in slide-in-from-bottom-2">
            <div className="flex items-center justify-between px-2">
                <div className="flex items-center gap-2 text-zinc-300 font-medium text-sm">
                    <FileDiff className="w-4 h-4 text-blue-400" />
                    <span>Найдено изменений: {blocks.length}</span>
                </div>
            </div>

            <div className="flex flex-col gap-3">
                {blocks.map((block, i) => {
                    // Генерируем превью для конкретного блока
                    const previewCode = applyDiff(currentPreviewCode, [block], [0]);
                    const originalForPreview = currentPreviewCode;
                    currentPreviewCode = block.status === 'rejected' ? currentPreviewCode : previewCode; // Накапливаем изменения для последующих блоков, если не отменено

                    return (
                        <div key={i} className={`rounded-xl border overflow-hidden transition-all duration-300 ${block.status === 'confirmed' ? 'border-emerald-500/30 shadow-[0_0_15px_rgba(16,185,129,0.1)]' :
                                block.status === 'rejected' ? 'border-red-500/30 opacity-70' :
                                    'border-blue-500/30 shadow-[0_0_15px_rgba(59,130,246,0.1)]'
                            }`}>

                            {/* Header блока */}
                            <div className={`flex items-center justify-between px-4 py-2 text-xs font-semibold uppercase tracking-widest ${block.status === 'confirmed' ? 'bg-emerald-500/10 text-emerald-400' :
                                    block.status === 'rejected' ? 'bg-red-500/10 text-red-500' :
                                        'bg-blue-500/10 text-blue-400'
                                }`}>
                                <span>Блок {i + 1} {block.lineStart ? `(Строка ~${block.lineStart})` : ''}</span>
                                <div className="flex items-center gap-1.5">
                                    <span className="opacity-70">
                                        {block.status === 'confirmed' ? '✅ Принято' : block.status === 'rejected' ? '❌ Отклонено' : '⏳ Ожидает'}
                                    </span>
                                </div>
                            </div>

                            {/* Тело диффа */}
                            <div className="relative bg-[#1e1e1e]">
                                {block.status === 'rejected' ? (
                                    <div className="p-4 text-center text-zinc-500 text-sm italic">
                                        Изменение отклонено
                                    </div>
                                ) : (
                                    <BslDiffEditor
                                        original={block.search} // Показываем чистый дифф текущего блока SEARCH->REPLACE
                                        modified={block.replace}
                                        height={Math.min(300, Math.max(block.search.split('\n').length, block.replace.split('\n').length) * 20 + 20)}
                                        hideBorder
                                    />
                                )}
                            </div>

                            {/* Actions */}
                            <div className="flex items-center justify-end gap-2 px-4 py-2 bg-zinc-900/80 border-t border-zinc-800">
                                <button
                                    onClick={() => handleReject(i)}
                                    className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg transition-colors text-xs font-semibold ${block.status === 'rejected'
                                            ? 'bg-red-500 text-white'
                                            : 'bg-zinc-800 text-zinc-400 hover:text-red-400 hover:bg-zinc-700'
                                        }`}
                                >
                                    <X className="w-3.5 h-3.5" />
                                    Отклонить
                                </button>
                                <button
                                    onClick={() => handleConfirm(i)}
                                    className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg transition-colors text-xs font-semibold ${block.status === 'confirmed'
                                            ? 'bg-emerald-500 text-white'
                                            : 'bg-zinc-800 text-zinc-400 hover:text-emerald-400 hover:bg-zinc-700'
                                        }`}
                                >
                                    <Check className="w-3.5 h-3.5" />
                                    Принять
                                </button>
                            </div>
                        </div>
                    );
                })}
            </div>

            <div className="flex items-center justify-between mt-2 px-2 pb-2">
                <div className="flex items-center gap-4 text-xs font-mono bg-zinc-900/50 py-1.5 px-3 rounded-lg border border-zinc-800">
                    <div className="flex items-center gap-1.5 text-emerald-400" title="Добавлено строк">
                        <Plus className="w-3.5 h-3.5" />
                        <span>{stats.added}</span>
                    </div>
                    <div className="flex items-center gap-1.5 text-blue-400" title="Изменено строк">
                        <Edit2 className="w-3.5 h-3.5" />
                        <span>{stats.modified}</span>
                    </div>
                    <div className="flex items-center gap-1.5 text-red-500" title="Удалено строк">
                        <Minus className="w-3.5 h-3.5" />
                        <span>{stats.removed}</span>
                    </div>
                </div>

                <div className="flex items-center gap-3">
                    <button
                        onClick={handleRejectAll}
                        className="px-4 py-2 text-zinc-400 hover:text-red-400 transition-colors text-sm font-medium"
                    >
                        Отменить
                    </button>
                    <button
                        onClick={handleApplyAll}
                        disabled={blocks.every(b => b.status === 'rejected')}
                        className="flex items-center gap-2 px-5 py-2.5 bg-blue-600 hover:bg-blue-500 disabled:bg-zinc-800 disabled:text-zinc-600 text-white rounded-xl transition-all shadow-lg shadow-blue-900/20 font-semibold text-sm"
                    >
                        <Send className="w-4 h-4" />
                        {pendingCount > 0 ? 'Применить оставшиеся' : 'Применить изменения'}
                    </button>
                </div>
            </div>
        </div>
    );
}
