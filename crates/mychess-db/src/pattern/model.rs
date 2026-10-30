//! The pattern model: a serializable tree of conditions (spec M6-2, D3251-D3258,
//! D3276), its validation and the TOML file format.
//!
//! Field and variant names are the **contract** of the file format
//! (`docs/formatos/padrao-mychess.md`): any change, even an optional new
//! field, raises [`PATTERN_VERSION`]. Unknown fields are errors.

use std::fmt;
use std::str::FromStr;

use mychess_core::{Role, Square};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The `format` key of a pattern file.
pub const PATTERN_FORMAT: &str = "mychess-pattern";
/// The only file version this build writes and reads.
pub const PATTERN_VERSION: u32 = 1;
/// The largest pattern file read (checked before reading).
pub const MAX_PATTERN_FILE_BYTES: u64 = 256 * 1024;
/// The deepest tree accepted (a leaf alone is depth 1).
pub const MAX_DEPTH: usize = 6;
/// The most nodes (groups and leaves) in a tree.
pub const MAX_NODES: usize = 48;
/// The most items in an AND / OR group.
pub const MAX_GROUP_ITEMS: usize = 24;
/// The longest pattern name, in characters.
pub const MAX_NAME_CHARS: usize = 80;
/// The longest description, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 500;
/// The most squares in a [`Region::Squares`].
pub const MAX_REGION_SQUARES: usize = 64;

/// Errors of the pattern model and of its files. Never a panic.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PatternError {
    /// A file operation failed.
    #[error("pattern file I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The pattern name cannot be used for a file.
    #[error("invalid pattern name: {0}")]
    InvalidName(String),
    /// No pattern with that name.
    #[error("pattern not found: {0}")]
    NotFound(String),
    /// The file is larger than [`MAX_PATTERN_FILE_BYTES`].
    #[error("pattern file too large: {size} bytes (limit {max})")]
    TooLarge {
        /// The size found.
        size: u64,
        /// The limit.
        max: u64,
    },
    /// The file is not text (it has NUL bytes).
    #[error("pattern file is not text")]
    NotText,
    /// The TOML could not be read.
    #[error("invalid TOML: {0}")]
    Toml(String),
    /// `format` is not `mychess-pattern`.
    #[error("not a MyChess pattern file (format {found:?})")]
    WrongFormat {
        /// The `format` found (empty if missing).
        found: String,
    },
    /// `version` is not one this build understands.
    #[error("unsupported pattern file version {found}")]
    UnsupportedVersion {
        /// The version found.
        found: i64,
    },
    /// A value is out of range or incoherent. `path` points at the field,
    /// for example `root.items[2].actor.region`.
    #[error("invalid value at {path}: {reason}")]
    InvalidValue {
        /// Where, in the tree.
        path: String,
        /// Why.
        reason: String,
    },
    /// `mover` makes no sense here: the condition is about the move that
    /// was just played, and the side to move now is the one that did not
    /// play it. Use `opponent` for the side that just moved.
    #[error("`mover` is not allowed at {path}: the side that just moved is `opponent`")]
    MoverNotAllowed {
        /// Where, in the tree.
        path: String,
    },
    /// An AND / OR group has no items.
    #[error("empty group at {path}")]
    EmptyGroup {
        /// Where, in the tree.
        path: String,
    },
    /// The tree is deeper than [`MAX_DEPTH`].
    #[error("pattern nested too deeply (limit {max})")]
    TooDeep {
        /// The limit.
        max: usize,
    },
    /// The tree has more than [`MAX_NODES`] nodes.
    #[error("pattern has too many nodes (limit {max})")]
    TooManyNodes {
        /// The limit.
        max: usize,
    },
}

fn invalid(path: &str, reason: impl Into<String>) -> PatternError {
    PatternError::InvalidValue {
        path: path.to_owned(),
        reason: reason.into(),
    }
}

// ---------------------------------------------------------------------------
// Small value types
// ---------------------------------------------------------------------------

/// A square, written as `"e4"` in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sq(pub Square);

impl Serialize for Sq {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sq {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Sq, D::Error> {
        struct SqVisitor;
        impl Visitor<'_> for SqVisitor {
            type Value = Sq;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a square such as \"e4\"")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Sq, E> {
                Square::from_str(v)
                    .map(Sq)
                    .map_err(|_| E::custom(format!("not a square: {v:?}")))
            }
        }
        deserializer.deserialize_str(SqVisitor)
    }
}

