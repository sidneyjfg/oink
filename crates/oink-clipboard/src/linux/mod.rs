//! Linux (X11) clipboard backend: plain text and PNG images.
//!
//! One thread owns a connection and a hidden window: it serves our selection to other clients
//! and watches ownership changes through XFixes. Reading another client's selection uses a
//! short-lived connection, so it never competes with that thread for events.
//!
//! shortcut: only text and `image/png` (no HTML, RTF or files). Large selections are received
//! in INCR chunks, but we never send INCR: an image bigger than the server's maximum request
//! size is not served to other X clients. Add INCR sending if that ever matters.

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
const PORTABLE_PNG: &str = "mime:image/png";
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
    png: Atom,
    /// Property used to receive a conversion.
    reply: Atom,
}

/// What the clipboard holds: text, or a PNG image (never both).
#[derive(Clone, Debug, PartialEq)]
enum Content {
    Text(String),
    Png(Vec<u8>),
}

/// What we serve while we own the selection.
#[derive(Clone)]
enum Served {
    Text(Arc<str>),
    Png(Arc<[u8]>),
}

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Default)]
struct Shared {
    count: AtomicU64,
    served: Mutex<Option<Served>>,
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
            png: intern(&conn, "image/png")?,
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

    fn served(&self) -> Option<Served> {
        self.shared.served.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Takes the selection and serves `content` until someone else copies.
    fn publish(&self, content: Served) -> Result<u64, ClipboardError> {
        *self.shared.served.lock().unwrap_or_else(|e| e.into_inner()) = Some(content);
        self.conn
            .set_selection_owner(self.window, self.atoms.clipboard, x11rb::CURRENT_TIME)
            .map_err(platform("SetSelectionOwner"))?;
        self.conn.flush().map_err(platform("flush"))?;
        // The XFixes event for this change names our window and is not counted by `serve`.
        Ok(self.shared.count.fetch_add(1, Ordering::AcqRel) + 1)
    }

    /// The current clipboard content, or None when it holds neither text nor a PNG.
    fn fetch(&self) -> Result<Option<Content>, ClipboardError> {
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
            return Ok(self.served().map(|served| match served {
                Served::Text(text) => Content::Text(text.to_string()),
                Served::Png(bytes) => Content::Png(bytes.to_vec()),
            }));
        }
        read_selection(&self.atoms)
    }
}

/// Reads another client's clipboard on a private connection. Text wins over an image when the
/// owner offers both (a spreadsheet copy), because that is what a text field would paste.
fn read_selection(atoms: &Atoms) -> Result<Option<Content>, ClipboardError> {
    let (conn, root) = open()?;
    let window = hidden_window(&conn, root)?;
    // INCR chunks announce themselves as property changes on our window.
    conn.change_window_attributes(
        window,
        &xproto::ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )
    .map_err(platform("ChangeWindowAttributes"))?;
    // An owner that does not answer TARGETS is still asked for text.
    let targets: Vec<Atom> = match convert(&conn, window, atoms, atoms.targets) {
        Ok(Some((_, bytes))) => bytes
            .chunks_exact(4)
            .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
        _ => vec![atoms.utf8],
    };
    if targets.contains(&atoms.utf8) {
        return match convert(&conn, window, atoms, atoms.utf8)? {
            Some((_, bytes)) => String::from_utf8(bytes)
                .map(|text| Some(Content::Text(text)))
                .map_err(|_| ClipboardError::ContentUnavailable(Unavailable::InvalidData)),
            None => Ok(None),
        };
    }
    if targets.contains(&atoms.png) {
        return Ok(convert(&conn, window, atoms, atoms.png)?.map(|(_, bytes)| Content::Png(bytes)));
    }
    Ok(None)
}

