// Derived from Scid vs PC 4.27, tcl/windows/tree.tcl (`::tree::mask`: data
// layout 1774-1818, accessors 2078-2341, `toShortFen` 2544). Copyright (c)
// 1999-2002 Shane Hudson; Scid vs PC contributors. Scid is distributed under
// the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The data model of a tree mask (`docs/specs/m5-4-mascaras-de-arvore.md`,
//! decisions D1803 to D1811): positions keyed by short FEN, each with its
//! moves (NAG, color, comment, two markers) and a comment.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::BuildHasher;

use mychess_core::{CastlingMode, Nag, Position};
use thiserror::Error;

use super::matching::{MoveMatcher, bare_move_text};

/// The most moves a position of a mask holds (D1815).
pub const MAX_MOVES_PER_POSITION: usize = 500;
/// The longest comment, in bytes (D1815).
pub const MAX_COMMENT_BYTES: usize = 64 * 1024;
/// The longest SAN, key, NAG, color or marker text, in bytes.
pub const MAX_FIELD_BYTES: usize = 1024;
/// The most positions a mask holds (D1850). A bigger file is read up to this
/// many positions (the rest is dropped with a warning), and nothing makes a
/// mask grow past it. At roughly 200 bytes per small position this is about
/// 200 MiB, the worst case of a hostile file.
pub const MAX_POSITIONS: usize = 1_000_000;
/// The longest preview of a comment kept by the outline and the search, in
/// bytes (D1851): the first line, cut at a character boundary.
pub const MAX_PREVIEW_BYTES: usize = 200;

/// Why a text is not accepted as a NAG, color, marker or move of a mask
/// (D1852).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum MaskValueError {
    /// Nothing but white space.
    #[error("empty text")]
    Empty,
    /// Longer than [`MAX_FIELD_BYTES`].
    #[error("text longer than {} bytes", MAX_FIELD_BYTES)]
    TooLong,
    /// The text names one of the standard values, which the file would give
    /// back as that value and not as `Other`.
    #[error("text is one of the standard values")]
    Reserved,
}

/// Why a position was not stored by [`TreeMask::try_insert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum MaskInsertError {
    /// The mask already has [`MAX_POSITIONS`] positions.
    #[error("the mask is full ({} positions)", MAX_POSITIONS)]
    Full,
    /// The key is longer than [`MAX_FIELD_BYTES`].
    #[error("key longer than {} bytes", MAX_FIELD_BYTES)]
    BadKey,
}

/// The trimmed text of a free-form value, or why it is not acceptable.
fn checked_text(text: &str) -> Result<&str, MaskValueError> {
    let t = text.trim();
    if t.is_empty() {
        Err(MaskValueError::Empty)
    } else if t.len() > MAX_FIELD_BYTES {
        Err(MaskValueError::TooLong)
    } else {
        Ok(t)
    }
}

/// The first line of `text`, at most `max` bytes, cut at a character boundary
/// (the preview of D1851).
pub(crate) fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("");
    if line.len() <= max {
        return line.to_owned();
    }
    let mut end = max;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    line.get(..end).unwrap_or_default().to_owned()
}

/// The key of a position: the short FEN (four fields), D1804.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MaskKey {
    /// A valid short FEN of standard chess, in canonical form.
    Fen(String),
    /// A key of the file that is not a short FEN: kept as it came, never
    /// matched.
    Opaque(String),
}

/// The first four fields of a FEN.
pub(crate) fn short_fen(fen: &str) -> &str {
    let mut it = fen.rsplitn(3, ' ');
    it.next();
    it.next();
    it.next().unwrap_or(fen)
}

impl MaskKey {
    /// The key of `position` (`toShortFen`).
    pub fn from_position(position: &Position) -> MaskKey {
        MaskKey::Fen(short_fen(&position.fen()).to_owned())
    }

