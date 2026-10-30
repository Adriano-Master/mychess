---
name: pgn-reader-lessons
description: Verified pitfalls from implementing the mychess-pgn reader (chardetng, Game tree legality, chunker/lexer agreement)
metadata:
  type: project
---

- chardetng 1.0 guesses windows-1250 ("Ł") for a lone `0xA3` byte; below ~32 bytes of undecodable text keep windows-1252. Feed it whole words (not single invalid bytes, and no separator between bytes of one word) or it fails to find windows-1251.
- `Game::add_move_new` validates strictly (`Position::after`), so SAN accepted with `SanWarning::IllegalCastling` cannot enter the tree. mychess-core needs a lenient insertion (using `play_lenient`) for the reader to keep such moves.
- The chunker and the movetext lexer must agree on word boundaries (UTF-8 NBSP `C2 A0` as delimiter): a zero-length word loops forever. Shared rules live in `read/chars.rs`.
- Workspace lint `unreachable_pub` + `let_underscore_drop`: use `pub(crate)` in private modules and `drop(result)` instead of `let _ = result`.
- Determinism of the encoding guess (D18): the prescan must read until a fixed byte budget (or EOF) before deciding and examine only that budget; deciding on "whatever the first `read` returned" varies with the reader. `fill_up_to` appends without compacting so indices held by `emit` stay valid.
- NBSP at line start: chunker `line_step`, `scan_header`, movetext `at_line_start` and the junk check all share `chars::{blank_len, first_non_space, space_len}`; a trailing `C2` at buffer end must wait for more data.
- After a read error the chunker hands out the buffered tail as a normal `Ok(RawGame)` (as if EOF) and only then the `Err`; a caller that limits the reader (size cap) must check its own "limit hit" flag before looking at the item, or the cut tail is taken for a game.
