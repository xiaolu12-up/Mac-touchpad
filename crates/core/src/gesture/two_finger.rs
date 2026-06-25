use crate::hid::types::TouchpadContact;

/// Edge zone where a touch started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeZone {
    /// No edge (center of touchpad).
    Center,
    /// Left edge (within edge_threshold of x_min).
    Left,
    /// Right edge (within edge_threshold of x_max).
    Right,
}

/// Two-finger swipe direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoFingerSwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Two-finger swipe detector with edge zone awareness.
///
/// Tracks the centroid of 2 fingers and detects swipe direction
/// once the movement exceeds `swipe_threshold`.
pub struct TwoFingerSwipeDetector {
    start_centroid: Option<(f32, f32)>,
    start_edge: EdgeZone,
    fired: bool,
    finger_count: usize,
}

impl TwoFingerSwipeDetector {
    pub fn new() -> Self {
        Self {
            start_centroid: None,
            start_edge: EdgeZone::Center,
            fired: false,
            finger_count: 0,
        }
    }

    /// Feed a new frame of contacts.
    ///
    /// Returns Some((direction, edge_zone)) if a swipe is detected.
    /// `x_range`: (x_min, x_max) of the touchpad logical coordinates.
    /// `edge_threshold`: how close to the edge (in logical units) to count as "edge".
    pub fn feed(
        &mut self,
        contacts: &[TouchpadContact],
        x_range: (i32, i32),
        swipe_threshold: f32,
        edge_threshold: i32,
    ) -> Option<(TwoFingerSwipeDirection, EdgeZone)> {
        let count = contacts.len();

        // Transition: fingers appeared
        if count >= 2 && self.finger_count < 2 {
            self.start_centroid = Some(centroid(contacts));
            self.start_edge = detect_edge(contacts, x_range, edge_threshold);
            self.fired = false;
            self.finger_count = count;
            return None;
        }

        // Fingers lifted or count changed
        if count < 2 && self.finger_count >= 2 {
            self.finger_count = count;
            self.start_centroid = None;
            return None;
        }

        if count == 0 {
            self.finger_count = 0;
            self.start_centroid = None;
            return None;
        }

        self.finger_count = count;

        if count < 2 || self.fired {
            return None;
        }

        let start = match self.start_centroid {
            Some(s) => s,
            None => {
                self.start_centroid = Some(centroid(contacts));
                self.start_edge = detect_edge(contacts, x_range, edge_threshold);
                return None;
            }
        };

        let current = centroid(contacts);
        let dx = current.0 - start.0;
        let dy = current.1 - start.1;
        let distance = (dx * dx + dy * dy).sqrt();

        if distance >= swipe_threshold {
            self.fired = true;
            let direction = if dx.abs() > dy.abs() {
                if dx > 0.0 {
                    TwoFingerSwipeDirection::Right
                } else {
                    TwoFingerSwipeDirection::Left
                }
            } else {
                if dy > 0.0 {
                    TwoFingerSwipeDirection::Down
                } else {
                    TwoFingerSwipeDirection::Up
                }
            };
            Some((direction, self.start_edge))
        } else {
            None
        }
    }

    pub fn reset(&mut self) {
        self.start_centroid = None;
        self.fired = false;
        self.finger_count = 0;
    }
}

/// Edge slide detector for volume/brightness adjustment.
///
/// Detects single-finger vertical movement on the left or right edge.
pub struct EdgeSlideDetector {
    active_edge: Option<EdgeZone>,
    start_y: Option<i32>,
    last_y: Option<i32>,
    total_dy: i32,
    is_invalid: bool,
}

/// Edge slide result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeSlideAction {
    /// Left edge slide up → volume up.
    VolumeUp,
    /// Left edge slide down → volume down.
    VolumeDown,
    /// Right edge slide up → brightness up.
    BrightnessUp,
    /// Right edge slide down → brightness down.
    BrightnessDown,
}

impl EdgeSlideDetector {
    pub fn new() -> Self {
        Self {
            active_edge: None,
            start_y: None,
            last_y: None,
            total_dy: 0,
            is_invalid: false,
        }
    }

    /// Feed a new frame. Returns Some(action) when a slide threshold is reached.
    ///
    /// Only triggers if the INITIAL touch was on the edge (prevents triggering
    /// when a finger slides from center to edge).
    pub fn feed(
        &mut self,
        contacts: &[TouchpadContact],
        x_range: (i32, i32),
        _y_range: (i32, i32),
        edge_threshold: i32,
        slide_threshold: i32,
    ) -> Option<EdgeSlideAction> {
        if contacts.len() != 1 {
            self.reset();
            return None;
        }

        if self.is_invalid {
            return None;
        }

        let contact = &contacts[0];

        if let Some(active_edge) = self.active_edge {
            // We are already tracking a left-edge slide!
            // If the finger drifts too far horizontally into the center, cancel it.
            if contact.x > x_range.0 + edge_threshold + 200 {
                self.reset();
                self.is_invalid = true;
                return None;
            }

            if let Some(last) = self.last_y {
                self.total_dy += contact.y - last;
                self.last_y = Some(contact.y);
            }

            if self.total_dy.abs() >= slide_threshold {
                let action = match (active_edge, self.total_dy > 0) {
                    (EdgeZone::Left, true) => EdgeSlideAction::VolumeUp,
                    (EdgeZone::Left, false) => EdgeSlideAction::VolumeDown,
                    _ => return None,
                };
                self.total_dy = 0;
                self.start_y = Some(contact.y);
                return Some(action);
            }
            None
        } else {
            // First touch on this slide — check if it's on/near the left edge
            let landing_tolerance = 150;
            if contact.x <= x_range.0 + edge_threshold + landing_tolerance {
                self.active_edge = Some(EdgeZone::Left);
                self.start_y = Some(contact.y);
                self.last_y = Some(contact.y);
                self.total_dy = 0;
                None
            } else {
                self.is_invalid = true;
                None
            }
        }
    }

    pub fn reset(&mut self) {
        self.active_edge = None;
        self.start_y = None;
        self.last_y = None;
        self.total_dy = 0;
        self.is_invalid = false;
    }
}

/// Compute centroid of contacts.
fn centroid(contacts: &[TouchpadContact]) -> (f32, f32) {
    let n = contacts.len() as f32;
    let sum = contacts
        .iter()
        .fold((0.0f32, 0.0f32), |acc, c| (acc.0 + c.x as f32, acc.1 + c.y as f32));
    (sum.0 / n, sum.1 / n)
}

/// Detect which edge zone the centroid of contacts is in.
fn detect_edge(contacts: &[TouchpadContact], x_range: (i32, i32), threshold: i32) -> EdgeZone {
    let cx = centroid(contacts).0 as i32;
    if cx <= x_range.0 + threshold {
        EdgeZone::Left
    } else if cx >= x_range.1 - threshold {
        EdgeZone::Right
    } else {
        EdgeZone::Center
    }
}

