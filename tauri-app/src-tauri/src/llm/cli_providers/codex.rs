//! OpenAI Codex CLI provider — OAuth2+PKCE browser-redirect flow
//!
//! Auth flow:
//!   1. `auth_start` — generates PKCE, starts local callback server on port 1455,
//!      returns browser auth URL
//!   2. User opens URL in browser, authorises → browser redirects to localhost:1455/auth/callback
//!   3. `auth_poll` — checks if callback received; if so, exchanges code for tokens
//!   4. Frontend calls `cli_save_token` to persist tokens

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use lazy_static::lazy_static;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use super::{CliAuthInitResponse, CliAuthStatus, CliStatus, CliUsageWindow};

// ─── Constants ─────────────────────────────────────────────────────────────

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const REDIRECT_PORT: u16 = 1455;
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const SCOPE: &str = "openid profile email offline_access";
const FIVE_HOUR_WINDOW_MINUTES: u32 = 5 * 60;
const WEEKLY_WINDOW_MINUTES: u32 = 7 * 24 * 60;
const FIVE_HOUR_WINDOW_SECONDS: i64 = (FIVE_HOUR_WINDOW_MINUTES as i64) * 60;
const WEEKLY_WINDOW_SECONDS: i64 = (WEEKLY_WINDOW_MINUTES as i64) * 60;

// ─── Callback State ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum CallbackResult {
    Pending,
    Success { code: String, state: Option<String> },
    Error(String),
}

lazy_static! {
    static ref CALLBACK: Mutex<CallbackResult> = Mutex::new(CallbackResult::Pending);
}

fn reset_callback() {
    if let Ok(mut cb) = CALLBACK.lock() {
        *cb = CallbackResult::Pending;
    }
}

fn set_callback(result: CallbackResult) {
    if let Ok(mut cb) = CALLBACK.lock() {
        *cb = result;
    }
}

fn read_callback() -> CallbackResult {
    CALLBACK
        .lock()
        .map(|cb| cb.clone())
        .unwrap_or(CallbackResult::Error("Lock error".to_string()))
}

// ─── PKCE Helpers ───────────────────────────────────────────────────────────

fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_challenge(code_verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(hash)
}

fn random_state() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn safe_error_detail(body: &str) -> Option<String> {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok()?;

    parsed
        .get("error_description")
        .and_then(|value| value.as_str())
        .or_else(|| parsed.get("message").and_then(|value| value.as_str()))
        .or_else(|| parsed.get("error").and_then(|value| value.as_str()))
        .or_else(|| {
            parsed
                .get("error")
                .and_then(|value| value.get("message"))
                .and_then(|value| value.as_str())
        })
        .map(|value| value.replace(['\r', '\n'], " "))
}

fn safe_response_summary(status: reqwest::StatusCode, body: &str) -> String {
    format!("status={} body_len={}", status.as_u16(), body.len())
}

fn json_value_as_f32(value: Option<&serde_json::Value>) -> Option<f32> {
    match value? {
        serde_json::Value::Number(number) => number.as_f64().map(|value| value as f32),
        serde_json::Value::String(text) => text.trim().parse::<f32>().ok(),
        _ => None,
    }
}

fn json_value_as_i64(value: Option<&serde_json::Value>) -> Option<i64> {
    match value? {
        serde_json::Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok())),
        serde_json::Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

// ─── Callback HTTP Server ───────────────────────────────────────────────────

