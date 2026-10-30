//! Keyboard move entry (spec rules 44-54), after the Scid's
//! `moveEntry_Char` and `sc_pos matchMoves`.

use mychess_core::{CastlingSide, Game, Move, NodeId, Position, SanError, SanMode, SanSuffix};

use super::message::PlayOptions;
use super::{GameSession, Notice};

/// A move that matches the text typed so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// What the user types to select it (`Nbd7`, `ed5`, `OO`, `e2e4`).
    pub text: String,
    /// The move.
    pub mv: Move,
}

/// The state of the keyboard entry: text typed and the moves it matches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoveEntry {
    text: String,
    candidates: Vec<Candidate>,
}

impl MoveEntry {
    /// The (normalized) text typed so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The moves that match [`MoveEntry::text`].
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    pub(super) fn clear(&mut self) {
        self.text.clear();
        self.candidates.clear();
    }
}

/// Normalizes a typed character; `None` ignores it.
///
/// Scid: `tcl/end.tcl` 1597-1607. Piece letters are upper case except the
/// bishop, which is also a file (`b`): both spellings are accepted.
fn normalize(ch: char) -> Option<char> {
    match ch {
        'n' | 'r' | 'q' | 'k' | 'o' => Some(ch.to_ascii_uppercase()),
        'N' | 'R' | 'Q' | 'K' | 'O' | 'B' | 'b' | 'a' | 'c'..='h' | '1'..='8' => Some(ch),
        'A' | 'C'..='H' => Some(ch.to_ascii_lowercase()),
        _ => None,
    }
}

/// The moves of `position` that start with `prefix` (port of Scid's
/// `sc_pos matchMoves`). The null move is never a candidate.
pub(super) fn candidates_for(
    position: &Position,
    prefix: &str,
    coordinate: bool,
) -> Vec<Candidate> {
    let Some(first) = prefix.chars().next() else {
        return Vec::new();
    };
    let legal = position.legal_moves();
    let short_castle_legal = legal
        .iter()
        .any(|m| m.castling_side() == Some(CastlingSide::KingSide));

    // Entry texts: SAN without `x`, `=`, `+`, `#`; castling as OO/OK/OQ.
    let mut texts: Vec<(String, Move)> = Vec::new();
    for &mv in &legal {
        match mv.castling_side() {
            Some(CastlingSide::KingSide) => {
                texts.push(("OO".to_owned(), mv));
                texts.push(("OK".to_owned(), mv));
            }
            Some(CastlingSide::QueenSide) => {
                texts.push(("OQ".to_owned(), mv));
                if !short_castle_legal {
                    texts.push(("OO".to_owned(), mv));
                }
            }
            None => {
                if let Ok(san) = position.san_with(mv, SanSuffix::None) {
                    let text = san
                        .chars()
                        .filter(|c| !matches!(c, 'x' | '=' | '+' | '#'))
                        .collect();
                    texts.push((text, mv));
                }
            }
        }
    }

    let mut out: Vec<Candidate> = Vec::new();
    let mut push = |text: String, mv: Move| {
        if !out.iter().any(|c| c.text == text) {
            out.push(Candidate { text, mv });
        }
    };
    for (text, mv) in &texts {
        if text.starts_with(prefix) {
            push(text.clone(), *mv);
        }
    }
    let file_prefix = matches!(first, 'a'..='h');
    if file_prefix {
        // A `b` may stand for a bishop: look up the prefix with a capital.
        let mut upper = String::with_capacity(prefix.len());
        upper.push(first.to_ascii_uppercase());
        upper.push_str(prefix.get(first.len_utf8()..).unwrap_or(""));
        for (text, mv) in &texts {
            if text.starts_with(&upper) {
                push(text.clone(), *mv);
            }
        }
    }
    if coordinate && file_prefix {
        for &mv in &legal {
            let mut text = position.uci(mv);
            if text.len() == 5
                && let Some(promotion) = text.pop()
            {
                text.push(promotion.to_ascii_uppercase());
            }
            if text.starts_with(prefix) {
                push(text, mv);
            }
        }
    }
    out
}

