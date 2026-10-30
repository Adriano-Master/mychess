// Derived from Scid vs PC 4.27, src/tree.cpp (`TreeCache::WriteFile`,
// `ReadFile`), src/movelist.cpp (`writeSimpleMove`, `readSimpleMove`) and
// src/filter.cpp (`CompressedFilter`). Copyright (c) 1999-2002 Shane Hudson;
// Scid vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The layout of the `.stc` file (decisions D1042 to D1044). All numbers are
//! big endian. The full description, with the Scid's source references, the
//! `bytemap` algorithm and the fields the Scid really uses, is in
//! `docs/formatos/stc.md`.
//!
//! ```text
//! header (22 bytes)   magic "stc\x18", version u16 (400), CacheSize u32,
//!                     NumInUse u32, LowestTotal u32, LowestTotalIndex u32
//! NumInUse entries    toMove u8 (0 White, 1 Black), 64 squares u8 (A1..H8;
//!                     7 empty, white K1 Q2 R3 B4 N5 P6, black +8),
//!                     moveCount u32 (0..=60), totalCount u32,
//!                     moveCount nodes of 71 bytes,
//!                     CFilterSize u32, CFilterCount u32, CompressedLength u32,
//!                     CompressedLength bytes (see `pack_bytemap`)
//! node (71 bytes)     pieceNum, movingPiece, from (65 = end of game), to,
//!                     capturedNum, capturedPiece (7 = none), promote
//!                     (7 = none), 0, capturedSquare, castleFlags, epSquare,
//!                     oldHalfMoveClock u16, san[8], freq[4] u32 (index 0 =
//!                     no result, 1 = White, 2 = Black, 3 = draw), total,
//!                     score, ecoCode u16, eloCount, eloSum, perfCount,
//!                     perfSum, yearCount, yearSum (u32 each)
//! ```

use std::io::Read;
use std::path::{Path, PathBuf};

use mychess_core::{Color, Position, Role, Square};

use super::{StcError, bytemap};
use crate::filter::Filter;
use crate::si4::{MAX_GAMES, RawMove};
use crate::tree::{MAX_TREE_NODES, TreeNode};

/// The four bytes at the start of the file (`TREEFILE_MAGIC`).
pub const MAGIC: [u8; 4] = [b's', b't', b'c', 24];
/// The version this implementation writes (`SCID_VERSION`). The version of a
/// file being read is not checked.
pub const FILE_VERSION: u16 = 400;
/// Size of the header.
pub const HEADER_LEN: usize = 22;
/// Size of a node.
pub const NODE_LEN: usize = 71;
/// The square value of the "no square" (`NULL_SQUARE`): the `from` of the
/// `[end]` node.
pub const NULL_SQUARE: u8 = 65;
/// The piece value of "no piece" (`EMPTY`), for empty squares, no capture and
/// no promotion.
pub const NO_PIECE: u8 = 7;
/// The largest valid board byte (black pawn).
const MAX_PIECE_CODE: u8 = 14;

/// The header of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StcHeader {
    /// The version written by the Scid (400); not validated on reading.
    pub version: u16,
    /// Capacity of the cache that wrote the file.
    pub cache_size: u32,
    /// Number of entries in the file.
    pub num_in_use: u32,
    /// The lowest `totalCount` among the entries.
    pub lowest_total: u32,
    /// Index of the entry with the lowest total (the next one evicted).
    pub lowest_total_index: u32,
}

impl StcHeader {
    /// The 22 bytes of the header.
    pub fn to_bytes(self) -> [u8; HEADER_LEN] {
        let mut out = Vec::with_capacity(HEADER_LEN);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&self.version.to_be_bytes());
        for value in [
            self.cache_size,
            self.num_in_use,
            self.lowest_total,
            self.lowest_total_index,
        ] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        let mut bytes = [0u8; HEADER_LEN];
        for (dst, src) in bytes.iter_mut().zip(out) {
            *dst = src;
        }
        bytes
    }

    /// Reads a header; only the magic is validated.
    ///
    /// # Errors
    ///
    /// [`StcError::Corrupt`] for a short buffer or a wrong magic.
    pub fn from_bytes(bytes: &[u8]) -> Result<StcHeader, StcError> {
        let mut cur = Cursor::new(bytes);
        if cur.take(4)? != MAGIC {
            return Err(StcError::corrupt("bad magic"));
        }
        Ok(StcHeader {
            version: cur.u16()?,
            cache_size: cur.u32()?,
            num_in_use: cur.u32()?,
            lowest_total: cur.u32()?,
            lowest_total_index: cur.u32()?,
        })
    }
}

