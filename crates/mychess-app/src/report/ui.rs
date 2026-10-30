//! The state and messages of the report panel (spec M4-10b, D1421, D1433).

use mychess_core::{Color, Game, Move, NodeId};

use super::{
    GameReport, MoveClass, ReportConfig, ReportDialog, ReportThresholds, Review, SideFilter,
};
use crate::annotate::PositionEval;
use crate::engines::RunId;
use crate::graphs::{Marker, MarkerKind, ScoreGraphView, report_graph_view};

/// How deep the engine looks at each position (D1422).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReportPreset {
    /// Depth 12.
    Fast,
    /// Depth 16 (the default).
    #[default]
    Standard,
    /// Depth 22.
    Deep,
}

impl ReportPreset {
    /// Every preset, shallowest first.
    pub const ALL: [ReportPreset; 3] = [
        ReportPreset::Fast,
        ReportPreset::Standard,
        ReportPreset::Deep,
    ];

    /// The depth of each search.
    pub fn depth(self) -> u32 {
        match self {
            ReportPreset::Fast => 12,
            ReportPreset::Standard => 16,
            ReportPreset::Deep => 22,
        }
    }

    /// The Fluent key of the label.
    pub fn label_key(self) -> &'static str {
        match self {
            ReportPreset::Fast => "report-preset-fast",
            ReportPreset::Standard => "report-preset-standard",
            ReportPreset::Deep => "report-preset-deep",
        }
    }

    /// The preset whose depth is the closest to `depth` (the label of a
    /// run with a depth of the user's own choosing).
    pub fn nearest(depth: u32) -> ReportPreset {
        ReportPreset::ALL
            .into_iter()
            .min_by_key(|p| p.depth().abs_diff(depth))
            .unwrap_or_default()
    }
}

/// An event of the report panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReportMessage {
    /// Chooses the depth preset of the next run.
    SetPreset(ReportPreset),
    /// Analyses the whole main line with the default engine.
    Generate,
    /// Stops the run; the positions analysed so far make a partial report
    /// (D1443).
    Cancel,
    /// Forgets the report.
    Clear,
    /// Writes the report into the game: `[%eval]` comments, NAGs and the
    /// best line of each fault, as one undo point (D1447).
    WriteToGame,
    /// Starts the guided review of the key moments of `side` (D1435), with
    /// the brilliant and great moves too when `highlights`.
    StartReview {
        /// Whose moves are visited.
        side: SideFilter,
        /// Also visit the highlights.
        highlights: bool,
    },
    /// The next moment of the review.
    ReviewNext,
    /// The previous moment of the review.
    ReviewPrevious,
    /// "Try again": the user looks for the better move on the board.
    ReviewTry,
    /// "See the best move".
    ReviewShowBest,
    /// Leaves the review.
    ReviewExit,
    /// Builds the report from the `[%eval]` comments of the game, without an
    /// engine (D1439).
    FromGame,
    /// Goes to the position after this ply of the main line (a click on a
    /// marker or on a moment).
    GoTo(usize),
    /// Lists only the moves of this class (`None`: every key moment).
    Filter(Option<MoveClass>),
    /// The timer of [`Effect::ScheduleReviewTick`](crate::Effect) fired: the
    /// walk of the review takes its next step (spec M4-11, D1666), or the
    /// clock of the run counts a second (D2121). A `stamp` that is not the
    /// one of the walk or of the clock in progress is ignored.
    WalkTick {
        /// The walk or the clock that asked for the timer.
        stamp: u64,
    },
    /// Opens the "Configure report" dialog (D2100).
    OpenConfig,
    /// Closes the dialog without running (Esc, Cancel).
    CloseConfig,
    /// The dialog's choices changed; the application limits them (D2100).
    ConfigSet(ReportConfig),
    /// The engine chosen in the dialog (`None`: the default one).
    ConfigEngine(Option<RunId>),
    /// Puts the defaults back in the dialog.
    ConfigDefaults,
    /// OK in the dialog: saves the choices in the preferences and runs the
    /// report (D2100).
    ConfigStart,
    /// Analyses only the positions a stopped run did not reach, keeping
    /// the ones it did (D1443, D2110).
    Continue,
    /// Whose side the user played (`None`: no side in particular), for the
    /// summary (D1437, D2115).
    SetUserSide(Option<Color>),
}

