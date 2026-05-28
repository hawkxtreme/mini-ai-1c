//! Overlay window management commands.
//! Controls the frameless always-on-top AI popup window.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

static OVERLAY_READY: AtomicBool = AtomicBool::new(false);
static PENDING_OVERLAY_STATE: Mutex<Option<OverlayState>> = Mutex::new(None);

fn fallback_window_size(state: &OverlayState) -> (u32, u32) {
    match state.phase.as_str() {
        "menu" => (316, 420),
        "input" => (396, 320),
        "loading" => (404, 168),
        "result" => {
            if state.result_type == Some(ActionResultType::ExplainOnly) {
                (548, 420)
            } else {
                (500, 430)
            }
        }
        _ => (396, 320),
    }
}

fn logical_to_physical_size(scale_factor: f64, width: u32, height: u32) -> (u32, u32) {
    let width = ((width as f64) * scale_factor).round().max(1.0) as u32;
    let height = ((height as f64) * scale_factor).round().max(1.0) as u32;
    (width, height)
}

fn set_overlay_size(
    window: &WebviewWindow,
    logical_width: u32,
    logical_height: u32,
) -> Result<(), String> {
    let scale_factor = window.scale_factor().map_err(|e| e.to_string())?;
    let (physical_width, physical_height) =
        logical_to_physical_size(scale_factor, logical_width, logical_height);

    window
        .set_size(tauri::PhysicalSize::new(physical_width, physical_height))
        .map_err(|e| e.to_string())
}

