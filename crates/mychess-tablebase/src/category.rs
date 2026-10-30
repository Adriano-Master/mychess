//! Results of a position and of its moves (spec m8-8, D2553, D2556).

use std::cmp::{Ordering, Reverse};

use mychess_core::Move;

/// The theoretical result under the 50-move rule, with the 8 values of the
/// Lichess tablebase API. Always from the point of view of a side: the side
/// to move for a position, the side that plays the move for a [`MoveEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Category {
    /// A win.
    Win,
    /// A win, or a win that the 50-move rule turns into a draw (the DTZ was
    /// rounded and the clock makes the difference).
    MaybeWin,
    /// A win that the 50-move rule turns into a draw.
    CursedWin,
    /// A draw.
    Draw,
    /// A loss that the 50-move rule turns into a draw.
    BlessedLoss,
    /// A loss, or a blessed loss (the DTZ was rounded).
    MaybeLoss,
    /// A loss.
    Loss,
    /// Not in the tablebase.
    Unknown,
}

impl Category {
    /// Every value, best first.
    pub const ALL: [Category; 8] = [
        Category::Win,
        Category::MaybeWin,
        Category::CursedWin,
        Category::Draw,
        Category::BlessedLoss,
        Category::MaybeLoss,
        Category::Loss,
        Category::Unknown,
    ];

    /// The same result seen by the other side.
    #[must_use]
    pub const fn negate(self) -> Category {
        match self {
            Category::Win => Category::Loss,
            Category::MaybeWin => Category::MaybeLoss,
            Category::CursedWin => Category::BlessedLoss,
            Category::Draw => Category::Draw,
            Category::BlessedLoss => Category::CursedWin,
            Category::MaybeLoss => Category::MaybeWin,
            Category::Loss => Category::Win,
            Category::Unknown => Category::Unknown,
        }
    }

    /// Order of preference for the side: higher is better, `Unknown` is the
    /// lowest.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Category::Win => 7,
            Category::MaybeWin => 6,
            Category::CursedWin => 5,
            Category::Draw => 4,
            Category::BlessedLoss => 3,
            Category::MaybeLoss => 2,
            Category::Loss => 1,
            Category::Unknown => 0,
        }
    }

    /// `1` for the winning values, `-1` for the losing ones, `0` for a draw
    /// or an unknown result.
    #[must_use]
    pub const fn signum(self) -> i8 {
        match self {
            Category::Win | Category::MaybeWin | Category::CursedWin => 1,
            Category::Loss | Category::MaybeLoss | Category::BlessedLoss => -1,
            Category::Draw | Category::Unknown => 0,
        }
    }

    /// The stable name (the spelling of the Lichess API).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Category::Win => "win",
            Category::MaybeWin => "maybe-win",
            Category::CursedWin => "cursed-win",
            Category::Draw => "draw",
            Category::BlessedLoss => "blessed-loss",
            Category::MaybeLoss => "maybe-loss",
            Category::Loss => "loss",
            Category::Unknown => "unknown",
        }
    }

    /// Reads the Lichess spelling; anything else is [`Category::Unknown`].
    #[must_use]
    pub fn from_lichess(text: &str) -> Category {
        Category::ALL
            .into_iter()
            .find(|c| c.as_str() == text)
            .unwrap_or(Category::Unknown)
    }
}

/// The result of a position or of a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Evaluation {
    /// The theoretical result.
    pub category: Category,
    /// Distance to zeroing the 50-move clock, in plies, positive when the
    /// side wins. Syzygy gives it only when the `.rtbz` file is available.
    pub dtz: Option<i32>,
    /// Whether `dtz` may be one ply off (rounding of the Syzygy tables).
    pub dtz_rounded: bool,
    /// Distance to mate, in plies, positive when the side mates. Syzygy does
    /// not have it (D2554); the online server has it for some positions.
    pub dtm: Option<i32>,
}

impl Evaluation {
    /// Not in the tablebase.
    pub const UNKNOWN: Evaluation = Evaluation {
        category: Category::Unknown,
        dtz: None,
        dtz_rounded: false,
        dtm: None,
    };

    /// A result without distances.
    #[must_use]
    pub const fn of(category: Category) -> Evaluation {
        Evaluation {
            category,
            dtz: None,
            dtz_rounded: false,
            dtm: None,
        }
    }

    /// The same result seen by the other side (the distances change sign).
    #[must_use]
    pub fn negate(self) -> Evaluation {
        Evaluation {
            category: self.category.negate(),
            dtz: self.dtz.map(i32::saturating_neg),
            dtz_rounded: self.dtz_rounded,
            dtm: self.dtm.map(i32::saturating_neg),
        }
    }
}

/// A legal move with its result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveEntry {
    /// The move.
    pub mv: Move,
    /// The move in SAN, in the position it is played in.
    pub san: String,
    /// The move in UCI.
    pub uci: String,
    /// The result for the side that plays the move (D2553).
    pub evaluation: Evaluation,
    /// The move captures or moves a pawn (resets the 50-move clock).
    pub zeroing: bool,
    /// The move mates.
    pub checkmate: bool,
    /// The move stalemates.
    pub stalemate: bool,
    /// The move leaves no material to mate with.
    pub insufficient_material: bool,
}

