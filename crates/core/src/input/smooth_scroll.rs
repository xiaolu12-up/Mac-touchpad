use windows::Win32::UI::Input::Pointer::{
    POINTER_TOUCH_INFO, POINTER_FLAG_DOWN, POINTER_FLAG_INRANGE,
    POINTER_FLAG_INCONTACT, POINTER_FLAG_UPDATE, POINTER_FLAG_UP,
};
use windows::Win32::UI::WindowsAndMessaging::TOUCH_MASK_CONTACTAREA;
use windows::Win32::Foundation::RECT;
use windows::Win32::System::LibraryLoader::{LoadLibraryW, GetProcAddress};
use windows::core::PCSTR;
use std::sync::atomic::Ordering;
use crate::input::wheel_hook::SYNTHETIC_DEVICE_ACTIVE;

// --- Windows 11 Synthetic Pointer Device FFI Definitions ---

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SYNTHETIC_DEVICE_CREATION_PARAMS {
    pub pointer_type: u32,       // POINTER_INPUT_TYPE (PT_TOUCHPAD is 5)
    pub max_count: u32,
    pub feedback_mode: u32,      // POINTER_FEEDBACK_MODE (POINTER_FEEDBACK_NONE is 0)
    pub h_monitor: isize,        // HMONITOR
    pub device_width: u32,
    pub device_height: u32,
    pub options: u32,            // SYNTHETIC_DEVICE_CREATION_OPTIONS
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct POINTER_TYPE_INFO {
    pub type_: u32,              // POINTER_INPUT_TYPE (PT_TOUCHPAD is 5)
    pub touch_info: POINTER_TOUCH_INFO,
}

type CreateSyntheticPointerDevice2Fn = unsafe extern "system" fn(
    params: *const SYNTHETIC_DEVICE_CREATION_PARAMS,
) -> isize; // HSYNTHETICPOINTERDEVICE is isize

type InjectSyntheticPointerInputFn = unsafe extern "system" fn(
    device: isize,
    pointer_info: *const POINTER_TYPE_INFO,
    count: u32,
) -> windows::Win32::Foundation::BOOL;

type DestroySyntheticPointerDeviceFn = unsafe extern "system" fn(
    device: isize,
);

struct SyntheticPointerApis {
    create_device: CreateSyntheticPointerDevice2Fn,
    inject_input: InjectSyntheticPointerInputFn,
    destroy_device: DestroySyntheticPointerDeviceFn,
}

static SYNTHETIC_APIS: std::sync::OnceLock<Option<SyntheticPointerApis>> = std::sync::OnceLock::new();

fn get_synthetic_pointer_apis() -> Option<&'static SyntheticPointerApis> {
    SYNTHETIC_APIS.get_or_init(|| {
        unsafe {
            // Load user32.dll dynamically so it remains compatible with Windows 10
            let user32 = LoadLibraryW(windows::core::w!("user32.dll")).ok()?;
            let create_proc = GetProcAddress(user32, PCSTR(b"CreateSyntheticPointerDevice2\0".as_ptr()))?;
            let inject_proc = GetProcAddress(user32, PCSTR(b"InjectSyntheticPointerInput\0".as_ptr()))?;
            let destroy_proc = GetProcAddress(user32, PCSTR(b"DestroySyntheticPointerDevice\0".as_ptr()))?;
            
            Some(SyntheticPointerApis {
                create_device: std::mem::transmute(create_proc),
                inject_input: std::mem::transmute(inject_proc),
                destroy_device: std::mem::transmute(destroy_proc),
            })
        }
    }).as_ref()
}

pub struct SmoothScroller {
    velocity_y: f32,
    velocity_x: f32,
    remainder_y: f32,
    remainder_x: f32,
    pub speed: f32,
    pub smoothing: f32,
    pub deceleration: f32,
    pub natural_scroll: bool,
    // Touch injection state
    touch_active: bool,
    touch_offset_x: f32,
    touch_offset_y: f32,
    // Win11 Synthetic Pointer device handle
    synthetic_device: Option<isize>,
}

