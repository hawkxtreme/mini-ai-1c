import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { Download, RefreshCw, Upload } from 'lucide-react';

import {
    exportSettings,
    importSettingsFromFile,
    validateImportSettingsFile,
} from '../../api/settings';
import { AppSettings } from '../../types/settings';

interface GeneralTabProps {
    settings: AppSettings;
    setSettings: (settings: AppSettings) => void;
    onConfigurationImported: () => Promise<void>;
}

type StatusTone = 'success' | 'error';

export function GeneralTab({
    settings,
    setSettings,
    onConfigurationImported,
}: GeneralTabProps) {
    const [transferStatus, setTransferStatus] = useState<string>('');
    const [statusTone, setStatusTone] = useState<StatusTone>('success');
    const [exporting, setExporting] = useState(false);
    const [importing, setImporting] = useState(false);

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
                                    const active = (settings.context_compress_strategy || 'disabled') === opt;

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

                        {(settings.context_compress_strategy === 'sliding_window' ||
                            settings.context_compress_strategy === 'summarize') && (
                            <div className="flex items-center justify-between gap-4">
                                <div>
                                    <div className="text-sm font-medium text-zinc-200">Порог сжатия</div>
                                    <div className="text-xs text-zinc-500">
                                        Сжимать когда история превышает N токенов (~символов÷4)
                                    </div>
                                </div>
                                <input
                                    type="number"
                                    min={1000}
                                    max={50000}
                                    step={1000}
                                    value={settings.max_context_tokens ?? 8000}
                                    onChange={(e) =>
                                        setSettings({
                                            ...settings,
                                            max_context_tokens: parseInt(e.target.value, 10) || 8000,
                                        })
                                    }
                                    className="w-24 rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-right text-xs text-zinc-200 focus:border-zinc-500 focus:outline-none"
                                />
                            </div>
                        )}

                        {settings.context_compress_strategy === 'summarize' && (
                            <p className="text-[11px] text-zinc-600">
                                ⚠ Суммаризация недоступна для CodexCLI, QwenCLI и 1С:Напарника — автоматически
                                используется скользящее окно.
                            </p>
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
            </div>
        </div>
    );
}
