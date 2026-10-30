---
name: gui-drag-copy-lessons
description: Facts from M3-4d (drag tabs to copy, copy job between bases) - iced mouse_area/button capture, event order, test locale
metadata:
  type: project
---

- A `button` captures both press and release, so an outer `mouse_area` never sees them; to see a whole drag the tab must be a `mouse_area` over a styled `container` (a disabled button shows the NotAllowed cursor). `mouse_area` publishes one fixed message per event, so decide what `on_release` means in `view` from the drag state.
- iced_winit pushes widget messages before the `event::listen_with` (subscription) messages of the same event, so a drop handled by a widget runs before the global "button released" cleanup.
- `Shell::new(None)` takes the language from the machine locale: GUI tests that read translated text must set `shell.i18n = I18n::new(Language::EnUs)`.
- `Shell::on_key` is private; tests outside `shell.rs` send `Message::Dialog(DialogMessage::Key { .. })`.
- A stale `drag` after a click-only test must not swallow Esc (m3_4c_keys `escape_leaves_the_search_field`): spend Esc only for a visible drag.
- `Si4Writer::add_prepared_batch` is all-or-none per batch; the Scid `sc_savegame` copies the whole IndexEntry (D flag included).
