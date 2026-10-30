//! The notation panel: the header, the option bar, the move list (a table
//! by default), the context menu, the board preview and the comment editor
//! under it (spec M2-4, D162-D173, D177; M2-5, D241-D255).
//!
//! The visual state is [`NotationPanel`], the one field the shell holds. Its
//! messages are [`NotationMessage`]; handling one returns an [`Outcome`]:
//! messages for the application plus a few requests for the shell (focus).
//! The pure parts (what the menu offers, what each item sends, which keys
//! feed the NAG shortcuts) are plain functions with tests.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use iced::widget::{checkbox, column, container, radio, row, text};
use iced::{Element, Length, Point, Size, Subscription, time};
use mychess_app::{
    AppState, CommentTarget, GameMessage, HeaderMode, HoverPreview, Message as AppMessage,
    MovePlace, Nav, NotationLayout, NotationOptions, PrefChange, PrefsMessage, StripKind,
    VariationTarget,
};
use mychess_core::{Game, Move, Nag, NodeId, Position};

use super::comment_editor::{self, CommentEditorUi, EditorMessage};
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme;
use crate::widgets::board::geometry::Orientation;
use crate::widgets::board::marks::MarkBrush;
use crate::widgets::board::pieces::{PieceSet, PieceSetId};
use crate::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};
use crate::widgets::context_menu::{self, Entry};
use crate::widgets::nag_palette::{self, nag_palette};
use crate::widgets::notation::hit::Hit;
use crate::widgets::notation::{Labels, NotationEvent, NotationView, text_style};
use crate::windows::notation_header;

/// Size of the text of the move list in logical pixels.
const TEXT_SIZE: f32 = 15.0;

/// The side of a square of the hover preview (spec D171).
pub const PREVIEW_SQUARE: f32 = 35.0;

/// Width of the context menu in logical pixels.
const MENU_WIDTH: f32 = 290.0;

/// How often the hover timer ticks while a hover is pending.
const HOVER_TICK: Duration = Duration::from_millis(50);

// ---- messages ----

/// A change of one option of the notation (the option bar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionChange {
    /// The way the moves are laid out: table, indented text or running
    /// text.
    Layout(NotationLayout),
    /// Piece symbols (figurines) before the moves.
    Figurines(bool),
    /// The column with the time spent on each move.
    TimeColumn(bool),
    /// Comments of the main line on blocks of their own.
    IndentComments(bool),
    /// Symbolic annotations.
    SymbolicNags(bool),
    /// A space after the move number.
    SpaceAfterNumber(bool),
    /// How much of the header to show.
    Header(HeaderMode),
    /// Hide `[%...]` codes in comments.
    StripMarks(bool),
    /// Main line in bold.
    BoldMainLine(bool),
    /// Show comments and annotations.
    ShowComments(bool),
    /// Show variations.
    ShowVariations(bool),
    /// Show the result.
    ShowResult(bool),
    /// Board preview on hover.
    HoverPreview(bool),
}

impl OptionChange {
    /// The options after the change.
    pub fn applied(self, mut options: NotationOptions) -> NotationOptions {
        match self {
            OptionChange::Layout(layout) => options.layout = layout,
            OptionChange::Figurines(on) => options.figurines = on,
            OptionChange::TimeColumn(on) => options.show_time_column = on,
            OptionChange::IndentComments(on) => options.indent_comments = on,
            OptionChange::SymbolicNags(on) => options.symbolic_nags = on,
            OptionChange::SpaceAfterNumber(on) => options.move_number_space = on,
            OptionChange::Header(mode) => options.header = mode,
            OptionChange::StripMarks(on) => options.strip_marks = on,
            OptionChange::BoldMainLine(on) => options.bold_main_line = on,
            OptionChange::ShowComments(on) => options.show_comments = on,
            OptionChange::ShowVariations(on) => options.show_variations = on,
            OptionChange::ShowResult(on) => options.show_result = on,
            OptionChange::HoverPreview(on) => {
                options.hover_preview = if on {
                    HoverPreview::Delay(DEFAULT_HOVER_MS)
                } else {
                    HoverPreview::Off
                };
            }
        }
        options
    }
}

/// The default delay of the hover preview (spec D162).
const DEFAULT_HOVER_MS: u32 = 600;

/// What a context menu item does. Node ids come from the click and are
/// dropped when the game changes under them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Move the cursor to the node.
    GoHere(NodeId),
    /// Make the variation the main line (the head of the variation).
    Promote(NodeId),
    /// Make the variation the first one.
    MakeFirst(NodeId),
    /// Delete the variation.
    DeleteVariation(NodeId),
    /// Delete this move and what follows.
    TruncateFrom(NodeId),
    /// Start the game from this position.
    TruncateBefore(NodeId),
    /// Remove every comment.
    StripComments,
    /// Remove every variation.
    StripVariations,
    /// Open the editor on a comment.
    EditComment(CommentTarget),
    /// Empty a comment.
    DeleteComment(CommentTarget),
    /// Toggle an annotation on the move of the node.
    ToggleNag(NodeId, Nag),
}

/// A message of the notation panel.
#[derive(Debug, Clone)]
pub enum NotationMessage {
    /// The move list reported something.
    Event(NotationEvent),
    /// An option of the bar changed.
    Option(OptionChange),
    /// Show or hide the option bar.
    ToggleOptions,
    /// A context menu item was chosen.
    Menu(MenuAction),
    /// The menu was dismissed.
    CloseMenu,
    /// The hover timer ticked.
    HoverTick(Instant),
    /// The comment editor reported something.
    Editor(EditorMessage),
    /// Open the editor on the comment of the cursor move (the summary of the
    /// information panel was clicked, D185).
    EditCursorComment,
    /// The left button went up somewhere in the window. The comment editor
    /// loses the focus when the button goes *down* (the toolkit's text
    /// area does), but a click on its own Cancel button only acts on the
    /// release, so the blur is judged after it: by then Cancel has already
    /// thrown the draft away.
    PointerReleased,
}

/// What handling a message asks of the shell.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Messages for the application, in order.
    pub messages: Vec<AppMessage>,
    /// Give the keyboard focus to the comment editor.
    pub focus_editor: bool,
    /// Take the focus away from the comment editor.
    pub blur_editor: bool,
    /// Ask whether the editor still has the focus (to save on blur).
    pub poll_editor_focus: bool,
    /// A remark for the status bar.
    pub notice: Option<mychess_app::AppNotice>,
}

// ---- state ----

/// A move the pointer rests on.
#[derive(Debug, Clone, Copy, PartialEq)]
struct HoverState {
    node: NodeId,
    at: Point,
    /// When the timer first saw the hover.
    since: Option<Instant>,
}

/// The position shown in the preview.
#[derive(Debug)]
struct Preview {
    at: Point,
    position: Position,
    last_move: Option<Move>,
}

/// The open context menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextMenu {
    /// What was clicked.
    pub hit: Option<Hit>,
    /// The node the menu acts on, if the click was on one.
    pub node: Option<NodeId>,
    /// Where the pointer was (window coordinates).
    pub position: Point,
}

/// The visual state of the move list.
#[derive(Debug)]
pub struct NotationUi {
    /// Grows when the list must scroll to the current move.
    pub reveal_token: u64,
    /// The open context menu.
    pub menu: Option<ContextMenu>,
    /// Whether the option bar is open.
    pub options_open: bool,
    /// The first moves of the variation cards that are folded (D248). Visual
    /// state only: dropped whenever the game changes.
    pub collapsed: HashSet<NodeId>,
    labels: Labels,
    labels_for: Option<crate::i18n::Language>,
    hover: Option<HoverState>,
    preview: Option<Preview>,
    /// The middle button is held: the preview stays until it is released.
    middle_held: bool,
    last_cursor: Option<NodeId>,
    /// The cursor was set by a click on the list: do not scroll.
    clicked: Option<NodeId>,
    seen_generation: Option<u64>,
    seen_changes: Option<u64>,
    caches: BoardCaches,
    pieces: PieceSet,
    brush: MarkBrush,
    board: BoardOptions,
    orientation: Orientation,
}