    /// Reads a key of a file. A short FEN of a legal standard position becomes
    /// [`MaskKey::Fen`] in canonical form; anything else is kept opaque.
    pub fn from_text(text: &str) -> MaskKey {
        let fields: Vec<&str> = text.split_ascii_whitespace().collect();
        if fields.len() == 4 && text.len() <= MAX_FIELD_BYTES {
            let full = format!("{} 0 1", fields.join(" "));
            if let Ok(pos) = Position::from_fen(&full)
                && pos.castling_mode() == CastlingMode::Standard
            {
                return MaskKey::Fen(short_fen(&pos.fen()).to_owned());
            }
        }
        MaskKey::Opaque(text.to_owned())
    }

    /// The key as the file gives it back (D1852): a short FEN of a legal
    /// standard position in canonical form is [`MaskKey::Fen`], anything else
    /// is [`MaskKey::Opaque`]. `None` when the text is longer than
    /// [`MAX_FIELD_BYTES`].
    pub fn normalized(&self) -> Option<MaskKey> {
        if self.as_str().len() > MAX_FIELD_BYTES {
            return None;
        }
        Some(MaskKey::from_text(self.as_str()))
    }

    /// The text of the key, as written in the file.
    pub fn as_str(&self) -> &str {
        match self {
            MaskKey::Fen(s) | MaskKey::Opaque(s) => s,
        }
    }

    /// The position of a [`MaskKey::Fen`] key (move counters 0 and 1).
    pub fn to_position(&self) -> Option<Position> {
        match self {
            MaskKey::Fen(s) => Position::from_fen(&format!("{s} 0 1")).ok(),
            MaskKey::Opaque(_) => None,
        }
    }

    /// The six-field FEN of a [`MaskKey::Fen`] key (counters `0 1`).
    pub fn full_fen(&self) -> Option<String> {
        match self {
            MaskKey::Fen(s) => Some(format!("{s} 0 1")),
            MaskKey::Opaque(_) => None,
        }
    }
}

impl fmt::Display for MaskKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The NAG of a move of the mask (D1807): one of seven glyphs, or an unknown
/// text that is preserved.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MaskNag {
    /// `!!`.
    Brilliant,
    /// `!`.
    Good,
    /// `!?`.
    Interesting,
    /// `?!`.
    Dubious,
    /// `?`.
    Poor,
    /// `??`.
    Blunder,
    /// `~`.
    Unclear,
    /// A text of the file that is none of the above.
    Other(String),
}

impl MaskNag {
    /// The seven standard NAGs, in the order of the Scid menu.
    pub const STANDARD: [MaskNag; 7] = [
        MaskNag::Brilliant,
        MaskNag::Good,
        MaskNag::Interesting,
        MaskNag::Dubious,
        MaskNag::Poor,
        MaskNag::Blunder,
        MaskNag::Unclear,
    ];

    /// Reads the text of the file (with or without the leading space of the
    /// Scid). Empty gives `None`.
    pub fn from_text(text: &str) -> Option<MaskNag> {
        let t = text.trim();
        Some(match t {
            "" => return None,
            "!!" => MaskNag::Brilliant,
            "!" => MaskNag::Good,
            "!?" => MaskNag::Interesting,
            "?!" => MaskNag::Dubious,
            "?" => MaskNag::Poor,
            "??" => MaskNag::Blunder,
            "~" => MaskNag::Unclear,
            other => MaskNag::Other(other.to_owned()),
        })
    }

    /// An unknown NAG text, validated (D1852): trimmed, not empty, at most
    /// [`MAX_FIELD_BYTES`], and not one of the seven standard glyphs (use the
    /// variant for those, or [`MaskNag::from_text`]).
    ///
    /// # Errors
    ///
    /// [`MaskValueError`].
    pub fn other(text: &str) -> Result<MaskNag, MaskValueError> {
        match MaskNag::from_text(checked_text(text)?) {
            Some(n @ MaskNag::Other(_)) => Ok(n),
            _ => Err(MaskValueError::Reserved),
        }
    }