/// Converts the selection to `target` and returns the property type and bytes, following INCR.
fn convert(
    conn: &RustConnection,
    window: Window,
    atoms: &Atoms,
    target: Atom,
) -> Result<Option<(Atom, Vec<u8>)>, ClipboardError> {
    conn.convert_selection(window, atoms.clipboard, target, atoms.reply, x11rb::CURRENT_TIME)
        .map_err(platform("ConvertSelection"))?;
    conn.flush().map_err(platform("flush"))?;
    let mut deadline = Instant::now() + READ_TIMEOUT;
    loop {
        match conn.poll_for_event().map_err(platform("poll"))? {
            Some(Event::SelectionNotify(event)) => {
                if event.property == x11rb::NONE {
                    return Ok(None);
                }
                break;
            }
            Some(_) => {}
            None if Instant::now() >= deadline => return Err(ClipboardError::Busy),
            None => std::thread::sleep(Duration::from_millis(4)),
        }
    }
    let take = || -> Result<_, ClipboardError> {
        conn.get_property(true, window, atoms.reply, AtomEnum::ANY, 0, 16 * 1024 * 1024)
            .map_err(platform("GetProperty"))?
            .reply()
            .map_err(platform("GetProperty"))
    };
    let first = take()?;
    if first.type_ != atoms.incr {
        if first.bytes_after > 0 {
            return Err(ClipboardError::ContentUnavailable(Unavailable::LimitExceeded));
        }
        return Ok(Some((first.type_, first.value)));
    }
    // Deleting the property above told the owner to start sending chunks; an empty one ends it.
    let (mut kind, mut data) = (target, Vec::new());
    deadline = Instant::now() + READ_TIMEOUT;
    loop {
        match conn.poll_for_event().map_err(platform("poll"))? {
            Some(Event::PropertyNotify(event))
                if event.window == window && event.atom == atoms.reply && event.state == xproto::Property::NEW_VALUE =>
            {
                let chunk = take()?;
                if chunk.value.is_empty() {
                    return Ok(Some((kind, data)));
                }
                if data.len() + chunk.value.len() > crate::formats::ITEM_MAX {
                    return Err(ClipboardError::ContentUnavailable(Unavailable::LimitExceeded));
                }
                kind = chunk.type_;
                data.extend_from_slice(&chunk.value);
                deadline = Instant::now() + READ_TIMEOUT;
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
                let served = shared.served.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let _ = answer(conn, atoms, &request, served.as_ref());
            }
            _ => {}
        }
    }
}

