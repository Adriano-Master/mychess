---
name: mychess-m3-4e-browser-test-notes
description: How the m3_4e_* tests drive the game browser (app Rig raw/perform, GUI key tasks dropped) and verified traps (list msgs blocked under the browser, Quit, flip before load)
metadata:
  type: project
---

App: `tests/m3_4e_common` reuses m3_4c `Rig`. `rig.send(Open)` runs the read; `raw` + `perform` gives races. Under the browser
`modal_open()` is true, so ALL `ListMessage` (except answers/CancelJob), `Select/Close/LoadGame/Clipbase*` are ignored: set up
selection/filter/jobs BEFORE opening. `close_slot` is `pub(crate)`, so slot-closing tests live in
`src/bases/browser_contract_tests.rs` (also the filter-ply tests: `slot_mut` is crate-private). A cancelled `OpenControl`
before `perform` gives an empty `Loaded` and boards become `Unreadable` (usable to render the unreadable text in the GUI crate
without a corrupt SI4). Corrupt-SI4 fixture that works: overwrite the game's `.sg4` bytes (offset/length from `Si4Database::entry`)
with 0xFF BEFORE opening the slot.

GUI: `Rig.press/key/msg` drop the Task, so Enter/Load never reaches the editor: assert `key_units(..) > 0` and that the browser is still
open (D603), or call the app message through `rig.send`. Highlight of the current move: count pixels of `Tokens::of(&Theme::Light).accent`
inside `BROWSER_MOVES_ID` bounds (render_common uses Light); grid columns: distinct x of the `"W{n} - B{n}"` labels.
Playing a move in the editor needs `Nav::GameEnd` first (game of 4 plies, then `a2a3`).

Findings (left failing/noted): shell flips the main board BEFORE LoadIntoBoard (Scid LoadGame:379-390 flips after a successful load), so a
cancelled discard question leaves it turned; `AppState::quit` does not close the browser (spec silent; harmless); PGN game loaded from the
browser gets `GameOrigin::PgnFile` (D519), only SI4/Clipbase get `New`; there is no "Game > Browse current game" menu (Scid menus.tcl:265).
