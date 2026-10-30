//! The Clipbase: a temporary database in memory (type 2), never written to
//! disk (spec M3-4, D507; Scid: `CLIPBASE_NUM`, "Temporary database, not
//! kept on disk.").
//!
//! The games are kept the way the SI4 format keeps them (47-byte index
//! entries, the encoded records and a name base), so the Clipbase reads like
//! any other database: it implements [`GameSource`] and the list code treats
//! it as one. Nothing is stored twice and no file or temporary directory is
//! involved.
//!
//! Adding a game is incremental (D524): the entry and the record are
//! appended, the names are appended to the [`NameBase`] in place
//! ([`NameBase::append_name`]) and found again through a hash index that
//! stores IDs only, so the cost is the encoding of the game plus the hashing
//! of its five names, independent of the size of the Clipbase.
//! [`Clipbase::push_many`] adds a batch, all or nothing.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::ops::Range;
use std::path::PathBuf;

use mychess_core::Game;
use mychess_db::GameSource;
use mychess_db::si4::{
    ENTRY_LEN, GameFlags, Header, HeaderBytes, IndexEntry, MAX_GAMES, NameBase, NameBuilder,
    NameKind, PreparedGame, RawGame, Si4Database, Si4Error,
};

use super::error::BaseError;

mod edit;

/// The default limit of games (`CLIPBASE_MAX_GAMES` of the Scid).
pub const DEFAULT_CLIPBASE_LIMIT: u32 = 5_000_000;

/// The description stored in the header of the Clipbase.
pub const CLIPBASE_DESCRIPTION: &str = "Temporary database, not kept on disk.";

/// The base type of the Clipbase (`2`, "clipbase").
pub const CLIPBASE_TYPE: u32 = 2;

type DbResult<T> = Result<T, Si4Error>;

/// The kind of name behind each of the five name IDs of an entry: White,
/// Black, Event, Site, Round.
const NAME_KINDS: [NameKind; 5] = [
    NameKind::Player,
    NameKind::Player,
    NameKind::Event,
    NameKind::Site,
    NameKind::Round,
];

fn kind_slot(kind: NameKind) -> usize {
    match kind {
        NameKind::Player => 0,
        NameKind::Event => 1,
        NameKind::Site => 2,
        NameKind::Round => 3,
    }
}

fn hash_of(name: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    hasher.finish()
}

/// A database in memory that only grows (until [`Clipbase::clear`]).
pub struct Clipbase {
    header_bytes: HeaderBytes,
    header: Header,
    /// The 47-byte entries, without the header.
    entries: Vec<u8>,
    /// The records, one after the other.
    records: Vec<u8>,
    names: NameBase,
    /// Per kind of name: hash of the name to the IDs that have it. The
    /// names themselves live only in `names`.
    lookup: [HashMap<u64, Vec<u32>>; 4],
    count: u32,
    limit: u32,
}

impl std::fmt::Debug for Clipbase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clipbase")
            .field("games", &self.count)
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

impl Clipbase {
    /// An empty Clipbase that holds at most `limit` games.
    ///
    /// # Errors
    ///
    /// [`BaseError::Internal`] if the empty header or name base cannot be
    /// built (it cannot happen; the error keeps the code free of panics).
    pub fn new(limit: u32) -> Result<Clipbase, BaseError> {
        let mut header_bytes = HeaderBytes::new(CLIPBASE_TYPE);
        header_bytes.set_description(CLIPBASE_DESCRIPTION);
        let header = header_bytes.parse().map_err(|_| BaseError::Internal)?;
        let names =
            NameBase::parse(&NameBuilder::new().to_bytes()).map_err(|_| BaseError::Internal)?;
        Ok(Clipbase {
            header_bytes,
            header,
            entries: Vec::new(),
            records: Vec::new(),
            names,
            lookup: Default::default(),
            count: 0,
            limit,
        })
    }

    /// The most games the Clipbase accepts.
    pub fn limit(&self) -> u32 {
        self.limit
    }

    /// Changes the limit. Games already in stay, even above the new limit;
    /// only new ones are refused.
    pub fn set_limit(&mut self, limit: u32) {
        self.limit = limit;
    }

    /// Removes every game (`sc_clipbase clear`). The limit stays.
    ///
    /// # Errors
    ///
    /// [`BaseError::Internal`], as for [`Clipbase::new`]; the Clipbase is
    /// unchanged then.
    pub fn clear(&mut self) -> Result<(), BaseError> {
        let fresh = Clipbase::new(self.limit)?;
        *self = fresh;
        Ok(())
    }

