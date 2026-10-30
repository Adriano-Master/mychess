//! A ready-to-draw snapshot of the window of the advanced searches. It holds
//! values the front-end can name (no `mychess-db` types).

use mychess_core::{File, Piece, Rank};
use mychess_db::{
    MaterialSearch, Pattern, PieceRange, SideRanges, material_presets, pattern_presets,
};

use super::AdvancedUi;
use super::message::{
    AdvancedKind, AdvancedSearchNotice, MaterialPiece, PATTERN_ROWS, SavedEntry, SearchSummary,
};
use super::prefs::{AdvancedSearchPrefs, BishopsKind, BoardKind, CheckKind, SearchOp, SideKind};
use crate::bases::{BaseKind, Bases, SlotId};

/// A range of the number of pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeView {
    /// Fewest.
    pub min: u8,
    /// Most.
    pub max: u8,
}

impl From<PieceRange> for RangeView {
    fn from(range: PieceRange) -> RangeView {
        RangeView {
            min: range.min,
            max: range.max,
        }
    }
}

/// The six rows of one colour, in the order of [`MaterialPiece::ALL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideView(pub [RangeView; 6]);

impl SideView {
    /// The range of a row.
    pub fn get(&self, piece: MaterialPiece) -> RangeView {
        let index = MaterialPiece::ALL
            .iter()
            .position(|p| *p == piece)
            .unwrap_or(0);
        self.0
            .get(index)
            .copied()
            .unwrap_or(RangeView { min: 0, max: 0 })
    }
}

impl From<SideRanges> for SideView {
    fn from(side: SideRanges) -> SideView {
        SideView([
            side.queen.into(),
            side.rook.into(),
            side.bishop.into(),
            side.knight.into(),
            side.minor.into(),
            side.pawn.into(),
        ])
    }
}

/// A row of patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternView {
    /// The piece, or none for an empty row.
    pub piece: Option<Piece>,
    /// The file, or any.
    pub file: Option<File>,
    /// The rank, or any.
    pub rank: Option<Rank>,
    /// Yes (the piece must be there) or No.
    pub present: bool,
}

impl From<Pattern> for PatternView {
    fn from(p: Pattern) -> PatternView {
        PatternView {
            piece: p.piece,
            file: p.file,
            rank: p.rank,
            present: p.present,
        }
    }
}

/// A common ending or pattern; the name is the Fluent key
/// `search-<key>` (for instance `search-ending-rooks`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetView {
    /// The index to send back.
    pub index: usize,
    /// The stable name.
    pub key: &'static str,
}

/// An open database the search may be aimed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseChoice {
    /// The slot.
    pub slot: SlotId,
    /// The file name, empty for the Clipbase.
    pub name: String,
    /// Whether it is the Clipbase.
    pub clipbase: bool,
    /// How many games it holds.
    pub games: u32,
}

/// The search by position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionView {
    /// The operation on the filter.
    pub op: SearchOp,
    /// What it compares.
    pub kind: BoardKind,
    /// "Look in variations".
    pub variations: bool,
    /// "Ignore colors".
    pub ignore_colors: bool,
    /// The text of the FEN field (possibly empty) when the search uses it, or
    /// `None` for the position on the board.
    pub fen: Option<String>,
    /// The FEN of the position on the board.
    pub board_fen: String,
    /// The slot to search, or `None` for the current one.
    pub reference: Option<SlotId>,
}

/// The search by material and patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialView {
    /// The operation on the filter.
    pub op: SearchOp,
    /// The ranges of White.
    pub white: SideView,
    /// The ranges of Black.
    pub black: SideView,
    /// The range of the material difference, or none.
    pub diff: Option<(i16, i16)>,
    /// The colour of the bishops' squares.
    pub bishops: BishopsKind,
    /// First move number.
    pub move_min: u16,
    /// Last move number.
    pub move_max: u16,
    /// "Match for at least" this many half moves.
    pub min_plies: u16,
    /// "End pos only".
    pub end_only: bool,
    /// "Ignore colors".
    pub ignore_colors: bool,
    /// The pattern rows ([`PATTERN_ROWS`] of them).
    pub patterns: Vec<PatternView>,
    /// The slot to search, or `None` for the current one.
    pub reference: Option<SlotId>,
}

