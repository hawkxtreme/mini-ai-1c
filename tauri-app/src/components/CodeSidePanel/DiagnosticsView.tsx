import { AlertTriangle } from 'lucide-react';
import { BslDiagnostic } from './types';

interface DiagnosticsViewProps {
    diagnostics: BslDiagnostic[];
    onDiagnosticClick: (line: number) => void;
}

export function DiagnosticsView({
    diagnostics,
    onDiagnosticClick
}: DiagnosticsViewProps) {
    return (
        <div className="h-40 border-t border-[#27272a] bg-[#18181b] flex flex-col">
            <div className="px-3 py-1.5 border-b border-[#27272a]/50 flex items-center gap-2 text-[10px] font-bold text-zinc-500 uppercase tracking-wider bg-[#1f1f23]">
                <AlertTriangle className="w-3 h-3" />
                <span>Problems</span>
                <span className="px-1.5 bg-[#27272a] rounded-full text-zinc-400">{diagnostics.length}</span>
            </div>

            <div className="overflow-y-auto flex-1 p-0">
                {diagnostics.length === 0 ? (
                    <div className="p-4 text-xs text-zinc-600 italic text-center text-[11px]">No problems found</div>
                ) : (
                    <table className="w-full text-left border-collapse">
                        <thead className="bg-[#1f1f23] text-zinc-500 sticky top-0">
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
                                    className="border-b border-[#27272a]/30 hover:bg-[#27272a]/50 transition-colors group cursor-pointer"
                                >
                                    <td className={`px-3 py-1.5 text-[11px] font-mono whitespace-nowrap align-top ${d.severity === 'error' ? 'text-red-400' : 'text-yellow-400'}`}>
                                        {d.line + 1}
                                    </td>
                                    <td className="px-3 py-1.5 text-[11px] text-zinc-300 align-top">
                                        {d.message}
                                    </td>
                                    <td className="px-3 py-1.5 text-[10px] text-zinc-500 align-top truncate">
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
