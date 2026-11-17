//! A page that keeps the file says whether the document is stored (storytold/craft-libs#2079):
//! the title bar's cluster shows `Saved` / `Unsaved` / `Saving…` in place, and a build no page
//! frames draws no cluster and changes no pixel of its title bar.

use egui_kittest::kittest::{By, NodeT, Queryable};
use wordcraft_engine::Session;
use wordcraft_ui_egui::host_status::{HostStatusHandle, StatusInput, handle};
use wordcraft_ui_egui::{Services, WordApp};

/// A WordCraft window on a blank document. `hosted` inserts what the web shell inserts when a page
/// frames the app: the status handle the title bar reads, and the bridge's own report of local
/// changes (the browser's leave-page guard closure does both, `apps/wordcraft-web/src/host.rs`).
fn window(hosted: bool) -> (egui_kittest::Harness<'static, WordApp>, Option<HostStatusHandle>) {
    let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
    // The labels below are English; an integration test otherwise follows the machine's language.
    app.ui.language = "en".into();
    let status = if hosted {
        let status = handle();
        app.services.host_status = Some(status.clone());
        let reporting = status.clone();
        app.services.on_dirty = Some(Box::new(move |dirty| {
            if dirty {
                reporting.lock().unwrap_or_else(|e| e.into_inner()).apply(StatusInput::Edited);
            }
        }));
        Some(status)
    } else {
        None
    };
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut WordApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    for _ in 0..6 {
        h.step();
    }
    (h, status)
}

/// The cluster's word, when the bar draws one: the cluster is the only `Status` in the window, so
/// its role says whether it is there at all.
fn cluster(h: &egui_kittest::Harness<'_, WordApp>) -> Option<String> {
    let mut nodes = h.query_all(By::new().role(egui::accesskit::Role::Status));
    let node = nodes.next()?;
    assert!(nodes.next().is_none(), "one status cluster, not a second one");
    node.accesskit_node().label()
}

/// Edit → `Unsaved`; a posted save → `Saving…`; the page's answer → `Saved`. The state comes from
/// the contract's messages and the app's own change tracking, not from the title's dirty flag.
#[test]
fn the_cluster_follows_the_edit_and_the_pages_answer() {
    let (mut h, status) = window(true);
    let status = status.expect("a hosted window has the page's status");
    assert_eq!(cluster(&h).as_deref(), Some("Saved"), "the page holds the document it sent");
    assert!(h.unnamed_widgets().is_empty(), "the cluster is named: {:?}", h.unnamed_widgets());

    h.state_mut().run("text.insert", serde_json::json!({"text": "Hello"})).unwrap();
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(cluster(&h).as_deref(), Some("Unsaved"), "an edit the page does not hold");

    // What the bridge does when it posts Save: the id is what the page's answer names, and the
    // save command marks the document clean (it is, as far as the app can tell, stored).
    status.lock().unwrap_or_else(|e| e.into_inner()).apply(StatusInput::SavePosted { id: Some("1") });
    h.state_mut().session.dirty = false;
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(cluster(&h).as_deref(), Some("Saving…"), "posted, not yet answered");

    status.lock().unwrap_or_else(|e| e.into_inner()).apply(StatusInput::Saved { id: "1" });
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(cluster(&h).as_deref(), Some("Saved"), "the page stored it");
}

/// A refused save is `Unsaved` again, with the reason on the word: the app does not claim a save
/// the page did not make.
#[test]
fn a_refused_save_is_unsaved_with_its_reason() {
    let (mut h, status) = window(true);
    let status = status.expect("a hosted window has the page's status");
    status.lock().unwrap_or_else(|e| e.into_inner()).apply(StatusInput::SavePosted { id: Some("1") });
    status.lock().unwrap_or_else(|e| e.into_inner()).apply(StatusInput::SaveFailed { id: Some("1"), error: "the disk is full" });
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(cluster(&h).as_deref(), Some("Unsaved"));
    assert_eq!(
        status.lock().unwrap_or_else(|e| e.into_inner()).error(),
        Some("the disk is full"),
        "the reason stays on the state, for the word's tooltip"
    );
}

/// No page keeps the file: no cluster at all, and the title bar keeps its exact height — the
/// cluster is a part of the existing bar, not a new row. The bar is 38 px tall
/// (`chrome::title_bar`); the test checks the things that would break if the cluster took room from
/// it: the title stays where it is, and the ribbon starts at the same y in both builds.
#[test]
fn without_a_page_there_is_no_cluster_and_the_bar_keeps_its_height() {
    let (plain, _) = window(false);
    assert!(cluster(&plain).is_none(), "a build no page frames draws no cluster");
    let (hosted, _) = window(true);
    assert!(cluster(&hosted).is_some(), "a hosted build draws one");

    let title = |h: &egui_kittest::Harness<'_, WordApp>, label: &str| h.get_by_role_and_label(egui::accesskit::Role::Button, label).rect();
    let ribbon = |h: &egui_kittest::Harness<'_, WordApp>| h.get_by_role_and_label(egui::accesskit::Role::Button, "Home").rect();
    let cluster_rect = hosted.get_by_role_and_label(egui::accesskit::Role::Status, "Saved").rect();
    // A build no page frames keeps the title's own save word, unchanged; the hosted one shows the
    // cluster instead (there is one save word, not two).
    let (hosted_title, plain_title) = (title(&hosted, "Document1"), title(&plain, "Document1 — Not saved"));

    assert_eq!(cluster_rect.center().y, hosted_title.center().y, "the cluster is on the title's line");
    assert!(cluster_rect.min.y >= hosted_title.min.y && cluster_rect.max.y <= hosted_title.max.y, "and within the bar's own band");
    assert_eq!(hosted_title.height(), plain_title.height(), "the title bar's row keeps its height");
    assert_eq!(ribbon(&hosted).min.y, ribbon(&plain).min.y, "the ribbon starts at the same y");
    assert_eq!(hosted_title.center().y, plain_title.center().y, "and the title sits where it did");
}
