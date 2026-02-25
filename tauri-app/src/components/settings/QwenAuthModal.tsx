import { useState, useEffect, useRef } from 'react';
import { X, ExternalLink, Loader2 } from 'lucide-react';
import { cliProvidersApi } from '../../api/cli_providers';
import { CliAuthInitResponse, CliAuthStatus } from '../../types/settings';

interface QwenAuthModalProps {
    isOpen: boolean;
    onClose: () => void;
    onSuccess: (accessToken: string, refreshToken: string | null, expiresAt: number, resourceUrl: string | null) => void;
}

export function QwenAuthModal({ isOpen, onClose, onSuccess }: QwenAuthModalProps) {
    const [step, setStep] = useState<'init' | 'waiting' | 'error'>('init');
    const [authData, setAuthData] = useState<CliAuthInitResponse | null>(null);
    const [timeLeft, setTimeLeft] = useState(0);
    const [error, setError] = useState<string | null>(null);
    const pollIntervalRef = useRef<any>(null);

    // Инициализация Device Flow
    const startAuth = async () => {
        setStep('init');
        setError(null);
        try {
            const data = await cliProvidersApi.authStart('qwen');
            setAuthData(data);
            setTimeLeft(data.expires_in);
            setStep('waiting');

            // Запуск опроса статуса
            startPolling(data.device_code, data.poll_interval, data.code_verifier);
        } catch (err: any) {
            setError(err.toString());
            setStep('error');
        }
    };

    const startPolling = (deviceCode: string, initialIntervalSec: number, codeVerifier?: string) => {
        stopPolling();

        let currentInterval = initialIntervalSec * 1000;
        let isPolling = true;

        const poll = async () => {
            if (!isPolling) return;
            try {
                const status = await cliProvidersApi.authPoll('qwen', deviceCode, codeVerifier);
                console.log('[DEBUG] Qwen Auth Poll Status:', status);

                if (status.status === 'Authorized' && status.data) {
                    isPolling = false;
                    const { access_token, refresh_token, expires_at, resource_url } = status.data as any;
                    await onSuccess(access_token, refresh_token, expires_at, resource_url ?? null);
                    onClose();
                    return;
                } else if (status.status === 'Expired') {
                    isPolling = false;
                    setError('Срок действия кода истек. Попробуйте еще раз.');
                    setStep('error');
                    return;
                } else if (status.status === 'SlowDown') {
                    currentInterval += 5000;
                    console.log(`[DEBUG] Slow down received. Increasing interval to ${currentInterval}ms`);
                } else if (status.status === 'Error') {
                    isPolling = false;
                    setError((status as any).data || 'Неизвестная ошибка при авторизации');
                    setStep('error');
                    return;
                }

                // Schedule next poll
                pollIntervalRef.current = setTimeout(poll, currentInterval);
            } catch (err) {
                console.error('Polling error:', err);
                pollIntervalRef.current = setTimeout(poll, currentInterval);
            }
        };

        // Start immediately
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

    // Таймер
    useEffect(() => {
        if (timeLeft > 0 && step === 'waiting') {
            const timer = setTimeout(() => setTimeLeft(prev => prev - 1), 1000);
            return () => clearTimeout(timer);
        } else if (timeLeft === 0 && step === 'waiting') {
            setStep('error');
            setError('Время ожидания истекло.');
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
                    <h3 className="text-lg font-semibold text-zinc-100">Вход в Qwen Code</h3>
                    <button onClick={onClose} className="text-zinc-400 hover:text-zinc-200 p-1">
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* Content */}
                <div className="p-8 flex flex-col gap-6">
                    {step === 'init' && (
                        <div className="flex flex-col items-center justify-center py-8 gap-4">
                            <Loader2 className="w-10 h-10 text-blue-500 animate-spin" />
                            <p className="text-zinc-400">Инициализация авторизации...</p>
                        </div>
                    )}

                    {step === 'waiting' && authData && (
                        <>
                            <p className="text-zinc-400 text-sm leading-relaxed">
                                Откройте страницу авторизации и войдите в аккаунт Qwen:
                            </p>

                            <a
                                href={authData.verification_url}
                                target="_blank"
                                rel="noopener noreferrer"
                                className="flex items-center justify-between p-3 bg-[#27272a] hover:bg-[#323236] rounded-lg text-blue-400 transition-colors group"
                            >
                                <span className="truncate text-sm font-medium">{authData.verification_url}</span>
                                <ExternalLink className="w-4 h-4 shrink-0 group-hover:translate-x-0.5 group-hover:-translate-y-0.5 transition-transform" />
                            </a>

                            <div className="flex flex-col items-center gap-3 pt-2 border-t border-[#27272a]">
                                <div className="flex items-center gap-2.5">
                                    <Loader2 className="w-4 h-4 text-blue-500 animate-spin" />
                                    <span className="text-zinc-400 text-sm">Ожидание подтверждения...</span>
                                </div>
                                <div className="text-[11px] text-zinc-500 font-medium">
                                    Осталось времени: <span className="text-zinc-300">{formatTime(timeLeft)}</span>
                                </div>
                                <div className="w-full h-1 bg-zinc-800 rounded-full overflow-hidden mt-1">
                                    <div
                                        className="h-full bg-blue-500 transition-all duration-1000"
                                        style={{ width: `${(timeLeft / authData.expires_in) * 100}%` }}
                                    />
                                </div>
                            </div>
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
                                    {error || 'Не удалось запустить процесс авторизации.'}
                                </p>
                            </div>
                            <button
                                onClick={startAuth}
                                className="w-full py-2.5 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition-colors font-medium active:scale-[0.98]"
                            >
                                Попробовать снова
                            </button>
                        </div>
                    )}
                </div>

                {/* Footer */}
                <div className="px-6 py-4 bg-[#09090b]/50 border-t border-[#27272a] flex justify-end">
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
