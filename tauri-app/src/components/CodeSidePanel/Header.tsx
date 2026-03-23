import { X, AlertTriangle, AlertCircle, FileCode, ArrowLeftRight, ChevronUp, ChevronDown, Trash2, Maximize2, Minimize2, Wrench } from 'lucide-react';
import { useSettings } from '@/contexts/SettingsContext';

interface HeaderProps {
    viewMode: 'editor' | 'diff' | 'tools';
    setViewMode: (mode: 'editor' | 'diff' | 'tools') => void;
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
    const { settings } = useSettings();
    const isLightTheme = settings?.theme === 'light';

    const shellClass = isLightTheme
        ? 'border-[#d4d4d8] bg-[#f4f4f5]'
        : 'border-[#27272a] bg-[#18181b]';
    const badgeClass = isLightTheme
        ? 'bg-[#e4e4e7] border-[#d4d4d8]'
        : 'bg-[#27272a] border-zinc-700';
    const toggleShellClass = isLightTheme ? 'bg-[#e4e4e7]' : 'bg-[#27272a]';
    const activeTabClass = isLightTheme ? 'bg-white text-[#18181b] shadow-sm' : 'bg-[#3f3f46] text-white shadow-sm';
    const inactiveTabClass = isLightTheme
        ? 'text-[#52525b] hover:text-[#18181b] hover:bg-white/80'
        : 'text-zinc-500 hover:text-zinc-300';
    const diffShellClass = isLightTheme ? 'bg-[#e4e4e7]' : 'bg-[#27272a]/50';
    const diffDividerClass = isLightTheme ? 'border-zinc-300' : 'border-zinc-700/50';
    const subtleButtonClass = isLightTheme
        ? 'text-[#52525b] hover:text-[#18181b] hover:bg-[#d4d4d8]'
        : 'text-zinc-400 hover:text-white hover:bg-zinc-800';
    const toolbarIconClass = isLightTheme ? 'text-[#52525b] hover:text-[#18181b]' : 'text-zinc-500 hover:text-zinc-300';
    const rejectClass = isLightTheme
        ? 'text-[#52525b] hover:text-[#dc2626] hover:bg-[#d4d4d8]'
        : 'text-zinc-400 hover:text-red-400 hover:bg-zinc-800';
    const validatingClass = isLightTheme ? 'bg-[#e4e4e7] text-[#52525b]' : 'bg-[#27272a]/50 text-zinc-500';
    const diffCounterClass = isLightTheme ? 'text-[#52525b]' : 'text-zinc-500';
    const errorBadgeTextClass = isLightTheme ? 'text-[#dc2626]' : 'text-red-400';
    const warningBadgeTextClass = isLightTheme ? 'text-[#b45309]' : 'text-yellow-500';

    return (
        <div className={`flex items-center justify-between px-4 py-3 border-b ${shellClass}`}>
            <div className="flex items-center gap-2">
                {isValidating ? (
                    <div className={`flex items-center gap-2 px-2 py-0.5 rounded text-[10px] animate-pulse ${validatingClass}`}>
                        <span>Validating...</span>
                    </div>
                ) : (errorCount > 0 || warningCount > 0) ? (
                    <div className={`flex items-center gap-2 px-2 py-0.5 rounded border flex-shrink-0 ${badgeClass}`}>
                        {errorCount > 0 && (
                            <div className={`flex items-center gap-1 text-[10px] font-bold ${errorBadgeTextClass}`}>
                                <AlertCircle className="w-3 h-3" />
                                <span>{errorCount}</span>
                            </div>
                        )}
                        {warningCount > 0 && (
                            <div className={`flex items-center gap-1 text-[10px] font-medium ${warningBadgeTextClass}`}>
                                <AlertTriangle className="w-3 h-3" />
                                <span>{warningCount}</span>
                            </div>
                        )}
                    </div>
                ) : null}

                <div className={`flex rounded-lg p-0.5 flex-shrink-0 ${toggleShellClass}`}>
                    <button
                        onClick={() => setViewMode('editor')}
                        className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors flex items-center gap-1.5 ${
                            viewMode === 'editor' ? activeTabClass : inactiveTabClass
                        }`}
                        title="Standard Editor"
                    >
                        <FileCode className="w-3 h-3" />
                        <span>Edit</span>
                    </button>
                    <button
                        onClick={() => setViewMode('diff')}
                        className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors flex items-center gap-1.5 ${
                            viewMode === 'diff' ? activeTabClass : inactiveTabClass
                        }`}
                        title="Diff View"
                    >
                        <ArrowLeftRight className="w-3 h-3" />
                        <span>Diff</span>
                    </button>
                    <button
                        onClick={() => setViewMode('tools')}
                        className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors flex items-center gap-1.5 ${
                            viewMode === 'tools' ? activeTabClass : inactiveTabClass
                        }`}
                        title="MCP Tools"
                    >
                        <Wrench className="w-3 h-3" />
                        <span>MCP</span>
                    </button>
                </div>

                {viewMode === 'diff' && diffChanges.length > 0 && (
                    <div className={`flex rounded-lg p-0.5 ml-2 flex-shrink-0 animate-in fade-in items-center ${diffShellClass}`}>
                        <div className={`flex items-center gap-0.5 mr-1 border-r pr-1 ${diffDividerClass}`}>
                            <button
                                onClick={prevDiff}
                                className={`p-1 rounded transition-colors ${subtleButtonClass}`}
                                title="К предыдущему изменению"
                            >
                                <ChevronUp className="w-3 h-3" />
                            </button>
                            <span className={`text-[9px] font-bold min-w-[32px] text-center tabular-nums ${diffCounterClass}`}>
                                {currentDiffIndex + 1} / {diffChanges.length}
                            </span>
                            <button
                                onClick={nextDiff}
                                className={`p-1 rounded transition-colors ${subtleButtonClass}`}
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
                            className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors flex items-center gap-1.5 ${rejectClass}`}
                            title="Отменить непринятые изменения"
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
                    className={`transition-colors p-1 flex items-center justify-center ${toolbarIconClass}`}
                    title="Fold All"
                >
                    <ChevronUp className="w-4 h-4" />
                </button>
                {!isFullWidth && (
                    <button
                        onClick={() => setIsExpanded(!isExpanded)}
                        className={`transition-colors p-1 flex items-center justify-center ${toolbarIconClass}`}
                        title={isExpanded ? 'Collapse Panel' : 'Expand Panel'}
                    >
                        {isExpanded ? <Minimize2 className="w-4 h-4" /> : <Maximize2 className="w-4 h-4" />}
                    </button>
                )}
                <button
                    onClick={onClose}
                    className={`transition-colors p-1 flex items-center justify-center ml-1 ${toolbarIconClass}`}
                    title="Close Panel"
                >
                    <X className="w-4 h-4" />
                </button>
            </div>
        </div>
    );
}
