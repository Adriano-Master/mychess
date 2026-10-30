// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_search_header`,
// `matchGameFlags`, `matchGameHeader`) and src/misc.cpp. Copyright (c)
// 1999-2002 Shane Hudson; Scid vs PC contributors. Scid is distributed under
// the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Search by header (`sc_search header`), decisions D328 to D340 and
//! D342 to D344 of `docs/specs/m3-3-filtro-busca-importacao.md`.

use std::time::{Duration, Instant};

use bitflags::bitflags;
use mychess_core::{Eco, Game, GameResult, PartialDate};
use mychess_pgn::write::{WriteOptions, game_to_string};
use thiserror::Error;

use super::names::{NamePattern, fold_case};
use crate::filter::MAX_UNREADABLE;
use crate::filter::{Filter, FilterOp};
use crate::player_stats::PlayerStats;
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, SEARCH_CHECK_EVERY, Ticker};
use crate::si4::{GameFlags, IndexEntry, NameBase, NameKind, bytes_to_string};
use crate::source::GameSource;

/// A three-way choice for a flag: must have it, must not, or do not care.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tri {
    /// The game must have the flag.
    Yes,
    /// The game must not have the flag.
    No,
    /// No restriction.
    #[default]
    Both,
}

impl Tri {
    /// Reads Scid's `strGetFlag` text (first character): `t y j o s 1` is
    /// yes, `f n 0` is no, `b 2` is both (any case). Anything else is `None`,
    /// an error for the caller to report (D335), not "matches nothing".
    pub fn from_scid_str(text: &str) -> Option<Tri> {
        match text.chars().next()?.to_ascii_lowercase() {
            't' | 'y' | 'j' | 'o' | 's' | '1' => Some(Tri::Yes),
            'f' | 'n' | '0' => Some(Tri::No),
            'b' | '2' => Some(Tri::Both),
            _ => None,
        }
    }
}

/// An inclusive range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MinMax<T> {
    /// Lower bound.
    pub min: T,
    /// Upper bound.
    pub max: T,
}

impl<T> MinMax<T> {
    /// A range from `min` to `max`.
    pub fn new(min: T, max: T) -> MinMax<T> {
        MinMax { min, max }
    }
}

impl<T: PartialOrd + Copy> MinMax<T> {
    fn contains(&self, value: T) -> bool {
        value >= self.min && value <= self.max
    }
}

/// Which results are accepted (Scid's `-results`: 1-0, draw, 0-1, none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResultSet {
    /// White wins (1-0).
    pub white: bool,
    /// Draw.
    pub draw: bool,
    /// Black wins (0-1).
    pub black: bool,
    /// No result (`*`).
    pub none: bool,
}

impl ResultSet {
    /// Every result.
    pub const ALL: ResultSet = ResultSet {
        white: true,
        draw: true,
        black: true,
        none: true,
    };

    fn accepts(self, result: GameResult, mirrored: bool) -> bool {
        match result {
            GameResult::White if mirrored => self.black,
            GameResult::White => self.white,
            GameResult::Black if mirrored => self.white,
            GameResult::Black => self.black,
            GameResult::Draw => self.draw,
            _ => self.none,
        }
    }
}

impl Default for ResultSet {
    fn default() -> ResultSet {
        ResultSet::ALL
    }
}

/// A FIDE title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Title {
    /// Grandmaster (`gm`, also `hgm`).
    Gm,
    /// International master.
    Im,
    /// FIDE master.
    Fm,
    /// Candidate master.
    Cm,
    /// Woman grandmaster.
    Wgm,
    /// Woman international master.
    Wim,
    /// Woman FIDE master.
    Wfm,
}

impl Title {
    /// Reads a title as Scid does: case-insensitive; `hgm` is `gm`.
    /// Other text (`cgm`, `cim`, empty) is no title: `None`.
    pub fn from_scid_str(text: &str) -> Option<Title> {
        match text.to_ascii_lowercase().as_str() {
            "gm" | "hgm" => Some(Title::Gm),
            "im" => Some(Title::Im),
            "fm" => Some(Title::Fm),
            "cm" => Some(Title::Cm),
            "wgm" => Some(Title::Wgm),
            "wim" => Some(Title::Wim),
            "wfm" => Some(Title::Wfm),
            _ => None,
        }
    }
}

