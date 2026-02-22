//! 1C Configurator integration using Windows APIs
//! Handles window detection, hotkeys, and clipboard operations

use windows::{
    Win32::Foundation::{HWND, MAX_PATH, RECT},
    Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    },
    Win32::System::Threading::{
        AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    },
    Win32::System::ProcessStatus::K32GetModuleFileNameExW,
    Win32::UI::Input::KeyboardAndMouse::{
        SendInput, SetFocus, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_A, VK_C, VK_V, VK_MENU, VK_SHIFT, VK_UP,
    },
    Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, IsIconic, IsZoomed,
        SetForegroundWindow, ShowWindow, SW_RESTORE, MoveWindow, GetWindowRect,
    },
    Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS},
};

/// Calculate a simple hash of content for conflict detection
pub fn calculate_content_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    content.trim().hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

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

/// Send Shift+Up `count` times to re-select pasted lines
fn send_shift_up(count: usize) {
    unsafe {
        // First: send Home to go to beginning of current line
        let home_inputs = vec![
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: windows::Win32::UI::Input::KeyboardAndMouse::VK_HOME, ..Default::default() },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT { wVk: windows::Win32::UI::Input::KeyboardAndMouse::VK_HOME, dwFlags: KEYEVENTF_KEYUP, ..Default::default() },
                },
            },
        ];
        SendInput(&home_inputs, std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Then: Shift+Up for each line to select upward
        for _ in 0..count {
            let shift_up = vec![
                // Shift down
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                        ki: KEYBDINPUT { wVk: VK_SHIFT, ..Default::default() },
                    },
                },
                // Up down
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                        ki: KEYBDINPUT { wVk: VK_UP, ..Default::default() },
                    },
                },
                // Up up
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                        ki: KEYBDINPUT { wVk: VK_UP, dwFlags: KEYEVENTF_KEYUP, ..Default::default() },
                    },
                },
                // Shift up
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                        ki: KEYBDINPUT { wVk: VK_SHIFT, dwFlags: KEYEVENTF_KEYUP, ..Default::default() },
                    },
                },
            ];
            SendInput(&shift_up, std::mem::size_of::<INPUT>() as i32);
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
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
            let _ = ShowWindow(window, SW_RESTORE);
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
        
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(window);
        
        if attached {
            let _ = AttachThreadInput(current_thread_id, target_thread_id, false);
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
            let _ = AttachThreadInput(current_thread_id, target_thread_id, false);
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
        
        // Wait for paste to complete
        std::thread::sleep(std::time::Duration::from_millis(300));
        
        // Restore selection
        if use_select_all {
            // For full module: re-select all
            println!("[Configurator] Re-selecting all after paste");
            send_ctrl_a();
        } else {
            // For fragment: try to re-select pasted lines using Shift+Up
            let line_count = code.lines().count();
            if line_count > 1 {
                println!("[Configurator] Re-selecting {} lines after paste", line_count - 1);
                send_shift_up(line_count - 1);
            }
        }
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

/// Check if there is an active selection in the window
pub fn is_selection_active(hwnd: isize) -> bool {
    #[cfg(windows)]
    {
        use clipboard_win::{empty, formats, get_clipboard};
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        
        let window = windows::Win32::Foundation::HWND(hwnd as *mut std::ffi::c_void);
        
        unsafe {
            // 1. Clear clipboard
            let _ = empty();
            
            // 2. Focus window
            if windows::Win32::UI::WindowsAndMessaging::IsIconic(window).as_bool() {
                let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(window, windows::Win32::UI::WindowsAndMessaging::SW_RESTORE);
            }
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(window);
            std::thread::sleep(std::time::Duration::from_millis(150));
            
            // 3. Send Ctrl+C
            let inputs = vec![
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                        ki: KEYBDINPUT { wVk: VK_CONTROL, ..Default::default() },
                    },
                },
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                        ki: KEYBDINPUT { wVk: VK_C, ..Default::default() },
                    },
                },
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                        ki: KEYBDINPUT { wVk: VK_C, dwFlags: KEYEVENTF_KEYUP, ..Default::default() },
                    },
                },
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                        ki: KEYBDINPUT { wVk: VK_CONTROL, dwFlags: KEYEVENTF_KEYUP, ..Default::default() },
                    },
                },
            ];
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            std::thread::sleep(std::time::Duration::from_millis(200));
            
            // 4. Check if clipboard is NOT empty
            match get_clipboard::<String, _>(formats::Unicode) {
                Ok(content) => !content.is_empty(),
                Err(_) => false,
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        false
    }
}

