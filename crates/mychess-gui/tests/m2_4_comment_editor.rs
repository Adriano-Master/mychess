//! M2-4 comment editor, NAG field, palette and NAG shortcuts (spec D177-D182,
//! rules 23-29, acceptance criterion "Editor de comentários").

#![allow(
    clippy::unwrap_used,
    clippy::assert_is_empty,
    clippy::float_cmp,
    clippy::unnecessary_wraps
)]

mod m2_4_common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::keyboard::{self, Key, key::Named};
use iced::widget::text_editor::{Action, Edit};
use iced::{Event, widget};
use m2_4_common::{Rig, ch, has, messages, sample};
use mychess_app::{
    CommentTarget, GameMessage, Message as AppMessage, NAG_SEQUENCE_TIMEOUT, NamedKey, Nav,
};
use mychess_core::{Game, Nag, NodeId};
use mychess_gui::Message;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::widgets::nag_palette::{PALETTE_NAGS, palette_nags, tip_key};
use mychess_gui::windows::comment_editor::{
    self, EDITOR_ID, EditorMessage, NAG_FIELD_ID, default_target, move_label, target_text,
};
use mychess_gui::windows::dialog_ui::{DialogMessage, TypedKey};
use mychess_gui::windows::notation::NotationMessage;

fn nag(n: u16) -> Nag {
    Nag::new(n).expect("valid NAG")
}

fn editor(m: EditorMessage) -> Message {
    Message::Notation(NotationMessage::Editor(m))
}

fn type_text(rig: &mut Rig, text: &str) {
    for c in text.chars() {
        let edit = if c == '\n' {
            Edit::Enter
        } else {
            Edit::Insert(c)
        };
        rig.msg(editor(EditorMessage::Action(Action::Edit(edit))));
    }
}

fn open(rig: &mut Rig, target: CommentTarget) {
    rig.msg(Message::Notation(NotationMessage::Event(
        mychess_gui::widgets::notation::NotationEvent::CommentClick(target),
    )));
}

fn comment(rig: &Rig, node: NodeId) -> Option<String> {
    rig.game().comment(node).unwrap().map(str::to_owned)
}

fn nags(rig: &Rig, node: NodeId) -> Vec<u8> {
    rig.game()
        .nags(node)
        .unwrap()
        .iter()
        .map(|n| n.value())
        .collect()
}

fn command_modifier() -> keyboard::Modifiers {
    if cfg!(target_os = "macos") {
        keyboard::Modifiers::LOGO
    } else {
        keyboard::Modifiers::CTRL
    }
}

fn key_event(named: Named, modifiers: keyboard::Modifiers) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(named),
        modified_key: Key::Named(named),
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers,
        repeat: false,
        text: None,
    })
}

// ---------------------------------------------------------------------------
// Targets (D177)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_the_header_names_the_comment_being_edited() {
    let mut s = sample();
    s.game.set_comment_before(s.nc3, "x").unwrap();
    let (e4, nc3, c5) = (s.e4, s.nc3, s.c5);
    let mut rig = Rig::with_game(s.game);
    // At the start of the game the editor is on the comment of the game.
    {
        let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
        assert!(sim.find("Comment at game start").is_ok());
    }
    rig.goto(e4);
    open(&mut rig, CommentTarget::After(e4));
    {
        let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
        assert!(sim.find("Comment after 1.e4").is_ok());
    }
    rig.goto(c5);
    open(&mut rig, CommentTarget::Before(nc3));
    {
        let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
        assert!(sim.find("Comment before 2.Nc3").is_ok());
    }
    rig.msg(Message::App(AppMessage::Game(GameMessage::Navigate(
        Nav::Start,
    ))));
    // Moving the cursor puts the editor back on the comment after it.
    assert_eq!(
        rig.shell.ui.notation.editor.target(),
        CommentTarget::Initial
    );
}