/// An inclusive range. `min <= max` is checked by validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinMax<T> {
    /// Lower bound, inclusive.
    pub min: T,
    /// Upper bound, inclusive.
    pub max: T,
}

impl<T: PartialOrd + Copy> MinMax<T> {
    /// Whether `v` is inside the range.
    pub fn contains(&self, v: T) -> bool {
        self.min <= v && v <= self.max
    }
}

/// A colour, as written in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// White.
    White,
    /// Black.
    Black,
}

/// Who a condition is about, always relative to the evaluated position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Who {
    /// White.
    White,
    /// Black.
    Black,
    /// The side to move in the evaluated position.
    Mover,
    /// The side that is not to move.
    Opponent,
    /// "White **or** Black": true if it holds for either colour.
    Any,
}

/// A piece kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleName {
    /// King.
    King,
    /// Queen.
    Queen,
    /// Rook.
    Rook,
    /// Bishop.
    Bishop,
    /// Knight.
    Knight,
    /// Pawn.
    Pawn,
}

impl RoleName {
    /// The `mychess-core` role.
    pub const fn role(self) -> Role {
        match self {
            RoleName::King => Role::King,
            RoleName::Queen => Role::Queen,
            RoleName::Rook => Role::Rook,
            RoleName::Bishop => Role::Bishop,
            RoleName::Knight => Role::Knight,
            RoleName::Pawn => Role::Pawn,
        }
    }

    /// The name of a `mychess-core` role.
    pub const fn from_role(role: Role) -> RoleName {
        match role {
            Role::King => RoleName::King,
            Role::Queen => RoleName::Queen,
            Role::Rook => RoleName::Rook,
            Role::Bishop => RoleName::Bishop,
            Role::Knight => RoleName::Knight,
            Role::Pawn => RoleName::Pawn,
        }
    }
}

/// A named set of squares with a fixed definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedRegion {
    /// d4 e4 d5 e5.
    Center,
    /// c3 to f6.
    ExtendedCenter,
    /// Files a to d.
    QueenSide,
    /// Files e to h.
    KingSide,
    /// Ranks 1 to 4.
    WhiteHalf,
    /// Ranks 5 to 8.
    BlackHalf,
    /// Light squares.
    LightSquares,
    /// Dark squares (a1 is dark).
    DarkSquares,
    /// Files a and h, ranks 1 and 8.
    Edge,
    /// The diagonals a1-h8 and h1-a8.
    LongDiagonals,
}

/// A set of squares.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Region {
    /// Every square.
    #[default]
    Anywhere,
    /// These squares (1 to 64).
    Squares(Vec<Sq>),
    /// A file, `'a'` to `'h'`.
    File(char),
    /// An absolute rank, 1 (White's first) to 8.
    Rank(u8),
    /// A rank from the point of view of the owner of the piece: `k` for
    /// White, `9 - k` for Black.
    OwnRank(u8),
    /// The rectangle with these opposite corners (any order).
    Rect {
        /// One corner.
        from: Sq,
        /// The opposite corner.
        to: Sq,
    },
    /// A named region.
    Named(NamedRegion),
}

/// The actor of a relation: whose pieces, which kinds, where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceSel {
    /// Whose pieces.
    pub side: Who,
    /// Kinds of piece; empty = any piece (the king included).
    #[serde(default)]
    pub roles: Vec<RoleName>,
    /// Where the piece stands.
    #[serde(default)]
    pub region: Region,
}

/// The target of a relation (its colour follows from the relation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSel {
    /// Kinds of piece; empty = any piece (the king included).
    #[serde(default)]
    pub roles: Vec<RoleName>,
    /// Where the piece stands.
    #[serde(default)]
    pub region: Region,
}

/// A yes / no / either filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    /// Must be so.
    Yes,
    /// Must not be so.
    No,
    /// Do not care.
    #[default]
    Both,
}

impl Flag {
    /// Whether a fact (`yes` for true) passes the filter.
    pub const fn allows(self, fact: bool) -> bool {
        match self {
            Flag::Both => true,
            Flag::Yes => fact,
            Flag::No => !fact,
        }
    }
}

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

fn one() -> u8 {
    1
}

fn two() -> u8 {
    2
}

fn three() -> u8 {
    3
}

fn yes() -> bool {
    true
}

fn default_nodes() -> u32 {
    20_000
}

