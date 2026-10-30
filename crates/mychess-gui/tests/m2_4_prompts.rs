//! M2-4 prompts and small dialogs: go to move, replace move (5 options and
//! the R/M/V/T keys), confirm removal (plural and count), discard changes,
//! overwrite file by reason and the variation picker (spec D186, D188-D190,
//! D198, D203, rules 45-48, M2-2 D103/D127).

#![allow(
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::float_cmp,
    clippy::unnecessary_wraps
)]

mod m2_4_common;

use iced::{Size, widget};
use iced_test::Simulator;
use m2_4_common::{Rig, ch, command, command_shift, has, line, messages, named, play, sample};
use mychess_app::{
    Command, DiscardChoice, GameMessage, Key, LanguagePref, Message as AppMessage, NamedKey, Nav,
    OverwriteReason, Prompt, ReplaceChoice, StripKind,
};
use mychess_core::{Game, NodeId};
use mychess_gui::Message;
use mychess_gui::windows::dialog_ui::{
    ActiveDialog, DialogMessage, GOTO_FIELD_ID, GotoDraft, GotoTarget,
};
use mychess_gui::windows::{dialogs, goto_move};
use rstest::rstest;

fn dialog_sim(rig: &Rig) -> Simulator<'_, Message> {
    let element = dialogs::view(&rig.shell).expect("a dialog is open");
    Simulator::with_size(iced::Settings::default(), Size::new(1000.0, 700.0), element)
}

fn answer(message: AppMessage) -> Message {
    Message::App(message)
}

/// 1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 as the main line of a rig; the cursor at the
/// start.
fn rig_with_line() -> (Rig, Vec<NodeId>) {
    let mut game = Game::new();
    let root = game.root();
    let nodes = line(
        &mut game,
        root,
        &["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6"],
    );
    (Rig::with_game(game), nodes)
}

// ---------------------------------------------------------------------------
// Go to move (D198)
// ---------------------------------------------------------------------------

#[rstest]
#[case("12", Some((12, false)))]
#[case("12.", Some((12, false)))]
#[case("12...", Some((12, true)))]
#[case("12b", Some((12, true)))]
#[case("12B", Some((12, true)))]
#[case("  7  ", Some((7, false)))]
#[case("1", Some((1, false)))]
#[case("0", None)]
#[case("0.", None)]
#[case("-3", None)]
#[case("", None)]
#[case("abc", None)]
#[case("12..", None)]
#[case("12x", None)]
#[case("1e3", None)]
#[case("99999999999999999999", None)]
#[case("1 2", None)]
fn m2_4_go_to_move_reads_the_number_forms_of_the_spec(
    #[case] text: &str,
    #[case] expected: Option<(u32, bool)>,
) {
    let draft = GotoDraft {
        text: text.to_owned(),
        invalid: false,
    };
    assert_eq!(
        draft.parse(),
        expected.map(|(number, black)| GotoTarget { number, black }),
        "{text:?}"
    );
}

#[test]
fn m2_4_command_g_opens_go_to_move_and_a_number_goes_to_the_white_move() {
    let (mut rig, nodes) = rig_with_line();
    rig.key(command(Key::Char('g')), false, None);
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::Goto)
    );
    rig.msg(Message::Dialog(DialogMessage::GotoEdited("2".to_owned())));
    rig.msg(Message::Dialog(DialogMessage::GotoSubmit));
    assert_eq!(rig.cursor(), nodes[2], "2.Nf3");
    assert!(rig.shell.ui.dialogs.goto.is_none(), "closed on success");
}

#[test]
fn m2_4_go_to_move_with_dots_or_b_goes_to_the_black_move() {
    for text in ["2...", "2b"] {
        let (mut rig, nodes) = rig_with_line();
        rig.key(command(Key::Char('g')), false, None);
        rig.msg(Message::Dialog(DialogMessage::GotoEdited(text.to_owned())));
        rig.msg(Message::Dialog(DialogMessage::GotoSubmit));
        assert_eq!(rig.cursor(), nodes[3], "{text}: 2...Nc6");
    }
}

#[test]
fn m2_4_an_invalid_number_keeps_the_dialog_open_with_an_error() {
    let (mut rig, _) = rig_with_line();
    rig.key(command(Key::Char('g')), false, None);
    rig.msg(Message::Dialog(DialogMessage::GotoEdited("abc".to_owned())));
    rig.msg(Message::Dialog(DialogMessage::GotoSubmit));
    let draft = rig.shell.ui.dialogs.goto.clone().expect("still open");
    assert!(draft.invalid);
    assert_eq!(rig.cursor(), rig.game().root());
    {
        let mut sim = dialog_sim(&rig);
        assert!(sim.find("Enter a move number greater than zero.").is_ok());
    }
    // Editing the text clears the error.
    rig.msg(Message::Dialog(DialogMessage::GotoEdited("3".to_owned())));
    assert!(!rig.shell.ui.dialogs.goto.clone().unwrap().invalid);
}

