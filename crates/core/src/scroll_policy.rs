//! Scroll policy (生效策略): decides whether smooth scrolling should be
//! applied for the current foreground window.
//!
//! A dedicated lightweight thread polls the foreground window every 300ms
//! (event-driven `SetWinEventHook` was evaluated and rejected — it needs a
//! dedicated message pump thread for out-of-context callbacks, and 300ms
//! latency is imperceptible for this use case).
//!
//! The filter chain is short-circuit evaluated in priority order:
//!   1. fullscreen (window covers the monitor)
//!   2. blacklist (process name listed)
//!   3. browser-only mode (process not a known browser)
//!   4. whitelist (process not listed)
//! If any step says "don't apply", evaluation stops immediately.
//!
//! The hook thread (wheel_hook.rs) reads [`SMOOTH_SCROLL_POLICY_GATE`] with a
//! relaxed atomic load per scroll event — no locks on the hot path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId};

use crate::config::Config;

/// Gate read by the scroll hook on every wheel event. `false` → the hook
/// passes native scroll events through untouched (smooth scrolling off).
/// Defaults to `true` (apply) until the policy thread has evaluated once.
pub static SMOOTH_SCROLL_POLICY_GATE: AtomicBool = AtomicBool::new(true);

/// Latest policy config snapshot, written by the message loop on config
/// updates, cloned by the policy thread each cycle.
static POLICY_CONFIG: Mutex<Option<PolicyConfig>> = Mutex::new(None);

/// Set by `stop_policy_thread()` to exit the polling loop.
static POLICY_SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Polling interval in ms.
const POLICY_POLL_INTERVAL_MS: u64 = 300;

/// Tolerance (px) for fullscreen detection — window borders/invisible 1px
/// edges can slightly overshoot or undershoot the monitor rect.
const FULLSCREEN_TOLERANCE: i32 = 2;

/// Mode strings, mirrored in the UI (`ui/index.html`).
pub const MODE_OFF: &str = "off";
pub const MODE_BLACKLIST: &str = "blacklist";
pub const MODE_BROWSER_ONLY: &str = "browser_only";
pub const MODE_WHITELIST: &str = "whitelist";

/// Policy configuration snapshot (plain values, no `Config` dependency so the
/// filter chain stays a pure function over explicit inputs).
#[derive(Debug, Clone, Default)]
pub struct PolicyConfig {
    pub enabled: bool,
    pub mode: String,
    pub blacklist: Vec<String>,
    pub whitelist: Vec<String>,
}

/// Geometry + identity of the foreground window at evaluation time.
#[derive(Debug, Clone)]
pub struct ForegroundInfo {
    /// Process base name, lowercased, without `.exe` (e.g. `"chrome"`).
    /// Empty if the process name could not be resolved.
    pub process_name: String,
    /// Window rect: (left, top, right, bottom).
    pub rect: (i32, i32, i32, i32),
    /// Full monitor rect (`rcMonitor`) of the monitor nearest the window.
    pub monitor_rect: (i32, i32, i32, i32),
    /// Working-area rect (`rcWork`) of the same monitor.
    pub monitor_work: (i32, i32, i32, i32),
}

/// Evaluate the filter chain (short-circuit).
///
/// Returns `(apply, layer)` where `layer` names the deciding step for logging:
/// `"disabled" | "fail" | "fullscreen" | "blacklist" | "browser" | "whitelist" | "pass"`.
/// Failures (e.g. unknown process) default to `apply = true` — a transient
/// lookup error must never silently disable scrolling.
pub fn evaluate(cfg: &PolicyConfig, info: &ForegroundInfo) -> (bool, &'static str) {
    // 0. Master switch
    if !cfg.enabled {
        return (true, "disabled");
    }
    // Failure relaxation: unknown process → keep applying
    if info.process_name.is_empty() {
        return (true, "fail");
    }
    // 1. Fullscreen detection
    if is_fullscreen(info) {
        return (false, "fullscreen");
    }
    // 2. Blacklist
    if !cfg.blacklist.is_empty() && in_list(&info.process_name, &cfg.blacklist) {
        return (false, "blacklist");
    }
    // 3. Browser-only mode
    if cfg.mode == MODE_BROWSER_ONLY && !is_browser(&info.process_name) {
        return (false, "browser");
    }
    // 4. Whitelist
    if cfg.mode == MODE_WHITELIST {
        if cfg.whitelist.is_empty() || !in_list(&info.process_name, &cfg.whitelist) {
            return (false, "whitelist");
        }
    }
    (true, "pass")
}

