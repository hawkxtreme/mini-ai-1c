import { useState, useMemo, useRef, useCallback, useEffect } from 'react';
import { X, Check, AlertTriangle, Terminal, AlertCircle, Maximize2, Minimize2, FileCode, ArrowLeftRight, GripVertical, ChevronUp, ChevronDown, Trash2 } from 'lucide-react';
import { DiffEditor, Editor, loader } from '@monaco-editor/react';
import { registerBSL } from '@/lib/monaco-bsl';
import { parseDiffBlocks } from '../utils/diffViewer';
import { invoke } from '@tauri-apps/api/core';

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
    isValidating: boolean;
    activeDiffContent?: string;
    onActiveDiffChange?: (content: string) => void;
}

export function CodeSidePanel({
    isOpen,
    onClose,
    originalCode,
    modifiedCode,
    onModifiedCodeChange,
    diagnostics,
    onApply,
    isApplying,
    isValidating,
    activeDiffContent,
    onActiveDiffChange
}: CodeSidePanelProps) {
    const [isExpanded, setIsExpanded] = useState(true);
    const [viewMode, setViewMode] = useState<'editor' | 'diff'>('diff');

    // Make localOriginalCode to sync with originalCode, but allow accepting changes
    const [localOriginalCode, setLocalOriginalCode] = useState(originalCode);
    useEffect(() => {
        setLocalOriginalCode(originalCode);
    }, [originalCode]);

    useEffect(() => {
        if (activeDiffContent && viewMode !== 'diff') {
            setViewMode('diff');
        }
    }, [activeDiffContent]);
    const [width, setWidth] = useState(500);
    const [isResizing, setIsResizing] = useState(false);

    // Ref to hold the latest activeDiffContent to avoid stale closures in Monaco callbacks
    const activeDiffContentRef = useRef(activeDiffContent);
    useEffect(() => {
        activeDiffContentRef.current = activeDiffContent;
    }, [activeDiffContent]);

    const panelRef = useRef<HTMLDivElement>(null);
    const editorRef = useRef<any>(null);
    const diffEditorRef = useRef<any>(null); // Reference to the standalone DiffEditor
    const monacoRef = useRef<any>(null);

    // Store references to view zones so we can remove/update them
    const viewZoneIdsRef = useRef<string[]>([]);
    const [diffChanges, setDiffChanges] = useState<any[]>([]);

    const errorCount = useMemo(() => diagnostics.filter(d => d.severity === 'error').length, [diagnostics]);
    const warningCount = useMemo(() => diagnostics.filter(d => d.severity !== 'error').length, [diagnostics]);

    // Очистка списка изменений если дифф закрыт, чтобы не висели старые кнопки
    useEffect(() => {
        if (!activeDiffContent) {
            setDiffChanges([]);
        }
    }, [activeDiffContent]);

    // Принудительное обновление виджетов, когда activeDiffContent меняется и мы в режиме diff
    useEffect(() => {
        if (viewMode === 'diff' && diffEditorRef.current && diffEditorRef.current.updateInlineWidgetsRef) {
            // Небольшая задержка, чтобы Monaco успел отрендерить новый контент
            setTimeout(() => {
                diffEditorRef.current.updateInlineWidgetsRef();
            }, 100);
        }
    }, [activeDiffContent, viewMode, modifiedCode]);

    // ... resizing logic same ...
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

    // Register BSL language
    useEffect(() => {
        loader.init().then(monaco => {
            registerBSL(monaco);
        });
    }, []);

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
            id="code-side-panel"
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

                    {viewMode === 'diff' && diffChanges.length > 0 && (
                        <div className="flex bg-[#27272a]/50 rounded-lg p-0.5 ml-2 flex-shrink-0 animate-in fade-in">
                            <button
                                onClick={() => {
                                    if (!diffEditorRef.current) return;
                                    const currentOriginalCode = diffEditorRef.current.getOriginalEditor().getModel().getValue();
                                    onModifiedCodeChange(currentOriginalCode);
                                    if (onActiveDiffChange) onActiveDiffChange('');
                                }}
                                className="px-2 py-0.5 rounded text-[10px] font-medium transition-colors flex items-center gap-1.5 text-zinc-400 hover:text-red-400 hover:bg-zinc-800"
                                title="Отменить непринятые (вернуться к оригиналу)"
                            >
                                <Trash2 className="w-3 h-3" />
                                <span>Сбросить непринятые</span>
                            </button>
                            <button
                                onClick={() => {
                                    if (!diffEditorRef.current) return;
                                    const currentModifiedCode = diffEditorRef.current.getModifiedEditor().getModel().getValue();
                                    setLocalOriginalCode(currentModifiedCode);
                                    if (onActiveDiffChange) onActiveDiffChange('');
                                }}
                                className="px-2 py-0.5 ml-1 rounded text-[10px] font-medium transition-colors flex items-center gap-1.5 text-emerald-500 hover:text-emerald-400 hover:bg-emerald-500/10"
                                title="Принять все оставшиеся изменения"
                            >
                                <Check className="w-3 h-3" />
                                <span>Принять все</span>
                            </button>
                        </div>
                    )}

                    {/* Validation Summary or Loader */}
                    {isValidating ? (
                        <div className="flex items-center gap-2 ml-2 px-2 py-0.5 rounded bg-[#27272a]/50 text-zinc-500 text-[10px] animate-pulse">
                            <span>Validating...</span>
                        </div>
                    ) : (errorCount > 0 || warningCount > 0) ? (
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
                    ) : null}
                </div>
                <div className="flex items-center gap-1">
                    <button
                        onClick={() => editorRef.current?.trigger('fold-all', 'editor.foldAll')}
                        className="text-zinc-500 hover:text-zinc-300 transition-colors p-1 flex items-center justify-center"
                        title="Fold All"
                    >
                        <ChevronUp className="w-4 h-4" />
                    </button>
                    <button
                        onClick={() => editorRef.current?.trigger('unfold-all', 'editor.unfoldAll')}
                        className="text-zinc-500 hover:text-zinc-300 transition-colors p-1 flex items-center justify-center"
                        title="Unfold All"
                    >
                        <ChevronDown className="w-4 h-4" />
                    </button>
                    <div className="w-px h-4 bg-zinc-700/50 mx-1" />
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
            <div id="tour-editor" className="flex-1 overflow-hidden relative group">
                {viewMode === 'editor' ? (
                    <Editor
                        height="100%"
                        language="bsl"
                        theme="vs-dark"
                        value={modifiedCode}
                        onMount={(editor, monaco) => {
                            registerBSL(monaco);
                            editorRef.current = editor;
                        }}
                        onChange={(value) => onModifiedCodeChange(value || '')}
                        options={{
                            minimap: { enabled: false },
                            fontSize: 12,
                            wordWrap: 'on',
                            lineNumbers: 'on',
                            scrollBeyondLastLine: false,
                            automaticLayout: true,
                            folding: true,
                            showFoldingControls: 'always',
                        }}
                    />
                ) : (
                    <DiffEditor
                        height="100%"
                        language="bsl"
                        theme="vs-dark"
                        original={localOriginalCode}
                        modified={modifiedCode}
                        onMount={(editor, monaco) => {
                            registerBSL(monaco);
                            monacoRef.current = monaco;
                            diffEditorRef.current = editor;
                            const modifiedEditor = editor.getModifiedEditor();
                            editorRef.current = modifiedEditor;

                            modifiedEditor.onDidChangeModelContent(() => {
                                onModifiedCodeChange(modifiedEditor.getValue());
                            });

                            // Функция для прорисовки кнопок, вынесена отдельно
                            const updateInlineWidgets = () => {
                                const changes = editor.getLineChanges();
                                setDiffChanges(changes || []);

                                // Remove old zones
                                modifiedEditor.changeViewZones((accessor: any) => {
                                    viewZoneIdsRef.current.forEach(id => accessor.removeZone(id));
                                    viewZoneIdsRef.current = [];
                                });

                                // Parse active diff blocks for filtering (using ref to avoid stale closure)
                                const currentContent = activeDiffContentRef.current;
                                console.log('[DiffUpdate] Triggered. Changes:', changes?.length, 'Content length:', currentContent?.length);

                                if (!changes || changes.length === 0 || !currentContent) {
                                    console.log('[DiffUpdate] Aborted. Reason: No changes or no activeDiffContent.');
                                    return;
                                }

                                const aiBlocks = parseDiffBlocks(currentContent);
                                console.log('[DiffUpdate] Parsed AI blocks:', aiBlocks.length);

                                // Add new zones
                                modifiedEditor.changeViewZones((accessor: any) => {
                                    changes.forEach((change: any) => {
                                        // HEURISTIC: Match against AI blocks
                                        const originalModel = editor.getOriginalEditor().getModel();
                                        if (!originalModel) return;

                                        // In Monaco, if lines are appended, End = 0.
                                        // If lines are inserted in the middle, End < Start (e.g., Start=5, End=4).
                                        // If inserted at the very beginning of the file, Start=1, End=0.
                                        const isAddition = change.originalEndLineNumber === 0 ||
                                            change.originalEndLineNumber < change.originalStartLineNumber ||
                                            (change.originalStartLineNumber === 1 && change.originalEndLineNumber === 0);

                                        const origText = isAddition ? "" : originalModel.getValueInRange({
                                            startLineNumber: change.originalStartLineNumber,
                                            startColumn: 1,
                                            endLineNumber: change.originalEndLineNumber,
                                            endColumn: originalModel.getLineMaxColumn(change.originalEndLineNumber)
                                        });

                                        // Normalize function for comparison (removes ALL whitespace for robust matching)
                                        const stripWhitespace = (s: string) => s.replace(/\s+/g, '');
                                        const normOrig = stripWhitespace(origText);

                                        const matchedBlock = aiBlocks.find(b => {
                                            const normSearch = stripWhitespace(b.search);
                                            const normReplace = stripWhitespace(b.replace);

                                            // 1. Exact or fuzzy match for SEARCH (context)
                                            const searchMatches = normSearch === normOrig || (normSearch.length > 0 && normOrig.length > 0 && (normSearch.includes(normOrig) || normOrig.includes(normSearch)));

                                            if (isAddition) {
                                                // For additions, get the added text from the modified editor
                                                const modifiedModel = editor.getModifiedEditor().getModel();
                                                const modifiedText = modifiedModel ? stripWhitespace(modifiedModel.getValueInRange({
                                                    startLineNumber: change.modifiedStartLineNumber,
                                                    startColumn: 1,
                                                    endLineNumber: change.modifiedEndLineNumber,
                                                    endColumn: modifiedModel.getLineMaxColumn(change.modifiedEndLineNumber)
                                                })) : "";

                                                // Check if the newly added text is actually proposed in this REPLACE block
                                                // For pure additions, original text reported by Monaco is empty, so we cannot match SEARCH context with it.
                                                // It's safe to just check if the new text is part of AI's replacement block.
                                                return modifiedText.length > 0 && normReplace.includes(modifiedText);
                                            }

                                            return searchMatches;
                                        });

                                        const isAiHunk = !!matchedBlock;

                                        // Debug logging
                                        if (changes.length > 0) {
                                            console.log(`[DiffHeuristic] Hunk at L${change.modifiedStartLineNumber}: isAiHunk=${isAiHunk}`, {
                                                isAddition,
                                                origText: normOrig,
                                                matchedSearch: matchedBlock ? stripWhitespace(matchedBlock.search) : 'NONE'
                                            });
                                        }

                                        // If this hunk doesn't match any AI block, don't show buttons
                                        if (!isAiHunk) return;

                                        const domNode = document.createElement('div');
                                        domNode.className = 'flex items-center justify-end pr-8 gap-2 z-50 pointer-events-none';
                                        domNode.style.height = '30px';

                                        // Wrapper for buttons to look like a toolbar
                                        const toolbar = document.createElement('div');
                                        toolbar.className = 'flex items-center gap-1 bg-[#18181b] border border-[#3f3f46] rounded-md shadow-2xl p-0.5 mt-[-15px] pointer-events-auto ring-1 ring-black/50';

                                        // Revert button
                                        const btnRevert = document.createElement('button');
                                        btnRevert.innerHTML = '<span style="display:flex;align-items:center;gap:6px;padding: 2px 6px;"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"></path><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"></path><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"></path></svg> Отменить</span>';
                                        btnRevert.className = 'px-1 py-1 text-[11px] font-bold text-zinc-400 hover:text-red-400 hover:bg-red-500/10 rounded-sm transition-all active:scale-95';
                                        btnRevert.onclick = (e) => {
                                            e.preventDefault();
                                            e.stopPropagation();

                                            // Perform Revert
                                            const originalEditor = editor.getOriginalEditor();
                                            const currentModifiedCode = modifiedEditor.getModel()?.getValue() || '';
                                            const currentOriginalCode = originalEditor.getModel()?.getValue() || '';

                                            const origStart = change.originalStartLineNumber;
                                            const origEnd = change.originalEndLineNumber;
                                            const modStart = change.modifiedStartLineNumber;
                                            const modEnd = change.modifiedEndLineNumber;

                                            let targetLines = currentModifiedCode.split('\n');
                                            const sourceLines = currentOriginalCode.split('\n');

                                            // Get original text block
                                            const origBlock = (origStart === 0 || origEnd === 0) ? [] : sourceLines.slice(origStart - 1, origEnd);

                                            // Length to remove
                                            const removeCount = modEnd === 0 ? 0 : (modEnd - modStart + 1);
                                            const insertIndex = modEnd === 0 ? modStart : modStart - 1;

                                            targetLines.splice(insertIndex, removeCount, ...origBlock);
                                            const resultCode = targetLines.join('\n');
                                            onModifiedCodeChange(resultCode);
                                        };

                                        // Accept button
                                        const btnAccept = document.createElement('button');
                                        btnAccept.innerHTML = '<span style="display:flex;align-items:center;gap:6px;padding: 2px 6px;"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"></path></svg> Принять</span>';
                                        btnAccept.className = 'px-1 py-1 text-[11px] font-bold text-emerald-500 hover:text-emerald-400 hover:bg-emerald-500/10 rounded-sm transition-all active:scale-95 ml-1';
                                        btnAccept.onclick = (e) => {
                                            e.preventDefault();
                                            e.stopPropagation();

                                            // Mark as accepted (update local baseline)
                                            const originalEditor = editor.getOriginalEditor();
                                            const currentModifiedCode = modifiedEditor.getModel()?.getValue() || '';
                                            const currentOriginalCode = originalEditor.getModel()?.getValue() || '';

                                            const origStart = change.originalStartLineNumber;
                                            const origEnd = change.originalEndLineNumber;
                                            const modStart = change.modifiedStartLineNumber;
                                            const modEnd = change.modifiedEndLineNumber;

                                            let targetLines = currentOriginalCode.split('\n');
                                            const sourceLines = currentModifiedCode.split('\n');

                                            const modBlock = modEnd === 0 ? [] : sourceLines.slice(modStart - 1, modEnd);
                                            const removeCount = (origStart === 0 || origEnd === 0) ? 0 : (origEnd - origStart + 1);
                                            const insertIndex = (origStart === 0 || origEnd === 0) ? (modStart > 1 ? modStart - 1 : 0) : origStart - 1;

                                            targetLines.splice(insertIndex, removeCount, ...modBlock);
                                            const res = targetLines.join('\n');
                                            setLocalOriginalCode(res);

                                            // Force immediate clean of zones
                                            modifiedEditor.changeViewZones((acc: any) => {
                                                viewZoneIdsRef.current.forEach(id => acc.removeZone(id));
                                                viewZoneIdsRef.current = [];
                                            });
                                        };

                                        toolbar.appendChild(btnRevert);
                                        toolbar.appendChild(btnAccept);
                                        domNode.appendChild(toolbar);

                                        const id = accessor.addZone({
                                            afterLineNumber: change.modifiedEndLineNumber || change.modifiedStartLineNumber || 1,
                                            heightInPx: 30,
                                            domNode: domNode,
                                            suppressMouseDown: false
                                        });
                                        viewZoneIdsRef.current.push(id);
                                    });
                                });
                            };

                            // Вызываем при обновлениях редактора
                            editor.onDidUpdateDiff(updateInlineWidgets);

                            // Сохраняем ссылку на функцию обновления для внешнего использования
                            (editor as any).updateInlineWidgetsRef = updateInlineWidgets;
                        }}
                        options={{
                            minimap: { enabled: false },
                            fontSize: 12,
                            wordWrap: 'on',
                            renderSideBySide: false, // Inline diff
                            readOnly: false,
                            originalEditable: false,
                            automaticLayout: true,
                            ignoreTrimWhitespace: false, // Better to see all changes
                        }}
                    />
                )}
            </div>

            <div className="hidden">
                {/* Hack: Pre-load context menu logic? No, we do it in onMount */}
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
                                    <tr
                                        key={i}
                                        onClick={() => {
                                            if (editorRef.current) {
                                                editorRef.current.revealLineInCenter(d.line);
                                                editorRef.current.setPosition({ lineNumber: d.line, column: 1 });
                                                editorRef.current.focus();
                                            }
                                        }}
                                        className="border-b border-[#27272a]/30 hover:bg-[#27272a]/50 transition-colors group cursor-pointer"
                                    >
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
                    id="tour-apply"
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