/// Starts a one-shot local HTTP server on port 1455 to receive the OAuth callback.
/// Stores the received code (or error) in CALLBACK global state.
fn start_callback_server() {
    tokio::spawn(async move {
        let listener = match TcpListener::bind(format!("127.0.0.1:{}", REDIRECT_PORT)).await {
            Ok(l) => l,
            Err(e) => {
                crate::app_log!(force: true, "[Codex] Failed to bind callback server on port {}: {}", REDIRECT_PORT, e);
                set_callback(CallbackResult::Error(format!(
                    "Не удалось запустить сервер авторизации (порт {} занят): {}",
                    REDIRECT_PORT, e
                )));
                return;
            }
        };

        crate::app_log!(force: true, "[Codex] Callback server listening on port {}", REDIRECT_PORT);

        match listener.accept().await {
            Ok((mut stream, _addr)) => {
                let mut reader = BufReader::new(&mut stream);
                let mut request_line = String::new();
                let _ = reader.read_line(&mut request_line).await;

                crate::app_log!(force: true, "[Codex] OAuth callback request received");

                // Parse: GET /auth/callback?code=...&state=... HTTP/1.1
                let callback_url = request_line.split_whitespace().nth(1).and_then(|path| {
                    let full = format!("http://localhost{}", path);
                    url::Url::parse(&full).ok()
                });
                let code = callback_url.as_ref().and_then(|u| {
                    u.query_pairs()
                        .find(|(k, _)| k == "code")
                        .map(|(_, v)| v.to_string())
                });
                let state = callback_url.as_ref().and_then(|u| {
                    u.query_pairs()
                        .find(|(k, _)| k == "state")
                        .map(|(_, v)| v.to_string())
                });

                let response_html = if code.is_some() {
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
                    <html><head><meta charset=\"utf-8\"></head><body style=\"font-family:sans-serif;text-align:center;padding:40px\">\
                    <h2>&#10003; Авторизация успешна!</h2>\
                    <p>Вернитесь в приложение Mini AI 1C.</p>\
                    <script>setTimeout(()=>window.close(),2000);</script>\
                    </body></html>"
                } else {
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
                    <html><head><meta charset=\"utf-8\"></head><body style=\"font-family:sans-serif;text-align:center;padding:40px\">\
                    <h2>&#10007; Ошибка авторизации</h2>\
                    <p>Код авторизации не получен. Попробуйте снова.</p>\
                    </body></html>"
                };

                let _ = stream.write_all(response_html.as_bytes()).await;
                let _ = stream.flush().await;

                match code {
                    Some(c) => {
                        crate::app_log!(force: true, "[Codex] Auth code received (len={})", c.len());
                        set_callback(CallbackResult::Success { code: c, state });
                    }
                    None => {
                        set_callback(CallbackResult::Error(
                            "No authorization code in callback".to_string(),
                        ));
                    }
                }
            }
            Err(e) => {
                crate::app_log!(force: true, "[Codex] Callback server accept error: {}", e);
                set_callback(CallbackResult::Error(format!(
                    "Ошибка сервера авторизации: {}",
                    e
                )));
            }
        }
    });
}

// ─── Token exchange ─────────────────────────────────────────────────────────

async fn exchange_code(code: &str, code_verifier: &str) -> Result<CliAuthStatus, String> {
    let client = crate::http_client::http_client_builder()?
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let params = [
        ("client_id", CLIENT_ID),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
        ("code_verifier", code_verifier),
    ];

    let resp = client
        .post(TOKEN_URL)
        .form(&params)
        .header("Accept", "application/json")
        .header(
            "User-Agent",
            "codex_cli_rs/0.114.0 (Windows NT 10.0; x86_64)",
        )
        .send()
        .await
        .map_err(|e| format!("Ошибка сети при обмене кода: {}", e))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    crate::app_log!(
        force: true,
        "[Codex] Token exchange response: {}",
        safe_response_summary(status, &body)
    );

    if !status.is_success() {
        return Ok(CliAuthStatus::Error(format!(
            "Ошибка получения токена ({}): {}",
            status.as_u16(),
            safe_error_detail(&body).unwrap_or_else(|| "подробности скрыты".to_string())
        )));
    }

    let data: CodexTokenResponse =
        serde_json::from_str(&body).map_err(|e| format!("Ошибка разбора ответа токена: {}", e))?;

    let expires_at = Utc::now() + Duration::seconds(data.expires_in.unwrap_or(3600) as i64);

    // Extract account_id from id_token for ChatGPT-Account-Id header
    crate::app_log!(force: true, "[Codex] id_token present: {}", data.id_token.is_some());
    let account_id = data
        .id_token
        .as_deref()
        .and_then(extract_account_id_from_id_token);

    Ok(CliAuthStatus::Authorized {
        access_token: data.access_token,
        refresh_token: data.refresh_token,
        expires_at: expires_at.timestamp() as u64,
        resource_url: account_id, // repurposed: stores ChatGPT account_id
    })
}

// ─── Provider ──────────────────────────────────────────────────────────────

pub struct CodexCliProvider;

impl CodexCliProvider {
    // ── Auth ─────────────────────────────────────────────────────────────────