fn get_or_create_overlay_window(app_handle: &AppHandle) -> Result<(WebviewWindow, bool), String> {
    if let Some(window) = app_handle.get_webview_window("overlay") {
        return Ok((window, false));
    }

    OVERLAY_READY.store(false, Ordering::SeqCst);

    let window = WebviewWindowBuilder::new(
        app_handle,
        "overlay",
        WebviewUrl::App("overlay.html".into()),
    )
    .title("mini-ai overlay")
    .inner_size(372.0, 234.0)
    .visible(false)
    .focused(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .build()
    .map_err(|e| e.to_string())?;

    Ok((window, true))
}

fn queue_overlay_state(state: &OverlayState) {
    if let Ok(mut guard) = PENDING_OVERLAY_STATE.lock() {
        *guard = Some(state.clone());
    }
}

fn emit_pending_overlay_state(app_handle: &AppHandle) -> Result<(), String> {
    let pending = PENDING_OVERLAY_STATE
        .lock()
        .map_err(|e| e.to_string())?
        .clone();

    if let Some(state) = pending {
        if let Some(overlay) = app_handle.get_webview_window("overlay") {
            overlay
                .emit("overlay-state", &state)
                .map_err(|e| e.to_string())?;
        } else {
            app_handle
                .emit("overlay-state", &state)
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

// ─── Data types ──────────────────────────────────────────────────────────────

/// Result contract differs by action type.
/// This enum drives both the overlay UI and the paste logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActionResultType {
    /// Text comment to insert BEFORE Процедура/Функция line
    Comment,
    /// SEARCH/REPLACE diff — apply via diffViewer
    Diff,
    /// Explanation text — display only, nothing to paste
    ExplainOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayState {
    /// "menu" | "input" | "loading" | "result"
    pub phase: String,
    /// "describe" | "elaborate" | "fix" | "explain"
    pub action: Option<String>,
    /// Drives paste logic and result UI buttons
    pub result_type: Option<ActionResultType>,
    /// First ~6 lines of result for preview
    pub preview: Option<String>,
    /// Final code to paste back into Configurator
    pub result_code: Option<String>,
    /// Raw SEARCH/REPLACE diff for the main diff viewer
    pub diff_content: Option<String>,
    /// Configurator HWND — needed to paste back
    pub conf_hwnd: isize,
    /// Original captured code — for conflict detection
    pub original_code: Option<String>,
    /// Whether full module was captured (Ctrl+A)
    pub use_select_all: bool,
    pub write_intent: Option<String>,
    pub can_apply_directly: Option<bool>,
    pub apply_unavailable_reason: Option<String>,
    pub preferred_writer: Option<String>,
    pub caret_line: Option<i32>,
    pub method_start_line: Option<i32>,
    pub method_name: Option<String>,
    pub runtime_id: Option<String>,
    pub target_x: Option<i32>,
    pub target_y: Option<i32>,
    pub target_child_hwnd: Option<isize>,
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

/// Show the overlay window at the cursor position where the right-click occurred.
/// The native 1C context menu is suppressed by the mouse hook, so our overlay
/// is the only "context menu" shown.
///
/// cursor_x / cursor_y — virtual screen coordinates from WH_MOUSE_LL (support multi-monitor).
#[tauri::command]
pub async fn show_overlay(
    app_handle: AppHandle,
    conf_hwnd: isize,
    cursor_x: i32,
    cursor_y: i32,
    state: OverlayState,
) -> Result<(), String> {
    let _ = conf_hwnd; // kept in signature for future use (paste-back)

    let (overlay, _) = get_or_create_overlay_window(&app_handle)?;
    queue_overlay_state(&state);
    let (fallback_w, fallback_h) = fallback_window_size(&state);

    #[cfg(windows)]
    let (fallback_physical_w, fallback_physical_h) = {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };

        // Get usable area of the monitor containing the cursor
        let pt = POINT {
            x: cursor_x,
            y: cursor_y,
        };
        let (fallback_physical_w, fallback_physical_h, mx, my, mw, mh) = unsafe {
            let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(hmon, &mut info);
            let mut dpi_x = 96u32;
            let mut dpi_y = 96u32;
            let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let scale_factor = (dpi_x as f64 / 96.0).max(1.0);
            let (fallback_physical_w, fallback_physical_h) =
                logical_to_physical_size(scale_factor, fallback_w, fallback_h);
            let r = info.rcWork; // work area (excludes taskbar)
            (
                fallback_physical_w,
                fallback_physical_h,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
            )
        };

        // Reserve enough physical space for the target monitor DPI before the
        // frontend performs its precise resize pass.
        let ow: i32 = fallback_physical_w as i32;
        let oh: i32 = fallback_physical_h as i32;

        // Place overlay just to the right-and-below the cursor.
        // Clamp so it doesn't overflow the monitor.
        let mut ox = cursor_x + 4;
        let mut oy = cursor_y + 4;

        if ox + ow > mx + mw {
            ox = cursor_x - ow - 4;
        }
        if oy + oh > my + mh {
            oy = cursor_y - oh - 4;
        }

        overlay
            .set_position(tauri::PhysicalPosition::new(ox, oy))
            .map_err(|e| e.to_string())?;
        (fallback_physical_w, fallback_physical_h)
    };

    #[cfg(not(windows))]
    let (fallback_physical_w, fallback_physical_h) = (fallback_w, fallback_h);

    // Show window first — WebView2 throttles JS in hidden windows,
    // so the event would be dropped if we emitted before show().
    overlay
        .set_size(tauri::PhysicalSize::new(
            fallback_physical_w,
            fallback_physical_h,
        ))
        .map_err(|e| e.to_string())?;
    overlay.show().map_err(|e| e.to_string())?;
    overlay.set_focus().map_err(|e| e.to_string())?;

    if OVERLAY_READY.load(Ordering::SeqCst) {
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        emit_pending_overlay_state(&app_handle)?;
    }

    Ok(())
}

/// Update overlay state (phase transitions: menu→loading, loading→result, etc.)
/// Also resizes the window to fit the new phase content.
#[tauri::command]
pub fn update_overlay_state(app_handle: AppHandle, state: OverlayState) -> Result<(), String> {
    let (overlay, _) = get_or_create_overlay_window(&app_handle)?;
    queue_overlay_state(&state);

    // Resize window based on phase and content
    let (w, h) = fallback_window_size(&state);
    set_overlay_size(&overlay, w, h)?;

    if OVERLAY_READY.load(Ordering::SeqCst) {
        emit_pending_overlay_state(&app_handle)?;
    }

    Ok(())
}

/// Resize overlay window to fit dynamic content (called from frontend after render).
#[tauri::command]
pub fn resize_overlay(app_handle: AppHandle, width: u32, height: u32) -> Result<(), String> {
    let (overlay, _) = get_or_create_overlay_window(&app_handle)?;
    let width = width.clamp(280, 760);
    let height = height.clamp(120, 560);
    set_overlay_size(&overlay, width, height)
}

#[tauri::command]
pub fn overlay_ready(app_handle: AppHandle) -> Result<(), String> {
    OVERLAY_READY.store(true, Ordering::SeqCst);
    emit_pending_overlay_state(&app_handle)
}

#[tauri::command]
pub fn get_pending_overlay_state() -> Result<Option<OverlayState>, String> {
    PENDING_OVERLAY_STATE
        .lock()
        .map(|guard| guard.clone())
        .map_err(|e| e.to_string())
}

/// Hide the overlay.
/// Optionally return focus to the Configurator when the caller is about to paste/apply.
#[tauri::command]
pub fn hide_overlay(
    app_handle: AppHandle,
    conf_hwnd: Option<isize>,
    restore_focus: Option<bool>,
) -> Result<(), String> {
    let Some(overlay) = app_handle.get_webview_window("overlay") else {
        return Ok(());
    };

    if let Ok(mut guard) = PENDING_OVERLAY_STATE.lock() {
        *guard = None;
    }

    overlay.hide().map_err(|e| e.to_string())?;

    // Only return focus to Configurator when explicitly requested by the caller.
    #[cfg(windows)]
    if restore_focus.unwrap_or(false) {
        if let Some(hwnd) = conf_hwnd.filter(|&h| h != 0) {
            unsafe {
                let window = HWND(hwnd as *mut std::ffi::c_void);
                let _ = SetForegroundWindow(window);
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub fn show_hidden_overlay(app_handle: AppHandle) -> Result<(), String> {
    let Some(overlay) = app_handle.get_webview_window("overlay") else {
        return Ok(());
    };

    overlay.show().map_err(|e| e.to_string())?;
    overlay.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

/// Emit an event to the main window (used by overlay to trigger actions).
/// This lets overlay → main window communication without importing app logic in Rust.
#[tauri::command]
pub fn emit_to_main(
    app_handle: AppHandle,
    event: String,
    payload: serde_json::Value,
) -> Result<(), String> {
    let main = app_handle
        .get_webview_window("main")
        .ok_or("Main window not found")?;
    main.emit(&event, payload).map_err(|e| e.to_string())?;
    Ok(())
}

/// Open the diff editor in the main window, then hide overlay.
/// Used by the "≡ Диф" button.
#[tauri::command]
pub fn open_diff_from_overlay(
    app_handle: AppHandle,
    diff_content: String,
    original_code: Option<String>,
    conf_hwnd: isize,
    use_select_all: bool,
) -> Result<(), String> {
    let main = app_handle
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    #[derive(Clone, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct DiffPayload {
        diff_content: String,
        original_code: Option<String>,
        conf_hwnd: isize,
        use_select_all: bool,
    }

    main.emit(
        "open-diff-from-overlay",
        DiffPayload {
            diff_content,
            original_code,
            conf_hwnd,
            use_select_all,
        },
    )
    .map_err(|e| e.to_string())?;

    main.show().map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())?;

    // Hide overlay — do NOT return focus to conf here (main window takes over)
    let overlay = app_handle
        .get_webview_window("overlay")
        .ok_or("Overlay window not found")?;
    overlay.hide().map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn focus_main_window_for_overlay_chat(app_handle: AppHandle) -> Result<(), String> {
    let main = app_handle
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    let was_always_on_top = main.is_always_on_top().map_err(|e| e.to_string())?;

    main.show().map_err(|e| e.to_string())?;
    let _ = main.unminimize();
    // Временно ставим topmost чтобы окно вышло вперёд, затем сразу восстанавливаем
    main.set_always_on_top(true).map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())?;
    main.set_always_on_top(was_always_on_top)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn set_main_window_always_on_top(
    app_handle: AppHandle,
    always_on_top: bool,
) -> Result<(), String> {
    let main = app_handle
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    main.set_always_on_top(always_on_top)
        .map_err(|e| e.to_string())
}

/// Simple non-streaming LLM call for quick actions.
/// Uses the active profile to call the LLM API directly.
#[tauri::command]
pub async fn quick_chat_invoke(prompt: String) -> Result<String, String> {
    use crate::llm_profiles::{get_active_profile, LLMProvider};

    fn persist_qwen_usage_headers(profile_id: &str, headers: &reqwest::header::HeaderMap) {
        let limit = headers
            .get("x-ratelimit-limit-requests")
            .or_else(|| headers.get("x-ratelimit-requests-limit"))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());
        let remaining = headers
            .get("x-ratelimit-remaining-requests")
            .or_else(|| headers.get("x-ratelimit-requests-remaining"))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());
        let reset = headers
            .get("x-ratelimit-reset-requests")
            .or_else(|| headers.get("x-ratelimit-requests-reset"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if let (Some(limit), Some(remaining)) = (limit, remaining) {
            let used = limit.saturating_sub(remaining);
            let _ = crate::llm::cli_providers::qwen::QwenCliProvider::save_usage(
                profile_id, used, limit, reset,
            );
        } else {
            crate::llm::cli_providers::qwen::QwenCliProvider::increment_request_count(profile_id);
        }
    }

    let profile = get_active_profile().ok_or("Нет активного LLM профиля")?;

    if matches!(profile.provider, LLMProvider::OneCNaparnik) {
        return Err("1С:Напарник не поддерживается для быстрых действий".to_string());
    }

    if matches!(profile.provider, LLMProvider::CodexCli) {
        return crate::ai::codex_client::quick_codex_invoke(prompt).await;
    }

    // Qwen CLI uses OAuth token + portal.qwen.ai/v1 (OpenAI-compatible)
    let (api_key, raw_url) = if matches!(profile.provider, LLMProvider::QwenCli) {
        use crate::llm::cli_providers::qwen::QwenCliProvider;
        let token_info =
            QwenCliProvider::get_token(&profile.id)?.ok_or("Qwen CLI: Требуется авторизация")?;
        let (access_token, refresh_token, expires_at, _resource_url) = token_info;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Refresh if expired or expiring in <5 min
        let access_token = if expires_at > 0 && now + 300 >= expires_at {
            if let Some(rt) = refresh_token {
                match QwenCliProvider::refresh_access_token(&profile.id, &rt).await {
                    Ok(_) => {
                        QwenCliProvider::get_token(&profile.id)?
                            .ok_or("Qwen CLI: Токен не найден после обновления")?
                            .0
                    }
                    Err(_) => {
                        return Err(
                            "Qwen CLI: Токен истек, требуется повторная авторизация".to_string()
                        )
                    }
                }
            } else {
                return Err("Qwen CLI: Токен истек, требуется повторная авторизация".to_string());
            }
        } else {
            access_token
        };
        (access_token, "https://portal.qwen.ai/v1".to_string())
    } else {
        (profile.get_api_key(), profile.get_base_url())
    };

    let raw_url = raw_url;

    // Normalize URL for Ollama/LMStudio
    let base_url = {
        let trimmed = raw_url.trim_end_matches('/');
        if matches!(
            profile.provider,
            LLMProvider::Ollama | LLMProvider::LMStudio
        ) && !trimmed.ends_with("/v1")
        {
            format!("{}/v1", trimmed)
        } else {
            trimmed.to_string()
        }
    };

    let client = crate::http_client::build_http_client()?;

    // Anthropic uses a different API format
    if matches!(profile.provider, LLMProvider::Anthropic) {
        let url = format!("{}/messages", base_url);
        let body = serde_json::json!({
            "model": profile.model,
            "max_tokens": profile.max_tokens.min(4096),
            "messages": [{"role": "user", "content": prompt}]
        });

        let response = client
            .post(&url)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("Anthropic API error {}: {}", status, text));
        }

        let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        return Ok(json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string());
    }

    // OpenAI-compatible endpoint
    let url = format!("{}/chat/completions", base_url);
    let body = serde_json::json!({
        "model": profile.model,
        "max_tokens": profile.max_tokens.min(4096),
        "temperature": profile.temperature,
        "stream": false,
        "messages": [{"role": "user", "content": prompt}]
    });

    let mut req = client.post(&url).header("content-type", "application/json");

    if !api_key.is_empty() {
        req = req.header("authorization", format!("Bearer {}", api_key));
    }

    // Qwen CLI requires specific headers for portal.qwen.ai
    if matches!(profile.provider, LLMProvider::QwenCli) {
        req = req
            .header("User-Agent", "QwenCode/0.10.3 (darwin; arm64)")
            .header("X-Dashscope-Useragent", "QwenCode/0.10.3 (darwin; arm64)")
            .header("X-Dashscope-Authtype", "qwen-oauth");
    }

    let response = req.json(&body).send().await.map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("LLM API error {}: {}", status, text));
    }

    if matches!(profile.provider, LLMProvider::QwenCli) {
        persist_qwen_usage_headers(&profile.id, response.headers());
    }

    let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    Ok(json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}
