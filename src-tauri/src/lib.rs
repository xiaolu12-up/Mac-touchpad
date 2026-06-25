use mac_touchpad_core::config::{Config, DeviceDragConfig, GestureAction};
use mac_touchpad_core::window::{CoreCommand, CoreSender};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::Manager;
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
    #[serde(default)] four_finger_swipe_up: Option<String>,
    #[serde(default)] four_finger_swipe_down: Option<String>,
    #[serde(default)] four_finger_swipe_left: Option<String>,
    #[serde(default)] four_finger_swipe_right: Option<String>,
    #[serde(default)] four_finger_spread: Option<String>,
    #[serde(default)] four_finger_pinch: Option<String>,
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

    if let Some(s) = args.four_finger_swipe_up { config.four_finger_swipe_up = parse_action(&s); }
    if let Some(s) = args.four_finger_swipe_down { config.four_finger_swipe_down = parse_action(&s); }
    if let Some(s) = args.four_finger_swipe_left { config.four_finger_swipe_left = parse_action(&s); }
    if let Some(s) = args.four_finger_swipe_right { config.four_finger_swipe_right = parse_action(&s); }
    if let Some(s) = args.four_finger_spread { config.four_finger_spread = parse_action(&s); }
    if let Some(s) = args.four_finger_pinch { config.four_finger_pinch = parse_action(&s); }

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

#[derive(Deserialize, Debug)]
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
fn check_update() -> UpdateInfo {
    let current = env!("CARGO_PKG_VERSION");
    let update = check_gitee_update().or_else(check_github_update);
    match update {
        Some(raw) if is_newer_version(current, &raw.tag_name) => UpdateInfo {
            has_update: true,
            current_version: current.into(),
            latest_version: raw.tag_name,
            download_url: raw.html_url,
            asset_url: raw.asset_url.unwrap_or_default(),
            body: raw.body.unwrap_or_default(),
        },
        _ => UpdateInfo {
            has_update: false,
            current_version: current.into(),
            latest_version: current.into(),
            download_url: String::new(),
            asset_url: String::new(),
            body: String::new(),
        },
    }
}

fn check_gitee_update() -> Option<RawRelease> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("powershell.exe")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(["-NoProfile", "-Command",
            r#"[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; try { $c = New-Object System.Net.WebClient; $c.Headers.Add('User-Agent', 'Mac-touchpad'); [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12; $bytes = $c.DownloadData('https://gitee.com/api/v5/repos/lu52/Mac-touchpad/releases/latest'); $text = [System.Text.Encoding]::UTF8.GetString($bytes); $json = $text | ConvertFrom-Json; $asset = $json.assets | Where-Object { $_.name -like '*.msi' }; if (-not $asset) { $asset = $json.assets | Where-Object { $_.name -like '*.exe' } }; $asset_url = if ($asset) { $asset.browser_download_url } else { '' }; $html_url = 'https://gitee.com/lu52/Mac-touchpad/releases/tag/' + $json.tag_name; [PSCustomObject]@{tag_name=$json.tag_name; html_url=$html_url; body=$json.body; asset_url=$asset_url} | ConvertTo-Json -Compress } catch { '' }"#
        ])
        .output()
        .ok()?;

    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim().trim_start_matches('\u{feff}');
    if text.is_empty() {
        None
    } else {
        serde_json::from_str::<RawRelease>(text).ok()
    }
}

fn check_github_update() -> Option<RawRelease> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("powershell.exe")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(["-NoProfile", "-Command",
            r#"[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; try { $c = New-Object System.Net.WebClient; $c.Headers.Add('User-Agent', 'Mac-touchpad'); [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12; $bytes = $c.DownloadData('https://api.github.com/repos/xiaolu12-up/Mac-touchpad/releases/latest'); $text = [System.Text.Encoding]::UTF8.GetString($bytes); $json = $text | ConvertFrom-Json; $asset = $json.assets | Where-Object { $_.name -like '*.msi' }; if (-not $asset) { $asset = $json.assets | Where-Object { $_.name -like '*.exe' } }; $asset_url = if ($asset) { $asset.browser_download_url } else { '' }; [PSCustomObject]@{tag_name=$json.tag_name; html_url=$json.html_url; body=$json.body; asset_url=$asset_url} | ConvertTo-Json -Compress } catch { '' }"#
        ])
        .output()
        .ok()?;

    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim().trim_start_matches('\u{feff}');
    if text.is_empty() {
        None
    } else {
        serde_json::from_str::<RawRelease>(text).ok()
    }
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
        _ => GestureAction::None,
    }
}

// ── Entry Point ──

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
        .init();

    tracing::info!("MacTouchpad Tauri starting...");

    let config = Config::load();
    tracing::info!("Config: drag={}, speed={}", config.three_finger_drag,
        config.device_configs.values().next().map(|d| d.cursor_speed as i32).unwrap_or(60));

    // Start core engine
    let core_tx = mac_touchpad_core::window::start_message_loop(config.clone(), None);
    tracing::info!("Core engine started");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