    pub async fn auth_start() -> Result<CliAuthInitResponse, String> {
        // Reset any previous callback result
        reset_callback();

        let code_verifier = generate_code_verifier();
        let code_challenge = generate_code_challenge(&code_verifier);
        let state = random_state();

        crate::app_log!(force: true, "[Codex] auth_start: PKCE challenge ready, starting callback server...");

        // Start callback server before returning URL
        start_callback_server();

        // Build browser auth URL
        let params: Vec<(&str, &str)> = vec![
            ("client_id", CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", REDIRECT_URI),
            ("scope", SCOPE),
            ("code_challenge", &code_challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            ("codex_cli_simplified_flow", "true"),
            ("id_token_add_organizations", "true"),
            ("originator", "codex_cli_rs"),
        ];

        let query = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        let auth_url = format!("{}?{}", AUTH_URL, query);

        crate::app_log!(force: true, "[Codex] auth_start: auth URL ready");

        Ok(CliAuthInitResponse {
            device_code: state,       // repurposed as session identifier
            user_code: String::new(), // not used in browser redirect flow
            verification_url: auth_url,
            expires_in: 300,
            poll_interval: 2,
            code_verifier: Some(code_verifier),
        })
    }

    pub async fn auth_poll(
        device_code: &str,
        code_verifier: Option<&str>,
    ) -> Result<CliAuthStatus, String> {
        match read_callback() {
            CallbackResult::Pending => Ok(CliAuthStatus::Pending),
            CallbackResult::Error(e) => Ok(CliAuthStatus::Error(e)),
            CallbackResult::Success { code, state } => {
                let verifier = code_verifier.unwrap_or("");
                if verifier.is_empty() {
                    return Ok(CliAuthStatus::Error("PKCE verifier missing".to_string()));
                }
                if state.as_deref() != Some(device_code) {
                    crate::app_log!(
                        force: true,
                        "[Codex] auth_poll: callback state mismatch (has_state={})",
                        state.is_some()
                    );
                    return Ok(CliAuthStatus::Error(
                        "Состояние OAuth-сессии не совпало. Повторите вход через браузер."
                            .to_string(),
                    ));
                }
                crate::app_log!(force: true, "[Codex] auth_poll: exchanging code for token...");
                exchange_code(&code, verifier).await
            }
        }
    }

    // ── Token storage (file-based AES-GCM, avoids Windows Credential Manager size limit) ──

    fn token_file_path(profile_id: &str) -> std::path::PathBuf {
        crate::settings::get_settings_dir().join(format!("codex-token-{}.dat", profile_id))
    }

    fn sessions_dir() -> Option<std::path::PathBuf> {
        dirs::home_dir().map(|path| path.join(".codex").join("sessions"))
    }

    fn collect_rollout_files(
        dir: &std::path::Path,
        files: &mut Vec<std::path::PathBuf>,
    ) -> Result<(), String> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) => return Err(err.to_string()),
        };

        for entry in entries {
            let entry = entry.map_err(|err| err.to_string())?;
            let path = entry.path();

            if path.is_dir() {
                Self::collect_rollout_files(&path, files)?;
                continue;
            }

            let is_rollout = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
                .unwrap_or(false);

            if is_rollout {
                files.push(path);
            }
        }

