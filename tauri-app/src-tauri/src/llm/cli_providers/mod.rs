pub mod qwen;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliAuthInitResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: u64,
    pub poll_interval: u64,
    pub code_verifier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "data")]
pub enum CliAuthStatus {
    Pending,
    Authorized {
        access_token: String,
        refresh_token: Option<String>,
        expires_at: u64,
        resource_url: Option<String>,
    },
    Expired,
    SlowDown,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliUsage {
    #[serde(alias = "requests_used", alias = "requestsUsed")]
    pub requests_used: u32,
    #[serde(alias = "requests_limit", alias = "requestsLimit")]
    pub requests_limit: u32,
    #[serde(alias = "resets_at", alias = "resetsAt")]
    pub resets_at: Option<String>,
}

// CliStatus for Rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliStatus {
    pub is_authenticated: bool,
    pub auth_expires_at: Option<String>,
    pub usage: Option<CliUsage>,
}
