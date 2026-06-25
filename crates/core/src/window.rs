use std::sync::{mpsc, Mutex, atomic::{AtomicBool, Ordering}};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::config::Config;
use crate::contacts::manager::{ContactResult, ContactsManager};
use crate::gesture::drag::TimerAction;
use crate::gesture::engine::GestureEngine;
use crate::hid::parser;
use crate::hid::raw_input::RawInputManager;
use crate::input::wheel_hook;

pub enum CoreCommand {
    UpdateConfig(Config),
    MouseWheel(i32, bool),
    Shutdown,
}

/// Shared flag: set to true when config needs reload.
static CONFIG_DIRTY: AtomicBool = AtomicBool::new(false);

/// Shared config for cross-thread update.
static SHARED_CONFIG: Mutex<Option<Config>> = Mutex::new(None);

#[derive(Clone)]
pub struct CoreSender {
    tx: mpsc::Sender<CoreCommand>,
}

impl CoreSender {
    pub fn send(&self, cmd: CoreCommand) -> std::result::Result<(), mpsc::SendError<CoreCommand>> {
        match &cmd {
            CoreCommand::UpdateConfig(config) => {
                // Store config in shared location and set dirty flag
                if let Ok(mut shared) = SHARED_CONFIG.lock() {
                    *shared = Some(config.clone());
                }
                crate::input::wheel_hook::SMOOTH_SCROLL_ENABLED.store(config.smooth_scroll_enabled, Ordering::Relaxed);
                CONFIG_DIRTY.store(true, Ordering::Release);
                // Also send via channel as fallback
                let _ = self.tx.send(cmd);
                Ok(())
            }
            CoreCommand::MouseWheel(_, _) | CoreCommand::Shutdown => {
                let _ = self.tx.send(cmd);
                Ok(())
            }
        }
    }
}

const TIMER_DRAG_END: usize = 1;

struct WindowState {
    engine: GestureEngine,
    raw_input: RawInputManager,
    contacts_manager: ContactsManager,
    command_rx: mpsc::Receiver<CoreCommand>,
    hwnd: HWND,
    scroller: crate::input::smooth_scroll::SmoothScroller,
}

pub fn start_message_loop(config: Config, ready_tx: Option<mpsc::Sender<()>>) -> CoreSender {
    let (command_tx, command_rx) = mpsc::channel();
    let (hwnd_tx, hwnd_rx) = mpsc::channel::<HWND>();

    crate::input::wheel_hook::SMOOTH_SCROLL_ENABLED.store(config.smooth_scroll_enabled, Ordering::Relaxed);

    std::thread::Builder::new()
        .name("msg-loop".into())
        .spawn(move || run_message_loop(config, command_rx, ready_tx, hwnd_tx))
        .expect("Failed to spawn msg-loop");

    let _hwnd = hwnd_rx.recv().expect("HWND not received");
    let sender = CoreSender { tx: command_tx };
    if let Ok(mut guard) = crate::input::wheel_hook::CORE_SENDER.lock() {
        *guard = Some(sender.clone());
    }
    sender
}

