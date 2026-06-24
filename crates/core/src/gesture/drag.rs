use crate::config::Config;
use crate::gesture::distance::{DistanceManager, RELEASE_FINGERS_THRESHOLD_MS};
use crate::gesture::finger::FingerCounter;
use crate::hid::types::TouchpadContact;
use crate::input::mouse::MouseSimulator;
use crate::speed;

/// Three-finger drag state machine.
///
/// Ported from ThreeFingerDrag.cs. Handles start/move/stop of a drag operation
/// when exactly 3 fingers move on the touchpad.
pub struct ThreeFingerDrag {
    is_dragging: bool,
    distance_manager: DistanceManager,
    finger_counter: FingerCounter,
    // Cursor averaging state
    averaging_x: f32,
    averaging_y: f32,
    averaging_count: u32,
}

impl ThreeFingerDrag {
    pub fn new() -> Self {
        Self {
            is_dragging: false,
            distance_manager: DistanceManager::new(),
            finger_counter: FingerCounter::new(),
            averaging_x: 0.0,
            averaging_y: 0.0,
            averaging_count: 0,
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    /// Process a new frame of touchpad contacts.
    ///
    /// Returns Some(release_delay_ms) if the drag-end timer should be (re)started,
    /// or None if no timer action is needed.
    pub fn on_touchpad_contact(
        &mut self,
        device_id: &str,
        config: &Config,
        mouse: &mut MouseSimulator,
        old_contacts: &[TouchpadContact],
        contacts: &[TouchpadContact],
        elapsed: i64,
    ) -> TimerAction {
        let has_fingers_released = elapsed > RELEASE_FINGERS_THRESHOLD_MS;
        let are_contacts_ids_common =
            FingerCounter::are_contacts_ids_common(old_contacts, contacts);

        let (_longest_id, longest_dist_delta, longest_dist_2d) = self
            .distance_manager
            .get_longest_dist_2d(old_contacts, contacts, has_fingers_released);

        let (fingers_count, short_delay_moving, long_delay_moving, original_count) = self
            .finger_counter
            .count_moving_fingers(
                device_id,
                config,
                contacts,
                are_contacts_ids_common,
                longest_dist_2d,
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
            mouse.drag_down(config.drag_button);
            tracing::debug!("START DRAG, click down");
        }
        // STOP drag
        else if self.is_dragging
            && (short_delay_moving < 2 || (original_count != 3 && original_count >= 2))
        {
            tracing::debug!("STOP DRAG, click up");
            self.stop_drag(config, mouse);
            return TimerAction::StopTimer;
        }
        // MOVE while dragging
        else if fingers_count >= 2
            && original_count == 3
            && are_contacts_ids_common
            && self.is_dragging
        {
            let device_config = config.device_config(device_id);
            if device_config.cursor_move {
                // Discard large jumps if configured
                if config.max_finger_move_distance != 0
                    && longest_dist_2d > config.max_finger_move_distance as f32
                {
                    tracing::debug!(
                        "DISCARDING MOVE, (x, y) = ({}, {})",
                        longest_dist_delta.0,
                        longest_dist_delta.1
                    );
                } else if longest_dist_delta.0 != 0.0 || longest_dist_delta.1 != 0.0 {
                    let delta = speed::apply_speed_and_accel(
                        device_id,
                        config,
                        longest_dist_delta,
                        elapsed,
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

            // Restart drag-end timer
            let delay = self.get_release_delay(config);
            return TimerAction::StartTimer(delay);
        }

        TimerAction::None
    }

    /// Called when the drag-end timer fires (no input for release_delay_ms).
    pub fn on_timer_fired(&mut self, config: &Config, mouse: &mut MouseSimulator) {
        if self.is_dragging {
            tracing::debug!("STOP DRAG FROM TIMER, click up");
            self.stop_drag(config, mouse);
        }
    }

    /// Force stop the drag (e.g., when switching to a 4-finger gesture).
    pub fn force_stop(&mut self, config: &Config, mouse: &mut MouseSimulator) {
        if self.is_dragging {
            self.stop_drag(config, mouse);
        }
    }

    /// Reset all internal state.
    pub fn reset(&mut self) {
        self.is_dragging = false;
        self.distance_manager.reset();
        self.averaging_x = 0.0;
        self.averaging_y = 0.0;
        self.averaging_count = 0;
    }

    fn stop_drag(&mut self, config: &Config, mouse: &mut MouseSimulator) {
        self.is_dragging = false;
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
