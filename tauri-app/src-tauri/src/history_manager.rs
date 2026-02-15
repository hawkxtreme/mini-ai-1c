use std::collections::HashMap;
use tokio::sync::Mutex;
use lazy_static::lazy_static;

#[derive(Debug, Clone)]
pub struct CodeSnapshot {
    pub hwnd: isize,
    pub original_code: String,
    pub timestamp: std::time::SystemTime,
}

lazy_static! {
    static ref SNAPSHOTS: Mutex<HashMap<isize, Vec<CodeSnapshot>>> = Mutex::new(HashMap::new());
}

pub async fn save_snapshot(hwnd: isize, code: String) {
    let mut guard = SNAPSHOTS.lock().await;
    let list = guard.entry(hwnd).or_insert_with(Vec::new);
    list.push(CodeSnapshot {
        hwnd,
        original_code: code,
        timestamp: std::time::SystemTime::now(),
    });
    
    // Keep only last 10 snapshots per window
    if list.len() > 10 {
        list.remove(0);
    }
}

pub async fn pop_snapshot(hwnd: isize) -> Option<CodeSnapshot> {
    let mut guard = SNAPSHOTS.lock().await;
    if let Some(list) = guard.get_mut(&hwnd) {
        list.pop()
    } else {
        None
    }
}
