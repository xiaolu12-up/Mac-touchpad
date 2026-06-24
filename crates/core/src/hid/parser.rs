use std::io::Write;
use windows::Win32::Devices::HumanInterfaceDevice::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::hid::types::{TouchpadContact, TouchpadContactCreator};

/// Debug log file for HID parsing diagnostics.
fn debug_log(msg: &str) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = std::fs::remove_file("C:\\a.WorkCode\\mactouchpad\\hid_debug.log");
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("C:\\a.WorkCode\\mactouchpad\\hid_debug.log")
    {
        let _ = writeln!(f, "{}", msg);
    }
}

macro_rules! dbg_log {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        debug_log(&msg);
    }};
}

/// Result of parsing a WM_INPUT message.
pub struct ParseResult {
    pub device: HANDLE,
    pub contacts: Vec<TouchpadContact>,
    pub contact_count: u32,
    /// Touchpad logical X range [min, max].
    pub x_range: (i32, i32),
    /// Touchpad logical Y range [min, max].
    pub y_range: (i32, i32),
}

/// Parse a WM_INPUT lParam into touchpad contacts.
///
/// Ported from TouchpadHelper.ParseInput(). This is the most critical
/// and platform-specific code in the entire project.
pub unsafe fn parse_input(lparam: LPARAM) -> Option<ParseResult> {
    let mut raw_input_size = 0u32;
    let raw_input_header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;

    // Get required buffer size
    if GetRawInputData(
        HRAWINPUT(lparam.0 as isize),
        RID_INPUT,
        None,
        &mut raw_input_size,
        raw_input_header_size,
    ) != 0
    {
        return None;
    }

    // Allocate buffer and get RAWINPUT
    let mut raw_input_buf = vec![0u8; raw_input_size as usize];

    if GetRawInputData(
        HRAWINPUT(lparam.0 as isize),
        RID_INPUT,
        Some(raw_input_buf.as_mut_ptr() as *mut _),
        &mut raw_input_size,
        raw_input_header_size,
    ) != raw_input_size
    {
        return None;
    }

    let raw_input = &*(raw_input_buf.as_ptr() as *const RAWINPUT);
    let current_device = raw_input.header.hDevice;
    let dw_size_hid = raw_input.data.hid.dwSizeHid;
    let dw_count = raw_input.data.hid.dwCount;

    // Extract HID report data bytes from the end of the RAWINPUT buffer
    let hid_data_size = (dw_size_hid * dw_count) as usize;
    let hid_offset = raw_input_size as usize - hid_data_size;
    let hid_data = &raw_input_buf[hid_offset..];

    // Get preparsed data size
    let mut preparsed_size = 0u32;
    let _ = GetRawInputDeviceInfoA(
        current_device,
        RIDI_PREPARSEDDATA,
        None,
        &mut preparsed_size,
    );

    if preparsed_size == 0 {
        return None;
    }

    // Get preparsed data
    let mut preparsed_buf = vec![0u8; preparsed_size as usize];
    if GetRawInputDeviceInfoA(
        current_device,
        RIDI_PREPARSEDDATA,
        Some(preparsed_buf.as_mut_ptr() as *mut _),
        &mut preparsed_size,
    ) != preparsed_size
    {
        return None;
    }

    let preparsed_data = PHIDP_PREPARSED_DATA(preparsed_buf.as_mut_ptr() as isize);

    // Get HID capabilities
    let mut caps = HIDP_CAPS::default();
    if HidP_GetCaps(preparsed_data, &mut caps) != HIDP_STATUS_SUCCESS {
        return None;
    }

    // Get value caps
    let mut value_caps_count = caps.NumberInputValueCaps;
    let mut value_caps = vec![HIDP_VALUE_CAPS::default(); value_caps_count as usize];

    if HidP_GetValueCaps(
        HidP_Input,
        value_caps.as_mut_ptr(),
        &mut value_caps_count,
        preparsed_data,
    ) != HIDP_STATUS_SUCCESS
    {
        return None;
    }
    value_caps.truncate(value_caps_count as usize);

    // Sort value caps by LinkCollection (matching reference project)
    value_caps.sort_by_key(|vc| vc.LinkCollection);

    // Extract touchpad logical ranges from value caps (LinkCollection 1)
    let mut x_range: (i32, i32) = (0, 0);
    let mut y_range: (i32, i32) = (0, 0);
    for vc in &value_caps {
        if vc.LinkCollection == 1 {
            match (vc.UsagePage, vc.Anonymous.NotRange.Usage) {
                (0x01, 0x30) => x_range = (vc.LogicalMin, vc.LogicalMax),
                (0x01, 0x31) => y_range = (vc.LogicalMin, vc.LogicalMax),
                _ => {}
            }
        }
    }

    // Find the maximum LinkCollection > 0 to know how many contact slots exist
    let max_link = value_caps
        .iter()
        .filter(|vc| vc.LinkCollection > 0)
        .map(|vc| vc.LinkCollection)
        .max()
        .unwrap_or(0);

    let mut _scan_time: u32 = 0;
    let mut contact_count: u32 = 0;
    let mut creators: Vec<TouchpadContactCreator> = vec![TouchpadContactCreator::default(); max_link as usize + 1];
    let mut contacts: Vec<TouchpadContact> = Vec::new();

    // Process each report (dw_count reports per WM_INPUT)
    for contact_index in 0..dw_count {
        let report_offset = (dw_size_hid * contact_index) as usize;
        let report_slice = &hid_data[report_offset..report_offset + dw_size_hid as usize];

        // Reset all creators for this report
        for c in &mut creators {
            c.clear();
        }

        // Extract ALL values from this report
        for value_cap in &value_caps {
            let mut usage_value: u32 = 0;
            let status = HidP_GetUsageValue(
                HidP_Input,
                value_cap.UsagePage,
                value_cap.LinkCollection,
                value_cap.Anonymous.NotRange.Usage,
                &mut usage_value,
                preparsed_data,
                report_slice,
            );

            if status != HIDP_STATUS_SUCCESS {
                continue;
            }

            match value_cap.LinkCollection {
                0 => {
                    match (value_cap.UsagePage, value_cap.Anonymous.NotRange.Usage) {
                        (0x0D, 0x56) => _scan_time = usage_value,
                        (0x0D, 0x54) => contact_count = usage_value,
                        _ => {}
                    }
                }
                link => {
                    let idx = link as usize;
                    if idx < creators.len() {
                        match (value_cap.UsagePage, value_cap.Anonymous.NotRange.Usage) {
                            (0x0D, 0x51) => creators[idx].contact_id = Some(usage_value as i32),
                            (0x01, 0x30) => creators[idx].x = Some(usage_value as i32),
                            (0x01, 0x31) => creators[idx].y = Some(usage_value as i32),
                            _ => {}
                        }
                    }
                }
            }
        }

        // Emit all valid contacts from this report
        // A contact is valid if it has all three fields (id, x, y) set
        // and at least one of x or y is non-zero (filter out empty slots)
        for creator in &creators {
            if let Some(contact) = creator.try_create() {
                // Skip empty contact slots (x=0, y=0)
                if contact.x != 0 || contact.y != 0 {
                    contacts.push(contact);
                }
            }
        }
    }

    // Use contact_count from HID if available, otherwise use detected count
    let final_count = if contact_count > 0 {
        contact_count
    } else {
        contacts.len() as u32
    };

    dbg_log!("ParseResult: hid_count={}, detected={}, contacts={:?}", final_count, contacts.len(),
        contacts.iter().map(|c| format!("({}, {}, {})", c.contact_id, c.x, c.y)).collect::<Vec<_>>());

    Some(ParseResult {
        device: current_device,
        contacts,
        contact_count: final_count,
        x_range,
        y_range,
    })
}
