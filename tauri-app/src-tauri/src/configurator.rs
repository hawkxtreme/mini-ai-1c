//! 1C Configurator integration using Windows APIs
//! Handles window detection, hotkeys, and clipboard operations

use windows::{
    Win32::Foundation::{HWND, MAX_PATH},
    Win32::System::Threading::{
        AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    },
    Win32::System::ProcessStatus::K32GetModuleFileNameExW,
    Win32::UI::Input::KeyboardAndMouse::{
        SendInput, SetFocus, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_A, VK_C, VK_V, VK_MENU,
    },
    Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, IsIconic,
        SetForegroundWindow, ShowWindow, SW_RESTORE,
    },
};

fn send_ctrl_a() {
    unsafe {
        let ctrl_a_inputs = vec![
            // Ctrl down
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_CONTROL, ..Default::default() },
                },
            },
            // A down
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_A, ..Default::default() },
                },
            },
            // A up
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_A, dwFlags: KEYEVENTF_KEYUP, ..Default::default() },
                },
            },
            // Ctrl up
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_CONTROL, dwFlags: KEYEVENTF_KEYUP, ..Default::default() },
                },
            },
        ];
        SendInput(&ctrl_a_inputs, std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

#[cfg(windows)]
use std::sync::Mutex;

/// Window information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
}

#[cfg(windows)]
static FOUND_WINDOWS: Mutex<Vec<WindowInfo>> = Mutex::new(Vec::new());

/// Find windows matching a pattern
#[cfg(windows)]
pub fn find_configurator_windows(pattern: &str) -> Vec<WindowInfo> {
    // Clear previous results
    if let Ok(mut windows) = FOUND_WINDOWS.lock() {
        windows.clear();
    }
    
    // Store pattern for callback
    let pattern_lower = pattern.to_lowercase();
    
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_callback),
            windows::Win32::Foundation::LPARAM(0),
        );
    }
    
    // Filter by pattern
    if let Ok(windows) = FOUND_WINDOWS.lock() {
        windows
            .iter()
            .filter(|w| w.title.to_lowercase().contains(&pattern_lower))
            .cloned()
            .collect()
    } else {
        Vec::new()
    }
}

#[cfg(windows)]
unsafe extern "system" fn enum_windows_callback(
    hwnd: HWND,
    _lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::BOOL {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    
    if !IsWindowVisible(hwnd).as_bool() {
        return windows::Win32::Foundation::BOOL::from(true);
    }

    // Check process name
    let mut process_id = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    
    if let Ok(process_handle) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, process_id) {
        let mut buffer = [0u16; MAX_PATH as usize];
        let len = K32GetModuleFileNameExW(process_handle, None, &mut buffer);
        let _ = windows::Win32::Foundation::CloseHandle(process_handle); // Always close handle

        if len > 0 {
            let process_path = OsString::from_wide(&buffer[..len as usize])
                .to_string_lossy()
                .to_string()
                .to_lowercase();
            
            // Allow 1cv8 (Client/Configurator), 1cv8c (Thin Client), 1cv8s (Thick Client)
            // But usually Configurator runs as 1cv8.exe
            let is_1c = process_path.ends_with("1cv8.exe") || 
                        process_path.ends_with("1cv8c.exe") || 
                        process_path.ends_with("1cv8s.exe");
            
            if !is_1c {
                return windows::Win32::Foundation::BOOL::from(true);
            }
        }
    }
    
    let mut buffer = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut buffer);
    
    if len > 0 {
        let title = OsString::from_wide(&buffer[..len as usize])
            .to_string_lossy()
            .to_string();
        
        if !title.is_empty() {
            if let Ok(mut windows) = FOUND_WINDOWS.lock() {
                windows.push(WindowInfo {
                    hwnd: hwnd.0 as isize,
                    title,
                });
            }
        }
    }
    
    windows::Win32::Foundation::BOOL::from(true)
}

