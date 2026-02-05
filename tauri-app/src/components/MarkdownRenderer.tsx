import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

interface MarkdownRendererProps {
    content: string;
}

export function MarkdownRenderer({ content }: MarkdownRendererProps) {
    return (
        <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            components={{
                // Code blocks with syntax highlighting styling
                code({ inline, className, children, ...props }: any) {
                    const match = /language-(\w+)/.exec(className || '');
                    const language = match ? match[1] : '';

                    if (inline) {
                        return (
                            <code className="bg-[#27272a] text-zinc-200 px-1.5 py-0.5 rounded-md text-[13px] font-mono border border-zinc-700/50" {...props}>
                                {children}
                            </code>
                        );
                    }

                    return (
                        <div className="relative my-2">
                            {language && (
                                <div className="absolute top-0 right-0 px-2 py-1 text-xs text-zinc-400 bg-zinc-800 rounded-bl">
                                    {language}
                                </div>
                            )}
                            <pre className="bg-[#18181b] border border-[#27272a] rounded-lg p-4 overflow-x-auto">
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