impl Default for NotationUi {
    fn default() -> NotationUi {
        NotationUi {
            reveal_token: 0,
            menu: None,
            options_open: false,
            collapsed: HashSet::new(),
            labels: Labels::default(),
            labels_for: None,
            hover: None,
            preview: None,
            middle_held: false,
            last_cursor: None,
            clicked: None,
            seen_generation: None,
            seen_changes: None,
            caches: BoardCaches::default(),
            pieces: PieceSet::default(),
            brush: MarkBrush::default(),
            board: BoardOptions::default(),
            orientation: Orientation::White,
        }
    }
}

impl NotationUi {
    /// Whether the hover timer must run.
    pub fn hover_pending(&self, options: &NotationOptions) -> bool {
        self.hover.is_some()
            && self.preview.is_none()
            && self.menu.is_none()
            && options.hover_preview != HoverPreview::Off
    }

    /// Whether a preview is on screen.
    pub fn preview_open(&self) -> bool {
        self.preview.is_some()
    }

    /// The node of the pending or shown hover.
    pub fn hovered(&self) -> Option<NodeId> {
        self.hover.map(|h| h.node)
    }

    fn drop_transients(&mut self) {
        self.menu = None;
        self.hover = None;
        if !self.middle_held {
            self.preview = None;
        }
    }

    /// Closes the menu and the preview. Returns whether something was open.
    pub fn close_transients(&mut self) -> bool {
        let open = self.menu.is_some() || self.preview.is_some();
        self.menu = None;
        self.preview = None;
        self.middle_held = false;
        open
    }

    /// Shows the preview board with the piece set `id`.
    pub fn set_piece_set(&mut self, id: PieceSetId) {
        if self.pieces.id() != id {
            self.pieces = PieceSet::load(id);
        }
    }

    /// Brings the state up to date with the application (after every
    /// message). Node ids kept here are dropped when the game changed under
    /// them (architecture 2.2.1).
    pub fn sync(
        &mut self,
        app: &AppState,
        i18n: &I18n,
        orientation: Orientation,
        board: BoardOptions,
    ) {
        if self.labels_for != Some(i18n.language()) {
            self.labels = labels(i18n);
            self.labels_for = Some(i18n.language());
        }
        let session = &app.session;
        let cursor = session.cursor();
        let generation = session.generation();
        let changes = session.changes();
        if self.seen_generation != Some(generation) {
            self.seen_generation = Some(generation);
            self.reveal_token += 1;
            self.last_cursor = None;
        }
        if self.seen_changes != Some(changes) {
            self.seen_changes = Some(changes);
            self.menu = None;
            self.hover = None;
            self.preview = None;
            self.middle_held = false;
            self.collapsed.clear();
        }
        if self.last_cursor != Some(cursor) {
            self.last_cursor = Some(cursor);
            if self.clicked != Some(cursor) {
                self.reveal_token += 1;
            }
            self.clicked = None;
            self.unfold_card_of(app, cursor);
        }
        // The preview board follows the main board's look and orientation.
        let look = BoardOptions {
            coords: crate::widgets::board::geometry::CoordsMode::None,
            show_legal_hints: false,
            show_marks: false,
            ..board
        };
        self.board = look;
        self.orientation = orientation;
        if let Some(preview) = &self.preview {
            let data = BoardData {
                position: &preview.position,
                last_move: preview.last_move,
                marks: &[],
                orientation,
                options: look,
                accepts_moves: false,
                brush: &self.brush,
                pieces: &self.pieces,
                animation: None,
            };
            self.caches.sync_data(&data);
        }
    }

    /// Unfolds the card the cursor moved into, so the current move is
    /// visible.
    fn unfold_card_of(&mut self, app: &AppState, cursor: NodeId) {
        if self.collapsed.is_empty() {
            return;
        }
        let table = app.notation_table();
        if let Some(MovePlace::Card { row, .. }) = table.find_move(app.notation(), cursor)
            && let Some(node) = table.rows.get(row).and_then(|r| r.key.node)
        {
            self.collapsed.remove(&node);
        }
    }

    fn open_preview(&mut self, app: &AppState, node: NodeId, at: Point) {
        let game = app.session.game();
        let Ok(position) = game.position_at(node) else {
            return;
        };
        let last_move = game.mv(node).ok().filter(|m| !m.is_null());
        self.preview = Some(Preview {
            at,
            position: position.clone(),
            last_move,
        });
        // The caches are brought up to date by the next `sync`; do it now
        // so the first frame of the preview is right.
        if let Some(preview) = &self.preview {
            let data = BoardData {
                position: &preview.position,
                last_move: preview.last_move,
                marks: &[],
                orientation: self.orientation,
                options: self.board,
                accepts_moves: false,
                brush: &self.brush,
                pieces: &self.pieces,
                animation: None,
            };
            self.caches.sync_data(&data);
        }
    }
}

fn labels(i18n: &I18n) -> Labels {
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("round", "");
    Labels {
        round: i18n
            .tr_args("notation-header-round", &args)
            .trim()
            .to_owned(),
        position: i18n.tr("notation-header-position"),
        empty: i18n.tr("notation-empty"),
    }
}

/// The one field of `UiState` for the notation panel.
#[derive(Debug, Default)]
pub struct NotationPanel {
    /// The move list.
    pub list: NotationUi,
    /// The comment editor.
    pub editor: CommentEditorUi,
}

impl NotationPanel {
    /// Brings the state up to date with the application.
    pub fn sync(
        &mut self,
        app: &AppState,
        i18n: &I18n,
        orientation: Orientation,
        board: BoardOptions,
    ) {
        self.list.sync(app, i18n, orientation, board);
        self.editor.sync(app);
    }

    /// Messages that save what the editor holds; the shell sends them
    /// before an action that moves the cursor or changes the game.
    pub fn take_commit(&mut self, app: &AppState) -> Vec<AppMessage> {
        self.editor.take_commit(app)
    }

    /// Opens the editor on `target` and gives it the focus. The shell shows
    /// the Comment panel (`focus_editor` in the outcome says so).
    pub fn open_editor(&mut self, app: &AppState, target: CommentTarget) -> Outcome {
        let mut out = Outcome {
            messages: self.editor.take_commit(app),
            ..Outcome::default()
        };
        self.editor.open(app, target);
        out.focus_editor = true;
        out
    }

    /// The Comment panel was selected (a click on its tab, a shortcut): the
    /// editor opens on the usual target.
    pub fn show_editor(&mut self, app: &AppState) -> Outcome {
        let target = self.editor.target();
        self.open_editor(app, target)
    }

    /// The Comment panel went out of sight: the draft is saved.
    pub fn hide_editor(&mut self, app: &AppState) -> Outcome {
        Outcome {
            messages: self.editor.take_commit(app),
            ..Outcome::default()
        }
    }

    /// Handles a message of the panel.
    pub fn update(&mut self, message: NotationMessage, app: &AppState) -> Outcome {
        match message {
            NotationMessage::Event(event) => self.on_event(event, app),
            NotationMessage::Option(change) => Outcome {
                messages: vec![AppMessage::Prefs(PrefsMessage::Set(PrefChange::Notation(
                    change.applied(*app.notation_options()),
                )))],
                ..Outcome::default()
            },
            NotationMessage::ToggleOptions => {
                self.list.options_open = !self.list.options_open;
                Outcome::default()
            }
            NotationMessage::Menu(action) => self.on_menu(action, app),
            NotationMessage::CloseMenu => {
                self.list.menu = None;
                Outcome::default()
            }
            NotationMessage::HoverTick(now) => {
                self.on_tick(now, app);
                Outcome::default()
            }
            NotationMessage::Editor(message) => self.editor.update(message, app),
            NotationMessage::EditCursorComment => {
                let session = &app.session;
                let target = comment_editor::default_target(session.game(), session.cursor());
                self.open_editor(app, target)
            }
            NotationMessage::PointerReleased => Outcome {
                poll_editor_focus: self.editor.is_dirty(),
                ..Outcome::default()
            },
        }
    }

