// Derived from Scid vs PC 4.27, src/pgnparse.cpp (GetGameToken and ParseMoves).
// Copyright (c) 2001-2003 Shane Hudson. All rights reserved (notice of
// pgnparse.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The movetext lexer and tree builder.
//!
//! Variations use an explicit stack of [`Line`]s, never recursion, so
//! thousands of nested parentheses are fine. After the first error of a line
//! (D21) the rest of that line is skipped, tracking only parentheses and
//! results.

use mychess_core::{GameError, GameResult, Nag, SanError, SanWarning};

use super::chars::{blank_len, find_word_end, is_eol, result_token, space_len};
use super::diagnostics::DiagnosticKind;
use super::glyph::glyph_exact;
use super::options::SemicolonComments;
use super::parser::{GameParser, Line, MAX_DIAG_TEXT};
use super::text::clip;

/// How the movetext ended.
pub(super) enum MovetextEnd {
    /// A result token at this position.
    Result(GameResult, usize),
    /// A `[` at the start of a line, at this position.
    NextHeader(usize),
    /// The data ended.
    Eof,
    /// `max_nodes_per_game` was reached; the rest was not read.
    Limit,
}

/// What reading a move word led to.
#[derive(PartialEq, Eq)]
enum MoveOutcome {
    /// The move was added to the tree.
    Played,
    /// The line is being skipped.
    Skipped,
    /// The word is not a move (reported, the line goes on).
    Unexpected,
    /// The move is illegal: the rest of the line is discarded.
    Failed,
}

/// Length of the SAN-like prefix of a word: letters, digits, `.`, `:`, a
/// `-` between alphanumerics (or in `--`) and `=` before a piece letter.
fn core_len(w: &[u8]) -> usize {
    let mut i = 0;
    while i < w.len() {
        let next = w.get(i + 1).copied();
        let ok = match w[i] {
            b if b.is_ascii_alphanumeric() => true,
            b'.' | b':' => true,
            b'-' => {
                (i > 0 && w[i - 1] == b'-')
                    || next.is_some_and(|n| n.is_ascii_alphanumeric() || n == b'-')
            }
            b'=' => matches!(
                next,
                Some(b'Q' | b'R' | b'B' | b'N' | b'K' | b'q' | b'r' | b'b' | b'n' | b'k')
            ),
            _ => false,
        };
        if !ok {
            break;
        }
        i += 1;
    }
    i
}

/// Longest SAN-like text worth handing to `parse_san`.
const MAX_MOVE_LEN: usize = 12;

/// Cheap "looks like a move" screen applied before `parse_san`. It only
/// rejects what `parse_san` would reject with a syntax error anyway: texts
/// that are too long, do not start with a piece letter, file, castling
/// (`O`/`o`/`0`), null move (`-`, `null`, `Z0`) character, or hold bytes that
/// never occur in SAN.
fn looks_like_move(text: &[u8]) -> bool {
    if text.is_empty() || text.len() > MAX_MOVE_LEN {
        return false;
    }
    let first_ok = matches!(
        text[0],
        b'a'..=b'h'
            | b'K'
            | b'Q'
            | b'R'
            | b'B'
            | b'N'
            | b'P'
            | b'O'
            | b'o'
            | b'0'
            | b'-'
            | b'n'
            | b'z'
            | b'Z'
    );
    first_ok
        && text
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'=' | b':' | b'.'))
}

/// Whether `[` at `pos` is the first non-blank byte of its line (NBSP counts
/// as blank).
fn at_line_start(data: &[u8], pos: usize) -> bool {
    // Forward from the line start, so that the chunker's rule (blanks and
    // no-break spaces) applies unchanged.
    let line_start = data[..pos]
        .iter()
        .rposition(|&b| is_eol(b))
        .map_or(0, |i| i + 1);
    let mut i = line_start;
    loop {
        match blank_len(&data[i..]) {
            0 => return i == pos,
            n => i += n,
        }
    }
}

fn find_eol(data: &[u8], from: usize) -> usize {
    data[from..]
        .iter()
        .position(|&b| is_eol(b))
        .map_or(data.len(), |i| from + i)
}

