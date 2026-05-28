//! Global low-level mouse hook (WH_MOUSE_LL).
//! Detects right-click on 1C Configurator windows and emits Tauri events.
//!
//! The hook lives in mini-ai process and runs on a dedicated Windows message loop thread.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use windows::Win32::Foundation::MAX_PATH;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::ProcessStatus::K32GetModuleFileNameExW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetKeyState, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetAncestor, GetMessageW, GetWindowThreadProcessId, SetWindowsHookExW,
    UnhookWindowsHookEx, WindowFromPoint, GA_ROOT, HHOOK, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL,
    WM_RBUTTONDOWN, WM_RBUTTONUP,
};

static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
/// True while we're suppressing a Ctrl+RClick sequence (DOWN was suppressed -> UP must be too).
static SUPPRESS_RBUTTONUP: AtomicBool = AtomicBool::new(false);
/// Monotonic token for timeout-based stale suppression recovery.
static SUPPRESS_RBUTTONUP_TOKEN: AtomicU64 = AtomicU64::new(0);
/// The overlay event captured on RMB down and dispatched only after the click gesture completes.
static PENDING_OVERLAY_EVENT: OnceLock<Mutex<Option<PendingOverlayEvent>>> = OnceLock::new();
/// Enables the experimental overlay interception for Ctrl+RightClick in Configurator.
static EDITOR_BRIDGE_ENABLED: AtomicBool = AtomicBool::new(false);
/// Enables safer timing for Remote Desktop sessions.
static RDP_MODE: AtomicBool = AtomicBool::new(false);
const SUPPRESS_RBUTTONUP_TIMEOUT_MS: u64 = 1_500;

/// Shared AppHandle for use inside the hook callback (set once before installing hook).
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RightClickEvent {
    pub x: i32,
    pub y: i32,
    pub hwnd: isize,
    pub child_hwnd: isize,
}

#[derive(Clone)]
struct PendingOverlayEvent {
    event: RightClickEvent,
    defer_until_ctrl_release: bool,
}

fn pending_overlay_event_slot() -> &'static Mutex<Option<PendingOverlayEvent>> {
    PENDING_OVERLAY_EVENT.get_or_init(|| Mutex::new(None))
}

pub fn set_editor_bridge_enabled(enabled: bool) {
    EDITOR_BRIDGE_ENABLED.store(enabled, Ordering::Relaxed);
    if !enabled {
        clear_pending_overlay_event("bridge disabled");
        clear_rbuttonup_suppression("bridge disabled");
    }
}

pub fn set_rdp_mode(enabled: bool) {
    RDP_MODE.store(enabled, Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InterceptPlan {
    suppress_mouse_down: bool,
    suppress_mouse_up: bool,
    emit_overlay_immediately: bool,
    defer_overlay_until_ctrl_release: bool,
    post_ctrl_keyup_to_1c: bool,
}

fn build_intercept_plan(ctrl_held: bool, bridge_enabled: bool, rdp_mode: bool) -> InterceptPlan {
    let intercept = ctrl_held && bridge_enabled;
    InterceptPlan {
        suppress_mouse_down: intercept,
        suppress_mouse_up: intercept,
        // Never open the overlay on RMB down: switching focus mid-gesture can make the next
        // ordinary right-click in 1C feel "stuck". The overlay is emitted only after the paired
        // RMB up has been safely suppressed.
        emit_overlay_immediately: false,
        defer_overlay_until_ctrl_release: intercept && rdp_mode,
        post_ctrl_keyup_to_1c: false,
    }
}

fn next_rbuttonup_suppression_state(suppression_active: bool, plan: Option<InterceptPlan>) -> bool {
    match plan {
        Some(plan) if plan.suppress_mouse_down => plan.suppress_mouse_up,
        _ => {
            let _ = suppression_active;
            false
        }
    }
}

fn clear_rbuttonup_suppression(reason: &str) {
    if SUPPRESS_RBUTTONUP.swap(false, Ordering::Relaxed) {
        crate::app_log!("[MouseHook] Clearing RMB suppression: {}", reason);
    }
    SUPPRESS_RBUTTONUP_TOKEN.store(0, Ordering::Relaxed);
}

fn store_pending_overlay_event(event: RightClickEvent, defer_until_ctrl_release: bool) {
    if let Ok(mut slot) = pending_overlay_event_slot().lock() {
        *slot = Some(PendingOverlayEvent {
            event,
            defer_until_ctrl_release,
        });
    }
}

fn take_pending_overlay_event() -> Option<PendingOverlayEvent> {
    pending_overlay_event_slot()
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
}

fn clear_pending_overlay_event(reason: &str) {
    if let Some(pending) = take_pending_overlay_event() {
        crate::app_log!(
            "[MouseHook] Clearing pending overlay event for HWND={} ({})",
            pending.event.hwnd,
            reason
        );
    }
}

fn arm_rbuttonup_suppression() {
    let token = SUPPRESS_RBUTTONUP_TOKEN.fetch_add(1, Ordering::Relaxed) + 1;
    SUPPRESS_RBUTTONUP.store(true, Ordering::Relaxed);

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(SUPPRESS_RBUTTONUP_TIMEOUT_MS));
        if SUPPRESS_RBUTTONUP.load(Ordering::Relaxed)
            && SUPPRESS_RBUTTONUP_TOKEN.load(Ordering::Relaxed) == token
        {
            clear_pending_overlay_event("timeout auto-reset");
            clear_rbuttonup_suppression("timeout auto-reset");
        }
    });
}

