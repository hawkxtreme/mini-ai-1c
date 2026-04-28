use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::Emitter;

use super::models::*;
use super::prompts::*;
use super::tools::*;
use crate::llm_profiles::{get_active_profile, LLMProvider};

const QWEN_MIN_REQUEST_GAP_MS: u64 = 1_100;
const QWEN_MAX_RETRY_DELAY_SECS: u64 = 10;
const QWEN_MAX_429_ATTEMPTS: u32 = 3;

static QWEN_REQUEST_SLOTS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
struct QwenRateLimitContext {
    retry_after_secs: Option<u64>,
    message: Option<String>,
    is_quota_exceeded: bool,
    is_hard_quota_exhausted: bool,
    is_burst_limited: bool,
}

fn qwen_request_slots() -> &'static Mutex<HashMap<String, Instant>> {
    QWEN_REQUEST_SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn wait_with_chat_status<F>(
    app_handle: &tauri::AppHandle,
    duration: Duration,
    mut format_status: F,
) where
    F: FnMut(u64) -> String,
{
    if duration.is_zero() {
        return;
    }

    let mut remaining = duration;
    while !remaining.is_zero() {
        let display_secs = remaining.as_secs().max(1);
        let _ = app_handle.emit("chat-status", format_status(display_secs));
        let tick = remaining.min(Duration::from_secs(1));
        tokio::time::sleep(tick).await;
        remaining = remaining.saturating_sub(tick);
    }
}

async fn wait_for_qwen_request_slot(profile_id: &str, app_handle: &tauri::AppHandle) {
    let wait_for = {
        let mut slots = qwen_request_slots()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let entry = slots.entry(profile_id.to_string()).or_insert(now);
        let send_at = if *entry > now { *entry } else { now };
        *entry = send_at + Duration::from_millis(QWEN_MIN_REQUEST_GAP_MS);
        send_at.saturating_duration_since(now)
    };

    if !wait_for.is_zero() {
        wait_with_chat_status(app_handle, wait_for, |secs| {
            format!(
                "Qwen выравнивает частоту запросов. Отправлю запрос через {}с.",
                secs
            )
        })
        .await;
    }
}

fn extend_qwen_cooldown(profile_id: &str, cooldown: Duration) {
    let mut slots = qwen_request_slots()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let candidate = Instant::now() + cooldown;
    let entry = slots
        .entry(profile_id.to_string())
        .or_insert_with(Instant::now);
    if *entry < candidate {
        *entry = candidate;
    }
}

fn extract_retry_after_secs(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|secs| secs.clamp(1, QWEN_MAX_RETRY_DELAY_SECS))
}

fn parse_qwen_rate_limit_context(headers: &HeaderMap, error_body: &str) -> QwenRateLimitContext {
    let body_lower = error_body.to_lowercase();
    let message = serde_json::from_str::<serde_json::Value>(error_body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        });
    let is_hard_quota_exhausted = body_lower.contains("free allocated quota exceeded")
        || body_lower.contains("commodity has not purchased yet")
        || body_lower.contains("prepaid bill is overdue")
        || body_lower.contains("postpaid bill is overdue");

    QwenRateLimitContext {
        retry_after_secs: extract_retry_after_secs(headers),
        is_quota_exceeded: body_lower.contains("insufficient_quota")
            || body_lower.contains("allocated quota exceeded")
            || body_lower.contains("current quota"),
        is_hard_quota_exhausted,
        is_burst_limited: body_lower.contains("request rate increased too quickly")
            || body_lower.contains("rate limit exceeded")
            || body_lower.contains("too many requests"),
        message,
    }
}

fn qwen_retry_delay(attempt: u32, ctx: &QwenRateLimitContext) -> Option<Duration> {
    if ctx.is_hard_quota_exhausted {
        return None;
    }

    if let Some(retry_after) = ctx.retry_after_secs {
        return Some(Duration::from_secs(retry_after));
    }

    let retry_secs = if ctx.is_quota_exceeded || ctx.is_burst_limited {
        match attempt {
            1 => 2,
            2 => 4,
            _ => 8,
        }
    } else if attempt == 1 {
        2
    } else {
        return None;
    };

    Some(Duration::from_secs(retry_secs))
}

fn qwen_has_tool_heavy_context(messages: &[ApiMessage]) -> bool {
    messages.iter().rev().take(8).any(|message| {
        message.role == "tool"
            || message
                .tool_calls
                .as_ref()
                .map(|calls| !calls.is_empty())
                .unwrap_or(false)
    })
}

fn qwen_max_tokens(profile_max_tokens: u32, tool_heavy: bool) -> u32 {
    let base = profile_max_tokens.max(8_192).min(65_536);
    if tool_heavy {
        base.min(16_384)
    } else {
        base
    }
}

fn qwen_thinking_budget(estimated_tokens: u32, tool_heavy: bool) -> u32 {
    let budget = estimated_tokens.saturating_mul(80) / 100;
    if tool_heavy {
        budget.max(4_096).min(8_192)
    } else {
        budget.max(8_192).min(32_768)
    }
}

fn reduce_qwen_request_pressure(
    request_body: &mut ChatRequest,
    tool_heavy: bool,
    quota_exceeded: bool,
) -> bool {
    let mut changed = false;

    let max_tokens_cap = if quota_exceeded {
        8_192
    } else if tool_heavy {
        12_288
    } else {
        16_384
    };
    if request_body.max_tokens > max_tokens_cap {
        request_body.max_tokens = max_tokens_cap;
        changed = true;
    }

    if let Some(current_budget) = request_body.thinking_budget_tokens {
        let budget_cap = if quota_exceeded { 4_096 } else { 8_192 };
        if current_budget > budget_cap {
            request_body.thinking_budget_tokens = Some(budget_cap);
            changed = true;
        }
    }

    changed
}

fn build_qwen_rate_limit_message(ctx: &QwenRateLimitContext) -> String {
    let provider_message = ctx
        .message
        .as_deref()
        .unwrap_or("Qwen временно ограничил запросы.");

    if ctx.is_hard_quota_exhausted {
        format!(
            "Qwen отклонил запрос из-за исчерпанной или недоступной квоты: {} Это не похоже на кратковременный всплеск нагрузки, поэтому автоматический повтор не помог бы. Дождитесь сброса бесплатной квоты или подключите подходящий план/подписку, если она требуется для этой модели.",
            provider_message
        )
    } else if ctx.is_quota_exceeded {
        format!(
            "Qwen отклонил запрос по лимиту квоты или токенов: {} Я уже сделал короткие автоповторы, но окно лимита ещё не восстановилось. По официальной документации такой код часто означает краткосрочный TPS/TPM/RPM-лимит, поэтому подождите 10-60 секунд, сократите объём запроса или используйте тариф с более высокими лимитами.",
            provider_message
        )
    } else if ctx.is_burst_limited {
        format!(
            "Qwen временно ограничил частоту запросов: {} Я уже сделал короткие автоповторы, но окно лимита ещё не освободилось. Попробуйте повторить чуть позже.",
            provider_message
        )
    } else {
        format!("Qwen вернул 429: {}", provider_message)
    }
}

