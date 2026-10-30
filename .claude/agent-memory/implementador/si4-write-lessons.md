---
name: si4-write-lessons
description: Verified facts from implementing the SI4 writer (mychess-db M3-2a): parity checks, stale START fields, lock caveat
metadata:
  type: project
---

- Parity against the 6 bundled bases is the best oracle: parse -> encode_record gives identical bytes for all 29,109 records; NameBuilder::to_bytes (frequencies recounted from entries) reproduces every .sn4 byte for byte; stored-line table codes match with no exceptions.
- Games with START (own FEN) in the bundled bases carry stale FinalMatSig (std-start value 6981288), NumHalfMoves and even record flag bits 1-2 (PROMO set with no promotion in the main line). Compare derived fields only for standard-start games; preserve `raw.flags & 6` in `encode_record` for byte-exact round trip.
- `Position::chess960_start(518)` equals the standard FEN, so the START test must compare `start.fen()` with the standard FEN string, not the castling mode alone.
- `File::try_lock` (std 1.89+) is mandatory on Windows: a live `Si4Writer` blocks reads of the .si4 by other handles; drop it before opening `Si4Database`.
- Pre-existing clippy errors in mychess-gui tests (`assert_is_empty`, geometry.rs, player_strip.rs) make `cargo xtask lint` fail workspace-wide; not related to mychess-db.
