---
name: crash-failpoint-lessons
description: M7-1a2/M7-3a fixes: FailPoint kill mode, repeat-after-kill stale sn4 frequencies, sink-triggered file intrusion tests
metadata:
  type: project
---

- `FailPoint::kill_at_label` (journal.rs) = injected failure WITHOUT cleanup for `strip_games`/`edit_names`; `at_label` still cuts the `.sg4`. Labels: `rewrite:appended|entries|mid`, `names:chunk` (per chunk, write pass).
- Writer open recounts name frequencies from entries (NameBuilder starts at 0), so a repeat after a kill is consistent; but the early "nothing changed" return of `edit_names` skipped the final `.sn4` write, leaving stale frequencies: fixed by comparing disk with `to_bytes()` (Names plan only).
- To simulate a foreign process touching a file mid-operation in tests, use the progress sink callback (fires at game 256, `done=255`, then every 256) with a second handle; unix-only because Windows locks the file.
- Fabricate a tag-255 record in tests: `db.raw_game(1)`, set `raw.event_date`, `encode_record`, append to `.sg4`, patch entry offset/length bytes (entry 1 at byte 182).
- `cargo fmt --all` reformats other agents' in-flight files; clippy `-D warnings --all-targets` can fail on their tests (m7_4a_*): filter by path.
