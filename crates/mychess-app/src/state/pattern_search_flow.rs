//! The search for tactics and patterns (spec M6-2, slice c): the tabs, the
//! presets, the tree being edited, the start of a search (with the question
//! about its cost), its answer, the first game of the filter, the
//! occurrences in the game of the editor and the user's patterns.
//!
//! Everything here is bookkeeping, which `update` may do. The scan of the
//! database is the effect [`Effect::PatternSearch`], run by `perform` on a
//! blocking thread over a copy of the filter of the slot (D3273, D3286); the
//! occurrences in one game are the effect [`Effect::PatternHits`].

use std::path::PathBuf;

use mychess_core::Square;
use mychess_core::{EmbeddedCommand, embedded_commands};
use mychess_db::pattern::model::{
    Condition, MAX_DEPTH, Pattern, PatternError, PatternFile, Region, Sq,
};
use mychess_db::pattern::presets::{Params, check_params};
use mychess_db::{FILTER_LAST_MOVE, FilterOp, estimate_pattern_cost};

use super::AppState;
use crate::bases::{OpenControl, SlotId};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::AppNotice;
use crate::pattern_search::edit as tree_edit;
use crate::pattern_search::{
    Confirm, DirChoice, EditRefusal, Focus, GameHits, HitsRequest, HitsView, NodePath, NodeView,
    ParamValue, PatternCommand, PatternDone, PatternEdit, PatternFailure, PatternIssue,
    PatternRunSummary, PatternRunningView, PatternSearchMessage, PatternSearchNotice,
    PatternSearchPrefs, PatternSearchView, PatternTab, PresetEntry, Running, SavedFailure, SavedOp,
    SavedOutcome, describe, patterns_dir,
};
use crate::pattern_search::{find_preset, preset_catalog};
use crate::prefs::{PrefChange, PrefsMessage};
use crate::session::{GameMessage, Nav};
use crate::tree::TreeMessage;

/// The name a pattern gets for a run or a test when it has none: it is
/// never shown and never saved.
const RUN_NAME: &str = "pattern";

/// The address a problem path (`root.items[2].actor.region`) points at: the
/// card, stopping at the first segment that is not a child.
fn path_of_issue(path: &str) -> Option<NodePath> {
    let mut rest = path.strip_prefix("root")?;
    let mut out = Vec::new();
    loop {
        if let Some(after) = rest.strip_prefix(".items[") {
            let (digits, tail) = after.split_once(']')?;
            out.push(digits.parse::<usize>().ok()?);
            rest = tail;
        } else if let Some(after) = rest.strip_prefix(".item")
            && (after.is_empty() || after.starts_with('.') || after.starts_with('['))
        {
            out.push(0);
            rest = after;
        } else {
            return Some(out);
        }
    }
}

/// The notice for an error of a preset's parameters.
fn param_notice(error: &PatternError) -> PatternSearchNotice {
    match error {
        PatternError::InvalidValue { path, .. } => match path.strip_prefix("params.") {
            Some(name) => PatternSearchNotice::InvalidParam(name.to_owned()),
            None if path == "id" => PatternSearchNotice::UnknownPreset(String::new()),
            None => PatternSearchNotice::from_error(error),
        },
        other => PatternSearchNotice::from_error(other),
    }
}

fn apply_param(params: &mut Params, value: ParamValue) {
    match value {
        ParamValue::By(v) => params.by = v,
        ParamValue::N(v) => params.n = v,
        ParamValue::MateMode(v) => params.mate_mode = v,
        ParamValue::ForcingOnly(v) => params.forcing_only = v,
        ParamValue::MaxNodes(v) => params.max_nodes = v,
        ParamValue::Roles(v) => params.roles = v,
        ParamValue::TargetRoles(v) => params.target_roles = v,
        ParamValue::AgainstKing(v) => params.against_king = v,
        ParamValue::Discovery(v) => params.discovery = v,
        ParamValue::MinNet(v) => params.min_net = v,
        ParamValue::Outcome(v) => params.outcome = v,
        ParamValue::Within(v) => params.within = v,
        ParamValue::RequireWin(v) => params.require_win = v,
        ParamValue::MinOwnRank(v) => params.min_own_rank = v,
        // `ParamValue` is non-exhaustive: a value this build does not know
        // changes nothing.
    }
}

fn flatten(
    condition: &Condition,
    path: &mut NodePath,
    issue: Option<&NodePath>,
    selection: Option<&NodePath>,
    out: &mut Vec<NodeView>,
) {
    let children = match condition {
        Condition::All { items } | Condition::Any { items } => items.len(),
        Condition::Not { .. } => 1,
        _ => 0,
    };
    out.push(NodeView {
        path: path.clone(),
        depth: path.len(),
        kind: tree_edit::ConditionKind::of(condition),
        sentence: describe(condition),
        condition: condition.clone(),
        children,
        has_issue: issue == Some(&*path),
        selected: selection == Some(&*path),
    });
    if path.len() > MAX_DEPTH {
        return;
    }
    for i in 0..children {
        if let Some(child) = tree_edit::node(condition, &[i]) {
            path.push(i);
            flatten(child, path, issue, selection, out);
            path.pop();
        }
    }
}