    fn on_tick(&mut self, now: Instant, app: &AppState) {
        let delay = match app.notation_options().hover_preview {
            HoverPreview::Off => return,
            HoverPreview::Delay(ms) => Duration::from_millis(u64::from(ms)),
        };
        let list = &mut self.list;
        if list.menu.is_some() || list.preview.is_some() {
            return;
        }
        let Some(hover) = &mut list.hover else {
            return;
        };
        match hover.since {
            None => hover.since = Some(now),
            Some(since) if now.saturating_duration_since(since) >= delay => {
                let (node, at) = (hover.node, hover.at);
                list.open_preview(app, node, at);
            }
            Some(_) => {}
        }
    }

    fn on_event(&mut self, event: NotationEvent, app: &AppState) -> Outcome {
        let list = &mut self.list;
        match event {
            NotationEvent::Click(node) => {
                list.clicked = Some(node);
                list.drop_transients();
                Outcome {
                    messages: go_to(node),
                    ..Outcome::default()
                }
            }
            NotationEvent::CommentClick(target) => {
                list.drop_transients();
                let mut out = Outcome::default();
                if let CommentTarget::After(node) = target {
                    list.clicked = Some(node);
                    out.messages.extend(self.editor.take_commit(app));
                    out.messages.extend(go_to(node));
                }
                let open = self.open_editor(app, target);
                out.messages.extend(open.messages);
                out.focus_editor = true;
                out
            }
            NotationEvent::Hover(Some((node, at))) => {
                if list.menu.is_none() {
                    list.hover = Some(HoverState {
                        node,
                        at,
                        since: None,
                    });
                    if !list.middle_held {
                        list.preview = None;
                    }
                }
                Outcome::default()
            }
            NotationEvent::Hover(None) => {
                list.hover = None;
                if !list.middle_held {
                    list.preview = None;
                }
                Outcome::default()
            }
            NotationEvent::Middle(Some((node, at))) => {
                list.middle_held = true;
                list.open_preview(app, node, at);
                Outcome::default()
            }
            NotationEvent::Middle(None) => {
                list.middle_held = false;
                list.preview = None;
                Outcome::default()
            }
            NotationEvent::Menu { hit, position } => {
                list.hover = None;
                list.preview = None;
                list.menu = Some(ContextMenu {
                    hit,
                    node: hit.and_then(Hit::node),
                    position,
                });
                Outcome::default()
            }
            NotationEvent::ToggleCard(node) => {
                list.drop_transients();
                if !list.collapsed.remove(&node) {
                    list.collapsed.insert(node);
                }
                Outcome::default()
            }
            NotationEvent::Scrolled => {
                list.menu = None;
                list.preview = None;
                Outcome::default()
            }
        }
    }

    fn on_menu(&mut self, action: MenuAction, app: &AppState) -> Outcome {
        self.list.menu = None;
        let cursor = app.session.cursor();
        match action {
            MenuAction::EditComment(target) => self.open_editor(app, target),
            other => {
                if let MenuAction::GoHere(node) = other {
                    self.list.clicked = Some(node);
                }
                let mut out = Outcome {
                    messages: self.editor.take_commit(app),
                    ..Outcome::default()
                };
                let confirm = app.prefs.editing.confirm_destructive;
                out.messages.extend(
                    menu_messages(other, cursor, app.session.game().root())
                        .into_iter()
                        .map(|message| with_confirmation(message, confirm)),
                );
                out
            }
        }
    }
}

fn go_to(node: NodeId) -> Vec<AppMessage> {
    vec![AppMessage::Game(GameMessage::Navigate(Nav::GoToNode(node)))]
}

/// The head of the variation that contains `node`: the nearest ancestor (or
/// the node itself) that is not the main continuation of its parent. `None`
/// on the main line (spec rule 18).
pub fn variation_head(game: &Game, node: NodeId) -> Option<NodeId> {
    let mut current = node;
    loop {
        if game.is_variation(current).ok()? {
            return Some(current);
        }
        current = game.parent(current).ok()??;
    }
}

/// The messages for the application that a menu item sends. `cursor` is
/// where the cursor is (no item needs it: they all act on their own node and
/// leave the cursor alone). Removals ask for confirmation; the shell turns
/// that off with the `confirm_destructive` preference.
pub fn menu_messages(action: MenuAction, _cursor: NodeId, root: NodeId) -> Vec<AppMessage> {
    let game = |message| AppMessage::Game(message);
    match action {
        MenuAction::GoHere(node) => go_to(node),
        MenuAction::Promote(head) => vec![game(GameMessage::PromoteVariation(
            VariationTarget::Node(head),
        ))],
        MenuAction::MakeFirst(head) => vec![game(GameMessage::MakeFirstVariation(
            VariationTarget::Node(head),
        ))],
        MenuAction::DeleteVariation(head) => vec![game(GameMessage::DeleteVariation(
            VariationTarget::Node(head),
        ))],
        MenuAction::TruncateFrom(node) => vec![request_strip(StripKind::TruncateFrom(node))],
        MenuAction::TruncateBefore(node) => vec![request_strip(StripKind::TruncateBefore(node))],
        MenuAction::StripComments => vec![request_strip(StripKind::Comments)],
        MenuAction::StripVariations => vec![request_strip(StripKind::Variations)],
        MenuAction::EditComment(_) => Vec::new(),
        MenuAction::DeleteComment(target) => {
            let clear = match target {
                CommentTarget::After(node) => GameMessage::SetComment {
                    node,
                    text: String::new(),
                },
                CommentTarget::Before(node) => GameMessage::SetCommentBefore {
                    node,
                    text: String::new(),
                },
                CommentTarget::Initial => GameMessage::SetComment {
                    node: root,
                    text: String::new(),
                },
            };
            vec![game(clear)]
        }
        // The item acts on the clicked move; the cursor stays (D172).
        MenuAction::ToggleNag(node, nag) => {
            vec![game(GameMessage::ToggleNagAt { node, nag })]
        }
    }
}

/// Sets whether a removal asks first (the `confirm_destructive`
/// preference); other messages pass unchanged.
fn with_confirmation(message: AppMessage, confirm: bool) -> AppMessage {
    match message {
        AppMessage::Game(GameMessage::RequestStrip { kind, .. }) => {
            AppMessage::Game(GameMessage::RequestStrip { kind, confirm })
        }
        other => other,
    }
}

/// A removal that asks for confirmation with a count; the shell turns the
/// question off with [`with_confirmation`].
fn request_strip(kind: StripKind) -> AppMessage {
    AppMessage::Game(GameMessage::RequestStrip {
        kind,
        confirm: true,
    })
}

// ---- the menu ----

/// The label of a menu row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuLabel {
    /// "Go to this move".
    GoHere,
    /// "Promote variation".
    Promote,
    /// "Make first variation".
    MakeFirst,
    /// "Delete variation".
    DeleteVariation,
    /// "Strip from this move on".
    TruncateFrom,
    /// "Strip moves from start".
    TruncateBefore,
    /// "Strip all comments".
    StripComments,
    /// "Strip all variations".
    StripVariations,
    /// "Edit comment".
    EditComment,
    /// "Delete comment".
    DeleteComment,
}

