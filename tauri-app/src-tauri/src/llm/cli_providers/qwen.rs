use serde::Deserialize;
use reqwest::Client;
use keyring::Entry;
use chrono::{DateTime, Utc, Duration};
use sha2::{Digest, Sha256};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;

use super::{CliAuthInitResponse, CliAuthStatus, CliStatus, CliUsage};

const CLIENT_ID: &str = "f0304373b74a44d2b584a3fb70ca9e56";
const AUTH_START_URL: &str = "https://chat.qwen.ai/api/v1/oauth2/device/code";
const AUTH_TOKEN_URL: &str = "https://chat.qwen.ai/api/v1/oauth2/token";
const SCOPE: &str = "openid profile email model.completion";

#[derive(Debug, Deserialize)]
struct QwenDeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct QwenTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    resource_url: Option<String>,
}

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

pub struct QwenCliProvider;

impl QwenCliProvider {
    // ── Auth ─────────────────────────────────────────────────────────────────

    pub async fn auth_start() -> Result<CliAuthInitResponse, String> {
        let client = Client::new();

        let code_verifier = generate_code_verifier();
        let code_challenge = generate_code_challenge(&code_verifier);

        crate::app_log!(force: true, "[DEBUG] Qwen Auth Start: Init with Client ID: {}, PKCE challenge: {}", CLIENT_ID, code_challenge);

        let mut params = std::collections::HashMap::new();
        params.insert("client_id", CLIENT_ID);
        params.insert("scope", SCOPE);
        params.insert("code_challenge", &code_challenge);
        params.insert("code_challenge_method", "S256");

        let resp = client.post(AUTH_START_URL)
            .form(&params)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| e.to_string())?;

        crate::app_log!(force: true, "[DEBUG] Qwen Auth Start response {}: {}", status, body);

        if !status.is_success() {
            return Err(format!("Auth server error {} {}: {}", status.as_u16(), status.canonical_reason().unwrap_or("Unknown"), body));
        }

        let data: QwenDeviceCodeResponse = serde_json::from_str(&body)
            .map_err(|e| format!("Parse error: {}, Body: {}", e, body))?;