        Ok(())
    }

    fn build_usage_window_identity(window_minutes: u32) -> (String, String) {
        match window_minutes {
            FIVE_HOUR_WINDOW_MINUTES => ("5h".to_string(), "5ч".to_string()),
            WEEKLY_WINDOW_MINUTES => ("weekly".to_string(), "7д".to_string()),
            other => (format!("{}m", other), format!("{}м", other)),
        }
    }

    fn resolve_rate_limit_reset_at(
        event_timestamp: Option<&DateTime<Utc>>,
        rate_limit: &CodexSessionRateLimit,
    ) -> Option<String> {
        rate_limit
            .resets_at
            .and_then(|timestamp| {
                DateTime::<Utc>::from_timestamp(timestamp, 0).map(|dt| dt.to_rfc3339())
            })
            .or_else(|| {
                rate_limit.resets_in_seconds.and_then(|seconds| {
                    event_timestamp
                        .cloned()
                        .map(|timestamp| timestamp + Duration::seconds(seconds))
                        .map(|dt| dt.to_rfc3339())
                })
            })
    }

    fn build_usage_window(
        event_timestamp: Option<&DateTime<Utc>>,
        rate_limit: &CodexSessionRateLimit,
    ) -> CliUsageWindow {
        let used_percent = rate_limit.used_percent.clamp(0.0, 100.0);
        let remaining_percent = (100.0 - used_percent).clamp(0.0, 100.0);
        let (key, label) = Self::build_usage_window_identity(rate_limit.window_minutes);

        CliUsageWindow {
            key,
            label,
            used_percent,
            remaining_percent,
            window_minutes: rate_limit.window_minutes,
            resets_at: Self::resolve_rate_limit_reset_at(event_timestamp, rate_limit),
        }
    }

    fn resolve_api_rate_limit_reset_at(rate_limit: &CodexApiUsageWindow) -> Option<String> {
        json_value_as_i64(rate_limit.reset_at.as_ref())
            .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
            .map(|dt| dt.to_rfc3339())
            .or_else(|| {
                json_value_as_i64(rate_limit.reset_after_seconds.as_ref())
                    .map(|seconds| Utc::now() + Duration::seconds(seconds))
                    .map(|dt| dt.to_rfc3339())
            })
    }

    fn build_usage_window_from_api(
        rate_limit: &CodexApiRateLimit,
        window: &CodexApiUsageWindow,
        fallback_window_minutes: u32,
    ) -> Option<CliUsageWindow> {
        let used_percent = json_value_as_f32(window.used_percent.as_ref())
            .or_else(|| {
                if rate_limit.limit_reached.unwrap_or(false)
                    || matches!(rate_limit.allowed, Some(false))
                {
                    Some(100.0)
                } else {
                    None
                }
            })?
            .clamp(0.0, 100.0);

        let window_minutes = json_value_as_i64(window.limit_window_seconds.as_ref())
            .and_then(|seconds| {
                if seconds > 0 {
                    u32::try_from(seconds / 60).ok()
                } else {
                    None
                }
            })
            .unwrap_or(fallback_window_minutes);

        let (key, label) = Self::build_usage_window_identity(window_minutes);
        Some(CliUsageWindow {
            key,
            label,
            used_percent,
            remaining_percent: (100.0 - used_percent).clamp(0.0, 100.0),
            window_minutes,
            resets_at: Self::resolve_api_rate_limit_reset_at(window),
        })
    }

    fn classify_api_usage_windows(
        rate_limit: &CodexApiRateLimit,
    ) -> (Option<&CodexApiUsageWindow>, Option<&CodexApiUsageWindow>) {
        let primary = rate_limit.primary_window.as_ref();
        let secondary = rate_limit.secondary_window.as_ref();

        let mut five_hour_window = [primary, secondary].into_iter().flatten().find(|window| {
            json_value_as_i64(window.limit_window_seconds.as_ref())
                == Some(FIVE_HOUR_WINDOW_SECONDS)
        });
        let mut weekly_window = [primary, secondary].into_iter().flatten().find(|window| {
            json_value_as_i64(window.limit_window_seconds.as_ref()) == Some(WEEKLY_WINDOW_SECONDS)
        });

        if five_hour_window.is_none() {
            five_hour_window = primary.or(secondary);
        }
        if weekly_window.is_none() {
            weekly_window = secondary
                .filter(|window| {
                    five_hour_window.map_or(true, |chosen| !std::ptr::eq(*window, chosen))
                })
                .or_else(|| {
                    primary.filter(|window| {
                        five_hour_window.map_or(true, |chosen| !std::ptr::eq(*window, chosen))
                    })
                });
        }

        (five_hour_window, weekly_window)
    }

    fn extract_usage_snapshot_from_api_payload(
        payload: CodexApiUsagePayload,
    ) -> Option<(Vec<CliUsageWindow>, Option<String>)> {
        let rate_limit = payload.rate_limit?;
        let (five_hour_window, weekly_window) = Self::classify_api_usage_windows(&rate_limit);

        let mut windows = Vec::new();
        if let Some(window) = five_hour_window.and_then(|window| {
            Self::build_usage_window_from_api(&rate_limit, window, FIVE_HOUR_WINDOW_MINUTES)
        }) {
            windows.push(window);
        }
        if let Some(window) = weekly_window.and_then(|window| {
            Self::build_usage_window_from_api(&rate_limit, window, WEEKLY_WINDOW_MINUTES)
        }) {
            windows.push(window);
        }

        if windows.is_empty() {
            return None;
        }

        windows.sort_by_key(|window| window.window_minutes);
        Some((windows, payload.plan_type))
    }

    fn extract_usage_snapshot_from_line(
        line: &str,
    ) -> Option<(Vec<CliUsageWindow>, Option<String>)> {
        let event: CodexSessionEvent = serde_json::from_str(line).ok()?;
        let payload = event.payload?;
        if payload.event_type.as_deref()? != "token_count" {
            return None;
        }

        let rate_limits = payload.rate_limits?;
        let event_timestamp = event
            .timestamp
            .as_deref()
            .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).ok())
            .map(|timestamp| timestamp.with_timezone(&Utc));

        let mut windows = Vec::new();
        if let Some(primary) = rate_limits.primary.as_ref() {
            windows.push(Self::build_usage_window(event_timestamp.as_ref(), primary));
        }
        if let Some(secondary) = rate_limits.secondary.as_ref() {
            windows.push(Self::build_usage_window(
                event_timestamp.as_ref(),
                secondary,
            ));
        }

        if windows.is_empty() {
            return None;
        }

        windows.sort_by_key(|window| window.window_minutes);
        Some((windows, rate_limits.plan_type))
    }

    fn usage_snapshot() -> Result<Option<(Vec<CliUsageWindow>, Option<String>)>, String> {
        let sessions_dir = match Self::sessions_dir() {
            Some(path) if path.exists() => path,
            _ => return Ok(None),
        };

        let mut files = Vec::new();
        Self::collect_rollout_files(&sessions_dir, &mut files)?;
        files.sort_by_key(|path| {
            path.metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
        });
        files.reverse();

        for file in files.into_iter().take(50) {
            let content = match std::fs::read_to_string(&file) {
                Ok(content) => content,
                Err(_) => continue,
            };

            for line in content.lines().rev() {
                if !line.contains("\"type\":\"token_count\"") || !line.contains("\"rate_limits\"") {
                    continue;
                }

                if let Some(snapshot) = Self::extract_usage_snapshot_from_line(line) {
                    return Ok(Some(snapshot));
                }
            }
        }

        Ok(None)
    }

    async fn send_usage_request(
        access_token: &str,
        account_id: &str,
    ) -> Result<(reqwest::StatusCode, String), String> {
        let client = crate::http_client::http_client_builder()?
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;

        let response = client
            .get(CODEX_USAGE_URL)
            .header("Accept", "application/json")
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Chatgpt-Account-Id", account_id)
            .header(
                "User-Agent",
                "codex_cli_rs/0.114.0 (Windows NT 10.0; x86_64) WindowsTerminal",
            )
            .send()
            .await
            .map_err(|e| format!("Ошибка сети при получении лимитов Codex: {}", e))?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Ok((status, body))
    }

    async fn fetch_usage_snapshot_from_api(
        profile_id: &str,
    ) -> Result<Option<(Vec<CliUsageWindow>, Option<String>)>, String> {
        let token_info = Self::get_token(profile_id)?;
        let (mut access_token, refresh_token, _expires_at, mut account_id) = match token_info {
            Some(token_info) => token_info,
            None => return Ok(None),
        };

        let account_id_value = account_id.clone().ok_or_else(|| {
            "Отсутствует ChatGPT account id для запроса лимитов Codex".to_string()
        })?;

        let (mut status, mut body) =
            Self::send_usage_request(&access_token, &account_id_value).await?;
        crate::app_log!(
            force: true,
            "[Codex] Usage response: {}",
            safe_response_summary(status, &body)
        );

        if status.as_u16() == 401 {
            if let Some(refresh_token) = refresh_token.as_deref() {
                crate::app_log!(
                    force: true,
                    "[Codex] Usage API returned 401, attempting token refresh for profile {}",
                    profile_id
                );
                Self::refresh_access_token(profile_id, refresh_token).await?;
                if let Some((new_access_token, _, _, new_account_id)) = Self::get_token(profile_id)?
                {
                    access_token = new_access_token;
                    account_id = new_account_id.or(account_id);
                    let refreshed_account_id = account_id.ok_or_else(|| {
                        "Отсутствует ChatGPT account id после обновления токена Codex".to_string()
                    })?;
                    let (retried_status, retried_body) =
                        Self::send_usage_request(&access_token, &refreshed_account_id).await?;
                    status = retried_status;
                    body = retried_body;
                    crate::app_log!(
                        force: true,
                        "[Codex] Usage response after refresh: {}",
                        safe_response_summary(status, &body)
                    );
                }
            }
        }

        if !status.is_success() {
            return Err(format!(
                "Ошибка API лимитов Codex ({}): {}",
                status.as_u16(),
                safe_error_detail(&body).unwrap_or_else(|| "подробности скрыты".to_string())
            ));
        }

        let payload: CodexApiUsagePayload = serde_json::from_str(&body)
            .map_err(|e| format!("Ошибка разбора ответа лимитов Codex: {}", e))?;
        Ok(Self::extract_usage_snapshot_from_api_payload(payload))
    }

    async fn resolve_usage_snapshot(
        profile_id: &str,
    ) -> Result<(Vec<CliUsageWindow>, Option<String>), String> {
        match Self::fetch_usage_snapshot_from_api(profile_id).await {
            Ok(Some(snapshot)) => Ok(snapshot),
            Ok(None) => Ok(Self::usage_snapshot()?.unwrap_or((Vec::new(), None))),
            Err(error) => {
                crate::app_log!(
                    force: true,
                    "[Codex] Live usage unavailable, falling back to local sessions: {}",
                    error
                );
                Ok(Self::usage_snapshot()?.unwrap_or((Vec::new(), None)))
            }
        }
    }

    pub fn save_token(
        profile_id: &str,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_at: u64,
        resource_url: Option<&str>, // repurposed: stores ChatGPT account_id
    ) -> Result<(), String> {
        let data = serde_json::json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "expires_at": expires_at,
            "account_id": resource_url,
        })
        .to_string();
        let encrypted = crate::crypto::encrypt_string(&data).map_err(|e| e.to_string())?;
        let path = Self::token_file_path(profile_id);
        std::fs::write(&path, encrypted)
            .map_err(|e| format!("Не удалось записать токен: {}", e))?;
        crate::app_log!(force: true, "[Codex] Token saved for profile {}, expires_at={}", profile_id, expires_at);
        Ok(())
    }

    /// Returns `(access_token, refresh_token, expires_at, account_id)`
    pub fn get_token(
        profile_id: &str,
    ) -> Result<Option<(String, Option<String>, u64, Option<String>)>, String> {
        let path = Self::token_file_path(profile_id);
        if !path.exists() {
            return Ok(None);
        }
        let encrypted = std::fs::read_to_string(&path)
            .map_err(|e| format!("Не удалось прочитать токен: {}", e))?;
        let decrypted = crate::crypto::decrypt_string(&encrypted)
            .map_err(|e| format!("Не удалось расшифровать токен: {}", e))?;
        let data: serde_json::Value = serde_json::from_str(&decrypted)
            .map_err(|e| format!("Ошибка разбора токена: {}", e))?;
        let access_token = data["access_token"]
            .as_str()
            .ok_or("No access_token in storage")?
            .to_string();
        let refresh_token = data["refresh_token"].as_str().map(|s| s.to_string());
        let expires_at = data["expires_at"]
            .as_u64()
            .ok_or("No expires_at in storage")?;
        let account_id = data["account_id"].as_str().map(|s| s.to_string());
        Ok(Some((access_token, refresh_token, expires_at, account_id)))
    }

    pub async fn refresh_access_token(profile_id: &str, refresh_token: &str) -> Result<(), String> {
        let client = crate::http_client::http_client_builder()?
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;

        let params = [
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];

        let resp = client
            .post(TOKEN_URL)
            .form(&params)
            .header("Accept", "application/json")
            .header(
                "User-Agent",
                "codex_cli_rs/0.114.0 (Windows NT 10.0; x86_64)",
            )
            .send()
            .await
            .map_err(|e| format!("Ошибка сети при обновлении токена: {}", e))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        crate::app_log!(
            force: true,
            "[Codex] Token refresh response: {}",
            safe_response_summary(status, &body)
        );

        if !status.is_success() {
            if status.as_u16() == 400 {
                crate::app_log!(force: true, "[Codex] Refresh token invalid for profile {}, logging out", profile_id);
                let _ = Self::logout(profile_id);
            }
            return Err(format!(
                "Обновление токена: ошибка {}: {}",
                status.as_u16(),
                safe_error_detail(&body).unwrap_or_else(|| "подробности скрыты".to_string())
            ));
        }

        let data: CodexTokenResponse = serde_json::from_str(&body)
            .map_err(|e| format!("Ошибка разбора ответа refresh: {}", e))?;

        let expires_at = Utc::now() + Duration::seconds(data.expires_in.unwrap_or(3600) as i64);
        let account_id = data
            .id_token
            .as_deref()
            .and_then(extract_account_id_from_id_token)
            .or_else(|| {
                Self::get_token(profile_id)
                    .ok()
                    .flatten()
                    .and_then(|(_, _, _, account_id)| account_id)
            });
        Self::save_token(
            profile_id,
            &data.access_token,
            data.refresh_token.as_deref(),
            expires_at.timestamp() as u64,
            account_id.as_deref(),
        )?;

        crate::app_log!(force: true, "[Codex] Token refreshed for profile {}, expires_in={}s", profile_id, data.expires_in.unwrap_or(0));
        Ok(())
    }

    pub fn logout(profile_id: &str) -> Result<(), String> {
        let path = Self::token_file_path(profile_id);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("Не удалось удалить токен: {}", e))?;
        }
        Ok(())
    }

    // ── Status ───────────────────────────────────────────────────────────────

    pub async fn get_status(profile_id: &str) -> Result<CliStatus, String> {
        let token_info = Self::get_token(profile_id)?;
        match token_info {
            None => Ok(CliStatus {
                is_authenticated: false,
                auth_expires_at: None,
                usage: None,
                usage_windows: None,
                usage_plan: None,
            }),
            Some((_, refresh_token, expires_at, _account_id)) => {
                let is_expired = Utc::now().timestamp() as u64 > expires_at;

                if is_expired {
                    if let Some(rt) = refresh_token.as_deref() {
                        crate::app_log!(force: true, "[Codex] get_status: token expired, attempting silent refresh for profile {}", profile_id);
                        match Self::refresh_access_token(profile_id, rt).await {
                            Ok(()) => {
                                if let Ok(Some((_, _, new_exp, _))) = Self::get_token(profile_id) {
                                    let (usage_windows, usage_plan) =
                                        Self::resolve_usage_snapshot(profile_id).await?;
                                    let expires_str =
                                        chrono::DateTime::<Utc>::from_timestamp(new_exp as i64, 0)
                                            .map(|dt| dt.to_rfc3339());
                                    return Ok(CliStatus {
                                        is_authenticated: true,
                                        auth_expires_at: expires_str,
                                        usage: None,
                                        usage_windows: if usage_windows.is_empty() {
                                            None
                                        } else {
                                            Some(usage_windows)
                                        },
                                        usage_plan,
                                    });
                                }
                            }
                            Err(e) => {
                                crate::app_log!(force: true, "[Codex] get_status: silent refresh failed: {}", e);
                            }
                        }
                    }
                    return Ok(CliStatus {
                        is_authenticated: false,
                        auth_expires_at: None,
                        usage: None,
                        usage_windows: None,
                        usage_plan: None,
                    });
                }

                let (usage_windows, usage_plan) = Self::resolve_usage_snapshot(profile_id).await?;
                let expires_str = chrono::DateTime::<Utc>::from_timestamp(expires_at as i64, 0)
                    .map(|dt| dt.to_rfc3339());
                Ok(CliStatus {
                    is_authenticated: true,
                    auth_expires_at: expires_str,
                    usage: None,
                    usage_windows: if usage_windows.is_empty() {
                        None
                    } else {
                        Some(usage_windows)
                    },
                    usage_plan,
                })
            }
        }
    }

    /// Returns the curated list of Codex-compatible models.
    ///
    /// `chatgpt.com/backend-api/models` exposes generic ChatGPT models such as `gpt-5-3`,
    /// which do not match the slugs accepted by `backend-api/codex/responses`.
    pub async fn fetch_models(
        _profile_id: &str,
    ) -> Result<Vec<crate::llm::providers::Model>, String> {
        Ok(crate::llm::providers::static_codex_models())
    }
}

