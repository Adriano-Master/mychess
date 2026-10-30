//! [`Si4Database`]: a read-only SI4 database (`.si4` + `.sn4` + `.sg4`).

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use mychess_core::Game;

use super::convert::{ConversionReport, build_game};
use super::entry::IndexEntry;
use super::error::{IndexEntryError, RecordError, Result, Si4Error, Si4File};
use super::gfile::GameFile;
use super::header::{ENTRY_LEN, HEADER_LEN, Header};
use super::names::NameBase;
use super::raw::{RawGame, RawMove};
use super::record::mainline_of_record;

/// A database opened for reading.
///
/// The index (47 bytes per game) and the name base are kept in memory; the
/// `.sg4` file is read one record at a time. Nothing here writes to disk.
///
/// ```no_run
/// use mychess_db::si4::Si4Database;
///
/// let mut db = Si4Database::open("games/tactics".as_ref())?;
/// for n in 1..=db.len() {
///     let game = db.game(n)?;
///     println!("{} - {}", game.tags().white, game.tags().black);
/// }
/// # Ok::<(), mychess_db::si4::Si4Error>(())
/// ```
#[derive(Debug)]
pub struct Si4Database<R = File> {
    header: Header,
    /// The whole `.si4` file: header followed by the 47-byte entries.
    index: Vec<u8>,
    names: NameBase,
    games: GameFile<R>,
}

impl Si4Database<File> {
    /// Opens a database. `base` may be the common base name (`games/tactics`)
    /// or the path of any of the three files (`tactics.si4`, `.sn4`, `.sg4`);
    /// extensions are matched without regard to case. The files are opened
    /// read-only.
    ///
    /// Validation order: the `.si4` header and its size, the `.sn4` file and
    /// every index entry (decision D262). Records of the `.sg4` file are
    /// checked when each game is read.
    pub fn open(base: &Path) -> Result<Si4Database<File>> {
        let stem = strip_known_extension(base);
        let index_path = find_file(&stem, "si4");
        let names_path = find_file(&stem, "sn4");
        let games_path = find_file(&stem, "sg4");
        // An unfinished transaction may have left the files inconsistent; only
        // a writer finishes it (decision D311).
        if let Some(path) = super::journal::pending_journal(&stem) {
            return Err(Si4Error::PendingJournal { path });
        }
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| Si4Error::Io { path, source }
        };
        // Read the header first, then only the `numGames` entries it declares
        // (bytes after them are tolerated and never read).
        let mut index_file = File::open(&index_path).map_err(io(&index_path))?;
        let mut index = Vec::new();
        (&mut index_file)
            .take(HEADER_LEN as u64)
            .read_to_end(&mut index)
            .map_err(io(&index_path))?;
        let header = Header::parse(&index)?;
        let expected = HEADER_LEN as u64 + ENTRY_LEN as u64 * u64::from(header.num_games());
        index_file
            .take(expected.saturating_sub(HEADER_LEN as u64))
            .read_to_end(&mut index)
            .map_err(io(&index_path))?;
        // The index is checked before the other files are touched.
        check_index_size(&header, index.len())?;
        let names = fs::read(&names_path).map_err(io(&names_path))?;
        let games = File::open(&games_path).map_err(io(&games_path))?;
        Si4Database::from_parts(index, &names, games, games_path)
    }
}

impl<R: Read + Seek> Si4Database<R> {
    /// Builds a database from the contents of the `.si4` and `.sn4` files and
    /// a reader over the `.sg4` file. `games_path` only labels I/O errors.
    /// Used by [`Si4Database::open`] and handy for in-memory databases.
    pub fn from_parts(
        index: Vec<u8>,
        names: &[u8],
        games: R,
        games_path: PathBuf,
    ) -> Result<Si4Database<R>> {
        let header = Header::parse(&index)?;
        check_index_size(&header, index.len())?;
        let names = NameBase::parse(names)?;
        let db = Si4Database {
            header,
            index,
            names,
            games: GameFile::new(games, games_path)?,
        };
        // Only the name IDs are verified here; a game with an invalid result
        // fails when it is read (decision D262).
        for n in 1..=db.len() {
            let corrupt = |e: IndexEntryError| Si4Error::CorruptIndex {
                game: n,
                reason: e.reason(),
            };
            let (entry, _) = IndexEntry::parse_lenient(db.entry_bytes(n)?).map_err(corrupt)?;
            entry.verify_names(&db.names).map_err(corrupt)?;
        }
        Ok(db)
    }

