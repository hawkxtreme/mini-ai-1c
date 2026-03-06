import { useEffect, useRef } from 'react';
import { Editor, loader } from '@monaco-editor/react';
import { registerBSL } from '@/lib/monaco-bsl';

interface BslEditorProps {
    code: string;
    height?: string | number;
    readOnly?: boolean;
    loading?: React.ReactNode;
    className?: string;
    hideBorder?: boolean;
}

export function BslEditor({ code, height = '200px', readOnly = true, loading, className, hideBorder = false }: BslEditorProps) {
    const editorRef = useRef<any>(null);

    // Register BSL language once
    useEffect(() => {
        loader.init().then(monaco => {
            registerBSL(monaco);
        });
    }, []);

    const defaultLoading = (
        <pre className="bg-[#1e1e1e] p-4 text-zinc-300 text-[13px] font-mono whitespace-pre opacity-50">
            {code}
        </pre>
    );

    return (
        <div
            className={`overflow-hidden transition-all duration-300 ${!hideBorder ? 'rounded-b-lg border border-[#27272a] border-t-0' : ''} ${className || ''}`}
            style={{ height: typeof height === 'number' ? `${height}px` : height }}
        >
            <Editor
                height="100%"
                language="bsl"
                theme="vs-dark"
                value={code}
                loading={loading || defaultLoading}
                onMount={(editor) => {
                    editorRef.current = editor;

                    const editorObserver = new ResizeObserver(() => {
                        window.requestAnimationFrame(() => {
                            if (editorRef.current) {
                                editorRef.current.layout();
                            }
                        });
                    });

                    const container = editor.getContainerDomNode();
                    if (container) {
                        editorObserver.observe(container);
                    }

                    editor.onDidDispose(() => {
                        editorObserver.disconnect();
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
