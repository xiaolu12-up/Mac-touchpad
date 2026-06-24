use crate::hid::types::TouchpadContact;

/// Direction of a detected swipe gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
}

/// 4-finger swipe detector using centroid tracking.
pub struct SwipeDetector {
    /// Centroid position when the gesture started.
    start_centroid: Option<(f32, f32)>,
    /// Whether a swipe has already been fired (prevents re-trigger until fingers lift).
    fired: bool,
}

impl SwipeDetector {
    pub fn new() -> Self {
        Self {
            start_centroid: None,
            fired: false,
        }
    }

    /// Feed a new frame of contacts. Returns Some(direction) if a swipe is detected.
    ///
    /// Should only be called when contacts.len() == 4.
    pub fn feed(
        &mut self,
        contacts: &[TouchpadContact],
        swipe_threshold: f32,
    ) -> Option<SwipeDirection> {
        if contacts.len() != 4 {
            return None;
        }

        if self.fired {
            return None;
        }

        let centroid = compute_centroid(contacts);

        match self.start_centroid {
            None => {
                // First frame with 4 fingers — record starting centroid
                self.start_centroid = Some(centroid);
                None
            }
            Some(start) => {
                let dx = centroid.0 - start.0;
                let dy = centroid.1 - start.1;
                let distance = (dx * dx + dy * dy).sqrt();

                if distance >= swipe_threshold {
                    self.fired = true;
                    let direction = if dx.abs() > dy.abs() {
                        if dx > 0.0 {
                            SwipeDirection::Right
                        } else {
                            SwipeDirection::Left
                        }
                    } else {
                        // Touchpad Y increases downward
                        if dy > 0.0 {
                            SwipeDirection::Down
                        } else {
                            SwipeDirection::Up
                        }
                    };
                    Some(direction)
                } else {
                    None
                }
            }
        }
    }

    /// Reset state when fingers lift.
    pub fn reset(&mut self) {
        self.start_centroid = None;
        self.fired = false;
    }
}

/// Compute the centroid (average position) of all contacts.
fn compute_centroid(contacts: &[TouchpadContact]) -> (f32, f32) {
    let n = contacts.len() as f32;
    let sum = contacts
        .iter()
        .fold((0.0f32, 0.0f32), |acc, c| (acc.0 + c.x as f32, acc.1 + c.y as f32));
    (sum.0 / n, sum.1 / n)
}
