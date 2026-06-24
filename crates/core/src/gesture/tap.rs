use std::collections::HashMap;
use crate::hid::types::TouchpadContact;

/// State machine for tap detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TapState {
    /// No fingers on pad.
    Idle,
    /// Fingers are touching — tracking if it's a tap or a gesture.
    Touching,
    /// Tap was detected and fired — wait for fingers to lift before re-arming.
    Fired,
}

/// Multi-finger tap detector using a state machine.
///
/// Detects a quick tap (short duration, minimal movement) with N fingers.
/// The key insight: a tap is fingers-appear → brief pause → fingers-lift.
pub struct TapDetector {
    state: TapState,
    finger_count: usize,
    target_fingers: usize,
    touch_start_time: i64,
    /// Starting positions of each finger by contact_id.
    start_positions: HashMap<i32, (i32, i32)>,
    /// Maximum movement seen so far.
    max_movement: f32,
    /// Whether the movement exceeded the threshold (cancel tap).
    cancelled: bool,
}

impl TapDetector {
    pub fn new(target_fingers: usize) -> Self {
        Self {
            state: TapState::Idle,
            finger_count: 0,
            target_fingers,
            touch_start_time: 0,
            start_positions: HashMap::new(),
            max_movement: 0.0,
            cancelled: false,
        }
    }

    /// Feed a new frame. Returns true if a tap was detected this frame.
    pub fn feed(
        &mut self,
        contacts: &[TouchpadContact],
        now_ms: i64,
        max_duration_ms: u64,
        max_distance: f32,
    ) -> bool {
        let count = contacts.len();

        match self.state {
            TapState::Idle => {
                if count >= self.target_fingers {
                    // Fingers appeared — start tracking
                    self.state = TapState::Touching;
                    self.finger_count = count;
                    self.touch_start_time = now_ms;
                    self.start_positions.clear();
                    for c in contacts {
                        self.start_positions.insert(c.contact_id, (c.x, c.y));
                    }
                    self.max_movement = 0.0;
                    self.cancelled = false;
                }
                false
            }
            TapState::Touching => {
                if count == 0 {
                    // All fingers lifted — check if it was a valid tap
                    let duration = (now_ms - self.touch_start_time) as u64;
                    if !self.cancelled && duration <= max_duration_ms {
                        self.state = TapState::Idle;
                        return true; // TAP DETECTED!
                    }
                    self.state = TapState::Idle;
                    return false;
                }

                if count != self.finger_count {
                    // Finger count changed (added or removed) — cancel
                    self.cancelled = true;
                }

                // Check movement for each finger
                if !self.cancelled {
                    for c in contacts {
                        if let Some(&(sx, sy)) = self.start_positions.get(&c.contact_id) {
                            let dx = (c.x - sx) as f32;
                            let dy = (c.y - sy) as f32;
                            let dist = (dx * dx + dy * dy).sqrt();
                            if dist > self.max_movement {
                                self.max_movement = dist;
                            }
                            if self.max_movement > max_distance {
                                self.cancelled = true;
                            }
                        }
                    }
                }

                // Check timeout
                if (now_ms - self.touch_start_time) as u64 > max_duration_ms {
                    self.cancelled = true;
                }

                false
            }
            TapState::Fired => {
                if count == 0 {
                    self.state = TapState::Idle;
                }
                false
            }
        }
    }

    pub fn reset(&mut self) {
        self.state = TapState::Idle;
        self.finger_count = 0;
        self.start_positions.clear();
        self.max_movement = 0.0;
        self.cancelled = false;
    }
}
