use std::collections::{HashMap, HashSet};
use crate::hid::types::TouchpadContact;

/// Threshold in milliseconds before a contact gap with changed contact IDs
/// is considered a finger release.
pub const RELEASE_FINGERS_THRESHOLD_MS: i64 = 55;

/// Quarantine duration in milliseconds for brand-new contacts to prevent
/// initial touchdown contact-patch expansion from jumping the cursor.
pub const QUARANTINE_THRESHOLD_MS: i64 = 12;

/// Maximum time in milliseconds that identical contact IDs can remain silent
/// (held motionless on the touchpad) before state is reset.
pub const STATIONARY_HOLD_MAX_MS: i64 = 650;

/// Per-finger static noise floor in normalized (0..4096) touchpad coordinates.
const FINGER_STATIC_NOISE_FLOOR: f32 = 1.15;

/// Multi-finger consensus displacement result for a touchpad frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConsensusDistance {
    /// Outlier-rejected multi-finger consensus delta `(dx, dy)` for cursor movement.
    pub delta: (f32, f32),
    /// Magnitude of `delta`.
    pub dist_2d: f32,
    /// Representative movement distance for start/stop gesture threshold accumulation.
    pub activity_dist: f32,
    /// Number of trusted contacts that moved above the static noise floor.
    pub moving_trusted_count: usize,
    /// Total number of matched trusted contacts in this frame.
    pub trusted_count: usize,
    /// Directional coherence in `[0.0, 1.0]` (1.0 = all fingers moving in the exact same direction).
    pub coherence: f32,
}

/// Tracks the distance moved by fingers across touchpad frames.
/// Uses a quarantine system for newly-appearing contacts and a multi-finger
/// median + coherence consensus filter to eliminate stationary jitter and
/// single-finger stick-slip jumps.
pub struct DistanceManager {
    /// Contacts in quarantine: maps contact_id -> (timestamp_ms, frame_count).
    quarantine_contacts: HashMap<i32, (i64, u32)>,
    /// Contact IDs that have survived quarantine and contribute to movement.
    trusted_contacts: HashSet<i32>,
}

impl DistanceManager {
    pub fn new() -> Self {
        Self {
            quarantine_contacts: HashMap::new(),
            trusted_contacts: HashSet::new(),
        }
    }

