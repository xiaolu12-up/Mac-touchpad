use std::sync::atomic::{AtomicBool, AtomicU32, AtomicI32, Ordering};
use std::sync::mpsc;
use windows::Win32::Foundation::*;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Magic marker in dwExtraInfo for scroll events we generate via SendInput.
/// The hook sees this and passes them through instead of suppressing.
pub const SYNTHETIC_SCROLL_MARKER: usize = 0xFACECAFE;

/// Set to true while 2+ fingers are on the touchpad with smooth scroll enabled.
pub static TOUCHPAD_SCROLLING: AtomicBool = AtomicBool::new(false);

/// Controls whether mouse wheel smooth scrolling is enabled.
pub static SMOOTH_SCROLL_ENABLED: AtomicBool = AtomicBool::new(true);

/// Tracks whether the synthetic touchpad device is initialized and active (Windows 11 exclusive)
pub static SYNTHETIC_DEVICE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Global channel sender to pass events to core message loop.
pub static CORE_SENDER: std::sync::Mutex<Option<crate::window::CoreSender>> = std::sync::Mutex::new(None);

static mut HOOK_HANDLE: HHOOK = HHOOK(0);
/// Thread ID of the dedicated hook thread (for PostThreadMessageW shutdown).
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);

/// Spawn a dedicated lightweight thread to host the WH_MOUSE_LL hook.
///
/// # Why a dedicated thread?
/// WH_MOUSE_LL callbacks run on the thread that called SetWindowsHookExW,
/// but only while it is pumping messages. Hosting the hook on the main
/// processing thread means every mouse event waits for our heavier logic
/// to complete, causing system-wide mouse lag — especially in debug builds.
/// A dedicated thread with a minimal GetMessage loop is always available.
///
/// Blocks until the hook is confirmed installed.
pub fn start_hook_thread() {
    let (ready_tx, ready_rx) = mpsc::channel::<bool>();

    std::thread::Builder::new()
        .name("scroll-hook".into())
        .spawn(move || unsafe {
            // Store thread ID so stop_hook_thread() can send WM_QUIT
            HOOK_THREAD_ID.store(GetCurrentThreadId(), Ordering::SeqCst);

            match SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), None, 0) {
                Ok(h) => {
                    HOOK_HANDLE = h;
                    tracing::info!("Scroll hook installed on dedicated thread");
                    let _ = ready_tx.send(true);
                }
                Err(e) => {
                    tracing::error!("Failed to install scroll hook: {}", e);
                    HOOK_THREAD_ID.store(0, Ordering::SeqCst);
                    let _ = ready_tx.send(false);
                    return;
                }
            }

            // Minimal message pump: GetMessageW blocks until a message arrives.
            // Windows delivers WH_MOUSE_LL callbacks while this thread is here.
            // This thread does nothing else, so it's always ready to respond.
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND(0), 0, 0).as_bool() {
                // Low-level hooks don't need TranslateMessage/DispatchMessage —
                // they're dispatched directly by the hook mechanism.
            }

            // Cleanup
            if !HOOK_HANDLE.is_invalid() {
                let _ = UnhookWindowsHookEx(HOOK_HANDLE);
                HOOK_HANDLE = HHOOK(0);
            }
            HOOK_THREAD_ID.store(0, Ordering::SeqCst);
            tracing::info!("Scroll hook thread exited");
        })
        .expect("Failed to spawn scroll-hook thread");

    // Wait for the hook to be confirmed installed before returning
    let _ = ready_rx.recv_timeout(std::time::Duration::from_secs(2));
}

/// Signal the dedicated hook thread to exit and uninstall the hook.
pub fn stop_hook_thread() {
    let tid = HOOK_THREAD_ID.load(Ordering::SeqCst);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe extern "system" fn low_level_mouse_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let msg = w_param.0 as u32;

        // Block touch-promoted mouse events to prevent cursor jumping and disappearing.
        // We cast dwExtraInfo to u32 to prevent sign-extension mismatches on 64-bit systems.
        if msg == WM_MOUSEMOVE || msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP
            || msg == WM_RBUTTONDOWN || msg == WM_RBUTTONUP || msg == WM_MBUTTONDOWN || msg == WM_MBUTTONUP
        {
            let info = l_param.0 as *const MSLLHOOKSTRUCT;
            if !info.is_null() {
                let info_ref = &*info;
                let is_touch = ((info_ref.dwExtraInfo as u32) & 0xFFFFFF00) == 0xFF515700;
                
                if is_touch {
                    return LRESULT(1); // Swallow
                }
            }
        }

        if msg == WM_MOUSEWHEEL || msg == WM_MOUSEHWHEEL {
            let info = l_param.0 as *const MSLLHOOKSTRUCT;
            if !info.is_null() {
                let info_ref = &*info;

                // 1. If it's our own synthetic scroll event, let it pass.
                if info_ref.dwExtraInfo == SYNTHETIC_SCROLL_MARKER {
                    return CallNextHookEx(None, n_code, w_param, l_param);
                }

                // 2. If touchpad scrolling is active, suppress other scroll events.
                if TOUCHPAD_SCROLLING.load(Ordering::Relaxed) {
                    return LRESULT(1);
                }

                // 3. If smooth scroll is enabled and Win11 synthetic device is active, intercept physical scrolls.
                if SMOOTH_SCROLL_ENABLED.load(Ordering::Relaxed) && SYNTHETIC_DEVICE_ACTIVE.load(Ordering::Relaxed) {
                    let flags = info_ref.flags;
                    let is_injected = (flags & 1) != 0 || (flags & 2) != 0;

                    if is_injected {
                        // Pass through other programmatic scroll events.
                        return CallNextHookEx(None, n_code, w_param, l_param);
                    }

                    // Intercept physical mouse wheel scrolls.
                    let mouse_data = info_ref.mouseData;
                    let delta = (mouse_data >> 16) as i16 as i32;
                    let horizontal = msg == WM_MOUSEHWHEEL;
                    if let Ok(guard) = CORE_SENDER.lock() {
                        if let Some(ref sender) = *guard {
                            let _ = sender.send(crate::window::CoreCommand::MouseWheel(delta, horizontal));
                        }
                    }
                    return LRESULT(1); // Suppress native event
                }
            }
        }
    }
    CallNextHookEx(None, n_code, w_param, l_param)
}