    /// The value as the file gives it back: an `Other` that spells a
    /// standard glyph becomes that glyph, an empty one becomes `None`.
    ///
    /// # Errors
    ///
    /// [`MaskValueError::TooLong`].
    pub fn normalized(&self) -> Result<Option<MaskNag>, MaskValueError> {
        match self {
            MaskNag::Other(s) => {
                if s.trim().len() > MAX_FIELD_BYTES {
                    Err(MaskValueError::TooLong)
                } else {
                    Ok(MaskNag::from_text(s))
                }
            }
            standard => Ok(Some(standard.clone())),
        }
    }

    /// The text written to the file (no leading space).
    pub fn as_str(&self) -> &str {
        match self {
            MaskNag::Brilliant => "!!",
            MaskNag::Good => "!",
            MaskNag::Interesting => "!?",
            MaskNag::Dubious => "?!",
            MaskNag::Poor => "?",
            MaskNag::Blunder => "??",
            MaskNag::Unclear => "~",
            MaskNag::Other(s) => s,
        }
    }

    /// The standard NAG that matches a numeric NAG of a game (`$1` to `$6`
    /// and `$13`).
    pub fn from_nag(nag: Nag) -> Option<MaskNag> {
        Some(match nag.value() {
            1 => MaskNag::Good,
            2 => MaskNag::Poor,
            3 => MaskNag::Brilliant,
            4 => MaskNag::Blunder,
            5 => MaskNag::Interesting,
            6 => MaskNag::Dubious,
            13 => MaskNag::Unclear,
            _ => return None,
        })
    }
}

/// The color of a move of the mask (D1808).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum MaskColor {
    /// No color (the default; written `white`).
    #[default]
    White,
    /// `Green`.
    Green,
    /// `Yellow`.
    Yellow,
    /// `Blue`.
    Blue,
    /// `Red`.
    Red,
    /// Any other Tk color (`#rrggbb`, a name): preserved.
    Other(String),
}

impl MaskColor {
    /// The five colors of the Scid menu.
    pub const STANDARD: [MaskColor; 5] = [
        MaskColor::White,
        MaskColor::Green,
        MaskColor::Yellow,
        MaskColor::Blue,
        MaskColor::Red,
    ];

    /// Reads the text of the file (case-insensitive for the known names; empty
    /// is the default).
    pub fn from_text(text: &str) -> MaskColor {
        let t = text.trim();
        for c in &MaskColor::STANDARD {
            if t.eq_ignore_ascii_case(c.as_str()) {
                return c.clone();
            }
        }
        if t.is_empty() {
            MaskColor::White
        } else {
            MaskColor::Other(t.to_owned())
        }
    }

    /// An unknown color text, validated (D1852): trimmed, not empty, at most
    /// [`MAX_FIELD_BYTES`], and not one of the five standard names in any
    /// case.
    ///
    /// # Errors
    ///
    /// [`MaskValueError`].
    pub fn other(text: &str) -> Result<MaskColor, MaskValueError> {
        match MaskColor::from_text(checked_text(text)?) {
            c @ MaskColor::Other(_) => Ok(c),
            _ => Err(MaskValueError::Reserved),
        }
    }

    /// The value as the file gives it back (`Other("green")` is
    /// [`MaskColor::Green`], an empty `Other` is [`MaskColor::White`]).
    ///
    /// # Errors
    ///
    /// [`MaskValueError::TooLong`].
    pub fn normalized(&self) -> Result<MaskColor, MaskValueError> {
        match self {
            MaskColor::Other(s) => {
                if s.trim().len() > MAX_FIELD_BYTES {
                    Err(MaskValueError::TooLong)
                } else {
                    Ok(MaskColor::from_text(s))
                }
            }
            standard => Ok(standard.clone()),
        }
    }

    /// The text written to the file.
    pub fn as_str(&self) -> &str {
        match self {
            MaskColor::White => "white",
            MaskColor::Green => "Green",
            MaskColor::Yellow => "Yellow",
            MaskColor::Blue => "Blue",
            MaskColor::Red => "Red",
            MaskColor::Other(s) => s,
        }
    }