bitflags! {
    /// A set of accepted titles; `NONE` stands for "no title".
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TitleSet: u8 {
        /// Grandmaster.
        const GM = 1;
        /// International master.
        const IM = 1 << 1;
        /// FIDE master.
        const FM = 1 << 2;
        /// Candidate master.
        const CM = 1 << 3;
        /// Woman grandmaster.
        const WGM = 1 << 4;
        /// Woman international master.
        const WIM = 1 << 5;
        /// Woman FIDE master.
        const WFM = 1 << 6;
        /// Players without a title.
        const NONE = 1 << 7;
    }
}

impl TitleSet {
    fn accepts(self, title: Option<Title>) -> bool {
        let bit = match title {
            Some(Title::Gm) => TitleSet::GM,
            Some(Title::Im) => TitleSet::IM,
            Some(Title::Fm) => TitleSet::FM,
            Some(Title::Cm) => TitleSet::CM,
            Some(Title::Wgm) => TitleSet::WGM,
            Some(Title::Wim) => TitleSet::WIM,
            Some(Title::Wfm) => TitleSet::WFM,
            None => TitleSet::NONE,
        };
        self.contains(bit)
    }
}

impl Default for TitleSet {
    fn default() -> TitleSet {
        TitleSet::all()
    }
}

/// Source of player titles, normally the spelling file (D340). There is no
/// implementation yet; without one the title restriction is ignored.
pub trait TitleProvider {
    /// The title of the player with this name, if known.
    fn title(&self, name: &str) -> Option<Title>;
}

/// ECO range. The maximum is expanded to its last subcode when searching
/// (`B07` includes `B07z4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EcoRange {
    /// First code.
    pub min: Eco,
    /// Last code.
    pub max: Eco,
    /// Whether games without an ECO code are accepted.
    pub include_none: bool,
}

/// The end-of-game test (Scid's `-gameend`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameEndKind {
    /// No test.
    #[default]
    Any,
    /// The main line ends in checkmate.
    Checkmate,
    /// The main line ends in stalemate.
    Stalemate,
}

/// Flag restrictions: for each of the 22 flags, [`Tri`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FlagFilter {
    required: GameFlags,
    forbidden: GameFlags,
}

impl FlagFilter {
    /// Sets the choice for `flag` (a single flag; for a set, every flag in it).
    pub fn set(&mut self, flag: GameFlags, choice: Tri) {
        self.required.remove(flag);
        self.forbidden.remove(flag);
        match choice {
            Tri::Yes => self.required.insert(flag),
            Tri::No => self.forbidden.insert(flag),
            Tri::Both => {}
        }
    }

    /// The choice for `flag` (`Both` if the flags in a set disagree).
    pub fn get(self, flag: GameFlags) -> Tri {
        if self.required.contains(flag) {
            Tri::Yes
        } else if self.forbidden.contains(flag) {
            Tri::No
        } else {
            Tri::Both
        }
    }

    /// Whether a game with `flags` passes.
    pub fn accepts(self, flags: GameFlags) -> bool {
        flags.contains(self.required) && flags.intersection(self.forbidden).is_empty()
    }
}

