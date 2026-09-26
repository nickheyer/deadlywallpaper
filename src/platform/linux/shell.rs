//! The presenter in use: Plasma renders wallpapers itself through the wallpaper plugin;
//! every other desktop gets GTK canvases behind its windows.

use crate::error::Result;
use crate::geom::Rect;
use crate::ipc::Capabilities;
use crate::model::Display;
use crate::platform::ShellApi;
use crate::platform::linux::{canvas, plasma};

pub enum Shell {
    Plasma(plasma::Shell),
    Canvas(canvas::Shell),
}

pub enum Slot {
    Plasma(plasma::Slot),
    Canvas(canvas::Slot),
}

impl Shell {
    pub fn presenter(&self) -> String {
        self.capabilities().presenter
    }
}

impl ShellApi for Shell {
    type Slot = Slot;

    fn spans_displays(&self) -> bool {
        match self {
            Shell::Plasma(s) => s.spans_displays(),
            Shell::Canvas(s) => s.spans_displays(),
        }
    }

    fn sync_displays(&mut self, displays: &[Display]) -> Result<bool> {
        match self {
            Shell::Plasma(s) => s.sync_displays(displays),
            Shell::Canvas(s) => s.sync_displays(displays),
        }
    }

    fn slot(&mut self, display: &Display, region: Rect) -> Result<Slot> {
        match self {
            Shell::Plasma(s) => s.slot(display, region).map(Slot::Plasma),
            Shell::Canvas(s) => s.slot(display, region).map(Slot::Canvas),
        }
    }

    fn settle(&mut self) {
        match self {
            Shell::Plasma(s) => s.settle(),
            Shell::Canvas(s) => s.settle(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        match self {
            Shell::Plasma(s) => s.capabilities(),
            Shell::Canvas(s) => s.capabilities(),
        }
    }
}
