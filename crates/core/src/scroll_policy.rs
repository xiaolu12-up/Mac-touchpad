//! Scroll policy (生效策略): decides whether smooth scrolling should be
//! applied for the current foreground window.
//!
//! A dedicated lightweight thread polls the foreground window every 300ms
//! (event-driven `SetWinEventHook` was evaluated and rejected — it needs a
//! dedicated message pump thread for out-of-context callbacks, and 300ms
//! latency is imperceptible for this use case).
//!
//! The filter chain is short-circuit evaluated in priority order (一票否决制):
//!   1. 全屏检查：若开启“全屏时禁用”且窗口覆盖屏幕，不生效。
//!   2. 黑名单检查：若开启黑名单且当前应用在黑名单中，不生效（最高拦截权）。
//!   3. 仅在浏览器生效：若开启“仅在浏览器生效”且当前应用不属于内置浏览器，不生效。
//!   4. 白名单检查：若开启白名单且列表非空且当前应用不在白名单中，不生效。
//! 若顺利通过所有启用的校验规则，功能正常生效。
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

use crate::config::{AppPolicyItem, Config};

/// Gate read by the scroll hook on every wheel event. `false` → the hook
/// passes native scroll events through untouched (smooth scrolling off).
/// Defaults to `true` (apply) until the policy thread has evaluated once.
pub static SMOOTH_SCROLL_POLICY_GATE: AtomicBool = AtomicBool::new(true);

/// Latest policy config snapshot, written by the message loop on config
/// updates, cloned by the policy thread each cycle.
static POLICY_CONFIG: Mutex<Option<PolicyConfig>> = Mutex::new(None);

/// Set by `stop_policy_thread()` to exit the polling loop.
static POLICY_SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Polling interval in ms (1s).
const POLICY_POLL_INTERVAL_MS: u64 = 1000;

/// Tolerance (px) for fullscreen detection — window borders/invisible 1px
/// edges can slightly overshoot or undershoot the monitor rect.
const FULLSCREEN_TOLERANCE: i32 = 2;

/// Policy configuration snapshot (plain values, no `Config` dependency so the
/// filter chain stays a pure function over explicit inputs).
#[derive(Debug, Clone, Default)]
pub struct PolicyConfig {
    pub enabled: bool,
    pub fullscreen_disabled: bool,
    pub blacklist_enabled: bool,
    pub browser_only: bool,
    pub whitelist_enabled: bool,
    pub blacklist: Vec<AppPolicyItem>,
    pub whitelist: Vec<AppPolicyItem>,
}

/// Geometry + identity of the foreground window at evaluation time.
#[derive(Debug, Clone)]
pub struct ForegroundInfo {
    /// Process base name, lowercased, without `.exe` (e.g. `"chrome"`).
    /// Empty if the process name could not be resolved.
    pub process_name: String,
    /// Process full image path, lowercased (e.g. `"c:\program files\...\chrome.exe"`).
    pub process_path: String,
    /// Window rect: (left, top, right, bottom).
    pub rect: (i32, i32, i32, i32),
    /// Full monitor rect (`rcMonitor`) of the monitor nearest the window.
    pub monitor_rect: (i32, i32, i32, i32),
    /// Working-area rect (`rcWork`) of the same monitor.
    pub monitor_work: (i32, i32, i32, i32),
}

/// Evaluate the filter chain (short-circuit, 一票否决制).
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
    // 校验 1（全屏检查）：若开启了“全屏时禁用”，且当前窗口尺寸覆盖了屏幕，不生效。
    if cfg.fullscreen_disabled && is_fullscreen(info) {
        return (false, "fullscreen");
    }
    // 校验 2（黑名单检查）：若开启了黑名单且当前应用在黑名单中，不生效（黑名单具有最高优先级的拦截权）。
    if cfg.blacklist_enabled && !cfg.blacklist.is_empty() && in_app_list(info, &cfg.blacklist) {
        return (false, "blacklist");
    }
    // 校验 3（仅在浏览器生效）：若开启了“仅在浏览器生效”，且当前应用不属于内置的浏览器进程列表，不生效。
    if cfg.browser_only && !is_browser(&info.process_name) {
        return (false, "browser");
    }
    // 校验 4（白名单检查）：若开启了白名单且白名单列表不为空，且当前应用不在白名单中，不生效。
    if cfg.whitelist_enabled && !cfg.whitelist.is_empty() && !in_app_list(info, &cfg.whitelist) {
        return (false, "whitelist");
    }
    // 最终判定：顺利通过所有启用的校验规则后，功能正常生效。
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

