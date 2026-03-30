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

export class SpeechRecognitionService {
    private recognition: any;
    private isListening = false;

    constructor() {
        // @ts-ignore
        const SpeechRecognition = window.SpeechRecognition || window.webkitSpeechRecognition;
        if (SpeechRecognition) {
            this.recognition = new SpeechRecognition();
            // Keep listening through short pauses so dictation feels closer to the main chat input.
            this.recognition.continuous = true;
            this.recognition.interimResults = true;
            this.recognition.lang = 'ru-RU';
        } else {
            console.error('Speech Recognition API not supported in this browser.');
        }
    }

    public start(
        onResult: (result: SpeechRecognitionResult) => void,
        onError: (error: SpeechRecognitionError) => void,
        onEnd?: () => void,
    ): boolean {
        if (!this.recognition || this.isListening) {
            return false;
        }

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

            onResult({
                text: finalTranscript || interimTranscript,
                isFinal: !!finalTranscript,
            });
        };

        this.recognition.onerror = (event: any) => {
            this.isListening = false;
            onError(event?.error ?? event);
        };

        this.recognition.onend = () => {
            this.isListening = false;
            onEnd?.();
        };

        try {
            this.recognition.start();
            this.isListening = true;
            return true;
        } catch (error) {
            console.error('Error starting speech recognition:', error);
            onError(error instanceof Error ? error.message : 'speech-start-error');
            return false;
        }
    }

    public stop(): boolean {
        if (this.recognition && this.isListening) {
            this.recognition.stop();
            return true;
        }

        return false;
    }

    public isSupported(): boolean {
        return !!this.recognition;
    }
}

export const speechService = new SpeechRecognitionService();
