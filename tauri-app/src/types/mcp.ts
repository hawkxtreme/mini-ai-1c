export interface McpToolInfo {
  server_name: string;
  tool_name: string;
  description: string | null;
  input_schema: Record<string, unknown> | null;
  is_enabled: boolean;
}

export interface McpToolSummary {
  name: string;
  description?: string;
}

