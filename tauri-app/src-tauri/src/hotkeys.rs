//! Global hotkey management using Tauri plugin

use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Register global hotkeys
pub fn register_hotkeys(app: &AppHandle) -> Result<(), String> {
    let shortcut: Shortcut = "ctrl+shift+1"
        .parse()
        .map_err(|e| format!("Failed to parse shortcut: {:?}", e))?;

    let app_handle = app.clone();
    
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                // Emit event to frontend
                let _ = app_handle.emit("hotkey-capture", ());
            }
        })
        .map_err(|e| format!("Failed to register shortcut: {}", e))?;

    Ok(())
}

/// Unregister all hotkeys
pub fn unregister_hotkeys(app: &AppHandle) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| format!("Failed to unregister shortcuts: {}", e))
}
