import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { Download, RefreshCw, Upload, Info, ExternalLink, FolderOpen, Terminal, CheckCircle2, AlertCircle, Network } from 'lucide-react';
import { getVersion } from '@tauri-apps/api/app';

import {
    exportSettings,
    importSettingsFromFile,
    validateImportSettingsFile,
} from '../../api/settings';
import { AppSettings, DEFAULT_PROXY_SETTINGS, ProxyMode, ProxyProtocol, ProxySettings } from '../../types/settings';
import { getNodePathInputValue, getNodePathPreview } from '../../utils/mcpNodePath';
import { normalizeProxyPortInput } from '../../utils/proxySettings';

type UpdateStatus = 'idle' | 'checking' | 'up-to-date' | 'update-available' | 'error';

interface ReleaseInfo {
    version: string;
    url: string;
    name: string;
}

interface GeneralTabProps {
    settings: AppSettings;
    setSettings: (settings: AppSettings) => void;
    onConfigurationImported: () => Promise<void>;
}

type StatusTone = 'success' | 'error';

type NodePathCheckResult = {
    success: boolean;
    message: string;
};

export function GeneralTab({
    settings,
    setSettings,
    onConfigurationImported,
}: GeneralTabProps) {
    const [transferStatus, setTransferStatus] = useState<string>('');
    const [statusTone, setStatusTone] = useState<StatusTone>('success');
    const [exporting, setExporting] = useState(false);
    const [importing, setImporting] = useState(false);

    const [appVersion, setAppVersion] = useState<string>('...');
    const [updateStatus, setUpdateStatus] = useState<UpdateStatus>('idle');
    const [latestRelease, setLatestRelease] = useState<ReleaseInfo | null>(null);
    const [detectedNodePath, setDetectedNodePath] = useState<string | null>(null);
    const [checkingNodePath, setCheckingNodePath] = useState(false);
    const [nodePathCheckResult, setNodePathCheckResult] = useState<NodePathCheckResult | null>(null);

    const nodePathInputValue = getNodePathInputValue(settings.node_path);
    const nodePathPreview = getNodePathPreview(settings.node_path, detectedNodePath);
    const nodePathToCheck = nodePathInputValue || settings.node_path || 'node';
    const proxy = settings.proxy ?? DEFAULT_PROXY_SETTINGS;

    const updateProxy = (updates: Partial<ProxySettings>) => {
        setSettings({
            ...settings,
            proxy: {
                ...DEFAULT_PROXY_SETTINGS,
                ...proxy,
                ...updates,
            },
        });
    };

    const updateProxyPort = (value: string) => {
        const port = normalizeProxyPortInput(value);
        if (port !== undefined) {
            updateProxy({ port });
        }
    };

    useEffect(() => {
        getVersion().then(setAppVersion).catch(() => setAppVersion('?'));
    }, []);

    useEffect(() => {
        let cancelled = false;

        invoke<string | null>('resolve_node_path_cmd')
            .then((path) => {
                if (!cancelled) setDetectedNodePath(path);
            })
            .catch(() => {
                if (!cancelled) setDetectedNodePath(null);
            });

        return () => {
            cancelled = true;
        };
    }, []);

    const setNodePath = (nodePath: string) => {
        setNodePathCheckResult(null);
        setSettings({ ...settings, node_path: nodePath });
    };

    const browseNodePath = async () => {
        try {
            const file = await open({
                multiple: false,
                directory: false,
                filters: [{ name: 'Node.js', extensions: ['exe'] }],
                title: 'Выберите node.exe',
            });

            if (file && typeof file === 'string') {
                setNodePath(file);
            }
        } catch (error) {
            console.error('Failed to open node executable dialog:', error);
        }
    };

    const checkNodePath = async () => {
        setCheckingNodePath(true);
        setNodePathCheckResult(null);

        try {
            const version = await invoke<string>('check_node_path_cmd', { nodePath: nodePathToCheck });
            setNodePathCheckResult({
                success: true,
                message: `${version} · ${nodePathPreview}`,
            });
        } catch (error) {
            setNodePathCheckResult({
                success: false,
                message: String(error),
            });
        } finally {
            setCheckingNodePath(false);
        }
    };

    const checkForUpdates = async () => {
        setUpdateStatus('checking');
        setLatestRelease(null);
        try {
            const res = await fetch('https://api.github.com/repos/hawkxtreme/mini-ai-1c/releases/latest', {
                headers: { Accept: 'application/vnd.github+json' },
            });
            if (!res.ok) throw new Error(`HTTP ${res.status}`);
            const data = await res.json();
            const latest = (data.tag_name as string).replace(/^v/, '');
            const release: ReleaseInfo = { version: latest, url: data.html_url, name: data.name || `v${latest}` };
            setLatestRelease(release);

            const current = appVersion.replace(/^v/, '');
            setUpdateStatus(current === latest ? 'up-to-date' : 'update-available');
        } catch {
            setUpdateStatus('error');
        }
    };

    const handleExport = async () => {
        setExporting(true);
        try {
            const result = await exportSettings();
            if (result.status === 'cancelled') {
                setTransferStatus('');
                return;
            }

            setStatusTone('success');
            setTransferStatus('✓ Настройки экспортированы.');
        } catch (error) {
            setStatusTone('error');
            setTransferStatus(`Ошибка экспорта: ${error}`);
        } finally {
            setExporting(false);
        }
    };

    const handleImport = async () => {
        setImporting(true);
        try {
            const selectedFile = await open({
                multiple: false,
                directory: false,
                filters: [{ name: 'JSON', extensions: ['json'] }],
            });

            if (!selectedFile || typeof selectedFile !== 'string') {
                setTransferStatus('');
                return;
            }

            await validateImportSettingsFile(selectedFile);

            const confirmed = window.confirm(
                'Импортировать настройки и LLM-профили? Текущая конфигурация будет заменена, а локальные API-ключи, токены и пароли сохранятся.'
            );

            if (!confirmed) {
                setTransferStatus('');
                return;
            }

            await importSettingsFromFile(selectedFile);
            await onConfigurationImported();

            setStatusTone('success');
            setTransferStatus('✓ Настройки импортированы и применены.');
        } catch (error) {
            setStatusTone('error');
            setTransferStatus(`Ошибка импорта: ${error}`);
        } finally {
            setImporting(false);
        }
    };

    return (
        <div className="h-full w-full overflow-y-auto p-4 sm:p-8">
            <div className="mx-auto max-w-2xl space-y-6 sm:space-y-8">
                <section>
                    <h3 className="mb-4 flex items-center gap-2 text-lg font-medium text-zinc-100">
                        <Network className="h-4 w-4 text-blue-400" />
                        Прокси
                    </h3>

                    <div className="space-y-4 rounded-xl border border-zinc-700 bg-zinc-800/50 p-5">
                        <div className="overflow-hidden rounded-lg border border-zinc-700 text-xs font-medium">
                            <div className="flex">
                                {([
                                    ['disabled', 'Выкл'],
                                    ['system', 'Системный'],
                                    ['custom', 'Свой'],
                                ] as const).map(([mode, label], i) => {
                                    const active = proxy.mode === mode;
                                    return (
                                        <button
                                            key={mode}
                                            type="button"
                                            onClick={() => updateProxy({ mode: mode as ProxyMode })}
                                            className={`flex-1 py-2 transition-colors ${
                                                i > 0 ? 'border-l border-zinc-700' : ''
                                            } ${
                                                active
                                                    ? 'bg-blue-600 text-white'
                                                    : 'bg-zinc-800 text-zinc-400 hover:bg-zinc-700 hover:text-zinc-200'
                                            }`}
                                        >
                                            {label}
                                        </button>
                                    );
                                })}
                            </div>
                        </div>

                        {proxy.mode === 'custom' && (
                            <div className="space-y-3">
                                <div className="grid gap-3 sm:grid-cols-[130px_minmax(0,1fr)_110px]">
                                    <select
                                        value={proxy.protocol}
                                        onChange={(event) => updateProxy({ protocol: event.target.value as ProxyProtocol })}
                                        className="rounded-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-sm text-zinc-200 focus:border-blue-500 focus:outline-none"
                                    >
                                        <option value="http">HTTP</option>
                                        <option value="socks5">SOCKS5</option>
                                    </select>
                                    <input
                                        type="text"
                                        value={proxy.host}
                                        onChange={(event) => updateProxy({ host: event.target.value })}
                                        placeholder="proxy.company.local"
                                        className="min-w-0 rounded-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-sm text-zinc-200 placeholder-zinc-600 focus:border-blue-500 focus:outline-none"
                                    />
                                    <input
                                        type="number"
                                        min={1}
                                        max={65535}
                                        step={1}
                                        value={proxy.port ?? ''}
                                        onChange={(event) => updateProxyPort(event.target.value)}
                                        placeholder="8080"
                                        className="rounded-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-sm text-zinc-200 placeholder-zinc-600 focus:border-blue-500 focus:outline-none"
                                    />
                                </div>

                                <div className="grid gap-3 sm:grid-cols-2">
                                    <input
                                        type="text"
                                        value={proxy.username}
                                        onChange={(event) => updateProxy({ username: event.target.value })}
                                        placeholder="Логин"
                                        className="min-w-0 rounded-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-sm text-zinc-200 placeholder-zinc-600 focus:border-blue-500 focus:outline-none"
                                    />
                                    <input
                                        type="password"
                                        value={proxy.password}
                                        onChange={(event) => updateProxy({ password: event.target.value })}
                                        placeholder="Пароль"
                                        className="min-w-0 rounded-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-sm text-zinc-200 placeholder-zinc-600 focus:border-blue-500 focus:outline-none"
                                    />
                                </div>
                            </div>
                        )}
                    </div>
                </section>

                <section>
                    <h3 className="mb-4 text-lg font-medium text-zinc-100">Сжатие контекста</h3>

                    <div className="space-y-4 rounded-xl border border-zinc-700 bg-zinc-800/50 p-5">
                        <p className="text-sm text-zinc-400">
                            Что делать, когда история чата становится слишком длинной.
                        </p>

                        <div className="overflow-hidden rounded-lg border border-zinc-700 text-xs font-medium">
                            <div className="flex">
                                {(['disabled', 'sliding_window', 'summarize'] as const).map((opt, i) => {
                                    const labels = {
                                        disabled: 'Выкл',
                                        sliding_window: 'Скользящее окно',
                                        summarize: 'Суммаризация',
                                    };
                                    const hints = {
                                        disabled: 'Без сжатия',
                                        sliding_window:
                                            'Сохраняет первое сообщение + последние N, удаляет середину',
                                        summarize:
                                            'LLM создаёт конспект диалога (не работает с QwenCLI / Напарником)',
                                    };
                                    const active = (settings.context_compress_strategy || 'summarize') === opt;

                                    return (
                                        <button
                                            key={opt}
                                            type="button"
                                            title={hints[opt]}
                                            onClick={() => setSettings({ ...settings, context_compress_strategy: opt })}
                                            className={`flex-1 py-2 transition-colors ${
                                                i > 0 ? 'border-l border-zinc-700' : ''
                                            } ${
                                                active
                                                    ? 'bg-blue-600 text-white'
                                                    : 'bg-zinc-800 text-zinc-400 hover:bg-zinc-700 hover:text-zinc-200'
                                            }`}
                                        >
                                            {labels[opt]}
                                        </button>
                                    );
                                })}
                            </div>
                        </div>


                        {settings.context_compress_strategy === 'summarize' && (
                            <p className="text-[11px] text-zinc-600">
                                ⚠ Суммаризация недоступна для CodexCLI, QwenCLI и 1С:Напарника — автоматически
                                используется скользящее окно.
                            </p>
                        )}
                    </div>
                </section>

                <section>
                    <h3 className="mb-4 text-lg font-medium text-zinc-100">Node.js</h3>

                    <div className="space-y-4 rounded-xl border border-zinc-700 bg-zinc-800/50 p-5">
                        <div className="space-y-2">
                            <label className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-zinc-500">
                                <Terminal className="h-3 w-3" />
                                Путь к Node.js
                            </label>
                            <div className="flex gap-2">
                                <input
                                    type="text"
                                    value={nodePathInputValue}
                                    onChange={(event) => setNodePath(event.target.value)}
                                    className="min-w-0 flex-1 rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-1.5 font-mono text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none focus:ring-1 focus:ring-blue-500"
                                    placeholder={detectedNodePath || 'node или C:\\portable\\node\\node.exe'}
                                />
                                <button
                                    type="button"
                                    onClick={() => void browseNodePath()}
                                    className="flex shrink-0 items-center gap-1.5 rounded-lg bg-zinc-700 px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:bg-zinc-600 hover:text-zinc-100"
                                    title="Выбрать node.exe"
                                >
                                    <FolderOpen className="h-3.5 w-3.5" />
                                </button>
                                <button
                                    type="button"
                                    onClick={() => void checkNodePath()}
                                    disabled={checkingNodePath}
                                    className="flex shrink-0 items-center gap-1.5 rounded-lg bg-zinc-700 px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:bg-zinc-600 hover:text-zinc-100 disabled:cursor-not-allowed disabled:opacity-60"
                                >
                                    <RefreshCw className={`h-3.5 w-3.5 ${checkingNodePath ? 'animate-spin' : ''}`} />
                                    Проверить
                                </button>
                            </div>
                            <p className="break-all font-mono text-[11px] text-zinc-500">
                                Текущий путь: <span className="text-zinc-300">{nodePathPreview}</span>
                            </p>
                        </div>

                        {nodePathCheckResult && (
                            <div className={`flex items-center gap-2 text-xs ${nodePathCheckResult.success ? 'text-green-400' : 'text-red-400'}`}>
                                {nodePathCheckResult.success ? (
                                    <CheckCircle2 className="h-3.5 w-3.5 shrink-0" />
                                ) : (
                                    <AlertCircle className="h-3.5 w-3.5 shrink-0" />
                                )}
                                <span className="break-all">{nodePathCheckResult.message}</span>
                            </div>
                        )}
                    </div>
                </section>

                <section>
                    <h3 className="mb-4 text-lg font-medium text-zinc-100">Экспорт / Импорт настроек</h3>

                    <div className="space-y-4 rounded-xl border border-zinc-700 bg-zinc-800/50 p-5">
                        <div>
                            <div className="text-xs text-zinc-500">
                                Перенос конфигурации приложения и LLM-профилей между компьютерами. API-ключи, токены и пароли в экспорт не включаются.
                            </div>
                        </div>

                        <div className="flex flex-wrap gap-2">
                            <button
                                type="button"
                                onClick={handleExport}
                                disabled={exporting || importing}
                                className="flex items-center gap-2 rounded-lg border border-zinc-600 bg-zinc-700 px-3 py-1.5 text-xs text-zinc-200 transition-colors hover:bg-zinc-600 disabled:cursor-not-allowed disabled:opacity-60"
                            >
                                {exporting ? (
                                    <RefreshCw className="h-4 w-4 animate-spin" />
                                ) : (
                                    <Download className="h-4 w-4" />
                                )}
                                Экспорт настроек
                            </button>

                            <button
                                type="button"
                                onClick={handleImport}
                                disabled={exporting || importing}
                                className="flex items-center gap-2 rounded-lg border border-zinc-600 bg-zinc-700 px-3 py-1.5 text-xs text-zinc-200 transition-colors hover:bg-zinc-600 disabled:cursor-not-allowed disabled:opacity-60"
                            >
                                {importing ? (
                                    <RefreshCw className="h-4 w-4 animate-spin" />
                                ) : (
                                    <Upload className="h-4 w-4" />
                                )}
                                Импорт настроек
                            </button>
                        </div>

                        {transferStatus && (
                            <p className={`text-xs ${statusTone === 'success' ? 'text-green-400' : 'text-red-400'}`}>
                                {transferStatus}
                            </p>
                        )}
                    </div>
                </section>

                <section>
                    <h3 className="mb-4 text-lg font-medium text-zinc-100">О приложении</h3>

                    <div className="space-y-4 rounded-xl border border-zinc-700 bg-zinc-800/50 p-5">
                        <div className="flex items-center gap-3">
                            <Info className="h-4 w-4 text-zinc-400 shrink-0" />
                            <span className="text-sm text-zinc-300">
                                Mini AI 1C — версия{' '}
                                <span className="font-mono font-semibold text-zinc-100">{appVersion}</span>
                            </span>
                        </div>

                        <div className="flex items-center gap-3 flex-wrap">
                            <button
                                type="button"
                                onClick={checkForUpdates}
                                disabled={updateStatus === 'checking'}
                                className="flex items-center gap-2 rounded-lg border border-zinc-600 bg-zinc-700 px-3 py-1.5 text-xs text-zinc-200 transition-colors hover:bg-zinc-600 disabled:cursor-not-allowed disabled:opacity-60"
                            >
                                {updateStatus === 'checking' ? (
                                    <RefreshCw className="h-4 w-4 animate-spin" />
                                ) : (
                                    <RefreshCw className="h-4 w-4" />
                                )}
                                Проверить обновления
                            </button>

                            {updateStatus === 'up-to-date' && (
                                <span className="text-xs text-green-400">✓ Установлена актуальная версия</span>
                            )}

                            {updateStatus === 'update-available' && latestRelease && (
                                <span className="flex items-center gap-1.5 text-xs text-yellow-400">
                                    Доступна версия {latestRelease.version} —{' '}
                                    <a
                                        href={latestRelease.url}
                                        target="_blank"
                                        rel="noreferrer"
                                        className="inline-flex items-center gap-1 underline hover:text-yellow-300"
                                    >
                                        скачать <ExternalLink className="h-3 w-3" />
                                    </a>
                                </span>
                            )}

                            {updateStatus === 'error' && (
                                <span className="text-xs text-red-400">Не удалось проверить обновления</span>
                            )}
                        </div>
                    </div>
                </section>
            </div>
        </div>
    );
}
