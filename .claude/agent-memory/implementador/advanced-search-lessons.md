---
name: advanced-search-lessons
description: M6-1a lessons - relative CARGO_TARGET_DIR trap, core rejects >16 pieces, pruning unsound in variation DFS, sso parser reuse
metadata:
  type: feedback
---

- `CARGO_TARGET_DIR=target/f1` is resolved against the cwd of the shell: after `cd crates/mychess-db` it builds inside the crate. Use the absolute path; env vars do not persist between Bash calls.
  **Why:** a stray `crates/mychess-db/target` appeared in M6-1a. **How to apply:** always prefix the absolute path on every cargo call.
- `Position::from_fen` refuses more than 16 pieces for a side, so `SearchError::UnsupportedPosition` (ListPosition::from_fen) cannot be triggered from a core `Position`; do not write a test expecting it.
- "Count only decreases" pruning (dead target) is valid along one line, not across sibling variations in a DFS: disable it there.
- Another agent edits `mychess-db` concurrently (M6-2a): `cargo fmt --all` / `clippy --all-targets` touch or break on their files; check only own files with `rustfmt --check <files>` and explicit `--test` targets.
- SI4 index entry: bytes 34-36 are FinalMatSig (byte 33 is the stored-line code); 38-46 home-pawn data. Handy to spoil the index in tests.
