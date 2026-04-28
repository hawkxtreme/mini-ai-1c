use futures::future::join_all;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub context_window: u32,
    pub description: Option<String>,
    pub cost_in: Option<f64>,  // Cost per 1M input tokens
    pub cost_out: Option<f64>, // Cost per 1M output tokens
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryData {
    pub providers: HashMap<String, RegistryProviderData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryProviderData {
    pub models: Vec<Model>,
}

const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/hawkxtreme/mini-ai-1c/main/registry/models.json"; // Placeholder
                                                                                         // const OPENAI_MODELS_ENDPOINT: &str = "/v1/models";

pub fn static_minimax_models() -> Vec<Model> {
    // Context windows per official docs: https://platform.minimax.io/docs/api-reference/api-overview
    // M2.x series: 204,800 tokens context window
    // Recommended max_tokens for coding integrations: 64,000 (per AI coding tools guide)
    vec![
        Model {
            id: "MiniMax-M2.7".into(),
            name: "MiniMax M2.7".into(),
            context_window: 204_800,
            description: Some(
                "MiniMax flagship model. Context: 204k. Recommended output: 64k.".into(),
            ),
            cost_in: Some(0.30),
            cost_out: Some(1.10),
        },
        Model {
            id: "MiniMax-M2.7-highspeed".into(),
            name: "MiniMax M2.7 Highspeed".into(),
            context_window: 204_800,
            description: Some("MiniMax M2.7 fast variant. Context: 204k.".into()),
            cost_in: Some(0.30),
            cost_out: Some(1.10),
        },
        Model {
            id: "MiniMax-M2.5".into(),
            name: "MiniMax M2.5".into(),
            context_window: 204_800,
            description: Some("MiniMax M2.5 model. Context: 204k.".into()),
            cost_in: Some(0.20),
            cost_out: Some(1.10),
        },
        Model {
            id: "MiniMax-Text-01".into(),
            name: "MiniMax Text-01".into(),
            context_window: 1_000_000,
            description: Some("MiniMax legacy long-context text model, 1M context.".into()),
            cost_in: Some(0.20),
            cost_out: Some(1.10),
        },
        Model {
            id: "abab7-preview".into(),
            name: "ABAB7 Preview".into(),
            context_window: 245_760,
            description: Some("MiniMax ABAB7 preview model.".into()),
            cost_in: None,
            cost_out: None,
        },
    ]
}

/// Fetch MiniMax models: try live /v1/models API, fallback to static list.
async fn fetch_minimax_models(base_url: &str, api_key: &str) -> Result<Vec<Model>, String> {
    if api_key.trim().is_empty() {
        return Ok(static_minimax_models());
    }

    let client = Client::new();
    let trimmed = base_url.trim_end_matches('/');
    let url = if trimmed.ends_with("/v1") {
        format!("{}/models", trimmed)
    } else {
        format!("{}/v1/models", trimmed)
    };

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let body = r.text().await.unwrap_or_default();
            #[derive(Deserialize)]
            struct MiniMaxModel {
                id: String,
                context_window: Option<u32>,
            }
            #[derive(Deserialize)]
            struct MiniMaxResponse {
                data: Vec<MiniMaxModel>,
            }
            if let Ok(parsed) = serde_json::from_str::<MiniMaxResponse>(&body) {
                let models: Vec<Model> = parsed
                    .data
                    .into_iter()
                    .map(|m| {
                        let cw = m.context_window.unwrap_or(1_000_000);
                        Model {
                            id: m.id.clone(),
                            name: m.id.clone(),
                            context_window: cw,
                            description: None,
                            cost_in: None,
                            cost_out: None,
                        }
                    })
                    .collect();
                if !models.is_empty() {
                    return Ok(models);
                }
            }
            Ok(static_minimax_models())
        }
        _ => Ok(static_minimax_models()),
    }
}

