//! USB HID usage <-> Linux evdev key code tables.
//!
//! Oink identifies keys by physical position (HID usage). X11 with the `evdev` xkb rules
//! also addresses physical keys: keycode = evdev code + 8, and the server applies the active
//! layout (ABNT2, US, AltGr...). So no layout handling is needed here; the same table
//! serves injection (HID -> evdev) and, later, capture (evdev -> HID).
//!
//! Source of the HID column: USB HID Usage Tables 1.4 (keyboard page 0x07, consumer page
//! 0x0C). The evdev column: Linux `input-event-codes.h`, as used by the kernel `hid-input`
//! driver. The main block is cross-checked in tests against the Windows scancode table,
//! because Set-1 scancodes and evdev codes are identical for non-extended keys.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use oink_foundation::input::KeyCode;

/// X11 keycode = evdev code + 8 (xkb `evdev` rules).
pub(crate) const X11_KEYCODE_OFFSET: u16 = 8;

/// Keyboard page 0x07: (HID usage, evdev code). Order matters for the reverse map:
/// when two usages share an evdev code, the first one wins.
const KEYBOARD: &[(u16, u16)] = &[
    // Letters a-z
    (0x04, 30),
    (0x05, 48),
    (0x06, 46),
    (0x07, 32),
    (0x08, 18),
    (0x09, 33),
    (0x0A, 34),
    (0x0B, 35),
    (0x0C, 23),
    (0x0D, 36),
    (0x0E, 37),
    (0x0F, 38),
    (0x10, 50),
    (0x11, 49),
    (0x12, 24),
    (0x13, 25),
    (0x14, 16),
    (0x15, 19),
    (0x16, 31),
    (0x17, 20),
    (0x18, 22),
    (0x19, 47),
    (0x1A, 17),
    (0x1B, 45),
    (0x1C, 21),
    (0x1D, 44),
    // Digits 1-9, 0
    (0x1E, 2),
    (0x1F, 3),
    (0x20, 4),
    (0x21, 5),
    (0x22, 6),
    (0x23, 7),
    (0x24, 8),
    (0x25, 9),
    (0x26, 10),
    (0x27, 11),
    // Enter, Esc, Backspace, Tab, Space, - = [ ] \ (non-US #) ; ' ` , . / CapsLock
    (0x28, 28),
    (0x29, 1),
    (0x2A, 14),
    (0x2B, 15),
    (0x2C, 57),
    (0x2D, 12),
    (0x2E, 13),
    (0x2F, 26),
    (0x30, 27),
    (0x31, 43),
    (0x32, 43),
    (0x33, 39),
    (0x34, 40),
    (0x35, 41),
    (0x36, 51),
    (0x37, 52),
    (0x38, 53),
    (0x39, 58),
    // F1-F12
    (0x3A, 59),
    (0x3B, 60),
    (0x3C, 61),
    (0x3D, 62),
    (0x3E, 63),
    (0x3F, 64),
    (0x40, 65),
    (0x41, 66),
    (0x42, 67),
    (0x43, 68),
    (0x44, 87),
    (0x45, 88),
    // PrintScreen, ScrollLock, Pause, Insert, Home, PageUp, Delete, End, PageDown, arrows
    (0x46, 99),
    (0x47, 70),
    (0x48, 119),
    (0x49, 110),
    (0x4A, 102),
    (0x4B, 104),
    (0x4C, 111),
    (0x4D, 107),
    (0x4E, 109),
    (0x4F, 106),
    (0x50, 105),
    (0x51, 108),
    (0x52, 103),
    // Keypad: NumLock / * - + Enter 1-9 0 .
    (0x53, 69),
    (0x54, 98),
    (0x55, 55),
    (0x56, 74),
    (0x57, 78),
    (0x58, 96),
    (0x59, 79),
    (0x5A, 80),
    (0x5B, 81),
    (0x5C, 75),
    (0x5D, 76),
    (0x5E, 77),
    (0x5F, 71),
    (0x60, 72),
    (0x61, 73),
    (0x62, 82),
    (0x63, 83),
    // Non-US \ (ABNT/ISO extra key), Application (menu), Power, Keypad =
    (0x64, 86),
    (0x65, 127),
    (0x66, 116),
    (0x67, 117),
    // F13-F24
    (0x68, 183),
    (0x69, 184),
    (0x6A, 185),
    (0x6B, 186),
    (0x6C, 187),
    (0x6D, 188),
    (0x6E, 189),
    (0x6F, 190),
    (0x70, 191),
    (0x71, 192),
    (0x72, 193),
    (0x73, 194),
    // Help
    (0x75, 138),
    // Mute, Volume Up, Volume Down (keyboard page copies)
    (0x7F, 113),
    (0x80, 115),
    (0x81, 114),
    // Keypad comma (ABNT2 numpad "."), International 1-5 (ABNT2 "/ ?" key is International1), Lang 1-2
    (0x85, 121),
    (0x87, 89),
    (0x88, 93),
    (0x89, 124),
    (0x8A, 92),
    (0x8B, 94),
    (0x90, 122),
    (0x91, 123),
    // Modifiers: LCtrl LShift LAlt LMeta RCtrl RShift RAlt (AltGr) RMeta
    (0xE0, 29),
    (0xE1, 42),
    (0xE2, 56),
    (0xE3, 125),
    (0xE4, 97),
    (0xE5, 54),
    (0xE6, 100),
    (0xE7, 126),
];

