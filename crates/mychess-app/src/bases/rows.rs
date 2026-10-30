//! Rows of the game list (spec M3-4, D509; Scid: `glistFields`,
//! `IndexEntry::PrintGameInfo`).
//!
//! [`list_page`] reads the index entries of the games of a filter, in the
//! order of the database, and turns them into [`GameRow`]s. Only the index
//! and the name base are looked at (both are in memory), never a game
//! record, so a page costs a few microseconds even in a database of a
//! million games and may be asked for from `view`.

use mychess_core::GameResult;
use mychess_db::si4::{GameFlags, IndexEntry, MaterialSignature, bytes_to_string};
use mychess_db::{Filter, GameSource};

/// One line of the game list: the values of the 22 columns of the Scid.
///
/// Fields keep the value; the front-end formats it (padding, `##` for long
/// games in a narrow column, the translation of the column names). Where
/// the Scid prints a blank for "none", the field is empty or zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameRow {
    /// Column `g`, Number: the number of the game in the database (from 1),
    /// not its position in the filter.
    pub number: u32,
    /// Column `w`, White: the name of the player.
    pub white: String,
    /// Column `b`, Black.
    pub black: String,
    /// Column `r`, Result.
    pub result: GameResult,
    /// Column `m`, Length: the number of full moves of the main line,
    /// `(half moves + 1) / 2` (the index keeps at most 1023 half moves, so
    /// longer games show 512). A column narrower than 3 characters shows
    /// `##` when this is above 99.
    pub length: u16,
    /// Column `d`, Date, without the unknown trailing parts (`2024.05`).
    pub date: String,
    /// Column `e`, Event.
    pub event: String,
    /// Column `W`, `WElo`: the Elo of White stored in the game, 0 if none.
    /// The fallback to the rating of the player in the spelling file (the
    /// `(2750)` form of the Scid) is up to the front-end.
    pub white_elo: u16,
    /// Column `B`, `BElo`, as `white_elo`.
    pub black_elo: u16,
    /// Column `n`, Round.
    pub round: String,
    /// Column `s`, Site.
    pub site: String,
    /// Column `D`, Deleted: the game is marked for deletion.
    pub deleted: bool,
    /// Column `V`, Variations: the approximate count of the index (scale
    /// 0 to 10, 15, 20, 30, 40, 50); 0 is shown blank.
    pub variations: u8,
    /// Column `C`, Comments: the approximate count, as `variations`.
    pub comments: u8,
    /// Column `A`, Annos: the approximate count of NAGs, as `variations`.
    pub annos: u8,
    /// Column `o`, ECO: the code with its extension (`B91a`), empty if the
    /// game has none.
    pub eco: String,
    /// Column `O`, Opening: empty in this slice (it needs the opening tree
    /// or the stored lines).
    pub opening: String,
    /// Column `U`, Flags: the letters of the flags set, `D` excluded, in
    /// the order of [`FLAG_LETTERS`](mychess_db::si4::FLAG_LETTERS); the
    /// custom flags are the digits `1` to `6`.
    pub flags: String,
    /// Column `S`, Start: the game starts from a position of its own.
    pub start: bool,
    /// Column `c`, Country: the last three characters of the Site when the
    /// one before them is a space and the first is an upper-case letter
    /// (`Wijk aan Zee NED` gives `NED`), else `?`.
    pub country: String,
    /// Column `E`, `EventDate`, as `date`.
    pub event_date: String,
    /// Column `F`, `EndMaterial`: the material of the final position, White
    /// before the colon and Black after (`Q2R:Qr3`): each queen, rook,
    /// bishop and knight is a letter, upper case for a pair and lower case
    /// for a single piece, then the number of pawns. Empty before the
    /// colon means no pieces.
    pub end_material: String,
    /// The game has an entry that cannot be read; every other field is
    /// empty (the front-end may draw the row dimmed).
    pub unreadable: bool,
}

impl GameRow {
    /// A row for a game whose entry cannot be read.
    pub fn unreadable(number: u32) -> GameRow {
        GameRow {
            number,
            white: String::new(),
            black: String::new(),
            result: GameResult::Unknown,
            length: 0,
            date: String::new(),
            event: String::new(),
            white_elo: 0,
            black_elo: 0,
            round: String::new(),
            site: String::new(),
            deleted: false,
            variations: 0,
            comments: 0,
            annos: 0,
            eco: String::new(),
            opening: String::new(),
            flags: String::new(),
            start: false,
            country: String::new(),
            event_date: String::new(),
            end_material: String::new(),
            unreadable: true,
        }
    }