pub fn static_codex_models() -> Vec<Model> {
    vec![
        Model {
            id: "gpt-5.5".into(),
            name: "GPT-5.5".into(),
            context_window: 272_000,
            description: Some("Most capable frontier agentic coding model.".into()),
            cost_in: None,
            cost_out: None,
        },
        Model {
            id: "gpt-5.5-mini".into(),
            name: "GPT-5.5 Mini".into(),
            context_window: 272_000,
            description: Some("Smaller, faster GPT-5.5 variant for everyday coding tasks.".into()),
            cost_in: None,
            cost_out: None,
        },
        Model {
            id: "gpt-5.4".into(),
            name: "GPT-5.4".into(),
            context_window: 272_000,
            description: Some("Latest frontier agentic coding model.".into()),
            cost_in: None,
            cost_out: None,
        },
        Model {
            id: "gpt-5.4-mini".into(),
            name: "GPT-5.4 Mini".into(),
            context_window: 272_000,
            description: Some("Smaller frontier agentic coding model.".into()),
            cost_in: None,
            cost_out: None,
        },
        Model {
            id: "gpt-5.3-codex".into(),
            name: "GPT-5.3 Codex".into(),
            context_window: 272_000,
            description: Some("Frontier Codex-optimized agentic coding model.".into()),
            cost_in: None,
            cost_out: None,
        },
        Model {
            id: "gpt-5.2-codex".into(),
            name: "GPT-5.2 Codex".into(),
            context_window: 272_000,
            description: Some("Frontier agentic coding model.".into()),
            cost_in: None,
            cost_out: None,
        },
        Model {
            id: "gpt-5.2".into(),
            name: "GPT-5.2".into(),
            context_window: 272_000,
            description: Some("Optimized for professional work and long-running agents.".into()),
            cost_in: None,
            cost_out: None,
        },
        Model {
            id: "gpt-5.1-codex-max".into(),
            name: "GPT-5.1 Codex Max".into(),
            context_window: 272_000,
            description: Some("Codex-optimized model for deep and fast reasoning.".into()),
            cost_in: None,
            cost_out: None,
        },
        Model {
            id: "gpt-5.1-codex-mini".into(),
            name: "GPT-5.1 Codex Mini".into(),
            context_window: 272_000,
            description: Some("Optimized for codex. Cheaper, faster, but less capable.".into()),
            cost_in: None,
            cost_out: None,
        },
    ]
}

pub async fn fetch_models_from_api(
    provider_id: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<Model>, String> {
    // Special handling for Qwen CLI — return static list immediately (no /v1/models endpoint via OAuth)
    if provider_id == "QwenCli" {
        return Ok(vec![
            Model {
                id: "coder-model".into(),
                name: "Qwen 3.5 Plus (1M ctx)".into(),
                context_window: 1_048_576,
                description: Some(
                    "Qwen 3.5 Plus — hybrid model, leading coding, 1M context".into(),
                ),
                cost_in: None,
                cost_out: None,
            },
            Model {
                id: "qwen3-coder-plus".into(),
                name: "Qwen3 Coder Plus".into(),
                context_window: 1_048_576,
                description: Some("Advanced code generation and understanding, 1M context".into()),
                cost_in: None,
                cost_out: None,
            },
            Model {
                id: "qwen3-coder-flash".into(),
                name: "Qwen3 Coder Flash".into(),
                context_window: 262_144,
                description: Some("Fast code generation model, 256K context".into()),
                cost_in: None,
                cost_out: None,
            },
            Model {
                id: "vision-model".into(),
                name: "Qwen3 Vision".into(),
                context_window: 262_144,
                description: Some("Multimodal vision-language model, 256K context".into()),
                cost_in: None,
                cost_out: None,
            },
        ]);
    }

    if provider_id == "CodexCli" {
        return Ok(static_codex_models());
    }

    // MiniMax: try live API first, fallback to static list on error
    if provider_id == "MiniMax" {
        return fetch_minimax_models(base_url, api_key).await;
    }

    let requires_api_key = matches!(
        provider_id,
        "OpenAI"
            | "Anthropic"
            | "OpenRouter"
            | "Google"
            | "DeepSeek"
            | "Groq"
            | "Mistral"
            | "XAI"
            | "Perplexity"
            | "ZAI"
            | "OneCNaparnik"
            | "OllamaCloud"
    );

    if requires_api_key && api_key.trim().is_empty() {
        return Err(format!(
            "Для провайдера {} требуется API key. Сохраните ключ в профиле и попробуйте снова.",
            provider_id
        ));
    }

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

    let resp = builder.send().await.map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("API request failed: {}", resp.status()));
    }

    // OpenAI/OpenRouter: { "data": [ { "id": "..." } ] }
    // Some proxies may include context_window or max_tokens.
    // LM Studio's /v1/models does NOT include context info — handled below via /api/v0/models.
    #[derive(Deserialize)]
    struct OpenAiModel {
        id: String,
        context_window: Option<u32>,
        max_tokens: Option<u32>,
    }
    #[derive(Deserialize)]
    struct OpenAiResponse {
        data: Vec<OpenAiModel>,
    }

    let body = resp.text().await.map_err(|e| e.to_string())?;
    crate::app_log!("[LLM] Raw API response for provider: {}", body);

    let completion: OpenAiResponse = serde_json::from_str(&body).map_err(|e| e.to_string())?;

    let mut models: Vec<Model> = completion
        .data
        .into_iter()
        .map(|m| {
            let cw = m.context_window.or(m.max_tokens).unwrap_or(4096);
            Model {
                id: m.id.clone(),
                name: m.id.clone(),
                context_window: cw,
                description: None,
                cost_in: None,
                cost_out: None,
            }
        })
        .collect();

    // For Ollama / Ollama Cloud: use native /api/show to get the actual llm.context_length per model.
    // The /v1/models endpoint does not expose this, so all models default to 4096 without this step.
    // Both localhost (http://localhost:11434) and ollama.com expose /api/show.
    if provider_id == "Ollama" || provider_id == "OllamaCloud" {
        let ollama_base = derive_ollama_native_base(trimmed_base);
        enrich_ollama_context_windows(&client, &ollama_base, api_key, &mut models).await;
    }

    // For LM Studio: /v1/models returns no context info. Use native /api/v0/models
    // which has loaded_context_length (the actual runtime context) and max_context_length.
    if provider_id == "LMStudio" {
        let lms_base = derive_ollama_native_base(trimmed_base);
        enrich_lmstudio_context_windows(&client, &lms_base, &mut models).await;
    }

    Ok(models)
}

