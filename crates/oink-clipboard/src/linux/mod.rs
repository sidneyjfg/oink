//! Linux (X11) clipboard backend, text only.
//!
//! One thread owns a connection and a hidden window: it serves our selection to other clients
//! and watches ownership changes through XFixes. Reading another client's selection uses a
//! short-lived connection, so it never competes with that thread for events.
//!
//! shortcut: plain text only (no images, HTML or files), and INCR (chunked) transfers are
//! refused, so a very large copy is not synced; add `image/png` and INCR when needed.

use std::io::{self, Cursor, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oink_foundation::DeviceId;
use oink_foundation::clipboard::ClipPlatform;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{self, Atom, AtomEnum, ConnectionExt as _, EventMask, PropMode, Window, WindowClass};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::content::{self, Provider, Translator, namespace::Namespace};
use crate::model::*;
use crate::{
    CapturedClipboard, ChangeHandler, ClipData, ClipSnapshot, ClipboardBackend, ClipboardError, NativeSpec,
    Sensitivity,
};

/// Same name the other platforms use for the portable UTF-8 text representation.
const PORTABLE_TEXT: &str = "mime:text/plain;charset=utf-8";
const READ_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn backend() -> Result<Box<dyn ClipboardBackend>, ClipboardError> {
    Ok(Box::new(X11Clipboard::connect()?))
}

struct Atoms {
    clipboard: Atom,
    utf8: Atom,
    mime_utf8: Atom,
    targets: Atom,
    text: Atom,
    incr: Atom,
    /// Property used to receive a conversion.
    reply: Atom,
}

#[derive(Default)]
struct Shared {
    count: AtomicU64,
    /// What we serve while we own the selection.
    served: Mutex<Option<Arc<str>>>,
    handlers: Mutex<Vec<ChangeHandler>>,
    paused: AtomicBool,
}

struct X11Clipboard {
    conn: Arc<RustConnection>,
    window: Window,
    atoms: Arc<Atoms>,
    shared: Arc<Shared>,
}

fn platform<E: std::fmt::Display>(what: &'static str) -> impl Fn(E) -> ClipboardError {
    move |e| ClipboardError::Platform(format!("{what}: {e}"))
}

fn open() -> Result<(RustConnection, Window), ClipboardError> {
    let (conn, screen) = RustConnection::connect(None)
        .map_err(|e| ClipboardError::Platform(format!("cannot connect to the X server (is DISPLAY set?): {e}")))?;
    let root = conn
        .setup()
        .roots
        .get(screen)
        .ok_or_else(|| ClipboardError::Platform("X server reported no screen".into()))?
        .root;
    Ok((conn, root))
}

fn hidden_window(conn: &RustConnection, root: Window) -> Result<Window, ClipboardError> {
    let window = conn.generate_id().map_err(platform("generate_id"))?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        x11rb::COPY_FROM_PARENT,
        &xproto::CreateWindowAux::new(),
    )
    .map_err(platform("create_window"))?;
    Ok(window)
}

fn intern(conn: &RustConnection, name: &str) -> Result<Atom, ClipboardError> {
    Ok(conn
        .intern_atom(false, name.as_bytes())
        .map_err(platform("intern_atom"))?
        .reply()
        .map_err(platform("intern_atom"))?
        .atom)
}

impl X11Clipboard {
    fn connect() -> Result<Self, ClipboardError> {
        let (conn, root) = open()?;
        let window = hidden_window(&conn, root)?;
        let atoms = Arc::new(Atoms {
            clipboard: intern(&conn, "CLIPBOARD")?,
            utf8: intern(&conn, "UTF8_STRING")?,
            mime_utf8: intern(&conn, PORTABLE_TEXT.trim_start_matches("mime:"))?,
            targets: intern(&conn, "TARGETS")?,
            text: intern(&conn, "TEXT")?,
            incr: intern(&conn, "INCR")?,
            reply: intern(&conn, "OINK_CLIPBOARD")?,
        });
        conn.xfixes_query_version(5, 0)
            .map_err(platform("XFixes"))?
            .reply()
            .map_err(platform("XFixes is required to watch the clipboard"))?;
        conn.xfixes_select_selection_input(
            window,
            atoms.clipboard,
            SelectionEventMask::SET_SELECTION_OWNER
                | SelectionEventMask::SELECTION_WINDOW_DESTROY
                | SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )
        .map_err(platform("XFixes select"))?;
        conn.flush().map_err(platform("flush"))?;

        let conn = Arc::new(conn);
        let shared = Arc::new(Shared::default());
        {
            let (conn, atoms, shared) = (conn.clone(), atoms.clone(), shared.clone());
            // Lives as long as the process: there is one clipboard backend per agent.
            std::thread::Builder::new()
                .name("oink-x11-clipboard".into())
                .spawn(move || serve(&conn, window, &atoms, &shared))
                .map_err(|e| ClipboardError::Platform(format!("clipboard thread: {e}")))?;
        }
        Ok(Self {
            conn,
            window,
            atoms,
            shared,
        })
    }

