import { useEffect } from 'react';
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
                original={original}
                modified={modified}
                loading={loading || defaultLoading}
                options={{
                    readOnly,
                    minimap: { enabled: false },
                    fontSize: 13,
                    lineNumbers: 'on',
                    scrollBeyondLastLine: false,
                    automaticLayout: true,
                    padding: { top: 8, bottom: 8 },
                    renderLineHighlight: 'none',
                    folding: true,
                    renderSideBySide: true,
                    enableSplitViewResizing: true,
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
