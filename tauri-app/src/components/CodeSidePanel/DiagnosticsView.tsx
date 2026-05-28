import { AlertTriangle, CheckSquare, Square, ToggleLeft } from 'lucide-react';
import { BslDiagnostic } from './types';

export function diagnosticKey(d: BslDiagnostic): string {
    return `${d.line}:${d.severity}:${d.message}`;
}

interface DiagnosticsViewProps {
    diagnostics: BslDiagnostic[];
    onDiagnosticClick: (line: number) => void;
    height: number;
    isResizing?: boolean;
    isLightTheme?: boolean;
    selectedKeys: Set<string>;
    onSelectionChange: (keys: Set<string>) => void;
}

export function DiagnosticsView({
    diagnostics,
    onDiagnosticClick,
    height,
    isResizing = false,
    isLightTheme = false,
    selectedKeys,
    onSelectionChange,
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
    const bulkButtonClass = isLightTheme
        ? 'text-[#52525b] hover:text-[#18181b] hover:bg-[#e4e4e7]'
        : 'text-zinc-500 hover:text-zinc-300 hover:bg-[#27272a]';
    const activeBulkButtonClass = isLightTheme
        ? 'bg-[#e4e4e7] text-[#18181b]'
        : 'bg-[#27272a] text-zinc-200';

    const selectedCount = diagnostics.filter((diagnostic) => selectedKeys.has(diagnosticKey(diagnostic))).length;
    const allSelected = diagnostics.length > 0 && selectedCount === diagnostics.length;
    const noneSelected = selectedCount === 0;

    const selectAll = () => {
        onSelectionChange(new Set(diagnostics.map(diagnosticKey)));
    };

    const excludeAll = () => {
        onSelectionChange(new Set());
    };

    const toggleOne = (diagnostic: BslDiagnostic) => {
        const key = diagnosticKey(diagnostic);
        const next = new Set(selectedKeys);
        if (next.has(key)) {
            next.delete(key);
        } else {
            next.add(key);
        }
        onSelectionChange(next);
    };

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
                {diagnostics.length > 0 ? (
                    <span className={`px-1.5 rounded-full ${countBadgeClass}`}>
                        {selectedCount}/{diagnostics.length}
                    </span>
                ) : (
                    <span className={`px-1.5 rounded-full ${countBadgeClass}`}>0</span>
                )}

                {diagnostics.length > 0 && (
                    <div className="ml-auto flex items-center gap-1.5">
                        <button
                            type="button"
                            data-testid="diagnostics-select-all"
                            onClick={selectAll}
                            className={`flex items-center gap-1 text-[10px] font-normal normal-case tracking-normal px-1.5 py-0.5 rounded transition-colors ${bulkButtonClass} ${allSelected ? activeBulkButtonClass : ''}`}
                            title="Выбрать все"
                        >
                            {allSelected ? <CheckSquare className="w-3 h-3" /> : <Square className="w-3 h-3" />}
                            Выбрать все
                        </button>

                        <button
                            type="button"
                            data-testid="diagnostics-exclude-all"
                            onClick={excludeAll}
                            className={`flex items-center gap-1 text-[10px] font-normal normal-case tracking-normal px-1.5 py-0.5 rounded transition-colors ${bulkButtonClass} ${noneSelected ? activeBulkButtonClass : ''}`}
                            title="Исключить все"
                        >
                            {noneSelected ? <ToggleLeft className="w-3 h-3" /> : <Square className="w-3 h-3" />}
                            Исключить все
                        </button>
                    </div>
                )}
            </div>

            <div className="overflow-y-auto flex-1 p-0">
                {diagnostics.length === 0 ? (
                    <div className={`p-4 text-xs italic text-center text-[11px] ${emptyStateClass}`}>No problems found</div>
                ) : (
                    <table className="w-full text-left border-collapse">
                        <thead className={`sticky top-0 ${tableHeaderClass}`}>
                            <tr>
                                <th className="px-2 py-1 text-[10px] font-normal w-8" />
                                <th className="px-3 py-1 text-[10px] font-normal w-12">Ln</th>
                                <th className="px-3 py-1 text-[10px] font-normal">Description</th>
                                <th className="px-3 py-1 text-[10px] font-normal w-20">Source</th>
                            </tr>
                        </thead>
                        <tbody>
                            {diagnostics.map((diagnostic, index) => {
                                const key = diagnosticKey(diagnostic);
                                const checked = selectedKeys.has(key);

                                return (
                                    <tr
                                        key={index}
                                        className={`border-b transition-colors group cursor-pointer ${rowClass} ${!checked ? 'opacity-50' : ''}`}
                                    >
                                        <td className="px-2 py-1.5 align-top">
                                            <button
                                                type="button"
                                                onClick={() => toggleOne(diagnostic)}
                                                className={`w-3.5 h-3.5 flex-shrink-0 transition-colors ${
                                                    isLightTheme ? 'text-[#52525b] hover:text-[#18181b]' : 'text-zinc-500 hover:text-zinc-300'
                                                }`}
                                                title={checked ? 'Снять отметку' : 'Отметить'}
                                            >
                                                {checked
                                                    ? <CheckSquare className="w-3.5 h-3.5" />
                                                    : <Square className="w-3.5 h-3.5" />
                                                }
                                            </button>
                                        </td>
                                        <td
                                            onClick={() => onDiagnosticClick(diagnostic.line + 1)}
                                            className={`px-3 py-1.5 text-[11px] font-mono whitespace-nowrap align-top ${
                                                diagnostic.severity === 'error'
                                                    ? isLightTheme ? 'text-red-600' : 'text-red-400'
                                                    : isLightTheme ? 'text-amber-600' : 'text-yellow-400'
                                            }`}
                                        >
                                            {diagnostic.line + 1}
                                        </td>
                                        <td
                                            onClick={() => onDiagnosticClick(diagnostic.line + 1)}
                                            className={`px-3 py-1.5 text-[11px] align-top ${messageClass}`}
                                        >
                                            {diagnostic.message}
                                        </td>
                                        <td
                                            onClick={() => onDiagnosticClick(diagnostic.line + 1)}
                                            className={`px-3 py-1.5 text-[10px] align-top truncate ${sourceClass}`}
                                        >
                                            BSL LS
                                        </td>
                                    </tr>
                                );
                            })}
                        </tbody>
                    </table>
                )}
            </div>
        </div>
    );
}
