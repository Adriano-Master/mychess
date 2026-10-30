//! The messages, notices and answers of the search for tactics and patterns
//! (spec M6-2, slice c).

use std::path::PathBuf;
use std::time::Duration;

use mychess_core::Square;
use mychess_db::pattern::model::{
    Condition, DiscoveredKind, MateMode, Outcome, PatternError, PatternFile, Region, RoleName,
    Scope, Who,
};
use mychess_db::{Filter, PatternHit, PatternReport};

use super::edit::ConditionKind;
use crate::advanced_search::SearchOp;
use crate::bases::SlotId;
use crate::error::RequestId;

/// The tabs of the window (D3281).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PatternTab {
    /// "Ready-made": the list of presets with their simple parameters.
    #[default]
    Presets,
    /// "Custom": the visual editor of the tree of conditions.
    Advanced,
}

impl PatternTab {
    /// The tabs, in the order the window shows them.
    pub const ALL: [PatternTab; 2] = [PatternTab::Presets, PatternTab::Advanced];
}

/// The address of a node of the tree: the index of the child to follow at
/// each level, from the root. The empty path is the root; the only child
/// of a NOT group is child 0.
pub type NodePath = Vec<usize>;

/// The three kinds of group of the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GroupKind {
    /// "All of these" (AND).
    All,
    /// "Any of these" (OR).
    Any,
    /// "Not this" (NOT).
    Not,
}

impl GroupKind {
    /// The kinds, in the order of the menu.
    pub const ALL: [GroupKind; 3] = [GroupKind::All, GroupKind::Any, GroupKind::Not];

    /// The Fluent key of the title of the group.
    pub fn title_key(self) -> &'static str {
        match self {
            GroupKind::All => "pattern-group-all",
            GroupKind::Any => "pattern-group-any",
            GroupKind::Not => "pattern-group-not",
        }
    }
}

/// Which set of squares of a node an edit is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionSlot {
    /// The region of a piece, feature or material-free condition (`Piece`,
    /// `Feature`).
    Main,
    /// The actor of a relation.
    Actor,
    /// The target of a relation.
    Target,
    /// The piece behind (pins and skewers).
    Behind,
    /// The origin of a move.
    From,
    /// The destination of a move.
    To,
}

/// Why an edit of the tree was refused (the tree is left as it was).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EditRefusal {
    /// The path does not name a node.
    BadPath,
    /// Only an AND / OR group takes new items.
    NotAGroup,
    /// The root cannot be removed or moved.
    RootCannotBeRemoved,
    /// The tree would be deeper than `MAX_DEPTH` (6).
    TooDeep,
    /// The tree would have more than `MAX_NODES` (48) nodes.
    TooManyNodes,
    /// A group would have more than `MAX_GROUP_ITEMS` (24) items; the
    /// message takes the limit as `{ $limit }`.
    GroupFull,
    /// A node cannot be moved into itself.
    MoveIntoItself,
    /// The change needs a group with exactly one item.
    NeedsOneItem,
    /// The node has no such set of squares.
    NoSuchRegion,
    /// The cursor's comment marks no square.
    NoMarks,
}

impl EditRefusal {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            EditRefusal::BadPath => "pattern-edit-refused-bad-path",
            EditRefusal::NotAGroup => "pattern-edit-refused-not-a-group",
            EditRefusal::RootCannotBeRemoved => "pattern-edit-refused-root",
            EditRefusal::TooDeep => "pattern-edit-refused-too-deep",
            EditRefusal::TooManyNodes => "pattern-edit-refused-too-many-nodes",
            EditRefusal::GroupFull => "pattern-edit-refused-group-full",
            EditRefusal::MoveIntoItself => "pattern-edit-refused-move-into-itself",
            EditRefusal::NeedsOneItem => "pattern-edit-refused-needs-one-item",
            EditRefusal::NoSuchRegion => "pattern-edit-refused-no-region",
            EditRefusal::NoMarks => "pattern-edit-refused-no-marks",
        }
    }
}

