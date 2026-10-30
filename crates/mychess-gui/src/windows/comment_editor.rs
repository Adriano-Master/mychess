//! The comment editor under the move list (spec M2-4, D177-D181).
//!
//! It edits one [`CommentTarget`] (after a move, before the first move of a
//! variation, or the comment of the game) and the annotations of the cursor
//! move. The text is a draft: it reaches the game when the editor loses the
//! focus, when the cursor is about to move or the game to change (the shell
//! asks [`CommentEditorUi::take_commit`] first), with `Ctrl+Enter` (`Cmd` on
//! macOS) or with the Save button. `Esc` and Cancel throw the draft away.
//!
//! Node ids kept here are dropped when the game changes under them by
//! undo, redo, leaving trial mode or another file (architecture 2.2.1): the
//! editor tells its own edits from the others by the change counter of the
//! session.

use fluent_bundle::FluentArgs;
use iced::keyboard::{self, key::Named};
use iced::widget::text_editor::{Binding, Edit};
use iced::widget::{button, column, container, row, space, text, text_editor, text_input};
use iced::{Element, Length};
use mychess_app::{
    AppNotice, AppState, CommentTarget, GameMessage, Message as AppMessage, nag_field_text,
    parse_nag_field,
};
use mychess_core::{Game, Nag, NodeId};

use super::notation::{NotationMessage, Outcome};
use crate::i18n::I18n;
use crate::keymap::{Platform, binding_label};
use crate::shell::{Message, Shell};
use crate::theme;
use crate::widgets::nag_palette::nag_palette;

/// Id of the text area (focus requests and focus queries).
pub const EDITOR_ID: &str = "comment-editor";

/// Id of the field with the annotations.
pub const NAG_FIELD_ID: &str = "comment-nags";

/// The longest comment the editor accepts, in bytes (spec rule 29: the limit
/// of the PGN reader for a comment).
pub const MAX_COMMENT_BYTES: usize = 1 << 20;

/// Height of the text area in logical pixels.
const TEXT_HEIGHT: f32 = 90.0;

/// A message of the editor.
#[derive(Debug, Clone)]
pub enum EditorMessage {
    /// The text area was used.
    Action(text_editor::Action),
    /// The annotation field was edited.
    NagEdited(String),
    /// Save now.
    Commit,
    /// Throw the draft away.
    Cancel,
    /// Empty the text.
    Clear,
    /// Toggle an annotation of the cursor move.
    Toggle(Nag),
    /// The answer to "does the text area still have the focus?".
    FocusChecked(bool),
}

/// The draft of the comment editor.
#[derive(Debug, Default)]
pub struct CommentEditorUi {
    target: Option<CommentTarget>,
    content: text_editor::Content,
    dirty: bool,
    nag_draft: String,
    nag_dirty: bool,
    nag_error: Option<String>,
    last_cursor: Option<NodeId>,
    seen_changes: Option<u64>,
    /// The change counter the editor's own messages lead to.
    expected: Option<u64>,
    /// An annotation field with an unknown token was dropped unsaved.
    dropped_nag_draft: bool,
}

/// The comment of the cursor: after its move, or the game's at the start.
pub fn default_target(game: &Game, cursor: NodeId) -> CommentTarget {
    if cursor == game.root() {
        CommentTarget::Initial
    } else {
        CommentTarget::After(cursor)
    }
}

/// The text a comment has in the game.
pub fn target_text(game: &Game, target: CommentTarget) -> String {
    let text = match target {
        CommentTarget::After(node) => game.comment(node).ok().flatten(),
        CommentTarget::Before(node) => game.comment_before(node).ok().flatten(),
        CommentTarget::Initial => game.initial_comment(),
    };
    text.unwrap_or_default().to_owned()
}

fn target_exists(game: &Game, target: CommentTarget) -> bool {
    match target {
        CommentTarget::After(node) | CommentTarget::Before(node) => game.contains(node),
        CommentTarget::Initial => true,
    }
}

/// The message that sets a comment.
pub fn set_comment_message(game: &Game, target: CommentTarget, text: String) -> AppMessage {
    AppMessage::Game(match target {
        CommentTarget::After(node) => GameMessage::SetComment { node, text },
        CommentTarget::Before(node) => GameMessage::SetCommentBefore { node, text },
        CommentTarget::Initial => GameMessage::SetComment {
            node: game.root(),
            text,
        },
    })
}

