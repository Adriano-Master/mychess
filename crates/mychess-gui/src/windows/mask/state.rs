//! The visual state of the masks and its events (spec M5-4c, D1880 to
//! D1899). Nothing here is a rule about the mask: the application decides;
//! this state holds what only the screen needs (an open menu, the text of an
//! editor, the folded nodes of the outline) and turns clicks into messages.

use std::collections::HashSet;
use std::sync::Arc;

use iced::Point;
use iced::widget::text_editor;
use mychess_app::{
    AppState, DiscardChoice, FillSide, Marker, MarkerSlot, MaskColor, MaskEdit, MaskKey,
    MaskMessage, MaskNag, MaskQuery, MaskScope, MaskTarget, Message as AppMessage, Outline,
    OutlineNode,
};

use super::app_message;
use crate::windows::dialog_ui::Outcome;

/// Height of a row of the result list and of the outline, in logical pixels.
pub const LIST_ROW_HEIGHT: f32 = 24.0;
/// Height of the visible part of those lists.
pub const LIST_HEIGHT: f32 = 280.0;

/// What the pointer is over: the tool tip of a comment shows its whole text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hover {
    /// The comment of a row.
    Row(MaskTarget),
    /// The strip with the comment of the position.
    Position,
}

/// The second card of the context menu (D1830).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextPage {
    /// The markers of a slot.
    Marker(MarkerSlot),
    /// The colors.
    Color,
    /// The NAGs.
    Nag,
    /// "Add this move to the mask": the legal moves.
    AddMove,
}

/// The open context menu of a row (D1830).
#[derive(Debug, Clone, PartialEq)]
pub struct ContextMenu {
    /// The move.
    pub target: MaskTarget,
    /// Its text, for the labels.
    pub san: String,
    /// The move is in the mask.
    pub in_mask: bool,
    /// Where the pointer was, in window coordinates.
    pub position: Point,
    /// The command key was held on the click: the edit reaches the line
    /// (D1822).
    pub command: bool,
    /// The submenu that is open.
    pub page: Option<ContextPage>,
}

/// An event of the visual state of the masks.
#[derive(Debug, Clone)]
pub enum MaskUiMessage {
    /// The right button went down on a row.
    OpenContext {
        /// The move.
        target: MaskTarget,
        /// Its text.
        san: String,
        /// It is in the mask.
        in_mask: bool,
        /// The pointer, in window coordinates.
        position: Point,
        /// The command key is held.
        command: bool,
    },
    /// Closes the context menu.
    CloseContext,
    /// Opens or closes a submenu of the context menu.
    ContextPage(ContextPage),
    /// The box "apply to the line" of the context menu.
    ToggleApplyLine,
    /// An edit chosen in the context menu.
    ContextEdit(MaskEdit),
    /// A move of "add this move to the mask".
    ContextAddMove(String),
    /// "Comment the move" (`false`) or "Comment the position" (`true`).
    ContextComment {
        /// The comment of the position.
        position: bool,
    },
    /// The pointer entered or left a comment.
    Hover(Option<Hover>),
    /// Opens the comment editor on the position (double click on the strip).
    OpenPositionComment,
    /// An action of the text area of the comment editor.
    CommentAction(text_editor::Action),
    /// Saves the comment.
    CommentCommit,
    /// Throws the comment away.
    CommentCancel,
    /// Opens the question of "fill with the line".
    OpenFillLine,
    /// Closes it.
    CloseFillLine,
    /// The answer: fill with the moves of this side.
    FillLine(FillSide),
    /// Opens the search dialog.
    OpenSearch,
    /// A change of the form of the search.
    Search(SearchEdit),
    /// Runs the search.
    SearchRun,
    /// Closes the search dialog.
    SearchClose,
    /// The result list scrolled.
    SearchScrolled(f32),
    /// A row of the result was clicked.
    SearchSelect(usize),
    /// A row of the result was double clicked (or Enter).
    SearchGo(usize),
    /// Fold or unfold a row of the outline.
    OutlineToggle(usize),
    /// Unfold every node.
    OutlineExpandAll,
    /// Fold every node.
    OutlineCollapseAll,
    /// "Show NAGs and markers".
    OutlineMarks(bool),
    /// "Show comments".
    OutlineComments(bool),
    /// The outline scrolled.
    OutlineScrolled(f32),
    /// A row of the outline was clicked.
    OutlineSelect(usize),
    /// A row of the outline was double clicked (or Enter).
    OutlineGo(usize),
    /// Builds the outline again.
    OutlineRefresh,
    /// Closes the outline.
    OutlineClose,
}

