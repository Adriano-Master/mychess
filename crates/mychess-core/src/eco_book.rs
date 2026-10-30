//! ECO opening book: parsing of `.eco` text, lookup by position and game
//! classification (Scid's `PBook::ReadEcoFile`, `sc_eco game|base|summary|find`).
//!
//! This module is pure: it parses text already in memory. Reading the file
//! (decoding, `.gz`) belongs to `mychess-pgn`.
//!
//! ```
//! use mychess_core::{ClassifyOptions, EcoBook, Position, SanMode};
//!
//! let book = EcoBook::parse("B20 \"Sicilian\" 1.e4 c5 *\nA00a \"Start\" *\n")?;
//! let mut pos = Position::new();
//! let mut line = Vec::new();
//! for san in ["e4", "c5", "Nf3"] {
//!     let mv = pos.parse_san(san, SanMode::Strict)?.mv;
//!     line.push(mv);
//!     pos.play(mv)?;
//! }
//! let found = book
//!     .classify_line(&Position::new(), &line, ClassifyOptions::game())
//!     .expect("classified");
//! assert_eq!((found.ply, found.entry().eco().to_string()), (2, "B20".into()));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::HashMap;

use thiserror::Error;

use shakmaty::{File, Role};

use crate::CastlingMode;
use crate::eco::Eco;
use crate::game::Game;
use crate::moves::Move;
use crate::position::Position;
use crate::san::SanMode;

/// Largest accepted size, in bytes, of one entry (name or move text).
const MAX_ENTRY_BYTES: usize = 1 << 20;
/// Largest accepted number of half-moves of one entry.
const MAX_ENTRY_PLIES: u32 = 1024;
/// Largest number of diagnostics kept; the rest is only counted.
const MAX_DIAGNOSTICS: usize = 10_000;
/// Scid's `PBOOK_MAX_MATERIAL`: pieces in the initial position.
const MAX_MATERIAL: u32 = 32;
/// Plies searched by Scid when classifying a whole database (`sc_eco base`).
const BATCH_MAX_PLY: u32 = 60;

/// Why an `.eco` entry is corrupt.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CorruptReason {
    /// The two characters after the letter `A`-`E` are not digits.
    #[error("the ECO code is not a letter followed by two digits")]
    BadCode,
    /// The end of the text came before the closing quote of the name.
    #[error("the opening name has no closing quote")]
    UnterminatedName,
    /// The end of the text came before the `*` that ends the moves.
    #[error("the moves have no closing '*'")]
    UnterminatedMoves,
    /// A move is not legal (or not understood) in the position it is played.
    #[error("illegal or unreadable move {text:?}")]
    IllegalMove {
        /// The offending word, clipped.
        text: String,
    },
    /// The entry is larger than 1 MiB.
    #[error("the entry is too large")]
    TooLarge,
    /// The line has more than 1024 half-moves.
    #[error("the line has too many moves")]
    TooManyPlies,
}

/// Error loading an `.eco` text.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum EcoBookError {
    /// The text is corrupt; no partial book is returned.
    #[error("corrupt ECO text at line {line}: {reason}")]
    Corrupt {
        /// 1-based line where the problem was detected.
        line: usize,
        /// What is wrong.
        reason: CorruptReason,
    },
}

/// What to do with a corrupt entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OnError {
    /// Stop with [`EcoBookError::Corrupt`] (Scid's behaviour).
    #[default]
    Abort,
    /// Drop the entry, report an [`EcoDiagnostic::EntrySkipped`] and go on.
    ///
    /// After a malformed entry the parser resumes at the next line (or after
    /// the `*` when the moves were already read), so hostile input yields
    /// one diagnostic per line at most. Only the first 10 000 diagnostics
    /// are kept ([`EcoReport::omitted_diagnostics`] counts the rest).
    SkipEntry,
}

/// Options of [`EcoBook::parse_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct EcoParseOptions {
    /// Policy for corrupt entries.
    pub on_error: OnError,
}

impl EcoParseOptions {
    /// Options that drop corrupt entries instead of aborting.
    pub fn skip_entry() -> EcoParseOptions {
        EcoParseOptions::default().with_on_error(OnError::SkipEntry)
    }

    /// Sets the policy for corrupt entries.
    #[must_use]
    pub fn with_on_error(mut self, on_error: OnError) -> EcoParseOptions {
        self.on_error = on_error;
        self
    }
}

/// A non-fatal remark made while loading.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EcoDiagnostic {
    /// An entry reached a position already in the book; the new entry
    /// replaced the old data (keeping its order), as `PBook::Insert` does.
    DuplicatePosition {
        /// Line of the `*` of the new entry.
        line: usize,
        /// Line of the `*` of the first entry that reached this position.
        first_line: usize,
    },
    /// A corrupt entry was dropped ([`OnError::SkipEntry`]).
    EntrySkipped {
        /// Line where the problem was detected.
        line: usize,
        /// What was wrong.
        reason: CorruptReason,
    },
}

/// Result of [`EcoBook::parse_with`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EcoReport {
    /// The loaded book.
    pub book: EcoBook,
    /// Remarks, in text order (at most 10 000; see `omitted_diagnostics`).
    pub diagnostics: Vec<EcoDiagnostic>,
    /// Remarks dropped because `diagnostics` was full.
    pub omitted_diagnostics: usize,
    /// Entries read, repeated positions included (`book.len()` counts
    /// distinct positions).
    pub entries_read: usize,
}

