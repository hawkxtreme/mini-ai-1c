import { ConfiguratorTitleContext } from '../utils/configurator';

export interface QueuedMessage {
    id: string;
    content: string;
    displayContent?: string;
    codeContext?: string;
    diagnostics?: string[];
    configuratorCtx?: ConfiguratorTitleContext | null;
    timestamp: number;
}

type QueueListener = (queue: QueuedMessage[]) => void;

class MessageQueueService {
    private _queue: QueuedMessage[] = [];
    private _listeners: Set<QueueListener> = new Set();

    get messages(): readonly QueuedMessage[] {
        return this._queue;
    }

    get isEmpty(): boolean {
        return this._queue.length === 0;
    }

    enqueue(msg: Omit<QueuedMessage, 'id' | 'timestamp'>): QueuedMessage {
        const item: QueuedMessage = {
            ...msg,
            id: Math.random().toString(36).substring(2, 15),
            timestamp: Date.now(),
        };
        this._queue.push(item);
        this._notify();
        return item;
    }

    dequeue(): QueuedMessage | undefined {
        const item = this._queue.shift();
        if (item !== undefined) this._notify();
        return item;
    }

    remove(id: string): void {
        const prev = this._queue.length;
        this._queue = this._queue.filter(m => m.id !== id);
        if (this._queue.length !== prev) this._notify();
    }

    update(id: string, content: string): void {
        const msg = this._queue.find(m => m.id === id);
        if (msg) {
            msg.content = content;
            this._notify();
        }
    }

    clear(): void {
        if (this._queue.length === 0) return;
        this._queue = [];
        this._notify();
    }

    subscribe(listener: QueueListener): () => void {
        this._listeners.add(listener);
        return () => this._listeners.delete(listener);
    }

    private _notify(): void {
        const snapshot = [...this._queue];
        this._listeners.forEach(fn => fn(snapshot));
    }
}

export const messageQueueService = new MessageQueueService();
