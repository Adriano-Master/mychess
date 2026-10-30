//! The state the analysis window shows for one execution (spec M4-4, D438)
//! and how a report of the engine merges into it (rules 1-3, 16).
//!
//! The merge is a plain function of the view and the report, with no access
//! to the application state, so every rule has a unit test. The text of a
//! line is formatted once, when the line is stored (or when the notation
//! options change), so drawing it costs nothing.

use std::collections::VecDeque;
use std::sync::Arc;

use mychess_core::{Color, Move, Position};
use mychess_engine::{BestMove, Bound, LogEntry, Score, SearchId, SearchInfo, Wdl};

use super::format::{NO_MOVES_MATE_CP, NumberStyle, format_pv_limited};
use crate::engines::{AnalysisTarget, RunId};
use crate::prefs::AnalysisPrefs;

/// How many sent searches remember their position while waiting to start.
const MAX_TAGS: usize = 16;

/// The longest history of a single-line analysis (rule 3).
pub const MAX_HISTORY: usize = 200;

/// How the lines are shown (D449).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Display {
    /// Only a hint to click: the lines are not shown (to train without
    /// seeing the engine).
    Hidden,
    /// One row per line; a long line scrolls sideways.
    #[default]
    NoWrap,
    /// A long line wraps.
    Wrap,
}

impl Display {
    /// The next mode of the middle click: hidden, then no wrap, then wrap.
    #[must_use]
    pub fn next(self) -> Display {
        match self {
            Display::Hidden => Display::NoWrap,
            Display::NoWrap => Display::Wrap,
            Display::Wrap => Display::Hidden,
        }
    }
}

/// How the text of the lines is built; a change of either value formats the
/// lines again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FormatOptions {
    /// Move numbers with or without a space.
    pub numbers: NumberStyle,
    /// Longest PV, in plies (`0`: unlimited). A mate line is never cut.
    pub max_ply: usize,
}

/// A position that has no moves (D450).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    /// The side to move is checkmated.
    Checkmate,
    /// The side to move has no move and is not in check.
    Stalemate,
}

/// One line of the analysis (a MultiPV line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PvLine {
    /// The line number, from 1 (the best one).
    pub multipv: u8,
    /// The depth that produced it.
    pub depth: u32,
    /// The selective depth, if the engine sent it.
    pub seldepth: Option<u32>,
    /// The score, from White's point of view.
    pub score: Score,
    /// Whether the score is exact or a bound.
    pub bound: Bound,
    /// The win/draw/loss expectation, if the engine sends it.
    pub wdl: Option<Wdl>,
    /// The moves, validated against the position.
    pub pv: Vec<Move>,
    /// The time spent when it was reported, in milliseconds.
    pub time_ms: u64,
    /// The moves in SAN with their numbers, cut at the PV limit.
    pub text: String,
}

/// A line of the history of a single-line analysis (rule 3): what the
/// engine believed at each depth that changed its mind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryLine {
    /// The depth.
    pub depth: u32,
    /// The score, from White's point of view.
    pub score: Score,
    /// Whether the score is exact or a bound.
    pub bound: Bound,
    /// The moves.
    pub pv: Vec<Move>,
    /// The time spent, in milliseconds.
    pub time_ms: u64,
    /// The moves in SAN with their numbers.
    pub text: String,
}

/// The numbers of the extra information line (rule 4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchStats {
    /// The depth of line 1.
    pub depth: u32,
    /// The selective depth of line 1.
    pub seldepth: Option<u32>,
    /// The time spent, in milliseconds.
    pub time_ms: u64,
    /// The nodes searched.
    pub nodes: u64,
    /// The speed the engine reports.
    pub nps: Option<u64>,
    /// The hash table fill, in per mille (UCI only).
    pub hashfull_permille: Option<u16>,
    /// The CPU load, in per mille (UCI only).
    pub cpuload_permille: Option<u16>,
    /// Tablebase hits.
    pub tbhits: Option<u64>,
    /// The move being searched, in SAN.
    pub currmove: Option<String>,
    /// Its number in the list of moves.
    pub currmove_number: Option<u32>,
    /// The largest `currmove_number` seen in this position.
    pub max_move_number: u32,
}

