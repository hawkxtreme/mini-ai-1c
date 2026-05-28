import { useCallback, useEffect, useRef, useState } from 'react';
import { Mic } from 'lucide-react';
import { useVoiceInput } from '../../voice/useVoiceInput';

type VoiceInputControlVariant = 'chat' | 'overlay';

interface VoiceInputControlProps {
    onText: (text: string) => void;
    selectedHwnd?: number | null;
    disabled?: boolean;
    variant?: VoiceInputControlVariant;
}

const MIC_BAR_MULTIPLIERS = [0.55, 0.85, 1.1] as const;

function buildHintCopy() {
    return 'Нажмите "Разрешить" в появившемся окне браузера в верхнем левом углу для доступа к микрофону.';
}

export function VoiceInputControl({
    onText,
    selectedHwnd = null,
    disabled = false,
    variant = 'chat',
}: VoiceInputControlProps) {
    const [showVoiceHint, setShowVoiceHint] = useState(false);
    const hintTimerRef = useRef<number | null>(null);
    const {
        isRecording,
        toggleRecording,
        isSupported,
        error: voiceError,
        rawErrorCode: voiceErrorCode,
        permissionState,
        micLevel,
        hasMicSignal,
        isMicMonitoringAvailable,
    } = useVoiceInput(onText, selectedHwnd);

    const clearHintTimer = useCallback(() => {
        if (hintTimerRef.current !== null) {
            window.clearTimeout(hintTimerRef.current);
            hintTimerRef.current = null;
        }
    }, []);

    const showHintFor = useCallback((durationMs: number) => {
        clearHintTimer();
        setShowVoiceHint(true);
        hintTimerRef.current = window.setTimeout(() => {
            setShowVoiceHint(false);
            hintTimerRef.current = null;
        }, durationMs);
    }, [clearHintTimer]);

    useEffect(() => {
        return () => {
            clearHintTimer();
        };
    }, [clearHintTimer]);

    useEffect(() => {
        if (voiceErrorCode === 'not-allowed') {
            showHintFor(8000);
        }
    }, [showHintFor, voiceErrorCode]);

    useEffect(() => {
        if (disabled && isRecording) {
            void toggleRecording();
        }
    }, [disabled, isRecording, toggleRecording]);

    if (!isSupported) {
        return null;
    }

    const handleToggleRecording = async () => {
        if (disabled) {
            return;
        }

        const wasRecording = isRecording;
        await toggleRecording();

        if (!wasRecording && (permissionState === 'prompt' || permissionState === 'unknown')) {
            showHintFor(5000);
        }
    };

    const renderBars = (activeClassName: string, idleClassName: string) => (
        <div className={variant === 'chat' ? 'flex items-end gap-[2px] h-3.5' : 'overlay-voice-bars'}>
            {MIC_BAR_MULTIPLIERS.map((multiplier, index) => {
                const levelPx = Math.max(3, Math.round((2 + micLevel * 10) * multiplier));
                return (
                    <span
                        key={index}
                        className={hasMicSignal ? activeClassName : idleClassName}
                        style={{ height: `${levelPx}px`, opacity: hasMicSignal ? 1 : 0.8 }}
                    />
                );
            })}
        </div>
    );

    if (variant === 'overlay') {
        return (
            <div className="overlay-voice-control">
                {showVoiceHint && (
                    <div className="overlay-voice-hint" role="status">
                        <div className="overlay-voice-hint-title">
                            <Mic className="overlay-voice-hint-icon" />
                            Нужно разрешение
                        </div>
                        <div>{buildHintCopy()}</div>
                    </div>
                )}

                {isRecording && isMicMonitoringAvailable && (
                    <div
                        className={`overlay-voice-indicator ${hasMicSignal ? 'overlay-voice-indicator--active' : 'overlay-voice-indicator--idle'}`}
                        aria-hidden="true"
                    >
                        {renderBars('overlay-voice-bar overlay-voice-bar--active', 'overlay-voice-bar overlay-voice-bar--idle')}
                    </div>
                )}

                <button
                    type="button"
                    className={`overlay-voice-btn ${isRecording ? 'overlay-voice-btn--recording' : ''}`}
                    onClick={() => void handleToggleRecording()}
                    disabled={disabled}
                    title={disabled ? 'Голосовой ввод недоступен' : (isRecording ? 'Остановить запись' : 'Голосовой ввод')}
                    aria-label={isRecording ? 'Остановить голосовой ввод' : 'Начать голосовой ввод'}
                >
                    <Mic className={`overlay-voice-btn-icon ${isRecording ? 'overlay-voice-btn-icon--recording' : ''}`} />
                    {isRecording && (
                        <span
                            className={`overlay-voice-dot ${hasMicSignal ? 'overlay-voice-dot--active' : ''}`}
                            aria-hidden="true"
                        />
                    )}
                </button>
            </div>
        );
    }

    return (
        <div className="flex items-center gap-1.5 flex-shrink-0">
            {isRecording && isMicMonitoringAvailable && (
                <div
                    className={`h-8 flex items-center justify-center px-2 rounded-lg border transition-all ${hasMicSignal ? 'bg-emerald-500/10 border-emerald-500/30 text-emerald-300' : 'bg-zinc-900/80 border-zinc-700 text-zinc-400'}`}
                    aria-hidden="true"
                >
                    {renderBars(
                        'w-[3px] rounded-full transition-all duration-100 bg-emerald-400',
                        'w-[3px] rounded-full transition-all duration-100 bg-zinc-500',
                    )}
                </div>
            )}

            <div className="relative">
                {voiceError && !isRecording && (
                    <div className="absolute bottom-full right-0 mb-4 w-72 p-3 bg-red-900/90 text-red-100 text-xs rounded-xl shadow-2xl z-50 border border-red-700/50 animate-in fade-in slide-in-from-bottom-2 duration-300">
                        <div className="font-semibold mb-1">Голосовой ввод недоступен</div>
                        <div className="leading-relaxed">{voiceError}</div>
                        <div className="absolute top-full right-4 w-3 h-3 bg-red-900/90 rotate-45 -translate-y-1.5 border-r border-b border-red-700/50" />
                    </div>
                )}
                {showVoiceHint && !voiceError && (
                    <div className="absolute bottom-full right-0 mb-4 w-64 p-3 bg-blue-600 text-white text-xs rounded-xl shadow-2xl animate-in fade-in slide-in-from-bottom-2 duration-300 z-50">
                        <div className="font-bold mb-1 flex items-center gap-2">
                            <Mic className="w-3 h-3" />
                            Нужно разрешение
                        </div>
                        {buildHintCopy()}
                        <div className="absolute top-full right-4 w-3 h-3 bg-blue-600 rotate-45 -translate-y-1.5" />
                    </div>
                )}

                <button
                    onClick={() => void handleToggleRecording()}
                    disabled={disabled}
                    className={`w-8 h-8 flex items-center justify-center rounded-lg transition-all ${disabled ? 'opacity-20 cursor-not-allowed' : ''} ${isRecording ? 'bg-red-500 text-white shadow-[0_0_10px_rgba(239,68,68,0.5)]' : 'bg-zinc-800/50 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
                    title={disabled ? 'Голосовой ввод недоступен во время генерации' : (isRecording ? 'Остановить запись' : 'Голосовой ввод')}
                    aria-label={isRecording ? 'Остановить голосовой ввод' : 'Начать голосовой ввод'}
                >
                    <Mic className={`w-4 h-4 ${isRecording ? 'animate-pulse' : ''}`} />
                    {isRecording && (
                        <span
                            className={`absolute top-1 right-1 w-2 h-2 rounded-full border border-[#09090b] transition-colors ${hasMicSignal ? 'bg-emerald-300' : 'bg-amber-300'}`}
                            aria-hidden="true"
                        />
                    )}
                </button>
            </div>
        </div>
    );
}
