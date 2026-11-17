//! The save status of a document a page hosts (`?host=parent`, storytold/craft-libs#2079).
//!
//! The page that frames a Craft web build keeps the file, so it — not the app — knows whether the
//! document is stored. The contract says so with `saved {id}` (the page stored the save), a failed
//! `save` and the app's own change tracking; the shared crate turns those into [`HostStatus`], and
//! [`HostStatus::cluster`] says what the title bar draws.
//!
//! [`HostStatusHandle`] is where the two owners meet: the web shell ([`Services::host_status`])
//! writes the contract's messages into it from the bridge's callbacks, and [`cluster`] reads it
//! while drawing the title bar. A desktop window, and a web build no page frames, has no handle
//! and the title bar draws no cluster at all — the save word in the document title stays as it was.
//!
//! This pilot wires the save state alone (storytold/craft-libs#2079): the participant count, the
//! stored version, the sync word and read-only come with the session messages of
//! storytold/craft-libs#716 and are drawn by the same call once they are inputs.
//!
//! [`Services::host_status`]: crate::Services::host_status

use std::sync::{Arc, Mutex};

pub use craft_web_host::status::{Headline, HostStatus, SaveState, StatusInput};

use egui::{Sense, Ui, pos2, vec2};

use crate::theme::{Tokens, paint_text, regular, text_width};

/// The save status of a hosted document, shared between the web shell that feeds it and the title
/// bar that draws it. Plain data (`Send`), so a page's callbacks can write it whatever threading
/// their shell has; a poisoned lock still answers, as the app must not crash over a status.
pub type HostStatusHandle = Arc<Mutex<HostStatus>>;

/// A handle for a document the page keeps and has not changed yet (it starts `Saved`: the document
/// the page sent is the one it holds). The page's bridge keeps the other half of the `Arc`.
pub fn handle() -> HostStatusHandle {
    Arc::new(Mutex::new(HostStatus::new()))
}

/// Draw the save state inside the title bar, where AutoSave and the Quick Access Toolbar are: the
/// words of the interface language, in the bar's own type. Nothing is drawn without a handle, so
/// a build no page keeps looks exactly as it did.
///
/// A refused save carries its reason as the tooltip of the word (`Unsaved`), which is where the
/// person looks when the document did not store.
pub(crate) fn cluster(ui: &mut Ui, handle: &HostStatusHandle, control_h: f32) {
    let t = Tokens::get(ui.ctx());
    let (headline, error) = {
        let status = handle.lock().unwrap_or_else(|e| e.into_inner());
        let cluster = status.cluster();
        (cluster.headline, cluster.error.map(str::to_string))
    };
    let (word, color) = match headline {
        // Read-only belongs to the session messages of storytold/craft-libs#716; the state knows
        // the word already, so the bar cannot promise a save the person may not make.
        Headline::ReadOnly => (tl!("Read-only"), t.text_dim),
        Headline::Save(SaveState::Saved) => (tl!("Saved"), t.text_dim),
        Headline::Save(SaveState::Saving) => (tl!("Saving…"), t.text_dim),
        Headline::Save(SaveState::Unsaved) => (tl!("Unsaved"), t.text),
    };
    let font = regular(11.5);
    let width = text_width(ui.painter(), word, &font, 0.0, color);
    let (rect, response) = ui.allocate_exact_size(vec2(width + 4.0, control_h), Sense::hover());
    paint_text(ui.painter(), pos2(rect.center().x, rect.center().y), egui::Align2::CENTER_CENTER, word, &font, 0.0, color);
    // A status, not a control: the word is its own name, and a screen reader reads it with the bar.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Status, true, word));
    if let Some(error) = error {
        response.on_hover_text(error);
    }
}
