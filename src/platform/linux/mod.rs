pub mod canvas;
pub mod displays;
pub mod gl;
pub mod layer;
pub mod mainloop;
pub mod media_view;
pub mod monitor;
pub mod plasma;
pub mod program;
pub mod session;
pub mod shell;

pub use mainloop::{MainLoop, MsgSender, Runtime};
pub use shell::{Shell, Slot};

use glib::translate::ToGlibPtr;
use std::ffi::c_void;

unsafe extern "C" {
    fn gdk_wayland_display_get_wl_display(display: *mut gdk::ffi::GdkDisplay) -> *mut c_void;
    fn gdk_wayland_window_get_wl_surface(window: *mut gdk::ffi::GdkWindow) -> *mut c_void;
    fn gdk_wayland_window_set_use_custom_surface(window: *mut gdk::ffi::GdkWindow);
    fn gdk_wayland_monitor_get_wl_output(monitor: *mut gdk::ffi::GdkMonitor) -> *mut c_void;
    fn gdk_x11_display_get_xdisplay(display: *mut gdk::ffi::GdkDisplay) -> *mut c_void;
}

pub fn is_wayland(display: &gdk::Display) -> bool {
    use glib::prelude::ObjectExt;
    display.type_().name() == "GdkWaylandDisplay"
}

/// Raw `wl_display*` of a Wayland GDK display, null otherwise.
pub fn wl_display_ptr(display: &gdk::Display) -> *mut c_void {
    if !is_wayland(display) {
        return std::ptr::null_mut();
    }
    // SAFETY: display is a live GdkWaylandDisplay.
    unsafe { gdk_wayland_display_get_wl_display(display.to_glib_none().0) }
}

/// Raw Xlib `Display*` of an X11 GDK display, null otherwise.
pub fn x11_display_ptr(display: &gdk::Display) -> *mut c_void {
    if is_wayland(display) {
        return std::ptr::null_mut();
    }
    // SAFETY: display is a live GdkX11Display.
    unsafe { gdk_x11_display_get_xdisplay(display.to_glib_none().0) }
}

pub fn wl_surface_ptr(window: &gdk::Window) -> *mut c_void {
    // SAFETY: window is a live GdkWaylandWindow.
    unsafe { gdk_wayland_window_get_wl_surface(window.to_glib_none().0) }
}

pub fn wl_output_ptr(monitor: &gdk::Monitor) -> *mut c_void {
    // SAFETY: monitor is a live GdkWaylandMonitor.
    unsafe { gdk_wayland_monitor_get_wl_output(monitor.to_glib_none().0) }
}

pub fn set_custom_surface(window: &gdk::Window) {
    // SAFETY: window is a realized, unmapped GdkWaylandWindow toplevel.
    unsafe { gdk_wayland_window_set_use_custom_surface(window.to_glib_none().0) }
}