/// The options of a header search (`sc_search header`). `Default` is the
/// Scid dialog's defaults: no restriction, operation [`FilterOp::Reset`].
///
/// New fields will be added as the search grows; callers must build it with
/// `..Default::default()` (the type is not `#[non_exhaustive]` so that struct
/// update syntax keeps working).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderSearch {
    /// White name: text, or a `"glob"` between double quotes.
    pub white: String,
    /// Black name.
    pub black: String,
    /// Event name.
    pub event: String,
    /// Site name.
    pub site: String,
    /// Round name.
    pub round: String,
    /// Game date range; unknown parts count as 0 (`0000.00.00` to
    /// `2047.12.31` by default).
    pub date: MinMax<PartialDate>,
    /// Accepted results.
    pub results: ResultSet,
    /// White's Elo range (default 0 to 4000). A game without Elo uses the
    /// highest Elo of the player in the database.
    pub white_elo: MinMax<u16>,
    /// Black's Elo range.
    pub black_elo: MinMax<u16>,
    /// White Elo minus Black Elo (default -4000 to 4000; reversed ends are
    /// swapped).
    pub elo_diff: MinMax<i32>,
    /// Accepted titles of White (only with a [`TitleProvider`]).
    pub white_titles: TitleSet,
    /// Accepted titles of Black.
    pub black_titles: TitleSet,
    /// Accept games that end with White to move (even number of half moves).
    pub end_white_to_move: bool,
    /// Accept games that end with Black to move.
    pub end_black_to_move: bool,
    /// ECO range; `None` = no restriction.
    pub eco: Option<EcoRange>,
    /// Length in half moves; `None` = no restriction (D334).
    pub length: Option<MinMax<u16>>,
    /// Game numbers, `1` to `-1` = all; negative numbers count from the end.
    pub game_number: MinMax<i64>,
    /// Ignore colors: also try the game with White and Black swapped.
    pub ignore_colors: bool,
    /// How the result combines with the current filter.
    pub op: FilterOp,
    /// Flag restrictions.
    pub flags: FlagFilter,
    /// Name of an extra tag the game must have (exact, case-sensitive).
    pub extra_tag: String,
    /// Value of that tag: starting with a digit, equality; otherwise prefix.
    pub extra_tag_value: String,
    /// Phrases that must all appear in the PGN text of the game.
    pub pgn_text: Vec<String>,
    /// Ignore case in `pgn_text`.
    pub ignore_case: bool,
    /// End-of-game test.
    pub game_end: GameEndKind,
    /// Require a pre-game comment (not hidden, i.e. not `[...]`).
    pub pre_comment: bool,
    /// Require a comment on the last move (not hidden).
    pub post_comment: bool,
}

impl Default for HeaderSearch {
    fn default() -> HeaderSearch {
        HeaderSearch {
            white: String::new(),
            black: String::new(),
            event: String::new(),
            site: String::new(),
            round: String::new(),
            date: MinMax::new(
                PartialDate::UNKNOWN,
                PartialDate::parse_lenient("2047.12.31"),
            ),
            results: ResultSet::ALL,
            white_elo: MinMax::new(0, 4000),
            black_elo: MinMax::new(0, 4000),
            elo_diff: MinMax::new(-4000, 4000),
            white_titles: TitleSet::all(),
            black_titles: TitleSet::all(),
            end_white_to_move: true,
            end_black_to_move: true,
            eco: None,
            length: None,
            game_number: MinMax::new(1, -1),
            ignore_colors: false,
            op: FilterOp::Reset,
            flags: FlagFilter::default(),
            extra_tag: String::new(),
            extra_tag_value: String::new(),
            pgn_text: Vec::new(),
            ignore_case: false,
            game_end: GameEndKind::Any,
            pre_comment: false,
            post_comment: false,
        }
    }
}

/// Completes a minimum date typed in the dialog (`checkDates`): empty is
/// `0000`; `yyyy` becomes `yyyy.??.??`; `yyyy.mm` becomes `yyyy.mm.??`;
/// `yyyy.01.01` becomes `yyyy.??.??` so games with unknown month and day
/// are included.
pub fn complete_min_date(text: &str) -> PartialDate {
    let text = text.trim();
    if text.is_empty() {
        return PartialDate::UNKNOWN;
    }
    if let Some(year) = text.strip_suffix(".01.01")
        && year.len() == 4
    {
        return PartialDate::parse_lenient(year);
    }
    PartialDate::parse_lenient(text)
}

