use windows::Win32::UI::Input::KeyboardAndMouse::*;
use crate::overlay;
use crate::input::wheel_hook::SYNTHETIC_SCROLL_MARKER;

/// Send a keyboard shortcut as key-down events (in order)
/// followed by key-up events (in reverse order).
pub fn send_key_combo(keys: &[VIRTUAL_KEY]) {
    let mut inputs = Vec::with_capacity(keys.len() * 2);
    for &key in keys {
        inputs.push(make_key_input(key, KEYBD_EVENT_FLAGS(0)));
    }
    for &key in keys.iter().rev() {
        inputs.push(make_key_input(key, KEYEVENTF_KEYUP));
    }
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn make_key_input(key: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0,
            },
        },
    }
}

// ── Gesture shortcuts ──

pub fn win_tab() { send_key_combo(&[VK_LWIN, VK_TAB]); }
pub fn alt_tab() { send_key_combo(&[VK_LMENU, VK_TAB]); }
pub fn ctrl_win_left() { send_key_combo(&[VK_LCONTROL, VK_LWIN, VK_LEFT]); }
pub fn ctrl_win_right() { send_key_combo(&[VK_LCONTROL, VK_LWIN, VK_RIGHT]); }
pub fn show_desktop() { send_key_combo(&[VK_LWIN, VK_D]); }
pub fn open_start() { send_key_combo(&[VK_LWIN]); }
pub fn search() { send_key_combo(&[VK_LWIN, VK_S]); }
pub fn notification_center() { send_key_combo(&[VK_LWIN, VK_A]); }
pub fn page_up() { send_key_combo(&[VK_PRIOR]); }
pub fn page_down() { send_key_combo(&[VK_NEXT]); }

// ── Volume ──

pub fn volume_up() {
    send_key_combo(&[VK_VOLUME_UP]);
    overlay::show_overlay(50, false);
}

pub fn volume_down() {
    send_key_combo(&[VK_VOLUME_DOWN]);
    overlay::show_overlay(50, false);
}

// ── Brightness (OSD via virtual key codes) ──

/// Brightness up using system OSD key (0xE0).
/// Triggers the system brightness bar like laptop Fn keys.
pub fn brightness_up() {
    send_key_press(VIRTUAL_KEY(0xE0));
    overlay::show_overlay(50, true);
}

/// Brightness down using system OSD key (0xE1).
pub fn brightness_down() {
    send_key_press(VIRTUAL_KEY(0xE1));
    overlay::show_overlay(50, true);
}

/// Send a single key press (down + up) for a virtual key code.
/// Uses KEYEVENTF_EXTENDEDKEY for OSD keys (brightness etc.).
fn send_key_press(vk: VIRTUAL_KEY) {
    unsafe {
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk, wScan: 0,
                        dwFlags: KEYEVENTF_EXTENDEDKEY,
                        time: 0, dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk, wScan: 0,
                        dwFlags: KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
                        time: 0, dwExtraInfo: 0,
                    },
                },
            },
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

// ── Smooth scroll ──

/// Send a smooth mouse wheel scroll event.
/// `delta`: positive = scroll up, negative = scroll down.
/// `speed`: multiplier for scroll amount.
pub fn smooth_scroll(delta: i32, speed: f32) {
    let wheel_delta = (delta as f32 * speed * 120.0 / 100.0) as i32;
    if wheel_delta == 0 { return; }

    unsafe {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: wheel_delta as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: SYNTHETIC_SCROLL_MARKER,
                },
            },
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

/// Send a horizontal scroll event (for horizontal swipe gestures).
pub fn smooth_scroll_horizontal(delta: i32, speed: f32) {
    let wheel_delta = (delta as f32 * speed * 120.0 / 100.0) as i32;
    if wheel_delta == 0 { return; }

    unsafe {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: wheel_delta as u32,
                    dwFlags: MOUSEEVENTF_HWHEEL,
                    time: 0,
                    dwExtraInfo: SYNTHETIC_SCROLL_MARKER,
                },
            },
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}