/// A change of the form of the search (D1832). A filter is on when its box
/// is ticked; typing in a text field ticks its box.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchEdit {
    /// The NAG box.
    NagOn(bool),
    /// The NAG (`None`: moves with no NAG).
    Nag(Option<MaskNag>),
    /// The box of a marker slot.
    MarkerOn(MarkerSlot, bool),
    /// The marker of a slot (`None`: moves with no marker).
    Marker(MarkerSlot, Option<Marker>),
    /// The color box.
    ColorOn(bool),
    /// The color.
    Color(MaskColor),
    /// The box of the text of the comment of the move.
    MoveTextOn(bool),
    /// The text of the comment of the move.
    MoveText(String),
    /// The box of the text of the comment of the position.
    PositionTextOn(bool),
    /// The text of the comment of the position.
    PositionText(String),
}

/// The form of the search: a value and a box for each filter, so that
/// un-ticking a filter does not forget what was typed.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchForm {
    /// The NAG filter is on.
    pub nag_on: bool,
    /// The NAG looked for.
    pub nag: Option<MaskNag>,
    /// The marker filters are on.
    pub marker_on: [bool; 2],
    /// The markers looked for.
    pub markers: [Option<Marker>; 2],
    /// The color filter is on.
    pub color_on: bool,
    /// The color looked for.
    pub color: MaskColor,
    /// The text filter of the move is on.
    pub move_on: bool,
    /// The text.
    pub move_text: String,
    /// The text filter of the position is on.
    pub position_on: bool,
    /// The text.
    pub position_text: String,
}

impl Default for SearchForm {
    fn default() -> SearchForm {
        SearchForm {
            nag_on: false,
            nag: Some(MaskNag::Good),
            marker_on: [false; 2],
            markers: [Some(Marker::Include), Some(Marker::Include)],
            color_on: false,
            color: MaskColor::Green,
            move_on: false,
            move_text: String::new(),
            position_on: false,
            position_text: String::new(),
        }
    }
}

fn slot_index(slot: MarkerSlot) -> usize {
    match slot {
        MarkerSlot::First => 0,
        MarkerSlot::Second => 1,
    }
}

impl SearchForm {
    /// The query of the filters that are on.
    pub fn query(&self) -> MaskQuery {
        MaskQuery {
            nag: self.nag_on.then(|| self.nag.clone()),
            marker1: self.marker_on[0].then(|| self.markers[0].clone()),
            marker2: self.marker_on[1].then(|| self.markers[1].clone()),
            color: self.color_on.then(|| self.color.clone()),
            move_text: self.move_on.then(|| self.move_text.clone()),
            position_text: self.position_on.then(|| self.position_text.clone()),
        }
    }

    /// Applies a change.
    pub fn apply(&mut self, edit: SearchEdit) {
        match edit {
            SearchEdit::NagOn(on) => self.nag_on = on,
            SearchEdit::Nag(nag) => {
                self.nag = nag;
                self.nag_on = true;
            }
            SearchEdit::MarkerOn(slot, on) => self.marker_on[slot_index(slot)] = on,
            SearchEdit::Marker(slot, marker) => {
                self.markers[slot_index(slot)] = marker;
                self.marker_on[slot_index(slot)] = true;
            }
            SearchEdit::ColorOn(on) => self.color_on = on,
            SearchEdit::Color(color) => {
                self.color = color;
                self.color_on = true;
            }
            SearchEdit::MoveTextOn(on) => self.move_on = on,
            SearchEdit::MoveText(text) => {
                self.move_text = text;
                self.move_on = true;
            }
            SearchEdit::PositionTextOn(on) => self.position_on = on,
            SearchEdit::PositionText(text) => {
                self.position_text = text;
                self.position_on = true;
            }
        }
    }
}

