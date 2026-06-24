use crate::hid::types::TouchpadContact;
use crate::speed;

/// Counts the number of fingers that are actively moving on the touchpad.
/// Uses dual-threshold accumulation:
/// - Short delay (stop threshold): determines if fingers are still active
/// - Long delay (start threshold): determines if user truly intends a drag
pub struct FingerCounter {
    /// Number of fingers originally on the touchpad when long-delay threshold was crossed.
    /// Reset when contacts list changes to <= 1 or fingers are released.
    original_fingers_count: i32,

    /// Number of fingers that have accumulated movement past the stop threshold.
    short_delay_fingers_count: i32,
    short_delay_fingers_move: f32,

    /// Number of fingers that have accumulated movement past the start threshold.
    long_delay_fingers_count: i32,
    long_delay_fingers_move: f32,
}

impl FingerCounter {
    pub fn new() -> Self {
        Self {
            original_fingers_count: 0,
            short_delay_fingers_count: 0,
            short_delay_fingers_move: 0.0,
            long_delay_fingers_count: 0,
            long_delay_fingers_move: 0.0,
        }
    }

    /// Count moving fingers based on distance thresholds.
    ///
    /// Returns (fingers_count, short_delay_moving, long_delay_moving, original_fingers_count).
    /// `fingers_count` is 0 if contacts changed (to signal the state machine).
    pub fn count_moving_fingers(
        &mut self,
        device_id: &str,
        config: &crate::config::Config,
        new_contacts: &[TouchpadContact],
        are_contacts_ids_common: bool,
        longest_dist_2d: f32,
        has_fingers_released: bool,
    ) -> (i32, i32, i32, i32) {
        // Reset original count when contacts change significantly
        if !are_contacts_ids_common && (new_contacts.len() <= 1 || has_fingers_released) {
            self.original_fingers_count = 0;
        }

        // Reset accumulators on contact change or finger release
        if !are_contacts_ids_common || has_fingers_released {
            self.short_delay_fingers_move = 0.0;
            self.long_delay_fingers_move = 0.0;
            return (
                0,
                self.short_delay_fingers_count,
                self.long_delay_fingers_count,
                self.original_fingers_count,
            );
        }

        // Apply speed to the distance (matching reference: ApplySpeed)
        let speed_dist = speed::apply_speed(device_id, config, longest_dist_2d);

        // Only accumulate meaningful distances
        if speed_dist >= 1.0 {
            self.short_delay_fingers_move += speed_dist;
            self.long_delay_fingers_move += speed_dist;
        }

        let fingers_count = new_contacts.len() as i32;

        // Short delay: past stop threshold -> fingers are active
        if self.short_delay_fingers_move >= config.stop_threshold {
            self.short_delay_fingers_count = fingers_count;
            self.short_delay_fingers_move = 0.0;
        }

        // Long delay: past start threshold -> user intends a gesture
        if self.long_delay_fingers_move > config.start_threshold {
            self.long_delay_fingers_count = fingers_count;
            self.long_delay_fingers_move = 0.0;
            if self.original_fingers_count <= 1 {
                self.original_fingers_count = fingers_count;
            }
        }

        (
            fingers_count,
            self.short_delay_fingers_count,
            self.long_delay_fingers_count,
            self.original_fingers_count,
        )
    }

    /// Check if old and new contact lists have the same set of contact IDs.
    pub fn are_contacts_ids_common(
        old_contacts: &[TouchpadContact],
        new_contacts: &[TouchpadContact],
    ) -> bool {
        if old_contacts.len() != new_contacts.len() {
            return false;
        }

        let count = new_contacts
            .iter()
            .filter(|nc| old_contacts.iter().any(|oc| oc.contact_id == nc.contact_id))
            .count();

        count == new_contacts.len()
    }
}
