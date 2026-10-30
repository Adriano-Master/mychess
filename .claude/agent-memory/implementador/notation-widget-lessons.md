---
name: notation-widget-lessons
description: Verified facts from the M2-4b1 notation widget/comment editor work (iced 0.14, fonts, tests)
metadata:
  type: project
---

- iced_test `Simulator::point_at` does NOT emit `CursorMoved`; a custom widget must recompute hover on press/release from `cursor.position_in`, not rely on an earlier move event. Real click coordinates for a custom text widget: build a `LayoutCache<<iced::Renderer as text::Renderer>::Paragraph>` with the same style/width and use `span_rects` (the font system is global, so it matches).
- `AppState` tests that call `session.replace_game` directly must call `app.refresh_notation()` before reading `app.notation()`.
- Noto Sans (embedded) has none of the `Nag::glyph()` symbols. Noto Sans Math + Symbols 2 cover all but `⌓ ⚨ ⚮ ⚯` (NAGs 142, 192, 193, 212; Noto Sans Symbols would cover them); those show `$n` via `nag_label` fallback. A test reads the `cmap` of the embedded fonts (format 4/12 by hand, no ttf dependency).
- iced 0.14 `text_editor` and `text_input` have no on-blur message: poll `iced::widget::operation::is_focused(id)` after a global left press (only while the draft is dirty). `text_editor::key_binding` + `Binding::Custom/Sequence/Unfocus` is the way to hook Cmd+Enter and Esc.
- `PaneKind` is `#[non_exhaustive]` in mychess-app (no Comment pane): the comment editor lives inside the notation pane, toggled by `UiState.notation.show_editor`.
- Private `Shell` methods (`dispatch`) are reachable from a child module `shell/<x>.rs` with its own `impl Shell`: the way to add glue without growing shell.rs.
- NAG session messages (`AddNag`, `ToggleNag`, `SetNags`) act on the cursor only; there is no per-node NAG message.
