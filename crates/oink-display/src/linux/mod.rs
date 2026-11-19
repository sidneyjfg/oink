//! Linux (X11) display backend: monitor enumeration and change notification via RandR.
//!
//! Coordinates are physical pixels in the X root-window space, as on Windows. Writing the
//! native layout (moving monitors) is not implemented and keeps the trait's default
//! `Unsupported`; enumeration and change watching are all the receiving side needs.

use std::time::Duration;

use oink_foundation::display::{DisplayInfo, REFERENCE_DIAGONAL_METERS};
use oink_foundation::{Rect, Size};
use x11rb::connection::Connection;
use x11rb::protocol::randr::{ConnectionExt as _, NotifyMask};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

use crate::{ChangeHandler, DisplayBackend, DisplayError};

/// Bursts of RandR events (a mode change emits several) are merged into one callback.
const COALESCE: Duration = Duration::from_millis(150);

pub(crate) fn backend() -> Result<Box<dyn DisplayBackend>, DisplayError> {
    Ok(Box::new(X11Display::connect()?))
}

struct X11Display {
    conn: RustConnection,
    root: xproto::Window,
}

impl X11Display {
    fn connect() -> Result<Self, DisplayError> {
        let (conn, root) = open()?;
        Ok(Self { conn, root })
    }

    /// Single-screen fallback for servers without RandR 1.5 monitors.
    fn whole_screen(&self) -> Result<Vec<DisplayInfo>, DisplayError> {
        let geometry = self
            .conn
            .get_geometry(self.root)
            .map_err(platform("GetGeometry"))?
            .reply()
            .map_err(platform("GetGeometry"))?;
        Ok(vec![build_info(
            "screen",
            0,
            0,
            geometry.width,
            geometry.height,
            0,
            0,
            true,
        )])
    }
}

fn open() -> Result<(RustConnection, xproto::Window), DisplayError> {
    let (conn, screen_num) = RustConnection::connect(None)
        .map_err(|e| DisplayError::Platform(format!("cannot connect to the X server (is DISPLAY set?): {e}")))?;
    let root = conn
        .setup()
        .roots
        .get(screen_num)
        .ok_or_else(|| DisplayError::Platform("X server reported no screen".into()))?
        .root;
    Ok((conn, root))
}

impl DisplayBackend for X11Display {
    fn displays(&self) -> Result<Vec<DisplayInfo>, DisplayError> {
        let reply = match self
            .conn
            .randr_get_monitors(self.root, true)
            .map_err(platform("RRGetMonitors"))?
            .reply()
        {
            Ok(reply) => reply,
            // RandR older than 1.5: treat the whole screen as one display.
            Err(_) => return self.whole_screen(),
        };
        let mut displays = Vec::with_capacity(reply.monitors.len());
        for monitor in &reply.monitors {
            let name = self
                .conn
                .get_atom_name(monitor.name)
                .map_err(platform("GetAtomName"))?
                .reply()
                .map_err(platform("GetAtomName"))?
                .name;
            displays.push(build_info(
                &String::from_utf8_lossy(&name),
                monitor.x,
                monitor.y,
                monitor.width,
                monitor.height,
                monitor.width_in_millimeters,
                monitor.height_in_millimeters,
                monitor.primary,
            ));
        }
        if displays.is_empty() {
            return self.whole_screen();
        }
        ensure_one_primary(&mut displays);
        Ok(displays)
    }

    fn watch(&self, on_change: ChangeHandler) -> Result<(), DisplayError> {
        // A dedicated connection: the watcher blocks on `wait_for_event`.
        let (conn, root) = open()?;
        conn.randr_select_input(
            root,
            NotifyMask::SCREEN_CHANGE | NotifyMask::CRTC_CHANGE | NotifyMask::OUTPUT_CHANGE,
        )
        .map_err(platform("RRSelectInput"))?;
        conn.flush().map_err(platform("flush"))?;
        std::thread::Builder::new()
            .name("oink-x11-display-watch".into())
            .spawn(move || {
                while conn.wait_for_event().is_ok() {
                    std::thread::sleep(COALESCE);
                    while matches!(conn.poll_for_event(), Ok(Some(_))) {}
                    on_change();
                }
            })
            .map_err(|e| DisplayError::Platform(format!("watch thread: {e}")))?;
        Ok(())
    }
}

