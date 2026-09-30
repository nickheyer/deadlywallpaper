//! Wallpaper Engine support: its project format, its Steam Workshop, and the scene, video,
//! web and application wallpapers found there.

pub mod condition;
pub mod pkg;
pub mod project;
pub mod steam;
pub mod vdf;
pub mod workshop;

/// The user's language as one of Wallpaper Engine's locale codes (`en-us`, `zh-chs`, ...).
pub fn language() -> String {
    to_we_locale(&system_locale())
}

#[cfg(windows)]
fn system_locale() -> String {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    // SAFETY: the buffer is LOCALE_NAME_MAX_LENGTH wide as the API requires.
    let n = unsafe { GetUserDefaultLocaleName(&mut buf) };
    if n > 1 {
        String::from_utf16_lossy(&buf[..(n as usize - 1)])
    } else {
        "en-US".into()
    }
}

#[cfg(not(windows))]
fn system_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.trim().is_empty() && v != "C" && v != "POSIX")
        .unwrap_or_else(|| "en_US".into())
}

/// Map a POSIX or BCP 47 locale to Wallpaper Engine's code list.
pub fn to_we_locale(locale: &str) -> String {
    let base = locale.split(['.', '@']).next().unwrap_or("").replace('_', "-");
    let mut parts = base.split('-');
    let lang = parts.next().unwrap_or("en").to_ascii_lowercase();
    let region = parts
        .find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
        .map(str::to_ascii_lowercase);
    let script = base
        .split('-')
        .find(|p| p.len() == 4)
        .map(str::to_ascii_lowercase);
    if lang == "zh" {
        return match (script.as_deref(), region.as_deref()) {
            (Some("hant"), _) | (None, Some("tw" | "hk" | "mo")) => "zh-cht".into(),
            _ => "zh-chs".into(),
        };
    }
    const KNOWN: &[&str] = &[
        "ar-sa", "be-by", "bg-bg", "cs-cz", "da-dk", "de-de", "el-gr", "en-us", "es-es",
        "eu-es", "fa-ir", "fi-fi", "fr-fr", "he-il", "hu-hu", "id-id", "it-it", "ja-jp",
        "ko-kr", "lt-lt", "nb-no", "nl-nl", "pl-pl", "pt-br", "pt-pt", "ro-ro", "ru-ru",
        "sk-sk", "sl-si", "sv-se", "th-th", "tr-tr", "uk-ua", "vi-vn",
    ];
    if let Some(r) = &region {
        let exact = format!("{lang}-{r}");
        if KNOWN.contains(&exact.as_str()) {
            return exact;
        }
    }
    KNOWN
        .iter()
        .find(|k| k.starts_with(&format!("{lang}-")))
        .map(|k| k.to_string())
        .unwrap_or_else(|| "en-us".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_map_to_wallpaper_engine_codes() {
        assert_eq!(to_we_locale("en_US.UTF-8"), "en-us");
        assert_eq!(to_we_locale("en_GB.UTF-8"), "en-us");
        assert_eq!(to_we_locale("pt_BR"), "pt-br");
        assert_eq!(to_we_locale("pt-PT"), "pt-pt");
        assert_eq!(to_we_locale("de_AT.UTF-8@euro"), "de-de");
        assert_eq!(to_we_locale("zh_CN"), "zh-chs");
        assert_eq!(to_we_locale("zh-Hant-TW"), "zh-cht");
        assert_eq!(to_we_locale("zh_TW"), "zh-cht");
        assert_eq!(to_we_locale("nb_NO"), "nb-no");
        assert_eq!(to_we_locale("xx"), "en-us");
        assert_eq!(to_we_locale(""), "en-us");
    }
}