fn provider_requires_api_key(provider: &LLMProvider) -> bool {
    matches!(
        provider,
        LLMProvider::OpenAI
            | LLMProvider::Anthropic
            | LLMProvider::OpenRouter
            | LLMProvider::Google
            | LLMProvider::DeepSeek
            | LLMProvider::Groq
            | LLMProvider::Mistral
            | LLMProvider::XAI
            | LLMProvider::Perplexity
            | LLMProvider::ZAI
            | LLMProvider::OneCNaparnik
            | LLMProvider::OllamaCloud
    )
}

pub fn resolve_profile_api_key(
    profile: &crate::llm_profiles::LLMProfile,
) -> Result<String, String> {
    let api_key = profile.try_get_api_key()?;
    if provider_requires_api_key(&profile.provider) && api_key.trim().is_empty() {
        return Err(format!(
            "Для провайдера {} не найден API key. Откройте профиль и сохраните ключ заново.",
            profile.provider
        ));
    }
    Ok(api_key)
}

/// Stream chat completion from OpenAI-compatible API
/// Returns the full accumulated response text
pub async fn stream_chat_completion(
    messages: Vec<ApiMessage>,
    app_handle: tauri::AppHandle,
) -> Result<ApiMessage, String> {
    // Route 1С:Напарник to its dedicated client (non-OpenAI API)
    {
        let p = get_active_profile().ok_or("No active LLM profile")?;
        if matches!(p.provider, LLMProvider::OneCNaparnik) {
            return super::naparnik_client::stream_naparnik_completion(messages, app_handle).await;
        }
    }

    let profile = get_active_profile().ok_or("No active LLM profile")?;
    let has_tool_heavy_context = qwen_has_tool_heavy_context(&messages);
    // Build system prompt: use lightweight variant for local providers (Ollama/LMStudio)
    // to avoid smaller models rephrasing instead of responding.
    let tools_info = get_available_tools().await;
    let tools: Vec<Tool> = tools_info.iter().map(|i| i.tool.clone()).collect();
    let tools_opt = if tools.is_empty() { None } else { Some(tools) };

    let system_prompt = if is_local_provider(Some(&profile.provider)) {
        get_lightweight_system_prompt(&tools_info, &messages)
    } else {
        get_system_prompt(&tools_info, &messages)
    };

    let mut api_messages = vec![ApiMessage {
        role: "system".to_string(),
        content: Some(system_prompt),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }];
    api_messages.extend(messages);

    if matches!(profile.provider, LLMProvider::CodexCli) {
        return super::codex_client::stream_codex_completion(api_messages, app_handle).await;
    }

    let (api_key, url) = if matches!(profile.provider, LLMProvider::QwenCli) {
        let token_info = crate::llm::cli_providers::qwen::QwenCliProvider::get_token(&profile.id)?;
        let (access_token, refresh_token, expires_at, resource_url) =
            token_info.ok_or("Qwen CLI: Требуется авторизация")?;

        // Auto-refresh if token expired (or expires within next 60 seconds)
        let (access_token, resource_url) = if chrono::Utc::now().timestamp() as u64 + 60
            > expires_at
        {
            if let Some(rt) = refresh_token.as_deref() {
                crate::app_log!(force: true, "[Qwen] Token expired/near-expiry, attempting refresh...");
                let _ =
                    app_handle.emit("chat-status", "Qwen: обновляю токен доступа...".to_string());
                match crate::llm::cli_providers::qwen::QwenCliProvider::refresh_access_token(
                    &profile.id,
                    rt,
                )
                .await
                {
                    Ok(()) => {
                        let new_info = crate::llm::cli_providers::qwen::QwenCliProvider::get_token(
                            &profile.id,
                        )?
                        .ok_or("Qwen CLI: Токен не найден после обновления")?;
                        crate::app_log!(force: true, "[Qwen] Token refreshed successfully");
                        (new_info.0, new_info.3)
                    }
                    Err(e) => {
                        crate::app_log!(force: true, "[Qwen] Token refresh failed: {}", e);
                        return Err("Qwen CLI: Токен истек и не удалось обновить. Требуется повторная авторизация".to_string());
                    }
                }
            } else {
                return Err("Qwen CLI: Токен истек. Требуется повторная авторизация".to_string());
            }
        } else {
            (access_token, resource_url)
        };

        let base = if let Some(ru) = resource_url
            .as_deref()
            .filter(|s| !s.is_empty() && !s.contains("dashscope") && !s.contains("aliyun"))
        {
            format!("https://{}/v1", ru)
        } else {
            "https://portal.qwen.ai/v1".to_string()
        };
        (access_token, format!("{}/chat/completions", base))
    } else {
        let api_key = resolve_profile_api_key(&profile)?;
        let raw_url = profile.get_base_url();
        // Normalize: Ollama and similar OpenAI-compatible servers require /v1/chat/completions.
        // If the user entered a bare URL without /v1 (e.g. http://host:11434), add it automatically.
        let base_url = {
            let trimmed = raw_url.trim_end_matches('/');
            if matches!(
                profile.provider,
                LLMProvider::Ollama | LLMProvider::OllamaCloud | LLMProvider::LMStudio
            ) && !trimmed.ends_with("/v1")
            {
                format!("{}/v1", trimmed)
            } else {
                trimmed.to_string()
            }
        };
        (api_key, format!("{}/chat/completions", base_url))
    };

    let api_max_tokens = if matches!(profile.provider, LLMProvider::QwenCli) {
        qwen_max_tokens(profile.max_tokens, has_tool_heavy_context)
    } else if matches!(profile.provider, LLMProvider::LMStudio) {
        // Qwen3 thinking models need large token budget to finish thinking before producing content
        profile.max_tokens.max(8192)
    } else if matches!(profile.provider, LLMProvider::MiniMax) {
        // Official docs (platform.minimax.io): context window 204,800; coding tools integration
        // recommends max_tokens=64,000. Cap at 64k, but respect lower user-set values.
        profile.max_tokens.min(64_000).max(4_096)
    } else if profile.max_tokens > 16384 {
        4096
    } else {
        profile.max_tokens
    };

    let thinking_enabled = matches!(profile.provider, LLMProvider::QwenCli)
        && profile.enable_thinking.unwrap_or(false);

    let effective_temperature = if thinking_enabled {
        1.0
    } else {
        profile.temperature
    };

    // Dynamic thinking budget: estimate total input tokens (chars / 4),
    // then allocate 80%, clamped to [8192, 32768].
    //
    // Rationale: Qwen3 hallucinates tool_calls when absolute thinking budget is too low.
    // Tests showed hallucination starts at input ~13k tokens. At that point:
    //   old formula 30% max(8192): budget = max(3.9k, 8192) = 8192t (62%) → hallucinates
    //   new formula 80% max(8192): budget = max(10.5k, 8192) = 10530t (80%) → should help
    // The min(8192) ensures small contexts always get at least 8192t of thinking.
    // The 80% ratio scales proportionally as context grows, providing more absolute tokens.
    let dynamic_thinking_budget: Option<u32> = if thinking_enabled {
        let total_chars: usize = api_messages
            .iter()
            .map(|m| m.content.as_deref().map(|c| c.len()).unwrap_or(0))
            .sum();
        let estimated_tokens = (total_chars / 4) as u32;
        let budget = qwen_thinking_budget(estimated_tokens, has_tool_heavy_context);
        crate::app_log!(
            "[AI] Thinking budget: {}t (input ~{}t)",
            budget,
            estimated_tokens
        );
        Some(budget)
    } else {
        None
    };

    let use_stream = !profile.disable_streaming.unwrap_or(false);

    let mut request_body = ChatRequest {
        model: profile.model.clone(),
        messages: api_messages,
        stream: use_stream,
        temperature: effective_temperature,
        max_tokens: api_max_tokens,
        tools: tools_opt,
        enable_thinking: if thinking_enabled {
            Some(true)
        } else if matches!(profile.provider, LLMProvider::LMStudio) {
            Some(false)
        } else {
            None
        },
        thinking_budget_tokens: dynamic_thinking_budget,
    };

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    if !api_key.is_empty() {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key)).map_err(|e| e.to_string())?,
        );
    }

    if matches!(profile.provider, LLMProvider::OpenRouter) {
        headers.insert(
            "HTTP-Referer",
            HeaderValue::from_static("https://mini-ai-1c.local"),
        );
        headers.insert("X-Title", HeaderValue::from_static("Mini AI 1C Agent"));
    }

    if matches!(profile.provider, LLMProvider::QwenCli) {
        headers.insert(
            "User-Agent",
            HeaderValue::from_static("QwenCode/0.10.3 (darwin; arm64)"),
        );
        headers.insert(
            "X-Dashscope-Useragent",
            HeaderValue::from_static("QwenCode/0.10.3 (darwin; arm64)"),
        );
        headers.insert(
            "X-Dashscope-Authtype",
            HeaderValue::from_static("qwen-oauth"),
        );
        headers.insert(
            "X-Dashscope-Cachecontrol",
            HeaderValue::from_static("enable"),
        );
        headers.insert("X-Stainless-Runtime", HeaderValue::from_static("node"));
        headers.insert(
            "X-Stainless-Runtime-Version",
            HeaderValue::from_static("v22.17.0"),
        );
        headers.insert("X-Stainless-Lang", HeaderValue::from_static("js"));
        headers.insert(
            "X-Stainless-Package-Version",
            HeaderValue::from_static("5.11.0"),
        );
        headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("cors"));
    }

    // === DIAGNOSTIC LOGGING: context breakdown ===
    {
        let mut role_counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        let mut role_tokens: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for m in &request_body.messages {
            let role = m.role.as_str();
            let chars = m.content.as_deref().map(|c| c.len()).unwrap_or(0);
            // tool_calls also count
            let tc_chars = m
                .tool_calls
                .as_ref()
                .map(|tc| {
                    tc.iter()
                        .map(|t| t.function.arguments.len() + t.function.name.len())
                        .sum::<usize>()
                })
                .unwrap_or(0);
            *role_counts.entry(role).or_default() += 1;
            *role_tokens.entry(role).or_default() += (chars + tc_chars) / 4;
        }
        let total_msgs = request_body.messages.len();
        let total_tokens: usize = role_tokens.values().sum();
        let tools_count = request_body.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        crate::app_log!(
            "[AI][CTX] msgs={} (sys={} user={} assistant={} tool={}) ~{}t tools={}",
            total_msgs,
            role_counts.get("system").copied().unwrap_or(0),
            role_counts.get("user").copied().unwrap_or(0),
            role_counts.get("assistant").copied().unwrap_or(0),
            role_counts.get("tool").copied().unwrap_or(0),
            total_tokens,
            tools_count,
        );
        // Log thinking budget vs context ratio
        if let Some(budget) = dynamic_thinking_budget {
            let ratio = if total_tokens > 0 {
                budget as f32 / total_tokens as f32 * 100.0
            } else {
                0.0
            };
            crate::app_log!(
                "[AI][CTX] thinking_budget={}t / input~{}t = {:.0}%",
                budget,
                total_tokens,
                ratio
            );
        }
    }
    crate::app_log!(
        "[AI] Sending request to {} (Model: {})",
        url,
        request_body.model
    );
    if matches!(profile.provider, LLMProvider::QwenCli) && has_tool_heavy_context {
        crate::app_log!(
            "[Qwen] Tool-heavy context detected, clamped request budget: max_tokens={} thinking_budget={:?}",
            request_body.max_tokens,
            request_body.thinking_budget_tokens
        );
    }

    // Local providers (Ollama, LMStudio) have no timeout — large contexts with thinking models
    // can take several minutes. Cloud APIs keep 120s to detect hanging connections.
    let is_local = matches!(
        profile.provider,
        LLMProvider::Ollama | LLMProvider::LMStudio
    );
    let mut client_builder = reqwest::Client::builder();
    if !is_local {
        client_builder = client_builder.timeout(std::time::Duration::from_secs(120));
    }
    let client = client_builder
        .build()
        .map_err(|e| format!("Failed to build client: {}", e))?;

    let mut attempt = 0;
    let max_retries = 3;
    let response = loop {
        attempt += 1;
        if matches!(profile.provider, LLMProvider::QwenCli) {
            wait_for_qwen_request_slot(&profile.id, &app_handle).await;
            let status = if attempt == 1 {
                "Qwen: отправляю запрос..."
            } else {
                "Qwen: повторяю запрос..."
            };
            let _ = app_handle.emit("chat-status", status.to_string());
        }
        let res = client
            .post(&url)
            .headers(headers.clone())
            .json(&request_body)
            .send()
            .await;

        match res {
            Ok(r) if r.status().is_success() => {
                if matches!(profile.provider, LLMProvider::QwenCli) {
                    let _ = app_handle.emit(
                        "chat-status",
                        "Qwen: запрос принят, жду первый ответ...".to_string(),
                    );
                }
                break r;
            }
            Ok(r) if r.status().as_u16() == 500 && attempt < max_retries => {
                crate::app_log!(
                    "[AI][RETRY] Attempt {} failed with 500. Retrying in 2s...",
                    attempt
                );
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
            Ok(r) => {
                let status = r.status();
                let response_headers = r.headers().clone();
                let error_body = r.text().await.unwrap_or_default();
                crate::app_log!(
                    "[AI] API Error (Attempt {}): {} - {}",
                    attempt,
                    status,
                    error_body
                );
                if matches!(profile.provider, LLMProvider::QwenCli) && status.as_u16() == 429 {
                    let ctx = parse_qwen_rate_limit_context(&response_headers, &error_body);
                    if attempt < QWEN_MAX_429_ATTEMPTS {
                        if let Some(retry_delay) = qwen_retry_delay(attempt, &ctx) {
                            extend_qwen_cooldown(&profile.id, retry_delay);
                            let request_changed = reduce_qwen_request_pressure(
                                &mut request_body,
                                has_tool_heavy_context,
                                ctx.is_quota_exceeded,
                            );
                            crate::app_log!(
                                "[Qwen][RETRY] 429 on attempt {}. cooldown={}s quota={} hard_quota={} burst={} request_changed={} max_tokens={} thinking_budget={:?}",
                                attempt,
                                retry_delay.as_secs(),
                                ctx.is_quota_exceeded,
                                ctx.is_hard_quota_exhausted,
                                ctx.is_burst_limited,
                                request_changed,
                                request_body.max_tokens,
                                request_body.thinking_budget_tokens
                            );
                            let _ = app_handle
                                .emit("chat-status", format!("Qwen временно ограничил запросы."));
                            wait_with_chat_status(&app_handle, retry_delay, |secs| {
                                format!(
                                    "Qwen временно ограничил запросы. Повторю автоматически через {}с.",
                                    secs
                                )
                            })
                            .await;
                            let _ = app_handle
                                .emit("chat-status", "Qwen: повторяю запрос...".to_string());
                            continue;
                        }
                    }

                    if ctx.is_hard_quota_exhausted {
                        crate::app_log!(
                            "[Qwen][RETRY] 429 on attempt {} classified as hard quota exhaustion; failing fast",
                            attempt
                        );
                    } else {
                        crate::app_log!(
                            "[Qwen][RETRY] 429 on attempt {} exhausted short retries; returning error",
                            attempt
                        );
                    }

                    return Err(build_qwen_rate_limit_message(&ctx));
                }
                if status.as_u16() == 429 && attempt < max_retries {
                    let retry_after = extract_retry_after_secs(&response_headers).unwrap_or(10);
                    crate::app_log!(
                        "[AI][RETRY] 429 rate-limit (attempt {}, provider {:?}), waiting {}s...",
                        attempt,
                        profile.provider,
                        retry_after
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
                    continue;
                }
                // OpenRouter 400 "Developer instruction is not enabled" — model doesn't support system role
                if matches!(profile.provider, LLMProvider::OpenRouter)
                    && status.as_u16() == 400
                    && error_body.contains("Developer instruction is not enabled")
                {
                    let has_system = request_body.messages.iter().any(|m| m.role == "system");
                    if has_system {
                        crate::app_log!("[AI][RETRY] OpenRouter: модель '{}' не поддерживает system-сообщение. Конвертирую в user.", profile.model);
                        // Merge system content into the first user message
                        let system_content: String = request_body
                            .messages
                            .iter()
                            .filter(|m| m.role == "system")
                            .filter_map(|m| m.content.as_deref())
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        request_body.messages.retain(|m| m.role != "system");
                        if let Some(first_user) =
                            request_body.messages.iter_mut().find(|m| m.role == "user")
                        {
                            let original = first_user.content.clone().unwrap_or_default();
                            first_user.content =
                                Some(format!("{}\n\n---\n\n{}", system_content, original));
                        }
                        let _ = app_handle.emit("chat-chunk", "\n\n⚠️ Модель не поддерживает системный промпт — встраиваю инструкции в запрос.\n\n");
                        attempt = 0;
                        continue;
                    }
                    return Err(
                        "Модель не поддерживает системные инструкции (Developer instruction is not enabled). Попробуйте другую модель."
                            .to_string(),
                    );
                }
                // OpenRouter 429 exhausted all retries — extract human-readable message
                if matches!(profile.provider, LLMProvider::OpenRouter) && status.as_u16() == 429 {
                    let is_free_model = profile.model.ends_with(":free");
                    let hint = if is_free_model {
                        format!(
                            "Бесплатная модель `{}` временно перегружена на Google AI Studio (общий пул). \
                            Варианты решения:\n\
                            • Подождите 1–2 минуты и повторите\n\
                            • Добавьте свой Google AI Studio ключ: https://openrouter.ai/settings/integrations\n\
                            • Переключитесь на другую модель (например, `google/gemma-3-27b-it` без `:free`)",
                            profile.model
                        )
                    } else {
                        serde_json::from_str::<serde_json::Value>(&error_body)
                            .ok()
                            .and_then(|v| {
                                v["error"]["metadata"]["raw"]
                                    .as_str()
                                    .map(|s| s.to_string())
                            })
                            .unwrap_or_else(|| {
                                "Превышен лимит запросов. Попробуйте позже.".to_string()
                            })
                    };
                    return Err(hint);
                }
                if matches!(profile.provider, LLMProvider::OpenRouter)
                    && status.as_u16() == 401
                    && error_body.contains("No cookie auth credentials found")
                {
                    return Err(
                        "OpenRouter не получил API key. Проверьте поле API Key в профиле и сохраните его заново."
                            .to_string(),
                    );
                }
                if matches!(profile.provider, LLMProvider::OpenRouter)
                    && status.as_u16() == 404
                    && error_body.contains("No endpoints found that support tool use")
                {
                    if request_body.tools.is_some() {
                        crate::app_log!("[AI][RETRY] OpenRouter: модель '{}' не поддерживает tool use. Повторяю без инструментов.", profile.model);
                        request_body.tools = None;
                        let _ = app_handle.emit("chat-chunk", "\n\n⚠️ Модель не поддерживает инструменты — отправляю запрос без MCP-инструментов.\n\n");
                        attempt = 0;
                        continue;
                    }
                    return Err(
                        "Модель не поддерживает инструменты (tool use) на OpenRouter. Попробуйте другую модель."
                            .to_string(),
                    );
                }
                if matches!(profile.provider, LLMProvider::OllamaCloud)
                    && status.as_u16() == 403
                    && error_body.contains("requires a subscription")
                {
                    return Err(format!(
                        "Модель '{}' требует платной подписки Ollama Cloud. \
                        Оформите подписку на https://ollama.com/upgrade или выберите другую модель в профиле. \
                        Бесплатно доступны: gpt-oss:20b/120b, qwen3-coder:480b, qwen3-next:80b, kimi-k2-thinking, glm-4.6, minimax-m2 и другие.",
                        profile.model
                    ));
                }
                return Err(format!("API error {}: {}", status, error_body));
            }
            Err(e) if attempt < max_retries => {
                crate::app_log!(
                    "[AI][RETRY] Request failed (Attempt {}): {}. Retrying in 2s...",
                    attempt,
                    e
                );
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
            Err(e) => return Err(format!("Request failed after {} attempts: {}", attempt, e)),
        }
    };

    crate::app_log!("[AI] Response received. Status: {}", response.status());

    if matches!(profile.provider, LLMProvider::QwenCli) {
        let hdrs = response.headers();
        let limit = hdrs
            .get("x-ratelimit-limit-requests")
            .or_else(|| hdrs.get("x-ratelimit-requests-limit"))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());
        let remaining = hdrs
            .get("x-ratelimit-remaining-requests")
            .or_else(|| hdrs.get("x-ratelimit-requests-remaining"))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());
        let reset = hdrs
            .get("x-ratelimit-reset-requests")
            .or_else(|| hdrs.get("x-ratelimit-requests-reset"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if let (Some(limit), Some(remaining)) = (limit, remaining) {
            let used = limit.saturating_sub(remaining);
            let _ = crate::llm::cli_providers::qwen::QwenCliProvider::save_usage(
                &profile.id,
                used,
                limit,
                reset,
            );
        }
    }

    // === Non-streaming path (disable_streaming = true) ===
    if !use_stream {
        crate::app_log!("[AI] Non-streaming mode: waiting for full response...");
        let body = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;
        let resp: NonStreamResponse = serde_json::from_str(&body).map_err(|e| {
            format!(
                "Failed to parse non-stream response: {} body={}",
                e,
                &body[..body.len().min(200)]
            )
        })?;
        let choice = resp
            .choices
            .into_iter()
            .next()
            .ok_or("Empty response from API")?;
        let content = choice.message.content.unwrap_or_default();
        let raw_tool_calls = choice.message.tool_calls.unwrap_or_default();
        // Convert NonStreamToolCall → ToolCall, normalising arguments to valid JSON string.
        let tool_calls: Vec<ToolCall> = raw_tool_calls
            .into_iter()
            .map(|tc| ToolCall {
                id: tc.id,
                r#type: tc.r#type,
                function: ToolCallFunction {
                    name: tc.function.name,
                    arguments: tc
                        .function
                        .arguments
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "{}".to_string()),
                },
            })
            .collect();
        if !content.is_empty() {
            let _ = app_handle.emit("chat-status", "Выполнение...");
            let _ = app_handle.emit("chat-chunk", content.clone());
        }
        for (idx, tc) in tool_calls.iter().enumerate() {
            let _ = app_handle.emit(
                "tool-call-started",
                serde_json::json!({
                    "index": idx, "id": tc.id, "name": tc.function.name
                }),
            );
        }
        crate::app_log!(
            "[AI][RESP] non-stream content_chars={} tool_calls={}",
            content.len(),
            tool_calls.len()
        );
        return Ok(ApiMessage {
            role: "assistant".to_string(),
            content: if content.is_empty() {
                None
            } else {
                Some(content)
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id: None,
            name: None,
        });
    }

    let mut stream = response.bytes_stream();
    let mut byte_buffer = Vec::new();
    let mut full_content = String::new();
    let mut content_search_temp = String::new();
    let mut accumulated_tool_calls: Vec<ToolCall> = Vec::new();
    let mut announced_tool_calls = std::collections::HashSet::new();
    let mut is_thinking = false;
    let mut is_qwen_fn = false;
    let mut qwen_fn_buf = String::new();
    let mut has_switched_to_executing = false;
    let mut first_token_received = false;
    let start_gen_time = std::time::Instant::now();

    loop {
        let is_local = matches!(
            profile.provider,
            LLMProvider::Ollama | LLMProvider::LMStudio
        );
        let default_timeout = if is_local {
            300u32
        } else if matches!(profile.provider, LLMProvider::MiniMax) {
            120u32
        } else if matches!(profile.provider, LLMProvider::OllamaCloud) {
            // Ollama Cloud hosts thinking-models (kimi-k2-thinking, qwen3.5, glm-5 etc.)
            // that may stay silent for 60+ seconds before emitting first token.
            120u32
        } else {
            30u32
        };
        let chunk_timeout = profile.stream_timeout_secs.unwrap_or(default_timeout);
        let chunk_result = match tokio::time::timeout(
            std::time::Duration::from_secs(chunk_timeout as u64),
            stream.next(),
        )
        .await
        {
            Err(_) => {
                return Err(format!(
                    "Stream timeout: no data from API for {}s",
                    chunk_timeout
                ))
            }
            Ok(None) => break,
            Ok(Some(r)) => r,
        };
        if !first_token_received {
            first_token_received = true;
            let ttft = start_gen_time.elapsed().as_millis();
            crate::app_log!("[AI][TIMER] TTFT (Time to First Token): {} ms", ttft);
        }
        let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;
        byte_buffer.extend_from_slice(&chunk);

        while let Some(pos) = byte_buffer.windows(2).position(|w| w == b"\n\n") {
            let event_bytes = byte_buffer.drain(..pos + 2).collect::<Vec<u8>>();
            let event_str = String::from_utf8_lossy(&event_bytes);

            for line in event_str.lines() {
                if let Some(data) = line
                    .strip_prefix("data: ")
                    .or_else(|| line.strip_prefix("data:"))
                {
                    if data == "[DONE]" {
                        if !content_search_temp.is_empty() {
                            if is_thinking {
                                let _ = app_handle
                                    .emit("chat-thinking-chunk", content_search_temp.clone());
                            } else if !is_qwen_fn {
                                full_content.push_str(&content_search_temp);
                                let _ = app_handle.emit("chat-chunk", content_search_temp.clone());
                            }
                            content_search_temp.clear();
                        }
                        if is_qwen_fn && !qwen_fn_buf.is_empty() {
                            full_content.push_str(&qwen_fn_buf);
                            let _ = app_handle.emit("chat-chunk", qwen_fn_buf.clone());
                            qwen_fn_buf.clear();
                        }

                        if matches!(profile.provider, LLMProvider::QwenCli) {
                            crate::llm::cli_providers::qwen::QwenCliProvider::increment_request_count(&profile.id);
                        }
                        // === DIAGNOSTIC: log response type ===
                        if !accumulated_tool_calls.is_empty() {
                            let names: Vec<&str> = accumulated_tool_calls
                                .iter()
                                .map(|tc| tc.function.name.as_str())
                                .collect();
                            crate::app_log!(
                                "[AI][RESP] tool_calls={} names={:?} content_chars={}",
                                accumulated_tool_calls.len(),
                                names,
                                full_content.len()
                            );
                        } else if full_content.is_empty() {
                            crate::app_log!("[AI][RESP] EMPTY response (no tool_calls, no content) — likely thinking-only");
                        } else {
                            // Check if content mentions known tool names (hallucination signal)
                            let known_tools = [
                                "list_objects",
                                "get_object_structure",
                                "get_module_functions",
                                "find_symbol",
                                "search_code",
                                "find_references",
                                "benchmark",
                                "smart_find",
                                "stats",
                            ];
                            let hallucinated: Vec<&&str> = known_tools
                                .iter()
                                .filter(|&&t| full_content.contains(t))
                                .collect();
                            if hallucinated.is_empty() {
                                crate::app_log!(
                                    "[AI][RESP] text_only content_chars={}",
                                    full_content.len()
                                );
                            } else {
                                crate::app_log!("[AI][RESP] ⚠️ HALLUCINATION DETECTED: text_only but mentions tools {:?} — no actual tool_calls!", hallucinated);
                            }
                        }
                        // Ensure all accumulated arguments are valid JSON (provider safety).
                        for tc in &mut accumulated_tool_calls {
                            if serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                                .is_err()
                            {
                                crate::app_log!(
                                    "[AI][WARN] tool_call {} has invalid JSON arguments, resetting to {{}}",
                                    tc.id
                                );
                                tc.function.arguments = "{}".to_string();
                            }
                        }
                        return Ok(ApiMessage {
                            role: "assistant".to_string(),
                            content: if full_content.is_empty() {
                                None
                            } else {
                                Some(full_content)
                            },
                            tool_calls: if accumulated_tool_calls.is_empty() {
                                None
                            } else {
                                Some(accumulated_tool_calls)
                            },
                            tool_call_id: None,
                            name: None,
                        });
                    }

                    if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                        if let Some(choice) = chunk.choices.first() {
                            // Handle Qwen3 native reasoning_content field (enable_thinking=true)
                            if let Some(reasoning) = &choice.delta.reasoning_content {
                                if !reasoning.is_empty() {
                                    if !is_thinking {
                                        is_thinking = true;
                                        let _ = app_handle.emit("chat-status", "Размышляю...");
                                    }
                                    let _ =
                                        app_handle.emit("chat-thinking-chunk", reasoning.clone());
                                }
                            } else if is_thinking
                                && choice
                                    .delta
                                    .content
                                    .as_deref()
                                    .map(|c| !c.is_empty())
                                    .unwrap_or(false)
                            {
                                // Thinking phase ended, text phase started
                                is_thinking = false;
                                has_switched_to_executing = true;
                                let _ = app_handle.emit("chat-status", "Выполнение...");
                            }

                            if let Some(content) = &choice.delta.content {
                                if !has_switched_to_executing
                                    && !is_thinking
                                    && !content.trim().is_empty()
                                {
                                    let _ = app_handle.emit("chat-status", "Выполнение...");
                                    has_switched_to_executing = true;
                                }

                                content_search_temp.push_str(content);

                                loop {
                                    if is_qwen_fn {
                                        break;
                                    }

                                    if !is_thinking {
                                        // Detect <tool_call>JSON</tool_call> (Qwen/other model XML format)
                                        if let Some(tc_start) =
                                            content_search_temp.find("<tool_call>")
                                        {
                                            if tc_start > 0 {
                                                let text =
                                                    content_search_temp[..tc_start].to_string();
                                                full_content.push_str(&text);
                                                let _ = app_handle.emit("chat-chunk", text);
                                            }
                                            is_qwen_fn = true;
                                            // buffer includes the opening tag so we can detect </tool_call>
                                            qwen_fn_buf =
                                                content_search_temp[tc_start..].to_string();
                                            content_search_temp.clear();
                                            break;
                                        }

                                        if let Some(fn_start) =
                                            content_search_temp.find("<function=")
                                        {
                                            if fn_start > 0 {
                                                let text =
                                                    content_search_temp[..fn_start].to_string();
                                                full_content.push_str(&text);
                                                let _ = app_handle.emit("chat-chunk", text);
                                            }
                                            is_qwen_fn = true;
                                            qwen_fn_buf =
                                                content_search_temp[fn_start..].to_string();
                                            content_search_temp.clear();
                                            break;
                                        }

                                        if let Some(start_pos) =
                                            content_search_temp.find("<thinking>")
                                        {
                                            if start_pos > 0 {
                                                let text =
                                                    content_search_temp[..start_pos].to_string();
                                                full_content.push_str(&text);
                                                let _ = app_handle.emit("chat-chunk", text);
                                            }
                                            is_thinking = true;
                                            let _ = app_handle
                                                .emit("chat-status", "Планирование (EN)...");
                                            content_search_temp =
                                                content_search_temp[start_pos + 10..].to_string();
                                        } else if let Some(last_lt) = content_search_temp.rfind('<')
                                        {
                                            let after_lt =
                                                content_search_temp[last_lt..].chars().nth(1);
                                            let is_potential_tag = matches!(after_lt, Some(c) if c.is_alphabetic() || c == '/' || c == '?');
                                            let potential_tag_len =
                                                content_search_temp.len() - last_lt;

                                            if is_potential_tag && potential_tag_len < 15 {
                                                if last_lt > 0 {
                                                    // Strip stray </tool_call> tags that leaked into text content
                                                    let raw = &content_search_temp[..last_lt];
                                                    let text = raw
                                                        .replace("</tool_call>", "")
                                                        .replace("<tool_call>", "");
                                                    if !text.is_empty() {
                                                        full_content.push_str(&text);
                                                        let _ = app_handle.emit("chat-chunk", text);
                                                    }
                                                    content_search_temp =
                                                        content_search_temp[last_lt..].to_string();
                                                }
                                                break;
                                            } else {
                                                let text = content_search_temp
                                                    .replace("</tool_call>", "")
                                                    .replace("<tool_call>", "");
                                                if !text.is_empty() {
                                                    full_content.push_str(&text);
                                                    let _ = app_handle.emit("chat-chunk", text);
                                                }
                                                content_search_temp.clear();
                                                break;
                                            }
                                        } else {
                                            let text = content_search_temp
                                                .replace("</tool_call>", "")
                                                .replace("<tool_call>", "");
                                            if !text.is_empty() {
                                                full_content.push_str(&text);
                                                let _ = app_handle.emit("chat-chunk", text);
                                            }
                                            content_search_temp.clear();
                                            break;
                                        }
                                    } else {
                                        if let Some(end_pos) =
                                            content_search_temp.find("</thinking>")
                                        {
                                            if end_pos > 0 {
                                                let text =
                                                    content_search_temp[..end_pos].to_string();
                                                let _ =
                                                    app_handle.emit("chat-thinking-chunk", text);
                                            }
                                            is_thinking = false;
                                            has_switched_to_executing = true;
                                            let _ = app_handle.emit("chat-status", "Выполнение...");
                                            content_search_temp =
                                                content_search_temp[end_pos + 11..].to_string();
                                        } else if let Some(last_lt) = content_search_temp.rfind('<')
                                        {
                                            let potential_tag_len =
                                                content_search_temp.len() - last_lt;
                                            if potential_tag_len < 15 {
                                                if last_lt > 0 {
                                                    let text =
                                                        content_search_temp[..last_lt].to_string();
                                                    let _ = app_handle
                                                        .emit("chat-thinking-chunk", text);
                                                    content_search_temp =
                                                        content_search_temp[last_lt..].to_string();
                                                }
                                                break;
                                            } else {
                                                let _ = app_handle.emit(
                                                    "chat-thinking-chunk",
                                                    content_search_temp.clone(),
                                                );
                                                content_search_temp.clear();
                                                break;
                                            }
                                        } else {
                                            let _ = app_handle.emit(
                                                "chat-thinking-chunk",
                                                content_search_temp.clone(),
                                            );
                                            content_search_temp.clear();
                                            break;
                                        }
                                    }
                                }
                            }

                            if is_qwen_fn {
                                qwen_fn_buf.push_str(&content_search_temp);
                                content_search_temp.clear();
                                // Handle <tool_call>JSON</tool_call> format (Qwen/other models)
                                if qwen_fn_buf.starts_with("<tool_call>") {
                                    if let Some(end_pos) = qwen_fn_buf.find("</tool_call>") {
                                        let json_content =
                                            qwen_fn_buf[11..end_pos].trim().to_string();
                                        let remainder = qwen_fn_buf[end_pos + 12..].to_string();
                                        qwen_fn_buf.clear();
                                        is_qwen_fn = false;
                                        if let Ok(parsed) =
                                            serde_json::from_str::<serde_json::Value>(&json_content)
                                        {
                                            let fn_name = parsed
                                                .get("name")
                                                .and_then(|n| n.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            let args = parsed
                                                .get("arguments")
                                                .map(|a| {
                                                    if a.is_object() {
                                                        a.to_string()
                                                    } else {
                                                        a.as_str().unwrap_or("{}").to_string()
                                                    }
                                                })
                                                .unwrap_or_default();
                                            if !fn_name.is_empty() {
                                                let tc_idx = accumulated_tool_calls.len();
                                                let tc = ToolCall {
                                                    id: format!("tc_xml_{}", tc_idx),
                                                    r#type: "function".to_string(),
                                                    function: ToolCallFunction {
                                                        name: fn_name.clone(),
                                                        arguments: args,
                                                    },
                                                };
                                                let _ = app_handle.emit("tool-call-started", serde_json::json!({
                                                    "index": tc_idx, "id": tc.id, "name": fn_name
                                                }));
                                                accumulated_tool_calls.push(tc);
                                            }
                                        }
                                        content_search_temp = remainder;
                                    }
                                // Handle <function=name>...</function> format (Qwen inline)
                                } else if let Some(end_pos) = qwen_fn_buf.find("</function>") {
                                    let full_block =
                                        qwen_fn_buf[..end_pos + "</function>".len()].to_string();
                                    let remainder =
                                        qwen_fn_buf[end_pos + "</function>".len()..].to_string();
                                    qwen_fn_buf.clear();
                                    is_qwen_fn = false;
                                    if let Some(fn_name_end) = full_block[10..].find('>') {
                                        let fn_name = full_block[10..10 + fn_name_end].to_string();
                                        let mut args_map = serde_json::Map::new();
                                        let body = &full_block[10 + fn_name_end + 1..];
                                        let mut pos = 0;
                                        while let Some(p_start) = body[pos..].find("<parameter=") {
                                            let abs = pos + p_start;
                                            if let Some(close_gt) = body[abs..].find('>') {
                                                let p_name =
                                                    body[abs + 11..abs + close_gt].to_string();
                                                let v_start = abs + close_gt + 1;
                                                if let Some(v_end) =
                                                    body[v_start..].find("</parameter>")
                                                {
                                                    let value = body[v_start..v_start + v_end]
                                                        .trim()
                                                        .to_string();
                                                    args_map.insert(
                                                        p_name,
                                                        serde_json::Value::String(value),
                                                    );
                                                    pos = v_start + v_end + 12;
                                                } else {
                                                    break;
                                                }
                                            } else {
                                                break;
                                            }
                                        }
                                        let args_json = serde_json::to_string(&args_map)
                                            .unwrap_or("{}".to_string());
                                        let tc_idx = accumulated_tool_calls.len();
                                        let tc = ToolCall {
                                            id: format!("qwen_fn_{}", tc_idx),
                                            r#type: "function".to_string(),
                                            function: ToolCallFunction {
                                                name: fn_name.clone(),
                                                arguments: args_json,
                                            },
                                        };
                                        let _ = app_handle.emit(
                                            "tool-call-started",
                                            serde_json::json!({
                                                "index": tc_idx, "id": tc.id, "name": fn_name
                                            }),
                                        );
                                        accumulated_tool_calls.push(tc);
                                    }
                                    content_search_temp = remainder;
                                }
                            }

                            if let Some(tool_calls) = &choice.delta.tool_calls {
                                for tc_delta in tool_calls {
                                    let idx = tc_delta.index.unwrap_or(0);

                                    while accumulated_tool_calls.len() <= idx {
                                        accumulated_tool_calls.push(ToolCall {
                                            id: String::new(),
                                            r#type: "function".to_string(),
                                            function: ToolCallFunction {
                                                name: String::new(),
                                                arguments: String::new(),
                                            },
                                        });
                                    }

                                    let tc = &mut accumulated_tool_calls[idx];
                                    // ID приходит только в первом delta — записываем только если ещё не установлен
                                    if let Some(id) = &tc_delta.id {
                                        if tc.id.is_empty() {
                                            tc.id.push_str(id);
                                        }
                                    }
                                    if let Some(f) = &tc_delta.function {
                                        if let Some(name) = &f.name {
                                            tc.function.name.push_str(name);
                                        }
                                        if let Some(args) = &f.arguments {
                                            tc.function.arguments.push_str(args);
                                            let _ = app_handle.emit(
                                                "tool-call-progress",
                                                serde_json::json!({
                                                    "index": idx,
                                                    "arguments": args
                                                }),
                                            );
                                        }
                                    }

                                    if !announced_tool_calls.contains(&idx)
                                        && (!tc.id.is_empty() || !tc.function.name.is_empty())
                                    {
                                        let _ = app_handle.emit(
                                            "tool-call-started",
                                            serde_json::json!({
                                                "index": idx,
                                                "id": tc.id,
                                                "name": tc.function.name
                                            }),
                                        );
                                        announced_tool_calls.insert(idx);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if !content_search_temp.is_empty() {
        if is_thinking {
            let _ = app_handle.emit("chat-thinking-chunk", content_search_temp.clone());
        } else if !is_qwen_fn {
            full_content.push_str(&content_search_temp);
            let _ = app_handle.emit("chat-chunk", content_search_temp.clone());
        }
        content_search_temp.clear();
    }
    if is_qwen_fn && !qwen_fn_buf.is_empty() {
        full_content.push_str(&qwen_fn_buf);
        let _ = app_handle.emit("chat-chunk", qwen_fn_buf.clone());
        qwen_fn_buf.clear();
    }

    Ok(ApiMessage {
        role: "assistant".to_string(),
        content: if full_content.is_empty() {
            None
        } else {
            Some(full_content)
        },
        tool_calls: if accumulated_tool_calls.is_empty() {
            None
        } else {
            Some(accumulated_tool_calls)
        },
        tool_call_id: None,
        name: None,
    })
}

/// Helper to extract BSL code blocks from text
pub fn extract_bsl_code(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut start_pos = 0;

    while let Some(start) = text[start_pos..].find("```bsl") {
        let actual_start = start_pos + start + 6;
        if let Some(end) = text[actual_start..].find("```") {
            let code = &text[actual_start..actual_start + end];
            blocks.push(code.trim().to_string());
            start_pos = actual_start + end + 3;
        } else {
            break;
        }
    }

    start_pos = 0;
    while let Some(start) = text[start_pos..].find("```1c") {
        let actual_start = start_pos + start + 5;
        if let Some(end) = text[actual_start..].find("```") {
            let code = &text[actual_start..actual_start + end];
            blocks.push(code.trim().to_string());
            start_pos = actual_start + end + 3;
        } else {
            break;
        }
    }

    blocks
}

/// Fetch models from provider
pub async fn fetch_models(
    profile: &crate::llm_profiles::LLMProfile,
) -> Result<Vec<String>, String> {
    let api_key = resolve_profile_api_key(profile)?;
    let raw_url = profile.get_base_url();
    // Normalize Ollama URL: auto-add /v1 if missing
    let base_url = {
        let trimmed = raw_url.trim_end_matches('/');
        if matches!(
            profile.provider,
            LLMProvider::Ollama | LLMProvider::OllamaCloud | LLMProvider::LMStudio
        ) && !trimmed.ends_with("/v1")
            && !trimmed.ends_with("/chat/completions")
        {
            format!("{}/v1", trimmed)
        } else {
            trimmed.to_string()
        }
    };
    let url = if base_url.ends_with("/chat/completions") {
        base_url.replace("/chat/completions", "/models")
    } else {
        format!("{}/models", base_url.trim_end_matches('/'))
    };

    let client = reqwest::Client::new();
    let mut builder = client.get(&url);
    builder = builder.header(CONTENT_TYPE, "application/json");

    if !api_key.is_empty() {
        builder = builder.header(AUTHORIZATION, format!("Bearer {}", api_key));
    }

    if matches!(profile.provider, LLMProvider::OpenRouter) {
        builder = builder
            .header("HTTP-Referer", "https://mini-ai-1c.local")
            .header("X-Title", "Mini AI 1C Agent");
    }

    let response = builder.send().await.map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch models: {}", response.status()));
    }

    let data: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let mut models = Vec::new();
    if let Some(list) = data.get("data").and_then(|d| d.as_array()) {
        for item in list {
            if let Some(id) = item.get("id").and_then(|id| id.as_str()) {
                models.push(id.to_string());
            }
        }
    }

    models.sort();
    Ok(models)
}

/// Test connection
pub async fn test_connection(profile: &crate::llm_profiles::LLMProfile) -> Result<String, String> {
    match fetch_models(profile).await {
        Ok(models) => Ok(format!("Success! Found {} models.", models.len())),
        Err(e) => Err(format!("Connection failed: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_quota_exceeded_from_qwen_body() {
        let headers = HeaderMap::new();
        let ctx = parse_qwen_rate_limit_context(
            &headers,
            r#"{"error":{"code":"insufficient_quota","message":"You exceeded your current quota."}}"#,
        );

        assert!(ctx.is_quota_exceeded);
        assert!(!ctx.is_hard_quota_exhausted);
        assert!(!ctx.is_burst_limited);
    }

    #[test]
    fn detects_tool_heavy_context_from_recent_tool_messages() {
        let messages = vec![
            ApiMessage {
                role: "user".to_string(),
                content: Some("test".to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ApiMessage {
                role: "tool".to_string(),
                content: Some("cached".to_string()),
                tool_calls: None,
                tool_call_id: Some("tc_1".to_string()),
                name: Some("get_object_structure".to_string()),
            },
        ];

        assert!(qwen_has_tool_heavy_context(&messages));
    }

    #[test]
    fn clamps_qwen_budget_for_tool_heavy_context() {
        assert_eq!(qwen_max_tokens(65_536, true), 16_384);
        assert_eq!(qwen_thinking_budget(20_000, true), 8_192);
    }

    #[test]
    fn insufficient_quota_uses_short_retry_schedule() {
        let ctx = QwenRateLimitContext {
            retry_after_secs: None,
            message: Some("quota".to_string()),
            is_quota_exceeded: true,
            is_hard_quota_exhausted: false,
            is_burst_limited: false,
        };

        assert_eq!(qwen_retry_delay(1, &ctx), Some(Duration::from_secs(2)));
        assert_eq!(qwen_retry_delay(2, &ctx), Some(Duration::from_secs(4)));
    }

    #[test]
    fn hard_quota_exhaustion_does_not_schedule_retry() {
        let ctx = QwenRateLimitContext {
            retry_after_secs: None,
            message: Some("free quota".to_string()),
            is_quota_exceeded: true,
            is_hard_quota_exhausted: true,
            is_burst_limited: false,
        };

        assert_eq!(qwen_retry_delay(1, &ctx), None);
    }

    #[test]
    fn burst_limit_uses_short_retry_schedule() {
        let ctx = QwenRateLimitContext {
            retry_after_secs: None,
            message: Some("burst".to_string()),
            is_quota_exceeded: false,
            is_hard_quota_exhausted: false,
            is_burst_limited: true,
        };

        assert_eq!(qwen_retry_delay(1, &ctx), Some(Duration::from_secs(2)));
        assert_eq!(qwen_retry_delay(2, &ctx), Some(Duration::from_secs(4)));
    }

    #[test]
    fn retry_after_is_clamped_to_short_wait() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("30"));

        let ctx =
            parse_qwen_rate_limit_context(&headers, r#"{"error":{"message":"Too many requests"}}"#);

        assert_eq!(ctx.retry_after_secs, Some(QWEN_MAX_RETRY_DELAY_SECS));
        assert_eq!(
            qwen_retry_delay(1, &ctx),
            Some(Duration::from_secs(QWEN_MAX_RETRY_DELAY_SECS))
        );
    }

    #[test]
    fn reduces_request_pressure_for_quota_retries() {
        let mut request = ChatRequest {
            model: "qwen3-max".to_string(),
            messages: Vec::new(),
            stream: true,
            temperature: 1.0,
            max_tokens: 65_536,
            tools: None,
            enable_thinking: Some(true),
            thinking_budget_tokens: Some(24_000),
        };

        let changed = reduce_qwen_request_pressure(&mut request, true, true);

        assert!(changed);
        assert_eq!(request.max_tokens, 8_192);
        assert_eq!(request.thinking_budget_tokens, Some(4_096));
    }
}
