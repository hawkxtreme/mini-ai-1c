//! EditorBridge client — communicates with EditorBridge.exe via Named Pipe.
//!
//! EditorBridge.exe reads the 1C Configurator code editor via UIAutomation TextPattern.
//! This module manages the bridge process lifecycle and provides a typed Rust API.
//!
//! Architecture:
//!   EditorBridge.exe (C#, .NET 8, self-contained single-file — no runtime install required)
//!     ↕ \\.\pipe\mini-ai-editor-bridge  (JSON-RPC lines)
//!   editor_bridge.rs  (Rust, this module)
//!     ↕ function calls
//!   commands/configurator.rs  (Tauri commands)

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, Once};
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[cfg(windows)]
use windows::core::{PCWSTR, PWSTR};
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
#[cfg(windows)]
use windows::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, TerminateProcess, WaitForSingleObject, CREATE_NO_WINDOW,
    CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
};

const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\mini-ai-editor-bridge";
const STARTUP_WAIT_MS: u64 = 700;
const RETRY_COUNT: u32 = 3;
const RETRY_DELAY_MS: u64 = 250;
const WATCHDOG_POLL_MS: u64 = 2_000;
const WATCHDOG_RESTART_DELAY_MS: u64 = 350;
const MIN_SELF_CONTAINED_BRIDGE_SIZE_BYTES: u64 = 5 * 1024 * 1024;

// ── Global bridge process ─────────────────────────────────────────────────────

enum BridgeProc {
    Child(Child),
    #[cfg(windows)]
    Desktop(DesktopProcess),
}

impl BridgeProc {
    fn is_running(&mut self) -> bool {
        match self {
            BridgeProc::Child(child) => matches!(child.try_wait(), Ok(None)),
            #[cfg(windows)]
            BridgeProc::Desktop(proc) => proc.is_running(),
        }
    }

    fn kill(&mut self) {
        match self {
            BridgeProc::Child(child) => {
                let _ = child.kill();
            }
            #[cfg(windows)]
            BridgeProc::Desktop(proc) => proc.kill(),
        }
    }
}

#[cfg(windows)]
struct DesktopProcess {
    process: HANDLE,
    thread: HANDLE,
}

#[cfg(windows)]
unsafe impl Send for DesktopProcess {}

#[cfg(windows)]
impl DesktopProcess {
    fn is_running(&self) -> bool {
        match unsafe { WaitForSingleObject(self.process, 0) } {
            value if value == WAIT_TIMEOUT => true,
            value if value == WAIT_OBJECT_0 => false,
            _ => {
                let mut exit_code = 0u32;
                unsafe {
                    GetExitCodeProcess(self.process, &mut exit_code).is_ok() && exit_code == 259
                }
            }
        }
    }

    fn kill(&mut self) {
        let _ = unsafe { TerminateProcess(self.process, 1) };
    }
}

#[cfg(windows)]
impl Drop for DesktopProcess {
    fn drop(&mut self) {
        if !self.thread.is_invalid() {
            let _ = unsafe { CloseHandle(self.thread) };
            self.thread = HANDLE::default();
        }
        if !self.process.is_invalid() {
            let _ = unsafe { CloseHandle(self.process) };
            self.process = HANDLE::default();
        }
    }
}

static BRIDGE_PROC: Mutex<Option<BridgeProc>> = Mutex::new(None);
static BRIDGE_WATCHDOG: Once = Once::new();

struct BridgeLaunchConfig {
    exe: PathBuf,
    pipe_name: Option<String>,
    fake_mode: bool,
    fixture_path: Option<String>,
}