    /// Encodes `game` (no user flags) and adds it at the end. Returns its
    /// number (from 1).
    ///
    /// # Errors
    ///
    /// [`BaseError::ClipbaseLimit`] at the limit; [`BaseError::Unencodable`]
    /// if the game cannot be stored in the SI4 format. Nothing changes on
    /// error.
    pub fn push_game(&mut self, game: &Game) -> Result<u32, BaseError> {
        let prepared = PreparedGame::from_game(game, GameFlags::empty())
            .map_err(|e| BaseError::from_encode(&e))?;
        self.push(&prepared)
    }

    /// Adds an encoded game at the end. Returns its number (from 1).
    ///
    /// # Errors
    ///
    /// As [`Clipbase::push_game`]. Nothing changes on error.
    pub fn push(&mut self, game: &PreparedGame) -> Result<u32, BaseError> {
        self.push_many(std::slice::from_ref(game)).map(|r| r.start)
    }

    /// Adds encoded games at the end, all or none (the copy of a filter,
    /// slice 4d, prepares them in batches). Returns the numbers they got,
    /// first included, last excluded.
    ///
    /// # Errors
    ///
    /// [`BaseError::ClipbaseLimit`] if they do not all fit under the limit;
    /// [`BaseError::Unencodable`] if one cannot be stored. Nothing changes
    /// on error.
    pub fn push_many(&mut self, games: &[PreparedGame]) -> Result<Range<u32>, BaseError> {
        let first = self.count + 1;
        let added = u32::try_from(games.len()).map_err(|_| BaseError::Unencodable)?;
        let end = self
            .count
            .checked_add(added)
            .ok_or(BaseError::Unencodable)?;
        if end > self.limit {
            return Err(BaseError::ClipbaseLimit { limit: self.limit });
        }
        if end > MAX_GAMES {
            return Err(BaseError::ClipbaseLimit { limit: MAX_GAMES });
        }
        let undo = Undo {
            entries: self.entries.len(),
            records: self.records.len(),
            names: NameKind::ALL.map(|kind| self.names.num_names(kind)),
        };
        let mut used = Vec::with_capacity(games.len().saturating_mul(NAME_KINDS.len()));
        for game in games {
            if self.append(game, &mut used).is_err() {
                self.rollback(&undo);
                return Err(BaseError::Unencodable);
            }
        }
        // The counts only move once everything is in.
        for (kind, id) in used {
            self.names.add_frequency(kind, id, 1);
        }
        self.count = end;
        self.header_bytes.set_num_games(end);
        self.header = self.header_bytes.parse().map_err(|_| BaseError::Internal)?;
        Ok(first..end + 1)
    }

    /// Appends one game; the name IDs used go to `used`. On error the
    /// caller rolls back.
    fn append(&mut self, game: &PreparedGame, used: &mut Vec<(NameKind, u32)>) -> Result<(), ()> {
        let record = game.record();
        let offset = u32::try_from(self.records.len()).map_err(|_| ())?;
        let length = u32::try_from(record.len()).map_err(|_| ())?;
        offset.checked_add(length).ok_or(())?;
        let mut ids = [0u32; 5];
        for ((slot, kind), name) in ids.iter_mut().zip(NAME_KINDS).zip(game.names()) {
            *slot = self.name_id(kind, name)?;
            used.push((kind, *slot));
        }
        let mut entry = game.entry().clone();
        entry.offset = offset;
        entry.length = length;
        [
            entry.white_id,
            entry.black_id,
            entry.event_id,
            entry.site_id,
            entry.round_id,
        ] = ids;
        self.entries.extend_from_slice(&entry.to_bytes());
        self.records.extend_from_slice(record);
        Ok(())
    }

    /// The ID of `name`, adding it when it is new.
    fn name_id(&mut self, kind: NameKind, name: &[u8]) -> Result<u32, ()> {
        // The base keeps at most 255 bytes of a name.
        let name = name.get(..255).unwrap_or(name);
        let key = hash_of(name);
        let table = &self.lookup[kind_slot(kind)];
        if let Some(ids) = table.get(&key)
            && let Some(id) = ids
                .iter()
                .copied()
                .find(|id| self.names.get(kind, *id) == Some(name))
        {
            return Ok(id);
        }
        let id = self.names.append_name(kind, name).map_err(|_| ())?;
        self.lookup[kind_slot(kind)]
            .entry(key)
            .or_default()
            .push(id);
        Ok(id)
    }

