---
name: mychess-gui-m3-4b-test-notes
description: How the m3_4b_* GUI tests drive the game list panel headless, and traps found (filters only internal, Rig messages drop effects)
metadata:
  type: project
---

`crates/mychess-gui/tests/m3_4b_common/mod.rs`: `open` (Open effect run, answer fed through the shell so the panel reveals),
`list` (runs `GameListUi::update` output through `Rig::send` so LoadGame effects execute; `rig.msg(Message::Games(..))` drops effects),
`rows_built` (counts `games-row-{n}` ids in ONE simulator). `Subscription::units()` tells whether the 150 ms timer exists.
Row looks tested by pixel probe: `render_common::render(rig.shell.view())` + `Target::bounds()` of the row id.

Traps (verified): the GUI cannot change a slot's filter (`slot_mut` internal) so `games-empty-filter` and deleted-row dimming are only
reachable by unit tests; an unreadable row needs a corrupt SI4 and mychess-gui has no mychess-db dev-dep.
`widget::Id::new` takes `&'static str`; use `widget::Id::from(String)`. The status of a file error lives in `ui.status_notice` or `app.notice`.
