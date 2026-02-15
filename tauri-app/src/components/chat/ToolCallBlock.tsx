import React, { useState } from 'react';
import { ToolCall } from '../../contexts/ChatContext';
import { Play, Check, AlertCircle, XCircle, Terminal, ChevronDown, ChevronRight } from 'lucide-react';

interface ToolCallBlockProps {
    toolCall: ToolCall;
}

const ToolCallBlock: React.FC<ToolCallBlockProps> = ({ toolCall }) => {
    const [isExpanded, setIsExpanded] = useState(false);

    const getStatusIcon = () => {
        switch (toolCall.status) {
            case 'pending': return <Play size={14} className="text-yellow-400 animate-pulse" />;
            case 'executing': return <Play size={14} className="text-blue-400 animate-spin" />;
            case 'done': return <Check size={14} className="text-green-400" />;
            case 'error': return <AlertCircle size={14} className="text-red-400" />;
            case 'rejected': return <XCircle size={14} className="text-gray-400" />;
            default: return <Terminal size={14} className="text-white/50" />;
        }
    };

    const getStatusColor = () => {
        switch (toolCall.status) {
            case 'pending': return 'border-yellow-500/30 bg-yellow-500/5';
            case 'executing': return 'border-blue-500/30 bg-blue-500/5';
            case 'done': return 'border-green-500/30 bg-green-500/5';
            case 'error': return 'border-red-500/30 bg-red-500/5';
            case 'rejected': return 'border-white/10 bg-white/5 opacity-60';
            default: return 'border-white/10 bg-white/5';
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

    if (toolCall.status === 'done') {
        const hasContent = toolCall.arguments && toolCall.arguments.trim().length > 0;
        return (
            <div className="my-1 flex items-center gap-2 px-2 py-0.5 opacity-50 hover:opacity-100 transition-opacity">
                <Check size={12} className="text-green-500" />
                <span className="font-mono text-[10px] text-zinc-500 font-medium">
                    MCP: {toolCall.name}
                </span>
                {hasContent && (
                    <button
                        onClick={() => setIsExpanded(!isExpanded)}
                        className="text-[10px] text-zinc-600 hover:text-zinc-300 underline"
                    >
                        {isExpanded ? 'hide args' : 'args'}
                    </button>
                )}
                {isExpanded && hasContent && (
                    <div className="absolute mt-5 left-10 z-10 p-2 bg-zinc-900 border border-zinc-800 rounded shadow-lg max-w-lg">
                        <pre className="text-[10px] text-zinc-400 whitespace-pre-wrap">{formatJSON(toolCall.arguments)}</pre>
                    </div>
                )}
            </div>
        );
    }

    // Default rendering for pending/executing/error
    return (
        <div className={`my-2 border rounded-lg overflow-hidden transition-all ${getStatusColor()}`}>
            <button
                onClick={() => hasContent && setIsExpanded(!isExpanded)}
                className={`w-full flex items-center gap-2 px-3 py-1.5 bg-white/5 border-b border-white/5 ${hasContent ? 'hover:bg-white/10 cursor-pointer' : 'cursor-default'}`}
            >
                {getStatusIcon()}
                <span className="font-mono text-[11px] uppercase tracking-wider font-bold text-white/80 flex-1 text-left">
                    Вызов MCP: {toolCall.name}
                </span>
                <span className={`text-[10px] px-1.5 py-0.5 rounded uppercase font-bold ${toolCall.status === 'error' ? 'bg-red-500/20 text-red-400' :
                        'bg-white/10 text-white/50'
                    }`}>
                    {toolCall.status}
                </span>
                {hasContent && (
                    isExpanded ?
                        <ChevronDown size={14} className="text-white/40" /> :
                        <ChevronRight size={14} className="text-white/40" />
                )}
            </button>
            {isExpanded && hasContent && (
                <div className="p-2.5 font-mono text-[11px] text-white/70 overflow-x-auto bg-black/40">
                    <pre className="whitespace-pre-wrap break-all">
                        {formatJSON(toolCall.arguments)}
                    </pre>
                </div>
            )}
        </div>
    );
};

export default ToolCallBlock;