/// An edit of the pattern in the "Custom" tab (D3282). Each one is applied
/// to a copy of the tree and kept only if the result still respects the
/// limits of the model (depth, nodes, items of a group); otherwise the tree
/// is left as it was and [`PatternSearchNotice::EditRefused`] says why.
/// Wrong values inside a node (a reversed range, say) are **not** refused
/// here: the window marks the node ([`PatternSearchView::issue_path`]) and a
/// search or a save is refused until it is fixed.
///
/// [`PatternSearchView::issue_path`]: super::PatternSearchView
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PatternEdit {
    /// Selects a node (the card whose fields the panel shows), or none.
    Select(Option<NodePath>),
    /// Starts a new pattern: an empty AND group. The scope is kept.
    Clear,
    /// The name and the description of the pattern.
    SetMeta {
        /// Name (1 to 80 characters when saving).
        name: String,
        /// Description (up to 500 characters).
        description: String,
    },
    /// Appends a new condition of `kind`, with sensible default values, to
    /// the AND / OR group at `parent`.
    Add {
        /// The group.
        parent: NodePath,
        /// The kind of condition.
        kind: ConditionKind,
    },
    /// Removes a node. Removing the only child of a NOT group removes the
    /// NOT as well.
    Remove(NodePath),
    /// Replaces the node at `path` (the way to edit the fields of a card:
    /// take the node from the view, change it, send it back).
    Replace {
        /// The node.
        path: NodePath,
        /// The new content.
        condition: Box<Condition>,
    },
    /// Moves a node into the AND / OR group `parent` at `index` (past the
    /// end appends). `parent` is an address in the tree **before** the move.
    Move {
        /// The node.
        path: NodePath,
        /// The group that receives it.
        parent: NodePath,
        /// Position among its items.
        index: usize,
    },
    /// Turns an AND / OR / NOT group into another kind (a NOT takes the
    /// items of an AND / OR only if there is exactly one).
    SetGroupKind {
        /// The group.
        path: NodePath,
        /// The new kind.
        kind: GroupKind,
    },
    /// Puts the node inside a new group of `kind`.
    Wrap {
        /// The node.
        path: NodePath,
        /// The kind of the new group.
        kind: GroupKind,
    },
    /// Replaces a group that has exactly one item by that item.
    Unwrap(NodePath),
    /// Sets one set of squares of a node.
    SetRegion {
        /// The node.
        path: NodePath,
        /// Which set of squares.
        slot: RegionSlot,
        /// The new region.
        region: Region,
    },
    /// Turns one square on or off in a set of squares (it becomes
    /// `Squares`; no square left means `Anywhere`).
    ToggleSquare {
        /// The node.
        path: NodePath,
        /// Which set of squares.
        slot: RegionSlot,
        /// The square.
        square: Square,
    },
    /// Copies the squares marked on the board (the square marks of the
    /// comment at the cursor) into a set of squares.
    UseMarks {
        /// The node.
        path: NodePath,
        /// Which set of squares.
        slot: RegionSlot,
    },
}

/// A value of a parameter of the ready-made patterns (D3279). It is stored
/// as given; a value out of range is reported by
/// [`PatternSearchView::param_issue`](super::PatternSearchView) and refused
/// when the pattern is built, never approximated (rule 22 of the spec).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParamValue {
    /// The colour (white, black or either).
    By(Who),
    /// The N of "mate in N" (1 to 5).
    N(u8),
    /// Mate that was played, or any mate that was available.
    MateMode(MateMode),
    /// Mate: only first moves that give check.
    ForcingOnly(bool),
    /// Mate: nodes per position (1000 to 2 000 000).
    MaxNodes(u32),
    /// The acting piece kinds (empty: any).
    Roles(Vec<RoleName>),
    /// The victim kinds (empty: the preset's default).
    TargetRoles(Vec<RoleName>),
    /// Pin: against the king.
    AgainstKing(bool),
    /// Discovered: attack, check or double check.
    Discovery(DiscoveredKind),
    /// Sacrifice: net material lost (1 to 20).
    MinNet(u8),
    /// Sacrifice: what must follow.
    Outcome(Outcome),
    /// Sacrifice: moves to wait for it (1 to 8).
    Within(u8),
    /// Sacrifice: only in games the sacrificer won.
    RequireWin(bool),
    /// Passed pawn: advanced to this rank of its owner (2 to 7), or any.
    MinOwnRank(Option<u8>),
}