impl MenuLabel {
    /// The Fluent id.
    pub fn key(self) -> &'static str {
        match self {
            MenuLabel::GoHere => "notation-menu-go-here",
            MenuLabel::Promote => "notation-menu-promote",
            MenuLabel::MakeFirst => "notation-menu-make-first",
            MenuLabel::DeleteVariation => "notation-menu-delete-variation",
            MenuLabel::TruncateFrom => "notation-menu-truncate-from",
            MenuLabel::TruncateBefore => "notation-menu-truncate-before",
            MenuLabel::StripComments => "notation-menu-strip-comments",
            MenuLabel::StripVariations => "notation-menu-strip-variations",
            MenuLabel::EditComment => "notation-menu-edit-comment",
            MenuLabel::DeleteComment => "notation-menu-delete-comment",
        }
    }
}

/// A row of the context menu, before it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuRow {
    /// An item; `None` as the action means disabled.
    Item(MenuLabel, Option<MenuAction>),
    /// A separator.
    Separator,
    /// The annotation strip for the move of this node.
    Nags(NodeId),
    /// The caption of the annotation strip.
    NagsHeading,
}

/// What the menu offers for a click on `hit` (spec D172, rules 17-19).
pub fn menu_model(game: &Game, hit: Option<Hit>) -> Vec<MenuRow> {
    let node = hit.and_then(Hit::node).filter(|n| game.contains(*n));
    let head = node.and_then(|n| variation_head(game, n));
    let item = |label, action: Option<MenuAction>| MenuRow::Item(label, action);
    let comment_target = match hit {
        Some(Hit::Comment(target)) => Some(target),
        _ => node.map(CommentTarget::After),
    };
    let has_comment = comment_target.is_some_and(|target| match target {
        CommentTarget::After(n) => game.comment(n).ok().flatten().is_some(),
        CommentTarget::Before(n) => game.comment_before(n).ok().flatten().is_some(),
        CommentTarget::Initial => game.initial_comment().is_some(),
    });
    let mut rows = vec![
        item(MenuLabel::GoHere, node.map(MenuAction::GoHere)),
        MenuRow::Separator,
        item(MenuLabel::Promote, head.map(MenuAction::Promote)),
        item(MenuLabel::MakeFirst, head.map(MenuAction::MakeFirst)),
        item(
            MenuLabel::DeleteVariation,
            head.map(MenuAction::DeleteVariation),
        ),
        MenuRow::Separator,
        item(MenuLabel::TruncateFrom, node.map(MenuAction::TruncateFrom)),
        item(
            MenuLabel::TruncateBefore,
            node.map(MenuAction::TruncateBefore),
        ),
        item(MenuLabel::StripComments, Some(MenuAction::StripComments)),
        item(
            MenuLabel::StripVariations,
            Some(MenuAction::StripVariations),
        ),
        MenuRow::Separator,
        item(
            MenuLabel::EditComment,
            comment_target.map(MenuAction::EditComment),
        ),
        item(
            MenuLabel::DeleteComment,
            comment_target
                .filter(|_| has_comment)
                .map(MenuAction::DeleteComment),
        ),
    ];
    if let Some(node) = node {
        rows.push(MenuRow::Separator);
        rows.push(MenuRow::NagsHeading);
        rows.push(MenuRow::Nags(node));
    }
    rows
}

// ---- view ----

/// The checkbox of an option.
fn option_box<'a>(
    i18n: &I18n,
    key: &str,
    on: bool,
    change: fn(bool) -> OptionChange,
) -> Element<'a, Message> {
    checkbox(on)
        .label(i18n.tr(key))
        .size(16)
        .text_size(13)
        .on_toggle(move |value| Message::Notation(NotationMessage::Option(change(value))))
        .into()
}

fn header_radio<'a>(
    i18n: &I18n,
    key: &str,
    mode: HeaderMode,
    selected: HeaderMode,
) -> Element<'a, Message> {
    radio(i18n.tr(key), mode, Some(selected), |mode| {
        Message::Notation(NotationMessage::Option(OptionChange::Header(mode)))
    })
    .size(16)
    .text_size(13)
    .into()
}

fn layout_radio<'a>(
    i18n: &I18n,
    key: &str,
    layout: NotationLayout,
    selected: NotationLayout,
) -> Element<'a, Message> {
    radio(i18n.tr(key), layout, Some(selected), |layout| {
        Message::Notation(NotationMessage::Option(OptionChange::Layout(layout)))
    })
    .size(16)
    .text_size(13)
    .into()
}

fn layout_row<'a>(i18n: &I18n, options: &NotationOptions) -> Element<'a, Message> {
    row![
        text(i18n.tr("notation-opt-layout")).size(13),
        layout_radio(
            i18n,
            "notation-opt-layout-table",
            NotationLayout::Table,
            options.layout
        ),
        layout_radio(
            i18n,
            "notation-opt-layout-indented",
            NotationLayout::Indented,
            options.layout
        ),
        layout_radio(
            i18n,
            "notation-opt-layout-inline",
            NotationLayout::Inline,
            options.layout
        ),
    ]
    .spacing(12)
    .wrap()
    .into()
}

fn options_bar<'a>(i18n: &I18n, options: &NotationOptions) -> Element<'a, Message> {
    let layouts = layout_row(i18n, options);
    let boxes: Vec<Element<'a, Message>> = vec![
        option_box(
            i18n,
            "notation-opt-figurines",
            options.figurines,
            OptionChange::Figurines,
        ),
        option_box(
            i18n,
            "notation-opt-time-column",
            options.show_time_column,
            OptionChange::TimeColumn,
        ),
        option_box(
            i18n,
            "notation-opt-indent-comments",
            options.indent_comments,
            OptionChange::IndentComments,
        ),
        option_box(
            i18n,
            "notation-opt-symbolic-nags",
            options.symbolic_nags,
            OptionChange::SymbolicNags,
        ),
        option_box(
            i18n,
            "notation-opt-space-after-number",
            options.move_number_space,
            OptionChange::SpaceAfterNumber,
        ),
        option_box(
            i18n,
            "notation-opt-strip-marks",
            options.strip_marks,
            OptionChange::StripMarks,
        ),
        option_box(
            i18n,
            "notation-opt-bold-main-line",
            options.bold_main_line,
            OptionChange::BoldMainLine,
        ),
        option_box(
            i18n,
            "notation-opt-show-comments",
            options.show_comments,
            OptionChange::ShowComments,
        ),
        option_box(
            i18n,
            "notation-opt-show-variations",
            options.show_variations,
            OptionChange::ShowVariations,
        ),
        option_box(
            i18n,
            "notation-opt-show-result",
            options.show_result,
            OptionChange::ShowResult,
        ),
        option_box(
            i18n,
            "notation-opt-hover-preview",
            options.hover_preview != HoverPreview::Off,
            OptionChange::HoverPreview,
        ),
    ];
    let header = row![
        text(i18n.tr("notation-opt-header")).size(13),
        header_radio(
            i18n,
            "notation-opt-header-full",
            HeaderMode::Full,
            options.header
        ),
        header_radio(
            i18n,
            "notation-opt-header-short",
            HeaderMode::Short,
            options.header
        ),
        header_radio(
            i18n,
            "notation-opt-header-hidden",
            HeaderMode::Hidden,
            options.header
        ),
    ]
    .spacing(12)
    .wrap();
    container(column![layouts, row(boxes).spacing(14).wrap(), header].spacing(6))
        .padding([6, 8])
        .into()
}