/// Pieces on squares: the number of pieces of the kinds in the region is in
/// `min..=max`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceCond {
    /// Whose pieces.
    pub side: Who,
    /// Kinds of piece; empty = any (the king included).
    #[serde(default)]
    pub roles: Vec<RoleName>,
    /// Where.
    #[serde(default)]
    pub region: Region,
    /// At least this many (default 1; 0 with `max = 0` means "none").
    #[serde(default = "one")]
    pub min: u8,
    /// At most this many (none = no limit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u8>,
}

/// Bishop colours, in the style of the Scid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BishopColors {
    /// No restriction.
    #[default]
    Either,
    /// Both sides have bishops, all on one colour of square, the same colour.
    Same,
    /// Both sides have bishops, each side on one colour, different colours.
    Opposite,
}

/// Counts per piece kind of one side. `None` = no restriction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    /// Queens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queens: Option<MinMax<u8>>,
    /// Rooks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rooks: Option<MinMax<u8>>,
    /// Bishops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bishops: Option<MinMax<u8>>,
    /// Knights.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knights: Option<MinMax<u8>>,
    /// Bishops plus knights.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minors: Option<MinMax<u8>>,
    /// Pawns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pawns: Option<MinMax<u8>>,
}

impl Counts {
    /// All the restrictions, with their names, for validation.
    fn entries(&self) -> [(&'static str, Option<MinMax<u8>>); 6] {
        [
            ("queens", self.queens),
            ("rooks", self.rooks),
            ("bishops", self.bishops),
            ("knights", self.knights),
            ("minors", self.minors),
            ("pawns", self.pawns),
        ]
    }
}

/// Material: counts per side, difference (Q9 R5 B3 N3 P1, White minus Black)
/// and bishop colours.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialCond {
    /// White's counts.
    #[serde(default)]
    pub white: Counts,
    /// Black's counts.
    #[serde(default)]
    pub black: Counts,
    /// White minus Black.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<MinMax<i32>>,
    /// Bishop colours.
    #[serde(default)]
    pub bishops: BishopColors,
}

/// Which move of the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhichMove {
    /// The move that led to the position.
    Previous,
    /// The move the game plays next.
    Next,
}

/// Castling kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastleKind {
    /// King side.
    Short,
    /// Queen side.
    Long,
    /// Either.
    Any,
}

/// The previous or the next move of the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveCond {
    /// Previous or next.
    pub which: WhichMove,
    /// Who plays it (relative to the evaluated position: for `previous`,
    /// `opponent` is the side that is not to move now, i.e. the one that
    /// just played, and `mover` is rejected by validation because it can
    /// never match).
    pub by: Who,
    /// Kinds of the moving piece; empty = any.
    #[serde(default)]
    pub roles: Vec<RoleName>,
    /// Origin squares.
    #[serde(default)]
    pub from: Region,
    /// Destination squares.
    #[serde(default)]
    pub to: Region,
    /// Is a capture.
    #[serde(default)]
    pub capture: Flag,
    /// Gives check.
    #[serde(default)]
    pub check: Flag,
    /// Promotion kinds; empty = no restriction (a non-empty list requires a
    /// promotion to one of them).
    #[serde(default)]
    pub promotion: Vec<RoleName>,
    /// Castling (none = no restriction).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub castling: Option<CastleKind>,
}

/// Game phase by non-pawn material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Opening.
    Opening,
    /// Middlegame.
    Middlegame,
    /// Endgame.
    Endgame,
}

/// State of the side to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// In check.
    Check,
    /// Checkmated.
    Checkmate,
    /// Stalemated.
    Stalemate,
}

/// Result of the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultKind {
    /// 1-0.
    WhiteWins,
    /// 1/2-1/2.
    Draw,
    /// 0-1.
    BlackWins,
    /// `*` or missing.
    Unknown,
}

impl ResultKind {
    /// The result with the colours swapped (1-0 <-> 0-1).
    #[must_use]
    pub const fn swapped(self) -> ResultKind {
        match self {
            ResultKind::WhiteWins => ResultKind::BlackWins,
            ResultKind::BlackWins => ResultKind::WhiteWins,
            other => other,
        }
    }
}

/// Structural features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    /// Passed pawns.
    PassedPawn,
    /// Isolated pawns.
    IsolatedPawn,
    /// Doubled pawns (both count).
    DoubledPawns,
    /// Exposed king (pawn shield <= 1 outside the endgame).
    ExposedKing,
    /// The bishop pair. At most one pair exists per side, so
    /// [`FeatureCond::min`] is ignored for this feature.
    BishopPair,
}