    /// Whether this is a hexadecimal Tk color (`#rgb`, `#rrggbb`...), which
    /// is valid although not one of the five.
    pub fn is_hex(&self) -> bool {
        matches!(self, MaskColor::Other(s)
            if s.strip_prefix('#').is_some_and(|h|
                matches!(h.len(), 3 | 6 | 9 | 12) && h.bytes().all(|b| b.is_ascii_hexdigit())))
    }
}

/// A marker of a move (D1809). The file stores the name of a Tk image.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Marker {
    /// Include.
    Include,
    /// Exclude.
    Exclude,
    /// Main line.
    MainLine,
    /// Bookmark.
    Bookmark,
    /// White.
    White,
    /// Black.
    Black,
    /// New line.
    NewLine,
    /// To be verified.
    ToBeVerified,
    /// To train.
    ToTrain,
    /// Dubious.
    Dubious,
    /// To remove.
    ToRemove,
    /// An image name the Scid does not know: preserved.
    Other(String),
}

impl Marker {
    /// The eleven markers, in the order of the Scid menu.
    pub const ALL: [Marker; 11] = [
        Marker::Include,
        Marker::Exclude,
        Marker::MainLine,
        Marker::Bookmark,
        Marker::White,
        Marker::Black,
        Marker::NewLine,
        Marker::ToBeVerified,
        Marker::ToTrain,
        Marker::Dubious,
        Marker::ToRemove,
    ];

    /// The name written to the file (`stm.md` 2.7).
    pub fn file_name(&self) -> &str {
        match self {
            Marker::Include => "::rep::_tb_include",
            Marker::Exclude => "::rep::_tb_exclude",
            Marker::MainLine => "::tree::mask::imageMainLine",
            Marker::Bookmark => "tb_bkm",
            Marker::White => "::tree::mask::imageWhite",
            Marker::Black => "::tree::mask::imageBlack",
            Marker::NewLine => "tb_new",
            Marker::ToBeVerified => "tb_rfilter",
            Marker::ToTrain => "tb_msearch",
            Marker::Dubious => "tb_help_small",
            Marker::ToRemove => "tb_cut",
            Marker::Other(s) => s,
        }
    }

    /// An unknown marker image name, validated (D1852): trimmed, not empty,
    /// at most [`MAX_FIELD_BYTES`], and not a name that has a variant (nor
    /// the old `tb_help`).
    ///
    /// # Errors
    ///
    /// [`MaskValueError`].
    pub fn other(text: &str) -> Result<Marker, MaskValueError> {
        match Marker::from_text(checked_text(text)?) {
            Some(m @ Marker::Other(_)) => Ok(m),
            _ => Err(MaskValueError::Reserved),
        }
    }

    /// The value as the file gives it back (an `Other` with a known name is
    /// that marker, an empty one is `None`).
    ///
    /// # Errors
    ///
    /// [`MaskValueError::TooLong`].
    pub fn normalized(&self) -> Result<Option<Marker>, MaskValueError> {
        match self {
            Marker::Other(s) => {
                if s.trim().len() > MAX_FIELD_BYTES {
                    Err(MaskValueError::TooLong)
                } else {
                    Ok(Marker::from_text(s))
                }
            }
            standard => Ok(Some(standard.clone())),
        }
    }

    /// Reads the name of the file; empty gives `None`. The old `tb_help` is
    /// read as [`Marker::Dubious`].
    pub fn from_text(text: &str) -> Option<Marker> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        if t == "tb_help" {
            return Some(Marker::Dubious);
        }
        for m in &Marker::ALL {
            if m.file_name() == t {
                return Some(m.clone());
            }
        }
        Some(Marker::Other(t.to_owned()))
    }
}

/// The name of the null move the Scid stores to hold a position comment
/// (D1810).
pub const NULL_MOVE: &str = "null";