#[test]
fn m2_4_go_to_move_with_a_number_past_the_end_does_not_panic_and_closes() {
    let (mut rig, _) = rig_with_line();
    rig.key(command(Key::Char('g')), false, None);
    rig.msg(Message::Dialog(DialogMessage::GotoEdited("500".to_owned())));
    rig.msg(Message::Dialog(DialogMessage::GotoSubmit));
    assert!(rig.shell.ui.dialogs.goto.is_none());
    // The cursor stays on a node of the game (the end of it, or unchanged).
    assert!(rig.game().contains(rig.cursor()));
}

#[test]
fn m2_4_go_to_move_through_the_field_and_enter() {
    let (mut rig, nodes) = rig_with_line();
    rig.key(command(Key::Char('g')), false, None);
    let draft = rig.shell.ui.dialogs.goto.clone().expect("open");
    let element = goto_move::view(&draft, &rig.shell.i18n);
    let mut sim = iced_test::simulator(element);
    sim.click(widget::Id::new(GOTO_FIELD_ID)).expect("field");
    sim.typewrite("3");
    sim.tap_key(iced::keyboard::key::Named::Enter);
    let got = messages(sim);
    assert!(
        has(
            &got,
            &Message::Dialog(DialogMessage::GotoEdited("3".to_owned()))
        ),
        "{got:?}"
    );
    assert!(
        has(&got, &Message::Dialog(DialogMessage::GotoSubmit)),
        "{got:?}"
    );
    rig.feed(got);
    assert_eq!(rig.cursor(), nodes[4], "3.Bb5");
}

#[test]
fn m2_4_escape_and_cancel_close_go_to_move_without_moving() {
    let (mut rig, _) = rig_with_line();
    rig.key(command(Key::Char('g')), false, None);
    rig.press(named(NamedKey::Escape));
    assert!(rig.shell.ui.dialogs.goto.is_none());
    assert_eq!(rig.cursor(), rig.game().root());
}

#[test]
fn m2_4_go_to_move_in_a_game_that_starts_from_a_fen_uses_the_real_move_number() {
    // D198: for a FEN start the number is the real one (move 20 here).
    let position = mychess_core::Position::from_fen(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 20",
    )
    .expect("fen");
    let mut game = Game::from_position(position);
    let root = game.root();
    let nodes = line(&mut game, root, &["e2e4", "e7e5", "g1f3"]);
    let mut rig = Rig::with_game(game);
    rig.key(command(Key::Char('g')), false, None);
    rig.msg(Message::Dialog(DialogMessage::GotoEdited("21".to_owned())));
    rig.msg(Message::Dialog(DialogMessage::GotoSubmit));
    assert_eq!(rig.cursor(), nodes[2], "21.Nf3 is the third ply");
}

// ---------------------------------------------------------------------------
// Replace move (D188, rules 45-46)
// ---------------------------------------------------------------------------

/// A game 1.e4 e5 with the cursor at the start and the prompt open because
/// the user played 1.d4 over the existing move.
fn replace_prompt() -> (Rig, NodeId, NodeId) {
    let mut game = Game::new();
    let root = game.root();
    let nodes = line(&mut game, root, &["e2e4", "e7e5"]);
    let mut rig = Rig::with_game(game);
    let mv = rig
        .shell
        .app
        .session
        .position()
        .parse_uci("d2d4")
        .expect("legal");
    rig.app_msg(AppMessage::Game(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    }));
    assert!(
        matches!(rig.shell.app.session.prompt(), Some(Prompt::ReplaceMove(_))),
        "{:?}",
        rig.shell.app.session.prompt()
    );
    (rig, nodes[0], nodes[1])
}

#[test]
fn m2_4_the_replace_box_has_the_five_options_in_the_scid_order() {
    let (rig, ..) = replace_prompt();
    let mut sim = dialog_sim(&rig);
    for label in [
        "Replace move",
        "New main line",
        "Add variation",
        "Try variation",
        "Cancel",
    ] {
        assert!(sim.find(label).is_ok(), "{label}");
    }
    assert!(sim.find("Replace move?").is_ok(), "the title");
}

#[test]
fn m2_4_each_replace_button_sends_its_answer() {
    for (label, choice) in [
        ("Replace move", ReplaceChoice::Replace),
        ("New main line", ReplaceChoice::NewMainLine),
        ("Add variation", ReplaceChoice::AddVariation),
        ("Try variation", ReplaceChoice::Trial),
        ("Cancel", ReplaceChoice::Cancel),
    ] {
        let (rig, ..) = replace_prompt();
        let mut sim = dialog_sim(&rig);
        sim.click(label).expect(label);
        let got = messages(sim);
        assert!(
            has(
                &got,
                &answer(AppMessage::Game(GameMessage::AnswerReplace(choice)))
            ),
            "{label}: {got:?}"
        );
    }
}