fn pipe_name() -> String {
    let fallback_name = DEFAULT_PIPE_NAME.to_string();
    let raw_name = std::env::var("MINI_AI_EDITOR_BRIDGE_PIPE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback_name.clone());

    if raw_name.starts_with(r"\\.\pipe\") {
        raw_name
    } else {
        format!(r"\\.\pipe\{}", raw_name)
    }
}

fn bridge_launch_config() -> Result<BridgeLaunchConfig, String> {
    Ok(BridgeLaunchConfig {
        exe: find_exe()?,
        pipe_name: std::env::var("MINI_AI_EDITOR_BRIDGE_PIPE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        fake_mode: std::env::var("MINI_AI_EDITOR_BRIDGE_MODE")
            .ok()
            .map(|value| value.eq_ignore_ascii_case("fake"))
            .unwrap_or(false),
        fixture_path: std::env::var("MINI_AI_EDITOR_BRIDGE_FIXTURE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    })
}

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorContext {
    pub available: bool,
    pub conf_hwnd: i64,
    pub window_title: String,
    pub primary_runtime_id: Option<String>,
    pub has_selection: bool,
    pub selection_text: String,
    pub caret_line: i32,
    pub current_method_name: Option<String>,
    pub method_start_line: Option<i32>,
    pub method_end_line: Option<i32>,
    pub current_method_text: Option<String>,
    pub module_text: String,
    pub capabilities: Option<EditorCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodInfo {
    pub text: String,
    pub header: String,
    pub start_line: i32,
    pub end_line: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorCapabilities {
    pub can_read: bool,
    pub can_replace_selection: bool,
    pub can_replace_current_method: bool,
    pub can_insert_before_method: bool,
    pub can_replace_module: bool,
    pub read_mode: String,
    pub write_mode: String,
    pub diagnostic_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorWriteResult {
    pub applied: bool,
    pub operation_kind: String,
    pub target_kind: String,
    pub before_hash: String,
    pub after_hash: String,
    pub fallback_used: bool,
    pub diagnostic_message: String,
    pub writer_kind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EditorMethodHints {
    pub caret_line: Option<i32>,
    pub method_start_line: Option<i32>,
    pub method_name: Option<String>,
    pub runtime_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiagnosticResult {
    pub report: String,
}

// ── Executable discovery ──────────────────────────────────────────────────────

fn is_launchable_bridge_exe(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    if path
        .metadata()
        .map(|meta| meta.len() >= MIN_SELF_CONTAINED_BRIDGE_SIZE_BYTES)
        .unwrap_or(false)
    {
        return true;
    }

    let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(dir) = path.parent() else {
        return false;
    };

    let required_runtime_files = [
        format!("{}.dll", stem),
        format!("{}.deps.json", stem),
        format!("{}.runtimeconfig.json", stem),
        "Interop.UIAutomationClient.dll".to_string(),
    ];

    required_runtime_files
        .iter()
        .all(|file_name| dir.join(file_name).is_file())
}

fn resolve_bridge_candidate(path: PathBuf, source: &str) -> Option<PathBuf> {
    if !path.exists() {
        return None;
    }

    if is_launchable_bridge_exe(&path) {
        return Some(path.canonicalize().unwrap_or(path));
    }

    crate::app_log!(
        "[Bridge] Skipping {} candidate {:?}: EditorBridge bundle is incomplete (ожидался self-contained exe или комплект .dll/.deps/.runtimeconfig рядом)",
        source,
        path
    );
    None
}

pub(crate) fn find_exe() -> Result<PathBuf, String> {
    // 0. Path from settings (downloaded via installer)
    let settings = crate::settings::load_settings();
    if !settings.configurator.editor_bridge_exe_path.is_empty() {
        let p = PathBuf::from(&settings.configurator.editor_bridge_exe_path);
        if let Some(candidate) = resolve_bridge_candidate(p, "settings") {
            return Ok(candidate);
        }
    }

    // 1. Next to the app binary (production)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("EditorBridge.exe");
            if let Some(candidate) = resolve_bridge_candidate(p, "next to app binary") {
                return Ok(candidate);
            }
        }
    }

    // 2. Development: target/debug/editor-bridge/EditorBridge.exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("editor-bridge").join("EditorBridge.exe");
            if let Some(candidate) = resolve_bridge_candidate(p, "target/debug/editor-bridge") {
                return Ok(candidate);
            }
        }
    }

    // 3. Development: manually placed next to app in editor-bridge/ subfolder
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors() {
            let p = ancestor
                .join("tauri-app")
                .join("src-tauri")
                .join("editor-bridge")
                .join("EditorBridge.exe");
            if let Some(candidate) =
                resolve_bridge_candidate(p, "tauri-app/src-tauri/editor-bridge")
            {
                return Ok(candidate);
            }
        }
    }

    Err(
        "EditorBridge.exe не найден. Скачайте его через Настройки → Конфиг → Скачать EditorBridge."
            .to_string(),
    )
}

fn spawn_bridge_log_reader<R: Read + Send + 'static>(reader: R, label: &'static str) {
    std::thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            match line {
                Ok(line) => crate::app_log!("[Bridge][{}] {}", label, line),
                Err(error) => {
                    crate::app_log!("[Bridge][{}] pipe read failed: {}", label, error);
                    break;
                }
            }
        }
        crate::app_log!("[Bridge][{}] stream closed", label);
    });
}

#[cfg(windows)]
fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(|ch| ch.is_whitespace() || ch == '"') {
        return arg.to_string();
    }

    let mut result = String::from("\"");
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                result.push_str(&"\\".repeat(backslashes * 2 + 1));
                result.push('"');
                backslashes = 0;
            }
            _ => {
                if backslashes > 0 {
                    result.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                }
                result.push(ch);
            }
        }
    }
    if backslashes > 0 {
        result.push_str(&"\\".repeat(backslashes * 2));
    }
    result.push('"');
    result
}

