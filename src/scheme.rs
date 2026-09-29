//! The system colour scheme, used to pick the tray icon variant that reads on the panel.

#[cfg(not(windows))]
use crate::msg::Msg;
use crate::platform::MsgSender;
#[cfg(not(windows))]
use crate::platform::MsgSenderApi;

#[cfg(target_os = "linux")]
pub fn prefers_dark() -> bool {
    portal_dark().unwrap_or_else(kdeglobals_dark)
}

/// The desktop portal's appearance setting: 1 means dark, 2 light, 0 no preference.
#[cfg(target_os = "linux")]
fn portal_dark() -> Option<bool> {
    use zbus::zvariant::{OwnedValue, Value};
    let conn = zbus::blocking::Connection::session().ok()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .ok()?;
    let value: OwnedValue =
        match proxy.call("ReadOne", &("org.freedesktop.appearance", "color-scheme")) {
            Ok(v) => v,
            Err(_) => {
                let nested: OwnedValue = proxy
                    .call("Read", &("org.freedesktop.appearance", "color-scheme"))
                    .ok()?;
                match Value::from(nested) {
                    Value::Value(inner) => OwnedValue::try_from(*inner).ok()?,
                    other => OwnedValue::try_from(other).ok()?,
                }
            }
        };
    scheme_value(&Value::from(value))
}

#[cfg(target_os = "linux")]
fn scheme_value(v: &zbus::zvariant::Value<'_>) -> Option<bool> {
    use zbus::zvariant::Value;
    match v {
        Value::U32(n) => Some(*n == 1),
        Value::I32(n) => Some(*n == 1),
        Value::U64(n) => Some(*n == 1),
        Value::Value(inner) => scheme_value(inner),
        _ => None,
    }
}

/// KDE without a settings portal: the active colour scheme's name.
#[cfg(target_os = "linux")]
fn kdeglobals_dark() -> bool {
    let Some(config) = dirs::config_dir() else {
        return false;
    };
    std::fs::read_to_string(config.join("kdeglobals"))
        .unwrap_or_default()
        .lines()
        .any(|l| {
            l.trim_start().starts_with("ColorScheme=") && l.to_ascii_lowercase().contains("dark")
        })
}

/// Report colour scheme changes as [`Msg::ColorScheme`].
#[cfg(target_os = "linux")]
pub fn watch(tx: MsgSender) {
    let _ = std::thread::Builder::new()
        .name("color-scheme".into())
        .spawn(move || {
            use zbus::zvariant::{OwnedValue, Value};
            let Ok(conn) = zbus::blocking::Connection::session() else {
                return;
            };
            let Ok(proxy) = zbus::blocking::Proxy::new(
                &conn,
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
            ) else {
                return;
            };
            let Ok(signals) = proxy.receive_signal("SettingChanged") else {
                return;
            };
            for message in signals {
                let Ok((namespace, key, value)) =
                    message.body().deserialize::<(String, String, OwnedValue)>()
                else {
                    continue;
                };
                if namespace == "org.freedesktop.appearance" && key == "color-scheme" {
                    if let Some(dark) = scheme_value(&Value::from(value)) {
                        tx.send(Msg::ColorScheme { dark });
                    }
                }
            }
        });
}

#[cfg(windows)]
pub fn prefers_dark() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::w;
    let mut value: u32 = 1;
    let mut size: u32 = 4;
    // SAFETY: the output buffer and its size are valid for the call.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    status.is_ok() && value == 0
}

/// Windows delivers theme changes as `WM_SETTINGCHANGE` to the daemon's message window, which
/// posts [`Msg::ColorScheme`] itself; nothing else needs to run.
#[cfg(windows)]
pub fn watch(_tx: MsgSender) {}

#[cfg(target_os = "macos")]
pub fn prefers_dark() -> bool {
    use objc2_foundation::{NSString, NSUserDefaults};
    let defaults = NSUserDefaults::standardUserDefaults();
    defaults
        .stringForKey(&NSString::from_str("AppleInterfaceStyle"))
        .is_some_and(|s| s.to_string().eq_ignore_ascii_case("dark"))
}

#[cfg(target_os = "macos")]
pub fn watch(tx: MsgSender) {
    use block2::RcBlock;
    use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSString};
    use std::ptr::NonNull;
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        tx.send(Msg::ColorScheme {
            dark: prefers_dark(),
        })
    });
    // SAFETY: distributed notification observer with a retained block; the token is kept
    // for the life of the daemon.
    let token = unsafe {
        NSDistributedNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(&NSString::from_str(
                "AppleInterfaceThemeChangedNotification",
            )),
            None,
            None,
            &block,
        )
    };
    std::mem::forget(token);
}
