import { useState, useCallback, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { MicActivityMonitor } from './micActivity';
import { speechService } from './speechRecognition';

const HOOK_PREFIX = '[VoiceInput:hook]';

function hookLog(level: 'info' | 'warn' | 'error', ...args: unknown[]) {
    const ts = new Date().toISOString().slice(11, 23);
    console[level](HOOK_PREFIX, `[${ts}]`, ...args);

    const parts = args.map((a) =>
        typeof a === 'object' ? JSON.stringify(a) : String(a),
    );
    const message = `${HOOK_PREFIX} [${ts}] ${parts.join(' ')}`;
    invoke('write_frontend_log', { message }).catch(() => {/* best effort */});
}

/**
 * Преобразует код ошибки Web Speech API в понятное пользователю сообщение на русском.
 * Отдельно выделяет ошибки, связанные с групповой политикой Windows.
 */
function mapSpeechError(code: string): string {
    switch (code) {
        case 'service-not-allowed':
            return 'Служба распознавания речи недоступна. Возможно, голосовые функции отключены групповой политикой Windows. Обратитесь к системному администратору или проверьте настройки конфиденциальности (Параметры → Конфиденциальность → Распознавание речи).';
        case 'not-allowed':
            return 'Доступ к микрофону запрещён. Проверьте разрешения браузера или групповую политику Windows.';
        case 'no-speech':
            return 'Речь не обнаружена. Проверьте микрофон и попробуйте снова.';
        case 'audio-capture':
            return 'Не удалось захватить аудио. Проверьте, что микрофон подключён и не занят другим приложением.';
        case 'network':
            return 'Ошибка сети при распознавании речи. Проверьте подключение к интернету.';
        case 'aborted':
            return 'Распознавание речи прервано.';
        case 'language-not-supported':
            return 'Язык "ru-RU" не поддерживается службой распознавания речи.';
        default:
            return `Ошибка голосового ввода: ${code}`;
    }
}

export function useVoiceInput(onText: (text: string) => void, _selectedHwnd: number | null) {
    const [isRecording, setIsRecording] = useState(false);
    const [permissionState, setPermissionState] = useState<PermissionState | 'unknown'>('unknown');
    const [error, setError] = useState<string | null>(null);
    const [rawErrorCode, setRawErrorCode] = useState<string | null>(null);
    const [micLevel, setMicLevel] = useState(0);
    const [hasMicSignal, setHasMicSignal] = useState(false);
    const [isMicMonitoringAvailable, setIsMicMonitoringAvailable] = useState(false);

    const pendingTranscriptRef = useRef('');
    const sessionIdRef = useRef(0);
    const isStoppingRef = useRef(false);
    const micMonitorRef = useRef<MicActivityMonitor | null>(null);

    const checkPermission = useCallback(async () => {
        try {
            if (navigator.permissions && navigator.permissions.query) {
                const result = await navigator.permissions.query({ name: 'microphone' as PermissionName });
                hookLog('info', 'Разрешение микрофона', { state: result.state });
                setPermissionState(result.state);
                result.onchange = () => {
                    hookLog('info', 'Разрешение микрофона изменилось', { state: result.state });
                    setPermissionState(result.state);
                };
            } else {
                hookLog('warn', 'Permissions API недоступен для microphone');
            }
        } catch (permissionError) {
            hookLog('warn', 'Ошибка запроса разрешения микрофона', { error: permissionError });
        }
    }, []);

    useEffect(() => {
        checkPermission();
    }, [checkPermission]);

    const stopMicMonitoring = useCallback(async () => {
        const monitor = micMonitorRef.current;
        micMonitorRef.current = null;

        setMicLevel(0);
        setHasMicSignal(false);
        setIsMicMonitoringAvailable(false);

        if (monitor) {
            await monitor.stop();
        }
    }, []);

    const startMicMonitoring = useCallback(async () => {
        if (!navigator.mediaDevices?.getUserMedia) {
            hookLog('warn', 'navigator.mediaDevices.getUserMedia недоступен — мониторинг микрофона отключён');
            setIsMicMonitoringAvailable(false);
            return;
        }

        await stopMicMonitoring();

        const monitor = new MicActivityMonitor();
        micMonitorRef.current = monitor;

        try {
            hookLog('info', 'Запуск мониторинга активности микрофона...');
            await monitor.start(({ level, hasSignal }) => {
                if (micMonitorRef.current !== monitor) {
                    return;
                }

                setMicLevel(prev => (Math.abs(prev - level) < 0.02 ? prev : level));
                setHasMicSignal(prev => (prev === hasSignal ? prev : hasSignal));
                setIsMicMonitoringAvailable(true);
            });
            hookLog('info', 'Мониторинг микрофона запущен успешно');
        } catch (monitorError) {
            if (micMonitorRef.current === monitor) {
                micMonitorRef.current = null;
            }

            setMicLevel(0);
            setHasMicSignal(false);
            setIsMicMonitoringAvailable(false);
            hookLog('warn', 'Мониторинг микрофона недоступен', { error: monitorError });
        }
    }, [stopMicMonitoring]);

    const resetTranscriptState = useCallback(() => {
        pendingTranscriptRef.current = '';
    }, []);

    const flushPendingTranscript = useCallback(() => {
        const text = pendingTranscriptRef.current.trim();

        if (!text) {
            pendingTranscriptRef.current = '';
            return;
        }

        onText(text);
        pendingTranscriptRef.current = '';
    }, [onText]);

    const processResult = useCallback((text: string, isFinal: boolean) => {
        if (isFinal) {
            pendingTranscriptRef.current = '';
            onText(text);
            return;
        }

        pendingTranscriptRef.current = text;
    }, [onText]);

    const finishRecording = useCallback(async () => {
        isStoppingRef.current = false;
        setIsRecording(false);
        flushPendingTranscript();
        await stopMicMonitoring();
    }, [flushPendingTranscript, stopMicMonitoring]);

    const toggleRecording = useCallback(async () => {
        if (isStoppingRef.current) {
            hookLog('info', 'toggleRecording — пропускаем, isStoppingRef=true');
            return;
        }

        if (isRecording) {
            hookLog('info', 'toggleRecording — остановка записи');
            isStoppingRef.current = true;
            setIsRecording(false);
            void stopMicMonitoring();
            if (!speechService.stop()) {
                hookLog('warn', 'speechService.stop() вернул false — вызываем finishRecording напрямую');
                void finishRecording();
            }
            return;
        }

        hookLog('info', 'toggleRecording — запуск записи', {
            sessionId: sessionIdRef.current + 1,
            isSupported: speechService.isSupported(),
            permissionState,
        });

        await checkPermission();
        setError(null);
        setRawErrorCode(null);
        resetTranscriptState();

        sessionIdRef.current += 1;
        const sessionId = sessionIdRef.current;

        const didStart = speechService.start(
            (result) => {
                if (sessionId !== sessionIdRef.current) {
                    return;
                }

                processResult(result.text, result.isFinal);
            },
            (rawError) => {
                if (sessionId !== sessionIdRef.current) {
                    return;
                }

                const rawCode =
                    typeof rawError === 'string'
                        ? rawError
                        : rawError instanceof Error
                            ? rawError.message
                            : rawError?.message || 'voice-error';

                const userMessage = mapSpeechError(rawCode);
                hookLog('error', 'Ошибка голосового ввода', { rawError, rawCode, userMessage, sessionId });
                setError(userMessage);
                setRawErrorCode(rawCode);
                void finishRecording();
                void checkPermission();
            },
            () => {
                if (sessionId !== sessionIdRef.current) {
                    return;
                }

                hookLog('info', 'Сессия завершена (onEnd callback)', { sessionId });
                void finishRecording();
            },
        );

        hookLog('info', 'speechService.start() вернул', { didStart, sessionId });

        if (!didStart) {
            hookLog('error', 'Запуск голосового ввода не удался (didStart=false)', { sessionId });
            return;
        }

        setIsRecording(true);
        void startMicMonitoring();
    }, [
        checkPermission,
        finishRecording,
        isRecording,
        permissionState,
        processResult,
        resetTranscriptState,
        startMicMonitoring,
        stopMicMonitoring,
    ]);

    useEffect(() => {
        return () => {
            sessionIdRef.current += 1;
            isStoppingRef.current = false;
            void stopMicMonitoring();
            speechService.stop();
        };
    }, [stopMicMonitoring]);
    return {
        isRecording,
        error,
        rawErrorCode,
        permissionState,
        toggleRecording,
        isSupported: speechService.isSupported(),
        micLevel,
        hasMicSignal,
        isMicMonitoringAvailable,
    };
}