/// The search dialog: the form, the scroll and the selection of the list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchUi {
    /// The filters.
    pub form: SearchForm,
    /// Pixels scrolled in the result list.
    pub scroll: f32,
    /// The row that Enter goes to.
    pub selected: Option<usize>,
    /// The application has had a result for this dialog: when it forgets
    /// it (the game was replaced by "go to"), the dialog closes too.
    pub seen_result: bool,
}

/// A row of the outline as it is on screen: the node (by its path of indices
/// from the roots) and how deep it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatRow {
    /// The indices from the roots.
    pub path: Box<[u32]>,
    /// How many levels the row is indented.
    pub depth: usize,
}

/// The outline dialog: the nodes shown, folded or not.
#[derive(Debug, Default)]
pub struct OutlineUi {
    /// The outline the rows were made from.
    pub outline: Option<Arc<Outline>>,
    /// The visible rows, in order.
    pub rows: Vec<FlatRow>,
    /// The nodes that are folded, by path.
    pub folded: HashSet<Box<[u32]>>,
    /// "Show NAGs and markers" is off.
    pub hide_marks: bool,
    /// "Show comments" is off.
    pub hide_comments: bool,
    /// Pixels scrolled.
    pub scroll: f32,
    /// The node that Enter goes to, by path: a row index would point at
    /// another node once a fold above it changes the rows.
    pub selected: Option<Box<[u32]>>,
}

impl OutlineUi {
    /// The row of the selected node, if it is on screen.
    pub fn selected_row(&self) -> Option<usize> {
        let path = self.selected.as_deref()?;
        self.rows.iter().position(|row| &*row.path == path)
    }
}

/// The node of `outline` at `path`.
pub fn node_at<'a>(outline: &'a Outline, path: &[u32]) -> Option<&'a OutlineNode> {
    let (first, rest) = path.split_first()?;
    let mut node = outline.roots.get(usize::try_from(*first).ok()?)?;
    for index in rest {
        node = node.children.get(usize::try_from(*index).ok()?)?;
    }
    Some(node)
}

fn child_path(path: &[u32], index: usize) -> Option<Vec<u32>> {
    let mut next = path.to_vec();
    next.push(u32::try_from(index).ok()?);
    Some(next)
}

/// The rows of the outline with the nodes of `folded` closed. The walk uses
/// a stack, not recursion: the depth of an outline is data.
pub fn flatten<S: std::hash::BuildHasher>(
    outline: &Outline,
    folded: &HashSet<Box<[u32]>, S>,
) -> Vec<FlatRow> {
    let mut rows = Vec::new();
    let mut stack: Vec<Vec<u32>> = (0..outline.roots.len())
        .rev()
        .filter_map(|i| child_path(&[], i))
        .collect();
    while let Some(path) = stack.pop() {
        let Some(node) = node_at(outline, &path) else {
            continue;
        };
        rows.push(FlatRow {
            depth: path.len().saturating_sub(1),
            path: path.clone().into_boxed_slice(),
        });
        if !node.children.is_empty() && !folded.contains(path.as_slice()) {
            stack.extend(
                (0..node.children.len())
                    .rev()
                    .filter_map(|i| child_path(&path, i)),
            );
        }
    }
    rows
}

/// Every node that has children (what "collapse all" folds).
fn branches(outline: &Outline) -> HashSet<Box<[u32]>> {
    let mut found = HashSet::new();
    let mut stack: Vec<Vec<u32>> = (0..outline.roots.len())
        .filter_map(|i| child_path(&[], i))
        .collect();
    while let Some(path) = stack.pop() {
        let Some(node) = node_at(outline, &path) else {
            continue;
        };
        if !node.children.is_empty() {
            stack.extend((0..node.children.len()).filter_map(|i| child_path(&path, i)));
            found.insert(path.into_boxed_slice());
        }
    }
    found
}