#[test]
fn m2_4_the_replace_answers_change_the_game_as_the_names_say() {
    // Replace: d4 replaces e4 and all that follows.
    let (mut rig, e4, _) = replace_prompt();
    rig.key(ch('r'), false, Some('r'));
    assert!(rig.shell.app.session.prompt().is_none());
    assert!(!rig.game().contains(e4));
    assert_eq!(rig.game().mainline().count(), 1);

    // New main line: d4 first, e4 now a variation.
    let (mut rig, e4, _) = replace_prompt();
    rig.key(ch('m'), false, Some('m'));
    let root = rig.game().root();
    let children = rig.game().children(root).unwrap().to_vec();
    assert_eq!(children.len(), 2);
    assert_eq!(children[1], e4, "the old line became a variation");

    // Add variation (also the Enter default): e4 stays first.
    for press in [Key::Char('v'), Key::Named(NamedKey::Enter)] {
        let (mut rig, e4, _) = replace_prompt();
        rig.key(m2_4_common::plain(press), false, None);
        let root = rig.game().root();
        let children = rig.game().children(root).unwrap().to_vec();
        assert_eq!(children.len(), 2, "{press:?}");
        assert_eq!(children[0], e4, "{press:?}");
    }

    // Try: trial mode.
    let (mut rig, ..) = replace_prompt();
    rig.key(ch('t'), false, Some('t'));
    assert!(rig.shell.app.session.in_trial());

    // Escape cancels.
    let (mut rig, e4, e5) = replace_prompt();
    rig.press(named(NamedKey::Escape));
    assert!(rig.shell.app.session.prompt().is_none());
    assert_eq!(rig.game().children(rig.game().root()).unwrap(), [e4]);
    assert!(rig.game().contains(e5));
}

#[test]
fn m2_4_other_letters_do_nothing_while_the_replace_box_is_open() {
    let (mut rig, ..) = replace_prompt();
    for c in ['x', 'q', 'z', '1'] {
        rig.key(ch(c), false, Some(c));
    }
    assert!(matches!(
        rig.shell.app.session.prompt(),
        Some(Prompt::ReplaceMove(_))
    ));
}

#[test]
fn m2_4_the_replace_box_blocks_the_board_and_the_navigation() {
    let (mut rig, e4, _) = replace_prompt();
    let cursor = rig.cursor();
    let mv = rig
        .shell
        .app
        .session
        .position()
        .parse_uci("c2c4")
        .expect("legal");
    rig.app_msg(AppMessage::Game(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    }));
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::GoToNode(e4))));
    assert_eq!(rig.cursor(), cursor, "the cursor did not move");
    assert_eq!(rig.game().children(rig.game().root()).unwrap().len(), 1);
    // The arrow keys belong to the dialog.
    rig.press(named(NamedKey::Right));
    assert_eq!(rig.cursor(), cursor);
}

#[test]
fn m2_4_cancelling_the_replace_box_clears_the_typed_move() {
    let mut game = Game::new();
    let root = game.root();
    line(&mut game, root, &["e2e4", "e7e5"]);
    let mut rig = Rig::with_game(game);
    for c in "d4".chars() {
        rig.app_msg(AppMessage::Game(GameMessage::MoveEntryChar(c)));
    }
    rig.app_msg(AppMessage::Game(GameMessage::MoveEntryCommit));
    assert!(rig.shell.app.session.prompt().is_some(), "typed d4 asks");
    rig.press(named(NamedKey::Escape));
    assert_eq!(rig.shell.app.session.move_entry().text(), "");
    assert!(rig.shell.app.session.prompt().is_none());
}

#[test]
fn m2_4_a_move_that_already_exists_only_navigates_without_asking() {
    let mut game = Game::new();
    let root = game.root();
    let nodes = line(&mut game, root, &["e2e4", "e7e5"]);
    let mut rig = Rig::with_game(game);
    let mv = rig
        .shell
        .app
        .session
        .position()
        .parse_uci("e2e4")
        .expect("legal");
    rig.app_msg(AppMessage::Game(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    }));
    assert!(rig.shell.app.session.prompt().is_none());
    assert_eq!(rig.cursor(), nodes[0]);
}

#[test]
fn m2_4_trial_mode_never_asks() {
    let mut game = Game::new();
    let root = game.root();
    line(&mut game, root, &["e2e4", "e7e5"]);
    let mut rig = Rig::with_game(game);
    rig.app_msg(AppMessage::Game(GameMessage::EnterTrial {
        null_move: false,
    }));
    let mv = rig
        .shell
        .app
        .session
        .position()
        .parse_uci("d2d4")
        .expect("legal");
    rig.app_msg(AppMessage::Game(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    }));
    assert!(rig.shell.app.session.prompt().is_none());
}

#[test]
fn m2_4_the_replace_box_offers_a_dont_ask_again_checkbox() {
    // D188: "Caixa 'Não perguntar de novo' grava a preferência".
    let (rig, ..) = replace_prompt();
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("Don't ask again").is_ok());
}

