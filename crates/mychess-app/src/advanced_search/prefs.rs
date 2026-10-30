//! The mirrors of the engine's options that the front-end may name, and the
//! `[advanced_search]` table of `preferences.toml` (spec M6-1, D3249 item 5).
//!
//! `mychess-gui` cannot name `mychess-db`, so [`SearchOp`], [`BoardKind`],
//! [`CheckKind`], [`SideKind`] and [`BishopsKind`] mirror
//! `FilterOp`, `BoardMatch`, `CheckTest`, `SideFilter` and `BishopColors` and
//! convert to them.

use mychess_db::{BishopColors, BoardMatch, CheckTest, FilterOp, SideFilter};
use serde::{Serialize, Serializer};

macro_rules! mirror {
    ($(#[$meta:meta])* $name:ident($engine:ty) { $($(#[$vmeta:meta])* $variant:ident => $text:literal => $to:expr),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// Every value, in the order a list shows them.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The spelling used in the preferences file.
            pub fn name(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }

            /// Reads the spelling used in the preferences file.
            pub fn from_name(text: &str) -> Option<$name> {
                match text { $($text => Some($name::$variant),)+ _ => None }
            }

            pub(crate) fn engine(self) -> $engine {
                match self { $($name::$variant => $to),+ }
            }
        }
    };
}

mirror! {
    /// How a search combines its result with the filter (the "Filter
    /// Setting" box of the Scid, one per tab here, D3204).
    SearchOp(FilterOp) {
        /// "Reset filter": search every game (the default of the Scid).
        #[default]
        Reset => "reset" => FilterOp::Reset,
        /// "Restrict filter (AND)".
        And => "and" => FilterOp::And,
        /// "Add to filter (OR)".
        Or => "or" => FilterOp::Or,
    }
}

mirror! {
    /// What a position search compares besides the side to move and the
    /// counts of pieces.
    BoardKind(BoardMatch) {
        /// All pieces on the same squares.
        #[default]
        Exact => "exact" => BoardMatch::Exact,
        /// All pawns on the same squares.
        Pawns => "pawns" => BoardMatch::Pawns,
        /// The same pawns on every file.
        Files => "files" => BoardMatch::Files,
        /// Only the material.
        Material => "material" => BoardMatch::Material,
    }
}

mirror! {
    /// The check test of the search by moves.
    CheckKind(CheckTest) {
        /// No marker is added to the generated SAN.
        #[default]
        No => "no" => CheckTest::No,
        /// `+` is added to the check moves.
        Check => "check" => CheckTest::Check,
        /// `+` and `#` are added.
        Mate => "mate" => CheckTest::Mate,
    }
}

mirror! {
    /// Who plays the first move of the search by moves.
    SideKind(SideFilter) {
        /// White.
        White => "white" => SideFilter::White,
        /// Black.
        Black => "black" => SideFilter::Black,
        /// Either.
        #[default]
        Both => "both" => SideFilter::Both,
    }
}

mirror! {
    /// The colour of the squares of the two bishops of a material search.
    BishopsKind(BishopColors) {
        /// No restriction.
        #[default]
        Either => "either" => BishopColors::Either,
        /// Both bishops on squares of the same colour.
        Same => "same" => BishopColors::Same,
        /// Bishops on squares of opposite colours.
        Opposite => "opposite" => BishopColors::Opposite,
    }
}

impl CheckKind {
    pub(crate) fn from_engine(test: CheckTest) -> CheckKind {
        match test {
            CheckTest::No => CheckKind::No,
            CheckTest::Check => CheckKind::Check,
            CheckTest::Mate => CheckKind::Mate,
        }
    }
}

impl SearchOp {
    pub(crate) fn from_engine(op: FilterOp) -> SearchOp {
        match op {
            FilterOp::And => SearchOp::And,
            FilterOp::Or => SearchOp::Or,
            FilterOp::Reset => SearchOp::Reset,
        }
    }
}