fn answer(
    conn: &RustConnection,
    atoms: &Atoms,
    request: &xproto::SelectionRequestEvent,
    content: Option<&Served>,
) -> Result<(), x11rb::errors::ReplyOrIdError> {
    // Old clients leave the property unset: the target doubles as the property name.
    let property = if request.property == x11rb::NONE {
        request.target
    } else {
        request.property
    };
    let max = conn.maximum_request_bytes().saturating_sub(64);
    let served = match (request.selection == atoms.clipboard, content) {
        (true, Some(content)) if request.target == atoms.targets => {
            let targets = match content {
                Served::Text(_) => vec![
                    atoms.targets,
                    atoms.utf8,
                    atoms.mime_utf8,
                    atoms.text,
                    AtomEnum::STRING.into(),
                ],
                Served::Png(_) => vec![atoms.targets, atoms.png],
            };
            conn.change_property32(PropMode::REPLACE, request.requestor, property, AtomEnum::ATOM, &targets)?;
            true
        }
        (true, Some(Served::Text(text)))
            if text.len() <= max
                && (request.target == atoms.utf8 || request.target == atoms.mime_utf8 || request.target == atoms.text) =>
        {
            conn.change_property8(PropMode::REPLACE, request.requestor, property, atoms.utf8, text.as_bytes())?;
            true
        }
        (true, Some(Served::Text(text))) if text.len() <= max && request.target == u32::from(AtomEnum::STRING) => {
            conn.change_property8(PropMode::REPLACE, request.requestor, property, AtomEnum::STRING, &latin1(text))?;
            true
        }
        (true, Some(Served::Png(png))) if png.len() <= max && request.target == atoms.png => {
            conn.change_property8(PropMode::REPLACE, request.requestor, property, atoms.png, png)?;
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

fn is_empty(content: &Content) -> bool {
    match content {
        Content::Text(text) => text.is_empty(),
        Content::Png(bytes) => bytes.is_empty(),
    }
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
    data: Vec<u8>,
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
        if rep.standard == Some(StandardFormat::Png) {
            return Ok(vec![ClipData::Png(bytes.to_vec())]);
        }
        Ok(vec![ClipData::Text(decode_text(rep, bytes.to_vec())?)])
    }
}

impl Provider for Snapshot {
    fn inventory(&self) -> Inventory {
        self.inventory.clone()
    }
    fn open(&self, _: RepresentationId) -> io::Result<crate::FileReader> {
        current(&self.shared, self.count)?;
        Ok(Box::new(Cursor::new(self.data.clone())))
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

/// Inventory with one portable item (text or PNG), or none for an empty clipboard.
fn inventory_for(count: u64, content: Option<&Content>) -> Inventory {
    let items = content
        .map(|content| {
            let (name, standard, size) = match content {
                Content::Text(text) => (PORTABLE_TEXT, StandardFormat::Text, text.len()),
                Content::Png(bytes) => (PORTABLE_PNG, StandardFormat::Png, bytes.len()),
            };
            ContentItem {
                virtual_files: false,
                id: ItemId(1),
                file: None,
                representations: vec![Representation {
                    id: RepresentationId(1),
                    format: NativeFormat::Unknown(name.into()),
                    medium: Medium::Bytes,
                    aspect: 1,
                    index: -1,
                    standard: Some(standard),
                    data: RepresentationData::Reader {
                        size: ContentSize::Known(size as u64),
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
            .fetch()?
            .filter(|content| !is_empty(content))
            .map(|content| match content {
                Content::Text(text) => ClipData::Text(text),
                Content::Png(bytes) => ClipData::Png(bytes),
            })
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
        let content = self.fetch()?.filter(|content| !is_empty(content));
        let inventory = inventory_for(count, content.as_ref());
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
                data: match content {
                    Some(Content::Text(text)) => text.into_bytes(),
                    Some(Content::Png(bytes)) => bytes,
                    None => Vec::new(),
                },
                shared: self.shared.clone(),
                count,
            })),
        })
    }

    fn write_native_guarded(&self, spec: NativeSpec, guard: content::ReaderGuard) -> Result<u64, ClipboardError> {
        // Only text and PNG can be published here; the agent retries without files on this error.
        if !spec.files.is_empty() {
            return Err(ClipboardError::FileProviderUnavailable);
        }
        let find = |standard| {
            spec.inventory
                .items
                .iter()
                .filter(|item| item.file.is_none())
                .flat_map(|item| &item.representations)
                .find(|rep| rep.standard == Some(standard) && rep.data.size().is_some())
        };
        let (rep, is_png) = match find(StandardFormat::Text) {
            Some(rep) => (rep, false),
            None => (find(StandardFormat::Png).ok_or(ClipboardError::Unsupported)?, true),
        };
        let mut bytes = Vec::new();
        spec.contents
            .open(rep.id)
            .and_then(|reader| reader.take(crate::formats::ITEM_MAX as u64).read_to_end(&mut bytes))
            .map_err(|e| ClipboardError::ContentUnavailable(content::reason(&e)))?;
        // Peers always convert to the portable formats (UTF-8 text, PNG) for this platform.
        let invalid = || ClipboardError::ContentUnavailable(Unavailable::InvalidData);
        let served = if is_png {
            if !bytes.starts_with(PNG_SIGNATURE) {
                return Err(invalid());
            }
            Served::Png(bytes.into())
        } else {
            Served::Text(String::from_utf8(bytes).map_err(|_| invalid())?.into())
        };
        guard().map_err(|_| ClipboardError::Busy)?;
        self.publish(served)
    }

    fn write(&self, items: &[ClipData], _: Sensitivity) -> Result<u64, ClipboardError> {
        match items.iter().find_map(|item| match item {
            ClipData::Text(text) => Some(Served::Text(text.as_str().into())),
            ClipData::Png(bytes) => Some(Served::Png(bytes.as_slice().into())),
            _ => None,
        }) {
            Some(served) => self.publish(served),
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
        let with = inventory_for(3, Some(&Content::Text("hi".into())));
        with.validate().unwrap();
        assert_eq!(with.items.len(), 1);
        let image = inventory_for(5, Some(&Content::Png(PNG_SIGNATURE.to_vec())));
        image.validate().unwrap();
        assert_eq!(image.items[0].representations[0].standard, Some(StandardFormat::Png));
        let without = inventory_for(4, None);
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
        // An image goes through the same path.
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend((0..300_000u32).map(|n| (n % 251) as u8));
        a.write(&[ClipData::Png(png.clone())], Sensitivity::Normal).unwrap();
        assert_eq!(b.read().unwrap().items, vec![ClipData::Png(png)]);
    }
}