/// Consumer page 0x0C: (HID usage, evdev code).
const CONSUMER: &[(u16, u16)] = &[
    (0x6F, 225), // Brightness up
    (0x70, 224), // Brightness down
    (0xB3, 208), // Fast forward
    (0xB4, 168), // Rewind
    (0xB5, 163), // Next track
    (0xB6, 165), // Previous track
    (0xB7, 166), // Stop
    (0xCD, 164), // Play/Pause
    (0xE2, 113), // Mute
    (0xE9, 115), // Volume up
    (0xEA, 114), // Volume down
    (0x183, 171), // Consumer control config
    (0x18A, 155), // Mail
    (0x192, 140), // Calculator
    (0x194, 150), // Browser
    (0x221, 217), // Search
    (0x223, 172), // Home page
    (0x224, 158), // Back
    (0x225, 159), // Forward
    (0x226, 128), // Stop (browser)
    (0x227, 173), // Refresh
    (0x22A, 156), // Bookmarks
];

const fn build_forward() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < KEYBOARD.len() {
        table[KEYBOARD[i].0 as usize] = KEYBOARD[i].1;
        i += 1;
    }
    table
}

/// evdev code (< 256) -> HID `KeyCode` value, 0 = unmapped. First table entry wins.
#[allow(dead_code)] // used by capture (next step)
const fn build_reverse() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < KEYBOARD.len() {
        let evdev = KEYBOARD[i].1 as usize;
        if table[evdev] == 0 {
            table[evdev] = ((KeyCode::PAGE_KEYBOARD as u32) << 16) | KEYBOARD[i].0 as u32;
        }
        i += 1;
    }
    let mut j = 0;
    while j < CONSUMER.len() {
        let evdev = CONSUMER[j].1 as usize;
        if table[evdev] == 0 {
            table[evdev] = ((KeyCode::PAGE_CONSUMER as u32) << 16) | CONSUMER[j].0 as u32;
        }
        j += 1;
    }
    table
}

/// Direct-indexed by keyboard-page usage: the injection hot path does no searching.
static KEYBOARD_FORWARD: [u16; 256] = build_forward();
#[allow(dead_code)] // used by capture (next step)
static REVERSE: [u32; 256] = build_reverse();

/// HID key -> Linux evdev code.
pub(crate) fn hid_to_evdev(code: KeyCode) -> Option<u16> {
    match code.page() {
        KeyCode::PAGE_KEYBOARD => match KEYBOARD_FORWARD.get(code.usage() as usize) {
            Some(&evdev) if evdev != 0 => Some(evdev),
            _ => None,
        },
        KeyCode::PAGE_CONSUMER => CONSUMER.iter().find(|&&(u, _)| u == code.usage()).map(|&(_, e)| e),
        _ => None,
    }
}