// ─── Serde types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CodexTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexApiUsagePayload {
    #[serde(alias = "planType")]
    plan_type: Option<String>,
    #[serde(alias = "rateLimit")]
    rate_limit: Option<CodexApiRateLimit>,
}

#[derive(Debug, Deserialize)]
struct CodexApiRateLimit {
    allowed: Option<bool>,
    #[serde(alias = "limitReached")]
    limit_reached: Option<bool>,
    #[serde(alias = "primaryWindow")]
    primary_window: Option<CodexApiUsageWindow>,
    #[serde(alias = "secondaryWindow")]
    secondary_window: Option<CodexApiUsageWindow>,
}

#[derive(Debug, Deserialize)]
struct CodexApiUsageWindow {
    #[serde(alias = "usedPercent")]
    used_percent: Option<serde_json::Value>,
    #[serde(alias = "limitWindowSeconds")]
    limit_window_seconds: Option<serde_json::Value>,
    #[serde(alias = "resetAfterSeconds")]
    reset_after_seconds: Option<serde_json::Value>,
    #[serde(alias = "resetAt")]
    reset_at: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct CodexSessionEvent {
    timestamp: Option<String>,
    payload: Option<CodexSessionPayload>,
}

#[derive(Debug, Deserialize)]
struct CodexSessionPayload {
    #[serde(rename = "type")]
    event_type: Option<String>,
    rate_limits: Option<CodexSessionRateLimits>,
}

#[derive(Debug, Deserialize)]
struct CodexSessionRateLimits {
    primary: Option<CodexSessionRateLimit>,
    secondary: Option<CodexSessionRateLimit>,
    plan_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexSessionRateLimit {
    used_percent: f32,
    window_minutes: u32,
    resets_at: Option<i64>,
    resets_in_seconds: Option<i64>,
}

/// Extract account_id (ChatGPT workspace) from id_token JWT claims.
/// JWT = header.payload.signature — we decode the payload (base64url → JSON).
fn extract_account_id_from_id_token(id_token: &str) -> Option<String> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() < 2 {
        crate::app_log!(force: true, "[Codex] id_token has {} parts, expected >=3", parts.len());
        return None;
    }
    // base64url may need padding
    let payload_b64 = parts[1];
    let payload = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .or_else(|_| {
            use base64::engine::general_purpose::URL_SAFE;
            URL_SAFE.decode(payload_b64)
        })
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    crate::app_log!(
        force: true,
        "[Codex] id_token claims parsed successfully: has_auth_claim={}",
        claims.get("https://api.openai.com/auth").is_some()
    );
    // OpenAI id_token: "https://api.openai.com/auth" → { "chatgpt_account_id": "..." }
    let auth_claim = claims.get("https://api.openai.com/auth");
    let account_id = auth_claim
        .and_then(|auth| {
            auth.get("chatgpt_account_id")
                .or_else(|| auth.get("account_id"))
        })
        .or_else(|| claims.get("chatgpt_account_id"))
        .or_else(|| claims.get("account_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    crate::app_log!(
        force: true,
        "[Codex] account_id extracted: {}",
        account_id.is_some()
    );
    account_id
}

#[cfg(test)]
mod tests {
    use super::{
        CodexApiUsagePayload, CodexCliProvider, CodexSessionRateLimit, FIVE_HOUR_WINDOW_MINUTES,
    };
    use chrono::{DateTime, Utc};

