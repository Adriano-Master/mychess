// Derived from Scid vs PC 4.27, src/game.cpp (DecodeVariation, DecodeTags, comments).
// Copyright (c) 2000-2003 Shane Hudson. All rights reserved (notice of
// game.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Decoding of one `.sg4` game record (`docs/formatos/si4.md`, section 9).
//!
//! `[extra tags] 00 [flags] [FEN NUL] [move stream] 0F [comments]`. Variations
//! are decoded with an explicit stack (decision D266), never recursively.

use std::ops::ControlFlow;

use mychess_core::Nag;

use super::bytes::Cursor;
use super::entry::date_from_bits;
use super::error::RecordError;
use super::moves::decode_move;
use super::position::{ListPosition, Undo};
use super::raw::{RawGame, RawLine, RawMove, RawNode};

/// Special tokens of the move stream (`docs/formatos/si4.md`, 9.3).
const TOKEN_NAG: u8 = 11;
const TOKEN_COMMENT: u8 = 12;
const TOKEN_START: u8 = 13;
const TOKEN_END: u8 = 14;
const TOKEN_END_GAME: u8 = 15;

/// Largest variation depth accepted (decision D266).
pub const MAX_VARIATION_DEPTH: usize = 10_000;
/// Largest number of distinct extra tags accepted (decision D258).
pub const MAX_EXTRA_TAGS: usize = 255;
/// A tag name is at most this long; larger codes are common-tag names.
const MAX_TAG_LEN: u8 = 240;
/// A game keeps at most this many NAGs per move (`game.h:132`).
const MAX_NAGS: usize = 7;

/// The common tags of codes 241 to 250 (`game.cpp`, `commonTags`).
const COMMON_TAGS: [&[u8]; 10] = [
    b"WhiteCountry",
    b"BlackCountry",
    b"Annotator",
    b"PlyCount",
    b"EventDate",
    b"Opening",
    b"Variation",
    b"Setup",
    b"Source",
    b"SetUp",
];

/// What precedes the move stream.
struct Prelude {
    extra_tags: Vec<(Vec<u8>, Vec<u8>)>,
    event_date: Option<mychess_core::PartialDate>,
    flags: u8,
    start_fen: Option<Vec<u8>>,
    position: ListPosition,
}

impl RawGame {
    /// Decodes a whole record: tags, start position, moves with variations,
    /// NAGs and comments. Errors carry the byte offset within the record.
    pub fn parse(record: &[u8]) -> Result<RawGame, RecordError> {
        let mut cur = Cursor::new(record);
        let prelude = parse_prelude(&mut cur)?;
        let mut lines = parse_stream(&mut cur, prelude.position)?;
        parse_comments(&mut cur, &mut lines)?;
        // Bytes after the last comment are tolerated, as Scid does.
        Ok(RawGame {
            extra_tags: prelude.extra_tags,
            event_date: prelude.event_date,
            flags: prelude.flags,
            start_fen: prelude.start_fen,
            lines,
        })
    }
}

/// Decodes only the main line of a record, skipping tags, variations,
/// NAGs and comments (Scid's `DecodeNextMove` path, used by searches).
pub fn mainline_of_record(record: &[u8]) -> Result<Vec<RawMove>, RecordError> {
    let mut moves = Vec::new();
    walk_mainline(record, |_, next| {
        if let Some(mv) = next {
            moves.push(mv);
        }
        ControlFlow::<()>::Continue(())
    })?;
    Ok(moves)
}

