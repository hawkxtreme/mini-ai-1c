
export interface SpeechRecognitionResult {
    text: string;
    isFinal: boolean;
}

export class SpeechRecognitionService {
    private recognition: any;
    private isListening: boolean = false;

    constructor() {
        // @ts-ignore
        const SpeechRecognition = window.SpeechRecognition || window.webkitSpeechRecognition;
        if (SpeechRecognition) {
            this.recognition = new SpeechRecognition();
            this.recognition.continuous = true;
            this.recognition.interimResults = true;
            this.recognition.lang = 'ru-RU';
        } else {
            console.error('Speech Recognition API not supported in this browser.');
        }
    }

    public start(onResult: (result: SpeechRecognitionResult) => void, onError: (error: any) => void) {
        if (!this.recognition || this.isListening) return;

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
                isFinal: !!finalTranscript
            });
        };

        this.recognition.onerror = (event: any) => {
            this.isListening = false;
            onError(event.error);
        };

        this.recognition.onend = () => {
            this.isListening = false;
        };

        try {
            this.recognition.start();
            this.isListening = true;
        } catch (e) {
            console.error('Error starting speech recognition:', e);
            onError(e);
        }
    }

    public stop() {
        if (this.recognition && this.isListening) {
            this.recognition.stop();
            this.isListening = false;
        }
    }

    public isSupported(): boolean {
        return !!this.recognition;
    }
}

export const speechService = new SpeechRecognitionService();
