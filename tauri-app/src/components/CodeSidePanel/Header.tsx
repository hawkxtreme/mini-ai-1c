import { X, AlertTriangle, AlertCircle, FileCode, ArrowLeftRight, ChevronUp, ChevronDown, Trash2, Maximize2, Minimize2 } from 'lucide-react';

interface HeaderProps {
    viewMode: 'editor' | 'diff';
    setViewMode: (mode: 'editor' | 'diff') => void;
    isValidating: boolean;
    errorCount: number;
    warningCount: number;
    diffChanges: any[];
    currentDiffIndex: number;
    prevDiff: () => void;
    nextDiff: () => void;
    onClose: () => void;
    setIsExpanded: (expanded: boolean) => void;
    isExpanded: boolean;
    isFullWidth?: boolean;
    diffEditorRef: any;
    onModifiedCodeChange: (code: string) => void;
    onActiveDiffChange?: (content: string) => void;
    onDiffRejected?: () => void;
    foldAll: () => void;
}

export function Header({
    viewMode,
    setViewMode,
    isValidating,
    errorCount,
    warningCount,
    diffChanges,
    currentDiffIndex,
    prevDiff,
    nextDiff,
    onClose,
    setIsExpanded,
    isExpanded,
    isFullWidth,
    diffEditorRef,
    onModifiedCodeChange,
    onActiveDiffChange,
    onDiffRejected,
    foldAll
}: HeaderProps) {
    return (
        <div className="flex items-center justify-between px-4 py-3 border-b border-[#27272a] bg-[#18181b]">
            <div className="flex items-center gap-2">
                {/* Validation Summary or Loader */}
                {isValidating ? (
                    <div className="flex items-center gap-2 px-2 py-0.5 rounded bg-[#27272a]/50 text-zinc-500 text-[10px] animate-pulse">
                        <span>Validating...</span>
                    </div>
                ) : (errorCount > 0 || warningCount > 0) ? (
                    <div className="flex items-center gap-2 px-2 py-0.5 rounded bg-[#27272a] border border-zinc-700 flex-shrink-0">
                        {errorCount > 0 && (
                            <div className="flex items-center gap-1 text-[10px] text-red-400 font-bold">
                                <AlertCircle className="w-3 h-3" />
                                <span>{errorCount}</span>
                            </div>
                        )}
                        {warningCount > 0 && (
                            <div className="flex items-center gap-1 text-[10px] text-yellow-500 font-medium">
                                <AlertTriangle className="w-3 h-3" />
                                <span>{warningCount}</span>
                            </div>
                        )}
                    </div>
                ) : null}

                <div className="flex bg-[#27272a] rounded-lg p-0.5 flex-shrink-0">
                    <button
                        onClick={() => setViewMode('editor')}
                        className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors flex items-center gap-1.5 ${viewMode === 'editor' ? 'bg-[#3f3f46] text-white shadow-sm' : 'text-zinc-500 hover:text-zinc-300'}`}
                        title="Standard Editor"
                    >
                        <FileCode className="w-3 h-3" />
                        <span>Edit</span>
                    </button>
                    <button
                        onClick={() => setViewMode('diff')}
                        className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors flex items-center gap-1.5 ${viewMode === 'diff' ? 'bg-[#3f3f46] text-white shadow-sm' : 'text-zinc-500 hover:text-zinc-300'}`}
                        title="Diff View"
                    >
                        <ArrowLeftRight className="w-3 h-3" />
                        <span>Diff</span>
                    </button>
                </div>

                {viewMode === 'diff' && diffChanges.length > 0 && (
                    <div className="flex bg-[#27272a]/50 rounded-lg p-0.5 ml-2 flex-shrink-0 animate-in fade-in items-center">
                        <div className="flex items-center gap-0.5 mr-1 border-r border-zinc-700/50 pr-1">
                            <button
                                onClick={prevDiff}
                                className="p-1 hover:bg-zinc-800 rounded text-zinc-400 hover:text-white transition-colors"
                                title="К предыдущему изменению"
                            >
                                <ChevronUp className="w-3 h-3" />
                            </button>
                            <span className="text-[9px] text-zinc-500 font-bold min-w-[32px] text-center tabular-nums">
                                {currentDiffIndex + 1} / {diffChanges.length}
                            </span>
                            <button
                                onClick={nextDiff}
                                className="p-1 hover:bg-zinc-800 rounded text-zinc-400 hover:text-white transition-colors"
                                title="К следующему изменению"
                            >
                                <ChevronDown className="w-3 h-3" />
                            </button>
                        </div>

                        <button
                            onClick={() => {
                                if (!diffEditorRef.current) return;
                                const currentOriginalCode = diffEditorRef.current.getOriginalEditor().getModel().getValue();
                                onModifiedCodeChange(currentOriginalCode);
                                if (onDiffRejected) onDiffRejected();
                                if (onActiveDiffChange) onActiveDiffChange('');
                            }}
                            className="px-2 py-0.5 rounded text-[10px] font-medium transition-colors flex items-center gap-1.5 text-zinc-400 hover:text-red-400 hover:bg-zinc-800"
                            title="Отменить непринятые (вернуться к оригиналу)"
                        >
                            <Trash2 className="w-3 h-3" />
                            <span>Сбросить непринятые</span>
                        </button>
                    </div>
                )}
            </div>
            <div className="flex items-center gap-1">
                <button
                    onClick={foldAll}
                    className="text-zinc-500 hover:text-zinc-300 transition-colors p-1 flex items-center justify-center"
                    title="Fold All"
                >
                    <ChevronUp className="w-4 h-4" />
                </button>
                {!isFullWidth && (
                    <button
                        onClick={() => setIsExpanded(!isExpanded)}
                        className="text-zinc-500 hover:text-zinc-300 transition-colors p-1 flex items-center justify-center"
                        title={isExpanded ? "Collapse Panel" : "Expand Panel"}
                    >
                        {isExpanded ? <Minimize2 className="w-4 h-4" /> : <Maximize2 className="w-4 h-4" />}
                    </button>
                )}
                <button
                    onClick={onClose}
                    className="text-zinc-500 hover:text-zinc-300 transition-colors p-1 flex items-center justify-center ml-1"
                    title="Close Panel"
                >
                    <X className="w-4 h-4" />
                </button>
            </div>
        </div>
    );
}