fn build_right_click_event(x: i32, y: i32, target_hwnd: HWND, child_hwnd: HWND) -> RightClickEvent {
    RightClickEvent {
        x,
        y,
        hwnd: target_hwnd.0 as isize,
        child_hwnd: child_hwnd.0 as isize,
    }
}

fn emit_overlay_event(handle: &AppHandle, event: RightClickEvent) {
    let _ = handle.emit("configurator-rclick", event.clone());
    crate::app_log!(
        "[MouseHook] Ctrl+RClick on 1C at ({}, {}), HWND={}",
        event.x,
        event.y,
        event.hwnd
    );
}

fn emit_overlay_immediately(event: RightClickEvent) {
    if let Some(handle) = APP_HANDLE.get() {
        emit_overlay_event(handle, event);
    }
}

fn spawn_deferred_overlay_event(event: RightClickEvent) {
    let Some(handle) = APP_HANDLE.get().cloned() else {
        return;
    };

    std::thread::spawn(move || {
        for _ in 0..150 {
            let ctrl_still_held =
                unsafe { (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
            if !ctrl_still_held {
                emit_overlay_event(&handle, event);
                return;
            }

            std::thread::sleep(Duration::from_millis(10));
        }

        emit_overlay_event(&handle, event);
    });
}

/// Install the global mouse hook on a dedicated thread with a Windows message loop.
/// Call once from `setup()`. Idempotent - safe to call multiple times.
pub fn install_mouse_hook(app_handle: AppHandle) {
    if HOOK_INSTALLED.load(Ordering::Relaxed) {
        return;
    }

    std::thread::spawn(move || {
        let _ = APP_HANDLE.set(app_handle);

        unsafe {
            let hook = match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_callback), None, 0) {
                Ok(h) => h,
                Err(e) => {
                    crate::app_log!(force: true, "[MouseHook] Failed to install WH_MOUSE_LL hook: {}", e);
                    return;
                }
            };

            HOOK_INSTALLED.store(true, Ordering::Relaxed);
            crate::app_log!("[MouseHook] WH_MOUSE_LL installed");

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {}

            let _ = UnhookWindowsHookEx(hook);
            HOOK_INSTALLED.store(false, Ordering::Relaxed);
            crate::app_log!("[MouseHook] WH_MOUSE_LL uninstalled");
        }
    });
}

