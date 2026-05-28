import { invoke } from '@tauri-apps/api/core';

export interface SpeechRecognitionResult {
    text: string;
    isFinal: boolean;
}

type SpeechRecognitionError =
    | string
    | {
        error?: string;
        message?: string;
    };

const LOG_PREFIX = '[VoiceInput]';

function voiceLog(level: 'info' | 'warn' | 'error', ...args: unknown[]) {
    const ts = new Date().toISOString().slice(11, 23); // HH:MM:SS.mmm
    console[level](LOG_PREFIX, `[${ts}]`, ...args);

    // Дублируем в Rust backend (попадает в save_debug_logs)
    const parts = args.map((a) =>
        typeof a === 'object' ? JSON.stringify(a) : String(a),
    );
    const message = `${LOG_PREFIX} [${ts}] ${parts.join(' ')}`;
    invoke('write_frontend_log', { message }).catch(() => {/* best effort */});
}

export class SpeechRecognitionService {
    private recognition: any;
    private isListening = false;
    private SpeechRecognitionCtor: any = null;

    constructor() {
        // @ts-ignore
        const SpeechRecognition = window.SpeechRecognition || window.webkitSpeechRecognition;
        if (SpeechRecognition) {
            this.SpeechRecognitionCtor = SpeechRecognition;
            this.recognition = this.createInstance();
            voiceLog('info', 'SpeechRecognition API доступен', {
                hasSpeechRecognition: !!window.SpeechRecognition,
                hasWebkitSpeechRecognition: !!(window as any).webkitSpeechRecognition,
                userAgent: navigator.userAgent,
            });
        } else {
            voiceLog('error', 'SpeechRecognition API недоступен в этом WebView. Возможная причина: групповая политика Windows отключила службу распознавания речи.', {
                hasSpeechRecognition: !!window.SpeechRecognition,
                hasWebkitSpeechRecognition: !!(window as any).webkitSpeechRecognition,
                userAgent: navigator.userAgent,
            });
        }
    }

    private createInstance(): any {
        const instance = new this.SpeechRecognitionCtor();
        instance.continuous = true;
        instance.interimResults = true;
        instance.lang = 'ru-RU';
        voiceLog('info', 'Создан новый экземпляр SpeechRecognition', { continuous: true, lang: 'ru-RU' });
        return instance;
    }

    public start(
        onResult: (result: SpeechRecognitionResult) => void,
        onError: (error: SpeechRecognitionError) => void,
        onEnd?: () => void,
    ): boolean {
        if (!this.SpeechRecognitionCtor) {
            voiceLog('error', 'start() вызван но SpeechRecognition API недоступен');
            return false;
        }

        if (this.isListening) {
            voiceLog('warn', 'start() вызван пока isListening=true — пропускаем');
            return false;
        }

        // Пересоздаём экземпляр на каждый запуск — WebView2 не поддерживает restart одного объекта
        this.recognition = this.createInstance();

        this.recognition.onresult = (event: any) => {
            let interimTranscript = '';
            let finalTranscript = '';

            for (let i = event.resultIndex; i < event.results.length; ++i) {
                if (event.results[i].isFinal) {
                    finalTranscript += event.results[i][0].transcript;
                } else {
                    interimTranscript += event.results[i][0].transcript;
                }
            }

            const text = finalTranscript || interimTranscript;
            voiceLog('info', 'onresult', { isFinal: !!finalTranscript, textLength: text.length, resultIndex: event.resultIndex });

            onResult({
                text,
                isFinal: !!finalTranscript,
            });
        };

        this.recognition.onerror = (event: any) => {
            const errorCode = event?.error ?? 'unknown';
            voiceLog('error', 'onerror', { errorCode, message: event?.message, type: event?.type });
            this.isListening = false;
            onError(errorCode);
        };

        this.recognition.onend = () => {
            voiceLog('info', 'onend — сессия завершена', { wasListening: this.isListening });
            this.isListening = false;
            onEnd?.();
        };

        this.recognition.onnomatch = () => {
            voiceLog('warn', 'onnomatch — речь не распознана');
        };

        this.recognition.onaudiostart = () => {
            voiceLog('info', 'onaudiostart — захват аудио начат');
        };

        this.recognition.onaudioend = () => {
            voiceLog('info', 'onaudioend — захват аудио завершён');
        };

        this.recognition.onsoundstart = () => {
            voiceLog('info', 'onsoundstart — звук обнаружен');
        };

        this.recognition.onsoundend = () => {
            voiceLog('info', 'onsoundend — звук прекратился');
        };

        this.recognition.onspeechstart = () => {
            voiceLog('info', 'onspeechstart — распознавание речи началось');
        };

        this.recognition.onspeechend = () => {
            voiceLog('info', 'onspeechend — речь завершена');
        };

        try {
            voiceLog('info', 'Вызов recognition.start()...');
            this.recognition.start();
            this.isListening = true;
            voiceLog('info', 'recognition.start() выполнен успешно, isListening=true');
            return true;
        } catch (error) {
            const errorName = error instanceof Error ? error.name : 'UnknownError';
            const errorMsg = error instanceof Error ? error.message : String(error);
            voiceLog('error', 'recognition.start() выбросил исключение', { errorName, errorMsg, error });
            onError(error instanceof Error ? error.message : 'speech-start-error');
            return false;
        }
    }

    public stop(): boolean {
        voiceLog('info', 'stop() вызван', { isListening: this.isListening, hasRecognition: !!this.recognition });
        if (this.recognition && this.isListening) {
            this.recognition.stop();
            return true;
        }

        return false;
    }

    public isSupported(): boolean {
        return !!this.SpeechRecognitionCtor;
    }
}

export const speechService = new SpeechRecognitionService();
