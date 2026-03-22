import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { McpToolInfo } from '@/types/mcp';
import { Wrench, RefreshCw, Info } from 'lucide-react';

interface McpToolsViewProps {
    serverName?: string | null;
}

export function McpToolsView({ serverName }: McpToolsViewProps) {
    const [tools, setTools] = useState<McpToolInfo[]>([]);
    const [loading, setLoading] = useState<boolean>(true);
    const [error, setError] = useState<string | null>(null);
    const [expandedTool, setExpandedTool] = useState<string | null>(null);

    const fetchTools = async (force = false) => {
        setLoading(true);
        setError(null);
        try {
            // Tauri invoke: command name and single arg object
            const res = (await invoke('list_mcp_tools', { force_refresh: force })) as McpToolInfo[];
            let filtered = serverName ? res.filter(t => t.server_name === serverName) : res;
            // Deduplicate tools by tool_name (keep first occurrence)
            const seen = new Set<string>();
            const deduped: McpToolInfo[] = [];
            for (const t of filtered) {
                if (!seen.has(t.tool_name)) {
                    seen.add(t.tool_name);
                    deduped.push(t);
                }
            }
            setTools(deduped);
        } catch (e: any) {
            setError(e?.toString() || 'Failed to fetch tools');
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        fetchTools(false);
    }, [serverName]);

    const grouped = tools.reduce<Record<string, McpToolInfo[]>>((acc, t) => {
        acc[t.server_name] = acc[t.server_name] || [];
        acc[t.server_name].push(t);
        return acc;
    }, {});

    if (loading) {
        return <div className="p-4"><div className="text-sm text-zinc-500">Loading MCP tools...</div></div>;
    }

    if (error) {
        return (
            <div className="p-4">
                <div className="text-sm text-red-400 mb-2">Error: {error}</div>
                <button onClick={() => fetchTools(true)} className="px-3 py-1 bg-zinc-800 rounded">Retry</button>
            </div>
        );
    }

    const serverNames = Object.keys(grouped);
    if (serverNames.length === 0) {
        return <div className="p-4 text-sm text-zinc-500">No MCP tools available.</div>;
    }

    return (
        <div className="p-3 overflow-auto max-h-full">
            <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2 text-sm font-semibold">
                    <Wrench className="w-4 h-4" /> MCP Tools
                </div>
                <div>
                    <button onClick={() => fetchTools(true)} className="px-2 py-1 rounded bg-zinc-800 hover:bg-zinc-700 text-sm flex items-center gap-2">
                        <RefreshCw className="w-3.5 h-3.5" /> Refresh
                    </button>
                </div>
            </div>
            <div className="flex flex-col gap-3">
                {serverNames.map(server => (
                    <div key={server} className="border border-zinc-800 rounded-lg p-3">
                        <div className="flex items-center justify-between mb-2">
                            <div className="text-sm font-semibold">{server}</div>
                        </div>
                        <div className="grid grid-cols-1 gap-2">
                            {grouped[server].map(tool => {
                                const isExpanded = expandedTool === tool.tool_name;
                                return (
                                    <div
                                        key={tool.tool_name}
                                        className={`p-2 border border-zinc-700 rounded transition-colors ${isExpanded ? 'bg-zinc-900' : 'hover:bg-zinc-900'} cursor-pointer`}
                                        onClick={() => setExpandedTool(isExpanded ? null : tool.tool_name)}
                                        role="button"
                                        tabIndex={0}
                                    >
                                        <div className="flex items-center justify-between">
                                            <div className="min-w-0">
                                                <div className="text-sm font-medium truncate">{tool.tool_name}</div>
                                                <div
                                                    className="text-xs text-zinc-500"
                                                    style={{
                                                        overflow: 'hidden',
                                                        maxHeight: isExpanded ? '1000px' : '3.2rem',
                                                        transition: 'max-height 220ms ease'
                                                    }}
                                                >
                                                    <div className={isExpanded ? '' : 'line-clamp-2'}>
                                                        {tool.description ?? ''}
                                                    </div>
                                                </div>
                                            </div>
                                            <div className="ml-3 flex items-center gap-2">
                                                <div className={`w-2.5 h-2.5 rounded-full ${tool.is_enabled ? 'bg-emerald-400' : 'bg-zinc-600'}`} />
                                                <button
                                                    className="text-zinc-400 hover:text-zinc-200 p-1"
                                                    title="Info"
                                                    onClick={(e) => {
                                                        e.stopPropagation();
                                                        setExpandedTool(isExpanded ? null : tool.tool_name);
                                                    }}
                                                >
                                                    <Info className="w-4 h-4" />
                                                </button>
                                            </div>
                                        </div>
                                    </div>
                                );
                            })}
                        </div>
                    </div>
                ))}
            </div>
        </div>
    );
}

export default McpToolsView;

