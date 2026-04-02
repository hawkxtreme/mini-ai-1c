import { invoke } from '@tauri-apps/api/core';

export interface ChatToolCall {
    id: string;
    type: string;
    function: {
        name: string;
        arguments: string;
    };
}

export interface ChatMessage {
    role: 'user' | 'assistant' | 'tool' | 'system';
    content: string;
    tool_calls?: ChatToolCall[];
    tool_call_id?: string;
    name?: string;
}

export interface ChatSession {
    id: string;
    title: string;
    timestamp: number;
    messages: ChatMessage[];
}

/**
 * Stream chat response
 * Note: This command emits events ('chat-chunk', 'chat-status', 'chat-done'), 
 * so the frontend needs to listen for them separately.
 */
export async function streamChat(messages: ChatMessage[]): Promise<void> {
    return await invoke('stream_chat', { messages });
}

/**
 * Stop current generation
 */
export async function stopChat(): Promise<void> {
    return await invoke('stop_chat');
}

/**
 * Inject a user message into the active agentic loop (mid-loop interrupt).
 * Returns true if accepted by an active loop, false if no loop is running.
 * When false the caller should fall back to the message queue.
 */
export async function interruptChat(message: string): Promise<boolean> {
    return await invoke('interrupt_chat', { message });
}


/**
 * Approve the pending tool call
 */
export async function approveTool(): Promise<void> {
    return await invoke('approve_tool');
}

/**
 * Reject the pending tool call
 */
export async function rejectTool(): Promise<void> {
    return await invoke('reject_tool');
}

/**
 * Clear 1С:Напарник conversation session (call on chat clear)
 */
export async function clearNaparnikSession(): Promise<void> {
    return await invoke('clear_naparnik_session');
}

export async function compactContext(messagesJson: string): Promise<string> {
    return await invoke<string>('compact_context', { messagesJson });
}
