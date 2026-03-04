import { useState, useMemo, useRef, useCallback, useEffect } from 'react';
import { DiffEditor, Editor, loader } from '@monaco-editor/react';
import { registerBSL } from '@/lib/monaco-bsl';
import { CodeSidePanelProps } from './types';
import { useResizing } from './useResizing';
import { Header } from './Header';
import { Footer } from './Footer';
import { DiagnosticsView } from './DiagnosticsView';
import { applyDiffWithDiagnostics, hasDiffBlocks } from '../../utils/diffViewer';

export { type BslDiagnostic, type CodeSidePanelProps } from './types';

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
    onActiveDiffChange,
    onDiffRejected,
    isFullWidth
}: CodeSidePanelProps) {
    const [viewMode, setViewMode] = useState<'editor' | 'diff'>('diff');
    const [localOriginalCode, setLocalOriginalCode] = useState(originalCode);
    const {
        width, setWidth, isResizing, isExpanded, setIsExpanded, startResizing
    } = useResizing(window.innerWidth > 1200 ? 600 : 500);

    useEffect(() => {
        setLocalOriginalCode(originalCode);
    }, [originalCode]);

    const activeDiffContentRef = useRef(activeDiffContent);
    const editorRef = useRef<any>(null);
    const diffEditorRef = useRef<any>(null);
    const viewZoneIdsRef = useRef<string[]>([]);
    const [diffChanges, setDiffChanges] = useState<any[]>([]);
    const [currentDiffIndex, setCurrentDiffIndex] = useState(-1);

    // Рефы для актуального доступа к коду из замыканий Monaco (onMount вызывается один раз)
    const baseCodeRef = useRef(localOriginalCode || modifiedCode);
    baseCodeRef.current = localOriginalCode || modifiedCode;
    const localOriginalCodeRef = useRef(localOriginalCode);
    localOriginalCodeRef.current = localOriginalCode;

    // ЗАМОРОЖЕННЫЙ превью-код: вычисляется ОДИН РАЗ при изменении activeDiffContent
    // и НЕ пересчитывается при принятии чанков — это предотвращает повторный fuzzy-match
    // уже принятых блоков (баг: REPLACE ≈ SEARCH по схожести ≥85% → блок применялся повторно).
    const [previewFrozenCode, setPreviewFrozenCode] = useState<string | null>(null);
    const previewFrozenCodeRef = useRef<string | null>(null);
    previewFrozenCodeRef.current = previewFrozenCode;

    // Флаг: пользователь хотя бы раз нажал Accept/Revert в текущем превью.
    // Защищает auto-commit от срабатывания до первого взаимодействия пользователя
    // (Monaco возвращает changes=[] до завершения вычисления диффа).
    const anyChunkHandledRef = useRef(false);
    // Флаг: авто-скролл к первому изменению уже выполнен для текущего превью.
    // Сбрасывается при смене activeDiffContent — чтобы не скроллить повторно.
    const hasAutoScrolledRef = useRef(false);

    useEffect(() => {
        anyChunkHandledRef.current = false;
        hasAutoScrolledRef.current = false;
        if (!activeDiffContent || !hasDiffBlocks(activeDiffContent)) {
            setPreviewFrozenCode(null);
            return;
        }
        const result = applyDiffWithDiagnostics(baseCodeRef.current, activeDiffContent);
        setPreviewFrozenCode(result.code);
    }, [activeDiffContent]);

    // Ref-флаг для блокировки onChange во время превью.
    // Устанавливаем СИНХРОННО во время рендера — Monaco стреляет onDidChangeModelContent
    // до того, как useEffect успеет обновить ref, поэтому useEffect здесь недостаточен.
    // ВАЖНО: используем !== null, а не !!, чтобы пустая строка "" тоже считалась превью.
    const previewModeRef = useRef(false);
    previewModeRef.current = previewFrozenCode !== null;

    useEffect(() => {
        activeDiffContentRef.current = activeDiffContent;
        if (activeDiffContent && viewMode !== 'diff') {
            setViewMode('diff');
        } else if (!activeDiffContent && viewMode === 'diff') {
            setViewMode('editor');
            setDiffChanges([]);
        }

        if (diffEditorRef.current?.updateInlineWidgetsRef && activeDiffContent) {
            setTimeout(() => {
                diffEditorRef.current.updateInlineWidgetsRef();
            }, 50);
        }
    }, [activeDiffContent, viewMode]);

    const goToDiff = useCallback((index: number) => {
        if (!diffChanges[index] || !editorRef.current) return;
        const change = diffChanges[index];
        const line = change.modifiedStartLineNumber || change.originalStartLineNumber || 1;
        editorRef.current.revealLineInCenter(line);
        setCurrentDiffIndex(index);
        editorRef.current.focus();
    }, [diffChanges]);

    const nextDiff = useCallback(() => {
        if (diffChanges.length === 0) return;
        const nextIndex = (currentDiffIndex + 1) % diffChanges.length;
        goToDiff(nextIndex);
    }, [currentDiffIndex, diffChanges, goToDiff]);

    const prevDiff = useCallback(() => {
        if (diffChanges.length === 0) return;
        const prevIndex = (currentDiffIndex - 1 + diffChanges.length) % diffChanges.length;
        goToDiff(prevIndex);
    }, [currentDiffIndex, diffChanges, goToDiff]);

    useEffect(() => {
        if (diffChanges.length === 0) {
            setCurrentDiffIndex(-1);
        } else if (currentDiffIndex >= diffChanges.length) {
            setCurrentDiffIndex(diffChanges.length - 1);
        }
    }, [diffChanges.length, currentDiffIndex]);

    const errorCount = useMemo(() => diagnostics.filter(d => d.severity === 'error').length, [diagnostics]);
    const warningCount = useMemo(() => diagnostics.filter(d => d.severity !== 'error').length, [diagnostics]);

    useEffect(() => {
        if (!activeDiffContent) {
            setDiffChanges([]);
        }
    }, [activeDiffContent]);

    useEffect(() => {
        if (viewMode === 'diff' && diffEditorRef.current && diffEditorRef.current.updateInlineWidgetsRef) {
            setTimeout(() => {
                diffEditorRef.current.updateInlineWidgetsRef();
            }, 100);
        }
    }, [activeDiffContent, viewMode, modifiedCode]);

    useEffect(() => {
        loader.init().then(monaco => {
            registerBSL(monaco);
        });
    }, []);

    useEffect(() => {
        if (isOpen) {
            setIsExpanded(true);
            setWidth(window.innerWidth > 1200 ? 600 : 500);
        }
    }, [isOpen, setWidth, setIsExpanded]);

    if (!isOpen) return null;

    return (
        <div
            id="code-side-panel"
            style={{ width: isFullWidth ? '100%' : (isExpanded ? `${width}px` : '280px') }}
            className={`border-l border-[#27272a] bg-[#09090b] flex flex-col h-full shadow-2xl transition-[width] duration-300 ease-in-out relative ${isResizing || isFullWidth ? 'transition-none' : ''} ${isFullWidth ? 'w-full' : 'flex-shrink-0'}`}
        >
            {/* Resize Handle */}
            <div
                onMouseDown={startResizing}
                className="absolute left-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-blue-500/30 transition-colors z-50 flex items-center justify-center group"
            >
                <div className="w-0.5 h-8 bg-zinc-700 group-hover:bg-blue-400 rounded-full opacity-0 group-hover:opacity-100" />
            </div>

            <Header
                viewMode={viewMode}
                setViewMode={setViewMode}
                isValidating={isValidating}
                errorCount={errorCount}
                warningCount={warningCount}
                diffChanges={diffChanges}
                currentDiffIndex={currentDiffIndex}
                prevDiff={prevDiff}
                nextDiff={nextDiff}
                onClose={onClose}
                setIsExpanded={setIsExpanded}
                isExpanded={isExpanded}
                isFullWidth={isFullWidth}
                diffEditorRef={diffEditorRef}
                onModifiedCodeChange={onModifiedCodeChange}
                onActiveDiffChange={onActiveDiffChange}
                onDiffRejected={onDiffRejected}
                foldAll={() => editorRef.current?.trigger('fold-all', 'editor.foldAll')}
            />

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
                        original={(localOriginalCode || modifiedCode).replace(/\r\n/g, '\n')}
                        modified={(previewFrozenCode !== null ? previewFrozenCode : modifiedCode).replace(/\r\n/g, '\n')}
                        onMount={(editor, monaco) => {
                            registerBSL(monaco);
                            diffEditorRef.current = editor;
                            const modifiedEditor = editor.getModifiedEditor();
                            editorRef.current = modifiedEditor;

                            modifiedEditor.onDidChangeModelContent(() => {
                                // В режиме превью не перезаписываем modifiedCode — это предпросмотр
                                if (!previewModeRef.current) {
                                    onModifiedCodeChange(modifiedEditor.getValue());
                                }
                            });

                            const updateInlineWidgets = () => {
                                const changes = editor.getLineChanges();
                                setDiffChanges(changes || []);

                                // Авто-скролл к первому изменению при первом появлении диффа.
                                // Решает проблему: Monaco при вставке строк в начало файла
                                // (originalStartLineNumber=0) скроллит к "оригинальной строке 1",
                                // которая в modified стоит после вставленных строк → зелёные
                                // строки уходят выше экрана и оказываются невидимы.
                                if (changes && changes.length > 0 && !hasAutoScrolledRef.current) {
                                    hasAutoScrolledRef.current = true;
                                    const firstChange = changes[0];
                                    const targetLine = firstChange.modifiedStartLineNumber
                                        || firstChange.originalStartLineNumber
                                        || 1;
                                    modifiedEditor.revealLineInCenter(targetLine);
                                }

                                modifiedEditor.changeViewZones((accessor: any) => {
                                    viewZoneIdsRef.current.forEach(id => accessor.removeZone(id));
                                    viewZoneIdsRef.current = [];
                                });

                                const currentContent = activeDiffContentRef.current;
                                if (!currentContent || changes === null) return;

                                if (changes.length === 0) {
                                    // Только если пользователь уже нажал Accept/Revert хотя бы раз —
                                    // иначе Monaco может вернуть [] до завершения вычисления диффа.
                                    if (anyChunkHandledRef.current && previewFrozenCodeRef.current !== null) {
                                        // Все чанки обработаны — фиксируем принятый код.
                                        onModifiedCodeChange(localOriginalCodeRef.current);
                                        anyChunkHandledRef.current = false;
                                        // НЕ вызываем setPreviewFrozenCode(null) здесь явно!
                                        // Если очистить previewFrozenCode до обновления modifiedCode,
                                        // DiffEditor увидит старый modifiedCode и покажет 13+ "призрачных" блоков.
                                        // Вместо этого: onActiveDiffChange('') очистит activeDiffContent,
                                        // и previewFrozenCode уберётся естественно через свой useEffect.
                                        if (onActiveDiffChange) {
                                            setTimeout(() => onActiveDiffChange(''), 0);
                                        }
                                    }
                                    return;
                                }

                                modifiedEditor.changeViewZones((accessor: any) => {
                                    changes.forEach((change: any) => {
                                        const domNode = document.createElement('div');
                                        domNode.className = 'flex items-center justify-end pr-8 gap-2 z-50 pointer-events-none';
                                        domNode.style.height = '18px';

                                        const toolbar = document.createElement('div');
                                        toolbar.className = 'flex items-center gap-1 bg-[#18181b]/80 backdrop-blur-sm border border-[#3f3f46]/30 rounded-md shadow-sm p-0 pointer-events-auto leading-none';

                                        const btnRevert = document.createElement('button');
                                        btnRevert.innerHTML = '<span style="display:flex;align-items:center;gap:4px;padding: 1px 4px;"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"></path><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"></path><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"></path></svg> Отменить</span>';
                                        btnRevert.className = 'px-1 py-0.5 text-[9px] font-bold text-zinc-400 hover:text-red-400 hover:bg-red-500/10 rounded-sm transition-all active:scale-95';
                                        btnRevert.onclick = (e) => {
                                            e.preventDefault();
                                            e.stopPropagation();
                                            const originalEditor = editor.getOriginalEditor();
                                            const currentModifiedCode = modifiedEditor.getModel()?.getValue() || '';
                                            const currentOriginalCode = originalEditor.getModel()?.getValue() || '';
                                            const origStart = change.originalStartLineNumber;
                                            const origEnd = change.originalEndLineNumber;
                                            const modStart = change.modifiedStartLineNumber;
                                            const modEnd = change.modifiedEndLineNumber;
                                            let targetLines = currentModifiedCode.split('\n');
                                            const sourceLines = currentOriginalCode.split('\n');
                                            const origBlock = (origStart === 0 || origEnd === 0) ? [] : sourceLines.slice(origStart - 1, origEnd);
                                            const removeCount = modEnd === 0 ? 0 : (modEnd - modStart + 1);
                                            const insertIndex = modEnd === 0 ? modStart : modStart - 1;
                                            targetLines.splice(insertIndex, removeCount, ...origBlock);
                                            // В режиме превью откатываем в замороженном превью-коде,
                                            // не трогая modifiedCode (он обновится при завершении превью)
                                            anyChunkHandledRef.current = true;
                                            if (previewFrozenCodeRef.current !== null) {
                                                setPreviewFrozenCode(targetLines.join('\n'));
                                            } else {
                                                onModifiedCodeChange(targetLines.join('\n'));
                                            }
                                            setTimeout(updateInlineWidgets, 50);
                                        };

                                        const btnAccept = document.createElement('button');
                                        btnAccept.innerHTML = '<span style="display:flex;align-items:center;gap:4px;padding: 1px 4px;"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"></path></svg> Принять</span>';
                                        btnAccept.className = 'px-1 py-0.5 text-[9px] font-bold text-emerald-500 hover:text-emerald-400 hover:bg-emerald-500/10 rounded-sm transition-all active:scale-95 ml-1';
                                        btnAccept.onclick = (e) => {
                                            e.preventDefault();
                                            e.stopPropagation();
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
                                            anyChunkHandledRef.current = true;
                                            setLocalOriginalCode(targetLines.join('\n'));
                                            modifiedEditor.changeViewZones((acc: any) => {
                                                viewZoneIdsRef.current.forEach(id => acc.removeZone(id));
                                                viewZoneIdsRef.current = [];
                                            });
                                            setTimeout(updateInlineWidgets, 50);
                                        };

                                        toolbar.appendChild(btnRevert);
                                        toolbar.appendChild(btnAccept);
                                        domNode.appendChild(toolbar);

                                        const id = accessor.addZone({
                                            afterLineNumber: change.modifiedEndLineNumber || change.modifiedStartLineNumber || 1,
                                            heightInPx: 18,
                                            domNode: domNode,
                                            suppressMouseDown: false
                                        });
                                        viewZoneIdsRef.current.push(id);
                                    });
                                });
                            };

                            editor.onDidUpdateDiff(updateInlineWidgets);
                            (editor as any).updateInlineWidgetsRef = updateInlineWidgets;
                        }}
                        options={{
                            minimap: { enabled: false },
                            fontSize: 12,
                            wordWrap: 'on',
                            renderSideBySide: false,
                            readOnly: previewFrozenCode !== null,
                            originalEditable: false,
                            automaticLayout: true,
                            ignoreTrimWhitespace: false,
                        }}
                    />
                )}
            </div>

            <DiagnosticsView
                diagnostics={diagnostics}
                onDiagnosticClick={(targetLine) => {
                    if (editorRef.current) {
                        editorRef.current.revealLineInCenter(targetLine);
                        editorRef.current.setPosition({ lineNumber: targetLine, column: 1 });
                        editorRef.current.focus();
                    }
                }}
            />

            <Footer
                onApply={onApply}
                isApplying={isApplying}
                modifiedCode={modifiedCode}
            />
        </div>
    );
}