/// What the window of the search for tactics and patterns can be asked.
///
/// The operation on the filter and the scope outlive the window
/// (`[pattern_search]` of the preferences); the rest (the preset and its
/// parameters, the tree being edited, the reference database) is kept by
/// the window until the program ends.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PatternSearchMessage {
    /// Opens the window (the menu item and [`Action::SearchPattern`]).
    ///
    /// [`Action::SearchPattern`]: crate::Action
    Open,
    /// Closes the window. A search in flight is stopped (rule 30).
    Close,
    /// Shows a tab.
    SelectTab(PatternTab),
    /// Picks a ready-made pattern by its id (`mate_in`, `fork`...). The
    /// parameters go back to their defaults (the colour is kept) and the
    /// scope becomes the one the preset brings (D3279).
    SelectPreset(String),
    /// Sets one parameter of the preset.
    SetParam(ParamValue),
    /// Copies the preset, with the current parameters, to the "Custom" tab
    /// and shows it. The preset itself is never changed. The name and the
    /// description are the translated ones (D3299 item 19); empty keeps the
    /// English ones of the preset.
    Customize {
        /// Name of the new pattern.
        name: String,
        /// Description of the new pattern.
        description: String,
    },
    /// Edits the pattern of the "Custom" tab.
    Edit(PatternEdit),
    /// The operation on the filter (Reset, AND, OR; D3204).
    SetOp(SearchOp),
    /// "Search in database": the slot whose filter the search changes, or
    /// `None` for the current one. A slot that is not open is ignored.
    SetReference(Option<SlotId>),
    /// The scope: range of moves, `hold`, final position only, ignore
    /// colours (numbers are clamped: `hold` to 1..=40, moves to 0..=999).
    SetScope(Scope),

    // ---- running ----
    /// Searches with the pattern of the tab on show. Starting a search while
    /// another runs stops the old one. An expensive search (D3285) first
    /// asks for confirmation: see [`PatternSearchView::confirm`].
    ///
    /// [`PatternSearchView::confirm`]: super::PatternSearchView
    Start,
    /// The answer to "this may take a long time": `true` runs the search.
    AnswerCost(bool),
    /// Stops the search in flight; the filter is left as it was (D328).
    Stop,
    /// A tick of the front-end while a search runs: reads the progress.
    Progress,
    /// Answer of the search (see [`Effect::PatternSearch`](crate::Effect)).
    Finished {
        /// The request.
        request: RequestId,
        /// The new filter and the report, or why it could not run.
        result: Result<Box<PatternDone>, PatternFailure>,
    },
    /// The search was stopped; the filter was not changed.
    Cancelled {
        /// The request.
        request: RequestId,
    },

    // ---- occurrences ----
    /// "Test on this game": runs the pattern of the tab on show on the game
    /// of the editor only, without touching the filter (D3283).
    TestGame,
    /// Forgets the occurrences shown (and stops following the games).
    ClearHits,
    /// The next occurrence in the game (after the last, the first).
    NextHit,
    /// The previous occurrence in the game (before the first, the last).
    PrevHit,
    /// Goes to occurrence `n` (from 0).
    GoToHit(usize),
    /// Answer of the computation of the occurrences (see
    /// [`Effect::PatternHits`](crate::Effect)).
    HitsDone {
        /// The request.
        request: RequestId,
        /// The occurrences, or why they could not be computed.
        result: Result<Vec<PatternHit>, PatternFailure>,
    },

    // ---- the user's patterns ----
    /// Lists the saved patterns again.
    RefreshSaved,
    /// Saves the pattern of the tab on show under a name (replacing one with
    /// the same name). The texts are the ones the user typed; for a preset
    /// the front-end sends the translated ones (D3299 item 19).
    SaveAs {
        /// Name of the pattern.
        name: String,
        /// Description of the pattern.
        description: String,
    },
    /// Loads a saved pattern into the "Custom" tab and shows it.
    LoadSaved(String),
    /// Deletes a saved pattern.
    DeleteSaved(String),
    /// Deletes a file of the list, good or damaged (see
    /// [`SavedPattern::path`]).
    DeleteSavedFile(PathBuf),
    /// Reads a pattern file chosen by the user and adds it to the saved ones
    /// (never replacing one: a name that is taken gets ` (2)`...), then
    /// loads it.
    Import(PathBuf),
    /// Writes the pattern of the tab on show to a file chosen by the user.
    Export {
        /// The file.
        path: PathBuf,
        /// Name of the pattern (empty: the one it already has).
        name: String,
        /// Description of the pattern.
        description: String,
    },
    /// Answer of an operation on the saved patterns.
    SavedDone {
        /// The request.
        request: RequestId,
        /// What happened.
        result: Result<SavedOutcome, SavedFailure>,
    },
}

