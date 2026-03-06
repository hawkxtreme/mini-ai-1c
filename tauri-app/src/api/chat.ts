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
    role: 'user' | 'assistant' | 'tool';
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
