//! Messages, effects, requests and notices of the maintenance tools (spec
//! `docs/specs/m7-1-manutencao.md`, slice M7-1b).
//!
//! The front-end sends a [`MaintRequest`] with the choices of a dialog; the
//! state checks it, asks for the confirmation that shows the count of what
//! will be affected, and starts one job of the game list. The job answers
//! with [`MaintenanceMessage::Done`].

use std::path::PathBuf;

use mychess_db::maintenance::{
    CheckReport, CleanerReport, DatabaseStats, DuplicateScan, GameDiff, SharedTag, TagListing,
};
use mychess_db::si4::{
    CompactGamesReport, CompactNamesReport, GameFlags, NameEditReport, RepairReport, RewriteReport,
};
use mychess_db::{CompactionStats, NameUsage};

use super::prefs::{MaintenancePrefs, NameScopeChoice, ScopeChoice, SortCriterion};
use crate::bases::{BaseError, FileStamp, OpenControl, SharedSource};
use crate::error::RequestId;
use crate::spelling_ui::Shared;

/// The ten kinds of job of the maintenance (the `ListJobKind`s that the spec
/// names `MaintFlags`, `MaintTwins`...). They run as jobs of the game list
/// (one at a time, with progress and Stop); the list reports them as
/// `ListJobKind::Flags`, and [`AppState::maintenance_job`](crate::AppState::maintenance_job)
/// says which one it is, because adding variants to `ListJobKind` would break
/// the exhaustive `match` of the front-end (the label is a task of the
/// interface slices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaintJobKind {
    /// Flags in batch, the header fields and the removal of deleted games
    /// from the filter.
    Flags,
    /// The search for duplicate games and the marking of what it found.
    Twins,
    /// "Check games".
    Check,
    /// Compacting the name file and/or the game file.
    Compact,
    /// Sorting the index.
    Sort,
    /// Removing comments and/or variations.
    Strip,
    /// The extra tags: list, filter, add, remove.
    Tags,
    /// The name editor.
    Names,
    /// Repairing a database that is not open.
    Repair,
    /// The Cleaner.
    Cleaner,
}

/// The games a flag operation takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagScope {
    /// The game in the editor, if it came from this database.
    Current,
    /// The games of the filter (frozen when the job starts).
    Filter,
    /// Every game.
    All,
}

/// What a flag operation does (`0`, `1` and `invert` of `sc_game flag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagAction {
    /// Turn it on.
    Set,
    /// Turn it off.
    Clear,
    /// Flip it.
    Toggle,
}

/// A flag the user can edit (the 19 of the Scid; the three derived ones
/// cannot be).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaintFlag {
    /// `D`: deleted.
    Delete,
    /// `W`: White opening.
    WhiteOpening,
    /// `B`: Black opening.
    BlackOpening,
    /// `M`: middlegame.
    Middlegame,
    /// `E`: endgame.
    Endgame,
    /// `N`: novelty.
    Novelty,
    /// `P`: pawn structure.
    PawnStructure,
    /// `T`: tactics.
    Tactics,
    /// `K`: kingside.
    Kingside,
    /// `Q`: queenside.
    Queenside,
    /// `!`: brilliancy.
    Brilliancy,
    /// `?`: blunder.
    Blunder,
    /// `U`: user.
    User,
    /// One of the six custom flags, `1..=6`.
    Custom(u8),
}