/// Case-insensitive match against an AppPolicyItem list.
/// Matches either exact executable path, process executable stem name, or wildcard `*`.
pub fn in_app_list(info: &ForegroundInfo, list: &[AppPolicyItem]) -> bool {
    let p_name = info.process_name.to_lowercase();
    let p_path = info.process_path.to_lowercase();

    list.iter().any(|entry| {
        let e_name = entry.name.trim().to_lowercase();
        let e_path = entry.path.trim().to_lowercase();

        if e_name == "*" || e_path == "*" {
            return true;
        }
        if !e_path.is_empty() && !p_path.is_empty() && e_path == p_path {
            return true;
        }
        let norm = normalize_process(&e_name);
        if !norm.is_empty() && norm == p_name {
            return true;
        }
        false
    })
}

/// Case-insensitive membership test for legacy string slices; `"*"` matches everything.
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
        fullscreen_disabled: config.scroll_policy_fullscreen_disabled,
        blacklist_enabled: config.scroll_policy_blacklist_enabled,
        browser_only: config.scroll_policy_browser_only,
        whitelist_enabled: config.scroll_policy_whitelist_enabled,
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
    let mut last_path: String = String::new();
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
            // Resolve process path & name (only when the foreground window changed).
            if let Some(path) = foreground_process_path(hwnd) {
                last_path = path.to_lowercase();
                last_process = normalize_process(&path);
            } else {
                last_path = String::new();
                last_process = String::new(); // failure relaxation: keep applying
            }
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
            process_path: last_path.clone(),
            rect,
            monitor_rect,
            monitor_work,
        };
        let (apply, layer) = evaluate(&cfg, &info);
        SMOOTH_SCROLL_POLICY_GATE.store(apply, Ordering::Relaxed);
        last_rect = rect;

        if layer != last_layer {
            tracing::info!(
                "Scroll policy: foreground={} path={} apply={} layer={}",
                info.process_name,
                info.process_path,
                apply,
                layer,
            );
            last_layer = layer;
        }

        std::thread::sleep(std::time::Duration::from_millis(POLICY_POLL_INTERVAL_MS));
    }
}