/// A structural feature of a side: at least `min` instances in the region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureCond {
    /// The feature.
    pub feature: Feature,
    /// Whose.
    pub side: Who,
    /// Where the pawns (or the king, or the bishops) stand.
    #[serde(default)]
    pub region: Region,
    /// At least this many instances (default 1).
    #[serde(default = "one")]
    pub min: u8,
}

/// Relations between pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelKind {
    /// Attacks.
    Attacks,
    /// Defends (same colour).
    Defends,
    /// Pins.
    Pins,
    /// Skewers.
    Skewers,
    /// Forks.
    Forks,
}

/// What an attack must gain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gain {
    /// Any attack.
    #[default]
    Any,
    /// The target has no defender (never the king).
    Undefended,
    /// The exchange wins material (or the target is the king).
    WinsMaterial,
}

/// When a relation counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    /// Always.
    #[default]
    Any,
    /// The actor's side has just moved (the other side is to move).
    ActorJustMoved,
    /// The actor's side is to move.
    ActorToMove,
}

/// A relation between an actor and a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationCond {
    /// The relation (`relation` in the file: the tree tag is already `kind`).
    #[serde(rename = "relation")]
    pub kind: RelKind,
    /// The actor.
    pub actor: PieceSel,
    /// The target (an enemy for attacks, pins, skewers and forks; a piece of
    /// the same colour for defends).
    pub target: TargetSel,
    /// Pins and skewers: the piece behind (`roles = ["king"]` = absolute pin).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behind: Option<TargetSel>,
    /// Attacks only.
    #[serde(default)]
    pub gain: Gain,
    /// Forks only: 2 to 8 targets.
    #[serde(default = "two")]
    pub min_targets: u8,
    /// When.
    #[serde(default)]
    pub when: When,
    /// Only instances that the last move created.
    #[serde(default)]
    pub new_this_move: bool,
}

/// Kinds of discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveredKind {
    /// A discovered attack.
    Attack,
    /// A discovered check.
    Check,
    /// A double check.
    DoubleCheck,
}

/// A discovered attack, check or double check made by the last move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveredCond {
    /// Which discovery (`discovery` in the file).
    pub discovery: DiscoveredKind,
    /// Who moved. Relative to the evaluated position, which is the one
    /// *after* the move: the side that just played is `opponent` (the side
    /// to move now never made it), so `mover` is rejected by validation.
    pub by: Who,
    /// Kinds of the piece that moved; empty = any.
    #[serde(default)]
    pub roles: Vec<RoleName>,
}

/// A double attack made by the last move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoubleAttackCond {
    /// Who moved. Relative to the evaluated position, which is the one
    /// *after* the move: the side that just played is `opponent`, so
    /// `mover` is rejected by validation.
    pub by: Who,
}

/// What must follow a sacrifice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The material comes back.
    Recovered,
    /// Mate is given.
    Mate,
    /// Either of the two.
    #[default]
    Either,
    /// Nothing is required.
    Any,
}

/// A sacrifice read from the line that was played.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SacrificeCond {
    /// Who sacrifices (must be to move in the position).
    pub by: Who,
    /// Kinds of the piece given; empty = any but the king.
    #[serde(default)]
    pub roles: Vec<RoleName>,
    /// Net material lost, 1 to 20 (default 2).
    #[serde(default = "two")]
    pub min_net: u8,
    /// What must follow.
    #[serde(default)]
    pub outcome: Outcome,
    /// How many moves of the sacrificer to wait for it, 1 to 8 (default 3).
    #[serde(default = "three")]
    pub within: u8,
}

/// How a mate is looked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MateMode {
    /// Only where the game itself ends in the mate (cheap).
    Played,
    /// In every position (expensive).
    #[default]
    Available,
}

/// A forced mate for the side to move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MateCond {
    /// Who mates (`opponent` is invalid).
    pub by: Who,
    /// Shortest allowed mate, 1 to 8.
    pub moves_min: u8,
    /// Longest allowed mate, 1 to 8.
    pub moves_max: u8,
    /// How to look.
    #[serde(default)]
    pub mode: MateMode,
    /// `available`: the first move must give check.
    #[serde(default = "yes")]
    pub forcing_only: bool,
    /// Node budget per position, 1000 to 2 000 000. With `forcing_only` it
    /// is split equally among the checking first moves
    /// (`max_nodes / number of checks`, at least 1), so the work per position
    /// is bounded and the answer deterministic; a check that exhausts its
    /// share makes the position `Unknown`.
    #[serde(default = "default_nodes")]
    pub max_nodes: u32,
}

