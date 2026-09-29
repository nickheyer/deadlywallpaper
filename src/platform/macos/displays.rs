use crate::geom::Rect;
use crate::model::Display;
use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_core_graphics::{CGDisplayModelNumber, CGDisplaySerialNumber, CGDisplayVendorNumber};
use objc2_foundation::{NSNumber, NSString};

/// Displays in top-left global coordinates (Quartz), converted from Cocoa's bottom-left frames.
pub fn list() -> Vec<Display> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Vec::new();
    };
    let screens = NSScreen::screens(mtm);
    let Some(primary) = screens.iter().next() else {
        return Vec::new();
    };
    let primary_h = primary.frame().size.height;
    let convert = |f: objc2_foundation::NSRect| -> Rect {
        Rect::new(
            f.origin.x.round() as i32,
            (primary_h - (f.origin.y + f.size.height)).round() as i32,
            f.size.width.round() as i32,
            f.size.height.round() as i32,
        )
    };
    let mut out = Vec::new();
    for (i, screen) in screens.iter().enumerate() {
        let desc = screen.deviceDescription();
        let number = desc
            .objectForKey(&NSString::from_str("NSScreenNumber"))
            .and_then(|o| o.downcast::<NSNumber>().ok())
            .map(|n| n.unsignedIntValue())
            .unwrap_or(0);
        let (vendor, model, serial) = (
            CGDisplayVendorNumber(number),
            CGDisplayModelNumber(number),
            CGDisplaySerialNumber(number),
        );
        let stable = format!("{vendor:x}-{model:x}-{serial:x}");
        let ordinal = out
            .iter()
            .filter(|d: &&Display| d.id.starts_with(&stable))
            .count();
        let id = if ordinal == 0 {
            stable
        } else {
            format!("{stable}-{}", ordinal + 1)
        };
        let name = screen.localizedName().to_string();
        out.push(Display {
            id,
            name: if name.is_empty() {
                format!("Display {}", i + 1)
            } else {
                name
            },
            rect: convert(screen.frame()),
            workarea: convert(screen.visibleFrame()),
            scale: screen.backingScaleFactor(),
            primary: i == 0,
        });
    }
    out
}

/// Height of the primary screen, needed to flip Cocoa coordinates.
pub fn primary_height() -> f64 {
    MainThreadMarker::new()
        .and_then(|mtm| {
            NSScreen::screens(mtm)
                .iter()
                .next()
                .map(|s| s.frame().size.height)
        })
        .unwrap_or(0.0)
}