/// A move of a position of the mask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskMove {
    /// The move in English SAN, as stored (D1805, D1806).
    pub san: String,
    /// The NAG, if any.
    pub nag: Option<MaskNag>,
    /// The color.
    pub color: MaskColor,
    /// The comment (trimmed, no `\r`, at most [`MAX_COMMENT_BYTES`]).
    pub comment: String,
    /// The two markers.
    pub markers: [Option<Marker>; 2],
}

impl MaskMove {
    /// A move with the defaults (no NAG, no color, no comment, no markers).
    /// The text is trimmed and cut at [`MAX_FIELD_BYTES`] (on a character
    /// boundary); an empty result is not a usable move (the writer and
    /// [`TreeMask::insert`] drop it). Use [`MaskMove::try_new`] to be told.
    pub fn new(san: impl Into<String>) -> MaskMove {
        let san: String = san.into();
        let t = san.trim();
        let mut end = t.len().min(MAX_FIELD_BYTES);
        while !t.is_char_boundary(end) {
            end -= 1;
        }
        let san = t.get(..end).unwrap_or_default().trim_end().to_owned();
        MaskMove {
            san,
            nag: None,
            color: MaskColor::White,
            comment: String::new(),
            markers: [None, None],
        }
    }

    /// Like [`MaskMove::new`], but refuses a text that is empty (after
    /// trimming) or longer than [`MAX_FIELD_BYTES`] (D1852).
    ///
    /// # Errors
    ///
    /// [`MaskValueError::Empty`] and [`MaskValueError::TooLong`].
    pub fn try_new(san: &str) -> Result<MaskMove, MaskValueError> {
        Ok(MaskMove::new(checked_text(san)?))
    }

    /// The move as the file gives it back (D1852): trimmed texts, values
    /// normalized, comment cleaned. `None` when the SAN is empty or too long.
    /// A NAG, color or marker that is too long is dropped.
    pub fn normalized(&self) -> Option<MaskMove> {
        let san = checked_text(&self.san).ok()?;
        Some(MaskMove {
            san: san.to_owned(),
            nag: self
                .nag
                .as_ref()
                .and_then(|n| n.normalized().ok().flatten()),
            color: self.color.normalized().unwrap_or_default(),
            comment: clean_comment(&self.comment).0,
            markers: self
                .markers
                .each_ref()
                .map(|m| m.as_ref().and_then(|m| m.normalized().ok().flatten())),
        })
    }

    /// Whether this is the hidden `null` move of the Scid (D1810).
    pub fn is_null(&self) -> bool {
        self.san == NULL_MOVE
    }
}

/// A position of the mask: moves in file order and a comment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MaskPosition {
    /// The moves, in insertion (file) order.
    pub moves: Vec<MaskMove>,
    /// The comment of the position (clean, see [`clean_comment`]).
    pub comment: String,
}

impl MaskPosition {
    /// The moves that the interface shows (all but `null`).
    pub fn visible_moves(&self) -> impl Iterator<Item = &MaskMove> {
        self.moves.iter().filter(|m| !m.is_null())
    }

    /// Whether the position holds nothing worth writing: no visible move and
    /// no comment (D1810).
    pub fn is_dead(&self) -> bool {
        self.comment.trim().is_empty() && self.visible_moves().next().is_none()
    }
}

/// Cleans a comment: `\r\n` and `\r` become `\n`, the ends are trimmed
/// (white space, as `string trim`), and the text is cut at
/// [`MAX_COMMENT_BYTES`] on a character boundary. The `bool` tells whether it
/// was cut.
pub fn clean_comment(text: &str) -> (String, bool) {
    let t = text.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}'));
    let mut out = if t.contains('\r') {
        t.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        t.to_owned()
    };
    let mut cut = false;
    if out.len() > MAX_COMMENT_BYTES {
        let mut end = MAX_COMMENT_BYTES;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
        let trimmed = out.trim_end().len();
        out.truncate(trimmed);
        cut = true;
    }
    (out, cut)
}

/// The end of a chain of slots with the same hash.
const NO_SLOT: usize = usize::MAX;

