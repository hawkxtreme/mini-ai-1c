import React, { useState } from 'react';
import { ToolCall } from '../../contexts/ChatContext';
import { Play, CheckCircle, AlertCircle, XCircle, Terminal, ChevronDown, ChevronRight, Loader2 } from 'lucide-react';

interface ToolCallBlockProps {
    toolCall: ToolCall;
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

    const hasContent = toolCall.arguments && toolCall.arguments.trim().length > 0;

    // Design for Pending / Executing
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

    // Design for Done / Error / Rejected
    return (
        <div className="flex flex-col gap-0.5 mb-2 w-full animate-in fade-in duration-300">
            <button
                onClick={() => hasContent && setIsExpanded(!isExpanded)}
                className={`flex items-center gap-2 py-1 px-2 w-fit rounded transition-colors group ${hasContent ? 'hover:bg-zinc-800/50 cursor-pointer' : 'cursor-default'}`}
                title={toolCall.status}
            >
                {getStatusIcon()}
                <span className={`text-[11px] font-mono group-hover:text-zinc-300 transition-colors ${toolCall.status === 'error' ? 'text-red-400/80' : 'text-zinc-500'}`}>
                    {toolCall.name} {toolCall.status === 'error' ? '(Ошибка)' : toolCall.status === 'rejected' ? '(Отклонено)' : toolCall.status === 'done' ? '(Завершено)' : ''}
                </span>
                {hasContent && (
                    <ChevronRight size={14} className={`text-zinc-600 transition-transform ${isExpanded ? 'rotate-90' : ''}`} />
                )}
            </button>

            {isExpanded && hasContent && (
                <div className="ml-6 mr-4 mt-1 p-2.5 rounded border border-zinc-800/50 bg-[#121214] overflow-x-auto shadow-inner">
                    <pre className="font-mono text-[10px] text-zinc-400 whitespace-pre-wrap break-words">
                        {formatJSON(toolCall.arguments)}
                    </pre>
                </div>
            )}
        </div>
    );
};

export default ToolCallBlock;