/// The fields of a node that the tree engine does not keep (the rest of the
/// Scid's `simpleMoveT`, plus `san`, `score` and `ecoCode`). They are ignored
/// on reading and written on a best-effort basis (decision D1041).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeExtras {
    /// Index of the moving piece in the piece list of the side to move.
    pub piece_num: u8,
    /// The moving piece (with the color bit).
    pub moving_piece: u8,
    /// Index of the captured piece in the list of the opponent.
    pub captured_num: u8,
    /// The captured piece, [`NO_PIECE`] if none.
    pub captured_piece: u8,
    /// Where the captured piece stood (differs from `to` in en passant).
    pub captured_square: u8,
    /// Castling flags before the move: white queen side 1, king side 2,
    /// black queen side 4, king side 8.
    pub castle_flags: u8,
    /// En passant target before the move, [`NULL_SQUARE`] if none.
    pub ep_square: u8,
    /// Half move clock before the move.
    pub old_half_move_clock: u16,
    /// The move in SAN (`[end]` for the end of the game); at most 7 bytes
    /// are written (the field is a C string of 8 bytes, decision D1063).
    pub san: String,
    /// Score for White in per mille.
    pub score: u32,
    /// The ECO code in the Scid's 16-bit form, 0 for none.
    pub eco_code: u16,
}

impl Default for NodeExtras {
    fn default() -> NodeExtras {
        NodeExtras {
            piece_num: 0,
            moving_piece: 0,
            captured_num: 0,
            captured_piece: NO_PIECE,
            captured_square: 0,
            castle_flags: 0,
            ep_square: NULL_SQUARE,
            old_half_move_clock: 0,
            san: String::new(),
            score: 0,
            eco_code: 0,
        }
    }
}

/// The code of a role in the Scid's piece numbering (`KING` = 1 to `PAWN` = 6).
pub fn role_code(role: Role) -> u8 {
    match role {
        Role::King => 1,
        Role::Queen => 2,
        Role::Rook => 3,
        Role::Bishop => 4,
        Role::Knight => 5,
        Role::Pawn => 6,
    }
}

fn role_from_code(code: u8) -> Option<Role> {
    Some(match code {
        1 => Role::King,
        2 => Role::Queen,
        3 => Role::Rook,
        4 => Role::Bishop,
        5 => Role::Knight,
        6 => Role::Pawn,
        _ => return None,
    })
}

/// The square number of the file format (A1 = 0 to H8 = 63).
pub fn square_code(sq: Square) -> u8 {
    // A square is always in 0..64.
    u8::try_from(sq.to_u32()).unwrap_or(0)
}

fn square_from_code(code: u8) -> Option<Square> {
    Square::try_from(u32::from(code)).ok()
}

/// The 64 board bytes of a position: the key of a cache entry (side to move
/// aside). Castling rights and en passant are not part of it (D1045).
pub fn board_key(pos: &Position) -> [u8; 64] {
    let mut board = [NO_PIECE; 64];
    for (sq, piece) in pos.pieces() {
        let code = role_code(piece.role) + if piece.color == Color::Black { 8 } else { 0 };
        if let Some(cell) = board.get_mut(usize::from(square_code(sq))) {
            *cell = code;
        }
    }
    board
}

/// The low 32 bits of a sum (the Scid keeps `unsigned int` sums).
fn low32(value: u64) -> u32 {
    u32::try_from(value & 0xFFFF_FFFF).unwrap_or(0)
}

/// Whether the three sums of a node fit the 32 bits of the file. A node that
/// does not would be written truncated (what the Scid does), so the writer
/// leaves the entry out instead (decision D1064).
pub fn sums_fit(node: &TreeNode) -> bool {
    [node.elo_sum, node.perf_sum, node.year_sum]
        .iter()
        .all(|&sum| u32::try_from(sum).is_ok())
}

