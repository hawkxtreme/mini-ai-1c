export interface MicActivitySnapshot {
    level: number;
    hasSignal: boolean;
}

const EMIT_INTERVAL_MS = 120;
const SIGNAL_ON_THRESHOLD = 0.05;
const SIGNAL_OFF_THRESHOLD = 0.032;
const SIGNAL_HOLD_MS = 260;
const LEVEL_SMOOTHING_FACTOR = 0.24;
const LEVEL_STEP = 0.035;

type AudioContextCtor = typeof AudioContext;
type WindowWithWebkitAudio = Window & typeof globalThis & {
    webkitAudioContext?: AudioContextCtor;
};

export class MicActivityMonitor {
    private audioContext: AudioContext | null = null;
    private analyser: AnalyserNode | null = null;
    private source: MediaStreamAudioSourceNode | null = null;
    private stream: MediaStream | null = null;
    private data: Uint8Array | null = null;
    private frameId: number | null = null;
    private lastEmitTs = 0;
    private lastSnapshot: MicActivitySnapshot = { level: 0, hasSignal: false };
    private onUpdate: ((snapshot: MicActivitySnapshot) => void) | null = null;
    private smoothedLevel = 0;
    private signalActive = false;
    private lastSignalDetectedAt = 0;

    public async start(onUpdate: (snapshot: MicActivitySnapshot) => void): Promise<void> {
        if (!navigator.mediaDevices?.getUserMedia) {
            throw new Error('Microphone monitoring is not supported in this environment.');
        }

        const AudioContextImpl =
            window.AudioContext || (window as WindowWithWebkitAudio).webkitAudioContext;

        if (!AudioContextImpl) {
            throw new Error('AudioContext is not supported in this environment.');
        }

        this.onUpdate = onUpdate;
        this.stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        this.audioContext = new AudioContextImpl();
        this.analyser = this.audioContext.createAnalyser();
        this.analyser.fftSize = 1024;
        this.analyser.smoothingTimeConstant = 0.78;
        this.data = new Uint8Array(this.analyser.fftSize);

        this.source = this.audioContext.createMediaStreamSource(this.stream);
        this.source.connect(this.analyser);

        if (this.audioContext.state === 'suspended') {
            await this.audioContext.resume();
        }

        this.emit({ level: 0, hasSignal: false }, true);
        this.loop();
    }

    public async stop(): Promise<void> {
        if (this.frameId !== null) {
            cancelAnimationFrame(this.frameId);
            this.frameId = null;
        }

        this.emit({ level: 0, hasSignal: false }, true);

        try {
            this.source?.disconnect();
        } catch {
            // Ignore disconnect errors during teardown.
        }

        try {
            this.analyser?.disconnect();
        } catch {
            // Ignore disconnect errors during teardown.
        }

        this.stream?.getTracks().forEach(track => track.stop());

        if (this.audioContext && this.audioContext.state !== 'closed') {
            try {
                await this.audioContext.close();
            } catch {
                // Ignore close errors during teardown.
            }
        }

        this.audioContext = null;
        this.analyser = null;
        this.source = null;
        this.stream = null;
        this.data = null;
        this.onUpdate = null;
        this.lastEmitTs = 0;
        this.lastSnapshot = { level: 0, hasSignal: false };
        this.smoothedLevel = 0;
        this.signalActive = false;
        this.lastSignalDetectedAt = 0;
    }

    private loop = () => {
        if (!this.analyser || !this.data) {
            return;
        }

        this.analyser.getByteTimeDomainData(this.data);

        let sumSquares = 0;
        for (const value of this.data) {
            const normalized = (value - 128) / 128;
            sumSquares += normalized * normalized;
        }

        const rms = Math.sqrt(sumSquares / this.data.length);
        const rawLevel = Math.min(1, rms * 8.5);
        const level = this.smoothLevel(rawLevel);
        const now = performance.now();
        const snapshot: MicActivitySnapshot = {
            level: this.quantizeLevel(level),
            hasSignal: this.resolveSignal(level, now),
        };

        const shouldEmit =
            now - this.lastEmitTs >= EMIT_INTERVAL_MS ||
            Math.abs(snapshot.level - this.lastSnapshot.level) >= LEVEL_STEP * 1.5 ||
            snapshot.hasSignal !== this.lastSnapshot.hasSignal;

        if (shouldEmit) {
            this.emit(snapshot, false);
        }

        this.frameId = requestAnimationFrame(this.loop);
    };

    private emit(snapshot: MicActivitySnapshot, force: boolean) {
        if (
            !force &&
            snapshot.level === this.lastSnapshot.level &&
            snapshot.hasSignal === this.lastSnapshot.hasSignal
        ) {
            return;
        }

        this.lastSnapshot = snapshot;
        this.lastEmitTs = performance.now();
        this.onUpdate?.(snapshot);
    }

    private smoothLevel(rawLevel: number): number {
        this.smoothedLevel += (rawLevel - this.smoothedLevel) * LEVEL_SMOOTHING_FACTOR;
        return this.smoothedLevel;
    }

    private quantizeLevel(level: number): number {
        return Math.min(1, Math.round(level / LEVEL_STEP) * LEVEL_STEP);
    }

    private resolveSignal(level: number, now: number): boolean {
        if (level >= SIGNAL_ON_THRESHOLD) {
            this.signalActive = true;
            this.lastSignalDetectedAt = now;
            return true;
        }

        if (!this.signalActive) {
            return false;
        }

        const keepSignalActive =
            level >= SIGNAL_OFF_THRESHOLD || now - this.lastSignalDetectedAt < SIGNAL_HOLD_MS;

        if (!keepSignalActive) {
            this.signalActive = false;
        }

        return this.signalActive;
    }
}
