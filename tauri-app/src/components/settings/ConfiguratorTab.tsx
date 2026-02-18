import React from 'react';
import { Monitor, RefreshCw } from 'lucide-react';
import { WindowInfo, AppSettings } from '../../types/settings';
import { parseConfiguratorTitle } from '../../utils/configurator';

interface ConfiguratorTabProps {
    settings: AppSettings;
    setSettings: (settings: AppSettings) => void;
    detectedWindows: WindowInfo[];
    refreshWindows: () => void;
    testCapture: (hwnd: number) => void;
    testCaptureResult: string | null;
}

export function ConfiguratorTab({
    settings,
    setSettings,
    detectedWindows,
    refreshWindows,
    testCapture,
    testCaptureResult
}: ConfiguratorTabProps) {
    return (
        <div className="p-4 sm:p-8 w-full h-full overflow-y-auto">
            <div className="max-w-2xl mx-auto space-y-6 sm:space-y-8">
                <section>
                    <h3 className="text-lg font-medium mb-4 flex items-center gap-2">
                        <Monitor className="w-5 h-5 text-blue-500" />
                        Window Detection
                    </h3>
                    <div className="bg-zinc-800/50 border border-zinc-700 rounded-xl p-5 space-y-4">
                        <div>
                            <label className="text-xs text-zinc-500 uppercase font-semibold mb-1 block">Title Pattern</label>
                            <input
                                type="text"
                                value={settings.configurator.window_title_pattern}
                                onChange={(e) => setSettings({
                                    ...settings,
                                    configurator: { ...settings.configurator, window_title_pattern: e.target.value }
                                })}
                                placeholder="e.g. Configurator"
                                className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none text-zinc-100"
                            />
                        </div>

                        <div className="mt-4">
                            <div className="flex items-center justify-between mb-2">
                                <label className="text-xs text-zinc-500 uppercase font-semibold">Detected Windows</label>
                                <button onClick={refreshWindows} className="text-xs bg-zinc-700 hover:bg-zinc-600 px-2 py-1 rounded flex items-center gap-1 text-zinc-200 transition-colors">
                                    <RefreshCw className="w-3 h-3" /> Refresh
                                </button>
                            </div>
                            <div className="bg-zinc-900 border border-zinc-700 rounded-lg h-32 overflow-y-auto overflow-x-hidden">
                                {detectedWindows.length === 0 ? (
                                    <div className="p-4 text-center text-zinc-500 text-sm italic">No windows detected</div>
                                ) : (
                                    detectedWindows.map(w => (
                                        <div key={w.hwnd} className="p-2 border-b border-zinc-800 text-sm hover:bg-zinc-800 flex justify-between items-center group">
                                            <span className="truncate text-zinc-300" title={w.title}>{parseConfiguratorTitle(w.title)}</span>
                                            <button onClick={() => testCapture(w.hwnd)} className="opacity-0 group-hover:opacity-100 text-xs bg-blue-600 px-2 py-0.5 rounded text-white transition-opacity">Test</button>
                                        </div>
                                    ))
                                )}
                            </div>
                        </div>

                        {testCaptureResult && (
                            <div className="mt-2 p-3 bg-zinc-900 rounded border border-zinc-700 text-xs font-mono max-h-32 overflow-y-auto whitespace-pre-wrap text-zinc-300">
                                {testCaptureResult}
                            </div>
                        )}
                    </div>
                </section>
            </div>
        </div>
    );
}