/// Appends the 71 bytes of a node to `out`. `freq` goes out in the Scid's
/// order (no result, White, Black, draw; decision D1042) and the sums are
/// truncated to 32 bits (see [`sums_fit`]).
pub fn encode_node(node: &TreeNode, extras: &NodeExtras, out: &mut Vec<u8>) {
    let start = out.len();
    let (from, to, promote) = match node.mv {
        Some(mv) => (
            square_code(mv.from),
            square_code(mv.to),
            mv.promotion.map_or(NO_PIECE, role_code),
        ),
        None => (NULL_SQUARE, NULL_SQUARE, NO_PIECE),
    };
    out.extend_from_slice(&[
        extras.piece_num,
        extras.moving_piece,
        from,
        to,
        extras.captured_num,
        extras.captured_piece,
        promote,
        0, // obsolete, unused value
        extras.captured_square,
        extras.castle_flags,
        extras.ep_square,
    ]);
    out.extend_from_slice(&extras.old_half_move_clock.to_be_bytes());
    // The Scid reads `san[8]` as a C string: at most 7 bytes and a final 0.
    let mut san = [0u8; 8];
    for (dst, src) in san.iter_mut().take(7).zip(extras.san.bytes()) {
        *dst = src;
    }
    out.extend_from_slice(&san);
    let [white, black, draw, none] = node.freq;
    for value in [none, white, black, draw, node.total, extras.score] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out.extend_from_slice(&extras.eco_code.to_be_bytes());
    for value in [
        node.elo_count,
        low32(node.elo_sum),
        node.perf_count,
        low32(node.perf_sum),
        node.year_count,
        low32(node.year_sum),
    ] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    debug_assert_eq!(out.len() - start, NODE_LEN);
}

/// Reads the 71 bytes of a node. The fields the engine does not keep are
/// skipped; `from == 65` is the end of the game (`mv` is `None`).
///
/// # Errors
///
/// [`StcError::Corrupt`] for a wrong length, a square above 63 or a
/// promotion that is not a queen, rook, bishop or knight.
pub fn decode_node(bytes: &[u8]) -> Result<TreeNode, StcError> {
    if bytes.len() != NODE_LEN {
        return Err(StcError::corrupt("node has the wrong length"));
    }
    let mut cur = Cursor::new(bytes);
    cur.take(2)?; // pieceNum, movingPiece
    let from = cur.u8()?;
    let to = cur.u8()?;
    cur.take(2)?; // capturedNum, capturedPiece
    let promote = cur.u8()?;
    cur.take(6)?; // reserved, capturedSquare, castleFlags, epSquare, clock
    cur.take(8)?; // san
    let mv = if from == NULL_SQUARE {
        None
    } else {
        let (Some(from), Some(to)) = (square_from_code(from), square_from_code(to)) else {
            return Err(StcError::corrupt("node square out of range"));
        };
        let promotion = match promote {
            NO_PIECE => None,
            2..=5 => role_from_code(promote),
            _ => return Err(StcError::corrupt("node promotion out of range")),
        };
        Some(RawMove {
            from,
            to,
            promotion,
        })
    };
    let [none, white, black, draw] = [cur.u32()?, cur.u32()?, cur.u32()?, cur.u32()?];
    let total = cur.u32()?;
    cur.take(4)?; // score
    cur.take(2)?; // ecoCode
    let elo_count = cur.u32()?;
    let elo_sum = cur.u32()?;
    let perf_count = cur.u32()?;
    let perf_sum = cur.u32()?;
    let year_count = cur.u32()?;
    let year_sum = cur.u32()?;
    Ok(TreeNode {
        mv,
        total,
        freq: [white, black, draw, none],
        elo_sum: u64::from(elo_sum),
        elo_count,
        perf_sum: u64::from(perf_sum),
        perf_count,
        year_sum: u64::from(year_sum),
        year_count,
    })
}

/// A filter in its compressed form (`CompressedFilter`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedFilter {
    /// Number of games the filter covers (`CFilterSize`).
    pub size: u32,
    /// Number of games in the filter (`CFilterCount`).
    pub count: u32,
    /// The bytes made by [`pack_bytemap`](super::pack_bytemap).
    pub data: Vec<u8>,
}

impl CompressedFilter {
    /// Compresses `filter`.
    pub fn compress(filter: &Filter) -> CompressedFilter {
        CompressedFilter {
            size: filter.len(),
            count: filter.count(),
            data: super::pack_bytemap(filter.as_bytes()),
        }
    }

    /// Rebuilds the filter.
    ///
    /// # Errors
    ///
    /// [`StcError::Corrupt`] if the data does not unpack to `size` bytes.
    pub fn uncompress(&self) -> Result<Filter, StcError> {
        let size = usize::try_from(self.size).map_err(|_| StcError::corrupt("filter too large"))?;
        let bytes = super::unpack_bytemap(&self.data, size)?;
        let mut filter = Filter::new(self.size);
        filter.fill(0);
        for (i, &value) in bytes.iter().enumerate() {
            if value != 0 {
                let game =
                    u32::try_from(i + 1).map_err(|_| StcError::corrupt("filter too large"))?;
                filter.set(game, value);
            }
        }
        Ok(filter)
    }
}