/// Whether the window rect fully covers the monitor's full rect (within
/// tolerance). Uses `rcMonitor` — the physical resolution — so borderless
/// fullscreen games/videos are detected, while maximized windows (which fall
/// a few px short of the monitor because of the taskbar) are not.
pub fn is_fullscreen(info: &ForegroundInfo) -> bool {
    let (l, t, r, b) = info.rect;
    let (ml, mt, mr, mb) = info.monitor_rect;
    l <= ml + FULLSCREEN_TOLERANCE
        && t <= mt + FULLSCREEN_TOLERANCE
        && r >= mr - FULLSCREEN_TOLERANCE
        && b >= mb - FULLSCREEN_TOLERANCE
}

/// Case-insensitive membership test; `"*"` matches everything.
pub fn in_list(process: &str, list: &[String]) -> bool {
    let p = process.to_lowercase();
    list.iter().any(|entry| {
        let e = entry.trim().to_lowercase();
        e == "*" || e == p
    })
}

/// Built-in browser process names (base names, lowercase, no `.exe`).
const BROWSERS: &[&str] = &[
    "chrome",
    "msedge",
    "edge",
    "firefox",
    "brave",
    "opera",
    "vivaldi",
    "iexplore",
    "360chrome",
    "360se",
    "qqbrowser",
    "centbrowser",
    "sogouexplorer",
    "maxthon",
    "seamonkey",
    "waterfox",
    "palemoon",
    "chromium",
];

/// Whether the process name is a known browser.
pub fn is_browser(process: &str) -> bool {
    let p = process.to_lowercase();
    BROWSERS.contains(&p.as_str())
}

/// Normalize a process image path/name to a comparable base name:
/// lowercase, extension removed, path stripped (e.g. `C:\...\Chrome.EXE` → `chrome`).
pub fn normalize_process(name: &str) -> String {
    let base = name.rsplit(['\\', '/']).next().unwrap_or(name);
    // Strip extension case-insensitively (.exe, .EXE, .Exe, etc.)
    let stem = if base.len() > 4 && base[base.len() - 4..].eq_ignore_ascii_case(".exe") {
        &base[..base.len() - 4]
    } else {
        base
    };
    stem.to_lowercase()
}

/// Push the latest config into the policy thread's snapshot.
/// When the policy is disabled, immediately re-open the gate — otherwise
/// there'd be up to one poll interval where the previous decision still
/// blocks scrolling.
pub fn update_policy_config(config: &Config) {
    let policy = PolicyConfig {
        enabled: config.scroll_policy_enabled,
        mode: config.scroll_policy_mode.clone(),
        blacklist: config.scroll_policy_blacklist.clone(),
        whitelist: config.scroll_policy_whitelist.clone(),
    };
    if let Ok(mut guard) = POLICY_CONFIG.lock() {
        *guard = Some(policy);
    }
    if !config.scroll_policy_enabled {
        SMOOTH_SCROLL_POLICY_GATE.store(true, Ordering::Relaxed);
    }
}

/// Start the dedicated policy polling thread.
pub fn start_policy_thread() {
    POLICY_SHUTDOWN.store(false, Ordering::SeqCst);
    std::thread::Builder::new()
        .name("scroll-policy".into())
        .spawn(policy_thread_main)
        .expect("Failed to spawn scroll-policy thread");
}

/// Signal the policy thread to exit. It will observe the flag within one
/// poll interval; we intentionally don't join (same style as the hook thread).
pub fn stop_policy_thread() {
    POLICY_SHUTDOWN.store(true, Ordering::SeqCst);
}