#[derive(Debug, Clone)]
struct Slot {
    key: MaskKey,
    position: MaskPosition,
    /// The next slot whose key has the same hash (D1850).
    next: usize,
}

/// A tree mask: positions in file order with O(1) access by key (D1803).
///
/// The key text is kept once, in the slot: the index maps the 64-bit hash of
/// a key to the first slot of a chain (D1850), which halves the memory of a
/// big mask against a `HashMap` keyed by a clone of the key.
#[derive(Debug, Clone, Default)]
pub struct TreeMask {
    slots: Vec<Option<Slot>>,
    index: HashMap<u64, usize>,
    live: usize,
    hasher: RandomState,
}

impl PartialEq for TreeMask {
    /// Equal when they hold the same positions in the same order. A position
    /// with nothing worth writing ([`MaskPosition::is_dead`]) does not count,
    /// because the file never keeps it (D1853).
    fn eq(&self, other: &TreeMask) -> bool {
        let live = |(_, p): &(&MaskKey, &MaskPosition)| !p.is_dead();
        self.iter().filter(live).eq(other.iter().filter(live))
    }
}

impl Eq for TreeMask {}

/// Drops duplicates of the moves of a position (D1805) without a quadratic
/// scan: moves are bucketed by their bare text.
#[derive(Debug, Default)]
pub(crate) struct DupIndex {
    seen: HashMap<String, Vec<usize>>,
}

impl DupIndex {
    /// Whether `moves` already has a move that `san` names; when not, the
    /// move that is about to be pushed at `moves.len()` is remembered.
    pub(crate) fn check_and_add(
        &mut self,
        moves: &[MaskMove],
        matcher: &MoveMatcher<'_>,
        san: &str,
    ) -> bool {
        let bucket = self.seen.entry(bare_move_text(san)).or_default();
        if bucket
            .iter()
            .any(|&i| moves.get(i).is_some_and(|m| matcher.same(&m.san, san)))
        {
            return true;
        }
        bucket.push(moves.len());
        false
    }
}

/// The position as the file gives it back (D1852): moves normalized, the
/// repeated and the surplus dropped, comment cleaned.
fn normalize_position(key: &MaskKey, position: MaskPosition) -> MaskPosition {
    let pos = key.to_position();
    let matcher = MoveMatcher::new(pos.as_ref());
    let mut dups = DupIndex::default();
    let mut moves: Vec<MaskMove> = Vec::new();
    for m in position.moves {
        let Some(m) = m.normalized() else { continue };
        if moves.len() >= MAX_MOVES_PER_POSITION {
            break;
        }
        if dups.check_and_add(&moves, &matcher, &m.san) {
            continue;
        }
        moves.push(m);
    }
    MaskPosition {
        moves,
        comment: clean_comment(&position.comment).0,
    }
}

impl TreeMask {
    /// An empty mask.
    pub fn new() -> TreeMask {
        TreeMask::default()
    }

    /// The number of positions.
    pub fn len(&self) -> usize {
        self.live
    }

    /// Whether the mask has no position.
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Whether the mask holds [`MAX_POSITIONS`] positions: a new key is
    /// refused.
    pub fn is_full(&self) -> bool {
        self.live >= MAX_POSITIONS
    }

    /// A rough count of the bytes the mask holds on the heap (slots, index,
    /// keys, moves and comments), to watch the size of a big file (D1850).
    pub fn approx_bytes(&self) -> usize {
        let mut n = self.slots.capacity() * size_of::<Option<Slot>>()
            + self.index.capacity() * (2 * size_of::<usize>() + 1);
        for (key, p) in self.iter() {
            n += key.as_str().len()
                + p.comment.capacity()
                + p.moves.capacity() * size_of::<MaskMove>();
            n += p
                .moves
                .iter()
                .map(|m| m.san.capacity() + m.comment.capacity())
                .sum::<usize>();
        }
        n
    }