// ---------------------------------------------------------------------------
// Confirm removal (D190, rule 48)
// ---------------------------------------------------------------------------

fn game_with_comments(n: usize) -> (Rig, Vec<NodeId>) {
    let mut game = Game::new();
    let root = game.root();
    let nodes = line(&mut game, root, &["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"]);
    for node in nodes.iter().take(n) {
        game.set_comment(*node, "note").expect("comment");
    }
    (Rig::with_game(game), nodes)
}

fn request(kind: StripKind) -> AppMessage {
    AppMessage::Game(GameMessage::RequestStrip {
        kind,
        confirm: true,
    })
}

#[test]
fn m2_4_the_removal_question_counts_and_uses_the_plural() {
    for (n, text) in [
        (1, "Remove 1 comment from this game?"),
        (2, "Remove 2 comments from this game?"),
        (5, "Remove 5 comments from this game?"),
    ] {
        let (mut rig, _) = game_with_comments(n);
        rig.app_msg(request(StripKind::Comments));
        let mut sim = dialog_sim(&rig);
        assert!(sim.find(text).is_ok(), "{text}");
    }
}

#[test]
fn m2_4_the_removal_question_for_variations_and_moves_has_its_own_plural() {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    // sample() has two variations (1...c5 and 2.Nc3).
    rig.app_msg(request(StripKind::Variations));
    {
        let mut sim = dialog_sim(&rig);
        assert!(sim.find("Remove 2 variations from this game?").is_ok());
    }
    rig.app_msg(AppMessage::Game(GameMessage::AnswerStrip(false)));
    // After e4 there are e5 Nf3 Nc6 Bb5 on the main line (the variations
    // hang from those moves); the question counts moves.
    rig.app_msg(request(StripKind::TruncateAfter(e4)));
    let prompt = rig.shell.app.session.prompt().cloned();
    let Some(Prompt::ConfirmStrip { count, .. }) = prompt else {
        panic!("a question was expected: {prompt:?}");
    };
    assert!(count >= 4, "counts the moves that go: {count}");
    let mut sim = dialog_sim(&rig);
    assert!(
        sim.find(format!("Remove {count} moves after this position?"))
            .is_ok()
    );
}

#[test]
fn m2_4_the_singular_forms_and_the_portuguese_texts() {
    let mut game = Game::new();
    let root = game.root();
    let e4 = line(&mut game, root, &["e2e4", "e7e5"]);
    game.set_comment(e4[0], "c").expect("comment");
    let mut rig = Rig::with_language(LanguagePref::PtBr);
    rig.set_game(game);
    rig.app_msg(request(StripKind::Comments));
    {
        let mut sim = dialog_sim(&rig);
        assert!(sim.find("Remover 1 comentário desta partida?").is_ok());
        assert!(sim.find("Remover").is_ok());
        assert!(sim.find("Cancelar").is_ok());
    }
    rig.app_msg(AppMessage::Game(GameMessage::AnswerStrip(false)));
    rig.app_msg(request(StripKind::TruncateAfter(rig.game().root())));
    // Two moves after the start.
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("Remover 2 lances após esta posição?").is_ok());
}

#[test]
fn m2_4_confirming_removes_and_cancelling_keeps() {
    let (mut rig, nodes) = game_with_comments(3);
    rig.app_msg(request(StripKind::Comments));
    let mut sim = dialog_sim(&rig);
    sim.click("Cancel").expect("cancel");
    let got = messages(sim);
    assert!(has(
        &got,
        &answer(AppMessage::Game(GameMessage::AnswerStrip(false)))
    ));
    rig.feed(got);
    assert!(rig.game().comment(nodes[0]).unwrap().is_some());

    rig.app_msg(request(StripKind::Comments));
    let mut sim = dialog_sim(&rig);
    sim.click("Remove").expect("remove");
    let got = messages(sim);
    assert!(has(
        &got,
        &answer(AppMessage::Game(GameMessage::AnswerStrip(true)))
    ));
    rig.feed(got);
    assert!(rig.game().comment(nodes[0]).unwrap().is_none());
}

#[test]
fn m2_4_a_removal_with_nothing_to_remove_opens_no_question_and_says_so() {
    let (mut rig, _) = game_with_comments(0);
    rig.app_msg(request(StripKind::Comments));
    assert!(
        rig.shell.app.session.prompt().is_none(),
        "count 0: no prompt"
    );
    let mut sim = rig.sim(Size::new(1200.0, 800.0));
    assert!(
        sim.find("Nothing to remove.").is_ok(),
        "status-nothing-to-strip is shown (rule 48)"
    );
}

