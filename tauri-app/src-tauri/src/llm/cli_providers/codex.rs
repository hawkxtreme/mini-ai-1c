//! OpenAI Codex CLI provider — OAuth2+PKCE browser-redirect flow
//!
//! Auth flow:
//!   1. `auth_start` — generates PKCE, starts local callback server on port 1455,
//!      returns browser auth URL
//!   2. User opens URL in browser, authorises → browser redirects to localhost:1455/auth/callback
//!   3. `auth_poll` — checks if callback received; if so, exchanges code for tokens
//!   4. Frontend calls `cli_save_token` to persist tokens

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use keyring::Entry;
use lazy_static::lazy_static;
use rand::RngCore;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use super::{CliAuthInitResponse, CliAuthStatus, CliStatus, CliUsage};

// ─── Constants ─────────────────────────────────────────────────────────────

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const REDIRECT_PORT: u16 = 1455;
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const SCOPE: &str = "openid profile email offline_access";

// ─── Callback State ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum CallbackResult {
    Pending,
    Success(String), // authorization code
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
    CALLBACK.lock().map(|cb| cb.clone()).unwrap_or(CallbackResult::Error("Lock error".to_string()))
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

                crate::app_log!(force: true, "[Codex] Callback request: {}", request_line.trim());

                // Parse: GET /auth/callback?code=...&state=... HTTP/1.1
                let code = request_line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|path| {
                        let full = format!("http://localhost{}", path);
                        url::Url::parse(&full).ok()
                    })
                    .and_then(|u| {
                        u.query_pairs()
                            .find(|(k, _)| k == "code")
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
                        set_callback(CallbackResult::Success(c));
                    }
                    None => {
                        set_callback(CallbackResult::Error("No authorization code in callback".to_string()));
                    }
                }
            }
            Err(e) => {
                crate::app_log!(force: true, "[Codex] Callback server accept error: {}", e);
                set_callback(CallbackResult::Error(format!("Ошибка сервера авторизации: {}", e)));
            }
        }
    });
}

// ─── Token exchange ─────────────────────────────────────────────────────────

