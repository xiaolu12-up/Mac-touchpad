mod exe_info;

use mac_touchpad_core::config::{AppPolicyItem, Config, DeviceDragConfig, GestureAction};
use mac_touchpad_core::window::{CoreCommand, CoreSender};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;

struct AppState {
    core_tx: Mutex<Option<CoreSender>>,
}

// ── Tauri Commands ──

#[tauri::command]
fn get_config() -> Result<Config, String> {
    Ok(Config::load())
}

#[tauri::command]
fn browse_and_get_exe_info() -> Result<Option<AppPolicyItem>, String> {
    Ok(exe_info::browse_and_inspect_exe())
}

#[tauri::command]
fn inspect_exe_path(path: String) -> Result<AppPolicyItem, String> {
    Ok(exe_info::inspect_exe_path(&path))
}

#[tauri::command]
fn show_system_notification(app: tauri::AppHandle, title: String, body: String) -> Result<(), String> {
    let app_handle = app.clone();
    let app_id = if tauri::is_dev() {
        tauri_winrt_notification::Toast::POWERSHELL_APP_ID.to_string()
    } else {
        app.config().identifier.clone()
    };

    tauri_winrt_notification::Toast::new(&app_id)
        .title(&title)
        .text1(&body)
        .on_activated(move |_action| {
            if let Some(window) = app_handle.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
                let _ = window.emit("navigate-about", ());
            }
            Ok(())
        })
        .show()
        .map_err(|e| e.to_string())
}

#[derive(Deserialize)]
struct SaveArgs {
    #[serde(default)] three_finger_drag: Option<bool>,
    #[serde(default)] three_finger_tap_enabled: Option<bool>,
    #[serde(default)] allow_release_and_restart: Option<bool>,
    #[serde(default)] release_delay_ms: Option<u32>,
    #[serde(default)] start_threshold: Option<f32>,
    #[serde(default)] stop_threshold: Option<f32>,
    #[serde(default)] cursor_speed: Option<f32>,
    #[serde(default)] cursor_acceleration: Option<f32>,
    #[serde(default)] edge_slide_enabled: Option<bool>,
    #[serde(default)] run_at_startup: Option<bool>,
    #[serde(default)] close_to_tray: Option<bool>,
    #[serde(default)] invert_volume: Option<bool>,
    #[serde(default)] invert_brightness: Option<bool>,
    #[serde(default)] smooth_scroll_enabled: Option<bool>,
    #[serde(default)] smooth_scroll_speed: Option<f32>,
    #[serde(default)] smooth_scroll_smoothing: Option<f32>,
    #[serde(default)] smooth_scroll_deceleration: Option<f32>,
    #[serde(default)] smooth_scroll_base_scale: Option<f32>,
    #[serde(default)] smooth_scroll_max_delta: Option<f32>,
    #[serde(default)] smooth_scroll_deadzone: Option<f32>,
    #[serde(default)] smooth_scroll_tick_ms: Option<u64>,
    #[serde(default)] natural_scroll: Option<bool>,
    // Scroll policy (生效策略)
    #[serde(default)] scroll_policy_enabled: Option<bool>,
    #[serde(default)] scroll_policy_fullscreen_disabled: Option<bool>,
    #[serde(default)] scroll_policy_blacklist_enabled: Option<bool>,
    #[serde(default)] scroll_policy_browser_only: Option<bool>,
    #[serde(default)] scroll_policy_whitelist_enabled: Option<bool>,
    #[serde(default)] scroll_policy_mode: Option<String>,
    #[serde(default)] scroll_policy_blacklist: Option<Vec<AppPolicyItem>>,
    #[serde(default)] scroll_policy_whitelist: Option<Vec<AppPolicyItem>>,
    #[serde(default)] four_finger_swipe_up: Option<String>,
    #[serde(default)] four_finger_swipe_down: Option<String>,
    #[serde(default)] four_finger_swipe_left: Option<String>,
    #[serde(default)] four_finger_swipe_right: Option<String>,
    #[serde(default)] four_finger_spread: Option<String>,
    #[serde(default)] four_finger_pinch: Option<String>,
    #[serde(default)] three_finger_tap_action: Option<String>,
    #[serde(default)] custom_shortcuts: Option<std::collections::HashMap<String, String>>,
}

