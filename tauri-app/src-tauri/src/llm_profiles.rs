//! LLM Profile management with encrypted API keys

use serde::{Deserialize, Serialize};
use std::fs;

use crate::crypto::{decrypt_string, encrypt_string};
use crate::settings::get_settings_dir;

/// Supported LLM providers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LLMProvider {
    OpenAI,
    Anthropic,
    OpenRouter,
    Google,
    DeepSeek,
    Groq,
    Mistral,
    XAI,
    Perplexity,
    Ollama,
    ZAI,
    Custom,
}

impl Default for LLMProvider {
    fn default() -> Self {
        LLMProvider::OpenAI
    }
}

impl std::fmt::Display for LLMProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// LLM Profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMProfile {
    pub id: String,
    pub name: String,
    pub provider: LLMProvider,
    pub model: String,
    pub api_key_encrypted: String,
    pub base_url: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub context_window_override: Option<u32>,
}

impl LLMProfile {
    /// Create a default OpenAI profile
    pub fn default_profile() -> Self {
        Self {
            id: "default".to_string(),
            name: "Default (OpenAI)".to_string(),
            provider: LLMProvider::OpenAI,
            model: "gpt-4o-mini".to_string(),
            api_key_encrypted: String::new(),
            base_url: None,
            max_tokens: 4096,
            temperature: 0.7,
            context_window_override: None,
        }
    }

    /// Get decrypted API key
    pub fn get_api_key(&self) -> String {
        if self.api_key_encrypted.is_empty() {
            return String::new();
        }
        decrypt_string(&self.api_key_encrypted).unwrap_or_default()
    }

    /// Set and encrypt API key
    pub fn set_api_key(&mut self, api_key: &str) {
        self.api_key_encrypted = encrypt_string(api_key).unwrap_or_default();
    }

    /// Get base URL with default fallback
    pub fn get_base_url(&self) -> String {
        self.base_url.clone().unwrap_or_else(|| {
            match self.provider {
                LLMProvider::OpenAI => "https://api.openai.com/v1".to_string(),
                LLMProvider::Anthropic => "https://api.anthropic.com/v1".to_string(),
                LLMProvider::OpenRouter => "https://openrouter.ai/api/v1".to_string(),
                LLMProvider::Google => "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
                LLMProvider::DeepSeek => "https://api.deepseek.com/v1".to_string(),
                LLMProvider::Groq => "https://api.groq.com/openai/v1".to_string(),
                LLMProvider::Mistral => "https://api.mistral.ai/v1".to_string(),
                LLMProvider::XAI => "https://api.x.ai/v1".to_string(),
                LLMProvider::Perplexity => "https://api.perplexity.ai".to_string(),
                LLMProvider::ZAI => "https://api.z.ai/api/coding/paas/v4".to_string(),
                LLMProvider::Ollama => "http://localhost:11434/v1".to_string(),
                LLMProvider::Custom => String::new(),
            }
        })
    }
}

/// Profile storage
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileStore {
    pub profiles: Vec<LLMProfile>,
    pub active_profile_id: String,
}

fn get_profiles_file() -> std::path::PathBuf {
    get_settings_dir().join("llm_profiles.json")
}

/// Load profiles from file
pub fn load_profiles() -> ProfileStore {
    let path = get_profiles_file();
    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(content) => {
                match serde_json::from_str::<ProfileStore>(&content) {
                    Ok(mut store) => {
                        if store.profiles.is_empty() {
                            store.profiles.push(LLMProfile::default_profile());
                            store.active_profile_id = "default".to_string();
                        }
                        store
                    }
                    Err(e) => {
                        eprintln!("[LLM Profiles] Failed to parse profiles file: {}. Creating defaults.", e);
                        create_default_store()
                    }
                }
            }
            Err(e) => {
                eprintln!("[LLM Profiles] Failed to read profiles file: {}. Creating defaults.", e);
                create_default_store()
            }
        }
    } else {
        create_default_store()
    }
}

fn create_default_store() -> ProfileStore {
    ProfileStore {
        profiles: vec![LLMProfile::default_profile()],
        active_profile_id: "default".to_string(),
    }
}

/// Save profiles to file
pub fn save_profiles(store: &ProfileStore) -> Result<(), String> {
    let dir = get_settings_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    
    let path = get_profiles_file();
    let content = serde_json::to_string_pretty(store)
        .map_err(|e| e.to_string())?;
    
    fs::write(path, content).map_err(|e| e.to_string())
}

/// Get active profile
pub fn get_active_profile() -> Option<LLMProfile> {
    let store = load_profiles();
    store.profiles
        .into_iter()
        .find(|p| p.id == store.active_profile_id)
}
