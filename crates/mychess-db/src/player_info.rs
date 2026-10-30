// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_name_info`) and
// tcl/tools/pinfo.tcl. Copyright (c) 1999-2003 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! Information about a player: statistics from the database, the links
//! that set the filter and the header from the spelling file (rules 81 to
//! 90; `docs/specs/m6-3-jogadores-torneios-spelling.md`, section 3).

use mychess_core::{GameResult, MAX_YEAR, PartialDate};

use crate::filter::Filter;
use crate::finder::{Cancelled, find_name_id};
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, SEARCH_CHECK_EVERY, Ticker};
use crate::si4::NameKind;
use crate::source::GameSource;
use crate::spelling::{PlayerComment, SpellChecker, country_name, title_name};

/// The first year of the rating history (`startYear` of the Scid).
pub const RATING_START_YEAR: u16 = 1900;

/// How many opening groups are listed per colour.
const TOP_OPENINGS: usize = 6;

/// Results of a player in some set of games, from the player's point of
/// view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    /// Wins.
    pub wins: u32,
    /// Draws.
    pub draws: u32,
    /// Losses.
    pub losses: u32,
    /// Games without a result (`*`).
    pub unfinished: u32,
}

impl Tally {
    /// Number of games.
    pub fn games(&self) -> u32 {
        self.wins + self.draws + self.losses + self.unfinished
    }

    /// Twice the points: 2 per win, 1 per draw and 1 per game without result
    /// (rule 83).
    pub fn points2(&self) -> u32 {
        self.wins * 2 + self.draws + self.unfinished
    }

    /// The score in hundredths of a percent (`8750` = `87.50%`); 0 without
    /// games.
    pub fn percent_hundredths(&self) -> u32 {
        let n = u64::from(self.games());
        if n == 0 {
            return 0;
        }
        u32::try_from(u64::from(self.points2()) * 5000 / n).unwrap_or(u32::MAX)
    }

    /// The points as text (`3.5`).
    pub fn points_text(&self) -> String {
        let p = self.points2();
        format!("{}.{}", p / 2, if p % 2 == 1 { '5' } else { '0' })
    }

    fn add(&mut self, result: Outcome) {
        match result {
            Outcome::Win => self.wins += 1,
            Outcome::Draw => self.draws += 1,
            Outcome::Loss => self.losses += 1,
            Outcome::Unfinished => self.unfinished += 1,
        }
    }
}

/// The three lines Total, White and Black.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Split {
    /// Both colours.
    pub total: Tally,
    /// As White.
    pub white: Tally,
    /// As Black.
    pub black: Tally,
}

impl Split {
    fn add(&mut self, white: bool, result: Outcome) {
        self.total.add(result);
        if white {
            self.white.add(result);
        } else {
            self.black.add(result);
        }
    }
}

/// An opening group (`B9` = B90 to B99) and how the player did in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpeningShare {
    /// The group, 0 to 49: `(letter - 'A') * 10 + first digit`.
    pub group: u8,
    /// Games with an ECO code in the group.
    pub games: u32,
    /// Score percentage: `sum of (win 2, draw 1, loss 0, none 1) * 50 /
    /// games`.
    pub score_percent: u32,
}

impl OpeningShare {
    /// The label of the group: `B9`.
    pub fn label(&self) -> String {
        let letter = char::from(b'A' + self.group / 10);
        format!("{letter}{}", self.group % 10)
    }
}

/// Everything the window shows about a player, from the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerInfo {
    /// The name asked for.
    pub name: String,
    /// The name exists in the database; if not, everything else is empty.
    pub found: bool,
    /// Date of the first game with a known year.
    pub first: PartialDate,
    /// Date of the last game with a known year (shown only if after the
    /// first).
    pub last: PartialDate,
    /// All games.
    pub all: Split,
    /// The games in the filter; `None` without a filter.
    pub filter: Option<Split>,
    /// The filter contains every game of the database (the window then
    /// hides the filter section).
    pub filter_is_everything: bool,
    /// The opponent and the games against him or her.
    pub versus: Option<(String, Split)>,
    /// The most played opening groups with White and with Black.
    pub openings: [Vec<OpeningShare>; 2],
    /// Rating history as `(year, month 1 to 12, Elo)`, only the changes.
    pub ratings: Vec<(u16, u8, u16)>,
}

#[derive(Clone, Copy)]
enum Outcome {
    Win,
    Draw,
    Loss,
    Unfinished,
}