#[test]
fn m2_4_the_header_is_translated() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    rig.set_game(s.game);
    rig.goto(e4);
    open(&mut rig, CommentTarget::After(e4));
    let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
    assert!(sim.find("Comentário depois de 1.e4").is_ok());
}

#[test]
fn m2_4_move_labels_number_black_moves_with_three_dots() {
    let s = sample();
    assert_eq!(move_label(&s.game, s.e4).as_deref(), Some("1.e4"));
    assert_eq!(move_label(&s.game, s.e5).as_deref(), Some("1...e5"));
    assert_eq!(move_label(&s.game, s.nc3).as_deref(), Some("2.Nc3"));
    assert_eq!(move_label(&s.game, s.game.root()), None);
    assert_eq!(
        default_target(&s.game, s.game.root()),
        CommentTarget::Initial
    );
    assert_eq!(default_target(&s.game, s.e4), CommentTarget::After(s.e4));
}

#[test]
fn m2_4_each_target_reads_and_writes_its_own_comment() {
    let mut s = sample();
    s.game.set_comment(s.e4, "after e4").unwrap();
    s.game.set_comment_before(s.nc3, "before nc3").unwrap();
    s.game.set_initial_comment("game");
    assert_eq!(target_text(&s.game, CommentTarget::After(s.e4)), "after e4");
    assert_eq!(
        target_text(&s.game, CommentTarget::Before(s.nc3)),
        "before nc3"
    );
    assert_eq!(target_text(&s.game, CommentTarget::Initial), "game");
    assert_eq!(target_text(&s.game, CommentTarget::After(s.e5)), "");
}

#[test]
fn m2_4_saving_writes_after_before_and_initial_comments_where_they_belong() {
    let s = sample();
    let (e4, nc3) = (s.e4, s.nc3);
    let mut rig = Rig::with_game(s.game);
    // Initial.
    open(&mut rig, CommentTarget::Initial);
    type_text(&mut rig, "start");
    rig.msg(editor(EditorMessage::Commit));
    assert_eq!(rig.game().initial_comment(), Some("start"));
    // After(e4): the cursor goes to e4.
    open(&mut rig, CommentTarget::After(e4));
    type_text(&mut rig, "after");
    rig.msg(editor(EditorMessage::Commit));
    assert_eq!(comment(&rig, e4).as_deref(), Some("after"));
    // Before(nc3): the cursor stays where it is.
    let cursor = rig.cursor();
    open(&mut rig, CommentTarget::Before(nc3));
    type_text(&mut rig, "variation idea");
    rig.msg(editor(EditorMessage::Commit));
    assert_eq!(
        rig.game().comment_before(nc3).unwrap(),
        Some("variation idea")
    );
    assert_eq!(comment(&rig, nc3), None, "not the comment after nc3");
    assert_eq!(rig.cursor(), cursor);
}

// ---------------------------------------------------------------------------
// Saving (D178, rules 23-24)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_cmd_enter_in_the_text_area_commits_and_escape_cancels() {
    let mut rig = Rig::new();
    let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
    sim.click(widget::Id::new(EDITOR_ID)).expect("text area");
    sim.typewrite("Hello");
    sim.simulate([key_event(Named::Enter, command_modifier())]);
    let got = messages(sim);
    assert!(has(&got, &editor(EditorMessage::Commit)), "{got:?}");
    rig.feed(got);
    assert_eq!(rig.game().initial_comment(), Some("Hello"));

    // Escape in the focused area throws the draft away.
    let mut rig = Rig::new();
    let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
    sim.click(widget::Id::new(EDITOR_ID)).expect("text area");
    sim.typewrite("draft");
    sim.simulate([key_event(Named::Escape, keyboard::Modifiers::empty())]);
    let got = messages(sim);
    assert!(has(&got, &editor(EditorMessage::Cancel)), "{got:?}");
    rig.feed(got);
    assert_eq!(rig.game().initial_comment(), None, "nothing was written");
    assert_eq!(rig.shell.ui.notation.editor.text(), "");
}

