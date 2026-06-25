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

pub fn key_down(key: VIRTUAL_KEY) {
    let input = make_key_input(key, KEYBD_EVENT_FLAGS(0));
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn key_up(key: VIRTUAL_KEY) {
    let input = make_key_input(key, KEYEVENTF_KEYUP);
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
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
pub fn maximize() { send_key_combo(&[VK_LWIN, VK_UP]); }

// ── Volume ──

pub fn volume_up() {
    send_key_combo(&[VK_VOLUME_UP]);
    overlay::show_overlay(50, false);
}

pub fn volume_down() {
    send_key_combo(&[VK_VOLUME_DOWN]);
    overlay::show_overlay(50, false);
}

// ── Brightness ──
// Strategy: try WMI COM first (laptop internal display), fall back to DDC/CI (external monitors).
// Uses windows crate COM/WMI APIs directly — no PowerShell, no extra crates.

use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::Graphics::Gdi::MonitorFromPoint;
use windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTOPRIMARY;
use windows::Win32::Foundation::POINT;

const BRIGHTNESS_STEP: u32 = 10;

// ── WMI brightness control via COM (IWbemServices, no cmd popup) ──

use windows::Win32::System::Com::*;
use windows::Win32::System::Wmi::*;
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, PCWSTR};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};

/// Helper: encode a Rust string to a null-terminated wide string for PCWSTR.
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Cached WMI IWbemServices connection to ROOT\WMI namespace.
struct WmiService {
    svc: IWbemServices,
}

// SAFETY: IWbemServices is a COM interface with thread-safe reference counting.
// We only use it from one calling context at a time (gesture callback thread).
unsafe impl Send for WmiService {}
unsafe impl Sync for WmiService {}

static WMI_SERVICE: std::sync::OnceLock<Option<WmiService>> = std::sync::OnceLock::new();

/// Initialize and cache the WMI connection. Only called once.
fn get_wmi_service() -> Option<&'static WmiService> {
    WMI_SERVICE.get_or_init(|| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let locator: IWbemLocator = CoCreateInstance(
                &WbemLocator,
                None,
                CLSCTX_INPROC_SERVER,
            ).ok()?;

            let svc = locator.ConnectServer(
                &BSTR::from("ROOT\\WMI"),
                &BSTR::new(), &BSTR::new(), &BSTR::new(),
                0, &BSTR::new(), None,
            ).ok()?;

            let _ = CoSetProxyBlanket(
                &svc,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                None,
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            );

            tracing::info!("WMI COM connection to ROOT\\WMI established");
            Some(WmiService { svc })
        }
    }).as_ref()
}

fn variant_to_u32(var: &VARIANT) -> Option<u32> {
    unsafe {
        let vt = (*var.Anonymous.Anonymous).vt;
        match vt {
            windows::Win32::System::Variant::VT_UI1 => Some((*var.Anonymous.Anonymous).Anonymous.bVal as u32),
            windows::Win32::System::Variant::VT_I1 => Some((*var.Anonymous.Anonymous).Anonymous.cVal as u32),
            windows::Win32::System::Variant::VT_UI2 => Some((*var.Anonymous.Anonymous).Anonymous.uiVal as u32),
            windows::Win32::System::Variant::VT_I2 => Some((*var.Anonymous.Anonymous).Anonymous.iVal as u32),
            windows::Win32::System::Variant::VT_UI4 => Some((*var.Anonymous.Anonymous).Anonymous.ulVal),
            windows::Win32::System::Variant::VT_I4 => Some((*var.Anonymous.Anonymous).Anonymous.lVal as u32),
            windows::Win32::System::Variant::VT_INT => Some((*var.Anonymous.Anonymous).Anonymous.intVal as u32),
            windows::Win32::System::Variant::VT_UINT => Some((*var.Anonymous.Anonymous).Anonymous.uintVal),
            _ => {
                tracing::warn!("Unsupported WMI brightness VARIANT type: {:?}", vt);
                None
            }
        }
    }
}

/// Get current brightness via WMI COM query. Returns None if unavailable.
fn wmi_get_brightness() -> Option<u32> {
    let wmi = get_wmi_service()?;
    unsafe {
        let enumerator = wmi.svc.ExecQuery(
            &BSTR::from("WQL"),
            &BSTR::from("SELECT CurrentBrightness FROM WmiMonitorBrightness WHERE Active = TRUE"),
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        ).ok()?;

        let mut objs = [None; 1];
        let mut returned = 0u32;
        if enumerator.Next(WBEM_INFINITE, &mut objs, &mut returned).is_err() {
            return None;
        }
        if returned == 0 { return None; }

        let obj = objs[0].as_ref()?;
        let mut val = VARIANT::default();
        let prop = to_wide("CurrentBrightness");
        obj.Get(PCWSTR(prop.as_ptr()), 0, &mut val, None, None).ok()?;

        variant_to_u32(&val)
    }
}