/// Get visual offsets caused by invisible window borders (DWM frames)
#[cfg(windows)]
unsafe fn get_window_visual_offsets(hwnd: HWND) -> (i32, i32, i32, i32) {
    let mut window_rect = RECT::default();
    if GetWindowRect(hwnd, &mut window_rect).is_err() {
        return (0, 0, 0, 0);
    }
    
    let mut extended_rect = RECT::default();
    let res = DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut extended_rect as *mut _ as *mut _,
        std::mem::size_of::<RECT>() as u32,
    );
    
    if res.is_err() {
        return (0, 0, 0, 0);
    }
    
    (
        extended_rect.left - window_rect.left,     // Left offset
        extended_rect.top - window_rect.top,       // Top offset
        window_rect.right - extended_rect.right,   // Right offset
        window_rect.bottom - extended_rect.bottom, // Bottom offset
    )
}
/// Align AI window and Configurator window side by side
#[cfg(windows)]
pub fn align_windows(configurator_hwnd: isize, ai_hwnd: isize) -> Result<(), String> {
    let conf_window = HWND(configurator_hwnd as *mut std::ffi::c_void);
    let ai_window = HWND(ai_hwnd as *mut std::ffi::c_void);

    unsafe {
        println!("[Configurator] Aligning windows (PIXEL-PERFECT V2): CONF={} AI={}", configurator_hwnd, ai_hwnd);

        // 1. PHASE ONE: Preparation (Restore and wait)
        let mut needs_delay = false;
        if IsIconic(conf_window).as_bool() || IsZoomed(conf_window).as_bool() {
            println!("[Configurator] Restoring Configurator");
            let _ = ShowWindow(conf_window, SW_RESTORE);
            needs_delay = true;
        }
        if IsIconic(ai_window).as_bool() || IsZoomed(ai_window).as_bool() {
            println!("[Configurator] Restoring AI window");
            let _ = ShowWindow(ai_window, SW_RESTORE);
            needs_delay = true;
        }

        if needs_delay {
            // Wait for OS to finish animations and update window state
            std::thread::sleep(std::time::Duration::from_millis(300));
        }

        // 2. PHASE TWO: Measurement (Post-restore)
        
        // Use Configurator's monitor as primary work screen
        let monitor = MonitorFromWindow(conf_window, MONITOR_DEFAULTTONEAREST);
        let mut monitor_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
            return Err("Failed to get monitor info".to_string());
        }

        let work_area = monitor_info.rcWork;
        let screen_width = work_area.right - work_area.left;
        let screen_height = work_area.bottom - work_area.top;

        // Get visual offsets for correct border alignment
        let (c_l, c_t, c_r, c_b) = get_window_visual_offsets(conf_window);
        let (a_l, a_t, a_r, a_b) = get_window_visual_offsets(ai_window);

        // Get CURRENT AI window width instead of assumption
        let mut ai_rect = RECT::default();
        let _ = GetWindowRect(ai_window, &mut ai_rect);
        let current_ai_logical_width = ai_rect.right - ai_rect.left;
        let ai_visual_width = current_ai_logical_width - a_l - a_r;
        
        // Use current width but clamp it to reasonable range (400 to 650)
        let ai_width = ai_visual_width.clamp(400, 650);
        
        // --- SAFETY MARGIN STRATEGY ---
        let margin = 7; // Pixels from edges and between windows
        
        // Available width for both windows minus margins (left, middle, right)
        let available_width = screen_width - (margin * 3);
        let conf_width = available_width - ai_width;

        println!("[Configurator] Screen {}x{}, Target AI: {}, Conf: {}, Margin: {}", 
            screen_width, screen_height, ai_width, conf_width, margin);

        // 3. PHASE THREE: Movement
        
        // Move Configurator to the left side with margin
        let conf_x = work_area.left + margin - c_l;
        let conf_y = work_area.top + margin - c_t;
        let conf_w = conf_width + c_l + c_r;
        let conf_h = screen_height - (margin * 2) + c_t + c_b;
        
        println!("[Configurator] Move CONF: X={}, Y={}, W={}, H={}", conf_x, conf_y, conf_w, conf_h);
        let _ = MoveWindow(conf_window, conf_x, conf_y, conf_w, conf_h, true);
        
        // Move AI window to the right side with margin
        let visual_ai_x = work_area.left + conf_width + (margin * 2);
        let ai_x = visual_ai_x - a_l;
        let ai_y = work_area.top + margin - a_t;
        let ai_w = ai_width + a_l + a_r;
        let ai_h = screen_height - (margin * 2) + a_t + a_b;

        println!("[Configurator] Move AI: X={}, Y={}, W={}, H={}", ai_x, ai_y, ai_w, ai_h);
        let _ = MoveWindow(ai_window, ai_x, ai_y, ai_w, ai_h, true);

        // Final focus
        let _ = SetForegroundWindow(ai_window);
        let _ = SetFocus(ai_window);
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn align_windows(_configurator_hwnd: isize, _ai_hwnd: isize) -> Result<(), String> {
    Err("Window alignment is only available on Windows".to_string())
}
