/// Global overlay indicator for brightness/volume feedback.
///
/// The app crate initializes this at startup. The core crate's keyboard
/// module calls show_brightness/show_volume to display the overlay.

use std::sync::Mutex;

/// Overlay display function type.
type OverlayFn = Box<dyn Fn(i32, bool) + Send + Sync>;

static OVERLAY: Mutex<Option<OverlayFn>> = Mutex::new(None);

/// Register the overlay function. Called once at startup by the app crate.
pub fn set_overlay_fn(f: impl Fn(i32, bool) + Send + Sync + 'static) {
    if let Ok(mut guard) = OVERLAY.lock() {
        *guard = Some(Box::new(f));
    }
}

/// Show a brightness/volume overlay bar.
/// `value`: 0-100, `is_brightness`: true for brightness, false for volume.
pub fn show_overlay(value: i32, is_brightness: bool) {
    if let Ok(guard) = OVERLAY.lock() {
        if let Some(ref f) = *guard {
            f(value, is_brightness);
        }
    }
}