impl AppState {
    // ---- reading ----

    /// The state of the window of the search for patterns.
    pub fn pattern_search(&self) -> &crate::pattern_search::PatternUi {
        &self.pattern
    }

    /// Replaces the folder of the user's patterns (tests and embedders). The
    /// default is `patterns_dir()`; `None` means there is no folder, and
    /// saving warns about it.
    pub fn set_pattern_dir(&mut self, dir: Option<PathBuf>) {
        self.pattern.dir = DirChoice::Fixed(dir);
    }

    fn pattern_dir(&self) -> Option<PathBuf> {
        match &self.pattern.dir {
            DirChoice::Fixed(dir) => dir.clone(),
            DirChoice::SystemDefault => patterns_dir(),
        }
    }

    /// Whether a search for a pattern is running.
    pub(crate) fn pattern_search_running(&self) -> bool {
        self.pattern.running.is_some()
    }

    /// Stops the search in flight (the program is ending).
    pub(crate) fn pattern_search_cancel(&self) {
        self.pattern.cancel();
    }

    fn pattern_notice(&mut self, notice: PatternSearchNotice) {
        self.pattern.issue = Some(notice.clone());
        self.notice = Some(AppNotice::PatternSearch(notice));
    }

    /// The scope in use.
    fn pattern_scope(&self) -> mychess_db::pattern::model::Scope {
        self.pattern
            .scope
            .clone()
            .unwrap_or_else(|| self.prefs.pattern_search.scope())
    }

    /// The game of the editor as the occurrences know it.
    fn pattern_game_key(&self) -> Option<(SlotId, u32)> {
        let current = self.bases.current();
        self.bases
            .slot(current)
            .and_then(|s| s.current_game)
            .map(|n| (current, n))
    }

    /// Whether the window is open (cheap: no view is built).
    pub fn pattern_search_open(&self) -> bool {
        self.pattern.open
    }

    /// The question about an expensive search, if one waits and the window
    /// is open. Cheap, for the code that runs at every draw.
    pub fn pattern_search_confirm(&self) -> Option<crate::pattern_search::CostView> {
        if !self.pattern.open {
            return None;
        }
        self.pattern
            .confirm
            .as_ref()
            .map(|c| crate::pattern_search::CostView {
                estimated_positions: c.estimated,
                games: c.games,
            })
    }

    /// The occurrences in the game of the editor, if the window is open
    /// and there are some to show. Cheap, for the code that runs at every
    /// draw.
    pub fn pattern_search_hits(&self) -> Option<HitsView> {
        if !self.pattern.open {
            return None;
        }
        let h = self.pattern.hits.as_ref()?;
        let current = self
            .cursor_mainline_ply()
            .and_then(|c| h.hits.iter().position(|x| u32::from(x.ply) == c))
            .unwrap_or_else(|| h.current.min(h.hits.len().saturating_sub(1)));
        Some(HitsView {
            plies: h.hits.iter().map(|x| x.ply).collect(),
            current,
            from_test: h.from_test,
            game: self.pattern_game_key().map(|(_, n)| n),
        })
    }

    /// What to draw on the board for the occurrence at the cursor (empty
    /// when the window is closed or the cursor is elsewhere). Borrowed: no
    /// copy per draw.
    pub fn pattern_search_highlights(&self) -> &[mychess_db::pattern::Highlight] {
        if !self.pattern.open {
            return &[];
        }
        self.pattern
            .hits
            .as_ref()
            .zip(self.cursor_mainline_ply())
            .and_then(|(h, c)| h.hits.iter().find(|x| u32::from(x.ply) == c))
            .map_or(&[], |x| x.highlights.as_slice())
    }