    /// Compute multi-finger outlier-rejected consensus delta and activity distance.
    pub fn get_consensus_dist_2d(
        &mut self,
        old_contacts: &[TouchpadContact],
        new_contacts: &[TouchpadContact],
        has_fingers_released: bool,
    ) -> ConsensusDistance {
        if has_fingers_released || new_contacts.is_empty() {
            self.quarantine_contacts.clear();
            self.trusted_contacts.clear();
            return ConsensusDistance::default();
        }

        let now = current_time_ms();

        // Remove contacts that are no longer on the touchpad
        self.trusted_contacts
            .retain(|c| new_contacts.iter().any(|nc| nc.contact_id == *c));
        self.quarantine_contacts
            .retain(|c, _| new_contacts.iter().any(|nc| nc.contact_id == *c));

        // Quarantine system: promote contacts after QUARANTINE_THRESHOLD_MS or >= 1 full matched frame
        for new_contact in new_contacts {
            let existed_in_old = old_contacts
                .iter()
                .any(|oc| oc.contact_id == new_contact.contact_id);

            if let Some(entry) = self.quarantine_contacts.get_mut(&new_contact.contact_id) {
                entry.1 += 1;
                let elapsed_q = now - entry.0;
                if elapsed_q >= QUARANTINE_THRESHOLD_MS || (existed_in_old && entry.1 >= 1) {
                    self.trusted_contacts.insert(new_contact.contact_id);
                    self.quarantine_contacts.remove(&new_contact.contact_id);
                }
            } else if !self.trusted_contacts.contains(&new_contact.contact_id) {
                self.quarantine_contacts
                    .insert(new_contact.contact_id, (now, 0));
            }
        }

        // Gather displacement vectors for all matched trusted contacts
        let mut matched_deltas: Vec<(f32, f32, f32)> = Vec::with_capacity(new_contacts.len());
        for new_contact in new_contacts {
            if !self.trusted_contacts.contains(&new_contact.contact_id) {
                continue;
            }
            if let Some(old_contact) = old_contacts
                .iter()
                .find(|oc| oc.contact_id == new_contact.contact_id)
            {
                let dx = (new_contact.x - old_contact.x) as f32;
                let dy = (new_contact.y - old_contact.y) as f32;
                let dist = (dx * dx + dy * dy).sqrt();
                matched_deltas.push((dx, dy, dist));
            }
        }

        let trusted_count = matched_deltas.len();
        if trusted_count == 0 {
            return ConsensusDistance::default();
        }

        let moving_trusted_count = matched_deltas
            .iter()
            .filter(|&&(_, _, d)| d >= FINGER_STATIC_NOISE_FLOOR)
            .count();

        if trusted_count >= 3 {
            // 1. Stationary single-finger twitch rejection ("拖动静止时指针防跳"):
            // If at least 2 of the 3 fingers are stationary (< FINGER_STATIC_NOISE_FLOOR),
            // any motion on the remaining 1 finger is pad deformation, capacitive noise,
            // or pre-lift finger roll — keep the cursor completely still.
            if moving_trusted_count <= 1 {
                return ConsensusDistance {
                    delta: (0.0, 0.0),
                    dist_2d: 0.0,
                    activity_dist: 0.0,
                    moving_trusted_count,
                    trusted_count,
                    coherence: 0.0,
                };
            }

            // 2. Component-wise median of the first 3 fingers (immune to single-finger jumps/wobble)
            let med_dx = median3(matched_deltas[0].0, matched_deltas[1].0, matched_deltas[2].0);
            let med_dy = median3(matched_deltas[0].1, matched_deltas[1].1, matched_deltas[2].1);

            // 3. Per-axis coherent mean of fingers agreeing with the median.
            // Evaluating X and Y agreement independently prevents fast horizontal movement
            // (large med_dx) from inflating the allowed vertical error bound (med_dy).
            let max_dev_x = (med_dx.abs() * 0.45).max(2.5);
            let max_dev_y = (med_dy.abs() * 0.45).max(2.5);
            let mut sum_x = 0.0f32;
            let mut count_x = 0usize;
            let mut sum_y = 0.0f32;
            let mut count_y = 0usize;

            for &(dx, dy, _) in &matched_deltas[..3] {
                if (dx - med_dx).abs() <= max_dev_x && dx * med_dx >= -0.5 {
                    sum_x += dx;
                    count_x += 1;
                }
                if (dy - med_dy).abs() <= max_dev_y && dy * med_dy >= -0.5 {
                    sum_y += dy;
                    count_y += 1;
                }
            }

            let coh_dx = if count_x > 0 { sum_x / count_x as f32 } else { med_dx };
            let coh_dy = if count_y > 0 { sum_y / count_y as f32 } else { med_dy };

            let mut final_dx = 0.65 * med_dx + 0.35 * coh_dx;
            let mut final_dy = 0.65 * med_dy + 0.35 * coh_dy;

            // 4. Multi-finger directional coherence ratio:
            // Rejects opposite-direction finger splay or peel-off jumps when lifting fingers.
            let mean_vx = (matched_deltas[0].0 + matched_deltas[1].0 + matched_deltas[2].0) / 3.0;
            let mean_vy = (matched_deltas[0].1 + matched_deltas[1].1 + matched_deltas[2].1) / 3.0;
            let vec_len = (mean_vx * mean_vx + mean_vy * mean_vy).sqrt();
            let mean_scalar =
                (matched_deltas[0].2 + matched_deltas[1].2 + matched_deltas[2].2) / 3.0;

            let coherence = if mean_scalar > 0.01 {
                (vec_len / mean_scalar).clamp(0.0, 1.0)
            } else {
                0.0
            };

            if coherence < 0.38 {
                return ConsensusDistance {
                    delta: (0.0, 0.0),
                    dist_2d: 0.0,
                    activity_dist: 0.0,
                    moving_trusted_count,
                    trusted_count,
                    coherence,
                };
            } else if coherence < 0.72 {
                let scale = (coherence - 0.38) / 0.34;
                final_dx *= scale;
                final_dy *= scale;
            }

            let dist_2d = (final_dx * final_dx + final_dy * final_dy).sqrt();
            ConsensusDistance {
                delta: (final_dx, final_dy),
                dist_2d,
                activity_dist: dist_2d,
                moving_trusted_count,
                trusted_count,
                coherence,
            }
        } else if trusted_count == 2 {
            let (dx0, dy0, d0) = matched_deltas[0];
            let (dx1, dy1, d1) = matched_deltas[1];
            let dot = dx0 * dx1 + dy0 * dy1;
            if moving_trusted_count == 2 && dot > 0.0 {
                let dx = (dx0 + dx1) * 0.5;
                let dy = (dy0 + dy1) * 0.5;
                let dist = (dx * dx + dy * dy).sqrt();
                let mean_scalar = (d0 + d1) * 0.5;
                let coherence = if mean_scalar > 0.01 {
                    (dist / mean_scalar).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                ConsensusDistance {
                    delta: (dx, dy),
                    dist_2d: dist,
                    activity_dist: dist,
                    moving_trusted_count,
                    trusted_count,
                    coherence,
                }
            } else {
                ConsensusDistance {
                    delta: (0.0, 0.0),
                    dist_2d: 0.0,
                    activity_dist: 0.0,
                    moving_trusted_count,
                    trusted_count,
                    coherence: 0.0,
                }
            }
        } else {
            // Single finger: provide activity_dist so FingerCounter can detect 1-finger cancel
            let (_, _, d0) = matched_deltas[0];
            ConsensusDistance {
                delta: (0.0, 0.0),
                dist_2d: 0.0,
                activity_dist: if d0 >= FINGER_STATIC_NOISE_FLOOR { d0 } else { 0.0 },
                moving_trusted_count,
                trusted_count,
                coherence: 1.0,
            }
        }
    }

    /// Legacy helper returning `(contact_id, delta, dist_2d)` via consensus calculation.
    pub fn get_longest_dist_2d(
        &mut self,
        old_contacts: &[TouchpadContact],
        new_contacts: &[TouchpadContact],
        has_fingers_released: bool,
    ) -> (i32, (f32, f32), f32) {
        let consensus = self.get_consensus_dist_2d(old_contacts, new_contacts, has_fingers_released);
        let id = new_contacts.first().map(|c| c.contact_id).unwrap_or(0);
        (id, consensus.delta, consensus.dist_2d)
    }

    /// Reset all quarantine and trusted state.
    pub fn reset(&mut self) {
        self.quarantine_contacts.clear();
        self.trusted_contacts.clear();
    }
}

#[inline]
fn median3(a: f32, b: f32, c: f32) -> f32 {
    a.max(b).min(a.min(b).max(c))
}

/// Current time in milliseconds since UNIX epoch.
fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_contacts(pts: &[(i32, i32, i32)]) -> Vec<TouchpadContact> {
        pts.iter()
            .map(|&(id, x, y)| TouchpadContact::new(id, x, y))
            .collect()
    }