#[cfg(windows)]
fn spawn_bridge_on_default_desktop(
    exe: &PathBuf,
    pipe_name: Option<&String>,
    fake_mode: bool,
    fixture_path: Option<&String>,
) -> Result<DesktopProcess, String> {
    let exe_string = exe.to_string_lossy().to_string();
    let mut args = vec![quote_windows_arg(&exe_string)];
    if let Some(pipe_name) = pipe_name {
        args.push("--pipe-name".to_string());
        args.push(quote_windows_arg(pipe_name));
    }
    if fake_mode {
        args.push("--fake".to_string());
    }
    if let Some(fixture_path) = fixture_path {
        args.push("--fixture".to_string());
        args.push(quote_windows_arg(fixture_path));
    }

    let mut cmdline: Vec<u16> = args
        .join(" ")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let exe_wide: Vec<u16> = exe_string
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let desktop_wide: Vec<u16> = "winsta0\\default"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut startup = STARTUPINFOW::default();
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    startup.lpDesktop = PWSTR(desktop_wide.as_ptr() as *mut _);

    let mut process_info = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR(exe_wide.as_ptr()),
            PWSTR(cmdline.as_mut_ptr()),
            None,
            None,
            false,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            None,
            None,
            &startup,
            &mut process_info,
        )
        .map_err(|e| format!("Cannot start EditorBridge.exe on default desktop: {}", e))?;
    }

    crate::app_log!(
        "[Bridge] EditorBridge started on winsta0\\default, pid={}",
        process_info.dwProcessId
    );
    crate::job_guard::assign_to_job(process_info.dwProcessId);

    Ok(DesktopProcess {
        process: process_info.hProcess,
        thread: process_info.hThread,
    })
}

fn spawn_bridge_child_process(
    exe: &PathBuf,
    pipe_name: Option<&String>,
    fake_mode: bool,
    fixture_path: Option<&String>,
) -> Result<BridgeProc, String> {
    let mut command = Command::new(exe);
    if let Some(pipe_name) = pipe_name {
        command.arg("--pipe-name").arg(pipe_name);
    }
    if fake_mode {
        command.arg("--fake");
    }
    if let Some(fixture_path) = fixture_path {
        command.arg("--fixture").arg(fixture_path);
    }

    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot start EditorBridge.exe: {}", e))?;
    crate::job_guard::assign_to_job(child.id());

    if let Some(stdout) = child.stdout.take() {
        spawn_bridge_log_reader(stdout, "STDOUT");
    }

    if let Some(stderr) = child.stderr.take() {
        spawn_bridge_log_reader(stderr, "STDERR");
    }

    Ok(BridgeProc::Child(child))
}

fn wait_for_bridge_start(proc: &mut BridgeProc) -> Result<(), String> {
    std::thread::sleep(Duration::from_millis(STARTUP_WAIT_MS));
    if proc.is_running() {
        Ok(())
    } else {
        Err("EditorBridge exited during startup".to_string())
    }
}

