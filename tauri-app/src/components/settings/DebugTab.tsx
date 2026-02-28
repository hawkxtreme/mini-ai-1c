import React from 'react';
import { Bug, Save } from 'lucide-react';
import { AppSettings } from '../../types/settings';

interface DebugTabProps {
    settings: AppSettings;
    setSettings: (settings: AppSettings) => void;
    showResetConfirm: boolean;
    setShowResetConfirm: (show: boolean) => void;
    resetOnboarding: () => void;
    saveDebugLogs: () => void;
}

export function DebugTab({
    settings,
    setSettings,
    showResetConfirm,
    setShowResetConfirm,
    resetOnboarding,
    saveDebugLogs
}: DebugTabProps) {
    return (
        <div className="p-4 sm:p-8 w-full h-full overflow-y-auto">
            <div className="max-w-2xl mx-auto space-y-6 sm:space-y-8">
                <section>
                    <h3 className="text-lg font-medium mb-4 flex items-center gap-2 text-zinc-100">
                        <Bug className="w-5 h-5 text-red-500" />
                        Advanced Debugging & Logs
                    </h3>
                    <div className="bg-zinc-800/50 border border-zinc-700 rounded-xl p-5 space-y-4">
                        <div className="flex items-center justify-between">
                            <div>
                                <div className="font-medium text-zinc-200 text-sm">Reset Onboarding</div>
                                <div className="text-xs text-zinc-500">Reset the "first run" flag to show the wizard again on next restart.</div>
                            </div>
                            <div className="flex gap-2">
                                {!showResetConfirm ? (
                                    <button
                                        onClick={() => setShowResetConfirm(true)}
                                        className="px-3 py-1 bg-red-900/40 text-red-300 border border-red-800/50 rounded-lg text-xs hover:bg-red-800/60 transition-colors"
                                    >
                                        Reset Onboarding
                                    </button>
                                ) : (
                                    <div className="flex items-center gap-2 bg-red-950/40 border border-red-900/50 rounded-lg p-1 animate-in fade-in zoom-in-95 duration-200">
                                        <span className="text-[10px] uppercase font-bold text-red-400 px-2">Are you sure?</span>
                                        <button
                                            onClick={resetOnboarding}
                                            className="px-3 py-1 bg-red-600 text-white rounded-md text-xs font-bold hover:bg-red-500 transition-colors"
                                        >
                                            YES, RESET
                                        </button>
                                        <button
                                            onClick={() => setShowResetConfirm(false)}
                                            className="px-3 py-1 bg-zinc-800 text-zinc-300 rounded-md text-xs hover:bg-zinc-700 transition-colors"
                                        >
                                            NO
                                        </button>
                                    </div>
                                )}
                                <button
                                    onClick={() => window.location.reload()}
                                    className="px-3 py-1 bg-zinc-800 border border-zinc-700 text-zinc-300 rounded-lg text-xs hover:bg-zinc-700 transition-colors"
                                >
                                    Reload App
                                </button>
                            </div>
                        </div>

                        <div className="flex items-center justify-between pt-4 border-t border-zinc-700">
                            <div>
                                <div className="font-medium text-zinc-200 text-sm">Debug Mode</div>
                                <div className="text-xs text-zinc-500">Подробное журналирование работы приложения и MCP-серверов в терминал.</div>
                            </div>
                            <div className={`relative inline-flex h-6 w-11 items-center rounded-full cursor-pointer transition-all duration-200 ${settings.debug_mode ? 'bg-blue-600 shadow-[0_0_10px_rgba(37,99,235,0.4)]' : 'bg-zinc-700'}`}
                                onClick={() => setSettings({ ...settings, debug_mode: !settings.debug_mode })}
                            >
                                <span className={`inline-block h-4 w-4 transform rounded-full bg-white shadow-sm transition-transform duration-200 ${settings.debug_mode ? 'translate-x-6' : 'translate-x-1'}`} />
                            </div>
                        </div>

                        <div className="flex flex-col pt-4 border-t border-zinc-700 space-y-4">
                            <div className="flex items-center justify-between">
                                <div>
                                    <div className="font-medium text-zinc-200 text-sm">Лимит шагов (MCP Iterations)</div>
                                    <div className="text-xs text-zinc-500">Ограничение количества вызовов инструментов ИИ в рамках одного запроса.</div>
                                </div>
                                <div className={`relative inline-flex h-6 w-11 items-center rounded-full cursor-pointer transition-all duration-200 ${settings.max_agent_iterations != null ? 'bg-blue-600 shadow-[0_0_10px_rgba(37,99,235,0.4)]' : 'bg-zinc-700'}`}
                                    onClick={() => setSettings({ ...settings, max_agent_iterations: settings.max_agent_iterations != null ? null : 7 })}
                                >
                                    <span className={`inline-block h-4 w-4 transform rounded-full bg-white shadow-sm transition-transform duration-200 ${settings.max_agent_iterations != null ? 'translate-x-6' : 'translate-x-1'}`} />
                                </div>
                            </div>

                            {settings.max_agent_iterations != null && (
                                <div className="flex items-center gap-4 bg-zinc-900/50 p-3 rounded-lg border border-zinc-800">
                                    <input
                                        type="range"
                                        min="1"
                                        max="25"
                                        value={settings.max_agent_iterations}
                                        onChange={(e) => setSettings({ ...settings, max_agent_iterations: parseInt(e.target.value, 10) })}
                                        className="flex-1 accent-blue-500"
                                    />
                                    <span className="text-zinc-300 font-mono text-sm w-8 text-right bg-zinc-800 px-2 py-1 rounded">{settings.max_agent_iterations}</span>
                                </div>
                            )}
                        </div>

                        <div className="flex items-center justify-between pt-4 border-t border-zinc-700">
                            <div>
                                <div className="font-medium text-zinc-200 text-sm">System Logs</div>
                                <div className="text-xs text-zinc-500">Экспорт всех логов приложения и серверов (BSL LS, MCP) в текстовый файл.</div>
                            </div>
                            <button
                                onClick={saveDebugLogs}
                                className="flex items-center gap-2 px-3 py-1 bg-zinc-700 text-zinc-200 border border-zinc-600 rounded-lg text-xs hover:bg-zinc-600 transition-colors"
                            >
                                <Save className="w-4 h-4" />
                                Save Logs
                            </button>
                        </div>
                    </div>
                </section>
            </div>
        </div>
    );
}