/// Completes a maximum date typed in the dialog (`checkDates`): empty is
/// `2047.12.31`; `yyyy` becomes `yyyy.12.31`; `yyyy.mm` becomes
/// `yyyy.mm.31`.
pub fn complete_max_date(text: &str) -> PartialDate {
    let text = text.trim();
    let full = match text.len() {
        0 => "2047.12.31".to_owned(),
        4 => format!("{text}.12.31"),
        7 => format!("{text}.31"),
        _ => text.to_owned(),
    };
    PartialDate::parse_lenient(&full)
}

/// The outcome of a search (D342). The interface formats `matched / start_count`
/// and `elapsed` (rule 23).
///
/// New fields will be added over time; read the ones you need and do not
/// build it with a full struct literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchReport {
    /// Games in the filter after the search. After an interrupted search the
    /// filter is the unchanged old one, so this is its count.
    pub matched: u32,
    /// The "before" count: the filter's count for AND, the number of games
    /// otherwise.
    pub start_count: u32,
    /// Time spent.
    pub elapsed: Duration,
    /// The search was cancelled; the filter was not changed (D328).
    pub interrupted: bool,
    /// Numbers of the first games that could not be read (at most
    /// [`MAX_UNREADABLE`] are kept); they count as not matching (D336).
    pub unreadable: Vec<u32>,
    /// How many games could not be read in total (all of them, not only the
    /// numbers kept).
    pub unreadable_total: u64,
}

/// Why a search could not run.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SearchError {
    /// The filter does not cover the same games as the database.
    #[error("filter has {filter} games but the database has {games}")]
    FilterSize {
        /// Games in the filter.
        filter: u32,
        /// Games in the database.
        games: u32,
    },
    /// The searched position does not fit the SI4 piece lists. Defensive
    /// (D3255): a `Position` of the core cannot trigger it (it refuses more
    /// than 16 pieces of a side and Chess960 castling rights are handled
    /// before), so it only guards the conversion; the filter was not changed
    /// (D3208, D3246).
    #[error("the position cannot be searched")]
    UnsupportedPosition,
    /// A material pattern is not valid (D3218).
    #[error("invalid pattern: {reason}")]
    InvalidPattern {
        /// What is wrong.
        reason: &'static str,
    },
    /// The range of move numbers of a material search is reversed or goes
    /// over 999 (D3216, D3258).
    #[error("invalid move range {min}..{max}")]
    InvalidMoveRange {
        /// First move number.
        min: u16,
        /// Last move number.
        max: u16,
    },
    /// The move list is empty (D3225).
    #[error("no moves specified")]
    EmptyMoveList,
    /// The first word of the move list is the wildcard `?` (D3225).
    #[error("the first move cannot be a wildcard")]
    FirstMoveIsWildcard,
    /// A word of the move list is not a move in SAN (D3226).
    #[error("not a move in SAN: {token:?}")]
    BadMoveToken {
        /// The word, clipped to 32 characters.
        token: String,
    },
    /// A word carries a check marker (`+` or `#`) that the check test of the
    /// search does not include, so it could never match (D3257).
    #[error("check test too low for {token:?}")]
    CheckTestTooLow {
        /// The word, clipped to 32 characters.
        token: String,
    },
}

impl SearchReport {
    pub(crate) fn note_unreadable(&mut self, n: u32) {
        self.unreadable_total += 1;
        if self.unreadable.len() < MAX_UNREADABLE {
            self.unreadable.push(n);
        }
    }
}

/// A name criterion with a cache of the answer per distinct name.
struct Criterion<'a> {
    kind: NameKind,
    pattern: NamePattern,
    titles: Option<(TitleSet, &'a dyn TitleProvider)>,
    /// 0 = not computed, 1 = no, 2 = yes, by name ID.
    cache: Vec<u8>,
}

impl<'a> Criterion<'a> {
    fn new(
        kind: NameKind,
        text: &str,
        titles: Option<(TitleSet, &'a dyn TitleProvider)>,
    ) -> Criterion<'a> {
        Criterion {
            kind,
            pattern: NamePattern::new(text),
            titles: titles.filter(|(set, _)| *set != TitleSet::all()),
            cache: Vec::new(),
        }
    }

