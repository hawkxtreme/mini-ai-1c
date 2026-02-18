use std::collections::VecDeque;
use std::sync::Mutex;
use lazy_static::lazy_static;
use chrono::Local;

const MAX_LOG_LINES: usize = 2000;

lazy_static! {
    static ref LOGS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES));
}

pub fn log(message: &str) {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let formatted_message = format!("[{}] {}", timestamp, message);
    
    // Print to real console too
    println!("{}", formatted_message);
    
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
    ($($arg:tt)*) => {
        $crate::logger::log(&format!($($arg)*));
    };
}
