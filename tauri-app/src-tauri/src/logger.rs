use std::collections::VecDeque;
use std::sync::Mutex;
use lazy_static::lazy_static;
use chrono::Local;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_LOG_LINES: usize = 2000;

lazy_static! {
    static ref LOGS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES));
    static ref DEBUG_MODE: AtomicBool = AtomicBool::new(false);
}

pub fn set_debug_mode(enabled: bool) {
    DEBUG_MODE.store(enabled, Ordering::Relaxed);
}

pub fn is_debug_mode() -> bool {
    DEBUG_MODE.load(Ordering::Relaxed)
}

pub fn log(message: &str, force: bool) {
    if !force && !is_debug_mode() {
        return;
    }
    
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let formatted_message = format!("[{}] {}", timestamp, message);
    
    // Print to real console — ignore broken pipe errors (os error 232 on Windows)
    use std::io::Write;
    let _ = writeln!(std::io::stdout().lock(), "{}", formatted_message);
    
    let mut logs = LOGS.lock().unwrap();
    if logs.len() >= MAX_LOG_LINES {
        logs.pop_front();
    }
    logs.push_back(formatted_message);
}

pub fn get_all_logs() -> String {
    let logs = LOGS.lock().unwrap();
    logs.iter().cloned().collect::<Vec<String>>().join("\n")
}

#[macro_export]
macro_rules! app_log {
    (force: $force:expr, $($arg:tt)*) => {
        $crate::logger::log(&format!($($arg)*), $force)
    };
    ($($arg:tt)*) => {
        $crate::logger::log(&format!($($arg)*), false)
    };
}