/// The tree. `kind` is the tag in the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    /// AND (1 to 24 items).
    All {
        /// The children.
        items: Vec<Condition>,
    },
    /// OR (1 to 24 items).
    Any {
        /// The children.
        items: Vec<Condition>,
    },
    /// NOT.
    Not {
        /// The child.
        item: Box<Condition>,
    },
    /// Pieces on squares.
    Piece(PieceCond),
    /// Material.
    Material(MaterialCond),
    /// The side to move.
    SideToMove {
        /// The side.
        side: Side,
    },
    /// The game phase.
    Phase {
        /// The phase.
        phase: Phase,
    },
    /// Check, checkmate or stalemate of the side to move.
    Status {
        /// The state.
        status: Status,
    },
    /// The previous or next move.
    Move(MoveCond),
    /// The result of the game (any of these).
    Result {
        /// The accepted results.
        results: Vec<ResultKind>,
    },
    /// A structural feature.
    Feature(FeatureCond),
    /// A relation between pieces.
    Relation(RelationCond),
    /// A discovery.
    Discovered(DiscoveredCond),
    /// A double attack.
    DoubleAttack(DoubleAttackCond),
    /// A sacrifice.
    Sacrifice(SacrificeCond),
    /// A forced mate.
    Mate(MateCond),
}

/// Where in the game the pattern is looked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Scope {
    /// First move number of White (`0` includes the initial position).
    pub min_move: u16,
    /// Last move number.
    pub max_move: u16,
    /// Consecutive plies the pattern must hold (1 to 40).
    pub hold: u8,
    /// Only the final position.
    pub end_only: bool,
    /// Also try with the colours swapped.
    pub ignore_colors: bool,
}

impl Default for Scope {
    fn default() -> Scope {
        Scope {
            min_move: 1,
            max_move: 999,
            hold: 1,
            end_only: false,
            ignore_colors: false,
        }
    }
}

/// A complete pattern: what is saved, loaded and run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    /// Name, 1 to 80 characters, no control characters.
    pub name: String,
    /// Description, up to 500 characters.
    #[serde(default)]
    pub description: String,
    /// Where to look.
    #[serde(default)]
    pub scope: Scope,
    /// The tree.
    pub root: Condition,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

struct Walk {
    nodes: usize,
}

impl Pattern {
    /// Checks the name, description, scope and the whole tree. Never panics.
    ///
    /// # Errors
    ///
    /// The first problem found, as a [`PatternError`] whose `path` (for
    /// `InvalidValue` and `EmptyGroup`) points at the offending field.
    pub fn validate(&self) -> Result<(), PatternError> {
        let name_len = self.name.chars().count();
        if name_len == 0 || name_len > MAX_NAME_CHARS {
            return Err(invalid("name", format!("1 to {MAX_NAME_CHARS} characters")));
        }
        if self.name.chars().any(char::is_control) {
            return Err(invalid("name", "control characters are not allowed"));
        }
        if self.description.chars().count() > MAX_DESCRIPTION_CHARS {
            return Err(invalid(
                "description",
                format!("at most {MAX_DESCRIPTION_CHARS} characters"),
            ));
        }
        if self
            .description
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(invalid("description", "control characters are not allowed"));
        }
        validate_scope(&self.scope)?;
        validate_condition(&self.root, "root", 1, &mut Walk { nodes: 0 })
    }

    /// The number of nodes of the tree.
    pub fn node_count(&self) -> usize {
        fn count(c: &Condition) -> usize {
            match c {
                Condition::All { items } | Condition::Any { items } => {
                    1 + items.iter().map(count).sum::<usize>()
                }
                Condition::Not { item } => 1 + count(item),
                _ => 1,
            }
        }
        count(&self.root)
    }
}

fn validate_scope(scope: &Scope) -> Result<(), PatternError> {
    if !(1..=40).contains(&scope.hold) {
        return Err(invalid("scope.hold", "must be 1 to 40"));
    }
    if scope.max_move < scope.min_move {
        return Err(invalid("scope.max_move", "must not be below min_move"));
    }
    Ok(())
}