    #[test]
    fn test_stationary_single_finger_twitch_rejected() {
        let mut dm = DistanceManager::new();
        let f0 = make_contacts(&[(1, 1000, 1000), (2, 1500, 1000), (3, 2000, 1000)]);
        // Frame 1: enter quarantine
        let _ = dm.get_consensus_dist_2d(&[], &f0, false);
        // Frame 2: promote to trusted
        let _ = dm.get_consensus_dist_2d(&f0, &f0, false);

        // Frame 3: fingers 1 and 2 are stationary (noise <= 1), finger 3 jumps by (12, -15)
        let f_twitch = make_contacts(&[(1, 1001, 1000), (2, 1500, 1000), (3, 2012, 985)]);
        let res = dm.get_consensus_dist_2d(&f0, &f_twitch, false);
        assert_eq!(res.delta, (0.0, 0.0), "Single-finger twitch while stationary must not jump cursor");
        assert_eq!(res.dist_2d, 0.0);
    }

    #[test]
    fn test_horizontal_drag_rejects_single_finger_vertical_outlier() {
        let mut dm = DistanceManager::new();
        let f0 = make_contacts(&[(1, 1000, 1000), (2, 1500, 1000), (3, 2000, 1000)]);
        let _ = dm.get_consensus_dist_2d(&[], &f0, false);
        let _ = dm.get_consensus_dist_2d(&f0, &f0, false);

        // All 3 fingers move right by +20, but finger 3 has a +14 vertical jump
        let f1 = make_contacts(&[(1, 1020, 1000), (2, 1520, 1001), (3, 2020, 1014)]);
        let res = dm.get_consensus_dist_2d(&f0, &f1, false);
        assert!((res.delta.0 - 20.0).abs() < 0.5);
        assert!(res.delta.1.abs() <= 1.0, "Vertical outlier on 1 finger should be rejected, got dy={}", res.delta.1);
    }

    #[test]
    fn test_splay_peel_non_coherent_rejected() {
        let mut dm = DistanceManager::new();
        let f0 = make_contacts(&[(1, 1000, 1000), (2, 1500, 1000), (3, 2000, 1000)]);
        let _ = dm.get_consensus_dist_2d(&[], &f0, false);
        let _ = dm.get_consensus_dist_2d(&f0, &f0, false);

        // Fingers splay in opposite directions during lift-off peel
        let f_peel = make_contacts(&[(1, 985, 1005), (2, 1500, 988), (3, 2015, 1006)]);
        let res = dm.get_consensus_dist_2d(&f0, &f_peel, false);
        assert_eq!(res.delta, (0.0, 0.0), "Non-coherent finger splay/peel must be rejected");
    }
}