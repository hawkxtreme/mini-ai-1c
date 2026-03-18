import { useState, useMemo, useRef, useCallback, useEffect } from 'react';
import { DiffEditor, Editor, loader } from '@monaco-editor/react';
import { registerBSL } from '@/lib/monaco-bsl';
import { CodeSidePanelProps } from './types';
import { useResizing } from './useResizing';
import { Header } from './Header';
import { Footer } from './Footer';
import { DiagnosticsView } from './DiagnosticsView';
import { applyDiffWithDiagnostics, hasDiffBlocks } from '../../utils/diffViewer';
import { useSettings } from '@/contexts/SettingsContext';

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
    const [localOriginalCode, setLocalOriginalCode] = useState(originalCode ?? '');
    const { settings } = useSettings();
    const monacoTheme = settings?.theme === 'light' ? 'vs' : 'vs-dark';
    const {
        width, setWidth, isResizing, isExpanded, setIsExpanded, startResizing
    } = useResizing(window.innerWidth > 1200 ? 600 : 500);

    // Sync global Monaco theme when setting changes
    useEffect(() => {
        loader.init().then(monaco => {
            monaco.editor.setTheme(monacoTheme);
        });
    }, [monacoTheme]);

    // 1. Сброс стейта при полной очистке (Clear Chat)
    useEffect(() => {
        if (originalCode === '' && modifiedCode === '') {
            setLocalOriginalCode('');
            setPreviewFrozenCode(null);
        }
    }, [originalCode, modifiedCode]);

    // 2. Синхронизация при загрузке нового базового кода
    useEffect(() => {
        setLocalOriginalCode(originalCode ?? '');
        if (originalCode !== undefined) {
            setPreviewFrozenCode(null);
        }
    }, [originalCode]);

    const activeDiffContentRef = useRef(activeDiffContent);
    const editorRef = useRef<any>(null);
    const diffEditorRef = useRef<any>(null);
    const viewZoneIdsRef = useRef<string[]>([]);
    const [diffChanges, setDiffChanges] = useState<any[]>([]);
    const [currentDiffIndex, setCurrentDiffIndex] = useState(-1);

    // Рефы для актуального доступа к коду из замыканий Monaco (onMount вызывается один раз)
    // Приоритет: localOriginalCode (state) > modifiedCode
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
        // Используем актуальный код через рефы (НЕ через state в deps),
        // чтобы previewFrozenCode вычислялся ОДИН РАЗ при смене activeDiffContent
        // и НЕ пересчитывался при принятии чанков (setLocalOriginalCode не должен триггерить этот эффект).
        const baseForDiff = localOriginalCodeRef.current || baseCodeRef.current;
        const result = applyDiffWithDiagnostics(baseForDiff, activeDiffContent);
        setPreviewFrozenCode(result.code);
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
            // Явно очищаем viewZones (кнопки Принять/Отменить) при сбросе диффа,
            // чтобы они не "залипали" на экране при очистке чата.
            if (diffEditorRef.current) {
                const editor = diffEditorRef.current.getModifiedEditor();
                if (editor) {
                    editor.changeViewZones((accessor: any) => {
                        viewZoneIdsRef.current.forEach(id => accessor.removeZone(id));
                        viewZoneIdsRef.current = [];
                    });
                }
            }
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
        loader.init().then(monaco => {
            registerBSL(monaco);
        });
    }, []);

    // Принудительно вызываем layout при смене вкладки, т.к. automaticLayout отключен
    useEffect(() => {
        if (!isOpen) return;

        // При включенном automaticLayout дополнительный ручной поллинг не нужен
        // Monaco Editor сам отследит изменение размеров контейнера
        if (viewMode === 'editor' && editorRef.current) {
            editorRef.current.layout();
        } else if (viewMode === 'diff' && diffEditorRef.current) {
            diffEditorRef.current.layout();
        }
    }, [viewMode, isOpen]);

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
                        theme={monacoTheme}
                        value={modifiedCode}
                        onMount={(editor, monaco) => {
                            registerBSL(monaco);
                            editorRef.current = editor;
                        }}
                        onChange={(value) => onModifiedCodeChange(value || '')}
                        options={{
                            minimap: { enabled: false },
                            fontSize: 13,
                            lineNumbers: 'on',
                            scrollBeyondLastLine: false,
                            automaticLayout: true,
                            wordWrap: 'on',
                            readOnly: false,
                        }}
                    />
                ) : (
                    <DiffEditor
                        height="100%"
                        language="bsl"
                        theme={monacoTheme}
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
                                // Всегда очищаем старые зоны перед перерисовкой или выходом
                                modifiedEditor.changeViewZones((accessor: any) => {
                                    viewZoneIdsRef.current.forEach(id => accessor.removeZone(id));
                                    viewZoneIdsRef.current = [];
                                });

                                const getEffStart = (start: number, end: number) => end === 0 ? start + 1 : start;
                                const getEffEnd = (start: number, end: number) => end === 0 ? start : end;

                                const changes = editor.getLineChanges();

                                let mergedChanges: any[] = [];
                                if (changes && changes.length > 0) {
                                    // Конец визуального блока = последняя строка модифицированного чанка
                                    const getEndLine = (raw: any) =>
                                        raw.modifiedEndLineNumber || raw.modifiedStartLineNumber || 1;

                                    let current = {
                                        effOrigStart: getEffStart(changes[0].originalStartLineNumber, changes[0].originalEndLineNumber),
                                        effOrigEnd: getEffEnd(changes[0].originalStartLineNumber, changes[0].originalEndLineNumber),
                                        effModStart: getEffStart(changes[0].modifiedStartLineNumber, changes[0].modifiedEndLineNumber),
                                        effModEnd: getEffEnd(changes[0].modifiedStartLineNumber, changes[0].modifiedEndLineNumber),
                                        // Массив позиций кнопок: по одной после каждого визуального блока
                                        viewZoneLines: [getEndLine(changes[0])]
                                    };

                                    for (let i = 1; i < changes.length; i++) {
                                        const nextRaw = changes[i];
                                        const nextEffModStart = getEffStart(nextRaw.modifiedStartLineNumber, nextRaw.modifiedEndLineNumber);
                                        const nextEffModEnd = getEffEnd(nextRaw.modifiedStartLineNumber, nextRaw.modifiedEndLineNumber);
                                        const next = {
                                            effOrigStart: getEffStart(nextRaw.originalStartLineNumber, nextRaw.originalEndLineNumber),
                                            effOrigEnd: getEffEnd(nextRaw.originalStartLineNumber, nextRaw.originalEndLineNumber),
                                            effModStart: nextEffModStart,
                                            effModEnd: nextEffModEnd,
                                        };

                                        const gapOrig = next.effOrigStart - current.effOrigEnd - 1;
                                        const gapMod = next.effModStart - current.effModEnd - 1;
                                        const gap = Math.max(gapOrig, gapMod);

                                        if (gap <= 5) {
                                            // Одна merged-группа (один Accept/Reject захватывает всё)
                                            // Если визуальный разрыв в modified > 2 строк — добавляем кнопку
                                            // после каждого визуального блока внутри группы
                                            if (gapMod > 2) {
                                                current.viewZoneLines.push(getEndLine(nextRaw));
                                            } else {
                                                current.viewZoneLines[current.viewZoneLines.length - 1] = getEndLine(nextRaw);
                                            }
                                            current.effOrigEnd = Math.max(current.effOrigEnd, next.effOrigEnd);
                                            current.effModEnd = Math.max(current.effModEnd, next.effModEnd);
                                        } else {
                                            mergedChanges.push(current);
                                            current = { ...next, viewZoneLines: [getEndLine(nextRaw)] };
                                        }
                                    }
                                    mergedChanges.push(current);
                                }

                                setDiffChanges(mergedChanges);

                                // Авто-скролл к первому изменению при первом появлении диффа.
                                if (mergedChanges.length > 0 && !hasAutoScrolledRef.current) {
                                    hasAutoScrolledRef.current = true;
                                    modifiedEditor.revealLineInCenter(mergedChanges[0].viewZoneLines[0]);
                                }

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
                                    mergedChanges.forEach((change: any) => {
                                        const makeRevertHandler = () => (e: MouseEvent) => {
                                            e.preventDefault();
                                            e.stopPropagation();
                                            const originalEditor = editor.getOriginalEditor();
                                            const currentModifiedCode = modifiedEditor.getModel()?.getValue() || '';
                                            const currentOriginalCode = originalEditor.getModel()?.getValue() || '';
                                            const sourceLines = currentOriginalCode.split('\n');
                                            let targetLines = currentModifiedCode.split('\n');

                                            const removeStartIndex = change.effModStart - 1;
                                            const removeCount = Math.max(0, change.effModEnd - change.effModStart + 1);
                                            const extractStartIndex = change.effOrigStart - 1;
                                            const extractEndIndex = change.effOrigEnd;
                                            const extractCount = Math.max(0, extractEndIndex - extractStartIndex);
                                            const origBlock = extractCount > 0 ? sourceLines.slice(extractStartIndex, extractEndIndex) : [];

                                            targetLines.splice(removeStartIndex, removeCount, ...origBlock);

                                            anyChunkHandledRef.current = true;
                                            if (previewFrozenCodeRef.current !== null) {
                                                setPreviewFrozenCode(targetLines.join('\n'));
                                            } else {
                                                onModifiedCodeChange(targetLines.join('\n'));
                                            }
                                            setTimeout(updateInlineWidgets, 50);
                                        };

                                        const makeAcceptHandler = () => (e: MouseEvent) => {
                                            e.preventDefault();
                                            e.stopPropagation();
                                            const originalEditor = editor.getOriginalEditor();
                                            const currentModifiedCode = modifiedEditor.getModel()?.getValue() || '';
                                            const currentOriginalCode = originalEditor.getModel()?.getValue() || '';
                                            let targetLines = currentOriginalCode.split('\n');
                                            const sourceLines = currentModifiedCode.split('\n');

                                            const removeStartIndex = change.effOrigStart - 1;
                                            const removeCount = Math.max(0, change.effOrigEnd - change.effOrigStart + 1);
                                            const extractStartIndex = change.effModStart - 1;
                                            const extractEndIndex = change.effModEnd;
                                            const extractCount = Math.max(0, extractEndIndex - extractStartIndex);
                                            const modBlock = extractCount > 0 ? sourceLines.slice(extractStartIndex, extractEndIndex) : [];

                                            targetLines.splice(removeStartIndex, removeCount, ...modBlock);

                                            anyChunkHandledRef.current = true;
                                            setLocalOriginalCode(targetLines.join('\n'));
                                            modifiedEditor.changeViewZones((acc: any) => {
                                                viewZoneIdsRef.current.forEach(id => acc.removeZone(id));
                                                viewZoneIdsRef.current = [];
                                            });
                                            setTimeout(updateInlineWidgets, 50);
                                        };

                                        // Рендерим кнопки после каждого визуального блока внутри одной merged-группы
                                        change.viewZoneLines.forEach((vzLine: number) => {
                                        const domNode = document.createElement('div');
                                        domNode.className = 'flex items-center justify-end pr-8 gap-2 z-50 pointer-events-none';
                                        domNode.style.height = '18px';

                                        const toolbar = document.createElement('div');
                                        toolbar.className = 'flex items-center gap-1 bg-[#18181b]/80 backdrop-blur-sm border border-[#3f3f46]/30 rounded-md shadow-sm p-0 pointer-events-auto leading-none';

                                        const btnRevert = document.createElement('button');
                                        btnRevert.innerHTML = '<span style="display:flex;align-items:center;gap:4px;padding: 1px 4px;"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"></path><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"></path><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"></path></svg> Отменить</span>';
                                        btnRevert.className = 'px-1 py-0.5 text-[9px] font-bold text-zinc-400 hover:text-red-400 hover:bg-red-500/10 rounded-sm transition-all active:scale-95';
                                        btnRevert.onclick = makeRevertHandler();

                                        const btnAccept = document.createElement('button');
                                        btnAccept.innerHTML = '<span style="display:flex;align-items:center;gap:4px;padding: 1px 4px;"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"></path></svg> Принять</span>';
                                        btnAccept.className = 'px-1 py-0.5 text-[9px] font-bold text-emerald-500 hover:text-emerald-400 hover:bg-emerald-500/10 rounded-sm transition-all active:scale-95 ml-1';
                                        btnAccept.onclick = makeAcceptHandler();

                                        toolbar.appendChild(btnRevert);
                                        toolbar.appendChild(btnAccept);
                                        domNode.appendChild(toolbar);

                                        const id = accessor.addZone({
                                            afterLineNumber: vzLine,
                                            heightInPx: 18,
                                            domNode: domNode,
                                            suppressMouseDown: false
                                        });
                                        viewZoneIdsRef.current.push(id);
                                    });  // viewZoneLines.forEach
                                });  // mergedChanges.forEach
                                });  // changeViewZones
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
