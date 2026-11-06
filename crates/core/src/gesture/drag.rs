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
    /// Set when remaining fingers intentionally continue the active drag.
    continuation_active: bool,
    /// Last remaining-finger contact used for continuation movement.
    continuation_last: Option<(i32, i32, i32)>,
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
            continuation_active: false,
            continuation_last: None,
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

        let are_contacts_ids_common =
            FingerCounter::are_contacts_ids_common(old_contacts, contacts);

        // If the exact same 3 finger contact IDs remain on the touchpad, holding them
        // stationary (elapsed > 55ms up to STATIONARY_HOLD_MAX_MS) is NOT a finger release.
        let has_fingers_released = if are_contacts_ids_common && !contacts.is_empty() {
            elapsed > STATIONARY_HOLD_MAX_MS
        } else {
            elapsed > RELEASE_FINGERS_THRESHOLD_MS || old_contacts.is_empty()
        };

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

            let delay = self.get_release_delay(config);
            return TimerAction::StartTimer(delay);
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
            let delay = self.get_release_delay(config);
            return TimerAction::StartTimer(delay);
        } else if self.is_dragging && contacts.len() == 3 {
            // 3 fingers just touched back down during release-and-restart window:
            // keep timer running while quarantine/threshold promotes them.
            let delay = self.get_release_delay(config);
            return TimerAction::StartTimer(delay);
        }

        let _ = speed::apply_speed(device_id, config, 0.0);
        TimerAction::None
    }

    /// Handle non-3-finger frames (`0`, `1`, or `2` fingers) while a drag may be active.
    ///
    /// After a three-finger drag, one remaining finger can intentionally take over and keep
    /// dragging. Short movement is ignored to absorb lift-off jitter; once the finger passes a
    /// small threshold, the active drag follows that finger until it is lifted or the timer ends.
    pub fn on_non_three_finger_contact(
        &mut self,
        device_id: &str,
        config: &Config,
        mouse: &mut MouseSimulator,
        contacts: &[TouchpadContact],
        elapsed_ms: i64,
    ) -> TimerAction {
        self.fingers_currently_down = contacts.len();

        if !self.is_dragging {
            self.cancel_anchor = None;
            self.continuation_active = false;
            self.continuation_last = None;
            return TimerAction::None;
        }

        let elapsed = elapsed_ms.max(1);
        let can_continue = config.allow_release_and_restart && !contacts.is_empty();

        if !can_continue {
            self.distance_manager.reset();
            self.accel_tracker.reset();
            mouse.clear_subpixel();
            self.averaging_x = 0.0;
            self.averaging_y = 0.0;
            self.averaging_count = 0;
            self.is_stationary = true;
            self.finger_counter.on_released_for_restart();
            self.cancel_anchor = None;
            self.continuation_active = false;
            self.continuation_last = None;

            if !config.allow_release_and_restart && contacts.len() <= 1 {
                self.stop_drag(config, mouse);
                return TimerAction::StopTimer;
            }

            let delay = self.get_release_delay(config);
            return TimerAction::StartTimer(delay);
        }

        let first = &contacts[0];
        match self.cancel_anchor {
            Some((id, anchor_x, anchor_y)) if id == first.contact_id => {
                let dx = (first.x - anchor_x) as f32;
                let dy = (first.y - anchor_y) as f32;
                let moved = (dx * dx + dy * dy).sqrt();

                if self.continuation_active {
                    if let Some((last_id, last_x, last_y)) = self.continuation_last {
                        if last_id == first.contact_id {
                            let raw_delta = (
                                (first.x - last_x) as f32,
                                (first.y - last_y) as f32,
                            );
                            if raw_delta.0 != 0.0 || raw_delta.1 != 0.0 {
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

                    self.continuation_last = Some((first.contact_id, first.x, first.y));
                    let delay = self.get_release_delay(config);
                    return TimerAction::StartTimer(delay);
                }

                // Ignore lift-off jitter, then let a deliberate remaining-finger movement take over.
                if moved > 24.0 {
                    self.accel_tracker.reset();
                    mouse.clear_subpixel();
                    self.averaging_x = 0.0;
                    self.averaging_y = 0.0;
                    self.averaging_count = 0;
                    self.is_stationary = true;
                    self.continuation_active = true;
                    self.continuation_last = Some((first.contact_id, first.x, first.y));
                    tracing::debug!("CONTINUE DRAG with {} finger", contacts.len());
                }
            }
            _ => {
                self.cancel_anchor = Some((first.contact_id, first.x, first.y));
                self.continuation_active = false;
                self.continuation_last = None;
            }
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
        self.continuation_active = false;
        self.continuation_last = None;
    }

    fn stop_drag(&mut self, config: &Config, mouse: &mut MouseSimulator) {
        self.reset();
        mouse.drag_up(config.drag_button);
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
    use crate::config::DragButton;

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
        let _ = drag.on_non_three_finger_contact("default", &config, &mut mouse, &[], 10);
        drag.on_timer_fired(&config, &mut mouse);
        assert!(!drag.is_dragging(), "Timer after finger release should end drag");
    }

    #[test]
    fn one_finger_can_continue_drag_after_three_finger_start() {
        let mut config = Config::default();
        config.drag_button = DragButton::None;
        config.allow_release_and_restart = true;
        config.start_threshold = 20.0;
        let mut mouse = MouseSimulator::new();
        let mut drag = ThreeFingerDrag::new();

        let c0 = contacts3(1000, 1000, 1500, 1000, 2000, 1000);
        let c1 = contacts3(1010, 1000, 1510, 1000, 2010, 1000);
        let c2 = contacts3(1060, 1000, 1560, 1000, 2060, 1000);
        let _ = drag.on_touchpad_contact("default", &config, &mut mouse, &[], &c0, 10);
        let _ = drag.on_touchpad_contact("default", &config, &mut mouse, &c0, &c1, 10);
        let _ = drag.on_touchpad_contact("default", &config, &mut mouse, &c1, &c2, 10);
        assert!(drag.is_dragging());

        // Small lift-off jitter must not cancel the drag.
        let jitter = vec![TouchpadContact::new(1, 1060, 1000)];
        let _ = drag.on_non_three_finger_contact("default", &config, &mut mouse, &jitter, 10);
        assert!(drag.is_dragging());

        // Deliberate one-finger movement takes over and keeps the drag alive.
        let moved = vec![TouchpadContact::new(1, 1090, 1000)];
        let action = drag.on_non_three_finger_contact("default", &config, &mut mouse, &moved, 10);
        assert!(drag.is_dragging());
        assert_eq!(action, TimerAction::StartTimer(config.release_delay_ms.max(15)));

        let moved_again = vec![TouchpadContact::new(1, 1130, 1000)];
        let action =
            drag.on_non_three_finger_contact("default", &config, &mut mouse, &moved_again, 10);
        assert!(drag.is_dragging());
        assert_eq!(action, TimerAction::StartTimer(config.release_delay_ms.max(15)));

        // Lifting the last finger still ends the drag when the timer fires.
        let _ = drag.on_non_three_finger_contact("default", &config, &mut mouse, &[], 10);
        drag.on_timer_fired(&config, &mut mouse);
        assert!(!drag.is_dragging());
    }
}
