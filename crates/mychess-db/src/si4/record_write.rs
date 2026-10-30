// Derived from Scid vs PC 4.27, src/game.cpp (Game::Encode, encodeTags,
// encodeVariation, encodeComments).
// Copyright (c) 2000-2003 Shane Hudson. All rights reserved (notice of
// game.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Encoding of one `.sg4` game record, the inverse of `RawGame::parse`
//! (`docs/formatos/si4.md`, section 9; `docs/specs/m3-2-escrita-si4.md`,
//! rules 10, 11 and 33).
//!
//! `[extra tags] 00 [flags] [FEN NUL] [move stream] 0F [comments]`. Like the
//! reader, the writer uses an explicit stack for variations (decision D266),
//! never recursion.

use mychess_core::{CastlingMode, Game, Move, NodeId};

use super::error::{Result, Si4Error};
use super::move_write::encode_move;
use super::position::{ListPosition, Undo};
use super::raw::{RawGame, RawLine, RawMove, RawNode};
use super::record::{MAX_EXTRA_TAGS, MAX_VARIATION_DEPTH};

/// The largest record the index can describe: its length has 17 bits. (Scid
/// accepts 131,072 but then stores a length of 0; decision of rule 10.)
pub const MAX_RECORD_LEN: usize = 131_071;
/// The largest number of extra tags the writer accepts (`MAX_TAGS`, rule 11).
pub const MAX_WRITE_TAGS: usize = 40;
/// A tag name is at most this long; larger codes name common tags.
const MAX_TAG_NAME_LEN: usize = 240;
/// A tag value is at most this long (its length is one byte).
const MAX_TAG_VALUE_LEN: usize = 255;
/// A move keeps at most this many NAGs (`game.h:132`).
const MAX_NAGS: usize = 7;
/// The text of the FEN of the standard start position.
const STANDARD_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

const TOKEN_NAG: u8 = 11;
const TOKEN_COMMENT: u8 = 12;
const TOKEN_START: u8 = 13;
const TOKEN_END: u8 = 14;
const TOKEN_END_GAME: u8 = 15;

/// The common tags and their codes 241 to 250.
const COMMON_TAGS: [&str; 10] = [
    "WhiteCountry",
    "BlackCountry",
    "Annotator",
    "PlyCount",
    "EventDate",
    "Opening",
    "Variation",
    "Setup",
    "Source",
    "SetUp",
];

/// An encoded `.sg4` record and the counts the index keeps about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedRecord {
    /// The bytes of the record.
    pub bytes: Vec<u8>,
    /// The number of variations (all depths).
    pub variation_count: u32,
    /// The number of comments (the game's, the variations' and the moves').
    pub comment_count: u32,
    /// The number of NAGs written.
    pub nag_count: u32,
}

/// The list position a record starts from: the one of its FEN, or the
/// standard one.
pub(crate) fn start_list_position(start_fen: Option<&[u8]>) -> Result<ListPosition> {
    match start_fen {
        None => Ok(ListPosition::standard()),
        Some(fen) => ListPosition::from_fen(&String::from_utf8_lossy(fen))
            .map_err(|e| Si4Error::UnsupportedStart { reason: e.reason() }),
    }
}

/// Encodes a raw game as a `.sg4` record.
///
/// The flags byte is rebuilt from `raw`: bit 0 from the presence of a start
/// FEN, bits 1 and 2 (promotions, under-promotions) kept as they are in
/// `raw.flags`. Tags are written in order; the names of the common tags
/// (`Annotator`, `PlyCount`...) use their one-byte codes as Scid does. An
/// event date from tag 255 is written first.
///
/// Errors: a tag that cannot be stored, a move the codec cannot represent
/// ([`Si4Error::UnencodableGame`] with its ply), a start position that is
/// not valid, text with a NUL byte, variations nested more than the reader
/// accepts, or a record longer than [`MAX_RECORD_LEN`].
pub fn encode_record(raw: &RawGame) -> Result<EncodedRecord> {
    let mut out = Vec::new();
    encode_tags(raw, &mut out)?;
    let has_fen = raw.start_fen.is_some();
    out.push(u8::from(has_fen) | (raw.flags & 6));
    if let Some(fen) = &raw.start_fen {
        if fen.contains(&0) {
            return Err(Si4Error::InvalidText { what: "start FEN" });
        }
        out.extend_from_slice(fen);
        out.push(0);
    }
    let start = start_list_position(raw.start_fen.as_deref())?;
    let mut counts = Counts::default();
    encode_stream(raw, start, &mut out, &mut counts)?;
    encode_comments(raw, &mut out, &mut counts)?;
    if out.len() > MAX_RECORD_LEN {
        return Err(Si4Error::RecordTooLarge {
            size: out.len(),
            limit: MAX_RECORD_LEN,
        });
    }
    Ok(EncodedRecord {
        bytes: out,
        variation_count: counts.variations,
        comment_count: counts.comments,
        nag_count: counts.nags,
    })
}