    /// What the window draws, if it is open.
    pub fn pattern_search_view(&self) -> Option<PatternSearchView> {
        if !self.pattern.open {
            return None;
        }
        let ui = &self.pattern;
        let scope = self.pattern_scope();
        let presets_list = preset_catalog();
        let selected = ui
            .preset
            .clone()
            .or_else(|| presets_list.first().map(|p| p.id.to_owned()));
        let param_issue = selected
            .as_deref()
            .and_then(find_preset)
            .and_then(|p| check_params(&p, &ui.params).err())
            .map(|e| param_notice(&e));
        let mut draft = ui.draft.clone();
        draft.scope = scope.clone();
        let mut checked = draft.clone();
        if checked.name.trim().is_empty() {
            RUN_NAME.clone_into(&mut checked.name);
        }
        let problem = checked.validate().err();
        let issue_path = problem.as_ref().and_then(|e| match e {
            PatternError::InvalidValue { path, .. }
            | PatternError::EmptyGroup { path }
            | PatternError::MoverNotAllowed { path } => path_of_issue(path),
            _ => None,
        });
        let tree_issue = problem.as_ref().map(PatternSearchNotice::from_error);
        let mut tree = Vec::new();
        flatten(
            &draft.root,
            &mut Vec::new(),
            issue_path.as_ref(),
            ui.selection.as_ref(),
            &mut tree,
        );
        let hits = self.pattern_search_hits();
        let highlights = self.pattern_search_highlights().to_vec();
        Some(PatternSearchView {
            tab: ui.tab,
            running: ui.running.as_ref().map(|r| PatternRunningView {
                slot: r.slot,
                progress: ui.progress,
            }),
            confirm: self.pattern_search_confirm(),
            presets: presets_list
                .iter()
                .map(|p| PresetEntry {
                    id: p.id,
                    name_key: p.name_key,
                    params: p.params.to_vec(),
                })
                .collect(),
            selected_preset: selected,
            params: ui.params.clone(),
            param_issue,
            op: self.prefs.pattern_search.op,
            scope,
            reference: ui.reference,
            databases: crate::advanced_search::database_choices(&self.bases),
            draft,
            tree,
            selection: ui.selection.clone(),
            tree_issue,
            issue_path,
            saved: ui.saved.clone(),
            can_save: self.pattern_dir().is_some(),
            summary: ui.summary.clone(),
            hits,
            highlights,
            issue: ui.issue.clone(),
        })
    }

    // ---- preferences ----

    fn pattern_prefs(
        &mut self,
        change: impl FnOnce(&mut PatternSearchPrefs),
        effects: &mut Vec<Effect>,
    ) {
        let mut prefs = (*self.prefs.pattern_search).clone();
        change(&mut prefs);
        let prefs = prefs.sanitized();
        if prefs != *self.prefs.pattern_search {
            self.prefs_message(
                PrefsMessage::Set(PrefChange::PatternSearch(Box::new(prefs))),
                effects,
            );
        }
    }

    // ---- the messages ----