impl MaintFlag {
    /// The flag as the database knows it; `None` for a custom number out of
    /// `1..=6`.
    pub(crate) fn bits(self) -> Option<GameFlags> {
        Some(match self {
            MaintFlag::Delete => GameFlags::DELETE,
            MaintFlag::WhiteOpening => GameFlags::WHITE_OPENING,
            MaintFlag::BlackOpening => GameFlags::BLACK_OPENING,
            MaintFlag::Middlegame => GameFlags::MIDDLEGAME,
            MaintFlag::Endgame => GameFlags::ENDGAME,
            MaintFlag::Novelty => GameFlags::NOVELTY,
            MaintFlag::PawnStructure => GameFlags::PAWN_STRUCTURE,
            MaintFlag::Tactics => GameFlags::TACTICS,
            MaintFlag::Kingside => GameFlags::KINGSIDE,
            MaintFlag::Queenside => GameFlags::QUEENSIDE,
            MaintFlag::Brilliancy => GameFlags::BRILLIANCY,
            MaintFlag::Blunder => GameFlags::BLUNDER,
            MaintFlag::User => GameFlags::USER,
            MaintFlag::Custom(1) => GameFlags::CUSTOM1,
            MaintFlag::Custom(2) => GameFlags::CUSTOM2,
            MaintFlag::Custom(3) => GameFlags::CUSTOM3,
            MaintFlag::Custom(4) => GameFlags::CUSTOM4,
            MaintFlag::Custom(5) => GameFlags::CUSTOM5,
            MaintFlag::Custom(6) => GameFlags::CUSTOM6,
            MaintFlag::Custom(_) => return None,
        })
    }

    /// The index of the flag among the 22 counters of [`DatabaseStats`].
    pub fn counter(self) -> Option<usize> {
        self.bits().map(|f| f.bits().trailing_zeros() as usize)
    }
}

/// What the Cleaner-less "remove comments/variations" removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripChoice {
    /// Comments (and the commands embedded in them).
    Comments,
    /// Variations.
    Variations,
}

/// A change of the header of the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderEdit {
    /// The type of the database (`0..=31`).
    BaseType(u32),
    /// The description (at most 107 bytes).
    Description(String),
    /// The description of one custom flag (`1..=6`, at most 8 bytes).
    CustomFlag {
        /// The flag, `1..=6`.
        number: u8,
        /// The description.
        text: String,
    },
    /// The game to load when the database opens (`0` none).
    AutoLoad(u32),
}

/// The extra tags request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagRequest {
    /// Lists the extra tags with the number of games of each.
    List,
    /// The filter becomes the games that have the tag.
    Filter(String),
    /// Adds (or replaces) a tag in the games of the scope.
    Add {
        /// The tag name.
        name: String,
        /// The value.
        value: String,
    },
    /// Removes every occurrence of the tag from the games of the scope.
    Strip(String),
}

/// What the name editor changes (the seven types of the Scid).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameKindEdit {
    /// A player.
    Player,
    /// An event.
    Event,
    /// A site.
    Site,
    /// A round.
    Round,
}

/// The request of the name editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameRequestKind {
    /// Replace a name of one kind by another.
    Name {
        /// The kind.
        kind: NameKindEdit,
        /// The name (or the pattern) to replace.
        old: String,
        /// The new name.
        new: String,
    },
    /// Set the rating of a player.
    Rating {
        /// The player (name or pattern).
        player: String,
        /// The Elo, `0..=4000`.
        elo: u16,
        /// The rating type, `0..=15` (0 = Elo).
        rating_type: u8,
    },
    /// Replace a game date (`YYYY.MM.DD`, `?` for unknown parts).
    Date {
        /// The date to replace (`*` for any).
        old: String,
        /// The new date.
        new: String,
    },
    /// Replace an event date.
    EventDate {
        /// The date to replace (`*` for any).
        old: String,
        /// The new date.
        new: String,
    },
}

/// A request of the name editor with its scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameRequest {
    /// What to change.
    pub kind: NameRequestKind,
    /// `*` and `?` in the old name are wildcards.
    pub pattern: bool,
    /// The games it looks at.
    pub scope: NameScopeChoice,
}

/// Which side of a pair of duplicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwinSide {
    /// The game on the left (the lower number).
    Left,
    /// The game on the right.
    Right,
}

