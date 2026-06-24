use winreg::enums::*;
use winreg::RegKey;

const REGISTRY_RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APP_NAME: &str = "MacTouchpad";

/// Set or remove the auto-start registry entry.
pub fn set_startup_enabled(enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu.create_subkey(REGISTRY_RUN_KEY)?;

    if enabled {
        let exe_path = std::env::current_exe()?;
        let path_str = exe_path.to_string_lossy().to_string();
        run_key.set_value(APP_NAME, &path_str)?;
        tracing::info!("Auto-start enabled: {}", path_str);
    } else {
        match run_key.delete_value(APP_NAME) {
            Ok(()) => {
                tracing::info!("Auto-start disabled");
            }
            Err(_) => {
                // Key didn't exist, that's fine
            }
        }
    }

    Ok(())
}

/// Check if auto-start is currently enabled.
pub fn is_startup_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match hkcu.open_subkey_with_flags(REGISTRY_RUN_KEY, KEY_READ) {
        Ok(run_key) => run_key.get_value::<String, _>(APP_NAME).is_ok(),
        Err(_) => false,
    }
}

/// Convenience: enable auto-start.
pub fn enable_auto_start() -> Result<(), Box<dyn std::error::Error>> {
    set_startup_enabled(true)
}

/// Convenience: disable auto-start.
pub fn disable_auto_start() -> Result<(), Box<dyn std::error::Error>> {
    set_startup_enabled(false)
}