#[derive(Debug, Default)]
struct Counts {
    variations: u32,
    comments: u32,
    nags: u32,
}

fn invalid_tag(name: &[u8], reason: &'static str) -> Si4Error {
    let clipped = &name[..name.len().min(40)];
    Si4Error::InvalidTag {
        name: String::from_utf8_lossy(clipped).into_owned(),
        reason,
    }
}

fn encode_tags(raw: &RawGame, out: &mut Vec<u8>) -> Result<()> {
    if raw.extra_tags.len() > MAX_EXTRA_TAGS {
        return Err(invalid_tag(b"", "too many tags"));
    }
    if let Some(date) = raw.event_date {
        out.push(255);
        out.extend_from_slice(&date.to_scid_u32().to_be_bytes()[1..]);
    }
    for (name, value) in &raw.extra_tags {
        if name.is_empty() {
            return Err(invalid_tag(name, "empty tag name"));
        }
        if value.len() > MAX_TAG_VALUE_LEN {
            return Err(invalid_tag(name, "tag value longer than 255 bytes"));
        }
        let common = COMMON_TAGS
            .iter()
            .position(|c| c.as_bytes() == name.as_slice());
        if let Some(i) = common {
            out.push(u8::try_from(241 + i).unwrap_or(241));
        } else {
            if name.len() > MAX_TAG_NAME_LEN {
                return Err(invalid_tag(name, "tag name longer than 240 bytes"));
            }
            out.push(u8::try_from(name.len()).unwrap_or(0));
            out.extend_from_slice(name);
        }
        out.push(u8::try_from(value.len()).unwrap_or(0));
        out.extend_from_slice(value);
    }
    out.push(0);
    Ok(())
}

/// One open line while the move stream is written.
#[derive(Clone, Copy)]
struct Frame {
    line: usize,
    /// Index of the move being written.
    idx: usize,
    /// Whether the move at `idx` and its NAGs and comment are written.
    emitted: bool,
    /// How many variations of the move at `idx` are done.
    done: usize,
    /// Length of the undo stack when the line started.
    undo_base: usize,
    /// The number of moves played before the first move of the line.
    base_ply: u32,
}

/// The ply of the move at `idx` of a line that starts after `base` moves.
fn ply_of(base: u32, idx: usize) -> u32 {
    base.saturating_add(u32::try_from(idx).unwrap_or(u32::MAX))
        .saturating_add(1)
}