/// The position an analysis is locked on (D444). Plain data: the front-end
/// writes the description (file, game number, move, variation depth).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedPosition {
    /// The moves from the start of the game to the position.
    pub target: Arc<AnalysisTarget>,
    /// The move number of the position (`12` in `12... e5`).
    pub move_number: u32,
    /// The side to move.
    pub side: Color,
    /// The last move leading to the position, in SAN; `None` at the start.
    pub last_move: Option<String>,
    /// How many variations deep the position is (0 on the main line).
    pub variation_depth: u32,
    /// The name of the file the game came from.
    pub source: Option<String>,
    /// The number of the game in that file, from 1.
    pub game_number: Option<usize>,
}

/// What the window shows for one execution.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisView {
    /// The execution.
    pub run: RunId,
    /// The lines, ordered by [`PvLine::multipv`]. Line 1 is the best one.
    pub lines: Vec<PvLine>,
    /// The numbers of the extra information line.
    pub stats: SearchStats,
    /// The history of a single-line analysis, oldest first.
    pub history: Vec<HistoryLine>,
    /// How many lines the engine is asked for.
    pub multipv: u8,
    /// The locked position, if any.
    pub lock: Option<LockedPosition>,
    /// The moves excluded from the search, in the position shown.
    pub exclusion: Vec<Move>,
    /// The same moves in SAN (for the dialog and the tooltip).
    pub exclusion_san: Vec<String>,
    /// The exclusion dialog is open.
    pub exclude_open: bool,
    /// Show the extra information line.
    pub show_info: bool,
    /// How the lines are shown.
    pub display: Display,
    /// The log is open.
    pub log_open: bool,
    /// The entries of the log read so far (oldest first).
    pub log: Vec<LogEntry>,
    /// Only the entries that contain this text are shown.
    pub log_filter: String,
    /// The search the lines belong to; reports of any other are dropped.
    pub search: Option<SearchId>,
    /// The search ended (its best move arrived).
    pub finished: bool,
    /// The best move of the finished search.
    pub best_move: Option<BestMove>,
    /// The position has no moves (D450); no search is sent.
    pub terminal: Option<Terminal>,
    /// The engine is being restarted (low priority switch, D446).
    pub restarting: bool,
    /// The moves the engine may search (the legal moves minus
    /// `exclusion`); empty when nothing is excluded.
    pub(crate) search_moves: Vec<Move>,
    /// The position the exclusion was made for.
    pub(crate) exclusion_target: Option<Arc<AnalysisTarget>>,
    target: Option<Arc<AnalysisTarget>>,
    position: Option<Position>,
    synthetic: Option<Score>,
    format: FormatOptions,
    last_depth: u32,
    last_history_pv: Vec<Move>,
    pub(crate) log_next: u64,
    pub(crate) log_in_flight: bool,
    /// A search was sent and its end has not been seen (it may not even have
    /// started): a `Stop` is due before the engine is left idle (D780).
    pub(crate) search_sent: bool,
    /// The positions of the searches sent, by tag (D780).
    tag_targets: VecDeque<(u64, Arc<AnalysisTarget>)>,
    last_tag: u64,
    /// The search announced by the engine, with its tag and the position
    /// that tag was given to (`None`: unknown).
    announced: Option<Announced>,
    /// The tag of the search the lines belong to.
    started_tag: Option<u64>,
    /// Counts the changes of `lines`, so a cache can tell that a line was
    /// replaced even when its depth, score and first move did not change.
    revision: u64,
}

/// The position of a search that starts, as the engine's label says (D780).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Binding {
    /// The engine sent no label for this search.
    Missing,
    /// A label this view does not know (too old, or never given).
    Unknown,
    /// The position the request was sent for.
    Target(Arc<AnalysisTarget>),
}

