import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { PanelRight, CheckCircle2, AlertCircle, Info } from 'lucide-react';
import { BslDiagnostic } from '../App';

interface MarkdownRendererProps {
    content: string;
    onApplyCode?: (code: string) => void;
}

export function MarkdownRenderer({ content, onApplyCode }: MarkdownRendererProps) {

    return (
        <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            components={{
                // Code blocks with syntax highlighting styling
                code({ inline, className, children, ...props }: any) {
                    const match = /language-(\w+)/.exec(className || '');
                    const language = match ? match[1] : '';
                    const isBsl = language === 'bsl' || language === '1c';


                    if (inline) {
                        return (
                            <code className="bg-[#27272a] text-zinc-200 px-1.5 py-0.5 rounded-md text-[13px] font-mono border border-zinc-700/50" {...props}>
                                {children}
                            </code>
                        );
                    }

                    const codeString = String(children).replace(/\n$/, '');

                    return (
                        <div className="relative my-2 group">
                            <div className="flex items-center justify-between px-3 py-1 bg-zinc-800 rounded-t-lg border-x border-t border-[#27272a]">
                                <div className="flex items-center gap-2">
                                    <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-widest">{language || 'code'}</span>
                                </div>
                                {isBsl && onApplyCode && (
                                    <button
                                        onClick={() => onApplyCode(codeString)}
                                        className="flex items-center gap-1.5 px-2 py-0.5 text-[10px] font-medium text-blue-400 hover:text-blue-300 transition-colors"
                                        title="Load into Side Panel"
                                    >
                                        <PanelRight className="w-3 h-3" />
                                        <span>Apply</span>
                                    </button>
                                )}
                            </div>
                            <pre className="bg-[#18181b] border border-[#27272a] rounded-b-lg p-4 overflow-x-auto border-t-0">
                                <code className={`text-[13px] font-mono leading-relaxed ${className || ''}`} {...props}>
                                    {children}
                                </code>
                            </pre>
                        </div>
                    );
                },
                // Styled paragraphs
                p({ children }) {
                    return <p className="mb-2 last:mb-0">{children}</p>;
                },
                // Styled lists
                ul({ children }) {
                    return <ul className="list-disc list-inside mb-2 space-y-1">{children}</ul>;
                },
                ol({ children }) {
                    return <ol className="list-decimal list-inside mb-2 space-y-1">{children}</ol>;
                },
                // Styled links
                a({ href, children }) {
                    return (
                        <a href={href} className="text-blue-400 hover:underline" target="_blank" rel="noopener noreferrer">
                            {children}
                        </a>
                    );
                },
                // Styled headings
                h1({ children }) {
                    return <h1 className="text-xl font-bold mb-2">{children}</h1>;
                },
                h2({ children }) {
                    return <h2 className="text-lg font-semibold mb-2">{children}</h2>;
                },
                h3({ children }) {
                    return <h3 className="text-base font-semibold mb-1">{children}</h3>;
                },
                // Styled blockquotes
                blockquote({ children }) {
                    return (
                        <blockquote className="border-l-4 border-zinc-600 pl-4 my-2 text-zinc-400 italic">
                            {children}
                        </blockquote>
                    );
                },
                // Styled tables
                table({ children }) {
                    return (
                        <div className="overflow-x-auto my-2">
                            <table className="min-w-full border border-zinc-700 rounded">{children}</table>
                        </div>
                    );
                },
                th({ children }) {
                    return <th className="bg-zinc-800 px-3 py-2 text-left border-b border-zinc-700">{children}</th>;
                },
                td({ children }) {
                    return <td className="px-3 py-2 border-b border-zinc-700">{children}</td>;
                },
            }}
        >
            {content}
        </ReactMarkdown>
    );
}
