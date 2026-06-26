use winreg::enums::*;
use winreg::RegKey;

const REGISTRY_RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APP_NAME: &str = "MacTouchpad";

/// Set or remove the auto-start registry entry and task.
pub fn set_startup_enabled(enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Clean up old registry key if present, so we don't have duplicate/broken entries in Task Manager's Startup tab.
    if let Ok(hkcu) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(REGISTRY_RUN_KEY, KEY_WRITE) {
        let _ = hkcu.delete_value(APP_NAME);
    }

    use std::os::windows::process::CommandExt;
    if enabled {
        let exe_path = std::env::current_exe()?;
        let path_str = exe_path.to_string_lossy().to_string();
        let task_run_cmd = format!("\"{}\" --autostart", path_str);
        
        let output = std::process::Command::new("schtasks.exe")
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .args([
                "/create",
                "/tn",
                APP_NAME,
                "/tr",
                &task_run_cmd,
                "/sc",
                "onlogon",
                "/rl",
                "highest",
                "/f",
            ])
            .output()?;
        
        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr).into_owned();
            return Err(format!("创建计划任务失败: {}", err_msg).into());
        }
        tracing::info!("Auto-start enabled via Task Scheduler: {}", task_run_cmd);
    } else {
        let output = std::process::Command::new("schtasks.exe")
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .args(["/delete", "/tn", APP_NAME, "/f"])
            .output()?;
        
        if !output.status.success() {
            // Task might not exist, that's fine
        }
        tracing::info!("Auto-start disabled");
    }

    Ok(())
}

/// Check if auto-start is currently enabled.
pub fn is_startup_enabled() -> bool {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("schtasks.exe")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(["/query", "/tn", APP_NAME])
        .output();
    
    match output {
        Ok(out) => out.status.success(),
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
