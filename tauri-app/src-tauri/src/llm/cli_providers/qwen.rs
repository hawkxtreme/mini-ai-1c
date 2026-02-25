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
    pub async fn auth_start() -> Result<CliAuthInitResponse, String> {
        let client = Client::new();

        // PKCE: generate code_verifier and code_challenge
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

        // PKCE: code_verifier is required
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

    pub fn save_token(access_token: &str, refresh_token: Option<&str>, expires_at: u64, resource_url: Option<&str>) -> Result<(), String> {
        crate::app_log!(force: true, "[DEBUG] QwenCliProvider::save_token called. Expires: {}, resource_url: {:?}", expires_at, resource_url);
        let entry = Entry::new("mini-ai-1c", "qwen-cli").map_err(|e| e.to_string())?;
        let data = serde_json::json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "expires_at": expires_at,
            "resource_url": resource_url
        });
        entry.set_password(&data.to_string()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_token() -> Result<Option<(String, Option<String>, u64, Option<String>)>, String> {
        let entry = Entry::new("mini-ai-1c", "qwen-cli").map_err(|e| e.to_string())?;
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

    pub fn logout() -> Result<(), String> {
        let entry = Entry::new("mini-ai-1c", "qwen-cli").map_err(|e| e.to_string())?;
        entry.delete_password().map_err(|e| e.to_string())?;
        // Also clear cached usage
        if let Ok(usage_entry) = Entry::new("mini-ai-1c", "qwen-cli-usage") {
            let _ = usage_entry.delete_password();
        }
        Ok(())
    }

    pub fn save_usage(requests_used: u32, requests_limit: u32, resets_at: Option<String>) -> Result<(), String> {
        let entry = Entry::new("mini-ai-1c", "qwen-cli-usage").map_err(|e| e.to_string())?;
        let data = serde_json::json!({
            "requests_used": requests_used,
            "requests_limit": requests_limit,
            "resets_at": resets_at,
        });
        entry.set_password(&data.to_string()).map_err(|e| e.to_string())?;
        crate::app_log!(force: true, "[DEBUG] Qwen usage cached: {}/{}, resets_at={:?}", requests_used, requests_limit, resets_at);
        Ok(())
    }

    fn get_cached_usage() -> Option<CliUsage> {
        let entry = Entry::new("mini-ai-1c", "qwen-cli-usage").ok()?;
        let pwd = entry.get_password().ok()?;
        let data: serde_json::Value = serde_json::from_str(&pwd).ok()?;
        Some(CliUsage {
            requests_used: data["requests_used"].as_u64()? as u32,
            requests_limit: data["requests_limit"].as_u64()? as u32,
            resets_at: data["resets_at"].as_str().map(|s| s.to_string()),
        })
    }

    pub async fn get_status() -> Result<CliStatus, String> {
        let token_info = Self::get_token()?;
        if let Some((_, _, expires_at, _)) = token_info {
            let is_expired = Utc::now().timestamp() as u64 > expires_at;
            let usage = if !is_expired { Self::get_cached_usage() } else { None };

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
