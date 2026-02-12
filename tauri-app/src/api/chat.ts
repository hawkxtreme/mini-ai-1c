import { invoke } from '@tauri-apps/api/core';

export interface ChatMessage {
    role: 'user' | 'assistant';
    content: string;
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
 * Get all chat sessions
 */
export async function getChatSessions(): Promise<ChatSession[]> {
    return await invoke<ChatSession[]>('get_chat_sessions');
}

/**
 * Get active chat session
 */
export async function getActiveChat(): Promise<ChatSession> {
    return await invoke<ChatSession>('get_active_chat');
}

/**
 * Create new chat session
 */
export async function createChat(): Promise<ChatSession> {
    return await invoke<ChatSession>('create_chat');
}

/**
 * Switch to a chat session
 */
export async function switchChat(sessionId: string): Promise<ChatSession> {
    return await invoke<ChatSession>('switch_chat', { sessionId });
}

/**
 * Delete a chat session
 */
export async function deleteChat(sessionId: string): Promise<void> {
    return await invoke('delete_chat', { sessionId });
}

/**
 * Save a message to the active chat
 */
export async function saveMessage(role: string, content: string): Promise<void> {
    return await invoke('save_chat_message', { role, content });
}