impl SmoothScroller {
    pub fn new(speed: f32, smoothing: f32, deceleration: f32, natural_scroll: bool) -> Self {
        let mut scroller = Self {
            velocity_y: 0.0,
            velocity_x: 0.0,
            remainder_y: 0.0,
            remainder_x: 0.0,
            speed,
            smoothing: smoothing.clamp(0.01, 1.0),
            deceleration: deceleration.clamp(0.01, 0.99),
            natural_scroll,
            touch_active: false,
            touch_offset_x: 0.0,
            touch_offset_y: 0.0,
            synthetic_device: None,
        };

        // Try to create Win11 synthetic touchpad device
        if let Some(apis) = get_synthetic_pointer_apis() {
            let params = SYNTHETIC_DEVICE_CREATION_PARAMS {
                pointer_type: 5,     // PT_TOUCHPAD
                max_count: 2,        // at least 2 fingers
                feedback_mode: 3,    // POINTER_FEEDBACK_NONE (value is 3)
                h_monitor: 0,
                device_width: 10000, // physical size width in himetric
                device_height: 7000, // physical size height in himetric
                options: 3,          // SDCO_PHYSICAL_SIZE | SDCO_TOUCHPAD_GESTURE_ONLY
            };
            let device = unsafe { (apis.create_device)(&params) };
            if device != 0 {
                scroller.synthetic_device = Some(device);
                SYNTHETIC_DEVICE_ACTIVE.store(true, Ordering::Relaxed);
                tracing::info!("Created user-mode synthetic touchpad device: {:?}", device);
            } else {
                let err = unsafe { windows::Win32::Foundation::GetLastError() };
                tracing::error!("Failed to create synthetic touchpad device, error: {:?}", err);
                SYNTHETIC_DEVICE_ACTIVE.store(false, Ordering::Relaxed);
            }
        } else {
            tracing::info!("CreateSyntheticPointerDevice2 API not found. Smooth scrolling disabled (Win11 only).");
            SYNTHETIC_DEVICE_ACTIVE.store(false, Ordering::Relaxed);
        }

        scroller
    }

    pub fn add_scroll(&mut self, delta: i32, horizontal: bool) {
        if self.synthetic_device.is_none() {
            return;
        }

        let scroll_dir = if self.natural_scroll { -1.0 } else { 1.0 };
        let amount = (delta as f32) * self.speed * scroll_dir;
        
        let r = (1.0 - self.smoothing) * self.deceleration;
        let k = (1.0 - r) / self.smoothing;
        let velocity_added = amount * k;

        if horizontal {
            self.velocity_x += velocity_added;
        } else {
            self.velocity_y += velocity_added;
        }

        if !self.touch_active {
            self.touch_offset_x = 0.0;
            self.touch_offset_y = 0.0;
            self.touch_active = true;
            self.inject_touch_frame(POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT);
        }
    }