/// What the user asks of the maintenance tools.
#[derive(Debug, Clone, PartialEq)]
pub enum MaintRequest {
    /// Flags of the games of a scope.
    Flags {
        /// The games.
        scope: FlagScope,
        /// The flag.
        flag: MaintFlag,
        /// What to do with it.
        action: FlagAction,
    },
    /// A change of the header.
    Header(HeaderEdit),
    /// Compact the name file and/or the game file.
    Compact {
        /// Compact the names.
        names: bool,
        /// Compact the games.
        games: bool,
    },
    /// Sort the index (at most 15 criteria, one direction).
    Sort {
        /// The criteria, first to last.
        keys: Vec<SortCriterion>,
        /// The whole order reversed.
        reverse: bool,
        /// Compact the games afterwards.
        compact_after: bool,
    },
    /// Search for duplicate games with the criteria of the preferences.
    FindTwins,
    /// Remove the games with the `D` flag from the filter.
    DropDeletedFromFilter,
    /// Check the games.
    Check {
        /// All the games or the filter.
        scope: ScopeChoice,
    },
    /// Repair a database that is not open.
    Repair {
        /// The `.si4` file.
        path: PathBuf,
        /// Also drop the index entries whose record cannot be read.
        drop_unreadable: bool,
    },
    /// The extra tags.
    Tags(TagRequest),
    /// Remove comments or variations.
    Strip {
        /// What.
        what: StripChoice,
        /// All the games or the filter.
        scope: ScopeChoice,
    },
    /// The name editor.
    Names(NameRequest),
    /// The Cleaner with the steps of the preferences.
    Cleaner,
}

/// What a confirmation is about (for the dialog's text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Mark the duplicates for deletion.
    MarkTwins,
    /// Undelete every game before the search (and mark afterwards).
    UndeleteFirst,
    /// Undelete every game of the database (`U` of the checker).
    UndeleteAll,
    /// Compact (games deleted for good).
    Compact,
    /// Sort.
    Sort,
    /// Remove comments or variations.
    Strip,
    /// Add a tag.
    TagAdd,
    /// Remove a tag.
    TagStrip,
    /// The name editor.
    Names,
    /// Repair.
    Repair,
    /// The Cleaner.
    Cleaner,
    /// Share the tags of a pair of duplicates.
    ShareTags,
    /// The criteria of the search for duplicates are weak (the warnings are
    /// in [`ConfirmView::weak`]).
    WeakTwinCriteria,
}

/// The question the dialog shows before something that cannot be undone.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmView {
    /// What it is about.
    pub kind: ConfirmKind,
    /// How many games the operation will affect (the count to show; `0`
    /// when it is not a count of games).
    pub count: u32,
    /// The weak-criteria warnings of the duplicates (`twinCriteriaOK`).
    pub weak: Vec<WeakCriteria>,
    /// The user may ask for a copy of the three files first.
    pub backup_offered: bool,
    /// The user asked for it.
    pub backup: bool,
}

/// The two warnings of `twinCriteriaOK`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeakCriteria {
    /// The moves are not compared and one of colours, event, site, year or
    /// month is off.
    NoMoves,
    /// Fewer than two of site, round and year are on.
    FewFields,
}

/// Where the job writes and what it needs to check first.
#[derive(Debug, Clone, PartialEq)]
pub struct MaintJob {
    pub(crate) source: SharedSource,
    /// The `.si4` file (`None` for the Clipbase).
    pub(crate) path: Option<PathBuf>,
    pub(crate) stamp: Option<FileStamp>,
    pub(crate) expected_total: u32,
    pub(crate) op: MaintOp,
    /// Copy the three files before the first write.
    pub(crate) backup: bool,
}