fn validate_range<T: PartialOrd>(path: &str, min: &T, max: &T) -> Result<(), PatternError> {
    if min > max {
        Err(invalid(path, "min is greater than max"))
    } else {
        Ok(())
    }
}

fn validate_region(region: &Region, path: &str) -> Result<(), PatternError> {
    match region {
        Region::Anywhere | Region::Named(_) | Region::Rect { .. } => Ok(()),
        Region::Squares(list) => {
            if list.is_empty() {
                Err(invalid(path, "no squares"))
            } else if list.len() > MAX_REGION_SQUARES {
                Err(invalid(path, "more than 64 squares"))
            } else {
                Ok(())
            }
        }
        Region::File(c) => {
            if ('a'..='h').contains(c) {
                Ok(())
            } else {
                Err(invalid(path, "file must be a to h"))
            }
        }
        Region::Rank(r) | Region::OwnRank(r) => {
            if (1..=8).contains(r) {
                Ok(())
            } else {
                Err(invalid(path, "rank must be 1 to 8"))
            }
        }
    }
}

fn validate_group(items: &[Condition], path: &str) -> Result<(), PatternError> {
    if items.is_empty() {
        return Err(PatternError::EmptyGroup {
            path: path.to_owned(),
        });
    }
    if items.len() > MAX_GROUP_ITEMS {
        return Err(invalid(path, "more than 24 items in a group"));
    }
    Ok(())
}

fn validate_condition(
    cond: &Condition,
    path: &str,
    depth: usize,
    walk: &mut Walk,
) -> Result<(), PatternError> {
    if depth > MAX_DEPTH {
        return Err(PatternError::TooDeep { max: MAX_DEPTH });
    }
    walk.nodes += 1;
    if walk.nodes > MAX_NODES {
        return Err(PatternError::TooManyNodes { max: MAX_NODES });
    }
    match cond {
        Condition::All { items } | Condition::Any { items } => {
            validate_group(items, &format!("{path}.items"))?;
            for (i, item) in items.iter().enumerate() {
                validate_condition(item, &format!("{path}.items[{i}]"), depth + 1, walk)?;
            }
            Ok(())
        }
        Condition::Not { item } => {
            validate_condition(item, &format!("{path}.item"), depth + 1, walk)
        }
        Condition::Piece(p) => {
            validate_region(&p.region, &format!("{path}.region"))?;
            if let Some(max) = p.max {
                validate_range(path, &p.min, &max)?;
            }
            Ok(())
        }
        Condition::Material(m) => validate_material(m, path),
        Condition::Move(m) => {
            if m.which == WhichMove::Previous {
                reject_mover(m.by, &format!("{path}.by"))?;
            }
            validate_region(&m.from, &format!("{path}.from"))?;
            validate_region(&m.to, &format!("{path}.to"))?;
            if m.promotion
                .iter()
                .any(|r| matches!(r, RoleName::King | RoleName::Pawn))
            {
                return Err(invalid(
                    &format!("{path}.promotion"),
                    "a pawn promotes to queen, rook, bishop or knight",
                ));
            }
            Ok(())
        }
        Condition::Result { results } => {
            if results.is_empty() {
                Err(invalid(&format!("{path}.results"), "no results"))
            } else {
                Ok(())
            }
        }
        Condition::Feature(f) => {
            validate_region(&f.region, &format!("{path}.region"))?;
            if !(1..=64).contains(&f.min) {
                return Err(invalid(&format!("{path}.min"), "must be 1 to 64"));
            }
            Ok(())
        }
        Condition::Relation(r) => validate_relation(r, path),
        Condition::Discovered(d) => reject_mover(d.by, &format!("{path}.by")),
        Condition::DoubleAttack(d) => reject_mover(d.by, &format!("{path}.by")),
        Condition::SideToMove { .. } | Condition::Phase { .. } | Condition::Status { .. } => Ok(()),
        Condition::Sacrifice(s) => {
            if !(1..=20).contains(&s.min_net) {
                return Err(invalid(&format!("{path}.min_net"), "must be 1 to 20"));
            }
            if !(1..=8).contains(&s.within) {
                return Err(invalid(&format!("{path}.within"), "must be 1 to 8"));
            }
            if s.roles.contains(&RoleName::King) {
                return Err(invalid(
                    &format!("{path}.roles"),
                    "the king cannot be sacrificed",
                ));
            }
            Ok(())
        }
        Condition::Mate(m) => {
            if m.by == Who::Opponent {
                return Err(invalid(&format!("{path}.by"), "opponent is not allowed"));
            }
            if !(1..=8).contains(&m.moves_min) || !(1..=8).contains(&m.moves_max) {
                return Err(invalid(path, "moves must be 1 to 8"));
            }
            validate_range(path, &m.moves_min, &m.moves_max)?;
            if !(1_000..=2_000_000).contains(&m.max_nodes) {
                return Err(invalid(
                    &format!("{path}.max_nodes"),
                    "must be 1000 to 2000000",
                ));
            }
            Ok(())
        }
    }
}