    /// The row of game `number` from its index entry and the names.
    pub fn from_entry<S: GameSource + ?Sized>(
        source: &S,
        number: u32,
        entry: &IndexEntry,
    ) -> GameRow {
        use mychess_db::si4::NameKind;
        let names = source.names();
        let name = |kind, id| {
            names
                .get(kind, id)
                .map(|b| bytes_to_string(b).into_owned())
                .unwrap_or_default()
        };
        let site = name(NameKind::Site, entry.site_id);
        GameRow {
            number,
            white: name(NameKind::Player, entry.white_id),
            black: name(NameKind::Player, entry.black_id),
            result: entry.result,
            length: u16::try_from(u32::from(entry.num_half_moves).div_ceil(2)).unwrap_or(0),
            date: entry.date.short(),
            event: name(NameKind::Event, entry.event_id),
            white_elo: entry.white_elo,
            black_elo: entry.black_elo,
            round: name(NameKind::Round, entry.round_id),
            deleted: entry.flags.contains(GameFlags::DELETE),
            variations: entry.variation_count,
            comments: entry.comment_count,
            annos: entry.nag_count,
            eco: entry.eco().map(|e| e.to_string()).unwrap_or_default(),
            opening: String::new(),
            flags: (entry.flags - GameFlags::DELETE)
                .letters()
                .chars()
                .collect(),
            start: entry.flags.contains(GameFlags::START),
            country: country_of(&site),
            site,
            event_date: entry.event_date.short(),
            end_material: end_material(entry.final_material()),
            unreadable: false,
        }
    }
}

/// The country column: the three upper-case letters that end the site
/// after a space (`Scid: IndexEntry::PrintGameInfo`, case `c`), else `?`.
pub(crate) fn country_of(site: &str) -> String {
    // Bytes, like the Scid: a space, an ASCII upper-case letter and two more
    // bytes close the name.
    let bytes = site.as_bytes();
    let len = bytes.len();
    if len > 3
        && bytes[len - 4] == b' '
        && bytes[len - 3].is_ascii_uppercase()
        && let Some(country) = site.get(len - 3..)
    {
        country.to_owned()
    } else {
        "?".to_owned()
    }
}

/// The text of the `EndMaterial` column (`Scid: PrintGameInfo`, case `F`).
fn end_material(sig: MaterialSignature) -> String {
    fn side(out: &mut String, pieces: [(u8, char); 4], pawns: u8) {
        for (count, letter) in pieces {
            for _ in 0..count / 2 {
                out.push(letter);
            }
            if count % 2 == 1 {
                out.extend(letter.to_lowercase());
            }
        }
        if pawns > 0 {
            // The Scid adds the count to '0' without a bound, so 10 to 15
            // pawns print ':' to '?'; kept for the same text.
            out.push(char::from(b'0'.saturating_add(pawns)));
        }
    }
    let mut out = String::new();
    side(
        &mut out,
        [
            (sig.white_queens, 'Q'),
            (sig.white_rooks, 'R'),
            (sig.white_bishops, 'B'),
            (sig.white_knights, 'N'),
        ],
        sig.white_pawns,
    );
    out.push(':');
    side(
        &mut out,
        [
            (sig.black_queens, 'Q'),
            (sig.black_rooks, 'R'),
            (sig.black_bishops, 'B'),
            (sig.black_knights, 'N'),
        ],
        sig.black_pawns,
    );
    out
}

/// The rows of the games of `filter`, from the `first`-th one of the filter
/// (from 1) on, at most `count` of them, in the order of the database.
///
/// Empty when `first` is 0 or past the end, when `count` is 0, or when the
/// filter does not match `source` (a different number of games).
pub fn list_page<S: GameSource + ?Sized>(
    source: &S,
    filter: &Filter,
    first: u32,
    count: u32,
) -> Vec<GameRow> {
    if filter.len() != source.len() {
        return Vec::new();
    }
    // `count` comes from the front-end (visible rows): never reserve more
    // than the filter can give.
    let wanted = count.min(filter.count().saturating_sub(first.saturating_sub(1)));
    let mut numbers = Vec::with_capacity(wanted as usize);
    let mut next = filter.nth(first);
    while let Some(number) = next {
        if numbers.len() >= wanted as usize {
            break;
        }
        numbers.push(number);
        next = filter.next_after(number);
    }
    rows_of(source, numbers)
}

/// The rows of the games `numbers`, in that order (the page of a sorted
/// view, slice 4c).
pub fn rows_of<S: GameSource + ?Sized>(
    source: &S,
    numbers: impl IntoIterator<Item = u32>,
) -> Vec<GameRow> {
    numbers
        .into_iter()
        .map(|number| match source.entry(number) {
            Ok(entry) => GameRow::from_entry(source, number, &entry),
            Err(_) => GameRow::unreadable(number),
        })
        .collect()
}