#[tauri::command]
fn save_config(args: SaveArgs, state: tauri::State<'_, AppState>) -> Result<Config, String> {
    let mut config = Config::load();

    if let Some(v) = args.three_finger_drag { config.three_finger_drag = v; }
    if let Some(v) = args.three_finger_tap_enabled { config.three_finger_tap_enabled = v; }
    if let Some(v) = args.allow_release_and_restart { config.allow_release_and_restart = v; }
    if let Some(v) = args.release_delay_ms { config.release_delay_ms = v; }
    if let Some(v) = args.start_threshold { config.start_threshold = v; }
    if let Some(v) = args.stop_threshold { config.stop_threshold = v; }
    if let Some(v) = args.edge_slide_enabled { config.edge_slide_enabled = v; }
    if let Some(v) = args.run_at_startup { config.run_at_startup = v; }
    if let Some(v) = args.close_to_tray { config.close_to_tray = v; }
    if let Some(v) = args.invert_volume { config.invert_volume = v; }
    if let Some(v) = args.invert_brightness { config.invert_brightness = v; }
    if let Some(v) = args.smooth_scroll_enabled { config.smooth_scroll_enabled = v; }
    if let Some(v) = args.smooth_scroll_speed { config.smooth_scroll_speed = v; }
    if let Some(v) = args.smooth_scroll_smoothing { config.smooth_scroll_smoothing = v; }
    if let Some(v) = args.smooth_scroll_deceleration { config.smooth_scroll_deceleration = v; }
    if let Some(v) = args.smooth_scroll_base_scale { config.smooth_scroll_base_scale = v; }
    if let Some(v) = args.smooth_scroll_max_delta { config.smooth_scroll_max_delta = v; }
    if let Some(v) = args.smooth_scroll_deadzone { config.smooth_scroll_deadzone = v; }
    if let Some(v) = args.smooth_scroll_tick_ms { config.smooth_scroll_tick_ms = v; }
    if let Some(v) = args.natural_scroll { config.natural_scroll = v; }

    // Scroll policy (生效策略)
    if let Some(v) = args.scroll_policy_enabled { config.scroll_policy_enabled = v; }
    if let Some(v) = args.scroll_policy_fullscreen_disabled { config.scroll_policy_fullscreen_disabled = v; }
    if let Some(v) = args.scroll_policy_blacklist_enabled { config.scroll_policy_blacklist_enabled = v; }
    if let Some(v) = args.scroll_policy_browser_only { config.scroll_policy_browser_only = v; }
    if let Some(v) = args.scroll_policy_whitelist_enabled { config.scroll_policy_whitelist_enabled = v; }
    if let Some(v) = args.scroll_policy_mode { config.scroll_policy_mode = v; }
    if let Some(v) = args.scroll_policy_blacklist { config.scroll_policy_blacklist = v; }
    if let Some(v) = args.scroll_policy_whitelist { config.scroll_policy_whitelist = v; }

    if let Some(s) = args.four_finger_swipe_up { config.four_finger_swipe_up = parse_action(&s); }
    if let Some(s) = args.four_finger_swipe_down { config.four_finger_swipe_down = parse_action(&s); }
    if let Some(s) = args.four_finger_swipe_left { config.four_finger_swipe_left = parse_action(&s); }
    if let Some(s) = args.four_finger_swipe_right { config.four_finger_swipe_right = parse_action(&s); }
    if let Some(s) = args.four_finger_spread { config.four_finger_spread = parse_action(&s); }
    if let Some(s) = args.four_finger_pinch { config.four_finger_pinch = parse_action(&s); }

    if let Some(s) = args.three_finger_tap_action { config.three_finger_tap_action = parse_action(&s); }
    if let Some(m) = args.custom_shortcuts { config.custom_shortcuts = m; }

    // Update cursor speed and acceleration independently
    let mut dc = config.device_configs.get("default").cloned()
        .unwrap_or_else(DeviceDragConfig::default);
    if let Some(speed) = args.cursor_speed { dc.cursor_speed = speed; }
    if let Some(accel) = args.cursor_acceleration { dc.cursor_acceleration = accel; }
    if args.cursor_speed.is_some() || args.cursor_acceleration.is_some() {
        config.device_configs.insert("default".into(), dc);
    }

    // Handle auto-start
    if let Some(auto) = args.run_at_startup {
        if auto {
            let _ = mac_touchpad_core::startup::enable_auto_start();
        } else {
            let _ = mac_touchpad_core::startup::disable_auto_start();
        }
    }

    if let Err(e) = config.save() {
        return Err(format!("无法写入配置文件: {}", e));
    }

    // Send to core engine
    if let Ok(guard) = state.core_tx.lock() {
        if let Some(ref tx) = *guard {
            let _ = tx.send(CoreCommand::UpdateConfig(config.clone()));
        }
    }

    Ok(config)
}