/// What a comment editor edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentFor {
    /// A move of the table.
    Move {
        /// Which.
        target: MaskTarget,
        /// Its text, for the title.
        san: String,
    },
    /// The position of the table.
    Position,
}

/// The comment editor: the text is a draft until it is saved.
#[derive(Debug)]
pub struct CommentDraft {
    /// What is edited.
    pub target: CommentFor,
    /// The text area.
    pub content: text_editor::Content,
    /// The text the mask had.
    pub original: String,
}

/// Everything the interface of the masks keeps on screen.
#[derive(Debug, Default)]
pub struct MaskUi {
    /// The context menu of a row.
    pub context: Option<ContextMenu>,
    /// The box "apply to the line" (it turns itself off after each use,
    /// D1822).
    pub apply_line: bool,
    /// The comment the pointer is over.
    pub hover: Option<Hover>,
    /// The question of "fill with the line" is open.
    pub fill_line_open: bool,
    /// The comment editor.
    pub comment: Option<CommentDraft>,
    /// The search dialog.
    pub search: Option<SearchUi>,
    /// The outline.
    pub outline: OutlineUi,
}

/// The dialogs of the masks, for the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskDialog {
    /// "Save the mask?".
    Prompt,
    /// "Fill with the line".
    FillLine,
    /// The comment editor.
    Comment,
    /// The search.
    Search,
    /// The outline.
    Outline,
}

fn send(message: MaskMessage) -> Outcome {
    Outcome::send(app_message(message))
}

impl MaskUi {
    /// Whether a dialog or popup of the masks is on screen.
    pub fn popup_open(&self) -> bool {
        self.context.is_some()
    }

    /// Closes the context menu; `true` if there was one.
    pub fn close_context(&mut self) -> bool {
        self.apply_line = false;
        self.context.take().is_some()
    }

    /// Whether the comment editor, the question of the line or the search
    /// is open (the dialogs the visual state owns).
    pub fn dialog(&self, app: &AppState) -> Option<MaskDialog> {
        if app.mask_prompt().is_some() {
            Some(MaskDialog::Prompt)
        } else if self.comment.is_some() {
            Some(MaskDialog::Comment)
        } else if self.fill_line_open {
            Some(MaskDialog::FillLine)
        } else if self.search.is_some() {
            Some(MaskDialog::Search)
        } else if app.mask_outline().is_some() {
            Some(MaskDialog::Outline)
        } else {
            None
        }
    }

    /// Brings the outline rows up to date with the application: a new
    /// outline resets the folds, the scroll and the selection.
    pub fn sync(&mut self, app: &AppState) {
        if !app.mask_open() {
            // The mask went away under the popups: nothing is left to edit.
            self.context = None;
            self.comment = None;
            self.fill_line_open = false;
            self.search = None;
            self.hover = None;
            self.apply_line = false;
        }
        if let Some(search) = &mut self.search {
            if app.mask_search().is_some() {
                search.seen_result = true;
            } else if search.seen_result {
                // "Go to" replaced the game and the application closed the
                // search with it.
                self.search = None;
            }
        }
        let current = app.mask_outline().and_then(|state| state.outline.clone());
        let same = match (&self.outline.outline, &current) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        if same {
            return;
        }
        let (hide_marks, hide_comments) = (self.outline.hide_marks, self.outline.hide_comments);
        self.outline = OutlineUi {
            rows: current
                .as_deref()
                .map(|o| flatten(o, &HashSet::new()))
                .unwrap_or_default(),
            outline: current,
            hide_marks,
            hide_comments,
            ..OutlineUi::default()
        };
    }

    fn reflatten(&mut self) {
        self.outline.rows = self
            .outline
            .outline
            .as_deref()
            .map(|o| flatten(o, &self.outline.folded))
            .unwrap_or_default();
    }