    #[test]
    fn build_usage_window_converts_percentages_and_absolute_reset() {
        let rate_limit = CodexSessionRateLimit {
            used_percent: 53.0,
            window_minutes: FIVE_HOUR_WINDOW_MINUTES,
            resets_at: Some(1_775_051_447),
            resets_in_seconds: None,
        };

        let window = CodexCliProvider::build_usage_window(None, &rate_limit);

        assert_eq!(window.key, "5h");
        assert_eq!(window.label, "5ч");
        assert_eq!(window.used_percent, 53.0);
        assert_eq!(window.remaining_percent, 47.0);
        assert_eq!(window.window_minutes, FIVE_HOUR_WINDOW_MINUTES);
        assert_eq!(
            window.resets_at.as_deref(),
            DateTime::<Utc>::from_timestamp(1_775_051_447, 0)
                .map(|dt| dt.to_rfc3339())
                .as_deref()
        );
    }

    #[test]
    fn build_usage_window_supports_relative_reset_seconds() {
        let event_timestamp = DateTime::parse_from_rfc3339("2026-04-01T08:58:56.066Z")
            .unwrap()
            .with_timezone(&Utc);
        let rate_limit = CodexSessionRateLimit {
            used_percent: 16.0,
            window_minutes: 10_080,
            resets_at: None,
            resets_in_seconds: Some(60),
        };

        let window = CodexCliProvider::build_usage_window(Some(&event_timestamp), &rate_limit);

        assert_eq!(window.key, "weekly");
        assert_eq!(window.remaining_percent, 84.0);
        assert_eq!(
            window.resets_at.as_deref(),
            Some("2026-04-01T08:59:56.066+00:00")
        );
    }