/// The work of one job, with everything it needs already resolved (numbers,
/// copies of the filter, the spelling file...).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MaintOp {
    Flags {
        games: GameSet,
        flag: GameFlags,
        action: mychess_db::si4::FlagOp,
    },
    Header(HeaderEdit),
    Compact {
        names: bool,
        games: bool,
    },
    Sort {
        keys: Vec<mychess_db::si4::SortKey>,
        reverse: bool,
        compact_after: bool,
    },
    Twins {
        criteria: mychess_db::maintenance::DuplicateCriteria,
        filter: Option<Box<mychess_db::Filter>>,
    },
    /// The marking after the confirmation: deletes, copies ratings.
    MarkTwins {
        scan: Shared<DuplicateScan>,
        copy_ratings: bool,
    },
    UndeleteAll,
    ShareTags {
        games: (u32, u32),
    },
    DropDeleted {
        filter: Box<mychess_db::Filter>,
    },
    Check {
        filter: Option<Box<mychess_db::Filter>>,
    },
    Repair {
        drop_unreadable: bool,
        /// The files of the databases that are open: the repair refuses one
        /// of them.
        open: Vec<PathBuf>,
    },
    TagList {
        filter: Option<Box<mychess_db::Filter>>,
    },
    TagFilter {
        tag: String,
        filter: Option<Box<mychess_db::Filter>>,
    },
    TagEdit {
        op: mychess_db::si4::TagOp,
        games: GameSet,
    },
    Strip {
        what: mychess_db::si4::StripWhat,
        games: GameSet,
    },
    Names {
        edit: mychess_db::si4::NameEdit,
        games: NameJobGames,
    },
    Cleaner(Box<super::cleaner::CleanerJob>),
}

/// The games a job looks at, resolved inside the job (a filter of a million
/// games is not walked by `update`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GameSet {
    /// These games.
    Numbers(Vec<u32>),
    /// The games of this filter (a copy made when the job was asked for).
    Filter(Box<mychess_db::Filter>),
    /// Every game.
    All,
}

impl GameSet {
    /// The numbers, ascending.
    ///
    /// # Errors
    ///
    /// [`BaseError::Stale`] when the filter does not cover `total` games.
    pub(crate) fn resolve(&self, total: u32) -> Result<Vec<u32>, BaseError> {
        match self {
            GameSet::Numbers(list) => Ok(list.clone()),
            GameSet::Filter(filter) if filter.len() == total => Ok(filter.iter_games().collect()),
            GameSet::Filter(_) => Err(BaseError::Stale),
            GameSet::All => Ok((1..=total).collect()),
        }
    }
}

/// The games the name editor works on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NameJobGames {
    All,
    Set(GameSet),
    /// The tournament of this game (selected inside the job).
    Tournament(u32),
}

/// The size of the files now and after a compaction, for the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactPreview {
    /// The game file.
    pub stats: CompactionStats,
    /// Names in total and unused: players, events, sites, rounds.
    pub names: [NameUsage; 4],
}

/// What a preview has to count (opaque: built by the state).
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewJob(pub(crate) PreviewWork);

/// What a preview counted.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreviewResult {
    /// Games of the scope that have comments or variations to remove.
    Strip(u32),
    /// The numbers of the compaction dialog.
    Compact(CompactPreview),
}

/// The effect that counts what a confirmation will show.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreviewWork {
    Strip {
        what: mychess_db::si4::StripWhat,
        games: GameSet,
        total: u32,
    },
    Compact,
}