/// The same text for comparing: `\r\n` as `\n`, nothing for blanks.
fn normalized(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    if text.trim().is_empty() {
        String::new()
    } else {
        text
    }
}

impl CommentEditorUi {
    /// The comment being edited.
    pub fn target(&self) -> CommentTarget {
        self.target.unwrap_or(CommentTarget::Initial)
    }

    /// Whether there is unsaved work.
    pub fn is_dirty(&self) -> bool {
        self.dirty || self.nag_dirty
    }

    /// The text of the draft.
    pub fn text(&self) -> String {
        self.content.text()
    }

    /// The annotation field as typed.
    pub fn nag_text(&self) -> &str {
        &self.nag_draft
    }

    /// Whether an annotation field that could not be saved (it held an
    /// unknown token) was dropped since the last call; the shell tells the
    /// user instead of losing it in silence.
    pub fn take_dropped_nag_draft(&mut self) -> bool {
        std::mem::take(&mut self.dropped_nag_draft)
    }

    /// The token of the annotation field that is not an annotation.
    pub fn nag_error(&self) -> Option<&str> {
        self.nag_error.as_deref()
    }

    /// Edits `target`: loads its text.
    pub fn open(&mut self, app: &AppState, target: CommentTarget) {
        self.target = Some(target);
        self.load(app);
    }

    fn load(&mut self, app: &AppState) {
        self.load_text(app);
        self.load_nags(app);
    }

    /// Takes the comment of the target from the game.
    fn load_text(&mut self, app: &AppState) {
        let game = app.session.game();
        let target = self
            .target
            .unwrap_or_else(|| default_target(game, app.session.cursor()));
        self.target = Some(target);
        self.content = text_editor::Content::with_text(&target_text(game, target));
        self.dirty = false;
    }

    /// Takes the annotations of the cursor move from the game.
    fn load_nags(&mut self, app: &AppState) {
        if self.nag_dirty && self.nag_error.is_some() {
            self.dropped_nag_draft = true;
        }
        self.nag_draft = app
            .session
            .game()
            .nags(app.session.cursor())
            .map(nag_field_text)
            .unwrap_or_default();
        self.nag_dirty = false;
        self.nag_error = None;
    }

    /// Follows the application: another cursor edits another comment, and a
    /// game that changed under the editor invalidates what it holds.
    pub fn sync(&mut self, app: &AppState) {
        let session = &app.session;
        let game = session.game();
        let cursor = session.cursor();
        let changes = session.changes();
        let mut text = self.target.is_none();
        let mut nags = text;
        let mut own_edit = false;
        if self.seen_changes != Some(changes) {
            let own = self.expected == Some(changes);
            self.seen_changes = Some(changes);
            self.expected = None;
            text = true;
            if own {
                // Show what the game kept (it may have trimmed the text),
                // but leave the text area alone when it already shows it:
                // reloading would throw the text cursor to the start.
                // An annotation field that was refused stays as typed.
                own_edit = true;
                nags |= !self.nag_dirty;
            } else {
                // Undo, redo, trial mode, another game or an edit from
                // elsewhere: the ids and the draft may be stale.
                self.target = None;
                nags = true;
            }
        }
        if self.last_cursor != Some(cursor) {
            self.last_cursor = Some(cursor);
            self.target = None;
            text = true;
            nags = true;
        }
        if let Some(target) = self.target
            && !target_exists(game, target)
        {
            self.target = None;
            text = true;
            nags = true;
        }
        if text || nags {
            if self.target.is_none() {
                self.target = Some(default_target(game, cursor));
            }
            if text {
                let shown = own_edit
                    && self.target.is_some_and(|target| {
                        normalized(&self.content.text()) == normalized(&target_text(game, target))
                    });
                if shown {
                    self.dirty = false;
                } else {
                    self.load_text(app);
                }
            }
            if nags {
                self.load_nags(app);
            }
        }
    }

    /// Whether the annotations can be edited: the cursor is on a move and
    /// the editor is on the comment after it.
    pub fn nags_enabled(&self, app: &AppState) -> bool {
        let cursor = app.session.cursor();
        cursor != app.session.game().root() && self.target == Some(CommentTarget::After(cursor))
    }

