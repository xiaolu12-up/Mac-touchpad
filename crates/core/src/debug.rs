use serde::Serialize;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use std::sync::atomic::{AtomicBool, Ordering};

const MAX_DEBUG_LOGS: usize = 500;

pub static DEBUG_MODE_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_debug_mode_enabled(enabled: bool) {
    DEBUG_MODE_ENABLED.store(enabled, Ordering::Relaxed);
    if !enabled {
        clear_debug_logs();
    }
}

pub fn is_debug_mode_enabled() -> bool {
    DEBUG_MODE_ENABLED.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugLogEntry {
    pub time: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DeviceDiagnostics {
    pub device_detected: bool,
    pub device_id: String,
    pub vendor_id: String,
    pub product_id: String,
    pub x_range: (i32, i32),
    pub y_range: (i32, i32),
    pub last_contact_count: u32,
    pub last_contacts_str: String,
}

static DEBUG_LOGS: Mutex<Option<VecDeque<DebugLogEntry>>> = Mutex::new(None);
static LATEST_DEVICE_INFO: Mutex<Option<DeviceDiagnostics>> = Mutex::new(None);

fn get_log_file_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("MacTouchpad");
    path.push("logs");
    let _ = std::fs::create_dir_all(&path);
    path.push("touchpad_runtime.log");
    Some(path)
}

pub fn log_debug(level: &str, msg: impl Into<String>) {
    // If debug mode is disabled, skip all allocations, formatting, and file I/O immediately
    if !DEBUG_MODE_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let message: String = msg.into();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let millis = now.as_millis() % 1000;
    let secs = now.as_secs();
    let hours = (secs / 3600 + 8) % 24; // Simple UTC+8 approximation
    let mins = (secs / 60) % 60;
    let s = secs % 60;
    let time_str = format!("{:02}:{:02}:{:02}.{:03}", hours, mins, s, millis);

    let entry = DebugLogEntry {
        time: time_str.clone(),
        level: level.to_string(),
        message: message.clone(),
    };

    // Append to runtime log file
    if let Some(log_path) = get_log_file_path() {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
            let line = format!("[{}] [{}] {}\n", time_str, level, message);
            let _ = file.write_all(line.as_bytes());
        }
    }

    if let Ok(mut lock) = DEBUG_LOGS.lock() {
        let deque = lock.get_or_insert_with(|| VecDeque::with_capacity(MAX_DEBUG_LOGS));
        if deque.len() >= MAX_DEBUG_LOGS {
            deque.pop_front();
        }
        deque.push_back(entry);
    }
}

pub fn update_device_diagnostics(diag: DeviceDiagnostics) {
    if let Ok(mut lock) = LATEST_DEVICE_INFO.lock() {
        *lock = Some(diag);
    }
}

pub fn get_device_diagnostics() -> DeviceDiagnostics {
    if let Ok(lock) = LATEST_DEVICE_INFO.lock() {
        if let Some(ref d) = *lock {
            return d.clone();
        }
    }
    DeviceDiagnostics::default()
}

pub fn get_debug_logs() -> Vec<DebugLogEntry> {
    if let Ok(lock) = DEBUG_LOGS.lock() {
        if let Some(ref deque) = *lock {
            return deque.iter().cloned().collect();
        }
    }
    Vec::new()
}

pub fn clear_debug_logs() {
    if let Ok(mut lock) = DEBUG_LOGS.lock() {
        if let Some(ref mut deque) = *lock {
            deque.clear();
        }
    }
}