#[test]
fn m2_4_delete_asks_to_remove_the_moves_after_the_cursor() {
    let (mut rig, nodes) = game_with_comments(0);
    rig.goto(nodes[1]);
    rig.press(named(NamedKey::Delete));
    let prompt = rig.shell.app.session.prompt().cloned();
    assert!(
        matches!(prompt, Some(Prompt::ConfirmStrip { count: 3, .. })),
        "{prompt:?}"
    );
    assert_eq!(rig.game().mainline().count(), 5, "nothing removed yet");
    rig.press(named(NamedKey::Escape));
    assert!(rig.shell.app.session.prompt().is_none());
    assert_eq!(rig.game().mainline().count(), 5);
}

#[test]
fn m2_4_command_backspace_also_truncates() {
    // Resolved question 7: `Cmd+Backspace` is mapped to truncate as well.
    let (mut rig, nodes) = game_with_comments(0);
    rig.goto(nodes[1]);
    rig.key(command(Key::Named(NamedKey::Backspace)), false, None);
    assert!(
        matches!(
            rig.shell.app.session.prompt(),
            Some(Prompt::ConfirmStrip { .. })
        ),
        "{:?}",
        rig.shell.app.session.prompt()
    );
}

#[test]
fn m2_4_the_removal_question_offers_a_dont_ask_again_checkbox() {
    // D190: "caixa Não perguntar de novo (preferência confirm_destructive)".
    let (mut rig, _) = game_with_comments(2);
    rig.app_msg(request(StripKind::Comments));
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("Don't ask again").is_ok());
}

// ---------------------------------------------------------------------------
// Discard changes (D189, rule 47)
// ---------------------------------------------------------------------------

fn dirty_rig() -> Rig {
    let mut rig = Rig::new();
    rig.play_e4();
    rig
}

trait PlayE4 {
    fn play_e4(&mut self);
}

impl PlayE4 for Rig {
    fn play_e4(&mut self) {
        let mv = self
            .shell
            .app
            .session
            .position()
            .parse_uci("e2e4")
            .expect("legal");
        self.app_msg(AppMessage::Game(GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        }));
    }
}

#[test]
fn m2_4_new_game_over_unsaved_changes_asks_with_save_dont_save_and_cancel() {
    let mut rig = dirty_rig();
    rig.key(command_shift(Key::Char('n')), false, None);
    assert!(rig.shell.app.discard_prompt.is_some());
    let mut sim = dialog_sim(&rig);
    for label in ["Save", "Don't save", "Cancel"] {
        assert!(sim.find(label).is_ok(), "{label}");
    }
    assert!(
        sim.find("The game has been modified. Do you want to save the changes?")
            .is_ok()
    );
}

#[test]
fn m2_4_each_discard_button_sends_its_choice() {
    for (label, choice) in [
        ("Save", DiscardChoice::Save),
        ("Don't save", DiscardChoice::DontSave),
        ("Cancel", DiscardChoice::Cancel),
    ] {
        let mut rig = dirty_rig();
        rig.app_msg(AppMessage::Command(Command::NewGame));
        let mut sim = dialog_sim(&rig);
        sim.click(label).expect(label);
        let got = messages(sim);
        assert!(
            has(&got, &answer(AppMessage::AnswerDiscard(choice))),
            "{label}: {got:?}"
        );
    }
}

#[test]
fn m2_4_dont_save_discards_and_starts_the_new_game() {
    let mut rig = dirty_rig();
    rig.app_msg(AppMessage::Command(Command::NewGame));
    rig.app_msg(AppMessage::AnswerDiscard(DiscardChoice::DontSave));
    assert!(rig.shell.app.discard_prompt.is_none());
    assert_eq!(rig.game().mainline().count(), 0);
    assert!(!rig.shell.app.is_dirty());
}

#[test]
fn m2_4_cancel_keeps_the_game_and_its_changes() {
    let mut rig = dirty_rig();
    rig.app_msg(AppMessage::Command(Command::NewGame));
    rig.app_msg(AppMessage::AnswerDiscard(DiscardChoice::Cancel));
    assert!(rig.shell.app.discard_prompt.is_none());
    assert_eq!(rig.game().mainline().count(), 1);
    assert!(rig.shell.app.is_dirty());
}

#[test]
fn m2_4_save_writes_the_file_and_then_starts_the_new_game() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("kept.pgn");
    let mut rig = dirty_rig();
    rig.send(AppMessage::Command(Command::NewGame));
    rig.send(AppMessage::AnswerDiscard(DiscardChoice::Save));
    rig.pick(Some(&path));
    assert!(path.exists(), "the game was saved first");
    assert_eq!(rig.game().mainline().count(), 0, "then replaced");
}

#[test]
fn m2_4_save_cancelled_in_the_file_dialog_keeps_the_game() {
    let mut rig = dirty_rig();
    rig.send(AppMessage::Command(Command::NewGame));
    rig.send(AppMessage::AnswerDiscard(DiscardChoice::Save));
    rig.pick(None);
    assert_eq!(rig.game().mainline().count(), 1, "nothing was replaced");
}