/// Set brightness via WMI COM ExecMethod (WmiMonitorBrightnessMethods.WmiSetBrightness).
fn wmi_set_brightness(value: u32) -> bool {
    let wmi = match get_wmi_service() {
        Some(w) => w,
        None => return false,
    };

    unsafe {
        // 1. Get class definition for input parameter template
        let mut class_obj: Option<IWbemClassObject> = None;
        if wmi.svc.GetObject(
            &BSTR::from("WmiMonitorBrightnessMethods"),
            WBEM_FLAG_RETURN_WBEM_COMPLETE,
            None,
            Some(&mut class_obj),
            None,
        ).is_err() {
            return false;
        }
        let class_obj = match class_obj { Some(c) => c, None => return false };

        // 2. Get method input parameter definition
        let method_name = to_wide("WmiSetBrightness");
        let mut in_params_def: Option<IWbemClassObject> = None;
        if class_obj.GetMethod(
            PCWSTR(method_name.as_ptr()),
            0,
            &mut in_params_def,
            std::ptr::null_mut(),
        ).is_err() {
            return false;
        }
        let in_params_def = match in_params_def { Some(c) => c, None => return false };

        // 3. Spawn a writable instance
        let in_params = match in_params_def.SpawnInstance(0) {
            Ok(p) => p,
            Err(_) => return false,
        };

        // 4. Set Timeout = 1
        let timeout_name = to_wide("Timeout");
        let mut timeout_var = VARIANT::default();
        (*timeout_var.Anonymous.Anonymous).vt = windows::Win32::System::Variant::VT_UI4;
        (*timeout_var.Anonymous.Anonymous).Anonymous.ulVal = 1;
        let _ = in_params.Put(PCWSTR(timeout_name.as_ptr()), 0, &timeout_var, 0);

        // 5. Set Brightness = value
        let brightness_name = to_wide("Brightness");
        let mut brightness_var = VARIANT::default();
        (*brightness_var.Anonymous.Anonymous).vt = windows::Win32::System::Variant::VT_UI1;
        (*brightness_var.Anonymous.Anonymous).Anonymous.bVal = value as u8;
        let _ = in_params.Put(PCWSTR(brightness_name.as_ptr()), 0, &brightness_var, 0);

        // 6. Query for the active instance path
        let enumerator = match wmi.svc.ExecQuery(
            &BSTR::from("WQL"),
            &BSTR::from("SELECT __PATH FROM WmiMonitorBrightnessMethods WHERE Active = TRUE"),
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        ) {
            Ok(e) => e,
            Err(_) => return false,
        };

        let mut objs = [None; 1];
        let mut returned = 0u32;
        if enumerator.Next(WBEM_INFINITE, &mut objs, &mut returned).is_err() || returned == 0 {
            return false;
        }
        let obj = match &objs[0] { Some(o) => o, None => return false };

        let path_prop = to_wide("__PATH");
        let mut path_var = VARIANT::default();
        if obj.Get(PCWSTR(path_prop.as_ptr()), 0, &mut path_var, None, None).is_err() {
            return false;
        }
        let path_bstr = (*(*path_var.Anonymous.Anonymous).Anonymous.bstrVal).to_string();
        if path_bstr.is_empty() { return false; }

        // 7. Execute the method
        wmi.svc.ExecMethod(
            &BSTR::from(&path_bstr),
            &BSTR::from("WmiSetBrightness"),
            Default::default(),
            None,
            &in_params,
            None,
            None,
        ).is_ok()
    }
}

/// Try adjusting brightness via WMI. Returns true if successful.
fn adjust_brightness_wmi(delta: i32) -> bool {
    let cur = match wmi_get_brightness() {
        Some(v) => v,
        None => return false,
    };
    let new_val = (cur as i32 + delta).clamp(0, 100) as u32;
    if wmi_set_brightness(new_val) {
        tracing::info!("Brightness (WMI): {} → {}", cur, new_val);
        overlay::show_overlay(new_val as i32, true);
        true
    } else {
        false
    }
}

