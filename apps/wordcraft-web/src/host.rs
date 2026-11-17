//! Host modes: a page that runs WordCraft (as an `<iframe>`, or in its own document) opens
//! documents in it and stores what it saves instead of the browser downloading. The messages and
//! the two transports are `craft-web-host`'s (storytold/craft-libs), the same contract as the
//! other Craft apps' bridges, with the `wordcraft:` prefix. This module is WordCraft's part of it.
//!
//! - `?host=parent` (an `<iframe>`, such as a Nextcloud app) talks to the same-origin parent over
//!   `postMessage`; `data-host="page"` on the canvas (the page's own document, no iframe) uses
//!   DOM `CustomEvent`s on the canvas instead. `?host=parent` wins when both are present. With no
//!   parent, or a parent on another origin, there is no bridge: the normal app with downloads.
//! - `wordcraft:ready` once the app is running and listening; send a file now. WordCraft reads
//!   bytes, so it says `file: false` and a host sends `bytes`.
//! - `wordcraft:open` (from the host): `{ name, bytes }`. It opens like a document picked with
//!   File › Open or dropped on the window; a file over [`MAX_OPEN_BYTES`] is refused before it is
//!   copied.
//! - `wordcraft:save` (to the host): `{ name, saveAs, bytes, id }` for every file WordCraft would
//!   otherwise download. The host stores it and reports failures itself; it answers `saved {id}`
//!   once it is stored, and the title bar's cluster says `Saving…` until it does
//!   (storytold/craft-libs#2079). Copies (exports, page images) carry no `id`: they are not the
//!   document. `saveAs` says whether the file becomes the document's file:
//!   - Save: `false`. It writes the opened file under its own name and format, as the desktop
//!     app does. A name the parent never sent (an untitled document, an imported .doc saved as
//!     .docx) is a new file: `true`, as for Save As.
//!   - Save As, and File › Export to another document format (which runs Save As): `true` for a
//!     format that keeps everything (.docx and the other Word packages, .odt, .rtf). The document
//!     is now that file; the next Save sends its name with `false`.
//!   - Save As or Export to a format that doesn't (PDF, Markdown, plain text), Export as PDF and
//!     Export Page as PNG: `false`. They are copies; the document's file stays the same.
//! - `wordcraft:dirty` whenever unsaved changes appear or go away, `wordcraft:failed` when the
//!   app couldn't start.
//!
//! In both host modes the host sends the document to edit, so no sample opens, saves are handed
//! to the host rather than downloaded, the host guards leaving the page, and `?author=` names the
//! user for comments and tracked changes.

use std::cell::Cell;
use std::rc::Rc;

use craft_web_host::web::{FileData, Host, Message};
use wordcraft_ui_egui::host_status::{HostStatusHandle, StatusInput, handle};
use wordcraft_ui_egui::{Inbox, Services, WordApp};

/// The app id in the host messages (`wordcraft:ready`).
const APP: &str = "wordcraft";

/// The name a document gets when the host sent none (the crate says `document` for a blank name).
const DEFAULT_NAME: &str = "Document.docx";

/// The largest document taken from the host page, refused before it is copied.
const MAX_OPEN_BYTES: u64 = 512 << 20;

/// The longest user name taken from `?author=`.
const MAX_AUTHOR_CHARS: usize = 200;

#[derive(Clone)]
pub struct Bridge {
    host: Rc<Host>,
    /// Whether the page has stored the document, shared with the title bar
    /// (storytold/craft-libs#2079). Only the save state is wired: participants, stored version,
    /// sync and read-only wait for the session messages of storytold/craft-libs#716.
    status: HostStatusHandle,
    /// The `id` the next save carries. The contract has a page answer a save only by its `id`, so
    /// without one the app could not tell a stored save from a posted one.
    saves: Rc<Cell<u64>>,
}

impl Bridge {
    /// The bridge for this page, or `None` when WordCraft isn't hosted: no `?host=parent` and no
    /// `data-host="page"` on the canvas, an unframed `?host=parent` page (it stays the normal app
    /// with downloads), or a parent on another origin.
    pub fn detect(canvas: &web_sys::HtmlCanvasElement) -> Option<Self> {
        Some(Self { host: Rc::new(Host::connect(APP, canvas)?), status: handle(), saves: Rc::new(Cell::new(0)) })
    }