#[cfg(windows)]
fn prefer_regular_child_launch() -> bool {
    if let Ok(value) = std::env::var("MINI_AI_EDITOR_BRIDGE_LAUNCH") {
        let normalized = value.trim().to_ascii_lowercase();
        if normalized == "child" || normalized == "stdio" {
            return true;
        }
        if normalized == "desktop" {
            return false;
        }
    }

    cfg!(debug_assertions)
}

fn spawn_bridge_from_config(config: &BridgeLaunchConfig) -> Result<BridgeProc, String> {
    #[cfg(windows)]
    if !config.fake_mode && !prefer_regular_child_launch() {
        match spawn_bridge_on_default_desktop(
            &config.exe,
            config.pipe_name.as_ref(),
            config.fake_mode,
            config.fixture_path.as_ref(),
        ) {
            Ok(proc) => {
                let mut proc = BridgeProc::Desktop(proc);
                match wait_for_bridge_start(&mut proc) {
                    Ok(()) => return Ok(proc),
                    Err(error) => {
                        proc.kill();
                        crate::app_log!(
                            "[Bridge] Default desktop launch failed startup check: {}; retrying as regular child process",
                            error
                        );
                    }
                }
            }
            Err(error) => {
                crate::app_log!(
                    "[Bridge] Default desktop launch failed: {}; retrying as regular child process",
                    error
                );
            }
        }
    }

    #[cfg(windows)]
    if !config.fake_mode && prefer_regular_child_launch() {
        crate::app_log!(
            "[Bridge] Using regular child-process launch for EditorBridge (debug/dev mode or MINI_AI_EDITOR_BRIDGE_LAUNCH=child)"
        );
    }

    let mut proc = spawn_bridge_child_process(
        &config.exe,
        config.pipe_name.as_ref(),
        config.fake_mode,
        config.fixture_path.as_ref(),
    )?;
    wait_for_bridge_start(&mut proc)?;
    Ok(proc)
}

fn log_launch_config(config: &BridgeLaunchConfig, reason: &str) {
    crate::app_log!(
        "[Bridge] {}: {:?}, fake_mode={}, pipe={:?}, fixture={:?}",
        reason,
        config.exe,
        config.fake_mode,
        config.pipe_name,
        config.fixture_path
    );
}

// ── Process lifecycle ─────────────────────────────────────────────────────────

/// Ensure EditorBridge.exe is running. Start (or restart) if needed.
pub fn ensure_running() -> Result<(), String> {
    let mut guard = BRIDGE_PROC.lock().map_err(|e| e.to_string())?;

    // Check if still alive
    if let Some(proc) = guard.as_mut() {
        if proc.is_running() {
            return Ok(());
        }
    }
    *guard = None;

    let config = bridge_launch_config()?;
    log_launch_config(&config, "Starting EditorBridge");
    drop(guard);

    let mut proc = spawn_bridge_from_config(&config)?;
    let mut guard = BRIDGE_PROC.lock().map_err(|e| e.to_string())?;
    if let Some(existing) = guard.as_mut() {
        if existing.is_running() {
            proc.kill();
            return Ok(());
        }
    }
    *guard = Some(proc);
    crate::app_log!("[Bridge] EditorBridge started");
    Ok(())
}

pub fn restart() -> Result<(), String> {
    stop();
    ensure_running()
}