fn encode_stream(
    raw: &RawGame,
    start: ListPosition,
    out: &mut Vec<u8>,
    counts: &mut Counts,
) -> Result<()> {
    let internal = |ply: u32| Si4Error::UnencodableGame {
        ply,
        reason: "internal: inconsistent game tree",
    };
    let fail = |ply: u32| move |reason: &'static str| Si4Error::UnencodableGame { ply, reason };
    let mut pos = start;
    let mut undos: Vec<Undo> = Vec::new();
    let mut stack = vec![Frame {
        line: 0,
        idx: 0,
        emitted: false,
        done: 0,
        undo_base: 0,
        base_ply: 0,
    }];
    if raw.lines.first().is_some_and(|l| l.start_comment.is_some()) {
        out.push(TOKEN_COMMENT);
    }
    loop {
        let top = stack.len().checked_sub(1).ok_or_else(|| internal(0))?;
        let Frame {
            line: line_ix,
            idx,
            emitted,
            done,
            undo_base,
            base_ply,
        } = *stack.get(top).ok_or_else(|| internal(0))?;
        let ply = ply_of(base_ply, idx);
        let line = raw.lines.get(line_ix).ok_or_else(|| internal(ply))?;
        let Some(node) = line.moves.get(idx) else {
            // End of the line.
            if top == 0 {
                out.push(TOKEN_END_GAME);
                return Ok(());
            }
            out.push(TOKEN_END);
            while undos.len() > undo_base {
                let undo = undos.pop().ok_or_else(|| internal(ply))?;
                pos.unmake(undo).map_err(fail(ply))?;
            }
            stack.pop();
            // Back in the parent: play again the move the variation replaced.
            let parent = stack.last_mut().ok_or_else(|| internal(ply))?;
            let parent_ply = ply_of(parent.base_ply, parent.idx);
            let replaced = raw
                .lines
                .get(parent.line)
                .and_then(|l| l.moves.get(parent.idx))
                .ok_or_else(|| internal(parent_ply))?;
            undos.push(pos.make(replaced.mv).map_err(fail(parent_ply))?);
            parent.done += 1;
            continue;
        };
        if !emitted {
            let encoded = encode_move(&pos, node.mv).map_err(fail(ply))?;
            out.extend_from_slice(encoded.as_bytes());
            undos.push(pos.make(node.mv).map_err(fail(ply))?);
            for nag in node.nags.iter().take(MAX_NAGS) {
                out.extend_from_slice(&[TOKEN_NAG, nag.value()]);
                counts.nags += 1;
            }
            if node.comment.is_some() {
                out.push(TOKEN_COMMENT);
            }
            if let Some(frame) = stack.get_mut(top) {
                frame.emitted = true;
            }
        }
        if let Some(&variation) = node.variations.get(done) {
            if stack.len() > MAX_VARIATION_DEPTH {
                return Err(fail(ply)("variations nested too deeply"));
            }
            let sub = raw.lines.get(variation).ok_or_else(|| internal(ply))?;
            // The variation starts from the position before this move.
            let undo = undos.pop().ok_or_else(|| internal(ply))?;
            pos.unmake(undo).map_err(fail(ply))?;
            out.push(TOKEN_START);
            counts.variations += 1;
            if sub.start_comment.is_some() {
                out.push(TOKEN_COMMENT);
            }
            stack.push(Frame {
                line: variation,
                idx: 0,
                emitted: false,
                done: 0,
                undo_base: undos.len(),
                base_ply: ply - 1,
            });
            continue;
        }
        if let Some(frame) = stack.get_mut(top) {
            frame.idx += 1;
            frame.emitted = false;
            frame.done = 0;
        }
    }
}

/// A slot that may hold a comment, in pre-order.
enum Slot {
    Line(usize),
    Start(usize),
    Node(usize, usize),
}

/// Writes the comment strings after the end-of-game token in the order the
/// reader expects: the start comment of a line, then for each move its comment
/// followed by the lines of its variations.
fn encode_comments(raw: &RawGame, out: &mut Vec<u8>, counts: &mut Counts) -> Result<()> {
    let mut put = |text: &[u8]| -> Result<()> {
        if text.contains(&0) {
            return Err(Si4Error::InvalidText { what: "comment" });
        }
        out.extend_from_slice(text);
        out.push(0);
        counts.comments += 1;
        Ok(())
    };
    let mut stack = vec![Slot::Line(0)];
    while let Some(slot) = stack.pop() {
        match slot {
            Slot::Line(l) => {
                let Some(line) = raw.lines.get(l) else {
                    continue;
                };
                for (i, node) in line.moves.iter().enumerate().rev() {
                    for &v in node.variations.iter().rev() {
                        stack.push(Slot::Line(v));
                    }
                    stack.push(Slot::Node(l, i));
                }
                stack.push(Slot::Start(l));
            }
            Slot::Start(l) => {
                if let Some(text) = raw.lines.get(l).and_then(|ln| ln.start_comment.as_deref()) {
                    put(text)?;
                }
            }
            Slot::Node(l, i) => {
                if let Some(text) = raw
                    .lines
                    .get(l)
                    .and_then(|ln| ln.moves.get(i))
                    .and_then(|n| n.comment.as_deref())
                {
                    put(text)?;
                }
            }
        }
    }
    Ok(())
}

