import { Bug, FlaskConical, Save } from 'lucide-react';
import { AppSettings } from '../../types/settings';
import { setConfiguratorRdpMode } from '../../api/configurator';

interface DebugTabProps {
    settings: AppSettings;
    setSettings: (settings: AppSettings) => void;
    showResetConfirm: boolean;
    setShowResetConfirm: (show: boolean) => void;
    resetOnboarding: () => void;
    saveDebugLogs: () => void;
    currentProvider?: string;
}

export function DebugTab({
    settings,
    setSettings,
    showResetConfirm,
    setShowResetConfirm,
    resetOnboarding,
    saveDebugLogs,
    currentProvider
}: DebugTabProps) {
    const bridgeEnabled = settings.configurator?.editor_bridge_enabled ?? false;
    const rdpMode = settings.configurator?.rdp_mode ?? false;

    const updateConfigurator = (patch: Partial<AppSettings['configurator']>) => {
        setSettings({
            ...settings,
            configurator: {
                ...settings.configurator,
                ...patch
            }
        });
    };

    const handleRdpModeToggle = () => {
        const newValue = !rdpMode;
        updateConfigurator({ rdp_mode: newValue });
        setConfiguratorRdpMode(newValue).catch(() => {});
    };

    return (
        <div className="h-full w-full overflow-y-auto p-4 sm:p-8">
            <div className="mx-auto max-w-2xl space-y-6 sm:space-y-8">
                <section>
                    <h3 className="mb-4 flex items-center gap-2 text-lg font-medium text-zinc-100">
                        <FlaskConical className="h-5 w-5 text-blue-400" />
                        Экспериментальные функции
                    </h3>

                    <p className="mb-4 text-sm text-zinc-400">
                        Здесь включаются функции, которые ещё доводятся на живом Конфигураторе.
                    </p>

                    <div className="space-y-4 rounded-xl border border-zinc-700 bg-zinc-800/50 p-5">
                        <ToggleRow
                            label="Включить быстрые действия в 1С Конфигураторе"
                            description="Ctrl + ПКМ в редакторе показывает экспериментальное меню mini-ai."
                            checked={bridgeEnabled}
                            onChange={(value) => updateConfigurator({ editor_bridge_enabled: value })}
                        />

                        <div className="border-t border-zinc-700" />

                        <ToggleRow
                            label="Режим RDP"
                            description="Увеличенные задержки и совместимость для удалённых подключений."
                            checked={rdpMode}
                            onChange={handleRdpModeToggle}
                        />
                    </div>
                </section>

                <section>
                    <h3 className="mb-4 flex items-center gap-2 text-lg font-medium text-zinc-100">
                        <Bug className="h-5 w-5 text-red-500" />
                        Отладка и обслуживание
                    </h3>

                    <div className="space-y-4 rounded-xl border border-zinc-700 bg-zinc-800/50 p-5">
                        <div className="flex items-center justify-between gap-4">
                            <div>
                                <div className="text-sm font-medium text-zinc-200">Сбросить onboarding</div>
                                <div className="text-xs text-zinc-500">
                                    Сбросить флаг первого запуска и снова показать мастер при следующем старте.
                                </div>
                            </div>

                            <div className="flex gap-2">
                                {!showResetConfirm ? (
                                    <button
                                        onClick={() => setShowResetConfirm(true)}
                                        className="rounded-lg border border-red-800/50 bg-red-900/40 px-3 py-1 text-xs text-red-300 transition-colors hover:bg-red-800/60"
                                    >
                                        Сбросить onboarding
                                    </button>
                                ) : (
                                    <div className="flex items-center gap-2 rounded-lg border border-red-900/50 bg-red-950/40 p-1">
                                        <span className="px-2 text-[10px] font-bold uppercase text-red-400">Вы уверены?</span>
                                        <button
                                            onClick={resetOnboarding}
                                            className="rounded-md bg-red-600 px-3 py-1 text-xs font-bold text-white transition-colors hover:bg-red-500"
                                        >
                                            Да, сбросить
                                        </button>
                                        <button
                                            onClick={() => setShowResetConfirm(false)}
                                            className="rounded-md bg-zinc-800 px-3 py-1 text-xs text-zinc-300 transition-colors hover:bg-zinc-700"
                                        >
                                            Нет
                                        </button>
                                    </div>
                                )}

                                <button
                                    onClick={() => window.location.reload()}
                                    className="rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-1 text-xs text-zinc-300 transition-colors hover:bg-zinc-700"
                                >
                                    Перезагрузить приложение
                                </button>
                            </div>
                        </div>

                        {currentProvider !== 'QwenCli' && (
                            <>
                                <div className="border-t border-zinc-700" />

                                <div className="flex flex-col space-y-4">
                                    <div className="flex items-center justify-between gap-4">
                                        <div>
                                            <div className="text-sm font-medium text-zinc-200">Лимит шагов агента</div>
                                            <div className="text-xs text-zinc-500">
                                                Ограничение количества вызовов инструментов ИИ в рамках одного запроса.
                                            </div>
                                        </div>

                                        <ToggleRowButton
                                            checked={settings.max_agent_iterations != null}
                                            onClick={() => setSettings({
                                                ...settings,
                                                max_agent_iterations: settings.max_agent_iterations != null ? null : 7
                                            })}
                                        />
                                    </div>

                                    {settings.max_agent_iterations != null && (
                                        <div className="flex items-center gap-4 rounded-lg border border-zinc-800 bg-zinc-900/50 p-3">
                                            <input
                                                type="range"
                                                min="1"
                                                max="25"
                                                value={settings.max_agent_iterations}
                                                onChange={(e) => setSettings({
                                                    ...settings,
                                                    max_agent_iterations: parseInt(e.target.value, 10)
                                                })}
                                                className="flex-1 accent-blue-500"
                                            />
                                            <span className="w-8 rounded bg-zinc-800 px-2 py-1 text-right font-mono text-sm text-zinc-300">
                                                {settings.max_agent_iterations}
                                            </span>
                                        </div>
                                    )}
                                </div>
                            </>
                        )}

                        <div className="border-t border-zinc-700" />

                        <div className="flex items-center justify-between gap-4">
                            <div>
                                <div className="text-sm font-medium text-zinc-200">Режим отладки</div>
                                <div className="text-xs text-zinc-500">
                                    Подробное журналирование работы приложения и MCP-серверов в терминал.
                                </div>
                            </div>

                            <ToggleRowButton
                                checked={settings.debug_mode}
                                onClick={() => setSettings({ ...settings, debug_mode: !settings.debug_mode })}
                            />
                        </div>

                        <div className="border-t border-zinc-700" />

                        <div className="flex items-center justify-between gap-4">
                            <div>
                                <div className="text-sm font-medium text-zinc-200">Системные логи</div>
                                <div className="text-xs text-zinc-500">
                                    Экспорт логов приложения, MCP-серверов и frontend-метрик производительности.
                                </div>
                            </div>

                            <button
                                onClick={saveDebugLogs}
                                className="flex items-center gap-2 rounded-lg border border-zinc-600 bg-zinc-700 px-3 py-1 text-xs text-zinc-200 transition-colors hover:bg-zinc-600"
                            >
                                <Save className="h-4 w-4" />
                                Сохранить логи
                            </button>
                        </div>
                    </div>
                </section>
            </div>
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

            <ToggleRowButton
                checked={checked}
                disabled={disabled}
                onClick={() => !disabled && onChange(!checked)}
            />
        </div>
    );
}

function ToggleRowButton({
    checked,
    onClick,
    disabled = false
}: {
    checked: boolean;
    onClick: () => void;
    disabled?: boolean;
}) {
    return (
        <button
            type="button"
            role="switch"
            aria-checked={checked}
            disabled={disabled}
            onClick={onClick}
            className={`relative inline-flex h-6 w-11 items-center rounded-full transition-all duration-200 ${
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
    );
}