#[tauri::command]
fn get_action_list() -> Vec<ActionInfo> {
    vec![
        ActionInfo { key: "None".into(), label: "无".into() },
        ActionInfo { key: "WinTab".into(), label: "任务视图".into() },
        ActionInfo { key: "AltTab".into(), label: "Alt+Tab 切换窗口".into() },
        ActionInfo { key: "CtrlWinLeft".into(), label: "左桌面".into() },
        ActionInfo { key: "CtrlWinRight".into(), label: "右桌面".into() },
        ActionInfo { key: "ShowDesktop".into(), label: "显示桌面".into() },
        ActionInfo { key: "OpenStart".into(), label: "开始菜单".into() },
        ActionInfo { key: "Search".into(), label: "搜索 (Win+S)".into() },
        ActionInfo { key: "NotificationCenter".into(), label: "通知中心".into() },
        ActionInfo { key: "VolumeUp".into(), label: "音量+".into() },
        ActionInfo { key: "VolumeDown".into(), label: "音量-".into() },
        ActionInfo { key: "PageUp".into(), label: "Page Up".into() },
        ActionInfo { key: "PageDown".into(), label: "Page Down".into() },
        ActionInfo { key: "Maximize".into(), label: "最大化窗口".into() },
        ActionInfo { key: "Custom".into(), label: "自定义快捷键".into() },
    ]
}

#[tauri::command]
fn get_version() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        name: "MacTouchpad".into(),
        description: "在 Windows 上实现 Mac 触控板手势体验".into(),
        author: "xiao.luy".into(),
        repo: "https://github.com/xiaolu12-up/Mac-touchpad".into(),
    }
}

#[derive(Deserialize, Debug, Clone)]
struct RawRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    asset_url: Option<String>,
}

fn is_newer_version(current: &str, latest: &str) -> bool {
    let current_clean = current.trim().trim_start_matches('v').trim_start_matches('V');
    let latest_clean = latest.trim().trim_start_matches('v').trim_start_matches('V');
    
    let c_parts: Vec<&str> = current_clean.split('.').collect();
    let l_parts: Vec<&str> = latest_clean.split('.').collect();
    
    for i in 0..std::cmp::max(c_parts.len(), l_parts.len()) {
        let c_val: u32 = c_parts.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
        let l_val: u32 = l_parts.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
        if l_val > c_val {
            return true;
        } else if c_val > l_val {
            return false;
        }
    }
    false
}

#[tauri::command]
fn check_update() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION");
    let gitee = check_gitee_update();
    let github = check_github_update();
    
    // Log any errors for debugging
    if let Err(ref e) = gitee {
        tracing::warn!("Gitee update check failed: {}", e);
    }
    if let Err(ref e) = github {
        tracing::warn!("GitHub update check failed: {}", e);
    }
    
    // If BOTH failed, return a combined error because we have no update information at all
    if gitee.is_err() && github.is_err() {
        return Err(format!(
            "无法连接到更新服务器。\nGitee 错误: {}\nGitHub 错误: {}",
            gitee.unwrap_err(),
            github.unwrap_err()
        ));
    }
    
    let mut best_update: Option<RawRelease> = None;
    
    // Check successful results for newer version
    for update in [gitee.as_ref().ok(), github.as_ref().ok()].into_iter().flatten() {
        if is_newer_version(current, &update.tag_name) {
            match best_update {
                None => best_update = Some(update.clone()),
                Some(ref best) => {
                    if is_newer_version(&best.tag_name, &update.tag_name) {
                        best_update = Some(update.clone());
                    }
                }
            }
        }
    }
    
    match best_update {
        Some(raw) => Ok(UpdateInfo {
            has_update: true,
            current_version: current.into(),
            latest_version: raw.tag_name,
            download_url: raw.html_url,
            asset_url: raw.asset_url.unwrap_or_default(),
            body: raw.body.unwrap_or_default(),
        }),
        None => {
            // Since at least one server succeeded, we return Ok(has_update: false)
            // instead of throwing an error for the failed server
            Ok(UpdateInfo {
                has_update: false,
                current_version: current.into(),
                latest_version: current.into(),
                download_url: String::new(),
                asset_url: String::new(),
                body: String::new(),
            })
        }
    }
}

