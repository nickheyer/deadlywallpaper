//! wlroots-style compositors: `zwlr_foreign_toplevel_manager_v1` reports window states and the
//! outputs each window occupies.

use crate::msg::Msg;
use crate::platform::linux::MsgSender;
use crate::platform::{MsgSenderApi, Snapshot, WindowInfo, WindowPlacement};
use std::collections::HashMap;
use wayland_backend::client::ObjectId;
use wayland_client::protocol::{wl_output, wl_registry};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::{
    self, ZwlrForeignToplevelHandleV1,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::{
    self, ZwlrForeignToplevelManagerV1,
};

#[derive(Default)]
struct Toplevel {
    app: String,
    outputs: Vec<ObjectId>,
    fullscreen: bool,
    maximized: bool,
    minimized: bool,
    activated: bool,
}

#[derive(Default)]
struct State {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    outputs: HashMap<ObjectId, (i32, i32)>,
    toplevels: HashMap<ObjectId, Toplevel>,
    dirty: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "zwlr_foreign_toplevel_manager_v1" => {
                    state.manager = Some(registry.bind(name, version.min(3), qh, ()));
                }
                "wl_output" => {
                    let output: wl_output::WlOutput = registry.bind(name, version.min(2), qh, ());
                    state.outputs.insert(output.id(), (0, 0));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Geometry { x, y, .. } = event {
            state.outputs.insert(output.id(), (x, y));
            state.dirty = true;
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            state.toplevels.insert(toplevel.id(), Toplevel::default());
        }
    }

    wayland_client::event_created_child!(State, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        let id = handle.id();
        match event {
            Event::Closed => {
                state.toplevels.remove(&id);
                state.dirty = true;
                handle.destroy();
                return;
            }
            Event::Done => {
                state.dirty = true;
                return;
            }
            _ => {}
        }
        let Some(t) = state.toplevels.get_mut(&id) else {
            return;
        };
        match event {
            Event::AppId { app_id } => t.app = app_id,
            Event::OutputEnter { output } => t.outputs.push(output.id()),
            Event::OutputLeave { output } => t.outputs.retain(|o| *o != output.id()),
            Event::State { state: bytes } => {
                let flags: Vec<u32> = bytes
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                t.maximized = flags.contains(&0);
                t.minimized = flags.contains(&1);
                t.activated = flags.contains(&2);
                t.fullscreen = flags.contains(&3);
            }
            _ => {}
        }
    }
}

/// Start the monitor thread; `false` when the compositor lacks the protocol.
pub fn start(tx: MsgSender) -> bool {
    let Ok(conn) = Connection::connect_to_env() else {
        return false;
    };
    let mut queue = conn.new_event_queue::<State>();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());
    let mut state = State::default();
    if queue.roundtrip(&mut state).is_err() || state.manager.is_none() {
        return false;
    }
    std::thread::Builder::new()
        .name("wlr-toplevel".into())
        .spawn(move || {
            loop {
                if queue.blocking_dispatch(&mut state).is_err() {
                    log::warn!("foreign toplevel monitor disconnected");
                    return;
                }
                if state.dirty {
                    state.dirty = false;
                    let windows = state
                        .toplevels
                        .values()
                        .filter(|t| !t.minimized)
                        .map(|t| WindowInfo {
                            placement: WindowPlacement::Outputs(
                                t.outputs
                                    .iter()
                                    .filter_map(|o| state.outputs.get(o).copied())
                                    .collect(),
                            ),
                            fullscreen: t.fullscreen,
                            maximized: t.maximized,
                            focused: t.activated,
                            app: t.app.clone(),
                            pid: None,
                        })
                        .collect();
                    tx.send(Msg::Windows(Snapshot { windows }));
                }
            }
        })
        .is_ok()
}
