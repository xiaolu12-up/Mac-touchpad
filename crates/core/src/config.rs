use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Current settings version for migration support.
const CURRENT_VERSION: u32 = 8;

/// Which mouse button to simulate for 3-finger drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DragButton {
    Left,
    Right,
    Middle,
    None,
}

impl Default for DragButton {
    fn default() -> Self {
        Self::Left
    }
}

/// Action to perform for a gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GestureAction {
    None,
    WinTab,
    AltTab,
    CtrlWinLeft,
    CtrlWinRight,
    ShowDesktop,
    OpenStart,
    Search,
    // New actions
    BrowserBack,
    BrowserForward,
    NotificationCenter,
    VolumeUp,
    VolumeDown,
    BrightnessUp,
    BrightnessDown,
    PageUp,
    PageDown,
    Maximize,
}

impl Default for GestureAction {
    fn default() -> Self {
        Self::None
    }
}

/// Per-device drag configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDragConfig {
    pub cursor_move: bool,
    pub cursor_speed: f32,
    pub cursor_acceleration: f32,
}

impl Default for DeviceDragConfig {
    fn default() -> Self {
        Self {
            cursor_move: true,
            cursor_speed: 60.0,
            cursor_acceleration: 10.0,
        }
    }
}

/// Main application configuration. Persisted as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,

    // 3-finger drag (ported from reference project)
    pub three_finger_drag: bool,
    pub drag_button: DragButton,
    pub allow_release_and_restart: bool,
    pub release_delay_ms: u32,
    pub device_configs: HashMap<String, DeviceDragConfig>,
    pub cursor_averaging: u32,
    pub max_finger_move_distance: u32,
    pub start_threshold: f32,
    pub stop_threshold: f32,

    // 3-finger tap
    pub three_finger_tap_enabled: bool,

    // 4-finger gestures
    pub four_finger_swipe_up: GestureAction,
    pub four_finger_swipe_down: GestureAction,
    pub four_finger_swipe_left: GestureAction,
    pub four_finger_swipe_right: GestureAction,
    pub four_finger_spread: GestureAction,
    pub four_finger_pinch: GestureAction,
    pub swipe_threshold: f32,

    // 2-finger gestures
    pub two_finger_swipe_left: GestureAction,
    pub two_finger_swipe_right: GestureAction,
    pub two_finger_swipe_threshold: f32,

    // Edge gestures
    pub edge_slide_enabled: bool,
    pub edge_threshold: i32,
    pub edge_slide_threshold: i32,
    pub tap_max_duration_ms: u64,
    pub tap_max_distance: f32,
    pub pinch_spread_threshold: f32,

    // Volume/Brightness
    pub invert_volume: bool,
    pub invert_brightness: bool,

    // Smooth scroll
    pub smooth_scroll_enabled: bool,
    pub smooth_scroll_speed: f32,
    pub smooth_scroll_smoothing: f32,
    pub smooth_scroll_deceleration: f32,
    pub smooth_scroll_base_scale: f32,
    pub smooth_scroll_max_delta: f32,
    pub smooth_scroll_deadzone: f32,
    pub smooth_scroll_tick_ms: u64,
    pub natural_scroll: bool,

    // General
    pub run_at_startup: bool,
    pub close_to_tray: bool,
    pub record_logs: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,

            three_finger_drag: true,
            drag_button: DragButton::Left,
            allow_release_and_restart: true,
            release_delay_ms: 500,
            device_configs: HashMap::new(),
            cursor_averaging: 1,
            max_finger_move_distance: 0,
            start_threshold: 40.0,
            stop_threshold: 5.0,

            three_finger_tap_enabled: true,

            four_finger_swipe_up: GestureAction::WinTab,
            four_finger_swipe_down: GestureAction::ShowDesktop,
            four_finger_swipe_left: GestureAction::AltTab,
            four_finger_swipe_right: GestureAction::AltTab,
            four_finger_spread: GestureAction::ShowDesktop,
            four_finger_pinch: GestureAction::OpenStart,
            swipe_threshold: 150.0,

            // 2-finger gestures — disabled by default (user requested removal)
            two_finger_swipe_left: GestureAction::None,
            two_finger_swipe_right: GestureAction::None,
            two_finger_swipe_threshold: 80.0,

            // Edge gestures
            edge_slide_enabled: true,
            edge_threshold: 800,    // logical units from edge
            edge_slide_threshold: 200, // logical units of movement to trigger

            tap_max_duration_ms: 200,
            tap_max_distance: 120.0,
            pinch_spread_threshold: 100.0,

            // Volume/Brightness
            invert_volume: false,
            invert_brightness: false,

            // Smooth scroll
            smooth_scroll_enabled: true,
            smooth_scroll_speed: 1.0,
            smooth_scroll_smoothing: 0.30,
            smooth_scroll_deceleration: 0.92,
            smooth_scroll_base_scale: 0.2,
            smooth_scroll_max_delta: 20.0,
            smooth_scroll_deadzone: 1.0,
            smooth_scroll_tick_ms: 4,
            natural_scroll: true,

            run_at_startup: false,
            close_to_tray: true,
            record_logs: false,
        }
    }
}

/// Get the config file path: %APPDATA%/MacTouchpad/preferences.json
fn config_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("MacTouchpad");
    path.push("preferences.json");
    path
}

impl Config {
    /// Load configuration from disk, applying migrations as needed.
    /// Returns default config if file doesn't exist or is corrupt.
    pub fn load() -> Self {
        let path = config_path();
        let data = match std::fs::read_to_string(&path) {
            Ok(data) => data,
            Err(_) => return Self::default(),
        };

        let mut config: Config = match serde_json::from_str(&data) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to parse config, using defaults: {}", e);
                return Self::default();
            }
        };

        // Apply version migrations
        if config.version < CURRENT_VERSION {
            config.migrate();
            let _ = config.save();
        }

        config
    }

    /// Save configuration to disk.
    pub fn save(&self) -> std::io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&path, json)
    }

    /// Apply incremental version migrations.
    fn migrate(&mut self) {
        // Future migrations go here, e.g.:
        // if self.version < 2 { ... }
        // if self.version < 3 { ... }
        self.version = CURRENT_VERSION;
    }

    /// Get the device-specific config, falling back to any available config, then defaults.
    pub fn device_config(&self, device_id: &str) -> DeviceDragConfig {
        // Try exact match first
        if let Some(cfg) = self.device_configs.get(device_id) {
            return cfg.clone();
        }
        // Fall back to "default" key
        if let Some(cfg) = self.device_configs.get("default") {
            return cfg.clone();
        }
        // Fall back to first available config
        if let Some(cfg) = self.device_configs.values().next() {
            return cfg.clone();
        }
        // Fall back to defaults
        DeviceDragConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_serializes() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, CURRENT_VERSION);
        assert!(parsed.three_finger_drag);
        assert_eq!(parsed.drag_button, DragButton::Left);
        assert_eq!(parsed.four_finger_swipe_up, GestureAction::WinTab);
    }

    #[test]
    fn test_config_migration() {
        let mut config = Config::default();
        config.version = 1;
        config.migrate();
        assert_eq!(config.version, CURRENT_VERSION);
    }
}