    fn find(&self, key: &MaskKey) -> Option<usize> {
        let mut i = *self.index.get(&self.hasher.hash_one(key))?;
        loop {
            let slot = self.slots.get(i)?.as_ref()?;
            if slot.key == *key {
                return Some(i);
            }
            if slot.next == NO_SLOT {
                return None;
            }
            i = slot.next;
        }
    }

    /// Adds a slot for a key that is not there.
    fn push_new(&mut self, key: MaskKey, position: MaskPosition) -> usize {
        let hash = self.hasher.hash_one(&key);
        let i = self.slots.len();
        let next = self.index.insert(hash, i).unwrap_or(NO_SLOT);
        self.slots.push(Some(Slot {
            key,
            position,
            next,
        }));
        self.live += 1;
        i
    }

    /// The position with this key.
    pub fn get(&self, key: &MaskKey) -> Option<&MaskPosition> {
        let i = self.find(key)?;
        self.slots.get(i)?.as_ref().map(|s| &s.position)
    }

    /// The entry of `position`.
    pub fn position(&self, position: &Position) -> Option<&MaskPosition> {
        self.get(&MaskKey::from_position(position))
    }

    pub(crate) fn get_mut(&mut self, key: &MaskKey) -> Option<&mut MaskPosition> {
        let i = self.find(key)?;
        self.slots.get_mut(i)?.as_mut().map(|s| &mut s.position)
    }

    /// The positions in file order (new ones at the end).
    pub fn iter(&self) -> impl Iterator<Item = (&MaskKey, &MaskPosition)> {
        self.slots
            .iter()
            .filter_map(|s| s.as_ref().map(|s| (&s.key, &s.position)))
    }

    /// Inserts a position; an existing key keeps its place and its content is
    /// replaced (`array set`). Returns whether a position was replaced.
    ///
    /// The key and the position are normalized to what the file would give
    /// back (D1852). A key that cannot be stored, or a new key when the mask
    /// is full, is ignored (`false`): use [`TreeMask::try_insert`] to know.
    /// Meant for readers and tests; the editing operations are
    /// [`TreeMask::apply`].
    pub fn insert(&mut self, key: MaskKey, position: MaskPosition) -> bool {
        self.try_insert(key, position).unwrap_or(false)
    }

    /// Like [`TreeMask::insert`], but tells why a position was not stored.
    ///
    /// # Errors
    ///
    /// [`MaskInsertError::BadKey`] for a key longer than [`MAX_FIELD_BYTES`];
    /// [`MaskInsertError::Full`] for a new key in a mask with
    /// [`MAX_POSITIONS`] positions.
    // By value like `insert`, which the callers already use.
    #[allow(clippy::needless_pass_by_value)]
    pub fn try_insert(
        &mut self,
        key: MaskKey,
        position: MaskPosition,
    ) -> Result<bool, MaskInsertError> {
        let key = key.normalized().ok_or(MaskInsertError::BadKey)?;
        let position = normalize_position(&key, position);
        self.insert_raw(key, position)
    }

    /// Inserts what is already normalized (the reader's output).
    pub(crate) fn insert_raw(
        &mut self,
        key: MaskKey,
        position: MaskPosition,
    ) -> Result<bool, MaskInsertError> {
        if let Some(i) = self.find(&key)
            && let Some(Some(slot)) = self.slots.get_mut(i)
        {
            slot.position = position;
            return Ok(true);
        }
        if self.is_full() {
            return Err(MaskInsertError::Full);
        }
        self.push_new(key, position);
        Ok(false)
    }

    /// The entry for `key`, created empty at the end if missing. `None` when
    /// it is missing and the mask is full.
    pub(crate) fn entry(&mut self, key: &MaskKey) -> Option<&mut MaskPosition> {
        let i = match self.find(key) {
            Some(i) => i,
            None if self.is_full() => return None,
            None => self.push_new(key.clone(), MaskPosition::default()),
        };
        self.slots
            .get_mut(i)
            .and_then(Option::as_mut)
            .map(|s| &mut s.position)
    }

