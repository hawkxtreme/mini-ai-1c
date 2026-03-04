use std::sync::Mutex;
use lazy_static::lazy_static;
use crate::mcp_client::McpClient;
use crate::settings::load_settings;
use super::models::{ToolInfo, Tool, ToolFunction};

lazy_static! {
    pub static ref TOOLS_CACHE: Mutex<Option<(std::time::Instant, Vec<ToolInfo>)>> = Mutex::new(None);
}

/// Collect all tools from enabled MCP servers to inject into LLM request
pub async fn get_available_tools() -> Vec<ToolInfo> {
    let settings = load_settings();
    let mut all_tools = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    crate::app_log!("[MCP][TOOLS] Collecting tools...");
    
    // Check cache first
    {
        if let Ok(cache) = TOOLS_CACHE.lock() {
            if let Some((time, tools)) = &*cache {
                if time.elapsed().as_secs() < 120 { // 2 minute cache
                    let duration = time.elapsed().as_millis();
                    crate::app_log!("[MCP][CACHE] Using cached tools ({} items, {} ms ago)", tools.len(), duration);
                    return tools.clone();
                }
            }
        }
    }

    let start_time = std::time::Instant::now();

    let mut all_configs = settings.mcp_servers.clone();
    
    // Add virtual BSL server only if not already present
    if !all_configs.iter().any(|c| c.id == "bsl-ls") {
        all_configs.push(crate::settings::McpServerConfig {
            id: "bsl-ls".to_string(),
            name: "BSL Language Server".to_string(),
            enabled: settings.bsl_server.enabled,
            transport: crate::settings::McpTransport::Internal,
            ..Default::default()
        });
    }

    let enabled_configs: Vec<_> = all_configs.into_iter().filter(|c| c.enabled).collect();
    let mut futures = Vec::new();

    for config in enabled_configs {
        futures.push(async move {
            let server_name = config.name.clone();
            let server_id = config.id.clone();
            let start = std::time::Instant::now();
            crate::app_log!("[MCP][TOOLS] Connecting to server: {} (ID: {})", server_name, server_id);
            
            match McpClient::new(config).await {
                Ok(client) => {
                    match client.list_tools().await {
                        Ok(tools) => {
                            let duration = start.elapsed().as_millis();
                            crate::app_log!("[MCP][TOOLS] Server {} returned {} tools in {} ms.", server_name, tools.len(), duration);
                            Ok((server_id, tools))
                        },
                        Err(e) => {
                            crate::app_log!("[MCP][TOOLS][ERROR] Failed to list tools for {}: {}", server_name, e);
                            Err(e)
                        }
                    }
                },
                Err(e) => {
                    crate::app_log!("[MCP][TOOLS][ERROR] Failed to connect to {}: {}", server_name, e);
                    Err(e)
                }
            }
        });
    }

    let results = futures::future::join_all(futures).await;

    for res in results {
        if let Ok((server_id, tools)) = res {
            for tool in tools {
                // 1. Sanitize Name (only alphanumeric, underscore, hyphen)
                let name = tool.name.chars()
                    .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                    .collect::<String>();
                
                if name.is_empty() { 
                    crate::app_log!("[MCP][TOOLS][WARN] Tool name became empty after sanitization: {}", tool.name);
                    continue; 
                }

                // 2. Ensure unique name
                if seen_names.contains(&name) {
                    crate::app_log!("[MCP][TOOLS][WARN] Duplicate tool name '{}'. Skipping.", name);
                    continue;
                }
                seen_names.insert(name.clone());

                // 3. Sanitize Schema (Gemini/OpenAI strictly require root type "object")
                let mut parameters = tool.input_schema.clone();
                if !parameters.is_object() {
                    parameters = serde_json::json!({
                        "type": "object",
                        "properties": {}
                    });
                } else {
                    let obj = parameters.as_object_mut().unwrap();
                    if !obj.contains_key("type") {
                        obj.insert("type".to_string(), serde_json::json!("object"));
                    }
                    if !obj.contains_key("properties") {
                            obj.insert("properties".to_string(), serde_json::json!({}));
                    }
                }

                crate::app_log!("[MCP][TOOLS]   + Registered: {}", name);
                all_tools.push(ToolInfo {
                    server_id: server_id.clone(),
                    tool: Tool {
                        r#type: "function".to_string(),
                        function: ToolFunction {
                            name,
                            description: tool.description,
                            parameters,
                        },
                    },
                });
            }
        }
    }
    
    let total_duration = start_time.elapsed().as_millis();
    crate::app_log!("[MCP][TOOLS] Total collection time: {} ms. Total tools: {}", total_duration, all_tools.len());
    
    // Update cache
    if let Ok(mut cache) = TOOLS_CACHE.lock() {
        *cache = Some((std::time::Instant::now(), all_tools.clone()));
    }

    all_tools
}

/// Force clear the MCP tools cache
pub fn clear_mcp_cache() {
    if let Ok(mut cache) = TOOLS_CACHE.lock() {
        *cache = None;
        crate::app_log!("[MCP][CACHE] Cache cleared.");
    }
}