#[derive(Debug, Clone, PartialEq)]
struct Announced {
    search: SearchId,
    tag: u64,
    target: Option<Arc<AnalysisTarget>>,
}

impl AnalysisView {
    /// A view of `run` that asks the engine for `multipv` lines and starts
    /// with the display preferences of `prefs`.
    pub fn new(run: RunId, multipv: u8, prefs: &AnalysisPrefs) -> AnalysisView {
        AnalysisView {
            run,
            lines: Vec::new(),
            stats: SearchStats::default(),
            history: Vec::new(),
            multipv: multipv.max(1),
            lock: None,
            exclusion: Vec::new(),
            exclusion_san: Vec::new(),
            exclude_open: false,
            show_info: prefs.show_info,
            display: if prefs.wrap_pv {
                Display::Wrap
            } else {
                Display::NoWrap
            },
            log_open: false,
            log: Vec::new(),
            log_filter: String::new(),
            search: None,
            finished: false,
            best_move: None,
            terminal: None,
            restarting: false,
            search_moves: Vec::new(),
            exclusion_target: None,
            target: None,
            position: None,
            synthetic: None,
            format: FormatOptions::default(),
            last_depth: 0,
            last_history_pv: Vec::new(),
            log_next: 0,
            log_in_flight: false,
            search_sent: false,
            tag_targets: VecDeque::new(),
            last_tag: 0,
            announced: None,
            started_tag: None,
            revision: 0,
        }
    }

    /// The view of the execution that replaces this one after a restart
    /// (D446): the settings stay (display, information, exclusion, number of
    /// lines, filter), what belonged to the old engine and search goes.
    pub(crate) fn for_new_run(mut self, run: RunId) -> AnalysisView {
        self.run = run;
        self.clear_lines();
        self.stats = SearchStats::default();
        self.search = None;
        self.finished = false;
        self.best_move = None;
        self.terminal = None;
        self.synthetic = None;
        self.target = None;
        self.position = None;
        self.restarting = false;
        self.lock = None;
        self.log.clear();
        self.log_next = 0;
        self.log_in_flight = false;
        self.search_sent = false;
        self.tag_targets.clear();
        self.announced = None;
        self.started_tag = None;
        self
    }

    /// Labels a search about to be sent for `target`; the engine gives the
    /// label back when that very search starts (D780).
    pub(crate) fn next_tag(&mut self, target: &Arc<AnalysisTarget>) -> u64 {
        self.last_tag += 1;
        self.tag_targets
            .push_back((self.last_tag, Arc::clone(target)));
        if self.tag_targets.len() > MAX_TAGS {
            self.tag_targets.pop_front();
        }
        self.search_sent = true;
        self.last_tag
    }