#[test]
fn m2_4_a_plain_enter_in_the_text_area_is_a_new_line_not_a_save() {
    let rig = Rig::new();
    let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
    sim.click(widget::Id::new(EDITOR_ID)).expect("text area");
    sim.typewrite("a");
    sim.simulate([key_event(Named::Enter, keyboard::Modifiers::empty())]);
    let got = messages(sim);
    assert!(!has(&got, &editor(EditorMessage::Commit)), "{got:?}");
}

#[test]
fn m2_4_navigating_saves_the_draft_on_the_comment_it_was_typed_for() {
    let s = sample();
    let (e4, e5) = (s.e4, s.e5);
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    open(&mut rig, CommentTarget::After(e4));
    type_text(&mut rig, "typed on e4");
    // Any move of the cursor first saves.
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::Forward(1))));
    assert_eq!(rig.cursor(), e5);
    assert_eq!(comment(&rig, e4).as_deref(), Some("typed on e4"));
    assert_eq!(comment(&rig, e5), None);
}

#[test]
fn m2_4_losing_the_focus_saves_and_keeping_it_does_not() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    open(&mut rig, CommentTarget::After(e4));
    type_text(&mut rig, "x");
    rig.msg(editor(EditorMessage::FocusChecked(true)));
    assert_eq!(comment(&rig, e4), None, "still focused");
    rig.msg(editor(EditorMessage::FocusChecked(false)));
    assert_eq!(comment(&rig, e4).as_deref(), Some("x"));
}

#[test]
fn m2_4_closing_the_panel_saves_the_draft() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    // Cmd+E opens, typing, Cmd+E closes.
    rig.key(
        m2_4_common::command(mychess_app::Key::Char('e')),
        false,
        None,
    );
    assert!(rig.shell.ui.comment_visible());
    type_text(&mut rig, "closing");
    rig.key(
        m2_4_common::command(mychess_app::Key::Char('e')),
        false,
        None,
    );
    assert!(!rig.shell.ui.comment_visible());
    assert_eq!(comment(&rig, e4).as_deref(), Some("closing"));
}

#[test]
fn m2_4_saving_an_unchanged_draft_creates_no_undo_point() {
    let mut s = sample();
    s.game.set_comment(s.e4, "same").unwrap();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    open(&mut rig, CommentTarget::After(e4));
    let before = (
        rig.shell.app.session.changes(),
        rig.shell.app.session.undo_depth(),
    );
    rig.msg(editor(EditorMessage::Commit));
    // Edit and undo the edit inside the text area: same text again.
    type_text(&mut rig, "!");
    rig.msg(editor(EditorMessage::Action(Action::Edit(Edit::Backspace))));
    rig.msg(editor(EditorMessage::Commit));
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::Forward(1))));
    assert_eq!(
        (
            rig.shell.app.session.changes(),
            rig.shell.app.session.undo_depth()
        ),
        before
    );
}

#[test]
fn m2_4_a_blank_draft_removes_the_comment() {
    let mut s = sample();
    s.game.set_comment(s.e4, "to remove").unwrap();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    open(&mut rig, CommentTarget::After(e4));
    rig.msg(editor(EditorMessage::Clear));
    type_text(&mut rig, "   \n  ");
    rig.msg(editor(EditorMessage::Commit));
    assert_eq!(comment(&rig, e4), None);
}

#[test]
fn m2_4_comments_with_crlf_compare_equal_to_lf() {
    let mut s = sample();
    s.game.set_comment(s.e4, "a\nb").unwrap();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    open(&mut rig, CommentTarget::After(e4));
    let before = rig.shell.app.session.changes();
    // Replace the content with the same text with a CRLF break.
    rig.msg(editor(EditorMessage::Clear));
    rig.msg(editor(EditorMessage::Action(Action::Edit(Edit::Paste(
        Arc::new("a\r\nb".to_owned()),
    )))));
    rig.msg(editor(EditorMessage::Commit));
    assert_eq!(rig.shell.app.session.changes(), before);
}