/// Where a result came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Source {
    /// Local Syzygy files.
    Local,
    /// The Lichess online tablebase.
    Online,
}

/// How many moves have each kind of result (D2556): wins are `Win` and
/// `MaybeWin`; draws are `Draw`, `CursedWin` and `BlessedLoss` (drawn under
/// the 50-move rule); losses are `Loss` and `MaybeLoss`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// Moves that win.
    pub wins: u16,
    /// Moves that draw.
    pub draws: u16,
    /// Moves that lose.
    pub losses: u16,
    /// Moves with an unknown result.
    pub unknown: u16,
}

/// The answer for a position: its result and the result of every legal move,
/// best move first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionReport {
    /// Where the answer came from.
    pub source: Source,
    /// Pieces on the board, kings included.
    pub pieces: u8,
    /// The result for the side to move.
    pub evaluation: Evaluation,
    /// Every legal move, in the order of [`compare_moves`].
    pub moves: Vec<MoveEntry>,
    /// Whether the source gave a result for every legal move. When it did
    /// not (a server that left some out), the missing moves are listed as
    /// unknown and no move is recommended: the best one may be among them.
    pub complete: bool,
}

impl PositionReport {
    /// Builds a report and sorts the moves.
    #[must_use]
    pub fn new(
        source: Source,
        pieces: u8,
        evaluation: Evaluation,
        mut moves: Vec<MoveEntry>,
    ) -> PositionReport {
        moves.sort_by(compare_moves);
        PositionReport {
            source,
            pieces,
            evaluation,
            moves,
            complete: true,
        }
    }

    /// The same report, marked as lacking the result of some legal moves.
    #[must_use]
    pub fn incomplete(mut self) -> PositionReport {
        self.complete = false;
        self
    }

    /// The moves that share the best result (same category and DTZ).
    /// Empty when the best move is unknown.
    #[must_use]
    pub fn best_group(&self) -> &[MoveEntry] {
        let Some(first) = self.moves.first().filter(|_| self.complete) else {
            return &[];
        };
        if first.evaluation.category == Category::Unknown {
            return &[];
        }
        let len = self
            .moves
            .iter()
            .take_while(|m| {
                m.evaluation.category == first.evaluation.category
                    && m.evaluation.dtz == first.evaluation.dtz
            })
            .count();
        self.moves.get(..len).unwrap_or(&[])
    }

    /// The best move, if its result is known.
    #[must_use]
    pub fn best(&self) -> Option<&MoveEntry> {
        self.best_group().first()
    }

    /// The move the training plays (D2557): one of the best group, picked by
    /// `seed` (the Zobrist hash of the position), so the same position always
    /// gets the same answer.
    #[must_use]
    pub fn recommended(&self, seed: u64) -> Option<&MoveEntry> {
        let group = self.best_group();
        let len = u64::try_from(group.len()).ok().filter(|n| *n > 0)?;
        group.get(usize::try_from(seed % len).ok()?)
    }

    /// The counts of results among the moves.
    #[must_use]
    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for entry in &self.moves {
            let slot = match entry.evaluation.category {
                Category::Win | Category::MaybeWin => &mut counts.wins,
                Category::Draw | Category::CursedWin | Category::BlessedLoss => &mut counts.draws,
                Category::Loss | Category::MaybeLoss => &mut counts.losses,
                Category::Unknown => &mut counts.unknown,
            };
            *slot = slot.saturating_add(1);
        }
        counts
    }

    /// Whether any move has a distance to mate.
    #[must_use]
    pub fn has_dtm(&self) -> bool {
        self.evaluation.dtm.is_some() || self.moves.iter().any(|m| m.evaluation.dtm.is_some())
    }
}

/// The order of the list (D2556): best category first; among wins the
/// shortest distance first, among losses the longest first; then SAN.
#[must_use]
pub fn compare_moves(a: &MoveEntry, b: &MoveEntry) -> Ordering {
    let (ea, eb) = (&a.evaluation, &b.evaluation);
    eb.category
        .rank()
        .cmp(&ea.category.rank())
        .then_with(|| {
            let sign = ea.category.signum();
            match sign.cmp(&0) {
                Ordering::Greater => distance_key(ea).cmp(&distance_key(eb)),
                Ordering::Less => loss_key(ea).cmp(&loss_key(eb)),
                Ordering::Equal => Ordering::Equal,
            }
        })
        .then_with(|| a.san.cmp(&b.san))
        .then_with(|| a.uci.cmp(&b.uci))
}

/// Ascending key of a win: the shortest known distance first, unknown
/// distances last.
fn distance_key(e: &Evaluation) -> (u8, u32, u8, u32) {
    (
        u8::from(e.dtz.is_none()),
        e.dtz.map_or(0, i32::unsigned_abs),
        u8::from(e.dtm.is_none()),
        e.dtm.map_or(0, i32::unsigned_abs),
    )
}

/// Ascending key of a loss: the longest known distance first (the move that
/// resists most), unknown distances last.
fn loss_key(e: &Evaluation) -> (u8, Reverse<u32>, u8, Reverse<u32>) {
    (
        u8::from(e.dtz.is_none()),
        Reverse(e.dtz.map_or(0, i32::unsigned_abs)),
        u8::from(e.dtm.is_none()),
        Reverse(e.dtm.map_or(0, i32::unsigned_abs)),
    )
}
