import React, { useEffect, useRef, useState } from 'react';
import {
    AlertCircle,
    CheckCircle,
    CircleHelp,
    Download,
    Monitor,
    RefreshCw,
    XCircle
} from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { AppSettings, WindowInfo } from '../../types/settings';
import { parseConfiguratorTitle } from '../../utils/configurator';

interface ConfiguratorTabProps {
    settings: AppSettings;
    setSettings: (settings: AppSettings) => void;
    detectedWindows: WindowInfo[];
    refreshWindows: () => void;
    testCapture: (hwnd: number) => void;
    testCaptureResult: string | null;
}

type BridgeStatus = 'checking' | 'ready' | 'missing' | 'unknown';

export function ConfiguratorTab({
    settings,
    setSettings,
    detectedWindows,
    refreshWindows,
    testCapture,
    testCaptureResult
}: ConfiguratorTabProps) {
    const [bridgeStatus, setBridgeStatus] = useState<BridgeStatus>('unknown');
    const [checking, setChecking] = useState(false);
    const [restartingBridge, setRestartingBridge] = useState(false);
    const [bridgeMessage, setBridgeMessage] = useState<string | null>(null);
    const [showUsageHelp, setShowUsageHelp] = useState(false);
    const usageHelpRef = useRef<HTMLDivElement>(null);
    const [downloading, setDownloading] = useState(false);
    const [downloadProgress, setDownloadProgress] = useState(0);
    const [downloadError, setDownloadError] = useState<string | null>(null);
    const [downloadSuccess, setDownloadSuccess] = useState(false);
    const downloadCancelledRef = useRef(false);

    const bridgeEnabled = settings.configurator.editor_bridge_enabled ?? false;
    const autoApply = settings.configurator.editor_bridge_auto_apply ?? false;

    async function checkStatus() {
        setChecking(true);
        setBridgeMessage(null);
        setBridgeStatus('checking');

        try {
            const result = await invoke<{ bridge: boolean }>('check_editor_bridge_status');
            setBridgeStatus(result.bridge ? 'ready' : 'missing');
        } catch {
            setBridgeStatus('unknown');
        } finally {
            setChecking(false);
        }
    }

    async function handleDownloadBridge() {
        downloadCancelledRef.current = false;
        setDownloading(true);
        setDownloadError(null);
        setDownloadSuccess(false);
        const unlisten = await listen<{ percent: number }>('editor-bridge-download-progress', (event) => {
            setDownloadProgress(event.payload.percent);
        });
        try {
            const path = await invoke<string>('install_editor_bridge_cmd');
            if (!downloadCancelledRef.current) {
                setSettings({
                    ...settings,
                    configurator: { ...settings.configurator, editor_bridge_exe_path: path }
                });
                setDownloadSuccess(true);
                await checkStatus();
            }
        } catch (e) {
            if (!downloadCancelledRef.current) {
                setDownloadError(String(e));
            }
        } finally {
            unlisten();
            setDownloading(false);
            setDownloadProgress(0);
        }
    }

    function cancelDownload() {
        downloadCancelledRef.current = true;
        setDownloading(false);
        setDownloadProgress(0);
        setDownloadError(null);
    }

    async function restartBridge() {
        setRestartingBridge(true);
        setBridgeMessage(null);

        try {
            await invoke('restart_editor_bridge_cmd');
            setBridgeMessage('EditorBridge перезапущен.');
            await checkStatus();
        } catch (error) {
            console.error('Failed to restart EditorBridge', error);
            setBridgeMessage(`Не удалось перезапустить EditorBridge: ${String(error)}`);
        } finally {
            setRestartingBridge(false);
        }
    }

    useEffect(() => {
        void checkStatus();
    }, []);

    useEffect(() => {
        if (!showUsageHelp) {
            return;
        }

        const handlePointerDown = (event: MouseEvent) => {
            if (!usageHelpRef.current?.contains(event.target as Node)) {
                setShowUsageHelp(false);
            }
        };

        document.addEventListener('mousedown', handlePointerDown);
        return () => document.removeEventListener('mousedown', handlePointerDown);
    }, [showUsageHelp]);

    function updateConf(patch: Partial<AppSettings['configurator']>) {
        setSettings({ ...settings, configurator: { ...settings.configurator, ...patch } });
    }

    return (
        <div className="h-full w-full overflow-y-auto p-4 sm:p-8">
            <div className="mx-auto max-w-2xl space-y-6">
                <section>
                    <h3 className="mb-3 flex items-center gap-2 text-base font-semibold">
                        <Monitor className="h-4 w-4 text-blue-500" />
                        Интеграция с 1С:Конфигуратором
                    </h3>

                    <div className="space-y-3 rounded-xl border border-zinc-700 bg-zinc-800/50 p-4">
                        <div className="mb-1 text-xs font-semibold uppercase text-zinc-400">Состояние</div>

                        <StatusRow label="EditorBridge.exe" status={bridgeStatus} checking={checking} />

                        {detectedWindows.length === 0 && (
                            <div className="mt-1 flex items-center gap-1.5 text-xs italic text-zinc-500">
                                <AlertCircle className="h-3.5 w-3.5 text-yellow-600" />
                                Конфигуратор не обнаружен. Откройте 1С:Конфигуратор.
                            </div>
                        )}

                        <div className="mt-2 flex flex-wrap items-center gap-2">
                            <button
                                onClick={() => void checkStatus()}
                                disabled={checking}
                                className="flex items-center gap-1.5 rounded bg-zinc-700 px-3 py-1.5 text-xs text-zinc-200 transition-colors hover:bg-zinc-600 disabled:opacity-50"
                            >
                                <RefreshCw className={`h-3 w-3 ${checking ? 'animate-spin' : ''}`} />
                                Проверить снова
                            </button>
                            <button
                                onClick={() => void restartBridge()}
                                disabled={checking || restartingBridge || bridgeStatus === 'missing'}
                                className="flex items-center gap-1.5 rounded bg-zinc-700 px-3 py-1.5 text-xs text-zinc-200 transition-colors hover:bg-zinc-600 disabled:opacity-50"
                            >
                                <RefreshCw className={`h-3 w-3 ${restartingBridge ? 'animate-spin' : ''}`} />
                                Перезапустить bridge
                            </button>
                            {!downloading ? (
                                <button
                                    onClick={() => void handleDownloadBridge()}
                                    className="flex items-center gap-1.5 rounded bg-green-700 px-3 py-1.5 text-xs text-white transition-colors hover:bg-green-600"
                                >
                                    <Download className="h-3 w-3" />
                                    Скачать EditorBridge
                                </button>
                            ) : (
                                <button
                                    onClick={cancelDownload}
                                    className="flex items-center gap-1.5 rounded bg-zinc-600 px-3 py-1.5 text-xs text-zinc-200 transition-colors hover:bg-zinc-500"
                                >
                                    Отменить
                                </button>
                            )}
                        </div>

                        {downloading && (
                            <div className="mt-2 space-y-1">
                                <div className="flex items-center justify-between text-xs text-zinc-400">
                                    <span>Скачиваю EditorBridge.exe...</span>
                                    <span>{downloadProgress}%</span>
                                </div>
                                <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-700">
                                    <div
                                        className="h-full rounded-full bg-green-500 transition-all duration-300"
                                        style={{ width: `${downloadProgress}%` }}
                                    />
                                </div>
                            </div>
                        )}

                        {downloadSuccess && (
                            <div className="mt-1 flex items-center gap-1.5 text-xs text-green-400">
                                <CheckCircle className="h-3.5 w-3.5" />
                                EditorBridge.exe успешно скачан
                            </div>
                        )}

                        {downloadError && (
                            <div className="mt-1 rounded-lg border border-red-700/50 bg-red-900/30 p-2 text-xs text-red-300">
                                {downloadError}
                            </div>
                        )}

                        {bridgeMessage && <div className="mt-2 text-xs text-zinc-400">{bridgeMessage}</div>}
                    </div>
                </section>

                {bridgeEnabled && (
                    <section>
                    <div className="space-y-3 rounded-xl border border-zinc-700 bg-zinc-800/50 p-4">
                        <div className="flex items-center justify-between gap-3">
                            <div className="text-xs font-semibold uppercase text-zinc-400">Параметры</div>
                            <div className="relative" ref={usageHelpRef}>
                                <button
                                    type="button"
                                    onClick={() => setShowUsageHelp((value) => !value)}
                                    className="inline-flex h-7 w-7 items-center justify-center rounded-full border border-zinc-700 bg-zinc-800 text-zinc-400 transition-colors hover:border-zinc-600 hover:text-zinc-200"
                                    title="Как использовать быстрые действия"
                                    aria-label="Как использовать быстрые действия"
                                >
                                    <CircleHelp className="h-4 w-4" />
                                </button>

                                {showUsageHelp && (
                                    <div className="absolute right-0 top-9 z-20 w-80 rounded-xl border border-zinc-700 bg-zinc-900 p-4 shadow-2xl">
                                        <div className="mb-2 text-sm font-semibold text-zinc-100">Как использовать</div>
                                        <ol className="list-decimal space-y-1.5 pl-5 text-xs leading-5 text-zinc-300">
                                            <li>Откройте любой модуль в Конфигураторе</li>
                                            <li>Поставьте курсор внутри процедуры или функции</li>
                                            <li>Нажмите <span className="rounded bg-zinc-800 px-1.5 py-0.5 font-mono text-[11px]">Ctrl</span> и вызовите правое меню</li>
                                            <li>Выберите нужное действие из меню mini-ai</li>
                                        </ol>
                                    </div>
                                )}
                            </div>
                        </div>

                        <ToggleRow
                            label="Применять сразу без просмотра"
                            description="Сразу записывать безопасный результат без показа диф-редактора."
                            checked={autoApply}
                            onChange={(value) => updateConf({ editor_bridge_auto_apply: value })}
                        />

                    </div>
                    </section>
                )}

                <section>
                    <div className="space-y-3 rounded-xl border border-zinc-700 bg-zinc-800/50 p-4">
                        <div className="text-xs font-semibold uppercase text-zinc-400">Обнаружение окна</div>

                        <div>
                            <label className="mb-1 block text-xs text-zinc-500">Шаблон заголовка</label>
                            <input
                                type="text"
                                value={settings.configurator.window_title_pattern}
                                onChange={(e) => updateConf({ window_title_pattern: e.target.value })}
                                placeholder="Конфигуратор"
                                className="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
                            />
                        </div>

                        <div>
                            <div className="mb-1.5 flex items-center justify-between">
                                <span className="text-xs text-zinc-500">Найденные окна</span>
                                <button
                                    onClick={refreshWindows}
                                    className="flex items-center gap-1 rounded bg-zinc-700 px-2 py-1 text-xs text-zinc-200 transition-colors hover:bg-zinc-600"
                                >
                                    <RefreshCw className="h-3 w-3" />
                                    Обновить
                                </button>
                            </div>

                            <div className="h-28 overflow-y-auto rounded-lg border border-zinc-700 bg-zinc-900">
                                {detectedWindows.length === 0 ? (
                                    <div className="p-4 text-center text-sm italic text-zinc-500">Окна не обнаружены</div>
                                ) : (
                                    detectedWindows.map((windowInfo) => (
                                        <div
                                            key={windowInfo.hwnd}
                                            className="group flex items-center justify-between border-b border-zinc-800 p-2 text-sm hover:bg-zinc-800"
                                        >
                                            <span className="truncate text-zinc-300" title={windowInfo.title}>
                                                {parseConfiguratorTitle(windowInfo.title)}
                                            </span>
                                            <button
                                                onClick={() => testCapture(windowInfo.hwnd)}
                                                className="rounded bg-blue-600 px-2 py-0.5 text-xs text-white opacity-0 transition-opacity group-hover:opacity-100"
                                            >
                                                Тест
                                            </button>
                                        </div>
                                    ))
                                )}
                            </div>
                        </div>

                        {testCaptureResult && (
                            <div className="max-h-28 overflow-y-auto whitespace-pre-wrap rounded border border-zinc-700 bg-zinc-900 p-3 font-mono text-xs text-zinc-300">
                                {testCaptureResult}
                            </div>
                        )}
                    </div>
                </section>
            </div>
        </div>
    );
}