#[test]
fn m2_4_cancel_discards_the_draft_and_restores_the_comment() {
    let mut s = sample();
    s.game.set_comment(s.e4, "original").unwrap();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    open(&mut rig, CommentTarget::After(e4));
    rig.msg(editor(EditorMessage::Clear));
    type_text(&mut rig, "discard me");
    rig.msg(editor(EditorMessage::Cancel));
    assert_eq!(rig.shell.ui.notation.editor.text(), "original");
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::Forward(1))));
    assert_eq!(comment(&rig, e4).as_deref(), Some("original"));
}

#[test]
fn m2_4_clicking_cancel_discards_the_draft_even_though_the_click_blurs_the_text_area() {
    // The toolkit's text area loses the focus when the button goes down; the
    // button acts on the release. The blur must not save the draft first.
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    open(&mut rig, CommentTarget::After(e4));
    type_text(&mut rig, "typed, then thrown away");
    let clicked = {
        let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
        sim.click("Cancel").expect("the Cancel button");
        messages(sim)
    };
    rig.feed(clicked);
    // What the shell does on the release of the button, and the answer of
    // its focus query (the area has no focus any more).
    rig.msg(Message::Notation(NotationMessage::PointerReleased));
    rig.msg(editor(EditorMessage::FocusChecked(false)));
    assert_eq!(
        comment(&rig, e4),
        None,
        "the draft was discarded, not saved"
    );
    assert_eq!(rig.shell.ui.notation.editor.text(), "");
}

#[test]
fn m2_4_a_release_elsewhere_with_the_draft_unsaved_still_saves_it() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    open(&mut rig, CommentTarget::After(e4));
    type_text(&mut rig, "kept");
    rig.msg(Message::Notation(NotationMessage::PointerReleased));
    rig.msg(editor(EditorMessage::FocusChecked(false)));
    assert_eq!(comment(&rig, e4).as_deref(), Some("kept"));
}

#[test]
fn m2_4_a_comment_of_more_than_one_mebibyte_is_refused() {
    // Rule 29: the field refuses what goes over 1 MiB.
    let mut rig = Rig::new();
    rig.msg(editor(EditorMessage::Action(Action::Edit(Edit::Paste(
        Arc::new("x".repeat((1 << 20) + 10)),
    )))));
    assert!(
        rig.shell.ui.notation.editor.text().len() <= 1 << 20,
        "the draft holds {} bytes",
        rig.shell.ui.notation.editor.text().len()
    );
}

// ---------------------------------------------------------------------------
// Dropping node ids when the game changes under the editor (arch. 2.2.1)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_undo_drops_the_target_and_the_draft_instead_of_writing_it_somewhere_else() {
    let s = sample();
    let (e4, nc3) = (s.e4, s.nc3);
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    // An edit to undo.
    rig.app_msg(AppMessage::Game(GameMessage::SetComment {
        node: e4,
        text: "first".to_owned(),
    }));
    open(&mut rig, CommentTarget::Before(nc3));
    type_text(&mut rig, "lost on undo");
    // The game changes behind the editor's back (not through the shell, so
    // the draft is not saved first).
    rig.send(AppMessage::Game(GameMessage::Undo));
    assert_eq!(comment(&rig, e4), None, "the undo happened");
    assert_eq!(
        rig.game().comment_before(nc3).unwrap(),
        None,
        "draft not saved"
    );
    let ed = &rig.shell.ui.notation.editor;
    assert_eq!(
        ed.target(),
        CommentTarget::After(rig.cursor()),
        "back to the cursor"
    );
    assert_eq!(ed.text(), "");
}