    fn accepts(&mut self, names: &NameBase, id: u32) -> bool {
        if self.pattern.is_any() && self.titles.is_none() {
            return true;
        }
        if self.cache.is_empty() {
            let n = usize::try_from(names.num_names(self.kind)).unwrap_or(0);
            self.cache = vec![0; n];
        }
        let Some(slot) = usize::try_from(id).ok().and_then(|i| self.cache.get_mut(i)) else {
            return false;
        };
        if *slot == 0 {
            let name = names.get(self.kind, id).map(bytes_to_string);
            let ok = name.is_some_and(|name| {
                self.pattern.matches(&name)
                    && self
                        .titles
                        .is_none_or(|(set, provider)| set.accepts(provider.title(&name)))
            });
            *slot = if ok { 2 } else { 1 };
        }
        *slot == 2
    }
}

/// Everything prepared once per search.
struct Matcher<'a> {
    query: &'a HeaderSearch,
    stats: &'a PlayerStats,
    white: Criterion<'a>,
    black: Criterion<'a>,
    event: Criterion<'a>,
    site: Criterion<'a>,
    round: Criterion<'a>,
    date: MinMax<u32>,
    eco: Option<(u16, u16, bool)>,
    diff: MinMax<i32>,
    diff_flipped: MinMax<i32>,
    game_min: u32,
    game_max: u32,
    /// Phrases to find in the PGN text (empty ones dropped, folded if
    /// ignoring case).
    phrases: Vec<String>,
    write_options: WriteOptions,
}

