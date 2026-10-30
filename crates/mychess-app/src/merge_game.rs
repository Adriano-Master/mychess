//! "Merge game" (spec `docs/specs/m7-6-mesclar-partida.md`, slice M7-6b,
//! D3850-D3899; Scid: `sc_game merge`, `mergeGame` in `tcl/end.tcl`).
//!
//! The tree work is [`Game::merge_game`] in `mychess-core`; this module is
//! the application around it:
//!
//! - [`MergeMessage`]: open the dialog on a game of a database, on a list
//!   of games (the batch of the opening report) or on the first games of a
//!   filter; choose the last ply; apply; cancel;
//! - [`Effect::LoadGamesForMerge`](crate::Effect): reads the games of the
//!   database off the interface thread; the answer carries the
//!   [`RequestId`] of the request and an older one is dropped;
//! - [`MergeView`]: the snapshot the dialog draws (the preview, the new
//!   nodes, the report);
//! - [`MergeNotice`]: the remarks, with their Fluent keys;
//! - [`MergePrefs`]: the `[merge]` table.
//!
//! The flow is in `state/merge_flow.rs`. The preview is worked out in
//! `update` (it is a copy of the game in memory, O(n)); only the reading of
//! the games is an effect.
//!
//! The comment before the first merged move is built here in neutral English
//! ([`MergeCommentParts::english`]); the front-end may send the translated
//! text with [`MergeMessage::Apply`].

mod prefs;

use std::sync::Arc;

use mychess_core::{
    Game, MergeCommentParts, MergeError, MergeOptions, MergeReport, NodeId, merge_comment_parts,
};

use crate::bases::{SharedSource, SlotId};
use crate::error::RequestId;
use crate::notation::{NotationDoc, NotationLayout, NotationOptions};
use crate::shared::SharedGame;

pub use prefs::{DEFAULT_END_PLY, MAX_END_PLY, MergePrefs};

/// The most games one batch merges (spec question 5: 1 to 200).
pub const BATCH_MAX: usize = 200;

/// The "up to move" buttons of the dialog, in full moves (`end.tcl` 149):
/// the button `n` means the ply `2 * n`. "All" is `None`.
pub const END_MOVE_CHOICES: [u32; 8] = [5, 10, 15, 20, 25, 30, 35, 40];

/// The last ply (counted in the merged game) that the button `moves` stands
/// for.
pub fn end_ply_of_moves(moves: u32) -> u32 {
    moves.saturating_mul(2)
}

/// An event of the merge dialog.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MergeMessage {
    /// "Merge into the current game...": reads game `number` (from 1) of
    /// the database in `slot` and, when it can be merged, opens the dialog
    /// with the preview.
    Open {
        /// The database.
        slot: SlotId,
        /// The game, from 1.
        number: u32,
    },
    /// "Merge games" of the opening report (spec section 4): merges these
    /// games, one after the other, with no limit of plies. The games that
    /// cannot be merged are skipped and counted, never an error.
    OpenBatch {
        /// The database.
        slot: SlotId,
        /// The games, from 1 (at most [`BATCH_MAX`]).
        numbers: Vec<u32>,
    },
    /// Like [`MergeMessage::OpenBatch`] on the first `limit` games of the
    /// filter of `slot`, in ascending order.
    OpenFilter {
        /// The database.
        slot: SlotId,
        /// How many games at most (limited to `1..=`[`BATCH_MAX`]).
        limit: u32,
    },
    /// The answer of [`Effect::LoadGamesForMerge`](crate::Effect).
    Loaded {
        /// The request it answers.
        request: RequestId,
        /// The games asked for, in the order asked.
        games: Vec<(u32, Result<SharedGame, MergeLoadError>)>,
    },
    /// "Up to move": the last ply of the merged game to copy (`None` is
    /// "All"). Ignored in a batch. Remembered in the preferences.
    SetEndPly(Option<u32>),
    /// "OK": merges into the game of the editor, as one step of the undo,
    /// and closes the dialog.
    Apply {
        /// The text of the comment before the first merged move, already
        /// translated; `None` uses the neutral English one and an empty
        /// text adds no comment. Ignored in a batch.
        comment: Option<String>,
    },
    /// "Cancel": closes the dialog, or drops the reading of the games in
    /// flight, and changes nothing.
    Cancel,
}