#[derive(Deserialize, Debug)]
struct GiteeRelease {
    tag_name: String,
    body: Option<String>,
    assets: Vec<GiteeAsset>,
}

#[derive(Deserialize, Debug)]
struct GiteeAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize, Debug)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize, Debug)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

fn check_gitee_update() -> Result<RawRelease, String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("curl.exe")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(["-s", "https://gitee.com/api/v5/repos/lu52/Mac-touchpad/releases/latest"])
        .output()
        .map_err(|e| format!("执行 curl 失败: {}", e))?;

    if !output.status.success() {
        return Err(format!("curl 返回错误状态: {:?}", output.status.code()));
    }

    let release: GiteeRelease = serde_json::from_slice(&output.stdout)
        .map_err(|e| {
            let body = String::from_utf8_lossy(&output.stdout);
            format!("解析 JSON 失败: {}, 响应体: {}", e, body)
        })?;
    
    // Find MSI asset, fallback to EXE
    let asset = release.assets.iter()
        .find(|a| a.name.ends_with(".msi"))
        .or_else(|| release.assets.iter().find(|a| a.name.ends_with(".exe")));
        
    let asset_url = asset.map(|a| a.browser_download_url.clone());
    let html_url = format!("https://gitee.com/lu52/Mac-touchpad/releases/tag/{}", release.tag_name);

    Ok(RawRelease {
        tag_name: release.tag_name,
        html_url,
        body: release.body,
        asset_url,
    })
}

fn check_github_update() -> Result<RawRelease, String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("curl.exe")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(["-s", "-H", "User-Agent: Mac-touchpad", "https://api.github.com/repos/xiaolu12-up/Mac-touchpad/releases/latest"])
        .output()
        .map_err(|e| format!("执行 curl 失败: {}", e))?;

    if !output.status.success() {
        return Err(format!("curl 返回错误状态: {:?}", output.status.code()));
    }

    let release: GithubRelease = serde_json::from_slice(&output.stdout)
        .map_err(|e| {
            let body = String::from_utf8_lossy(&output.stdout);
            format!("解析 JSON 失败: {}, 响应体: {}", e, body)
        })?;
    
    // Find MSI asset, fallback to EXE
    let asset = release.assets.iter()
        .find(|a| a.name.ends_with(".msi"))
        .or_else(|| release.assets.iter().find(|a| a.name.ends_with(".exe")));
        
    let asset_url = asset.map(|a| a.browser_download_url.clone());

    Ok(RawRelease {
        tag_name: release.tag_name,
        html_url: release.html_url,
        body: release.body,
        asset_url,
    })
}

#[tauri::command]
fn open_url(url: String) {
    let _ = tauri_plugin_opener::open_url(url, None::<&str>);
}

#[tauri::command]
fn download_update(url: String) -> Result<String, String> {
    use std::os::windows::process::CommandExt;

    // Get temp directory path
    let mut dest_path = std::env::temp_dir();
    let file_name = url.split('/').last().unwrap_or("update.exe");
    dest_path.push(file_name);
    let dest_str = dest_path.to_string_lossy().to_string();

    let output = std::process::Command::new("powershell.exe")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                r#"[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12; (New-Object System.Net.WebClient).DownloadFile('{}', '{}')"#,
                url, dest_str
            )
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => Ok(dest_str),
        Ok(out) => Err(format!(
            "下载失败: {}",
            String::from_utf8_lossy(&out.stderr)
        )),
        Err(e) => Err(format!("无法执行下载命令: {}", e)),
    }
}

#[tauri::command]
fn run_installer(path: String) -> Result<(), String> {
    let is_msi = path.ends_with(".msi");
    let status = if is_msi {
        std::process::Command::new("msiexec.exe")
            .args(["/i", &path])
            .spawn()
    } else {
        std::process::Command::new(&path)
            .spawn()
    };

    match status {
        Ok(_) => {
            std::process::exit(0);
        }
        Err(e) => Err(format!("启动安装程序失败: {}", e)),
    }
}

#[derive(Serialize)]
struct ActionInfo { key: String, label: String }