        Ok(CliAuthInitResponse {
            device_code: data.device_code,
            user_code: data.user_code,
            verification_url: data.verification_uri_complete.unwrap_or(data.verification_uri),
            expires_in: data.expires_in,
            poll_interval: data.interval.unwrap_or(5),
            code_verifier: Some(code_verifier),
        })
    }

    pub async fn auth_poll(device_code: &str, code_verifier: Option<&str>) -> Result<CliAuthStatus, String> {
        let client = Client::new();

        let mut params = std::collections::HashMap::new();
        params.insert("client_id", CLIENT_ID);
        params.insert("device_code", device_code);
        params.insert("grant_type", "urn:ietf:params:oauth:grant-type:device_code");

        let verifier_owned;
        if let Some(cv) = code_verifier {
            verifier_owned = cv.to_string();
            params.insert("code_verifier", &verifier_owned);
        }

        let resp = client.post(AUTH_TOKEN_URL)
            .form(&params)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        let status = resp.status();
        crate::app_log!(force: true, "[DEBUG] Qwen Auth Poll Status: {}", status);

        if status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            crate::app_log!(force: true, "[DEBUG] Qwen Auth Poll Success Body: {}", body);

            let data: QwenTokenResponse = serde_json::from_str(&body)
                .map_err(|e| format!("Token parse error: {}, body: {}", e, body))?;
            let expires_at = Utc::now() + Duration::seconds(data.expires_in as i64);

            Ok(CliAuthStatus::Authorized {
                access_token: data.access_token,
                refresh_token: data.refresh_token,
                expires_at: expires_at.timestamp() as u64,
                resource_url: data.resource_url,
            })
        } else if status.as_u16() == 400 {
            let body = resp.text().await.unwrap_or_default();
            crate::app_log!(force: true, "[DEBUG] Qwen Auth Poll 400 Body: {}", body);

            let err_data: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
            if let Some(err) = err_data.get("error").and_then(|e| e.as_str()) {
                match err {
                    "authorization_pending" => Ok(CliAuthStatus::Pending),
                    "expired_token" => Ok(CliAuthStatus::Expired),
                    "slow_down" => Ok(CliAuthStatus::SlowDown),
                    _ => Ok(CliAuthStatus::Error(err.to_string())),
                }
            } else {
                Err(format!("Auth failed with 400: {:?}", err_data))
            }
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("Auth failed with status {}: {}", status, body))
        }
    }

    // ── Token storage (keyring) ───────────────────────────────────────────────

    pub fn save_token(profile_id: &str, access_token: &str, refresh_token: Option<&str>, expires_at: u64, resource_url: Option<&str>) -> Result<(), String> {
        crate::app_log!(force: true, "[DEBUG] QwenCliProvider::save_token called for profile {}. Expires: {}, resource_url: {:?}", profile_id, expires_at, resource_url);
        let entry_name = format!("qwen-cli-{}", profile_id);
        let entry = Entry::new("mini-ai-1c", &entry_name).map_err(|e| e.to_string())?;
        let data = serde_json::json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "expires_at": expires_at,
            "resource_url": resource_url
        });
        entry.set_password(&data.to_string()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_token(profile_id: &str) -> Result<Option<(String, Option<String>, u64, Option<String>)>, String> {
        let entry_name = format!("qwen-cli-{}", profile_id);
        let entry = Entry::new("mini-ai-1c", &entry_name).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(pwd) => {
                let data: serde_json::Value = serde_json::from_str(&pwd).map_err(|e| e.to_string())?;
                let access_token = data["access_token"].as_str().ok_or("No access token")?.to_string();
                let refresh_token = data["refresh_token"].as_str().map(|s| s.to_string());
                let expires_at = data["expires_at"].as_u64().ok_or("No expires_at")?;
                let resource_url = data["resource_url"].as_str().map(|s| s.to_string());
                Ok(Some((access_token, refresh_token, expires_at, resource_url)))
            },
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn logout(profile_id: &str) -> Result<(), String> {
        let entry_name = format!("qwen-cli-{}", profile_id);
        let entry = Entry::new("mini-ai-1c", &entry_name).map_err(|e| e.to_string())?;
        entry.delete_password().map_err(|e| e.to_string())?;
        Ok(())
    }

    // ── Local usage counter (file-based, multi-instance safe) ─────────────────

    fn usage_file_path(profile_id: &str) -> std::path::PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("mini-ai-1c")
            .join(format!("qwen-usage-{}.json", profile_id))
    }

    /// Read today's usage from the local file. Resets to 0 if the date has changed.
    pub fn get_local_usage(profile_id: &str) -> CliUsage {
        let path = Self::usage_file_path(profile_id);
        let today = Utc::now().format("%Y-%m-%d").to_string();

        let resets_at = (Utc::now().date_naive() + chrono::Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc).to_rfc3339());

        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                if data.get("date").and_then(|d| d.as_str()) == Some(today.as_str()) {
                    let count = data.get("count").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
                    let limit = data.get("limit").and_then(|l| l.as_u64()).unwrap_or(0) as u32;
                    return CliUsage {
                        requests_used: count,
                        requests_limit: limit,
                        resets_at,
                    };
                }
            }
        }

        CliUsage {
            requests_used: 0,
            requests_limit: 0,
            resets_at,
        }
    }

    /// Increment the local request counter after a successful API call.
    /// Uses atomic temp-file rename to be safe across multiple app instances.
    pub fn increment_request_count(profile_id: &str) {
        let path = Self::usage_file_path(profile_id);
        let today = Utc::now().format("%Y-%m-%d").to_string();

        // Read current count (reset if date changed)
        let mut count: u64 = 0;
        let mut limit: u64 = 0;
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                if data.get("date").and_then(|d| d.as_str()) == Some(today.as_str()) {
                    count = data.get("count").and_then(|c| c.as_u64()).unwrap_or(0);
                    limit = data.get("limit").and_then(|l| l.as_u64()).unwrap_or(0);
                }
            }
        }
        count += 1;

        let data = serde_json::json!({ "date": today, "count": count, "limit": limit });

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let tmp_path = path.with_extension("tmp");
        if std::fs::write(&tmp_path, data.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp_path, &path);
        }
        crate::app_log!("[Qwen] Request count: {}", count);
    }

    /// Called when the server provides rate-limit info via response headers.
    /// Overwrites the local counter with accurate server data.
    pub fn save_usage(profile_id: &str, requests_used: u32, requests_limit: u32, resets_at: Option<String>) -> Result<(), String> {
        let path = Self::usage_file_path(profile_id);
        let today = Utc::now().format("%Y-%m-%d").to_string();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let data = serde_json::json!({
            "date": today,
            "count": requests_used,
            "limit": requests_limit,
            "resets_at": resets_at,
        });

        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, data.to_string()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp_path, &path).map_err(|e| e.to_string())?;
        crate::app_log!("[Qwen] Usage saved from server: {}/{}", requests_used, requests_limit);
        Ok(())
    }

    /// Returns current local usage (no external API call).
    pub async fn fetch_usage_from_api(profile_id: &str) -> Result<CliUsage, String> {
        Ok(Self::get_local_usage(profile_id))
    }

    pub async fn get_status(profile_id: &str) -> Result<CliStatus, String> {
        let token_info = Self::get_token(profile_id)?;
        if let Some((_, _, expires_at, _)) = token_info {
            let is_expired = Utc::now().timestamp() as u64 > expires_at;
            let usage = if !is_expired { Some(Self::get_local_usage(profile_id)) } else { None };

            Ok(CliStatus {
                is_authenticated: !is_expired,
                auth_expires_at: Some(DateTime::from_timestamp(expires_at as i64, 0).unwrap_or(Utc::now()).to_rfc3339()),
                usage,
            })
        } else {
            Ok(CliStatus {
                is_authenticated: false,
                auth_expires_at: None,
                usage: None,
            })
        }
    }
}
