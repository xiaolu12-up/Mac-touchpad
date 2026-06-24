use std::collections::{HashMap, HashSet};
use crate::hid::types::TouchpadContact;

/// Threshold in milliseconds before a finger is considered "released".
/// Windows Precision Touchpad sends contacts about every 10ms.
pub const RELEASE_FINGERS_THRESHOLD_MS: i64 = 15;

/// Tracks the distance moved by fingers across touchpad frames.
/// Uses a quarantine system to prevent newly-appearing fingers from
/// affecting distance calculations for RELEASE_FINGERS_THRESHOLD_MS.
pub struct DistanceManager {
    /// Contacts in quarantine (waiting to become trusted).
    /// Maps contact_id -> timestamp when added.
    quarantine_contacts: HashMap<i32, i64>,
    /// Contact IDs that have survived quarantine and can contribute to distance.
    trusted_contacts: HashSet<i32>,
}

impl DistanceManager {
    pub fn new() -> Self {
        Self {
            quarantine_contacts: HashMap::new(),
            trusted_contacts: HashSet::new(),
        }
    }

    /// Find the longest distance between two TouchpadContacts of the same ID.
    ///
    /// New contacts are quarantined for RELEASE_FINGERS_THRESHOLD_MS before
    /// they can affect the distance calculation.
    ///
    /// Returns (contact_id, (dx, dy), scalar_distance).
    /// Returns (0, (0, 0), 0.0) if fingers were released.
    pub fn get_longest_dist_2d(
        &mut self,
        old_contacts: &[TouchpadContact],
        new_contacts: &[TouchpadContact],
        has_fingers_released: bool,
    ) -> (i32, (f32, f32), f32) {
        if has_fingers_released {
            self.quarantine_contacts.clear();
            self.trusted_contacts.clear();
            return (0, (0.0, 0.0), 0.0);
        }

        let now = current_time_ms();

        // Remove contacts that don't exist anymore
        self.trusted_contacts
            .retain(|c| new_contacts.iter().any(|nc| nc.contact_id == *c));
        self.quarantine_contacts
            .retain(|c, _| new_contacts.iter().any(|nc| nc.contact_id == *c));

        // Quarantine system: promote or add new contacts
        for new_contact in new_contacts {
            if let Some(&contact_ctms) = self.quarantine_contacts.get(&new_contact.contact_id) {
                // Contact exists in quarantine — promote if enough time has passed
                if now - contact_ctms > RELEASE_FINGERS_THRESHOLD_MS {
                    self.trusted_contacts.insert(new_contact.contact_id);
                    self.quarantine_contacts.remove(&new_contact.contact_id);
                }
            } else if !self.trusted_contacts.contains(&new_contact.contact_id) {
                // Contact is brand new — add to quarantine
                self.quarantine_contacts.insert(new_contact.contact_id, now);
            }
        }

        // Find the longest distance among trusted contacts
        let mut longest_dist_2d: f32 = 0.0;
        let mut longest_dist_id = 0i32;
        let mut longest_dist_delta = (0.0f32, 0.0f32);

        for new_contact in new_contacts {
            if !self.trusted_contacts.contains(&new_contact.contact_id) {
                continue;
            }

            for old_contact in old_contacts {
                if new_contact.contact_id != old_contact.contact_id {
                    continue;
                }

                let dist_2d = new_contact.dist_2d(old_contact);
                if dist_2d > longest_dist_2d {
                    longest_dist_2d = dist_2d;
                    longest_dist_id = new_contact.contact_id;
                    longest_dist_delta = (
                        (new_contact.x - old_contact.x) as f32,
                        (new_contact.y - old_contact.y) as f32,
                    );
                }
                break;
            }
        }

        (longest_dist_id, longest_dist_delta, longest_dist_2d)
    }

    /// Reset all quarantine and trusted state.
    pub fn reset(&mut self) {
        self.quarantine_contacts.clear();
        self.trusted_contacts.clear();
    }
}

/// Current time in milliseconds since UNIX epoch.
fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