#[derive(Serialize)]
struct AppInfo {
    version: String,
    name: String,
    description: String,
    author: String,
    repo: String,
}

#[derive(Serialize)]
struct UpdateInfo {
    has_update: bool,
    current_version: String,
    latest_version: String,
    download_url: String,
    asset_url: String,
    body: String,
}

fn parse_action(s: &str) -> GestureAction {
    match s {
        "WinTab" => GestureAction::WinTab,
        "AltTab" => GestureAction::AltTab,
        "CtrlWinLeft" => GestureAction::CtrlWinLeft,
        "CtrlWinRight" => GestureAction::CtrlWinRight,
        "ShowDesktop" => GestureAction::ShowDesktop,
        "OpenStart" => GestureAction::OpenStart,
        "Search" => GestureAction::Search,
        "NotificationCenter" => GestureAction::NotificationCenter,
        "VolumeUp" => GestureAction::VolumeUp,
        "VolumeDown" => GestureAction::VolumeDown,
        "BrightnessUp" => GestureAction::BrightnessUp,
        "BrightnessDown" => GestureAction::BrightnessDown,
        "PageUp" => GestureAction::PageUp,
        "PageDown" => GestureAction::PageDown,
        "BrowserBack" => GestureAction::BrowserBack,
        "BrowserForward" => GestureAction::BrowserForward,
        "Maximize" => GestureAction::Maximize,
        "Custom" => GestureAction::Custom,
        _ => GestureAction::None,
    }
}

#[derive(Serialize)]
struct DiagnosticsReport {
    app_version: String,
    os_info: String,
    is_admin: bool,
    touchpad: mac_touchpad_core::debug::DeviceDiagnostics,
    recent_logs: Vec<mac_touchpad_core::debug::DebugLogEntry>,
}

#[tauri::command]
fn get_diagnostics_report() -> DiagnosticsReport {
    let os_info = format!("Windows ({})", std::env::consts::ARCH);
    let is_admin = is_process_elevated();
    DiagnosticsReport {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os_info,
        is_admin,
        touchpad: mac_touchpad_core::debug::get_device_diagnostics(),
        recent_logs: mac_touchpad_core::debug::get_debug_logs(),
    }
}

#[tauri::command]
fn set_debug_mode(enabled: bool) {
    mac_touchpad_core::debug::set_debug_mode_enabled(enabled);
}

#[tauri::command]
fn clear_diagnostics_logs() {
    mac_touchpad_core::debug::clear_debug_logs();
}

#[tauri::command]
fn open_log_dir() -> Result<(), String> {
    let mut path = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    path.push("MacTouchpad");
    path.push("logs");
    let _ = std::fs::create_dir_all(&path);
    std::process::Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn export_log_file() -> Result<String, String> {
    let rep = get_diagnostics_report();
    let mut dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    dir.push("MacTouchpad");
    dir.push("logs");
    let _ = std::fs::create_dir_all(&dir);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let file_name = format!("touchpad_diagnostics_{}.log", now);
    let mut file_path = dir.clone();
    file_path.push(&file_name);

    let logs_str = rep.recent_logs.iter()
        .map(|l| format!("[{}] [{}] {}", l.time, l.level, l.message))
        .collect::<Vec<_>>()
        .join("\r\n");

    let content = format!(
        "====================================================\r\n\
MacTouchpad 诊断与触控日志报告\r\n\
====================================================\r\n\
软件版本: v{}\r\n\
系统架构: {}\r\n\
管理员权限: {}\r\n\
触控板检测状态: {}\r\n\
硬件厂商ID (Vendor ID): {}\r\n\
产品ID (Product ID): {}\r\n\
设备哈希 ID: {}\r\n\
坐标逻辑范围: X[{}, {}], Y[{}, {}]\r\n\
最近触点数据: {}\r\n\
====================================================\r\n\
触控与手势日志流:\r\n\
====================================================\r\n\
{}\r\n",
        rep.app_version,
        rep.os_info,
        if rep.is_admin { "是 (Elevated)" } else { "否 (Standard)" },
        if rep.touchpad.device_detected { "已检测到 (PTP)" } else { "未检测到" },
        rep.touchpad.vendor_id,
        rep.touchpad.product_id,
        rep.touchpad.device_id,
        rep.touchpad.x_range.0, rep.touchpad.x_range.1,
        rep.touchpad.y_range.0, rep.touchpad.y_range.1,
        if rep.touchpad.last_contacts_str.is_empty() { "无" } else { &rep.touchpad.last_contacts_str },
        logs_str,
    );

    std::fs::write(&file_path, content).map_err(|e| format!("写入日志文件失败: {}", e))?;
    let path_str = file_path.to_string_lossy().to_string();

    // Select file in explorer
    let _ = std::process::Command::new("explorer.exe")
        .args(["/select,", &path_str])
        .spawn();

    Ok(path_str)
}

fn is_process_elevated() -> bool {
    use windows::Win32::Security::*;
    use windows::Win32::System::Threading::*;
    unsafe {
        let mut token = windows::Win32::Foundation::HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_ok() {
            let mut elevation = TOKEN_ELEVATION::default();
            let mut ret_len = 0u32;
            let res = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret_len,
            );
            let _ = windows::Win32::Foundation::CloseHandle(token);
            if res.is_ok() {
                return elevation.TokenIsElevated != 0;
            }
        }
        false
    }
}