// ── DDC/CI brightness control (for external monitors via dxva2.dll) ──

/// Dynamically loaded dxva2.dll brightness functions.
struct BrightnessApis {
    get_num: unsafe extern "system" fn(HMONITOR, *mut u32) -> i32,
    get_monitors: unsafe extern "system" fn(HMONITOR, u32, *mut PHYSICAL_MONITOR) -> i32,
    get_brightness: unsafe extern "system" fn(isize, *mut u32, *mut u32, *mut u32) -> i32,
    set_brightness: unsafe extern "system" fn(isize, u32) -> i32,
    destroy: unsafe extern "system" fn(isize) -> i32,
}

#[repr(C)]
#[derive(Clone)]
struct PHYSICAL_MONITOR {
    handle: isize,
    #[allow(dead_code)]
    description: [u16; 128],
}

static BRIGHTNESS_APIS: std::sync::OnceLock<Option<BrightnessApis>> = std::sync::OnceLock::new();

fn get_brightness_apis() -> Option<&'static BrightnessApis> {
    BRIGHTNESS_APIS.get_or_init(|| unsafe {
        let lib = windows::Win32::System::LibraryLoader::LoadLibraryW(windows::core::w!("dxva2.dll")).ok()?;
        let get_num = std::mem::transmute(windows::Win32::System::LibraryLoader::GetProcAddress(lib, windows::core::PCSTR(b"GetNumberOfPhysicalMonitorsFromHMONITOR\0".as_ptr()))?);
        let get_monitors = std::mem::transmute(windows::Win32::System::LibraryLoader::GetProcAddress(lib, windows::core::PCSTR(b"GetPhysicalMonitorsFromHMONITOR\0".as_ptr()))?);
        let get_brightness = std::mem::transmute(windows::Win32::System::LibraryLoader::GetProcAddress(lib, windows::core::PCSTR(b"GetMonitorBrightness\0".as_ptr()))?);
        let set_brightness = std::mem::transmute(windows::Win32::System::LibraryLoader::GetProcAddress(lib, windows::core::PCSTR(b"SetMonitorBrightness\0".as_ptr()))?);
        let destroy = std::mem::transmute(windows::Win32::System::LibraryLoader::GetProcAddress(lib, windows::core::PCSTR(b"DestroyPhysicalMonitor\0".as_ptr()))?);
        Some(BrightnessApis { get_num, get_monitors, get_brightness, set_brightness, destroy })
    }).as_ref()
}

/// Try adjusting brightness via DDC/CI. Returns true if successful.
fn adjust_brightness_ddcci(delta: i32) -> bool {
    let apis = match get_brightness_apis() {
        Some(a) => a,
        None => { return false; }
    };
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut count: u32 = 0;
        if (apis.get_num)(monitor, &mut count) == 0 || count == 0 { return false; }
        let mut monitors = vec![PHYSICAL_MONITOR { handle: 0, description: [0u16; 128] }; count as usize];
        if (apis.get_monitors)(monitor, count, monitors.as_mut_ptr()) == 0 { return false; }
        let h = monitors[0].handle;
        let mut min: u32 = 0;
        let mut cur: u32 = 0;
        let mut max: u32 = 100;
        let ok = if (apis.get_brightness)(h, &mut min, &mut cur, &mut max) != 0 {
            let new_val = (cur as i32 + delta).clamp(min as i32, max as i32) as u32;
            (apis.set_brightness)(h, new_val);
            tracing::info!("Brightness (DDC/CI): {} → {}", cur, new_val);
            overlay::show_overlay(new_val as i32, true);
            true
        } else {
            false
        };
        (apis.destroy)(h);
        ok
    }
}

// ── Public API: WMI first, then DDC/CI fallback ──

fn adjust_brightness(delta: i32) {
    if adjust_brightness_wmi(delta) {
        return;
    }
    if adjust_brightness_ddcci(delta) {
        return;
    }
    tracing::warn!("Brightness adjustment failed: both WMI and DDC/CI unavailable");
}

pub fn brightness_up() {
    adjust_brightness(BRIGHTNESS_STEP as i32);
}

pub fn brightness_down() {
    adjust_brightness(-(BRIGHTNESS_STEP as i32));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wmi_brightness() {
        if let Some(b) = wmi_get_brightness() {
            println!("Current brightness from WMI: {}", b);
            assert!(b <= 100);
        } else {
            println!("WMI brightness not available on this hardware.");
        }
    }
}