#[test]
fn m2_4_undo_from_the_shell_saves_the_draft_first_and_then_undoes_that_save() {
    // D178: every action that changes the game saves the draft first; the
    // undo then takes that save back, so an earlier edit is kept.
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    rig.app_msg(AppMessage::Game(GameMessage::SetComment {
        node: e4,
        text: "first".to_owned(),
    }));
    open(&mut rig, CommentTarget::After(e4));
    rig.msg(editor(EditorMessage::Clear));
    type_text(&mut rig, "second");
    rig.app_msg(AppMessage::Game(GameMessage::Undo));
    assert_eq!(comment(&rig, e4).as_deref(), Some("first"));
    assert_eq!(rig.shell.ui.notation.editor.text(), "first");
}

#[test]
fn m2_4_redo_and_a_new_game_drop_the_target_too() {
    let s = sample();
    let (e4, nc3) = (s.e4, s.nc3);
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    rig.app_msg(AppMessage::Game(GameMessage::SetComment {
        node: e4,
        text: "first".to_owned(),
    }));
    rig.app_msg(AppMessage::Game(GameMessage::Undo));
    open(&mut rig, CommentTarget::Before(nc3));
    type_text(&mut rig, "lost on redo");
    rig.send(AppMessage::Game(GameMessage::Redo));
    assert_eq!(rig.game().comment_before(nc3).unwrap(), None);
    assert_eq!(
        rig.shell.ui.notation.editor.target(),
        CommentTarget::After(rig.cursor())
    );
    // A new game: nothing typed reaches it.
    open(&mut rig, CommentTarget::After(e4));
    type_text(&mut rig, "for the old game");
    rig.set_game(Game::new());
    assert_eq!(rig.game().initial_comment(), None);
    assert_eq!(
        rig.shell.ui.notation.editor.target(),
        CommentTarget::Initial
    );
    assert_eq!(rig.shell.ui.notation.editor.text(), "");
}

#[test]
fn m2_4_entering_and_leaving_trial_mode_resets_the_editor() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    open(&mut rig, CommentTarget::After(e4));
    type_text(&mut rig, "typed before the trial");
    rig.app_msg(AppMessage::Game(GameMessage::EnterTrial {
        null_move: false,
    }));
    rig.app_msg(AppMessage::Game(GameMessage::ExitTrial));
    // The draft is not lost silently nor written to a stale id: either it
    // was saved before the trial started or it is gone; in both cases the
    // editor shows what the game has.
    let shown = rig.shell.ui.notation.editor.text();
    let held = comment(&rig, e4).unwrap_or_default();
    assert_eq!(shown, held);
}

#[test]
fn m2_4_a_target_that_was_deleted_never_panics_or_writes() {
    let s = sample();
    let (nc3, nf3) = (s.nc3, s.nf3);
    let mut rig = Rig::with_game(s.game);
    rig.goto(nf3);
    open(&mut rig, CommentTarget::Before(nc3));
    type_text(&mut rig, "for a variation about to vanish");
    rig.app_msg(AppMessage::Game(GameMessage::DeleteVariation(
        mychess_app::VariationTarget::Node(nc3),
    )));
    rig.msg(editor(EditorMessage::Commit));
    assert!(!rig.game().contains(nc3));
    assert_eq!(rig.game().initial_comment(), None);
    assert_eq!(
        rig.shell.ui.notation.editor.target(),
        CommentTarget::After(nf3)
    );
}

// ---------------------------------------------------------------------------
// The NAG field (D179, rule 27)
// ---------------------------------------------------------------------------

fn at_e4(rig: &mut Rig, e4: NodeId) {
    rig.goto(e4);
    open(rig, CommentTarget::After(e4));
}

fn commit_nag_text(rig: &mut Rig, text: &str) {
    rig.msg(editor(EditorMessage::NagEdited(text.to_owned())));
    rig.msg(editor(EditorMessage::Commit));
}