/// The longest a walk to a moment may take (D1660).
pub const WALK_MAX_MS: u32 = 1000;
/// The longest interval between two steps of a walk.
const WALK_STEP_MAX_MS: u32 = 250;
/// The most moves a walk animates; a longer way is jumped first (D1661).
const WALK_MAX_STEPS: u32 = 12;

/// How the cursor gets from one ply to another (D1660-D1662): pure, so the
/// time bound is tested without a clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkPlan {
    /// Where to jump at once before the steps (the far part of a long way,
    /// or the whole way when the walk is off); `None` when the steps start
    /// from where the cursor is.
    pub jump_to: Option<u32>,
    /// The moves to animate, one per tick.
    pub steps: u32,
    /// The wait before each step, in milliseconds; `steps * interval_ms`
    /// never exceeds [`WALK_MAX_MS`].
    pub interval_ms: u32,
}

/// Plans the walk from main-line ply `from` to ply `to`, both ways. With
/// `animation_ms == 0` (animations off) it is a plain jump.
pub fn walk_plan(from: u32, to: u32, animation_ms: u32) -> WalkPlan {
    let distance = from.abs_diff(to);
    if distance == 0 {
        return WalkPlan {
            jump_to: None,
            steps: 0,
            interval_ms: 0,
        };
    }
    if animation_ms == 0 {
        return WalkPlan {
            jump_to: Some(to),
            steps: 0,
            interval_ms: 0,
        };
    }
    let steps = distance.min(WALK_MAX_STEPS);
    // `steps` is at least 1 here.
    let interval_ms = (WALK_MAX_MS / steps).min(WALK_STEP_MAX_MS);
    let jump_to = (distance > WALK_MAX_STEPS).then(|| {
        if to >= from {
            to - WALK_MAX_STEPS
        } else {
            to + WALK_MAX_STEPS
        }
    });
    WalkPlan {
        jump_to,
        steps,
        interval_ms,
    }
}

/// Which way a walk goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkDirection {
    /// Towards the end of the game.
    Forward,
    /// Towards the start.
    Backward,
}

/// A walk of the cursor to the moment on show in progress (D1666). Only one
/// at a time; the stamp tells its timer from the stale ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewWalk {
    /// The main-line ply the walk ends at.
    pub target_ply: u32,
    /// The ply the next step goes to.
    pub next_ply: u32,
    /// Which way it goes.
    pub direction: WalkDirection,
    /// The wait between steps, in milliseconds.
    pub interval_ms: u32,
    /// The identity of the walk and of its timer.
    pub stamp: u64,
}

/// The `[report]` table of the preferences (spec M4-11, D1667).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ReportPrefs {
    /// Draw the arrow of the best move on the position of the review (it
    /// is hidden while the user tries, whatever this says).
    pub review_arrow: bool,
}

impl Default for ReportPrefs {
    fn default() -> ReportPrefs {
        ReportPrefs { review_arrow: true }
    }
}

/// The effort of the run that produced the evaluations of a report: the
/// limit of each search and the engine (D2190).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportEffort {
    /// Searches were limited by time instead of by depth.
    pub by_time: bool,
    /// The depth of each search when `by_time` is off.
    pub depth: u32,
    /// The time of each search, in milliseconds, when `by_time` is on.
    pub movetime_ms: u32,
    /// The engine run that searched; it may be gone by now.
    pub engine: Option<RunId>,
}