/// Appends `extra` to a comment, separated by a space.
fn merge_comment(slot: &mut Option<Vec<u8>>, extra: &str) {
    match slot {
        Some(text) => {
            text.push(b' ');
            text.extend_from_slice(extra.as_bytes());
        }
        None => *slot = Some(extra.as_bytes().to_vec()),
    }
}

/// Converts a game of the core to the raw tree of a record: extra tags, the
/// start FEN and the lines (main line first) with NAGs and comments.
///
/// Variations of a move are the other children of its parent; a comment
/// before a move becomes the start comment of its variation. A comment
/// before a move that is not the first of its line has no place in the
/// format: it is appended to the comment of the previous move (or to the
/// start comment of the line), so no text is lost. At most 7 NAGs per move are
/// kept.
///
/// Errors: more than [`MAX_WRITE_TAGS`] extra tags, a tag name that is
/// empty or longer than 240 bytes, a value longer than 255 bytes
/// ([`Si4Error::InvalidTag`], decision D280); a start position that is
/// Chess960 or has more than 16 pieces for a side
/// ([`Si4Error::UnsupportedStart`], decision D285); a null move without a king.
pub fn raw_from_game(game: &Game) -> Result<RawGame> {
    let mut raw = RawGame::default();
    raw_tags(game, &mut raw)?;
    raw_start(game, &mut raw)?;
    raw_lines(game, &mut raw)?;
    // The promotion flags of the record come from the main line.
    if let Some(main) = raw.lines.first() {
        for node in &main.moves {
            if let Some(role) = node.mv.promotion {
                raw.flags |= 2;
                if role != mychess_core::Role::Queen {
                    raw.flags |= 4;
                }
            }
        }
    }
    Ok(raw)
}

/// Copies the extra tags, checking the limits of decision D280.
fn raw_tags(game: &Game, raw: &mut RawGame) -> Result<()> {
    let extras = game.tags().extras();
    if extras.len() > MAX_WRITE_TAGS {
        return Err(invalid_tag(b"", "more than 40 extra tags"));
    }
    for (name, value) in extras {
        if name.is_empty() {
            return Err(invalid_tag(name.as_bytes(), "empty tag name"));
        }
        if name.len() > MAX_TAG_NAME_LEN {
            return Err(invalid_tag(
                name.as_bytes(),
                "tag name longer than 240 bytes",
            ));
        }
        if value.len() > MAX_TAG_VALUE_LEN {
            return Err(invalid_tag(
                name.as_bytes(),
                "tag value longer than 255 bytes",
            ));
        }
        raw.extra_tags
            .push((name.as_bytes().to_vec(), value.as_bytes().to_vec()));
    }
    Ok(())
}

/// Sets the start FEN when the game does not start from the standard position.
fn raw_start(game: &Game, raw: &mut RawGame) -> Result<()> {
    let start = game.start_position();
    let fen = start.fen();
    if fen != STANDARD_FEN {
        if start.castling_mode() == CastlingMode::Chess960 {
            return Err(Si4Error::UnsupportedStart {
                reason: "Chess960 start positions cannot be stored",
            });
        }
        raw.start_fen = Some(fen.into_bytes());
    }
    Ok(())
}