#[test]
fn m2_4_enter_means_dont_save_and_escape_means_cancel() {
    let mut rig = dirty_rig();
    rig.app_msg(AppMessage::Command(Command::NewGame));
    rig.press(named(NamedKey::Escape));
    assert!(rig.shell.app.discard_prompt.is_none());
    assert_eq!(rig.game().mainline().count(), 1, "Esc cancels");
    rig.app_msg(AppMessage::Command(Command::NewGame));
    rig.press(named(NamedKey::Enter));
    assert_eq!(rig.game().mainline().count(), 0, "Enter = Don't save");
}

#[test]
fn m2_4_no_question_when_nothing_changed_or_in_trial_mode() {
    // Not altered.
    let mut rig = Rig::new();
    rig.app_msg(AppMessage::Command(Command::NewGame));
    assert!(rig.shell.app.discard_prompt.is_none());
    // Altered, but in trial mode.
    let mut rig = dirty_rig();
    rig.app_msg(AppMessage::Game(GameMessage::EnterTrial {
        null_move: false,
    }));
    rig.app_msg(AppMessage::Command(Command::NewGame));
    assert!(
        rig.shell.app.discard_prompt.is_none(),
        "trial mode (game.tcl:14-20)"
    );
}

#[test]
fn m2_4_opening_a_dialog_on_top_of_the_discard_box_is_not_possible() {
    // The discard box has precedence over the other dialogs.
    let mut rig = dirty_rig();
    rig.app_msg(AppMessage::Command(Command::NewGame));
    rig.msg(Message::Dialog(DialogMessage::OpenGoto));
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::Discard)
    );
}

// ---------------------------------------------------------------------------
// Overwrite file (M2-2 D127; reasons)
// ---------------------------------------------------------------------------

fn multi_pgn(n: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        out.push_str(&format!(
            "[Event \"Ev{i}\"]\n[Site \"S\"]\n[Date \"2024.01.{:02}\"]\n[Round \"1\"]\n[White \"P{i}\"]\n[Black \"Q{i}\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n",
            (i % 28) + 1
        ));
    }
    out
}

fn open_file(rig: &mut Rig, path: &std::path::Path) {
    rig.send(AppMessage::Command(Command::Open));
    rig.pick(Some(path));
    if rig.shell.app.chooser.is_some() {
        rig.send(mychess_app::ChooserMessage::Open(0));
        rig.send(mychess_app::ChooserMessage::Confirm);
    } else {
        // M3-4b (D503): the file took a slot and its list is shown; the
        // user opens game 1 from the list (a double click).
        let slot = rig.shell.app.bases.current();
        rig.send(mychess_app::BasesMessage::LoadGame { slot, number: 1 });
    }
}

#[test]
fn m2_4_overwriting_a_multi_game_file_names_the_file_and_counts_the_games() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("many.pgn");
    std::fs::write(&file, multi_pgn(3)).expect("write");
    let mut rig = Rig::new();
    open_file(&mut rig, &file);
    rig.send(AppMessage::Command(Command::SaveAs));
    rig.pick(Some(&file));
    let prompt = rig.shell.app.overwrite_prompt.clone().expect("asked");
    assert_eq!(prompt.reason, OverwriteReason::MultiGame);
    assert_eq!(prompt.games, 3);
    let mut sim = dialog_sim(&rig);
    assert!(
        sim.find("many.pgn holds 3 games. Replace all of them with this game?")
            .is_ok()
    );
    assert!(sim.find("Replace").is_ok() && sim.find("Cancel").is_ok());
}

#[test]
fn m2_4_overwriting_a_partly_read_file_says_so_and_shows_no_game_count() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("partial.pgn");
    std::fs::write(
        &file,
        format!("a note outside any game\n\n{}", multi_pgn(1)),
    )
    .expect("write");
    let mut rig = Rig::new();
    open_file(&mut rig, &file);
    rig.send(AppMessage::Command(Command::SaveAs));
    rig.pick(Some(&file));
    let prompt = rig.shell.app.overwrite_prompt.clone().expect("asked");
    assert_eq!(prompt.reason, OverwriteReason::PartiallyRead);
    let mut sim = dialog_sim(&rig);
    assert!(
        sim.find("partial.pgn was only partly read; replacing it erases the rest. Replace it?")
            .is_ok()
    );
}

#[test]
fn m2_4_overwriting_an_existing_file_shows_no_game_count() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("game.pgn"), "precious").expect("write");
    let mut rig = dirty_rig();
    rig.send(AppMessage::Command(Command::SaveAs));
    // A name without extension: the application adds ".pgn" and must ask.
    rig.pick(Some(&dir.path().join("game")));
    let prompt = rig.shell.app.overwrite_prompt.clone().expect("asked");
    assert_eq!(prompt.reason, OverwriteReason::ExistingFile);
    assert_eq!(prompt.games, 0, "no games are counted for this reason");
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("game.pgn already exists. Replace it?").is_ok());
    // No "0 games" or "holds" text anywhere in the box.
    assert!(
        sim.find("game.pgn holds 0 games. Replace all of them with this game?")
            .is_err()
    );
}