impl<'a> Matcher<'a> {
    fn new(
        query: &'a HeaderSearch,
        stats: &'a PlayerStats,
        titles: Option<&'a dyn TitleProvider>,
        games: u32,
    ) -> Matcher<'a> {
        let diff = if query.elo_diff.min > query.elo_diff.max {
            MinMax::new(query.elo_diff.max, query.elo_diff.min)
        } else {
            query.elo_diff
        };
        let (game_min, game_max) = game_range(query.game_number, games);
        let phrases = query
            .pgn_text
            .iter()
            .filter(|p| !p.is_empty())
            .map(|p| {
                if query.ignore_case {
                    fold_case(p)
                } else {
                    p.clone()
                }
            })
            .collect();
        Matcher {
            query,
            stats,
            white: Criterion::new(
                NameKind::Player,
                &query.white,
                titles.map(|t| (query.white_titles, t)),
            ),
            black: Criterion::new(
                NameKind::Player,
                &query.black,
                titles.map(|t| (query.black_titles, t)),
            ),
            event: Criterion::new(NameKind::Event, &query.event, None),
            site: Criterion::new(NameKind::Site, &query.site, None),
            round: Criterion::new(NameKind::Round, &query.round, None),
            date: MinMax::new(query.date.min.to_scid_u32(), query.date.max.to_scid_u32()),
            eco: query.eco.map(|r| {
                (
                    r.min.to_scid_code(),
                    last_sub_code(r.max.to_scid_code()),
                    r.include_none,
                )
            }),
            diff,
            // Saturating: `diff` comes from saved searches and the UI, and
            // `i32::MIN` has no positive counterpart.
            diff_flipped: MinMax::new(diff.max.saturating_neg(), diff.min.saturating_neg()),
            game_min,
            game_max,
            phrases,
            write_options: WriteOptions::search_text(),
        }
    }

    /// Whether decoding the game is needed (rule 19(e)).
    fn needs_game(&self) -> bool {
        let q = self.query;
        !q.extra_tag.is_empty()
            || !self.phrases.is_empty()
            || q.game_end != GameEndKind::Any
            || q.pre_comment
            || q.post_comment
    }

    /// `matchGameHeader`. With `flipped`, White and Black criteria, Elo
    /// ranges, results and the Elo difference are swapped; the side to move
    /// at the end is not (D333).
    fn header_matches(&mut self, names: &NameBase, e: &IndexEntry, flipped: bool) -> bool {
        let q = self.query;
        if !q.results.accepts(e.result, flipped) {
            return false;
        }
        let half_moves = e.num_half_moves;
        if let Some(length) = q.length
            && !length.contains(half_moves)
        {
            return false;
        }
        let white_moves = half_moves.is_multiple_of(2);
        if (white_moves && !q.end_white_to_move) || (!white_moves && !q.end_black_to_move) {
            return false;
        }
        if !self.date.contains(e.date.to_scid_u32()) {
            return false;
        }
        let white_elo = if e.white_elo == 0 {
            self.stats.max_elo(e.white_id)
        } else {
            e.white_elo
        };
        let black_elo = if e.black_elo == 0 {
            self.stats.max_elo(e.black_id)
        } else {
            e.black_elo
        };
        let (white_range, black_range, diff) = if flipped {
            (q.black_elo, q.white_elo, self.diff_flipped)
        } else {
            (q.white_elo, q.black_elo, self.diff)
        };
        if !white_range.contains(white_elo)
            || !black_range.contains(black_elo)
            || !diff.contains(i32::from(white_elo) - i32::from(black_elo))
        {
            return false;
        }
        if let Some((min, max, include_none)) = self.eco {
            if e.eco_code == 0 {
                if !include_none {
                    return false;
                }
            } else if e.eco_code < min || e.eco_code > max {
                return false;
            }
        }
        if !self.event.accepts(names, e.event_id)
            || !self.site.accepts(names, e.site_id)
            || !self.round.accepts(names, e.round_id)
        {
            return false;
        }
        if flipped {
            self.black.accepts(names, e.white_id) && self.white.accepts(names, e.black_id)
        } else {
            self.white.accepts(names, e.white_id) && self.black.accepts(names, e.black_id)
        }
    }

    /// The tests that need the decoded game (rule 19(e), D337 to D339).
    fn game_matches(&self, game: &Game) -> bool {
        let q = self.query;
        if !q.extra_tag.is_empty() {
            let found = game
                .tags()
                .extras()
                .iter()
                .find(|(name, _)| *name == q.extra_tag);
            let Some((_, value)) = found else {
                return false;
            };
            let wanted = q.extra_tag_value.as_str();
            let ok = if wanted.is_empty() {
                true
            } else if wanted.starts_with(|c: char| c.is_ascii_digit()) {
                value == wanted
            } else {
                value.starts_with(wanted)
            };
            if !ok {
                return false;
            }
        }
        let last = game.mainline().last().unwrap_or_else(|| game.root());
        if q.game_end != GameEndKind::Any {
            let Ok(position) = game.position_at(last) else {
                return false;
            };
            let ended = match q.game_end {
                GameEndKind::Checkmate => position.is_checkmate(),
                GameEndKind::Stalemate => position.is_stalemate(),
                GameEndKind::Any => true,
            };
            if !ended {
                return false;
            }
        }
        if q.pre_comment && !visible_comment(game.initial_comment()) {
            return false;
        }
        if q.post_comment && !visible_comment(game.comment(last).ok().flatten()) {
            return false;
        }
        if !self.phrases.is_empty() {
            let Ok(text) = game_to_string(game, &self.write_options) else {
                return false;
            };
            let text = if q.ignore_case {
                fold_case(&text)
            } else {
                text
            };
            if !self.phrases.iter().all(|p| text.contains(p.as_str())) {
                return false;
            }
        }
        true
    }
}

/// A comment exists and is not hidden (`[...]`), rule 39.
fn visible_comment(comment: Option<&str>) -> bool {
    comment.is_some_and(|c| !(c.starts_with('[') && c.ends_with(']')))
}

/// Scid's `eco_LastSubCode` on the numeric code (see
/// [`Eco::last_sub_code`]); 0 and numbers beyond `E99z4` stay as they are.
fn last_sub_code(code: u16) -> u16 {
    Eco::from_scid_code(code).map_or(code, |eco| eco.last_sub_code().to_scid_code())
}

/// The game-number range of rule 32.
fn game_range(range: MinMax<i64>, games: u32) -> (u32, u32) {
    let n = i64::from(games);
    let fix = |v: i64| {
        let v = if v < 0 { v + n + 1 } else { v };
        u32::try_from(v.clamp(0, n)).unwrap_or(0)
    };
    let (a, b) = (fix(range.min), fix(range.max));
    (a.min(b), a.max(b))
}

