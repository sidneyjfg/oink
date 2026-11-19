//! Linux (X11) input backend.
//!
//! Step 1-2 of the port: connect to the X server, query the pointer and inject
//! pointer motion, buttons and wheel through the XTEST extension. Keyboard
//! injection, global capture (`start`) and `CaptureMode::Remote` are not
//! implemented yet and report `InputError::Unsupported` instead of pretending.
//!
//! Wheel sign follows the Windows backend: `dy > 0` scrolls the view up
//! (X11 button 4), `dx > 0` scrolls it left (button 6).

use std::sync::Mutex;

use oink_foundation::Point;
use oink_foundation::input::{
    CaptureMode, FeelProfile, InjectEvent, InputPermissions, MouseButton, PermissionState, ScrollUnit,
};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    self, BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, MOTION_NOTIFY_EVENT,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use crate::{EventHandler, InputBackend, InputError};

/// Pixel scroll deltas are converted to wheel notches with this many pixels per notch.
const PIXELS_PER_NOTCH: f64 = 40.0;

pub(crate) fn backend() -> Result<Box<dyn InputBackend>, InputError> {
    Ok(Box::new(LinuxInput::connect()?))
}

struct LinuxInput {
    conn: RustConnection,
    root: xproto::Window,
    /// Sub-unit remainders carried between events (wheel notches, relative pixels).
    acc: Mutex<Accumulators>,
}

#[derive(Default)]
struct Accumulators {
    wheel: (f64, f64),
    relative: (f64, f64),
}

impl LinuxInput {
    fn connect() -> Result<Self, InputError> {
        let (conn, screen_num) = RustConnection::connect(None)
            .map_err(|e| InputError::Platform(format!("cannot connect to the X server (is DISPLAY set?): {e}")))?;
        let root = conn
            .setup()
            .roots
            .get(screen_num)
            .ok_or_else(|| InputError::Platform("X server reported no screen".into()))?
            .root;
        // Fails when the XTEST extension is missing, which would make injection impossible.
        conn.xtest_get_version(2, 2)
            .map_err(platform("XTEST get_version"))?
            .reply()
            .map_err(platform("XTEST extension unavailable"))?;
        Ok(Self {
            conn,
            root,
            acc: Mutex::new(Accumulators::default()),
        })
    }

    fn fake(&self, kind: u8, detail: u8, root: xproto::Window, x: i16, y: i16) -> Result<(), InputError> {
        self.conn
            .xtest_fake_input(kind, detail, 0, root, x, y, 0)
            .map_err(platform("XTestFakeInput"))?;
        Ok(())
    }

    fn flush(&self) -> Result<(), InputError> {
        self.conn.flush().map_err(platform("flush"))
    }

    fn click_notches(&self, button: u8, count: i32) -> Result<(), InputError> {
        for _ in 0..count {
            self.fake(BUTTON_PRESS_EVENT, button, x11rb::NONE, 0, 0)?;
            self.fake(BUTTON_RELEASE_EVENT, button, x11rb::NONE, 0, 0)?;
        }
        Ok(())
    }

    fn scroll(&self, dx: f64, dy: f64, unit: ScrollUnit) -> Result<(), InputError> {
        let (nx, ny) = match unit {
            ScrollUnit::Line => (dx, dy),
            ScrollUnit::Pixel => (dx / PIXELS_PER_NOTCH, dy / PIXELS_PER_NOTCH),
        };
        let (wx, wy) = {
            let mut acc = self.acc.lock().unwrap_or_else(|e| e.into_inner());
            (take_whole(&mut acc.wheel.0, nx), take_whole(&mut acc.wheel.1, ny))
        };
        // Buttons 4/5 = up/down, 6/7 = left/right.
        if wy != 0 {
            self.click_notches(if wy > 0 { 4 } else { 5 }, wy.abs())?;
        }
        if wx != 0 {
            self.click_notches(if wx > 0 { 6 } else { 7 }, wx.abs())?;
        }
        self.flush()
    }
}

impl InputBackend for LinuxInput {
    fn start(&self, _handler: EventHandler) -> Result<(), InputError> {
        // Step 4: XInput2 raw-event capture.
        Err(InputError::Unsupported)
    }

    fn stop(&self) {}

    fn set_mode(&self, mode: CaptureMode) -> Result<(), InputError> {
        match mode {
            CaptureMode::Local => Ok(()),
            // Step 5: pointer/keyboard grab with the cursor parked.
            CaptureMode::Remote { .. } => Err(InputError::Unsupported),
        }
    }

