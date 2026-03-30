import { useState, useCallback, useRef, useEffect } from 'react';
import { MicActivityMonitor } from './micActivity';
import { speechService } from './speechRecognition';

export function useVoiceInput(onText: (text: string) => void, _selectedHwnd: number | null) {
    const [isRecording, setIsRecording] = useState(false);
    const [permissionState, setPermissionState] = useState<PermissionState | 'unknown'>('unknown');
    const [error, setError] = useState<string | null>(null);
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
                setPermissionState(result.state);
                result.onchange = () => setPermissionState(result.state);
            }
        } catch (permissionError) {
            console.warn('Permissions API not supported for microphone', permissionError);
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
            setIsMicMonitoringAvailable(false);
            return;
        }

        await stopMicMonitoring();

        const monitor = new MicActivityMonitor();
        micMonitorRef.current = monitor;

        try {
            await monitor.start(({ level, hasSignal }) => {
                if (micMonitorRef.current !== monitor) {
                    return;
                }

                setMicLevel(prev => (Math.abs(prev - level) < 0.02 ? prev : level));
                setHasMicSignal(prev => (prev === hasSignal ? prev : hasSignal));
                setIsMicMonitoringAvailable(true);
            });
        } catch (monitorError) {
            if (micMonitorRef.current === monitor) {
                micMonitorRef.current = null;
            }

            setMicLevel(0);
            setHasMicSignal(false);
            setIsMicMonitoringAvailable(false);
            console.warn('Microphone activity monitor is unavailable:', monitorError);
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
            return;
        }

        if (isRecording) {
            isStoppingRef.current = true;
            setIsRecording(false);
            void stopMicMonitoring();
            if (!speechService.stop()) {
                void finishRecording();
            }
            return;
        }

        await checkPermission();
        setError(null);
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

                const normalizedError =
                    typeof rawError === 'string'
                        ? rawError
                        : rawError instanceof Error
                            ? rawError.message
                            : rawError?.message || 'voice-error';

                setError(normalizedError);
                void finishRecording();
                void checkPermission();
            },
            () => {
                if (sessionId !== sessionIdRef.current) {
                    return;
                }

                void finishRecording();
            },
        );

        if (!didStart) {
            return;
        }

        setIsRecording(true);
        void startMicMonitoring();
    }, [
        checkPermission,
        finishRecording,
        isRecording,
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
        permissionState,
        toggleRecording,
        isSupported: speechService.isSupported(),
        micLevel,
        hasMicSignal,
        isMicMonitoringAvailable,
    };
}
