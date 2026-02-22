use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use reqwest::Client;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub context_window: u32,
    pub description: Option<String>,
    pub cost_in: Option<f64>, // Cost per 1M input tokens
    pub cost_out: Option<f64>, // Cost per 1M output tokens
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub api_base: String,
    pub models: Vec<Model>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryData {
    pub providers: HashMap<String, RegistryProviderData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryProviderData {
    pub models: Vec<Model>,
}

const REGISTRY_URL: &str = "https://raw.githubusercontent.com/hawkxtreme/mini-ai-1c/main/registry/models.json"; // Placeholder
// const OPENAI_MODELS_ENDPOINT: &str = "/v1/models";

pub async fn fetch_models_from_api(_provider_id: &str, base_url: &str, api_key: &str) -> Result<Vec<Model>, String> {
    let client = Client::new();
    let trimmed_base = base_url.trim_end_matches('/');
    
    let url = if trimmed_base.ends_with("/v1") {
        format!("{}/models", trimmed_base)
    } else {
        format!("{}/v1/models", trimmed_base)
    };

    // Basic logic for OpenAI compatible APIs
    let mut builder = client.get(&url);
    
    if !api_key.is_empty() {
        builder = builder.header("Authorization", format!("Bearer {}", api_key));
    }

    let resp = builder
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("API request failed: {}", resp.status()));
    }

    // OpenAI/OpenRouter usually returns { "data": [ { "id": "..." } ] }
    #[derive(Deserialize)]
    struct OpenAiModel {
        id: String,
        // Some proxies (like OpenRouter) or local servers might include these
        context_window: Option<u32>,
        max_tokens: Option<u32>,
    }
    #[derive(Deserialize)]
    struct OpenAiResponse {
        data: Vec<OpenAiModel>,
    }

    let body = resp.text().await.map_err(|e| e.to_string())?;
    println!("Raw API response for provider: {}", body);
    
    let completion: OpenAiResponse = serde_json::from_str(&body).map_err(|e| e.to_string())?;

    let models = completion.data.into_iter().map(|m| {
        let cw = m.context_window.or(m.max_tokens).unwrap_or(4096);
        Model {
            id: m.id.clone(),
            name: m.id.clone(),
            context_window: cw,
            description: None,
            cost_in: None,
            cost_out: None,
        }
    }).collect();

    Ok(models)
}

pub async fn fetch_registry() -> Result<RegistryData, String> {
    let client = Client::new();
    let resp = client.get(REGISTRY_URL).send().await.map_err(|e| e.to_string())?;
    
    if !resp.status().is_success() {
        // Fallback to empty registry if offline/missing
        return Ok(RegistryData { providers: HashMap::new() });
    }

    let registry: RegistryData = resp.json().await.map_err(|e| e.to_string())?;
    Ok(registry)
}

/// Merges API models with Registry metadata
pub fn merge_models(api_models: Vec<Model>, registry: &RegistryData, provider_id: &str) -> Vec<Model> {
    println!("Merging models for provider_id: '{}'. Registry size: {} providers", provider_id, registry.providers.len());
    
    api_models.into_iter().map(|mut model| {
        let initial_cw = model.context_window;
        
        // 1. Try specified provider
        let mut source = "api_default";
        let mut found_in_registry = false;
        if let Some(p_data) = registry.providers.get(provider_id) {
            if let Some(reg_model) = p_data.models.iter().find(|m| m.id == model.id) {
                enrich_model(&mut model, reg_model);
                found_in_registry = true;
                source = "registry_local";
            }
        }

        // 2. Try global search if not found
        if !found_in_registry {
            for (p_id, p_data) in &registry.providers {
                if let Some(reg_model) = p_data.models.iter().find(|m| m.id == model.id) {
                    enrich_model(&mut model, reg_model);
                    let _ = true; // flag was found_in_registry
                    source = "registry_global";
                    println!("  Model '{}' found in global registry under '{}'", model.id, p_id);
                    break;
                }
            }
        }

        // 3. Apply Heuristics if still using default or lower context
        if model.context_window <= 4096 {
            let id_lower = model.id.to_lowercase();
            if id_lower.contains("gemini") {
                if id_lower.contains("1.5") || id_lower.contains("2.") || id_lower.contains("3.") || 
                   id_lower.contains("-2") || id_lower.contains("-3") || id_lower.contains("flash") || id_lower.contains("pro") {
                    model.context_window = 1_048_576; // 1M
                } else {
                    model.context_window = 128_000;
                }
                source = "heuristic_gemini";
            } else if id_lower.contains("claude-3") || id_lower.contains("claude-2") {
                model.context_window = 200_000;
                source = "heuristic_claude";
            } else if id_lower.contains("gpt-4") || id_lower.contains("gpt-4o") {
                model.context_window = 128_000;
                source = "heuristic_gpt4";
            } else if id_lower.contains("o1-") || id_lower.contains("o3-") {
                model.context_window = 128_000;
                source = "heuristic_openai_o";
            } else if id_lower.contains("deepseek") {
                if id_lower.contains("-v3") || id_lower.contains("-r1") {
                    model.context_window = 64_000;
                } else {
                    model.context_window = 32_000;
                }
                source = "heuristic_deepseek";
            } else if id_lower.contains("llama-3") {
                model.context_window = 128_000;
                source = "heuristic_llama3";
            }
        }

        if initial_cw != model.context_window {
            println!("  Model updated: '{}' | {} -> {} (Source: {})", model.id, initial_cw, model.context_window, source);
        }

        model
    }).collect()
}

fn enrich_model(model: &mut Model, reg_model: &Model) {
    model.context_window = reg_model.context_window;
    model.cost_in = reg_model.cost_in;
    model.cost_out = reg_model.cost_out;
    model.description = reg_model.description.clone();
}