/// Walks the main line of a record one move at a time, like Scid's
/// `Game::ExactMatch` (`game.cpp`): tags, variations, NAGs and comments are
/// skipped and nothing after the point where `visit` breaks is decoded.
///
/// `visit` is called with the position before each move and the move that is
/// played from it, and a last time with `None` at the end of the game. If it
/// returns [`ControlFlow::Break`] the walk stops and the value is returned in
/// `Ok(Some(_))`; errors in the rest of the record are then never seen.
/// `Ok(None)` means the walk reached the end of the game.
pub(crate) fn walk_mainline<B>(
    record: &[u8],
    mut visit: impl FnMut(&ListPosition, Option<RawMove>) -> ControlFlow<B>,
) -> Result<Option<B>, RecordError> {
    let mut cur = Cursor::new(record);
    let mut pos = parse_prelude(&mut cur)?.position;
    loop {
        let rest = cur.rest();
        let Some(&byte) = rest.first() else {
            return Err(cur.error("move stream ends without an end-of-game token"));
        };
        match byte {
            TOKEN_END_GAME => {
                return Ok(match visit(&pos, None) {
                    ControlFlow::Break(b) => Some(b),
                    ControlFlow::Continue(()) => None,
                });
            }
            TOKEN_END => return Err(cur.error("end of variation in the main line")),
            TOKEN_COMMENT => cur.skip(1),
            TOKEN_NAG => {
                cur.skip(1);
                cur.u8("NAG token without a value")?;
            }
            TOKEN_START => {
                cur.skip(1);
                skip_variation(&mut cur)?;
            }
            _ => {
                let (mv, used) = decode_move(&pos, rest).map_err(|e| cur.error(e.reason()))?;
                if let ControlFlow::Break(b) = visit(&pos, Some(mv)) {
                    return Ok(Some(b));
                }
                pos.make(mv).map_err(|r| cur.error(r))?;
                cur.skip(used);
            }
        }
    }
}

/// The start position (FEN bytes) of a record, `None` for the standard start.
/// Reads only the tags and the flags, not the moves.
pub(crate) fn start_fen_of_record(record: &[u8]) -> Result<Option<Vec<u8>>, RecordError> {
    let mut cur = Cursor::new(record);
    Ok(parse_prelude(&mut cur)?.start_fen)
}

/// A tag as stored: name and value, raw bytes.
pub(crate) type RawTag = (Vec<u8>, Vec<u8>);

/// The extra tags of a record (names and values as raw bytes, common-tag codes
/// expanded), without decoding the moves. Used by the tag maintenance.
pub(crate) fn extra_tags_of_record(record: &[u8]) -> Result<Vec<RawTag>, RecordError> {
    let mut cur = Cursor::new(record);
    Ok(parse_prelude(&mut cur)?.extra_tags)
}

/// Skips a variation after its start token, with its nested variations.
fn skip_variation(cur: &mut Cursor<'_>) -> Result<(), RecordError> {
    let mut depth = 1usize;
    while depth > 0 {
        match cur.u8("variation without an end token")? {
            TOKEN_NAG => {
                cur.u8("NAG token without a value")?;
            }
            TOKEN_START => depth += 1,
            TOKEN_END => depth -= 1,
            TOKEN_END_GAME => return Err(cur.error("end of game inside a variation")),
            _ => {}
        }
    }
    Ok(())
}

fn parse_prelude(cur: &mut Cursor<'_>) -> Result<Prelude, RecordError> {
    let mut extra_tags: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut event_date = None;
    loop {
        let code = cur.u8("tag list is not terminated")?;
        let name: &[u8] = match code {
            0 => break,
            255 => {
                let bytes = cur.bytes(3, "event date tag is truncated")?;
                let bits = bytes.iter().fold(0u32, |acc, &b| (acc << 8) | u32::from(b));
                event_date = Some(date_from_bits(bits & 0x000F_FFFF));
                continue;
            }
            241..=254 => COMMON_TAGS
                .get(usize::from(code - MAX_TAG_LEN - 1))
                .copied()
                .ok_or_else(|| cur.error("unassigned common tag code"))?,
            len => cur.bytes(usize::from(len), "tag name is truncated")?,
        };
        let value_len = cur.u8("tag value length is missing")?;
        let value = cur.bytes(usize::from(value_len), "tag value is truncated")?;
        if let Some((_, v)) = extra_tags.iter_mut().find(|(n, _)| n.as_slice() == name) {
            *v = value.to_vec();
        } else if extra_tags.len() >= MAX_EXTRA_TAGS {
            return Err(cur.error("too many extra tags"));
        } else {
            extra_tags.push((name.to_vec(), value.to_vec()));
        }
    }

    let flags = cur.u8("game flags are missing")?;
    let (start_fen, position) = if flags & 1 != 0 {
        let fen = cur.c_string("start position is not terminated")?;
        let text = String::from_utf8_lossy(fen);
        let position = ListPosition::from_fen(&text).map_err(|e| cur.error(e.reason()))?;
        (Some(fen.to_vec()), position)
    } else {
        (None, ListPosition::standard())
    };
    Ok(Prelude {
        extra_tags,
        event_date,
        flags,
        start_fen,
        position,
    })
}