#[tauri::command]
async fn record_shortcut_cmd() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        mac_touchpad_core::recorder::record_shortcut()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
        .init();

    let args: Vec<String> = std::env::args().collect();
    let is_autostart = args.iter().any(|arg| arg == "--autostart");
    if is_autostart {
        tracing::info!("App launched via autostart. Delaying startup by 5 seconds to let system settle...");
        std::thread::sleep(std::time::Duration::from_secs(5));
    }

    tracing::info!("MacTouchpad Tauri starting...");

    let config = Config::load();
    tracing::info!("Config: drag={}, speed={}", config.three_finger_drag,
        config.device_configs.values().next().map(|d| d.cursor_speed as i32).unwrap_or(60));

    // Start core engine
    let core_tx = mac_touchpad_core::window::start_message_loop(config.clone(), None);
    tracing::info!("Core engine started");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tracing::info!("Another instance attempted to start, focusing existing window.");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .manage(AppState {
            core_tx: Mutex::new(Some(core_tx)),
        })
        .setup(|app| {
            // Build system tray menu
            let show_item = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray_icon.png"))
                .expect("Failed to load tray icon");
            let _tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .menu(&menu)
                .tooltip("MacTouchpad")
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Check if --autostart argument is passed. If true, keep hidden; if false, show.
            let args: Vec<String> = std::env::args().collect();
            let is_autostart = args.iter().any(|arg| arg == "--autostart");
            if is_autostart {
                tracing::info!("App launched via autostart. Remaining hidden in tray.");
            } else {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                    tracing::info!("App launched manually. Showing main window.");
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Close to tray instead of exiting
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let config = Config::load();
                if config.close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_action_list,
            get_version,
            check_update,
            open_url,
            download_update,
            run_installer,
            record_shortcut_cmd,
            browse_and_get_exe_info,
            inspect_exe_path,
            show_system_notification,
            get_diagnostics_report,
            set_debug_mode,
            clear_diagnostics_logs,
            open_log_dir,
            export_log_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("0.1.2", "0.1.3"));
        assert!(is_newer_version("v0.1.2", "0.1.3"));
        assert!(is_newer_version("0.1.2", "v0.1.3"));
        assert!(is_newer_version("v0.1.2", "v0.1.3"));
        assert!(is_newer_version("0.1.2", "V0.1.3"));
        
        assert!(!is_newer_version("0.1.3", "0.1.2"));
        assert!(!is_newer_version("v0.1.3", "0.1.2"));
        assert!(!is_newer_version("0.1.3", "v0.1.2"));
        assert!(!is_newer_version("v0.1.3", "v0.1.2"));
        
        assert!(!is_newer_version("0.1.2", "0.1.2"));
        assert!(!is_newer_version("v0.1.2", "0.1.2"));
        assert!(!is_newer_version("0.1.2", "v0.1.2"));
        assert!(!is_newer_version("v0.1.2", "v0.1.2"));
    }

    #[test]
    fn test_check_updates() {
        match check_github_update() {
            Ok(gh) => println!("GitHub latest release: {:?}", gh),
            Err(e) => println!("GitHub update check failed (warning only): {}", e),
        }

        match check_gitee_update() {
            Ok(gt) => println!("Gitee latest release: {:?}", gt),
            Err(e) => println!("Gitee update check failed (warning only): {}", e),
        }
    }
}
