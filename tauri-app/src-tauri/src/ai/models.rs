use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Deserialize `arguments` field that may be either a JSON string or a JSON object/value.
/// OpenAI sends it as a JSON-encoded string; some providers (e.g. MiniMax) send it as a raw object.
fn deserialize_arguments_flexible<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let v: Option<Value> = Option::deserialize(deserializer)?;
    Ok(match v {
        None => None,
        Some(Value::String(s)) => Some(s),
        Some(other) => Some(other.to_string()),
    })
}

/// Chat message for API (OpenAI compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub r#type: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Request body for OpenAI-compatible API
#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ApiMessage>,
    pub stream: bool,
    pub temperature: f32,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    /// Qwen3 extended thinking mode (must use temperature=1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_thinking: Option<bool>,
    /// Token budget for thinking step (1024–38912, default 8192)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget_tokens: Option<u32>,
}

/// Streaming chunk from OpenAI API
#[derive(Debug, Deserialize)]
pub struct StreamChunk {
    pub choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
pub struct StreamChoice {
    pub delta: StreamDelta,
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamDelta {
    pub content: Option<String>,
    /// Thinking field. Qwen3 uses `reasoning_content` (enable_thinking=true).
    /// Some Ollama Cloud models (gpt-oss, minimax) emit `reasoning` — accepted via alias.
    #[serde(alias = "reasoning")]
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
pub struct ToolCallDelta {
    pub index: Option<usize>,
    pub id: Option<String>,
    #[allow(dead_code)]
    pub r#type: Option<String>, // Renamed from _type for consistency and to avoid prefix
    pub function: Option<ToolCallFunctionDelta>,
}

#[derive(Debug, Deserialize)]
pub struct ToolCallFunctionDelta {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_arguments_flexible")]
    pub arguments: Option<String>,
}

/// Non-streaming response from OpenAI-compatible API (stream: false)
#[derive(Debug, Deserialize)]
pub struct NonStreamResponse {
    pub choices: Vec<NonStreamChoice>,
}

#[derive(Debug, Deserialize)]
pub struct NonStreamChoice {
    pub message: NonStreamMessage,
}

#[derive(Debug, Deserialize)]
pub struct NonStreamMessage {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<NonStreamToolCall>>,
}

/// Tool call from non-streaming response — arguments may be string or object (provider-specific).
#[derive(Debug, Deserialize)]
pub struct NonStreamToolCall {
    pub id: String,
    pub r#type: String,
    pub function: NonStreamToolCallFunction,
}

#[derive(Debug, Deserialize)]
pub struct NonStreamToolCallFunction {
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_arguments_flexible")]
    pub arguments: Option<String>,
}

/// System prompt for 1C assistant
/// Extended tool info for internal prompt generation
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub tool: Tool,
    pub server_id: String,
}
