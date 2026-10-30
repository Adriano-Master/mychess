// Derived from Scid vs PC 4.27, src/crosstab.cpp (`AddPlayer`, `AddResult`,
// `BestMode`, `AvgRating`, `PrintTable`). Copyright (c) 2001 Shane Hudson;
// Scid vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The computation of the table: players, games, points, performance, order
//! and the rows (rules T1 to T29 of the spec).

use std::collections::{HashMap, HashSet};

use mychess_core::{Color, GameResult, PartialDate};

use super::elo::{fide_category, opponent_elo, performance_rating, rating_change};
use super::select::{TournamentGame, TournamentGames};
use super::tiebreak;
use super::{
    ALL_PLAY_ALL_LARGE_OUTPUT, ALL_PLAY_ALL_LIMIT, Clash, Columns, Crosstable, CrosstableError,
    CrosstableFormat, CrosstableOptions, KnockoutMatch, KnockoutRound, MAX_PLAYERS, MAX_ROUNDS,
    Notice, Outcome, Performance, PlayerBioProvider, PlayerRow, ResultCounts, TableMode,
};
use crate::progress::{CHECK_INTERVAL, CancelToken, SEARCH_CHECK_EVERY, Ticker};

/// A counter of the data of the database as the `u32` of the public rows:
/// the accumulators are `u64` and saturate here, never wrap.
fn narrow(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// A player while the table is being built (`playerDataT`). Clashes keep the
/// opponent as the index in first-appearance order.
pub(super) struct Player {
    id: u32,
    pub(super) name: String,
    pub(super) elo: u32,
    // The counters are `u64` and only grow with `saturating_add`: the games
    // come from a file, so a huge tournament must not wrap (or panic).
    pub(super) score: u64,
    pub(super) game_count: u64,
    pub(super) tiebreak: u64,
    pub(super) won: u64,
    pub(super) lost: u64,
    pub(super) drawn: u64,
    pub(super) head_to_head: u64,
    opp_elo_count: u64,
    opp_elo_total: u64,
    opp_elo_score: u64,
    /// Every game of the player, in the order of the games.
    pub(super) clashes: Vec<Clash>,
    /// Indexes into `clashes` by opponent.
    by_opponent: HashMap<usize, Vec<usize>>,
    /// The last game of each round (index = round).
    round_clash: Vec<Option<usize>>,
    title: String,
    pub(super) country: String,
    birth: PartialDate,
}

struct Builder<'a> {
    options: &'a CrosstableOptions,
    bio: Option<&'a dyn PlayerBioProvider>,
    players: Vec<Player>,
    index: HashMap<u32, usize>,
    game_count: u64,
    max_clashes: u32,
    max_round: u32,
    results: ResultCounts,
    /// `FirstDate`: the smallest non-empty date, for the ages.
    first_date: PartialDate,
    /// Every game so far fits a knockout: Round `N.M` with `N` in
    /// `1..MAX_ROUNDS`.
    knockout_shape: bool,
    /// Per round (index = round): the players of the round and their only
    /// opponent. A player with two different opponents in a round rules a
    /// knockout out (`knockout_shape`).
    round_opponent: Vec<HashMap<usize, usize>>,
}