    /// The `.si4` header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The number of games.
    pub fn len(&self) -> u32 {
        self.header.num_games()
    }

    /// Whether the database has no games.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The name base.
    pub fn names(&self) -> &NameBase {
        &self.names
    }

    /// The size in bytes of the `.sg4` file.
    pub fn games_file_len(&self) -> u64 {
        self.games.len()
    }

    /// The index entry of game `n` (`1..=len()`).
    pub fn entry(&self, n: u32) -> Result<IndexEntry> {
        let bytes = self.entry_bytes(n)?;
        IndexEntry::parse(bytes).map_err(|e| Si4Error::CorruptIndex {
            game: n,
            reason: e.reason(),
        })
    }

    /// All index entries, in order.
    pub fn entries(&self) -> impl Iterator<Item = Result<IndexEntry>> + '_ {
        (1..=self.len()).map(|n| self.entry(n))
    }

    fn entry_bytes(&self, n: u32) -> Result<&[u8]> {
        let invalid = Si4Error::InvalidGameNumber {
            number: n,
            len: self.len(),
        };
        if n == 0 || n > self.len() {
            return Err(invalid);
        }
        let start = usize::try_from(n - 1)
            .ok()
            .and_then(|i| i.checked_mul(ENTRY_LEN))
            .and_then(|i| i.checked_add(HEADER_LEN))
            .ok_or(Si4Error::CorruptIndex {
                game: n,
                reason: "entry offset overflows",
            })?;
        self.index
            .get(start..start + ENTRY_LEN)
            .ok_or(Si4Error::Truncated {
                file: Si4File::Index,
                offset: start as u64,
                need: ENTRY_LEN as u64,
            })
    }

    /// The bytes of the `.sg4` record of game `n`, as stored.
    pub fn record_bytes(&mut self, n: u32) -> Result<&[u8]> {
        let entry = self.entry(n)?;
        self.games.read_record(n, entry.offset, entry.length)
    }

    /// The decoded record of game `n`: extra tags, start position, the tree
    /// of moves with variations, NAGs and comments, all as raw bytes and
    /// squares, without checking that the moves are legal.
    pub fn raw_game(&mut self, n: u32) -> Result<RawGame> {
        let entry = self.entry(n)?;
        self.raw_game_of(n, &entry)
    }

    /// Parses the record of game `n` given its already decoded entry.
    fn raw_game_of(&mut self, n: u32, entry: &IndexEntry) -> Result<RawGame> {
        let record = self.games.read_record(n, entry.offset, entry.length)?;
        RawGame::parse(record).map_err(|e| corrupt_record(n, entry, e))
    }

    /// Game `n` as a [`Game`] of the core: tags from the index and the name
    /// base plus the extra tags, start position, moves checked for legality,
    /// variations, NAGs and comments.
    ///
    /// Errors in one game do not affect the others.
    pub fn game(&mut self, n: u32) -> Result<Game> {
        self.game_with_report(n).map(|(game, _)| game)
    }

    /// Like [`Si4Database::game`], also telling how many variations that
    /// repeat a move had to be merged or dropped (see [`ConversionReport`]).
    pub fn game_with_report(&mut self, n: u32) -> Result<(Game, ConversionReport)> {
        let entry = self.entry(n)?;
        let raw = self.raw_game_of(n, &entry)?;
        build_game(n, &entry, &self.names, &raw)
    }

    /// The main line of game `n` as raw moves, without building a game: tags,
    /// variations, NAGs and comments are skipped and legality is not checked.
    pub fn decode_mainline(&mut self, n: u32) -> Result<Vec<RawMove>> {
        let entry = self.entry(n)?;
        let record = self.games.read_record(n, entry.offset, entry.length)?;
        mainline_of_record(record).map_err(|e| corrupt_record(n, &entry, e))
    }
}