/// One opening of the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcoEntry {
    eco: Eco,
    name: String,
    moves: String,
    plies: u32,
    line: usize,
    position: Position,
}

impl EcoEntry {
    /// The (extended) ECO code.
    pub fn eco(&self) -> Eco {
        self.eco
    }

    /// The opening name, as written in the file (unquoted).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The moves as written, whitespace collapsed (`1.f3 e5 2.Kf2`).
    pub fn moves(&self) -> &str {
        &self.moves
    }

    /// Number of half-moves of the line.
    pub fn plies(&self) -> u32 {
        self.plies
    }

    /// Line of the closing `*` in the source text.
    pub fn line(&self) -> usize {
        self.line
    }

    /// The position reached by the line.
    pub fn position(&self) -> &Position {
        &self.position
    }

    fn label(&self) -> String {
        format!("{} [{}]", self.eco, self.name)
    }
}

/// The deepest book position found in a game by [`EcoBook::classify`].
#[derive(Debug, Clone, Copy)]
pub struct EcoMatch<'a> {
    entry: &'a EcoEntry,
    /// Half-moves played to reach the matching position (0 = start).
    pub ply: u32,
}

impl<'a> EcoMatch<'a> {
    /// The matching book entry.
    pub fn entry(&self) -> &'a EcoEntry {
        self.entry
    }

    /// The code to store: the extended one, or its basic form
    /// (`extended = false`).
    pub fn eco(&self, extended: bool) -> Eco {
        if extended {
            self.entry.eco
        } else {
            self.entry.eco.basic()
        }
    }
}

/// How deep [`EcoBook::classify`] looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ClassifyOptions {
    /// Last half-move examined; `None` examines the whole main line.
    pub max_ply: Option<u32>,
}

impl ClassifyOptions {
    /// Examines at most `max_ply` half-moves.
    #[must_use]
    pub fn with_max_ply(mut self, max_ply: u32) -> ClassifyOptions {
        self.max_ply = Some(max_ply);
        self
    }

    /// Whole game, like `sc_eco game`.
    pub fn game() -> ClassifyOptions {
        ClassifyOptions { max_ply: None }
    }

    /// At most 60 half-moves, like `sc_eco base`.
    pub fn batch() -> ClassifyOptions {
        ClassifyOptions {
            max_ply: Some(BATCH_MAX_PLY),
        }
    }
}

impl Default for ClassifyOptions {
    fn default() -> Self {
        ClassifyOptions::game()
    }
}

/// An ECO book: immutable once loaded, `Send + Sync`.
#[derive(Debug, Clone, Default)]
pub struct EcoBook {
    entries: Vec<EcoEntry>,
    index: HashMap<Position, usize>,
    fewest_pieces: Option<u32>,
}

impl EcoBook {
    /// Parses `.eco` text, aborting at the first corrupt entry.
    ///
    /// Format: `CODE "Name" moves *`, with `#` comments between entries. See
    /// `docs/specs/m1-4-epd-eco.md` for the rules inherited from Scid.
    ///
    /// # Deliberate differences from Scid
    ///
    /// - Positions are keyed by [`Position`] equality (clocks ignored), where
    ///   Scid uses a compact string that keeps the en passant square when a
    ///   pawn could capture even illegally (spec D40).
    /// - Only standard-castling games are classified; Chess960 yields `None`.
    /// - Limits against hostile input: names and move texts up to 1 MiB, at
    ///   most 1024 half-moves per entry, at most 10 000 diagnostics.
    /// - The moves are played with `SanMode::Tolerant` after Scid's word
    ///   reduction, so a few ambiguous or odd words Scid accepts may differ.
    pub fn parse(text: &str) -> Result<EcoBook, EcoBookError> {
        Ok(EcoBook::parse_with(text, EcoParseOptions::default())?.book)
    }