impl Builder<'_> {
    /// `AddPlayer`: `false` when the table is full.
    fn add_player(&mut self, id: u32, name: &str, elo: u32) -> bool {
        if let Some(&i) = self.index.get(&id) {
            if let Some(p) = self.players.get_mut(i) {
                p.elo = p.elo.max(elo);
            }
            return true;
        }
        if self.players.len() >= MAX_PLAYERS {
            return false;
        }
        let bio = self.bio.and_then(|provider| provider.bio(name));
        let (title, country, birth) = bio.map_or_else(
            || (String::new(), String::new(), PartialDate::UNKNOWN),
            |b| (b.title, b.country, b.birth),
        );
        self.index.insert(id, self.players.len());
        self.players.push(Player {
            id,
            name: name.to_owned(),
            elo,
            score: 0,
            game_count: 0,
            tiebreak: 0,
            won: 0,
            lost: 0,
            drawn: 0,
            head_to_head: 0,
            opp_elo_count: 0,
            opp_elo_total: 0,
            opp_elo_score: 0,
            clashes: Vec::new(),
            by_opponent: HashMap::new(),
            round_clash: vec![None; MAX_ROUNDS as usize],
            title,
            country,
            birth,
        });
        true
    }

    fn record(&mut self, player: usize, clash: Clash) {
        let Some(p) = self.players.get_mut(player) else {
            return;
        };
        let at = p.clashes.len();
        p.clashes.push(clash);
        let list = p.by_opponent.entry(clash.opponent).or_default();
        list.push(at);
        let same = u32::try_from(list.len()).unwrap_or(u32::MAX);
        self.max_clashes = self.max_clashes.max(same);
        if clash.round > 0 && clash.round < MAX_ROUNDS {
            if let Some(slot) = p.round_clash.get_mut(clash.round as usize) {
                *slot = Some(at);
            }
            self.max_round = self.max_round.max(clash.round);
        }
        p.game_count = p.game_count.saturating_add(1);
    }

    /// `AddResult`. Both players were added before.
    fn add_result(&mut self, game: &TournamentGame) {
        let (Some(&w), Some(&b)) = (self.index.get(&game.white), self.index.get(&game.black))
        else {
            return;
        };
        if w == b {
            return;
        }
        let (white_outcome, black_outcome) = match game.result {
            GameResult::White => (Outcome::Win, Outcome::Loss),
            GameResult::Black => (Outcome::Loss, Outcome::Win),
            GameResult::Draw => (Outcome::Draw, Outcome::Draw),
            _ => (Outcome::Unknown, Outcome::Unknown),
        };
        self.record(
            w,
            Clash {
                game: game.number,
                outcome: white_outcome,
                color: Color::White,
                round: game.round,
                opponent: b,
            },
        );
        self.record(
            b,
            Clash {
                game: game.number,
                outcome: black_outcome,
                color: Color::Black,
                round: game.round,
                opponent: w,
            },
        );
        self.account(w, b, game.result);
        self.game_count = self.game_count.saturating_add(1);
        let counter = match game.result {
            GameResult::White => &mut self.results.white,
            GameResult::Black => &mut self.results.black,
            GameResult::Draw => &mut self.results.draws,
            _ => &mut self.results.unknown,
        };
        *counter = counter.saturating_add(1);
        self.track_knockout(game, w, b);
        if self.first_date.is_unknown() {
            self.first_date = game.date;
        }
        if !game.date.is_unknown() && game.date < self.first_date {
            self.first_date = game.date;
        }
    }

    /// Keeps what `best_mode` needs to tell a knockout from a Swiss or a
    /// round robin that also numbers its boards (`round.board`).
    fn track_knockout(&mut self, game: &TournamentGame, w: usize, b: usize) {
        if !game.sub_round || game.round == 0 || game.round >= MAX_ROUNDS {
            self.knockout_shape = false;
            return;
        }
        if !self.knockout_shape {
            return;
        }
        let Some(pairs) = self.round_opponent.get_mut(game.round as usize) else {
            self.knockout_shape = false;
            return;
        };
        for (player, opponent) in [(w, b), (b, w)] {
            if *pairs.entry(player).or_insert(opponent) != opponent {
                self.knockout_shape = false;
            }
        }
    }

    /// A knockout (D3399): every Round is `N.M` with `N >= 1`, in each round
    /// a player meets a single opponent, and the players of round `r + 1` are
    /// a strict subset of those of round `r` (the losers leave).
    fn is_knockout(&self) -> bool {
        if !self.knockout_shape || self.game_count == 0 || self.max_round == 0 {
            return false;
        }
        let players = |round: u32| -> HashSet<usize> {
            self.round_opponent
                .get(round as usize)
                .map(|pairs| pairs.keys().copied().collect())
                .unwrap_or_default()
        };
        (1..self.max_round).all(|round| {
            let (this, next) = (players(round), players(round + 1));
            next.len() < this.len() && next.is_subset(&this)
        })
    }

    /// Points and the figures of the performance. The Elo of each player is
    /// the one known so far, as in the Scid (D3359 of the spec, as decided in
    /// D3395).
    fn account(&mut self, w: usize, b: usize, result: GameResult) {
        let win: u64 = if self.options.three_points { 6 } else { 2 };
        let draw: u64 = if self.options.three_points { 2 } else { 1 };
        let (we, be) = match (self.players.get(w), self.players.get(b)) {
            (Some(x), Some(y)) => (x.elo, y.elo),
            _ => return,
        };
        let decided = result != GameResult::Unknown;
        if let Some(p) = self.players.get_mut(w).filter(|_| decided && be > 0) {
            p.opp_elo_count = p.opp_elo_count.saturating_add(1);
            p.opp_elo_total = p
                .opp_elo_total
                .saturating_add(u64::from(opponent_elo(we, be)));
        }
        if let Some(p) = self.players.get_mut(b).filter(|_| decided && we > 0) {
            p.opp_elo_count = p.opp_elo_count.saturating_add(1);
            p.opp_elo_total = p
                .opp_elo_total
                .saturating_add(u64::from(opponent_elo(be, we)));
        }
        match result {
            GameResult::White => {
                if let Some(p) = self.players.get_mut(w) {
                    p.score = p.score.saturating_add(win);
                    if be > 0 {
                        p.opp_elo_score = p.opp_elo_score.saturating_add(2);
                    }
                }
            }
            GameResult::Black => {
                if let Some(p) = self.players.get_mut(b) {
                    p.score = p.score.saturating_add(win);
                    if we > 0 {
                        p.opp_elo_score = p.opp_elo_score.saturating_add(2);
                    }
                }
            }
            GameResult::Draw => {
                if let Some(p) = self.players.get_mut(w) {
                    p.score = p.score.saturating_add(draw);
                    if be > 0 {
                        p.opp_elo_score = p.opp_elo_score.saturating_add(1);
                    }
                }
                if let Some(p) = self.players.get_mut(b) {
                    p.score = p.score.saturating_add(draw);
                    if we > 0 {
                        p.opp_elo_score = p.opp_elo_score.saturating_add(1);
                    }
                }
            }
            _ => {}
        }
    }

    /// `BestMode`, plus the structural knockout rule of D3399.
    fn best_mode(&self) -> TableMode {
        if self.is_knockout() {
            return TableMode::Knockout;
        }
        let players = self.players.len() as u64;
        let games = self.game_count;
        if players <= 12 {
            return TableMode::AllPlayAll;
        }
        if players > 30 || games / players < 5 {
            return TableMode::Swiss;
        }
        let complete = players * (players - 1) / 2;
        if games < complete / 2 {
            TableMode::Swiss
        } else {
            TableMode::AllPlayAll
        }
    }

    /// `AvgRating`: the mean of the nonzero Elos, truncated.
    fn average_elo(&self) -> u32 {
        let rated: Vec<u64> = self
            .players
            .iter()
            .map(|p| u64::from(p.elo))
            .filter(|&e| e > 0)
            .collect();
        let count = rated.len() as u64;
        let total = rated.iter().fold(0u64, |a, &e| a.saturating_add(e));
        narrow(total.checked_div(count).unwrap_or(0))
    }
}

