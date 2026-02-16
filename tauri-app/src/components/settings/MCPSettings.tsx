import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Database, Link2, Key, ShieldCheck, Activity, CheckCircle2, AlertCircle, Plus, Trash2, Globe, Settings2, Terminal, Cpu, FileText, X, Sparkles } from 'lucide-react';

export type McpTransport = 'http' | 'stdio' | 'internal';

export interface McpServerConfig {
    id: string;
    name: string;
    enabled: boolean;
    transport: McpTransport;
    // HTTP specific
    url?: string | null;
    login?: string | null;
    password?: string | null;
    // Stdio specific
    command?: string | null;
    args?: string[] | null;
    env?: Record<string, string> | null;
}

export interface McpServerStatus {
    id: string;
    name: string;
    status: string;
    transport: string;
}

interface MCPSettingsProps {
    servers: McpServerConfig[];
    onUpdate: (servers: McpServerConfig[]) => void;
}

const BUILTIN_1C_SERVER_ID = 'builtin-1c-naparnik';
const BUILTIN_1C_METADATA_ID = 'builtin-1c-metadata';
const BUILTIN_BSL_LS_ID = 'bsl-ls';

export function MCPSettings({ servers, onUpdate }: MCPSettingsProps) {
    const [testingId, setTestingId] = useState<string | null>(null);
    const [testResults, setTestResults] = useState<Record<string, { success: boolean; message: string }>>({});
    const [statuses, setStatuses] = useState<Record<string, McpServerStatus>>({});
    const [viewingLogsId, setViewingLogsId] = useState<string | null>(null);
    const [logs, setLogs] = useState<string[]>([]);
    const [isLoadingLogs, setIsLoadingLogs] = useState(false);

    // Ensure pre-installed servers exist
    useEffect(() => {
        const naparnikArgs = ['tsx', 'src/mcp-servers/1c-naparnik.ts'];
        const metadataArgs = ['tsx', 'src/mcp-servers/1c-metadata.ts'];

        let updatedServers = [...servers];
        let needsUpdate = false;

        // Check Naparnik
        const naparnikIndex = updatedServers.findIndex(s => s.id === BUILTIN_1C_SERVER_ID);
        if (naparnikIndex === -1) {
            updatedServers.push({
                id: BUILTIN_1C_SERVER_ID,
                name: '1C:Напарник',
                enabled: false,
                transport: 'stdio',
                command: 'npx',
                args: naparnikArgs,
                env: { 'ONEC_AI_TOKEN': '' }
            });
            needsUpdate = true;
        } else if (JSON.stringify(updatedServers[naparnikIndex].args) !== JSON.stringify(naparnikArgs)) {
            updatedServers[naparnikIndex] = { ...updatedServers[naparnikIndex], args: naparnikArgs };
            needsUpdate = true;
        }

        // Check Metadata
        const metadataIndex = updatedServers.findIndex(s => s.id === BUILTIN_1C_METADATA_ID);
        if (metadataIndex === -1) {
            updatedServers.push({
                id: BUILTIN_1C_METADATA_ID,
                name: '1C:Метаданные',
                enabled: false,
                transport: 'stdio',
                command: 'npx',
                args: metadataArgs,
                env: { 'ONEC_METADATA_URL': 'http://localhost/base/hs/mcp', 'ONEC_USERNAME': '', 'ONEC_PASSWORD': '' }
            });
            needsUpdate = true;
        } else if (JSON.stringify(updatedServers[metadataIndex].args) !== JSON.stringify(metadataArgs)) {
            updatedServers[metadataIndex] = { ...updatedServers[metadataIndex], args: metadataArgs };
            needsUpdate = true;
        }

        // Check BSL LS
        const bslIndex = updatedServers.findIndex(s => s.id === BUILTIN_BSL_LS_ID);
        if (bslIndex === -1) {
            updatedServers.push({
                id: BUILTIN_BSL_LS_ID,
                name: 'BSL Language Server',
                enabled: false,
                transport: 'internal',
            });
            needsUpdate = true;
        }

        if (needsUpdate) {
            onUpdate(updatedServers);
        }
    }, [servers, onUpdate]);

    useEffect(() => {
        const fetchStatuses = async () => {
            try {
                const result = await invoke<McpServerStatus[]>('get_mcp_server_statuses');
                const statusMap = result.reduce((acc, s) => ({ ...acc, [s.id]: s }), {} as Record<string, McpServerStatus>);
                setStatuses(statusMap);
            } catch (e) {
                console.error("Failed to fetch statuses", e);
            }
        };

        fetchStatuses();
        const interval = setInterval(fetchStatuses, 5000);
        return () => clearInterval(interval);
    }, []);

    useEffect(() => {
        if (viewingLogsId) {
            const fetchLogs = async () => {
                setIsLoadingLogs(true);
                try {
                    const result = await invoke<string[]>('get_mcp_server_logs', { serverId: viewingLogsId });
                    setLogs(result);
                } catch (e) {
                    console.error("Failed to fetch logs", e);
                    setLogs(["Failed to fetch logs"]);
                } finally {
                    setIsLoadingLogs(false);
                }
            };
            fetchLogs();
            const interval = setInterval(fetchLogs, 2000);
            return () => clearInterval(interval);
        }
    }, [viewingLogsId]);

    const handleAddServer = () => {
        const newServer: McpServerConfig = {
            id: Math.random().toString(36).substring(2, 9),
            name: 'New MCP Server',
            enabled: false,
            transport: 'http',
            url: 'http://',
        };
        onUpdate([...servers, newServer]);
    };

    const handleRemoveServer = (id: string) => {
        onUpdate(servers.filter(s => s.id !== id));
    };

    const handleUpdateServer = (id: string, updates: Partial<McpServerConfig>) => {
        onUpdate(servers.map(s => s.id === id ? { ...s, ...updates } : s));
    };

    const handleTestConnection = async (config: McpServerConfig) => {
        setTestingId(config.id);
        try {
            const result = await invoke<string>('test_mcp_connection', { config });
            setTestResults(prev => ({ ...prev, [config.id]: { success: true, message: result } }));
        } catch (e: any) {
            setTestResults(prev => ({ ...prev, [config.id]: { success: false, message: e.toString() } }));
        } finally {
            setTestingId(null);
        }
    };

    // Sort servers: Built-in first, then others
    const sortedServers = [...servers].sort((a, b) => {
        const builtinIds = [BUILTIN_BSL_LS_ID, BUILTIN_1C_METADATA_ID, BUILTIN_1C_SERVER_ID];
        const aIdx = builtinIds.indexOf(a.id);
        const bIdx = builtinIds.indexOf(b.id);

        if (aIdx !== -1 && bIdx !== -1) return aIdx - bIdx;
        if (aIdx !== -1) return -1;
        if (bIdx !== -1) return 1;
        return 0;
    });

    const isInternal = (transport: string) => transport.toLowerCase() === 'internal';

    return (
        <div className="space-y-6 relative">
            <div className="flex items-center justify-between">
                <h3 className="text-lg font-medium flex items-center gap-2">
                    <Globe className="w-5 h-5 text-blue-500" />
                    MCP Servers
                </h3>
                <button
                    onClick={handleAddServer}
                    className="flex items-center gap-2 px-3 py-1.5 bg-blue-600 hover:bg-blue-700 text-white rounded-lg text-sm font-medium transition"
                >
                    <Plus className="w-4 h-4" /> Добавить сервер
                </button>
            </div>

            {servers.length === 0 ? (
                <div className="text-center py-12 bg-zinc-800/30 border border-zinc-700/50 border-dashed rounded-xl">
                    <Database className="w-12 h-12 text-zinc-700 mx-auto mb-3" />
                    <p className="text-zinc-500 text-sm">Список серверов пуст. Добавьте первый сервер для начала работы.</p>
                </div>
            ) : (
                <div className="space-y-4">
                    {sortedServers.map((server) => {
                        const status = statuses[server.id];
                        const isConnected = status?.status === 'connected';
                        const isStopped = status?.status === 'stopped';
                        const isMetadata = server.id === BUILTIN_1C_METADATA_ID;
                        const isBslLs = server.id === BUILTIN_BSL_LS_ID;
                        const isBuiltin = server.id === BUILTIN_1C_SERVER_ID || isMetadata || isBslLs;

                        return (
                            <div
                                key={server.id}
                                className={`
                                    rounded-xl overflow-hidden shadow-sm border transition-all duration-300
                                    ${isBuiltin
                                        ? `bg-gradient-to-br from-zinc-800/80 to-yellow-900/10 border-yellow-500/30 shadow-[0_0_15px_rgba(234,179,8,0.05)]`
                                        : 'bg-zinc-800/50 border-zinc-700'
                                    }
                                `}
                            >
                                {/* Server Header */}
                                <div className={`
                                    px-4 py-3 border-b flex items-center justify-between
                                    ${isBuiltin
                                        ? 'bg-yellow-500/5 border-yellow-500/20'
                                        : 'bg-zinc-800/80 border-zinc-700'
                                    }
                                `}>
                                    <div className="flex items-center gap-3">
                                        <div className={`w-2 h-2 rounded-full transition-all duration-300 ${server.enabled ? (isConnected ? 'bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.5)]' : 'bg-red-500 animate-pulse') : 'bg-zinc-600'}`} title={server.enabled ? (isConnected ? "Connected" : "Disconnected") : "Disabled"} />

                                        {isBuiltin ? (
                                            <div className="flex items-center gap-2">
                                                {isMetadata ? <Database className="w-4 h-4 text-yellow-500" /> : isBslLs ? <Cpu className="w-4 h-4 text-yellow-500" /> : <Sparkles className="w-4 h-4 text-yellow-500" />}
                                                <span className="text-zinc-100 font-medium text-sm">{server.name}</span>
                                                <span className="text-[10px] px-1.5 py-0.5 rounded border bg-yellow-500/10 text-yellow-400 border-yellow-500/20">
                                                    PRE-INSTALLED
                                                </span>
                                            </div>
                                        ) : (
                                            <input
                                                type="text"
                                                value={server.name}
                                                onChange={(e) => handleUpdateServer(server.id, { name: e.target.value })}
                                                className="bg-transparent border-none text-zinc-100 font-medium focus:ring-0 p-0 text-sm w-48"
                                                placeholder="Название сервера"
                                            />
                                        )}

                                        {server.enabled && (
                                            <span className={`text-[10px] px-1.5 py-0.5 rounded border ${isConnected ? 'bg-green-500/10 text-green-400 border-green-500/20' : 'bg-red-500/10 text-red-400 border-red-500/20'}`}>
                                                {isConnected ? 'LIVE' : (isStopped ? 'STOPPED' : 'OFFLINE')}
                                            </span>
                                        )}
                                    </div>

                                    <div className="flex items-center gap-3">
                                        {!isBuiltin && (
                                            <div className="flex bg-zinc-900 rounded-lg p-0.5 border border-zinc-700">
                                                <button
                                                    onClick={() => handleUpdateServer(server.id, { transport: 'http' })}
                                                    className={`px-2 py-0.5 rounded-md text-[10px] uppercase font-bold transition ${server.transport === 'http' ? 'bg-zinc-700 text-blue-400' : 'text-zinc-500 hover:text-zinc-300'}`}
                                                    title="HTTP Transport"
                                                >
                                                    HTTP
                                                </button>
                                                <button
                                                    onClick={() => handleUpdateServer(server.id, { transport: 'stdio' })}
                                                    className={`px-2 py-0.5 rounded-md text-[10px] uppercase font-bold transition ${server.transport === 'stdio' ? 'bg-zinc-700 text-blue-400' : 'text-zinc-500 hover:text-zinc-300'}`}
                                                    title="Stdio (Local command)"
                                                >
                                                    Stdio
                                                </button>
                                            </div>
                                        )}

                                        <button
                                            onClick={() => handleUpdateServer(server.id, { enabled: !server.enabled })}
                                            className={`relative inline-flex h-4 w-8 items-center rounded-full transition-colors focus:outline-none ${server.enabled ? 'bg-blue-600' : 'bg-zinc-700'}`}
                                        >
                                            <span className={`inline-block h-2.5 w-2.5 transform rounded-full bg-white transition-transform ${server.enabled ? 'translate-x-4.5' : 'translate-x-1'}`} />
                                        </button>

                                        {!isBuiltin && (
                                            <button
                                                onClick={() => handleRemoveServer(server.id)}
                                                className="p-1 hover:bg-red-500/20 text-zinc-500 hover:text-red-400 rounded transition"
                                                title="Удалить"
                                            >
                                                <Trash2 className="w-4 h-4" />
                                            </button>
                                        )}
                                    </div>
                                </div>

                                {/* Server Settings */}
                                <div className={`p-4 space-y-4 transition-opacity ${!server.enabled ? 'opacity-60' : ''}`}>
                                    {isBuiltin ? (
                                        <div className="mt-0 space-y-4">
                                            {server.id === BUILTIN_1C_SERVER_ID ? (
                                                <div>
                                                    <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                        <Key className="w-3 h-3" /> 1C.ai Token
                                                    </label>
                                                    <input
                                                        type="password"
                                                        value={server.env?.['ONEC_AI_TOKEN'] || ''}
                                                        onChange={(e) => {
                                                            const newEnv = { ...(server.env || {}), 'ONEC_AI_TOKEN': e.target.value };
                                                            handleUpdateServer(server.id, { env: newEnv });
                                                        }}
                                                        className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:ring-1 focus:ring-blue-500 focus:outline-none"
                                                        placeholder="Вставьте ваш токен 1C.ai"
                                                    />
                                                </div>
                                            ) : server.id === BUILTIN_BSL_LS_ID ? (
                                                <div className="bg-zinc-900/50 border border-yellow-500/10 rounded-lg p-3 text-xs text-zinc-400 italic">
                                                    Этот сервер интегрирован как внутренний инструмент анализа кода.
                                                    Основные настройки (путь к Java, JAR и порт) находятся во вкладке <b>"BSL Server"</b> выше.
                                                </div>
                                            ) : (
                                                <>
                                                    <div className="grid grid-cols-12 gap-2">
                                                        <div className="col-span-3">
                                                            <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                                <Globe className="w-3 h-3" /> Protocol
                                                            </label>
                                                            <select
                                                                value={(server.env?.['ONEC_METADATA_URL'] || '').startsWith('https') ? 'https' : 'http'}
                                                                onChange={(e) => {
                                                                    const currentUrl = server.env?.['ONEC_METADATA_URL'] || 'http://localhost/base/hs/mcp';
                                                                    const urlWithoutProto = currentUrl.replace(/^https?:\/\//, '');
                                                                    const newUrl = `${e.target.value}://${urlWithoutProto}`;
                                                                    const newEnv = { ...(server.env || {}), 'ONEC_METADATA_URL': newUrl };
                                                                    handleUpdateServer(server.id, { env: newEnv });
                                                                }}
                                                                className="w-full bg-zinc-900 border border-zinc-700 font-bold rounded-lg px-2 py-1.5 text-[11px] focus:ring-1 focus:ring-yellow-500 focus:outline-none text-yellow-500 bg-yellow-500/5"
                                                            >
                                                                <option value="http">HTTP</option>
                                                                <option value="https">HTTPS</option>
                                                            </select>
                                                        </div>
                                                        <div className="col-span-5">
                                                            <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                                <Terminal className="w-3 h-3" /> Server (Host)
                                                            </label>
                                                            <input
                                                                type="text"
                                                                value={(server.env?.['ONEC_METADATA_URL'] || '').replace(/^https?:\/\//, '').split('/')[0] || ''}
                                                                onChange={(e) => {
                                                                    const currentUrl = server.env?.['ONEC_METADATA_URL'] || 'http://localhost/base/hs/mcp';
                                                                    const proto = currentUrl.startsWith('https') ? 'https' : 'http';
                                                                    const pathParts = currentUrl.replace(/^https?:\/\//, '').split('/');
                                                                    const base = pathParts[1] || 'base';
                                                                    const newUrl = `${proto}://${e.target.value}/${base}/hs/mcp`;
                                                                    const newEnv = { ...(server.env || {}), 'ONEC_METADATA_URL': newUrl };
                                                                    handleUpdateServer(server.id, { env: newEnv });
                                                                }}
                                                                className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:ring-1 focus:ring-yellow-500 focus:outline-none"
                                                                placeholder="localhost"
                                                            />
                                                        </div>
                                                        <div className="col-span-4">
                                                            <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                                <Database className="w-3 h-3" /> Base Name
                                                            </label>
                                                            <input
                                                                type="text"
                                                                value={(server.env?.['ONEC_METADATA_URL'] || '').replace(/^https?:\/\//, '').split('/')[1] || ''}
                                                                onChange={(e) => {
                                                                    const currentUrl = server.env?.['ONEC_METADATA_URL'] || 'http://localhost/base/hs/mcp';
                                                                    const proto = currentUrl.startsWith('https') ? 'https' : 'http';
                                                                    const host = currentUrl.replace(/^https?:\/\//, '').split('/')[0] || 'localhost';
                                                                    const newUrl = `${proto}://${host}/${e.target.value}/hs/mcp`;
                                                                    const newEnv = { ...(server.env || {}), 'ONEC_METADATA_URL': newUrl };
                                                                    handleUpdateServer(server.id, { env: newEnv });
                                                                }}
                                                                className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:ring-1 focus:ring-yellow-500 focus:outline-none"
                                                                placeholder="demo"
                                                            />
                                                        </div>
                                                    </div>
                                                    <div className="text-[10px] text-zinc-500 mt-1 flex items-center gap-1 italic">
                                                        <Link2 className="w-2.5 h-2.5" />
                                                        Будет использован: {server.env?.['ONEC_METADATA_URL']}/...
                                                    </div>
                                                    <div className="grid grid-cols-2 gap-4">
                                                        <div>
                                                            <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                                <Key className="w-3 h-3" /> Login
                                                            </label>
                                                            <input
                                                                type="text"
                                                                value={server.env?.['ONEC_USERNAME'] || ''}
                                                                onChange={(e) => {
                                                                    const newEnv = { ...(server.env || {}), 'ONEC_USERNAME': e.target.value };
                                                                    handleUpdateServer(server.id, { env: newEnv });
                                                                }}
                                                                className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:ring-1 focus:ring-blue-500 focus:outline-none"
                                                                placeholder="Администратор"
                                                            />
                                                        </div>
                                                        <div>
                                                            <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                                <ShieldCheck className="w-3 h-3" /> Password
                                                            </label>
                                                            <input
                                                                type="password"
                                                                value={server.env?.['ONEC_PASSWORD'] || ''}
                                                                onChange={(e) => {
                                                                    const newEnv = { ...(server.env || {}), 'ONEC_PASSWORD': e.target.value };
                                                                    handleUpdateServer(server.id, { env: newEnv });
                                                                }}
                                                                className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:ring-1 focus:ring-blue-500 focus:outline-none"
                                                                placeholder="••••••"
                                                            />
                                                        </div>
                                                    </div>
                                                </>
                                            )}
                                        </div>
                                    ) : (
                                        <>
                                            {server.transport === 'http' ? (
                                                <>
                                                    <div>
                                                        <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                            <Link2 className="w-3 h-3" /> Service URL
                                                        </label>
                                                        <input
                                                            type="text"
                                                            value={server.url || ''}
                                                            onChange={(e) => handleUpdateServer(server.id, { url: e.target.value })}
                                                            placeholder="http://example.com/mcp"
                                                            className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:ring-1 focus:ring-blue-500 focus:outline-none"
                                                        />
                                                    </div>

                                                    <div className="grid grid-cols-2 gap-4">
                                                        <div>
                                                            <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                                <Key className="w-3 h-3" /> Login (Optional)
                                                            </label>
                                                            <input
                                                                type="text"
                                                                value={server.login || ''}
                                                                onChange={(e) => handleUpdateServer(server.id, { login: e.target.value || null })}
                                                                className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:ring-1 focus:ring-blue-500 focus:outline-none"
                                                            />
                                                        </div>
                                                        <div>
                                                            <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                                <ShieldCheck className="w-3 h-3" /> Password
                                                            </label>
                                                            <input
                                                                type="password"
                                                                value={server.password || ''}
                                                                onChange={(e) => handleUpdateServer(server.id, { password: e.target.value || null })}
                                                                className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:ring-1 focus:ring-blue-500 focus:outline-none"
                                                            />
                                                        </div>
                                                    </div>
                                                </>
                                            ) : (
                                                <>
                                                    <div>
                                                        <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                            <Terminal className="w-3 h-3" /> Command
                                                        </label>
                                                        <input
                                                            type="text"
                                                            value={server.command || ''}
                                                            onChange={(e) => handleUpdateServer(server.id, { command: e.target.value })}
                                                            placeholder="npx"
                                                            className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:ring-1 focus:ring-blue-500 focus:outline-none font-mono"
                                                        />
                                                    </div>
                                                    <div>
                                                        <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                            <Cpu className="w-3 h-3" /> Arguments (Space or comma separated)
                                                        </label>
                                                        <input
                                                            type="text"
                                                            value={server.args?.join(' ') || ''}
                                                            onChange={(e) => {
                                                                // Split by spaces or commas, filter out empties
                                                                const raw = e.target.value;
                                                                const parsed = raw.split(/[,\s]+/).filter(a => a);
                                                                handleUpdateServer(server.id, { args: parsed });
                                                            }}
                                                            placeholder="chrome-devtools-mcp@latest --browser-url=http://127.0.0.1:9222 -y"
                                                            className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:ring-1 focus:ring-blue-500 focus:outline-none font-mono"
                                                        />
                                                    </div>
                                                </>
                                            )}
                                        </>
                                    )}
                                    <div className="flex items-center justify-between pt-1">
                                        <div className="flex gap-2">
                                            <button
                                                onClick={() => handleTestConnection(server)}
                                                disabled={!server.enabled || testingId === server.id || (server.transport === 'http' && !server.url) || (server.transport === 'stdio' && !server.command)}
                                                className={`flex items-center gap-2 px-3 py-1.5 rounded-lg text-xs font-semibold transition-all ${testingId === server.id ? 'bg-zinc-700 text-zinc-500' : 'bg-zinc-700 hover:bg-zinc-600 text-zinc-300 disabled:opacity-50 disabled:cursor-not-allowed'}`}
                                            >
                                                <Activity className={`w-3.5 h-3.5 ${testingId === server.id ? 'animate-pulse' : ''}`} />
                                                {testingId === server.id ? 'Checking...' : 'Проверить'}
                                            </button>
                                            <button
                                                onClick={() => setViewingLogsId(server.id)}
                                                disabled={!server.enabled}
                                                className="flex items-center gap-2 px-3 py-1.5 rounded-lg text-xs font-semibold bg-zinc-700 hover:bg-zinc-600 text-zinc-300 transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                                            >
                                                <FileText className="w-3.5 h-3.5" />
                                                Logs
                                            </button>
                                        </div>

                                        {testResults[server.id] && (
                                            <div className={`flex items-center gap-2 text-xs font-medium ${testResults[server.id].success ? 'text-green-400' : 'text-red-400'}`}>
                                                {testResults[server.id].success ? <CheckCircle2 className="w-3.5 h-3.5" /> : <AlertCircle className="w-3.5 h-3.5" />}
                                                <span className="truncate max-w-[200px]">{testResults[server.id].message}</span>
                                            </div>
                                        )}
                                    </div>
                                </div>
                            </div>
                        );
                    })}
                </div>
            )}

            <div className="bg-blue-500/5 border border-blue-500/20 rounded-lg p-3 flex gap-3 mt-4">
                <Settings2 className="w-5 h-5 text-blue-400 shrink-0" />
                <p className="text-xs text-zinc-400 leading-relaxed">
                    Поддерживаются два вида транспорта: <b>HTTP</b> (для удаленных сервисов) и <b>Stdio</b> (для локальных CLI-инструментов). Для Stdio укажите команду (напр. <code>npx</code>) и аргументы.
                </p>
            </div>

            {/* Logs Modal */}
            {viewingLogsId && (
                <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm p-4">
                    <div className="bg-zinc-800 border border-zinc-700 rounded-xl w-full max-w-3xl h-[600px] flex flex-col shadow-2xl">
                        <div className="px-4 py-3 border-b border-zinc-700 flex items-center justify-between">
                            <h3 className="font-medium text-zinc-100 flex items-center gap-2">
                                <FileText className="w-4 h-4 text-zinc-400" />
                                Server Logs: {servers.find(s => s.id === viewingLogsId)?.name}
                            </h3>
                            <button
                                onClick={() => setViewingLogsId(null)}
                                className="p-1 hover:bg-zinc-700 rounded text-zinc-400 hover:text-zinc-200 transition"
                            >
                                <X className="w-5 h-5" />
                            </button>
                        </div>
                        <div className="flex-1 overflow-auto p-4 bg-zinc-950 font-mono text-xs text-zinc-300">
                            {isLoadingLogs && logs.length === 0 ? (
                                <p className="text-zinc-500">Loading...</p>
                            ) : logs.length === 0 ? (
                                <p className="text-zinc-500">No logs available.</p>
                            ) : (
                                logs.map((line, i) => (
                                    <div key={i} className="whitespace-pre-wrap mb-0.5 border-b border-zinc-900/50 pb-0.5">{line}</div>
                                ))
                            )}
                            <div className="h-4" /> {/* Spacer */}
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