/// Derives the native Ollama base URL (port 11434 root) from any OpenAI-compat base URL.
/// e.g. "http://localhost:11434/v1" → "http://localhost:11434"
fn derive_ollama_native_base(openai_base: &str) -> String {
    // Strip trailing /v1 (and /v1/) to get the Ollama root
    openai_base
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string()
}

/// Parses num_ctx from Ollama's plain-text parameters string.
/// Format: "num_ctx                        8192\ntemperature                    0.7\n..."
fn parse_num_ctx(parameters: &str) -> Option<u32> {
    parameters.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        if parts.next()? == "num_ctx" {
            parts.next()?.parse::<u32>().ok()
        } else {
            None
        }
    })
}

/// Calls POST /api/show for each model in parallel and updates context_window
/// from model_info["llm.context_length"].
/// `api_key` is required for ollama.com (cloud); ignored for local Ollama.
async fn enrich_ollama_context_windows(
    client: &Client,
    ollama_base: &str,
    api_key: &str,
    models: &mut Vec<Model>,
) {
    let show_url = format!("{}/api/show", ollama_base);

    #[derive(Deserialize)]
    struct ShowResponse {
        // "parameters" is a plain-text string with lines like "num_ctx 8192\ntemperature 0.7"
        parameters: Option<String>,
        model_info: Option<serde_json::Map<String, serde_json::Value>>,
    }

    // Fetch all in parallel
    let futures: Vec<_> = models
        .iter()
        .map(|m| {
            let url = show_url.clone();
            let name = m.id.clone();
            let client = client.clone();
            let auth_header = if api_key.is_empty() {
                None
            } else {
                Some(format!("Bearer {}", api_key))
            };
            async move {
                let mut req = client
                    .post(&url)
                    .json(&serde_json::json!({ "name": name }))
                    .timeout(std::time::Duration::from_secs(10));
                if let Some(h) = auth_header {
                    req = req.header(reqwest::header::AUTHORIZATION, h);
                }
                let result = req.send().await;

                match result {
                    Ok(resp) if resp.status().is_success() => {
                        match resp.json::<ShowResponse>().await {
                            Ok(show) => {
                                // Priority 1: num_ctx from Modelfile parameters
                                // (user-configured context, e.g. "PARAMETER num_ctx 8192")
                                let num_ctx = show.parameters.as_deref().and_then(parse_num_ctx);

                                // Priority 2: architecture max from model_info
                                // e.g. "qwen2.context_length", "llama.context_length"
                                let arch_ctx = show
                                    .model_info
                                    .as_ref()
                                    .and_then(|mi| {
                                        mi.iter()
                                            .find(|(k, _)| k.ends_with(".context_length"))
                                            .and_then(|(_, v)| v.as_u64())
                                    })
                                    .map(|v| v as u32);

                                let ctx = num_ctx.or(arch_ctx);
                                (name, ctx)
                            }
                            Err(e) => {
                                crate::app_log!(
                                    "[Ollama] /api/show parse error for {}: {}",
                                    name,
                                    e
                                );
                                (name, None)
                            }
                        }
                    }
                    Ok(resp) => {
                        crate::app_log!(
                            "[Ollama] /api/show returned {} for {}",
                            resp.status(),
                            name
                        );
                        (name, None)
                    }
                    Err(e) => {
                        crate::app_log!("[Ollama] /api/show request failed for {}: {}", name, e);
                        (name, None)
                    }
                }
            }
        })
        .collect();

    let results = join_all(futures).await;

    for (model_id, ctx_opt) in results {
        if let Some(ctx) = ctx_opt {
            if let Some(m) = models.iter_mut().find(|m| m.id == model_id) {
                crate::app_log!(
                    "[Ollama] context_window for {}: {} → {}",
                    model_id,
                    m.context_window,
                    ctx
                );
                m.context_window = ctx;
            }
        }
    }
}

