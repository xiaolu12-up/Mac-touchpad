use crate::config::{Config, GestureAction};
use crate::gesture::drag::{ThreeFingerDrag, TimerAction};
use crate::gesture::pinch::{PinchDetector, PinchDirection};
use crate::gesture::swipe::{SwipeDetector, SwipeDirection};
use crate::gesture::tap::TapDetector;
use crate::gesture::two_finger::{
    EdgeSlideDetector, TwoFingerSwipeDetector,
};
use crate::hid::types::TouchpadContact;
use crate::input::{keyboard, mouse::MouseSimulator};
use crate::input::wheel_hook::TOUCHPAD_SCROLLING;
use std::sync::atomic::Ordering;

/// Central gesture orchestrator. Receives each frame of contacts and routes
/// them to the appropriate gesture recognizer based on finger count.
///
/// Priority: 4-finger > 3-finger drag > 2-finger swipe/edge > 3-finger tap.
pub struct GestureEngine {
    config: Config,
    drag: ThreeFingerDrag,
    tap_detector: TapDetector,           // 4-finger tap
    three_finger_tap: TapDetector,       // 3-finger tap (dedicated)
    swipe_detector: SwipeDetector,           // 4-finger
    two_finger_swipe: TwoFingerSwipeDetector, // 2-finger
    edge_slide: EdgeSlideDetector,            // single-finger edge
    pinch_detector: PinchDetector,
    mouse: MouseSimulator,
    old_contacts: Vec<TouchpadContact>,
    last_contact_time: i64,
    current_device_id: String,
    /// Touchpad logical coordinate ranges (updated from HID parser).
    x_range: (i32, i32),
    y_range: (i32, i32),
}

impl GestureEngine {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            drag: ThreeFingerDrag::new(),
            tap_detector: TapDetector::new(4),
            three_finger_tap: TapDetector::new(3),
            swipe_detector: SwipeDetector::new(),
            two_finger_swipe: TwoFingerSwipeDetector::new(),
            edge_slide: EdgeSlideDetector::new(),
            pinch_detector: PinchDetector::new(),
            mouse: MouseSimulator::new(),
            old_contacts: Vec::new(),
            last_contact_time: 0,
            current_device_id: String::new(),
            x_range: (0, 4096),  // default, updated from HID
            y_range: (0, 4096),
        }
    }

    /// Update touchpad logical coordinate ranges (from HID value caps).
    pub fn set_touchpad_ranges(&mut self, x_range: (i32, i32), y_range: (i32, i32)) {
        self.x_range = x_range;
        self.y_range = y_range;
    }

    /// Process a new frame of touchpad contacts.
    pub fn on_touchpad_contact(
        &mut self,
        device_id: &str,
        contacts: Vec<TouchpadContact>,
    ) -> TimerAction {
        let now = current_time_ms();
        let elapsed = if self.last_contact_time > 0 {
            now - self.last_contact_time
        } else {
            0
        };

        // Swap contacts without cloning — reuse the old buffer
        let old = std::mem::take(&mut self.old_contacts);
        let finger_count = contacts.len();

        let timer_action = if finger_count == 4 {
            // 4-finger gestures take priority — force stop any active drag
            if self.drag.is_dragging() {
                self.drag.force_stop(&self.config, &mut self.mouse);
            }
            self.two_finger_swipe.reset();
            self.edge_slide.reset();

            // Feed tap detector
            self.tap_detector.feed(
                &contacts,
                now,
                self.config.tap_max_duration_ms,
                self.config.tap_max_distance,
            );

            // Feed 4-finger swipe detector
            if let Some(direction) = self.swipe_detector.feed(&contacts, self.config.swipe_threshold)
            {
                self.execute_swipe_action(direction);
            }

            // Feed pinch/spread detector
            if let Some(direction) =
                self.pinch_detector.feed(&contacts, self.config.pinch_spread_threshold)
            {
                self.execute_pinch_action(direction);
            }

            TimerAction::None
        } else if finger_count == 3 {
            // 3-finger: drag + tap detection
            self.two_finger_swipe.reset();
            self.edge_slide.reset();

            let action = if self.config.three_finger_drag {
                self.drag.on_touchpad_contact(
                    device_id,
                    &self.config,
                    &mut self.mouse,
                    &old,
                    &contacts,
                    elapsed,
                )
            } else {
                TimerAction::None
            };

            // Three-finger tap → Win+S search
            if !self.drag.is_dragging() && self.config.three_finger_tap_enabled {
                if self.three_finger_tap.feed(
                    &contacts,
                    now,
                    self.config.tap_max_duration_ms,
                    self.config.tap_max_distance,
                ) {
                    tracing::info!("Three-finger tap detected → Win+S");
                    keyboard::search();
                }
            }

            action
        } else if finger_count == 2 {
            // 2 fingers — native scroll
            if self.drag.is_dragging() {
                self.drag.force_stop(&self.config, &mut self.mouse);
            }
            self.edge_slide.reset();
            self.swipe_detector.reset();
            self.pinch_detector.reset();

            TOUCHPAD_SCROLLING.store(false, Ordering::Relaxed);

            TimerAction::None
        } else if finger_count == 1 {
            // Single finger: edge slide for volume/brightness
            if self.drag.is_dragging() {
                self.drag.force_stop(&self.config, &mut self.mouse);
            }
            self.two_finger_swipe.reset();
            self.swipe_detector.reset();
            self.pinch_detector.reset();

            if self.config.edge_slide_enabled {
                if let Some(action) = self.edge_slide.feed(
                    &contacts,
                    self.x_range,
                    self.y_range,
                    self.config.edge_threshold,
                    self.config.edge_slide_threshold,
                ) {
                    self.execute_edge_action(action);
                }
            }

            TimerAction::None
        } else {
            // No fingers — reset position tracking but keep velocity for momentum decay
            TOUCHPAD_SCROLLING.store(false, Ordering::Relaxed);
            self.swipe_detector.reset();
            self.two_finger_swipe.reset();
            self.edge_slide.reset();
            self.pinch_detector.reset();
            self.tap_detector.reset();
            self.three_finger_tap.reset();
            TimerAction::None
        };

        self.last_contact_time = now;
        if self.current_device_id != device_id {
            self.current_device_id = device_id.to_string();
        }
        self.old_contacts = contacts;

        timer_action
    }

    /// Called when the drag-end timer fires.
    pub fn on_timer_fired(&mut self) {
        self.drag
            .on_timer_fired(&self.config, &mut self.mouse);
    }

    /// Update configuration (called from UI thread).
    pub fn update_config(&mut self, config: Config) {
        self.config = config;
    }

    /// Get a reference to the current config.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Execute the action mapped to a 4-finger swipe direction.
    fn execute_swipe_action(&self, direction: SwipeDirection) {
        let action = match direction {
            SwipeDirection::Up => self.config.four_finger_swipe_up,
            SwipeDirection::Down => self.config.four_finger_swipe_down,
            SwipeDirection::Left => self.config.four_finger_swipe_left,
            SwipeDirection::Right => self.config.four_finger_swipe_right,
        };
        execute_action(action);
    }

    /// Execute the action mapped to a 2-finger swipe direction.


    /// Execute an edge slide action with direction inversion support.
    fn execute_edge_action(&self, action: crate::gesture::two_finger::EdgeSlideAction) {
        use crate::gesture::two_finger::EdgeSlideAction;
        match action {
            EdgeSlideAction::VolumeUp => {
                if self.config.invert_volume { keyboard::volume_down() }
                else { keyboard::volume_up() }
            }
            EdgeSlideAction::VolumeDown => {
                if self.config.invert_volume { keyboard::volume_up() }
                else { keyboard::volume_down() }
            }
            EdgeSlideAction::BrightnessUp => {
                if self.config.invert_brightness { keyboard::brightness_down() }
                else { keyboard::brightness_up() }
            }
            EdgeSlideAction::BrightnessDown => {
                if self.config.invert_brightness { keyboard::brightness_up() }
                else { keyboard::brightness_down() }
            }
        }
    }

    /// Execute the action mapped to a pinch direction.
    fn execute_pinch_action(&self, direction: PinchDirection) {
        let action = match direction {
            PinchDirection::Spread => self.config.four_finger_spread,
            PinchDirection::Pinch => self.config.four_finger_pinch,
        };
        execute_action(action);
    }
}

