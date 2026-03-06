import { useEffect, useRef } from 'react';
import { DiffEditor, loader } from '@monaco-editor/react';
import { registerBSL } from '@/lib/monaco-bsl';

interface BslDiffEditorProps {
    original: string;
    modified: string;
    height?: string | number;
    readOnly?: boolean;
    loading?: React.ReactNode;
    className?: string;
    hideBorder?: boolean;
}

export function BslDiffEditor({
    original,
    modified,
    height = '400px',
    readOnly = true,
    loading,
    className,
    hideBorder = false
}: BslDiffEditorProps) {
    const editorRef = useRef<any>(null);

    // Normalize line endings to LF to prevent Monaco from highlighting the entire file as changed
    const normalizedOriginal = original ? original.replace(/\r\n/g, '\n') : '';
    const normalizedModified = modified ? modified.replace(/\r\n/g, '\n') : '';

    // Register BSL language once
    useEffect(() => {
        loader.init().then(monaco => {
            registerBSL(monaco);
        });
    }, []);

    const defaultLoading = (
        <div className="bg-[#1e1e1e] p-4 text-zinc-300 text-[13px] font-mono">
            <div className="opacity-50">Loading diff...</div>
        </div>
    );

    return (
        <div
            className={`overflow-hidden transition-all duration-300 ${!hideBorder ? 'rounded-b-lg border border-[#27272a] border-t-0' : ''} ${className || ''}`}
            style={{ height: typeof height === 'number' ? `${height}px` : height }}
        >
            <DiffEditor
                height="100%"
                language="bsl"
                theme="vs-dark"
                original={normalizedOriginal}
                modified={normalizedModified}
                onMount={(editor) => {
                    editorRef.current = editor;

                    const diffObserver = new ResizeObserver(() => {
                        window.requestAnimationFrame(() => {
                            if (editorRef.current) {
                                editorRef.current.layout();
                            }
                        });
                    });

                    const container = editor.getContainerDomNode();
                    if (container) {
                        diffObserver.observe(container);
                    }

                    editor.onDidDispose(() => {
                        diffObserver.disconnect();
                    });

                    // Первичный layout с ожиданием монтирования DOM
                    let attempts = 0;
                    const checkAndLayout = () => {
                        if (attempts > 10) return;
                        attempts++;
                        if (container && container.clientWidth > 0) {
                            editor.layout();
                            return;
                        }
                        setTimeout(checkAndLayout, 50);
                    };
                    checkAndLayout();
                }}
                options={{
                    readOnly,
                    minimap: { enabled: false },
                    fontSize: 13,
                    lineNumbers: 'on',
                    scrollBeyondLastLine: false,
                    automaticLayout: false,
                    padding: { top: 8, bottom: 8 },
                    renderLineHighlight: 'none',
                    folding: true,
                    renderSideBySide: true,
                    enableSplitViewResizing: true,
                    // Используем продвинутый алгоритм сравнения (Myers diff)
                    // для лучшего сопоставления строк при добавлении/удалении блоков
                    diffAlgorithm: 'advanced',
                    // Игнорируем незначительные изменения пробелов
                    ignoreTrimWhitespace: false,
                    // Показываем только изменения
                    renderIndicators: true,
                    scrollbar: {
                        vertical: 'auto',
                        horizontal: 'auto',
                        verticalScrollbarSize: 8,
                        horizontalScrollbarSize: 8
                    }
                }}
            />
        </div>
    );
}