fn performance_of(p: &Player) -> Option<Performance> {
    if p.opp_elo_count == 0 {
        return None;
    }
    let count = p.opp_elo_count;
    let average = narrow(p.opp_elo_total / count);
    let percentage = narrow(p.opp_elo_score.saturating_mul(50).saturating_add(count / 2) / count);
    let rating = performance_rating(average, percentage);
    if rating == 0 || rating >= 5000 {
        return None;
    }
    Some(Performance {
        average_opponent: average,
        percentage,
        rating,
        change: (p.elo > 0).then(|| rating_change(p.elo, average, percentage, narrow(count))),
    })
}

/// A clash seen from the table: the opponent is a sorted position.
fn shown(clash: &Clash, inverted: &[usize]) -> Clash {
    Clash {
        opponent: inverted.get(clash.opponent).copied().unwrap_or(0),
        ..*clash
    }
}

impl Crosstable {
    /// Builds the table from the games of a tournament, in the order of their
    /// numbers. `bio` gives title, country and birth date; without it those
    /// columns do not exist.
    ///
    /// Games with the same player on both sides are ignored. When a game
    /// would add the 1001st player it is dropped and [`Notice::PlayerLimit`]
    /// is added once. The counters saturate instead of wrapping, however many
    /// games there are.
    ///
    /// # Errors
    ///
    /// [`CrosstableError::NoTable`] when there are fewer than two players.
    pub fn from_games(
        data: &TournamentGames,
        options: &CrosstableOptions,
        bio: Option<&dyn PlayerBioProvider>,
    ) -> Result<Crosstable, CrosstableError> {
        Crosstable::from_games_cancellable(data, options, bio, &CancelToken::new())
    }