fn policy_thread_main() {
    tracing::info!("Scroll policy thread started (poll {}ms)", POLICY_POLL_INTERVAL_MS);

    // Cache of the last seen foreground window state, to skip the expensive
    // process-name / monitor lookups when nothing changed.
    let mut last_hwnd: Option<HWND> = None;
    let mut last_process: String = String::new();
    let mut last_rect: (i32, i32, i32, i32) = (0, 0, 0, 0);
    let mut last_monitor_rect: (i32, i32, i32, i32) = (0, 0, 0, 0);
    let mut last_layer: &'static str = "";

    loop {
        if POLICY_SHUTDOWN.load(Ordering::SeqCst) {
            tracing::info!("Scroll policy thread exiting");
            return;
        }

        let cfg = POLICY_CONFIG
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default();

        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0 == 0 {
            // No foreground window (rare); keep the current gate.
            std::thread::sleep(std::time::Duration::from_millis(POLICY_POLL_INTERVAL_MS));
            continue;
        }

        // Window rect is cheap; read it every cycle to catch enter/leave
        // fullscreen transitions.
        let rect = unsafe {
            let mut win_rect = RECT::default();
            match GetWindowRect(hwnd, &mut win_rect) {
                Ok(()) => (win_rect.left, win_rect.top, win_rect.right, win_rect.bottom),
                Err(_) => last_rect,
            }
        };

        let same_window = last_hwnd == Some(hwnd);
        if same_window {
            // Only need re-evaluation when the geometry changed.
            let fullscreen = is_fullscreen_internal(rect, last_monitor_rect);
            let prev_fullscreen = last_layer == "fullscreen";
            if rect == last_rect && fullscreen == prev_fullscreen {
                std::thread::sleep(std::time::Duration::from_millis(POLICY_POLL_INTERVAL_MS));
                continue;
            }
        } else {
            // Resolve process name (only when the foreground window changed).
            last_process = match foreground_process_name(hwnd) {
                Some(name) => normalize_process(&name),
                None => String::new(), // failure relaxation: keep applying
            };
            last_hwnd = Some(hwnd);
        }

        // Monitor info for fullscreen comparison.
        let mut monitor_rect = (0, 0, 0, 0);
        let mut monitor_work = (0, 0, 0, 0);
        unsafe {
            let hm = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hm, &mut mi).as_bool() {
                monitor_rect = (mi.rcMonitor.left, mi.rcMonitor.top, mi.rcMonitor.right, mi.rcMonitor.bottom);
                monitor_work = (mi.rcWork.left, mi.rcWork.top, mi.rcWork.right, mi.rcWork.bottom);
            }
        }
        last_monitor_rect = monitor_rect;

        let info = ForegroundInfo {
            process_name: last_process.clone(),
            rect,
            monitor_rect,
            monitor_work,
        };
        let (apply, layer) = evaluate(&cfg, &info);
        SMOOTH_SCROLL_POLICY_GATE.store(apply, Ordering::Relaxed);
        last_rect = rect;

        if layer != last_layer {
            tracing::info!(
                "Scroll policy: foreground={} apply={} layer={} (mode={})",
                info.process_name,
                apply,
                layer,
                cfg.mode,
            );
            last_layer = layer;
        }

        std::thread::sleep(std::time::Duration::from_millis(POLICY_POLL_INTERVAL_MS));
    }
}

/// Resolve the process base name of the window's owner process.
fn foreground_process_name(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        if QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut size).is_ok() {
            Some(String::from_utf16_lossy(&buf[..size as usize]))
        } else {
            None
        }
    }
}