    /// Like [`EcoBook::parse`], with options and diagnostics.
    ///
    /// A position reached twice keeps its first place in the order but takes
    /// the data of the last entry.
    pub fn parse_with(text: &str, options: EcoParseOptions) -> Result<EcoReport, EcoBookError> {
        let mut sc = Scanner {
            b: text.as_bytes(),
            i: 0,
            line: 1,
        };
        let mut book = EcoBook::default();
        let mut first_lines: Vec<usize> = Vec::new();
        let mut diagnostics = Vec::new();
        let mut omitted = 0usize;
        let mut note = |d: EcoDiagnostic| {
            if diagnostics.len() < MAX_DIAGNOSTICS {
                diagnostics.push(d);
            } else {
                omitted += 1;
            }
        };
        let mut entries_read = 0;
        while let Some(start) = sc.find_code() {
            let (line, reason) = match sc.read_entry(start) {
                Ok(raw) => match play_line(&raw.moves) {
                    Ok((position, plies)) => {
                        entries_read += 1;
                        let pieces =
                            u32::try_from(position.pieces().count()).unwrap_or(MAX_MATERIAL);
                        book.fewest_pieces =
                            Some(book.fewest_pieces.map_or(pieces, |f| f.min(pieces)));
                        let entry = EcoEntry {
                            eco: raw.eco,
                            name: raw.name,
                            moves: raw.moves,
                            plies,
                            line: sc.line,
                            position: position.clone(),
                        };
                        if let Some(&slot) = book.index.get(&position) {
                            note(EcoDiagnostic::DuplicatePosition {
                                line: sc.line,
                                first_line: first_lines.get(slot).copied().unwrap_or(sc.line),
                            });
                            if let Some(old) = book.entries.get_mut(slot) {
                                *old = entry;
                            }
                        } else {
                            book.index.insert(position, book.entries.len());
                            first_lines.push(sc.line);
                            book.entries.push(entry);
                        }
                        continue;
                    }
                    // The whole entry (up to `*`) was consumed.
                    Err(reason) => (sc.line, reason),
                },
                Err((line, reason)) => {
                    if options.on_error == OnError::SkipEntry {
                        // Resynchronise: one error per line at most.
                        sc.skip_line();
                    }
                    (line, reason)
                }
            };
            match options.on_error {
                OnError::Abort => return Err(EcoBookError::Corrupt { line, reason }),
                OnError::SkipEntry => note(EcoDiagnostic::EntrySkipped { line, reason }),
            }
        }
        Ok(EcoReport {
            book,
            diagnostics,
            omitted_diagnostics: omitted,
            entries_read,
        })
    }

    /// Number of distinct positions (what Scid prints when loading).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the book has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries in insertion order.
    pub fn entries(&self) -> &[EcoEntry] {
        &self.entries
    }

    /// Fewest pieces (both colours) in any book position; 32 when empty.
    pub fn fewest_pieces(&self) -> u32 {
        self.fewest_pieces.unwrap_or(MAX_MATERIAL)
    }

    /// The entry of exactly this position (clocks ignored).
    pub fn lookup(&self, position: &Position) -> Option<&EcoEntry> {
        self.index.get(position).and_then(|&i| self.entries.get(i))
    }

    /// Material only decreases (promotions keep the count), so once a
    /// position has fewer pieces than any book position, nothing deeper can
    /// match (Scid's `fewestPieces` cut-off).
    fn below_fewest_pieces(&self, position: &Position) -> bool {
        let fewest = self.fewest_pieces();
        position.pieces().take(fewest as usize).count() < fewest as usize
    }

    /// Classifies the main line of `game`: the deepest position (at most
    /// `max_ply`) that is in the book, so transpositions match. Variations
    /// are ignored. `None` when nothing matches, or for Chess960 games.
    pub fn classify(&self, game: &Game, options: ClassifyOptions) -> Option<EcoMatch<'_>> {
        let start = game.start_position();
        if start.castling_mode() != CastlingMode::Standard {
            return None;
        }
        let limit = options.max_ply.map_or(usize::MAX, |m| m as usize);
        let mut best = self.lookup(start).map(|entry| EcoMatch { entry, ply: 0 });
        for (idx, node) in game.mainline().enumerate() {
            if idx >= limit {
                break;
            }
            let Ok(position) = game.position_at(node) else {
                break;
            };
            if self.below_fewest_pieces(position) {
                break;
            }
            if let Some(entry) = self.lookup(position) {
                let ply = u32::try_from(idx + 1).unwrap_or(u32::MAX);
                best = Some(EcoMatch { entry, ply });
            }
        }
        best
    }

    /// Like [`EcoBook::classify`] for a plain list of moves from `start`.
    /// Stops at the first illegal move.
    pub fn classify_line(
        &self,
        start: &Position,
        moves: &[Move],
        options: ClassifyOptions,
    ) -> Option<EcoMatch<'_>> {
        if start.castling_mode() != CastlingMode::Standard {
            return None;
        }
        let limit = options.max_ply.map_or(usize::MAX, |m| m as usize);
        let mut best = self.lookup(start).map(|entry| EcoMatch { entry, ply: 0 });
        let mut pos = start.clone();
        for (idx, &mv) in moves.iter().enumerate() {
            if idx >= limit || pos.play(mv).is_err() || self.below_fewest_pieces(&pos) {
                break;
            }
            if let Some(entry) = self.lookup(&pos) {
                let ply = u32::try_from(idx + 1).unwrap_or(u32::MAX);
                best = Some(EcoMatch { entry, ply });
            }
        }
        best
    }

    /// Entries whose code starts with `prefix`, thinned like Scid's
    /// `EcoSummary`: for a prefix shorter than 3 characters, an entry that
    /// shares at least `len + 1` characters of `code [name]` with the last
    /// one shown is skipped.
    pub fn summary(&self, prefix: &str) -> Vec<&EcoEntry> {
        let depth = prefix.len();
        let mut shown: Vec<&EcoEntry> = Vec::new();
        let mut prev = String::new();
        for entry in &self.entries {
            let label = entry.label();
            if !label.starts_with(prefix) {
                continue;
            }
            if depth < 3 && common_prefix(&label, &prev) > depth {
                continue;
            }
            prev = label;
            shown.push(entry);
        }
        shown
    }

    /// Entries whose `code [name]` text contains `text` (case sensitive).
    ///
    /// Like Scid's `EcoFind` (`pbook.cpp:451`), only the code and name are
    /// searched; the moves are merely printed next to a hit, not compared.
    pub fn find(&self, text: &str) -> Vec<&EcoEntry> {
        self.entries
            .iter()
            .filter(|e| e.label().contains(text))
            .collect()
    }
}