    fn served(&self) -> Option<Arc<str>> {
        self.shared.served.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Takes the selection and serves `text` until someone else copies.
    fn publish(&self, text: &str) -> Result<u64, ClipboardError> {
        *self.shared.served.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.into());
        self.conn
            .set_selection_owner(self.window, self.atoms.clipboard, x11rb::CURRENT_TIME)
            .map_err(platform("SetSelectionOwner"))?;
        self.conn.flush().map_err(platform("flush"))?;
        // The XFixes event for this change names our window and is not counted by `serve`.
        Ok(self.shared.count.fetch_add(1, Ordering::AcqRel) + 1)
    }

    /// The current clipboard text, or None when it holds none.
    fn fetch_text(&self) -> Result<Option<String>, ClipboardError> {
        let owner = self
            .conn
            .get_selection_owner(self.atoms.clipboard)
            .map_err(platform("GetSelectionOwner"))?
            .reply()
            .map_err(platform("GetSelectionOwner"))?
            .owner;
        if owner == x11rb::NONE {
            return Ok(None);
        }
        if owner == self.window {
            return Ok(self.served().map(|text| text.to_string()));
        }
        read_selection(&self.atoms)
    }
}

/// Asks the selection owner for UTF-8 text on a private connection.
fn read_selection(atoms: &Atoms) -> Result<Option<String>, ClipboardError> {
    let (conn, root) = open()?;
    let window = hidden_window(&conn, root)?;
    conn.convert_selection(window, atoms.clipboard, atoms.utf8, atoms.reply, x11rb::CURRENT_TIME)
        .map_err(platform("ConvertSelection"))?;
    conn.flush().map_err(platform("flush"))?;
    let deadline = Instant::now() + READ_TIMEOUT;
    loop {
        match conn.poll_for_event().map_err(platform("poll"))? {
            Some(Event::SelectionNotify(event)) => {
                if event.property == x11rb::NONE {
                    return Ok(None);
                }
                let reply = conn
                    .get_property(true, window, atoms.reply, AtomEnum::ANY, 0, 16 * 1024 * 1024)
                    .map_err(platform("GetProperty"))?
                    .reply()
                    .map_err(platform("GetProperty"))?;
                if reply.type_ == atoms.incr || reply.bytes_after > 0 {
                    return Err(ClipboardError::ContentUnavailable(Unavailable::LimitExceeded));
                }
                return String::from_utf8(reply.value)
                    .map(Some)
                    .map_err(|_| ClipboardError::ContentUnavailable(Unavailable::InvalidData));
            }
            Some(_) => {}
            None if Instant::now() >= deadline => return Err(ClipboardError::Busy),
            None => std::thread::sleep(Duration::from_millis(4)),
        }
    }
}

