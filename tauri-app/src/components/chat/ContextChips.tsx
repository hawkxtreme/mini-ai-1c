import React from 'react';
import { FileText, AlertCircle, X } from 'lucide-react';

interface ContextChip {
    id: string;
    type: 'code' | 'diagnostics' | 'selection';
    label: string;
    size: number;
    removable: boolean;
}

interface ContextChipsProps {
    codeContext?: string;
    isSelection?: boolean;
    diagnostics?: any[];
    onRemoveCode?: () => void;
    onRemoveDiagnostics?: () => void;
}

export function ContextChips({ 
    codeContext, 
    isSelection, 
    diagnostics, 
    onRemoveCode,
    onRemoveDiagnostics 
}: ContextChipsProps) {
    const chips: ContextChip[] = [];

    // Code context chip
    if (codeContext && codeContext.length > 0) {
        chips.push({
            id: 'code',
            type: isSelection ? 'selection' : 'code',
            label: isSelection ? 'Выделенный фрагмент' : 'Модуль целиком',
            size: codeContext.length,
            removable: true
        });
    }

    // Diagnostics chip
    if (diagnostics && diagnostics.length > 0) {
        const errorCount = diagnostics.filter(d => d.severity === 'error').length;
        const warningCount = diagnostics.filter(d => d.severity === 'warning').length;
        
        chips.push({
            id: 'diagnostics',
            type: 'diagnostics',
            label: `${errorCount} ошибок, ${warningCount} предупреждений`,
            size: diagnostics.length,
            removable: true
        });
    }

    if (chips.length === 0) return null;

    const formatSize = (bytes: number): string => {
        if (bytes < 1000) return `${bytes} chars`;
        if (bytes < 1000000) return `${(bytes / 1000).toFixed(1)}K`;
        return `${(bytes / 1000000).toFixed(1)}M`;
    };

    const getChipIcon = (type: string) => {
        switch (type) {
            case 'code':
            case 'selection':
                return <FileText className="w-3 h-3" />;
            case 'diagnostics':
                return <AlertCircle className="w-3 h-3" />;
            default:
                return null;
        }
    };

    const getChipColor = (type: string) => {
        switch (type) {
            case 'code':
                return 'bg-blue-500/10 border-blue-500/30 text-blue-400';
            case 'selection':
                return 'bg-purple-500/10 border-purple-500/30 text-purple-400';
            case 'diagnostics':
                return 'bg-red-500/10 border-red-500/30 text-red-400';
            default:
                return 'bg-zinc-800/50 border-zinc-700 text-zinc-400';
        }
    };

    const handleRemove = (chipId: string) => {
        if (chipId === 'code' && onRemoveCode) {
            onRemoveCode();
        } else if (chipId === 'diagnostics' && onRemoveDiagnostics) {
            onRemoveDiagnostics();
        }
    };

    return (
        <div className="flex items-center gap-2 flex-wrap px-1 py-2">
            <span className="text-[10px] text-zinc-600 uppercase tracking-wider font-bold">
                Контекст:
            </span>
            {chips.map(chip => (
                <div
                    key={chip.id}
                    className={`flex items-center gap-1.5 px-2 py-1 rounded-md border text-[11px] font-medium transition-all ${getChipColor(chip.type)}`}
                >
                    {getChipIcon(chip.type)}
                    <span className="whitespace-nowrap">{chip.label}</span>
                    <span className="text-[10px] opacity-60">
                        ({formatSize(chip.size)})
                    </span>
                    {chip.removable && (
                        <button
                            onClick={() => handleRemove(chip.id)}
                            className="ml-1 hover:bg-white/10 rounded p-0.5 transition-colors"
                            title="Удалить из контекста"
                        >
                            <X className="w-3 h-3" />
                        </button>
                    )}
                </div>
            ))}
        </div>
    );
}