    #[test]
    fn extract_usage_snapshot_from_api_payload_uses_live_windows() {
        let payload: CodexApiUsagePayload = serde_json::from_str(
            r#"{
                "plan_type": "plus",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 53,
                        "limit_window_seconds": 18000,
                        "reset_at": 1775051447
                    },
                    "secondary_window": {
                        "used_percent": "16",
                        "limit_window_seconds": "604800",
                        "reset_after_seconds": 60
                    }
                }
            }"#,
        )
        .unwrap();

        let (windows, plan_type) =
            CodexCliProvider::extract_usage_snapshot_from_api_payload(payload).unwrap();

        assert_eq!(plan_type.as_deref(), Some("plus"));
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].key, "5h");
        assert_eq!(windows[0].used_percent, 53.0);
        assert_eq!(windows[0].remaining_percent, 47.0);
        assert_eq!(windows[1].key, "weekly");
        assert_eq!(windows[1].used_percent, 16.0);
        assert_eq!(windows[1].remaining_percent, 84.0);
    }

    #[test]
    fn extract_usage_snapshot_from_api_payload_falls_back_to_primary_secondary_order() {
        let payload: CodexApiUsagePayload = serde_json::from_str(
            r#"{
                "plan_type": "pro",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 20,
                        "reset_at": 1775051447
                    },
                    "secondary_window": {
                        "used_percent": 40,
                        "reset_at": 1775656247
                    }
                }
            }"#,
        )
        .unwrap();

        let (windows, plan_type) =
            CodexCliProvider::extract_usage_snapshot_from_api_payload(payload).unwrap();

        assert_eq!(plan_type.as_deref(), Some("pro"));
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].key, "5h");
        assert_eq!(windows[0].window_minutes, FIVE_HOUR_WINDOW_MINUTES);
        assert_eq!(windows[1].key, "weekly");
    }
}