/// The event loop: answers requests for our selection and reports foreign copies.
fn serve(conn: &RustConnection, window: Window, atoms: &Atoms, shared: &Shared) {
    while let Ok(event) = conn.wait_for_event() {
        match event {
            Event::XfixesSelectionNotify(event) if event.selection == atoms.clipboard => {
                if event.owner == window || event.owner == x11rb::NONE {
                    continue;
                }
                shared.count.fetch_add(1, Ordering::AcqRel);
                if shared.paused.load(Ordering::Acquire) {
                    continue;
                }
                let handlers = shared.handlers.lock().unwrap_or_else(|e| e.into_inner()).clone();
                for handler in handlers {
                    handler();
                }
            }
            Event::SelectionClear(event) if event.selection == atoms.clipboard => {
                *shared.served.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
            Event::SelectionRequest(request) => {
                let text = shared.served.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let _ = answer(conn, atoms, &request, text.as_deref());
            }
            _ => {}
        }
    }
}

fn answer(
    conn: &RustConnection,
    atoms: &Atoms,
    request: &xproto::SelectionRequestEvent,
    text: Option<&str>,
) -> Result<(), x11rb::errors::ReplyOrIdError> {
    // Old clients leave the property unset: the target doubles as the property name.
    let property = if request.property == x11rb::NONE {
        request.target
    } else {
        request.property
    };
    let max = conn.maximum_request_bytes().saturating_sub(64);
    let served = match (request.selection == atoms.clipboard, text) {
        (true, Some(_)) if request.target == atoms.targets => {
            let targets = [
                atoms.targets,
                atoms.utf8,
                atoms.mime_utf8,
                atoms.text,
                AtomEnum::STRING.into(),
            ];
            conn.change_property32(PropMode::REPLACE, request.requestor, property, AtomEnum::ATOM, &targets)?;
            true
        }
        (true, Some(text))
            if text.len() <= max
                && (request.target == atoms.utf8 || request.target == atoms.mime_utf8 || request.target == atoms.text) =>
        {
            conn.change_property8(PropMode::REPLACE, request.requestor, property, atoms.utf8, text.as_bytes())?;
            true
        }
        (true, Some(text)) if text.len() <= max && request.target == u32::from(AtomEnum::STRING) => {
            conn.change_property8(PropMode::REPLACE, request.requestor, property, AtomEnum::STRING, &latin1(text))?;
            true
        }
        _ => false,
    };
    let notify = xproto::SelectionNotifyEvent {
        response_type: xproto::SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: request.time,
        requestor: request.requestor,
        selection: request.selection,
        target: request.target,
        property: if served { property } else { x11rb::NONE },
    };
    conn.send_event(false, request.requestor, EventMask::NO_EVENT, notify)?;
    conn.flush()?;
    Ok(())
}

/// Characters outside Latin-1 cannot be represented in the legacy STRING target.
fn latin1(text: &str) -> Vec<u8> {
    text.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect()
}

fn offer_id(count: u64) -> OfferId {
    OfferId {
        clip: ClipId {
            origin: DeviceId([0; 32]),
            seq: count,
        },
        generation: count,
    }
}

/// Text decoded from a text representation, for the formats a peer may offer.
fn decode_text(rep: &Representation, bytes: Vec<u8>) -> io::Result<String> {
    let invalid = || content::unavailable(Unavailable::InvalidData);
    match &rep.format {
        NativeFormat::WindowsPredefined(13) => {
            let words: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .take_while(|n| *n != 0)
                .collect();
            String::from_utf16(&words).map_err(|_| invalid())
        }
        _ => String::from_utf8(bytes).map_err(|_| invalid()),
    }
}

struct Snapshot {
    inventory: Inventory,
    text: Vec<u8>,
    shared: Arc<Shared>,
    count: u64,
}

fn current(shared: &Shared, count: u64) -> io::Result<()> {
    if shared.count.load(Ordering::Acquire) == count {
        Ok(())
    } else {
        Err(content::unavailable(Unavailable::Superseded))
    }
}

struct Portable;
impl Translator for Portable {
    fn standard(&self, rep: &Representation, bytes: &[u8]) -> io::Result<Vec<ClipData>> {
        Ok(vec![ClipData::Text(decode_text(rep, bytes.to_vec())?)])
    }
}

impl Provider for Snapshot {
    fn inventory(&self) -> Inventory {
        self.inventory.clone()
    }
    fn open(&self, _: RepresentationId) -> io::Result<crate::FileReader> {
        current(&self.shared, self.count)?;
        Ok(Box::new(Cursor::new(self.text.clone())))
    }
    fn reader_guard(&self, _: RepresentationId) -> io::Result<content::ReaderGuard> {
        let (shared, count) = (self.shared.clone(), self.count);
        Ok(Arc::new(move || current(&shared, count)))
    }
    fn translator(&self) -> Arc<dyn Translator> {
        Arc::new(Portable)
    }
    fn namespace(&self) -> Option<Arc<dyn Namespace>> {
        None
    }
}

/// Inventory with one portable text item, or none for an empty clipboard.
fn text_inventory(count: u64, text: Option<&str>) -> Inventory {
    let items = text
        .map(|text| {
            let id = RepresentationId(1);
            ContentItem {
                virtual_files: false,
                id: ItemId(1),
                file: None,
                representations: vec![Representation {
                    id,
                    format: NativeFormat::Unknown(PORTABLE_TEXT.into()),
                    medium: Medium::Bytes,
                    aspect: 1,
                    index: -1,
                    standard: Some(StandardFormat::Text),
                    data: RepresentationData::Reader {
                        size: ContentSize::Known(text.len() as u64),
                    },
                }],
            }
        })
        .into_iter()
        .collect();
    Inventory {
        id: offer_id(count),
        platform: ClipPlatform::Unknown,
        items,
        roots: vec![],
    }
}

impl ClipboardBackend for X11Clipboard {
    fn change_count(&self) -> u64 {
        self.shared.count.load(Ordering::Acquire)
    }

    fn read(&self) -> Result<ClipSnapshot, ClipboardError> {
        let change_count = self.change_count();
        let items = self
            .fetch_text()?
            .filter(|text| !text.is_empty())
            .map(ClipData::Text)
            .into_iter()
            .collect();
        Ok(ClipSnapshot {
            items,
            sensitivity: Sensitivity::Normal,
            change_count,
        })
    }

    fn capture(&self, _: &std::path::Path) -> Result<CapturedClipboard, ClipboardError> {
        let count = self.change_count();
        let text = self.fetch_text()?.filter(|text| !text.is_empty());
        let inventory = text_inventory(count, text.as_deref());
        inventory.validate().map_err(ClipboardError::ContentUnavailable)?;
        let snapshot = ClipSnapshot {
            items: content::items(&inventory),
            sensitivity: Sensitivity::Normal,
            change_count: count,
        };
        Ok(CapturedClipboard {
            snapshot,
            content: Some(Arc::new(Snapshot {
                inventory,
                text: text.unwrap_or_default().into_bytes(),
                shared: self.shared.clone(),
                count,
            })),
        })
    }

    fn write_native_guarded(&self, spec: NativeSpec, guard: content::ReaderGuard) -> Result<u64, ClipboardError> {
        // Only text can be published here; the agent retries without files on this error.
        if !spec.files.is_empty() {
            return Err(ClipboardError::FileProviderUnavailable);
        }
        let rep = spec
            .inventory
            .items
            .iter()
            .filter(|item| item.file.is_none())
            .flat_map(|item| &item.representations)
            .find(|rep| rep.standard == Some(StandardFormat::Text) && rep.data.size().is_some())
            .ok_or(ClipboardError::Unsupported)?;
        let mut bytes = Vec::new();
        spec.contents
            .open(rep.id)
            .and_then(|reader| reader.take(crate::formats::ITEM_MAX as u64).read_to_end(&mut bytes))
            .map_err(|e| ClipboardError::ContentUnavailable(content::reason(&e)))?;
        let text = decode_text(rep, bytes).map_err(|e| ClipboardError::ContentUnavailable(content::reason(&e)))?;
        guard().map_err(|_| ClipboardError::Busy)?;
        self.publish(&text)
    }

    fn write(&self, items: &[ClipData], _: Sensitivity) -> Result<u64, ClipboardError> {
        match items.iter().find_map(|item| match item {
            ClipData::Text(text) => Some(text),
            _ => None,
        }) {
            Some(text) => self.publish(text),
            None => Err(ClipboardError::Unsupported),
        }
    }

    fn watch(&self, on_change: ChangeHandler) -> Result<(), ClipboardError> {
        self.shared.handlers.lock().unwrap_or_else(|e| e.into_inner()).push(on_change);
        Ok(())
    }

    fn set_watch_paused(&self, paused: bool) -> Result<(), ClipboardError> {
        self.shared.paused.store(paused, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin1_replaces_what_it_cannot_represent() {
        assert_eq!(latin1("ação"), vec![b'a', 0xe7, 0xe3, b'o']);
        assert_eq!(latin1("a→b"), b"a?b");
    }

    #[test]
    fn windows_utf16_text_is_decoded_up_to_the_terminator() {
        let rep = Representation {
            id: RepresentationId(1),
            format: NativeFormat::WindowsPredefined(13),
            medium: Medium::HGlobal,
            aspect: 1,
            index: -1,
            standard: Some(StandardFormat::Text),
            data: RepresentationData::Reader { size: ContentSize::Unknown },
        };
        let bytes: Vec<u8> = "olá".encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode_text(&rep, bytes).unwrap(), "olá");
    }

    #[test]
    fn inventory_for_text_is_valid_and_empty_clipboard_has_no_items() {
        let with = text_inventory(3, Some("hi"));
        with.validate().unwrap();
        assert_eq!(with.items.len(), 1);
        let without = text_inventory(4, None);
        without.validate().unwrap();
        assert!(without.items.is_empty());
    }

    /// Needs a running X server: one backend owns the clipboard, a second one reads it
    /// through the real selection protocol and sees the change.
    #[test]
    #[ignore = "needs an X server"]
    fn text_round_trips_between_two_clients() {
        let a = X11Clipboard::connect().unwrap();
        let b = X11Clipboard::connect().unwrap();
        let seen = Arc::new(AtomicU64::new(0));
        let counter = seen.clone();
        b.watch(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }))
        .unwrap();
        a.write(&[ClipData::Text("olá, área de transferência ç".into())], Sensitivity::Normal)
            .unwrap();
        let start = Instant::now();
        while seen.load(Ordering::SeqCst) == 0 {
            assert!(start.elapsed() < Duration::from_secs(2), "b never saw the change");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            b.read().unwrap().items,
            vec![ClipData::Text("olá, área de transferência ç".into())]
        );
        // A later write by b is seen by a, and a stops serving the old text.
        b.write(&[ClipData::Text("segunda".into())], Sensitivity::Normal).unwrap();
        let start = Instant::now();
        while a.served().is_some() {
            assert!(start.elapsed() < Duration::from_secs(2), "a kept serving after losing ownership");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(a.read().unwrap().items, vec![ClipData::Text("segunda".into())]);
    }
}