/// The result of a game from the point of view of the side with `white`.
fn outcome(result: GameResult, white: bool) -> Outcome {
    match (result, white) {
        (GameResult::White, true) | (GameResult::Black, false) => Outcome::Win,
        (GameResult::Black, true) | (GameResult::White, false) => Outcome::Loss,
        (GameResult::Draw, _) => Outcome::Draw,
        _ => Outcome::Unfinished,
    }
}

fn empty_info(name: &str) -> PlayerInfo {
    PlayerInfo {
        name: name.to_owned(),
        found: false,
        first: PartialDate::UNKNOWN,
        last: PartialDate::UNKNOWN,
        all: Split::default(),
        filter: None,
        filter_is_everything: false,
        versus: None,
        openings: [Vec::new(), Vec::new()],
        ratings: Vec::new(),
    }
}

/// The group of an ECO code, 0 to 49.
fn eco_group(code: mychess_core::Eco) -> Option<usize> {
    let text = code.basic_string();
    let b = text.as_bytes();
    let letter = b.first()?.checked_sub(b'A')?;
    let digit = b.get(1)?.checked_sub(b'0')?;
    let group = usize::from(letter) * 10 + usize::from(digit);
    (group < 50).then_some(group)
}

/// Statistics of the player `name` in one pass over the index (rule 82).
/// `filter` gives the "Filter games" section; `opponent` the section
/// "Against" (when that name exists in the database). Cancellable.
///
/// # Errors
///
/// [`Cancelled`] if `cancel` is set during the pass.
#[allow(clippy::too_many_lines)]
pub fn player_info<D: GameSource + ?Sized>(
    db: &D,
    name: &str,
    filter: Option<&Filter>,
    opponent: Option<&str>,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<PlayerInfo, Cancelled> {
    let names = db.names();
    let Some(id) = find_name_id(names, NameKind::Player, name) else {
        return Ok(empty_info(name));
    };
    let opponent_id =
        opponent.and_then(|o| find_name_id(names, NameKind::Player, o).map(|i| (o, i)));
    let mut info = empty_info(name);
    info.found = true;
    let mut filter_split = filter.map(|_| Split::default());
    let mut versus = opponent_id.map(|(o, _)| (o.to_owned(), Split::default()));
    let mut eco_count = [[0u32; 50]; 2];
    let mut eco_score = [[0u32; 50]; 2];
    let mut first: Option<PartialDate> = None;
    let mut last: Option<PartialDate> = None;
    let months = usize::from(MAX_YEAR) * 12;
    let mut elo_by_month = vec![0u16; months + 12];
    let games = db.len();
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    for n in 1..=games {
        if ticker.tick() {
            progress.progress(u64::from(n - 1), u64::from(games));
            if cancel.is_cancelled() {
                return Err(Cancelled);
            }
        }
        let Ok(e) = db.entry(n) else {
            continue;
        };
        // White has priority when both sides are the same name.
        let (white, elo, other) = if e.white_id == id {
            (true, e.white_elo, e.black_id)
        } else if e.black_id == id {
            (false, e.black_elo, e.white_id)
        } else {
            continue;
        };
        let result = outcome(e.result, white);
        info.all.add(white, result);
        if let (Some(f), Some(split)) = (filter, filter_split.as_mut())
            && f.contains(n)
        {
            split.add(white, result);
        }
        if let (Some((_, oid)), Some((_, split))) = (opponent_id, versus.as_mut())
            && other == oid
        {
            split.add(white, result);
        }
        if let Some(group) = e.eco().and_then(eco_group) {
            let side = usize::from(!white);
            let score = match result {
                Outcome::Win => 2,
                Outcome::Loss => 0,
                _ => 1,
            };
            eco_count[side][group] += 1;
            eco_score[side][group] += score;
        }
        if e.date.year().is_some() {
            if first.is_none_or(|f| e.date < f) {
                first = Some(e.date);
            }
            if last.is_none_or(|l| e.date > l) {
                last = Some(e.date);
            }
        }
        let year = usize::from(e.date.year().unwrap_or(0));
        let month = usize::from(e.date.month().unwrap_or(1)).saturating_sub(1);
        if let Some(slot) = elo_by_month.get_mut(year * 12 + month.min(11)) {
            *slot = elo;
        }
    }
    progress.progress(u64::from(games), u64::from(games));
    info.first = first.unwrap_or(PartialDate::UNKNOWN);
    info.last = last.unwrap_or(PartialDate::UNKNOWN);
    info.filter = filter_split;
    info.filter_is_everything = filter.is_some_and(|f| f.count() == games);
    info.versus = versus;
    for side in 0..2 {
        let (counts, scores) = (&mut eco_count[side], &eco_score[side]);
        for _ in 0..TOP_OPENINGS {
            // The first group with the most games (strictly greater).
            let mut best: Option<usize> = None;
            for (i, &c) in counts.iter().enumerate() {
                if c > best.map_or(0, |b| counts[b]) {
                    best = Some(i);
                }
            }
            let Some(b) = best else {
                break;
            };
            let played = counts[b];
            if let Some(slot) = info.openings.get_mut(side) {
                slot.push(OpeningShare {
                    group: u8::try_from(b).unwrap_or(0),
                    games: played,
                    score_percent: scores[b] * 50 / played,
                });
            }
            counts[b] = 0;
        }
    }
    let mut previous = 0u16;
    for (i, &elo) in elo_by_month
        .iter()
        .enumerate()
        .skip(usize::from(RATING_START_YEAR) * 12)
    {
        if elo != 0 && elo != previous {
            previous = elo;
            let year = u16::try_from(i / 12).unwrap_or(0);
            let month = u8::try_from(i % 12 + 1).unwrap_or(1);
            info.ratings.push((year, month, elo));
        }
    }
    Ok(info)
}

/// One side of a filter link: which results of games with White and with
/// Black to take (rule 88).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GameSelection {
    /// Results taken from games as White, `[win, draw, loss, unfinished]`.
    pub white: [bool; 4],
    /// Results taken from games as Black.
    pub black: [bool; 4],
}

