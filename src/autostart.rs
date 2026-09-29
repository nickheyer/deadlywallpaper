use crate::error::{Error, Result};

fn entry() -> Result<auto_launch::AutoLaunch> {
    let exe = std::env::current_exe()?;
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name(crate::paths::APP_ID)
        .set_app_path(&exe.to_string_lossy())
        .set_args(&["daemon"])
        .build()
        .map_err(|e| Error::Platform(format!("autostart: {e}")))
}

/// Register or remove the daemon's login autostart entry.
pub fn apply(enabled: bool) -> Result<()> {
    let e = entry()?;
    let current = e.is_enabled().unwrap_or(false);
    if enabled && !current {
        e.enable()
            .map_err(|x| Error::Platform(format!("enable autostart: {x}")))?;
    } else if !enabled && current {
        e.disable()
            .map_err(|x| Error::Platform(format!("disable autostart: {x}")))?;
    }
    Ok(())
}