/// Where the evaluations of a report came from (D2191).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportOrigin {
    /// A run of an engine, with the effort it was made with.
    Engine(ReportEffort),
    /// The `[%eval]` comments of the game: there is no run to go on with.
    Game,
}

/// A finished (or partial) report with what the panel needs to show it.
#[derive(Debug, Clone)]
pub struct ReportData {
    /// The verdicts and the totals.
    pub report: GameReport,
    /// The SAN of each move analysed, like `report.moves`.
    pub sans: Vec<String>,
    /// Every move of the main line was analysed.
    pub complete: bool,
    /// The number of moves of the main line when it was analysed.
    pub plies: usize,
    /// The preset used.
    pub preset: ReportPreset,
    /// The thresholds the classes were computed with.
    pub thresholds: ReportThresholds,
    /// The curve and its markers.
    pub graph: ScoreGraphView,
    /// The evaluation after each move, `evals[0]` being the start position
    /// (kept to write the report into the game).
    pub(crate) evals: Vec<PositionEval>,
    /// Where the evaluations came from (D2191).
    pub origin: ReportOrigin,
    /// The main line analysed; the report is stale when it differs.
    moves: Vec<Move>,
}

impl ReportData {
    /// Builds the data of a report; `scores[i]` is the evaluation in pawns,
    /// from White's point of view, after `i` moves.
    pub(crate) fn new(
        game: &Game,
        report: GameReport,
        sans: Vec<String>,
        evals: Vec<PositionEval>,
        preset: ReportPreset,
        thresholds: ReportThresholds,
    ) -> ReportData {
        let moves: Vec<Move> = game.mainline().filter_map(|n| game.mv(n).ok()).collect();
        let scores: Vec<Option<f32>> = evals.iter().map(|e| Some(e.score)).collect();
        let markers: Vec<Marker> = report
            .moves
            .iter()
            .filter_map(|m| marker_kind(m.class).map(|kind| Marker { ply: m.ply, kind }))
            .collect();
        ReportData {
            complete: report.moves.len() == moves.len(),
            plies: moves.len(),
            graph: report_graph_view(&scores, &markers),
            evals,
            report,
            sans,
            preset,
            thresholds,
            origin: ReportOrigin::Game,
            moves,
        }
    }

    /// The same data, marked as made by an engine run with `effort` (D2190).
    #[must_use]
    pub(crate) fn with_effort(mut self, effort: ReportEffort) -> ReportData {
        self.origin = ReportOrigin::Engine(effort);
        self
    }

    /// The effort of the engine run that made the report, if one did.
    pub fn effort(&self) -> Option<ReportEffort> {
        match self.origin {
            ReportOrigin::Engine(effort) => Some(effort),
            ReportOrigin::Game => None,
        }
    }

    /// Whether this report still describes the main line of `game`.
    pub fn matches(&self, game: &Game) -> bool {
        let mut lines = game.mainline();
        for played in &self.moves {
            match lines.next().map(|n| game.mv(n)) {
                Some(Ok(m)) if m == *played => {}
                _ => return false,
            }
        }
        lines.next().is_none()
    }

    /// The node of the position before the move of `ply`: the root for the
    /// first move.
    pub fn node_before(game: &Game, ply: usize) -> Option<NodeId> {
        match ply.checked_sub(2) {
            None if ply == 1 => Some(game.root()),
            None => None,
            Some(index) => game.mainline().nth(index),
        }
    }

    /// The SAN of the engine's best move at `ply`, when it is known.
    pub fn best_san(&self, game: &Game, ply: usize) -> Option<String> {
        let best = self.report.moves.get(ply.checked_sub(1)?)?.best?;
        let before = game.position_at(Self::node_before(game, ply)?).ok()?;
        before.san(best).ok()
    }

    /// Whether a stopped run can be continued: an engine made the report,
    /// it describes the game but did not reach its end (D2110, D2191).
    pub fn resumable(&self) -> bool {
        matches!(self.origin, ReportOrigin::Engine(_))
            && !self.complete
            && self.evals.len() <= self.plies
    }