async fn exchange_code(code: &str, code_verifier: &str) -> Result<CliAuthStatus, String> {
    let client = Client::builder()
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
        .header("User-Agent", "codex_cli_rs/0.114.0 (Windows NT 10.0; x86_64)")
        .send()
        .await
        .map_err(|e| format!("Ошибка сети при обмене кода: {}", e))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    crate::app_log!(force: true, "[Codex] Token exchange response {}: {:.200}", status, body);

    if !status.is_success() {
        return Ok(CliAuthStatus::Error(format!(
            "Ошибка получения токена ({}): {}",
            status.as_u16(),
            body
        )));
    }

    let data: CodexTokenResponse = serde_json::from_str(&body)
        .map_err(|e| format!("Ошибка разбора ответа токена: {}, body: {}", e, body))?;

    let expires_at = Utc::now() + Duration::seconds(data.expires_in.unwrap_or(3600) as i64);

    Ok(CliAuthStatus::Authorized {
        access_token: data.access_token,
        refresh_token: data.refresh_token,
        expires_at: expires_at.timestamp() as u64,
        resource_url: None,
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
        ];

        let query = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        let auth_url = format!("{}?{}", AUTH_URL, query);

        crate::app_log!(force: true, "[Codex] auth_start: auth URL ready (state={})", state);

        Ok(CliAuthInitResponse {
            device_code: state, // repurposed as session identifier
            user_code: String::new(),  // not used in browser redirect flow
            verification_url: auth_url,
            expires_in: 300,
            poll_interval: 2,
            code_verifier: Some(code_verifier),
        })
    }

    pub async fn auth_poll(
        _device_code: &str,
        code_verifier: Option<&str>,
    ) -> Result<CliAuthStatus, String> {
        match read_callback() {
            CallbackResult::Pending => Ok(CliAuthStatus::Pending),
            CallbackResult::Error(e) => Ok(CliAuthStatus::Error(e)),
            CallbackResult::Success(code) => {
                let verifier = code_verifier.unwrap_or("");
                if verifier.is_empty() {
                    return Ok(CliAuthStatus::Error("PKCE verifier missing".to_string()));
                }
                crate::app_log!(force: true, "[Codex] auth_poll: exchanging code for token...");
                exchange_code(&code, verifier).await
            }
        }
    }

    // ── Token storage (keyring) ───────────────────────────────────────────────

    pub fn save_token(
        profile_id: &str,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_at: u64,
        _resource_url: Option<&str>,
    ) -> Result<(), String> {
        let entry_name = format!("codex-cli-{}", profile_id);
        let entry = Entry::new("mini-ai-1c", &entry_name).map_err(|e| e.to_string())?;
        let data = serde_json::json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "expires_at": expires_at,
        });
        entry.set_password(&data.to_string()).map_err(|e| e.to_string())?;
        crate::app_log!(force: true, "[Codex] Token saved for profile {}, expires_at={}", profile_id, expires_at);
        Ok(())
    }

    /// Returns `(access_token, refresh_token, expires_at)`
    pub fn get_token(
        profile_id: &str,
    ) -> Result<Option<(String, Option<String>, u64)>, String> {
        let entry_name = format!("codex-cli-{}", profile_id);
        let entry = Entry::new("mini-ai-1c", &entry_name).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(pwd) => {
                let data: serde_json::Value = serde_json::from_str(&pwd).map_err(|e| e.to_string())?;
                let access_token = data["access_token"]
                    .as_str()
                    .ok_or("No access_token in storage")?
                    .to_string();
                let refresh_token = data["refresh_token"].as_str().map(|s| s.to_string());
                let expires_at = data["expires_at"].as_u64().ok_or("No expires_at in storage")?;
                Ok(Some((access_token, refresh_token, expires_at)))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub async fn refresh_access_token(profile_id: &str, refresh_token: &str) -> Result<(), String> {
        let client = Client::builder()
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
            .header("User-Agent", "codex_cli_rs/0.114.0 (Windows NT 10.0; x86_64)")
            .send()
            .await
            .map_err(|e| format!("Ошибка сети при обновлении токена: {}", e))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        crate::app_log!(force: true, "[Codex] Token refresh response {}: {:.200}", status, body);

        if !status.is_success() {
            if status.as_u16() == 400 {
                crate::app_log!(force: true, "[Codex] Refresh token invalid for profile {}, logging out", profile_id);
                let _ = Self::logout(profile_id);
            }
            return Err(format!("Обновление токена: ошибка {}: {}", status.as_u16(), body));
        }

        let data: CodexTokenResponse = serde_json::from_str(&body)
            .map_err(|e| format!("Ошибка разбора ответа refresh: {}, body: {}", e, body))?;

        let expires_at = Utc::now() + Duration::seconds(data.expires_in.unwrap_or(3600) as i64);
        Self::save_token(
            profile_id,
            &data.access_token,
            data.refresh_token.as_deref(),
            expires_at.timestamp() as u64,
            None,
        )?;

        crate::app_log!(force: true, "[Codex] Token refreshed for profile {}, expires_in={}s", profile_id, data.expires_in.unwrap_or(0));
        Ok(())
    }

    pub fn logout(profile_id: &str) -> Result<(), String> {
        let entry_name = format!("codex-cli-{}", profile_id);
        let entry = Entry::new("mini-ai-1c", &entry_name).map_err(|e| e.to_string())?;
        match entry.delete_password() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    // ── Status ───────────────────────────────────────────────────────────────

    pub async fn get_status(profile_id: &str) -> Result<CliStatus, String> {
        let token_info = Self::get_token(profile_id)?;
        match token_info {
            None => Ok(CliStatus {
                is_authenticated: false,
                auth_expires_at: None,
                usage: Some(CliUsage { requests_used: 0, requests_limit: 0, resets_at: None }),
            }),
            Some((_, refresh_token, expires_at)) => {
                let is_expired = Utc::now().timestamp() as u64 > expires_at;

                if is_expired {
                    if let Some(rt) = refresh_token.as_deref() {
                        crate::app_log!(force: true, "[Codex] get_status: token expired, attempting silent refresh for profile {}", profile_id);
                        match Self::refresh_access_token(profile_id, rt).await {
                            Ok(()) => {
                                if let Ok(Some((_, _, new_exp))) = Self::get_token(profile_id) {
                                    let expires_str = chrono::DateTime::<Utc>::from_timestamp(new_exp as i64, 0)
                                        .map(|dt| dt.to_rfc3339());
                                    return Ok(CliStatus {
                                        is_authenticated: true,
                                        auth_expires_at: expires_str,
                                        usage: Some(CliUsage { requests_used: 0, requests_limit: 0, resets_at: None }),
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
                        usage: Some(CliUsage { requests_used: 0, requests_limit: 0, resets_at: None }),
                    });
                }

                let expires_str = chrono::DateTime::<Utc>::from_timestamp(expires_at as i64, 0)
                    .map(|dt| dt.to_rfc3339());
                Ok(CliStatus {
                    is_authenticated: true,
                    auth_expires_at: expires_str,
                    usage: Some(CliUsage { requests_used: 0, requests_limit: 0, resets_at: None }),
                })
            }
        }
    }

    pub async fn fetch_usage_from_api(_profile_id: &str) -> Result<CliUsage, String> {
        // Codex API doesn't expose usage endpoint — return zero
        Ok(CliUsage { requests_used: 0, requests_limit: 0, resets_at: None })
    }
}

// ─── Serde types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CodexTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}
