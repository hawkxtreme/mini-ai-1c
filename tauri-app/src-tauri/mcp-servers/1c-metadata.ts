import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
    CallToolRequestSchema,
    ListToolsRequestSchema,
    ListResourcesRequestSchema,
    ReadResourceRequestSchema,
    ListPromptsRequestSchema,
    GetPromptRequestSchema,
    InitializeRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";

/**
 * 1C:Metadata MCP Server (Thin Proxy for Kharin's 1c_mcp extension)
 * 
 * This server acts as a bridge between the IDE (stdio) and 1C:Enterprise (HTTP).
 * It forwards all standard MCP requests directly to the 1C extension.
 * It forwards standard MCP requests directly to the 1C extension using JSON-RPC 2.0.
 */

const BASE_URL = process.env.ONEC_METADATA_URL || "http://localhost/base/hs/mcp";

const USERNAME = process.env.ONEC_USERNAME || "";
const PASSWORD = process.env.ONEC_PASSWORD || "";
const DEBUG = process.env.ONEC_AI_DEBUG === "true";

const server = new Server(
    {
        name: "1c-mcp-native-proxy",
        version: "1.2.0",
    },
    {
        capabilities: {
            tools: {},
            resources: {},
            prompts: {},
        },
    }
);

/**
 * Helper to call 1C HTTP Service using JSON-RPC 2.0
 */
async function call1C(method: string, params: any = {}) {
    const url = BASE_URL.endsWith("/") ? `${BASE_URL}rpc` : `${BASE_URL}/rpc`;
    const requestId = Math.floor(Math.random() * 1000000);

    if (DEBUG) {
        console.error(`[1C:Native] Sending to 1C: ${method}`, JSON.stringify(params));
    }

    const headers: Record<string, string> = {
        "Content-Type": "application/json",
    };

    if (USERNAME) {
        const auth = Buffer.from(`${USERNAME}:${PASSWORD}`).toString("base64");
        headers["Authorization"] = `Basic ${auth}`;
    }

    try {
        const response = await fetch(url, {
            method: "POST",
            headers: headers,
            body: JSON.stringify({
                jsonrpc: "2.0",
                method: method,
                params: params,
                id: requestId,
            }),
        });

        if (!response.ok) {
            const errorText = await response.text();
            throw new Error(`HTTP Error ${response.status}: ${errorText}`);
        }

        const json: any = await response.json();

        if (json.error) {
            throw new Error(`1C Error [${json.error.code}]: ${json.error.message}`);
        }

        return json.result;
    } catch (error: any) {
        if (DEBUG) {
            console.error(`[1C:Native] Error calling 1C:`, error.message);
        }
        throw error;
    }
}

// Forward Tools Listing
server.setRequestHandler(ListToolsRequestSchema, async () => {
    try {
        return await call1C("tools/list");
    } catch (error: any) {
        return { tools: [] };
    }
});

// Forward Tool Execution
server.setRequestHandler(CallToolRequestSchema, async (request) => {
    try {
        return await call1C("tools/call", {
            name: request.params.name,
            arguments: request.params.arguments,
        });
    } catch (error: any) {
        return {
            content: [{ type: "text", text: `Ошибка вызова инструмента в 1С: ${error.message}` }],
            isError: true,
        };
    }
});

// Forward Resources Mapping
server.setRequestHandler(ListResourcesRequestSchema, async () => {
    try {
        return await call1C("resources/list");
    } catch (error: any) {
        return { resources: [] };
    }
});

server.setRequestHandler(ReadResourceRequestSchema, async (request) => {
    try {
        return await call1C("resources/read", {
            uri: request.params.uri,
        });
    } catch (error: any) {
        throw new Error(`Ошибка чтения ресурса из 1С: ${error.message}`);
    }
});

// Forward Prompts Mapping
server.setRequestHandler(ListPromptsRequestSchema, async () => {
    try {
        return await call1C("prompts/list");
    } catch (error: any) {
        return { prompts: [] };
    }
});

server.setRequestHandler(GetPromptRequestSchema, async (request) => {
    try {
        return await call1C("prompts/get", {
            name: request.params.name,
            arguments: request.params.arguments,
        });
    } catch (error: any) {
        throw new Error(`Ошибка получения промпта из 1С: ${error.message}`);
    }
});

async function main() {
    const transport = new StdioServerTransport();
    await server.connect(transport);
    console.error("1C Metadata Proxy (Kharin-compatible) started");
}

main().catch((error) => {
    console.error("Server error:", error);
    process.exit(1);
});
