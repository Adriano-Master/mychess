//! Builder of synthetic SI4 v400 databases, written from `docs/formatos/si4.md`
//! (not from the reader): every offset, width and mask below comes from the
//! format document. Included with `#[path]` by the integration tests.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::missing_panics_doc,
    clippy::cast_possible_truncation
)]

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use mychess_core::{Game, Move, Position, SanMode};
use mychess_db::si4::{Si4Database, Si4Error};

pub const HEADER_LEN: usize = 182;
pub const ENTRY_LEN: usize = 47;
pub const BLOCK: usize = 131_072;

// ---------------------------------------------------------------- .si4 header

/// The 182-byte header (section 2).
#[derive(Clone)]
pub struct IndexHeader {
    pub magic: [u8; 8],
    pub version: u16,
    pub base_type: u32,
    /// Declared `numGames`; `None` = the real number of entries.
    pub num_games: Option<u32>,
    pub auto_load: u32,
    pub description: Vec<u8>,
    pub custom_flags: [Vec<u8>; 6],
}

impl Default for IndexHeader {
    fn default() -> Self {
        IndexHeader {
            magic: *b"Scid.si\0",
            version: 400,
            base_type: 0,
            num_games: None,
            auto_load: 2,
            description: Vec::new(),
            custom_flags: Default::default(),
        }
    }
}

impl IndexHeader {
    pub fn encode(&self, real_games: usize) -> Vec<u8> {
        let mut h = vec![0u8; HEADER_LEN];
        h[..8].copy_from_slice(&self.magic);
        h[8..10].copy_from_slice(&self.version.to_be_bytes());
        h[10..14].copy_from_slice(&self.base_type.to_be_bytes());
        let n = self.num_games.unwrap_or(real_games as u32);
        h[14..17].copy_from_slice(&n.to_be_bytes()[1..]);
        h[17..20].copy_from_slice(&self.auto_load.to_be_bytes()[1..]);
        let d = &self.description;
        let take = d.len().min(107);
        h[20..20 + take].copy_from_slice(&d[..take]);
        for (i, text) in self.custom_flags.iter().enumerate() {
            let at = 128 + 9 * i;
            let take = text.len().min(8);
            h[at..at + take].copy_from_slice(&text[..take]);
        }
        h
    }
}

/// `.si4` file: header and the already encoded 47-byte entries.
pub fn index_file(header: &IndexHeader, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut out = header.encode(entries.len());
    for e in entries {
        out.extend_from_slice(e);
    }
    out
}

// ---------------------------------------------------------------- .si4 entry

/// The fields of a 47-byte entry (section 3), in their natural sizes.
#[derive(Clone, Debug)]
pub struct EntrySpec {
    pub offset: u32,
    /// 17-bit record length.
    pub length: u32,
    /// Bit 6 of `Length_High`.
    pub spare: bool,
    /// Custom flags 1-6 (bits 0-5).
    pub custom_flags: u8,
    /// The 16 `Flags` bits.
    pub flags: u16,
    pub white: u32,
    pub black: u32,
    pub event: u32,
    pub site: u32,
    pub round: u32,
    pub variations: u8,
    pub comments: u8,
    pub nags: u8,
    /// 4 bits (0-3 valid).
    pub result: u8,
    pub eco: u16,
    /// 20 bits, `year << 9 | month << 5 | day`.
    pub game_date: u32,
    /// 12 bits: `off << 9 | month << 5 | day`.
    pub event_date: u32,
    pub white_elo: u16,
    pub white_type: u8,
    pub black_elo: u16,
    pub black_type: u8,
    /// 24 bits.
    pub final_mat: u32,
    pub stored_line: u8,
    /// 10 bits.
    pub half_moves: u16,
    /// Byte 38 low 6 bits and bytes 39..47.
    pub home_pawn: [u8; 9],
}