/// What a job reports, by kind.
#[derive(Debug, Clone, PartialEq)]
pub enum MaintReport {
    /// Nothing to report (a header change, a refusal before the work).
    Nothing,
    /// Flags: how many entries changed.
    Flags {
        /// Entries that changed.
        changed: u32,
    },
    /// The filter without the deleted games.
    FilterWithoutDeleted {
        /// The new filter.
        filter: Shared<mychess_db::Filter>,
        /// The games taken out.
        removed: u32,
    },
    /// The compaction.
    Compact {
        /// The game file, if it was compacted.
        games: Option<CompactGamesReport>,
        /// The name file, if it was compacted.
        names: Option<CompactNamesReport>,
    },
    /// The sort.
    Sort {
        /// Games in the index.
        games: u32,
        /// Games that changed position.
        moved: u32,
        /// The permutation: `order[i]` is the new position (0-based) of the
        /// game that was at `i`.
        order: Shared<Vec<u32>>,
        /// The games were compacted afterwards.
        compacted: Option<CompactGamesReport>,
    },
    /// The search for duplicates.
    Twins(Shared<DuplicateScan>),
    /// The duplicates were marked.
    Marked {
        /// Games that received the flag.
        marked: u32,
        /// Games that had the flag and lost it before the search.
        undeleted: u32,
        /// Entries that got a rating.
        ratings: u32,
    },
    /// Every game was undeleted.
    Undeleted {
        /// Games that lost the flag.
        changed: u32,
    },
    /// The check of the games.
    Check(Shared<CheckReport>),
    /// A repair.
    Repair(Shared<RepairReport>),
    /// The extra tags listing.
    TagList(Shared<TagListing>),
    /// The games that have a tag.
    TagFilter {
        /// The new filter.
        filter: Shared<mychess_db::Filter>,
        /// Games in it.
        matched: u32,
    },
    /// Strip or tag edit.
    Rewrite(RewriteReport),
    /// The name editor.
    Names(NameEditReport),
    /// The name editor refused the request (the reason is log text).
    NamesRefused(String),
    /// Tags shared between the two games of a pair of duplicates.
    TagsShared {
        /// Tags written.
        count: u32,
    },
    /// The Cleaner.
    Cleaner(Shared<CleanerReport>),
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq)]
pub struct MaintOutcome {
    /// What it did.
    pub report: MaintReport,
    /// The database opened again (a write): `None` when nothing was
    /// written or it could not be opened (`error` says why).
    pub source: Option<SharedSource>,
    /// The state of the file after the write.
    pub stamp: Option<FileStamp>,
    /// The Clipbase was changed in place.
    pub in_memory: bool,
    /// Something was written (the slot is out of date if it was not reopened).
    pub changed: bool,
    /// Why it stopped early; what was written stays.
    pub error: Option<BaseError>,
    /// The copies made before the write (and kept).
    pub backups: Vec<PathBuf>,
    /// The last step of the Cleaner wrote the `.stc` file of the database.
    pub tree_primed: bool,
}

impl MaintOutcome {
    pub(crate) fn nothing() -> MaintOutcome {
        MaintOutcome {
            report: MaintReport::Nothing,
            source: None,
            stamp: None,
            in_memory: false,
            changed: false,
            error: None,
            backups: Vec::new(),
            tree_primed: false,
        }
    }
}

/// The effects of the maintenance tools.
#[derive(Debug, Clone, PartialEq)]
pub enum MaintenanceEffect {
    /// Runs a job (the work of a list job).
    Run {
        /// The request the answer carries.
        request: RequestId,
        /// The work.
        job: Box<MaintJob>,
        /// Progress and Stop.
        control: OpenControl,
    },
    /// Counts the statistics of the database.
    Stats {
        /// The request the answer carries.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// Stop.
        control: OpenControl,
    },
    /// Counts what a confirmation will show.
    Preview {
        /// The request the answer carries.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// What to count.
        work: Box<PreviewJob>,
        /// Stop and progress.
        control: OpenControl,
    },
    /// Compares two games of a pair of duplicates.
    Diff {
        /// The request the answer carries.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// The games.
        games: (u32, u32),
    },
    /// Works out the tags two duplicates would share.
    SharePlan {
        /// The request the answer carries.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// The games.
        games: (u32, u32),
    },
}

