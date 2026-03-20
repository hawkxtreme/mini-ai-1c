use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
}

/// Find 1C Configurator windows
#[tauri::command]
pub fn find_configurator_windows_cmd(pattern: String) -> Vec<WindowInfo> {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::find_configurator_windows(&pattern)
            .into_iter()
            .map(|w| WindowInfo { hwnd: w.hwnd, title: w.title })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = pattern;
        Vec::new()
    }
}

/// Check if there is an active selection in the window
#[tauri::command]
pub fn check_selection_state(hwnd: isize) -> bool {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::is_selection_active(hwnd)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        false
    }
}

/// Get code from 1C Configurator window
#[tauri::command]
pub fn get_code_from_configurator(hwnd: isize, use_select_all: Option<bool>) -> Result<String, String> {
    crate::app_log!("[1C] get_code (HWND: {}, select_all: {:?})", hwnd, use_select_all);
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::get_selected_code(hwnd, use_select_all.unwrap_or(false))
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        let _ = use_select_all;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Get active fragment from 1C Configurator window
#[tauri::command]
pub fn get_active_fragment_cmd(hwnd: isize) -> Result<String, String> {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::get_active_fragment(hwnd)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Paste code to 1C Configurator window with conflict detection
#[tauri::command]
pub async fn paste_code_to_configurator<R: Runtime>(
    app_handle: AppHandle<R>,
    hwnd: isize,
    code: String,
    use_select_all: Option<bool>,
    original_content: Option<String>,
) -> Result<(), String> {
    crate::app_log!("[1C] paste_code (HWND: {}, len: {})", hwnd, code.len());
    #[cfg(windows)]
    {
        use crate::configurator;
        use crate::history_manager;
        
        let select_all = use_select_all.unwrap_or(false);
        
        // 1. Read current code for conflict detection & snapshot
        if let Ok(current_code) = configurator::get_selected_code(hwnd, select_all) {
            // 2. Conflict detection
            if let Some(ref original) = original_content {
                let original_hash = configurator::calculate_content_hash(original);
                let current_hash = configurator::calculate_content_hash(&current_code);
                
                if original_hash != current_hash {
                    return Err("CONFLICT: Код в Конфигураторе был изменён с момента последнего чтения. Получите код заново перед применением.".to_string());
                }
            }
            
            // 3. Save snapshot for undo
            history_manager::save_snapshot(hwnd, current_code).await;
        }
        
        // 4. Paste code
        let result = configurator::paste_code(hwnd, &code, select_all);
        
        if result.is_ok() {
            let _ = app_handle.emit("RESET_DIFF", code);
        }
        
        result
    }
    #[cfg(not(windows))]
    {
        let _ = app_handle;
        let _ = hwnd;
        let _ = code;
        let _ = use_select_all;
        let _ = original_content;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Align AI window with Configurator
#[tauri::command]
pub fn align_with_configurator(app_handle: AppHandle, hwnd: isize) -> Result<(), String> {
    #[cfg(windows)]
    {
        use crate::configurator;
        let ai_window = app_handle.get_webview_window("main").ok_or("Main window not found")?;
        let ai_hwnd = ai_window.hwnd().map_err(|e| e.to_string())?;
        
        configurator::align_windows(hwnd, ai_hwnd.0 as isize)
    }
    #[cfg(not(windows))]
    {
        let _ = app_handle;
        let _ = hwnd;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Undo last code change in 1C Configurator
#[tauri::command]
pub async fn undo_last_change(hwnd: isize) -> Result<(), String> {
    crate::app_log!("[1C] undo_last_change (HWND: {})", hwnd);
    #[cfg(windows)]
    {
        use crate::configurator;
        use crate::history_manager;
        
        if let Some(snapshot) = history_manager::pop_snapshot(hwnd).await {
            configurator::paste_code(hwnd, &snapshot.original_code, true)
        } else {
            Err("No history for this window".to_string())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err("Configurator integration is only available on Windows".to_string())
    }
}

/// Set RDP compatibility mode for Configurator keyboard operations
#[tauri::command]
pub fn set_configurator_rdp_mode(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::set_rdp_mode(enabled);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Ok(())
    }
}

/// Send a hotkey combination to 1C Configurator
#[tauri::command]
pub fn send_hotkey_cmd(hwnd: isize, key: u16, modifiers: Vec<u16>) -> Result<(), String> {
    #[cfg(windows)]
    {
        use crate::configurator;
        configurator::send_hotkey(hwnd, key, modifiers);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        let _ = key;
        let _ = modifiers;
        Err("Hotkeys are only available on Windows".to_string())
    }
}