    /// The engine said that the request labelled `tag` starts as `search`.
    pub(crate) fn announce(&mut self, search: SearchId, tag: u64) {
        let target = self
            .tag_targets
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, target)| Arc::clone(target));
        self.announced = Some(Announced {
            search,
            tag,
            target,
        });
    }

    /// What the announcement of `search` says about its position.
    pub(crate) fn take_announced(&mut self, search: SearchId) -> Binding {
        self.started_tag = None;
        let Some(announced) = self.announced.take().filter(|a| a.search == search) else {
            return Binding::Missing;
        };
        self.started_tag = Some(announced.tag);
        announced.target.map_or(Binding::Unknown, Binding::Target)
    }

    /// The position the lines are about.
    pub fn position(&self) -> Option<&Position> {
        self.position.as_ref()
    }

    /// The moves to the position the lines are about.
    pub fn target(&self) -> Option<&Arc<AnalysisTarget>> {
        self.target.as_ref()
    }

    /// The score of line 1, or the fixed score of a position without moves
    /// (D450: `+327.00` or `-327.00` for a mate, `0.00` for a stalemate).
    pub fn main_score(&self) -> Option<(Score, Bound)> {
        self.synthetic
            .map(|s| (s, Bound::Exact))
            .or_else(|| self.lines.first().map(|l| (l.score, l.bound)))
    }

    /// The entries of the log that match the filter (ASCII letters match in
    /// either case; nothing is allocated per entry, as this runs in `view`).
    pub fn filtered_log(&self) -> impl Iterator<Item = &LogEntry> {
        let filter = self.log_filter.as_bytes();
        self.log.iter().filter(move |e| {
            filter.is_empty()
                || e.text
                    .as_bytes()
                    .windows(filter.len())
                    .any(|w| w.eq_ignore_ascii_case(filter))
        })
    }

    /// Whether `target` is the position the lines are about.
    pub(crate) fn is_about(&self, target: &Arc<AnalysisTarget>) -> bool {
        self.target
            .as_ref()
            .is_some_and(|t| Arc::ptr_eq(t, target) || **t == **target)
    }

    /// A search started: the lines of another position are forgotten; those
    /// of the same one stay until the engine replaces them. With an unknown
    /// `target` nothing can be trusted: the lines go and the reports of this
    /// search are dropped (they cannot be checked against a position).
    pub(crate) fn begin_search(&mut self, id: SearchId, target: Option<Arc<AnalysisTarget>>) {
        let same = target.as_ref().is_some_and(|t| self.is_about(t));
        if !same {
            self.clear_lines();
            self.stats = SearchStats::default();
            self.target = target;
            self.position = self.target.as_deref().and_then(position_of);
            self.terminal = None;
            self.synthetic = None;
        }
        self.search = Some(id);
        self.finished = false;
        self.best_move = None;
    }

    /// How many times `lines` changed (wraps); for caches that must notice a
    /// changed tail of a PV (spec M4-5, D829).
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Forgets the lines and the history (a new position, an unlock).
    pub(crate) fn clear_lines(&mut self) {
        self.lines.clear();
        self.bump_revision();
        self.history.clear();
        self.last_depth = 0;
        self.last_history_pv.clear();
    }

    /// The position has no moves: nothing is searched and the score is
    /// fixed (D450).
    pub(crate) fn set_terminal(&mut self, target: &Arc<AnalysisTarget>, position: &Position) {
        self.clear_lines();
        self.stats = SearchStats::default();
        self.search = None;
        self.finished = true;
        self.search_sent = false;
        self.best_move = None;
        self.target = Some(Arc::clone(target));
        self.position = Some(position.clone());
        let (terminal, score) = if position.is_checkmate() {
            let loss = if position.turn() == Color::White {
                -NO_MOVES_MATE_CP
            } else {
                NO_MOVES_MATE_CP
            };
            (Terminal::Checkmate, loss)
        } else {
            (Terminal::Stalemate, 0)
        };
        self.terminal = Some(terminal);
        self.synthetic = Some(Score::Cp(score));
    }

    /// A real search is about to be sent: the fixed score goes away.
    pub(crate) fn clear_terminal(&mut self) {
        self.terminal = None;
        self.synthetic = None;
    }

    /// Sets the excluded moves of `target` (`search_moves` is what the
    /// engine may search).
    pub(crate) fn set_exclusion(
        &mut self,
        target: &Arc<AnalysisTarget>,
        excluded: Vec<Move>,
        san: Vec<String>,
        search_moves: Vec<Move>,
    ) {
        self.exclusion = excluded;
        self.exclusion_san = san;
        self.search_moves = search_moves;
        self.exclusion_target = Some(Arc::clone(target));
    }

    /// Forgets the exclusion.
    pub(crate) fn clear_exclusion(&mut self) {
        self.exclusion.clear();
        self.exclusion_san.clear();
        self.search_moves.clear();
        self.exclusion_target = None;
        self.exclude_open = false;
    }

    /// Shortens the list of lines to `n` and clears the history if the
    /// analysis became a single line (rule 2).
    pub(crate) fn set_multipv(&mut self, n: u8) {
        let n = n.max(1);
        self.multipv = n;
        self.lines.truncate(usize::from(n));
        self.bump_revision();
        if n == 1 {
            self.history.clear();
            self.last_history_pv.clear();
        }
    }

    /// The search ended.
    pub(crate) fn finish(&mut self, best: BestMove) {
        if self.search == Some(best.search) {
            // Only the end of the last search sent settles the engine.
            if self.started_tag.is_none_or(|t| t == self.last_tag) {
                self.search_sent = false;
            }
            self.finished = true;
            self.best_move = Some(best);
        }
    }

    /// Formats the text of the lines again when `options` changed.
    pub(crate) fn reformat(&mut self, options: FormatOptions) {
        if self.format == options {
            return;
        }
        self.format = options;
        let Some(position) = self.position.as_ref() else {
            return;
        };
        for line in &mut self.lines {
            line.text = line_text(position, &line.pv, line.score, options);
        }
        for line in &mut self.history {
            line.text = line_text(position, &line.pv, line.score, options);
        }
    }

    /// Merges one report of the engine (rule 1).
    ///
    /// A report of another search is dropped. Every report updates the
    /// numbers it carries; only one with a score **and** a non-empty PV
    /// replaces a line. A line number above the configured count is
    /// dropped, and so is a PV whose first move is not playable from the
    /// position (a report that crossed a change of position).
    pub(crate) fn apply_info(&mut self, info: SearchInfo, options: FormatOptions) {
        if self.search != Some(info.search) {
            return;
        }
        let Some(position) = self.position.as_ref() else {
            return;
        };
        self.format = options;
        merge_stats(&mut self.stats, &info, position);
        let (Some((score, bound)), Some(&first)) = (info.score, info.pv.first()) else {
            return;
        };
        let idx = usize::from(info.multipv.max(1)) - 1;
        if idx >= usize::from(self.multipv) || !position.is_legal(first) {
            return;
        }
        let multipv = info.multipv.max(1);
        let previous_depth = self
            .lines
            .iter()
            .find(|l| l.multipv == multipv)
            .map_or(0, |l| l.depth);
        let depth = info.depth.unwrap_or(previous_depth);
        let time_ms = info.time_ms.unwrap_or(self.stats.time_ms);
        let text = line_text(position, &info.pv, score, options);
        let line = PvLine {
            multipv,
            depth,
            seldepth: info.seldepth,
            score,
            bound,
            wdl: info.wdl,
            pv: info.pv,
            time_ms,
            text,
        };
        if multipv == 1 {
            self.stats.depth = depth;
            self.stats.seldepth = info.seldepth;
            if self.multipv == 1 {
                self.push_history(&line);
            }
        }
        self.bump_revision();
        match self.lines.binary_search_by_key(&multipv, |l| l.multipv) {
            Ok(at) => self.lines[at] = line,
            Err(at) => self.lines.insert(at, line),
        }
    }

    /// Rule 3: a new line of the history when the PV changed; the history
    /// starts again when the depth goes down (a new search) or is 0.
    fn push_history(&mut self, line: &PvLine) {
        if line.depth < self.last_depth || line.depth == 0 {
            self.history.clear();
            self.last_history_pv.clear();
        }
        self.last_depth = line.depth;
        if self.last_history_pv == line.pv {
            return;
        }
        self.last_history_pv.clone_from(&line.pv);
        self.history.push(HistoryLine {
            depth: line.depth,
            score: line.score,
            bound: line.bound,
            pv: line.pv.clone(),
            time_ms: line.time_ms,
            text: line.text.clone(),
        });
        if self.history.len() > MAX_HISTORY {
            let excess = self.history.len() - MAX_HISTORY;
            self.history.drain(..excess);
        }
    }
}

