---
name: gui-dialogs-lessons
description: Non-obvious facts from the M2-4b2 dialogs work in mychess-gui (iced 0.14, iced_test, app quirks)
metadata:
  type: project
---

- `mychess-gui` may not depend on `mychess-pgn`; to get an `ImportNotice`/`Severity` in tests, run the real flow (`ImportMessage::Import` -> `Effect::ScanPgn` -> `perform`). `perform` needs a tokio runtime: `crate::test_support::run_effect`.
- Typing `e4` in the move entry plays the move at the 2nd key (single exact candidate); a test that expects a buffer "e4" fails.
- A `mouse_area` wrapping a dialog also publishes `on_release` for clicks on children (children do not capture releases): tests must filter those messages.
- `text_input` default values like "?" (tags) are real text: typing appends ("?K").
- `iced::futures::executor::block_on` is not enough for `mychess_app::perform` (spawn_blocking needs tokio).
- Focus trap: custom `Operation` pair (count then focus) scoped by container id in `widgets/modal.rs`; `container()` is announced right before its `traverse`.