    pub fn tick(&mut self) {
        if !self.touch_active || self.synthetic_device.is_none() {
            return;
        }

        let is_moving = self.velocity_y.abs() > 0.1 || self.velocity_x.abs() > 0.1;

        if is_moving {
            if self.velocity_y.abs() > 0.1 {
                let scroll_y = self.velocity_y * self.smoothing;
                self.velocity_y = (self.velocity_y - scroll_y) * self.deceleration;
                
                let total_y = scroll_y + self.remainder_y;
                let move_y = total_y.trunc();
                self.remainder_y = total_y - move_y;
                self.touch_offset_y += move_y;
            }

            if self.velocity_x.abs() > 0.1 {
                let scroll_x = self.velocity_x * self.smoothing;
                self.velocity_x = (self.velocity_x - scroll_x) * self.deceleration;

                let total_x = scroll_x + self.remainder_x;
                let move_x = total_x.trunc();
                self.remainder_x = total_x - move_x;
                self.touch_offset_x += move_x;
            }

            self.inject_touch_frame(POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT);
        } else {
            self.inject_touch_frame(POINTER_FLAG_UP);
            self.touch_active = false;
            self.touch_offset_x = 0.0;
            self.touch_offset_y = 0.0;
            self.velocity_x = 0.0;
            self.velocity_y = 0.0;
            self.remainder_x = 0.0;
            self.remainder_y = 0.0;
        }
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    pub fn set_smoothing(&mut self, s: f32) {
        self.smoothing = s.clamp(0.01, 1.0);
    }

    pub fn set_deceleration(&mut self, d: f32) {
        self.deceleration = d.clamp(0.01, 0.99);
    }

    pub fn set_natural_scroll(&mut self, natural: bool) {
        self.natural_scroll = natural;
    }

    fn inject_touch_frame(&self, flags: windows::Win32::UI::Input::Pointer::POINTER_FLAGS) {
        let device = match self.synthetic_device {
            Some(d) => d,
            None => return,
        };
        let apis = match get_synthetic_pointer_apis() {
            Some(a) => a,
            None => return,
        };

        unsafe {
            let mut cursor_pt = windows::Win32::Foundation::POINT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut cursor_pt);

            // Simulate finger 0 (on touchpad, coordinates are relative to virtual surface [10000 x 7000])
            let mut finger0: POINTER_TOUCH_INFO = std::mem::zeroed();
            finger0.pointerInfo.pointerType = windows::Win32::UI::WindowsAndMessaging::POINTER_INPUT_TYPE(5); // PT_TOUCHPAD
            finger0.pointerInfo.pointerId = 0;
            finger0.pointerInfo.pointerFlags = flags;
            finger0.touchMask = TOUCH_MASK_CONTACTAREA;
            finger0.pointerInfo.ptHimetricLocation.x = 4000 - self.touch_offset_x as i32;
            finger0.pointerInfo.ptHimetricLocation.y = 3500 - self.touch_offset_y as i32;
            
            // Set ptPixelLocation to target the correct window under cursor
            finger0.pointerInfo.ptPixelLocation = cursor_pt;
            
            finger0.rcContact = RECT {
                left: finger0.pointerInfo.ptHimetricLocation.x - 50,
                top: finger0.pointerInfo.ptHimetricLocation.y - 50,
                right: finger0.pointerInfo.ptHimetricLocation.x + 50,
                bottom: finger0.pointerInfo.ptHimetricLocation.y + 50,
            };

            // Simulate finger 1 (moving in parallel, spaced by 2000 himetric units = 2.0 cm)
            let mut finger1: POINTER_TOUCH_INFO = std::mem::zeroed();
            finger1.pointerInfo.pointerType = windows::Win32::UI::WindowsAndMessaging::POINTER_INPUT_TYPE(5); // PT_TOUCHPAD
            finger1.pointerInfo.pointerId = 1;
            finger1.pointerInfo.pointerFlags = flags;
            finger1.touchMask = TOUCH_MASK_CONTACTAREA;
            finger1.pointerInfo.ptHimetricLocation.x = 6000 - self.touch_offset_x as i32;
            finger1.pointerInfo.ptHimetricLocation.y = finger0.pointerInfo.ptHimetricLocation.y;
            
            finger1.pointerInfo.ptPixelLocation = cursor_pt;
            
            finger1.rcContact = RECT {
                left: finger1.pointerInfo.ptHimetricLocation.x - 50,
                top: finger1.pointerInfo.ptHimetricLocation.y - 50,
                right: finger1.pointerInfo.ptHimetricLocation.x + 50,
                bottom: finger1.pointerInfo.ptHimetricLocation.y + 50,
            };

            let type_info = [
                POINTER_TYPE_INFO {
                    type_: 5,
                    touch_info: finger0,
                },
                POINTER_TYPE_INFO {
                    type_: 5,
                    touch_info: finger1,
                },
            ];

            let _ = (apis.inject_input)(device, type_info.as_ptr(), 2);
        }
    }
}

impl Drop for SmoothScroller {
    fn drop(&mut self) {
        if let Some(device) = self.synthetic_device {
            if let Some(apis) = get_synthetic_pointer_apis() {
                unsafe {
                    (apis.destroy_device)(device);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_synthetic_device() {
        if let Some(apis) = get_synthetic_pointer_apis() {
            let params = SYNTHETIC_DEVICE_CREATION_PARAMS {
                pointer_type: 5,     // PT_TOUCHPAD
                max_count: 2,        // at least 2 fingers
                feedback_mode: 3,    // POINTER_FEEDBACK_NONE (value is 3)
                h_monitor: 0,
                device_width: 10000, // physical size width in himetric
                device_height: 7000, // physical size height in himetric
                options: 3,          // SDCO_PHYSICAL_SIZE | SDCO_TOUCHPAD_GESTURE_ONLY
            };
            let device = unsafe { (apis.create_device)(&params) };
            println!("TEST: device handle = {}, error = {:?}", device, unsafe { windows::Win32::Foundation::GetLastError() });
            assert_ne!(device, 0, "Failed to create synthetic touchpad device");
            if device != 0 {
                unsafe { (apis.destroy_device)(device); }
            }
        } else {
            println!("TEST: APIs not available");
        }
    }
}