/// Why a game could not be read for the merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeLoadError {
    /// The game number is not in the database (`merge-err-number`).
    Number,
    /// The game could not be read or decoded (`merge-err-decode`).
    Decode,
}

/// What a batch did (spec rule 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BatchReport {
    /// Games merged.
    pub merged: u32,
    /// Skipped: the game of the editor itself.
    pub skipped_same: u32,
    /// Skipped: no position in common.
    pub skipped_no_match: u32,
    /// Skipped: the game ends at the matching position.
    pub skipped_nothing: u32,
    /// Skipped: the game could not be read, or the tree refused its moves.
    pub skipped_unreadable: u32,
}

impl BatchReport {
    /// The games that were not merged.
    pub fn skipped(&self) -> u32 {
        self.skipped_same
            .saturating_add(self.skipped_no_match)
            .saturating_add(self.skipped_nothing)
            .saturating_add(self.skipped_unreadable)
    }
}

/// A remark about the merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeNotice {
    /// The game to merge is not chosen (`merge-err-no-game`).
    NoGame,
    /// The database is not open (`merge-err-closed`).
    Closed,
    /// The game number does not exist (`merge-err-number`).
    InvalidNumber,
    /// The game is the one in the editor (`merge-err-self`).
    SameGame,
    /// The game of the editor has no moves (`merge-err-empty`).
    EmptyGame,
    /// The game could not be read (`merge-err-decode`).
    Decode,
    /// No position in common (`merge-err-nomatch`).
    NoMatch,
    /// The game ends at the matching position (`merge-err-nothing`).
    NothingToAdd,
    /// The tree refused a move of the game (`merge-err-failed`).
    Failed,
    /// Trial mode is on (`merge-err-trial`).
    Trial,
    /// A batch of more than [`BATCH_MAX`] games (`merge-err-batch-limit`).
    TooMany,
    /// A batch ended (`merge-batch-done`).
    BatchDone {
        /// Games merged.
        merged: u32,
        /// Games left out.
        skipped: u32,
    },
}

impl MergeNotice {
    /// The Fluent key of the message. `merge-err-*` are the reasons that
    /// "Unable to merge the selected game: { $reason }" (`merge-err-unable`)
    /// wraps.
    pub fn message_key(self) -> &'static str {
        match self {
            MergeNotice::NoGame => "merge-err-no-game",
            MergeNotice::Closed => "merge-err-closed",
            MergeNotice::InvalidNumber => "merge-err-number",
            MergeNotice::SameGame => "merge-err-self",
            MergeNotice::EmptyGame => "merge-err-empty",
            MergeNotice::Decode => "merge-err-decode",
            MergeNotice::NoMatch => "merge-err-nomatch",
            MergeNotice::NothingToAdd => "merge-err-nothing",
            MergeNotice::Failed => "merge-err-failed",
            MergeNotice::Trial => "merge-err-trial",
            MergeNotice::TooMany => "merge-err-batch-limit",
            MergeNotice::BatchDone { .. } => "merge-batch-done",
        }
    }

    /// Whether the notice is a refusal that the front-end shows wrapped in
    /// `merge-err-unable` (the Scid's "Unable to merge the selected game").
    pub fn is_refusal(self) -> bool {
        !matches!(self, MergeNotice::BatchDone { .. })
    }
}

impl From<&MergeError> for MergeNotice {
    fn from(error: &MergeError) -> MergeNotice {
        match error {
            MergeError::NoMoves => MergeNotice::EmptyGame,
            MergeError::NoMatch => MergeNotice::NoMatch,
            MergeError::NothingToAdd => MergeNotice::NothingToAdd,
            _ => MergeNotice::Failed,
        }
    }
}

impl From<MergeLoadError> for MergeNotice {
    fn from(error: MergeLoadError) -> MergeNotice {
        match error {
            MergeLoadError::Number => MergeNotice::InvalidNumber,
            MergeLoadError::Decode => MergeNotice::Decode,
        }
    }
}

/// A game read for the merge.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MergeSource {
    pub(crate) number: u32,
    pub(crate) game: SharedGame,
}

/// The reading of the games in flight.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PendingLoad {
    pub(crate) request: RequestId,
    pub(crate) slot: SlotId,
    pub(crate) batch: bool,
    /// Games left out before reading (the game of the editor itself).
    pub(crate) skipped_same: u32,
    /// Games left out before reading (numbers outside the database).
    pub(crate) skipped_invalid: u32,
}