impl EntrySpec {
    pub fn new(offset: u32, length: u32) -> EntrySpec {
        EntrySpec {
            offset,
            length,
            spare: false,
            custom_flags: 0,
            flags: 0,
            white: 0,
            black: 1,
            event: 0,
            site: 0,
            round: 0,
            variations: 0,
            comments: 0,
            nags: 0,
            result: 0,
            eco: 0,
            game_date: (1999 << 9) | (12 << 5) | 31,
            event_date: 0,
            white_elo: 2700,
            white_type: 0,
            black_elo: 2650,
            black_type: 0,
            final_mat: 0,
            stored_line: 0,
            half_moves: 0,
            home_pawn: [0; 9],
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut e = vec![0u8; ENTRY_LEN];
        e[0..4].copy_from_slice(&self.offset.to_be_bytes());
        e[4..6].copy_from_slice(&((self.length & 0xFFFF) as u16).to_be_bytes());
        e[6] = (((self.length >> 16) & 1) as u8) << 7
            | u8::from(self.spare) << 6
            | (self.custom_flags & 0x3F);
        e[7..9].copy_from_slice(&self.flags.to_be_bytes());
        e[9] = (((self.white >> 16) & 0xF) as u8) << 4 | ((self.black >> 16) & 0xF) as u8;
        e[10..12].copy_from_slice(&((self.white & 0xFFFF) as u16).to_be_bytes());
        e[12..14].copy_from_slice(&((self.black & 0xFFFF) as u16).to_be_bytes());
        e[14] = (((self.event >> 16) & 7) as u8) << 5
            | (((self.site >> 16) & 7) as u8) << 2
            | ((self.round >> 16) & 3) as u8;
        e[15..17].copy_from_slice(&((self.event & 0xFFFF) as u16).to_be_bytes());
        e[17..19].copy_from_slice(&((self.site & 0xFFFF) as u16).to_be_bytes());
        e[19..21].copy_from_slice(&((self.round & 0xFFFF) as u16).to_be_bytes());
        let counts = u16::from(self.variations & 15)
            | u16::from(self.comments & 15) << 4
            | u16::from(self.nags & 15) << 8
            | u16::from(self.result & 15) << 12;
        e[21..23].copy_from_slice(&counts.to_be_bytes());
        e[23..25].copy_from_slice(&self.eco.to_be_bytes());
        let dates = (self.game_date & 0xF_FFFF) | ((self.event_date & 0xFFF) << 20);
        e[25..29].copy_from_slice(&dates.to_be_bytes());
        let w = (self.white_elo & 0xFFF) | (u16::from(self.white_type & 15) << 12);
        let b = (self.black_elo & 0xFFF) | (u16::from(self.black_type & 15) << 12);
        e[29..31].copy_from_slice(&w.to_be_bytes());
        e[31..33].copy_from_slice(&b.to_be_bytes());
        let fm = (self.final_mat & 0xFF_FFFF) | (u32::from(self.stored_line) << 24);
        e[33..37].copy_from_slice(&fm.to_be_bytes());
        e[37] = (self.half_moves & 0xFF) as u8;
        e[38] = (self.home_pawn[0] & 63) | (((self.half_moves >> 8) & 3) as u8) << 6;
        e[39..47].copy_from_slice(&self.home_pawn[1..9]);
        e
    }
}

// ---------------------------------------------------------------- .sn4

/// One name record: `(id, frequency, name)`, in file order.
pub type NameRec = (u32, u32, Vec<u8>);

/// Builds a `.sn4` with the widths of section 7. `declared_counts` replaces the
/// `numNames` header fields (for hostile files); `max_freq` is the declared
/// `maxFrequency` of each kind.
pub fn names_file_with(
    sections: &[Vec<NameRec>; 4],
    max_freq: [u32; 4],
    declared_counts: Option<[u32; 4]>,
) -> Vec<u8> {
    let counts = declared_counts.unwrap_or([
        sections[0].len() as u32,
        sections[1].len() as u32,
        sections[2].len() as u32,
        sections[3].len() as u32,
    ]);
    let mut out = b"Scid.sn\0".to_vec();
    out.extend_from_slice(&1234u32.to_be_bytes());
    for c in counts {
        out.extend_from_slice(&c.to_be_bytes()[1..]);
    }
    for m in max_freq {
        out.extend_from_slice(&m.to_be_bytes()[1..]);
    }
    for (k, section) in sections.iter().enumerate() {
        let id_width = if counts[k] >= 65_536 { 3 } else { 2 };
        let freq_width = if max_freq[k] >= 65_536 {
            3
        } else if max_freq[k] >= 256 {
            2
        } else {
            1
        };
        let mut prev: &[u8] = &[];
        for (i, (id, freq, name)) in section.iter().enumerate() {
            out.extend_from_slice(&id.to_be_bytes()[4 - id_width..]);
            out.extend_from_slice(&freq.to_be_bytes()[4 - freq_width..]);
            out.push(name.len() as u8);
            let mut prefix = 0;
            if i > 0 {
                prefix = prev.iter().zip(name).take_while(|(a, b)| a == b).count();
                out.push(prefix as u8);
            }
            out.extend_from_slice(&name[prefix..]);
            prev = name;
        }
    }
    out
}

/// A section whose IDs equal the positions, frequency 1.
pub fn section(names: &[&str]) -> Vec<NameRec> {
    names
        .iter()
        .enumerate()
        .map(|(i, n)| (i as u32, 1, n.as_bytes().to_vec()))
        .collect()
}

/// Standard names: players Anand (0) and Carlsen (1); event Wijk; site
/// Wijk aan Zee; round 1.
pub fn default_names() -> Vec<u8> {
    names_file_with(
        &[
            section(&["Anand", "Carlsen"]),
            section(&["Wijk"]),
            section(&["Wijk aan Zee"]),
            section(&["1"]),
        ],
        [1; 4],
        None,
    )
}

// ---------------------------------------------------------------- .sg4 record

/// A record with no extra tags, flags 0, the given move stream (which must
/// contain its own `0x0F`) and comment strings.
pub fn rec(stream: &[u8], comments: &[&[u8]]) -> Vec<u8> {
    rec_full(&[], 0, None, stream, comments)
}

/// A record starting from `fen` (flags bit 0).
pub fn rec_fen(fen: &str, stream: &[u8], comments: &[&[u8]]) -> Vec<u8> {
    rec_full(&[], 1, Some(fen.as_bytes()), stream, comments)
}

/// Full control: encoded tag items, flags byte, optional FEN, stream, comments.
pub fn rec_full(
    tags: &[u8],
    flags: u8,
    fen: Option<&[u8]>,
    stream: &[u8],
    comments: &[&[u8]],
) -> Vec<u8> {
    let mut r = tags.to_vec();
    r.push(0);
    r.push(flags);
    if let Some(f) = fen {
        r.extend_from_slice(f);
        r.push(0);
    }
    r.extend_from_slice(stream);
    for c in comments {
        r.extend_from_slice(c);
        r.push(0);
    }
    r
}

/// Tag item with a literal name (1..=240 bytes).
pub fn tag_literal(name: &[u8], value: &[u8]) -> Vec<u8> {
    let mut t = vec![name.len() as u8];
    t.extend_from_slice(name);
    t.push(value.len() as u8);
    t.extend_from_slice(value);
    t
}

/// Tag item with a code of 241..=254.
pub fn tag_code(code: u8, value: &[u8]) -> Vec<u8> {
    let mut t = vec![code, value.len() as u8];
    t.extend_from_slice(value);
    t
}

/// Tag item 255: event date in 3 bytes.
pub fn tag_event_date(bits: u32) -> Vec<u8> {
    let mut t = vec![255];
    t.extend_from_slice(&bits.to_be_bytes()[1..]);
    t
}

// ---------------------------------------------------------------- assembling

/// The three files of a database as bytes.
#[derive(Clone)]
pub struct Parts {
    pub index: Vec<u8>,
    pub names: Vec<u8>,
    pub sg4: Vec<u8>,
}

/// Packs `records` one after the other (no block rule applied: callers keep
/// them small) and builds default entries for them.
pub fn parts_from_records(records: &[Vec<u8>]) -> Parts {
    let mut sg4 = Vec::new();
    let mut entries = Vec::new();
    for r in records {
        entries.push(EntrySpec::new(sg4.len() as u32, r.len() as u32).encode());
        sg4.extend_from_slice(r);
    }
    Parts {
        index: index_file(&IndexHeader::default(), &entries),
        names: default_names(),
        sg4,
    }
}

pub type MemDb = Si4Database<Cursor<Vec<u8>>>;

pub fn open_parts(p: &Parts) -> Result<MemDb, Si4Error> {
    Si4Database::from_parts(
        p.index.clone(),
        &p.names,
        Cursor::new(p.sg4.clone()),
        PathBuf::from("mem.sg4"),
    )
}

/// An in-memory database of `records`.
pub fn mem_db(records: &[Vec<u8>]) -> MemDb {
    open_parts(&parts_from_records(records)).expect("synthetic database opens")
}

/// One record in an otherwise default database.
pub fn one(record: Vec<u8>) -> MemDb {
    mem_db(&[record])
}

/// Writes the three files of `p` into `dir` as `stem` + the given extensions
/// (`[si4, sn4, sg4]`, any case) and returns the stem path.
pub fn write_files(dir: &Path, stem: &str, exts: [&str; 3], p: &Parts) -> PathBuf {
    fs::write(dir.join(format!("{stem}.{}", exts[0])), &p.index).unwrap();
    fs::write(dir.join(format!("{stem}.{}", exts[1])), &p.names).unwrap();
    fs::write(dir.join(format!("{stem}.{}", exts[2])), &p.sg4).unwrap();
    dir.join(stem)
}

// ---------------------------------------------------------------- oracle

/// Plays `sans` (strict SAN of the core) from `fen` (or the standard start)
/// and returns the moves and the final FEN: an oracle independent of the
/// codec for the legality-checked tests.
pub fn san_line(fen: Option<&str>, sans: &[&str]) -> (Vec<Move>, String) {
    let mut pos = match fen {
        Some(f) => Position::from_fen(f).expect("valid fen"),
        None => Position::new(),
    };
    let mut moves = Vec::new();
    for s in sans {
        let mv = pos
            .parse_san(s, SanMode::Strict)
            .unwrap_or_else(|e| panic!("{s}: {e:?}"))
            .mv;
        pos.play(mv).expect("legal");
        moves.push(mv);
    }
    (moves, pos.fen())
}

/// The moves of the main line of `game`.
pub fn mainline_moves(game: &Game) -> Vec<Move> {
    game.mainline().map(|id| game.mv(id).unwrap()).collect()
}

/// FEN after the last main-line move.
pub fn final_fen(game: &Game) -> String {
    match game.mainline().last() {
        Some(id) => game.position_at(id).unwrap().fen(),
        None => game.start_position().fen(),
    }
}

/// Move byte `(piece_index << 4) | v`.
pub const fn mb(piece: u8, v: u8) -> u8 {
    (piece << 4) | v
}

/// A record long enough to need the 17th length bit: knight shuffles
/// (`Ng1-f3 Ng8-f6 Nf3-g1 Nf6-g8`) filling exactly `total` bytes.
pub fn long_record(total: usize) -> Vec<u8> {
    let stream_len = total - 3; // two prelude bytes and the end token
    assert_eq!(stream_len % 4, 0);
    let mut stream = Vec::with_capacity(stream_len + 1);
    for _ in 0..stream_len / 4 {
        stream.extend_from_slice(&[mb(6, 7), mb(6, 1), mb(6, 2), mb(6, 8)]);
    }
    stream.push(15);
    let r = rec(&stream, &[]);
    assert_eq!(r.len(), total);
    r
}