    /// Hand saved and exported files to the host instead of downloading them, report unsaved
    /// changes to it, hear the files it sends, and show whether it stored the document.
    pub fn connect(&self, services: &mut Services, inbox: Inbox, ctx: egui::Context) {
        self.listen(inbox, ctx);
        let bridge = self.clone();
        services.host_save = Some(Box::new(move |name: &str, bytes: &[u8], save_as: bool| bridge.save(name, bytes, save_as)));
        // Anything else that would download goes to the host too. Those are copies (an export, a
        // page image), not the document, so they say nothing about whether it is stored.
        let bridge = self.clone();
        services.download = Some(Box::new(move |name: &str, bytes: &[u8]| bridge.copy(name, bytes)));
        // The browser's save picker is never used, so no save finishes later.
        services.async_saves = None;
        // The crate does not repeat a state it already reported.
        let host = self.host.clone();
        let status = self.status.clone();
        services.on_dirty = Some(Box::new(move |dirty: bool| {
            // A change is what the status state does not know by itself; a clean document is what
            // its own save answers already said.
            if dirty {
                status.lock().unwrap_or_else(|e| e.into_inner()).apply(StatusInput::Edited);
            }
            host.post_dirty(dirty);
        }));
        services.host_status = Some(self.status.clone());
    }

    /// `?author=`: the host's name for the user signs comments and tracked changes.
    pub fn set_author(app: &mut WordApp, author: &str) {
        let author: String = author.trim().chars().take(MAX_AUTHOR_CHARS).collect();
        if !author.is_empty() {
            app.session.author = author;
        }
    }

    pub fn post_ready(&self) {
        let _ = self.host.post_ready(env!("CARGO_PKG_VERSION"), false);
    }

    /// The app couldn't start (no WebGPU or WebGL2, for example): let the host say so.
    pub fn post_failed(&self, error: &str) {
        let _ = self.host.post_failed(error, None);
    }

    /// Hand the page a file the document keeps, and take the answer it gives by `id`: `Saving…`
    /// while it is out, `Saved` once the page says it stored it, `Unsaved` when the page could not
    /// be reached (storytold/craft-libs#2079).
    fn save(&self, path: &str, bytes: &[u8], save_as: bool) -> Result<(), String> {
        let id = self.saves.get() + 1;
        self.saves.set(id);
        let id = id.to_string();
        let posted = self.host.post_save(&file_name(path), bytes, Some(&id), save_as);
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        match &posted {
            Ok(()) => status.apply(StatusInput::SavePosted { id: Some(&id) }),
            // The bytes never got to the page: the document is not stored, and the reason is the one
            // the save reports in the status bar.
            Err(error) => status.apply(StatusInput::SaveFailed { id: Some(&id), error }),
        }
        posted
    }

    /// Hand the page a copy of the document (an export, a page image). A copy is not the document,
    /// so it asks for no answer and leaves the save state alone.
    fn copy(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.host.post_save(&file_name(path), bytes, None, false)
    }

    /// Hear `wordcraft:open` and queue the file like a dropped one, and `wordcraft:saved`: the page
    /// stored the document (storytold/craft-libs#2079). The rest of the contract is the app's
    /// business only once the session messages arrive (storytold/craft-libs#716).
    fn listen(&self, inbox: Inbox, ctx: egui::Context) {
        let status = self.status.clone();
        self.host.listen(move |message| match message {
            Message::Open { name, data } => deliver_open(&inbox, &ctx, &name, &data),
            Message::Saved { id } => {
                status.lock().unwrap_or_else(|e| e.into_inner()).apply(StatusInput::Saved { id: &id });
                // The person may have stopped typing: the bar has to draw the page's answer even
                // when no input wakes the app.
                ctx.request_repaint();
            }
            _ => {}
        });
    }
}

/// The file name a path ends in, as the host messages carry it.
fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string())
}

/// The file an open message carries, pushed onto the inbox for the next frame, like a dropped
/// file. A host's file is untrusted input: an oversized one is refused before it is copied into
/// the app's memory (the data's own length, not the host's claim). The crate has cleaned the name
/// (and cut it to 255 characters); a blank one comes as `document` and becomes `Document.docx`.
fn deliver_open(inbox: &Inbox, ctx: &egui::Context, name: &str, data: &FileData) {
    let name = if name == "document" { DEFAULT_NAME } else { name }.to_string();
    let FileData::Bytes(array) = data else {
        log::warn!("host file {name} came as a File; WordCraft reads bytes only; not opened");
        return;
    };
    if data.size() > MAX_OPEN_BYTES as f64 {
        log::warn!("host file is {} bytes, over the {} MiB host open limit; not opened", data.size(), MAX_OPEN_BYTES >> 20);
        return;
    }
    inbox.lock().unwrap_or_else(|e| e.into_inner()).push((name, array.to_vec()));
    ctx.request_repaint();
}
