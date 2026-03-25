import { useState, useCallback, useRef, useEffect } from 'react';
import { speechService } from './speechRecognition';

export function useVoiceInput(onText: (text: string) => void, selectedHwnd: number | null) {
    const [isRecording, setIsRecording] = useState(false);
    const [permissionState, setPermissionState] = useState<PermissionState | 'unknown'>('unknown');
    const [error, setError] = useState<string | null>(null);
    const lastTranscriptRef = useRef('');

    const checkPermission = useCallback(async () => {
        try {
            if (navigator.permissions && navigator.permissions.query) {
                const result = await navigator.permissions.query({ name: 'microphone' as PermissionName });
                setPermissionState(result.state);
                result.onchange = () => setPermissionState(result.state);
            }
        } catch (e) {
            console.warn('Permissions API not supported for microphone');
        }
    }, []);

    useEffect(() => {
        checkPermission();
    }, [checkPermission]);

    const processResult = useCallback((text: string, isFinal: boolean) => {
        lastTranscriptRef.current = text;
        if (isFinal) {
            onText(text);
            lastTranscriptRef.current = '';
        }
    }, [onText]);

    const toggleRecording = useCallback(async () => {
        if (isRecording) {
            speechService.stop();
            setIsRecording(false);

            if (lastTranscriptRef.current) {
                onText(lastTranscriptRef.current);
                lastTranscriptRef.current = '';
            }
        } else {
            await checkPermission();

            setError(null);
            lastTranscriptRef.current = '';
            speechService.start(
                (result) => {
                    processResult(result.text, result.isFinal);
                },
                (err) => {
                    setError(err);
                    setIsRecording(false);
                    checkPermission();
                }
            );
            setIsRecording(true);
        }
    }, [isRecording, processResult, checkPermission, onText]);

    return {
        isRecording,
        error,
        permissionState,
        toggleRecording,
        isSupported: speechService.isSupported()
    };
}
