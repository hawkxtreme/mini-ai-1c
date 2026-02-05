//! Encryption utilities using AES-GCM

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::Rng;
use std::fs;

use crate::settings::get_settings_dir;

const NONCE_SIZE: usize = 12;

/// Get or create master key
fn get_master_key() -> [u8; 32] {
    let key_file = get_settings_dir().join(".key");
    
    if key_file.exists() {
        if let Ok(data) = fs::read(&key_file) {
            if data.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&data);
                return key;
            }
        }
    }
    
    // Generate new key
    let mut key = [0u8; 32];
    rand::thread_rng().fill(&mut key);
    
    // Save key
    let dir = get_settings_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(&key_file, &key);
    
    key
}

/// Encrypt a string
pub fn encrypt_string(plaintext: &str) -> Result<String, String> {
    let key = get_master_key();
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| e.to_string())?;
    
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rand::thread_rng().fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| e.to_string())?;
    
    // Combine nonce + ciphertext and encode as base64
    let mut combined = nonce_bytes.to_vec();
    combined.extend(ciphertext);
    
    Ok(STANDARD.encode(&combined))
}

/// Decrypt a string
pub fn decrypt_string(encrypted: &str) -> Result<String, String> {
    let key = get_master_key();
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| e.to_string())?;
    
    let combined = STANDARD.decode(encrypted)
        .map_err(|e| e.to_string())?;
    
    if combined.len() < NONCE_SIZE {
        return Err("Invalid encrypted data".to_string());
    }
    
    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_SIZE);
    let nonce = Nonce::from_slice(nonce_bytes);
    
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| e.to_string())?;
    
    String::from_utf8(plaintext).map_err(|e| e.to_string())
}
