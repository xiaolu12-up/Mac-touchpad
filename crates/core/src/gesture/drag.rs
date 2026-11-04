use crate::config::Config;
use crate::gesture::distance::{
    DistanceManager, RELEASE_FINGERS_THRESHOLD_MS, STATIONARY_HOLD_MAX_MS,
};
use crate::gesture::finger::FingerCounter;
use crate::hid::types::TouchpadContact;
use crate::input::mouse::MouseSimulator;
use crate::speed::{self, DynamicAccelTracker};

/// Below this consensus distance (in 0..4096 normalized units), movement is treated as stationary noise.
const STATIONARY_ENTER_DIST: f32 = 1.35;

/// Hysteresis breakout threshold required to leave stationary state.
const STATIONARY_EXIT_DIST: f32 = 1.95;

/// Three-finger drag state machine.
///
/// Handles start/move/stop of a drag operation when 3 fingers move on the touchpad,
/// with dynamic velocity acceleration, horizontal trajectory stabilization,
/// and stationary/lift-off anti-jump filtering.
pub struct ThreeFingerDrag {
    is_dragging: bool,
    distance_manager: DistanceManager,
    finger_counter: FingerCounter,
    accel_tracker: DynamicAccelTracker,
    /// Physical touchpad aspect ratio (X span / Y span), default ~1.5.
    aspect_ratio: f32,
    // Cursor averaging state
    averaging_x: f32,
    averaging_y: f32,
    averaging_count: u32,
    // Stationary & anti-jump state
    is_stationary: bool,
    stationary_frames: u32,
    time_since_last_move_ms: i64,
    last_move_dir: (f32, f32),
    fingers_currently_down: usize,
    /// Anchor position when 1 or 2 fingers touch during release-and-restart window.
    cancel_anchor: Option<(i32, i32, i32)>,
}