/// One cached position: the key (side to move and board), the counters of its
/// moves and the compressed tree filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StcEntry {
    /// The side to move.
    pub to_move: Color,
    /// The 64 board bytes, see [`board_key`].
    pub board: [u8; 64],
    /// `totalCount`: the number of games in all the nodes.
    pub total_count: u32,
    /// The nodes (at most 60).
    pub nodes: Vec<TreeNode>,
    /// The tree filter.
    pub filter: CompressedFilter,
}

impl StcEntry {
    /// The exact number of bytes [`StcEntry::encode`] appends.
    pub fn encoded_len(&self) -> u64 {
        // toMove, board, moveCount, totalCount, then the filter header.
        let fixed: u64 = 1 + 64 + 4 + 4 + 12;
        let nodes = u64::try_from(self.nodes.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(NODE_LEN as u64);
        let data = u64::try_from(self.filter.data.len()).unwrap_or(u64::MAX);
        fixed.saturating_add(nodes).saturating_add(data)
    }

    /// Whether every node sum fits 32 bits, see [`sums_fit`].
    pub fn sums_fit(&self) -> bool {
        self.nodes.iter().all(sums_fit)
    }

    /// Appends the entry to `out`; `extras` has one item per node (missing
    /// ones are written as defaults).
    ///
    /// # Errors
    ///
    /// [`StcError::Corrupt`] if the entry cannot be represented: more than
    /// 60 nodes or a compressed filter longer than 4 GiB. Nothing is
    /// appended in that case.
    pub fn encode(&self, extras: &[NodeExtras], out: &mut Vec<u8>) -> Result<(), StcError> {
        let count = u32::try_from(self.nodes.len())
            .ok()
            .filter(|&n| usize::try_from(n).is_ok_and(|n| n <= MAX_TREE_NODES))
            .ok_or_else(|| StcError::corrupt("more than 60 moves in a tree"))?;
        let len = u32::try_from(self.filter.data.len())
            .map_err(|_| StcError::corrupt("compressed filter too long"))?;
        out.push(u8::from(self.to_move == Color::Black));
        out.extend_from_slice(&self.board);
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&self.total_count.to_be_bytes());
        let fallback = NodeExtras::default();
        for (i, node) in self.nodes.iter().enumerate() {
            encode_node(node, extras.get(i).unwrap_or(&fallback), out);
        }
        for value in [self.filter.size, self.filter.count] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&self.filter.data);
        Ok(())
    }
}

/// A reader with a byte budget (the size of the file): nothing larger than
/// what is left of the file is ever allocated or read.
pub(super) struct Reader<R> {
    inner: R,
    left: u64,
    path: PathBuf,
}

impl<R: Read> Reader<R> {
    pub(super) fn new(inner: R, len: u64, path: &Path) -> Reader<R> {
        Reader {
            inner,
            left: len,
            path: path.to_path_buf(),
        }
    }

    fn fill(&mut self, buf: &mut [u8]) -> Result<(), StcError> {
        let n = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if n > self.left {
            return Err(StcError::corrupt("file ends early"));
        }
        self.inner.read_exact(buf).map_err(|source| {
            if source.kind() == std::io::ErrorKind::UnexpectedEof {
                StcError::corrupt("file ends early")
            } else {
                StcError::Io {
                    path: self.path.clone(),
                    source,
                }
            }
        })?;
        self.left -= n;
        Ok(())
    }

    pub(super) fn array<const N: usize>(&mut self) -> Result<[u8; N], StcError> {
        let mut buf = [0u8; N];
        self.fill(&mut buf)?;
        Ok(buf)
    }

    pub(super) fn u8(&mut self) -> Result<u8, StcError> {
        Ok(self.array::<1>()?[0])
    }

    pub(super) fn u32(&mut self) -> Result<u32, StcError> {
        Ok(u32::from_be_bytes(self.array::<4>()?))
    }