/// The messages of the maintenance tools.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MaintenanceMessage {
    /// Opens the window (and counts the database).
    Open,
    /// Closes the window.
    Close,
    /// Counts the database again.
    Refresh,
    /// Asks for an operation.
    Start(MaintRequest),
    /// The answer to a confirmation.
    Confirm(bool),
    /// The "copy the files first" box of the confirmation.
    SetBackup(bool),
    /// The text of the autoload field: checked here (`0..=16 777 214`) and
    /// applied.
    AutoLoadText(String),
    /// Replaces the preferences `[maintenance]`.
    SetPrefs(Box<MaintenancePrefs>),
    /// Stops the operation in flight.
    Stop,
    /// Opens the checker of duplicates on the pairs of the last search.
    OpenChecker,
    /// Closes the checker.
    CloseChecker,
    /// Goes to the next (`1`) or the previous (`-1`) pair.
    TwinStep(i8),
    /// Flips the `D` flag of a game of the pair (`1` and `2` of the Scid).
    TwinToggle(TwinSide),
    /// Flips both (`t`).
    TwinToggleBoth,
    /// Asks to undelete every game of the database (`U`).
    TwinUndeleteAll,
    /// Works out the tags the pair would share (`s`).
    TwinShare,
    /// The answer of a job.
    Done {
        /// The request of the job.
        request: RequestId,
        /// The result; `Err` when nothing could be tried.
        result: Box<Result<MaintOutcome, BaseError>>,
    },
    /// The answer of the statistics.
    StatsDone {
        /// The request.
        request: RequestId,
        /// The statistics; `None` when stopped or not available.
        stats: Option<Shared<DatabaseStats>>,
    },
    /// The answer of a preview.
    Previewed {
        /// The request.
        request: RequestId,
        /// What was counted; `None` when it failed.
        result: Box<Option<PreviewDone>>,
    },
    /// The answer of a comparison.
    Diffed {
        /// The request.
        request: RequestId,
        /// The comparison; `None` when a game cannot be read.
        diff: Option<Shared<GameDiff>>,
    },
    /// The tags a pair would share.
    SharePlanned {
        /// The request.
        request: RequestId,
        /// The tags; `None` when a game cannot be read.
        tags: Option<Shared<Vec<SharedTag>>>,
    },
}

/// A preview that came back.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewDone(pub(crate) PreviewResult);

/// A remark about the maintenance tools.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MaintNotice {
    /// The database is read-only (marked so, or its files are).
    ReadOnly,
    /// A PGN file cannot be edited.
    Pgn,
    /// The Clipbase does not take this operation.
    ClipbaseUnsupported,
    /// No game of this database is in the editor.
    NoCurrentGame,
    /// Nothing to do (no game in the scope).
    NothingToDo,
    /// The value is not valid (the autoload, the tag name...).
    InvalidValue,
    /// The sort needs between one and fifteen criteria.
    BadSort,
    /// The database is open: close it before repairing.
    RepairOpen,
    /// The file to repair is not a `.si4`.
    RepairNotSi4,
    /// The pair of the checker does not exist.
    NoPair,
    /// Flags written.
    FlagsDone {
        /// Entries that changed.
        changed: u32,
    },
    /// The header was changed.
    HeaderDone,
    /// The duplicates were found and not marked.
    TwinsFound {
        /// Games that would be deleted.
        marked: u32,
        /// Pairs kept by the protection.
        protected: u32,
    },
    /// No duplicate was found.
    TwinsNone,
    /// The duplicates were marked.
    TwinsMarked {
        /// Games marked.
        marked: u32,
        /// Games undeleted before the search.
        undeleted: u32,
    },
    /// The search was stopped.
    Interrupted,
    /// The check found no problem.
    CheckClean {
        /// Games examined.
        examined: u32,
    },
    /// The check found problems.
    CheckProblems {
        /// Games with a problem.
        problems: u32,
    },
    /// The compaction ended.
    Compacted {
        /// Games removed.
        games_removed: u32,
        /// Names removed.
        names_removed: u32,
    },
    /// The sort ended.
    Sorted {
        /// Games that changed position.
        moved: u32,
    },
    /// The filter has no deleted game now.
    FilterWithoutDeleted {
        /// Games taken out.
        removed: u32,
    },
    /// Comments or variations were removed.
    Stripped {
        /// Games rewritten.
        changed: u32,
        /// Games left alone.
        skipped: u32,
    },
    /// An extra tag was added or removed.
    TagsDone {
        /// Games rewritten.
        changed: u32,
        /// Games left alone.
        skipped: u32,
    },
    /// The filter is the games that have the tag.
    TagFiltered {
        /// Games in the filter.
        matched: u32,
    },
    /// The name editor ended.
    NamesDone {
        /// Games rewritten.
        games: u32,
        /// Changes made.
        edits: u32,
    },
    /// The name editor refused the request.
    NamesRefused,
    /// The repair ended; the copies of the original files are kept where
    /// `backups` says (`.si4` first, then `.sn4`).
    Repaired {
        /// Games in the repaired index.
        games: u32,
        /// The copies of the original files.
        backups: Vec<PathBuf>,
    },
    /// The repair failed; the original files are intact and the copies made
    /// before the failure are kept where `backups` says.
    RepairFailed {
        /// The copies of the original files (may be empty).
        backups: Vec<PathBuf>,
    },
    /// The Cleaner ended.
    CleanerDone {
        /// Steps that failed.
        failed: u32,
    },
    /// The tags of the pair were shared.
    TagsShared {
        /// Tags written.
        count: u32,
    },
    /// The pair has nothing to share.
    NothingToShare,
    /// The `.stc` cache was removed or left alone: the files copied before
    /// the work are kept.
    BackupsKept {
        /// How many files were copied.
        count: u32,
    },
    /// The windows that were open on the database show old data.
    ReportsStale,
    /// Another job holds the game list: try again when it ends.
    Busy,
    /// The work failed.
    Failed(BaseError),
}

