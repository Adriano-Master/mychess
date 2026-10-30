//! A ready-to-draw snapshot of the window of the search for tactics and
//! patterns (spec M6-2). It holds the model types of
//! [`pattern_search::model`](super::model), which the front-end may name,
//! and no type of the database file formats.

use mychess_db::pattern::model::{Condition, Pattern, Scope};
use mychess_db::pattern::presets::{ParamKey, Params};

use super::describe::Sentence;
use super::edit::ConditionKind;
use super::message::{NodePath, PatternRunSummary, PatternSearchNotice, PatternTab, SavedPattern};
use crate::advanced_search::{DatabaseChoice, SearchOp};
use crate::bases::SlotId;
use mychess_db::pattern::Highlight;

/// A ready-made pattern in the list of the first tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetEntry {
    /// The id to send back to `SelectPreset`.
    pub id: &'static str,
    /// The Fluent key of its name (`pattern-preset-...`).
    pub name_key: &'static str,
    /// The parameters the window shows for it.
    pub params: Vec<ParamKey>,
}

/// The Fluent key of the label of a parameter (`pattern-param-...`).
pub fn param_label_key(key: ParamKey) -> &'static str {
    match key {
        ParamKey::By => "pattern-param-by",
        ParamKey::N => "pattern-param-n",
        ParamKey::MateMode => "pattern-param-mate-mode",
        ParamKey::ForcingOnly => "pattern-mate-forcing-only",
        ParamKey::MaxNodes => "pattern-param-max-nodes",
        ParamKey::Roles => "pattern-param-piece",
        ParamKey::TargetRoles => "pattern-param-target-piece",
        ParamKey::AgainstKing => "pattern-param-against-king",
        ParamKey::Discovery => "pattern-param-discovery",
        ParamKey::MinNet => "pattern-param-min-net",
        ParamKey::Outcome => "pattern-param-outcome",
        ParamKey::Within => "pattern-param-within",
        ParamKey::RequireWin => "pattern-param-require-win",
        ParamKey::MinOwnRank => "pattern-param-min-own-rank",
        _ => "pattern-param-other",
    }
}

/// A search in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternRunningView {
    /// The slot whose filter will change.
    pub slot: SlotId,
    /// Games done and games to do (0 until the first report).
    pub progress: (u64, u64),
}

/// "This may take a long time": the search waits for
/// `PatternSearchMessage::AnswerCost` (D3285).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostView {
    /// Estimated positions to evaluate.
    pub estimated_positions: u64,
    /// Games in the scope of the search.
    pub games: u32,
}

/// One card of the tree, in pre-order.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeView {
    /// Its address (send it back in the edits).
    pub path: NodePath,
    /// Nesting level, 0 for the root.
    pub depth: usize,
    /// Its kind.
    pub kind: ConditionKind,
    /// The sentence of the card.
    pub sentence: Sentence,
    /// The node itself (a group shows its items as the cards that follow):
    /// the panel edits a copy and sends it back with `PatternEdit::Replace`.
    pub condition: Condition,
    /// How many children it has.
    pub children: usize,
    /// This node is the one the first problem of the pattern points at.
    pub has_issue: bool,
    /// This node is the selected one.
    pub selected: bool,
}

/// The occurrences of the pattern in the game of the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitsView {
    /// The ply of each occurrence (positions after that many half moves of
    /// the main line), in order: a dot goes on those cells of the list of
    /// moves.
    pub plies: Vec<u16>,
    /// The occurrence in view: the one at the cursor, or the last visited.
    pub current: usize,
    /// They come from "test on this game", not from the filter.
    pub from_test: bool,
    /// The number of the game in its database, if it has one.
    pub game: Option<u32>,
}

/// Everything the window draws.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternSearchView {
    /// The tab on show.
    pub tab: PatternTab,
    /// The search in flight, if any.
    pub running: Option<PatternRunningView>,
    /// The question about an expensive search, if one waits.
    pub confirm: Option<CostView>,
    /// The ready-made patterns, in the order of the window.
    pub presets: Vec<PresetEntry>,
    /// The id of the preset picked.
    pub selected_preset: Option<String>,
    /// The parameters now (a preset reads only the ones in its list).
    pub params: Params,
    /// What is wrong with the parameters, if anything.
    pub param_issue: Option<PatternSearchNotice>,
    /// The operation on the filter.
    pub op: SearchOp,
    /// The scope of the search (both tabs).
    pub scope: Scope,
    /// The slot to search, or `None` for the current one.
    pub reference: Option<SlotId>,
    /// The open databases, the Clipbase last.
    pub databases: Vec<DatabaseChoice>,
    /// The pattern of the "Custom" tab (its scope is the one above).
    pub draft: Pattern,
    /// The cards of the tree of the "Custom" tab, in pre-order.
    pub tree: Vec<NodeView>,
    /// The selected card.
    pub selection: Option<NodePath>,
    /// The first problem of the pattern of the "Custom" tab, if any.
    pub tree_issue: Option<PatternSearchNotice>,
    /// The card the problem points at (`None` for a problem of the whole
    /// pattern).
    pub issue_path: Option<NodePath>,
    /// The user's patterns.
    pub saved: Vec<SavedPattern>,
    /// Whether patterns can be saved (there is a folder for them).
    pub can_save: bool,
    /// How the last search went.
    pub summary: Option<PatternRunSummary>,
    /// The occurrences in the game of the editor.
    pub hits: Option<HitsView>,
    /// What to draw on the board now: the arrows and squares of the
    /// occurrence at the cursor (empty when the cursor is elsewhere).
    pub highlights: Vec<Highlight>,
    /// The remark about the form or the last operation.
    pub issue: Option<PatternSearchNotice>,
}
