use crate::error::{Error, Result};
use crate::msg::{Msg, TrayAction};
use crate::platform::{MsgSender, MsgSenderApi};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub struct Tray {
    icon: TrayIcon,
    pause: CheckMenuItem,
}

/// The white circle keeps the flower visible on either panel theme.
fn icon(dark: bool) -> Result<Icon> {
    let bytes: &[u8] = if dark {
        include_bytes!("../assets/tray-dark.png")
    } else {
        include_bytes!("../assets/tray-light.png")
    };
    let img = image::load_from_memory(bytes)
        .map_err(|e| Error::Platform(format!("tray icon: {e}")))?
        .into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).map_err(|e| Error::Platform(format!("tray icon: {e}")))
}

impl Tray {
    pub fn new(tx: MsgSender, paused: bool, dark: bool) -> Result<Tray> {
        let menu = Menu::new();
        let open = MenuItem::new("Open Deadly Wallpaper", true, None);
        let pause = CheckMenuItem::new("Pause wallpapers", true, paused, None);
        let random = MenuItem::new("Shuffle wallpapers", true, None);
        let close = MenuItem::new("Close wallpapers", true, None);
        let quit = MenuItem::new("Quit", true, None);
        menu.append_items(&[
            &open,
            &PredefinedMenuItem::separator(),
            &pause,
            &random,
            &close,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| Error::Platform(format!("tray menu: {e}")))?;
        let ids = [
            (open.id().clone(), TrayAction::OpenUi),
            (pause.id().clone(), TrayAction::TogglePause),
            (close.id().clone(), TrayAction::CloseAll),
            (random.id().clone(), TrayAction::Random),
            (quit.id().clone(), TrayAction::Quit),
        ];
        let menu_tx = tx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Some((_, action)) = ids.iter().find(|(id, _)| *id == e.id) {
                menu_tx.send(Msg::Tray(*action));
            }
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::DoubleClick { .. } = e {
                tx.send(Msg::Tray(TrayAction::OpenUi));
            }
        }));
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(crate::paths::APP_NAME)
            .with_icon(icon(dark)?)
            .build()
            .map_err(|e| Error::Platform(format!("tray: {e}")))?;
        Ok(Tray { icon, pause })
    }

    pub fn set_paused(&self, paused: bool) {
        self.pause.set_checked(paused);
    }

    pub fn set_dark(&self, dark: bool) {
        match icon(dark) {
            Ok(i) => {
                if let Err(e) = self.icon.set_icon(Some(i)) {
                    log::warn!("tray icon: {e}");
                }
            }
            Err(e) => log::warn!("{e}"),
        }
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.icon.set_visible(visible);
    }
}
