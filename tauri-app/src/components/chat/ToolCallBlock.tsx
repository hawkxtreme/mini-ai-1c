import React, { useState } from 'react';
import { ToolCall } from '../../contexts/ChatContext';
import { CheckCircle, AlertCircle, XCircle, Terminal, ChevronRight, Loader2, Clock } from 'lucide-react';

interface ToolCallBlockProps {
    toolCall: ToolCall;
}

function formatDuration(ms: number): string {
    if (ms < 1000) return `${ms}мс`;
    const s = ms / 1000;
    return s < 60 ? `${s.toFixed(1)}с` : `${Math.floor(s / 60)}м ${Math.round(s % 60)}с`;
}

const ToolCallBlock: React.FC<ToolCallBlockProps> = ({ toolCall }) => {
    const [isExpanded, setIsExpanded] = useState(false);

    const getStatusIcon = () => {
        switch (toolCall.status) {
            case 'pending': return <Loader2 size={14} className="text-blue-400 animate-spin" />;
            case 'executing': return <Loader2 size={14} className="text-blue-400 animate-spin" />;
            case 'done': return <CheckCircle size={14} className="text-emerald-500" />;
            case 'error': return <AlertCircle size={14} className="text-red-400" />;
            case 'rejected': return <XCircle size={14} className="text-gray-400" />;
            default: return <Terminal size={14} className="text-white/50" />;
        }
    };

    const formatJSON = (str: string) => {
        try {
            const parsed = JSON.parse(str);
            return JSON.stringify(parsed, null, 2);
        } catch {
            return str;
        }
    };

    const hasArgs = toolCall.arguments && toolCall.arguments.trim().length > 0;
    const hasResult = toolCall.result && toolCall.result.trim().length > 0;
    const hasContent = hasArgs || hasResult;
    const isDone = toolCall.status === 'done' || toolCall.status === 'error' || toolCall.status === 'rejected';

    // Pending / Executing
    if (toolCall.status === 'pending' || toolCall.status === 'executing') {
        return (
            <div className="flex items-center gap-2 py-1.5 px-3 mb-2 bg-zinc-800/30 rounded-lg w-fit border border-white/5 shadow-sm animate-pulse origin-left animate-in zoom-in-95 duration-200">
                {getStatusIcon()}
                <span className="text-[12px] font-medium text-zinc-300">
                    Работа с {toolCall.name}...
                </span>
            </div>
        );
    }

    const statusLabel = toolCall.status === 'error' ? '(Ошибка)' : toolCall.status === 'rejected' ? '(Отклонено)' : '';

    // Done / Error / Rejected — с бейджем времени
    return (
        <div className="flex flex-col gap-0.5 mb-2 w-full animate-in fade-in duration-300">
            <div className="flex items-center gap-1.5 flex-wrap">
                <button
                    onClick={() => hasContent && setIsExpanded(!isExpanded)}
                    className={`flex items-center gap-2 py-1 px-2 rounded transition-colors group ${hasContent ? 'hover:bg-zinc-800/50 cursor-pointer' : 'cursor-default'}`}
                    title={toolCall.status}
                >
                    {getStatusIcon()}
                    <span className={`text-[11px] font-mono group-hover:text-zinc-300 transition-colors ${toolCall.status === 'error' ? 'text-red-400/80' : 'text-zinc-500'}`}>
                        {toolCall.name}{statusLabel && ` ${statusLabel}`}
                    </span>
                    {hasContent && (
                        <ChevronRight size={14} className={`text-zinc-600 transition-transform ${isExpanded ? 'rotate-90' : ''}`} />
                    )}
                </button>

                {isDone && toolCall.duration != null && (
                    <button
                        onClick={() => hasContent && setIsExpanded(!isExpanded)}
                        className={`flex items-center gap-1 px-2 py-0.5 rounded-md border text-[10px] font-mono tabular-nums transition-colors
                            ${toolCall.status === 'error'
                                ? 'border-red-500/20 bg-red-500/5 text-red-400/60 hover:bg-red-500/10'
                                : 'border-zinc-700/50 bg-zinc-800/40 text-zinc-500 hover:bg-zinc-800/70 hover:text-zinc-400'
                            } ${hasContent ? 'cursor-pointer' : 'cursor-default'}`}
                    >
                        <Clock size={10} className="opacity-60" />
                        <span>{formatDuration(toolCall.duration)}</span>
                    </button>
                )}
            </div>

            {isExpanded && hasContent && (
                <div className="ml-6 mr-4 mt-1 flex flex-col gap-1.5">
                    {hasArgs && (
                        <div className="p-2.5 rounded border border-zinc-800/50 bg-[#121214] overflow-x-auto shadow-inner">
                            <div className="text-[9px] text-zinc-600 uppercase tracking-wider mb-1 font-semibold">Аргументы</div>
                            <pre className="font-mono text-[10px] text-zinc-400 whitespace-pre-wrap break-words">
                                {formatJSON(toolCall.arguments)}
                            </pre>
                        </div>
                    )}
                    {hasResult && (
                        <div className="p-2.5 rounded border border-zinc-800/50 bg-[#121214] overflow-x-auto shadow-inner">
                            <div className="text-[9px] text-zinc-600 uppercase tracking-wider mb-1 font-semibold">Результат</div>
                            <pre className="font-mono text-[10px] text-zinc-400 whitespace-pre-wrap break-words max-h-[200px] overflow-y-auto">
                                {formatJSON(toolCall.result!)}
                            </pre>
                        </div>
                    )}
                </div>
            )}
        </div>
    );
};

export default ToolCallBlock;