/// The Moves tab: the header, the option bar and the move list.
pub fn moves_tab(shell: &Shell) -> Element<'_, Message> {
    let app = &shell.app;
    let i18n = &shell.i18n;
    let panel = &shell.ui.notation;
    let options = app.notation_options();
    let theme = shell.theme();
    let style = text_style(
        &theme,
        TEXT_SIZE,
        options.symbolic_nags,
        options.bold_main_line,
    )
    .with_figurines(options.figurines);
    let session = &app.session;
    let current = Some(session.cursor()).filter(|c| *c != session.game().root());
    let mut view = NotationView::new(
        app.notation(),
        &panel.list.labels,
        style,
        current,
        |event| Message::Notation(NotationMessage::Event(event)),
    )
    .menu_target(panel.list.menu.and_then(|m| m.node))
    .reveal_token(panel.list.reveal_token);
    if options.layout == NotationLayout::Table {
        view = view.table(app.notation_table(), &panel.list.collapsed);
    }
    let list = view.into_element();

    let mut page = column![notation_header::view(app, i18n, panel.list.options_open)];
    if panel.list.options_open {
        page = page.push(options_bar(i18n, options));
    }
    page = page.push(container(list).height(Length::Fill).width(Length::Fill));
    page.height(Length::Fill).into()
}

fn menu_entries<'a>(shell: &'a Shell, menu: &ContextMenu) -> Vec<Entry<'a, Message>> {
    let i18n = &shell.i18n;
    let game = shell.app.session.game();
    let mut entries = Vec::new();
    for row in menu_model(game, menu.hit) {
        entries.push(match row {
            MenuRow::Item(label, action) => Entry::Item {
                label: i18n.tr(label.key()),
                message: action.map(|a| Message::Notation(NotationMessage::Menu(a))),
            },
            MenuRow::Separator => Entry::Separator,
            MenuRow::NagsHeading => Entry::Heading(i18n.tr("notation-menu-nags")),
            MenuRow::Nags(node) => {
                let active = game.nags(node).map(<[Nag]>::to_vec).unwrap_or_default();
                let palette = nag_palette(i18n, &active, true, move |nag| {
                    Message::Notation(NotationMessage::Menu(MenuAction::ToggleNag(node, nag)))
                });
                Entry::Block(palette, nag_palette::HEIGHT)
            }
        });
    }
    entries
}

fn window_size(shell: &Shell) -> Size {
    shell.ui.window_size.unwrap_or(Size::new(1280.0, 800.0))
}

/// The layer to push on the window `stack`: the context menu or the board
/// preview, if one is open.
pub fn overlay(shell: &Shell) -> Option<Element<'_, Message>> {
    let list = &shell.ui.notation.list;
    if let Some(menu) = &list.menu {
        let entries = menu_entries(shell, menu);
        let window = window_size(shell);
        let size = context_menu::shown_size(context_menu::card_size(&entries, MENU_WIDTH), window);
        let at = context_menu::place(menu.position, size, window);
        let card = context_menu::card(entries, MENU_WIDTH, window.height);
        return Some(context_menu::layer(
            card,
            at,
            Message::Notation(NotationMessage::CloseMenu),
        ));
    }
    let preview = list.preview.as_ref().filter(|_| !shell.app.modal_open())?;
    let side = PREVIEW_SQUARE * 8.0;
    let data = BoardData {
        position: &preview.position,
        last_move: preview.last_move,
        marks: &[],
        orientation: list.orientation,
        options: list.board,
        accepts_moves: false,
        brush: &list.brush,
        pieces: &list.pieces,
        animation: None,
    };
    let board = BoardWidget::new(data, &list.caches).into_element();
    let card = container(
        container(board.map(|_| Message::Notation(NotationMessage::CloseMenu)))
            .width(Length::Fixed(side))
            .height(Length::Fixed(side)),
    )
    .padding(3)
    .style(theme::pane_body);
    // A little away from the pointer, so the move stays visible.
    let at = Point::new(preview.at.x + 16.0, preview.at.y + 16.0);
    let size = Size::new(side + 8.0, side + 8.0);
    let at = context_menu::place(at, size, window_size(shell));
    Some(
        iced::widget::pin(card)
            .x(at.x)
            .y(at.y)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}