/// What a merge gives: the game with the merge and the reports.
#[derive(Debug, Clone)]
pub(crate) struct Outcome {
    pub(crate) preview: Game,
    /// The notation of `preview`, built once per outcome: the dialog redraws
    /// on every message and must not walk the tree (and write a SAN per
    /// node) each time (spec D3881).
    pub(crate) doc: Arc<NotationDoc>,
    /// The report of the single merge.
    pub(crate) report: Option<MergeReport>,
    /// The counts (a single merge counts as one merged game).
    pub(crate) batch: BatchReport,
}

/// The dialog on show.
#[derive(Debug, Clone)]
pub(crate) struct MergeDialog {
    pub(crate) slot: SlotId,
    pub(crate) batch: bool,
    pub(crate) sources: Vec<MergeSource>,
    /// "Up to move": `None` is "All". Always `None` in a batch.
    pub(crate) end_ply: Option<u32>,
    /// Games left out before they were merged, counted in the report: the
    /// game of the editor itself.
    pub(crate) skipped_same: u32,
    /// Likewise: numbers outside the database and games that did not read.
    pub(crate) skipped_unreadable: u32,
    pub(crate) outcome: Outcome,
    /// The description of the game merged (single merge).
    pub(crate) parts: Option<MergeCommentParts>,
    /// [`GameSession::changes`](crate::GameSession::changes) when `outcome`
    /// was worked out: if the game of the editor moved on since (a remote
    /// move, an observed game), the preview is stale (spec D3884).
    pub(crate) changes: u64,
}

/// The options of the notation of the preview: the structure of the game,
/// with variations as blocks of their own and no comments, NAGs or result.
pub(crate) fn preview_options() -> NotationOptions {
    NotationOptions {
        layout: NotationLayout::Indented,
        indent_comments: false,
        move_number_space: false,
        show_comments: false,
        show_result: false,
        figurines: false,
        ..NotationOptions::default()
    }
}

/// The state of the merge: the reading in flight and the dialog.
#[derive(Debug, Default)]
pub struct MergeUi {
    pub(crate) pending: Option<PendingLoad>,
    pub(crate) dialog: Option<MergeDialog>,
    /// How many notation documents of the preview were built (for tests: the
    /// view must not build one).
    pub(crate) doc_builds: u32,
}

impl MergeUi {
    /// Whether the dialog is open.
    pub fn is_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// Drops the reading and the dialog (the program is ending, or the
    /// user cancelled).
    pub(crate) fn cancel(&mut self) {
        self.pending = None;
        self.dialog = None;
    }

    /// The database `slot` closed or was emptied: what depends on it goes.
    pub(crate) fn slot_gone(&mut self, slot: SlotId) {
        if self.pending.as_ref().is_some_and(|p| p.slot == slot) {
            self.pending = None;
        }
        if self.dialog.as_ref().is_some_and(|d| d.slot == slot) {
            self.dialog = None;
        }
    }
}

/// What the merge dialog draws.
#[derive(Debug, Clone, Copy)]
pub struct MergeView<'a> {
    /// A batch of games (no "up to move" buttons; the dialog asks to
    /// confirm).
    pub batch: bool,
    /// The database of the games.
    pub slot: SlotId,
    /// The games to merge, from 1.
    pub numbers: MergeNumbers<'a>,
    /// "Up to move" as a ply; `None` is "All".
    pub end_ply: Option<u32>,
    /// The game of the editor as it is now.
    pub original: &'a Game,
    /// The game of the editor with the merge done.
    pub preview: &'a Game,
    /// The notation of `preview` for the dialog, built once per outcome.
    pub preview_doc: &'a NotationDoc,
    /// The report of a single merge (`None` in a batch).
    pub report: Option<MergeReport>,
    /// The counts: of a batch, or `merged = 1` for a single merge.
    pub batch_report: BatchReport,
    /// The description of the game merged, for the comment (`None` in a
    /// batch).
    pub comment_parts: Option<&'a MergeCommentParts>,
}

/// The numbers of the games of a dialog.
#[derive(Debug, Clone, Copy)]
pub struct MergeNumbers<'a>(pub(crate) &'a [MergeSource]);

impl MergeNumbers<'_> {
    /// How many games.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there is none.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The numbers, in order.
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().map(|s| s.number)
    }
}