/// Calls GET /api/v0/models (LM Studio native API) and updates context_window
/// from loaded_context_length (actual runtime context) or max_context_length.
async fn enrich_lmstudio_context_windows(client: &Client, lms_base: &str, models: &mut Vec<Model>) {
    let url = format!("{}/api/v0/models", lms_base);

    #[derive(Deserialize)]
    struct LmsModel {
        id: String,
        loaded_context_length: Option<u32>,
        max_context_length: Option<u32>,
    }
    #[derive(Deserialize)]
    struct LmsResponse {
        data: Vec<LmsModel>,
    }

    let result = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<LmsResponse>().await {
                Ok(lms) => {
                    for lm in lms.data {
                        // Prefer loaded_context_length (actual runtime) over max_context_length
                        let ctx = lm.loaded_context_length.or(lm.max_context_length);
                        if let Some(ctx) = ctx {
                            if let Some(m) = models.iter_mut().find(|m| m.id == lm.id) {
                                crate::app_log!(
                                    "[LMStudio] context_window for {}: {} → {}",
                                    lm.id,
                                    m.context_window,
                                    ctx
                                );
                                m.context_window = ctx;
                            }
                        }
                    }
                }
                Err(e) => {
                    crate::app_log!("[LMStudio] /api/v0/models parse error: {}", e);
                }
            }
        }
        Ok(resp) => {
            crate::app_log!("[LMStudio] /api/v0/models returned {}", resp.status());
        }
        Err(e) => {
            crate::app_log!("[LMStudio] /api/v0/models request failed: {}", e);
        }
    }
}

pub async fn fetch_registry() -> Result<RegistryData, String> {
    let client = Client::new();
    let resp = client
        .get(REGISTRY_URL)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        // Fallback to empty registry if offline/missing
        return Ok(RegistryData {
            providers: HashMap::new(),
        });
    }

    let registry: RegistryData = resp.json().await.map_err(|e| e.to_string())?;
    Ok(registry)
}

/// Merges API models with Registry metadata
pub fn merge_models(
    api_models: Vec<Model>,
    registry: &RegistryData,
    provider_id: &str,
) -> Vec<Model> {
    crate::app_log!(
        "[LLM] Merging models for provider_id: '{}'. Registry size: {} providers",
        provider_id,
        registry.providers.len()
    );

    api_models
        .into_iter()
        .map(|mut model| {
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
                        crate::app_log!(
                            "  Model '{}' found in global registry under '{}'",
                            model.id,
                            p_id
                        );
                        break;
                    }
                }
            }

            // 3. Apply Heuristics if still using default or lower context
            if model.context_window <= 4096 {
                let id_lower = model.id.to_lowercase();
                if id_lower.contains("gemini") {
                    if id_lower.contains("1.5")
                        || id_lower.contains("2.")
                        || id_lower.contains("3.")
                        || id_lower.contains("-2")
                        || id_lower.contains("-3")
                        || id_lower.contains("flash")
                        || id_lower.contains("pro")
                    {
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
                crate::app_log!(
                    "  Model updated: '{}' | {} -> {} (Source: {})",
                    model.id,
                    initial_cw,
                    model.context_window,
                    source
                );
            }

            model
        })
        .collect()
}