    /// `n` bytes; fails before allocating if the file does not have them.
    pub(super) fn vec(&mut self, n: u32) -> Result<Vec<u8>, StcError> {
        if u64::from(n) > self.left {
            return Err(StcError::corrupt("file ends early"));
        }
        let len = usize::try_from(n).map_err(|_| StcError::corrupt("block too large"))?;
        let mut buf = vec![0u8; len];
        self.fill(&mut buf)?;
        Ok(buf)
    }

    /// Reads the header.
    pub(super) fn header(&mut self) -> Result<StcHeader, StcError> {
        StcHeader::from_bytes(&self.array::<HEADER_LEN>()?)
    }

    /// Reads one entry and validates it (decision D1048).
    pub(super) fn entry(&mut self) -> Result<StcEntry, StcError> {
        let to_move = match self.u8()? {
            0 => Color::White,
            1 => Color::Black,
            _ => return Err(StcError::corrupt("side to move out of range")),
        };
        let board = self.array::<64>()?;
        if board.iter().any(|&b| b > MAX_PIECE_CODE) {
            return Err(StcError::corrupt("board square out of range"));
        }
        let move_count = self.u32()?;
        if usize::try_from(move_count).map_or(true, |n| n > MAX_TREE_NODES) {
            return Err(StcError::corrupt("more than 60 moves in a tree"));
        }
        let total_count = self.u32()?;
        let mut nodes = Vec::new();
        for _ in 0..move_count {
            nodes.push(decode_node(&self.array::<NODE_LEN>()?)?);
        }
        let size = self.u32()?;
        let count = self.u32()?;
        let length = self.u32()?;
        if size > MAX_GAMES || count > size {
            return Err(StcError::corrupt("filter size out of range"));
        }
        let data = self.vec(length)?;
        let out_len = usize::try_from(size).map_err(|_| StcError::corrupt("filter too large"))?;
        bytemap::check_bytemap(&data, out_len)?;
        Ok(StcEntry {
            to_move,
            board,
            total_count,
            nodes,
            filter: CompressedFilter { size, count, data },
        })
    }
}