impl BoardKind {
    pub(crate) fn from_engine(kind: BoardMatch) -> BoardKind {
        match kind {
            BoardMatch::Exact => BoardKind::Exact,
            BoardMatch::Pawns => BoardKind::Pawns,
            BoardMatch::Files => BoardKind::Files,
            BoardMatch::Material => BoardKind::Material,
        }
    }
}

impl SideKind {
    pub(crate) fn from_engine(side: SideFilter) -> SideKind {
        match side {
            SideFilter::White => SideKind::White,
            SideFilter::Black => SideKind::Black,
            SideFilter::Both => SideKind::Both,
        }
    }
}

impl BishopsKind {
    pub(crate) fn from_engine(colors: BishopColors) -> BishopsKind {
        match colors {
            BishopColors::Either => BishopsKind::Either,
            BishopColors::Same => BishopsKind::Same,
            BishopColors::Opposite => BishopsKind::Opposite,
        }
    }
}

/// The largest move number of the range of a material search (the window of
/// the Scid stops at 999).
pub const MAX_MOVE_NUMBER: u16 = 999;
/// The most half moves a material search may ask for ("match for at least").
pub const MAX_MIN_PLIES: u16 = 99;

/// The options that outlive the window, in `[advanced_search]` of
/// `preferences.toml`: the operation of each tab and the last options of the
/// three searches (D3249 item 5). What is typed (the FEN, the moves, the
/// counts and the patterns) is not saved: the saved searches are for that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdvancedSearchPrefs {
    /// Operation of the position tab.
    pub board_op: SearchOp,
    /// What the position search compares.
    pub board_kind: BoardKind,
    /// "Look in variations" of the position search.
    pub board_variations: bool,
    /// "Ignore colors" of the position search.
    pub board_ignore_colors: bool,
    /// Operation of the material tab.
    pub material_op: SearchOp,
    /// "Ignore colors" of the material search.
    pub material_ignore_colors: bool,
    /// "End pos only" of the material search.
    pub material_end_only: bool,
    /// The colour of the bishops' squares.
    pub material_bishops: BishopsKind,
    /// First move number of the range (0..=999).
    pub material_move_min: u16,
    /// Last move number of the range (0..=999).
    pub material_move_max: u16,
    /// "Match for at least" this many half moves (0..=99).
    pub material_min_plies: u16,
    /// Operation of the moves tab.
    pub moves_op: SearchOp,
    /// Who plays the first move.
    pub moves_side: SideKind,
    /// The check test.
    pub moves_check: CheckKind,
    /// "Look in variations" of the search by moves.
    pub moves_variations: bool,
}

impl Default for AdvancedSearchPrefs {
    fn default() -> AdvancedSearchPrefs {
        AdvancedSearchPrefs {
            board_op: SearchOp::Reset,
            board_kind: BoardKind::Exact,
            board_variations: false,
            board_ignore_colors: false,
            material_op: SearchOp::Reset,
            material_ignore_colors: false,
            material_end_only: false,
            material_bishops: BishopsKind::Either,
            material_move_min: 1,
            material_move_max: MAX_MOVE_NUMBER,
            material_min_plies: 1,
            moves_op: SearchOp::Reset,
            moves_side: SideKind::Both,
            moves_check: CheckKind::No,
            moves_variations: false,
        }
    }
}

impl AdvancedSearchPrefs {
    /// The preferences with every number inside its range.
    #[must_use]
    pub fn sanitized(mut self) -> AdvancedSearchPrefs {
        self.material_move_min = self.material_move_min.min(MAX_MOVE_NUMBER);
        self.material_move_max = self.material_move_max.min(MAX_MOVE_NUMBER);
        self.material_min_plies = self.material_min_plies.min(MAX_MIN_PLIES);
        self
    }
}

#[derive(Serialize)]
struct Table {
    board_op: &'static str,
    board_kind: &'static str,
    board_variations: bool,
    board_ignore_colors: bool,
    material_op: &'static str,
    material_ignore_colors: bool,
    material_end_only: bool,
    material_bishops: &'static str,
    material_move_min: u16,
    material_move_max: u16,
    material_min_plies: u16,
    moves_op: &'static str,
    moves_side: &'static str,
    moves_check: &'static str,
    moves_variations: bool,
}

