import { useState, useMemo, useRef, useCallback, useEffect } from 'react';
import { X, Check, AlertTriangle, Terminal, AlertCircle, Maximize2, Minimize2, FileCode, ArrowLeftRight, GripVertical } from 'lucide-react';
import { DiffEditor, Editor } from '@monaco-editor/react';

interface BslDiagnostic {
    line: number;
    message: string;
    severity: string;
}

interface CodeSidePanelProps {
    isOpen: boolean;
    onClose: () => void;
    originalCode: string;
    modifiedCode: string;
    onModifiedCodeChange: (code: string) => void;
    diagnostics: BslDiagnostic[];
    onApply: () => void;
    isApplying: boolean;
}

export function CodeSidePanel({
    isOpen,
    onClose,
    originalCode,
    modifiedCode,
    onModifiedCodeChange,
    diagnostics,
    onApply,
    isApplying
}: CodeSidePanelProps) {
    const [isExpanded, setIsExpanded] = useState(true);
    const [viewMode, setViewMode] = useState<'editor' | 'diff'>('diff');
    const [width, setWidth] = useState(500);
    const [isResizing, setIsResizing] = useState(false);

    const panelRef = useRef<HTMLDivElement>(null);

    const errorCount = useMemo(() => diagnostics.filter(d => d.severity === 'error').length, [diagnostics]);
    const warningCount = useMemo(() => diagnostics.filter(d => d.severity !== 'error').length, [diagnostics]);

    // Handle resizing
    const startResizing = useCallback((e: React.MouseEvent) => {
        setIsResizing(true);
        e.preventDefault();
    }, []);

    const stopResizing = useCallback(() => {
        setIsResizing(false);
    }, []);

    const resize = useCallback((e: MouseEvent) => {
        if (isResizing) {
            const newWidth = window.innerWidth - e.clientX;
            // Constrain width between 280 and 80% of window
            if (newWidth > 280 && newWidth < window.innerWidth * 0.8) {
                setWidth(newWidth);
                if (newWidth > 400) setIsExpanded(true);
                else setIsExpanded(false);
            }
        }
    }, [isResizing]);

    useEffect(() => {
        if (isResizing) {
            window.addEventListener('mousemove', resize);
            window.addEventListener('mouseup', stopResizing);
        } else {
            window.removeEventListener('mousemove', resize);
            window.removeEventListener('mouseup', stopResizing);
        }
        return () => {
            window.removeEventListener('mousemove', resize);
            window.removeEventListener('mouseup', stopResizing);
        };
    }, [isResizing, resize, stopResizing]);

    // Default to max (expanded) when opened
    useEffect(() => {
        if (isOpen) {
            setIsExpanded(true);
            // Default expanded width based on screen size
            setWidth(window.innerWidth > 1200 ? 600 : 500);
        }
    }, [isOpen]);

    if (!isOpen) return null;

    return (
        <div
            ref={panelRef}
            style={{ width: isExpanded ? `${width}px` : '280px' }}
            className={`border-l border-[#27272a] bg-[#09090b] flex flex-col h-full shadow-2xl transition-[width] duration-300 ease-in-out flex-shrink-0 relative ${isResizing ? 'transition-none' : ''}`}
        >
            {/* Resize Handle */}
            <div
                onMouseDown={startResizing}
                className="absolute left-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-blue-500/30 transition-colors z-50 flex items-center justify-center group"
            >
                <div className="w-0.5 h-8 bg-zinc-700 group-hover:bg-blue-400 rounded-full opacity-0 group-hover:opacity-100" />
            </div>

            {/* Header */}
            <div className="flex items-center justify-between px-4 py-3 border-b border-[#27272a] bg-[#18181b]">
                <div className="flex items-center gap-2">
                    <Terminal className="w-4 h-4 text-blue-400" />
                    <span className="font-semibold text-zinc-200 text-sm whitespace-nowrap">Code Editor</span>

                    <div className="flex bg-[#27272a] rounded-lg p-0.5 ml-4 flex-shrink-0">
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

                    {/* Validation Summary */}
                    {(errorCount > 0 || warningCount > 0) && (
                        <div className="flex items-center gap-2 ml-2 px-2 py-0.5 rounded bg-[#27272a] border border-zinc-700 flex-shrink-0">
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
                    )}
                </div>
                <div className="flex items-center gap-1">
                    <button
                        onClick={() => {
                            if (isExpanded) {
                                setWidth(280);
                                setIsExpanded(false);
                            } else {
                                setWidth(500);
                                setIsExpanded(true);
                            }
                        }}
                        className="text-zinc-500 hover:text-zinc-300 transition-colors p-1"
                        title={isExpanded ? "Collapse" : "Expand"}
                    >
                        {isExpanded ? <Minimize2 className="w-3.5 h-3.5" /> : <Maximize2 className="w-3.5 h-3.5" />}
                    </button>
                    <button onClick={onClose} className="text-zinc-500 hover:text-zinc-300 transition-colors p-1">
                        <X className="w-4 h-4" />
                    </button>
                </div>
            </div>

            {/* Editor Area */}
            <div className="flex-1 overflow-hidden relative group">
                {viewMode === 'editor' ? (
                    <Editor
                        height="100%"
                        language="vb" // Closest to BSL
                        theme="vs-dark"
                        value={modifiedCode}
                        onChange={(value) => onModifiedCodeChange(value || '')}
                        options={{
                            minimap: { enabled: false },
                            fontSize: 12,
                            wordWrap: 'on',
                            lineNumbers: 'on',
                            scrollBeyondLastLine: false,
                            automaticLayout: true,
                        }}
                    />
                ) : (
                    <DiffEditor
                        height="100%"
                        language="vb" // Closest to BSL
                        theme="vs-dark"
                        original={originalCode}
                        modified={modifiedCode}
                        onMount={(editor) => {
                            const modifiedEditor = editor.getModifiedEditor();
                            modifiedEditor.onDidChangeModelContent(() => {
                                onModifiedCodeChange(modifiedEditor.getValue());
                            });
                        }}
                        options={{
                            minimap: { enabled: false },
                            fontSize: 12,
                            wordWrap: 'on',
                            renderSideBySide: false, // Inline diff
                            readOnly: false,
                            originalEditable: false,
                            automaticLayout: true,
                        }}
                    />
                )}
            </div>

            {/* Problems Panel - VS Code Style */}
            <div className="h-40 border-t border-[#27272a] bg-[#18181b] flex flex-col">
                <div className="px-3 py-1.5 border-b border-[#27272a]/50 flex items-center gap-2 text-[10px] font-bold text-zinc-500 uppercase tracking-wider bg-[#1f1f23]">
                    <AlertTriangle className="w-3 h-3" />
                    <span>Problems</span>
                    <span className="px-1.5 bg-[#27272a] rounded-full text-zinc-400">{diagnostics.length}</span>
                </div>

                <div className="overflow-y-auto flex-1 p-0">
                    {diagnostics.length === 0 ? (
                        <div className="p-4 text-xs text-zinc-600 italic text-center text-[11px]">No problems found</div>
                    ) : (
                        <table className="w-full text-left border-collapse">
                            <thead className="bg-[#1f1f23] text-zinc-500 sticky top-0">
                                <tr>
                                    <th className="px-3 py-1 text-[10px] font-normal w-12">Ln</th>
                                    <th className="px-3 py-1 text-[10px] font-normal">Description</th>
                                    <th className="px-3 py-1 text-[10px] font-normal w-20">Source</th>
                                </tr>
                            </thead>
                            <tbody>
                                {diagnostics.map((d, i) => (
                                    <tr key={i} className="border-b border-[#27272a]/30 hover:bg-[#27272a]/50 transition-colors group cursor-pointer">
                                        <td className={`px-3 py-1.5 text-[11px] font-mono whitespace-nowrap align-top ${d.severity === 'error' ? 'text-red-400' : 'text-yellow-400'}`}>
                                            {d.line}
                                        </td>
                                        <td className="px-3 py-1.5 text-[11px] text-zinc-300 align-top">
                                            {d.message}
                                        </td>
                                        <td className="px-3 py-1.5 text-[10px] text-zinc-500 align-top truncate">
                                            BSL LS
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    )}
                </div>
            </div>

            {/* Footer Actions */}
            <div className="p-3 border-t border-[#27272a] bg-[#18181b] flex items-center justify-between">
                <div className="text-[10px] text-zinc-500 flex items-center gap-2">
                    {viewMode === 'diff' && (
                        <>
                            <div className="w-2 h-2 rounded-full bg-blue-500"></div>
                            <span>Modified</span>
                            <span className="text-zinc-600">|</span>
                            <div className="w-2 h-2 rounded-full border border-zinc-600"></div>
                            <span>Original</span>
                        </>
                    )}
                    {viewMode === 'editor' && (
                        <span>Standard Editor Mode</span>
                    )}
                </div>

                <button
                    onClick={onApply}
                    disabled={isApplying || !modifiedCode.trim()}
                    className={`flex items-center gap-2 px-4 py-1.5 rounded text-xs font-medium transition-colors ${isApplying || !modifiedCode.trim()
                        ? 'bg-[#27272a] text-zinc-500 cursor-not-allowed'
                        : 'bg-blue-600 hover:bg-blue-500 text-white shadow-lg shadow-blue-500/10'
                        }`}
                >
                    {isApplying ? (
                        <>Applying...</>
                    ) : (
                        <>
                            <Check className="w-3.5 h-3.5" />
                            Apply Changes
                        </>
                    )}
                </button>
            </div>
        </div>
    );
}