fn reject_mover(by: Who, path: &str) -> Result<(), PatternError> {
    if by == Who::Mover {
        Err(PatternError::MoverNotAllowed {
            path: path.to_owned(),
        })
    } else {
        Ok(())
    }
}

fn validate_material(m: &MaterialCond, path: &str) -> Result<(), PatternError> {
    for (side, counts) in [("white", &m.white), ("black", &m.black)] {
        for (name, range) in counts.entries() {
            if let Some(r) = range {
                validate_range(&format!("{path}.{side}.{name}"), &r.min, &r.max)?;
            }
        }
    }
    if let Some(d) = m.diff {
        validate_range(&format!("{path}.diff"), &d.min, &d.max)?;
    }
    Ok(())
}

fn validate_relation(r: &RelationCond, path: &str) -> Result<(), PatternError> {
    validate_region(&r.actor.region, &format!("{path}.actor.region"))?;
    validate_region(&r.target.region, &format!("{path}.target.region"))?;
    if let Some(b) = &r.behind {
        validate_region(&b.region, &format!("{path}.behind.region"))?;
        if !matches!(r.kind, RelKind::Pins | RelKind::Skewers) {
            return Err(invalid(
                &format!("{path}.behind"),
                "only pins and skewers have a piece behind",
            ));
        }
    }
    match r.kind {
        RelKind::Forks => {
            if !(2..=8).contains(&r.min_targets) {
                return Err(invalid(&format!("{path}.min_targets"), "must be 2 to 8"));
            }
        }
        _ => {
            if r.min_targets != 2 {
                return Err(invalid(
                    &format!("{path}.min_targets"),
                    "only forks use min_targets",
                ));
            }
        }
    }
    if r.kind != RelKind::Attacks && r.gain != Gain::Any {
        return Err(invalid(&format!("{path}.gain"), "only attacks use gain"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The file
// ---------------------------------------------------------------------------

/// A pattern as stored in a file: the pattern plus the optional creation
/// date (`yyyy-mm-dd`, given by the caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternFile {
    /// The pattern.
    pub pattern: Pattern,
    /// Creation date, `yyyy-mm-dd`.
    pub created: Option<String>,
}

/// The wire form: scalar keys first, then the tables, in this order.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRepr {
    format: String,
    version: u32,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created: Option<String>,
    #[serde(default)]
    scope: Scope,
    root: Condition,
}

/// Only `format` and `version`, read leniently so that a file of a future
/// version is reported as such instead of as an unknown field.
#[derive(Deserialize)]
struct Header {
    format: Option<String>,
    version: Option<i64>,
}

fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Deepest table nesting accepted before handing the document to serde (the
/// tree allows 6 levels, each one or two TOML tables, plus the region).
const MAX_TOML_DEPTH: usize = 40;

fn toml_depth(value: &toml::Value) -> usize {
    // Iterative, so a hostile document cannot overflow the stack.
    let mut deepest = 0;
    let mut stack = vec![(value, 1usize)];
    while let Some((v, d)) = stack.pop() {
        deepest = deepest.max(d);
        if deepest > MAX_TOML_DEPTH {
            return deepest;
        }
        match v {
            toml::Value::Table(t) => stack.extend(t.values().map(|x| (x, d + 1))),
            toml::Value::Array(a) => stack.extend(a.iter().map(|x| (x, d + 1))),
            _ => {}
        }
    }
    deepest
}

impl PatternFile {
    /// Wraps a pattern with no creation date.
    pub fn new(pattern: Pattern) -> PatternFile {
        PatternFile {
            pattern,
            created: None,
        }
    }

    /// Serializes to TOML (`format`, `version`, scalars, `[scope]`,
    /// `[root]`), validating first.
    ///
    /// # Errors
    ///
    /// Validation errors, or [`PatternError::Toml`] if the serializer fails.
    pub fn to_toml(&self) -> Result<String, PatternError> {
        self.pattern.validate()?;
        if let Some(date) = &self.created
            && !valid_date(date)
        {
            return Err(invalid("created", "expected yyyy-mm-dd"));
        }
        // `format` and `version` first, then the fields in struct order.
        let repr = FileRepr {
            format: PATTERN_FORMAT.to_owned(),
            version: PATTERN_VERSION,
            name: self.pattern.name.clone(),
            description: self.pattern.description.clone(),
            created: self.created.clone(),
            scope: self.pattern.scope.clone(),
            root: self.pattern.root.clone(),
        };
        toml::to_string(&repr).map_err(|e| PatternError::Toml(e.to_string()))
    }

    /// Reads the bytes of a pattern file: size limit, NUL check, UTF-8 (BOM
    /// allowed) or Windows-1252 text, `format` and `version`, unknown fields,
    /// then [`Pattern::validate`].
    ///
    /// # Errors
    ///
    /// [`PatternError::TooLarge`], [`PatternError::NotText`],
    /// [`PatternError::Toml`], [`PatternError::WrongFormat`],
    /// [`PatternError::UnsupportedVersion`] or a validation error.
    pub fn from_bytes(bytes: &[u8]) -> Result<PatternFile, PatternError> {
        let size = bytes.len() as u64;
        if size > MAX_PATTERN_FILE_BYTES {
            return Err(PatternError::TooLarge {
                size,
                max: MAX_PATTERN_FILE_BYTES,
            });
        }
        if bytes.contains(&0) {
            return Err(PatternError::NotText);
        }
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let text = match std::str::from_utf8(bytes) {
            Ok(s) => std::borrow::Cow::Borrowed(s),
            Err(_) => encoding_rs::WINDOWS_1252.decode(bytes).0,
        };
        PatternFile::from_toml_str(&text)
    }

    /// Like [`PatternFile::from_bytes`] for text already decoded.
    ///
    /// # Errors
    ///
    /// See [`PatternFile::from_bytes`].
    pub fn from_toml_str(text: &str) -> Result<PatternFile, PatternError> {
        if text.len() as u64 > MAX_PATTERN_FILE_BYTES {
            return Err(PatternError::TooLarge {
                size: text.len() as u64,
                max: MAX_PATTERN_FILE_BYTES,
            });
        }
        let value: toml::Value =
            toml::from_str(text).map_err(|e| PatternError::Toml(e.to_string()))?;
        if toml_depth(&value) > MAX_TOML_DEPTH {
            return Err(PatternError::TooDeep { max: MAX_DEPTH });
        }
        let header: Header = value
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| PatternError::Toml(e.to_string()))?;
        if header.format.as_deref() != Some(PATTERN_FORMAT) {
            return Err(PatternError::WrongFormat {
                found: header.format.unwrap_or_default(),
            });
        }
        match header.version {
            Some(v) if v == i64::from(PATTERN_VERSION) => {}
            Some(v) => return Err(PatternError::UnsupportedVersion { found: v }),
            None => return Err(PatternError::UnsupportedVersion { found: 0 }),
        }
        let repr: FileRepr = value
            .try_into()
            .map_err(|e: toml::de::Error| PatternError::Toml(e.to_string()))?;
        if let Some(date) = &repr.created
            && !valid_date(date)
        {
            return Err(invalid("created", "expected yyyy-mm-dd"));
        }
        let file = PatternFile {
            pattern: Pattern {
                name: repr.name,
                description: repr.description,
                scope: repr.scope,
                root: repr.root,
            },
            created: repr.created,
        };
        file.pattern.validate()?;
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Pattern {
        Pattern {
            name: "Sample".into(),
            description: String::new(),
            scope: Scope::default(),
            root: Condition::Not {
                item: Box::new(Condition::SideToMove { side: Side::Black }),
            },
        }
    }

    #[test]
    fn pattern_roundtrips_through_toml() {
        let file = PatternFile::new(sample());
        let text = file.to_toml().expect("serialize");
        assert!(text.contains("kind = \"not\""), "{text}");
        assert_eq!(PatternFile::from_toml_str(&text).expect("reload"), file);
    }

    #[test]
    fn pattern_validation_rejects_an_empty_group() {
        let mut p = sample();
        p.root = Condition::All { items: vec![] };
        assert!(matches!(p.validate(), Err(PatternError::EmptyGroup { .. })));
        assert_eq!(sample().node_count(), 2);
    }
}
