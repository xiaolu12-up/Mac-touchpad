use crate::hid::types::TouchpadContact;

/// Result of a pinch/spread gesture detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinchDirection {
    /// Fingers moving apart (spread) → show desktop
    Spread,
    /// Fingers moving together (pinch) → open start
    Pinch,
}

/// Pinch and spread gesture detector.
///
/// Tracks the average pairwise distance between all finger pairs.
/// When 4 fingers are present, there are C(4,2) = 6 pairs.
pub struct PinchDetector {
    /// Initial average pairwise distance when gesture started.
    initial_avg_distance: Option<f32>,
    /// Whether a gesture has been detected and fired.
    fired: bool,
}

impl PinchDetector {
    pub fn new() -> Self {
        Self {
            initial_avg_distance: None,
            fired: false,
        }
    }

    /// Feed a new frame of contacts. Returns Some(direction) if pinch/spread detected.
    ///
    /// Should only be called when contacts.len() == 4.
    pub fn feed(
        &mut self,
        contacts: &[TouchpadContact],
        pinch_spread_threshold: f32,
    ) -> Option<PinchDirection> {
        if contacts.len() != 4 {
            return None;
        }

        if self.fired {
            return None;
        }

        let current_avg = avg_pairwise_distance(contacts);

        match self.initial_avg_distance {
            None => {
                self.initial_avg_distance = Some(current_avg);
                None
            }
            Some(initial) => {
                let delta = current_avg - initial;

                if delta >= pinch_spread_threshold {
                    // Fingers moved apart → spread
                    self.fired = true;
                    Some(PinchDirection::Spread)
                } else if delta <= -pinch_spread_threshold {
                    // Fingers moved together → pinch
                    self.fired = true;
                    Some(PinchDirection::Pinch)
                } else {
                    None
                }
            }
        }
    }

    /// Reset state when fingers lift.
    pub fn reset(&mut self) {
        self.initial_avg_distance = None;
        self.fired = false;
    }
}

/// Compute the average distance between all pairs of contacts.
fn avg_pairwise_distance(contacts: &[TouchpadContact]) -> f32 {
    let n = contacts.len();
    if n < 2 {
        return 0.0;
    }

    let mut total = 0.0f32;
    let mut count = 0u32;

    for i in 0..n {
        for j in (i + 1)..n {
            total += contacts[i].dist_2d(&contacts[j]);
            count += 1;
        }
    }

    if count > 0 {
        total / count as f32
    } else {
        0.0
    }
}
