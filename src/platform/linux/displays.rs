use crate::geom::Rect;
use crate::model::Display;
use crate::model::display::composite_id;
use crate::msg::Msg;
use crate::platform::MsgSenderApi;
use crate::platform::linux::MsgSender;
use gdk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

fn rect(r: gdk::Rectangle) -> Rect {
    Rect::new(r.x(), r.y(), r.width(), r.height())
}

fn monitors(display: &gdk::Display) -> Vec<gdk::Monitor> {
    (0..display.n_monitors())
        .filter_map(|i| display.monitor(i))
        .collect()
}

pub fn list(display: &gdk::Display) -> Vec<Display> {
    let mut raw: Vec<(String, String, gdk::Monitor)> = monitors(display)
        .into_iter()
        .map(|m| {
            let make = m.manufacturer().map(|s| s.to_string()).unwrap_or_default();
            let model = m.model().map(|s| s.to_string()).unwrap_or_default();
            (make, model, m)
        })
        .collect();
    raw.sort_by_key(|(_, _, m)| (m.geometry().x(), m.geometry().y()));
    let mut out = Vec::with_capacity(raw.len());
    for (i, (make, model, m)) in raw.iter().enumerate() {
        let ordinal = raw[..i]
            .iter()
            .filter(|(mk, md, _)| mk == make && md == model)
            .count();
        let name = if model.is_empty() {
            format!("Display {}", i + 1)
        } else {
            model.clone()
        };
        out.push(Display {
            id: composite_id(make, model, ordinal),
            name,
            rect: rect(m.geometry()),
            workarea: rect(m.workarea()),
            scale: m.scale_factor() as f64,
            primary: m.is_primary(),
        });
    }
    if !out.iter().any(|d| d.primary) {
        if let Some(d) = out.iter_mut().find(|d| d.rect.x == 0 && d.rect.y == 0) {
            d.primary = true;
        } else if let Some(d) = out.first_mut() {
            d.primary = true;
        }
    }
    out
}

/// The GDK monitor whose geometry matches `display`.
pub fn monitor_for(display: &gdk::Display, target: &Display) -> Option<gdk::Monitor> {
    monitors(display)
        .into_iter()
        .find(|m| rect(m.geometry()) == target.rect)
}

/// Report the display list whenever monitors are added, removed, or reconfigured.
pub fn watch(display: &gdk::Display, tx: MsgSender) {
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    let schedule = {
        let display = display.clone();
        let pending = pending.clone();
        Rc::new(move || {
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
            let display = display.clone();
            let tx = tx.clone();
            let p2 = pending.clone();
            let id = glib::timeout_add_local_once(Duration::from_millis(400), move || {
                p2.borrow_mut().take();
                tx.send(Msg::Displays(list(&display)));
            });
            *pending.borrow_mut() = Some(id);
        })
    };
    let hook = {
        let schedule = schedule.clone();
        Rc::new(move |m: &gdk::Monitor| {
            let (a, b, c) = (schedule.clone(), schedule.clone(), schedule.clone());
            m.connect_geometry_notify(move |_| a());
            m.connect_workarea_notify(move |_| b());
            m.connect_scale_factor_notify(move |_| c());
        })
    };
    for m in monitors(display) {
        hook(&m);
    }
    let (h2, s2) = (hook.clone(), schedule.clone());
    display.connect_monitor_added(move |_, m| {
        h2(m);
        s2();
    });
    let s3 = schedule.clone();
    display.connect_monitor_removed(move |_, _| s3());
}
