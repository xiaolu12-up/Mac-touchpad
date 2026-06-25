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
    alt_tab_active: bool,
    alt_tab_base_centroid: (f32, f32),
    four_finger_action_triggered: bool,
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
            alt_tab_active: false,
            alt_tab_base_centroid: (0.0, 0.0),
            four_finger_action_triggered: false,
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
        mut contacts: Vec<TouchpadContact>,
    ) -> TimerAction {
        self.check_timeouts();
        let now = current_time_ms();
        let elapsed = if self.last_contact_time > 0 {
            now - self.last_contact_time
        } else {
            0
        };

        // Normalize coordinates to standard (0, 4096) range
        if self.x_range.1 > self.x_range.0 && self.y_range.1 > self.y_range.0 {
            let x_span = (self.x_range.1 - self.x_range.0) as f32;
            let y_span = (self.y_range.1 - self.y_range.0) as f32;
            for c in &mut contacts {
                c.x = (((c.x - self.x_range.0) as f32 / x_span) * 4096.0) as i32;
                c.y = (((c.y - self.y_range.0) as f32 / y_span) * 4096.0) as i32;
            }
        }

        // Swap contacts without cloning — reuse the old buffer
        let old = std::mem::take(&mut self.old_contacts);
        let finger_count = contacts.len();

        // Release Alt key if finger count changes from 4
        if finger_count != 4 && self.alt_tab_active {
            tracing::info!("Alt-Tab release (finger count changed to {})", finger_count);
            keyboard::key_up(windows::Win32::UI::Input::KeyboardAndMouse::VK_LMENU);
            self.alt_tab_active = false;
        }

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
            // Let 3-finger tap detector see finger count change → cancel
            self.three_finger_tap.feed(
                &contacts,
                now,
                self.config.tap_max_duration_ms,
                self.config.tap_max_distance,
            );

            if self.alt_tab_active {
                let centroid = compute_centroid(&contacts);
                let dx = centroid.0 - self.alt_tab_base_centroid.0;
                let dy = centroid.1 - self.alt_tab_base_centroid.1;
                let distance = (dx * dx + dy * dy).sqrt();
                let nav_threshold = 120.0;
                if distance >= nav_threshold {
                    if dx.abs() > dy.abs() {
                        if dx > 0.0 {
                            tracing::info!("Alt-Tab navigate: Right");
                            keyboard::send_key_combo(&[windows::Win32::UI::Input::KeyboardAndMouse::VK_RIGHT]);
                        } else {
                            tracing::info!("Alt-Tab navigate: Left");
                            keyboard::send_key_combo(&[windows::Win32::UI::Input::KeyboardAndMouse::VK_LEFT]);
                        }
                    } else {
                        if dy > 0.0 {
                            tracing::info!("Alt-Tab navigate: Down");
                            keyboard::send_key_combo(&[windows::Win32::UI::Input::KeyboardAndMouse::VK_DOWN]);
                        } else {
                            tracing::info!("Alt-Tab navigate: Up");
                            keyboard::send_key_combo(&[windows::Win32::UI::Input::KeyboardAndMouse::VK_UP]);
                        }
                    }
                    self.alt_tab_base_centroid = centroid;
                }
            } else if !self.four_finger_action_triggered {
                let pinch_delta = self.pinch_detector.current_delta(&contacts).unwrap_or(0.0);
                // Suppress swipe detector if fingers are actively spreading or pinching (delta is large)
                let is_pinching_or_spreading = pinch_delta.abs() >= self.config.pinch_spread_threshold * 0.4;

                let swipe_detected = if !is_pinching_or_spreading {
                    self.swipe_detector.feed(&contacts, self.config.swipe_threshold)
                } else {
                    None
                };

                if let Some(direction) = swipe_detected {
                    let action = match direction {
                        SwipeDirection::Up => self.config.four_finger_swipe_up,
                        SwipeDirection::Down => self.config.four_finger_swipe_down,
                        SwipeDirection::Left => self.config.four_finger_swipe_left,
                        SwipeDirection::Right => self.config.four_finger_swipe_right,
                    };

                    if action == GestureAction::AltTab && (direction == SwipeDirection::Left || direction == SwipeDirection::Right) {
                        tracing::info!("Alt-Tab triggered by 4F swipe {:?}", direction);
                        self.alt_tab_active = true;
                        self.alt_tab_base_centroid = compute_centroid(&contacts);
                        keyboard::key_down(windows::Win32::UI::Input::KeyboardAndMouse::VK_LMENU);
                        keyboard::send_key_combo(&[windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB]);
                        
                        // Send initial navigation if swiping Left
                        if direction == SwipeDirection::Left {
                            keyboard::send_key_combo(&[windows::Win32::UI::Input::KeyboardAndMouse::VK_LEFT]);
                        }
                    } else {
                        self.execute_swipe_action(direction);
                    }
                    self.four_finger_action_triggered = true;
                }

                // Feed pinch/spread detector (only if Alt-Tab and swipe was not triggered)
                if !self.alt_tab_active && !self.four_finger_action_triggered {
                    if let Some(direction) =
                        self.pinch_detector.feed(&contacts, self.config.pinch_spread_threshold)
                    {
                        self.execute_pinch_action(direction);
                        self.four_finger_action_triggered = true;
                    }
                }
            }

            TimerAction::None
        } else if finger_count == 3 {
            // 3-finger: drag + tap detection
            self.two_finger_swipe.reset();
            self.edge_slide.reset();

            if self.four_finger_action_triggered {
                TimerAction::None
            } else {
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

                // Always feed 3-finger tap detector so it tracks the full lifecycle
                if self.config.three_finger_tap_enabled {
                    self.three_finger_tap.feed(
                        &contacts,
                        now,
                        self.config.tap_max_duration_ms,
                        self.config.tap_max_distance,
                    );
                }
                // Drag active → cancel tap tracking to avoid false positives
                if self.drag.is_dragging() {
                    self.three_finger_tap.reset();
                }

                action
            }
        } else if finger_count == 2 {
            // 2 fingers — native scroll
            if self.drag.is_dragging() {
                self.drag.force_stop(&self.config, &mut self.mouse);
            }
            self.edge_slide.reset();
            self.swipe_detector.reset();
            self.pinch_detector.reset();

            // NOTE: Do NOT feed three_finger_tap here.
            // When lifting 3 fingers, hardware reports 3→2→1→0 frames.
            // Feeding intermediate frames (2 fingers) would cancel the tap
            // due to finger count mismatch in the TapDetector.

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

            // NOTE: Do NOT feed three_finger_tap here.
            // When lifting 3 fingers, hardware reports 3→2→1→0 frames.
            // Feeding intermediate frames (1 finger) would cancel the tap
            // due to finger count mismatch in the TapDetector.

            if self.config.edge_slide_enabled && !self.four_finger_action_triggered {
                if let Some(action) = self.edge_slide.feed(
                    &contacts,
                    (0, 4096),
                    (0, 4096),
                    self.config.edge_threshold,
                    self.config.edge_slide_threshold,
                ) {
                    tracing::info!("Edge slide action: {:?}", action);
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

            // Feed empty contacts — tap detector fires if it saw a valid 3-finger tap
            let empty: [TouchpadContact; 0] = [];
            if !self.four_finger_action_triggered
                && self.config.three_finger_tap_enabled
                && self.three_finger_tap.feed(
                    &empty,
                    now,
                    self.config.tap_max_duration_ms,
                    self.config.tap_max_distance,
                )
            {
                tracing::info!("Three-finger tap detected → Win+S");
                keyboard::search();
            }
            self.drag.force_stop(&self.config, &mut self.mouse);

            self.four_finger_action_triggered = false;

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

    /// Check for timeouts (e.g. lost release reports).
    /// Called periodically from the main window message loop.
    pub fn check_timeouts(&mut self) {
        if self.last_contact_time > 0 {
            let now = current_time_ms();
            let elapsed = now - self.last_contact_time;

            // If silent for > 80ms, check if we were in the middle of a 3-finger tap
            if elapsed > 80 && self.three_finger_tap.is_touching() {
                let tap_detected = self.three_finger_tap.feed(
                    &[],
                    self.last_contact_time,
                    self.config.tap_max_duration_ms,
                    self.config.tap_max_distance,
                );
                if tap_detected {
                    tracing::info!("Three-finger tap detected via timeout recovery → Win+S");
                    keyboard::search();
                }
            }

            if elapsed > 250 {
                let mut reset_needed = false;
                if self.alt_tab_active {
                    tracing::info!("Alt-Tab timeout recovery release (no contact for {}ms)", elapsed);
                    keyboard::key_up(windows::Win32::UI::Input::KeyboardAndMouse::VK_LMENU);
                    self.alt_tab_active = false;
                    reset_needed = true;
                }
                if self.four_finger_action_triggered {
                    self.four_finger_action_triggered = false;
                    reset_needed = true;
                }
                if reset_needed {
                    self.swipe_detector.reset();
                    self.pinch_detector.reset();
                    self.two_finger_swipe.reset();
                    self.edge_slide.reset();
                    self.tap_detector.reset();
                    self.three_finger_tap.reset();
                }
            }
        }
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
        GestureAction::Maximize => keyboard::maximize(),
    }
}

/// Current time in milliseconds since UNIX epoch.
fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn compute_centroid(contacts: &[TouchpadContact]) -> (f32, f32) {
    let n = contacts.len() as f32;
    let sum = contacts
        .iter()
        .fold((0.0f32, 0.0f32), |acc, c| (acc.0 + c.x as f32, acc.1 + c.y as f32));
    (sum.0 / n, sum.1 / n)
}


