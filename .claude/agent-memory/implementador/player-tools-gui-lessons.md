---
name: player-tools-gui-lessons
description: M6-3c GUI lessons - Simulator find() sees widgets behind modals, scripted regex edits hit production code, parallel agents share menu test lists
metadata:
  type: project
---

- `Simulator::find("text")` matches widgets behind a modal too (the game list shows the same names): click rows by `container(..).id(Id::from(String))`, and for text checks use a `Candidate::Text` closure with `contains`.
- Fluent: a `$kind` that already holds a noun ("player names") breaks "{ $kind } name" templates; put it in parentheses. `games-job-*` keys must live in `games.ftl` (m3_4c scans only games.ftl/status.ftl), and `m4_3_gui_i18n` has a list of ftl files that Tools-menu keys may come from.
- Tools-menu keys must not start with `menu-tools-tournament` (m4_3 test forbids the old engine-tournament keys): used `menu-tools-find-*`.
- Shared menu tests (`m4_3_gui_keys_menu`, `m9_2a_menus`, `menu_bar.rs` unit, `ribbon.rs` unit) list the exact Tools entries and the group count; a regex "move this block" over the whole file also matches production code. Edit with exact anchors.
- A modal whose Add Elo precondition fails never opens: the notice lands in `shell.ui.status_notice`, not in the dialog.