    /// Esc in a dialog of the masks.
    pub fn escape(&mut self, dialog: MaskDialog) -> Outcome {
        match dialog {
            MaskDialog::Prompt => send(MaskMessage::Answer(DiscardChoice::Cancel)),
            MaskDialog::FillLine => {
                self.fill_line_open = false;
                Outcome::default()
            }
            MaskDialog::Comment => {
                self.comment = None;
                Outcome::default()
            }
            MaskDialog::Search => {
                self.search = None;
                send(MaskMessage::CloseSearch)
            }
            MaskDialog::Outline => send(MaskMessage::CloseOutline),
        }
    }

    /// Enter in a dialog of the masks (and no text field using it): the
    /// default answer.
    pub fn enter(&mut self, dialog: MaskDialog, app: &AppState) -> Outcome {
        match dialog {
            // Saving loses nothing, so it is the default (the Esc answer is
            // "Cancel").
            MaskDialog::Prompt => send(MaskMessage::Answer(DiscardChoice::Save)),
            MaskDialog::FillLine => self.update(MaskUiMessage::FillLine(FillSide::All), app),
            MaskDialog::Comment => self.update(MaskUiMessage::CommentCommit, app),
            MaskDialog::Search => match self.search.as_ref().and_then(|s| s.selected) {
                Some(row) => self.update(MaskUiMessage::SearchGo(row), app),
                None => self.update(MaskUiMessage::SearchRun, app),
            },
            MaskDialog::Outline => match self.outline.selected_row() {
                Some(row) => self.update(MaskUiMessage::OutlineGo(row), app),
                None => Outcome::default(),
            },
        }
    }

