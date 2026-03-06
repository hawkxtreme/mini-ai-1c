import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeRaw from 'rehype-raw';
import { PanelRight, ChevronDown, ChevronRight, BrainCircuit, Maximize2, Minimize2, X as CloseIcon, GitCompare } from 'lucide-react';
import { BslEditor } from './ui/BslEditor';
import { BslDiffEditor } from './ui/BslDiffEditor';
import { useState, useMemo, memo } from 'react';

interface MarkdownRendererProps {
    content: string;
    isStreaming?: boolean;
    onApplyCode?: (code: string) => void;
    originalCode?: string; // Original code for diff view
}

// Утилита для очистки diff-артефактов и технических фраз
export function cleanDiffArtifacts(content: string, originalCode?: string): string {
    if (!content) return '';
    let cleaned = content;

    // 0. Очищаем XML-формат вызова инструментов (Qwen / некоторые другие модели)
    // Формат: <function=name>\n<parameter=x>\n...\n</parameter>\n</function>
    // или: <function=name><parameter=x>...</parameter></function>
    cleaned = cleaned.replace(/<function=[^>]+>[\s\S]*?<\/function>/g, '');
    // Также удаляем незавершённые блоки (при стриминге)
    const qwenFnMatch = cleaned.match(/<function=[^>]+>/);
    if (qwenFnMatch) {
        cleaned = cleaned.split(qwenFnMatch[0])[0];
    }

    // 1. Удаляем малформед блоки: =======\nКОНТЕНТ\n>>>>>>> REPLACE без <<<<<<< SEARCH
    // Qwen Coder иногда пропускает SEARCH-часть и генерирует только REPLACE
    cleaned = cleaned.replace(/^={7}[\s\S]*?^>{5,10}\s+REPLACE\s*$/gm, '');

    // 2. Очищаем одиночные разделители ======= (невалидные полу-диффы без SEARCH/REPLACE)
    // Такой разделитель без <<<<<<< SEARCH и >>>>>>> REPLACE — мусор от модели
    cleaned = cleaned.replace(/^={7}\s*$/gm, '');

    // 3. Удаляем специфические префиксы, которые иногда добавляет ЛЛМ
    const metaPhrases = [
        /Ниже приведены исправления в формате SEARCH\/REPLACE:?/gi,
        /Ниже приведены исправления в формате «Поиск\/Замена»:?/gi,
        /Below are the changes in SEARCH\/REPLACE format:?/gi,
        /Ниже приведен код с исправлениями:?/gi
    ];

    for (const phrase of metaPhrases) {
        cleaned = cleaned.replace(phrase, '');
    }

    // 4. Скрываем завершенные блоки (match 5-10 chevrons)
    cleaned = cleaned.replace(/<{5,10} SEARCH[\s\S]*?>{5,10} REPLACE/g, '');

    // 5. Скрываем незавершенные блоки при стриминге или обрыве
    cleaned = cleaned.replace(/<{5,10} SEARCH[\s\S]*?={7}[\s\S]*?(?:\n|$)/g, '');
    cleaned = cleaned.replace(/<{5,10} SEARCH[\s\S]*?(?:\n|$)/g, '');

    // 6. Удаляем одиночные строки из шевронов и маркеры REPLACE/SEARCH
    cleaned = cleaned.replace(/^<{5,10}\s*$/gm, '');
    cleaned = cleaned.replace(/^>{5,10}\s*$/gm, '');
    cleaned = cleaned.replace(/^>{5,10}\s+REPLACE\s*$/gm, '');
    cleaned = cleaned.replace(/^<{5,10}\s+SEARCH\s*$/gm, '');
    cleaned = cleaned.replace(/^={7}\s*$/gm, '');

    // 7. Обработка нового XML формата <diff>
    if (!originalCode || originalCode.trim().length === 0) {
        // Если контекста нет, вытаскиваем текст из <replace> и рендерим как обычный блок
        cleaned = cleaned.replace(/<diff>[\s\S]*?<replace>([\s\S]*?)<\/replace>[\s\S]*?<\/diff>/g, (_, p1) => {
            return '\n```bsl\n' + p1.trim() + '\n```\n';
        });

        // Для стриминга: если тег открыт, но не закрыт
        if (cleaned.includes('<replace>') && !cleaned.includes('</replace>')) {
            cleaned = cleaned.replace(/<replace>([\s\S]*)$/, (_, p1) => {
                return '\n```bsl\n' + p1.trim() + '\n```\n';
            });
        }

        // Скрываем мусор от <diff> или <search>
        cleaned = cleaned.replace(/<diff>[\s\S]*?(?:<\/search>|<search>|$)/g, '');
    } else {
        // Очищаем XML формат <diff>, так как дифф будет отрендерен DiffViewer'ом
        cleaned = cleaned.replace(/<diff>[\s\S]*?<\/diff>/g, '');
        // Очищаем незавершенные XML блоки при стриминге
        if (cleaned.includes('<diff>') && !cleaned.includes('</diff>')) {
            cleaned = cleaned.replace(/<diff>[\s\S]*/, '');
        }
    }

    const hasBlocks = /<{5,10} SEARCH/.test(content);
    const result = cleaned.trim();

    if (!result && hasBlocks) {
        return '';
    }

    return result;
}