/// Execute a gesture action by sending the corresponding keyboard shortcut.
fn execute_action(action: GestureAction) {
    match action {
        GestureAction::None => {}
        GestureAction::WinTab => keyboard::win_tab(),
        GestureAction::AltTab => keyboard::alt_tab(),
        GestureAction::CtrlWinLeft => keyboard::ctrl_win_left(),
        GestureAction::CtrlWinRight => keyboard::ctrl_win_right(),
        GestureAction::ShowDesktop => keyboard::show_desktop(),
        GestureAction::OpenStart => keyboard::open_start(),
        GestureAction::Search => keyboard::search(),
        GestureAction::BrowserBack => {
            // Alt+Left = browser back
            keyboard::send_key_combo(&[
                windows::Win32::UI::Input::KeyboardAndMouse::VK_LMENU,
                windows::Win32::UI::Input::KeyboardAndMouse::VK_LEFT,
            ]);
        }
        GestureAction::BrowserForward => {
            // Alt+Right = browser forward
            keyboard::send_key_combo(&[
                windows::Win32::UI::Input::KeyboardAndMouse::VK_LMENU,
                windows::Win32::UI::Input::KeyboardAndMouse::VK_RIGHT,
            ]);
        }
        GestureAction::NotificationCenter => keyboard::notification_center(),
        GestureAction::VolumeUp => keyboard::volume_up(),
        GestureAction::VolumeDown => keyboard::volume_down(),
        GestureAction::BrightnessUp => keyboard::brightness_up(),
        GestureAction::BrightnessDown => keyboard::brightness_down(),
        GestureAction::PageUp => keyboard::page_up(),
        GestureAction::PageDown => keyboard::page_down(),
    }
}

/// Current time in milliseconds since UNIX epoch.
fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