    /// Handles an event; the outcome carries the messages for the
    /// application.
    #[allow(clippy::too_many_lines)] // one arm per event of the masks
    pub fn update(&mut self, message: MaskUiMessage, app: &AppState) -> Outcome {
        match message {
            MaskUiMessage::OpenContext {
                target,
                san,
                in_mask,
                position,
                command,
            } => {
                self.apply_line = false;
                if app.mask_open() {
                    self.context = Some(ContextMenu {
                        target,
                        san,
                        in_mask,
                        position,
                        command,
                        page: None,
                    });
                }
                Outcome::default()
            }
            MaskUiMessage::CloseContext => {
                self.context = None;
                self.apply_line = false;
                Outcome::default()
            }
            MaskUiMessage::ContextPage(page) => {
                if let Some(context) = &mut self.context {
                    context.page = (context.page != Some(page)).then_some(page);
                }
                Outcome::default()
            }
            MaskUiMessage::ToggleApplyLine => {
                self.apply_line = !self.apply_line;
                Outcome::default()
            }
            MaskUiMessage::ContextEdit(edit) => {
                let Some(context) = self.context.take() else {
                    return Outcome::default();
                };
                let scope = if context.command || self.apply_line {
                    MaskScope::Line
                } else {
                    MaskScope::One
                };
                self.apply_line = false;
                send(MaskMessage::Edit {
                    target: context.target,
                    edit,
                    scope,
                })
            }
            MaskUiMessage::ContextAddMove(san) => {
                self.context = None;
                send(MaskMessage::Edit {
                    target: MaskTarget::San(san),
                    edit: MaskEdit::Add,
                    scope: MaskScope::One,
                })
            }
            MaskUiMessage::ContextComment { position } => {
                let Some(context) = self.context.take() else {
                    return Outcome::default();
                };
                if position {
                    self.open_comment(CommentFor::Position, app)
                } else {
                    self.open_comment(
                        CommentFor::Move {
                            target: context.target,
                            san: context.san,
                        },
                        app,
                    )
                }
            }
            MaskUiMessage::OpenPositionComment => {
                self.context = None;
                self.open_comment(CommentFor::Position, app)
            }
            MaskUiMessage::Hover(hover) => {
                self.hover = hover;
                Outcome::default()
            }
            MaskUiMessage::CommentAction(action) => {
                if let Some(draft) = &mut self.comment {
                    draft.content.perform(action);
                }
                Outcome::default()
            }
            MaskUiMessage::CommentCommit => self.commit_comment(),
            MaskUiMessage::CommentCancel => {
                self.comment = None;
                Outcome::default()
            }
            MaskUiMessage::OpenFillLine => {
                self.fill_line_open = app.mask_open();
                Outcome::default()
            }
            MaskUiMessage::CloseFillLine => {
                self.fill_line_open = false;
                Outcome::default()
            }
            MaskUiMessage::FillLine(side) => {
                self.fill_line_open = false;
                send(MaskMessage::FillLine(side))
            }
            MaskUiMessage::OpenSearch => {
                if app.mask_open() && self.search.is_none() {
                    self.search = Some(SearchUi::default());
                }
                Outcome::default()
            }
            MaskUiMessage::Search(edit) => {
                if let Some(search) = &mut self.search {
                    search.form.apply(edit);
                }
                Outcome::default()
            }
            MaskUiMessage::SearchRun => {
                let Some(search) = &mut self.search else {
                    return Outcome::default();
                };
                search.scroll = 0.0;
                search.selected = None;
                send(MaskMessage::Search(Box::new(search.form.query())))
            }
            MaskUiMessage::SearchClose => self.escape(MaskDialog::Search),
            MaskUiMessage::SearchScrolled(offset) => {
                if let Some(search) = &mut self.search {
                    search.scroll = if offset.is_finite() {
                        offset.max(0.0)
                    } else {
                        0.0
                    };
                }
                Outcome::default()
            }
            MaskUiMessage::SearchSelect(row) => {
                if let Some(search) = &mut self.search {
                    search.selected = Some(row);
                }
                Outcome::default()
            }
            MaskUiMessage::SearchGo(row) => {
                let key = app
                    .mask_search()
                    .and_then(|state| state.hits.as_ref())
                    .and_then(|hits| hits.rows.get(row))
                    .map(|hit| hit.key.clone());
                Self::go_to(key)
            }
            MaskUiMessage::OutlineToggle(row) => {
                if let Some(flat) = self.outline.rows.get(row) {
                    let path = flat.path.clone();
                    if !self.outline.folded.remove(&path) {
                        self.outline.folded.insert(path);
                    }
                    self.reflatten();
                }
                Outcome::default()
            }
            MaskUiMessage::OutlineExpandAll => {
                self.outline.selected = None;
                self.outline.folded.clear();
                self.reflatten();
                Outcome::default()
            }
            MaskUiMessage::OutlineCollapseAll => {
                self.outline.selected = None;
                self.outline.folded = self
                    .outline
                    .outline
                    .as_deref()
                    .map(branches)
                    .unwrap_or_default();
                self.reflatten();
                // Fewer rows: the scroll is clamped by the list itself.
                Outcome::default()
            }
            MaskUiMessage::OutlineMarks(on) => {
                self.outline.hide_marks = !on;
                Outcome::default()
            }
            MaskUiMessage::OutlineComments(on) => {
                self.outline.hide_comments = !on;
                Outcome::default()
            }
            MaskUiMessage::OutlineScrolled(offset) => {
                self.outline.scroll = if offset.is_finite() {
                    offset.max(0.0)
                } else {
                    0.0
                };
                Outcome::default()
            }
            MaskUiMessage::OutlineSelect(row) => {
                self.outline.selected = self.outline.rows.get(row).map(|flat| flat.path.clone());
                Outcome::default()
            }
            MaskUiMessage::OutlineGo(row) => {
                let key = self
                    .outline
                    .rows
                    .get(row)
                    .and_then(|flat| {
                        let outline = self.outline.outline.as_deref()?;
                        node_at(outline, &flat.path)
                    })
                    .and_then(|node| node.steps.last())
                    .and_then(|step| step.after.clone());
                Self::go_to(key)
            }
            MaskUiMessage::OutlineRefresh => send(MaskMessage::BuildOutline),
            MaskUiMessage::OutlineClose => self.escape(MaskDialog::Outline),
        }
    }