/// The result of a search that ran to the end.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternDone {
    /// The filter after the search (the copy that was searched).
    pub filter: Filter,
    /// The report of the search.
    pub report: PatternReport,
}

/// What is wrong with a pattern, in a form that can be compared and cloned.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatternIssue {
    /// A value is out of range or incoherent; `path` names the field, for
    /// instance `root.items[2].actor.region` or `params.n`.
    InvalidValue {
        /// Where.
        path: String,
    },
    /// An AND / OR group has no items.
    EmptyGroup {
        /// Where.
        path: String,
    },
    /// `mover` where only `opponent` makes sense.
    MoverNotAllowed {
        /// Where.
        path: String,
    },
    /// The tree is too deep.
    TooDeep {
        /// The limit.
        max: usize,
    },
    /// The tree has too many nodes.
    TooManyNodes {
        /// The limit.
        max: usize,
    },
    /// Anything else (a name that does not fit, for instance).
    Other,
}

impl PatternIssue {
    pub(crate) fn from_error(error: &PatternError) -> PatternIssue {
        match error {
            PatternError::InvalidValue { path, .. } => {
                PatternIssue::InvalidValue { path: path.clone() }
            }
            PatternError::EmptyGroup { path } => PatternIssue::EmptyGroup { path: path.clone() },
            PatternError::MoverNotAllowed { path } => {
                PatternIssue::MoverNotAllowed { path: path.clone() }
            }
            PatternError::TooDeep { max } => PatternIssue::TooDeep { max: *max },
            PatternError::TooManyNodes { max } => PatternIssue::TooManyNodes { max: *max },
            _ => PatternIssue::Other,
        }
    }

    /// The path of the field, if the issue has one.
    pub fn path(&self) -> Option<&str> {
        match self {
            PatternIssue::InvalidValue { path }
            | PatternIssue::EmptyGroup { path }
            | PatternIssue::MoverNotAllowed { path } => Some(path),
            _ => None,
        }
    }
}

/// Why a search or the computation of occurrences did not produce a result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatternFailure {
    /// The pattern is not valid.
    Pattern(PatternIssue),
    /// The filter does not cover the database: it changed under the search.
    FilterSize,
    /// The work thread failed.
    Internal,
}

/// The line under the window: how the last search went (D3274).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternRunSummary {
    /// The slot whose filter changed.
    pub slot: SlotId,
    /// Games in the filter now.
    pub matched: u32,
    /// Games in the filter before (AND) or in the database (the others).
    pub start_count: u32,
    /// Time spent.
    pub elapsed: Duration,
    /// Games that could not be read (they did not match).
    pub unreadable: u64,
    /// Positions where the mate search ran out of its budget: they neither
    /// match nor exclude.
    pub unknown_positions: u64,
    /// Positions evaluated.
    pub positions_examined: u64,
    /// Games skipped without being read because of the result condition.
    pub games_folded_out: u32,
}