impl MaintNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            MaintNotice::ReadOnly => "maint-notice-read-only",
            MaintNotice::Pgn => "maint-notice-pgn",
            MaintNotice::ClipbaseUnsupported => "maint-notice-clipbase",
            MaintNotice::NoCurrentGame => "maint-notice-no-current-game",
            MaintNotice::NothingToDo => "maint-notice-nothing-to-do",
            MaintNotice::InvalidValue => "maint-notice-invalid-value",
            MaintNotice::BadSort => "maint-notice-bad-sort",
            MaintNotice::RepairOpen => "maint-notice-repair-open",
            MaintNotice::RepairNotSi4 => "maint-notice-repair-not-si4",
            MaintNotice::NoPair => "maint-notice-no-pair",
            MaintNotice::FlagsDone { .. } => "maint-notice-flags-done",
            MaintNotice::HeaderDone => "maint-notice-header-done",
            MaintNotice::TwinsFound { .. } => "maint-notice-twins-found",
            MaintNotice::TwinsNone => "maint-notice-twins-none",
            MaintNotice::TwinsMarked { .. } => "maint-notice-twins-marked",
            MaintNotice::Interrupted => "maint-notice-interrupted",
            MaintNotice::CheckClean { .. } => "maint-notice-check-clean",
            MaintNotice::CheckProblems { .. } => "maint-notice-check-problems",
            MaintNotice::Compacted { .. } => "maint-notice-compacted",
            MaintNotice::Sorted { .. } => "maint-notice-sorted",
            MaintNotice::FilterWithoutDeleted { .. } => "maint-notice-filter-undeleted",
            MaintNotice::Stripped { .. } => "maint-notice-stripped",
            MaintNotice::TagsDone { .. } => "maint-notice-tags-done",
            MaintNotice::TagFiltered { .. } => "maint-notice-tag-filtered",
            MaintNotice::NamesDone { .. } => "maint-notice-names-done",
            MaintNotice::NamesRefused => "maint-notice-names-refused",
            MaintNotice::Repaired { .. } => "maint-notice-repaired",
            MaintNotice::RepairFailed { .. } => "maint-notice-repair-failed",
            MaintNotice::CleanerDone { .. } => "maint-notice-cleaner-done",
            MaintNotice::TagsShared { .. } => "maint-notice-tags-shared",
            MaintNotice::NothingToShare => "maint-notice-nothing-to-share",
            MaintNotice::BackupsKept { .. } => "maint-notice-backups-kept",
            MaintNotice::ReportsStale => "maint-notice-reports-stale",
            MaintNotice::Busy => "maint-notice-busy",
            MaintNotice::Failed(error) => error.message_key(),
        }
    }
}