function ThoughtSection({ title, children }: { title: string, children: React.ReactNode }) {
    const [isCollapsed, setIsCollapsed] = useState(true);

    return (
        <div className="my-2 mb-4">
            <button
                onClick={() => setIsCollapsed(!isCollapsed)}
                className="flex items-center gap-2 text-[11px] text-white/40 hover:text-white/60 uppercase tracking-widest font-semibold transition-colors group mb-1.5"
            >
                <BrainCircuit className="w-3.5 h-3.5" />
                <span>{title}</span>
                <ChevronRight className={`w-3.5 h-3.5 transition-transform ${!isCollapsed ? 'rotate-90' : ''}`} />
            </button>
            {!isCollapsed && (
                <div className="text-[12px] italic text-white/40 leading-relaxed border-l-2 border-white/10 pl-3 py-1 my-2 animate-in fade-in slide-in-from-top-1">
                    {children}
                </div>
            )}
        </div>
    );
}

const ThoughtSectionMemo = memo(ThoughtSection);

interface CodeBlockProps {
    inline?: boolean;
    className?: string;
    children: any;
    isStreaming?: boolean;
    onApplyCode?: (code: string) => void;
    originalCode?: string;
    [key: string]: any;
}

const CodeBlock = memo(({ inline, className, children, isStreaming, onApplyCode, originalCode, ...props }: CodeBlockProps) => {
    const match = /language-(\w+)/.exec(className || '');
    const language = match ? match[1] : '';
    const isBsl = language === 'bsl' || language === '1c';
    const codeString = String(children).replace(/\n$/, '');
    const isMultiline = codeString.includes('\n');

    if (inline || !isMultiline) {
        return (
            <code
                className="bg-[#27272a] text-blue-300 font-semibold px-1.5 py-0.5 rounded-md text-[13px] font-mono border border-zinc-700/50 max-w-full overflow-x-auto inline-flex align-middle scrollbar-hide"
                style={{ verticalAlign: 'middle', whiteSpace: 'nowrap' }}
                {...props}
            >
                {children}
            </code>
        );
    }

    if (isStreaming) {
        return (
            <div className="relative my-4 group w-full">
                <div className="flex items-center justify-between px-3 py-1.5 bg-zinc-800/80 backdrop-blur-sm rounded-t-lg border-x border-t border-[#27272a]">
                    <div className="flex items-center gap-2">
                        <div className="w-2 h-2 rounded-full bg-blue-500 animate-pulse" />
                        <span className="text-[10px] font-bold text-zinc-400 uppercase tracking-widest">{isBsl ? 'BSL (1C:Enterprise)' : (language || 'code')}</span>
                    </div>
                </div>
                <div className="bg-[#1e1e1e] border border-[#27272a] rounded-b-lg overflow-hidden border-t-0 min-h-[50px] max-h-[400px] flex">
                    <pre className="p-4 overflow-auto w-full text-zinc-300 text-[13px] font-mono whitespace-pre flex-1 custom-scrollbar">
                        {codeString}
                    </pre>
                </div>
            </div>
        );
    }

    if (isBsl) {
        const [isFullscreen, setIsFullscreen] = useState(false);
        const [showDiff, setShowDiff] = useState(false); // Default to Code View to avoid "Red Wall" confusion
        const hasDiff = originalCode && originalCode.trim().length > 0;

        return (
            <>
                <div className="relative my-4 group w-full">
                    <div className="flex flex-wrap items-center justify-between gap-y-1 px-3 py-1.5 bg-zinc-800/80 backdrop-blur-sm rounded-t-lg border-x border-t border-[#27272a]">
                        <div className="flex items-center gap-2">
                            <div className="w-2 h-2 rounded-full bg-emerald-500" />
                            <span className="text-[10px] font-bold text-zinc-400 uppercase tracking-widest whitespace-nowrap">BSL (1C:Enterprise)</span>
                            {hasDiff && (
                                <span className="text-[9px] px-1.5 py-0.5 rounded bg-blue-500/20 text-blue-400 font-bold uppercase whitespace-nowrap">
                                    Changes
                                </span>
                            )}
                        </div>
                        <div className="flex items-center gap-1.5 ml-auto">
                            {hasDiff && (
                                <button
                                    onClick={() => setShowDiff(!showDiff)}
                                    className={`p-1 px-2 text-[11px] font-medium transition-all rounded-md flex items-center gap-1 whitespace-nowrap ${showDiff
                                        ? 'bg-blue-500/20 text-blue-400'
                                        : 'text-zinc-400 hover:text-white hover:bg-zinc-700/50'
                                        }`}
                                    title={showDiff ? "Show code only" : "Show diff"}
                                >
                                    <GitCompare className="w-3.5 h-3.5" />
                                    <span>{showDiff ? 'Diff' : 'Code'}</span>
                                </button>
                            )}
                            <button
                                onClick={() => setIsFullscreen(true)}
                                className="p-1 px-2 text-[11px] font-medium text-zinc-400 hover:text-white transition-all hover:bg-zinc-700/50 rounded-md flex items-center gap-1 whitespace-nowrap"
                                title="Maximize"
                            >
                                <Maximize2 className="w-3.5 h-3.5" />
                                <span>Max</span>
                            </button>
                            {onApplyCode && (
                                <button
                                    onClick={() => onApplyCode(codeString)}
                                    className="flex items-center gap-1.5 px-2 py-0.5 text-[11px] font-medium text-blue-400 hover:text-blue-300 transition-all hover:bg-blue-400/10 rounded-md whitespace-nowrap"
                                    title="Load into Side Panel"
                                >
                                    <PanelRight className="w-3.5 h-3.5" />
                                    <span>Apply</span>
                                </button>
                            )}
                        </div>
                    </div>
                    {showDiff && hasDiff ? (
                        <BslDiffEditor
                            original={originalCode}
                            modified={codeString}
                            height={Math.min(400, Math.max(codeString.split('\n').length, originalCode.split('\n').length) * 20 + 20)}
                        />
                    ) : (
                        <BslEditor code={codeString} height={Math.min(400, (codeString.split('\n').length * 20) + 20)} />
                    )}
                </div>

                {isFullscreen && (
                    <div className="fixed inset-0 z-[100] bg-zinc-950/95 flex flex-col backdrop-blur-md animate-in fade-in zoom-in duration-200">
                        <div className="flex items-center justify-between px-6 py-3 border-b border-zinc-800 bg-zinc-900/50">
                            <div className="flex items-center gap-3">
                                <div className="w-3 h-3 rounded-full bg-emerald-500 shadow-[0_0_10px_rgba(16,185,129,0.3)]" />
                                <span className="text-xs font-bold text-zinc-300 uppercase tracking-[0.2em]">BSL Fullscreen View</span>
                            </div>
                            <div className="flex items-center gap-4">
                                {hasDiff && (
                                    <button
                                        onClick={() => setShowDiff(!showDiff)}
                                        className={`flex items-center gap-2 px-4 py-1.5 rounded-lg transition-all text-xs font-semibold ${showDiff
                                            ? 'bg-blue-600 hover:bg-blue-500 text-white'
                                            : 'bg-zinc-800 hover:bg-zinc-700 text-zinc-300'
                                            }`}
                                    >
                                        <GitCompare className="w-4 h-4" />
                                        <span>{showDiff ? 'Show Code Only' : 'Show Diff'}</span>
                                    </button>
                                )}
                                {onApplyCode && (
                                    <button
                                        onClick={() => {
                                            onApplyCode(codeString);
                                            setIsFullscreen(false);
                                        }}
                                        className="flex items-center gap-2 px-4 py-1.5 bg-blue-600 hover:bg-blue-500 text-white rounded-lg transition-all text-xs font-semibold shadow-lg shadow-blue-900/20"
                                    >
                                        <PanelRight className="w-4 h-4" />
                                        <span>Apply Changes & Close</span>
                                    </button>
                                )}
                                <button
                                    onClick={() => setIsFullscreen(false)}
                                    className="p-2 hover:bg-zinc-800 rounded-full transition-all text-zinc-400 hover:text-white hover:rotate-90 duration-300"
                                >
                                    <CloseIcon className="w-6 h-6" />
                                </button>
                            </div>
                        </div>
                        <div className="flex-1 p-8 overflow-hidden">
                            <div className="w-full h-full rounded-2xl border border-zinc-800 overflow-hidden shadow-2xl bg-[#1e1e1e]">
                                {showDiff && hasDiff ? (
                                    <BslDiffEditor original={originalCode} modified={codeString} height="100%" hideBorder className="h-full" />
                                ) : (
                                    <BslEditor code={codeString} height="100%" hideBorder className="h-full" />
                                )}
                            </div>
                        </div>
                    </div>
                )}
            </>
        );
    }

    return (
        <div className="relative my-2 group w-full">
            <div className="flex items-center justify-between px-3 py-1 bg-zinc-800 rounded-t-lg border-x border-t border-[#27272a]">
                <div className="flex items-center gap-2">
                    <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-widest">{language || 'code'}</span>
                </div>
            </div>
            <pre className="bg-[#18181b] border border-[#27272a] rounded-b-lg p-4 overflow-x-auto border-t-0 text-zinc-300">
                <code className={`text-[13px] font-mono leading-relaxed ${className || ''}`} {...props}>
                    {children}
                </code>
            </pre>
        </div>
    );
});