/// The search by moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovesView {
    /// The operation on the filter.
    pub op: SearchOp,
    /// The words typed.
    pub text: String,
    /// Who plays the first move.
    pub side: SideKind,
    /// The check test.
    pub check: CheckKind,
    /// "Look in variations".
    pub variations: bool,
    /// The slot to search, or `None` for the current one.
    pub reference: Option<SlotId>,
}

/// A search in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunningView {
    /// Which search.
    pub kind: AdvancedKind,
    /// The slot whose filter will change.
    pub slot: SlotId,
    /// Games done and games to do (0 until the first report).
    pub progress: (u64, u64),
}

/// Everything the window draws.
#[derive(Debug, Clone, PartialEq)]
pub struct AdvancedSearchView {
    /// The tab on show.
    pub tab: AdvancedKind,
    /// The search in flight, if any.
    pub running: Option<RunningView>,
    /// The position tab.
    pub position: PositionView,
    /// The material tab.
    pub material: MaterialView,
    /// The moves tab.
    pub moves: MovesView,
    /// The open databases, the Clipbase last.
    pub databases: Vec<DatabaseChoice>,
    /// The common endings of the material tab (`Common Endings`).
    pub endings: Vec<PresetView>,
    /// The common patterns of the material tab (`Common Patterns`).
    pub patterns: Vec<PresetView>,
    /// The saved searches.
    pub saved: Vec<SavedEntry>,
    /// Whether searches can be saved (there is a folder for them).
    pub can_save: bool,
    /// How the last search went.
    pub summary: Option<SearchSummary>,
    /// The remark about the form or the last operation.
    pub issue: Option<AdvancedSearchNotice>,
}

pub(crate) fn databases(bases: &Bases) -> Vec<DatabaseChoice> {
    bases
        .iter()
        .map(|(slot, base)| DatabaseChoice {
            slot,
            name: base
                .path
                .as_deref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            clipbase: base.kind == BaseKind::Clipbase,
            games: base.total(),
        })
        .collect()
}

fn material_view(
    search: &MaterialSearch,
    prefs: &AdvancedSearchPrefs,
    reference: Option<SlotId>,
) -> MaterialView {
    let mut patterns: Vec<PatternView> = search.patterns.iter().copied().map(Into::into).collect();
    patterns.resize(
        PATTERN_ROWS,
        PatternView {
            piece: None,
            file: None,
            rank: None,
            present: true,
        },
    );
    MaterialView {
        op: prefs.material_op,
        white: search.white.into(),
        black: search.black.into(),
        diff: search.diff,
        bishops: prefs.material_bishops,
        move_min: prefs.material_move_min,
        move_max: prefs.material_move_max,
        min_plies: prefs.material_min_plies,
        end_only: prefs.material_end_only,
        ignore_colors: prefs.material_ignore_colors,
        patterns,
        reference,
    }
}

impl AdvancedUi {
    /// The snapshot for the front-end.
    pub(crate) fn build_view(
        &self,
        prefs: &AdvancedSearchPrefs,
        bases: &Bases,
        board_fen: String,
        can_save: bool,
    ) -> AdvancedSearchView {
        AdvancedSearchView {
            tab: self.tab,
            running: self.running.as_ref().map(|r| RunningView {
                kind: r.kind,
                slot: r.slot,
                progress: self.progress,
            }),
            position: PositionView {
                op: prefs.board_op,
                kind: prefs.board_kind,
                variations: prefs.board_variations,
                ignore_colors: prefs.board_ignore_colors,
                fen: self.fen_mode.then(|| self.fen.clone()),
                board_fen,
                reference: self.reference(AdvancedKind::Position),
            },
            material: material_view(
                &self.material,
                prefs,
                self.reference(AdvancedKind::Material),
            ),
            moves: MovesView {
                op: prefs.moves_op,
                text: self.moves_text.clone(),
                side: prefs.moves_side,
                check: prefs.moves_check,
                variations: prefs.moves_variations,
                reference: self.reference(AdvancedKind::Moves),
            },
            databases: databases(bases),
            endings: material_presets()
                .iter()
                .enumerate()
                .map(|(index, p)| PresetView { index, key: p.key })
                .collect(),
            patterns: pattern_presets()
                .iter()
                .enumerate()
                .map(|(index, p)| PresetView { index, key: p.key })
                .collect(),
            saved: self.saved.clone(),
            can_save,
            summary: self.summary.clone(),
            issue: self.issue.clone(),
        }
    }
}
