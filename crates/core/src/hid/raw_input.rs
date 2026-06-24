use std::collections::HashMap;
use windows::Win32::Devices::HumanInterfaceDevice::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::*;

use crate::hid::types::TouchpadDeviceInfo;

/// Manages raw input registration and device tracking.
pub struct RawInputManager {
    /// Known touchpad devices indexed by handle value (isize).
    pub devices: HashMap<isize, TouchpadDeviceInfo>,
}

impl RawInputManager {
    pub fn new() -> Self {
        Self {
            devices: HashMap::new(),
        }
    }

    /// Register the window to receive raw input from precision touchpads.
    ///
    /// Uses usage page 0x000D (Digitizers), usage 0x0005 (Touch Pad),
    /// with RIDEV_INPUTSINK | RIDEV_DEVNOTIFY so messages arrive even
    /// when the window is not foreground.
    pub fn register_input(&self, hwnd: HWND) -> bool {
        unsafe {
            let device = RAWINPUTDEVICE {
                usUsagePage: 0x000D,
                usUsage: 0x0005,
                dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                hwndTarget: hwnd,
            };

            RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32)
                .is_ok()
        }
    }

    /// Check if a device handle is a precision touchpad.
    /// If it is, track it and return true.
    pub fn exists(&mut self, hdevice: HANDLE) -> bool {
        let handle_val = hdevice.0 as isize;

        unsafe {
            let mut device_info_size = 0u32;

            // Get required size
            let _ = GetRawInputDeviceInfoA(
                hdevice,
                RIDI_DEVICEINFO,
                None,
                &mut device_info_size,
            );

            if device_info_size == 0 {
                return false;
            }

            let mut device_info = RID_DEVICE_INFO {
                cbSize: device_info_size,
                ..Default::default()
            };

            let result = GetRawInputDeviceInfoA(
                hdevice,
                RIDI_DEVICEINFO,
                Some(&mut device_info as *mut _ as *mut _),
                &mut device_info_size,
            );

            if result == u32::MAX {
                return false;
            }

            // Check if this is a precision touchpad (usage page 0x0D, usage 0x05)
            if device_info.Anonymous.hid.usUsagePage == 0x000D
                && device_info.Anonymous.hid.usUsage == 0x0005
            {
                if !self.devices.contains_key(&handle_val) {
                    let mut info = TouchpadDeviceInfo::new(handle_val);
                    info.vendor_id = device_info.Anonymous.hid.dwVendorId.to_string();
                    info.product_id = device_info.Anonymous.hid.dwProductId.to_string();

                    // Get device name and compute MD5 hash for stable ID
                    if let Some(name) = get_device_name(hdevice) {
                        info.device_id = compute_md5(&name);
                    }

                    tracing::info!(
                        "Touchpad detected: id={}, vendor={}, product={}",
                        info.device_id,
                        info.vendor_id,
                        info.product_id
                    );
                    self.devices.insert(handle_val, info);
                }
                return true;
            }
        }

        false
    }

    /// Check if any touchpad device exists by enumerating all HID devices.
    pub fn exists_any(&mut self) -> bool {
        unsafe {
            let mut device_count = 0u32;
            let device_list_size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;

            if GetRawInputDeviceList(None, &mut device_count, device_list_size) != 0 {
                return false;
            }

            let mut devices = vec![RAWINPUTDEVICELIST::default(); device_count as usize];

            if GetRawInputDeviceList(
                Some(devices.as_mut_ptr()),
                &mut device_count,
                device_list_size,
            ) != device_count
            {
                return false;
            }

            for device in &devices {
                if device.dwType == RIM_TYPEHID && self.exists(device.hDevice) {
                    return true;
                }
            }
        }

        false
    }

    /// Get device info for a given handle value.
    pub fn get_device_info(&self, handle: isize) -> Option<&TouchpadDeviceInfo> {
        self.devices.get(&handle)
    }
}

/// Get the device name string for a HID device.
unsafe fn get_device_name(hdevice: HANDLE) -> Option<String> {
    let mut name_size = 0u32;

    let _ = GetRawInputDeviceInfoA(hdevice, RIDI_DEVICENAME, None, &mut name_size);

    if name_size == 0 {
        return None;
    }

    let mut name_buf = vec![0u8; name_size as usize];

    let result = GetRawInputDeviceInfoA(
        hdevice,
        RIDI_DEVICENAME,
        Some(name_buf.as_mut_ptr() as *mut _),
        &mut name_size,
    );

    if result == u32::MAX {
        return None;
    }

    // Convert to string (null-terminated ANSI)
    let len = name_buf
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(name_buf.len());
    String::from_utf8(name_buf[..len].to_vec()).ok()
}

/// Compute MD5 hash of a string, returning lowercase hex.
fn compute_md5(input: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    result
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}