/// Resolve the full executable path of the window's owner process.
fn foreground_process_path(hwnd: HWND) -> Option<String> {
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
            process_path: format!("C:\\Apps\\{}.exe", process),
            rect,
            monitor_rect: (0, 0, 1920, 1080),
            monitor_work: (0, 0, 1920, 1040),
        }
    }

    fn info_with_path(process: &str, path: &str, rect: (i32, i32, i32, i32)) -> ForegroundInfo {
        ForegroundInfo {
            process_name: process.into(),
            process_path: path.into(),
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
    fn test_in_app_list() {
        let list = vec![
            AppPolicyItem::from_name("chrome"),
            AppPolicyItem::new("C:\\Program Files\\Notepad++\\notepad++.exe", "notepad++.exe", "Notepad++", ""),
        ];
        let info1 = info_with_path("chrome", "C:\\Google\\chrome.exe", (0, 0, 100, 100));
        assert!(in_app_list(&info1, &list));

        let info2 = info_with_path("notepad++", "c:\\program files\\notepad++\\notepad++.exe", (0, 0, 100, 100));
        assert!(in_app_list(&info2, &list));

        let info3 = info_with_path("code", "c:\\vs code\\code.exe", (0, 0, 100, 100));
        assert!(!in_app_list(&info3, &list));
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
    fn test_evaluate_short_circuit_chain() {
        // 0. Master switch disabled → always apply
        let cfg = PolicyConfig {
            enabled: false,
            fullscreen_disabled: true,
            blacklist_enabled: true,
            browser_only: false,
            whitelist_enabled: false,
            blacklist: vec![AppPolicyItem::from_name("game")],
            whitelist: vec![],
        };
        assert_eq!(evaluate(&cfg, &info("game", (0, 0, 1920, 1080))), (true, "disabled"));

        // 1. Fullscreen disabled check
        let cfg = PolicyConfig {
            enabled: true,
            fullscreen_disabled: true,
            blacklist_enabled: false,
            browser_only: false,
            whitelist_enabled: false,
            blacklist: vec![],
            whitelist: vec![],
        };
        assert_eq!(evaluate(&cfg, &info("game", (0, 0, 1920, 1080))), (false, "fullscreen"));
        assert_eq!(evaluate(&cfg, &info("game", (10, 10, 800, 600))), (true, "pass"));

        // Fullscreen disabled = false → fullscreen doesn't block
        let cfg = PolicyConfig {
            enabled: true,
            fullscreen_disabled: false,
            blacklist_enabled: false,
            browser_only: false,
            whitelist_enabled: false,
            blacklist: vec![],
            whitelist: vec![],
        };
        assert_eq!(evaluate(&cfg, &info("game", (0, 0, 1920, 1080))), (true, "pass"));

        // 2. Blacklist check (higher priority than browser-only)
        let cfg = PolicyConfig {
            enabled: true,
            fullscreen_disabled: true,
            blacklist_enabled: true,
            browser_only: true,
            whitelist_enabled: false,
            blacklist: vec![AppPolicyItem::from_name("chrome")],
            whitelist: vec![],
        };
        // Fullscreen blocks first
        assert_eq!(evaluate(&cfg, &info("chrome", (0, 0, 1920, 1080))), (false, "fullscreen"));
        // Windowed chrome blocked by blacklist
        assert_eq!(evaluate(&cfg, &info("chrome", (10, 10, 800, 600))), (false, "blacklist"));

        // 3. Browser-only check
        let cfg = PolicyConfig {
            enabled: true,
            fullscreen_disabled: false,
            blacklist_enabled: true,
            browser_only: true,
            whitelist_enabled: false,
            blacklist: vec![],
            whitelist: vec![],
        };
        assert_eq!(evaluate(&cfg, &info("notepad", (10, 10, 800, 600))), (false, "browser"));
        assert_eq!(evaluate(&cfg, &info("chrome", (10, 10, 800, 600))), (true, "pass"));

        // 4. Whitelist check
        let cfg = PolicyConfig {
            enabled: true,
            fullscreen_disabled: false,
            blacklist_enabled: false,
            browser_only: false,
            whitelist_enabled: true,
            blacklist: vec![],
            whitelist: vec![AppPolicyItem::from_name("notepad")],
        };
        assert_eq!(evaluate(&cfg, &info("chrome", (10, 10, 800, 600))), (false, "whitelist"));
        assert_eq!(evaluate(&cfg, &info("notepad", (10, 10, 800, 600))), (true, "pass"));

        // Multiple rules combined: Fullscreen + Blacklist + Browser Only + Whitelist
        let cfg = PolicyConfig {
            enabled: true,
            fullscreen_disabled: true,
            blacklist_enabled: true,
            browser_only: true,
            whitelist_enabled: true,
            blacklist: vec![AppPolicyItem::from_name("edge")],
            whitelist: vec![AppPolicyItem::from_name("chrome"), AppPolicyItem::from_name("edge")],
        };
        // chrome (windowed) passes all 4 rules
        assert_eq!(evaluate(&cfg, &info("chrome", (10, 10, 800, 600))), (true, "pass"));
        // edge is in whitelist, but blocked by blacklist (veto)
        assert_eq!(evaluate(&cfg, &info("edge", (10, 10, 800, 600))), (false, "blacklist"));
        // firefox is browser, but not in whitelist
        assert_eq!(evaluate(&cfg, &info("firefox", (10, 10, 800, 600))), (false, "whitelist"));
        // notepad is not browser
        assert_eq!(evaluate(&cfg, &info("notepad", (10, 10, 800, 600))), (false, "browser"));
    }
}