#[test]
fn m2_4_the_field_shows_the_nags_of_the_move_as_text() {
    let mut s = sample();
    s.game.add_nag(s.e4, nag(1)).unwrap();
    s.game.add_nag(s.e4, nag(18)).unwrap();
    s.game.add_nag(s.e4, nag(201)).unwrap();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    at_e4(&mut rig, e4);
    // `Nag::ascii()` text, one space apart (or `$n` when there is none).
    let text = rig.shell.ui.notation.editor.nag_text().to_owned();
    let tokens: Vec<_> = text.split(' ').collect();
    assert_eq!(tokens.len(), 3, "{text:?}");
    assert_eq!(tokens[0], "!");
    assert_eq!(tokens[1], "+-");
}

#[test]
fn m2_4_valid_tokens_in_any_notation_become_the_nags_without_duplicates() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    at_e4(&mut rig, e4);
    commit_nag_text(&mut rig, "!? +- $18 18 N");
    assert_eq!(nags(&rig, e4), [5, 18, 146], "order kept, 18 once");
    assert_eq!(rig.shell.ui.notation.editor.nag_error(), None);
    // Emptying the field removes them all.
    commit_nag_text(&mut rig, "");
    assert_eq!(nags(&rig, e4), Vec::<u8>::new());
}

#[test]
fn m2_4_an_unknown_token_blocks_the_save_and_shows_the_error_inline() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    at_e4(&mut rig, e4);
    commit_nag_text(&mut rig, "! foo");
    assert_eq!(nags(&rig, e4), Vec::<u8>::new(), "nothing saved");
    assert_eq!(rig.shell.ui.notation.editor.nag_error(), Some("foo"));
    {
        let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
        assert!(sim.find("Unknown annotation: foo").is_ok());
    }
    // Typing again clears the message.
    rig.msg(editor(EditorMessage::NagEdited("!".to_owned())));
    assert_eq!(rig.shell.ui.notation.editor.nag_error(), None);
}

#[test]
fn m2_4_dollar_zero_and_dollar_256_are_errors() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    for bad in ["$0", "$256", "0", "256", "$", "$-1", "!!!"] {
        at_e4(&mut rig, e4);
        commit_nag_text(&mut rig, bad);
        assert!(
            rig.shell.ui.notation.editor.nag_error().is_some(),
            "{bad:?} must be refused"
        );
        assert_eq!(nags(&rig, e4), Vec::<u8>::new(), "{bad:?}");
    }
}

#[test]
fn m2_4_the_error_is_translated() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    rig.set_game(s.game);
    at_e4(&mut rig, e4);
    commit_nag_text(&mut rig, "zzz");
    let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
    assert!(sim.find("Anotação desconhecida: zzz").is_ok());
}

#[test]
fn m2_4_at_the_start_the_field_and_the_palette_are_disabled() {
    let rig = Rig::new();
    let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
    // The field cannot be typed into.
    if sim.click(widget::Id::new(NAG_FIELD_ID)).is_ok() {
        sim.typewrite("!");
    }
    drop(sim.click("!"));
    drop(sim.click("D"));
    let got = messages(sim);
    assert!(
        !got.iter().any(|m| matches!(
            m,
            Message::Notation(NotationMessage::Editor(
                EditorMessage::Toggle(_) | EditorMessage::NagEdited(_)
            ))
        )),
        "nothing editable at the start: {got:?}"
    );
}

// ---------------------------------------------------------------------------
// The palette (D179, D180)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_the_palette_has_the_sixteen_buttons_of_the_scid_in_order() {
    assert_eq!(
        PALETTE_NAGS,
        [1, 2, 3, 4, 5, 6, 18, 19, 16, 17, 14, 15, 10, 13, 146, 201]
    );
    let from_iter: Vec<u8> = palette_nags().map(Nag::value).collect();
    assert_eq!(from_iter.len(), 16);
    for (got, want) in from_iter.iter().zip(PALETTE_NAGS) {
        assert_eq!(u16::from(*got), want);
    }
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for nag in palette_nags() {
            let key = tip_key(nag);
            assert!(i18n.has(&key), "{key} in {language:?}");
            assert_ne!(i18n.tr(&key), key);
        }
    }
}