impl ThreeFingerDrag {
    pub fn new() -> Self {
        Self {
            is_dragging: false,
            distance_manager: DistanceManager::new(),
            finger_counter: FingerCounter::new(),
            accel_tracker: DynamicAccelTracker::new(),
            aspect_ratio: 1.5,
            averaging_x: 0.0,
            averaging_y: 0.0,
            averaging_count: 0,
            is_stationary: true,
            stationary_frames: 0,
            time_since_last_move_ms: 0,
            last_move_dir: (0.0, 0.0),
            fingers_currently_down: 0,
            cancel_anchor: None,
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    pub fn fingers_currently_down(&self) -> usize {
        self.fingers_currently_down
    }

    /// Update the touchpad's physical aspect ratio (`x_span / y_span`).
    pub fn set_aspect_ratio(&mut self, aspect_ratio: f32) {
        if aspect_ratio.is_finite() && aspect_ratio > 0.2 {
            self.aspect_ratio = aspect_ratio.clamp(1.0, 2.0);
        }
    }

    /// Process a new frame of 3-finger touchpad contacts.
    pub fn on_touchpad_contact(
        &mut self,
        device_id: &str,
        config: &Config,
        mouse: &mut MouseSimulator,
        old_contacts: &[TouchpadContact],
        contacts: &[TouchpadContact],
        elapsed: i64,
    ) -> TimerAction {
        self.fingers_currently_down = contacts.len();
        self.cancel_anchor = None;

        let mut are_contacts_ids_common =
            FingerCounter::are_contacts_ids_common(old_contacts, contacts);

        // If the exact same 3 finger contact IDs remain on the touchpad, holding them
        // stationary (elapsed > 55ms up to STATIONARY_HOLD_MAX_MS) is NOT a finger release.
        let mut has_fingers_released = if are_contacts_ids_common && !contacts.is_empty() {
            elapsed > STATIONARY_HOLD_MAX_MS
        } else {
            elapsed > RELEASE_FINGERS_THRESHOLD_MS || old_contacts.is_empty()
        };

        // Fingers put back on the pad while the release timer still holds the drag:
        // continue the existing drag instead of restarting it. Without this the
        // counters are reset by the empty previous frame and the cursor only resumes
        // after the user moves past the start threshold again.
        if self.is_dragging && contacts.len() == 3 && !are_contacts_ids_common {
            are_contacts_ids_common = true;
            has_fingers_released = false;
            self.finger_counter
                .restore_continuation(contacts.len() as i32);
            self.distance_manager.trust_contacts(contacts);
            tracing::debug!("RESUME DRAG, click still down");
        }

        let consensus = self.distance_manager.get_consensus_dist_2d(
            old_contacts,
            contacts,
            has_fingers_released,
        );

        let (fingers_count, short_delay_moving, long_delay_moving, original_count) = self
            .finger_counter
            .count_moving_fingers(
                device_id,
                config,
                contacts,
                are_contacts_ids_common,
                consensus.activity_dist,
                has_fingers_released,
            );

        // START drag
        if fingers_count >= 3
            && are_contacts_ids_common
            && long_delay_moving == 3
            && original_count == 3
            && !self.is_dragging
        {
            self.is_dragging = true;
            self.is_stationary = false;
            self.stationary_frames = 0;
            self.time_since_last_move_ms = 0;
            self.last_move_dir = (0.0, 0.0);
            self.averaging_x = 0.0;
            self.averaging_y = 0.0;
            self.averaging_count = 0;
            self.accel_tracker.reset();
            mouse.drag_down(config.drag_button);
            tracing::debug!("START DRAG, click down");
            return self.arm_release_timer(config);
        }
        // STOP drag
        else if self.is_dragging
            && (short_delay_moving < 2 || (original_count != 3 && original_count >= 2))
        {
            tracing::debug!("STOP DRAG, click up");
            self.stop_drag(config, mouse);
            return TimerAction::StopTimer;
        }
        // MOVE or HOLD while dragging (strictly require 3 fingers for cursor movement to prevent lift-off jumps)
        else if fingers_count == 3
            && original_count == 3
            && are_contacts_ids_common
            && self.is_dragging
        {
            let device_config = config.device_config(device_id);
            if device_config.cursor_move {
                let mut raw_delta = consensus.delta;
                let dist = consensus.dist_2d;

                // Discard abnormal hardware teleport spikes if configured or exceeding safe bound
                let max_jump = if config.max_finger_move_distance != 0 {
                    config.max_finger_move_distance as f32
                } else {
                    450.0
                };

                if dist > max_jump {
                    tracing::debug!(
                        "DISCARDING TELEPORT MOVE, (x, y) = ({}, {})",
                        raw_delta.0,
                        raw_delta.1
                    );
                } else if dist < STATIONARY_ENTER_DIST || consensus.moving_trusted_count < 2 {
                    // Fingers are stationary on the touchpad: lock cursor and clear sub-pixel residue
                    self.stationary_frames = self.stationary_frames.saturating_add(1);
                    self.time_since_last_move_ms =
                        self.time_since_last_move_ms.saturating_add(elapsed.max(1));
                    if self.stationary_frames >= 2 || self.time_since_last_move_ms >= 18 {
                        self.is_stationary = true;
                    }
                    self.accel_tracker.on_stationary();
                    mouse.clear_subpixel();
                    self.averaging_x = 0.0;
                    self.averaging_y = 0.0;
                    self.averaging_count = 0;
                } else {
                    // Check stationary breakout hysteresis & stop anti-recoil filter
                    let mut should_move = true;

                    if self.is_stationary {
                        if dist < STATIONARY_EXIT_DIST
                            || consensus.moving_trusted_count < 2
                            || consensus.coherence < 0.55
                        {
                            should_move = false;
                            mouse.clear_subpixel();
                        } else {
                            // Soft-knee breakout so leaving stationary state starts smoothly from ~0px
                            self.is_stationary = false;
                            self.stationary_frames = 0;
                            let soft_scale = ((dist - STATIONARY_ENTER_DIST) / dist).clamp(0.1, 1.0);
                            raw_delta.0 *= soft_scale;
                            raw_delta.1 *= soft_scale;
                        }
                    } else if self.time_since_last_move_ms <= 45 && dist < 6.5 {
                        // Anti-recoil filter: when stopping a drag, skin elasticity can rebound
                        // briefly in the opposite direction.
                        let (lx, ly) = self.last_move_dir;
                        if lx != 0.0 || ly != 0.0 {
                            let nx = raw_delta.0 / dist;
                            let ny = raw_delta.1 / dist;
                            let dot = nx * lx + ny * ly;
                            if dot < -0.25 {
                                should_move = false;
                                self.is_stationary = true;
                                self.accel_tracker.on_stationary();
                                mouse.clear_subpixel();
                            }
                        }
                    }

                    if should_move {
                        self.stationary_frames = 0;
                        self.time_since_last_move_ms = 0;
                        let nx = raw_delta.0 / dist;
                        let ny = raw_delta.1 / dist;
                        if self.last_move_dir == (0.0, 0.0) {
                            self.last_move_dir = (nx, ny);
                        } else {
                            self.last_move_dir = (
                                0.6 * self.last_move_dir.0 + 0.4 * nx,
                                0.6 * self.last_move_dir.1 + 0.4 * ny,
                            );
                        }

                        let delta = self.accel_tracker.process_delta(
                            device_id,
                            config,
                            raw_delta,
                            elapsed,
                            self.aspect_ratio,
                        );

                        if config.cursor_averaging > 1 {
                            self.averaging_x += delta.0;
                            self.averaging_y += delta.1;
                            self.averaging_count += 1;
                            if self.averaging_count >= config.cursor_averaging {
                                mouse.shift_cursor_position(self.averaging_x, self.averaging_y);
                                self.averaging_x = 0.0;
                                self.averaging_y = 0.0;
                                self.averaging_count = 0;
                            }
                        } else {
                            mouse.shift_cursor_position(delta.0, delta.1);
                        }
                    }
                }
            }

            // Keep drag-end timer alive whenever 3 fingers are on the touchpad during drag
            return self.arm_release_timer(config);
        } else if self.is_dragging && contacts.len() == 3 {
            // 3 fingers just touched back down during release-and-restart window:
            // keep timer running while quarantine/threshold promotes them.
            return self.arm_release_timer(config);
        }

        let _ = speed::apply_speed(device_id, config, 0.0);
        TimerAction::None
    }

    /// Handle non-3-finger frames (`0`, `1`, or `2` fingers) while a drag may be active.
    ///
    /// Prevents cursor jumps during finger lift-off (`3 -> 2 -> 1 -> 0`) while supporting
    /// `allow_release_and_restart` and immediately canceling if the user starts a 1-finger move
    /// or 2-finger scroll.
    pub fn on_non_three_finger_contact(
        &mut self,
        config: &Config,
        mouse: &mut MouseSimulator,
        contacts: &[TouchpadContact],
    ) -> TimerAction {
        self.fingers_currently_down = contacts.len();

        if !self.is_dragging {
            self.cancel_anchor = None;
            return TimerAction::None;
        }

        // Freeze cursor movement state immediately when finger count leaves 3 (prevents lift-off jump)
        self.distance_manager.reset();
        self.accel_tracker.reset();
        mouse.clear_subpixel();
        self.averaging_x = 0.0;
        self.averaging_y = 0.0;
        self.averaging_count = 0;
        self.is_stationary = true;
        self.finger_counter.on_released_for_restart();

        if !config.allow_release_and_restart {
            // When release-and-restart is disabled, stop drag as soon as fingers lift (0 or 1 finger)
            if contacts.len() <= 1 {
                self.stop_drag(config, mouse);
                return TimerAction::StopTimer;
            }
            // 2 fingers keep the drag pressed without a timer: a timeout would
            // release the button between reports, and the re-press at the same
            // spot is seen by Windows as a click (a second pair opens the item).
            return TimerAction::None;
        }

        // When release-and-restart is enabled:
        // If 1 or 2 fingers remain on the pad and actively move past a small cancel threshold,
        // the user is starting a 1-finger pointer move or 2-finger scroll → release drag immediately.
        if let Some(first) = contacts.first() {
            match self.cancel_anchor {
                Some((id, ax, ay)) if id == first.contact_id => {
                    let dx = (first.x - ax) as f32;
                    let dy = (first.y - ay) as f32;
                    let moved = (dx * dx + dy * dy).sqrt();
                    if moved > 24.0 {
                        tracing::debug!("STOP DRAG: {}-finger movement detected after release", contacts.len());
                        self.stop_drag(config, mouse);
                        return TimerAction::StopTimer;
                    }
                }
                _ => {
                    self.cancel_anchor = Some((first.contact_id, first.x, first.y));
                }
            }
        } else {
            self.cancel_anchor = None;
        }

        let delay = self.get_release_delay(config);
        TimerAction::StartTimer(delay)
    }

    /// Called when the drag-end timer fires (no input for release_delay_ms).
    pub fn on_timer_fired(&mut self, config: &Config, mouse: &mut MouseSimulator) {
        if self.is_dragging {
            // Do NOT drop the drag if all 3 fingers are still resting stationary on the touchpad
            if self.fingers_currently_down == 3 {
                tracing::debug!("TIMER FIRED while 3 fingers stationary on pad — holding drag");
                return;
            }
            tracing::debug!("STOP DRAG FROM TIMER, click up");
            self.stop_drag(config, mouse);
        }
    }

    /// Force stop the drag (e.g., when switching to a 4-finger gesture).
    pub fn force_stop(&mut self, config: &Config, mouse: &mut MouseSimulator) {
        if self.is_dragging {
            self.stop_drag(config, mouse);
        } else {
            self.reset();
        }
    }

    /// Reset all internal state.
    pub fn reset(&mut self) {
        self.is_dragging = false;
        self.distance_manager.reset();
        self.finger_counter.reset();
        self.accel_tracker.reset();
        self.averaging_x = 0.0;
        self.averaging_y = 0.0;
        self.averaging_count = 0;
        self.is_stationary = true;
        self.stationary_frames = 0;
        self.time_since_last_move_ms = 0;
        self.last_move_dir = (0.0, 0.0);
        self.fingers_currently_down = 0;
        self.cancel_anchor = None;
    }

    fn stop_drag(&mut self, config: &Config, mouse: &mut MouseSimulator) {
        self.reset();
        mouse.drag_up(config.drag_button);
    }

    /// Arm the drag-end timer. With release-and-restart disabled the drag ends on
    /// a real contact loss, so a timer must not run while the fingers stay down:
    /// it would release the button on any gap between reports, and the immediate
    /// re-press at the same spot becomes a click (a second pair opens the item).
    fn arm_release_timer(&self, config: &Config) -> TimerAction {
        if config.allow_release_and_restart {
            TimerAction::StartTimer(self.get_release_delay(config))
        } else {
            TimerAction::None
        }
    }

    fn get_release_delay(&self, config: &Config) -> u32 {
        if config.allow_release_and_restart {
            config.release_delay_ms.max(RELEASE_FINGERS_THRESHOLD_MS as u32)
        } else {
            RELEASE_FINGERS_THRESHOLD_MS as u32
        }
    }
}

/// Actions the caller should take regarding the drag-end timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerAction {
    /// No timer action needed.
    None,
    /// Start/restart the drag-end timer with the given delay in ms.
    StartTimer(u32),
    /// Stop the drag-end timer.
    StopTimer,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeviceDragConfig, DragButton};

