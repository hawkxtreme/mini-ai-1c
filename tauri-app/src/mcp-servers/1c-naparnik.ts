import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { createParser } from "eventsource-parser";

// Configuration
const BASE_URL = "https://code.1c.ai";
const API_TOKEN = process.env.ONEC_AI_TOKEN;

if (!API_TOKEN) {
    console.error("Error: ONEC_AI_TOKEN environment variable is required.");
    process.exit(1);
}

// Session Management
interface ConversationSession {
    id: string;
    lastUsed: number;
}

const sessions: Record<string, ConversationSession> = {};
const MAX_SESSIONS = 10;
const SESSION_TTL = 3600 * 1000; // 1 hour

// 1C API Client
class OneCApiClient {
    private token: string;
    private lastMessageId: string | null = null;
    private currentSessionId: string | null = null;

    constructor(token: string) {
        this.token = value_or_throw(token, "Token is required");
    }

    async getOrCreateSession(createNew = false): Promise<string> {
        this.cleanupSessions();

        if (createNew || Object.keys(sessions).length === 0 || !this.currentSessionId) {
            const id = await this.createConversation();
            sessions[id] = { id, lastUsed: Date.now() };
            this.currentSessionId = id;
            return id;
        }

        // Return most recently used or current
        if (this.currentSessionId) {
            const session = sessions[this.currentSessionId];
            if (session) {
                session.lastUsed = Date.now();
                return session.id;
            }
        }

        const sorted = Object.values(sessions).sort((a, b) => b.lastUsed - a.lastUsed);
        if (sorted.length > 0) {
            const session = sorted[0];
            session.lastUsed = Date.now();
            this.currentSessionId = session.id;
            return session.id;
        }

        const id = await this.createConversation();
        this.currentSessionId = id;
        return id;
    }

    async createConversation(programmingLanguage = "1C (BSL)", scriptLanguage = "1C (BSL)"): Promise<string> {
        const response = await this.fetchWithRetry(`${BASE_URL}/chat_api/v1/conversations`, { // Removed trailing slash
            method: "POST",
            headers: {
                "Content-Type": "application/json",
                "Authorization": this.token,
                "Origin": BASE_URL,
                "Referer": `${BASE_URL}/chat//`,
            },
            body: JSON.stringify({
                ui_language: "ru",
                programming_language: programmingLanguage,
                script_language: "ru",
                skill_name: "custom",
                is_chat: true,
            }),
        });

        if (!response.ok) {
            const errorText = await response.text();
            throw new Error(`Failed to create conversation: ${response.status} ${response.statusText} - ${errorText}`);
        }

        const data = await response.json();
        console.error(`[1C:Naparnik] createConversation response: ${JSON.stringify(data)}`);

        if (data.root_message_uuid) {
            this.lastMessageId = data.root_message_uuid;
        } else {
            this.lastMessageId = null;
        }

        return data.uuid;
    }

    private async fetchWithRetry(url: string, options: any, retries = 3, backoff = 1000): Promise<Response> {
        try {
            const response = await fetch(url, options);
            if (!response.ok && retries > 0 && (response.status >= 500 || response.status === 429)) {
                console.error(`[1C:Naparnik] Request failed with status ${response.status}. Retrying in ${backoff}ms... (${retries} left)`);
                await new Promise(resolve => setTimeout(resolve, backoff));
                return this.fetchWithRetry(url, options, retries - 1, backoff * 2);
            }
            return response;
        } catch (error: any) {
            if (retries > 0) {
                console.error(`[1C:Naparnik] Network error: ${error.message}. Retrying in ${backoff}ms... (${retries} left)`);
                await new Promise(resolve => setTimeout(resolve, backoff));
                return this.fetchWithRetry(url, options, retries - 1, backoff * 2);
            }
            throw error;
        }
    }

    async sendMessage(conversationId: string, message: string): Promise<string> {
        const payload = {
            role: "user",
            content: {
                content: {
                    instruction: message
                }
            },
            parent_uuid: this.lastMessageId
        };
        console.error(`[1C:Naparnik] Sending message payload: ${JSON.stringify(payload)}`);

        const response = await this.fetchWithRetry(`${BASE_URL}/chat_api/v1/conversations/${conversationId}/messages`, {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
                "Authorization": this.token,
                "Origin": BASE_URL,
                "Referer": `${BASE_URL}/chat//`,
                "Accept": "text/event-stream",
            },
            body: JSON.stringify(payload),
        });

        if (!response.ok) {
            const errorText = await response.text();
            throw new Error(`Failed to send message: ${response.status} ${response.statusText} - ${errorText}`);
        }

        if (!response.body) {
            throw new Error("Response body is empty");
        }

        const DEBUG = process.env.ONEC_AI_DEBUG === "true";
        // Parse SSE
        let fullText = "";
        const self = this;
        const parser = createParser({
            onEvent(event) {
                try {
                    const data = JSON.parse(event.data);
                    if (DEBUG) {
                        console.error(`[1C:Naparnik] SSE Event Data: ${JSON.stringify(data)}`);
                    }

                    // Handle incremental updates
                    if (data.content_delta && data.content_delta.content) {
                        fullText += data.content_delta.content;
                    }

                    // Handle full result on finish or assistant role
                    if (data.role === "assistant" && data.content) {
                        if (data.content.text) fullText = data.content.text;
                        else if (data.content.content) fullText = data.content.content;
                    }

                    if (data.role === "assistant" && data.uuid) {
                        self.lastMessageId = data.uuid;
                    }
                } catch (e) {
                    // ignore json errors
                }
            }
        });

