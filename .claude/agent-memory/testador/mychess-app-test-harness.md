---
name: mychess-app-test-harness
description: How the m2_2_* integration tests of mychess-app drive update/perform (Harness pattern) and traps found while writing them
metadata:
  type: project
---

`crates/mychess-app/tests/m2_2_common/mod.rs` has a `Harness` (AppState + current-thread tokio runtime):
`send(msg)` runs `update`, feeds every non-host effect through `perform` and back into `update`
(skips `ScheduleAutoplayTick`, which would sleep) and returns the host effects; `raw(msg)` is update only
(use it to catch an effect and answer it late/stale); `pick(Some(path))` answers the last `PickFile`.
Other test crates cannot import it across files: use `mod m2_2_common;` in each test file.

Traps (verified):
- `tokio::test(flavor = "multi_thread")` does not compile: the crate only enables `rt` (current_thread).
- Playing a move at the root of a game that already has moves opens a "replace/variation" prompt in the
  session and the move is NOT played: navigate to `Nav::GameEnd` first.
- `Game` comments lose `{}` on PGN write (brace replacement): not a lossless round-trip for such comments.
- Rule 40 of the M2-2 spec (ECO "B9" invalid vs "C1" valid) is a spec error: core `Eco::parse_prefix`
  accepts both (B9 -> B90, C1 -> C10).
- Scid `sc_game info` comment cut (tkscid.cpp:7770-7815): copy 70 chars, finish the current word,
  add "..." only if text remains; the impl matches this.