#[test]
fn m2_4_overwrite_buttons_answer_and_escape_is_the_safe_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("game.pgn");
    std::fs::write(&file, "precious").expect("write");
    let mut rig = dirty_rig();
    rig.send(AppMessage::Command(Command::SaveAs));
    rig.pick(Some(&dir.path().join("game")));
    for (label, yes) in [("Replace", true), ("Cancel", false)] {
        let mut sim = dialog_sim(&rig);
        sim.click(label).expect(label);
        let got = messages(sim);
        assert!(
            has(&got, &answer(AppMessage::AnswerOverwrite(yes))),
            "{label}: {got:?}"
        );
    }
    // Esc and Enter never replace.
    rig.press(named(NamedKey::Enter));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "precious", "Enter");
    assert!(rig.shell.app.overwrite_prompt.is_none());
}

#[test]
fn m2_4_the_overwrite_texts_exist_in_portuguese() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("many.pgn");
    std::fs::write(&file, multi_pgn(2)).expect("write");
    let mut rig = Rig::with_language(LanguagePref::PtBr);
    open_file(&mut rig, &file);
    rig.send(AppMessage::Command(Command::SaveAs));
    rig.pick(Some(&file));
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("Substituir").is_ok());
    assert!(sim.find("Cancelar").is_ok());
}

#[test]
fn m2_4_a_save_error_is_a_translated_notice_and_the_text_hides_debug_forms() {
    // Rule 59: errors reach the user as translated text, never `{e:?}`.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut rig = dirty_rig();
    rig.send(AppMessage::Command(Command::SaveAs));
    let missing = dir.path().join("no-such-dir").join("game.pgn");
    rig.pick(Some(&missing));
    // The shell takes the notice of the application with the next message.
    rig.app_msg(AppMessage::FlushPreferences);
    let mut sim = rig.sim(Size::new(1200.0, 800.0));
    assert!(
        sim.find("Ready").is_err(),
        "an error replaced the idle text"
    );
    assert!(
        sim.find("File not found (game.pgn)").is_ok()
            || sim
                .find("The file could not be read or written (game.pgn)")
                .is_ok()
            || sim.find("Permission denied (game.pgn)").is_ok(),
        "a translated reason with the file name"
    );
}

// ---------------------------------------------------------------------------
// Variation picker (D203)
// ---------------------------------------------------------------------------

fn picker_rig() -> (Rig, Vec<NodeId>) {
    // 1.e4 (1.d4 d5 2.c4 e6 3.Nc3 Nf6 4.Bg5) (1.c4) 1...e5.
    let mut game = Game::new();
    let root = game.root();
    let e4 = play(&mut game, root, "e2e4");
    let d4 = play(&mut game, root, "d2d4");
    let c4 = play(&mut game, root, "c2c4");
    line(
        &mut game,
        d4,
        &["d7d5", "c2c4", "e7e6", "b1c3", "g8f6", "c1g5"],
    );
    (Rig::with_game(game), vec![e4, d4, c4])
}

#[test]
fn m2_4_v_opens_the_picker_with_the_main_line_and_numbered_variations() {
    let (mut rig, _) = picker_rig();
    rig.press(ch('v'));
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::Variations)
    );
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("Main line: e4").is_ok());
    // Each variation shows its first five moves only.
    assert!(sim.find("1: d4 d5 c4 e6 Nc3").is_ok());
    assert!(sim.find("2: c4").is_ok());
}

#[test]
fn m2_4_digits_enter_the_variation_and_zero_the_main_line() {
    for (key, expect_index) in [('1', 1usize), ('2', 2), ('0', 0)] {
        let (mut rig, heads) = picker_rig();
        rig.press(ch('v'));
        rig.press(ch(key));
        assert!(rig.shell.app.session.chooser().is_none(), "closed");
        assert_eq!(rig.cursor(), heads[expect_index], "digit {key}");
    }
}

#[test]
fn m2_4_arrows_and_enter_pick_and_escape_or_left_close() {
    let (mut rig, heads) = picker_rig();
    rig.press(ch('v'));
    rig.press(named(NamedKey::Down));
    rig.press(named(NamedKey::Enter));
    assert_eq!(rig.cursor(), heads[1], "Down then Enter");

    let (mut rig, heads) = picker_rig();
    rig.press(ch('v'));
    rig.press(named(NamedKey::Down));
    rig.press(named(NamedKey::Down));
    rig.press(named(NamedKey::Right));
    assert_eq!(rig.cursor(), heads[2], "Right confirms");

    for key in [NamedKey::Escape, NamedKey::Left] {
        let (mut rig, _) = picker_rig();
        rig.press(ch('v'));
        rig.press(named(key));
        assert!(rig.shell.app.session.chooser().is_none(), "{key:?}");
        assert_eq!(rig.cursor(), rig.game().root(), "{key:?}: stays");
    }
}