    /// Undoes a failed batch.
    fn rollback(&mut self, undo: &Undo) {
        self.entries.truncate(undo.entries);
        self.records.truncate(undo.records);
        for (kind, keep) in NameKind::ALL.into_iter().zip(undo.names) {
            for ids in self.lookup[kind_slot(kind)].values_mut() {
                ids.retain(|id| *id < keep);
            }
            self.lookup[kind_slot(kind)].retain(|_, ids| !ids.is_empty());
            self.names.truncate_names(kind, keep);
        }
    }

    /// The bytes of the record described by `entry`.
    fn record_of(&self, n: u32, entry: &IndexEntry) -> DbResult<&[u8]> {
        let corrupt = |reason| Si4Error::CorruptRecord {
            game: n,
            offset: u64::from(entry.offset),
            reason,
        };
        let start = usize::try_from(entry.offset).map_err(|_| corrupt("offset"))?;
        let len = usize::try_from(entry.length).map_err(|_| corrupt("length"))?;
        let end = start.checked_add(len).ok_or_else(|| corrupt("length"))?;
        self.records
            .get(start..end)
            .ok_or_else(|| corrupt("record outside the data"))
    }

    /// A database of one game that stands for game `n`, to decode it with
    /// the code of the SI4 reader.
    fn single(&self, n: u32) -> DbResult<Si4Database<Cursor<Vec<u8>>>> {
        let mut entry = self.entry(n)?;
        let record = self.record_of(n, &entry)?.to_vec();
        let mut builder = NameBuilder::new();
        let wanted = [
            entry.white_id,
            entry.black_id,
            entry.event_id,
            entry.site_id,
            entry.round_id,
        ];
        let mut ids = [0u32; 5];
        for ((slot, kind), id) in ids.iter_mut().zip(NAME_KINDS).zip(wanted) {
            let name = self.names.get(kind, id).ok_or(Si4Error::CorruptIndex {
                game: n,
                reason: "name id outside the name base",
            })?;
            *slot = builder.add_name(kind, name)?;
        }
        entry.offset = 0;
        [
            entry.white_id,
            entry.black_id,
            entry.event_id,
            entry.site_id,
            entry.round_id,
        ] = ids;
        let mut header = HeaderBytes::new(CLIPBASE_TYPE);
        header.set_num_games(1);
        let mut index = header.as_bytes().to_vec();
        index.extend_from_slice(&entry.to_bytes());
        Si4Database::from_parts(
            index,
            &builder.to_bytes(),
            Cursor::new(record),
            PathBuf::new(),
        )
    }
}

/// What a failed batch has to give back.
struct Undo {
    entries: usize,
    records: usize,
    names: [u32; 4],
}

impl GameSource for Clipbase {
    fn header(&self) -> &Header {
        &self.header
    }

    fn len(&self) -> u32 {
        self.count
    }

    fn entry(&self, n: u32) -> DbResult<IndexEntry> {
        let invalid = Si4Error::InvalidGameNumber {
            number: n,
            len: self.count,
        };
        if n == 0 || n > self.count {
            return Err(invalid);
        }
        let start = usize::try_from(n - 1)
            .ok()
            .and_then(|i| i.checked_mul(ENTRY_LEN))
            .ok_or(Si4Error::InvalidGameNumber {
                number: n,
                len: self.count,
            })?;
        let bytes =
            self.entries
                .get(start..start + ENTRY_LEN)
                .ok_or(Si4Error::InvalidGameNumber {
                    number: n,
                    len: self.count,
                })?;
        let entry = IndexEntry::parse(bytes).map_err(|e| Si4Error::CorruptIndex {
            game: n,
            reason: e.reason(),
        })?;
        entry
            .verify_names(&self.names)
            .map_err(|e| Si4Error::CorruptIndex {
                game: n,
                reason: e.reason(),
            })?;
        Ok(entry)
    }

    fn names(&self) -> &NameBase {
        &self.names
    }

    fn record_bytes(&mut self, n: u32) -> DbResult<&[u8]> {
        let entry = self.entry(n)?;
        self.record_of(n, &entry)
    }

    fn raw_game(&mut self, n: u32) -> DbResult<RawGame> {
        self.single(n)?.raw_game(1)
    }

    fn game(&mut self, n: u32) -> DbResult<Game> {
        self.single(n)?.game(1)
    }
}

#[cfg(test)]
mod tests {
    use mychess_core::GameResult;

    use super::*;