impl GameParser<'_> {
    fn line(&self) -> &Line {
        // The stack always holds the main line.
        &self.stack[self.stack.len() - 1]
    }

    fn line_mut(&mut self) -> &mut Line {
        let last = self.stack.len() - 1;
        &mut self.stack[last]
    }

    /// Whether the current line is being skipped.
    fn inactive(&self) -> bool {
        let line = self.line();
        line.dead || line.skip > 0
    }

    /// Reads the movetext from `start` to its end.
    pub(super) fn run_movetext(&mut self, start: usize) -> MovetextEnd {
        let data = self.data;
        let len = data.len();
        let mut pos = start;
        loop {
            if self.stop {
                return MovetextEnd::Limit;
            }
            loop {
                let n = space_len(&data[pos.min(len)..]);
                if n == 0 {
                    break;
                }
                pos += n;
            }
            if pos >= len {
                return MovetextEnd::Eof;
            }
            match data[pos] {
                b'{' => pos = self.read_brace_comment(pos),
                b';' => pos = self.read_semicolon_comment(pos),
                b'%' if pos == 0 || is_eol(data[pos - 1]) => pos = find_eol(data, pos),
                b'[' if at_line_start(data, pos) => return MovetextEnd::NextHeader(pos),
                b'}' => {
                    if !self.stray_brace {
                        self.stray_brace = true;
                        self.diag(DiagnosticKind::StrayCloseBrace, pos, None);
                    }
                    pos += 1;
                }
                b'(' => {
                    self.open_variation(pos);
                    pos += 1;
                }
                b')' => {
                    self.close_variation(pos);
                    pos += 1;
                }
                b'$' => pos = self.read_nag_number(pos),
                _ => {
                    let end = find_word_end(data, pos, pos + 1).unwrap_or(len);
                    if let Some(result) = result_token(&data[pos..end]) {
                        return MovetextEnd::Result(result, pos);
                    }
                    self.handle_word(pos, end);
                    pos = end;
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Comments
    // ------------------------------------------------------------------

    fn read_brace_comment(&mut self, pos: usize) -> usize {
        let data = self.data;
        let body = pos + 1;
        let (end, next) = if let Some(i) = data[body..].iter().position(|&b| b == b'}') {
            (body + i, body + i + 1)
        } else {
            self.diag(DiagnosticKind::UnterminatedComment, pos, None);
            (data.len(), data.len())
        };
        if !self.inactive() {
            self.comment(&data[body..end], pos);
        }
        next
    }

    fn read_semicolon_comment(&mut self, pos: usize) -> usize {
        let data = self.data;
        let end = find_eol(data, pos);
        if self.opts.semicolon_comments == SemicolonComments::Keep && !self.inactive() {
            self.comment(&data[pos + 1..end], pos);
        }
        end
    }

    /// Attaches a comment to the last move of the line (D20, rule 19).
    fn comment(&mut self, bytes: &[u8], pos: usize) {
        if bytes.len() > self.opts.max_comment_len {
            self.diag(DiagnosticKind::CommentTruncated, pos, None);
        }
        let text = self.comment_string(bytes);
        if text.is_empty() {
            return;
        }
        let depth = self.stack.len();
        let (cursor, has_move) = {
            let line = self.line();
            (line.cursor, line.has_move)
        };
        if has_move || depth == 1 {
            // Nodes of the line exist, so this cannot fail.
            drop(self.game.append_comment(cursor, &text));
        } else {
            let pending = &mut self.line_mut().pending_before;
            match pending {
                Some(existing) => {
                    existing.push(' ');
                    existing.push_str(&text);
                }
                None => *pending = Some(text),
            }
        }
    }

    // ------------------------------------------------------------------
    // Variations
    // ------------------------------------------------------------------

    fn open_variation(&mut self, pos: usize) {
        if self.inactive() {
            let line = self.line_mut();
            line.skip = line.skip.saturating_add(1);
            return;
        }
        let (cursor, has_move) = {
            let line = self.line();
            (line.cursor, line.has_move)
        };
        if !has_move {
            self.diag(DiagnosticKind::VariationBeforeFirstMove, pos, None);
            self.line_mut().skip = 1;
            return;
        }
        match self.game.parent(cursor) {
            Ok(Some(parent)) => self.stack.push(Line {
                cursor: parent,
                has_move: false,
                dead: false,
                skip: 0,
                pending_before: None,
            }),
            _ => self.line_mut().skip = 1,
        }
    }

    fn close_variation(&mut self, pos: usize) {
        let depth = self.stack.len();
        let line = self.line_mut();
        if line.skip > 0 {
            line.skip -= 1;
        } else if depth > 1 {
            self.stack.pop();
        } else if !line.dead {
            self.diag(DiagnosticKind::UnbalancedCloseParen, pos, None);
        }
    }

    // ------------------------------------------------------------------
    // NAGs
    // ------------------------------------------------------------------

    fn add_nag(&mut self, nag: Nag, pos: usize) {
        if self.inactive() {
            return;
        }
        let (cursor, has_move) = {
            let line = self.line();
            (line.cursor, line.has_move)
        };
        if has_move {
            // A move node: cannot fail. A repeated NAG is stored once.
            drop(self.game.add_nag(cursor, nag));
        } else {
            self.diag(DiagnosticKind::NagWithoutMove, pos, None);
        }
    }

    /// `$n`; returns the position after the number.
    fn read_nag_number(&mut self, pos: usize) -> usize {
        let data = self.data;
        let digits_start = pos + 1;
        let digits = data[digits_start..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        let end = digits_start + digits;
        if self.inactive() {
            return end;
        }
        if digits == 0 {
            self.diag_token(DiagnosticKind::InvalidNagSymbol, pos, b"$");
            return end;
        }
        let value = data[digits_start..end].iter().fold(0u32, |n, d| {
            n.saturating_mul(10).saturating_add(u32::from(d - b'0'))
        });
        match u16::try_from(value).ok().map(Nag::new) {
            Some(Ok(nag)) => self.add_nag(nag, pos),
            // `$0` means "no annotation".
            _ if value == 0 => {}
            _ => self.diag_token(DiagnosticKind::NagOutOfRange, pos, &data[pos..end]),
        }
        end
    }

    // ------------------------------------------------------------------
    // Words
    // ------------------------------------------------------------------

    /// Handles a word that is not a result: a move number, a move with
    /// annotations, or a glyph.
    fn handle_word(&mut self, start: usize, end: usize) {
        let word = &self.data[start..end];
        let digits = word.iter().take_while(|b| b.is_ascii_digit()).count();
        let dots = word[digits..].iter().take_while(|&&b| b == b'.').count();
        if digits + dots == word.len() {
            return; // a move number, `12.`, `12...`, `...` or `.`
        }
        // `12.e4` and `...e5`: skip the number, keep the move.
        let skip = if digits == 0 || dots > 0 {
            digits + dots
        } else {
            0
        };
        let rest = &word[skip..];
        let rest_pos = start + skip;
        if matches!(rest, b"ep" | b"e.p." | b"e.p") {
            return;
        }
        if self.inactive() {
            return;
        }
        if let Some(nag) = glyph_exact(rest) {
            self.add_nag(nag, rest_pos);
            return;
        }
        let core = core_len(rest);
        let (move_text, annotations) = rest.split_at(core);
        if !move_text.is_empty() && self.play_move(move_text, rest_pos) != MoveOutcome::Played {
            return;
        }
        self.annotations(annotations, rest_pos + core, !move_text.is_empty());
    }

    /// Reads the symbols that follow a move: `!`, `?`, `+`, `#`, `+-`...
    fn annotations(&mut self, ann: &[u8], pos: usize, after_move: bool) {
        let mut i = 0;
        while i < ann.len() {
            let c = ann[i];
            if c == b'!' || c == b'?' {
                let j = if matches!(ann.get(i + 1), Some(b'!' | b'?')) {
                    i + 2
                } else {
                    i + 1
                };
                match glyph_exact(&ann[i..j]) {
                    Some(nag) => self.add_nag(nag, pos + i),
                    None => {
                        self.diag_token(DiagnosticKind::InvalidNagSymbol, pos + i, &ann[i..j]);
                    }
                }
                i = j;
                continue;
            }
            let longest = (1..=(ann.len() - i).min(6))
                .rev()
                .find_map(|n| glyph_exact(&ann[i..i + n]).map(|nag| (n, nag)));
            if let Some((n, nag)) = longest {
                self.add_nag(nag, pos + i);
                i += n;
            } else if c == b'+' || c == b'#' {
                // Check and mate marks are not stored (the writer recomputes them).
                while i < ann.len() && matches!(ann[i], b'+' | b'#') {
                    i += 1;
                }
            } else {
                // Text that follows a move is a bad symbol; on its own it is a
                // bad token.
                let kind = if after_move {
                    DiagnosticKind::InvalidNagSymbol
                } else {
                    DiagnosticKind::UnexpectedToken
                };
                self.diag_token(kind, pos + i, &ann[i..]);
                return;
            }
        }
    }

    // ------------------------------------------------------------------
    // Moves
    // ------------------------------------------------------------------

    fn play_move(&mut self, text: &[u8], pos: usize) -> MoveOutcome {
        if self.inactive() {
            return MoveOutcome::Skipped;
        }
        let Ok(san) = std::str::from_utf8(text) else {
            self.diag_token(DiagnosticKind::UnexpectedToken, pos, text);
            return MoveOutcome::Unexpected;
        };
        if !looks_like_move(text) {
            // Cheap syntax screen: same outcome `parse_san` would give
            // (a syntax error), without generating legal moves for junk.
            self.diag(
                DiagnosticKind::UnexpectedToken,
                pos,
                Some(clip(san, MAX_DIAG_TEXT)),
            );
            return MoveOutcome::Unexpected;
        }
        if self.game.node_count() > self.opts.max_nodes_per_game {
            return self.node_limit(san, pos);
        }
        let cursor = self.line().cursor;
        let Ok(position) = self.game.position_at(cursor) else {
            self.line_mut().dead = true;
            return MoveOutcome::Failed;
        };
        let parsed = position.parse_san(san, self.opts.san_mode);
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(error) => return self.san_failure(error, san, pos),
        };
        match self.game.add_move_lenient(cursor, parsed.mv) {
            Ok((id, created)) => {
                for warning in &parsed.warnings {
                    let kind = match warning {
                        SanWarning::IllegalCastling => DiagnosticKind::IllegalCastling,
                        SanWarning::CastlingReinterpreted => DiagnosticKind::CastlingReinterpreted,
                        _ => continue,
                    };
                    self.diag(kind, pos, Some(clip(san, MAX_DIAG_TEXT)));
                }
                if !created {
                    self.diag(
                        DiagnosticKind::DuplicateVariation,
                        pos,
                        Some(clip(san, MAX_DIAG_TEXT)),
                    );
                }
                let pending = {
                    let line = self.line_mut();
                    line.cursor = id;
                    line.has_move = true;
                    line.pending_before.take()
                };
                if let Some(text) = pending
                    && matches!(self.game.comment_before(id), Ok(None))
                {
                    // `id` is a move node: cannot fail.
                    drop(self.game.set_comment_before(id, &text));
                }
                MoveOutcome::Played
            }
            Err(GameError::TooManyNodes) => self.node_limit(san, pos),
            Err(_) => {
                // Not even the lenient insertion takes it: really illegal.
                let fen = self
                    .game
                    .position_at(cursor)
                    .map(mychess_core::Position::fen)
                    .unwrap_or_default();
                let error = SanError::Illegal {
                    text: clip(san, MAX_DIAG_TEXT),
                    fen,
                };
                self.move_error(DiagnosticKind::IllegalMove(error), san, pos)
            }
        }
    }

    /// The game has too many nodes: report it once and stop reading.
    fn node_limit(&mut self, san: &str, pos: usize) -> MoveOutcome {
        self.diag(
            DiagnosticKind::TooManyNodes,
            pos,
            Some(clip(san, MAX_DIAG_TEXT)),
        );
        self.stop = true;
        MoveOutcome::Failed
    }

    fn san_failure(&mut self, error: SanError, san: &str, pos: usize) -> MoveOutcome {
        let kind = match error {
            SanError::Empty | SanError::TooLong { .. } | SanError::Syntax { .. } => {
                self.diag(
                    DiagnosticKind::UnexpectedToken,
                    pos,
                    Some(clip(san, MAX_DIAG_TEXT)),
                );
                return MoveOutcome::Unexpected;
            }
            SanError::Ambiguous { .. } => DiagnosticKind::AmbiguousMove(error),
            SanError::Illegal { .. }
            | SanError::IllegalCastling { .. }
            | SanError::NullMoveInCheck { .. } => DiagnosticKind::IllegalMove(error),
            _ => DiagnosticKind::InvalidMove(error),
        };
        self.move_error(kind, san, pos)
    }

    /// First error of a line (D21): report it, add the comment on the first
    /// error of the game, and discard the rest of the line.
    fn move_error(&mut self, kind: DiagnosticKind, san: &str, pos: usize) -> MoveOutcome {
        let token = clip(san, MAX_DIAG_TEXT);
        self.diag(kind, pos, Some(token.clone()));
        if !self.error_commented {
            self.error_commented = true;
            let cursor = self.line().cursor;
            // The cursor is a live node of the game.
            drop(
                self.game
                    .append_comment(cursor, &format!("Error reading move: {token}")),
            );
        }
        let line = self.line_mut();
        line.dead = true;
        line.pending_before = None;
        MoveOutcome::Failed
    }
}