/// One open line during stream decoding.
struct Frame {
    /// Index of the line in the arena.
    line: usize,
    /// Length of the undo stack when the line started.
    undo_base: usize,
    /// The last move played in this line (to replay it after a variation).
    last: Option<RawMove>,
}

/// State of the move-stream decoder: the position, the arena of lines, the
/// stack of open lines and the undo records of the moves played.
struct Stream {
    pos: ListPosition,
    lines: Vec<RawLine>,
    frames: Vec<Frame>,
    undos: Vec<Undo>,
}

impl Stream {
    fn new(pos: ListPosition) -> Stream {
        Stream {
            pos,
            lines: vec![RawLine::default()],
            frames: vec![Frame {
                line: 0,
                undo_base: 0,
                last: None,
            }],
            undos: Vec::new(),
        }
    }

    fn current(&self, cur: &Cursor<'_>) -> Result<&Frame, RecordError> {
        self.frames
            .last()
            .ok_or_else(|| cur.error("internal: no open line"))
    }

    /// Plays a move in the current line.
    fn play(&mut self, cur: &mut Cursor<'_>) -> Result<(), RecordError> {
        let line = self.current(cur)?.line;
        let (mv, used) = decode_move(&self.pos, cur.rest()).map_err(|e| cur.error(e.reason()))?;
        let undo = self.pos.make(mv).map_err(|r| cur.error(r))?;
        cur.skip(used);
        self.undos.push(undo);
        if let Some(frame) = self.frames.last_mut() {
            frame.last = Some(mv);
        }
        self.lines
            .get_mut(line)
            .ok_or_else(|| cur.error("internal: line"))?
            .moves
            .push(RawNode {
                mv,
                nags: Vec::new(),
                comment: None,
                variations: Vec::new(),
            });
        Ok(())
    }

    /// Opens a variation: an alternative to the last move of the current line.
    fn start_variation(&mut self, cur: &Cursor<'_>) -> Result<(), RecordError> {
        let frame = self.current(cur)?;
        let line = frame.line;
        if frame.last.is_none() {
            return Err(cur.error("variation before the first move of its line"));
        }
        if self.frames.len() > MAX_VARIATION_DEPTH {
            return Err(cur.error("variations nested too deeply"));
        }
        // The variation starts from the position before the last move.
        let undo = self
            .undos
            .pop()
            .ok_or_else(|| cur.error("internal: undo stack"))?;
        self.pos.unmake(undo).map_err(|r| cur.error(r))?;
        let new_index = self.lines.len();
        self.lines.push(RawLine::default());
        self.lines
            .get_mut(line)
            .and_then(|l| l.moves.last_mut())
            .ok_or_else(|| cur.error("internal: variation owner"))?
            .variations
            .push(new_index);
        self.frames.push(Frame {
            line: new_index,
            undo_base: self.undos.len(),
            last: None,
        });
        Ok(())
    }

    /// Closes the current variation: back to the position before it, then
    /// replays the move it replaces in the parent line.
    fn end_variation(&mut self, cur: &Cursor<'_>) -> Result<(), RecordError> {
        if self.frames.len() == 1 {
            return Err(cur.error("end of variation in the main line"));
        }
        let base = self.current(cur)?.undo_base;
        while self.undos.len() > base {
            let undo = self
                .undos
                .pop()
                .ok_or_else(|| cur.error("internal: undo stack"))?;
            self.pos.unmake(undo).map_err(|r| cur.error(r))?;
        }
        self.frames.pop();
        let replaced = self
            .frames
            .last()
            .and_then(|f| f.last)
            .ok_or_else(|| cur.error("variation without a move to replace"))?;
        let undo = self.pos.make(replaced).map_err(|r| cur.error(r))?;
        self.undos.push(undo);
        Ok(())
    }