    fn game(white: &str, uci: &[&str]) -> Game {
        let mut game = Game::new();
        game.tags_mut().white = white.to_owned();
        game.tags_mut().result = GameResult::Draw;
        let mut node = game.root();
        let mut position = game.start_position().clone();
        for text in uci {
            let mv = position.parse_uci(text).expect("legal");
            node = game.add_move(node, mv).expect("add");
            position.play(mv).expect("play");
        }
        game
    }

    #[test]
    fn an_empty_clipbase_is_type_2_with_its_description() {
        let clip = Clipbase::new(10).expect("new");
        assert_eq!(clip.len(), 0);
        assert!(clip.is_empty());
        assert_eq!(clip.header().base_type(), CLIPBASE_TYPE);
        assert_eq!(clip.header().description(), CLIPBASE_DESCRIPTION.as_bytes());
    }

    #[test]
    fn games_come_back_as_they_went_in() {
        let mut clip = Clipbase::new(10).expect("new");
        let a = game("Anderssen", &["e2e4", "e7e5"]);
        let b = game("Kieseritzky", &["d2d4"]);
        assert_eq!(clip.push_game(&a).expect("push"), 1);
        assert_eq!(clip.push_game(&b).expect("push"), 2);
        assert_eq!(clip.len(), 2);
        assert_eq!(clip.header().num_games(), 2);
        let back = clip.game(1).expect("game 1");
        assert_eq!(back.tags().white, "Anderssen");
        assert_eq!(back.mainline().count(), 2);
        assert_eq!(clip.game(2).expect("game 2").tags().white, "Kieseritzky");
        assert!(clip.record_bytes(1).expect("record").len() > 2);
        assert_eq!(clip.raw_game(2).expect("raw").mainline().len(), 1);
        assert_eq!(clip.entry(2).expect("entry").result, GameResult::Draw);
    }

    #[test]
    fn the_limit_refuses_the_next_game_and_changes_nothing() {
        let mut clip = Clipbase::new(2).expect("new");
        let g = game("A", &["e2e4"]);
        clip.push_game(&g).expect("1");
        clip.push_game(&g).expect("2");
        assert_eq!(
            clip.push_game(&g),
            Err(BaseError::ClipbaseLimit { limit: 2 })
        );
        assert_eq!(clip.len(), 2);
        assert!(clip.game(2).is_ok());
        assert!(clip.game(3).is_err());
    }

    #[test]
    fn names_are_shared_between_games_and_batches_are_all_or_nothing() {
        let mut clip = Clipbase::new(4).expect("new");
        let a = PreparedGame::from_game(&game("Same", &["e2e4"]), GameFlags::empty()).expect("a");
        let b = PreparedGame::from_game(&game("Same", &["d2d4"]), GameFlags::empty()).expect("b");
        let range = clip
            .push_many(&[a.clone(), b.clone(), a.clone()])
            .expect("batch");
        assert_eq!(range, 1..4);
        assert_eq!(clip.len(), 3);
        // One White name for three games (Black is empty: a second player name).
        assert!(clip.names().num_names(NameKind::Player) <= 2);
        assert_eq!(clip.names().frequency(NameKind::Player, 0), Some(3));
        assert_eq!(clip.game(3).expect("game").tags().white, "Same");
        let before = (clip.len(), clip.names().num_names(NameKind::Player));
        let too_many = clip.push_many(&[a.clone(), b.clone()]);
        assert_eq!(too_many, Err(BaseError::ClipbaseLimit { limit: 4 }));
        assert_eq!(
            (clip.len(), clip.names().num_names(NameKind::Player)),
            before
        );
        assert_eq!(clip.push_many(&[]).expect("empty"), 4..4);
        assert_eq!(clip.push(&b).expect("one"), 4);
    }

    #[test]
    fn clear_empties_and_keeps_the_limit() {
        let mut clip = Clipbase::new(7).expect("new");
        clip.push_game(&game("A", &["e2e4"])).expect("push");
        clip.clear().expect("clear");
        assert_eq!(clip.len(), 0);
        assert_eq!(clip.limit(), 7);
        assert_eq!(clip.names().num_names(NameKind::Player), 0);
        assert_eq!(clip.push_game(&game("B", &[])).expect("push"), 1);
    }

    #[test]
    fn out_of_range_numbers_are_errors_not_panics() {
        let mut clip = Clipbase::new(3).expect("new");
        assert!(clip.entry(0).is_err());
        assert!(clip.entry(1).is_err());
        assert!(clip.game(1).is_err());
        assert!(clip.record_bytes(5).is_err());
    }
}
