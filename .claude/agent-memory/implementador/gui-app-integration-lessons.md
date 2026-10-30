---
name: gui-app-integration-lessons
description: Non-obvious facts about wiring mychess-gui to mychess-app (iced 0.14), verified in M2-3b
metadata:
  type: project
---

- `AppState.notice` is never cleared by the app; the GUI shell takes it (`take`) into `UiState.status_notice` and clears it on the next user action message.
- Dialogs the GUI cannot show (setup, import, info, session prompt) block the app (`modal_open`) or hang it; the shell answers Cancel right after each update (`unsupported_dialogs`). A multi-game PGN opens `app.chooser` (not modal); the shell auto-confirms game 1 only after a `PgnScanned` answer, to avoid an infinite re-dispatch.
- `Effect`, `Action`, `PaneKind`, `PrefChange`, `AppNotice` and the error enums are `#[non_exhaustive]`: matches need `_` arms.
- `iced::time::every` is the only timer without a new dependency (no `sleep` in iced); used for the 2 s preferences flush while `prefs_dirty`.
- Running the built binary with `HOME` overridden is fine; `cargo run` with `HOME` overridden makes rustup try to re-install the toolchain.
- `event::listen_with` takes a plain fn: it cannot see the shortcut map, so the key event is sent as `UiMessage::Key` and looked up in `update`.
