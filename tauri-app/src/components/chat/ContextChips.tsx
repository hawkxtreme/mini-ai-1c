import { Monitor, AlertTriangle } from 'lucide-react';
import { ConfiguratorTitleContext } from '../../utils/configurator';

interface ContextChipsProps {
    codeContext?: string;
    isSelection?: boolean;
    diagnostics?: any[];
    configuratorCtx?: ConfiguratorTitleContext | null;
    onRemoveCode?: () => void;
    onRemoveDiagnostics?: () => void;
}

export function ContextChips({ configuratorCtx }: ContextChipsProps) {
    if (!configuratorCtx) return null;

    const label = configuratorCtx.object_type && configuratorCtx.object_name
        ? `${configuratorCtx.object_type} ${configuratorCtx.object_name}`
        : configuratorCtx.object_name ?? configuratorCtx.config_name ?? null;

    if (!label) return null;

    const lowConfidence = configuratorCtx.confidence < 0.7;

    return (
        <div className="flex items-center gap-1 px-1 py-1">
            {lowConfidence
                ? <AlertTriangle className="w-3 h-3 text-yellow-600/70 flex-shrink-0" />
                : <Monitor className="w-3 h-3 text-zinc-600 flex-shrink-0" />
            }
            <span
                className="text-[11px] text-zinc-500 truncate max-w-[280px]"
                title={configuratorCtx.raw_title}
            >
                {label}
            </span>
            {configuratorCtx.read_only && (
                <span className="text-[10px] text-zinc-600">·&nbsp;ro</span>
            )}
        </div>
    );
}