function StatusRow({
    label,
    status,
    checking
}: {
    label: string;
    status: BridgeStatus;
    checking: boolean;
}) {
    let icon: React.ReactNode;
    let text: string;
    let color: string;

    if (checking || status === 'checking') {
        icon = <RefreshCw className="h-4 w-4 animate-spin text-zinc-400" />;
        text = 'Проверяю...';
        color = 'text-zinc-400';
    } else if (status === 'ready') {
        icon = <CheckCircle className="h-4 w-4 text-green-500" />;
        text = 'Готов';
        color = 'text-green-400';
    } else if (status === 'missing') {
        icon = <XCircle className="h-4 w-4 text-red-500" />;
        text = 'Не найден';
        color = 'text-red-400';
    } else {
        icon = <AlertCircle className="h-4 w-4 text-zinc-500" />;
        text = '—';
        color = 'text-zinc-500';
    }

    return (
        <div className="flex items-center justify-between text-sm">
            <span className="text-zinc-300">{label}</span>
            <span className={`flex items-center gap-1.5 ${color}`}>
                {icon}
                {text}
            </span>
        </div>
    );
}

function ToggleRow({
    label,
    description,
    checked,
    onChange,
    disabled = false
}: {
    label: string;
    description?: string;
    checked: boolean;
    onChange: (value: boolean) => void;
    disabled?: boolean;
}) {
    return (
        <div className={`flex items-start justify-between gap-4 ${disabled ? 'opacity-50' : ''}`}>
            <div>
                <div className="text-sm text-zinc-200">{label}</div>
                {description && <div className="mt-0.5 text-xs text-zinc-500">{description}</div>}
            </div>

            <button
                type="button"
                role="switch"
                aria-checked={checked}
                disabled={disabled}
                onClick={() => !disabled && onChange(!checked)}
                className={`relative inline-flex h-6 w-11 flex-shrink-0 items-center rounded-full transition-all duration-200 ${
                    checked
                        ? 'bg-blue-600 shadow-[0_0_10px_rgba(37,99,235,0.4)]'
                        : 'bg-zinc-700'
                } ${disabled ? 'cursor-not-allowed' : 'cursor-pointer'}`}
            >
                <span
                    className={`inline-block h-4 w-4 transform rounded-full bg-white shadow-sm transition-transform duration-200 ${
                        checked ? 'translate-x-6' : 'translate-x-1'
                    }`}
                />
            </button>
        </div>
    );
}