    /// The messages that save the draft (nothing when it equals the game),
    /// and forget it. An annotation field with an unknown token blocks only
    /// the annotations and shows the error.
    pub fn take_commit(&mut self, app: &AppState) -> Vec<AppMessage> {
        let session = &app.session;
        let game = session.game();
        let mut messages = Vec::new();
        if self.dirty {
            self.dirty = false;
            let target = self.target();
            let text = normalized(&self.content.text());
            if target_exists(game, target) && text != normalized(&target_text(game, target)) {
                messages.push(set_comment_message(game, target, text));
            }
        }
        if self.nag_dirty {
            match parse_nag_field(&self.nag_draft) {
                Ok(nags) => {
                    self.nag_dirty = false;
                    self.nag_error = None;
                    let current = game.nags(session.cursor()).map(<[Nag]>::to_vec);
                    if self.nags_enabled(app) && current.as_deref() != Ok(nags.as_slice()) {
                        messages.push(AppMessage::Game(GameMessage::SetNags(nags)));
                    }
                }
                Err(error) => self.nag_error = Some(error.token),
            }
        }
        if !messages.is_empty() {
            let n = u64::try_from(messages.len()).unwrap_or(u64::MAX);
            self.expected = Some(session.changes().saturating_add(n));
        }
        messages
    }

    /// Whether `action` would take the text over [`MAX_COMMENT_BYTES`].
    fn would_overflow(&self, action: &text_editor::Action) -> bool {
        let text_editor::Action::Edit(edit) = action else {
            return false;
        };
        let added = match edit {
            Edit::Insert(c) => c.len_utf8(),
            Edit::Paste(text) => text.len(),
            Edit::Enter => 1,
            _ => return false,
        };
        self.content.text().len().saturating_add(added) > MAX_COMMENT_BYTES
    }

    /// Handles a message of the editor.
    pub fn update(&mut self, message: EditorMessage, app: &AppState) -> Outcome {
        match message {
            EditorMessage::Action(action) => {
                if self.would_overflow(&action) {
                    return Outcome {
                        notice: Some(AppNotice::CommentTooLong),
                        ..Outcome::default()
                    };
                }
                if action.is_edit() {
                    self.dirty = true;
                }
                self.content.perform(action);
                Outcome::default()
            }
            EditorMessage::NagEdited(text) => {
                self.nag_draft = text;
                self.nag_dirty = true;
                self.nag_error = None;
                Outcome::default()
            }
            EditorMessage::Commit => Outcome {
                messages: self.take_commit(app),
                ..Outcome::default()
            },
            EditorMessage::Cancel => {
                self.dirty = false;
                self.nag_dirty = false;
                self.nag_error = None;
                self.load(app);
                Outcome::default()
            }
            EditorMessage::Clear => {
                self.content = text_editor::Content::with_text("");
                self.dirty = true;
                // The click took the focus from the text area; give it back
                // so the emptied draft is not saved by the blur.
                Outcome {
                    focus_editor: true,
                    ..Outcome::default()
                }
            }
            EditorMessage::Toggle(nag) => {
                let mut messages = self.take_commit(app);
                messages.push(AppMessage::Game(GameMessage::ToggleNag(nag)));
                Outcome {
                    messages,
                    ..Outcome::default()
                }
            }
            EditorMessage::FocusChecked(focused) => {
                if focused || !self.is_dirty() {
                    Outcome::default()
                } else {
                    Outcome {
                        messages: self.take_commit(app),
                        ..Outcome::default()
                    }
                }
            }
        }
    }
}

/// `12.Nf3` / `12...Nf6`: the move of `node` with its number.
pub fn move_label(game: &Game, node: NodeId) -> Option<String> {
    let parent = game.parent(node).ok()??;
    let before = game.position_at(parent).ok()?;
    let mv = game.mv(node).ok()?;
    let san = if mv.is_null() {
        "--".to_owned()
    } else {
        before.san(mv).ok()?
    };
    let number = before.fullmove_number();
    Some(match before.turn() {
        mychess_core::Color::White => format!("{number}.{san}"),
        mychess_core::Color::Black => format!("{number}...{san}"),
    })
}