/// What a saved-pattern operation asks for
/// ([`Effect::PatternSaved`](crate::Effect)).
#[derive(Debug, Clone, PartialEq)]
pub enum PatternCommand {
    /// List the saved patterns.
    List,
    /// Save a pattern (replacing one with the same name).
    Save(Box<PatternFile>),
    /// Load the pattern with this name.
    Load(String),
    /// Delete the pattern with this name.
    Delete(String),
    /// Delete one file of the listing (good or damaged).
    DeleteFile(PathBuf),
    /// Copy a pattern file into the folder (never replacing).
    Import(PathBuf),
    /// Write a pattern to a file.
    Export {
        /// The file.
        path: PathBuf,
        /// The pattern.
        file: Box<PatternFile>,
    },
}

/// What a saved-pattern operation did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SavedOutcome {
    /// The saved patterns, by name.
    Listed(Vec<SavedPattern>),
    /// A pattern was saved.
    Saved(String),
    /// A pattern was loaded.
    Loaded(Box<PatternFile>),
    /// A pattern was deleted.
    Deleted(String),
    /// A file was imported (this is the pattern, already stored under the
    /// name it got).
    Imported(Box<PatternFile>),
    /// A file was written.
    Exported,
}

/// A saved pattern in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedPattern {
    /// The name (the file name for a damaged file).
    pub name: String,
    /// The description.
    pub description: String,
    /// The creation date, `yyyy-mm-dd`, if the file has one.
    pub created: Option<String>,
    /// The file; send it to
    /// [`PatternSearchMessage::DeleteSavedFile`] to delete a damaged one.
    pub path: PathBuf,
    /// The file could not be read; it cannot be loaded.
    pub damaged: bool,
}

/// Why a saved-pattern operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SavedFailure {
    /// There is no pattern with this name.
    NotFound,
    /// The name is not acceptable.
    InvalidName,
    /// The file is too large to be a pattern.
    TooLarge,
    /// The file is not a MyChess pattern (not text, bad TOML, other format).
    NotAPattern,
    /// The file was saved by a newer version of the program.
    NewerVersion,
    /// The pattern in the file is not valid.
    Invalid(PatternIssue),
    /// A file operation failed.
    Io,
    /// The work thread failed.
    Internal,
}

impl SavedFailure {
    pub(crate) fn from_error(error: &PatternError) -> SavedFailure {
        match error {
            PatternError::NotFound(_) => SavedFailure::NotFound,
            PatternError::InvalidName(_) => SavedFailure::InvalidName,
            PatternError::TooLarge { .. } => SavedFailure::TooLarge,
            PatternError::NotText | PatternError::Toml(_) | PatternError::WrongFormat { .. } => {
                SavedFailure::NotAPattern
            }
            PatternError::UnsupportedVersion { .. } => SavedFailure::NewerVersion,
            PatternError::Io(_) => SavedFailure::Io,
            other => SavedFailure::Invalid(PatternIssue::from_error(other)),
        }
    }
}

/// A remark about the search for patterns, for the status bar or the line of
/// the window. The front-end has a Fluent message for each key.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatternSearchNotice {
    /// The pattern is not valid (the `path` of the issue names the card).
    InvalidPattern(PatternIssue),
    /// A parameter of the preset is out of range; the text is the name of
    /// the field (`n`, `min_net`, `within`...).
    InvalidParam(String),
    /// There is no preset with this id (an empty id has its own message,
    /// which does not name it).
    UnknownPreset(String),
    /// An edit of the tree was refused.
    EditRefused(EditRefusal),
    /// The database to search is not open.
    NoDatabase,
    /// The database or its filter changed while the search ran: the result
    /// was dropped.
    Stale,
    /// The search was stopped; the filter is as it was.
    Interrupted,
    /// Some games could not be read; they did not match.
    Unreadable {
        /// How many.
        count: u64,
    },
    /// Positions too complex to check (mate search out of budget).
    UnknownPositions {
        /// How many.
        count: u64,
    },
    /// The search failed for a reason that is not the user's.
    Failed,
    /// The occurrences in the game could not be computed.
    HitsFailed,
    /// There is no folder for the user's patterns.
    NoSavedFolder,
    /// A pattern was saved.
    Saved(String),
    /// A saved pattern was deleted.
    Deleted(String),
    /// There is no saved pattern with this name.
    SavedNotFound(String),
    /// The name of a pattern is not acceptable.
    InvalidSavedName,
    /// A saved pattern could not be read or written.
    SavedFailed,
    /// A pattern file was imported (the name it got).
    Imported(String),
    /// A pattern file could not be imported.
    ImportFailed,
    /// A pattern was exported.
    Exported,
    /// A pattern could not be exported.
    ExportFailed,
    /// The file is not a pattern the program can read.
    FileUnreadable,
    /// The file was saved by a newer version of the program.
    NewerVersion,
}

