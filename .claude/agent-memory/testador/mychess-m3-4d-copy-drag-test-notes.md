---
name: mychess-m3-4d-copy-drag-test-notes
description: How the m3_4d_* tests simulate tab drags/menus headless and traps found (fresh simulator loses hover state, open menu swallows clicks, Esc then release selects)
metadata:
  type: project
---

GUI: `tests/m3_4d_common` replays the last pointer position (thread_local) before each gesture, because every
`rig.sim()` is a NEW Simulator and mouse_area `on_exit`/`on_enter` need a previous position. `apply()` runs `Message::Games`
through `rig.shell.ui.games.update` (returns the app messages, so assertions can see `CopyGames`) and then `rig.send`; the
global release is sent as `Message::Notation(NotationMessage::PointerReleased)` after the widget messages.
Tab pixels: `render_common::render` + hash of `games-tab-N` bounds (hover/drop-target/source differ). Cursor icon is not observable.
Menu items: `Simulator::with_size(.., overlay(&shell))` then `sim.click("label")`; an open menu layer swallows the next
right click on another tab (dismiss), so reset `ui.games.menu = None` before opening another.

App: `Rig::raw` + `perform` splits a copy job (cancel from a watcher thread at progress >= 50 gives 51 copied: deterministic
enough). Modal prompt blocks `CopyGames`, `Select`, `Close`, `Open` (so revalidation on AnswerCopy is only reachable from
`src/bases/copy_tests.rs` via `bases.remove/slot_mut`). `confirm_copy` persists as a non-debounced pref: needs
`AppState::with_preferences_path(Some(..))` to see `Effect::SavePreferences`.

Finding left failing: after Esc cancels a visible tab drag, releasing the button over tab B still sends `Select(B)`
(the view decides release = click when `drag` is None).