fn target_title(game: &Game, target: CommentTarget, i18n: &I18n) -> String {
    let with_move = |key: &str, node: NodeId| {
        let mut args = FluentArgs::new();
        let label = move_label(game, node).unwrap_or_else(|| i18n.tr("comment-no-move"));
        args.set("move", label);
        i18n.tr_args(key, &args)
    };
    match target {
        CommentTarget::After(node) => with_move("comment-target-after", node),
        CommentTarget::Before(node) => with_move("comment-target-before", node),
        CommentTarget::Initial => i18n.tr("comment-target-initial"),
    }
}

fn editor_message(message: EditorMessage) -> Message {
    Message::Notation(NotationMessage::Editor(message))
}

/// The key bindings of the text area: `Cmd+Enter` saves, `Esc` discards and
/// gives up the focus; everything else is the toolkit's.
fn key_binding(press: text_editor::KeyPress) -> Option<Binding<Message>> {
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return None;
    }
    match press.key.as_ref() {
        keyboard::Key::Named(Named::Enter) if press.modifiers.command() => {
            Some(Binding::Custom(editor_message(EditorMessage::Commit)))
        }
        keyboard::Key::Named(Named::Escape) => Some(Binding::Sequence(vec![
            Binding::Custom(editor_message(EditorMessage::Cancel)),
            Binding::Unfocus,
        ])),
        _ => Binding::from_key_press(press),
    }
}

/// The editor panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let app = &shell.app;
    let i18n = &shell.i18n;
    let editor = &shell.ui.notation.editor;
    let game = app.session.game();
    let enabled = editor.nags_enabled(app);
    let active = game
        .nags(app.session.cursor())
        .map(<[Nag]>::to_vec)
        .unwrap_or_default();

    let title = text(target_title(game, editor.target(), i18n)).size(14);
    let area = text_editor(&editor.content)
        .id(EDITOR_ID)
        .placeholder(i18n.tr("comment-placeholder"))
        .height(Length::Fixed(TEXT_HEIGHT))
        .size(14)
        .on_action(|action| editor_message(EditorMessage::Action(action)))
        .key_binding(key_binding);

    let field = text_input("", editor.nag_text())
        .id(NAG_FIELD_ID)
        .size(14)
        .on_input_maybe(enabled.then_some(|text| editor_message(EditorMessage::NagEdited(text))))
        .on_submit_maybe(enabled.then(|| editor_message(EditorMessage::Commit)));
    let nags = row![text(i18n.tr("comment-nags-field")).size(13), field].spacing(8);

    let mut page = column![title, area, nags].spacing(6);
    if let Some(token) = editor.nag_error() {
        let mut args = FluentArgs::new();
        args.set("token", token.to_owned());
        page = page.push(
            text(i18n.tr_args("nag-invalid-token", &args))
                .size(13)
                .style(text::danger),
        );
    }
    page = page.push(nag_palette(i18n, &active, enabled, |nag| {
        editor_message(EditorMessage::Toggle(nag))
    }));

    let shortcut = save_shortcut_label();
    let mut args = FluentArgs::new();
    args.set("shortcut", shortcut);
    let buttons = row![
        button(text(i18n.tr("comment-save")).size(13))
            .padding([3, 10])
            .on_press(editor_message(EditorMessage::Commit)),
        button(text(i18n.tr("comment-cancel")).size(13))
            .padding([3, 10])
            .style(button::secondary)
            .on_press(editor_message(EditorMessage::Cancel)),
        button(text(i18n.tr("comment-clear")).size(13))
            .padding([3, 10])
            .style(button::secondary)
            .on_press(editor_message(EditorMessage::Clear)),
        space::horizontal(),
        text(i18n.tr_args("comment-save-hint", &args))
            .size(12)
            .style(theme::muted_text),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    page = page.push(buttons);
    container(page).padding(8).width(Length::Fill).into()
}

