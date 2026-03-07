use crate::settings::{self, AppSettings};
use tauri::AppHandle;

/// Get application settings
#[tauri::command]
pub fn get_settings() -> AppSettings {
    crate::app_log!("[DEBUG] get_settings called");
    settings::load_settings()
}

/// Save application settings
#[tauri::command]
pub fn save_settings(new_settings: AppSettings) -> Result<(), String> {
    settings::save_settings(&new_settings)
}

/// Mark onboarding as completed
#[tauri::command]
pub fn complete_onboarding() -> Result<(), String> {
    let mut settings = settings::load_settings();
    settings.onboarding_completed = true;
    settings::save_settings(&settings)
}

#[tauri::command]
pub fn reset_onboarding() -> Result<(), String> {
    let mut settings = settings::load_settings();
    settings.onboarding_completed = false;
    settings::save_settings(&settings)
}

/// Restart the application
#[tauri::command]
pub fn restart_app_cmd(app_handle: AppHandle) {
    app_handle.restart();
}

/// Check if Node.js is installed and return its version string, or None if not found
#[tauri::command]
pub fn check_node_version_cmd() -> Option<String> {
    use std::process::Command;

    #[cfg(target_os = "windows")]
    let output = Command::new("cmd")
        .args(["/C", "node --version"])
        .output();

    #[cfg(not(target_os = "windows"))]
    let output = Command::new("node")
        .arg("--version")
        .output();

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
        }
        _ => None,
    }
}

/// Check if Java is installed and available in PATH
#[tauri::command]
pub fn check_java_cmd() -> bool {
    use std::process::Command;
    
    // Try verification by running java -version
    #[cfg(target_os = "windows")]
    let output = Command::new("cmd")
        .args(["/C", "java -version"])
        .output();
        
    #[cfg(not(target_os = "windows"))]
    let output = Command::new("java")
        .arg("-version")
        .output();

    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}