    /// Attaches a NAG to the last move of the current line. NAG 0 and NAGs
    /// beyond the seventh are dropped; a NAG before any move is ignored.
    fn add_nag(&mut self, value: u8, cur: &Cursor<'_>) -> Result<(), RecordError> {
        let line = self.current(cur)?.line;
        let node = self.lines.get_mut(line).and_then(|l| l.moves.last_mut());
        if let (Some(node), Ok(nag)) = (node, Nag::new(u16::from(value)))
            && node.nags.len() < MAX_NAGS
        {
            node.nags.push(nag);
        }
        Ok(())
    }

    /// Marks that the last move (or the start of the line) has a comment; an
    /// empty buffer stands for "present", the text is read after the stream.
    fn mark_comment(&mut self, cur: &Cursor<'_>) -> Result<(), RecordError> {
        let line = self.current(cur)?.line;
        let line = self
            .lines
            .get_mut(line)
            .ok_or_else(|| cur.error("internal: line"))?;
        match line.moves.last_mut() {
            Some(node) => node.comment.get_or_insert_with(Vec::new),
            None => line.start_comment.get_or_insert_with(Vec::new),
        };
        Ok(())
    }
}

/// Decodes the move stream into the line arena; stops after the end-of-game
/// token (consumed).
fn parse_stream(cur: &mut Cursor<'_>, pos: ListPosition) -> Result<Vec<RawLine>, RecordError> {
    let mut stream = Stream::new(pos);
    loop {
        let Some(&byte) = cur.rest().first() else {
            return Err(cur.error("move stream ends without an end-of-game token"));
        };
        match byte {
            TOKEN_END_GAME => {
                if stream.frames.len() != 1 {
                    return Err(cur.error("end of game inside a variation"));
                }
                cur.skip(1);
                return Ok(stream.lines);
            }
            TOKEN_END => {
                stream.end_variation(cur)?;
                cur.skip(1);
            }
            TOKEN_START => {
                stream.start_variation(cur)?;
                cur.skip(1);
            }
            TOKEN_NAG => {
                cur.skip(1);
                let value = cur.u8("NAG token without a value")?;
                stream.add_nag(value, cur)?;
            }
            TOKEN_COMMENT => {
                stream.mark_comment(cur)?;
                cur.skip(1);
            }
            _ => stream.play(cur)?,
        }
    }
}

/// A slot that may hold a comment, in pre-order.
enum Slot {
    Line(usize),
    Start(usize),
    Node(usize, usize),
}