/// Linux evdev code -> HID key (used by capture).
#[allow(dead_code)] // used by capture (next step)
pub(crate) fn evdev_to_hid(evdev: u16) -> Option<KeyCode> {
    match REVERSE.get(evdev as usize) {
        Some(&hid) if hid != 0 => Some(KeyCode(hid)),
        _ => None,
    }
}

/// HID key -> X11 keycode (evdev + 8), if it fits the protocol's 8..=255 range.
pub(crate) fn hid_to_x11(code: KeyCode) -> Option<u8> {
    u8::try_from(hid_to_evdev(code)? + X11_KEYCODE_OFFSET).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn known_keys() {
        assert_eq!(hid_to_evdev(KeyCode::keyboard(0x04)), Some(30)); // a
        assert_eq!(hid_to_evdev(KeyCode::keyboard(0x2C)), Some(57)); // space
        assert_eq!(hid_to_evdev(KeyCode::keyboard(0xE6)), Some(100)); // AltGr
        assert_eq!(hid_to_evdev(KeyCode::keyboard(0x87)), Some(89)); // ABNT2 "/ ?" key
        assert_eq!(hid_to_x11(KeyCode::keyboard(0x04)), Some(38)); // xev reports keycode 38 for "a"
        assert_eq!(hid_to_x11(KeyCode::keyboard(0x33)), Some(47)); // "ç" position on ABNT2
        assert_eq!(hid_to_evdev(KeyCode::new(KeyCode::PAGE_CONSUMER, 0xE9)), Some(115)); // volume up
        assert_eq!(hid_to_evdev(KeyCode::new(0xFF, 3)), None); // Apple Fn has no Linux key
        assert_eq!(hid_to_evdev(KeyCode::NONE), None);
    }

    #[test]
    fn no_duplicate_usages_and_all_fit_x11() {
        let mut seen = HashSet::new();
        for &(usage, evdev) in KEYBOARD {
            assert!(seen.insert(usage), "duplicate keyboard usage {usage:#x}");
            assert!(usage < 256, "keyboard usage {usage:#x} does not fit the direct table");
            assert!(evdev + X11_KEYCODE_OFFSET <= 255, "evdev {evdev} exceeds the X11 range");
        }
        let mut seen = HashSet::new();
        for &(usage, evdev) in CONSUMER {
            assert!(seen.insert(usage), "duplicate consumer usage {usage:#x}");
            assert!(evdev + X11_KEYCODE_OFFSET <= 255, "evdev {evdev} exceeds the X11 range");
        }
    }

    #[test]
    fn round_trips_except_the_known_alias() {
        for &(usage, evdev) in KEYBOARD {
            let hid = KeyCode::keyboard(usage);
            assert_eq!(hid_to_evdev(hid), Some(evdev));
            let back = evdev_to_hid(evdev).expect("reverse entry");
            if usage == 0x32 {
                // Non-US # shares the physical code of "\"; the reverse map keeps 0x31.
                assert_eq!(back, KeyCode::keyboard(0x31));
            } else if matches!(usage, 0x7F | 0x80 | 0x81) {
                assert_eq!(hid_to_evdev(back), Some(evdev));
            } else {
                assert_eq!(back, hid, "evdev {evdev}");
            }
        }
    }

    /// Windows Set-1 scancodes equal evdev codes for the non-extended keys. This checks the
    /// hand-written table against the independent Chromium-derived Windows table.
    #[test]
    fn main_block_matches_the_windows_scancodes() {
        let mut compared = 0;
        let mut mismatches = Vec::new();
        for &(usage, evdev) in KEYBOARD {
            let Some(entry) = crate::win_keymap::lookup_hid(KeyCode::keyboard(usage)) else {
                continue;
            };
            if entry.vk != 0 || entry.scancode >> 8 != 0 || entry.scancode > 0x58 {
                continue; // extended, virtual-key or outside the shared Set-1 block
            }
            compared += 1;
            if entry.scancode != evdev {
                mismatches.push((usage, evdev, entry.scancode));
            }
        }
        assert!(compared >= 70, "only {compared} keys compared; the check is vacuous");
        assert!(mismatches.is_empty(), "usage/evdev/windows-scancode mismatches: {mismatches:x?}");
    }
}