    /// The moves worth looking at again, in game order, for the list of the
    /// panel (D1436): the faults and the highlights, or only `filter`.
    pub fn moments(&self, filter: Option<MoveClass>) -> Vec<usize> {
        self.report
            .moves
            .iter()
            .filter(|m| match filter {
                Some(class) => m.class == class,
                None => {
                    m.class.is_fault() || matches!(m.class, MoveClass::Brilliant | MoveClass::Great)
                }
            })
            .map(|m| m.ply)
            .collect()
    }
}

/// The marker a class draws on the curve (D1432); the quiet classes draw
/// none.
pub fn marker_kind(class: MoveClass) -> Option<MarkerKind> {
    match class {
        MoveClass::Brilliant => Some(MarkerKind::Brilliant),
        MoveClass::Great => Some(MarkerKind::Great),
        MoveClass::Inaccuracy => Some(MarkerKind::Inaccuracy),
        MoveClass::Mistake => Some(MarkerKind::Mistake),
        MoveClass::Miss => Some(MarkerKind::Miss),
        MoveClass::Blunder => Some(MarkerKind::Blunder),
        MoveClass::Best | MoveClass::Excellent | MoveClass::Good | MoveClass::Book => None,
    }
}

/// What the panel shows.
#[derive(Debug, Clone, Default)]
pub struct ReportUi {
    /// The depth preset of the next run.
    pub preset: ReportPreset,
    /// The report of the game, until the game changes.
    pub data: Option<ReportData>,
    /// The class the list of moments is limited to.
    pub filter: Option<MoveClass>,
    /// The guided review in progress, if any (M4-10c).
    pub review: Option<Review>,
    /// Changes whenever `data` is replaced (the canvas cache key).
    pub stamp: u64,
    /// The cursor is walking to the moment on show (M4-11).
    pub walk: Option<ReviewWalk>,
    /// The arrows of the review for the board now, refreshed at the end of
    /// every `update` (M4-11).
    pub(crate) arrows: Vec<crate::evalbar::EngineArrow>,
    /// The "Configure report" dialog, while it is open (D2100).
    pub config_dialog: Option<ReportDialog>,
    /// The engine chosen in the dialog for the next runs, if it still runs
    /// (D2100).
    pub engine: Option<RunId>,
    /// The side the user played (D2115).
    pub user_side: Option<Color>,
    /// The `generation` of the game `user_side` was set for (D2196).
    pub(crate) user_side_game: u64,
    /// The clock of the run in progress, for the time left (D2121).
    pub clock: Option<ReportClock>,
}

/// The class of the move under the cursor, to draw on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassMark {
    /// The destination square of the move (the king's for castling).
    pub square: mychess_core::Square,
    /// What the report says of the move.
    pub class: MoveClass,
}

/// The seconds a run of the report has taken, counted by the ticks of a
/// timer (the application reads no clock, D2121).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportClock {
    /// The identity of the clock and of its timer.
    pub stamp: u64,
    /// The ticks counted since the run began.
    pub seconds: u32,
    /// The moves already analysed when the run began (a run that continues
    /// a report does not start from zero).
    pub base_done: u32,
}

/// The seconds a tick of the clock stands for.
pub const CLOCK_TICK_SECS: u32 = 1;

/// The most seconds the estimate may promise (a run that stalls does not
/// show an absurd figure).
const ETA_MAX_SECS: u32 = 99 * 3600;

/// The time left of a run, from its pace: `elapsed` seconds took
/// `done_now` moves (counted from where the run began) and `left` moves are
/// still to do. `None` until the first move is done, or when the pace is
/// unknown. Rounded up to whole seconds.
pub fn eta_seconds(elapsed: u32, done_now: u32, left: u32) -> Option<u32> {
    if done_now == 0 || elapsed == 0 {
        return None;
    }
    let total = u64::from(elapsed) * u64::from(left);
    let done = u64::from(done_now);
    let secs = total.div_ceil(done);
    Some(u32::try_from(secs).unwrap_or(u32::MAX).min(ETA_MAX_SECS))
}