    /// Handles a [`PatternSearchMessage`].
    #[allow(clippy::too_many_lines)] // one flat match over the messages reads best
    pub(crate) fn pattern_message(
        &mut self,
        message: PatternSearchMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            PatternSearchMessage::Open => self.pattern_open(effects),
            PatternSearchMessage::Close => {
                self.pattern.open = false;
                self.pattern.confirm = None;
                self.pattern.cancel();
                // A late answer must neither apply its filter nor warn
                // about an interruption (spec item 31, D3299 item 42).
                self.pattern.running = None;
                self.pattern.focus = None;
                self.pattern.hits = None;
                self.pattern.hits_request = None;
            }
            PatternSearchMessage::SelectTab(tab) => {
                self.pattern.tab = tab;
                self.pattern.issue = None;
            }
            PatternSearchMessage::SelectPreset(id) => self.pattern_select_preset(&id, effects),
            PatternSearchMessage::SetParam(value) => apply_param(&mut self.pattern.params, value),
            PatternSearchMessage::Customize { name, description } => {
                self.pattern_customize(&name, &description);
            }
            PatternSearchMessage::Edit(edit) => self.pattern_edit(edit),
            PatternSearchMessage::SetOp(op) => self.pattern_prefs(|p| p.op = op, effects),
            PatternSearchMessage::SetReference(slot) => {
                if slot.is_none_or(|id| self.bases.slot(id).is_some()) {
                    self.pattern.reference = slot;
                }
            }
            PatternSearchMessage::SetScope(scope) => {
                let mut scope = scope;
                scope.hold = scope.hold.clamp(1, crate::pattern_search::MAX_HOLD);
                scope.min_move = scope.min_move.min(crate::pattern_search::MAX_SCOPE_MOVE);
                scope.max_move = scope.max_move.min(crate::pattern_search::MAX_SCOPE_MOVE);
                self.pattern_prefs(|p| *p = p.clone().with_scope(&scope), effects);
                self.pattern.scope = Some(scope);
            }
            PatternSearchMessage::Start => self.pattern_start(false, effects),
            PatternSearchMessage::AnswerCost(yes) => {
                if self.pattern.confirm.take().is_some() && yes {
                    self.pattern_start(true, effects);
                }
            }
            PatternSearchMessage::Stop => {
                self.pattern.confirm = None;
                self.pattern.cancel();
            }
            PatternSearchMessage::Progress => {
                if let Some(running) = &self.pattern.running {
                    self.pattern.progress = running.control.progress();
                }
            }
            PatternSearchMessage::Finished { request, result } => {
                self.pattern_finished(request, result, effects);
            }
            PatternSearchMessage::Cancelled { request } => {
                if self.pattern.running.as_ref().map(|r| r.request) == Some(request) {
                    self.pattern.running = None;
                    self.pattern_notice(PatternSearchNotice::Interrupted);
                }
            }
            PatternSearchMessage::TestGame => self.pattern_test_game(effects),
            PatternSearchMessage::ClearHits => {
                self.pattern.focus = None;
                self.pattern.hits = None;
                self.pattern.hits_request = None;
            }
            PatternSearchMessage::NextHit => self.pattern_step_hit(true),
            PatternSearchMessage::PrevHit => self.pattern_step_hit(false),
            PatternSearchMessage::GoToHit(index) => self.pattern_go_to_hit(index),
            PatternSearchMessage::HitsDone { request, result } => {
                self.pattern_hits_done(request, result);
            }
            PatternSearchMessage::RefreshSaved => {
                self.pattern_saved_command(PatternCommand::List, SavedOp::List, effects);
            }
            PatternSearchMessage::SaveAs { name, description } => {
                match self.pattern_file(&name, &description, true) {
                    Ok(file) => self.pattern_saved_command(
                        PatternCommand::Save(Box::new(file)),
                        SavedOp::Save,
                        effects,
                    ),
                    Err(notice) => self.pattern_notice(notice),
                }
            }
            PatternSearchMessage::LoadSaved(name) => self.pattern_saved_command(
                PatternCommand::Load(name.clone()),
                SavedOp::Load(name),
                effects,
            ),
            PatternSearchMessage::DeleteSaved(name) => self.pattern_saved_command(
                PatternCommand::Delete(name.clone()),
                SavedOp::Delete(name),
                effects,
            ),
            PatternSearchMessage::DeleteSavedFile(path) => {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.pattern_saved_command(
                    PatternCommand::DeleteFile(path),
                    SavedOp::Delete(name),
                    effects,
                );
            }
            PatternSearchMessage::Import(path) => {
                self.pattern_saved_command(PatternCommand::Import(path), SavedOp::Import, effects);
            }
            PatternSearchMessage::Export {
                path,
                name,
                description,
            } => match self.pattern_file(&name, &description, false) {
                Ok(file) => self.pattern_saved_command(
                    PatternCommand::Export {
                        path,
                        file: Box::new(file),
                    },
                    SavedOp::Export,
                    effects,
                ),
                Err(notice) => self.pattern_notice(notice),
            },
            PatternSearchMessage::SavedDone { request, result } => {
                self.pattern_saved_done(request, result, effects);
            }
        }
    }

    fn pattern_open(&mut self, effects: &mut Vec<Effect>) {
        self.pattern.open = true;
        self.pattern.issue = None;
        if self.pattern.scope.is_none() {
            self.pattern.scope = Some(self.prefs.pattern_search.scope());
        }
        if self.pattern.preset.is_none() {
            let last = &self.prefs.pattern_search.last_preset;
            self.pattern.preset = find_preset(last)
                .map(|p| p.id)
                .or_else(|| preset_catalog().first().map(|p| p.id))
                .map(str::to_owned);
        }
        if !self.pattern.saved_listed {
            self.pattern_saved_command(PatternCommand::List, SavedOp::List, effects);
        }
    }

    fn pattern_select_preset(&mut self, id: &str, effects: &mut Vec<Effect>) {
        let Some(preset) = find_preset(id) else {
            return self.pattern_notice(PatternSearchNotice::UnknownPreset(id.to_owned()));
        };
        let by = self.pattern.params.by;
        self.pattern.params = Params {
            by,
            ..Params::default()
        };
        // The preset brings its own scope (D3279).
        if let Ok(built) = preset.build_checked(&self.pattern.params) {
            self.pattern.scope = Some(built.scope);
        }
        self.pattern.preset = Some(preset.id.to_owned());
        self.pattern.issue = None;
        let id = preset.id.to_owned();
        self.pattern_prefs(|p| p.last_preset = id, effects);
    }

    /// The pattern of the preset picked, with the current parameters.
    fn pattern_from_preset(&self, params: &Params) -> Result<Pattern, PatternSearchNotice> {
        let presets_list = preset_catalog();
        let id = self
            .pattern
            .preset
            .clone()
            .or_else(|| presets_list.first().map(|p| p.id.to_owned()))
            .unwrap_or_default();
        let Some(preset) = find_preset(&id) else {
            return Err(PatternSearchNotice::UnknownPreset(id));
        };
        preset.build_checked(params).map_err(|e| param_notice(&e))
    }

    fn pattern_customize(&mut self, name: &str, description: &str) {
        let mut params = self.pattern.params.clone();
        if !name.trim().is_empty() {
            params.name = Some(name.trim().to_owned());
        }
        if !description.trim().is_empty() {
            params.description = Some(description.to_owned());
        }
        match self.pattern_from_preset(&params) {
            Ok(mut pattern) => {
                let scope = self.pattern_scope();
                pattern.scope = scope.clone();
                self.pattern.draft = pattern;
                self.pattern.scope = Some(scope);
                self.pattern.selection = None;
                self.pattern.tab = PatternTab::Advanced;
                self.pattern.issue = None;
            }
            Err(notice) => self.pattern_notice(notice),
        }
    }

    /// The pattern of the tab on show, with the scope in use, validated.
    fn pattern_current(&self) -> Result<Pattern, PatternSearchNotice> {
        let mut pattern = match self.pattern.tab {
            PatternTab::Presets => self.pattern_from_preset(&self.pattern.params)?,
            PatternTab::Advanced => self.pattern.draft.clone(),
        };
        pattern.scope = self.pattern_scope();
        if pattern.name.trim().is_empty() {
            RUN_NAME.clone_into(&mut pattern.name);
        }
        pattern
            .validate()
            .map_err(|e| PatternSearchNotice::from_error(&e))?;
        Ok(pattern)
    }

    /// The pattern to save or export under the texts given.
    fn pattern_file(
        &self,
        name: &str,
        description: &str,
        name_required: bool,
    ) -> Result<PatternFile, PatternSearchNotice> {
        let mut pattern = self.pattern_current()?;
        let name = name.trim();
        if name.is_empty() {
            // A file never carries the placeholder of a run (`RUN_NAME`):
            // saving needs a name, and so does exporting a pattern of the
            // "Custom" tab that has none (a preset always has its own).
            let unnamed = self.pattern.tab == PatternTab::Advanced
                && self.pattern.draft.name.trim().is_empty();
            if name_required || unnamed {
                return Err(PatternSearchNotice::InvalidSavedName);
            }
        } else {
            name.clone_into(&mut pattern.name);
            description.clone_into(&mut pattern.description);
        }
        pattern.validate().map_err(|e| match e {
            PatternError::InvalidValue { ref path, .. } if path == "name" => {
                PatternSearchNotice::InvalidSavedName
            }
            other => PatternSearchNotice::from_error(&other),
        })?;
        Ok(PatternFile::new(pattern))
    }

    // ---- editing the tree ----

    fn pattern_edit(&mut self, edit: PatternEdit) {
        let root = &self.pattern.draft.root;
        let outcome: Result<(Condition, Option<NodePath>), EditRefusal> = match edit {
            PatternEdit::Select(path) => {
                self.pattern.selection = path.filter(|p| tree_edit::node(root, p).is_some());
                return;
            }
            PatternEdit::Clear => {
                self.pattern.draft.root = tree_edit::empty_tree();
                self.pattern.selection = None;
                self.pattern.issue = None;
                return;
            }
            PatternEdit::SetMeta { name, description } => {
                self.pattern.draft.name = name;
                self.pattern.draft.description = description;
                return;
            }
            PatternEdit::Add { parent, kind } => {
                tree_edit::add(root, &parent, kind.default_condition()).map(|new| {
                    let count = match tree_edit::node(&new, &parent) {
                        Some(Condition::All { items } | Condition::Any { items }) => items.len(),
                        _ => 1,
                    };
                    let mut selected = parent;
                    selected.push(count.saturating_sub(1));
                    (new, Some(selected))
                })
            }
            PatternEdit::Remove(path) => tree_edit::remove(root, &path).map(|n| (n, None)),
            PatternEdit::Replace { path, condition } => {
                tree_edit::replace(root, &path, *condition).map(|n| (n, Some(path)))
            }
            PatternEdit::Move {
                path,
                parent,
                index,
            } => tree_edit::move_node(root, &path, &parent, index).map(|n| (n, None)),
            PatternEdit::SetGroupKind { path, kind } => {
                tree_edit::set_group_kind(root, &path, kind).map(|n| (n, Some(path)))
            }
            PatternEdit::Wrap { path, kind } => {
                tree_edit::wrap(root, &path, kind).map(|n| (n, Some(path)))
            }
            PatternEdit::Unwrap(path) => {
                tree_edit::unwrap_group(root, &path).map(|n| (n, Some(path)))
            }
            PatternEdit::SetRegion { path, slot, region } => {
                tree_edit::set_region(root, &path, slot, region).map(|n| (n, Some(path)))
            }
            PatternEdit::ToggleSquare { path, slot, square } => {
                tree_edit::toggle_square(root, &path, slot, square).map(|n| (n, Some(path)))
            }
            PatternEdit::UseMarks { path, slot } => {
                let squares = self.marked_squares();
                if squares.is_empty() {
                    Err(EditRefusal::NoMarks)
                } else {
                    let region = Region::Squares(squares.into_iter().map(Sq).collect());
                    tree_edit::set_region(root, &path, slot, region).map(|n| (n, Some(path)))
                }
            }
        };
        match outcome {
            Ok((new, selection)) => {
                self.pattern.draft.root = new;
                self.pattern.selection =
                    selection.filter(|p| tree_edit::node(&self.pattern.draft.root, p).is_some());
                self.pattern.issue = None;
            }
            Err(why) => self.pattern_notice(PatternSearchNotice::EditRefused(why)),
        }
    }

    /// The squares marked (not arrows) in the comment at the cursor.
    fn marked_squares(&self) -> Vec<Square> {
        let comment = self
            .session
            .game()
            .comment(self.session.cursor())
            .ok()
            .flatten()
            .unwrap_or_default();
        let mut out: Vec<Square> = Vec::new();
        for command in embedded_commands(comment) {
            if let EmbeddedCommand::Mark(mark) = command
                && mark.to.is_none()
                && !out.contains(&mark.from)
            {
                out.push(mark.from);
            }
        }
        out
    }

    // ---- running ----

    fn pattern_start(&mut self, confirmed: bool, effects: &mut Vec<Effect>) {
        // Everything that can refuse the search is checked before the one
        // in flight is touched: a form error, the question about the cost or
        // a busy list leave it running (D3389, D3299 item 41). The search
        // itself may be replaced, so it is not a reason to be busy.
        let old = self.pattern.running.take();
        self.pattern.confirm = None;
        if self.list_busy() {
            self.pattern.running = old;
            return;
        }
        let pattern = match self.pattern_current() {
            Ok(pattern) => pattern,
            Err(notice) => {
                self.pattern.running = old;
                return self.pattern_notice(notice);
            }
        };
        let id = self
            .pattern
            .reference
            .unwrap_or_else(|| self.bases.current());
        let op = self.prefs.pattern_search.op.engine();
        let Some(slot) = self.bases.slot(id).filter(|s| !s.needs_reload) else {
            self.pattern.running = old;
            return self.pattern_notice(PatternSearchNotice::NoDatabase);
        };
        let games = match op {
            FilterOp::And => slot.filtered(),
            FilterOp::Or => slot.total().saturating_sub(slot.filtered()),
            FilterOp::Reset => slot.total(),
        };
        if !confirmed {
            let cost = estimate_pattern_cost(&pattern, games);
            if cost.needs_confirmation {
                self.pattern.running = old;
                self.pattern.confirm = Some(Confirm {
                    estimated: cost.estimated_positions,
                    games,
                });
                return;
            }
        }
        // Starting a search stops the one in flight; its answer is dropped
        // by the request.
        if let Some(old) = old {
            old.control.cancel();
        }
        let (source, filter) = (slot.source.clone(), Box::new(slot.filter.clone()));
        let (len, count) = (slot.total(), slot.filtered());
        let request = self.new_request();
        let control = OpenControl::new();
        self.pattern.running = Some(Running {
            request,
            slot: id,
            source: source.clone(),
            len,
            count,
            control: control.clone(),
            pattern: pattern.clone(),
        });
        self.pattern.progress = (0, 0);
        self.pattern.issue = None;
        // The scope in use is what the user will find next time.
        let scope = pattern.scope.clone();
        self.pattern_prefs(|p| *p = p.clone().with_scope(&scope), effects);
        effects.push(Effect::PatternSearch {
            request,
            slot: id,
            source,
            filter,
            pattern: Box::new(pattern),
            op,
            control,
        });
    }

    fn pattern_finished(
        &mut self,
        request: RequestId,
        result: Result<Box<PatternDone>, PatternFailure>,
        effects: &mut Vec<Effect>,
    ) {
        if self.pattern.running.as_ref().map(|r| r.request) != Some(request) {
            return;
        }
        let Some(running) = self.pattern.running.take() else {
            return;
        };
        let done = match result {
            Ok(done) => *done,
            Err(PatternFailure::Pattern(issue)) => {
                return self.pattern_notice(PatternSearchNotice::InvalidPattern(issue));
            }
            Err(PatternFailure::FilterSize) => {
                return self.pattern_notice(PatternSearchNotice::Stale);
            }
            Err(_) => return self.pattern_notice(PatternSearchNotice::Failed),
        };
        if done.report.interrupted {
            return self.pattern_notice(PatternSearchNotice::Interrupted);
        }
        let current = self.bases.slot(running.slot).is_some_and(|slot| {
            slot.source == running.source
                && !slot.needs_reload
                && slot.total() == running.len
                && slot.filtered() == running.count
        });
        if !current || done.filter.len() != running.len {
            return self.pattern_notice(PatternSearchNotice::Stale);
        }
        let report = done.report;
        let Some(slot) = self.bases.slot_mut(running.slot) else {
            return;
        };
        slot.filter = done.filter;
        slot.filter_changed();
        slot.list.first = 1;
        let first = slot.filter.first().map(|n| {
            let ply = if slot.filter.get(n) == FILTER_LAST_MOVE {
                usize::MAX
            } else {
                usize::from(slot.filter.ply(n))
            };
            (n, ply)
        });
        self.pattern.summary = Some(PatternRunSummary {
            slot: running.slot,
            matched: report.matched,
            start_count: report.start_count,
            elapsed: report.elapsed,
            unreadable: report.unreadable_total,
            unknown_positions: report.unknown_positions,
            positions_examined: report.positions_examined,
            games_folded_out: report.games_folded_out,
        });
        self.pattern.issue = None;
        if report.unreadable_total > 0 {
            self.pattern_notice(PatternSearchNotice::Unreadable {
                count: report.unreadable_total,
            });
        } else if report.unknown_positions > 0 {
            self.pattern_notice(PatternSearchNotice::UnknownPositions {
                count: report.unknown_positions,
            });
        }
        // The games of this filter show the occurrences of this pattern.
        self.pattern.focus = Some(Focus {
            slot: running.slot,
            pattern: running.pattern,
            first: first.map(|(n, _)| n),
        });
        self.pattern.hits = None;
        self.pattern.hits_request = None;
        self.pattern.hits_key = None;
        if let Some((number, ply)) = first {
            self.advanced_load_first(running.slot, number, ply, effects);
        }
        self.tree_message(TreeMessage::SetAdjust(false), effects);
    }

    // ---- the occurrences ----

    fn pattern_request_hits(
        &mut self,
        pattern: Pattern,
        key: Option<(SlotId, u32)>,
        from_test: bool,
        land_first: bool,
        effects: &mut Vec<Effect>,
    ) {
        let request = self.new_request();
        self.pattern.hits_request = Some(HitsRequest {
            request,
            key,
            from_test,
            land_first,
        });
        effects.push(Effect::PatternHits {
            request,
            game: crate::shared::SharedGame::new(self.session.game().clone()),
            pattern: Box::new(pattern),
        });
    }

    fn pattern_test_game(&mut self, effects: &mut Vec<Effect>) {
        let pattern = match self.pattern_current() {
            Ok(pattern) => pattern,
            Err(notice) => return self.pattern_notice(notice),
        };
        let key = self.pattern_game_key();
        self.pattern.hits_key = key;
        self.pattern.hits = None;
        self.pattern.issue = None;
        self.pattern_request_hits(pattern, key, true, false, effects);
    }

    /// Follows the game of the editor: when it changes and the last search
    /// is still in view, asks for its occurrences (D3284). Cheap when
    /// nothing is shown.
    pub(crate) fn pattern_search_guard(&mut self, effects: &mut Vec<Effect>) {
        let ui = &self.pattern;
        if ui.focus.is_none() && ui.hits.is_none() && ui.hits_request.is_none() {
            return;
        }
        // A game is on its way: wait for it.
        if self.flow.base_game.is_some() || self.discard_prompt.is_some() {
            return;
        }
        let key = self.pattern_game_key();
        if key == self.pattern.hits_key {
            return;
        }
        self.pattern.hits = None;
        self.pattern.hits_request = None;
        // Occurrences in the game being played are help (online lock). The
        // key is forgotten, not kept: the game is looked at again when the
        // lock is lifted (D3299 item 44).
        if self.assist_locked() {
            self.pattern.hits_key = None;
            return;
        }
        self.pattern.hits_key = key;
        let (Some(focus), Some((slot, number))) = (self.pattern.focus.as_ref(), key) else {
            return;
        };
        if slot != focus.slot {
            return;
        }
        let (pattern, land_first) = (focus.pattern.clone(), focus.first == Some(number));
        self.pattern_request_hits(pattern, key, false, land_first, effects);
    }

    fn pattern_hits_done(
        &mut self,
        request: RequestId,
        result: Result<Vec<mychess_db::PatternHit>, PatternFailure>,
    ) {
        let Some(asked) = self.pattern.hits_request.filter(|r| r.request == request) else {
            return;
        };
        self.pattern.hits_request = None;
        if self.assist_locked() || self.pattern.hits_key != asked.key {
            return;
        }
        match result {
            Ok(hits) => {
                let first = hits.first().map(|h| h.ply);
                self.pattern.hits = Some(GameHits {
                    hits,
                    current: 0,
                    from_test: asked.from_test,
                });
                if asked.land_first {
                    if let Some(focus) = self.pattern.focus.as_mut() {
                        focus.first = None;
                    }
                    if let Some(ply) = first {
                        self.pattern_navigate(u32::from(ply));
                    }
                }
            }
            Err(PatternFailure::Pattern(issue)) => {
                self.pattern_notice(PatternSearchNotice::InvalidPattern(issue));
            }
            Err(_) => self.pattern_notice(PatternSearchNotice::HitsFailed),
        }
    }

    fn pattern_navigate(&mut self, ply: u32) {
        drop(
            self.session
                .update(GameMessage::Navigate(Nav::GoToMainlinePly(ply))),
        );
    }

    fn pattern_go_to_hit(&mut self, index: usize) {
        let Some(hits) = self.pattern.hits.as_mut() else {
            return;
        };
        let Some(ply) = hits.hits.get(index).map(|h| h.ply) else {
            return;
        };
        hits.current = index;
        self.pattern_navigate(u32::from(ply));
    }

    fn pattern_step_hit(&mut self, forward: bool) {
        let at = self.cursor_mainline_ply();
        let Some(hits) = self.pattern.hits.as_ref() else {
            return;
        };
        let len = hits.hits.len();
        if len == 0 {
            return;
        }
        let target = match (at, forward) {
            (Some(c), true) => hits
                .hits
                .iter()
                .position(|h| u32::from(h.ply) > c)
                .unwrap_or(0),
            (Some(c), false) => hits
                .hits
                .iter()
                .rposition(|h| u32::from(h.ply) < c)
                .unwrap_or(len - 1),
            (None, true) => (hits.current + 1) % len,
            (None, false) => (hits.current + len - 1) % len,
        };
        self.pattern_go_to_hit(target);
    }

    // ---- the user's patterns ----

    fn pattern_saved_command(
        &mut self,
        command: PatternCommand,
        op: SavedOp,
        effects: &mut Vec<Effect>,
    ) {
        // Writing a file needs no folder.
        let needs_dir = !matches!(command, PatternCommand::Export { .. });
        let dir = match (self.pattern_dir(), needs_dir) {
            (Some(dir), _) => dir,
            (None, false) => PathBuf::new(),
            (None, true) => {
                // Listing with nowhere to look is not an error to shout
                // about: the window just has no list.
                if !matches!(command, PatternCommand::List) {
                    self.pattern_notice(PatternSearchNotice::NoSavedFolder);
                }
                return;
            }
        };
        if matches!(command, PatternCommand::List) {
            self.pattern.saved_listed = true;
        }
        let request = self.new_request();
        self.pattern.saved_request = Some(request);
        self.pattern.saved_op = op;
        effects.push(Effect::PatternSaved {
            request,
            dir,
            command,
        });
    }

    fn pattern_saved_done(
        &mut self,
        request: RequestId,
        result: Result<SavedOutcome, SavedFailure>,
        effects: &mut Vec<Effect>,
    ) {
        if self.pattern.saved_request != Some(request) {
            return;
        }
        self.pattern.saved_request = None;
        let op = std::mem::replace(&mut self.pattern.saved_op, SavedOp::List);
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(failure) => return self.pattern_saved_failed(&op, failure),
        };
        match outcome {
            SavedOutcome::Listed(list) => self.pattern.saved = list,
            SavedOutcome::Saved(name) => {
                self.pattern_notice(PatternSearchNotice::Saved(name));
                self.pattern_saved_command(PatternCommand::List, SavedOp::List, effects);
            }
            SavedOutcome::Deleted(name) => {
                self.pattern_notice(PatternSearchNotice::Deleted(name));
                self.pattern_saved_command(PatternCommand::List, SavedOp::List, effects);
            }
            SavedOutcome::Loaded(file) => self.pattern_apply_file(*file),
            SavedOutcome::Imported(file) => {
                let name = file.pattern.name.clone();
                self.pattern_apply_file(*file);
                self.pattern_notice(PatternSearchNotice::Imported(name));
                self.pattern_saved_command(PatternCommand::List, SavedOp::List, effects);
            }
            SavedOutcome::Exported => self.pattern_notice(PatternSearchNotice::Exported),
        }
    }

    fn pattern_saved_failed(&mut self, op: &SavedOp, failure: SavedFailure) {
        let notice = match failure {
            SavedFailure::NotFound => {
                let name = match op {
                    SavedOp::Load(name) | SavedOp::Delete(name) => name.clone(),
                    _ => String::new(),
                };
                PatternSearchNotice::SavedNotFound(name)
            }
            // The name was refused by the folder of patterns or by the file
            // chosen: for an import or an export that is the file's failure.
            SavedFailure::InvalidName => match op {
                SavedOp::Import => PatternSearchNotice::ImportFailed,
                SavedOp::Export => PatternSearchNotice::ExportFailed,
                _ => PatternSearchNotice::InvalidSavedName,
            },
            SavedFailure::NewerVersion => PatternSearchNotice::NewerVersion,
            SavedFailure::TooLarge | SavedFailure::NotAPattern => match op {
                SavedOp::Import => PatternSearchNotice::ImportFailed,
                _ => PatternSearchNotice::FileUnreadable,
            },
            SavedFailure::Invalid(PatternIssue::Other) => PatternSearchNotice::FileUnreadable,
            SavedFailure::Invalid(issue) => PatternSearchNotice::InvalidPattern(issue),
            _ => match op {
                SavedOp::Import => PatternSearchNotice::ImportFailed,
                SavedOp::Export => PatternSearchNotice::ExportFailed,
                _ => PatternSearchNotice::SavedFailed,
            },
        };
        self.pattern_notice(notice);
    }

    /// Puts a loaded pattern into the "Custom" tab and shows it. Loading or
    /// importing does not touch `[pattern_search]`: its scope is kept in
    /// the window and reaches the preferences at the next `Start`
    /// (D3299 item 44).
    fn pattern_apply_file(&mut self, file: PatternFile) {
        self.pattern.scope = Some(file.pattern.scope.clone());
        self.pattern.draft = file.pattern;
        self.pattern.selection = None;
        self.pattern.tab = PatternTab::Advanced;
        self.pattern.issue = None;
    }
}
