/// A single touchpad contact (finger) with ID and position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchpadContact {
    pub contact_id: i32,
    pub x: i32,
    pub y: i32,
}

impl TouchpadContact {
    pub fn new(contact_id: i32, x: i32, y: i32) -> Self {
        Self { contact_id, x, y }
    }

    /// 2D Euclidean distance to another contact.
    pub fn dist_2d(&self, other: &TouchpadContact) -> f32 {
        let dx = (self.x - other.x) as f32;
        let dy = (self.y - other.y) as f32;
        (dx * dx + dy * dy).sqrt()
    }
}

impl std::fmt::Display for TouchpadContact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "(id={}, x={}, y={})", self.contact_id, self.x, self.y)
    }
}

/// Accumulates partial contact fields (id, x, y) from HID value caps.
/// Emits a complete `TouchpadContact` once all three fields are set.
#[derive(Debug, Clone, Default)]
pub struct TouchpadContactCreator {
    pub contact_id: Option<i32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl TouchpadContactCreator {
    /// Try to create a complete contact if all fields are present.
    /// Returns Some(contact) if id, x, and y are all set.
    pub fn try_create(&self) -> Option<TouchpadContact> {
        match (self.contact_id, self.x, self.y) {
            (Some(id), Some(x), Some(y)) => Some(TouchpadContact::new(id, x, y)),
            _ => None,
        }
    }

    /// Reset all fields to None.
    pub fn clear(&mut self) {
        self.contact_id = None;
        self.x = None;
        self.y = None;
    }
}

/// Information about a detected touchpad device.
#[derive(Debug, Clone)]
pub struct TouchpadDeviceInfo {
    /// MD5 hash of the device name string, used as stable identifier.
    pub device_id: String,
    /// USB Vendor ID as hex string.
    pub vendor_id: String,
    /// USB Product ID as hex string.
    pub product_id: String,
    /// Raw HID device handle.
    pub handle: isize,
}

impl TouchpadDeviceInfo {
    pub fn new(handle: isize) -> Self {
        Self {
            device_id: "default".to_string(),
            vendor_id: String::new(),
            product_id: String::new(),
            handle,
        }
    }
}
