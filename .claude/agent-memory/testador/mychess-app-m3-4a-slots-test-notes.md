---
name: mychess-app-m3-4a-slots-test-notes
description: How the m3_4a_* contract tests of mychess-app drive slots/Clipbase and Scid quirks found (EventDate year, mainline count)
metadata:
  type: project
---

`crates/mychess-app/tests/m3_4a_common/mod.rs` has `Rig` (AppState with `open_pgn_in_slots = true` + current-thread
runtime): `send` = update + perform of every non-host effect; `raw` = update only (catch an `Effect::OpenBase` /
`LoadBaseGame` and answer it late to test stale requests); `make_si4` builds an `.si4` through `Si4Writer::create` +
`import_pgn`. Filters of a slot can only be changed from inside the crate (`Bases::slot_mut` is `pub(crate)`):
those tests live in `src/bases/contract_tests.rs`.

Traps (verified):
- `game.mainline().count()` equals the number of plies (root not counted); one ply => 1.
- SI4 `EventDate` stores the year as a 3-bit offset from the game year; with an unknown game year the event date
  collapses to unknown, so the list shows `????` (Scid `SetEventDate` does the same). Date column of `????.05.??` is `????.05`.
- Scid `Length` = `(half moves + 1)/2`; `EndMaterial` single piece lower case, pair upper case, pawns digit before `:`.
- Integration tests of mychess-app need file-level `allow(clippy::assert_is_empty, clippy::format_push_string)`.