impl GameSelection {
    /// Reads the letters of the Scid: `a` all games as White, `A` as
    /// Black, `w`/`W` wins, `d`/`D` draws, `l`/`L` losses (lowercase for
    /// White, uppercase for Black). `None` for any other letter.
    ///
    /// ```
    /// use mychess_db::player_info::GameSelection;
    ///
    /// let s = GameSelection::from_letters("wW").unwrap();
    /// assert!(s.white[0] && s.black[0] && !s.white[1]);
    /// assert!(GameSelection::from_letters("x").is_none());
    /// ```
    pub fn from_letters(letters: &str) -> Option<GameSelection> {
        let mut s = GameSelection::default();
        for c in letters.chars() {
            match c {
                'a' => s.white = [true; 4],
                'A' => s.black = [true; 4],
                'w' => s.white[0] = true,
                'W' => s.black[0] = true,
                'd' => s.white[1] = true,
                'D' => s.black[1] = true,
                'l' => s.white[2] = true,
                'L' => s.black[2] = true,
                _ => return None,
            }
        }
        Some(s)
    }

    fn takes(self, white: bool, result: Outcome) -> bool {
        let index = match result {
            Outcome::Win => 0,
            Outcome::Draw => 1,
            Outcome::Loss => 2,
            Outcome::Unfinished => 3,
        };
        let side = if white { &self.white } else { &self.black };
        side.get(index).copied().unwrap_or(false)
    }
}

/// How a link changes the filter (`sc_name info -f`, `-F`, `-o`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerFilterMode {
    /// `-f`: the filter becomes the selected games of the player.
    Set,
    /// `-F`: games of other players leave the filter, then the selected
    /// games of the player are added.
    Refine,
    /// `-o`: the filter becomes the selected games against the opponent.
    Opponent,
}

/// Sets `filter` from a link of the information window (rules 88, D3320)
/// and returns the number of games in it. `opponent` is needed (and used)
/// only by [`PlayerFilterMode::Opponent`]. If the filter does not cover the
/// database, or `name` does not exist (or, for the opponent mode, the
/// opponent), nothing is included. Cancelling leaves `filter` as it was.
///
/// # Errors
///
/// [`Cancelled`] if `cancel` is set during the pass.
#[allow(clippy::too_many_arguments)]
pub fn player_games_filter<D: GameSource + ?Sized>(
    db: &D,
    name: &str,
    selection: GameSelection,
    mode: PlayerFilterMode,
    opponent: Option<&str>,
    filter: &mut Filter,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<u32, Cancelled> {
    let games = db.len();
    if filter.len() != games {
        return Ok(filter.count());
    }
    let names = db.names();
    let id = find_name_id(names, NameKind::Player, name);
    let opponent_id = match mode {
        PlayerFilterMode::Opponent => {
            opponent.and_then(|o| find_name_id(names, NameKind::Player, o))
        }
        _ => None,
    };
    let mut work = filter.clone();
    if mode != PlayerFilterMode::Refine {
        work.fill(0);
    }
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    for n in 1..=games {
        if ticker.tick() {
            progress.progress(u64::from(n - 1), u64::from(games));
            if cancel.is_cancelled() {
                return Err(Cancelled);
            }
        }
        let Ok(e) = db.entry(n) else {
            continue;
        };
        let involved = id.is_some_and(|i| e.white_id == i || e.black_id == i);
        if mode == PlayerFilterMode::Refine && !involved {
            work.set(n, 0);
            continue;
        }
        let Some(id) = id.filter(|_| involved) else {
            continue;
        };
        let white = e.white_id == id;
        let other = if white { e.black_id } else { e.white_id };
        if mode == PlayerFilterMode::Opponent && opponent_id != Some(other) {
            continue;
        }
        if selection.takes(white, outcome(e.result, white)) {
            work.set(n, 1);
        }
    }
    progress.progress(u64::from(games), u64::from(games));
    *filter = work;
    Ok(filter.count())
}

/// The part of the window that comes from the spelling file (section 3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellingHeader {
    /// The title codes as written (`GM`, `im`).
    pub title_codes: Vec<String>,
    /// English names of the titles; unknown codes appear as written.
    pub title_names: Vec<String>,
    /// English names of the countries; unknown codes appear as written.
    pub country_names: Vec<String>,
    /// The peak rating; 0 if unknown.
    pub peak: u16,
    /// The peak ends in `*`.
    pub peak_is_estimate: bool,
    /// The birth field as written (`1904.??.??--1979.08.15`).
    pub born: String,
    /// The biography notes, in file order.
    pub bio: Vec<String>,
    /// The digits of a `FIDEID` note.
    pub fide_id: Option<String>,
    /// The name matched exactly; if not, the data may belong to another
    /// player with the same prefix (D3319).
    pub exact: bool,
}