/// Every Fluent key of the confirmations, the windows and the notices of the
/// maintenance (a test checks them against the two languages).
pub const MAINT_KEYS: &[&str] = &[
    "maint-notice-read-only",
    "maint-notice-pgn",
    "maint-notice-clipbase",
    "maint-notice-no-current-game",
    "maint-notice-nothing-to-do",
    "maint-notice-invalid-value",
    "maint-notice-bad-sort",
    "maint-notice-repair-open",
    "maint-notice-repair-not-si4",
    "maint-notice-no-pair",
    "maint-notice-flags-done",
    "maint-notice-header-done",
    "maint-notice-twins-found",
    "maint-notice-twins-none",
    "maint-notice-twins-marked",
    "maint-notice-interrupted",
    "maint-notice-check-clean",
    "maint-notice-check-problems",
    "maint-notice-compacted",
    "maint-notice-sorted",
    "maint-notice-filter-undeleted",
    "maint-notice-stripped",
    "maint-notice-tags-done",
    "maint-notice-tag-filtered",
    "maint-notice-names-done",
    "maint-notice-names-refused",
    "maint-notice-repaired",
    "maint-notice-repair-failed",
    "maint-notice-cleaner-done",
    "maint-notice-tags-shared",
    "maint-notice-nothing-to-share",
    "maint-notice-backups-kept",
    "maint-notice-reports-stale",
    "maint-notice-busy",
    "maint-confirm-mark-twins",
    "maint-confirm-undelete-first",
    "maint-confirm-undelete-all",
    "maint-confirm-compact",
    "maint-confirm-sort",
    "maint-confirm-strip",
    "maint-confirm-tag-add",
    "maint-confirm-tag-strip",
    "maint-confirm-names",
    "maint-confirm-repair",
    "maint-confirm-cleaner",
    "maint-confirm-share-tags",
    "maint-confirm-weak-twins",
    "maint-confirm-backup",
    "maint-weak-no-moves",
    "maint-weak-few-fields",
    "maint-cleaner-step-players",
    "maint-cleaner-step-events",
    "maint-cleaner-step-sites",
    "maint-cleaner-step-rounds",
    "maint-cleaner-step-eco",
    "maint-cleaner-step-elo",
    "maint-cleaner-step-twins",
    "maint-cleaner-step-compact-names",
    "maint-cleaner-step-compact-games",
    "maint-cleaner-step-tree",
    "maint-cleaner-status-done",
    "maint-cleaner-status-skipped",
    "maint-cleaner-status-interrupted",
    "maint-cleaner-status-failed",
    "maint-cleaner-detail-no-spelling",
    "maint-cleaner-detail-no-eco-book",
    "maint-cleaner-detail-no-elo",
    "maint-cleaner-detail-stale",
    "maint-cleaner-detail-failed",
    "maint-cleaner-detail-stopped-before",
    "maint-check-cause-index",
    "maint-check-cause-record",
    "maint-check-cause-decode",
    "maint-check-cause-names",
    "maint-check-cause-illegal-move",
    "maint-repair-backup-kept",
];

/// A tag list and the scope it was made for, kept while the window shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct TagListView {
    /// The tags and their counts.
    pub listing: Shared<TagListing>,
    /// The scope the list covers.
    pub scope: ScopeChoice,
}