impl PatternSearchNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            PatternSearchNotice::InvalidPattern(issue) => match issue {
                PatternIssue::EmptyGroup { .. } => "pattern-error-empty-group",
                PatternIssue::TooDeep { .. } => "pattern-error-too-deep",
                PatternIssue::TooManyNodes { .. } => "pattern-error-too-many-nodes",
                PatternIssue::MoverNotAllowed { .. } => "pattern-error-mover-not-allowed",
                PatternIssue::InvalidValue { .. } | PatternIssue::Other => "pattern-error-invalid",
            },
            PatternSearchNotice::InvalidParam(_) => "pattern-error-invalid-param",
            PatternSearchNotice::UnknownPreset(id) if id.is_empty() => {
                "pattern-notice-unknown-preset-blank"
            }
            PatternSearchNotice::UnknownPreset(_) => "pattern-notice-unknown-preset",
            PatternSearchNotice::EditRefused(why) => why.message_key(),
            PatternSearchNotice::NoDatabase => "pattern-notice-no-database",
            PatternSearchNotice::Stale => "pattern-notice-stale",
            PatternSearchNotice::Interrupted => "pattern-interrupted",
            PatternSearchNotice::Unreadable { .. } => "pattern-unreadable",
            PatternSearchNotice::UnknownPositions { .. } => "pattern-unknown",
            PatternSearchNotice::Failed => "pattern-notice-failed",
            PatternSearchNotice::HitsFailed => "pattern-notice-hits-failed",
            PatternSearchNotice::NoSavedFolder => "pattern-notice-no-saved-folder",
            PatternSearchNotice::Saved(_) => "pattern-notice-saved",
            PatternSearchNotice::Deleted(_) => "pattern-notice-deleted",
            PatternSearchNotice::SavedNotFound(_) => "pattern-notice-saved-not-found",
            PatternSearchNotice::InvalidSavedName => "pattern-notice-invalid-saved-name",
            PatternSearchNotice::SavedFailed => "pattern-notice-saved-failed",
            PatternSearchNotice::Imported(_) => "pattern-notice-imported",
            PatternSearchNotice::ImportFailed => "pattern-notice-import-failed",
            PatternSearchNotice::Exported => "pattern-notice-exported",
            PatternSearchNotice::ExportFailed => "pattern-notice-export-failed",
            PatternSearchNotice::FileUnreadable => "pattern-error-file",
            PatternSearchNotice::NewerVersion => "pattern-error-newer-version",
        }
    }

    /// Whether the remark is a refusal of the form (the window marks the
    /// card or the field) and not just information.
    pub fn is_form_error(&self) -> bool {
        matches!(
            self,
            PatternSearchNotice::InvalidPattern(_)
                | PatternSearchNotice::InvalidParam(_)
                | PatternSearchNotice::UnknownPreset(_)
                | PatternSearchNotice::EditRefused(_)
        )
    }

    pub(crate) fn from_error(error: &PatternError) -> PatternSearchNotice {
        PatternSearchNotice::InvalidPattern(PatternIssue::from_error(error))
    }
}
