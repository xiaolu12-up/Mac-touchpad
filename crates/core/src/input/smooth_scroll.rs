use windows::Win32::UI::Input::Pointer::{
    InitializeTouchInjection, InjectTouchInput, POINTER_TOUCH_INFO,
    TOUCH_FEEDBACK_NONE, POINTER_FLAG_DOWN, POINTER_FLAG_INRANGE,
    POINTER_FLAG_INCONTACT, POINTER_FLAG_UPDATE, POINTER_FLAG_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{PT_TOUCH, TOUCH_MASK_CONTACTAREA};
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
};

/// Smooth scroll handler for the external mouse wheel.
///
/// Converts discrete mouse wheel rotations into virtual two-finger touchpad pan events
/// using the Windows Touch Injection API. This activates the OS's native DirectComposition
/// hardware-accelerated smooth scrolling and inertia animations.
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
    touch_start_x: i32,
    touch_start_y: i32,
    touch_offset_x: f32,
    touch_offset_y: f32,
    touch_initialized: bool,
}

impl SmoothScroller {
    pub fn new(speed: f32, smoothing: f32, deceleration: f32, natural_scroll: bool) -> Self {
        Self {
            velocity_y: 0.0,
            velocity_x: 0.0,
            remainder_y: 0.0,
            remainder_x: 0.0,
            speed,
            smoothing: smoothing.clamp(0.01, 1.0),
            deceleration: deceleration.clamp(0.01, 0.99),
            natural_scroll,
            touch_active: false,
            touch_start_x: 0,
            touch_start_y: 0,
            touch_offset_x: 0.0,
            touch_offset_y: 0.0,
            touch_initialized: false,
        }
    }

    pub fn add_scroll(&mut self, delta: i32, horizontal: bool) {
        // Natural scroll on Windows inverts the direction:
        // Normally, wheel down (delta < 0) moves page down (scroll down).
        // With natural scroll, wheel down moves content up, so we invert it.
        let scroll_dir = if self.natural_scroll { -1.0 } else { 1.0 };
        // Accumulate velocity/kinetic energy
        let amount = (delta as f32) * self.speed * scroll_dir;
        
        // Calculate scaling factor K to ensure the total scrolled distance
        // matches `amount` exactly, while keeping both smoothing and deceleration active.
        let r = (1.0 - self.smoothing) * self.deceleration;
        let k = (1.0 - r) / self.smoothing;
        let velocity_added = amount * k;

        if horizontal {
            self.velocity_x += velocity_added;
        } else {
            self.velocity_y += velocity_added;
        }

        // Start touch injection session if not already active
        if !self.touch_active {
            let mut pt = windows::Win32::Foundation::POINT::default();
            let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt) };
            self.touch_start_x = pt.x;
            self.touch_start_y = pt.y;
            self.touch_offset_x = 0.0;
            self.touch_offset_y = 0.0;
            self.touch_active = true;

            if !self.touch_initialized {
                let _ = unsafe { InitializeTouchInjection(2, TOUCH_FEEDBACK_NONE) };
                self.touch_initialized = true;
            }

            self.inject_touch_frame(POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT);
        }
    }

    pub fn tick(&mut self) {
        if !self.touch_active {
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
            // Under threshold: release touch contacts
            self.inject_touch_frame(POINTER_FLAG_UP);
            self.touch_active = false;
            self.touch_offset_x = 0.0;
            self.touch_offset_y = 0.0;
            self.velocity_x = 0.0;
            self.velocity_y = 0.0;
            self.remainder_x = 0.0;
            self.remainder_y = 0.0;

            // Inject a dummy mouse event to force Windows back to mouse mode and restore cursor visibility
            unsafe {
                let input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dx: 0,
                            dy: 0,
                            mouseData: 0,
                            dwFlags: MOUSEEVENTF_MOVE,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                let _ = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            }
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
        unsafe {
            // Simulate finger 0
            let mut finger0: POINTER_TOUCH_INFO = std::mem::zeroed();
            finger0.pointerInfo.pointerType = PT_TOUCH;
            finger0.pointerInfo.pointerId = 0;
            // Moving fingers up (subtracting offset_y) drags content up -> page scrolls down.
            finger0.pointerInfo.ptPixelLocation.x = self.touch_start_x - 20 - self.touch_offset_x as i32;
            finger0.pointerInfo.ptPixelLocation.y = self.touch_start_y - self.touch_offset_y as i32;
            finger0.pointerInfo.pointerFlags = flags;
            finger0.touchMask = TOUCH_MASK_CONTACTAREA;
            finger0.rcContact = RECT {
                left: finger0.pointerInfo.ptPixelLocation.x - 2,
                top: finger0.pointerInfo.ptPixelLocation.y - 2,
                right: finger0.pointerInfo.ptPixelLocation.x + 2,
                bottom: finger0.pointerInfo.ptPixelLocation.y + 2,
            };

            // Simulate finger 1 (moving in parallel 40 pixels apart)
            let mut finger1: POINTER_TOUCH_INFO = std::mem::zeroed();
            finger1.pointerInfo.pointerType = PT_TOUCH;
            finger1.pointerInfo.pointerId = 1;
            finger1.pointerInfo.ptPixelLocation.x = self.touch_start_x + 20 - self.touch_offset_x as i32;
            finger1.pointerInfo.ptPixelLocation.y = finger0.pointerInfo.ptPixelLocation.y;
            finger1.pointerInfo.pointerFlags = flags;
            finger1.touchMask = TOUCH_MASK_CONTACTAREA;
            finger1.rcContact = RECT {
                left: finger1.pointerInfo.ptPixelLocation.x - 2,
                top: finger1.pointerInfo.ptPixelLocation.y - 2,
                right: finger1.pointerInfo.ptPixelLocation.x + 2,
                bottom: finger1.pointerInfo.ptPixelLocation.y + 2,
            };

            let _ = InjectTouchInput(&[finger0, finger1]);
        }
    }
}