/// Builds the lines of the raw tree, main line first.
fn raw_lines(game: &Game, raw: &mut RawGame) -> Result<()> {
    raw.lines.push(RawLine {
        start_comment: game.initial_comment().map(|c| c.as_bytes().to_vec()),
        moves: Vec::new(),
    });
    // (line index, first node of the line)
    let mut tasks: Vec<(usize, NodeId)> = Vec::new();
    if let Some(&first) = game
        .children(game.root())
        .map_err(|_| internal_error())?
        .first()
    {
        tasks.push((0, first));
    }
    while let Some((line_index, first)) = tasks.pop() {
        let mut current = Some(first);
        let mut moves: Vec<RawNode> = Vec::new();
        let mut start_comment: Option<Vec<u8>> = raw
            .lines
            .get(line_index)
            .and_then(|l| l.start_comment.clone());
        while let Some(id) = current {
            let parent = game
                .parent(id)
                .map_err(|_| internal_error())?
                .ok_or_else(internal_error)?;
            let siblings = game.children(parent).map_err(|_| internal_error())?;
            let mv = raw_move(game, parent, id)?;
            let mut node = RawNode {
                mv,
                nags: game
                    .nags(id)
                    .map_err(|_| internal_error())?
                    .iter()
                    .take(MAX_NAGS)
                    .copied()
                    .collect(),
                comment: game
                    .comment(id)
                    .map_err(|_| internal_error())?
                    .map(|c| c.as_bytes().to_vec()),
                variations: Vec::new(),
            };
            if let Some(before) = game.comment_before(id).map_err(|_| internal_error())? {
                match moves.last_mut() {
                    Some(previous) => merge_comment(&mut previous.comment, before),
                    None => merge_comment(&mut start_comment, before),
                }
            }
            // Variations hang from the main continuation of the parent.
            if siblings.first() == Some(&id) {
                for &sibling in siblings.iter().skip(1) {
                    let index = raw.lines.len();
                    raw.lines.push(RawLine::default());
                    node.variations.push(index);
                    tasks.push((index, sibling));
                }
            }
            moves.push(node);
            current = game
                .children(id)
                .map_err(|_| internal_error())?
                .first()
                .copied();
        }
        if let Some(line) = raw.lines.get_mut(line_index) {
            line.moves = moves;
            line.start_comment = start_comment;
        }
    }
    Ok(())
}

fn internal_error() -> Si4Error {
    Si4Error::UnencodableGame {
        ply: 0,
        reason: "internal: inconsistent game tree",
    }
}