/// Reads the comment strings after the end-of-game token and assigns them in
/// pre-order: the start comment of a line, then for each move its comment and
/// the lines of its variations in order.
fn parse_comments(cur: &mut Cursor<'_>, lines: &mut [RawLine]) -> Result<(), RecordError> {
    let mut stack = vec![Slot::Line(0)];
    while let Some(slot) = stack.pop() {
        match slot {
            Slot::Line(l) => {
                let line = lines
                    .get(l)
                    .ok_or_else(|| cur.error("internal: line index"))?;
                for (i, node) in line.moves.iter().enumerate().rev() {
                    for &v in node.variations.iter().rev() {
                        stack.push(Slot::Line(v));
                    }
                    stack.push(Slot::Node(l, i));
                }
                stack.push(Slot::Start(l));
            }
            Slot::Start(l) => {
                if let Some(c) = lines.get_mut(l).and_then(|ln| ln.start_comment.as_mut()) {
                    *c = cur
                        .c_string("comment is missing or not terminated")?
                        .to_vec();
                }
            }
            Slot::Node(l, i) => {
                if let Some(c) = lines
                    .get_mut(l)
                    .and_then(|ln| ln.moves.get_mut(i))
                    .and_then(|n| n.comment.as_mut())
                {
                    *c = cur
                        .c_string("comment is missing or not terminated")?
                        .to_vec();
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::Square;

    /// Wraps a move stream and comments into a record with no tags.
    fn record(stream: &[u8]) -> Vec<u8> {
        let mut r = vec![0u8, 0u8];
        r.extend_from_slice(stream);
        r
    }

    #[test]
    fn parses_the_minimal_example() {
        let game = RawGame::parse(&record(&[0xCF, 0xCF, 0x0F])).unwrap();
        assert_eq!(game.mainline().len(), 2);
        assert_eq!(game.mainline()[0].mv.from, Square::E2);
        assert_eq!(game.mainline()[1].mv.to, Square::E5);
        assert_eq!(game.lines.len(), 1);
        assert_eq!(game.flags, 0);
        assert_eq!(game.start_fen, None);
        assert_eq!(game.move_count(), 2);
    }

    #[test]
    fn parses_tags_literal_common_and_event_date() {
        let mut r = vec![];
        r.extend_from_slice(&[4]);
        r.extend_from_slice(b"Name");
        r.push(3);
        r.extend_from_slice(b"abc");
        r.push(243); // Annotator
        r.push(2);
        r.extend_from_slice(b"Me");
        r.push(255);
        let date = (2010u32 << 9) | (5 << 5) | 7;
        r.extend_from_slice(&date.to_be_bytes()[1..]);
        r.push(250); // SetUp
        r.push(1);
        r.push(b'1');
        r.push(0); // end of tags
        r.extend_from_slice(&[0, 0x0F]);
        let game = RawGame::parse(&r).unwrap();
        assert_eq!(
            game.extra_tags,
            vec![
                (b"Name".to_vec(), b"abc".to_vec()),
                (b"Annotator".to_vec(), b"Me".to_vec()),
                (b"SetUp".to_vec(), b"1".to_vec()),
            ]
        );
        let d = game.event_date.unwrap();
        assert_eq!(
            (d.year(), d.month(), d.day()),
            (Some(2010), Some(5), Some(7))
        );
    }

    #[test]
    fn a_repeated_tag_replaces_the_value_and_keeps_its_place() {
        let r = vec![
            1, b'A', 1, b'x', 1, b'B', 1, b'y', 1, b'A', 1, b'z', 0, 0, 0x0F,
        ];
        let game = RawGame::parse(&r).unwrap();
        assert_eq!(
            game.extra_tags,
            vec![
                (b"A".to_vec(), b"z".to_vec()),
                (b"B".to_vec(), b"y".to_vec())
            ]
        );
    }

    #[test]
    fn tag_errors() {
        // 251..=254 are unassigned.
        for code in 251u8..=254 {
            assert!(
                RawGame::parse(&[code, 1, b'x', 0, 0, 0x0F]).is_err(),
                "code {code}"
            );
        }
        // Truncated everywhere.
        assert!(RawGame::parse(&[]).is_err());
        assert!(RawGame::parse(&[4, b'N']).is_err());
        assert!(RawGame::parse(&[1, b'N']).is_err());
        assert!(RawGame::parse(&[1, b'N', 2, b'x']).is_err());
        assert!(RawGame::parse(&[255, 1, 2]).is_err());
        assert!(RawGame::parse(&[0]).is_err(), "flags missing");
    }

    #[test]
    fn rejects_more_than_255_distinct_tags() {
        let build = |n: usize| {
            let mut r = Vec::new();
            for i in 0..n {
                let name = format!("T{i:03}");
                r.push(u8::try_from(name.len()).unwrap());
                r.extend_from_slice(name.as_bytes());
                r.extend_from_slice(&[0]);
            }
            r.extend_from_slice(&[0, 0, 0x0F]);
            r
        };
        assert_eq!(RawGame::parse(&build(255)).unwrap().extra_tags.len(), 255);
        assert!(RawGame::parse(&build(256)).is_err());
    }

    #[test]
    fn parses_a_start_position_with_the_king_after_other_pieces() {
        let mut r = vec![0u8, 1];
        r.extend_from_slice(b"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\0");
        // White list: [E1, A1]. Rook move a1-a5: piece 1, v = 8 + 4.
        r.extend_from_slice(&[0x1C, 0x0F]);
        let game = RawGame::parse(&r).unwrap();
        assert_eq!(game.mainline()[0].mv.from, Square::A1);
        assert_eq!(game.mainline()[0].mv.to, Square::A5);
        assert!(game.start_fen.is_some());
    }

    #[test]
    fn invalid_fens_are_errors() {
        for fen in [
            "4k1k1/8/8/8/8/8/8/4K3 w - - 0 1",
            "4k3/8/8/8/8/8/8/P3K3 w - - 0 1",
            "4k3/8/8/8/8/8/8/4K3 q - - 0 1",
            "garbage",
        ] {
            let mut r = vec![0u8, 1];
            r.extend_from_slice(fen.as_bytes());
            r.extend_from_slice(&[0, 0x0F]);
            assert!(RawGame::parse(&r).is_err(), "{fen}");
        }
        // Unterminated FEN.
        let mut r = vec![0u8, 1];
        r.extend_from_slice(b"4k3/8/8/8/8/8/8/4K3 w");
        assert!(RawGame::parse(&r).is_err());
    }

    #[test]
    fn variations_nags_and_comments_in_pre_order() {
        // 1.e4 (1.d4 d5) e5 {c}: stream = e4 START d4 d5 END e5 NAG(1) COMMENT END_GAME
        // d2-d4 is the pawn at list index 11 with v = 15; black d7-d5 is 0xBF.
        let stream = [
            0xCF, // e4
            TOKEN_COMMENT,
            TOKEN_START,
            TOKEN_COMMENT, // start comment of the variation
            0xBF,          // d4
            0xBF,          // d5
            TOKEN_END,
            0xCF, // e5
            TOKEN_NAG,
            1,
            TOKEN_COMMENT,
            TOKEN_END_GAME,
        ];
        let mut r = record(&stream);
        r.extend_from_slice(b"after e4\0var start\0after e5\0");
        let game = RawGame::parse(&r).unwrap();
        assert_eq!(game.lines.len(), 2);
        let main = game.mainline();
        assert_eq!(main.len(), 2);
        assert_eq!(main[0].comment.as_deref(), Some(&b"after e4"[..]));
        assert_eq!(main[0].variations, vec![1]);
        assert_eq!(main[1].comment.as_deref(), Some(&b"after e5"[..]));
        assert_eq!(main[1].nags, vec![Nag::new(1).unwrap()]);
        let var = &game.lines[1];
        assert_eq!(var.start_comment.as_deref(), Some(&b"var start"[..]));
        assert_eq!(var.moves.len(), 2);
        assert_eq!(var.moves[0].mv.from, Square::D2);
        assert_eq!(var.moves[1].mv.from, Square::D7);
    }

    #[test]
    fn game_start_comment_nag_zero_and_extra_nags() {
        let mut stream = vec![TOKEN_COMMENT, 0xCF, TOKEN_NAG, 0];
        for n in 1..=9u8 {
            stream.extend_from_slice(&[TOKEN_NAG, n]);
        }
        stream.push(TOKEN_END_GAME);
        let mut r = record(&stream);
        r.extend_from_slice(b"intro\0");
        let game = RawGame::parse(&r).unwrap();
        assert_eq!(game.lines[0].start_comment.as_deref(), Some(&b"intro"[..]));
        let nags: Vec<u8> = game.mainline()[0].nags.iter().map(|n| n.value()).collect();
        assert_eq!(
            nags,
            vec![1, 2, 3, 4, 5, 6, 7],
            "NAG 0 ignored, more than 7 dropped"
        );
    }

    #[test]
    fn variations_after_captures_restore_and_replay_the_piece_lists() {
        // 1.e4 d5 2.exd5 (2.Nc3) Qxd5 (2...Nf6) 3.h3
        // After exd5 the black list swaps its last piece (h7) into index 11;
        // after Qxd5 the white list swaps h2 into index 12. Each variation
        // takes the capture back and the main line replays it, so the later
        // bytes only decode to the right pieces if the lists are exact.
        let stream = [
            0xCF, // e4
            0xBF, // d5
            0xC0, // exd5 (+7)
            TOKEN_START,
            0x28, // Nc3 (b1 knight, +17)
            TOKEN_END,
            0x4C, // Qxd5 (queen index 4, file d, rank 5)
            TOKEN_START,
            0x61, // Nf6 (g8 knight index 6, -17)
            TOKEN_END,
            0xC1, // h3 (index 12 now holds the h2 pawn)
            0xBF, // ...h5 (index 11 holds the h7 pawn)
            TOKEN_END_GAME,
        ];
        let game = RawGame::parse(&record(&stream)).unwrap();
        assert_eq!(game.lines.len(), 3);
        assert_eq!(game.lines[1].moves[0].mv.to, Square::C3);
        assert_eq!(game.lines[2].moves[0].mv.from, Square::G8);
        let main = game.mainline();
        assert_eq!(main.len(), 6);
        assert_eq!((main[3].mv.from, main[3].mv.to), (Square::D8, Square::D5));
        assert_eq!((main[4].mv.from, main[4].mv.to), (Square::H2, Square::H3));
        assert_eq!((main[5].mv.from, main[5].mv.to), (Square::H7, Square::H5));
    }

    #[test]
    fn nested_and_multiple_variations() {
        // 1.e4 (1.d4 (1.c4) (1.b4)) (1.a4) e5
        let c4 = 0xAF; // pawn index 10 (c2), v = 15
        let b4 = 0x9F;
        let a4 = 0x8F;
        let d4 = 0xBF;
        let stream = [
            0xCF,
            TOKEN_START,
            d4,
            TOKEN_START,
            c4,
            TOKEN_END,
            TOKEN_START,
            b4,
            TOKEN_END,
            TOKEN_END,
            TOKEN_START,
            a4,
            TOKEN_END,
            0xCF,
            TOKEN_END_GAME,
        ];
        let game = RawGame::parse(&record(&stream)).unwrap();
        assert_eq!(game.lines.len(), 5);
        let main = game.mainline();
        assert_eq!(main[0].variations, vec![1, 4]);
        assert_eq!(game.lines[1].moves[0].variations, vec![2, 3]);
        assert_eq!(main.len(), 2);
    }

    #[test]
    fn structural_errors() {
        // Missing end of game.
        assert!(RawGame::parse(&record(&[0xCF])).is_err());
        // End marker at level 0.
        assert!(RawGame::parse(&record(&[0xCF, TOKEN_END, TOKEN_END_GAME])).is_err());
        // Start marker before the first move.
        assert!(RawGame::parse(&record(&[TOKEN_START, 0xCF, TOKEN_END, TOKEN_END_GAME])).is_err());
        // End of game inside a variation.
        assert!(RawGame::parse(&record(&[0xCF, TOKEN_START, 0xBF, TOKEN_END_GAME])).is_err());
        // Variation never closed.
        assert!(RawGame::parse(&record(&[0xCF, TOKEN_START, 0xBF])).is_err());
        // Piece number beyond the list (only two pieces on the board).
        let mut r = vec![0u8, 1];
        r.extend_from_slice(b"4k3/8/8/8/8/8/8/4K3 w - - 0 1\0");
        r.extend_from_slice(&[0x10, TOKEN_END_GAME]);
        assert!(RawGame::parse(&r).is_err());
        // NAG without a value.
        assert!(RawGame::parse(&record(&[0xCF, TOKEN_NAG])).is_err());
        // Destination off the board: the king on e1 with delta -9.
        assert!(RawGame::parse(&record(&[0x01, TOKEN_END_GAME])).is_err());
    }

    #[test]
    fn comment_count_must_match_the_strings() {
        let stream = [0xCF, TOKEN_COMMENT, TOKEN_END_GAME];
        // No string at all.
        assert!(RawGame::parse(&record(&stream)).is_err());
        // String without a NUL.
        let mut r = record(&stream);
        r.extend_from_slice(b"abc");
        assert!(RawGame::parse(&r).is_err());
        // Bytes after the last comment are tolerated, as Scid does.
        let mut r = record(&stream);
        r.extend_from_slice(b"a\0b\0");
        assert!(RawGame::parse(&r).is_ok());
        // Exactly one.
        let mut r = record(&stream);
        r.extend_from_slice(b"a\0");
        assert!(RawGame::parse(&r).is_ok());
        // An empty comment is a comment.
        let mut r = record(&stream);
        r.extend_from_slice(b"\0");
        let game = RawGame::parse(&r).unwrap();
        assert_eq!(game.mainline()[0].comment.as_deref(), Some(&b""[..]));
    }

    #[test]
    fn deep_variations_do_not_overflow_the_stack() {
        // Each level: a move, then a variation that repeats the pattern.
        // Level moves alternate colours, so use null moves (king, 0x00),
        // which are legal for the codec and keep the piece lists intact.
        let depth = 9_000;
        let mut stream = vec![0x00u8];
        for _ in 0..depth {
            stream.extend_from_slice(&[TOKEN_START, 0x00]);
        }
        stream.extend(std::iter::repeat_n(TOKEN_END, depth));
        stream.push(TOKEN_END_GAME);
        let game = RawGame::parse(&record(&stream)).unwrap();
        assert_eq!(game.lines.len(), depth + 1);
        drop(game);
        // Beyond the limit: an error, not a crash.
        let mut stream = vec![0x00u8];
        for _ in 0..=MAX_VARIATION_DEPTH + 1 {
            stream.extend_from_slice(&[TOKEN_START, 0x00]);
        }
        assert!(RawGame::parse(&record(&stream)).is_err());
    }

    #[test]
    fn mainline_skips_variations_nags_and_comments() {
        let stream = [
            0xCF,
            TOKEN_NAG,
            TOKEN_START, // NAG value 13 must not open a variation
            TOKEN_COMMENT,
            TOKEN_START,
            0xBF,
            TOKEN_START,
            0xAF,
            TOKEN_END,
            TOKEN_END,
            0xCF,
            TOKEN_END_GAME,
        ];
        let moves = mainline_of_record(&record(&stream)).unwrap();
        assert_eq!(moves.len(), 2);
        assert_eq!(moves[0].to, Square::E4);
        assert_eq!(moves[1].to, Square::E5);
        // Same record parsed fully gives the same main line (comments aside).
        assert!(mainline_of_record(&record(&[0xCF])).is_err());
        assert!(mainline_of_record(&record(&[0xCF, TOKEN_END])).is_err());
        assert!(mainline_of_record(&record(&[0xCF, TOKEN_START, 0xBF, TOKEN_END_GAME])).is_err());
    }

    #[test]
    fn walk_mainline_stops_before_damage_after_the_break() {
        // 1.e4 then a truncated variation: a full walk fails, a walk that
        // breaks at ply 1 never reads it.
        let rec = record(&[0xCF, 0xCF, TOKEN_START, 0xAF]);
        assert!(mainline_of_record(&rec).is_err());
        let mut plies = 0;
        let found = walk_mainline(&rec, |_, next| {
            if plies == 1 {
                return ControlFlow::Break(next.map(|m| m.to));
            }
            plies += 1;
            ControlFlow::Continue(())
        })
        .unwrap();
        assert_eq!(found, Some(Some(Square::E5)));
        // Breaking at the first position needs only the first move.
        let first = walk_mainline(&rec, |_, next| ControlFlow::Break(next.is_some())).unwrap();
        assert_eq!(first, Some(true));
    }

    #[test]
    fn random_records_never_panic() {
        // A small deterministic generator (no dependency) biased towards the
        // special tokens, to reach the variation and comment paths.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let len = usize::try_from(next() % 40).unwrap();
            let mut data = vec![0u8, 0u8];
            for _ in 0..len {
                let r = next();
                data.push(match r % 4 {
                    0 => u8::try_from(11 + (r >> 8) % 5).unwrap(),
                    1 => u8::try_from((r >> 8) % 256).unwrap(),
                    _ => 0xC0 | u8::try_from((r >> 8) % 16).unwrap(),
                });
            }
            drop(RawGame::parse(&data));
            drop(mainline_of_record(&data));
        }
    }
}