/// `Ctrl+Enter` (`⌘Enter` on macOS) as the platform writes it.
fn save_shortcut_label() -> String {
    let binding = mychess_app::Binding::new(
        mychess_app::Key::Named(mychess_app::NamedKey::Enter),
        mychess_app::Modifiers::COMMAND,
    );
    binding_label(&binding, Platform::current())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use iced::widget::text_editor::Action;
    use mychess_app::{Nav, update};
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

    /// 1.e4 e5 (1...c5) 2.Nf3 and the nodes.
    fn app() -> (AppState, NodeId, NodeId, NodeId, NodeId) {
        let mut game = Game::new();
        let root = game.root();
        let e4 = play(&mut game, root, "e2e4");
        let e5 = play(&mut game, e4, "e7e5");
        let c5 = play(&mut game, e4, "c7c5");
        let nf3 = play(&mut game, e5, "g1f3");
        let mut app = AppState::default();
        app.session.replace_game(game);
        app.refresh_notation();
        (app, e4, e5, c5, nf3)
    }

    fn go(app: &mut AppState, ed: &mut CommentEditorUi, node: NodeId) {
        update(
            app,
            AppMessage::Game(GameMessage::Navigate(Nav::GoToNode(node))),
        );
        ed.sync(app);
    }

    fn apply(app: &mut AppState, ed: &mut CommentEditorUi, messages: Vec<AppMessage>) {
        for message in messages {
            update(app, message);
        }
        ed.sync(app);
    }

    fn type_text(ed: &mut CommentEditorUi, app: &AppState, text: &str) {
        for c in text.chars() {
            let edit = if c == '\n' {
                Edit::Enter
            } else {
                Edit::Insert(c)
            };
            ed.update(EditorMessage::Action(Action::Edit(edit)), app);
        }
    }

    fn comment(app: &AppState, node: NodeId) -> Option<String> {
        app.session
            .game()
            .comment(node)
            .ok()
            .flatten()
            .map(str::to_owned)
    }

    fn nag(n: u16) -> Nag {
        Nag::new(n).expect("nag")
    }

    #[test]
    fn at_the_start_the_editor_edits_the_comment_of_the_game() {
        let (mut app, ..) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        assert_eq!(ed.target(), CommentTarget::Initial);
        type_text(&mut ed, &app, "A fine game");
        assert!(ed.is_dirty());
        let messages = ed.take_commit(&app);
        assert_eq!(messages.len(), 1);
        apply(&mut app, &mut ed, messages);
        assert_eq!(app.session.game().initial_comment(), Some("A fine game"));
        assert!(!ed.is_dirty());
        assert!(ed.take_commit(&app).is_empty(), "saved once");
    }

    #[test]
    fn the_editor_follows_the_cursor_and_loads_that_comment() {
        let (mut app, e4, e5, ..) = app();
        update(
            &mut app,
            AppMessage::Game(GameMessage::SetComment {
                node: e5,
                text: "reply".to_owned(),
            }),
        );
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        assert_eq!(ed.target(), CommentTarget::After(e4));
        assert_eq!(ed.text(), "");
        go(&mut app, &mut ed, e5);
        assert_eq!(ed.target(), CommentTarget::After(e5));
        assert_eq!(ed.text(), "reply");
    }

    #[test]
    fn the_draft_is_saved_for_the_node_it_was_typed_on_before_the_cursor_moves() {
        let (mut app, e4, e5, ..) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        type_text(&mut ed, &app, "good");
        // What the shell does before a message that moves the cursor.
        let saving = ed.take_commit(&app);
        for m in saving {
            update(&mut app, m);
        }
        go(&mut app, &mut ed, e5);
        assert_eq!(comment(&app, e4).as_deref(), Some("good"));
        assert_eq!(comment(&app, e5), None);
    }

    #[test]
    fn saving_an_unchanged_text_makes_no_undo_point() {
        let (mut app, e4, ..) = app();
        update(
            &mut app,
            AppMessage::Game(GameMessage::SetComment {
                node: e4,
                text: "same".to_owned(),
            }),
        );
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        assert_eq!(ed.text(), "same");
        // Typing a letter and deleting it again leaves the text as it was.
        type_text(&mut ed, &app, "x");
        ed.update(EditorMessage::Action(Action::Edit(Edit::Backspace)), &app);
        assert!(ed.is_dirty());
        assert!(ed.take_commit(&app).is_empty());
        assert!(!ed.is_dirty());
    }

    #[test]
    fn a_blank_text_removes_the_comment() {
        let (mut app, e4, ..) = app();
        update(
            &mut app,
            AppMessage::Game(GameMessage::SetComment {
                node: e4,
                text: "temp".to_owned(),
            }),
        );
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        ed.update(EditorMessage::Clear, &app);
        let out = ed.update(EditorMessage::Commit, &app);
        apply(&mut app, &mut ed, out.messages);
        assert_eq!(comment(&app, e4), None);
    }

    #[test]
    fn saving_keeps_the_text_cursor_where_it_was() {
        let (mut app, e4, ..) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        type_text(&mut ed, &app, "abc\ndef");
        let before = ed.content.cursor();
        let out = ed.update(EditorMessage::Commit, &app);
        apply(&mut app, &mut ed, out.messages);
        assert_eq!(comment(&app, e4).as_deref(), Some("abc\ndef"));
        assert_eq!(ed.content.cursor(), before, "not thrown to the start");
        assert!(!ed.is_dirty());
    }

    #[test]
    fn clear_asks_for_the_focus_back() {
        let (app, ..) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        assert!(ed.update(EditorMessage::Clear, &app).focus_editor);
    }

    #[test]
    fn text_over_one_mebibyte_is_refused_with_a_notice() {
        let (app, e4, ..) = app();
        let mut app = app;
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        let paste = |len: usize| {
            EditorMessage::Action(Action::Edit(Edit::Paste(std::sync::Arc::new(
                "x".repeat(len),
            ))))
        };
        let out = ed.update(paste(MAX_COMMENT_BYTES + 1), &app);
        assert_eq!(out.notice, Some(AppNotice::CommentTooLong));
        assert_eq!(ed.text(), "", "nothing was taken");
        assert!(!ed.is_dirty());
        let out = ed.update(paste(MAX_COMMENT_BYTES - 10), &app);
        assert_eq!(out.notice, None);
        assert!(ed.is_dirty());
        // A few more characters still fit; ten more do not.
        let more = ed.update(paste(5), &app);
        assert_eq!(more.notice, None);
        let over = ed.update(paste(MAX_COMMENT_BYTES), &app);
        assert_eq!(over.notice, Some(AppNotice::CommentTooLong));
    }

    #[test]
    fn line_breaks_are_compared_without_the_carriage_return() {
        assert_eq!(normalized("a\r\nb"), "a\nb");
        assert_eq!(normalized("  \r\n "), "");
        assert_eq!(normalized("a"), "a");
    }

    #[test]
    fn a_comment_before_a_variation_is_edited_without_moving_the_cursor() {
        let (mut app, _e4, _e5, c5, _) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        ed.open(&app, CommentTarget::Before(c5));
        type_text(&mut ed, &app, "sharp");
        let out = ed.update(EditorMessage::Commit, &app);
        apply(&mut app, &mut ed, out.messages);
        assert_eq!(
            app.session.game().comment_before(c5).ok().flatten(),
            Some("sharp")
        );
        assert_eq!(app.session.cursor(), app.session.game().root());
        assert_eq!(
            ed.target(),
            CommentTarget::Before(c5),
            "its own edit does not change the target"
        );
        assert_eq!(ed.text(), "sharp");
    }

    #[test]
    fn undo_drops_the_target_and_the_draft() {
        let (mut app, e4, _e5, c5, _) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        // An edit from elsewhere (the board draws a mark, say), then the
        // editor is pointed at a comment before a variation and typed in.
        update(
            &mut app,
            AppMessage::Game(GameMessage::SetComment {
                node: e4,
                text: "one".to_owned(),
            }),
        );
        ed.sync(&app);
        ed.open(&app, CommentTarget::Before(c5));
        type_text(&mut ed, &app, "draft");
        update(&mut app, AppMessage::Game(GameMessage::Undo));
        ed.sync(&app);
        assert_eq!(ed.target(), CommentTarget::After(e4), "back to the cursor");
        assert_eq!(ed.text(), "", "the undone comment, not the draft");
        assert!(!ed.is_dirty());
    }

    #[test]
    fn a_target_whose_node_is_gone_falls_back_to_the_cursor() {
        let (mut app, e4, _e5, c5, _) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        ed.open(&app, CommentTarget::Before(c5));
        update(
            &mut app,
            AppMessage::Game(GameMessage::DeleteVariation(
                mychess_app::VariationTarget::Node(c5),
            )),
        );
        ed.sync(&app);
        assert_eq!(ed.target(), CommentTarget::After(e4));
    }

    #[test]
    fn another_game_resets_the_editor() {
        let (mut app, e4, ..) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        type_text(&mut ed, &app, "lost");
        app.session.replace_game(Game::new());
        ed.sync(&app);
        assert_eq!(ed.target(), CommentTarget::Initial);
        assert!(!ed.is_dirty());
        assert_eq!(ed.text(), "");
    }

    #[test]
    fn cancel_throws_the_draft_away() {
        let (mut app, e4, ..) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        type_text(&mut ed, &app, "nope");
        ed.update(EditorMessage::NagEdited("!".to_owned()), &app);
        let out = ed.update(EditorMessage::Cancel, &app);
        assert!(out.messages.is_empty());
        assert!(!ed.is_dirty());
        assert_eq!(ed.text(), "");
        assert_eq!(ed.nag_text(), "");
        assert!(ed.take_commit(&app).is_empty());
    }

    #[test]
    fn the_annotation_field_sets_the_annotations_of_the_move() {
        let (mut app, e4, ..) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        assert!(ed.nags_enabled(&app));
        ed.update(EditorMessage::NagEdited("! +- $146".to_owned()), &app);
        let out = ed.update(EditorMessage::Commit, &app);
        apply(&mut app, &mut ed, out.messages);
        let nags = app.session.game().nags(e4).expect("nags").to_vec();
        assert_eq!(nags, [nag(1), nag(18), nag(146)]);
        assert_eq!(
            ed.nag_text(),
            "! +- N",
            "the field is rewritten in its canonical form"
        );
        assert_eq!(ed.nag_error(), None);
    }

    #[test]
    fn an_unknown_annotation_blocks_saving_the_annotations_and_shows_the_token() {
        let (mut app, e4, ..) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        type_text(&mut ed, &app, "text still saved");
        ed.update(EditorMessage::NagEdited("! banana".to_owned()), &app);
        let out = ed.update(EditorMessage::Commit, &app);
        assert_eq!(out.messages.len(), 1, "only the comment");
        apply(&mut app, &mut ed, out.messages);
        assert_eq!(ed.nag_error(), Some("banana"));
        assert!(app.session.game().nags(e4).expect("nags").is_empty());
        assert!(ed.is_dirty(), "the field still holds unsaved work");
        // Fixing it clears the error.
        ed.update(EditorMessage::NagEdited("!".to_owned()), &app);
        assert_eq!(ed.nag_error(), None);
        let out = ed.update(EditorMessage::Commit, &app);
        apply(&mut app, &mut ed, out.messages);
        assert_eq!(app.session.game().nags(e4).expect("nags"), [nag(1)]);
    }

    #[test]
    fn an_invalid_annotation_draft_is_reported_when_the_cursor_moves_away() {
        let (mut app, e4, e5, ..) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        ed.update(EditorMessage::NagEdited("! banana".to_owned()), &app);
        assert!(ed.take_commit(&app).is_empty());
        assert!(!ed.take_dropped_nag_draft(), "still on screen");
        go(&mut app, &mut ed, e5);
        assert!(ed.take_dropped_nag_draft(), "dropped with a notice");
        assert!(!ed.take_dropped_nag_draft(), "reported once");
        assert_eq!(ed.nag_text(), "");
        // A cancelled draft is not a loss to report.
        ed.update(EditorMessage::NagEdited("! banana".to_owned()), &app);
        drop(ed.take_commit(&app));
        ed.update(EditorMessage::Cancel, &app);
        assert!(!ed.take_dropped_nag_draft());
    }

    #[test]
    fn the_palette_toggles_an_annotation_and_replaces_one_of_its_group() {
        let (mut app, e4, ..) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        let toggle = |n, ed: &mut CommentEditorUi, app: &mut AppState| {
            let out = ed.update(EditorMessage::Toggle(nag(n)), app);
            apply(app, ed, out.messages);
        };
        toggle(1, &mut ed, &mut app);
        assert_eq!(app.session.game().nags(e4).expect("nags"), [nag(1)]);
        assert_eq!(ed.nag_text(), "!", "the field follows");
        toggle(2, &mut ed, &mut app);
        assert_eq!(
            app.session.game().nags(e4).expect("nags"),
            [nag(2)],
            "exclusive group"
        );
        toggle(2, &mut ed, &mut app);
        assert!(app.session.game().nags(e4).expect("nags").is_empty());
    }

    #[test]
    fn annotations_are_disabled_at_the_start_and_for_other_targets() {
        let (app, e4, _e5, c5, _) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        assert!(!ed.nags_enabled(&app), "at the start");
        let mut app = app;
        go(&mut app, &mut ed, e4);
        assert!(ed.nags_enabled(&app));
        ed.open(&app, CommentTarget::Before(c5));
        assert!(!ed.nags_enabled(&app), "the comment of another node");
        // A typed field cannot reach the game then.
        ed.update(EditorMessage::NagEdited("!".to_owned()), &app);
        assert!(ed.take_commit(&app).is_empty());
    }

    #[test]
    fn losing_the_focus_saves_and_keeping_it_does_not() {
        let (mut app, e4, ..) = app();
        let mut ed = CommentEditorUi::default();
        ed.sync(&app);
        go(&mut app, &mut ed, e4);
        type_text(&mut ed, &app, "blur");
        assert!(
            ed.update(EditorMessage::FocusChecked(true), &app)
                .messages
                .is_empty()
        );
        assert!(ed.is_dirty());
        let out = ed.update(EditorMessage::FocusChecked(false), &app);
        apply(&mut app, &mut ed, out.messages);
        assert_eq!(comment(&app, e4).as_deref(), Some("blur"));
        // Nothing to save: nothing sent.
        assert!(
            ed.update(EditorMessage::FocusChecked(false), &app)
                .messages
                .is_empty()
        );
    }

    fn press(key: keyboard::Key, modifiers: keyboard::Modifiers) -> text_editor::KeyPress {
        text_editor::KeyPress {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            modifiers,
            text: None,
            status: text_editor::Status::Focused { is_hovered: false },
        }
    }

    #[test]
    fn command_enter_saves_and_escape_discards_and_leaves() {
        let enter = keyboard::Key::Named(Named::Enter);
        let saved = key_binding(press(enter.clone(), keyboard::Modifiers::COMMAND));
        assert!(matches!(
            saved,
            Some(Binding::Custom(Message::Notation(NotationMessage::Editor(
                EditorMessage::Commit
            ))))
        ));
        // A plain Enter is a line break, as the toolkit has it.
        assert!(matches!(
            key_binding(press(enter, keyboard::Modifiers::empty())),
            Some(Binding::Enter)
        ));
        let escape = key_binding(press(
            keyboard::Key::Named(Named::Escape),
            keyboard::Modifiers::empty(),
        ));
        let Some(Binding::Sequence(steps)) = escape else {
            panic!("escape is a sequence");
        };
        assert!(matches!(
            steps.as_slice(),
            [
                Binding::Custom(Message::Notation(NotationMessage::Editor(
                    EditorMessage::Cancel
                ))),
                Binding::Unfocus
            ]
        ));
    }

    #[test]
    fn move_labels_carry_the_number_and_the_dots_of_black() {
        let (app, e4, e5, c5, nf3) = app();
        let game = app.session.game();
        assert_eq!(move_label(game, e4).as_deref(), Some("1.e4"));
        assert_eq!(move_label(game, e5).as_deref(), Some("1...e5"));
        assert_eq!(move_label(game, c5).as_deref(), Some("1...c5"));
        assert_eq!(move_label(game, nf3).as_deref(), Some("2.Nf3"));
        assert_eq!(move_label(game, game.root()), None);
    }

    #[test]
    fn the_titles_name_the_comment_in_both_languages() {
        let (app, e4, _e5, c5, _) = app();
        let game = app.session.game();
        let en = I18n::new(Language::EnUs);
        let pt = I18n::new(Language::PtBr);
        assert_eq!(
            target_title(game, CommentTarget::After(e4), &en),
            "Comment after 1.e4"
        );
        assert_eq!(
            target_title(game, CommentTarget::Before(c5), &pt),
            "Comentário antes de 1...c5"
        );
        assert_eq!(
            target_title(game, CommentTarget::Initial, &pt),
            "Comentário no início da partida"
        );
    }

    #[test]
    fn the_shortcut_label_uses_the_platform_command_key() {
        let label = save_shortcut_label();
        if cfg!(target_os = "macos") {
            assert!(label.contains('⌘'), "{label}");
        } else {
            assert!(label.starts_with("Ctrl+"), "{label}");
        }
    }
}