/// The raw move of node `id`, played from the position of `parent`. Castling
/// is the king's two-square move and the null move is the king staying on
/// its square.
fn raw_move(game: &Game, parent: NodeId, id: NodeId) -> Result<RawMove> {
    let ply = game.ply(id).unwrap_or(0);
    let mv = game.mv(id).map_err(|_| internal_error())?;
    let squares = match mv {
        Move::Null => {
            let pos = game.position_at(parent).map_err(|_| internal_error())?;
            let king = pos
                .king_square(pos.turn())
                .ok_or(Si4Error::UnencodableGame {
                    ply,
                    reason: "null move without a king",
                })?;
            Some((king, king))
        }
        other => other.from().zip(other.to()),
    };
    let (from, to) = squares.ok_or(Si4Error::UnencodableGame {
        ply,
        reason: "move without squares",
    })?;
    Ok(RawMove {
        from,
        to,
        promotion: mv.promotion(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::{Nag, Position};

    fn record(bytes: &[u8]) -> RawGame {
        RawGame::parse(bytes).unwrap()
    }

    #[test]
    fn the_minimal_record_round_trips() {
        let bytes = [0u8, 0, 0xCF, 0xCF, 0x0F];
        let enc = encode_record(&record(&bytes)).unwrap();
        assert_eq!(enc.bytes, bytes);
        assert_eq!(
            (enc.variation_count, enc.comment_count, enc.nag_count),
            (0, 0, 0)
        );
    }

    #[test]
    fn tags_use_common_codes_and_keep_their_order() {
        let raw = RawGame {
            extra_tags: vec![
                (b"Zeta".to_vec(), b"1".to_vec()),
                (b"Annotator".to_vec(), b"Me".to_vec()),
                (b"SetUp".to_vec(), b"1".to_vec()),
            ],
            lines: vec![RawLine::default()],
            ..RawGame::default()
        };
        let enc = encode_record(&raw).unwrap();
        assert_eq!(
            enc.bytes,
            [
                4, b'Z', b'e', b't', b'a', 1, b'1', 243, 2, b'M', b'e', 250, 1, b'1', 0, 0, 15
            ]
        );
        assert_eq!(record(&enc.bytes).extra_tags, raw.extra_tags);
    }

    #[test]
    fn variations_nags_and_comments_round_trip_byte_for_byte() {
        // 1.e4 {a} (1.d4 {b} d5) e5 $1 $3 {c} (1...c5 (1...e6)) 2.Nf3
        let mut bytes = vec![0u8, 0];
        bytes.extend_from_slice(&[0xCF, 12, 13, 12, 0xBF, 0xBF, 14]);
        bytes.extend_from_slice(&[0xCF, 11, 1, 11, 3, 12, 13, 0xAF, 13, 0xC1, 14, 14, 0x28, 15]);
        bytes.extend_from_slice(b"a\0b\0c\0");
        let raw = record(&bytes);
        let enc = encode_record(&raw).unwrap();
        assert_eq!(enc.bytes, bytes);
        assert_eq!(enc.variation_count, 3);
        assert_eq!(enc.comment_count, 3);
        assert_eq!(enc.nag_count, 2);
    }

    #[test]
    fn captures_inside_variations_restore_the_piece_lists() {
        let stream = [
            0xCF, 0xBF, 0xC0, 13, 0x28, 14, 0x4C, 13, 0x61, 14, 0xC1, 0xBF, 15,
        ];
        let mut bytes = vec![0u8, 0];
        bytes.extend_from_slice(&stream);
        let enc = encode_record(&record(&bytes)).unwrap();
        assert_eq!(enc.bytes, bytes);
    }

    #[test]
    fn start_positions_and_start_comments_round_trip() {
        let mut bytes = vec![0u8, 1];
        bytes.extend_from_slice(b"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\0");
        bytes.extend_from_slice(&[12, 0x1C, 13, 12, 0x1A, 0x02, 14, 0x01, 0x1E, 15]);
        bytes.extend_from_slice(b"intro\0first\0");
        let enc = encode_record(&record(&bytes)).unwrap();
        assert_eq!(enc.bytes, bytes);
        assert_eq!(enc.comment_count, 2);
    }

    #[test]
    fn errors_for_bad_tags_moves_and_nul_text() {
        let tagged = |name: &[u8], value: &[u8]| {
            let mut raw = RawGame::default();
            raw.extra_tags.push((name.to_vec(), value.to_vec()));
            raw.lines.push(RawLine::default());
            raw
        };
        assert!(matches!(
            encode_record(&tagged(b"", b"x")),
            Err(Si4Error::InvalidTag { .. })
        ));
        assert!(matches!(
            encode_record(&tagged(b"N", &[b'x'; 256])),
            Err(Si4Error::InvalidTag { .. })
        ));
        assert!(encode_record(&tagged(b"N", &[b'x'; 255])).is_ok());
        assert!(matches!(
            encode_record(&tagged(&[b'n'; 241], b"x")),
            Err(Si4Error::InvalidTag { .. })
        ));
        // A move the codec cannot write: a rook that moves diagonally.
        let mut raw = RawGame::default();
        raw.lines.push(RawLine {
            start_comment: None,
            moves: vec![RawNode {
                mv: RawMove {
                    from: mychess_core::Square::A1,
                    to: mychess_core::Square::B2,
                    promotion: None,
                },
                nags: vec![],
                comment: None,
                variations: vec![],
            }],
        });
        assert!(matches!(
            encode_record(&raw),
            Err(Si4Error::UnencodableGame { ply: 1, .. })
        ));
        // NUL in a comment.
        let mut raw = RawGame::default();
        raw.lines.push(RawLine {
            start_comment: Some(b"a\0b".to_vec()),
            moves: vec![],
        });
        assert!(matches!(
            encode_record(&raw),
            Err(Si4Error::InvalidText { .. })
        ));
    }

    #[test]
    fn the_record_size_limit_is_131071() {
        // Tag values of 255 bytes: 3 + 1 + 255 + 1 bytes each (name "T00").
        let mut raw = RawGame::default();
        raw.lines.push(RawLine::default());
        let huge = Some(vec![b'x'; MAX_RECORD_LEN - 5]);
        raw.lines[0].start_comment = huge;
        // tags end, flags, COMMENT and END_GAME (4 bytes) + text + NUL.
        assert_eq!(encode_record(&raw).unwrap().bytes.len(), MAX_RECORD_LEN);
        raw.lines[0].start_comment = Some(vec![b'x'; MAX_RECORD_LEN - 4]);
        assert!(matches!(
            encode_record(&raw),
            Err(Si4Error::RecordTooLarge {
                size: 131_072,
                limit: 131_071
            })
        ));
    }

    #[test]
    fn deep_variations_are_written_without_recursion() {
        // Null moves keep the piece lists intact (as in the reader's test).
        let depth = 9_000;
        let mut stream = vec![0u8, 0, 0x00];
        for _ in 0..depth {
            stream.extend_from_slice(&[TOKEN_START, 0x00]);
        }
        stream.extend(std::iter::repeat_n(TOKEN_END, depth));
        stream.push(TOKEN_END_GAME);
        let raw = record(&stream);
        assert_eq!(encode_record(&raw).unwrap().bytes, stream);
        // Beyond the limit the reader would refuse the record, so the writer
        // does too. The chain is built by hand: line `i` has one null move
        // with line `i + 1` as its only variation.
        let mut raw = RawGame::default();
        let total = MAX_VARIATION_DEPTH + 3;
        for i in 0..total {
            raw.lines.push(RawLine {
                start_comment: None,
                moves: vec![RawNode {
                    mv: RawMove {
                        from: mychess_core::Square::E1,
                        to: mychess_core::Square::E1,
                        promotion: None,
                    },
                    nags: vec![],
                    comment: None,
                    variations: if i + 1 < total { vec![i + 1] } else { vec![] },
                }],
            });
        }
        assert!(matches!(
            encode_record(&raw),
            Err(Si4Error::UnencodableGame {
                reason: "variations nested too deeply",
                ..
            })
        ));
    }

    #[test]
    fn a_game_becomes_a_raw_tree_with_variations_and_comments() {
        let mut game = Game::new();
        let pos = Position::new();
        let e4 = game
            .add_move(game.root(), pos.parse_uci("e2e4").unwrap())
            .unwrap();
        let d4 = game
            .add_move(game.root(), pos.parse_uci("d2d4").unwrap())
            .unwrap();
        game.set_comment(e4, "main").unwrap();
        game.set_comment_before(d4, "alt").unwrap();
        game.set_initial_comment("intro");
        game.add_nag(e4, Nag::new(1).unwrap()).unwrap();
        game.tags_mut().set_extra("Annotator", "Me").unwrap();
        let raw = raw_from_game(&game).unwrap();
        assert_eq!(raw.lines.len(), 2);
        assert_eq!(raw.lines[0].start_comment.as_deref(), Some(&b"intro"[..]));
        assert_eq!(raw.lines[0].moves[0].variations, vec![1]);
        assert_eq!(raw.lines[1].start_comment.as_deref(), Some(&b"alt"[..]));
        assert_eq!(raw.start_fen, None);
        let enc = encode_record(&raw).unwrap();
        assert_eq!(
            (enc.variation_count, enc.comment_count, enc.nag_count),
            (1, 3, 1)
        );
        let back = RawGame::parse(&enc.bytes).unwrap();
        assert_eq!(back.lines.len(), 2);
        assert_eq!(back.mainline()[0].comment.as_deref(), Some(&b"main"[..]));
    }

    #[test]
    fn games_limits_and_chess960_are_refused() {
        let mut game = Game::new();
        for i in 0..40 {
            game.tags_mut().set_extra(&format!("T{i}"), "v").unwrap();
        }
        assert!(raw_from_game(&game).is_ok());
        game.tags_mut().set_extra("T40", "v").unwrap();
        assert!(matches!(
            raw_from_game(&game),
            Err(Si4Error::InvalidTag { .. })
        ));

        let mut game = Game::new();
        game.tags_mut().set_extra("Long", &"x".repeat(256)).unwrap();
        assert!(matches!(
            raw_from_game(&game),
            Err(Si4Error::InvalidTag { .. })
        ));

        let chess960 = Position::chess960_start(0).unwrap();
        assert!(matches!(
            raw_from_game(&Game::from_position(chess960)),
            Err(Si4Error::UnsupportedStart { .. })
        ));
        // The standard arrangement is not Chess960 even in that mode.
        let standard = Position::chess960_start(518).unwrap();
        assert!(raw_from_game(&Game::from_position(standard)).is_ok());
    }
}