impl GameSession {
    fn candidates_at_cursor(&self, text: &str) -> Vec<Candidate> {
        candidates_at(
            &self.game,
            self.cursor,
            text,
            self.settings.move_entry.coordinate,
        )
    }

    pub(super) fn entry_char(&mut self, ch: char) {
        if !self.settings.move_entry.enabled {
            return;
        }
        let Some(ch) = normalize(ch) else {
            return;
        };
        let mut text = self.entry.text.clone();
        text.push(ch);
        let mut candidates = self.candidates_at_cursor(&text);
        if candidates.is_empty() && ch == 'b' {
            // `b` is not a pawn move here: it is the bishop (`B`).
            text.pop();
            text.push('B');
            candidates = self.candidates_at_cursor(&text);
        }
        if candidates.is_empty() {
            return;
        }
        self.entry.text = text;
        self.entry.candidates = candidates;

        let auto_expand = self.settings.move_entry.auto_expand;
        let typed = self.entry.text.as_str();
        let chosen = match self.entry.candidates.as_slice() {
            [only] if auto_expand || only.text.eq_ignore_ascii_case(typed) => Some(only.mv),
            // Pawn `b` and bishop `B`: only an exact match decides.
            [a, b] if a.text.eq_ignore_ascii_case(&b.text) => {
                [a, b].iter().find(|c| c.text == typed).map(|c| c.mv)
            }
            _ => None,
        };
        if let Some(mv) = chosen {
            self.entry.clear();
            self.play_move(mv, PlayOptions::default());
        }
    }

    /// A key typed at the board: the entry takes it, or the user is told
    /// that no legal move starts that way (spec M2-4, rule 63). Control
    /// characters and blanks are no keys of the entry and stay silent.
    pub(super) fn entry_typed(&mut self, ch: char) {
        if !self.settings.move_entry.enabled {
            return;
        }
        let before = (self.entry.text.clone(), self.cursor, self.changes);
        self.entry_char(ch);
        let unchanged =
            before.0 == self.entry.text && before.1 == self.cursor && before.2 == self.changes;
        if unchanged && !ch.is_control() && !ch.is_whitespace() {
            let mut shown = before.0;
            shown.push(ch);
            self.notice = Some(Notice::unreadable_entry(&shown));
        }
    }

    pub(super) fn entry_backspace(&mut self) {
        if !self.settings.move_entry.enabled || self.entry.text.pop().is_none() {
            return;
        }
        self.entry.candidates = if self.entry.text.is_empty() {
            Vec::new()
        } else {
            let text = self.entry.text.clone();
            self.candidates_at_cursor(&text)
        };
    }

    pub(super) fn entry_commit(&mut self) {
        if !self.settings.move_entry.enabled || self.entry.text.is_empty() {
            return;
        }
        let candidates = &self.entry.candidates;
        let chosen = candidates
            .iter()
            .find(|c| c.text == self.entry.text)
            .or(match candidates.as_slice() {
                [only] => Some(only),
                _ => None,
            })
            .map(|c| c.mv);
        if let Some(mv) = chosen {
            self.entry.clear();
            self.play_move(mv, PlayOptions::default());
        }
    }

    pub(super) fn enter_move_text(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let Ok(position) = self.game.position_at(self.cursor) else {
            self.notice = Some(Notice::StaleNode);
            return;
        };
        let text = text.trim();
        let san = position.parse_san(text, SanMode::Tolerant);
        let mv = match san {
            Ok(parsed) => Some(parsed.mv),
            Err(SanError::IllegalCastling { .. }) => {
                self.notice = Some(Notice::IllegalMove);
                return;
            }
            Err(SanError::NullMoveInCheck { .. }) => {
                self.notice = Some(Notice::NullMoveInCheck);
                return;
            }
            Err(_) => position.parse_uci(text).ok(),
        };
        match mv {
            Some(mv) => self.play_move(mv, PlayOptions::default()),
            None => self.notice = Some(Notice::unreadable(text)),
        }
    }
}

fn candidates_at(game: &Game, cursor: NodeId, text: &str, coordinate: bool) -> Vec<Candidate> {
    game.position_at(cursor)
        .map(|position| candidates_for(position, text, coordinate))
        .unwrap_or_default()
}