pub fn start_watchdog() {
    BRIDGE_WATCHDOG.call_once(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(WATCHDOG_POLL_MS));

            let should_restart = {
                let mut guard = match BRIDGE_PROC.lock() {
                    Ok(guard) => guard,
                    Err(error) => {
                        crate::app_log!("[Bridge] Watchdog failed to lock process state: {}", error);
                        continue;
                    }
                };

                if let Some(proc) = guard.as_mut() {
                    if proc.is_running() {
                        false
                    } else {
                        crate::app_log!("[Bridge] Watchdog detected stopped EditorBridge process");
                        *guard = None;
                        true
                    }
                } else {
                    false
                }
            };

            if !should_restart {
                continue;
            }

            std::thread::sleep(Duration::from_millis(WATCHDOG_RESTART_DELAY_MS));

            let config = match bridge_launch_config() {
                Ok(config) => config,
                Err(error) => {
                    crate::app_log!("[Bridge] Watchdog cannot resolve launch config: {}", error);
                    continue;
                }
            };

            log_launch_config(&config, "Watchdog restarting EditorBridge");
            match spawn_bridge_from_config(&config) {
                Ok(proc) => {
                    let mut guard = match BRIDGE_PROC.lock() {
                        Ok(guard) => guard,
                        Err(error) => {
                            crate::app_log!("[Bridge] Watchdog failed to store restarted process: {}", error);
                            let mut proc = proc;
                            proc.kill();
                            continue;
                        }
                    };

                    if guard.is_none() {
                        *guard = Some(proc);
                        crate::app_log!("[Bridge] Watchdog restarted EditorBridge");
                    } else {
                        drop(guard);
                        let mut proc = proc;
                        proc.kill();
                        crate::app_log!("[Bridge] Watchdog skipped restart because a new EditorBridge is already running");
                    }
                }
                Err(error) => {
                    crate::app_log!("[Bridge] Watchdog restart failed: {}", error);
                }
            }
        });
        crate::app_log!("[Bridge] Watchdog started");
    });
}

/// Stop the bridge process.
pub fn stop() {
    if let Ok(mut guard) = BRIDGE_PROC.lock() {
        if let Some(mut proc) = guard.take() {
            proc.kill();
            crate::app_log!("[Bridge] EditorBridge stopped");
        }
    }
}

// ── Named Pipe communication ──────────────────────────────────────────────────

fn pipe_request(json: &str) -> Result<String, String> {
    use std::fs::OpenOptions;

    let pipe_name = pipe_name();
    let pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pipe_name)
        .map_err(|e| {
            format!(
                "Pipe connect failed (is EditorBridge running?) [{}]: {}",
                pipe_name, e
            )
        })?;

    // Write request line
    (&pipe)
        .write_all(json.as_bytes())
        .map_err(|e| format!("Pipe write failed: {}", e))?;
    (&pipe)
        .write_all(b"\n")
        .map_err(|e| format!("Pipe write failed: {}", e))?;

    // Read response line
    let mut reader = BufReader::new(&pipe);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("Pipe read failed: {}", e))?;

    if line.is_empty() {
        return Err("Empty response from EditorBridge".to_string());
    }
    Ok(line.trim().to_string())
}

fn send_command(cmd: &str, args: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    ensure_running()?;

    let req = match args {
        Some(a) => serde_json::json!({"command": cmd, "args": a}),
        None => serde_json::json!({"command": cmd}),
    };
    let json = serde_json::to_string(&req).unwrap();

    let mut last_err = String::new();
    for attempt in 0..RETRY_COUNT {
        match pipe_request(&json) {
            Ok(resp) => {
                let v: serde_json::Value = serde_json::from_str(&resp).map_err(|e| {
                    format!(
                        "Bridge JSON parse error: {} | resp={}",
                        e,
                        &resp[..resp.len().min(200)]
                    )
                })?;

                return if v["ok"].as_bool().unwrap_or(false) {
                    Ok(v["result"].clone())
                } else {
                    let code = v["error"].as_str().unwrap_or("unknown_error");
                    let msg = v["message"].as_str().unwrap_or("");
                    Err(format!("[{}] {}", code, msg))
                };
            }
            Err(e) => {
                last_err = e.clone();
                crate::app_log!("[Bridge] Attempt {}/{}: {}", attempt + 1, RETRY_COUNT, e);
                if attempt < RETRY_COUNT - 1 {
                    // If the process crashed during this request, restart it before retrying.
                    // ensure_running() already sleeps STARTUP_WAIT_MS after spawning.
                    let crashed = BRIDGE_PROC
                        .lock()
                        .ok()
                        .and_then(|mut g| g.as_mut().map(|p| !p.is_running()))
                        .unwrap_or(true);
                    if crashed {
                        crate::app_log!(
                            "[Bridge] Process crashed mid-request, restarting (attempt {}/{})…",
                            attempt + 1,
                            RETRY_COUNT
                        );
                        if let Err(e2) = ensure_running() {
                            crate::app_log!("[Bridge] Restart failed: {}", e2);
                            std::thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
                        }
                        // ensure_running() already waited STARTUP_WAIT_MS — no extra sleep needed
                    } else {
                        std::thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
                    }
                }
            }
        }
    }
    Err(format!(
        "EditorBridge unavailable after {} retries: {}",
        RETRY_COUNT, last_err
    ))
}

