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
    // 1С:Справка — поля прогресса индексации
    index_progress?: number;     // 0-100 (%)
    index_message?: string;      // Сообщение прогресса
    help_status?: string;        // 'unavailable' | 'indexing' | 'ready' | ''
}

interface MCPSettingsProps {
    servers: McpServerConfig[];
    onUpdate: (servers: McpServerConfig[]) => void;
}

const BUILTIN_1C_SERVER_ID = 'builtin-1c-naparnik';
const BUILTIN_1C_METADATA_ID = 'builtin-1c-metadata';
const BUILTIN_BSL_LS_ID = 'bsl-ls';
const BUILTIN_1C_HELP_ID = 'builtin-1c-help';

export function MCPSettings({ servers, onUpdate }: MCPSettingsProps) {
    const [testingId, setTestingId] = useState<string | null>(null);
    const [testResults, setTestResults] = useState<Record<string, { success: boolean; message: string }>>({});
    const [statuses, setStatuses] = useState<Record<string, McpServerStatus>>({});
    const [viewingLogsId, setViewingLogsId] = useState<string | null>(null);
    const [logs, setLogs] = useState<string[]>([]);
    const [isLoadingLogs, setIsLoadingLogs] = useState(false);
    const [smartImportId, setSmartImportId] = useState<string | null>(null);
    const [smartImportUrl, setSmartImportUrl] = useState('');

    // Ensure pre-installed servers exist
    useEffect(() => {
        // Use npx --yes to auto-install tsx without prompting (cached after first run)
        const naparnikArgs = ['--yes', 'tsx', 'src/mcp-servers/1c-naparnik.ts'];
        const metadataArgs = ['--yes', 'tsx', 'src/mcp-servers/1c-metadata.ts'];

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
        } else {
            const srv = updatedServers[naparnikIndex];
            // Migrate from node_modules/.bin/tsx to npx
            const needsCommandFix = srv.command !== 'npx' || JSON.stringify(srv.args) !== JSON.stringify(naparnikArgs);
            if (needsCommandFix) {
                updatedServers[naparnikIndex] = { ...srv, command: 'npx', args: naparnikArgs };
                needsUpdate = true;
            }
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
        } else {
            const srv = updatedServers[metadataIndex];
            // Migrate from node_modules/.bin/tsx to npx
            const needsCommandFix = srv.command !== 'npx' || JSON.stringify(srv.args) !== JSON.stringify(metadataArgs);
            if (needsCommandFix) {
                updatedServers[metadataIndex] = { ...srv, command: 'npx', args: metadataArgs };
                needsUpdate = true;
            }
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

        // Check 1С:Справка
        const helpIndex = updatedServers.findIndex(s => s.id === BUILTIN_1C_HELP_ID);
        if (helpIndex === -1) {
            updatedServers.push({
                id: BUILTIN_1C_HELP_ID,
                name: '1С:Справка',
                enabled: false,
                transport: 'stdio',
                command: 'npx',
                args: ['--yes', 'tsx', 'src/mcp-servers/1c-help.ts'],
            });
            needsUpdate = true;
        } else {
            const srv = updatedServers[helpIndex];
            const expectedArgs = ['--yes', 'tsx', 'src/mcp-servers/1c-help.ts'];
            if (srv.command !== 'npx' || JSON.stringify(srv.args) !== JSON.stringify(expectedArgs)) {
                updatedServers[helpIndex] = { ...srv, command: 'npx', args: expectedArgs };
                needsUpdate = true;
            }
        }

        // Сортируем серверы по нужному порядку карточек
        const ORDER: Record<string, number> = {
            [BUILTIN_BSL_LS_ID]: 0,
            [BUILTIN_1C_HELP_ID]: 1,
            [BUILTIN_1C_SERVER_ID]: 2,
            [BUILTIN_1C_METADATA_ID]: 3,
        };
        updatedServers.sort((a, b) => {
            const oa = ORDER[a.id] ?? 99;
            const ob = ORDER[b.id] ?? 99;
            return oa - ob;
        });
        needsUpdate = true; // всегда сохраняем порядок

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

    const handleSmartImport = (id: string, urlStr: string) => {
        const val = urlStr.trim();
        if (!val) return;

        try {
            const url = new URL(val.startsWith('http') ? val : `http://${val}`);
            const proto = url.protocol.replace(':', '');
            const host = url.host;
            const pathParts = url.pathname.split('/').filter(p => p && p !== 'hs' && p !== 'mcp');
            const base = pathParts[0] || 'base';

            const newUrl = `${proto}://${host}/${base}/hs/mcp`;
            const server = servers.find(s => s.id === id);
            if (server) {
                const newEnv = {
                    ...(server.env || {}),
                    'ONEC_METADATA_URL': newUrl
                };
                handleUpdateServer(id, { env: newEnv });
            }
            setSmartImportId(null);
            setSmartImportUrl('');
        } catch (err) {
            console.error("Invalid URL for smart import", err);
        }
    };

    const sortedServers = [...servers].sort((a, b) => {
        const builtinIds = [BUILTIN_BSL_LS_ID, BUILTIN_1C_HELP_ID, BUILTIN_1C_SERVER_ID, BUILTIN_1C_METADATA_ID];
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
                        const isHelp = server.id === BUILTIN_1C_HELP_ID;
                        const isBuiltin = server.id === BUILTIN_1C_SERVER_ID || isMetadata || isBslLs || isHelp;

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
                                    px-4 py-3 border-b flex items-center justify-between gap-3 flex-wrap
                                    ${isBuiltin
                                        ? 'bg-yellow-500/5 border-yellow-500/20'
                                        : 'bg-zinc-800/80 border-zinc-700'
                                    }
                                `}>
                                    <div className="flex items-center gap-3 min-w-0">
                                        <div className={`w-2 h-2 rounded-full shrink-0 transition-all duration-300 ${server.enabled ? (isConnected ? 'bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.5)]' : 'bg-red-500 animate-pulse') : 'bg-zinc-600'}`} title={server.enabled ? (isConnected ? "Connected" : "Disconnected") : "Disabled"} />

                                        {isBuiltin ? (
                                            <div className="flex items-center gap-2 min-w-0">
                                                {isMetadata ? <Database className="w-4 h-4 text-yellow-500 shrink-0" /> : isBslLs ? <Cpu className="w-4 h-4 text-yellow-500 shrink-0" /> : isHelp ? <FileText className="w-4 h-4 text-yellow-500 shrink-0" /> : <Sparkles className="w-4 h-4 text-yellow-500 shrink-0" />}
                                                <span className="text-zinc-100 font-medium text-sm truncate">{server.name}</span>
                                                <span className="text-[10px] px-1.5 py-0.5 rounded border bg-yellow-500/10 text-yellow-400 border-yellow-500/20 whitespace-nowrap shrink-0">
                                                    PRE-INSTALLED
                                                </span>
                                            </div>
                                        ) : (
                                            <input
                                                type="text"
                                                value={server.name}
                                                onChange={(e) => handleUpdateServer(server.id, { name: e.target.value })}
                                                className="bg-transparent border-none text-zinc-100 font-medium focus:ring-0 p-0 text-sm w-full min-w-[100px]"
                                                placeholder="Название сервера"
                                            />
                                        )}

                                        {server.enabled && (
                                            <span className={`text-[10px] px-1.5 py-0.5 rounded border whitespace-nowrap shrink-0 ${isConnected ? 'bg-green-500/10 text-green-400 border-green-500/20' : 'bg-red-500/10 text-red-400 border-red-500/20'}`}>
                                                {isConnected ? 'LIVE' : (isStopped ? 'STOPPED' : 'OFFLINE')}
                                            </span>
                                        )}
                                    </div>

                                    <div className="flex items-center gap-3 ml-auto">
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
                                                    <div className="flex items-center justify-between mb-1">
                                                        <label className="text-[10px] text-zinc-500 uppercase font-bold flex items-center gap-1">
                                                            <Key className="w-3 h-3" /> 1C.ai Token
                                                        </label>
                                                        <a
                                                            href="https://code.1c.ai/tokens/"
                                                            target="_blank"
                                                            rel="noopener noreferrer"
                                                            className="text-[10px] text-blue-400 hover:text-blue-300 transition-colors flex items-center gap-1"
                                                        >
                                                            <Link2 className="w-2.5 h-2.5" /> Получить токен
                                                        </a>
                                                    </div>
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
                                            ) : isHelp ? (
                                                (() => {
                                                    const helpSt = status?.help_status || '';
                                                    const prog = status?.index_progress || 0;
                                                    const msg = status?.index_message || '';
                                                    // Парсим index_message: "Готово: 52064 тем (платформа 8.3.27.1989)"
                                                    let helpVersion = ''; let helpCount = '';
                                                    if (helpSt === 'ready') {
                                                        const countMatch = msg.match(/Готово: ([\d\s]+) тем/);
                                                        const versionMatch = msg.match(/платформа ([^\)]+)/);
                                                        helpCount = countMatch?.[1]?.trim() || '';
                                                        helpVersion = versionMatch?.[1]?.trim() || '';
                                                    }
                                                    const handleReindex = async () => {
                                                        try {
                                                            await invoke('call_mcp_tool', { serverId: server.id, toolName: 'reindex_1c_help', args: {} });
                                                        } catch { /* UI обновится через статус */ }
                                                    };
                                                    if (helpSt === 'unavailable') {
                                                        return (
                                                            <div className="bg-amber-500/5 border border-amber-500/20 rounded-lg p-3 flex items-start gap-3">
                                                                <AlertCircle className="w-4 h-4 text-amber-500 shrink-0 mt-0.5" />
                                                                <div>
                                                                    <p className="text-xs text-amber-300 font-medium">Платформа 1С:Предприятие не найдена</p>
                                                                    <p className="text-[10px] text-zinc-500 mt-1">Установите 1С:Предприятие 8.3 для использования справки.</p>
                                                                </div>
                                                            </div>
                                                        );
                                                    } else if (helpSt === 'indexing') {
                                                        return (
                                                            <div className="space-y-2">
                                                                <div className="flex items-center justify-between text-[10px] text-zinc-400">
                                                                    <span className="flex items-center gap-1">
                                                                        <Activity className="w-3 h-3 animate-pulse text-blue-400" />
                                                                        Подготовка базы данных справки...
                                                                    </span>
                                                                    <span className="font-mono text-blue-400">{prog}%</span>
                                                                </div>
                                                                <div className="w-full bg-zinc-800 rounded-full h-1.5 overflow-hidden">
                                                                    <div
                                                                        className="bg-gradient-to-r from-blue-600 to-blue-400 h-1.5 rounded-full transition-all duration-500"
                                                                        style={{ width: `${Math.max(2, prog)}%` }}
                                                                    />
                                                                </div>
                                                                {msg && <p className="text-[10px] text-zinc-500 truncate">{msg}</p>}
                                                            </div>
                                                        );
                                                    } else if (helpSt === 'ready') {
                                                        return (
                                                            <div className="space-y-2">
                                                                <div className="bg-green-500/5 border border-green-500/20 rounded-lg p-3 flex items-center justify-between">
                                                                    <div className="flex items-start gap-3">
                                                                        <CheckCircle2 className="w-4 h-4 text-green-500 shrink-0 mt-0.5" />
                                                                        <div>
                                                                            <p className="text-xs text-green-300 font-medium">Справка готова к использованию</p>
                                                                            <p className="text-[10px] text-zinc-500 mt-0.5">
                                                                                {helpCount ? `${Number(helpCount).toLocaleString('ru')} тем` : 'тем: —'}
                                                                                {helpVersion ? ` · платформа ${helpVersion}` : ''}
                                                                            </p>
                                                                        </div>
                                                                    </div>
                                                                    <button
                                                                        onClick={handleReindex}
                                                                        className="flex items-center gap-1 px-2 py-1 bg-zinc-700/60 hover:bg-zinc-600/60 text-zinc-400 hover:text-zinc-200 rounded text-[10px] font-medium transition shrink-0"
                                                                        title="Переиндексировать справку"
                                                                    >
                                                                        <Activity className="w-3 h-3" /> Обновить
                                                                    </button>
                                                                </div>
                                                            </div>
                                                        );
                                                    } else {
                                                        return (
                                                            <div className="bg-zinc-900/50 border border-yellow-500/10 rounded-lg p-3 text-xs text-zinc-400 italic">
                                                                Поиск по официальной справке платформы 1С:Предприятие 8.3.
                                                                При первом включении индексация займёт 1-3 минуты.
                                                            </div>
                                                        );
                                                    }
                                                })()
                                            ) : (
                                                <>
                                                    <div className="flex items-center justify-between mb-4">
                                                        <div className="flex items-center gap-2">
                                                            <div className="w-1.5 h-1.5 rounded-full bg-blue-500 animate-pulse" />
                                                            <span className="text-[10px] text-zinc-400 font-medium">Параметры соединения</span>
                                                        </div>
                                                        <button
                                                            onClick={() => setSmartImportId(server.id)}
                                                            className="flex items-center gap-1.5 px-2 py-1 bg-blue-500/10 hover:bg-blue-500/20 text-blue-400 rounded-md text-[10px] font-bold transition border border-blue-500/20"
                                                        >
                                                            <Sparkles className="w-3 h-3" /> Импорт URL
                                                        </button>
                                                    </div>

                                                    <div className="flex flex-wrap gap-2">
                                                        <div className="flex-1 min-w-[100px]">
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
                                                        <div className="flex-[2] min-w-[150px]">
                                                            <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                                <Terminal className="w-3 h-3" /> Host
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
                                                        <div className="flex-1 min-w-[120px]">
                                                            <label className="text-[10px] text-zinc-500 uppercase font-bold mb-1 block flex items-center gap-1">
                                                                <Database className="w-3 h-3" /> Base
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
                                                    <div className="flex flex-wrap gap-4">
                                                        <div className="flex-1 min-w-[140px]">
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
                                                        <div className="flex-1 min-w-[140px]">
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
                                    <div className="flex flex-wrap items-center justify-between gap-y-3 pt-1">
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
                                            <div className={`flex items-center gap-2 text-xs font-medium ${testResults[server.id].success ? 'text-green-400' : 'text-red-400'} min-w-0 max-w-full`}>
                                                {testResults[server.id].success ? <CheckCircle2 className="w-3.5 h-3.5 shrink-0" /> : <AlertCircle className="w-3.5 h-3.5 shrink-0" />}
                                                <span className="truncate">{testResults[server.id].message}</span>
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
            {/* Smart Import Modal */}
            {smartImportId && (
                <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 backdrop-blur-md p-4 animate-in fade-in duration-200">
                    <div className="bg-zinc-900 border border-zinc-700/50 rounded-2xl w-full max-w-lg shadow-[0_20px_50px_rgba(0,0,0,0.5)] overflow-hidden">
                        <div className="px-6 py-4 border-b border-zinc-800 flex items-center justify-between bg-zinc-900/50">
                            <div className="flex items-center gap-3">
                                <div className="p-2 bg-blue-500/10 rounded-xl">
                                    <Sparkles className="w-5 h-5 text-blue-400" />
                                </div>
                                <div>
                                    <h3 className="font-bold text-zinc-100 italic">Импорт публикации</h3>
                                    <p className="text-[10px] text-zinc-500">Автозаполнение параметров из URL</p>
                                </div>
                            </div>
                            <button
                                onClick={() => {
                                    setSmartImportId(null);
                                    setSmartImportUrl('');
                                }}
                                className="p-2 hover:bg-zinc-800 rounded-lg text-zinc-500 hover:text-zinc-200 transition"
                            >
                                <X className="w-5 h-5" />
                            </button>
                        </div>

                        <div className="p-6 space-y-4">
                            <div className="space-y-2">
                                <label className="text-[10px] text-zinc-500 uppercase font-bold tracking-wider">Вставьте URL публикации</label>
                                <div className="relative group">
                                    <input
                                        autoFocus
                                        type="text"
                                        value={smartImportUrl}
                                        onChange={(e) => setSmartImportUrl(e.target.value)}
                                        onKeyDown={(e) => {
                                            if (e.key === 'Enter') handleSmartImport(smartImportId, smartImportUrl);
                                            if (e.key === 'Escape') setSmartImportId(null);
                                        }}
                                        className="w-full bg-zinc-950 border border-zinc-800 group-focus-within:border-blue-500/50 rounded-xl px-4 py-3 text-sm text-zinc-100 focus:outline-none focus:ring-4 focus:ring-blue-500/5 transition-all placeholder:text-zinc-700"
                                        placeholder="http://myserver/demo_base"
                                    />
                                    <Globe className="absolute right-4 top-1/2 -translate-y-1/2 w-4 h-4 text-zinc-800 group-focus-within:text-blue-500/30 transition-colors" />
                                </div>
                            </div>

                            <div className="bg-blue-500/5 border border-blue-500/10 rounded-xl p-4 flex gap-3">
                                <Activity className="w-5 h-5 text-blue-400/50 shrink-0" />
                                <div className="space-y-1">
                                    <p className="text-[11px] text-zinc-300 leading-relaxed font-medium">
                                        Система автоматически извлечет протокол, хост и имя базы.
                                    </p>
                                    <p className="text-[10px] text-zinc-500">
                                        Например: из <code>http://dev/base</code> получится <b>dev</b> и <b>base</b>.
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div className="px-6 py-4 bg-zinc-900/80 border-t border-zinc-800 flex items-center justify-end gap-3">
                            <button
                                onClick={() => {
                                    setSmartImportId(null);
                                    setSmartImportUrl('');
                                }}
                                className="px-4 py-2 hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 rounded-xl text-xs font-bold transition-colors"
                            >
                                Отмена
                            </button>
                            <button
                                onClick={() => handleSmartImport(smartImportId, smartImportUrl)}
                                disabled={!smartImportUrl.trim()}
                                className="px-6 py-2 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 disabled:bg-zinc-800 disabled:text-zinc-600 text-white rounded-xl text-xs font-bold shadow-lg shadow-blue-900/20 transition-all active:scale-95"
                            >
                                Импортировать
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