/// The side the user played in `game`, found by the name of the player in
/// the preferences against the `White` and `Black` tags (D2115): a name that
/// is both, or neither, tells nothing.
pub fn infer_user_side(game: &Game, names: &[&str]) -> Option<Color> {
    let norm = |s: &str| s.trim().to_lowercase();
    let names: Vec<String> = names
        .iter()
        .map(|n| norm(n))
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        return None;
    }
    let tags = game.tags();
    let is_user = |tag: &str| {
        let tag = norm(tag);
        !tag.is_empty() && names.contains(&tag)
    };
    match (is_user(&tags.white), is_user(&tags.black)) {
        (true, false) => Some(Color::White),
        (false, true) => Some(Color::Black),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build_report;

    #[test]
    fn walk_plan_never_lasts_more_than_a_second_either_way() {
        for animation in [1, 200, 2000] {
            for from in 0..60_u32 {
                for to in 0..60_u32 {
                    let plan = walk_plan(from, to, animation);
                    assert!(
                        plan.steps * plan.interval_ms <= WALK_MAX_MS,
                        "{from}->{to}: {plan:?}"
                    );
                    assert!(plan.steps <= WALK_MAX_STEPS);
                    let distance = from.abs_diff(to);
                    let jumped = plan.jump_to.map_or(0, |j| from.abs_diff(j));
                    // The jump and the steps cover the way exactly.
                    assert_eq!(jumped + plan.steps, distance, "{from}->{to}");
                    if let Some(j) = plan.jump_to {
                        assert_eq!(j.abs_diff(to), plan.steps);
                    }
                }
            }
        }
    }

    #[test]
    fn walk_plan_cases_of_the_spec() {
        let three = walk_plan(5, 8, 200);
        assert_eq!(
            (three.jump_to, three.steps, three.interval_ms),
            (None, 3, 250)
        );
        let forty = walk_plan(0, 40, 200);
        assert_eq!(
            (forty.jump_to, forty.steps, forty.interval_ms),
            (Some(28), 12, 83)
        );
        let back = walk_plan(40, 0, 200);
        assert_eq!((back.jump_to, back.steps), (Some(12), 12));
        let same = walk_plan(7, 7, 200);
        assert_eq!((same.jump_to, same.steps), (None, 0));
    }

    #[test]
    fn walk_plan_is_a_plain_jump_when_animations_are_off() {
        let plan = walk_plan(3, 30, 0);
        assert_eq!((plan.jump_to, plan.steps), (Some(30), 0));
    }

    #[test]
    fn presets_map_to_increasing_depths_inside_the_valid_range() {
        let depths: Vec<u32> = ReportPreset::ALL.iter().map(|p| p.depth()).collect();
        assert!(depths.windows(2).all(|w| w[0] < w[1]));
        for preset in ReportPreset::ALL {
            assert_eq!(ReportPreset::nearest(preset.depth()), preset);
        }
    }

    #[test]
    fn only_the_notable_classes_draw_markers() {
        let marked = MoveClass::ALL
            .iter()
            .filter(|c| marker_kind(**c).is_some())
            .count();
        assert_eq!(marked, 6);
        assert_eq!(marker_kind(MoveClass::Book), None);
    }

    #[test]
    fn an_empty_report_matches_only_an_empty_game() {
        let game = Game::new();
        let data = ReportData::new(
            &game,
            build_report(&[], &ReportThresholds::default()),
            Vec::new(),
            Vec::new(),
            ReportPreset::Fast,
            ReportThresholds::default(),
        );
        assert!(data.matches(&game));
        assert!(data.complete);
        assert!(data.moments(None).is_empty());
    }
}