impl Serialize for AdvancedSearchPrefs {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Table {
            board_op: self.board_op.name(),
            board_kind: self.board_kind.name(),
            board_variations: self.board_variations,
            board_ignore_colors: self.board_ignore_colors,
            material_op: self.material_op.name(),
            material_ignore_colors: self.material_ignore_colors,
            material_end_only: self.material_end_only,
            material_bishops: self.material_bishops.name(),
            material_move_min: self.material_move_min,
            material_move_max: self.material_move_max,
            material_min_plies: self.material_min_plies,
            moves_op: self.moves_op.name(),
            moves_side: self.moves_side.name(),
            moves_check: self.moves_check.name(),
            moves_variations: self.moves_variations,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Preferences, load_preferences, render_preferences};

    #[test]
    fn names_round_trip_and_convert() {
        for op in SearchOp::ALL {
            assert_eq!(SearchOp::from_name(op.name()), Some(*op));
            assert_eq!(SearchOp::from_engine(op.engine()), *op);
        }
        for kind in BoardKind::ALL {
            assert_eq!(BoardKind::from_name(kind.name()), Some(*kind));
            assert_eq!(BoardKind::from_engine(kind.engine()), *kind);
        }
        for kind in CheckKind::ALL {
            assert_eq!(CheckKind::from_engine(kind.engine()), *kind);
        }
        for kind in SideKind::ALL {
            assert_eq!(SideKind::from_engine(kind.engine()), *kind);
        }
        for kind in BishopsKind::ALL {
            assert_eq!(BishopsKind::from_engine(kind.engine()), *kind);
        }
        assert_eq!(SearchOp::from_name("AND"), None);
        assert_eq!(SearchOp::default(), SearchOp::Reset);
    }

    #[test]
    fn the_file_round_trips() {
        let prefs = Preferences {
            advanced_search: AdvancedSearchPrefs {
                board_op: SearchOp::And,
                board_kind: BoardKind::Files,
                board_variations: true,
                board_ignore_colors: true,
                material_op: SearchOp::Or,
                material_ignore_colors: true,
                material_end_only: true,
                material_bishops: BishopsKind::Opposite,
                material_move_min: 0,
                material_move_max: 40,
                material_min_plies: 3,
                moves_op: SearchOp::And,
                moves_side: SideKind::Black,
                moves_check: CheckKind::Mate,
                moves_variations: true,
            },
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("[advanced_search]"), "{text}");
        let loaded = load_preferences(&text);
        assert_eq!(loaded.prefs.advanced_search, prefs.advanced_search);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn a_file_without_the_table_has_the_defaults_and_no_issue() {
        let loaded = load_preferences("[board]\nanimation_ms = 100\n");
        assert_eq!(loaded.prefs.advanced_search, AdvancedSearchPrefs::default());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn bad_values_fall_back_one_by_one() {
        let loaded = load_preferences(
            "[advanced_search]\nboard_op = \"or\"\nboard_kind = \"x\"\nmaterial_move_max = 5000\n\
             moves_variations = 3\nmaterial_min_plies = 7\n",
        );
        let want = AdvancedSearchPrefs {
            board_op: SearchOp::Or,
            material_min_plies: 7,
            ..AdvancedSearchPrefs::default()
        };
        assert_eq!(loaded.prefs.advanced_search, want);
        assert_eq!(loaded.issues.len(), 3, "{:?}", loaded.issues);
        let loaded = load_preferences("advanced_search = 1\n");
        assert_eq!(loaded.prefs.advanced_search, AdvancedSearchPrefs::default());
        assert_eq!(loaded.issues.len(), 1);
    }

    #[test]
    fn sanitizing_limits_the_numbers() {
        let p = AdvancedSearchPrefs {
            material_move_min: 5000,
            material_move_max: 1000,
            material_min_plies: 500,
            ..AdvancedSearchPrefs::default()
        }
        .sanitized();
        assert_eq!(
            (
                p.material_move_min,
                p.material_move_max,
                p.material_min_plies
            ),
            (999, 999, 99)
        );
    }
}