        // Use a stream reader
        const reader = response.body.getReader();
        const decoder = new TextDecoder();

        try {
            while (true) {
                const { done, value } = await reader.read();
                if (done) break;
                parser.feed(decoder.decode(value, { stream: true }));
            }
        } finally {
            reader.releaseLock();
        }

        return fullText;
    }



    private cleanupSessions() {
        const now = Date.now();
        for (const id in sessions) {
            if (now - sessions[id].lastUsed > SESSION_TTL) {
                delete sessions[id];
            }
        }
    }
}

function value_or_throw<T>(value: T | undefined | null, message: string): T {
    if (value === undefined || value === null) {
        throw new Error(message);
    }
    return value;
}

const apiClient = new OneCApiClient(API_TOKEN);

// Server Setup
const server = new McpServer({
    name: "1c-naparnik",
    version: "1.0.0",
});

// Tools

server.tool(
    "ask_1c_ai",
    "Задать вопрос ИИ-консультанту по платформе 1С, стандартам разработки и БСП",
    {
        question: z.string().describe("Вопрос для модели 1С.ai"),
        programming_language: z.string().optional().describe("Язык программирования (опционально)"),
        create_new_session: z.boolean().optional().describe("Создать новую сессию для этого вопроса"),
    },
    async ({ question, programming_language, create_new_session }) => {
        console.error(`[1C:Naparnik] Tool 'ask_1c_ai' called with question: ${question.substring(0, 50)}...`);
        try {
            const conversationId = await apiClient.getOrCreateSession(create_new_session);
            console.error(`[1C:Naparnik] Session ID: ${conversationId}`);
            const answer = await apiClient.sendMessage(conversationId, question);
            console.error(`[1C:Naparnik] Received answer (len: ${answer.length}): ${answer}`);
            return {
                content: [
                    {
                        type: "text",
                        text: answer
                    }
                ]
            };
        } catch (error: any) {
            console.error(`[1C:Naparnik] Error in 'ask_1c_ai': ${error.message}`);
            return {
                content: [
                    {
                        type: "text",
                        text: `Ошибка: ${error.message}`
                    }
                ],
                isError: true
            };
        }
    }
);

server.tool(
    "explain_1c_syntax",
    "Объяснить синтаксис конкретного метода, функции или встроенного объекта 1С",
    {
        syntax_element: z.string().describe("Элемент синтаксиса или объект 1С для объяснения"),
        context: z.string().optional().describe("Контекст использования"),
    },
    async ({ syntax_element, context }) => {
        console.error(`[1C:Naparnik] Tool 'explain_1c_syntax' called for: ${syntax_element}`);
        try {
            const question = `Объясни синтаксис и использование: ${syntax_element}${context ? ` в контексте: ${context}` : ""}`;
            const conversationId = await apiClient.getOrCreateSession();
            const answer = await apiClient.sendMessage(conversationId, question);
            console.error(`[1C:Naparnik] Received answer (len: ${answer.length}): ${answer}`);
            return {
                content: [
                    {
                        type: "text",
                        text: answer
                    }
                ]
            };
        } catch (error: any) {
            console.error(`[1C:Naparnik] Error in 'explain_1c_syntax': ${error.message}`);
            return {
                content: [
                    {
                        type: "text",
                        text: `Ошибка: ${error.message}`
                    }
                ],
                isError: true
            };
        }
    }
);

server.tool(
    "check_1c_code",
    "Проверить фрагмент кода 1С на наличие логических ошибок, проблем производительности или соответствие стандартам",
    {
        code: z.string().describe("Код 1С для проверки"),
        check_type: z.enum(["syntax", "logic", "performance"]).optional().default("syntax").describe("Тип проверки"),
    },
    async ({ code, check_type }) => {
        console.error(`[1C:Naparnik] Tool 'check_1c_code' called. Type: ${check_type}, Code len: ${code.length}`);
        try {
            const checkDescriptions: Record<string, string> = {
                "syntax": "синтаксические ошибки",
                "logic": "логические ошибки и потенциальные проблемы",
                "performance": "проблемы производительности и оптимизации"
            };
            const desc = checkDescriptions[check_type] || "ошибки";
            const question = `Проверь этот код 1С на ${desc} и дай рекомендации:\n\n\`\`\`bsl\n${code}\n\`\`\``;

            const conversationId = await apiClient.getOrCreateSession();
            const answer = await apiClient.sendMessage(conversationId, question);
            console.error(`[1C:Naparnik] Received answer (len: ${answer.length}): ${answer}`);
            return {
                content: [
                    {
                        type: "text",
                        text: answer
                    }
                ]
            };
        } catch (error: any) {
            console.error(`[1C:Naparnik] Error in 'check_1c_code': ${error.message}`);
            return {
                content: [
                    {
                        type: "text",
                        text: `Ошибка: ${error.message}`
                    }
                ],
                isError: true
            };
        }
    }
);

// Start Server
async function run() {
    const transport = new StdioServerTransport();
    await server.connect(transport);
}

run().catch(console.error);