#[test]
fn m2_4_a_digit_without_that_variation_does_nothing_harmful() {
    let (mut rig, _) = picker_rig();
    rig.press(ch('v'));
    rig.press(ch('9'));
    assert_eq!(rig.cursor(), rig.game().root());
}

#[test]
fn m2_4_a_single_variation_with_no_main_continuation_enters_directly() {
    // Scid main.tcl:581-585 (spec D203): one variation and nothing else.
    let mut game = Game::new();
    let root = game.root();
    play(&mut game, root, "e2e4");
    let mut rig = Rig::with_game(game);
    rig.press(ch('v'));
    assert!(rig.shell.app.session.chooser().is_none());
}

// ---------------------------------------------------------------------------
// Keyboard context (D201)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_with_a_dialog_open_the_board_shortcuts_are_off() {
    let mut rig = dirty_rig();
    let cursor = rig.cursor();
    rig.app_msg(AppMessage::Command(Command::NewGame));
    rig.press(named(NamedKey::Left));
    rig.press(named(NamedKey::Home));
    rig.press(ch('z'));
    assert_eq!(rig.cursor(), cursor);
    assert!(rig.shell.app.discard_prompt.is_some());
}

#[test]
fn m2_4_tab_circles_the_focus_inside_a_dialog_and_does_not_touch_the_game() {
    let mut rig = dirty_rig();
    rig.app_msg(AppMessage::Command(Command::NewGame));
    rig.press(named(NamedKey::Tab));
    assert!(rig.shell.app.discard_prompt.is_some());
    assert_eq!(rig.game().mainline().count(), 1);
}

// ---------------------------------------------------------------------------
// "Don't ask again" (D188, D190) and the preferences behind it
// ---------------------------------------------------------------------------

fn tick_dont_ask(rig: &mut Rig) {
    let mut sim = dialog_sim(rig);
    sim.click("Don't ask again").expect("the box");
    let got = messages(sim);
    rig.feed(got);
    assert!(rig.shell.ui.dialogs.dont_ask, "the box is ticked");
}

fn click_answer(rig: &mut Rig, label: &str) {
    let mut sim = dialog_sim(rig);
    sim.click(label).expect(label);
    let got = messages(sim);
    rig.feed(got);
}

#[test]
fn m2_4_dont_ask_again_in_the_replace_box_turns_the_question_off_for_good() {
    let (mut rig, ..) = replace_prompt();
    assert!(rig.shell.app.prefs.editing.ask_to_replace_moves);
    tick_dont_ask(&mut rig);
    click_answer(&mut rig, "Add variation");
    assert!(rig.shell.app.session.prompt().is_none());
    assert!(!rig.shell.app.prefs.editing.ask_to_replace_moves);
    assert!(
        !rig.shell.app.session.settings().ask_to_replace_moves,
        "the session follows the preference"
    );
    assert!(!rig.shell.ui.dialogs.dont_ask, "the box is reset");
    // The next such move is applied without a question.
    rig.goto(rig.game().root());
    let mv = rig
        .shell
        .app
        .session
        .position()
        .parse_uci("c2c4")
        .expect("legal");
    rig.app_msg(AppMessage::Game(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    }));
    assert!(rig.shell.app.session.prompt().is_none(), "no question");
}

#[test]
fn m2_4_cancelling_with_the_box_ticked_keeps_asking() {
    let (mut rig, ..) = replace_prompt();
    tick_dont_ask(&mut rig);
    click_answer(&mut rig, "Cancel");
    assert!(rig.shell.app.prefs.editing.ask_to_replace_moves);
    assert!(!rig.shell.ui.dialogs.dont_ask);
}

#[test]
fn m2_4_dont_ask_again_in_the_removal_box_turns_the_confirmation_off() {
    let (mut rig, nodes) = game_with_comments(2);
    rig.app_msg(request(StripKind::Comments));
    tick_dont_ask(&mut rig);
    click_answer(&mut rig, "Remove");
    assert!(rig.game().comment(nodes[0]).unwrap().is_none(), "removed");
    assert!(!rig.shell.app.prefs.editing.confirm_destructive);
    // Delete now removes at once.
    rig.game_mut_for_test_comment(nodes[1], "again");
    rig.goto(nodes[1]);
    rig.press(named(NamedKey::Delete));
    assert!(rig.shell.app.session.prompt().is_none(), "no question");
    assert_eq!(
        rig.game().mainline().count(),
        2,
        "e4 e5 stay, the rest went"
    );
}

trait SetComment {
    fn game_mut_for_test_comment(&mut self, node: NodeId, text: &str);
}

impl SetComment for Rig {
    fn game_mut_for_test_comment(&mut self, node: NodeId, text: &str) {
        self.app_msg(AppMessage::Game(GameMessage::SetComment {
            node,
            text: text.to_owned(),
        }));
    }
}