/// Get selected code from configurator window using Ctrl+C
#[cfg(windows)]
pub fn get_selected_code(hwnd: isize, use_select_all: bool) -> Result<String, String> {
    use clipboard_win::{formats, get_clipboard};
    
    let window = HWND(hwnd as *mut std::ffi::c_void);
    
    unsafe {
        // Always restore and focus functionality
        
        // Restore window if minimized
        if IsIconic(window).as_bool() {
            ShowWindow(window, SW_RESTORE);
        }
        
        // "Alt-key" trick to bypass SetForegroundWindow restrictions
        let alt_inputs = vec![
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_MENU,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_MENU,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
        ];
        SendInput(&alt_inputs, std::mem::size_of::<INPUT>() as i32);
        
        // Robustly bring to foreground
        let target_thread_id = GetWindowThreadProcessId(window, None);
        let current_thread_id = GetCurrentThreadId();
        
        let mut attached = false;
        if target_thread_id != current_thread_id {
            attached = AttachThreadInput(current_thread_id, target_thread_id, true).as_bool();
        }
        
        SetForegroundWindow(window);
        SetFocus(window);
        
        if attached {
            AttachThreadInput(current_thread_id, target_thread_id, false);
        }
        
        std::thread::sleep(std::time::Duration::from_millis(300));
        
        if use_select_all {
            // Send Ctrl+A
            send_ctrl_a();
        }
    
        // Send Ctrl+C
        let ctrl_c_inputs = vec![
            // Ctrl down
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_CONTROL, ..Default::default() },
                },
            },
            // C down
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_C, ..Default::default() },
                },
            },
            // C up
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_C, dwFlags: KEYEVENTF_KEYUP, ..Default::default() },
                },
            },
            // Ctrl up
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_CONTROL, dwFlags: KEYEVENTF_KEYUP, ..Default::default() },
                },
            },
        ];
        SendInput(&ctrl_c_inputs, std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    
    // Retry loop for clipboard
    let mut retries = 5;
    while retries > 0 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        match get_clipboard::<String, _>(formats::Unicode) {
            Ok(content) => return Ok(content),
            Err(_) => {
                retries -= 1;
                continue;
            }
        }
    }
    
    Err("Failed to get clipboard content after retries".to_string())
}

/// Get active fragment (selection or current line)
#[cfg(windows)]
pub fn get_active_fragment(hwnd: isize) -> Result<String, String> {
    // 1. Try to get selection first
    if let Ok(selection) = get_selected_code(hwnd, false) {
        if !selection.trim().is_empty() {
            return Ok(selection);
        }
    }

    // 2. Fallback: Try to select current line if nothing is selected
    // Note: 1C Configurator doesn't have a simple "select current line" hotkey, 
    // but we can try Home, Shift+End or similar if needed.
    // For now, let's just return what we got or empty.
    
    Err("No selection or active fragment found".to_string())
}

/// Paste code into configurator window
#[cfg(windows)]
pub fn paste_code(hwnd: isize, code: &str, use_select_all: bool) -> Result<(), String> {
    use clipboard_win::{formats, set_clipboard};
    
    // Set clipboard content
    set_clipboard(formats::Unicode, code)
        .map_err(|e| e.to_string())?;

    println!("[Configurator] Clipboard updated, focusing window: {}", hwnd);
    
    unsafe {
        let window = HWND(hwnd as *mut std::ffi::c_void);
        let current_thread_id = GetCurrentThreadId();
        let target_thread_id = GetWindowThreadProcessId(window, None);
        
        let mut attached = false;
        if current_thread_id != target_thread_id {
            let res = AttachThreadInput(current_thread_id, target_thread_id, true);
            attached = res.as_bool();
            println!("[Configurator] Attached to thread: {}", attached);
        }
        
        // Force window to foreground
        if IsIconic(window).as_bool() {
             let _ = ShowWindow(window, SW_RESTORE);
        }
        
        let success = SetForegroundWindow(window);
         println!("[Configurator] SetForegroundWindow result: {:?}", success);
         
        if !success.as_bool() {
             // Try aggressive approach
             // let _ = keybd_event(0, 0, Default::default(), 0); // Not available in current bindings
             let _ = SetForegroundWindow(window);
        }

        let _ = SetFocus(window);
        
        if attached {
            AttachThreadInput(current_thread_id, target_thread_id, false);
        }
        
        std::thread::sleep(std::time::Duration::from_millis(100)); // Wait for focus
        println!("[Configurator] Sending inputs...");

        if use_select_all {
             send_ctrl_a();
             std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Send Ctrl+V using SendInput (more reliable than WM_PASTE)
        let ctrl_v_inputs = vec![
            // Ctrl down
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_CONTROL, ..Default::default() },
                },
            },
            // V down
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_V, ..Default::default() },
                },
            },
            // V up
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_V, dwFlags: KEYEVENTF_KEYUP, ..Default::default() },
                },
            },
            // Ctrl up
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: VK_CONTROL, dwFlags: KEYEVENTF_KEYUP, ..Default::default() },
                },
            },
        ];
        
        SendInput(&ctrl_v_inputs, std::mem::size_of::<INPUT>() as i32);
        println!("[Configurator] Sent Ctrl+V inputs");
    }
    
    Ok(())
}


// Non-Windows stubs
#[cfg(not(windows))]
pub fn find_configurator_windows(_pattern: &str) -> Vec<WindowInfo> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn get_selected_code(_hwnd: isize) -> Result<String, String> {
    Err("Configurator integration is only available on Windows".to_string())
}

#[cfg(not(windows))]
pub fn paste_code(_hwnd: isize, _code: &str, _use_select_all: bool) -> Result<(), String> {
    Err("Configurator integration is only available on Windows".to_string())
}