#[test]
fn m2_4_each_palette_button_toggles_its_nag_on_the_current_move() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    at_e4(&mut rig, e4);
    for (label, number) in [
        ("!", 1u8),
        ("?", 2),
        ("!!", 3),
        ("??", 4),
        ("!?", 5),
        ("?!", 6),
        ("+-", 18),
        ("-+", 19),
        ("=", 10),
        ("~", 13),
        ("N", 146),
        ("D", 201),
    ] {
        let mut sim = iced_test::simulator(comment_editor::view(&rig.shell));
        sim.click(label)
            .unwrap_or_else(|_| panic!("button {label}"));
        let got = messages(sim);
        assert!(
            has(&got, &editor(EditorMessage::Toggle(nag(u16::from(number))))),
            "{label}: {got:?}"
        );
    }
}

#[test]
fn m2_4_toggling_a_nag_twice_removes_it() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    at_e4(&mut rig, e4);
    rig.msg(editor(EditorMessage::Toggle(nag(1))));
    assert_eq!(nags(&rig, e4), [1]);
    rig.msg(editor(EditorMessage::Toggle(nag(1))));
    assert_eq!(nags(&rig, e4), Vec::<u8>::new());
}

#[test]
fn m2_4_move_quality_nags_exclude_each_other() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    at_e4(&mut rig, e4);
    rig.msg(editor(EditorMessage::Toggle(nag(1))));
    rig.msg(editor(EditorMessage::Toggle(nag(2))));
    assert_eq!(nags(&rig, e4), [2], "? replaced !");
    rig.msg(editor(EditorMessage::Toggle(nag(6))));
    assert_eq!(nags(&rig, e4), [6]);
}

#[test]
fn m2_4_position_evaluation_nags_exclude_each_other_and_ignore_the_other_group() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    at_e4(&mut rig, e4);
    rig.msg(editor(EditorMessage::Toggle(nag(1))));
    rig.msg(editor(EditorMessage::Toggle(nag(18))));
    assert_eq!(
        nags(&rig, e4),
        [1, 18],
        "a quality and an evaluation coexist"
    );
    rig.msg(editor(EditorMessage::Toggle(nag(19))));
    assert_eq!(nags(&rig, e4), [1, 19], "-+ replaced +-");
    // += (14) and = (10) share the group {10, 13, 14..19}.
    rig.msg(editor(EditorMessage::Toggle(nag(14))));
    assert_eq!(nags(&rig, e4), [1, 14]);
    rig.msg(editor(EditorMessage::Toggle(nag(10))));
    assert_eq!(nags(&rig, e4), [1, 10]);
    // Novelty and diagram are in no group.
    rig.msg(editor(EditorMessage::Toggle(nag(146))));
    rig.msg(editor(EditorMessage::Toggle(nag(201))));
    assert_eq!(nags(&rig, e4), [1, 10, 146, 201]);
}

#[test]
fn m2_4_the_text_field_does_not_impose_groups() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    at_e4(&mut rig, e4);
    commit_nag_text(&mut rig, "! ?");
    assert_eq!(nags(&rig, e4), [1, 2], "free text is taken as typed");
}

// ---------------------------------------------------------------------------
// NAG shortcuts (D181, rule 28)
// ---------------------------------------------------------------------------

fn type_keys(rig: &mut Rig, chars: &str) {
    for c in chars.chars() {
        rig.press(ch(c));
    }
}

fn enter(rig: &mut Rig) {
    rig.press(m2_4_common::named(NamedKey::Enter));
}

