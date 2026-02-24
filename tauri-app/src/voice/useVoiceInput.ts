
import { useState, useCallback, useRef, useEffect } from 'react';
import { speechService } from './speechRecognition';

export function useVoiceInput(onText: (text: string) => void, selectedHwnd: number | null) {
    const [isRecording, setIsRecording] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [permissionState, setPermissionState] = useState<PermissionState | 'unknown'>('unknown');
    const lastFinalTextRef = useRef('');

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
        if (isFinal) {
            onText(text);
        }
    }, [onText]);

    const toggleRecording = useCallback(async () => {
        if (isRecording) {
            speechService.stop();
            setIsRecording(false);
        } else {
            // Check permission before starting
            await checkPermission();

            setError(null);
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
    }, [isRecording, processResult, checkPermission]);

    return {
        isRecording,
        error,
        permissionState,
        toggleRecording,
        isSupported: speechService.isSupported()
    };
}