/// The text of a PV: cut at the limit unless it ends in a mate
/// (`uci.tcl` 194-196).
fn line_text(position: &Position, pv: &[Move], score: Score, options: FormatOptions) -> String {
    let limit = if matches!(score, Score::Mate(_)) {
        0
    } else {
        options.max_ply
    };
    format_pv_limited(position, pv, options.numbers, limit)
}

/// The numbers every report carries: counters from any report, the depth of
/// line 1 only with a line (so a `currmove` report cannot lower it).
fn merge_stats(stats: &mut SearchStats, info: &SearchInfo, position: &Position) {
    if let Some(nodes) = info.nodes {
        stats.nodes = nodes;
    }
    if let Some(time) = info.time_ms {
        stats.time_ms = time;
    }
    if info.nps.is_some() {
        stats.nps = info.nps;
    }
    if info.hashfull_permille.is_some() {
        stats.hashfull_permille = info.hashfull_permille;
    }
    if info.cpuload_permille.is_some() {
        stats.cpuload_permille = info.cpuload_permille;
    }
    if info.tbhits.is_some() {
        stats.tbhits = info.tbhits;
    }
    if let Some(mv) = info.currmove {
        stats.currmove = position.san(mv).ok();
    }
    if let Some(number) = info.currmovenumber {
        stats.currmove_number = Some(number);
        stats.max_move_number = stats.max_move_number.max(number);
    }
}

