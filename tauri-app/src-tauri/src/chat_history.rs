//! Chat history persistence

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;

use crate::settings::get_settings_dir;

/// Chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// Chat session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<ChatMessage>,
}

impl ChatSession {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: format!("chat_{}", now.timestamp_millis()),
            title: "Новый чат".to_string(),
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
        }
    }

    pub fn add_message(&mut self, role: &str, content: &str) {
        self.messages.push(ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
        });
        self.updated_at = Utc::now();
        
        // Auto-generate title from first user message
        if self.title == "Новый чат" && role == "user" && !content.is_empty() {
            self.title = content.chars().take(50).collect::<String>();
            if content.len() > 50 {
                self.title.push_str("...");
            }
        }
    }
}

/// Chat history store
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatHistory {
    pub sessions: Vec<ChatSession>,
    pub active_session_id: Option<String>,
}

fn get_history_file() -> std::path::PathBuf {
    get_settings_dir().join("chat_history.json")
}

/// Load chat history
pub fn load_history() -> ChatHistory {
    let path = get_history_file();
    if path.exists() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    } else {
        ChatHistory::default()
    }
}

/// Save chat history
pub fn save_history(history: &ChatHistory) -> Result<(), String> {
    let dir = get_settings_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    
    let path = get_history_file();
    let content = serde_json::to_string_pretty(history).map_err(|e| e.to_string())?;
    fs::write(path, content).map_err(|e| e.to_string())
}

/// Get or create active session
pub fn get_active_session() -> ChatSession {
    let mut history = load_history();
    
    if let Some(id) = &history.active_session_id {
        if let Some(session) = history.sessions.iter().find(|s| &s.id == id) {
            return session.clone();
        }
    }
    
    // Create new session
    let session = ChatSession::new();
    history.active_session_id = Some(session.id.clone());
    history.sessions.push(session.clone());
    let _ = save_history(&history);
    session
}

/// Save message to active session
pub fn save_message(role: &str, content: &str) -> Result<(), String> {
    let mut history = load_history();
    
    if let Some(id) = &history.active_session_id {
        if let Some(session) = history.sessions.iter_mut().find(|s| &s.id == id) {
            session.add_message(role, content);
            return save_history(&history);
        }
    }
    
    // Create new session if needed
    let mut session = ChatSession::new();
    session.add_message(role, content);
    history.active_session_id = Some(session.id.clone());
    history.sessions.push(session);
    save_history(&history)
}

/// Create new chat session
pub fn create_new_session() -> ChatSession {
    let mut history = load_history();
    let session = ChatSession::new();
    history.active_session_id = Some(session.id.clone());
    history.sessions.push(session.clone());
    let _ = save_history(&history);
    session
}

/// Get all sessions (for sidebar)
pub fn get_sessions() -> Vec<ChatSession> {
    let history = load_history();
    let mut sessions = history.sessions;
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}

/// Delete session
pub fn delete_session(session_id: &str) -> Result<(), String> {
    let mut history = load_history();
    history.sessions.retain(|s| s.id != session_id);
    
    if history.active_session_id.as_deref() == Some(session_id) {
        history.active_session_id = history.sessions.first().map(|s| s.id.clone());
    }
    
    save_history(&history)
}

/// Set active session
pub fn set_active_session(session_id: &str) -> Result<ChatSession, String> {
    let mut history = load_history();
    
    if let Some(session) = history.sessions.iter().find(|s| s.id == session_id) {
        history.active_session_id = Some(session_id.to_string());
        save_history(&history)?;
        return Ok(session.clone());
    }
    
    Err("Session not found".to_string())
}