    /// [`Crosstable::from_games`] that looks at `cancel` every few thousand
    /// games.
    ///
    /// # Errors
    ///
    /// [`CrosstableError::Cancelled`] when `cancel` is set, and the errors of
    /// [`Crosstable::from_games`].
    // One pass over the Scid's `PrintTable` set-up; splitting it would only
    // scatter the shared state.
    #[allow(clippy::too_many_lines)]
    pub fn from_games_cancellable(
        data: &TournamentGames,
        options: &CrosstableOptions,
        bio: Option<&dyn PlayerBioProvider>,
        cancel: &CancelToken,
    ) -> Result<Crosstable, CrosstableError> {
        let mut builder = Builder {
            options,
            bio,
            players: Vec::new(),
            index: HashMap::new(),
            game_count: 0,
            max_clashes: 0,
            max_round: 0,
            results: ResultCounts::default(),
            first_date: PartialDate::UNKNOWN,
            knockout_shape: true,
            round_opponent: vec![HashMap::new(); MAX_ROUNDS as usize],
        };
        let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
        let mut first_seen = data.reference.date;
        let mut last_seen = data.reference.date;
        let mut full = false;
        for game in &data.games {
            if ticker.tick() && cancel.is_cancelled() {
                return Err(CrosstableError::Cancelled);
            }
            if game.white == game.black {
                continue;
            }
            let name = |id: u32| data.names.get(&id).map_or("", String::as_str);
            if !builder.add_player(game.white, name(game.white), u32::from(game.white_elo))
                || !builder.add_player(game.black, name(game.black), u32::from(game.black_elo))
            {
                full = true;
                continue;
            }
            builder.add_result(game);
            first_seen = first_seen.min(game.date);
            last_seen = last_seen.max(game.date);
        }
        if builder.players.len() < 2 {
            return Err(CrosstableError::NoTable);
        }

        let mode = match options.format {
            CrosstableFormat::Auto => builder.best_mode(),
            CrosstableFormat::AllPlayAll => TableMode::AllPlayAll,
            CrosstableFormat::Swiss => TableMode::Swiss,
            CrosstableFormat::Knockout => TableMode::Knockout,
        };
        tiebreak::compute(&mut builder.players, mode);

        // The sort is stable: ties keep the order of first appearance.
        let mut sorted: Vec<usize> = (0..builder.players.len()).collect();
        sorted.sort_by(
            |&a, &b| match (builder.players.get(a), builder.players.get(b)) {
                (Some(pa), Some(pb)) => tiebreak::compare(options, pa, pb),
                _ => std::cmp::Ordering::Equal,
            },
        );
        let mut inverted = vec![0usize; sorted.len()];
        for (position, &index) in sorted.iter().enumerate() {
            if let Some(slot) = inverted.get_mut(index) {
                *slot = position;
            }
        }

        let total_players = builder.players.len();
        let limit = if mode == TableMode::AllPlayAll {
            total_players.min(ALL_PLAY_ALL_LIMIT)
        } else {
            total_players
        };
        let mut notices = Vec::new();
        if data.truncated {
            notices.push(Notice::GameLimit);
        }
        if data.unreadable > 0 {
            notices.push(Notice::Unreadable(data.unreadable));
        }
        if full {
            notices.push(Notice::PlayerLimit);
        } else if mode == TableMode::AllPlayAll && total_players > ALL_PLAY_ALL_LIMIT {
            notices.push(Notice::TooManyPlayers);
        }
        let max_clashes = builder.max_clashes;
        if mode == TableMode::AllPlayAll {
            let columns = if limit == 2 { 1 } else { limit };
            let cells = limit
                .saturating_mul(columns)
                .saturating_mul(max_clashes.max(1) as usize);
            if cells > ALL_PLAY_ALL_LARGE_OUTPUT {
                notices.push(Notice::LargeOutput);
            }
        }

        let longest_name = builder
            .players
            .iter()
            .map(|p| p.name.chars().count())
            .max()
            .unwrap_or(0);
        let any = |f: &dyn Fn(&Player) -> bool| builder.players.iter().any(f);
        let columns = Columns {
            ratings: options.show_ratings && any(&|p| p.elo > 0),
            titles: options.show_titles && any(&|p| !p.title.is_empty()),
            countries: options.show_countries && any(&|p| !p.country.is_empty()),
            ages: options.show_ages && any(&|p| !p.birth.is_unknown()),
            tiebreaks: options.show_tiebreaks && mode != TableMode::Knockout,
            tallies: options.show_tallies,
        };
        let number_width = if limit > 99 { 3 } else { 2 };
        let max_round = builder.max_round;
        let line_width = line_width(
            mode,
            columns,
            longest_name,
            number_width,
            limit,
            max_clashes as usize,
            max_round as usize,
            options.swiss_colors,
        );

        let average_elo = builder.average_elo();
        let rows: Vec<PlayerRow> = sorted
            .iter()
            .take(limit)
            .enumerate()
            .filter_map(|(position, &index)| {
                let p = builder.players.get(index)?;
                let performance = performance_of(p);
                let versus = if mode == TableMode::AllPlayAll {
                    sorted
                        .iter()
                        .take(limit)
                        .enumerate()
                        .map(|(column, opponent)| {
                            p.by_opponent
                                .get(opponent)
                                .map(|list| {
                                    list.iter()
                                        .filter_map(|&c| p.clashes.get(c))
                                        .map(|c| Clash {
                                            opponent: column,
                                            ..*c
                                        })
                                        .collect()
                                })
                                .unwrap_or_default()
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let by_round = if mode == TableMode::Swiss {
                    (1..=max_round as usize)
                        .map(|round| {
                            p.round_clash
                                .get(round)
                                .copied()
                                .flatten()
                                .and_then(|c| p.clashes.get(c))
                                .map(|c| shown(c, &inverted))
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let age = (!p.birth.is_unknown())
                    .then(|| {
                        let year = |d: PartialDate| i32::from(d.year().unwrap_or(0));
                        let month = |d: PartialDate| d.month().unwrap_or(0);
                        let mut age = year(builder.first_date) - year(p.birth);
                        if month(p.birth) > month(builder.first_date) {
                            age -= 1;
                        }
                        age
                    })
                    .and_then(|a| u32::try_from(a).ok())
                    .filter(|&a| a > 0);
                Some(PlayerRow {
                    rank: position + 1,
                    player: p.id,
                    name: p.name.clone(),
                    elo: p.elo,
                    title: p.title.clone(),
                    country: p.country.clone(),
                    age,
                    score_half: narrow(p.score),
                    games: narrow(p.game_count),
                    tiebreak: narrow(p.tiebreak),
                    head_to_head: narrow(p.head_to_head),
                    won: narrow(p.won),
                    lost: narrow(p.lost),
                    drawn: narrow(p.drawn),
                    rated_games: narrow(p.opp_elo_count),
                    performance,
                    versus,
                    by_round,
                })
            })
            .collect();

        let knockout = if mode == TableMode::Knockout {
            knockout_rounds(&builder, &sorted, &inverted)
        } else {
            Vec::new()
        };
        let category = fide_category(average_elo);
        Ok(Crosstable {
            event: data.reference.event.clone(),
            site: data.reference.site.clone(),
            first_date: first_seen,
            last_date: last_seen,
            mode,
            options: options.clone(),
            rows,
            total_players,
            game_count: narrow(builder.game_count),
            results: builder.results,
            max_clashes,
            max_round,
            average_elo,
            category,
            columns,
            knockout,
            notices,
            longest_name,
            number_width,
            line_width,
        })
    }
}

/// `LineWidth` of `PrintTable`.
#[allow(clippy::too_many_arguments)]
fn line_width(
    mode: TableMode,
    columns: Columns,
    longest_name: usize,
    number_width: usize,
    limit: usize,
    max_clashes: usize,
    max_round: usize,
    swiss_colors: bool,
) -> usize {
    let mut width = longest_name;
    if columns.ratings {
        width += 16;
    }
    if columns.titles || columns.countries || columns.ages {
        width += 4 * usize::from(columns.titles)
            + 4 * usize::from(columns.countries)
            + 4 * usize::from(columns.ages);
    }
    match mode {
        TableMode::Swiss => {
            width += 16 + number_width;
            width += (number_width + if swiss_colors { 3 } else { 2 }) * max_round;
            if columns.tiebreaks {
                width += 5;
            }
        }
        TableMode::AllPlayAll => {
            width += 16 + number_width;
            width += if limit == 2 {
                max_clashes + 1
            } else {
                limit * (max_clashes + 1)
            };
            if columns.tiebreaks {
                width += 7;
            }
        }
        TableMode::Knockout => {
            width = longest_name * 2 + 17 + max_clashes;
            if columns.ratings {
                width += 10;
            }
            width += 8
                * (usize::from(columns.titles)
                    + usize::from(columns.countries)
                    + usize::from(columns.ages));
        }
    }
    width
}

/// The blocks of a knockout table (`PrintKnockout`): a match once, with the
/// player who had White in their first game on the left (T26).
fn knockout_rounds(
    builder: &Builder<'_>,
    sorted: &[usize],
    inverted: &[usize],
) -> Vec<KnockoutRound> {
    let mut rounds = Vec::new();
    for round in 1..=builder.max_round {
        let mut printed = vec![false; builder.players.len()];
        let mut matches = Vec::new();
        for &index in sorted {
            let Some(p) = builder.players.get(index) else {
                continue;
            };
            let Some(last) = p
                .round_clash
                .get(round as usize)
                .copied()
                .flatten()
                .and_then(|c| p.clashes.get(c))
            else {
                continue;
            };
            if printed.get(index).copied().unwrap_or(true) {
                continue;
            }
            let opponent = last.opponent;
            let Some(list) = p.by_opponent.get(&opponent) else {
                continue;
            };
            let first_is_black = list
                .first()
                .and_then(|&c| p.clashes.get(c))
                .is_some_and(|c| c.color == Color::Black);
            if first_is_black {
                continue;
            }
            let games: Vec<Clash> = list
                .iter()
                .filter_map(|&c| p.clashes.get(c))
                .filter(|c| c.round == round)
                .map(|c| shown(c, inverted))
                .collect();
            let score_half = games.iter().map(|c| c.outcome.knockout_half_points()).sum();
            matches.push(KnockoutMatch {
                first: inverted.get(index).copied().unwrap_or(0),
                second: inverted.get(opponent).copied().unwrap_or(0),
                games,
                score_half,
            });
            if let Some(f) = printed.get_mut(index) {
                *f = true;
            }
            if let Some(f) = printed.get_mut(opponent) {
                *f = true;
            }
        }
        rounds.push(KnockoutRound { round, matches });
    }
    rounds
}