    /// Removes a position (the file order of the others is kept).
    pub(crate) fn remove(&mut self, key: &MaskKey) -> bool {
        let hash = self.hasher.hash_one(key);
        let Some(&head) = self.index.get(&hash) else {
            return false;
        };
        let (mut prev, mut cur) = (NO_SLOT, head);
        let (found, next) = loop {
            let Some(Some(slot)) = self.slots.get(cur) else {
                return false;
            };
            if slot.key == *key {
                break (cur, slot.next);
            }
            if slot.next == NO_SLOT {
                return false;
            }
            prev = cur;
            cur = slot.next;
        };
        if prev == NO_SLOT {
            if next == NO_SLOT {
                self.index.remove(&hash);
            } else {
                self.index.insert(hash, next);
            }
        } else if let Some(Some(p)) = self.slots.get_mut(prev) {
            p.next = next;
        }
        if let Some(slot) = self.slots.get_mut(found) {
            *slot = None;
        }
        self.live -= 1;
        if self.slots.len() > 2 * self.live + 16 {
            self.compact();
        }
        true
    }

    fn compact(&mut self) {
        self.slots.retain(Option::is_some);
        self.index.clear();
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if let Some(s) = slot {
                s.next = self
                    .index
                    .insert(self.hasher.hash_one(&s.key), i)
                    .unwrap_or(NO_SLOT);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_of_the_start_position_is_the_short_fen() {
        let key = MaskKey::from_position(&Position::new());
        assert_eq!(
            key.as_str(),
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -"
        );
        assert_eq!(MaskKey::from_text(key.as_str()), key);
        assert_eq!(
            MaskKey::from_text("  rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR   w KQkq - "),
            key
        );
    }

    #[test]
    fn bad_keys_are_opaque() {
        for t in ["", "garbage", "8/8/8/8/8/8/8/8 w - -", "a b c d e"] {
            assert_eq!(MaskKey::from_text(t), MaskKey::Opaque(t.to_owned()), "{t}");
        }
    }

    #[test]
    fn nag_color_marker_texts() {
        assert_eq!(MaskNag::from_text(" !"), Some(MaskNag::Good));
        assert_eq!(MaskNag::from_text(""), None);
        assert_eq!(MaskNag::from_text("+-"), Some(MaskNag::Other("+-".into())));
        assert_eq!(MaskColor::from_text("white"), MaskColor::White);
        assert_eq!(MaskColor::from_text("green"), MaskColor::Green);
        assert_eq!(MaskColor::from_text(""), MaskColor::White);
        assert!(MaskColor::from_text("#a0b1c2").is_hex());
        assert_eq!(Marker::from_text("tb_help"), Some(Marker::Dubious));
        assert_eq!(Marker::from_text("tb_cut"), Some(Marker::ToRemove));
        assert_eq!(Marker::from_text("zzz"), Some(Marker::Other("zzz".into())));
        for m in &Marker::ALL {
            assert_eq!(Marker::from_text(m.file_name()).as_ref(), Some(m));
        }
    }

    #[test]
    fn comments_are_cleaned_and_cut() {
        assert_eq!(clean_comment("  a\r\nb\rc \n").0, "a\nb\nc");
        let (c, cut) = clean_comment(&"\u{e9}".repeat(MAX_COMMENT_BYTES));
        assert!(cut && c.len() <= MAX_COMMENT_BYTES);
        assert!(!clean_comment("ok").1);
    }

    #[test]
    fn removal_keeps_order_and_compacts() {
        let mut m = TreeMask::new();
        let keys: Vec<MaskKey> = (0..100).map(|i| MaskKey::Opaque(format!("k{i}"))).collect();
        for k in &keys {
            m.insert(k.clone(), MaskPosition::default());
        }
        for k in keys.iter().take(90) {
            assert!(m.remove(k));
        }
        assert!(!m.remove(&keys[0]));
        assert_eq!(m.len(), 10);
        let left: Vec<_> = m.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(left, keys[90..]);
        assert!(m.get(&keys[95]).is_some());
    }
}