impl MergeView<'_> {
    /// Whether `node` of the preview was created by the merge (the dialog
    /// highlights these nodes).
    pub fn is_new(&self, node: NodeId) -> bool {
        self.preview.contains(node) && !self.original.contains(node)
    }

    /// The block and item of the first merged move in `preview_doc`, to
    /// scroll to it.
    pub fn first_position(&self) -> Option<(usize, usize)> {
        self.preview_doc.find_move(self.first()?)
    }

    /// The first merged node, if a single merge.
    pub fn first(&self) -> Option<NodeId> {
        self.report.map(|r| r.first)
    }

    /// The matching position is the end of the game: the moves continue the
    /// main line and the dialog says so ("merge-note-extends").
    pub fn extends_mainline(&self) -> bool {
        self.report.is_some_and(|r| r.extended_mainline)
    }

    /// `end_ply` cut the game merged short (the comment then says how long
    /// the whole game is).
    pub fn truncated(&self) -> bool {
        self.report.is_some_and(|r| r.truncated)
    }

    /// The comment in neutral English that [`MergeMessage::Apply`] adds
    /// when it is given no text.
    pub fn english_comment(&self) -> Option<String> {
        Some(self.comment_parts?.english(self.truncated()))
    }
}

/// Merges `source` into `game` with the comment `comment` (the neutral
/// English one when `None`).
pub(crate) fn merge_one(
    game: &mut Game,
    source: &Game,
    end_ply: Option<u32>,
    comment: Option<&str>,
) -> Result<MergeReport, MergeError> {
    let text = if let Some(text) = comment {
        text.to_owned()
    } else {
        // Whether the merge cuts the game short is only known after it is
        // done; a copy tells it before the comment is chosen.
        let truncated = match end_ply {
            Some(_) => {
                let options = MergeOptions {
                    end_ply,
                    comment: None,
                };
                game.clone().merge_game(source, &options)?.truncated
            }
            None => false,
        };
        merge_comment_parts(source).english(truncated)
    };
    let options = MergeOptions {
        end_ply,
        comment: Some(text),
    };
    game.merge_game(source, &options)
}

/// Merges every source into `game`. A single merge returns its error; a
/// batch skips and counts the games that cannot be merged (spec rule 10).
pub(crate) fn merge_all(
    game: &mut Game,
    sources: &[MergeSource],
    batch: bool,
    end_ply: Option<u32>,
    comment: Option<&str>,
) -> Result<(Option<MergeReport>, BatchReport), MergeError> {
    if game.mainline().next().is_none() {
        return Err(MergeError::NoMoves);
    }
    if !batch {
        let Some(source) = sources.first() else {
            return Err(MergeError::NothingToAdd);
        };
        let report = merge_one(game, &source.game, end_ply, comment)?;
        let counts = BatchReport {
            merged: 1,
            ..BatchReport::default()
        };
        return Ok((Some(report), counts));
    }
    let mut counts = BatchReport::default();
    for source in sources {
        match merge_one(game, &source.game, None, None) {
            Ok(_) => counts.merged += 1,
            Err(MergeError::NoMatch) => counts.skipped_no_match += 1,
            Err(MergeError::NothingToAdd) => counts.skipped_nothing += 1,
            Err(MergeError::NoMoves) => return Err(MergeError::NoMoves),
            Err(_) => counts.skipped_unreadable += 1,
        }
    }
    Ok((None, counts))
}

/// Reads the games for the merge, off the interface thread (the body of
/// [`Effect::LoadGamesForMerge`](crate::Effect)).
pub(crate) fn load_games(
    source: &SharedSource,
    numbers: &[u32],
) -> Vec<(u32, Result<SharedGame, MergeLoadError>)> {
    numbers
        .iter()
        .map(|&number| (number, load_one(source, number)))
        .collect()
}

fn load_one(source: &SharedSource, number: u32) -> Result<SharedGame, MergeLoadError> {
    use mychess_db::GameSource;
    let mut guard = source.lock();
    if number == 0 || number > GameSource::len(&*guard) {
        return Err(MergeLoadError::Number);
    }
    GameSource::game(&mut *guard, number)
        .map(SharedGame::new)
        .map_err(|error| {
            tracing::warn!(game = number, %error, "cannot read a game to merge");
            MergeLoadError::Decode
        })
}