fn corrupt_record(game: u32, entry: &IndexEntry, error: RecordError) -> Si4Error {
    Si4Error::CorruptRecord {
        game,
        offset: u64::from(entry.offset) + error.at() as u64,
        reason: error.reason(),
    }
}

pub(crate) fn check_index_size(header: &Header, len: usize) -> Result<()> {
    let expected = HEADER_LEN as u64 + ENTRY_LEN as u64 * u64::from(header.num_games());
    if (len as u64) < expected {
        return Err(Si4Error::Truncated {
            file: Si4File::Index,
            offset: len as u64,
            need: expected - len as u64,
        });
    }
    Ok(())
}

/// Removes a `.si4`, `.sn4` or `.sg4` extension (any case); other paths are
/// kept whole, so a base name may contain dots.
pub(crate) fn strip_known_extension(path: &Path) -> PathBuf {
    let known = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["si4", "sn4", "sg4"]
            .iter()
            .any(|k| e.eq_ignore_ascii_case(k))
    });
    if known {
        path.with_extension("")
    } else {
        path.to_path_buf()
    }
}

/// `stem` + `.` + `ext`, or an existing file in the same directory whose name
/// matches it without regard to case (the first in name order). When nothing
/// matches, the exact name is returned so opening it reports the error.
pub(crate) fn find_file(stem: &Path, ext: &str) -> PathBuf {
    let mut exact: OsString = stem.as_os_str().to_owned();
    exact.push(".");
    exact.push(ext);
    let exact = PathBuf::from(exact);
    if exact.exists() {
        return exact;
    }
    let (Some(parent), Some(wanted)) = (
        exact.parent().map(|p| {
            if p.as_os_str().is_empty() {
                Path::new(".")
            } else {
                p
            }
        }),
        exact.file_name().and_then(|n| n.to_str()),
    ) else {
        return exact;
    };
    let mut matches: Vec<PathBuf> = fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(wanted))
        })
        .map(|e| e.path())
        .collect();
    matches.sort();
    matches.into_iter().next().unwrap_or(exact)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use mychess_core::GameResult;

    use super::*;

    /// Names for one section: single-byte ids in the given order.
    fn name_section(names: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut prev: &[u8] = &[];
        for (i, name) in names.iter().enumerate() {
            let bytes = name.as_bytes();
            out.extend_from_slice(&u16::try_from(i).unwrap().to_be_bytes());
            out.push(1); // frequency
            out.push(u8::try_from(bytes.len()).unwrap());
            let mut prefix = 0;
            if i > 0 {
                prefix = prev.iter().zip(bytes).take_while(|(a, b)| a == b).count();
                out.push(u8::try_from(prefix).unwrap());
            }
            out.extend_from_slice(&bytes[prefix..]);
            prev = bytes;
        }
        out
    }

    fn names_file(players: &[&str], events: &[&str], sites: &[&str], rounds: &[&str]) -> Vec<u8> {
        let sections = [players, events, sites, rounds];
        let mut out = b"Scid.sn\0".to_vec();
        out.extend_from_slice(&0u32.to_be_bytes());
        for s in sections {
            out.extend_from_slice(&u32::try_from(s.len()).unwrap().to_be_bytes()[1..]);
        }
        for _ in 0..4 {
            out.extend_from_slice(&[0, 0, 1]);
        }
        for s in sections {
            out.extend_from_slice(&name_section(s));
        }
        out
    }

    fn entry_bytes(offset: u32, length: u32, white: u16, black: u16, result: u16) -> Vec<u8> {
        let mut e = vec![0u8; ENTRY_LEN];
        e[0..4].copy_from_slice(&offset.to_be_bytes());
        e[4..6].copy_from_slice(&u16::try_from(length & 0xFFFF).unwrap().to_be_bytes());
        e[10..12].copy_from_slice(&white.to_be_bytes());
        e[12..14].copy_from_slice(&black.to_be_bytes());
        e[21..23].copy_from_slice(&(result << 12).to_be_bytes());
        let date = (1999u32 << 9) | (12 << 5) | 31;
        e[25..29].copy_from_slice(&date.to_be_bytes());
        e[29..31].copy_from_slice(&2700u16.to_be_bytes());
        e
    }

    fn index_file(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![0u8; HEADER_LEN];
        out[..8].copy_from_slice(b"Scid.si\0");
        out[8..10].copy_from_slice(&400u16.to_be_bytes());
        out[14..17].copy_from_slice(&u32::try_from(entries.len()).unwrap().to_be_bytes()[1..]);
        out[17..20].copy_from_slice(&[0, 0, 2]);
        for e in entries {
            out.extend_from_slice(e);
        }
        out
    }

    /// Record: tags (a literal `Note`=`x` and `Annotator`), standard start,
    /// 1.e4 e5 with a comment after e4.
    fn record_one() -> Vec<u8> {
        let mut r = vec![4];
        r.extend_from_slice(b"Note");
        r.extend_from_slice(&[1, b'x', 243, 2, b'A', b'B', 0, 0]);
        r.extend_from_slice(&[0xCF, 12, 0xCF, 15]);
        r.extend_from_slice(b"good\0");
        r
    }

    /// Record: own start position, a variation and a NAG.
    fn record_two(stream: &[u8]) -> Vec<u8> {
        let mut r = vec![0, 1];
        r.extend_from_slice(b"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\0");
        r.extend_from_slice(stream);
        r
    }

    fn database(records: &[Vec<u8>]) -> Si4Database<Cursor<Vec<u8>>> {
        database_with(records, |_, _| {})
    }

    /// Like `database`, letting `patch` change each raw index entry.
    fn database_with(
        records: &[Vec<u8>],
        patch: impl Fn(usize, &mut Vec<u8>),
    ) -> Si4Database<Cursor<Vec<u8>>> {
        let mut sg4 = Vec::new();
        let mut entries = Vec::new();
        for (i, r) in records.iter().enumerate() {
            let white = u16::try_from(i % 2).unwrap();
            entries.push(entry_bytes(
                u32::try_from(sg4.len()).unwrap(),
                u32::try_from(r.len()).unwrap(),
                white,
                1 - white,
                u16::try_from(i % 4).unwrap(),
            ));
            patch(i, entries.last_mut().unwrap());
            sg4.extend_from_slice(r);
        }
        let names = names_file(&["Anand", "Carlsen"], &["Wijk"], &["Wijk aan Zee"], &["1"]);
        Si4Database::from_parts(
            index_file(&entries),
            &names,
            Cursor::new(sg4),
            PathBuf::from("mem.sg4"),
        )
        .unwrap()
    }

    #[test]
    fn reads_tags_moves_and_comments_of_a_game() {
        let mut db = database(&[record_one()]);
        assert_eq!((db.len(), db.is_empty()), (1, false));
        assert_eq!(db.header().auto_load(), Some(1));
        let game = db.game(1).unwrap();
        let tags = game.tags();
        assert_eq!(
            (tags.white.as_str(), tags.black.as_str()),
            ("Anand", "Carlsen")
        );
        assert_eq!(tags.event, "Wijk");
        assert_eq!(tags.site, "Wijk aan Zee");
        assert_eq!(tags.round, "1");
        assert_eq!(tags.date.to_string(), "1999.12.31");
        assert_eq!(tags.result, GameResult::Unknown);
        assert_eq!(tags.white_rating.map(|r| r.value.get()), Some(2700));
        assert_eq!(tags.get_extra("Note"), Some("x"));
        assert_eq!(tags.get_extra("Annotator"), Some("AB"));
        let ids: Vec<_> = game.mainline().collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(game.comment(ids[0]).unwrap(), Some("good"));
        assert_eq!(
            game.position_at(ids[1]).unwrap().fen(),
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2"
        );
        let moves = db.decode_mainline(1).unwrap();
        assert_eq!(moves.len(), 2);
        let raw = db.raw_game(1).unwrap();
        assert_eq!(raw.extra_tags.len(), 2);
        assert!(db.record_bytes(1).unwrap().ends_with(b"good\0"));
    }

    #[test]
    fn builds_variations_nags_and_start_comments_from_a_fen_game() {
        // 1.Ra5 $1 (1.{first} Ra3 Ke7) Kd7 2.Ra7+
        // White list [E1, A1]: rook a1-a5 is 0x1C (piece 1, v = 8 + 4) and
        // a1-a3 is 0x1A; black list [E8]: Ke7 is -8 (v = 2), Kd7 is -9 (v = 1);
        // Ra5-a7 is 0x1E.
        let stream = [0x1C, 11, 1, 13, 12, 0x1A, 0x02, 14, 0x01, 0x1E, 15];
        let mut rec = record_two(&stream);
        rec.extend_from_slice(b"first\0");
        let mut db = database(&[rec]);
        let game = db.game(1).unwrap();
        assert_eq!(
            game.start_position().fen(),
            "4k3/8/8/8/8/8/8/R3K3 w - - 0 1"
        );
        let children = game.children(game.root()).unwrap().to_vec();
        assert_eq!(children.len(), 2, "main move and one variation");
        assert_eq!(game.nags(children[0]).unwrap().len(), 1);
        assert_eq!(game.comment_before(children[1]).unwrap(), Some("first"));
        assert_eq!(game.comment(children[1]).unwrap(), None);
        assert_eq!(game.mainline().count(), 3);
        assert_eq!(game.variations(children[1]).unwrap().len(), 0);
    }

    #[test]
    fn a_variation_repeating_a_move_with_a_continuation_is_merged() {
        // 1.Ra5 (1.Ra5 Ke7) Kd7: the variation joins the main Ra5, and Ke7
        // becomes a variation of Kd7.
        let stream = [0x1C, 13, 0x1C, 0x02, 14, 0x01, 15];
        let mut db = database(&[record_two(&stream)]);
        assert_eq!(db.raw_game(1).unwrap().lines.len(), 2);
        let (game, report) = db.game_with_report(1).unwrap();
        assert_eq!(
            report,
            ConversionReport {
                merged_variations: 1,
                discarded_variations: 0
            }
        );
        assert_eq!(game.mainline().count(), 2);
        assert_eq!(game.node_count(), 4);
        let ra5 = game.children(game.root()).unwrap()[0];
        assert_eq!(game.children(ra5).unwrap().len(), 2);
    }

    #[test]
    fn sister_variations_with_the_same_first_move_share_it() {
        // 1.Ra3 (1.Ra5 Kd7) (1.Ra5 Ke7) Kd7
        let stream = [0x1A, 13, 0x1C, 0x01, 14, 13, 0x1C, 0x02, 14, 0x01, 15];
        let mut db = database(&[record_two(&stream)]);
        let (game, report) = db.game_with_report(1).unwrap();
        assert_eq!(report.merged_variations, 1);
        assert_eq!(report.discarded_variations, 0);
        assert_eq!(game.children(game.root()).unwrap().len(), 2);
        assert_eq!(game.node_count(), 6);
    }

    #[test]
    fn a_repeated_move_without_continuation_drops_the_variation() {
        // 1.Ra5 (1.Ra5 Kd7): merging would extend the main line.
        let stream = [0x1C, 13, 0x1C, 0x01, 14, 15];
        let mut db = database(&[record_two(&stream)]);
        let (game, report) = db.game_with_report(1).unwrap();
        assert_eq!(report.discarded_variations, 1);
        assert_eq!(report.merged_variations, 0);
        assert_eq!(game.mainline().count(), 1);
        assert_eq!(game.node_count(), 2);
        // The raw tree keeps it.
        assert_eq!(db.raw_game(1).unwrap().lines[1].moves.len(), 2);
        // A variation that is only the repeated move is a plain merge.
        let stream = [0x1C, 13, 0x1C, 14, 15];
        let mut db = database(&[record_two(&stream)]);
        let (_, report) = db.game_with_report(1).unwrap();
        assert_eq!(
            (report.merged_variations, report.discarded_variations),
            (1, 0)
        );
    }

    #[test]
    fn a_merged_duplicate_only_adds_missing_comment_and_nags() {
        // 1.Ra5 {first} (1.Ra5 {second} $1 Ke7) Kd7: the existing node keeps
        // its comment; it had no NAG, so it takes the duplicate's.
        let stream = [0x1C, 12, 13, 0x1C, 12, 11, 1, 0x02, 14, 0x01, 15];
        let mut rec = record_two(&stream);
        rec.extend_from_slice(b"first\0second\0");
        let mut db = database(&[rec]);
        let game = db.game(1).unwrap();
        let ra5 = game.children(game.root()).unwrap()[0];
        assert_eq!(game.comment(ra5).unwrap(), Some("first"));
        assert_eq!(game.nags(ra5).unwrap().len(), 1);
    }

    #[test]
    fn castling_promotion_en_passant_and_null_become_core_moves() {
        // White list [E1, A1, H1], black list [E8, A8, H8].
        let mut castle = vec![0, 1];
        castle.extend_from_slice(b"r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1\0");
        castle.extend_from_slice(&[0x0A, 0x09, 15]);
        // White pawn a7 is piece 1 (the king goes first): a7-a8=Q is v = 1 + 3.
        let mut promo = vec![0, 1];
        promo.extend_from_slice(b"8/P6k/8/8/8/8/8/K7 w - - 0 1\0");
        promo.extend_from_slice(&[0x14, 15]);
        // Black pawn d4 is piece 1: d4xe3 e.p. is 7 down (v = 0).
        let mut ep = vec![0, 1];
        ep.extend_from_slice(b"4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1\0");
        ep.extend_from_slice(&[0x10, 15]);
        let null = vec![0, 0, 0xCF, 0x00, 15];
        let mut db = database(&[castle, promo, ep, null]);

        let g = db.game(1).unwrap();
        let moves: Vec<_> = g.mainline().map(|id| g.mv(id).unwrap()).collect();
        assert!(moves[0].is_castle() && moves[1].is_castle());
        assert_eq!(
            moves[0].castling_side(),
            Some(mychess_core::CastlingSide::KingSide)
        );
        assert_eq!(
            moves[1].castling_side(),
            Some(mychess_core::CastlingSide::QueenSide)
        );

        let g = db.game(2).unwrap();
        let id = g.mainline().next().unwrap();
        assert_eq!(
            g.mv(id).unwrap().promotion(),
            Some(mychess_core::Role::Queen)
        );

        let g = db.game(3).unwrap();
        let id = g.mainline().next().unwrap();
        assert!(g.mv(id).unwrap().is_en_passant());

        let g = db.game(4).unwrap();
        let ids: Vec<_> = g.mainline().collect();
        assert!(g.mv(ids[1]).unwrap().is_null());
    }

    #[test]
    fn extra_tags_event_date_and_ratings_follow_the_rules_of_the_conversion() {
        let tag = |code: &[u8], value: &[u8]| {
            let mut t = code.to_vec();
            t.push(u8::try_from(value.len()).unwrap());
            t.extend_from_slice(value);
            t
        };
        let literal = |name: &[u8], value: &[u8]| {
            let mut t = vec![u8::try_from(name.len()).unwrap()];
            t.extend_from_slice(name);
            t.push(u8::try_from(value.len()).unwrap());
            t.extend_from_slice(value);
            t
        };
        let record = |tags: Vec<Vec<u8>>| {
            let mut r: Vec<u8> = tags.concat();
            r.extend_from_slice(&[0, 0, 15]);
            r
        };
        let date255 = (2010u32 << 9) | (3 << 5) | 4;
        let mut tag255 = vec![255];
        tag255.extend_from_slice(&date255.to_be_bytes()[1..]);
        let records = vec![
            // 0: EventDate (245) fills an unknown event date.
            record(vec![tag(&[245], b"2001.02.03")]),
            // 1: the index has an event date, the 245 tag does not override it.
            record(vec![tag(&[245], b"1990.01.01")]),
            // 2: tag 255 wins over the index.
            record(vec![tag255]),
            // 3: refused names are dropped; CR, LF and NUL in values become spaces.
            record(vec![
                literal(b"Bad Name", b"x"),
                literal(b"Event", b"y"),
                literal(b"Good", b"a\r\nb\0c"),
            ]),
            // 4: reserved rating type.
            record(vec![]),
        ];
        let mut db = database_with(&records, |i, e| {
            if i == 1 || i == 2 {
                // Event date 2000.02.03 (offset 5 from the game year 1999).
                let event = (5u32 << 9) | (2 << 5) | 3;
                let dates = (event << 20) | (1999 << 9) | (12 << 5) | 31;
                e[25..29].copy_from_slice(&dates.to_be_bytes());
            }
            if i == 4 {
                e[29..31].copy_from_slice(&((9u16 << 12) | 2000).to_be_bytes());
            }
        });
        let date = |db: &mut Si4Database<Cursor<Vec<u8>>>, n| {
            db.game(n).unwrap().tags().event_date.to_string()
        };
        assert_eq!(date(&mut db, 1), "2001.02.03");
        assert_eq!(date(&mut db, 2), "2000.02.03");
        assert_eq!(date(&mut db, 3), "2010.03.04");
        let g = db.game(4).unwrap();
        assert_eq!(g.tags().get_extra("Good"), Some("a  b c"));
        assert_eq!(g.tags().get_extra("Bad Name"), None);
        assert_eq!(
            g.tags().event,
            "Wijk",
            "a tag named Event does not replace it"
        );
        assert_eq!(g.tags().extras().len(), 1);
        assert_eq!(db.game(5).unwrap().tags().white_rating, None);
        assert_eq!(db.entry(5).unwrap().white_elo, 2000);
    }

    #[test]
    fn an_invalid_result_fails_only_when_the_game_is_read() {
        let mut db = database_with(&[record_one(), record_one()], |i, e| {
            if i == 1 {
                e[21..23].copy_from_slice(&(5u16 << 12).to_be_bytes());
            }
        });
        assert_eq!(db.len(), 2, "open does not reject the database");
        assert!(db.game(1).is_ok());
        assert!(matches!(
            db.entry(2),
            Err(Si4Error::CorruptIndex { game: 2, .. })
        ));
        assert!(matches!(
            db.game(2),
            Err(Si4Error::CorruptIndex { game: 2, .. })
        ));
    }

    #[test]
    fn errors_for_bad_game_numbers_illegal_moves_and_illegal_starts() {
        let illegal_move = {
            // Standard start: rook index 1, v = 8 + 3 is a1-a4, blocked by a2.
            vec![0, 0, 0x1B, 15]
        };
        let illegal_start = {
            let mut r = vec![0, 1];
            r.extend_from_slice(b"4k3/8/8/8/8/8/4r3/4K3 b - - 0 1\0");
            r.push(15);
            r
        };
        let mut db = database(&[record_one(), illegal_move, illegal_start]);
        assert!(matches!(
            db.game(0),
            Err(Si4Error::InvalidGameNumber { number: 0, len: 3 })
        ));
        assert!(matches!(
            db.game(4),
            Err(Si4Error::InvalidGameNumber { number: 4, .. })
        ));
        assert!(matches!(
            db.entry(4),
            Err(Si4Error::InvalidGameNumber { .. })
        ));
        assert!(matches!(
            db.game(2),
            Err(Si4Error::IllegalMove { game: 2, ply: 1 })
        ));
        assert!(matches!(
            db.game(3),
            Err(Si4Error::IllegalStart { game: 3, .. })
        ));
        // A failing game does not prevent reading the others.
        assert!(db.game(1).is_ok());
        assert_eq!(db.entries().count(), 3);
    }

    #[test]
    fn corrupt_records_report_the_game_and_the_offset() {
        let mut db = database(&[record_one(), vec![0, 0, 0xCF]]);
        match db.game(2) {
            Err(Si4Error::CorruptRecord {
                game: 2, offset, ..
            }) => {
                assert_eq!(offset, u64::try_from(record_one().len() + 3).unwrap());
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            db.decode_mainline(2),
            Err(Si4Error::CorruptRecord { .. })
        ));
        assert!(db.game(1).is_ok());
    }

    #[test]
    fn open_checks_the_index_size_and_name_ids() {
        let names = names_file(&["A"], &["E"], &["S"], &["R"]);
        let good = entry_bytes(0, 3, 0, 0, 0);
        let mut ok = index_file(std::slice::from_ref(&good));
        assert!(
            Si4Database::from_parts(ok.clone(), &names, Cursor::new(vec![0, 0, 15]), "x".into())
                .is_ok()
        );
        // Truncated by one byte.
        ok.pop();
        assert!(matches!(
            Si4Database::from_parts(ok, &names, Cursor::new(vec![]), "x".into()),
            Err(Si4Error::Truncated {
                file: Si4File::Index,
                ..
            })
        ));
        // The second game uses a player id that does not exist.
        let bad = entry_bytes(0, 3, 5, 0, 0);
        let index = index_file(&[good, bad]);
        assert!(matches!(
            Si4Database::from_parts(index, &names, Cursor::new(vec![0, 0, 15]), "x".into()),
            Err(Si4Error::CorruptIndex { game: 2, .. })
        ));
    }

    #[test]
    fn opens_files_by_base_name_or_any_extension() {
        let dir = tempfile::tempdir().unwrap();
        let names = names_file(&["A", "B"], &["E"], &["S"], &["R"]);
        let record = vec![0, 0, 0xCF, 15];
        fs::write(
            dir.path().join("games.si4"),
            index_file(&[entry_bytes(0, 4, 0, 1, 1)]),
        )
        .unwrap();
        fs::write(dir.path().join("games.SN4"), names).unwrap();
        fs::write(dir.path().join("games.sg4"), record).unwrap();
        for name in ["games", "games.si4", "games.sn4", "games.sg4", "games.SG4"] {
            let mut db = Si4Database::open(&dir.path().join(name)).unwrap();
            assert_eq!(db.len(), 1, "{name}");
            assert_eq!(db.game(1).unwrap().tags().result, GameResult::White);
        }
        assert!(matches!(
            Si4Database::open(&dir.path().join("missing")),
            Err(Si4Error::Io { .. })
        ));
    }

    #[test]
    fn strips_only_known_extensions() {
        assert_eq!(
            strip_known_extension(Path::new("a/b.si4")),
            Path::new("a/b")
        );
        assert_eq!(
            strip_known_extension(Path::new("a/b.SN4")),
            Path::new("a/b")
        );
        assert_eq!(
            strip_known_extension(Path::new("a/b.v2")),
            Path::new("a/b.v2")
        );
        assert_eq!(strip_known_extension(Path::new("a/b")), Path::new("a/b"));
    }

    #[test]
    fn finds_files_without_regard_to_case() {
        let dir = tempfile::tempdir().unwrap();
        let upper = dir.path().join("Base.SI4");
        fs::write(&upper, b"x").unwrap();
        let found = find_file(&dir.path().join("Base"), "si4");
        assert_eq!(found.file_name().and_then(|n| n.to_str()), Some("Base.SI4"));
        // A missing file gives the exact name back.
        let missing = find_file(&dir.path().join("Other"), "sn4");
        assert_eq!(missing, dir.path().join("Other.sn4"));
    }
}