    /// Asks for a game at `key`; a node
    /// whose move is illegal has no position (nothing happens).
    fn go_to(key: Option<MaskKey>) -> Outcome {
        let Some(key) = key else {
            return Outcome::default();
        };
        // The dialog stays: the application closes the search or the outline
        // when the game is really replaced (D1900), not before the answer to
        // "save the game?".
        Outcome::send(app_message(MaskMessage::GoTo(key)))
    }

    fn open_comment(&mut self, target: CommentFor, app: &AppState) -> Outcome {
        let original = match &target {
            CommentFor::Position => app.mask_comment(None),
            CommentFor::Move { target, .. } => app.mask_comment(Some(target)),
        };
        let Some(original) = original else {
            return Outcome::default();
        };
        self.comment = Some(CommentDraft {
            content: text_editor::Content::with_text(&original),
            target,
            original,
        });
        // `DialogUi::newly_opened` focuses the text area once it exists.
        Outcome::default()
    }

    fn commit_comment(&mut self) -> Outcome {
        let Some(draft) = self.comment.take() else {
            return Outcome::default();
        };
        let text = draft.content.text().trim().to_owned();
        if text == draft.original.trim() {
            return Outcome::default();
        }
        match draft.target {
            CommentFor::Position => send(MaskMessage::SetPositionComment(text)),
            CommentFor::Move { target, .. } => send(MaskMessage::Edit {
                target,
                edit: MaskEdit::Comment(text),
                scope: MaskScope::One,
            }),
        }
    }
}

/// The first row to build and how many, for a list of `total` rows of
/// `row` pixels scrolled by `scroll` in a view of `viewport` pixels: the
/// visible rows and a margin, so fast scrolling shows no gap.
pub fn window(scroll: f32, total: usize, row: f32, viewport: f32) -> (usize, usize) {
    const MARGIN: usize = 3;
    let scroll = if scroll.is_finite() {
        scroll.max(0.0)
    } else {
        0.0
    };
    // Non-negative quotients of a few thousand pixels at most.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let first = (scroll / row).floor() as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let fit = (viewport / row).ceil() as usize;
    let first = first.saturating_sub(MARGIN).min(total);
    let count = (fit + 2 * MARGIN + 1).min(total - first);
    (first, count)
}

/// The application message that answers the question about the mask.
pub fn answer(choice: DiscardChoice) -> AppMessage {
    app_message(MaskMessage::Answer(choice))
}

#[cfg(test)]
mod tests {
    use mychess_app::{OutlineNode, OutlineStep};
    use pretty_assertions::assert_eq;

    use super::*;

    fn step(san: &str) -> OutlineStep {
        OutlineStep {
            position_comment: String::new(),
            san: san.to_owned(),
            nag: None,
            color: MaskColor::White,
            markers: [None, None],
            comment: String::new(),
            after: Some(MaskKey::Opaque(san.to_owned())),
        }
    }

    fn node(san: &str, children: Vec<OutlineNode>) -> OutlineNode {
        OutlineNode {
            steps: vec![step(san)],
            children,
            truncated: false,
        }
    }

    fn sample() -> Outline {
        Outline {
            comment: String::new(),
            roots: vec![
                node(
                    "e4",
                    vec![node("e5", vec![node("Nf3", vec![])]), node("c5", vec![])],
                ),
                node("d4", vec![]),
            ],
            nodes: 5,
            truncated: false,
        }
    }

    fn paths(rows: &[FlatRow]) -> Vec<Vec<u32>> {
        rows.iter().map(|r| r.path.to_vec()).collect()
    }

    #[test]
    fn flatten_lists_nodes_depth_first_and_skips_the_children_of_folded_ones() {
        let outline = sample();
        let all = flatten(&outline, &HashSet::new());
        assert_eq!(
            paths(&all),
            vec![vec![0], vec![0, 0], vec![0, 0, 0], vec![0, 1], vec![1]]
        );
        assert_eq!(
            all.iter().map(|r| r.depth).collect::<Vec<_>>(),
            vec![0, 1, 2, 1, 0]
        );
        let folded: HashSet<Box<[u32]>> = [vec![0u32].into_boxed_slice()].into_iter().collect();
        assert_eq!(paths(&flatten(&outline, &folded)), vec![vec![0], vec![1]]);
        assert_eq!(branches(&outline).len(), 2);
    }