    fn inject(&self, event: &InjectEvent) -> Result<(), InputError> {
        match event {
            InjectEvent::MoveTo { pos } | InjectEvent::MoveToWithDelta { pos, .. } => {
                self.fake(MOTION_NOTIFY_EVENT, 0, self.root, to_i16(pos.x), to_i16(pos.y))?;
                self.flush()
            }
            InjectEvent::MoveRelative { dx, dy } => {
                let (ix, iy) = {
                    let mut acc = self.acc.lock().unwrap_or_else(|e| e.into_inner());
                    (take_whole(&mut acc.relative.0, *dx), take_whole(&mut acc.relative.1, *dy))
                };
                if ix == 0 && iy == 0 {
                    return Ok(());
                }
                // detail = 1 makes the coordinates relative to the current pointer position.
                self.fake(MOTION_NOTIFY_EVENT, 1, x11rb::NONE, clamp_i16(ix), clamp_i16(iy))?;
                self.flush()
            }
            InjectEvent::Button { button, pressed } => {
                let detail = x11_button(*button).ok_or(InputError::Unsupported)?;
                let kind = if *pressed { BUTTON_PRESS_EVENT } else { BUTTON_RELEASE_EVENT };
                self.fake(kind, detail, x11rb::NONE, 0, 0)?;
                self.flush()
            }
            InjectEvent::Scroll { dx, dy, unit, .. } => self.scroll(*dx, *dy, *unit),
            // The wheel fields are the fallback delivery the sender prepared for platforms
            // without precision touchpad scrolling.
            InjectEvent::TouchpadScroll { wheel, .. } => self.scroll(wheel.dx, wheel.dy, wheel.unit),
            // Step 3.
            InjectEvent::Key { .. } | InjectEvent::KeyWithoutLock { .. } | InjectEvent::Volume { .. } => {
                Err(InputError::Unsupported)
            }
            // Platform cannot replay these; the other backends also drop them.
            InjectEvent::Gesture(_) | InjectEvent::TouchpadContacts(_) => Ok(()),
        }
    }

    fn cursor_position(&self) -> Result<Point, InputError> {
        let reply = self
            .conn
            .query_pointer(self.root)
            .map_err(platform("QueryPointer"))?
            .reply()
            .map_err(platform("QueryPointer"))?;
        Ok(Point::new(f64::from(reply.root_x), f64::from(reply.root_y)))
    }

    fn feel_profile(&self) -> FeelProfile {
        FeelProfile::default()
    }

    fn permissions(&self) -> InputPermissions {
        // X11 has no per-application input permission.
        InputPermissions {
            accessibility: PermissionState::NotApplicable,
            input_monitoring: PermissionState::NotApplicable,
            secure_input_active: false,
        }
    }
}

fn platform<E: std::fmt::Display>(what: &'static str) -> impl Fn(E) -> InputError {
    move |e| InputError::Platform(format!("{what}: {e}"))
}

/// X11 core pointer button numbers: 1 left, 2 middle, 3 right, 8 back, 9 forward.
fn x11_button(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(1),
        MouseButton::Middle => Some(2),
        MouseButton::Right => Some(3),
        MouseButton::Back => Some(8),
        MouseButton::Forward => Some(9),
        MouseButton::Other(_) => None,
    }
}

fn to_i16(value: f64) -> i16 {
    value.round().clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

fn clamp_i16(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// Adds `add` to the accumulator and returns the whole units now due, keeping the remainder.
fn take_whole(acc: &mut f64, add: f64) -> i32 {
    // Tolerance so 0.3 / 0.1 style float error cannot strand a unit in the remainder.
    const EPS: f64 = 1e-6;
    *acc += add;
    let whole = if *acc >= 0.0 { (*acc + EPS).floor() } else { (*acc - EPS).ceil() };
    *acc -= whole;
    whole as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_map_to_x11_numbers() {
        assert_eq!(x11_button(MouseButton::Left), Some(1));
        assert_eq!(x11_button(MouseButton::Middle), Some(2));
        assert_eq!(x11_button(MouseButton::Right), Some(3));
        assert_eq!(x11_button(MouseButton::Back), Some(8));
        assert_eq!(x11_button(MouseButton::Forward), Some(9));
        assert_eq!(x11_button(MouseButton::Other(5)), None);
    }

    #[test]
    fn take_whole_carries_the_remainder() {
        let mut acc = 0.0;
        assert_eq!(take_whole(&mut acc, 0.4), 0);
        assert_eq!(take_whole(&mut acc, 0.4), 0);
        assert_eq!(take_whole(&mut acc, 0.4), 1);
        assert!((acc - 0.2).abs() < 1e-9);
        assert_eq!(take_whole(&mut acc, -1.7), -1);
        assert!((acc + 0.5).abs() < 1e-9);
    }

    #[test]
    fn take_whole_absorbs_float_error() {
        let mut acc = 0.0;
        let total: i32 = (0..10).map(|_| take_whole(&mut acc, 0.1)).sum();
        assert_eq!(total, 1);
    }

    #[test]
    fn coordinates_are_clamped_to_i16() {
        assert_eq!(to_i16(1.6), 2);
        assert_eq!(to_i16(-70000.0), i16::MIN);
        assert_eq!(to_i16(70000.0), i16::MAX);
        assert_eq!(to_i16(f64::NAN), 0);
        assert_eq!(clamp_i16(100_000), i16::MAX);
    }
}
