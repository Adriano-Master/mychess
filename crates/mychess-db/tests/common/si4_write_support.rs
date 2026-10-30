//! Helpers of the `si4_write_*` contract tests (spec M3-2a). Includes an
//! independent `.sn4` decoder written from `docs/formatos/si4.md` section 7,
//! so the writer is not checked against its own reader.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::cast_possible_truncation
)]

use std::path::{Path, PathBuf};

use mychess_core::{Game, GameResult, NodeId};

pub const HEADER_LEN: usize = 182;
pub const ENTRY_LEN: usize = 47;
pub const BLOCK: usize = 131_072;

pub fn tmp() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("games");
    (dir, base)
}

pub fn ext(base: &Path, e: &str) -> PathBuf {
    base.with_extension(e)
}

pub fn play(game: &mut Game, parent: NodeId, uci: &str) -> NodeId {
    let mv = game.position_at(parent).unwrap().parse_uci(uci).unwrap();
    game.add_move(parent, mv).unwrap()
}

pub fn game_ucis(ucis: &[&str]) -> Game {
    let mut game = Game::new();
    let mut at = game.root();
    for u in ucis {
        at = play(&mut game, at, u);
    }
    game
}

pub fn named(white: &str, black: &str, event: &str, site: &str, round: &str) -> Game {
    let mut game = game_ucis(&["e2e4", "e7e5"]);
    let t = game.tags_mut();
    t.white = white.into();
    t.black = black.into();
    t.event = event.into();
    t.site = site.into();
    t.round = round.into();
    t.result = GameResult::Draw;
    game
}

/// A game of `plies` knight shuffles (one byte per move in the record).
pub fn shuffle(plies: usize) -> Game {
    const CYCLE: [&str; 4] = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let mut game = Game::new();
    let mut at = game.root();
    for i in 0..plies {
        at = play(&mut game, at, CYCLE[i % 4]);
    }
    game
}

/// A game without moves whose record has exactly `len` bytes (`len >= 5`):
/// `00 flags 0C 0F <comment> 00`.
pub fn game_of_record_len(len: usize) -> Game {
    let mut game = Game::new();
    game.set_initial_comment(&"a".repeat(len - 5));
    game
}

pub fn read(base: &Path, e: &str) -> Vec<u8> {
    std::fs::read(ext(base, e)).unwrap()
}

pub fn entry_bytes(base: &Path, n: u32) -> Vec<u8> {
    let si4 = read(base, "si4");
    let at = HEADER_LEN + ENTRY_LEN * (n as usize - 1);
    si4[at..at + ENTRY_LEN].to_vec()
}

pub fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0u64, |a, &b| (a << 8) | u64::from(b))
}

/// One decoded `.sn4` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameRec {
    pub id: u64,
    pub freq: u64,
    pub name: Vec<u8>,
}

#[derive(Debug)]
pub struct Sn4 {
    pub time_stamp: u32,
    pub num_names: [u64; 4],
    pub max_freq: [u64; 4],
    /// Records of each section, in file order.
    pub sections: [Vec<NameRec>; 4],
    pub prefixes: [Vec<u8>; 4],
}

/// Decodes a `.sn4` following the widths of the specification only.
pub fn parse_sn4(data: &[u8]) -> Sn4 {
    assert_eq!(&data[..8], b"Scid.sn\0");
    let time_stamp = be(&data[8..12]) as u32;
    let mut num_names = [0u64; 4];
    let mut max_freq = [0u64; 4];
    for i in 0..4 {
        num_names[i] = be(&data[12 + 3 * i..15 + 3 * i]);
        max_freq[i] = be(&data[24 + 3 * i..27 + 3 * i]);
    }
    let mut at = 36usize;
    let mut sections: [Vec<NameRec>; 4] = Default::default();
    let mut prefixes: [Vec<u8>; 4] = Default::default();
    for t in 0..4 {
        let id_w = if num_names[t] >= 65_536 { 3 } else { 2 };
        let fq_w = if max_freq[t] >= 65_536 {
            3
        } else if max_freq[t] >= 256 {
            2
        } else {
            1
        };
        let mut prev: Vec<u8> = Vec::new();
        for k in 0..num_names[t] {
            let id = be(&data[at..at + id_w]);
            at += id_w;
            let freq = be(&data[at..at + fq_w]);
            at += fq_w;
            let len = data[at] as usize;
            at += 1;
            let prefix = if k > 0 {
                let p = data[at] as usize;
                at += 1;
                prefixes[t].push(p as u8);
                p
            } else {
                0
            };
            assert!(prefix <= len && prefix <= prev.len(), "bad prefix");
            let mut name = prev[..prefix].to_vec();
            name.extend_from_slice(&data[at..at + len - prefix]);
            at += len - prefix;
            prev.clone_from(&name);
            sections[t].push(NameRec { id, freq, name });
        }
    }
    assert_eq!(at, data.len(), "trailing or missing bytes in .sn4");
    Sn4 {
        time_stamp,
        num_names,
        max_freq,
        sections,
        prefixes,
    }
}

impl Sn4 {
    pub fn name(&self, kind: usize, id: u64) -> Option<&NameRec> {
        self.sections[kind].iter().find(|r| r.id == id)
    }
    pub fn by_name(&self, kind: usize, name: &[u8]) -> Option<&NameRec> {
        self.sections[kind].iter().find(|r| r.name == name)
    }
}

/// Scid's order, written independently: first byte unsigned, rest as signed
/// chars with the NUL terminator.
pub fn scid_key(name: &[u8]) -> (u8, Vec<i8>) {
    let first = name.first().copied().unwrap_or(0);
    let mut rest: Vec<i8> = name.iter().map(|&b| b.cast_signed()).collect();
    rest.push(0);
    (first, rest)
}

/// Directory entries named like the database, to detect leftovers.
pub fn leftovers(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| {
            Path::new(n)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("tmp") || e.eq_ignore_ascii_case("txn"))
        })
        .collect();
    v.sort();
    v
}

/// Offset of the entry of game `n` (base 1) inside the entries area.
pub fn ent_off(n: u32) -> usize {
    ENTRY_LEN * (n as usize - 1)
}
