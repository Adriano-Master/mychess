---
name: spelling-core-lessons
description: M6-3a lessons - env vars do not persist between Bash calls (target dir), rustfmt after scripted edits, Filter::new starts full, Scid rating type numbering
metadata:
  type: feedback
---

- Prefix EVERY cargo call with `CARGO_BUILD_JOBS=3 CARGO_TARGET_DIR=/abs/target/f1`; an `export` in an earlier Bash call is gone, and the build silently lands in `target/debug`.
- Edits made through python/sed bypass the rustfmt hook: run `rustfmt --edition 2024 <own files>` at the end (never on `lib.rs` blindly: it formats every module in the tree; check with `--check` first).
- `Filter::new(n)` starts with every game IN the filter; call `fill(0)` before building a test filter.
- Scid rating type numbers: Elo 0, Rating 1, Rapid 2, ICCF 3, USCF 4, DWZ 5, ECF 6.
- The Scid `@TIPO` parser rejects `@pl "x"` (spaces break `strIsAlphaPrefix`) although the spec test expects it to open the player section; MyChess accepts it.