    fn contacts3(x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32) -> Vec<TouchpadContact> {
        vec![
            TouchpadContact::new(1, x1, y1),
            TouchpadContact::new(2, x2, y2),
            TouchpadContact::new(3, x3, y3),
        ]
    }

    #[test]
    fn test_stationary_hold_does_not_drop_drag_on_timer() {
        let mut config = Config::default();
        config.drag_button = DragButton::None; // Avoid sending real OS mouse events in unit test
        config.start_threshold = 20.0;
        let mut mouse = MouseSimulator::new();
        let mut drag = ThreeFingerDrag::new();

        let c0 = contacts3(1000, 1000, 1500, 1000, 2000, 1000);
        let c1 = contacts3(1010, 1000, 1510, 1000, 2010, 1000);
        let c2 = contacts3(1060, 1000, 1560, 1000, 2060, 1000);

        let _ = drag.on_touchpad_contact("default", &config, &mut mouse, &[], &c0, 10);
        let _ = drag.on_touchpad_contact("default", &config, &mut mouse, &c0, &c1, 10);
        let _ = drag.on_touchpad_contact("default", &config, &mut mouse, &c1, &c2, 10);
        assert!(drag.is_dragging(), "Drag should have started");

        // Firing timer while 3 fingers are still resting stationary on the touchpad must NOT drop drag
        drag.on_timer_fired(&config, &mut mouse);
        assert!(drag.is_dragging(), "Stationary 3-finger hold must not drop drag when timer fires");

        // Lifting fingers (0 contacts) and then firing timer should end drag
        let _ = drag.on_non_three_finger_contact(&config, &mut mouse, &[]);
        drag.on_timer_fired(&config, &mut mouse);
        assert!(!drag.is_dragging(), "Timer after finger release should end drag");
    }