/// The timers this panel needs: the hover timer while a hover is pending.
/// (The timer of half-typed NAG sequences belongs to the typed-key path of
/// the shell.)
pub fn subscriptions(shell: &Shell) -> Vec<Subscription<Message>> {
    let mut all = Vec::new();
    if shell
        .ui
        .notation
        .list
        .hover_pending(shell.app.notation_options())
    {
        all.push(
            time::every(HOVER_TICK).map(|now| Message::Notation(NotationMessage::HoverTick(now))),
        );
    }
    all
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use mychess_app::{Prompt, update};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::i18n::Language;

    fn play(game: &mut Game, parent: NodeId, uci: &str) -> NodeId {
        let mv = game
            .position_at(parent)
            .expect("position")
            .parse_uci(uci)
            .expect("legal");
        game.add_move(parent, mv).expect("move")
    }

    /// 1.e4 e5 2.Nf3 (2.Nc3 Nf6) 2...Nc6 and the nodes in that order.
    struct Fixture {
        app: AppState,
        e4: NodeId,
        e5: NodeId,
        nf3: NodeId,
        nc3: NodeId,
        nf6: NodeId,
        nc6: NodeId,
    }

    fn fixture() -> Fixture {
        let mut game = Game::new();
        let root = game.root();
        let e4 = play(&mut game, root, "e2e4");
        let e5 = play(&mut game, e4, "e7e5");
        let nf3 = play(&mut game, e5, "g1f3");
        let nc3 = play(&mut game, e5, "b1c3");
        let nf6 = play(&mut game, nc3, "g8f6");
        let nc6 = play(&mut game, nf3, "b8c6");
        let mut app = AppState::default();
        app.session.replace_game(game);
        app.refresh_notation();
        Fixture {
            app,
            e4,
            e5,
            nf3,
            nc3,
            nf6,
            nc6,
        }
    }

    fn send(app: &mut AppState, messages: Vec<AppMessage>) {
        for message in messages {
            update(app, message);
        }
    }

    fn panel_for(app: &AppState) -> NotationPanel {
        let mut panel = NotationPanel::default();
        panel.sync(
            app,
            &I18n::new(Language::EnUs),
            Orientation::White,
            BoardOptions::default(),
        );
        panel
    }

    fn resync(panel: &mut NotationPanel, app: &AppState) {
        panel.sync(
            app,
            &I18n::new(Language::EnUs),
            Orientation::White,
            BoardOptions::default(),
        );
    }

    fn rows(f: &Fixture, hit: Option<Hit>) -> Vec<MenuRow> {
        menu_model(f.app.session.game(), hit)
    }

    fn item(rows: &[MenuRow], label: MenuLabel) -> Option<MenuAction> {
        rows.iter().find_map(|row| match row {
            MenuRow::Item(l, action) if *l == label => Some(*action),
            _ => None,
        })?
    }

    #[test]
    fn variation_heads_are_found_from_any_node_of_the_line() {
        let f = fixture();
        let game = f.app.session.game();
        assert_eq!(variation_head(game, f.nc3), Some(f.nc3));
        assert_eq!(variation_head(game, f.nf6), Some(f.nc3));
        assert_eq!(variation_head(game, f.nf3), None, "main continuation");
        assert_eq!(variation_head(game, f.nc6), None);
        assert_eq!(variation_head(game, f.e4), None);
        assert_eq!(variation_head(game, game.root()), None);
    }

    #[test]
    fn the_menu_on_a_main_line_move_disables_the_variation_items() {
        let f = fixture();
        let rows = rows(&f, Some(Hit::Move(f.e5)));
        assert_eq!(
            item(&rows, MenuLabel::GoHere),
            Some(MenuAction::GoHere(f.e5))
        );
        assert_eq!(item(&rows, MenuLabel::Promote), None);
        assert_eq!(item(&rows, MenuLabel::MakeFirst), None);
        assert_eq!(item(&rows, MenuLabel::DeleteVariation), None);
        assert_eq!(
            item(&rows, MenuLabel::TruncateFrom),
            Some(MenuAction::TruncateFrom(f.e5))
        );
        assert_eq!(
            item(&rows, MenuLabel::TruncateBefore),
            Some(MenuAction::TruncateBefore(f.e5))
        );
        assert_eq!(
            item(&rows, MenuLabel::StripComments),
            Some(MenuAction::StripComments)
        );
        assert_eq!(
            item(&rows, MenuLabel::EditComment),
            Some(MenuAction::EditComment(CommentTarget::After(f.e5)))
        );
        assert_eq!(
            item(&rows, MenuLabel::DeleteComment),
            None,
            "no comment to delete"
        );
        assert!(rows.contains(&MenuRow::Nags(f.e5)));
    }

    #[test]
    fn the_menu_on_a_variation_acts_on_the_head_of_the_variation() {
        let f = fixture();
        let rows = rows(&f, Some(Hit::Move(f.nf6)));
        assert_eq!(
            item(&rows, MenuLabel::Promote),
            Some(MenuAction::Promote(f.nc3))
        );
        assert_eq!(
            item(&rows, MenuLabel::MakeFirst),
            Some(MenuAction::MakeFirst(f.nc3))
        );
        assert_eq!(
            item(&rows, MenuLabel::DeleteVariation),
            Some(MenuAction::DeleteVariation(f.nc3))
        );
        // The move itself is what "strip from here" and the NAGs use.
        assert_eq!(
            item(&rows, MenuLabel::TruncateFrom),
            Some(MenuAction::TruncateFrom(f.nf6))
        );
        assert!(rows.contains(&MenuRow::Nags(f.nf6)));
    }

    #[test]
    fn the_menu_on_empty_space_offers_only_the_global_items() {
        let f = fixture();
        let rows = rows(&f, None);
        assert_eq!(item(&rows, MenuLabel::GoHere), None);
        assert_eq!(item(&rows, MenuLabel::TruncateFrom), None);
        assert_eq!(item(&rows, MenuLabel::EditComment), None);
        assert_eq!(
            item(&rows, MenuLabel::StripVariations),
            Some(MenuAction::StripVariations)
        );
        assert!(!rows.iter().any(|r| matches!(r, MenuRow::Nags(_))));
    }

    #[test]
    fn the_menu_on_a_comment_edits_and_deletes_that_comment() {
        let mut f = fixture();
        let mut game = f.app.session.game().clone();
        game.set_comment_before(f.nc3, "before").expect("comment");
        f.app.session.replace_game(game);
        let target = CommentTarget::Before(f.nc3);
        let rows = rows(&f, Some(Hit::Comment(target)));
        assert_eq!(
            item(&rows, MenuLabel::EditComment),
            Some(MenuAction::EditComment(target))
        );
        assert_eq!(
            item(&rows, MenuLabel::DeleteComment),
            Some(MenuAction::DeleteComment(target))
        );
        // The initial comment belongs to no move.
        let rows = menu_model(
            f.app.session.game(),
            Some(Hit::Comment(CommentTarget::Initial)),
        );
        assert_eq!(item(&rows, MenuLabel::GoHere), None);
        assert!(!rows.iter().any(|r| matches!(r, MenuRow::Nags(_))));
    }

    #[test]
    fn a_stale_node_gets_the_menu_of_empty_space() {
        let mut f = fixture();
        let stale = f.nf6;
        let game = Game::new();
        f.app.session.replace_game(game);
        let rows = rows(&f, Some(Hit::Move(stale)));
        assert_eq!(item(&rows, MenuLabel::GoHere), None);
    }

    #[test]
    fn menu_items_become_the_messages_of_the_session() {
        let f = fixture();
        let root = f.app.session.game().root();
        let cursor = f.e4;
        let game = |m| AppMessage::Game(m);
        assert_eq!(
            menu_messages(MenuAction::Promote(f.nc3), cursor, root),
            [game(GameMessage::PromoteVariation(VariationTarget::Node(
                f.nc3
            )))]
        );
        assert_eq!(
            menu_messages(MenuAction::MakeFirst(f.nc3), cursor, root),
            [game(GameMessage::MakeFirstVariation(
                VariationTarget::Node(f.nc3)
            ))]
        );
        assert_eq!(
            menu_messages(MenuAction::DeleteVariation(f.nc3), cursor, root),
            [game(GameMessage::DeleteVariation(VariationTarget::Node(
                f.nc3
            )))]
        );
        assert_eq!(
            menu_messages(MenuAction::TruncateFrom(f.nf3), cursor, root),
            [game(GameMessage::RequestStrip {
                kind: StripKind::TruncateFrom(f.nf3),
                confirm: true
            })]
        );
        assert_eq!(
            menu_messages(MenuAction::TruncateBefore(f.nf3), cursor, root),
            [game(GameMessage::RequestStrip {
                kind: StripKind::TruncateBefore(f.nf3),
                confirm: true
            })]
        );
        assert_eq!(
            menu_messages(MenuAction::StripComments, cursor, root),
            [game(GameMessage::RequestStrip {
                kind: StripKind::Comments,
                confirm: true
            })]
        );
        assert_eq!(
            menu_messages(MenuAction::GoHere(f.e5), cursor, root),
            [game(GameMessage::Navigate(Nav::GoToNode(f.e5)))]
        );
    }

    #[test]
    fn annotations_act_on_the_clicked_node_wherever_the_cursor_is() {
        let f = fixture();
        let root = f.app.session.game().root();
        let nag = Nag::new(1).expect("nag");
        let expected = [AppMessage::Game(GameMessage::ToggleNagAt {
            node: f.e5,
            nag,
        })];
        assert_eq!(
            menu_messages(MenuAction::ToggleNag(f.e5, nag), f.e4, root),
            expected
        );
        assert_eq!(
            menu_messages(MenuAction::ToggleNag(f.e5, nag), f.e5, root),
            expected
        );
    }

    #[test]
    fn comments_are_cleared_by_the_right_message() {
        let f = fixture();
        let root = f.app.session.game().root();
        let clear = |target| menu_messages(MenuAction::DeleteComment(target), f.e4, root);
        assert_eq!(
            clear(CommentTarget::After(f.e5)),
            [AppMessage::Game(GameMessage::SetComment {
                node: f.e5,
                text: String::new()
            })]
        );
        assert_eq!(
            clear(CommentTarget::Before(f.nc3)),
            [AppMessage::Game(GameMessage::SetCommentBefore {
                node: f.nc3,
                text: String::new()
            })]
        );
        assert_eq!(
            clear(CommentTarget::Initial),
            [AppMessage::Game(GameMessage::SetComment {
                node: root,
                text: String::new()
            })]
        );
    }

    #[test]
    fn promoting_from_the_menu_changes_the_game_without_moving_the_cursor() {
        let mut f = fixture();
        let mut panel = panel_for(&f.app);
        send(
            &mut f.app,
            vec![AppMessage::Game(GameMessage::Navigate(Nav::GoToNode(f.e4)))],
        );
        resync(&mut panel, &f.app);
        let out = panel.update(
            NotationMessage::Event(NotationEvent::Menu {
                hit: Some(Hit::Move(f.nf6)),
                position: Point::new(10.0, 10.0),
            }),
            &f.app,
        );
        assert!(out.messages.is_empty());
        assert!(panel.list.menu.is_some());
        let out = panel.update(NotationMessage::Menu(MenuAction::Promote(f.nc3)), &f.app);
        assert!(panel.list.menu.is_none(), "choosing closes the menu");
        send(&mut f.app, out.messages);
        assert_eq!(f.app.session.cursor(), f.e4, "the cursor stays");
        let game = f.app.session.game();
        assert_eq!(game.children(f.e5).expect("children"), [f.nc3, f.nf3]);
    }

    #[test]
    fn removing_from_the_menu_asks_with_a_count() {
        let mut f = fixture();
        let mut panel = panel_for(&f.app);
        let out = panel.update(NotationMessage::Menu(MenuAction::StripVariations), &f.app);
        send(&mut f.app, out.messages);
        assert!(matches!(
            f.app.session.prompt(),
            Some(Prompt::ConfirmStrip {
                kind: StripKind::Variations,
                count: 1
            })
        ));
    }

    #[test]
    fn a_click_goes_to_the_node_and_does_not_scroll_the_list() {
        let mut f = fixture();
        let mut panel = panel_for(&f.app);
        let token = panel.list.reveal_token;
        let out = panel.update(NotationMessage::Event(NotationEvent::Click(f.e5)), &f.app);
        send(&mut f.app, out.messages);
        assert_eq!(f.app.session.cursor(), f.e5);
        resync(&mut panel, &f.app);
        assert_eq!(
            panel.list.reveal_token, token,
            "a click inside does not reveal"
        );
        // Moving the cursor from elsewhere does.
        send(
            &mut f.app,
            vec![AppMessage::Game(GameMessage::Navigate(Nav::GoToNode(f.e4)))],
        );
        resync(&mut panel, &f.app);
        assert_eq!(panel.list.reveal_token, token + 1);
    }

    #[test]
    fn opening_another_game_reveals_again() {
        let mut f = fixture();
        let mut panel = panel_for(&f.app);
        let token = panel.list.reveal_token;
        f.app.session.replace_game(Game::new());
        resync(&mut panel, &f.app);
        assert!(panel.list.reveal_token > token);
    }

    fn hover_over(panel: &mut NotationPanel, app: &AppState, node: NodeId) {
        panel.update(
            NotationMessage::Event(NotationEvent::Hover(Some((node, Point::new(50.0, 60.0))))),
            app,
        );
    }

    #[test]
    fn a_preview_opens_after_the_delay_and_only_while_the_pointer_rests() {
        let f = fixture();
        let mut panel = panel_for(&f.app);
        let options = *f.app.notation_options();
        assert!(!panel.list.hover_pending(&options));
        hover_over(&mut panel, &f.app, f.e5);
        assert!(panel.list.hover_pending(&options), "timer needed");
        let t0 = Instant::now();
        panel.update(NotationMessage::HoverTick(t0), &f.app);
        panel.update(
            NotationMessage::HoverTick(t0 + Duration::from_millis(500)),
            &f.app,
        );
        assert!(!panel.list.preview_open(), "600 ms have not passed");
        panel.update(
            NotationMessage::HoverTick(t0 + Duration::from_millis(650)),
            &f.app,
        );
        assert!(panel.list.preview_open());
        assert!(!panel.list.hover_pending(&options), "timer off again");
        // Leaving closes it.
        panel.update(NotationMessage::Event(NotationEvent::Hover(None)), &f.app);
        assert!(!panel.list.preview_open());
        assert_eq!(panel.list.hovered(), None);
    }

    #[test]
    fn moving_to_another_move_restarts_the_delay() {
        let f = fixture();
        let mut panel = panel_for(&f.app);
        let t0 = Instant::now();
        hover_over(&mut panel, &f.app, f.e5);
        panel.update(NotationMessage::HoverTick(t0), &f.app);
        hover_over(&mut panel, &f.app, f.nf3);
        panel.update(
            NotationMessage::HoverTick(t0 + Duration::from_millis(700)),
            &f.app,
        );
        assert!(
            !panel.list.preview_open(),
            "the new hover was first seen at 700 ms"
        );
    }

    #[test]
    fn the_preview_can_be_turned_off() {
        let mut f = fixture();
        let off = NotationOptions {
            hover_preview: HoverPreview::Off,
            ..NotationOptions::default()
        };
        update(
            &mut f.app,
            AppMessage::Prefs(PrefsMessage::Set(PrefChange::Notation(off))),
        );
        let mut panel = panel_for(&f.app);
        hover_over(&mut panel, &f.app, f.e5);
        let options = *f.app.notation_options();
        assert!(!panel.list.hover_pending(&options), "no timer when off");
        panel.update(NotationMessage::HoverTick(Instant::now()), &f.app);
        assert!(!panel.list.preview_open());
    }

    #[test]
    fn the_middle_button_previews_at_once_and_releasing_closes_it() {
        let f = fixture();
        let mut panel = panel_for(&f.app);
        panel.update(
            NotationMessage::Event(NotationEvent::Middle(Some((f.e5, Point::new(5.0, 5.0))))),
            &f.app,
        );
        assert!(panel.list.preview_open());
        // A hover event in between does not close it while held.
        panel.update(NotationMessage::Event(NotationEvent::Hover(None)), &f.app);
        assert!(panel.list.preview_open());
        panel.update(NotationMessage::Event(NotationEvent::Middle(None)), &f.app);
        assert!(!panel.list.preview_open());
    }

    #[test]
    fn opening_the_menu_scrolling_or_escape_close_the_preview() {
        let f = fixture();
        let mut panel = panel_for(&f.app);
        let open = |panel: &mut NotationPanel| {
            panel.update(
                NotationMessage::Event(NotationEvent::Middle(Some((f.e5, Point::ORIGIN)))),
                &f.app,
            );
        };
        open(&mut panel);
        panel.update(NotationMessage::Event(NotationEvent::Scrolled), &f.app);
        assert!(!panel.list.preview_open());
        open(&mut panel);
        panel.update(
            NotationMessage::Event(NotationEvent::Menu {
                hit: None,
                position: Point::ORIGIN,
            }),
            &f.app,
        );
        assert!(!panel.list.preview_open());
        assert!(panel.list.menu.is_some());
        assert!(panel.list.close_transients());
        assert!(!panel.list.close_transients(), "nothing left to close");
    }

    #[test]
    fn undo_drops_the_node_ids_kept_by_the_list() {
        let mut f = fixture();
        let mut panel = panel_for(&f.app);
        send(
            &mut f.app,
            vec![AppMessage::Game(GameMessage::SetComment {
                node: f.e4,
                text: "x".to_owned(),
            })],
        );
        resync(&mut panel, &f.app);
        panel.update(
            NotationMessage::Event(NotationEvent::Menu {
                hit: Some(Hit::Move(f.e5)),
                position: Point::ORIGIN,
            }),
            &f.app,
        );
        hover_over(&mut panel, &f.app, f.e5);
        send(&mut f.app, vec![AppMessage::Game(GameMessage::Undo)]);
        resync(&mut panel, &f.app);
        assert!(panel.list.menu.is_none());
        assert_eq!(panel.list.hovered(), None);
    }

    #[test]
    fn an_option_change_is_a_notation_preference() {
        let mut f = fixture();
        let mut panel = panel_for(&f.app);
        let out = panel.update(
            NotationMessage::Option(OptionChange::Layout(NotationLayout::Inline)),
            &f.app,
        );
        send(&mut f.app, out.messages);
        assert_eq!(f.app.notation_options().layout, NotationLayout::Inline);
        assert_eq!(f.app.notation().blocks.len(), 1, "all in one paragraph");
    }

    #[test]
    fn every_option_changes_exactly_its_own_field() {
        let base = NotationOptions::default();
        let cases = [
            OptionChange::Layout(NotationLayout::Indented),
            OptionChange::Figurines(false),
            OptionChange::TimeColumn(false),
            OptionChange::IndentComments(false),
            OptionChange::SymbolicNags(false),
            OptionChange::SpaceAfterNumber(true),
            OptionChange::Header(HeaderMode::Short),
            OptionChange::StripMarks(true),
            OptionChange::BoldMainLine(true),
            OptionChange::ShowComments(false),
            OptionChange::ShowVariations(false),
            OptionChange::ShowResult(false),
            OptionChange::HoverPreview(false),
        ];
        for change in cases {
            let changed = change.applied(base);
            assert_ne!(changed, base, "{change:?}");
            // Undoing it by hand gives the base back.
            let again = match change {
                OptionChange::Layout(_) => OptionChange::Layout(NotationLayout::Table),
                OptionChange::Figurines(_) => OptionChange::Figurines(true),
                OptionChange::TimeColumn(_) => OptionChange::TimeColumn(true),
                OptionChange::IndentComments(_) => OptionChange::IndentComments(true),
                OptionChange::SymbolicNags(_) => OptionChange::SymbolicNags(true),
                OptionChange::SpaceAfterNumber(_) => OptionChange::SpaceAfterNumber(false),
                OptionChange::Header(_) => OptionChange::Header(HeaderMode::Hidden),
                OptionChange::StripMarks(_) => OptionChange::StripMarks(false),
                OptionChange::BoldMainLine(_) => OptionChange::BoldMainLine(false),
                OptionChange::ShowComments(_) => OptionChange::ShowComments(true),
                OptionChange::ShowVariations(_) => OptionChange::ShowVariations(true),
                OptionChange::ShowResult(_) => OptionChange::ShowResult(true),
                OptionChange::HoverPreview(_) => OptionChange::HoverPreview(true),
            };
            assert_eq!(again.applied(changed), base, "{change:?}");
        }
    }

    #[test]
    fn clicking_a_comment_after_a_move_goes_there_and_opens_the_editor() {
        let mut f = fixture();
        let mut panel = panel_for(&f.app);
        let out = panel.update(
            NotationMessage::Event(NotationEvent::CommentClick(CommentTarget::After(f.e5))),
            &f.app,
        );
        assert!(out.focus_editor);
        send(&mut f.app, out.messages);
        assert_eq!(f.app.session.cursor(), f.e5);
        resync(&mut panel, &f.app);
        assert_eq!(panel.editor.target(), CommentTarget::After(f.e5));
    }

    #[test]
    fn clicking_a_comment_before_a_variation_keeps_the_cursor() {
        let mut f = fixture();
        let mut panel = panel_for(&f.app);
        let target = CommentTarget::Before(f.nc3);
        let out = panel.update(
            NotationMessage::Event(NotationEvent::CommentClick(target)),
            &f.app,
        );
        send(&mut f.app, out.messages);
        resync(&mut panel, &f.app);
        assert_eq!(f.app.session.cursor(), f.app.session.game().root());
        assert_eq!(panel.editor.target(), target);
    }

    #[test]
    fn the_editor_opens_with_its_panel_and_saves_on_the_way_out() {
        let mut f = fixture();
        let mut panel = panel_for(&f.app);
        let out = panel.show_editor(&f.app);
        assert!(out.focus_editor);
        // Type something and hide the editor: the draft is saved.
        panel.editor.update(
            EditorMessage::Action(iced::widget::text_editor::Action::Edit(
                iced::widget::text_editor::Edit::Insert('x'),
            )),
            &f.app,
        );
        let out = panel.hide_editor(&f.app);
        send(&mut f.app, out.messages);
        let root_comment = f.app.session.game().initial_comment();
        assert_eq!(root_comment, Some("x"));
    }

    #[test]
    fn the_window_overlay_is_absent_without_a_menu_or_preview() {
        let shell = Shell::new(None);
        assert!(overlay(&shell).is_none());
    }

    #[test]
    fn the_context_menu_and_the_preview_become_window_layers() {
        let mut shell = Shell::new(None);
        shell.ui.notation.list.menu = Some(ContextMenu {
            hit: None,
            node: None,
            position: Point::new(100.0, 100.0),
        });
        assert!(overlay(&shell).is_some());
        shell.ui.notation.list.menu = None;
        let root = shell.app.session.game().root();
        let app = &shell.app;
        shell
            .ui
            .notation
            .list
            .open_preview(app, root, Point::ORIGIN);
        assert!(shell.ui.notation.list.preview_open());
        assert!(overlay(&shell).is_some());
    }

    #[test]
    fn menu_labels_have_text_in_both_languages() {
        let labels = [
            MenuLabel::GoHere,
            MenuLabel::Promote,
            MenuLabel::MakeFirst,
            MenuLabel::DeleteVariation,
            MenuLabel::TruncateFrom,
            MenuLabel::TruncateBefore,
            MenuLabel::StripComments,
            MenuLabel::StripVariations,
            MenuLabel::EditComment,
            MenuLabel::DeleteComment,
        ];
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for label in labels {
                assert!(i18n.has(label.key()), "{} in {language:?}", label.key());
            }
        }
    }

    #[test]
    fn toggling_a_card_folds_it_and_toggling_again_unfolds_it() {
        let f = fixture();
        let mut panel = panel_for(&f.app);
        assert!(panel.list.collapsed.is_empty());
        let fold = NotationMessage::Event(NotationEvent::ToggleCard(f.nc3));
        let out = panel.update(fold.clone(), &f.app);
        assert!(out.messages.is_empty(), "folding is only visual");
        assert!(panel.list.collapsed.contains(&f.nc3));
        panel.update(fold, &f.app);
        assert!(panel.list.collapsed.is_empty());
    }

    #[test]
    fn folding_a_card_closes_the_menu_and_the_preview() {
        let f = fixture();
        let mut panel = panel_for(&f.app);
        panel.update(
            NotationMessage::Event(NotationEvent::Menu {
                hit: Some(Hit::Move(f.e5)),
                position: Point::new(10.0, 10.0),
            }),
            &f.app,
        );
        assert!(panel.list.menu.is_some());
        panel.update(
            NotationMessage::Event(NotationEvent::ToggleCard(f.nc3)),
            &f.app,
        );
        assert!(panel.list.menu.is_none());
    }

    #[test]
    fn folded_cards_are_forgotten_when_the_game_changes() {
        let mut f = fixture();
        let mut panel = panel_for(&f.app);
        panel.update(
            NotationMessage::Event(NotationEvent::ToggleCard(f.nc3)),
            &f.app,
        );
        // Moving the cursor is not a change of the game.
        send(&mut f.app, go_to(f.e5));
        resync(&mut panel, &f.app);
        assert!(panel.list.collapsed.contains(&f.nc3));
        // Editing is.
        send(
            &mut f.app,
            vec![AppMessage::Game(GameMessage::SetComment {
                node: f.e4,
                text: "edited".to_owned(),
            })],
        );
        resync(&mut panel, &f.app);
        assert!(panel.list.collapsed.is_empty());
    }

    #[test]
    fn the_cursor_entering_a_folded_card_unfolds_it() {
        let mut f = fixture();
        let mut panel = panel_for(&f.app);
        panel.update(
            NotationMessage::Event(NotationEvent::ToggleCard(f.nc3)),
            &f.app,
        );
        send(&mut f.app, go_to(f.nf6));
        resync(&mut panel, &f.app);
        assert!(panel.list.collapsed.is_empty(), "nf6 is inside the card");
    }

    #[test]
    fn the_table_is_the_default_layout_and_the_options_reach_the_preferences() {
        let mut f = fixture();
        assert_eq!(f.app.notation_options().layout, NotationLayout::Table);
        assert!(f.app.notation_options().figurines);
        let mut panel = panel_for(&f.app);
        for change in [
            OptionChange::Figurines(false),
            OptionChange::TimeColumn(false),
        ] {
            let out = panel.update(NotationMessage::Option(change), &f.app);
            send(&mut f.app, out.messages);
        }
        assert!(!f.app.notation_options().figurines);
        assert!(!f.app.notation_options().show_time_column);
    }
}