/// Fullscreen check for the polling loop (same math as [`is_fullscreen`]).
fn is_fullscreen_internal(rect: (i32, i32, i32, i32), monitor: (i32, i32, i32, i32)) -> bool {
    rect.0 <= monitor.0 + FULLSCREEN_TOLERANCE
        && rect.1 <= monitor.1 + FULLSCREEN_TOLERANCE
        && rect.2 >= monitor.2 - FULLSCREEN_TOLERANCE
        && rect.3 >= monitor.3 - FULLSCREEN_TOLERANCE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(process: &str, rect: (i32, i32, i32, i32)) -> ForegroundInfo {
        ForegroundInfo {
            process_name: process.into(),
            rect,
            monitor_rect: (0, 0, 1920, 1080),
            monitor_work: (0, 0, 1920, 1040),
        }
    }

    #[test]
    fn test_normalize_process() {
        assert_eq!(normalize_process("Chrome.EXE"), "chrome");
        assert_eq!(normalize_process("C:\\Program Files\\Google\\Chrome\\chrome.exe"), "chrome");
        assert_eq!(normalize_process("C:/Users/x/AppData/Local/msedge.exe"), "msedge");
        assert_eq!(normalize_process("NOTEPAD"), "notepad");
        assert_eq!(normalize_process(""), "");
    }

    #[test]
    fn test_in_list() {
        let list: Vec<String> = vec!["chrome".into(), "notepad".into()];
        assert!(in_list("chrome", &list));
        assert!(in_list("CHROME", &list));
        assert!(!in_list("firefox", &list));
        assert!(!in_list("chrome", &[]));
        let wild: Vec<String> = vec!["*".into()];
        assert!(in_list("anything", &wild));
    }

    #[test]
    fn test_is_browser() {
        assert!(is_browser("chrome"));
        assert!(is_browser("msedge"));
        assert!(is_browser("360chrome"));
        assert!(is_browser("qqbrowser"));
        assert!(!is_browser("notepad"));
        assert!(!is_browser("explorer"));
    }

    #[test]
    fn test_is_fullscreen() {
        // Exact match of monitor rect
        assert!(is_fullscreen(&info("game", (0, 0, 1920, 1080))));
        // Within 2px tolerance (window decorations)
        assert!(is_fullscreen(&info("game", (1, 1, 1920, 1080))));
        assert!(is_fullscreen(&info("game", (-1, 0, 1920, 1080))));
        // Overshoot (borderless window slightly larger)
        assert!(is_fullscreen(&info("game", (-2, -2, 1922, 1082))));
        // Maximized window: short of the monitor bottom (taskbar)
        assert!(!is_fullscreen(&info("app", (0, 0, 1920, 1072))));
        // Regular window
        assert!(!is_fullscreen(&info("app", (100, 100, 800, 600))));
    }

    #[test]
    fn test_evaluate_short_circuit() {
        // Disabled → always apply
        let cfg = PolicyConfig { enabled: false, mode: MODE_BLACKLIST.into(), blacklist: vec!["game".into()], whitelist: vec![] };
        assert_eq!(evaluate(&cfg, &info("game", (0, 0, 1920, 1080))), (true, "disabled"));

        // Fullscreen beats whitelist
        let cfg = PolicyConfig { enabled: true, mode: MODE_WHITELIST.into(), blacklist: vec![], whitelist: vec!["game".into()] };
        assert_eq!(evaluate(&cfg, &info("game", (0, 0, 1920, 1080))), (false, "fullscreen"));

        // Blacklist beats browser-only
        let cfg = PolicyConfig { enabled: true, mode: MODE_BROWSER_ONLY.into(), blacklist: vec!["chrome".into()], whitelist: vec![] };
        assert_eq!(evaluate(&cfg, &info("chrome", (10, 10, 800, 600))), (false, "blacklist"));

        // Blacklist hit in blacklist mode
        let cfg = PolicyConfig { enabled: true, mode: MODE_BLACKLIST.into(), blacklist: vec!["notepad".into()], whitelist: vec![] };
        assert_eq!(evaluate(&cfg, &info("notepad", (10, 10, 800, 600))), (false, "blacklist"));

        // Browser-only: non-browser rejected
        let cfg = PolicyConfig { enabled: true, mode: MODE_BROWSER_ONLY.into(), blacklist: vec![], whitelist: vec![] };
        assert_eq!(evaluate(&cfg, &info("notepad", (10, 10, 800, 600))), (false, "browser"));
        assert_eq!(evaluate(&cfg, &info("chrome", (10, 10, 800, 600))), (true, "pass"));

        // Whitelist: not listed rejected, listed accepted
        let cfg = PolicyConfig { enabled: true, mode: MODE_WHITELIST.into(), blacklist: vec![], whitelist: vec!["chrome".into()] };
        assert_eq!(evaluate(&cfg, &info("notepad", (10, 10, 800, 600))), (false, "whitelist"));
        assert_eq!(evaluate(&cfg, &info("chrome", (10, 10, 800, 600))), (true, "pass"));

        // Empty whitelist → nothing applies
        let cfg = PolicyConfig { enabled: true, mode: MODE_WHITELIST.into(), blacklist: vec![], whitelist: vec![] };
        assert_eq!(evaluate(&cfg, &info("notepad", (10, 10, 800, 600))), (false, "whitelist"));

        // Unknown process → relax to apply
        let cfg = PolicyConfig { enabled: true, mode: MODE_BLACKLIST.into(), blacklist: vec![], whitelist: vec![] };
        assert_eq!(evaluate(&cfg, &info("", (10, 10, 800, 600))), (true, "fail"));

        // Default blacklist mode with empty list → everything passes (except fullscreen)
        let cfg = PolicyConfig { enabled: true, mode: MODE_BLACKLIST.into(), blacklist: vec![], whitelist: vec![] };
        assert_eq!(evaluate(&cfg, &info("notepad", (10, 10, 800, 600))), (true, "pass"));
        assert_eq!(evaluate(&cfg, &info("notepad", (0, 0, 1920, 1080))), (false, "fullscreen"));
    }
}
