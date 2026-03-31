import { useState, useEffect, useRef } from 'react';
import { X, ExternalLink, Loader2, Globe } from 'lucide-react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { cliProvidersApi } from '../../api/cli_providers';
import { CliAuthInitResponse } from '../../types/settings';

interface CodexAuthModalProps {
    isOpen: boolean;
    onClose: () => void;
    onSuccess: (accessToken: string, refreshToken: string | null, expiresAt: number) => void;
}

type Step = 'init' | 'waiting' | 'error';

export function CodexAuthModal({ isOpen, onClose, onSuccess }: CodexAuthModalProps) {
    const [step, setStep] = useState<Step>('init');
    const [authData, setAuthData] = useState<CliAuthInitResponse | null>(null);
    const [timeLeft, setTimeLeft] = useState(0);
    const [error, setError] = useState<string | null>(null);
    const pollIntervalRef = useRef<any>(null);

    const startAuth = async () => {
        setStep('init');
        setError(null);
        stopPolling();
        try {
            const data = await cliProvidersApi.authStart('codex');
            setAuthData(data);
            setTimeLeft(data.expires_in);
            setStep('waiting');
            // Auto-open browser and start polling immediately
            try { await openUrl(data.verification_url); } catch { /* ignore */ }
            startPolling(data.device_code, data.poll_interval, data.code_verifier);
        } catch (err: any) {
            setError(err.toString());
            setStep('error');
        }
    };

    const openBrowser = async () => {
        if (!authData) return;
        try { await openUrl(authData.verification_url); } catch { /* ignore */ }
    };

    const startPolling = (deviceCode: string, initialIntervalSec: number, codeVerifier?: string) => {
        stopPolling();
        let currentInterval = initialIntervalSec * 1000;
        let isPolling = true;

        const poll = async () => {
            if (!isPolling) return;
            try {
                const status = await cliProvidersApi.authPoll('codex', deviceCode, codeVerifier);

                if (status.status === 'Authorized' && status.data) {
                    isPolling = false;
                    const { access_token, refresh_token, expires_at } = status.data as any;
                    await onSuccess(access_token, refresh_token ?? null, expires_at);
                    onClose();
                    return;
                } else if (status.status === 'Error') {
                    isPolling = false;
                    setError((status as any).data || 'Ошибка авторизации');
                    setStep('error');
                    return;
                }
                // Pending — continue polling
                pollIntervalRef.current = setTimeout(poll, currentInterval);
            } catch (err) {
                console.error('[Codex] Polling error:', err);
                pollIntervalRef.current = setTimeout(poll, currentInterval);
            }
        };

        poll();
    };

    const stopPolling = () => {
        if (pollIntervalRef.current) {
            clearTimeout(pollIntervalRef.current);
            pollIntervalRef.current = null;
        }
    };

    useEffect(() => {
        if (isOpen) {
            startAuth();
        } else {
            stopPolling();
        }
        return () => stopPolling();
    }, [isOpen]);

    // Countdown timer
    useEffect(() => {
        if (timeLeft > 0 && step === 'waiting') {
            const timer = setTimeout(() => setTimeLeft(prev => prev - 1), 1000);
            return () => clearTimeout(timer);
        } else if (timeLeft === 0 && step === 'waiting') {
            stopPolling();
            setStep('error');
            setError('Время ожидания истекло. Попробуйте снова.');
        }
    }, [timeLeft, step]);

    const formatTime = (seconds: number) => {
        const m = Math.floor(seconds / 60);
        const s = seconds % 60;
        return `${m}:${s.toString().padStart(2, '0')}`;
    };

    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 bg-black/70 flex items-center justify-center z-[100] p-4 animate-in fade-in duration-200">
            <div className="bg-[#18181b] border border-[#27272a] rounded-xl w-full max-w-md overflow-hidden shadow-2xl flex flex-col">
                {/* Header */}
                <div className="flex items-center justify-between px-6 py-4 border-b border-[#27272a]">
                    <div className="flex items-center gap-2">
                        <div className="w-6 h-6 rounded bg-emerald-600 flex items-center justify-center">
                            <Globe className="w-3.5 h-3.5 text-white" />
                        </div>
                        <h3 className="text-lg font-semibold text-zinc-100">Вход в OpenAI Codex</h3>
                    </div>
                    <button onClick={onClose} className="text-zinc-400 hover:text-zinc-200 p-1">
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* Content */}
                <div className="p-8 flex flex-col gap-6">
                    {step === 'init' && (
                        <div className="flex flex-col items-center justify-center py-8 gap-4">
                            <Loader2 className="w-10 h-10 text-emerald-500 animate-spin" />
                            <p className="text-zinc-400">Подготовка авторизации...</p>
                        </div>
                    )}

                    {step === 'waiting' && authData && (
                        <>
                            <p className="text-zinc-400 text-sm leading-relaxed">
                                Браузер открыт. Войдите в аккаунт OpenAI и подтвердите доступ.
                                Приложение получит токен автоматически.
                            </p>

                            <div className="flex flex-col items-center gap-3 pt-2 border-t border-[#27272a]">
                                <div className="flex items-center gap-2.5">
                                    <Loader2 className="w-4 h-4 text-emerald-500 animate-spin" />
                                    <span className="text-zinc-400 text-sm">Ожидание авторизации в браузере...</span>
                                </div>
                                <div className="text-[11px] text-zinc-500 font-medium">
                                    Осталось: <span className="text-zinc-300">{formatTime(timeLeft)}</span>
                                </div>
                                <div className="w-full h-1 bg-zinc-800 rounded-full overflow-hidden mt-1">
                                    <div
                                        className="h-full bg-emerald-500 transition-all duration-1000"
                                        style={{ width: `${(timeLeft / authData.expires_in) * 100}%` }}
                                    />
                                </div>
                            </div>

                            <button
                                onClick={openBrowser}
                                className="w-full h-9 flex items-center justify-center gap-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg border border-zinc-700 text-xs font-medium transition-all"
                            >
                                <ExternalLink className="w-3.5 h-3.5" /> Открыть браузер снова
                            </button>
                        </>
                    )}

                    {step === 'error' && (
                        <div className="flex flex-col items-center text-center gap-6 py-4">
                            <div className="w-16 h-16 bg-red-500/10 rounded-full flex items-center justify-center">
                                <X className="w-8 h-8 text-red-500" />
                            </div>
                            <div className="space-y-2">
                                <h4 className="text-zinc-100 font-medium">Ошибка авторизации</h4>
                                <p className="text-zinc-400 text-sm max-w-[280px]">
                                    {error || 'Не удалось выполнить авторизацию.'}
                                </p>
                            </div>
                            <button
                                onClick={startAuth}
                                className="w-full py-2.5 bg-emerald-600 hover:bg-emerald-700 text-white rounded-lg transition-colors font-medium active:scale-[0.98]"
                            >
                                Попробовать снова
                            </button>
                        </div>
                    )}
                </div>

                {/* Footer */}
                <div className="px-6 py-4 bg-[#09090b]/50 border-t border-[#27272a] flex justify-between items-center">
                    <p className="text-[10px] text-zinc-600">
                        OAuth2 + PKCE · Токен хранится в Keychain
                    </p>
                    <button
                        onClick={onClose}
                        className="px-4 py-2 text-zinc-400 hover:text-zinc-200 text-sm font-medium transition-colors"
                    >
                        Отмена
                    </button>
                </div>
            </div>
        </div>
    );
}