fn enrich_model(model: &mut Model, reg_model: &Model) {
    model.context_window = reg_model.context_window;
    model.cost_in = reg_model.cost_in;
    model.cost_out = reg_model.cost_out;
    model.description = reg_model.description.clone();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_ollama_native_base_strips_v1() {
        assert_eq!(
            derive_ollama_native_base("http://localhost:11434/v1"),
            "http://localhost:11434"
        );
        assert_eq!(
            derive_ollama_native_base("http://localhost:11434/v1/"),
            "http://localhost:11434"
        );
        assert_eq!(
            derive_ollama_native_base("http://192.168.1.10:11434"),
            "http://192.168.1.10:11434"
        );
        // Custom port, no /v1 suffix
        assert_eq!(
            derive_ollama_native_base("http://localhost:8080/v1"),
            "http://localhost:8080"
        );
    }

    /// Интеграционный тест: Ollama возвращает реальный context_window > 4096.
    ///
    /// Запустить:
    ///   OLLAMA_HOST=http://localhost:11434 cargo test -p mini-ai-1c -- ollama_context --nocapture --ignored
    #[tokio::test]
    #[ignore = "requires Ollama with at least one model; run with --ignored"]
    async fn ollama_context_window_is_fetched_from_show() {
        let host =
            std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
        let base_url = format!("{}/v1", host.trim_end_matches('/'));

        let result = fetch_models_from_api("Ollama", &base_url, "").await;
        let models = result.expect("fetch_models_from_api should succeed for Ollama");

        assert!(
            !models.is_empty(),
            "Ollama should return at least one model"
        );

        eprintln!("[INFO] Ollama models:");
        for m in &models {
            eprintln!("  {} → context_window = {}", m.id, m.context_window);
        }

        // Every model must have context_window > 4096 — the API-default fallback.
        // If any model still has 4096 it means /api/show is not being called or returned nothing.
        let all_above_default = models.iter().all(|m| m.context_window > 4096);
        assert!(
            all_above_default,
            "All Ollama models should have context_window > 4096 (fetched from /api/show). \
             Got: {:?}",
            models
                .iter()
                .map(|m| (&m.id, m.context_window))
                .collect::<Vec<_>>()
        );
    }

    /// Интеграционный тест: LM Studio возвращает context_window через max_context_length.
    ///
    /// Запустить:
    ///   LMSTUDIO_HOST=http://localhost:1234 cargo test -p mini-ai-1c -- lmstudio_context --nocapture --ignored
    #[tokio::test]
    #[ignore = "requires LM Studio with Local Server started; run with --ignored"]
    async fn lmstudio_context_window_is_fetched_from_models() {
        let host =
            std::env::var("LMSTUDIO_HOST").unwrap_or_else(|_| "http://localhost:1234".to_string());
        let base_url = format!("{}/v1", host.trim_end_matches('/'));

        let result = fetch_models_from_api("LMStudio", &base_url, "").await;

        // If server is not running — gracefully skip
        let models = match result {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[SKIP] LM Studio server not reachable at {}: {}", host, e);
                return;
            }
        };

        if models.is_empty() {
            eprintln!("[SKIP] LM Studio returned empty model list — no model loaded");
            return;
        }

        eprintln!("[INFO] LM Studio models:");
        for m in &models {
            eprintln!("  {} → context_window = {}", m.id, m.context_window);
        }

        // Every loaded model should have context_window > 4096
        let all_above_default = models.iter().all(|m| m.context_window > 4096);
        assert!(
            all_above_default,
            "All LM Studio models should have context_window > 4096 (from max_context_length). \
             Got: {:?}",
            models
                .iter()
                .map(|m| (&m.id, m.context_window))
                .collect::<Vec<_>>()
        );
    }
}