/// The position at the end of a target; `None` if a move is not playable.
pub(crate) fn position_of(target: &AnalysisTarget) -> Option<Position> {
    let mut position = target.start.clone();
    for &mv in &target.moves {
        position.play(mv).ok()?;
    }
    Some(position)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Arc<AnalysisTarget> {
        Arc::new(AnalysisTarget {
            start: Position::new(),
            moves: Vec::new(),
        })
    }

    fn view(multipv: u8) -> AnalysisView {
        let mut v = AnalysisView::new(RunId(1), multipv, &AnalysisPrefs::default());
        v.begin_search(SearchId(1), Some(target()));
        v
    }

    fn mv(uci: &str) -> Move {
        Position::new().parse_uci(uci).expect("uci")
    }

    fn line_info(multipv: u8, depth: u32, cp: i32, ucis: &[&str]) -> SearchInfo {
        let mut pos = Position::new();
        let pv = ucis
            .iter()
            .map(|u| {
                let m = pos.parse_uci(u).expect("uci");
                pos.play(m).expect("play");
                m
            })
            .collect();
        SearchInfo {
            search: SearchId(1),
            multipv,
            depth: Some(depth),
            score: Some((Score::Cp(cp), Bound::Exact)),
            pv,
            ..SearchInfo::default()
        }
    }

    const OPTIONS: FormatOptions = FormatOptions {
        numbers: NumberStyle::Tight,
        max_ply: 0,
    };

    #[test]
    fn lines_arriving_in_reverse_order_are_kept_in_line_order() {
        let mut v = view(3);
        v.apply_info(line_info(3, 10, -5, &["g1f3"]), OPTIONS);
        v.apply_info(line_info(1, 10, 30, &["e2e4"]), OPTIONS);
        v.apply_info(line_info(2, 10, 20, &["d2d4"]), OPTIONS);
        let numbers: Vec<u8> = v.lines.iter().map(|l| l.multipv).collect();
        assert_eq!(numbers, [1, 2, 3]);
        assert_eq!(v.lines[1].text, "1.d4");
        // A newer report of line 2 replaces it.
        v.apply_info(line_info(2, 11, 25, &["c2c4"]), OPTIONS);
        assert_eq!(v.lines.len(), 3);
        assert_eq!((v.lines[1].depth, v.lines[1].text.as_str()), (11, "1.c4"));
    }

    #[test]
    fn a_line_beyond_the_configured_count_is_dropped() {
        let mut v = view(2);
        v.apply_info(line_info(3, 10, 0, &["e2e4"]), OPTIONS);
        assert!(v.lines.is_empty());
        v.apply_info(line_info(2, 10, 0, &["e2e4"]), OPTIONS);
        assert_eq!(v.lines.len(), 1);
    }

    #[test]
    fn a_report_without_score_or_pv_only_updates_the_numbers() {
        let mut v = view(1);
        v.apply_info(line_info(1, 10, 30, &["e2e4"]), OPTIONS);
        let mut counters = SearchInfo {
            search: SearchId(1),
            depth: Some(3),
            nodes: Some(5_000_000),
            nps: Some(1_000_000),
            hashfull_permille: Some(120),
            cpuload_permille: Some(400),
            tbhits: Some(2),
            currmove: Some(mv("d2d4")),
            currmovenumber: Some(7),
            time_ms: Some(4000),
            ..SearchInfo::default()
        };
        v.apply_info(counters.clone(), OPTIONS);
        assert_eq!(v.lines.len(), 1);
        assert_eq!(v.lines[0].depth, 10, "the line keeps its depth");
        assert_eq!(v.stats.depth, 10, "a currmove report cannot lower it");
        assert_eq!(v.stats.nodes, 5_000_000);
        assert_eq!(v.stats.currmove.as_deref(), Some("d4"));
        assert_eq!(v.stats.max_move_number, 7);
        counters.currmovenumber = Some(3);
        v.apply_info(counters, OPTIONS);
        assert_eq!(v.stats.currmove_number, Some(3));
        assert_eq!(v.stats.max_move_number, 7, "the largest is kept");
    }

    #[test]
    fn a_report_of_an_old_search_is_dropped() {
        let mut v = view(1);
        let mut old = line_info(1, 10, 30, &["e2e4"]);
        old.search = SearchId(0);
        v.apply_info(old, OPTIONS);
        assert!(v.lines.is_empty());
        v.begin_search(SearchId(2), None);
        v.apply_info(line_info(1, 10, 30, &["e2e4"]), OPTIONS);
        assert!(v.lines.is_empty(), "search 1 is old now");
    }

    #[test]
    fn a_pv_that_does_not_start_in_the_position_is_dropped() {
        let mut v = view(1);
        // `e7e5` is not playable by White.
        let mut info = line_info(1, 10, 30, &["e2e4"]);
        info.pv = vec![
            Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1")
                .expect("fen")
                .parse_uci("e7e5")
                .expect("uci"),
        ];
        v.apply_info(info, OPTIONS);
        assert!(v.lines.is_empty());
    }

    #[test]
    fn history_adds_only_new_pvs_and_restarts_when_the_depth_drops() {
        let mut v = view(1);
        v.apply_info(line_info(1, 5, 10, &["e2e4"]), OPTIONS);
        v.apply_info(line_info(1, 6, 12, &["e2e4"]), OPTIONS);
        v.apply_info(line_info(1, 7, 15, &["d2d4"]), OPTIONS);
        assert_eq!(v.history.len(), 2, "the same PV is not repeated");
        assert_eq!(v.history[1].text, "1.d4");
        // A new search: the depth goes back.
        v.apply_info(line_info(1, 2, 0, &["g1f3"]), OPTIONS);
        assert_eq!(v.history.len(), 1);
        assert_eq!(v.history[0].depth, 2);
    }

    #[test]
    fn history_is_only_kept_for_a_single_line_and_is_bounded() {
        let mut multi = view(2);
        multi.apply_info(line_info(1, 5, 10, &["e2e4"]), OPTIONS);
        assert!(multi.history.is_empty());
        let mut v = view(1);
        let cycle = [["e2e4"], ["d2d4"]];
        let last = u32::try_from(MAX_HISTORY).expect("small") + 50;
        for depth in 1..=last {
            v.apply_info(line_info(1, depth, 0, &cycle[depth as usize % 2]), OPTIONS);
        }
        assert_eq!(v.history.len(), MAX_HISTORY);
        assert_eq!(v.history.last().map(|h| h.depth), Some(last));
    }

    #[test]
    fn shrinking_the_count_cuts_the_lines_and_one_line_clears_the_history() {
        let mut v = view(3);
        for n in 1..=3 {
            v.apply_info(line_info(n, 8, 0, &["e2e4"]), OPTIONS);
        }
        v.set_multipv(2);
        assert_eq!(v.lines.len(), 2);
        v.set_multipv(1);
        assert_eq!(v.lines.len(), 1);
        v.apply_info(line_info(1, 9, 0, &["d2d4"]), OPTIONS);
        assert_eq!(v.history.len(), 1);
        v.set_multipv(3);
        assert_eq!(v.history.len(), 1, "growing keeps it");
        v.set_multipv(1);
        assert!(v.history.is_empty());
    }

    #[test]
    fn the_pv_limit_cuts_the_text_but_not_a_mate() {
        let mut v = view(1);
        let long = ["e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6"];
        let limited = FormatOptions {
            numbers: NumberStyle::Tight,
            max_ply: 2,
        };
        v.apply_info(line_info(1, 5, 10, &long), limited);
        assert_eq!(v.lines[0].text, "1.e4 e5");
        assert_eq!(v.lines[0].pv.len(), 6, "the moves are kept whole");
        let mut mate = line_info(1, 6, 0, &long);
        mate.score = Some((Score::Mate(3), Bound::Exact));
        v.apply_info(mate, limited);
        assert_eq!(v.lines[0].text, "1.e4 e5 2.Nf3 Nc6 3.Bc4 Nf6");
        // A change of the options formats again, with no new report.
        v.reformat(FormatOptions {
            numbers: NumberStyle::Spaced,
            max_ply: 0,
        });
        assert!(v.lines[0].text.starts_with("1. e4"));
    }

    #[test]
    fn a_new_position_clears_the_lines_and_the_same_one_keeps_them() {
        let mut v = view(1);
        v.apply_info(line_info(1, 5, 10, &["e2e4"]), OPTIONS);
        v.begin_search(SearchId(2), Some(target()));
        assert_eq!(v.lines.len(), 1, "same position: kept");
        let after_e4 = Arc::new(AnalysisTarget {
            start: Position::new(),
            moves: vec![mv("e2e4")],
        });
        v.begin_search(SearchId(3), Some(after_e4));
        assert!(v.lines.is_empty());
        assert!(v.history.is_empty());
        assert_eq!(v.position().map(Position::turn), Some(Color::Black));
    }

    #[test]
    fn a_terminal_position_has_a_fixed_score() {
        // Fool's mate: White is checkmated.
        let pos =
            Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
                .expect("fen");
        let target = Arc::new(AnalysisTarget {
            start: pos.clone(),
            moves: Vec::new(),
        });
        let mut v = view(1);
        v.set_terminal(&target, &pos);
        assert_eq!(v.terminal, Some(Terminal::Checkmate));
        assert_eq!(v.main_score(), Some((Score::Cp(-32_700), Bound::Exact)));
        let stalemate = Position::from_fen("k7/8/1K6/8/8/8/8/1R6 b - - 0 1").expect("fen");
        v.set_terminal(&target, &stalemate);
        assert_eq!(v.terminal, Some(Terminal::Stalemate));
        assert_eq!(v.main_score(), Some((Score::Cp(0), Bound::Exact)));
        v.clear_terminal();
        assert_eq!(v.main_score(), None);
    }

    #[test]
    fn the_log_filter_ignores_case() {
        let mut v = view(1);
        v.log = ["uci", "UCIOK", "readyok"]
            .iter()
            .enumerate()
            .map(|(i, t)| LogEntry {
                seq: i as u64,
                kind: mychess_engine::LogKind::Received,
                text: (*t).to_owned(),
            })
            .collect();
        v.log_filter = "uci".to_owned();
        assert_eq!(v.filtered_log().count(), 2);
        v.log_filter.clear();
        assert_eq!(v.filtered_log().count(), 3);
    }

    #[test]
    fn display_cycles_through_the_three_modes() {
        assert_eq!(Display::Hidden.next(), Display::NoWrap);
        assert_eq!(Display::NoWrap.next(), Display::Wrap);
        assert_eq!(Display::Wrap.next(), Display::Hidden);
    }
}