fn platform<E: std::fmt::Display>(what: &'static str) -> impl Fn(E) -> DisplayError {
    move |e| DisplayError::Platform(format!("{what}: {e}"))
}

#[allow(clippy::too_many_arguments)]
fn build_info(name: &str, x: i16, y: i16, width: u16, height: u16, mm_w: u32, mm_h: u32, primary: bool) -> DisplayInfo {
    let reference_extent = (mm_w > 0 && mm_h > 0).then(|| {
        Size::new(
            f64::from(mm_w) / 1000.0 / REFERENCE_DIAGONAL_METERS,
            f64::from(mm_h) / 1000.0 / REFERENCE_DIAGONAL_METERS,
        )
    });
    let upper = name.to_ascii_uppercase();
    DisplayInfo {
        // Output names (eDP-1, HDMI-1...) are stable across restarts for a given port.
        key: format!("x11:{name}"),
        name: name.to_owned(),
        bounds: Rect::new(f64::from(x), f64::from(y), f64::from(width), f64::from(height)),
        pixel_width: u32::from(width),
        pixel_height: u32::from(height),
        reference_extent,
        // RandR monitors already report the rotated geometry; the rotation itself is not queried.
        rotation_deg: 0,
        is_primary: primary,
        is_builtin: ["EDP", "LVDS", "DSI"].iter().any(|prefix| upper.starts_with(prefix)),
        is_virtual: upper.starts_with("VIRTUAL"),
    }
}

/// Layout code expects exactly one primary display.
fn ensure_one_primary(displays: &mut [DisplayInfo]) {
    match displays.iter().filter(|d| d.is_primary).count() {
        1 => {}
        _ => {
            for (index, display) in displays.iter_mut().enumerate() {
                display.is_primary = index == 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_builtin_laptop_panel() {
        let d = build_info("eDP-1", 0, 0, 1366, 768, 309, 173, true);
        assert_eq!(d.key, "x11:eDP-1");
        assert_eq!(d.bounds, Rect::new(0.0, 0.0, 1366.0, 768.0));
        assert_eq!((d.pixel_width, d.pixel_height), (1366, 768));
        assert!(d.is_builtin && d.is_primary && !d.is_virtual);
        let extent = d.reference_extent.expect("physical size known");
        assert!((extent.w / extent.h - 309.0 / 173.0).abs() < 1e-9);
    }

    #[test]
    fn unknown_physical_size_has_no_reference_extent() {
        let d = build_info("HDMI-1", 1366, 0, 1920, 1080, 0, 0, false);
        assert!(d.reference_extent.is_none());
        assert!(!d.is_builtin);
        assert_eq!(d.bounds.x, 1366.0);
    }

    #[test]
    fn virtual_outputs_are_marked() {
        assert!(build_info("VIRTUAL1", 0, 0, 800, 600, 0, 0, false).is_virtual);
    }

    #[test]
    fn exactly_one_primary_is_guaranteed() {
        let mut none = vec![
            build_info("A", 0, 0, 10, 10, 0, 0, false),
            build_info("B", 10, 0, 10, 10, 0, 0, false),
        ];
        ensure_one_primary(&mut none);
        assert_eq!(none.iter().filter(|d| d.is_primary).count(), 1);
        assert!(none[0].is_primary);

        let mut second = vec![
            build_info("A", 0, 0, 10, 10, 0, 0, false),
            build_info("B", 10, 0, 10, 10, 0, 0, true),
        ];
        ensure_one_primary(&mut second);
        assert!(!second[0].is_primary && second[1].is_primary);
    }
}
