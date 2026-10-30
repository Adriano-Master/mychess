---
name: iced-text-editor-blur-on-press
description: iced 0.14 text_editor loses focus on mouse press, while buttons act on release; judge blur on ButtonReleased
metadata:
  type: feedback
---

In iced 0.14.2 the `text_editor` drops focus when the left button goes DOWN anywhere else, but a `button` publishes its message on RELEASE. Saving a draft "on blur" triggered by the press therefore runs before Cancel/Clear/Save act.

**Why:** the M2-4 comment editor saved the draft when the user clicked "Cancelar" (blocking review finding).

**How to apply:** poll `operation::is_focused` on `ButtonReleased(Left)` (widget messages are processed before subscription messages of the same event), and give focus back after actions that keep editing (Clear). Also: Python/sed edits bypass the rustfmt hook, so run `rustfmt --edition 2024 --check <file>` on touched files (not on `lib.rs`/`prefs.rs`/`state.rs` when other agents own children modules: it formats submodules too).