// ── Public API ────────────────────────────────────────────────────────────────

fn editor_read_args(
    hwnd: isize,
    prefer_clipboard_fulltext: bool,
    prefer_clipboard_selection: bool,
) -> serde_json::Value {
    serde_json::json!({
        "hwnd": hwnd as i64,
        "prefer_clipboard_fulltext": prefer_clipboard_fulltext,
        "prefer_clipboard_selection": prefer_clipboard_selection,
    })
}

fn hwnd_args(hwnd: isize) -> serde_json::Value {
    editor_read_args(hwnd, false, false)
}

fn semantic_method_args(
    hwnd: isize,
    text: &str,
    hints: Option<&EditorMethodHints>,
) -> serde_json::Value {
    let hints = hints.cloned().unwrap_or_default();
    serde_json::json!({
        "hwnd": hwnd as i64,
        "text": text,
        "caret_line": hints.caret_line,
        "method_start_line": hints.method_start_line,
        "method_name": hints.method_name,
        "runtime_id": hints.runtime_id,
    })
}

/// Full editor context: text, selection, caret, current method.
pub fn get_editor_context(hwnd: isize) -> Result<EditorContext, String> {
    get_editor_context_with_read_preferences(hwnd, false, false)
}

/// Full editor context with optional clipboard-backed reread for clean text capture.
pub fn get_editor_context_with_read_preferences(
    hwnd: isize,
    prefer_clipboard_fulltext: bool,
    prefer_clipboard_selection: bool,
) -> Result<EditorContext, String> {
    let v = send_command(
        "get_editor_context",
        Some(editor_read_args(
            hwnd,
            prefer_clipboard_fulltext,
            prefer_clipboard_selection,
        )),
    )?;
    serde_json::from_value(v).map_err(|e| format!("Deserialize EditorContext: {}", e))
}

/// Full module text.
pub fn get_text(hwnd: isize) -> Result<String, String> {
    let v = send_command("get_text", Some(hwnd_args(hwnd)))?;
    v["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Bridge: missing 'text' in response".to_string())
}

/// Current selection: (has_selection, selection_text).
pub fn get_selection(hwnd: isize) -> Result<(bool, String), String> {
    let v = send_command("get_selection", Some(editor_read_args(hwnd, false, true)))?;
    let has_sel = v["has_selection"].as_bool().unwrap_or(false);
    let text = v["text"].as_str().unwrap_or("").to_string();
    Ok((has_sel, text))
}

/// Current method info (fails if caret is not inside a procedure/function).
pub fn get_current_method(hwnd: isize) -> Result<MethodInfo, String> {
    let v = send_command("get_current_method", Some(hwnd_args(hwnd)))?;
    serde_json::from_value(v).map_err(|e| format!("Deserialize MethodInfo: {}", e))
}

/// Replace the entire module text.
pub fn replace_module(hwnd: isize, text: &str) -> Result<EditorWriteResult, String> {
    let v = send_command(
        "replace_module",
        Some(serde_json::json!({"hwnd": hwnd as i64, "text": text})),
    )?;
    serde_json::from_value(v).map_err(|e| format!("Deserialize EditorWriteResult: {}", e))
}

/// Backward-compatible alias for replacing the entire module text.
pub fn set_text(hwnd: isize, text: &str) -> Result<EditorWriteResult, String> {
    replace_module(hwnd, text)
}

/// Paste text over the current selection.
pub fn replace_selection(hwnd: isize, text: &str) -> Result<EditorWriteResult, String> {
    let v = send_command(
        "replace_selection",
        Some(serde_json::json!({"hwnd": hwnd as i64, "text": text})),
    )?;
    serde_json::from_value(v).map_err(|e| format!("Deserialize EditorWriteResult: {}", e))
}