    #[test]
    fn flatten_survives_a_very_deep_outline() {
        let mut deep = node("a", vec![]);
        for _ in 0..1000 {
            deep = node("a", vec![deep]);
        }
        let outline = Outline {
            roots: vec![deep],
            ..Outline::default()
        };
        assert_eq!(flatten(&outline, &HashSet::new()).len(), 1001);
        assert_eq!(branches(&outline).len(), 1000);
    }

    #[test]
    fn node_at_follows_a_path_and_refuses_a_stale_one() {
        let outline = sample();
        assert_eq!(
            node_at(&outline, &[0, 0, 0]).map(|n| n.steps[0].san.clone()),
            Some("Nf3".to_owned())
        );
        assert!(node_at(&outline, &[0, 9]).is_none());
        assert!(node_at(&outline, &[]).is_none());
    }

    #[test]
    fn the_form_builds_a_query_from_the_filters_that_are_on() {
        let mut form = SearchForm::default();
        assert_eq!(form.query(), MaskQuery::default());
        form.apply(SearchEdit::Nag(Some(MaskNag::Blunder)));
        form.apply(SearchEdit::MoveText("trap".to_owned()));
        form.apply(SearchEdit::Marker(MarkerSlot::Second, None));
        let query = form.query();
        assert_eq!(query.nag, Some(Some(MaskNag::Blunder)));
        assert_eq!(query.move_text.as_deref(), Some("trap"));
        assert_eq!(query.marker2, Some(None));
        assert!(query.marker1.is_none() && query.color.is_none());
        // Un-ticking keeps the typed text for the next time.
        form.apply(SearchEdit::MoveTextOn(false));
        assert!(form.query().move_text.is_none());
        assert_eq!(form.move_text, "trap");
    }

    #[test]
    fn an_outline_of_twenty_thousand_nodes_builds_a_few_rows() {
        let outline = Outline {
            roots: (0..20_000)
                .map(|i| node(&format!("m{i}"), vec![]))
                .collect(),
            nodes: 20_000,
            ..Outline::default()
        };
        let rows = flatten(&outline, &HashSet::new());
        assert_eq!(rows.len(), 20_000);
        let (first, count) = window(24.0 * 10_000.0, rows.len(), LIST_ROW_HEIGHT, LIST_HEIGHT);
        assert!(count < 30, "{count}");
        assert_eq!(first + count, 10_000 - 3 + count);
    }

    #[test]
    fn the_selection_of_the_outline_follows_its_node_when_the_rows_change() {
        let outline = Arc::new(sample());
        let mut ui = OutlineUi {
            rows: flatten(&outline, &HashSet::new()),
            outline: Some(outline),
            ..OutlineUi::default()
        };
        ui.selected = Some(ui.rows[4].path.clone());
        assert_eq!(ui.selected_row(), Some(4));
        ui.folded.insert(vec![0u32].into_boxed_slice());
        ui.rows = flatten(ui.outline.as_deref().unwrap(), &ui.folded);
        assert_eq!(ui.selected_row(), Some(1), "the same node, a new row");
        ui.selected = Some(vec![0u32, 1].into_boxed_slice());
        assert_eq!(ui.selected_row(), None, "folded away");
    }

    #[test]
    fn the_window_covers_the_visible_rows_with_a_margin() {
        assert_eq!(window(0.0, 10_000, 24.0, 280.0), (0, 19));
        let (first, count) = window(2400.0, 10_000, 24.0, 280.0);
        assert_eq!(first, 97);
        assert_eq!(first + count, 97 + 19);
        assert_eq!(window(f32::NAN, 5, 24.0, 280.0), (0, 5));
        assert_eq!(window(1.0e9, 5, 24.0, 280.0), (5, 0));
    }
}
