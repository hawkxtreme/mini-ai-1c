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
                        Advanced Debugging
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
                                <div className="font-medium text-zinc-200 text-sm">MCP Verbose Logging</div>
                                <div className="text-xs text-zinc-500">Log all SSE events and tool payloads to terminal. Can impact performance.</div>
                            </div>
                            <div className="relative inline-flex h-6 w-11 items-center rounded-full bg-zinc-700 cursor-pointer transition-colors"
                                onClick={() => setSettings({ ...settings, debug_mcp: !settings.debug_mcp })}
                            >
                                <span className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${settings.debug_mcp ? 'translate-x-6 bg-blue-500' : 'translate-x-1'}`} />
                            </div>
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
