use windows::Win32::UI::Input::KeyboardAndMouse::*;
use crate::config::DragButton;

/// Mouse simulator using SendInput. Maintains sub-pixel accumulation
/// for smooth cursor movement since SendInput only works with integer deltas.
pub struct MouseSimulator {
    decimal_x: f32,
    decimal_y: f32,
}

impl MouseSimulator {
    pub fn new() -> Self {
        Self {
            decimal_x: 0.0,
            decimal_y: 0.0,
        }
    }

    /// Move the cursor by a relative delta (dx, dy) in pixels.
    pub fn shift_cursor_position(&mut self, x: f32, y: f32) {
        let int_x = (x + self.decimal_x) as i32;
        let int_y = (y + self.decimal_y) as i32;
        self.decimal_x = x + self.decimal_x - int_x as f32;
        self.decimal_y = y + self.decimal_y - int_y as f32;
        move_mouse(int_x, int_y);
    }

    /// Send mouse button down event.
    pub fn drag_down(&self, button: DragButton) {
        let flag = match button {
            DragButton::Left => MOUSEEVENTF_LEFTDOWN,
            DragButton::Right => MOUSEEVENTF_RIGHTDOWN,
            DragButton::Middle => MOUSEEVENTF_MIDDLEDOWN,
            DragButton::None => return,
        };
        send_mouse_event(flag);
    }

    /// Send mouse button up event.
    pub fn drag_up(&self, button: DragButton) {
        let flag = match button {
            DragButton::Left => MOUSEEVENTF_LEFTUP,
            DragButton::Right => MOUSEEVENTF_RIGHTUP,
            DragButton::Middle => MOUSEEVENTF_MIDDLEUP,
            DragButton::None => return,
        };
        send_mouse_event(flag);
    }
}

fn move_mouse(dx: i32, dy: i32) {
    unsafe {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let result = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        if result == 0 {
            tracing::warn!("Failed to move mouse");
        }
    }
}

fn send_mouse_event(flags: MOUSE_EVENT_FLAGS) {
    unsafe {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let result = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        if result == 0 {
            tracing::warn!("Failed to send mouse event");
        }
    }
}