/// WH_MOUSE_LL callback - called for every global mouse event.
unsafe extern "system" fn mouse_hook_callback(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code < 0 {
        return CallNextHookEx(HHOOK::default(), n_code, w_param, l_param);
    }

    let msg = w_param.0 as u32;

    // 1C opens its context menu on WM_RBUTTONUP, so suppress the paired UP as well.
    if msg == WM_RBUTTONUP && SUPPRESS_RBUTTONUP.load(Ordering::Relaxed) {
        clear_rbuttonup_suppression("paired RMB up suppressed");
        if let Some(pending) = take_pending_overlay_event() {
            if pending.defer_until_ctrl_release {
                spawn_deferred_overlay_event(pending.event);
            } else {
                emit_overlay_immediately(pending.event);
            }
        }
        return LRESULT(1);
    }

    if msg == WM_RBUTTONDOWN {
        let hook_struct = &*(l_param.0 as *const MSLLHOOKSTRUCT);
        let pt = hook_struct.pt;
        let hwnd = WindowFromPoint(pt);
        let root_hwnd = GetAncestor(hwnd, GA_ROOT);
        let target_hwnd = if root_hwnd.0.is_null() {
            hwnd
        } else {
            root_hwnd
        };
        let suppression_active = SUPPRESS_RBUTTONUP.load(Ordering::Relaxed);
        let plan_for_down = if is_configurator_window(target_hwnd) {
            let ctrl_held = (GetKeyState(VK_CONTROL.0 as i32) & 0x8000u16 as i16) != 0;
            Some(build_intercept_plan(
                ctrl_held,
                EDITOR_BRIDGE_ENABLED.load(Ordering::Relaxed),
                RDP_MODE.load(Ordering::Relaxed),
            ))
        } else {
            None
        };

        let next_suppression = next_rbuttonup_suppression_state(suppression_active, plan_for_down);
        if next_suppression {
            arm_rbuttonup_suppression();
        } else {
            if suppression_active {
                crate::app_log!(
                    "[MouseHook] Clearing stale RMB suppression before processing a new right click"
                );
            }
            clear_pending_overlay_event("new right click does not require suppression");
            clear_rbuttonup_suppression("new right click does not require suppression");
        }

        if let Some(plan) = plan_for_down {
            if plan.suppress_mouse_down {
                let event = build_right_click_event(pt.x, pt.y, target_hwnd, hwnd);
                store_pending_overlay_event(event, plan.defer_overlay_until_ctrl_release);

                return LRESULT(1);
            }
        }
    }

    CallNextHookEx(HHOOK::default(), n_code, w_param, l_param)
}

/// Check if the given HWND belongs to a 1C Configurator process (1cv8.exe).
unsafe fn is_configurator_window(hwnd: HWND) -> bool {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    if hwnd.0.is_null() {
        return false;
    }

    let mut process_id = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut process_id));

    if process_id == 0 {
        return false;
    }

    let Ok(process_handle) = OpenProcess(
        PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
        false,
        process_id,
    ) else {
        return false;
    };

    let mut buffer = [0u16; MAX_PATH as usize];
    let len = K32GetModuleFileNameExW(process_handle, None, &mut buffer);
    let _ = windows::Win32::Foundation::CloseHandle(process_handle);

    if len == 0 {
        return false;
    }

    let path = OsString::from_wide(&buffer[..len as usize])
        .to_string_lossy()
        .to_lowercase();

    path.ends_with("1cv8.exe")
        || path.ends_with("1cv8c.exe")
        || path.ends_with("1cv8s.exe")
        || path.ends_with("1cv8t.exe")
        || path.ends_with("1cv8ct.exe")
        || path.ends_with("1cv8st.exe")
}

#[cfg(test)]
mod tests {
    use super::{build_intercept_plan, next_rbuttonup_suppression_state, InterceptPlan};

    #[test]
    fn rdp_ctrl_rclick_defers_overlay_until_ctrl_release() {
        let plan = build_intercept_plan(true, true, true);

        assert_eq!(
            plan,
            InterceptPlan {
                suppress_mouse_down: true,
                suppress_mouse_up: true,
                emit_overlay_immediately: false,
                defer_overlay_until_ctrl_release: true,
                post_ctrl_keyup_to_1c: false,
            }
        );
    }

    #[test]
    fn local_ctrl_rclick_waits_for_mouse_release_before_overlay() {
        let plan = build_intercept_plan(true, true, false);

        assert!(!plan.emit_overlay_immediately);
        assert!(!plan.defer_overlay_until_ctrl_release);
    }

    #[test]
    fn local_ctrl_rclick_does_not_post_synthetic_ctrl_keyup() {
        let plan = build_intercept_plan(true, true, false);

        assert!(!plan.post_ctrl_keyup_to_1c);
    }

    #[test]
    fn stale_rbuttonup_suppression_is_cleared_by_regular_next_click() {
        let regular_click_plan = Some(build_intercept_plan(false, true, false));

        assert!(!next_rbuttonup_suppression_state(true, regular_click_plan));
    }

    #[test]
    fn intercepted_ctrl_rclick_arms_rbuttonup_suppression_for_one_pair() {
        let intercepted_plan = Some(build_intercept_plan(true, true, false));

        assert!(next_rbuttonup_suppression_state(false, intercepted_plan));
    }
}