/// The id after `FIDEID ` in a note: 1 to 12 digits, nothing else.
///
/// ```
/// use mychess_db::player_info::fide_id_of;
///
/// assert_eq!(fide_id_of("FIDEID 4633865"), Some("4633865"));
/// assert_eq!(fide_id_of("FIDEID 12ab"), None);
/// ```
pub fn fide_id_of(note: &str) -> Option<&str> {
    let id = note.strip_prefix("FIDEID ")?.trim();
    (!id.is_empty() && id.len() <= 12 && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// The FIDE profile page for an id from [`fide_id_of`].
pub fn fide_profile_url(id: &str) -> String {
    format!("https://ratings.fide.com/profile/{id}")
}

/// The header of the window from the spelling file, with the loose
/// matching of the Scid; `None` when no name matches.
pub fn spelling_header(checker: &SpellChecker, name: &str) -> Option<SpellingHeader> {
    let comment = checker.comment_loose(name)?;
    let parsed = PlayerComment::parse(comment);
    let title_codes: Vec<String> = parsed.titles_raw().into_iter().map(str::to_owned).collect();
    let title_names = title_codes
        .iter()
        .map(|c| title_name(c).map_or_else(|| c.clone(), str::to_owned))
        .collect();
    let country_names = parsed
        .countries()
        .into_iter()
        .map(|c| country_name(c).unwrap_or(c).to_owned())
        .collect();
    let bio: Vec<String> = checker.bio(name).to_vec();
    let fide_id = bio.iter().find_map(|n| fide_id_of(n)).map(str::to_owned);
    Some(SpellingHeader {
        title_codes,
        title_names,
        country_names,
        peak: parsed.peak(),
        peak_is_estimate: parsed.peak_is_estimate(),
        born: parsed.born_field().to_owned(),
        bio,
        fide_id,
        exact: checker.match_is_exact(name),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tally_math_matches_the_example() {
        let t = Tally {
            wins: 3,
            draws: 1,
            losses: 0,
            unfinished: 0,
        };
        assert_eq!(t.points2(), 7);
        assert_eq!(t.percent_hundredths(), 8750);
        assert_eq!(t.points_text(), "3.5");
        assert_eq!(Tally::default().percent_hundredths(), 0);
    }

    #[test]
    fn letters() {
        let s = GameSelection::from_letters("faA").is_none();
        assert!(s, "f is not a selection letter");
        let all = GameSelection::from_letters("aA").unwrap();
        assert_eq!(all.white, [true; 4]);
        assert_eq!(all.black, [true; 4]);
        assert_eq!(
            GameSelection::from_letters("").unwrap(),
            GameSelection::default()
        );
        let d = GameSelection::from_letters("dL").unwrap();
        assert!(d.white[1] && d.black[2] && !d.white[2]);
    }

    #[test]
    fn labels_and_fide() {
        assert_eq!(
            OpeningShare {
                group: 19,
                games: 1,
                score_percent: 0
            }
            .label(),
            "B9"
        );
        assert_eq!(fide_id_of("FIDEID 1234567890123"), None);
        assert_eq!(
            fide_profile_url("42"),
            "https://ratings.fide.com/profile/42"
        );
    }
}
