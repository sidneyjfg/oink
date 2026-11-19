//! Live keyboard check for the Linux backend. Sends physical keys (HID usages) to whatever
//! window has focus, so run it with a harmless window such as `xev` focused.
//! Run: DISPLAY=:1 XAUTHORITY=... cargo run -p oink-input --example linux_keys --release
use std::time::Duration;

use oink_input::{InjectEvent, KeyCode};

fn key(backend: &dyn oink_input::InputBackend, usage: u16, pressed: bool) {
    backend
        .inject(&InjectEvent::Key {
            code: KeyCode::keyboard(usage),
            pressed,
        })
        .expect("key");
    std::thread::sleep(Duration::from_millis(12));
}

fn tap(backend: &dyn oink_input::InputBackend, usage: u16) {
    key(backend, usage, true);
    key(backend, usage, false);
}

fn chord(backend: &dyn oink_input::InputBackend, modifier: u16, usage: u16) {
    key(backend, modifier, true);
    tap(backend, usage);
    key(backend, modifier, false);
}

fn main() {
    let backend = oink_input::platform_backend().expect("backend");
    let b = backend.as_ref();
    // "oink" by physical position: o=0x12 i=0x0C n=0x11 k=0x0E
    for usage in [0x12, 0x0C, 0x11, 0x0E] {
        tap(b, usage);
    }
    tap(b, 0x33); // ABNT2: "ç"
    tap(b, 0x87); // ABNT2 extra key: "/"
    chord(b, 0xE1, 0x87); // Shift + extra key: "?"
    chord(b, 0xE1, 0x1E); // Shift + 1: "!"
    chord(b, 0xE6, 0x14); // AltGr + q: "/" on ABNT2
    tap(b, 0x64); // non-US backslash: "\"
    tap(b, 0x50); // Left arrow
    // Held-key cleanup: leave Left Shift down and let the backend drop; the server must see a release.
    key(b, 0xE1, true);
    println!("done; Left Shift left held on purpose, Drop must release it");
}