/// A cursor over a byte slice.
struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Cursor<'a> {
        Cursor { data, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], StcError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| StcError::corrupt("buffer ends early"))?;
        let slice = self
            .data
            .get(self.at..end)
            .ok_or_else(|| StcError::corrupt("buffer ends early"))?;
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, StcError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or_else(|| StcError::corrupt("buffer ends early"))
    }

    fn u16(&mut self) -> Result<u16, StcError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([
            b.first().copied().unwrap_or(0),
            b.get(1).copied().unwrap_or(0),
        ]))
    }

    fn u32(&mut self) -> Result<u32, StcError> {
        let b = self.take(4)?;
        let mut a = [0u8; 4];
        for (dst, src) in a.iter_mut().zip(b) {
            *dst = *src;
        }
        Ok(u32::from_be_bytes(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_node() -> TreeNode {
        TreeNode {
            mv: Some(RawMove {
                from: Square::E2,
                to: Square::E4,
                promotion: None,
            }),
            total: 10,
            freq: [4, 3, 2, 1],
            elo_sum: 0x1_0000_0005,
            elo_count: 6,
            perf_sum: 7,
            perf_count: 8,
            year_sum: 9,
            year_count: 10,
        }
    }

    #[test]
    fn header_golden_bytes() {
        let header = StcHeader {
            version: FILE_VERSION,
            cache_size: 1000,
            num_in_use: 2,
            lowest_total: 7,
            lowest_total_index: 1,
        };
        let bytes = header.to_bytes();
        assert_eq!(
            bytes,
            [
                0x73, 0x74, 0x63, 0x18, // "stc" 24
                0x01, 0x90, // 400
                0, 0, 0x03, 0xE8, // 1000
                0, 0, 0, 2, // 2
                0, 0, 0, 7, // 7
                0, 0, 0, 1, // 1
            ]
        );
        assert_eq!(StcHeader::from_bytes(&bytes).ok(), Some(header));
        let mut bad = bytes;
        bad[3] = 25;
        assert!(StcHeader::from_bytes(&bad).is_err());
        assert!(StcHeader::from_bytes(&bytes[..21]).is_err());
    }

    #[test]
    fn node_golden_bytes_and_result_order() {
        let extras = NodeExtras {
            san: "e4".to_owned(),
            score: 600,
            eco_code: 0x0102,
            ..NodeExtras::default()
        };
        let mut out = Vec::new();
        encode_node(&sample_node(), &extras, &mut out);
        assert_eq!(out.len(), NODE_LEN);
        #[rustfmt::skip]
        let expected: [u8; NODE_LEN] = [
            0, 0, 12, 28, 0, 7, 7, 0, 0, 0, 65, 0, 0, // 13-byte move
            b'e', b'4', 0, 0, 0, 0, 0, 0, // san
            0, 0, 0, 1, // freq[0] = no result (TreeNode freq[3])
            0, 0, 0, 4, // freq[1] = White
            0, 0, 0, 3, // freq[2] = Black
            0, 0, 0, 2, // freq[3] = draw
            0, 0, 0, 10, // total
            0, 0, 0x02, 0x58, // score 600
            0x01, 0x02, // eco
            0, 0, 0, 6, 0, 0, 0, 5, // elo count, sum (low 32 bits)
            0, 0, 0, 8, 0, 0, 0, 7, // perf
            0, 0, 0, 10, 0, 0, 0, 9, // year
        ];
        assert_eq!(out, expected);
        let back = decode_node(&out).ok();
        let mut want = sample_node();
        want.elo_sum = 5; // truncated to 32 bits
        assert_eq!(back, Some(want));
    }

    #[test]
    fn san_is_cut_to_seven_bytes() {
        let extras = NodeExtras {
            san: "Qa1xb2+??".to_owned(),
            ..NodeExtras::default()
        };
        let mut out = Vec::new();
        encode_node(&sample_node(), &extras, &mut out);
        assert_eq!(out.get(13..21), Some(&b"Qa1xb2+\0"[..]));
    }

    #[test]
    fn end_node_has_from_65_and_decodes_to_none() {
        let mut node = sample_node();
        node.mv = None;
        let mut out = Vec::new();
        encode_node(&node, &NodeExtras::default(), &mut out);
        assert_eq!(out.get(2..4), Some(&[65u8, 65][..]));
        assert_eq!(decode_node(&out).map(|n| n.mv).ok(), Some(None));
    }

    #[test]
    fn promotion_round_trips_and_bad_fields_are_rejected() {
        let mut node = sample_node();
        node.mv = Some(RawMove {
            from: Square::E7,
            to: Square::E8,
            promotion: Some(Role::Knight),
        });
        let mut out = Vec::new();
        encode_node(&node, &NodeExtras::default(), &mut out);
        assert_eq!(out.get(6), Some(&5));
        assert_eq!(decode_node(&out).map(|n| n.mv).ok(), Some(node.mv));
        out[6] = 6; // pawn promotion
        assert!(decode_node(&out).is_err());
        out[6] = 7;
        out[3] = 64; // square out of range
        assert!(decode_node(&out).is_err());
        assert!(decode_node(&out[..70]).is_err());
    }

    #[test]
    fn entry_round_trips_through_the_reader() {
        let filter = {
            let mut f = Filter::new(30);
            f.fill(0);
            f.set(3, 5);
            f
        };
        let entry = StcEntry {
            to_move: Color::Black,
            board: board_key(&Position::new()),
            total_count: 10,
            nodes: vec![sample_node()],
            filter: CompressedFilter::compress(&filter),
        };
        let mut bytes = Vec::new();
        entry.encode(&[], &mut bytes).expect("encodes");
        let mut reader = Reader::new(bytes.as_slice(), bytes.len() as u64, Path::new("x"));
        let mut back = reader.entry().expect("entry reads back");
        // The sums are truncated to 32 bits on the way.
        back.nodes[0].elo_sum = 0x1_0000_0005;
        assert_eq!(back, entry);
        assert_eq!(entry.filter.uncompress().ok(), Some(filter));
        assert_eq!(entry.encoded_len(), bytes.len() as u64);
        // The sample node has an Elo sum over 32 bits.
        assert!(!entry.sums_fit());
        // Truncated at every position: never a panic, always an error.
        for cut in 0..bytes.len() {
            let mut r = Reader::new(&bytes[..cut], cut as u64, Path::new("x"));
            assert!(r.entry().is_err(), "cut at {cut}");
        }
    }

    #[test]
    fn board_key_of_the_start_position() {
        let key = board_key(&Position::new());
        assert_eq!(&key[..8], &[3, 5, 4, 2, 1, 4, 5, 3]);
        assert_eq!(&key[8..16], &[6; 8]);
        assert_eq!(&key[16..48], &[7; 32]);
        assert_eq!(&key[48..56], &[14; 8]);
        assert_eq!(&key[56..], &[11, 13, 12, 10, 9, 12, 13, 11]);
    }
}