fn run_message_loop(
    config: Config,
    command_rx: mpsc::Receiver<CoreCommand>,
    ready_tx: Option<mpsc::Sender<()>>,
    hwnd_tx: mpsc::Sender<HWND>,
) {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None).unwrap().0);
        let class_name = w!("MacTouchpadMsgWindow");

        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassExW(&wnd_class);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(), class_name, w!("MacTouchpad"),
            WINDOW_STYLE::default(), 0, 0, 0, 0,
            HWND_MESSAGE, HMENU(0), instance, None,
        );

        let _ = hwnd_tx.send(hwnd);

        // Start dedicated hook thread — keeps mouse responsive independently of
        // our processing load. Must be started BEFORE registering raw input.
        wheel_hook::start_hook_thread();

        let mut raw_input = RawInputManager::new();
        tracing::info!("Touchpad detected: {}", raw_input.exists_any());
        tracing::info!("Raw input registered: {}", raw_input.register_input(hwnd));

        let scroller = crate::input::smooth_scroll::SmoothScroller::new(
            config.smooth_scroll_speed,
            config.smooth_scroll_smoothing,
            config.smooth_scroll_deceleration,
            config.smooth_scroll_base_scale,
            config.smooth_scroll_max_delta,
            config.smooth_scroll_deadzone,
            config.smooth_scroll_tick_ms,
            config.natural_scroll,
        );

        let state = Box::new(WindowState {
            engine: GestureEngine::new(config),
            raw_input,
            contacts_manager: ContactsManager::new(),
            command_rx,
            hwnd,
            scroller,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

        if let Some(tx) = ready_tx { let _ = tx.send(()); }

        // Use PeekMessage + MsgWaitForMultipleObjects for responsive message loop
        let mut msg = MSG::default();
        let mut last_tick = std::time::Instant::now();
        loop {
            // Check for config updates (atomic flag, no lock needed for check)
            if CONFIG_DIRTY.swap(false, Ordering::Acquire) {
                if let Ok(mut shared) = SHARED_CONFIG.lock() {
                    if let Some(new_config) = shared.take() {
                        let sp = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
                        if !sp.is_null() {
                            (*sp).engine.update_config(new_config.clone());
                            (*sp).scroller.set_speed(new_config.smooth_scroll_speed);
                            (*sp).scroller.set_smoothing(new_config.smooth_scroll_smoothing);
                            (*sp).scroller.set_deceleration(new_config.smooth_scroll_deceleration);
                            (*sp).scroller.set_base_scale(new_config.smooth_scroll_base_scale);
                            (*sp).scroller.set_max_delta(new_config.smooth_scroll_max_delta);
                            (*sp).scroller.set_deadzone(new_config.smooth_scroll_deadzone);
                            (*sp).scroller.set_natural_scroll(new_config.natural_scroll);
                            (*sp).scroller.set_tick_ms(new_config.smooth_scroll_tick_ms);
                            tracing::info!("Config updated in engine and scroller (atomic)");
                        }
                    }
                }
            }

            // Check channel for shutdown/updates/mouse wheel commands
            {
                let sp = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
                if !sp.is_null() {
                    while let Ok(cmd) = (*sp).command_rx.try_recv() {
                        match cmd {
                            CoreCommand::Shutdown => {
                                drop(Box::from_raw(sp));
                                return;
                            }
                            CoreCommand::UpdateConfig(c) => {
                                (*sp).engine.update_config(c.clone());
                                (*sp).scroller.set_speed(c.smooth_scroll_speed);
                                (*sp).scroller.set_smoothing(c.smooth_scroll_smoothing);
                                (*sp).scroller.set_deceleration(c.smooth_scroll_deceleration);
                                (*sp).scroller.set_base_scale(c.smooth_scroll_base_scale);
                                (*sp).scroller.set_max_delta(c.smooth_scroll_max_delta);
                                (*sp).scroller.set_deadzone(c.smooth_scroll_deadzone);
                                (*sp).scroller.set_natural_scroll(c.natural_scroll);
                                (*sp).scroller.set_tick_ms(c.smooth_scroll_tick_ms);
                                tracing::info!("Config updated in engine and scroller (channel)");
                            }
                            CoreCommand::MouseWheel(delta, horizontal) => {
                                (*sp).scroller.add_scroll(delta, horizontal);
                            }
                        }
                    }
                }
            }

            // Tick smooth scroller (dynamic interval based on velocity)
            let now = std::time::Instant::now();
            {
                let sp = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
                if !sp.is_null() && now.duration_since(last_tick) >= std::time::Duration::from_millis((*sp).scroller.tick_interval_ms()) {
                    last_tick = now;
                    (*sp).scroller.tick();
                    (*sp).engine.check_timeouts();
                }
            }

            // Process Windows messages (non-blocking)
            if PeekMessageW(&mut msg, HWND(0), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT { break; }
                if msg.message == WM_TIMER && msg.wParam.0 == TIMER_DRAG_END {
                    let sp = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
                    if !sp.is_null() {
                        (*sp).engine.on_timer_fired();
                        let _ = KillTimer(hwnd, TIMER_DRAG_END);
                    }
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                // No messages — sleep briefly; hook is on a separate thread
                // so this sleep does not affect mouse responsiveness
                {
                    let sp = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
                    let sleep_ms = if !sp.is_null() { (*sp).scroller.tick_ms.min(1) } else { 1 };
                    std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
                }
            }
        }

        let sp = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
        if !sp.is_null() { drop(Box::from_raw(sp)); }

        // Stop the dedicated hook thread
        wheel_hook::stop_hook_thread();
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_INPUT => {
            let sp = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if !sp.is_null() { handle_wm_input(&mut *sp, lp); }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_INPUT_DEVICE_CHANGE => {
            let sp = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if !sp.is_null() {
                let exists = (*sp).raw_input.exists(HANDLE(lp.0 as isize));
                tracing::info!("Device change: exists={}", exists);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_DESTROY => { PostQuitMessage(0); LRESULT(0) }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn handle_wm_input(state: &mut WindowState, lparam: LPARAM) {
    let result = match parser::parse_input(lparam) { Some(r) => r, None => return };
    let hv = result.device.0 as isize;
    if !state.raw_input.devices.contains_key(&hv) && !state.raw_input.exists(result.device) {
        return;
    }
    let device_id = state.raw_input.get_device_info(hv)
        .map(|d| d.device_id.clone()).unwrap_or_else(|| "default".into());

    if let crate::contacts::manager::ContactResult::Complete(contacts) = state.contacts_manager.receive(result.contacts, result.contact_count) {
        state.engine.set_touchpad_ranges(result.x_range, result.y_range);
        match state.engine.on_touchpad_contact(&device_id, contacts) {
            TimerAction::StartTimer(ms) => { let _ = SetTimer(state.hwnd, TIMER_DRAG_END, ms, None); }
            TimerAction::StopTimer => { let _ = KillTimer(state.hwnd, TIMER_DRAG_END); }
            TimerAction::None => {}
        }
    }
}
