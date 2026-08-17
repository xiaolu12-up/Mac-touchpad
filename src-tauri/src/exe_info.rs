use base64::Engine;
use mac_touchpad_core::config::AppPolicyItem;
use std::path::Path;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

/// Opens a native Windows OpenFileDialog for selecting a `.exe` file,
/// then extracts its absolute path, base name, file/product description, and icon as base64 PNG.
pub fn browse_and_inspect_exe() -> Option<AppPolicyItem> {
    let path = open_exe_file_dialog()?;
    Some(inspect_exe_path(&path))
}

/// Inspects a given executable path to extract metadata and icon.
pub fn inspect_exe_path(path: &str) -> AppPolicyItem {
    let p = Path::new(path);
    let name = p
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string();

    let description = get_exe_description(path);
    let icon = get_exe_icon_base64(path);

    AppPolicyItem {
        path: path.to_string(),
        name,
        description,
        icon,
    }
}

/// Opens a Windows file dialog for selecting a `.exe` file.
pub fn open_exe_file_dialog() -> Option<String> {
    let mut file_buf = [0u16; 1024];
    // Filter format: "Executable Files (*.exe)\0*.exe\0All Files (*.*)\0*.*\0\0"
    let filter = "可执行程序 (*.exe)\0*.exe\0所有文件 (*.*)\0*.*\0\0"
        .encode_utf16()
        .collect::<Vec<u16>>();
    let title = "选择可执行程序 (.exe)\0".encode_utf16().collect::<Vec<u16>>();

    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: HWND(0),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file_buf.as_mut_ptr()),
        nMaxFile: file_buf.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };

    unsafe {
        if GetOpenFileNameW(&mut ofn).as_bool() {
            let len = file_buf.iter().position(|&c| c == 0).unwrap_or(file_buf.len());
            if len > 0 {
                return Some(String::from_utf16_lossy(&file_buf[..len]));
            }
        }
    }
    None
}

/// Extracts software description from Windows FileVersionInfo resource.
pub fn get_exe_description(path: &str) -> String {
    let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut handle = 0u32;
        let size = GetFileVersionInfoSizeW(PCWSTR(wide_path.as_ptr()), Some(&mut handle));
        if size > 0 {
            let mut data = vec![0u8; size as usize];
            if GetFileVersionInfoW(
                PCWSTR(wide_path.as_ptr()),
                0,
                size,
                data.as_mut_ptr() as *mut _,
            )
            .is_ok()
            {
                let subblocks = [
                    "\\StringFileInfo\\080404b0\\FileDescription",
                    "\\StringFileInfo\\080404b0\\ProductName",
                    "\\StringFileInfo\\040904b0\\FileDescription",
                    "\\StringFileInfo\\040904b0\\ProductName",
                    "\\StringFileInfo\\040904E4\\FileDescription",
                    "\\StringFileInfo\\040904E4\\ProductName",
                    "\\StringFileInfo\\000004b0\\FileDescription",
                    "\\StringFileInfo\\000004b0\\ProductName",
                ];

                for sub in subblocks {
                    let wide_sub: Vec<u16> = sub.encode_utf16().chain(std::iter::once(0)).collect();
                    let mut val_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
                    let mut val_len = 0u32;
                    if VerQueryValueW(
                        data.as_ptr() as *const _,
                        PCWSTR(wide_sub.as_ptr()),
                        &mut val_ptr,
                        &mut val_len,
                    )
                    .as_bool()
                        && !val_ptr.is_null()
                        && val_len > 1
                    {
                        let slice =
                            std::slice::from_raw_parts(val_ptr as *const u16, (val_len - 1) as usize);
                        let s = String::from_utf16_lossy(slice).trim().to_string();
                        if !s.is_empty() {
                            return s;
                        }
                    }
                }
            }
        }
    }

    // Fallback: file stem name without .exe
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path);
    stem.to_string()
}

/// Extracts executable icon and encodes it to base64 PNG data URL.
pub fn get_exe_icon_base64(path: &str) -> String {
    let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut sfi = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR(wide_path.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );

        if result != 0 && sfi.hIcon.0 != 0 {
            let hicon = sfi.hIcon;
            let icon_res = hicon_to_base64_png(hicon);
            let _ = DestroyIcon(hicon);
            if let Ok(b64) = icon_res {
                return format!("data:image/png;base64,{}", b64);
            }
        }
    }
    String::new()
}

unsafe fn hicon_to_base64_png(hicon: HICON) -> Result<String, String> {
    let mut icon_info = ICONINFO::default();
    if GetIconInfo(hicon, &mut icon_info).is_err() {
        return Err("GetIconInfo failed".into());
    }

    let hbm_color = icon_info.hbmColor;
    let hbm_mask = icon_info.hbmMask;

    let cleanup = |hcolor: HBITMAP, hmask: HBITMAP| {
        if hcolor.0 != 0 {
            let _ = DeleteObject(hcolor);
        }
        if hmask.0 != 0 {
            let _ = DeleteObject(hmask);
        }
    };

    if hbm_color.0 == 0 {
        cleanup(hbm_color, hbm_mask);
        return Err("No color bitmap in icon".into());
    }

    let mut bm = BITMAP::default();
    if GetObjectW(
        windows::Win32::Graphics::Gdi::HGDIOBJ(hbm_color.0),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut _ as *mut _),
    ) == 0
    {
        cleanup(hbm_color, hbm_mask);
        return Err("GetObjectW failed".into());
    }

    let width = bm.bmWidth as u32;
    let height = bm.bmHeight as u32;

    if width == 0 || height == 0 {
        cleanup(hbm_color, hbm_mask);
        return Err("Invalid icon dimensions".into());
    }

    let hdc = CreateCompatibleDC(HDC(0));
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32), // Top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut buffer: Vec<u8> = vec![0u8; (width * height * 4) as usize];
    let lines = GetDIBits(
        hdc,
        hbm_color,
        0,
        height,
        Some(buffer.as_mut_ptr() as *mut _),
        &mut bmi,
        DIB_RGB_COLORS,
    );

    let _ = DeleteDC(hdc);
    cleanup(hbm_color, hbm_mask);

    if lines == 0 {
        return Err("GetDIBits failed".into());
    }

    // BGRA -> RGBA conversion
    let mut has_nonzero_alpha = false;
    for chunk in buffer.chunks_exact(4) {
        if chunk[3] > 0 {
            has_nonzero_alpha = true;
            break;
        }
    }

    let mut rgba_buffer = Vec::with_capacity(buffer.len());
    for chunk in buffer.chunks_exact(4) {
        let b = chunk[0];
        let g = chunk[1];
        let r = chunk[2];
        let a = if has_nonzero_alpha { chunk[3] } else { 255 };
        rgba_buffer.push(r);
        rgba_buffer.push(g);
        rgba_buffer.push(b);
        rgba_buffer.push(a);
    }

    let mut png_bytes = Vec::new();
    let img = image::RgbaImage::from_raw(width, height, rgba_buffer)
        .ok_or_else(|| "Failed to create image from raw pixels".to_string())?;

    let mut cursor = std::io::Cursor::new(&mut png_bytes);
    img.write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode PNG: {}", e))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    Ok(b64)
}
