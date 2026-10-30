---
name: mychess-m3-4c-list-actions-test-notes
description: How the m3_4c_* tests drive list jobs/toolbar/keys headless, and verified traps (Scid sort directions, notices, closed panel)
metadata:
  type: project
---

App side: `tests/m3_4c_common` has `Rig` (as 4a) + `G` PGN builder (movetext ends with the tag result, so `*` must be the
placeholder) + `make_si4`. Jobs: `rig.raw(msg)` returns the effect without running it, `rig.perform(effect)` answers it
later; `CancelJob` before `perform` gives `Err(Cancelled)` (Find may ignore cancel on small bases: use Filter/Sort/Copy).
`Effect::SearchList/SortList { control, .. }` exposes `control.progress()` after perform.

GUI side: `tests/m3_4c_common` reuses `m3_4b_common` through `#[path]`. Tool buttons have no ids: find the 24x24
`Candidate::Container` boxes inside `glist-toolbar` with a collecting closure selector, click with
`sim.point_at` + `simulate(click())`, and drop `Hover(` messages (pointing enters the panel). `Shell::update(..).units()`
tells whether a key produced work (Enter -> LoadGame is not observable otherwise).

Traps (verified):
- The shell moves `app.notice` to `ui.status_notice` after each dispatch: read both.
- `GameListMessage::Menu(Row(n))` selects n when outside the selection; set `ui.games.menu` directly for a menu without that.
- Adjacent `{c} {c}` comments after one move merge into one: comment_count counts comments on distinct moves (0/1/..).
- `[Round ""]`/missing round is stored as "?" and sorts AFTER numbered rounds (Scid: before). Result `*` sorts first
  (Scid: last); Elo/Comments/Variations first click is ascending (Scid: largest first). Left failing on purpose.
- Central list shortcuts (Command+Shift+A, Shift+Delete, Command+Delete) act with the panel closed (shell.rs run_action
  has no show_games guard); Command+Delete would write flags to a file nobody sees.
- mychess-gui has no mychess-db dev-dep: SI4-only GUI behaviour (Delete button enabled, read-only menu item) is untestable there.
