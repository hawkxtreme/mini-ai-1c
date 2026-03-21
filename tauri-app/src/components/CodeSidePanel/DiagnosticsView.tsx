import { AlertTriangle } from 'lucide-react';
import { BslDiagnostic } from './types';

interface DiagnosticsViewProps {
    diagnostics: BslDiagnostic[];
    onDiagnosticClick: (line: number) => void;
    height: number;
    isResizing?: boolean;
    isLightTheme?: boolean;
}

export function DiagnosticsView({
    diagnostics,
    onDiagnosticClick,
    height,
    isResizing = false,
    isLightTheme = false
}: DiagnosticsViewProps) {
    const headerClass = isLightTheme
        ? 'border-[#d4d4d8] text-[#3f3f46] bg-[#f4f4f5]'
        : 'border-[#27272a]/50 text-zinc-500 bg-[#1f1f23]';
    const countBadgeClass = isLightTheme
        ? 'bg-[#e4e4e7] text-[#3f3f46]'
        : 'bg-[#27272a] text-zinc-400';
    const emptyStateClass = isLightTheme ? 'text-[#71717a]' : 'text-zinc-600';
    const tableHeaderClass = isLightTheme
        ? 'bg-[#f4f4f5] text-[#52525b]'
        : 'bg-[#1f1f23] text-zinc-500';
    const rowClass = isLightTheme
        ? 'border-[#e4e4e7] hover:bg-[#f4f4f5]'
        : 'border-[#27272a]/30 hover:bg-[#27272a]/50';
    const messageClass = isLightTheme ? 'text-[#18181b]' : 'text-zinc-300';
    const sourceClass = isLightTheme ? 'text-[#52525b]' : 'text-zinc-500';

    return (
        <div
            style={{ height: `${height}px` }}
            className={`flex flex-col flex-shrink-0 transition-[border-color,box-shadow,background-color] ${
                isResizing
                    ? 'border-blue-500/70 shadow-[inset_0_1px_0_rgba(59,130,246,0.45)]'
                    : isLightTheme
                        ? 'border-[#d4d4d8] bg-[#fafafa]'
                        : 'border-[#27272a] bg-[#18181b]'
            }`}
        >
            <div className={`px-3 py-1.5 border-b flex items-center gap-2 text-[10px] font-bold uppercase tracking-wider ${headerClass}`}>
                <AlertTriangle className="w-3 h-3" />
                <span>Problems</span>
                <span className={`px-1.5 rounded-full ${countBadgeClass}`}>{diagnostics.length}</span>
            </div>

            <div className="overflow-y-auto flex-1 p-0">
                {diagnostics.length === 0 ? (
                    <div className={`p-4 text-xs italic text-center text-[11px] ${emptyStateClass}`}>No problems found</div>
                ) : (
                    <table className="w-full text-left border-collapse">
                        <thead className={`sticky top-0 ${tableHeaderClass}`}>
                            <tr>
                                <th className="px-3 py-1 text-[10px] font-normal w-12">Ln</th>
                                <th className="px-3 py-1 text-[10px] font-normal">Description</th>
                                <th className="px-3 py-1 text-[10px] font-normal w-20">Source</th>
                            </tr>
                        </thead>
                        <tbody>
                            {diagnostics.map((d, i) => (
                                <tr
                                    key={i}
                                    onClick={() => onDiagnosticClick(d.line + 1)}
                                    className={`border-b transition-colors group cursor-pointer ${rowClass}`}
                                >
                                    <td className={`px-3 py-1.5 text-[11px] font-mono whitespace-nowrap align-top ${
                                        d.severity === 'error'
                                            ? isLightTheme ? 'text-red-600' : 'text-red-400'
                                            : isLightTheme ? 'text-amber-600' : 'text-yellow-400'
                                    }`}>
                                        {d.line + 1}
                                    </td>
                                    <td className={`px-3 py-1.5 text-[11px] align-top ${messageClass}`}>
                                        {d.message}
                                    </td>
                                    <td className={`px-3 py-1.5 text-[10px] align-top truncate ${sourceClass}`}>
                                        BSL LS
                                    </td>
                                </tr>
                            ))}
                        </tbody>
                    </table>
                )}
            </div>
        </div>
    );
}
