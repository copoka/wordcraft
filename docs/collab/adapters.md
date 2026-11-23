# Adapter plan: this branch against the RFC-0020 contracts

`#5834` is closed (ADR-0021 §3): the contract it predicted landed in `storytold/craft-libs` and
RFC-0020 puts the code in shared libraries with a per-app adapter of its own PR. The branch
`feature/realtime-collaboration` stays in `copoka/wordcraft` as the source of the wordcraft
adapter. This page is the inventory it starts from: what each module becomes when the contracts
land, what the adapter must not lose, and the first increment.

## What the branch holds

| module | lines | what it is under RFC-0020 |
|---|---|---|
| `crates/collab` | 2885 | the session half: wire kinds and limits, journal and dedup, room state, per-user undo. Becomes the adapter's server half behind the session trait. |
| `crates/collab-doc` | 7413 | the document half that is ours alone: commands diffed into normalized primitives and structural operations applied to `wordcraft_doc::Document`, with node-id addressing and the inverse operations per-user undo stores. Stays app-side; `Texts` replaces its text path. |
| `crates/collab-engine` | 3011 | session commands and remote replay over the engine. App-side policy, stays here. |
| `crates/collab-client` | 1346 | client state machine: pending queue, resend, recovery, `Synced` application. Becomes the client half behind the session trait. |
| `crates/collab-ws` | 537 | the socket transport. Kept, behind the client's transport seam. |
| `crates/collab-web` | 1580 | the browser session and socket. Follows `craft-web-host`'s session messages (`craft-libs#716`) instead of its own. |
| `crates/collab-app` | 2671 | the desktop session: join/leave, panel state, the file lease, commit. App glue; the panel and carets move to the shared presence control. |
| `apps/collab-server` | 5816 | the relay binary: WebSocket, JWT sessions, journal, snapshots and compaction, the cabinet commit callback, a 1 MiB frame and a 64 MiB document limit. Stays a backend; nothing in the app may name it. |
| `apps/wordcraft-web/src/{host,host_session}.rs` | 263 | the hosting page's bridge, on `craft-web-host` pinned to `7666e2b1` (ADR-0016 §3). |
| `crates/ui-egui/src/collab.rs`, `crates/engine/src/cmd/collab.rs` | 586 | session commands, viewer read-only in command entry, presence rendering. App-side policy. |
| `docs/collab` (without this page and the log) | 1976 | the protocol, its measurements and the run books; the conformance cases below are taken from it. |

## The contract surfaces it must meet

1. **Session.** Ordered submission with opaque scope locks for commands that do not commute, and
   lock-free `crdt.*` passthrough. The branch's relay already holds the constraint list (join
   sequence from the persistent counter, relay-granted replicas, opaque frames, document-id
   rooms); the adapter exposes it through the trait rather than keeping its own wire types.
2. **Presence.** Ephemeral latest-wins per peer and kind, allowed without edit permission, never in
   the document. Today it rides the room protocol; it moves to the trait, and the title-bar cluster
   reads it from there (`craft_web_host::status`, `craft-libs#2108`).
3. **Text.** `Texts` behind the engine, `craft-crdt` (`craft-libs#345`) reached only as the
   differential oracle. `collab-doc`'s text path is replaced; the structural half is not.
4. **The document model.** The app's model is `wordcraft_doc::Document` over `crates/docx`'s parts.
   The adapter edits through the document's own primitives and never re-encodes a part it does not
   model: drawings, VML, custom XML, theme and settings parts must survive a session round-trip
   byte for byte. Layout must not depend on who typed last: pagination is computed from the
   document, not from the session, and a remote edit is applied before the next layout pass rather
   than inside one.
5. **Commit and the cabinet.** The cabinet owns the file. `commit` → `committed {version, etag}`,
   a stale etag is refused, and a `409` offers the room's own bytes as a copy; the server keeps the
   journal with `kept` entries for catch-up and a lease per document. The file after a session must
   open in Word and LibreOffice unchanged outside the edited runs.
6. **Roles.** Viewer is refused in command entry (UI) and by the backend; the adapter must not be
   the only gate.

## State of the contracts (2026-11-23)

- `craft-libs#160` (cloud contracts) **merged** 2026-11-17, `fd7232d7`: `craft_ui::cloud::Event::Saved`
  answers a save job with its entry or the `Fault` that refused it, `Fault::Conflict` is a stale
  etag, `craft_cloud::Entry::writable` is read-only.
- `craft-libs#345` (`craft-crdt`) open; rebased onto `main` `249a41bd` on 2026-11-23, gate green
  (42 tests), patch series in the PR.
- `craft-libs#2108` (host statuses) open, head `2882d72`: the save/commit/presence/read-only
  surface for the title bar, with one authority per document by its origin (the cloud contract for
  a cloud entry, the page for a handed-in document).
- `craft-libs#716` (session messages in the host contract) open. RFC-0020 item 2 — the live-session
  contract beside `craft-cloud` — has no PR yet.
- House rule 10 forbids pinning an unmerged revision; until `#345` and `#2108` merge the app pins
  the fork under ADR-0016 §3, and no contract change is made locally.

## First increment

One PR, app side only, no protocol code, within the 1 500-line limit: an `adapter` module that
speaks the contract session trait, keeps the relay behind the trait, and takes `Texts` from the
shared layer while `collab-doc` keeps the structural operations. It runs the branch's existing
cases against an in-process mock — two clients converging, reconnect, structural-versus-text
conflict, viewer refusal, a 100-page document's round-trip — before the desktop and web wiring
move onto it. Anything that needs a contract change is an issue on `craft-libs`, not a local fork.

Landed as `crates/collab-adapter` on `feat/collab-adapter` from `origin/main` (STATUS §5.62): the
five cases above over the seam, no wire type, no pin. The missing shared contract is
`craft-libs#3235`; the branch waits for the fork push (STATUS §5.62).