/// Runs a header search over `db`, combining the result with `filter`
/// according to `query.op`.
///
/// The search works on a copy of the filter and replaces `filter` only when
/// it finishes; if `cancel` is set it stops and `filter` stays as it was
/// (D328, `interrupted = true`). Progress is reported to `progress` every
/// 5000 games or 100 ms, where `cancel` is also looked at.
///
/// `stats` gives the fallback Elo of players; `titles` is used for the title
/// restrictions and ignored when `None`. Games that cannot be read are
/// listed in [`SearchReport::unreadable`] and do not match.
///
/// ```no_run
/// use mychess_db::si4::Si4Database;
/// use mychess_db::{
///     CancelToken, Filter, HeaderSearch, NoProgress, PlayerStats, search_header,
/// };
///
/// let mut db = Si4Database::open("games/tactics".as_ref())?;
/// let stats = PlayerStats::from_database(&db);
/// let mut filter = Filter::new(db.len());
/// let query = HeaderSearch { white: "kasparov".into(), ..HeaderSearch::default() };
/// let report = search_header(
///     &mut db, &query, &mut filter, &stats, None, &mut NoProgress, &CancelToken::new(),
/// )?;
/// println!("{} / {}", report.matched, report.start_count);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn search_header<D: GameSource + ?Sized>(
    db: &mut D,
    query: &HeaderSearch,
    filter: &mut Filter,
    stats: &PlayerStats,
    titles: Option<&dyn TitleProvider>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<SearchReport, SearchError> {
    let games = db.len();
    if filter.len() != games {
        return Err(SearchError::FilterSize {
            filter: filter.len(),
            games,
        });
    }
    let started = Instant::now();
    let start_count = if query.op == FilterOp::And {
        filter.count()
    } else {
        games
    };
    let mut report = SearchReport {
        matched: filter.count(),
        start_count,
        elapsed: Duration::ZERO,
        interrupted: false,
        unreadable: Vec::new(),
        unreadable_total: 0,
    };
    if cancel.is_cancelled() {
        report.interrupted = true;
        report.elapsed = started.elapsed();
        return Ok(report);
    }

    let mut work = filter.clone();
    let mut op = query.op;
    if op == FilterOp::Reset {
        work.fill(1);
        op = FilterOp::And;
    }
    let mut matcher = Matcher::new(query, stats, titles, games);
    let needs_game = matcher.needs_game();
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);

    for n in 1..=games {
        if ticker.tick() {
            progress.progress(u64::from(n - 1), u64::from(games));
            if cancel.is_cancelled() {
                report.interrupted = true;
                break;
            }
        }
        let was_in = work.contains(n);
        match op {
            FilterOp::And if !was_in => continue,
            FilterOp::Or if was_in => continue,
            FilterOp::Or => {
                // Keeps the count right while the game is examined.
                work.set(n, 1);
            }
            _ => {}
        }
        if n < matcher.game_min || n > matcher.game_max {
            work.set(n, 0);
            continue;
        }
        let Ok(entry) = db.entry(n) else {
            work.set(n, 0);
            report.note_unreadable(n);
            continue;
        };
        if entry.length == 0 {
            work.set(n, 0);
            continue;
        }
        let mut matched = false;
        if matcher.query.flags.accepts(entry.flags) {
            matched = matcher.header_matches(db.names(), &entry, false);
            if !matched && query.ignore_colors {
                matched = matcher.header_matches(db.names(), &entry, true);
            }
        }
        if matched && needs_game {
            matched = if let Ok(game) = db.game(n) {
                matcher.game_matches(&game)
            } else {
                report.note_unreadable(n);
                false
            };
        }
        if matched {
            if !work.contains(n) {
                work.set(n, 1);
            }
        } else {
            work.set(n, 0);
        }
    }

    if !report.interrupted {
        progress.progress(u64::from(games), u64::from(games));
        *filter = work;
        report.matched = filter.count();
    }
    report.elapsed = started.elapsed();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_reads_scid_letters() {
        for yes in ["t", "Y", "yes", "j", "o", "s", "1"] {
            assert_eq!(Tri::from_scid_str(yes), Some(Tri::Yes), "{yes}");
        }
        for no in ["f", "N", "no", "0"] {
            assert_eq!(Tri::from_scid_str(no), Some(Tri::No), "{no}");
        }
        assert_eq!(Tri::from_scid_str("both"), Some(Tri::Both));
        assert_eq!(Tri::from_scid_str("2"), Some(Tri::Both));
        assert_eq!(Tri::from_scid_str("x"), None);
        assert_eq!(Tri::from_scid_str(""), None);
    }

    #[test]
    fn flag_filter_yes_no_both() {
        let mut f = FlagFilter::default();
        let with = GameFlags::TACTICS | GameFlags::CUSTOM3;
        assert!(f.accepts(GameFlags::empty()));
        f.set(GameFlags::TACTICS, Tri::Yes);
        assert!(f.accepts(with) && !f.accepts(GameFlags::empty()));
        f.set(GameFlags::CUSTOM3, Tri::No);
        assert!(!f.accepts(with) && f.accepts(GameFlags::TACTICS));
        f.set(GameFlags::CUSTOM3, Tri::Both);
        assert!(f.accepts(with));
        assert_eq!(f.get(GameFlags::TACTICS), Tri::Yes);
        assert_eq!(f.get(GameFlags::DELETE), Tri::Both);
    }

    #[test]
    fn last_sub_code_matches_scid_examples() {
        let code = |s: &str| s.parse::<Eco>().map_or(0, Eco::to_scid_code);
        assert_eq!(last_sub_code(code("B07")), code("B07z4"));
        assert_eq!(last_sub_code(code("B91a")), code("B91a4"));
        assert_eq!(last_sub_code(code("B91a2")), code("B91a2"));
        assert_eq!(last_sub_code(0), 0);
    }

    #[test]
    fn game_range_follows_rule_32() {
        let r = |a, b, n| game_range(MinMax::new(a, b), n);
        assert_eq!(r(1, -1, 100), (1, 100));
        assert_eq!(r(-10, -1, 100), (91, 100));
        assert_eq!(r(5, 3, 100), (3, 5));
        assert_eq!(r(0, 0, 100), (0, 0));
        assert_eq!(r(1, 500, 100), (1, 100));
        assert_eq!(r(-500, -1, 100), (0, 100));
    }

    #[test]
    fn dates_are_completed_like_the_dialog() {
        assert_eq!(complete_min_date("2000").to_string(), "2000.??.??");
        assert_eq!(complete_max_date("2000").to_string(), "2000.12.31");
        assert_eq!(complete_min_date("2000.05").to_string(), "2000.05.??");
        assert_eq!(complete_max_date("2000.05").to_string(), "2000.05.31");
        assert_eq!(complete_min_date("2000.01.01").to_string(), "2000.??.??");
        assert_eq!(complete_min_date("").to_string(), "????.??.??");
        assert_eq!(complete_max_date("").to_string(), "2047.12.31");
    }

    #[test]
    fn results_are_mirrored_when_flipped() {
        let only_white = ResultSet {
            white: true,
            draw: false,
            black: false,
            none: false,
        };
        assert!(only_white.accepts(GameResult::White, false));
        assert!(only_white.accepts(GameResult::Black, true));
        assert!(!only_white.accepts(GameResult::White, true));
        assert!(!only_white.accepts(GameResult::Unknown, false));
    }

    #[test]
    fn title_set_treats_no_title_as_none() {
        let set = TitleSet::GM | TitleSet::NONE;
        assert!(set.accepts(Some(Title::Gm)) && set.accepts(None));
        assert!(!set.accepts(Some(Title::Im)));
        assert_eq!(Title::from_scid_str("HGM"), Some(Title::Gm));
        assert_eq!(Title::from_scid_str("cim"), None);
    }
}