// Fix unclosed code blocks during streaming — prevents content from "falling into" a code block
function fixStreamingMarkdown(content: string): string {
    const codeBlockCount = (content.match(/```/g) || []).length;
    if (codeBlockCount % 2 !== 0) {
        return content + '\n```';
    }
    return content;
}

export const MarkdownRenderer = memo(function MarkdownRenderer({ content, isStreaming = false, onApplyCode, originalCode }: MarkdownRendererProps) {
    const components = useMemo(() => ({
        // Handle <thought> or <thinking> tags as collapsible sections
        thought: (({ children }: any) => <ThoughtSection title="Reasoning">{children}</ThoughtSection>) as any,
        thinking: (({ children }: any) => <ThoughtSection title="Thinking">{children}</ThoughtSection>) as any,

        code: (props: any) => <CodeBlock {...props} isStreaming={isStreaming} onApplyCode={onApplyCode} originalCode={originalCode} />,
        // Styled paragraphs
        p({ children }: any) {
            return <p className="mb-3 last:mb-0 leading-relaxed text-zinc-300">{children}</p>;
        },
        // Styled lists
        ul({ children }: any) {
            return <ul className="list-disc list-outside ml-4 mb-4 space-y-1.5 text-zinc-300">{children}</ul>;
        },
        ol({ children }: any) {
            return <ol className="list-decimal list-outside ml-4 mb-4 space-y-1.5 text-zinc-300">{children}</ol>;
        },
        // Styled links
        a({ href, children }: any) {
            return (
                <a href={href} className="text-blue-400 hover:underline decoration-blue-400/30 underline-offset-4" target="_blank" rel="noopener noreferrer">
                    {children}
                </a>
            );
        },
        // Styled headings
        h1({ children }: any) {
            return <h1 className="text-xl font-bold mb-4 mt-6 text-white border-b border-zinc-800 pb-2 leading-tight">{children}</h1>;
        },
        h2({ children }: any) {
            return <h2 className="text-lg font-semibold mb-3 mt-5 text-zinc-100 leading-snug">{children}</h2>;
        },
        h3({ children }: any) {
            return <h3 className="text-base font-semibold mb-2 mt-4 text-zinc-200">{children}</h3>;
        },
        // Styled blockquotes
        blockquote({ children }: any) {
            return (
                <blockquote className="border-l-4 border-zinc-700 pl-4 my-4 text-zinc-400 italic bg-zinc-900/50 py-2 pr-4 rounded-r-md">
                    {children}
                </blockquote>
            );
        },
        // Styled tables
        table({ children }: any) {
            return (
                <div className="overflow-x-auto my-6 rounded-lg border border-zinc-800 shadow-sm">
                    <table className="min-w-full border-collapse bg-zinc-900/30">{children}</table>
                </div>
            );
        },
        th({ children }: any) {
            return <th className="bg-zinc-800/80 px-4 py-2.5 text-left border-b border-zinc-700 text-zinc-300 font-semibold text-sm uppercase tracking-wider">{children}</th>;
        },
        td({ children }: any) {
            return <td className="px-4 py-2.5 border-b border-zinc-800 text-zinc-400 text-sm leading-relaxed">{children}</td>;
        },
    }), [isStreaming, onApplyCode, originalCode]);

    const processedContent = isStreaming
        ? fixStreamingMarkdown(cleanDiffArtifacts(content, originalCode))
        : cleanDiffArtifacts(content, originalCode);

    return (
        <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            rehypePlugins={[rehypeRaw]}
            components={components as any}
        >
            {processedContent}
        </ReactMarkdown>
    );
});
