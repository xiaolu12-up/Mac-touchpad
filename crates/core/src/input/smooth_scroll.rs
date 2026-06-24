use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL, MOUSEINPUT,
};
use windows::Win32::System::LibraryLoader::{LoadLibraryW, GetProcAddress};
use windows::core::PCSTR;
use std::sync::atomic::Ordering;
use crate::input::wheel_hook::{SYNTHETIC_DEVICE_ACTIVE, SYNTHETIC_SCROLL_MARKER};

// --- Windows 11 Synthetic Pointer Device FFI (device creation only) ---

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SYNTHETIC_DEVICE_CREATION_PARAMS {
    pub pointer_type: u32,
    pub max_count: u32,
    pub feedback_mode: u32,
    pub h_monitor: isize,
    pub device_width: u32,
    pub device_height: u32,
    pub options: u32,
}

type CreateSyntheticPointerDevice2Fn = unsafe extern "system" fn(
    params: *const SYNTHETIC_DEVICE_CREATION_PARAMS,
) -> isize;

type DestroySyntheticPointerDeviceFn = unsafe extern "system" fn(
    device: isize,
);

struct SyntheticPointerApis {
    create_device: CreateSyntheticPointerDevice2Fn,
    destroy_device: DestroySyntheticPointerDeviceFn,
}

static SYNTHETIC_APIS: std::sync::OnceLock<Option<SyntheticPointerApis>> = std::sync::OnceLock::new();

fn get_synthetic_pointer_apis() -> Option<&'static SyntheticPointerApis> {
    SYNTHETIC_APIS.get_or_init(|| {
        unsafe {
            let user32 = LoadLibraryW(windows::core::w!("user32.dll")).ok()?;
            let create_proc = GetProcAddress(user32, PCSTR(b"CreateSyntheticPointerDevice2\0".as_ptr()))?;
            let destroy_proc = GetProcAddress(user32, PCSTR(b"DestroySyntheticPointerDevice\0".as_ptr()))?;

            Some(SyntheticPointerApis {
                create_device: std::mem::transmute(create_proc),
                destroy_device: std::mem::transmute(destroy_proc),
            })
        }
    }).as_ref()
}

pub struct SmoothScroller {
    // Dual-velocity model: target decays via damping, current lerps toward target
    current_velocity_x: f64,
    current_velocity_y: f64,
    target_velocity_x: f64,
    target_velocity_y: f64,
    // Core physics parameters
    pub speed: f32,          // sensitivity multiplier
    pub smoothing: f32,      // lerp factor: (0, 1]. Smaller = smoother but laggier
    pub deceleration: f32,   // damping: [0, 1). Closer to 1 = longer glide
    pub base_scale: f32,     // raw delta → target velocity scale (default 0.2)
    pub max_delta: f32,      // max units sent per tick (default 20, lower = less jump)
    pub deadzone: f32,       // velocity cutoff threshold (default 1.0)
    pub natural_scroll: bool,
    // Wheel accumulator for sub-pixel delta accumulation
    wheel_accum_x: f64,
    wheel_accum_y: f64,
    // Win11 Synthetic Pointer device handle (used for 3-finger drag)
    synthetic_device: Option<isize>,
}

impl SmoothScroller {
    pub fn new(speed: f32, smoothing: f32, deceleration: f32, base_scale: f32, max_delta: f32, deadzone: f32, natural_scroll: bool) -> Self {
        let mut scroller = Self {
            current_velocity_x: 0.0,
            current_velocity_y: 0.0,
            target_velocity_x: 0.0,
            target_velocity_y: 0.0,
            speed,
            smoothing: smoothing.clamp(0.01, 1.0),
            deceleration: deceleration.clamp(0.01, 0.99),
            base_scale: base_scale.clamp(0.05, 0.5),
            max_delta: max_delta.clamp(5.0, 60.0),
            deadzone: deadzone.clamp(0.1, 5.0),
            natural_scroll,
            wheel_accum_x: 0.0,
            wheel_accum_y: 0.0,
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

    /// Called when a physical wheel event is intercepted.
    /// `delta`: raw wheel delta (±120 per notch from Windows).
    pub fn add_scroll(&mut self, delta: i32, horizontal: bool) {
        let scroll_dir = if self.natural_scroll { -1.0 } else { 1.0 };
        let amount = delta as f64 * self.base_scale as f64 * self.speed as f64 * scroll_dir;

        if horizontal {
            self.target_velocity_x += amount;
        } else {
            self.target_velocity_y += amount;
        }
    }

    /// Called every ~8ms from the message loop.
    /// Applies dual-velocity physics and sends wheel events via SendInput.
    pub fn tick(&mut self) {
        // Step A: target velocity decays via damping
        self.target_velocity_x *= self.deceleration as f64;
        self.target_velocity_y *= self.deceleration as f64;

        // Step B: current velocity lerps toward target (the "smoothness" magic)
        self.current_velocity_x += (self.target_velocity_x - self.current_velocity_x) * self.smoothing as f64;
        self.current_velocity_y += (self.target_velocity_y - self.current_velocity_y) * self.smoothing as f64;

        // Deadzone cutoff to prevent CPU spin on tiny float residuals
        let dz = self.deadzone as f64;
        if self.current_velocity_x.abs() < dz && self.target_velocity_x.abs() < dz {
            self.current_velocity_x = 0.0;
            self.target_velocity_x = 0.0;
        }
        if self.current_velocity_y.abs() < dz && self.target_velocity_y.abs() < dz {
            self.current_velocity_y = 0.0;
            self.target_velocity_y = 0.0;
        }

        // Accumulate velocity, send moderate-sized deltas for responsive smooth scrolling.
        self.wheel_accum_y += self.current_velocity_y;
        self.wheel_accum_x += self.current_velocity_x;

        let mut inputs: Vec<INPUT> = Vec::new();
        let max_delta = self.max_delta as f64;

        if self.wheel_accum_y.abs() >= 1.0 {
            let wy = self.wheel_accum_y.abs().min(max_delta) as i32 * self.wheel_accum_y.signum() as i32;
            inputs.push(INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0, dy: 0,
                        mouseData: wy as u32,
                        dwFlags: MOUSEEVENTF_WHEEL,
                        time: 0,
                        dwExtraInfo: SYNTHETIC_SCROLL_MARKER,
                    },
                },
            });
            self.wheel_accum_y -= wy as f64;
        }

        if self.wheel_accum_x.abs() >= 1.0 {
            let wx = self.wheel_accum_x.abs().min(max_delta) as i32 * self.wheel_accum_x.signum() as i32;
            inputs.push(INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0, dy: 0,
                        mouseData: wx as u32,
                        dwFlags: MOUSEEVENTF_HWHEEL,
                        time: 0,
                        dwExtraInfo: SYNTHETIC_SCROLL_MARKER,
                    },
                },
            });
            self.wheel_accum_x -= wx as f64;
        }

        if !inputs.is_empty() {
            unsafe {
                SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            }
        }
    }

    pub fn is_moving(&self) -> bool {
        let dz = self.deadzone as f64;
        self.current_velocity_x.abs() >= dz || self.current_velocity_y.abs() >= dz
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

    pub fn set_base_scale(&mut self, v: f32) {
        self.base_scale = v.clamp(0.05, 0.5);
    }

    pub fn set_max_delta(&mut self, v: f32) {
        self.max_delta = v.clamp(5.0, 60.0);
    }

    pub fn set_deadzone(&mut self, v: f32) {
        self.deadzone = v.clamp(0.1, 5.0);
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