    fn test_config(allow_release_and_restart: bool) -> Config {
        let mut config = Config::default();
        config.drag_button = DragButton::None;
        config.allow_release_and_restart = allow_release_and_restart;
        config.release_delay_ms = 500;
        config.start_threshold = 40.0;
        config.stop_threshold = 5.0;
        config.device_configs.insert(
            "default".into(),
            DeviceDragConfig {
                cursor_move: false,
                ..Default::default()
            },
        );
        config
    }

    fn contacts3_at(offset: i32) -> Vec<TouchpadContact> {
        contacts3(
            1000 + offset,
            1000,
            1500 + offset,
            1000,
            2000 + offset,
            1000,
        )
    }

    fn start_drag(drag: &mut ThreeFingerDrag, config: &Config, mouse: &mut MouseSimulator) {
        let mut old: Vec<TouchpadContact> = Vec::new();
        for i in 0..8 {
            let new = contacts3_at(i * 10);
            drag.on_touchpad_contact("default", config, mouse, &old, &new, 10);
            old = new;
            if i == 0 {
                // Let the new contacts leave the 12ms quarantine.
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        assert!(
            drag.is_dragging(),
            "drag should start once the start threshold is crossed"
        );
    }

    #[test]
    fn lift_keeps_drag_alive_until_timer_fires() {
        let config = test_config(true);
        let mut mouse = MouseSimulator::new();
        let mut drag = ThreeFingerDrag::new();
        start_drag(&mut drag, &config, &mut mouse);

        let lifted = drag.on_non_three_finger_contact(&config, &mut mouse, &[]);
        assert_eq!(lifted, TimerAction::StartTimer(500));
        assert!(drag.is_dragging());

        drag.on_timer_fired(&config, &mut mouse);
        assert!(!drag.is_dragging());
    }

    #[test]
    fn lift_stops_drag_immediately_when_restart_is_disabled() {
        let config = test_config(false);
        let mut mouse = MouseSimulator::new();
        let mut drag = ThreeFingerDrag::new();
        start_drag(&mut drag, &config, &mut mouse);

        let lifted = drag.on_non_three_finger_contact(&config, &mut mouse, &[]);
        assert_eq!(lifted, TimerAction::StopTimer);
        assert!(!drag.is_dragging());
    }

    #[test]
    fn disabled_restart_does_not_arm_timer_while_fingers_stay_down() {
        let config = test_config(false);
        let mut mouse = MouseSimulator::new();
        let mut drag = ThreeFingerDrag::new();
        start_drag(&mut drag, &config, &mut mouse);

        // A pause in reporting while the fingers are still down must not
        // release the button: the release/repress pair is seen by Windows as
        // a click, and a repeated pair opens the dragged item.
        let action = drag.on_touchpad_contact(
            "default",
            &config,
            &mut mouse,
            &contacts3_at(70),
            &contacts3_at(72),
            10,
        );
        assert_eq!(action, TimerAction::None);
        assert!(drag.is_dragging());

        // A momentary 2-finger report keeps the drag pressed without a timer.
        let two = vec![
            TouchpadContact::new(1, 1072, 1000),
            TouchpadContact::new(2, 1572, 1000),
        ];
        let action = drag.on_non_three_finger_contact(&config, &mut mouse, &two);
        assert_eq!(action, TimerAction::None);
        assert!(drag.is_dragging());

        // The drag only ends on a real contact loss.
        let one = vec![TouchpadContact::new(1, 1072, 1000)];
        let action = drag.on_non_three_finger_contact(&config, &mut mouse, &one);
        assert_eq!(action, TimerAction::StopTimer);
        assert!(!drag.is_dragging());
    }

    #[test]
    fn retouch_within_release_window_resumes_without_start_threshold() {
        let config = test_config(true);
        let mut mouse = MouseSimulator::new();
        let mut drag = ThreeFingerDrag::new();
        start_drag(&mut drag, &config, &mut mouse);

        let held = drag.on_non_three_finger_contact(&config, &mut mouse, &[]);
        assert_eq!(held, TimerAction::StartTimer(500));
        assert!(drag.is_dragging());

        // Fingers come back within the release window.
        let retouch = contacts3_at(80);
        let resumed =
            drag.on_touchpad_contact("default", &config, &mut mouse, &[], &retouch, 200);
        assert_eq!(resumed, TimerAction::StartTimer(500));
        assert!(drag.is_dragging());
        assert_eq!(drag.finger_counter.original_count(), 3);

        // A move well below the start threshold must already be a drag move.
        let continued = drag.on_touchpad_contact(
            "default",
            &config,
            &mut mouse,
            &retouch,
            &contacts3_at(82),
            10,
        );
        assert_eq!(continued, TimerAction::StartTimer(500));
        assert!(drag.is_dragging());
        assert_eq!(drag.finger_counter.original_count(), 3);
    }

    #[test]
    fn two_fingers_keep_drag_alive_when_restart_is_enabled() {
        let config = test_config(true);
        let mut mouse = MouseSimulator::new();
        let mut drag = ThreeFingerDrag::new();
        start_drag(&mut drag, &config, &mut mouse);

        let two = vec![
            TouchpadContact::new(1, 1070, 1000),
            TouchpadContact::new(2, 1570, 1000),
        ];
        let action = drag.on_non_three_finger_contact(&config, &mut mouse, &two);
        assert_eq!(action, TimerAction::StartTimer(500));
        assert!(drag.is_dragging());
    }
}