/// Replace the current method (identified by caret position or semantic hints).
pub fn replace_current_method(
    hwnd: isize,
    text: &str,
    hints: Option<&EditorMethodHints>,
) -> Result<EditorWriteResult, String> {
    let v = send_command(
        "replace_current_method",
        Some(semantic_method_args(hwnd, text, hints)),
    )?;
    serde_json::from_value(v).map_err(|e| format!("Deserialize EditorWriteResult: {}", e))
}

/// Insert text before the current method.
pub fn insert_before_method(
    hwnd: isize,
    text: &str,
    hints: Option<&EditorMethodHints>,
) -> Result<EditorWriteResult, String> {
    let v = send_command(
        "insert_before_method",
        Some(semantic_method_args(hwnd, text, hints)),
    )?;
    serde_json::from_value(v).map_err(|e| format!("Deserialize EditorWriteResult: {}", e))
}

/// BSL insertion context: where the caret is relative to methods, recommended insert line,
/// whether new Процедура/Функция declarations are allowed at the cursor position.
/// Returns the raw JSON value for flexible use in callers.
pub fn get_insertion_context(hwnd: isize) -> Result<serde_json::Value, String> {
    send_command("get_insertion_context", Some(hwnd_args(hwnd)))
}

/// Insert text before the given line number (0-based) without replacing anything.
pub fn insert_at_line(hwnd: isize, line: i64, text: &str) -> Result<EditorWriteResult, String> {
    let v = send_command(
        "insert_at_line",
        Some(serde_json::json!({"hwnd": hwnd as i64, "line": line, "text": text})),
    )?;
    serde_json::from_value(v).map_err(|e| format!("Deserialize EditorWriteResult: {}", e))
}

/// Append text to the end of the module (e.g. a new Процедура/Функция block).
pub fn append_to_module(hwnd: isize, text: &str) -> Result<EditorWriteResult, String> {
    let v = send_command(
        "append_to_module",
        Some(serde_json::json!({"hwnd": hwnd as i64, "text": text})),
    )?;
    serde_json::from_value(v).map_err(|e| format!("Deserialize EditorWriteResult: {}", e))
}

/// Detailed diagnostic report about window visibility, UIAutomation focus and editor resolution.
pub fn diagnose_editor(hwnd: isize) -> Result<String, String> {
    let v = send_command("diagnose_editor", Some(hwnd_args(hwnd)))?;
    let result: DiagnosticResult =
        serde_json::from_value(v).map_err(|e| format!("Deserialize DiagnosticResult: {}", e))?;
    Ok(result.report)
}

#[cfg(test)]
mod tests {
    use super::is_launchable_bridge_exe;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "mini-ai-1c-editor-bridge-tests-{}-{}-{}",
            std::process::id(),
            name,
            unique
        ))
    }

    #[test]
    fn launchable_bridge_requires_companion_runtime_files_for_small_exe() {
        let dir = temp_test_dir("framework-dependent");
        fs::create_dir_all(&dir).unwrap();

        let exe = dir.join("EditorBridge.exe");
        fs::write(&exe, vec![0u8; 1024]).unwrap();
        assert!(!is_launchable_bridge_exe(&exe));

        fs::write(dir.join("EditorBridge.dll"), b"dll").unwrap();
        fs::write(dir.join("EditorBridge.deps.json"), b"{}").unwrap();
        fs::write(dir.join("EditorBridge.runtimeconfig.json"), b"{}").unwrap();
        assert!(!is_launchable_bridge_exe(&exe));

        fs::write(
            dir.join("Interop.UIAutomationClient.dll"),
            b"uia-package-runtime",
        )
        .unwrap();
        assert!(is_launchable_bridge_exe(&exe));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn self_contained_bridge_exe_is_accepted_without_companion_files() {
        let dir = temp_test_dir("self-contained");
        fs::create_dir_all(&dir).unwrap();

        let exe = dir.join("EditorBridge.exe");
        fs::write(&exe, vec![0u8; (6 * 1024 * 1024) as usize]).unwrap();
        assert!(is_launchable_bridge_exe(&exe));

        let _ = fs::remove_dir_all(&dir);
    }
}
