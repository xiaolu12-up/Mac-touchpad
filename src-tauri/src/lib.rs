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
        repo: "https://github.com/user/mac-touchpad".into(),
    }
}

#[tauri::command]
fn check_update() -> UpdateInfo {
    // Check GitHub for latest release
    let current = env!("CARGO_PKG_VERSION");
    match check_github_update() {
        Some((ver, url)) if ver != current => UpdateInfo {
            has_update: true,
            current_version: current.into(),
            latest_version: ver,
            download_url: url,
        },
        _ => UpdateInfo {
            has_update: false,
            current_version: current.into(),
            latest_version: current.into(),
            download_url: String::new(),
        },
    }
}

fn check_github_update() -> Option<(String, String)> {
    // Simple HTTP check via GitHub API (no external crate needed)
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command",
            "try { $r = Invoke-RestMethod -Uri 'https://api.github.com/repos/user/mac-touchpad/releases/latest' -TimeoutSec 5; $r.tag_name + '|' + $r.html_url } catch { '' }"
        ])
        .output()
        .ok()?;

    let text = String::from_utf8(output.stdout).ok()?;
    let parts: Vec<&str> = text.trim().split('|').collect();
    if parts.len() >= 2 && !parts[0].is_empty() {
        Some((parts[0].trim().to_string(), parts[1].trim().to_string()))
    } else {
        None
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

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