/// Translates an opening name with a glossary of `(from, to)` pairs given in
/// registration order. Like Scid, which prepends each pair to its list, the
/// pairs are applied from the last registered to the first, replacing every
/// occurrence.
pub fn translate_eco_name(name: &str, glossary: &[(&str, &str)]) -> String {
    let mut out = name.to_owned();
    for (from, to) in glossary.iter().rev() {
        if !from.is_empty() && out.contains(from) {
            out = out.replace(from, to);
        }
    }
    out
}

/// Length in bytes of the common prefix.
fn common_prefix(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

struct RawEntry {
    eco: Eco,
    name: String,
    moves: String,
}

type Failure = (usize, CorruptReason);

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
    line: usize,
}

impl Scanner<'_> {
    fn next_byte(&mut self) -> Option<u8> {
        let c = *self.b.get(self.i)?;
        self.i += 1;
        if c == b'\n' {
            self.line += 1;
        }
        Some(c)
    }

    /// Skips the rest of the current line.
    fn skip_line(&mut self) {
        while let Some(d) = self.next_byte() {
            if d == b'\n' {
                break;
            }
        }
    }

    /// Skips to the next `A`-`E`, dropping `#` comment lines; returns its
    /// offset.
    fn find_code(&mut self) -> Option<usize> {
        while let Some(c) = self.next_byte() {
            match c {
                b'A'..=b'E' => return Some(self.i - 1),
                b'#' => {
                    while let Some(d) = self.next_byte() {
                        if d == b'\n' {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn fail<T>(&self, reason: CorruptReason) -> Result<T, Failure> {
        Err((self.line, reason))
    }

    fn read_entry(&mut self, start: usize) -> Result<RawEntry, Failure> {
        // Code: letter, two digits, optional `a`-`z`, optional `1`-`4`.
        for _ in 0..2 {
            match self.b.get(self.i) {
                Some(c) if c.is_ascii_digit() => self.i += 1,
                _ => return self.fail(CorruptReason::BadCode),
            }
        }
        if matches!(self.b.get(self.i), Some(b'a'..=b'z')) {
            self.i += 1;
            if matches!(self.b.get(self.i), Some(b'1'..=b'4')) {
                self.i += 1;
            }
        }
        let code = std::str::from_utf8(&self.b[start..self.i]).unwrap_or_default();
        let Ok(eco) = code.parse::<Eco>() else {
            return self.fail(CorruptReason::BadCode);
        };
        // Name: between the next two double quotes.
        loop {
            match self.next_byte() {
                Some(b'"') => break,
                Some(_) => {}
                None => return self.fail(CorruptReason::UnterminatedName),
            }
        }
        let name_start = self.i;
        loop {
            match self.next_byte() {
                Some(b'"') => break,
                Some(_) if self.i - name_start > MAX_ENTRY_BYTES => {
                    return self.fail(CorruptReason::TooLarge);
                }
                Some(_) => {}
                None => return self.fail(CorruptReason::UnterminatedName),
            }
        }
        // Slicing at ASCII quotes keeps the UTF-8 boundaries.
        let name = String::from_utf8_lossy(&self.b[name_start..self.i - 1]).into_owned();
        // Moves: up to `*`, whitespace collapsed.
        let mut moves = Vec::new();
        loop {
            match self.next_byte() {
                Some(b'*') => break,
                Some(c) if c.is_ascii_whitespace() => {
                    if moves.last().is_some_and(|&l| l != b' ') {
                        moves.push(b' ');
                    }
                }
                Some(c) => moves.push(c),
                None => return self.fail(CorruptReason::UnterminatedMoves),
            }
            if moves.len() > MAX_ENTRY_BYTES {
                return self.fail(CorruptReason::TooLarge);
            }
        }
        while moves.last() == Some(&b' ') {
            moves.pop();
        }
        let moves = String::from_utf8_lossy(&moves).into_owned();
        Ok(RawEntry { eco, name, moves })
    }
}

/// The pawn move of the b-file named by `word` (`b3`, `bxc3`, `bc3`,
/// `b8=Q`, `b8Q`), if exactly one legal move matches.
fn b_pawn_move(pos: &Position, word: &str) -> Option<Move> {
    let rest = word.get(1..)?;
    let promo = match rest.bytes().last()? {
        b'q' | b'Q' => Some(Role::Queen),
        b'r' | b'R' => Some(Role::Rook),
        b'b' | b'B' => Some(Role::Bishop),
        b'n' | b'N' => Some(Role::Knight),
        _ => None,
    };
    let mut found = None;
    for mv in pos.legal_moves() {
        if let Move::Normal {
            role: Role::Pawn,
            from,
            to,
            promotion,
            ..
        } = mv
            && from.file() == File::B
            && promotion == promo
            && word.contains(&to.to_string())
        {
            if found.is_some() {
                return None;
            }
            found = Some(mv);
        }
    }
    found
}

/// Plays a move text the way `Position::ReadLine` does: junk before a letter
/// is dropped, each word is reduced to letters, digits and `=`.
fn play_line(text: &str) -> Result<(Position, u32), CorruptReason> {
    let b = text.as_bytes();
    let mut pos = Position::new();
    let mut plies = 0u32;
    let mut i = 0;
    let mut word = String::new();
    loop {
        while i < b.len() && !b[i].is_ascii_alphabetic() {
            i += 1;
        }
        if i >= b.len() {
            return Ok((pos, plies));
        }
        word.clear();
        while i < b.len() && !b[i].is_ascii_whitespace() {
            if b[i].is_ascii_alphanumeric() || b[i] == b'=' {
                word.push(char::from(b[i]));
            }
            i += 1;
        }
        if plies >= MAX_ENTRY_PLIES {
            return Err(CorruptReason::TooManyPlies);
        }
        let illegal = || CorruptReason::IllegalMove {
            text: word.chars().take(32).collect(),
        };
        let san = match word.as_bytes()[0] {
            b'O' => match word.as_str() {
                "OO" => "O-O".to_owned(),
                "OOO" => "O-O-O".to_owned(),
                _ => return Err(illegal()),
            },
            b'K' | b'Q' | b'R' | b'B' | b'N' | b'k' | b'q' | b'r' | b'n' => {
                word[..1].to_ascii_uppercase() + &word[1..]
            }
            // Scid: lowercase `b` starts a pawn move, never a bishop, which
            // the tolerant SAN parser would otherwise accept (`bb2`, `bc3`).
            b'b' => {
                let mv = b_pawn_move(&pos, &word).ok_or_else(illegal)?;
                pos.play(mv).map_err(|_| illegal())?;
                plies += 1;
                continue;
            }
            b'a'..=b'h' => word.clone(),
            _ => return Err(illegal()),
        };
        let mv = pos
            .parse_san(&san, SanMode::Tolerant)
            .map_err(|_| illegal())?
            .mv;
        pos.play(mv).map_err(|_| illegal())?;
        plies += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use shakmaty::{FromSetup, Position as _};

    const SAMPLE: &str = "# comment\n\
        A00a \"Start\"  *\n\
        A00b \"Barnes Opening\"  1.f3 *\n\
        B90a \"Sicilian: Najdorf\" 1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6\n 5.Nc3 a6 *\n\
        E20 \"Nimzo-Indian Defence\" 1.d4 Nf6 2.c4 e6 3.Nc3 Bb4 *\n\
        B20 \"Sicilian\" 1.e4 c5 *\n";

    fn line(sans: &str) -> (Position, Vec<Move>) {
        let start = Position::new();
        let mut pos = start.clone();
        let mut mvs = Vec::new();
        for san in sans.split_whitespace() {
            let mv = pos.parse_san(san, SanMode::Strict).expect("san").mv;
            pos.play(mv).expect("play");
            mvs.push(mv);
        }
        (start, mvs)
    }

    fn classify(book: &EcoBook, sans: &str, o: ClassifyOptions) -> Option<(u32, String)> {
        let (start, mvs) = line(sans);
        book.classify_line(&start, &mvs, o)
            .map(|m| (m.ply, m.entry().eco().to_string()))
    }

    #[test]
    fn book_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<EcoBook>();
    }

    #[test]
    fn parses_sample() {
        let r = EcoBook::parse_with(SAMPLE, EcoParseOptions::default()).expect("parse");
        assert_eq!(r.book.len(), 5);
        assert_eq!(r.entries_read, 5);
        assert_eq!(r.diagnostics.len(), 0);
        let e = &r.book.entries()[2];
        assert_eq!(e.eco().to_string(), "B90a");
        assert_eq!(e.name(), "Sicilian: Najdorf");
        assert_eq!(e.moves(), "1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6");
        assert_eq!(e.plies(), 10);
        assert_eq!(e.line(), 5);
        assert_eq!(r.book.entries()[0].plies(), 0);
        assert_eq!(r.book.fewest_pieces(), 30);
    }

    #[test]
    fn extended_codes_and_comments() {
        let b =
            EcoBook::parse("A00a1 \"x\" 1.e4 *\n# note A00 \"y\" *\nA00z4\"z\" 1.d4 *").expect("p");
        let codes: Vec<String> = b.entries().iter().map(|e| e.eco().to_string()).collect();
        assert_eq!(codes, ["A00a1", "A00z4"]);
    }

    #[test]
    fn castling_promotion_and_odd_words() {
        let b = EcoBook::parse(
            "C00 \"c\" 1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5 4.O-O Nf6 ( 99 ) 12 *\n\
             D00 \"p\" 1.h4 g5 2.hxg5 h6 3.gxh6 Nf6 4.h7 Ng8 5.hxg8=Q *",
        )
        .expect("p");
        assert_eq!(b.len(), 2);
        assert_eq!(b.entries()[0].plies(), 8);
        assert_eq!(b.entries()[1].plies(), 9);
        let q = EcoBook::parse("C00 \"l\" 1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5 4.OO Nf6 5.d3 O-O *");
        assert!(q.is_ok());
    }

    #[test]
    fn corrupt_cases() {
        let cases = [
            ("A00 \"x\" 1.e5 *", 1, "illegal"),
            ("A00 \"x\" 1.e4", 1, "moves"),
            ("A00 \"x", 1, "name"),
            ("\n\nA0b \"x\" *", 3, "code"),
            ("A00 \"x\" 1.e4 *\nA00 \"y\" 1.Ke2 *", 2, "illegal"),
        ];
        for (text, line, what) in cases {
            match EcoBook::parse(text) {
                Err(EcoBookError::Corrupt { line: l, reason }) => {
                    assert_eq!(l, line, "{text}");
                    let ok = matches!(
                        (what, &reason),
                        ("illegal", CorruptReason::IllegalMove { .. })
                            | ("moves", CorruptReason::UnterminatedMoves)
                            | ("name", CorruptReason::UnterminatedName)
                            | ("code", CorruptReason::BadCode)
                    );
                    assert!(ok, "{text}: {reason:?}");
                }
                other => panic!("{text}: {other:?}"),
            }
        }
    }

    #[test]
    fn skip_entry_continues() {
        let opts = EcoParseOptions {
            on_error: OnError::SkipEntry,
        };
        let r = EcoBook::parse_with("A00 \"x\" 1.e5 *\nB00 \"ok\" 1.e4 *\n", opts).expect("p");
        assert_eq!(r.book.len(), 1);
        assert_eq!(r.diagnostics.len(), 1);
        assert!(matches!(
            r.diagnostics[0],
            EcoDiagnostic::EntrySkipped { line: 1, .. }
        ));
    }

    #[test]
    fn last_duplicate_wins_keeping_slot() {
        let text = "A00 \"first\" 1.e4 *\nB00 \"mid\" 1.d4 *\nA01 \"second\" 1.e4 *\n";
        let r = EcoBook::parse_with(text, EcoParseOptions::default()).expect("p");
        assert_eq!(r.entries_read, 3);
        assert_eq!(r.book.len(), 2);
        assert_eq!(r.book.entries()[0].name(), "second");
        assert_eq!(r.book.entries()[1].name(), "mid");
        assert_eq!(
            r.diagnostics,
            [EcoDiagnostic::DuplicatePosition {
                line: 3,
                first_line: 1
            }]
        );
    }

    #[test]
    fn quotes_and_accents_in_names() {
        let b = EcoBook::parse("A00 \"Tübingen – défense d’Amsterdam\" 1.g4 *").expect("p");
        assert_eq!(b.entries()[0].name(), "Tübingen – défense d’Amsterdam");
    }

    #[test]
    fn classification() {
        let b = EcoBook::parse(SAMPLE).expect("p");
        let g = ClassifyOptions::game();
        assert_eq!(classify(&b, "", g), Some((0, "A00a".into())));
        assert_eq!(classify(&b, "e4 c5 Nf3", g), Some((2, "B20".into())));
        assert_eq!(
            classify(&b, "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be2", g),
            Some((10, "B90a".into()))
        );
        // Transposition.
        assert_eq!(
            classify(&b, "c4 e6 Nc3 Nf6 d4 Bb4", g),
            Some((6, "E20".into()))
        );
        // Return to the start position.
        assert_eq!(classify(&b, "Nf3 Nf6 Ng1 Ng8", g), Some((4, "A00a".into())));
        // Depth limit.
        let o = ClassifyOptions { max_ply: Some(5) };
        assert_eq!(
            classify(&b, "c4 e6 Nc3 Nf6 d4 Bb4", o),
            Some((0, "A00a".into()))
        );
        assert_eq!(ClassifyOptions::batch().max_ply, Some(60));
    }

    #[test]
    fn classifies_game_ignoring_variations_and_fen_start() {
        let b = EcoBook::parse(SAMPLE).expect("p");
        let mut game = Game::new();
        let e4 = game.add_move(game.root(), line("e4").1[0]).expect("e4");
        let c5 = game
            .add_move(
                e4,
                Position::new()
                    .after(line("e4").1[0])
                    .expect("a")
                    .parse_san("c5", SanMode::Strict)
                    .expect("c5")
                    .mv,
            )
            .expect("c5");
        let _ = c5;
        let m = b.classify(&game, ClassifyOptions::game()).expect("match");
        assert_eq!((m.ply, m.eco(false).to_string()), (2, "B20".into()));
        // A game starting from a book position is classified at ply 0.
        let start = Position::new().after(line("b3").1[0]).expect("b3");
        assert!(
            b.classify(&Game::from_position(start), ClassifyOptions::game())
                .is_none()
        );
        let g0 = Game::new();
        assert_eq!(
            b.classify(&g0, ClassifyOptions::game()).map(|m| m.ply),
            Some(0)
        );
    }

    fn skip_opts() -> EcoParseOptions {
        EcoParseOptions::default().with_on_error(OnError::SkipEntry)
    }

    #[test]
    fn skip_entry_hostile_input_is_bounded() {
        let r = EcoBook::parse_with(&"A".repeat(1 << 20), skip_opts()).expect("p");
        assert_eq!(r.book.len(), 0);
        assert!(r.diagnostics.len() <= 1, "{}", r.diagnostics.len());

        let lines = "Bird\n".repeat(MAX_DIAGNOSTICS + 500);
        let r = EcoBook::parse_with(&lines, skip_opts()).expect("p");
        assert_eq!(r.diagnostics.len(), MAX_DIAGNOSTICS);
        assert_eq!(r.omitted_diagnostics, 500);

        let r =
            EcoBook::parse_with("A00 \"Bird\" 1.e4 *\nB00 \"x\" 1.d4 *", skip_opts()).expect("p");
        assert_eq!((r.book.len(), r.diagnostics.len()), (2, 0));
        let r = EcoBook::parse_with("A0 \"Bird Opening\" 1.e4 *\nB00 \"x\" 1.d4 *", skip_opts())
            .expect("p");
        assert_eq!(r.book.len(), 1);
        assert_eq!(r.diagnostics.len(), 1);
    }

    #[test]
    fn too_large_and_too_many_plies() {
        let big = format!("A00 \"{}\" 1.e4 *", "x".repeat(MAX_ENTRY_BYTES + 10));
        assert!(matches!(
            EcoBook::parse(&big),
            Err(EcoBookError::Corrupt {
                reason: CorruptReason::TooLarge,
                ..
            })
        ));
        let big = format!("A00 \"x\" {} *", "e4 ".repeat(MAX_ENTRY_BYTES / 3 + 10));
        assert!(matches!(
            EcoBook::parse(&big),
            Err(EcoBookError::Corrupt {
                reason: CorruptReason::TooLarge,
                ..
            })
        ));
        let shuffle = "Nf3 Nf6 Ng1 Ng8 ".repeat(260);
        let text = format!("A00 \"x\" {shuffle} *");
        assert!(matches!(
            EcoBook::parse(&text),
            Err(EcoBookError::Corrupt {
                reason: CorruptReason::TooManyPlies,
                ..
            })
        ));
    }

    #[test]
    fn chess960_is_not_classified() {
        let b = EcoBook::parse(SAMPLE).expect("p");
        let setup = shakmaty::Chess::default().to_setup(shakmaty::EnPassantMode::Legal);
        let inner =
            shakmaty::Chess::from_setup(setup, CastlingMode::Chess960).expect("chess960 start");
        let start = Position { inner };
        assert_eq!(start.castling_mode(), CastlingMode::Chess960);
        assert!(
            b.classify(&Game::from_position(start.clone()), ClassifyOptions::game())
                .is_none()
        );
        assert!(
            b.classify_line(&start, &[], ClassifyOptions::game())
                .is_none()
        );
    }

    #[test]
    fn game_from_fen_is_classified_at_ply_zero() {
        let b = EcoBook::parse(SAMPLE).expect("p");
        let after_f3 = Position::new().after(line("f3").1[0]).expect("f3");
        let m = b
            .classify(&Game::from_position(after_f3), ClassifyOptions::game())
            .expect("match");
        assert_eq!((m.ply, m.entry().eco().to_string()), (0, "A00b".into()));
    }

    #[test]
    fn variations_do_not_affect_classification() {
        let b = EcoBook::parse(SAMPLE).expect("p");
        let mut game = Game::new();
        let f3 = line("f3").1[0];
        let e4 = line("e4").1[0];
        game.add_move(game.root(), e4).expect("e4");
        // Side line 1.f3 (a book position) next to the main move.
        game.add_move(game.root(), f3).expect("f3");
        let m = b.classify(&game, ClassifyOptions::game()).expect("match");
        assert_eq!((m.ply, m.entry().eco().to_string()), (0, "A00a".into()));
    }

    #[test]
    fn batch_limit_matches_game_up_to_sixty_plies() {
        let b = EcoBook::parse(SAMPLE).expect("p");
        // 64 plies of shuffling that return to the start every four plies.
        let sans = "Nf3 Nf6 Ng1 Ng8 ".repeat(16);
        let (start, mvs) = line(&sans);
        assert_eq!(mvs.len(), 64);
        let mut game = Game::new();
        let mut node = game.root();
        for &mv in &mvs {
            node = game.add_move(node, mv).expect("add");
        }
        let batch = b.classify(&game, ClassifyOptions::batch()).expect("m");
        let whole = b.classify(&game, ClassifyOptions::game()).expect("m");
        assert_eq!((batch.ply, whole.ply), (60, 64));
        let by_line = b
            .classify_line(&start, &mvs, ClassifyOptions::default().with_max_ply(60))
            .expect("m");
        assert_eq!(by_line.ply, 60);
    }

    #[test]
    fn fewest_pieces_cutoff_keeps_results() {
        // Book positions have 30 pieces; a game that captures down far below
        // that still matches the earlier entry.
        let b = EcoBook::parse(SAMPLE).expect("p");
        let sans = "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Nb3 Nxe4 Nxe4 d5";
        let (start, mvs) = line(sans);
        let m = b
            .classify_line(&start, &mvs, ClassifyOptions::game())
            .expect("m");
        assert_eq!(m.ply, 10);
    }

    #[test]
    fn summary_and_find() {
        let text = "A00a \"S\" *\nA00b \"B\" 1.f3 *\nB90a \"N\" 1.e4 c5 *\nB90b \"M\" 1.e4 c5 2.Nf3 *\nB91 \"Z\" 1.e4 e5 *\n";
        let b = EcoBook::parse(text).expect("p");
        let codes = |v: Vec<&EcoEntry>| v.iter().map(|e| e.eco().to_string()).collect::<Vec<_>>();
        assert_eq!(codes(b.summary("")), ["A00a", "B90a"]);
        assert_eq!(codes(b.summary("B")), ["B90a"]);
        assert_eq!(codes(b.summary("B9")), ["B90a", "B91"]);
        assert_eq!(codes(b.summary("B90")), ["B90a", "B90b"]);
        assert_eq!(codes(b.find("[M]")), ["B90b"]);
        assert_eq!(codes(b.find("b90")), Vec::<String>::new());
        assert_eq!(codes(b.find("B9")), ["B90a", "B90b", "B91"]);
    }

    #[test]
    fn translation_applies_pairs_in_reverse() {
        let g = [
            ("Defence", "Verteidigung"),
            ("Sicilian Defence", "Sizilianisch"),
        ];
        assert_eq!(translate_eco_name("Sicilian Defence", &g), "Sizilianisch");
        assert_eq!(
            translate_eco_name("Dutch Defence Defence", &[("Defence", "D")]),
            "Dutch D D"
        );
        assert_eq!(translate_eco_name("x", &[("", "y")]), "x");
    }

    #[test]
    fn hostile_input_does_not_panic() {
        for t in [
            "",
            "A",
            "A0",
            "A00",
            "A00 \"",
            "\u{feff}A00 \"é\" *",
            "E99z \"\"\n*",
            "A00 \"x\" \u{0}\u{ff} *",
        ] {
            drop(EcoBook::parse(t));
        }
    }

    /// Needs the Scid distribution in `reference/`.
    #[test]
    #[ignore = "requires reference/scid_vs_pc-4.27/scid.eco"]
    fn compat_eco_loads_scid_eco() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../reference/scid_vs_pc-4.27/scid.eco"
        );
        let text = std::fs::read_to_string(path).expect("read scid.eco");
        let r = EcoBook::parse_with(&text, EcoParseOptions::default()).expect("parse");
        assert_eq!(r.entries_read, 10_360);
        assert_eq!(r.book.len(), 10_360);
        assert_eq!(r.diagnostics.len(), 0);
        assert_eq!(r.book.fewest_pieces(), 21);
        assert_eq!(r.book.entries()[0].eco().to_string(), "A00a");
        assert_eq!(r.book.entries()[0].name(), "Start");
        assert!(
            r.book
                .entries()
                .iter()
                .any(|e| e.name().contains("Tübingen"))
        );
        let basic = r
            .book
            .entries()
            .iter()
            .filter(|e| !e.eco().is_extended())
            .count();
        assert_eq!((basic, r.book.len() - basic), (2_537, 7_823));

        // Every entry classifies to itself.
        for e in r.book.entries() {
            let m = r.book.lookup(e.position()).expect("lookup");
            assert_eq!(m.line(), e.line());
        }
        let g = ClassifyOptions::game();
        let cases: [(&str, u32, &str, &str); 9] = [
            (
                "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7 Re1 b5 Bb3 d6 c3 O-O h3",
                17,
                "C92a",
                "Spanish: Closed, 9.h3",
            ),
            (
                "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6",
                10,
                "B90a",
                "Sicilian: Najdorf",
            ),
            ("d4 Nf6 c4 e6 Nc3 Bb4", 6, "E20", "Nimzo-Indian Defence"),
            ("c4 e6 Nc3 Nf6 d4 Bb4", 6, "E20", "Nimzo-Indian Defence"),
            ("b4", 1, "A00p", "Polish (Sokolsky; Orang-Utan)"),
            ("a3 a6", 1, "A00f", "Anderssen Opening"),
            (
                "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7 Re1 b5 Bb3 O-O c3 d5 exd5 Nxd5 Nxe5 Nxe5 Rxe5 c6",
                22,
                "C89f",
                "Spanish: Marshall, 11.Rxe5 c6",
            ),
            ("Nf3 Nf6 Ng1 Ng8", 4, "A00a", "Start"),
            ("", 0, "A00a", "Start"),
        ];
        for (sans, ply, code, name) in cases {
            let (start, mvs) = line(sans);
            let m = r.book.classify_line(&start, &mvs, g).expect(sans);
            assert_eq!(
                (m.ply, m.entry().eco().to_string(), m.entry().name()),
                (ply, code.to_owned(), name),
                "{sans}"
            );
        }
    }

    /// Needs the Scid distribution in `reference/`.
    #[test]
    #[ignore = "requires reference/scid_vs_pc-4.27/scid.eco"]
    fn compat_eco_every_entry_classifies_to_itself() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../reference/scid_vs_pc-4.27/scid.eco"
        );
        let text = std::fs::read_to_string(path).expect("read scid.eco");
        let book = EcoBook::parse(&text).expect("parse");
        for e in book.entries() {
            let mut game = Game::new();
            let mut node = game.root();
            let mut p = Position::new();
            for w in e.moves().split_whitespace() {
                let w = w.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
                if w.is_empty() {
                    continue;
                }
                let mv = p.parse_san(w, SanMode::Tolerant).expect("san").mv;
                p.play(mv).expect("play");
                node = game.add_move(node, mv).expect("add");
            }
            let m = book
                .classify(&game, ClassifyOptions::game())
                .expect("match");
            assert_eq!(m.ply, e.plies(), "{}", e.moves());
            assert_eq!(m.entry().eco(), e.eco());
        }
    }
}
