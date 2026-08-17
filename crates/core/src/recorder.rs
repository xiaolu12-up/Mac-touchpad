use std::sync::mpsc;
use std::time::{Duration, Instant};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

const MODIFIER_VKS: &[u32] = &[
    0x10, 0x11, 0x12,       // VK_SHIFT, VK_CONTROL, VK_MENU
    0xA0, 0xA1, 0xA2, 0xA3, // VK_LSHIFT, VK_RSHIFT, VK_LCONTROL, VK_RCONTROL
    0xA4, 0xA5,             // VK_LMENU, VK_RMENU
    0x5B, 0x5C,             // VK_LWIN, VK_RWIN
];

const RECORD_TIMEOUT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Record a keyboard shortcut by polling keyboard hardware state with
/// GetAsyncKeyState. This approach reads the keyboard state directly from
/// the OS and works reliably regardless of which window has focus — unlike
/// WH_KEYBOARD_LL which can miss keystrokes when WebView2/Chromium has focus.
///
/// The recording runs on a dedicated background thread. Modifier keys (Ctrl,
/// Alt, Shift, Win) are detected at the moment a non-modifier key is pressed.
/// Escape cancels and returns an empty string.
///
/// Returns the combo string (e.g. "Ctrl+Alt+T") or empty string if cancelled/timeout.
pub fn record_shortcut() -> Result<String, String> {
    let (tx, rx) = mpsc::channel();

    std::thread::Builder::new()
        .name("hotkey-recorder".into())
        .spawn(move || {
            let start = Instant::now();
            let mut prev = [false; 256];

            // Snapshot current key state so we only react to *new* presses.
            for vk in 0u32..256 {
                prev[vk as usize] = key_down(vk);
            }

            loop {
                if start.elapsed() > RECORD_TIMEOUT {
                    let _ = tx.send(String::new());
                    return;
                }

                // Scan for freshly pressed keys.
                for vk in 0u32..256 {
                    let down = key_down(vk);
                    let fresh = down && !prev[vk as usize];
                    prev[vk as usize] = down;

                    if !fresh {
                        continue;
                    }

                    // Escape → cancel recording
                    if vk == 0x1B {
                        let _ = tx.send(String::new());
                        return;
                    }

                    // Skip modifier-only key presses; wait for a real key.
                    if is_modifier(vk) {
                        continue;
                    }

                    // Non-modifier key pressed → build combo from current modifier state.
                    let combo = unsafe { build_combo_string(vk) };
                    let _ = tx.send(combo);
                    return;
                }

                std::thread::sleep(POLL_INTERVAL);
            }
        })
        .map_err(|e| format!("Failed to spawn recorder thread: {e}"))?
        .join()
        .map_err(|_| -> String { "Recorder thread panicked".into() })?;

    rx.recv().map_err(|_| "Recording channel closed".into())
}

fn key_down(vk: u32) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
}

fn is_modifier(vk: u32) -> bool {
    MODIFIER_VKS.contains(&vk)
}

unsafe fn build_combo_string(vk: u32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16) & 0x8000 != 0 {
        parts.push("Ctrl".into());
    }
    if (GetAsyncKeyState(VK_MENU.0 as i32) as u16) & 0x8000 != 0 {
        parts.push("Alt".into());
    }
    if (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16) & 0x8000 != 0 {
        parts.push("Shift".into());
    }
    if (GetAsyncKeyState(VK_LWIN.0 as i32) as u16) & 0x8000 != 0
        || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16) & 0x8000 != 0
    {
        parts.push("Win".into());
    }
    parts.push(vk_to_name(vk));
    parts.join("+")
}

fn vk_to_name(vk: u32) -> String {
    match vk {
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x13 => "Pause",
        0x14 => "CapsLock",
        0x1B => "Esc",
        0x20 => "Space",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2C => "PrtSc",
        0x2D => "Insert",
        0x2E => "Delete",
        0x5B | 0x5C => "Win",
        0x60 => "Num0", 0x61 => "Num1", 0x62 => "Num2", 0x63 => "Num3",
        0x64 => "Num4", 0x65 => "Num5", 0x66 => "Num6", 0x67 => "Num7",
        0x68 => "Num8", 0x69 => "Num9",
        0x6A => "Num*", 0x6B => "Num+", 0x6C => "NumEnter",
        0x6D => "Num-", 0x6E => "Num.", 0x6F => "Num/",
        0x70 => "F1", 0x71 => "F2", 0x72 => "F3", 0x73 => "F4",
        0x74 => "F5", 0x75 => "F6", 0x76 => "F7", 0x77 => "F8",
        0x78 => "F9", 0x79 => "F10", 0x7A => "F11", 0x7B => "F12",
        0x90 => "NumLock", 0x91 => "ScrollLock",
        0xA0 | 0xA1 => "Shift",
        0xA2 | 0xA3 => "Ctrl",
        0xA4 | 0xA5 => "Alt",
        0xAD => "VolumeMute", 0xAE => "VolumeDown", 0xAF => "VolumeUp",
        0xB0 => "MediaNext", 0xB1 => "MediaPrev",
        0xB2 => "MediaStop", 0xB3 => "MediaPlay",
        0xBA => ";", 0xBB => "=", 0xBC => ",", 0xBD => "-",
        0xBE => ".", 0xBF => "/", 0xC0 => "`",
        0xDB => "[", 0xDC => "\\", 0xDD => "]", 0xDE => "'",
        _ => {
            if vk >= 0x30 && vk <= 0x39 {
                return ((b'0' + (vk - 0x30) as u8) as char).to_string();
            }
            if vk >= 0x41 && vk <= 0x5A {
                return ((b'A' + (vk - 0x41) as u8) as char).to_string();
            }
            return format!("VK_0x{:02X}", vk);
        }
    }.into()
}