#[test]
fn m2_4_sequences_without_enter_fire_at_once() {
    let s = sample();
    let e4 = s.e4;
    for (keys, expected) in [
        ("+-", 18u8),
        ("-+", 19),
        ("+=", 14),
        ("=+", 15),
        ("+/", 16),
        ("-/", 17),
    ] {
        let mut rig = Rig::with_game(s.game.clone());
        rig.goto(e4);
        type_keys(&mut rig, keys);
        assert_eq!(nags(&rig, e4), [expected], "{keys}");
    }
}

#[test]
fn m2_4_sequences_with_enter_wait_for_it() {
    let s = sample();
    let e4 = s.e4;
    for (keys, expected) in [
        ("!", 1u8),
        ("!!", 3),
        ("?", 2),
        ("??", 4),
        ("!?", 5),
        ("?!", 6),
        ("=", 10),
        ("~", 13),
    ] {
        let mut rig = Rig::with_game(s.game.clone());
        rig.goto(e4);
        type_keys(&mut rig, keys);
        assert_eq!(nags(&rig, e4), Vec::<u8>::new(), "{keys}: not before Enter");
        enter(&mut rig);
        assert_eq!(nags(&rig, e4), [expected], "{keys}+Enter");
    }
}

#[test]
fn m2_4_two_minuses_are_a_null_move_not_a_nag() {
    let s = sample();
    let bb5 = s.bb5;
    let mut rig = Rig::with_game(s.game);
    rig.goto(bb5);
    let before = rig.game().node_count();
    type_keys(&mut rig, "--");
    assert_eq!(nags(&rig, bb5), Vec::<u8>::new());
    assert_eq!(
        rig.game().node_count(),
        before + 1,
        "a null move was played"
    );
}

#[test]
fn m2_4_a_half_typed_sequence_times_out_after_1500_ms() {
    assert_eq!(NAG_SEQUENCE_TIMEOUT, Duration::from_millis(1500));
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    let t0 = Instant::now();
    rig.msg(Message::Dialog(DialogMessage::Typed(
        TypedKey::Char('+'),
        t0,
    )));
    rig.msg(Message::Dialog(DialogMessage::Typed(
        TypedKey::Char('-'),
        t0 + Duration::from_millis(1499),
    )));
    assert_eq!(nags(&rig, e4), [18], "+- within the window");

    let mut rig = Rig::with_game(sample().game);
    let e4 = rig.game().children(rig.game().root()).unwrap()[0];
    rig.goto(e4);
    rig.msg(Message::Dialog(DialogMessage::Typed(
        TypedKey::Char('+'),
        t0,
    )));
    rig.msg(Message::Dialog(DialogMessage::Typed(
        TypedKey::Char('-'),
        t0 + Duration::from_millis(1501),
    )));
    assert_eq!(nags(&rig, e4), Vec::<u8>::new(), "too late: no NAG");
}

#[test]
fn m2_4_another_key_resets_the_sequence() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    rig.press(ch('+'));
    rig.press(m2_4_common::named(NamedKey::Left));
    rig.press(ch('-'));
    assert_eq!(nags(&rig, e4), Vec::<u8>::new());
}

#[test]
fn m2_4_no_shortcut_fires_while_a_text_field_has_the_focus() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    for c in ['+', '-', '!'] {
        rig.key(ch(c), true, Some(c));
    }
    rig.key(m2_4_common::named(NamedKey::Enter), true, None);
    assert_eq!(nags(&rig, e4), Vec::<u8>::new());
}

#[test]
fn m2_4_at_the_start_of_the_game_a_nag_shortcut_changes_nothing() {
    let mut rig = Rig::new();
    let before = rig.shell.app.session.changes();
    type_keys(&mut rig, "+-");
    assert_eq!(rig.shell.app.session.changes(), before);
    assert!(
        rig.game()
            .nags(rig.game().root())
            .map_or(true, <[Nag]>::is_empty)
    );
}

#[test]
fn m2_4_shortcut_nags_follow_the_exclusive_groups_too() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    type_keys(&mut rig, "+-");
    type_keys(&mut rig, "-+");
    assert_eq!(nags(&rig, e4), [19]);
}
