//! M2-4 navigation bar, status bar (including the 8 s remark with an
//! injected clock), move entry over the board and keyboard shortcuts (spec
//! D199-D204, rules 61-67 as seen from the front-end, table 3.10).

#![allow(clippy::assert_is_empty, clippy::float_cmp, clippy::unnecessary_wraps)]

mod m2_4_common;

use std::time::{Duration, Instant};

use iced::{Settings, Size, widget};
use iced_test::Simulator;
use m2_4_common::{Rig, ch, command, command_shift, has, line, messages, named, plain, sample};
use mychess_app::{GameMessage, Key, Message as AppMessage, NamedKey, Nav, NavState, Prompt};
use mychess_core::{Game, NodeId};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::keymap::Platform;
use mychess_gui::windows::dialog_ui::{ActiveDialog, DialogMessage, NOTICE_LIFETIME, StatusClock};
use mychess_gui::windows::layout::PaneKind;
use mychess_gui::windows::nav_bar::{self, MOVE_FIELD_ID, entry_edit};
use mychess_gui::{Message, UiMessage};
use rstest::rstest;

fn bar(rig: &Rig) -> Simulator<'static, Message> {
    Simulator::with_size(
        Settings::default(),
        Size::new(1000.0, 120.0),
        nav_bar::command_bar(&rig.shell.app, &rig.shell.i18n, Platform::current()),
    )
}

fn field(rig: &Rig) -> Simulator<'static, Message> {
    Simulator::with_size(
        Settings::default(),
        Size::new(300.0, 40.0),
        nav_bar::move_field(&rig.shell.app, &rig.shell.i18n),
    )
}

/// The id of the button that the M2-4 bar drew with this glyph.
fn id_of(label: &str) -> &'static str {
    match label {
        "«" => nav_bar::NAV_START,
        "‹" => nav_bar::NAV_BACK,
        "›" => nav_bar::NAV_FORWARD,
        "»" => nav_bar::NAV_END,
        "↑" => nav_bar::NAV_EXIT_VARIATION,
        "↓" => nav_bar::NAV_ENTER_VARIATION,
        "+" => nav_bar::NAV_ADD_VARIATION,
        "T" => nav_bar::NAV_TRIAL,
        "⇅" => nav_bar::NAV_FLIP,
        "▶" => nav_bar::NAV_PLAY,
        other => panic!("unknown glyph {other}"),
    }
}

/// Whether pressing the button with this label sends something.
fn press(rig: &Rig, label: &str) -> Vec<Message> {
    let mut sim = bar(rig);
    sim.click(widget::Id::new(id_of(label)))
        .unwrap_or_else(|_| panic!("button {label} exists"));
    messages(sim)
}

fn game_msg(m: GameMessage) -> Message {
    Message::App(AppMessage::Game(m))
}

// ---------------------------------------------------------------------------
// Navigation bar (D202)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_in_an_empty_game_only_trial_flip_and_autoplay_are_on() {
    let rig = Rig::new();
    for label in ["«", "‹", "›", "»", "↑", "↓", "+"] {
        assert!(press(&rig, label).is_empty(), "{label} is off");
    }
    for label in ["T", "⇅", "▶"] {
        assert!(!press(&rig, label).is_empty(), "{label} is on");
    }
}

#[test]
fn m2_4_at_the_start_of_a_game_back_and_start_are_off_forward_and_end_are_on() {
    let s = sample();
    let rig = Rig::with_game(s.game);
    assert!(press(&rig, "«").is_empty());
    assert!(press(&rig, "‹").is_empty());
    assert!(has(
        &press(&rig, "›"),
        &game_msg(GameMessage::Navigate(Nav::Forward(1)))
    ));
    assert!(has(
        &press(&rig, "»"),
        &game_msg(GameMessage::Navigate(Nav::GameEnd))
    ));
    // Exit-variation is off in the main line; add-variation is on because
    // there is a move to hang it from.
    assert!(press(&rig, "↑").is_empty());
    assert!(has(
        &press(&rig, "+"),
        &game_msg(GameMessage::StartVariation)
    ));
}

#[test]
fn m2_4_at_the_end_of_the_main_line_forward_and_end_are_off() {
    let s = sample();
    let bb5 = s.bb5;
    let mut rig = Rig::with_game(s.game);
    rig.goto(bb5);
    assert!(press(&rig, "›").is_empty());
    assert!(press(&rig, "»").is_empty());
    assert!(has(
        &press(&rig, "‹"),
        &game_msg(GameMessage::Navigate(Nav::Back(1)))
    ));
    assert!(has(
        &press(&rig, "«"),
        &game_msg(GameMessage::Navigate(Nav::Start))
    ));
    assert!(press(&rig, "↓").is_empty(), "no variation to enter");
}

#[test]
fn m2_4_inside_a_variation_exit_is_on_and_at_a_branch_enter_is_on() {
    let s = sample();
    let (nc3, e5) = (s.nc3, s.e5);
    let mut rig = Rig::with_game(s.game);
    rig.goto(nc3);
    assert!(has(
        &press(&rig, "↑"),
        &game_msg(GameMessage::Navigate(Nav::ExitVariation))
    ));
    rig.goto(e5);
    assert!(press(&rig, "↑").is_empty());
    assert!(has(
        &press(&rig, "↓"),
        &game_msg(GameMessage::OpenVariationChooser)
    ));
}

#[test]
fn m2_4_trial_flip_and_autoplay_buttons_send_their_messages() {
    let rig = Rig::new();
    assert!(has(
        &press(&rig, "T"),
        &game_msg(GameMessage::ToggleTrial { null_move: false })
    ));
    assert!(has(&press(&rig, "⇅"), &Message::Ui(UiMessage::FlipBoard)));
    assert!(has(
        &press(&rig, "▶"),
        &game_msg(GameMessage::ToggleAutoplay)
    ));
}

#[test]
fn m2_4_the_buttons_follow_nav_state_in_every_position_of_a_game() {
    let s = sample();
    let all: Vec<NodeId> = s.game.descendants(s.game.root()).unwrap().collect();
    let mut rig = Rig::with_game(s.game);
    for node in all {
        rig.goto(node);
        let nav = NavState::of(&rig.shell.app.session);
        assert_eq!(press(&rig, "«").is_empty(), nav.at_start, "start at {node}");
        assert_eq!(press(&rig, "»").is_empty(), nav.at_end, "end at {node}");
        assert_eq!(
            press(&rig, "↑").is_empty(),
            !nav.in_variation,
            "exit at {node}"
        );
        assert_eq!(
            press(&rig, "↓").is_empty(),
            nav.variation_count == 0,
            "enter at {node}"
        );
        assert_eq!(
            press(&rig, "+").is_empty(),
            !nav.can_add_variation,
            "add at {node}"
        );
    }
}

#[test]
fn m2_4_the_tooltips_and_labels_exist_in_both_languages() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for key in [
            "nav-start",
            "nav-back",
            "nav-forward",
            "nav-end",
            "nav-exit-variation",
            "nav-enter-variation",
            "nav-add-variation",
            "nav-trial",
            "nav-flip",
            "nav-autoplay",
            "move-entry-placeholder",
        ] {
            assert!(i18n.has(key), "{key} in {language:?}");
            assert_ne!(i18n.tr(key), key);
        }
    }
}

#[rstest]
#[case("", "e", &["e"])]
#[case("N", "Nf", &["f"])]
#[case("Nf3", "N", &["<", "<"])]
#[case("Nf", "Nf", &[])]
#[case("e", "d", &["clear", "d"])]
#[case("", "", &[])]
fn m2_4_a_field_edit_becomes_the_keystrokes_of_the_buffer(
    #[case] old: &str,
    #[case] new: &str,
    #[case] expected: &[&str],
) {
    let got: Vec<String> = entry_edit(old, new)
        .into_iter()
        .map(|m| match m {
            GameMessage::MoveEntryChar(c) => c.to_string(),
            GameMessage::MoveEntryBackspace => "<".to_owned(),
            GameMessage::MoveEntryClear => "clear".to_owned(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(got, expected);
}

#[test]
fn m2_4_typing_in_the_field_sends_keystrokes_and_enter_commits() {
    let rig = Rig::new();
    let mut sim = field(&rig);
    sim.click(widget::Id::new(MOVE_FIELD_ID)).expect("field");
    sim.typewrite("e");
    sim.tap_key(iced::keyboard::key::Named::Enter);
    let got = messages(sim);
    assert!(
        has(
            &got,
            &Message::Dialog(DialogMessage::MoveEntryKeys(vec![
                GameMessage::MoveEntryChar('e')
            ]))
        ),
        "{got:?}"
    );
    assert!(
        has(&got, &game_msg(GameMessage::MoveEntryCommit)),
        "{got:?}"
    );
}

#[test]
fn m2_4_a_move_typed_in_the_field_is_played_and_shown_with_its_candidates() {
    let mut rig = Rig::new();
    rig.msg(Message::Dialog(DialogMessage::MoveEntryKeys(entry_edit(
        "", "N",
    ))));
    assert_eq!(rig.shell.app.session.move_entry().text(), "N");
    {
        let entry = rig.shell.app.session.move_entry();
        let candidates: Vec<&str> = entry.candidates().iter().map(|c| c.text.as_str()).collect();
        assert_eq!(candidates.len(), 4, "{candidates:?}");
        for expected in ["Na3", "Nc3", "Nf3", "Nh3"] {
            assert!(
                candidates.contains(&expected),
                "{expected} in {candidates:?}"
            );
        }
        let line = mychess_gui::windows::status_bar::entry_text(&rig.shell.i18n, entry);
        assert!(line.starts_with("Enter move: [N] "), "{line}");
        let mut sim = rig.sim(Size::new(1300.0, 800.0));
        assert!(sim.find(line.as_str()).is_ok(), "the bar shows {line:?}");
    }
    rig.msg(Message::Dialog(DialogMessage::MoveEntryKeys(entry_edit(
        "N", "Nf3",
    ))));
    rig.app_msg(AppMessage::Game(GameMessage::MoveEntryCommit));
    assert_eq!(rig.game().mainline().count(), 1);
    assert_eq!(rig.shell.app.session.move_entry().text(), "");
}

// ---------------------------------------------------------------------------
// Typing over the board (D199, rules 61-66)
// ---------------------------------------------------------------------------

fn type_over_board(rig: &mut Rig, text: &str) {
    for c in text.chars() {
        rig.key(plain(Key::Char(c)), false, Some(c));
    }
}

#[test]
fn m2_4_a_move_typed_over_the_board_is_played_as_soon_as_it_is_unambiguous() {
    // Rule 64: with exactly one candidate equal to the buffer the move is
    // played at once (the buffer equals the candidate ignoring case).
    let mut rig = Rig::new();
    type_over_board(&mut rig, "e");
    assert_eq!(rig.shell.app.session.move_entry().text(), "e");
    assert_eq!(rig.game().mainline().count(), 0);
    type_over_board(&mut rig, "4");
    assert_eq!(rig.game().mainline().count(), 1, "e4 played");
    assert_eq!(
        rig.shell.app.session.move_entry().text(),
        "",
        "buffer emptied"
    );
}

#[test]
fn m2_4_enter_plays_a_buffer_that_has_one_candidate_but_is_not_complete() {
    // "Nf" has one candidate (Nf3): Enter plays it (rule 65).
    let mut rig = Rig::new();
    type_over_board(&mut rig, "Nf");
    assert_eq!(rig.game().mainline().count(), 0);
    rig.press(named(NamedKey::Enter));
    assert_eq!(rig.game().mainline().count(), 1);
}

#[test]
fn m2_4_piece_letters_typed_in_lower_case_become_upper_case() {
    let mut rig = Rig::new();
    type_over_board(&mut rig, "n");
    assert_eq!(rig.shell.app.session.move_entry().text(), "N");
    type_over_board(&mut rig, "f3");
    assert_eq!(rig.game().mainline().count(), 1, "Nf3 played");
}

#[test]
fn m2_4_a_key_without_candidates_is_rejected_and_the_user_is_told() {
    let mut rig = Rig::new();
    type_over_board(&mut rig, "x");
    assert_eq!(rig.shell.app.session.move_entry().text(), "", "rejected");
    let mut sim = rig.sim(Size::new(1300.0, 800.0));
    assert!(
        sim.find("No legal move starts with \"x\"").is_ok(),
        "rule 63: status-move-entry-no-match"
    );
}

#[test]
fn m2_4_escape_clears_the_typed_move() {
    let mut rig = Rig::new();
    type_over_board(&mut rig, "e");
    assert_eq!(rig.shell.app.session.move_entry().text(), "e");
    rig.press(named(NamedKey::Escape));
    assert_eq!(rig.shell.app.session.move_entry().text(), "");
}

#[test]
fn m2_4_backspace_removes_the_last_typed_character() {
    let mut rig = Rig::new();
    type_over_board(&mut rig, "Nf");
    rig.press(named(NamedKey::Backspace));
    assert_eq!(rig.shell.app.session.move_entry().text(), "N");
}

#[test]
fn m2_4_keys_typed_in_a_text_field_never_reach_the_move_entry() {
    let mut rig = Rig::new();
    for c in "e4".chars() {
        rig.key(plain(Key::Char(c)), true, Some(c));
    }
    assert_eq!(rig.shell.app.session.move_entry().text(), "");
}

#[test]
fn m2_4_the_castling_shorthand_oo_is_accepted_when_legal() {
    let mut game = Game::new();
    let root = game.root();
    let nodes = line(
        &mut game,
        root,
        &["e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6"],
    );
    let mut rig = Rig::with_game(game);
    rig.goto(nodes[5]);
    type_over_board(&mut rig, "oo");
    rig.press(named(NamedKey::Enter));
    assert_eq!(rig.game().mainline().count(), 7, "O-O played");
}

// ---------------------------------------------------------------------------
// Status bar (D204)
// ---------------------------------------------------------------------------

fn tick(rig: &mut Rig, at: Instant) {
    rig.msg(Message::Dialog(DialogMessage::StatusTick(at)));
}

fn status_has(rig: &Rig, text: &str) -> bool {
    let mut sim = rig.sim(Size::new(1300.0, 800.0));
    sim.find(text).is_ok()
}

/// A rig showing a remark of the session ("That is not a variation").
fn rig_with_remark() -> Rig {
    let s = sample();
    let e4 = s.e4;
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    remark(&mut rig);
    rig
}

/// Asks to promote a main-line move: "That is not a variation".
fn remark(rig: &mut Rig) {
    let e4 = rig.game().children(rig.game().root()).expect("children")[0];
    rig.app_msg(AppMessage::Game(GameMessage::PromoteVariation(
        mychess_app::VariationTarget::Node(e4),
    )));
}

#[test]
fn m2_4_the_idle_bar_shows_ready_the_side_to_move_and_the_modified_mark() {
    let mut rig = Rig::new();
    assert!(status_has(&rig, "Ready"));
    assert!(status_has(&rig, "White to move (1)"));
    assert!(!status_has(&rig, "● Modified"));
    let mv = rig.shell.app.session.position().parse_uci("e2e4").unwrap();
    rig.app_msg(AppMessage::Game(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    }));
    assert!(status_has(&rig, "Black to move (1)"));
    assert!(status_has(&rig, "● Modified"));
}

#[test]
fn m2_4_a_remark_replaces_ready_and_goes_away_after_eight_seconds() {
    assert_eq!(NOTICE_LIFETIME, Duration::from_secs(8));
    let mut rig = rig_with_remark();
    assert!(status_has(&rig, "That is not a variation"));
    assert!(!status_has(&rig, "Ready"));
    let t0 = Instant::now();
    tick(&mut rig, t0);
    tick(&mut rig, t0 + Duration::from_millis(7_999));
    assert!(status_has(&rig, "That is not a variation"), "7.999 s");
    tick(&mut rig, t0 + Duration::from_secs(8));
    assert!(!status_has(&rig, "That is not a variation"), "8 s");
    assert!(status_has(&rig, "Ready"));
}

#[test]
fn m2_4_the_clock_is_injected_and_counts_from_the_first_tick() {
    let mut clock = StatusClock::default();
    let t0 = Instant::now();
    clock.tick(t0, true);
    assert!(!clock.expired());
    clock.tick(
        (t0 + Duration::from_secs(8))
            .checked_sub(Duration::from_nanos(1))
            .expect("an instant just before"),
        true,
    );
    assert!(!clock.expired());
    clock.tick(t0 + Duration::from_secs(8), true);
    assert!(clock.expired());
    clock.reset();
    assert!(!clock.expired());
    // Ticks with nothing on show keep the clock reset.
    clock.tick(t0, false);
    clock.tick(t0 + Duration::from_secs(60), false);
    assert!(!clock.expired());
}

#[test]
fn m2_4_the_next_action_of_the_user_clears_the_remark() {
    let mut rig = rig_with_remark();
    assert!(status_has(&rig, "That is not a variation"));
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::Forward(1))));
    assert!(!status_has(&rig, "That is not a variation"));
    assert!(status_has(&rig, "Ready"));
}

#[test]
fn m2_4_a_new_remark_after_an_expired_one_is_shown_for_eight_seconds_again() {
    let mut rig = rig_with_remark();
    let t0 = Instant::now();
    tick(&mut rig, t0);
    tick(&mut rig, t0 + Duration::from_secs(9));
    assert!(status_has(&rig, "Ready"));
    remark(&mut rig);
    assert!(status_has(&rig, "That is not a variation"), "shown again");
    tick(&mut rig, t0 + Duration::from_secs(10));
    tick(&mut rig, t0 + Duration::from_secs(17));
    assert!(status_has(&rig, "That is not a variation"));
    tick(&mut rig, t0 + Duration::from_secs(18));
    assert!(status_has(&rig, "Ready"));
}

#[test]
fn m2_4_the_expiry_timer_runs_only_while_a_remark_is_on_show() {
    let mut rig = Rig::with_game(sample().game);
    assert!(!rig.shell.has_temporary_notice(), "idle: no timer");
    remark(&mut rig);
    assert!(rig.shell.has_temporary_notice());
    let t0 = Instant::now();
    tick(&mut rig, t0);
    tick(&mut rig, t0 + Duration::from_secs(9));
    assert!(
        !rig.shell.has_temporary_notice(),
        "once the remark expired nothing needs the timer (spec 4.2: no idle timers)"
    );
}

#[test]
fn m2_4_the_typed_move_has_priority_over_a_remark() {
    let mut rig = rig_with_remark();
    type_over_board(&mut rig, "e");
    let line = mychess_gui::windows::status_bar::entry_text(
        &rig.shell.i18n,
        rig.shell.app.session.move_entry(),
    );
    assert!(line.starts_with("Enter move: [e]"), "{line}");
    assert!(status_has(&rig, &line), "the buffer wins");
    assert!(!status_has(&rig, "That is not a variation"));
}

#[test]
fn m2_4_remarks_are_translated() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    rig.set_game(sample().game);
    // Leaving a variation while in none is silent by design (M2-1, rule 14);
    // promoting "the current variation" on the main line is a remark.
    remark(&mut rig);
    assert!(
        status_has(&rig, "Isso não é uma variante"),
        "uses \"variante\""
    );
}

// ---------------------------------------------------------------------------
// Shortcuts (D200-D201, table 3.10)
// ---------------------------------------------------------------------------

fn dirty_game_rig() -> (Rig, Vec<NodeId>) {
    let mut game = Game::new();
    let root = game.root();
    let nodes = line(&mut game, root, &["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"]);
    (Rig::with_game(game), nodes)
}

#[test]
fn m2_4_the_arrows_home_and_end_navigate_with_the_board_focused() {
    let (mut rig, nodes) = dirty_game_rig();
    rig.press(named(NamedKey::Right));
    assert_eq!(rig.cursor(), nodes[0]);
    rig.press(named(NamedKey::End));
    assert_eq!(rig.cursor(), nodes[4]);
    rig.press(named(NamedKey::Left));
    assert_eq!(rig.cursor(), nodes[3]);
    rig.press(named(NamedKey::Home));
    assert_eq!(rig.cursor(), rig.game().root());
}

#[test]
fn m2_4_up_and_down_move_ten_plies_and_up_inside_a_variation_leaves_it() {
    let mut game = Game::new();
    let root = game.root();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let mut at = root;
    let mut nodes = Vec::new();
    for ply in 0..24 {
        at = m2_4_common::play(&mut game, at, cycle[ply % 4]);
        nodes.push(at);
    }
    let mut rig = Rig::with_game(game);
    rig.goto(nodes[19]);
    rig.press(named(NamedKey::Up));
    assert_eq!(rig.cursor(), nodes[9], "10 plies back");
    rig.press(named(NamedKey::Down));
    assert_eq!(rig.cursor(), nodes[19], "10 plies forward");

    // Inside a variation Up leaves it.
    let s = sample();
    let nf6 = s.nf6;
    let mut rig = Rig::with_game(s.game);
    rig.goto(nf6);
    rig.press(named(NamedKey::Up));
    assert!(!NavState::of(&rig.shell.app.session).in_variation);
}

#[test]
fn m2_4_home_and_end_inside_a_variation_go_to_its_ends_first() {
    // Table 3.10: "com variante: vai ao início/fim da variante; repetido, ao
    // da partida".
    let s = sample();
    let (nc3, nf6, bb5) = (s.nc3, s.nf6, s.bb5);
    let mut rig = Rig::with_game(s.game);
    rig.goto(nc3);
    rig.press(named(NamedKey::End));
    assert_eq!(rig.cursor(), nf6, "end of the variation");
    rig.press(named(NamedKey::End));
    assert_eq!(rig.cursor(), bb5, "pressed again: end of the game");
}

#[test]
fn m2_4_z_and_v_work_without_modifiers_only_on_the_board() {
    let s = sample();
    let (nc3, e5) = (s.nc3, s.e5);
    let mut rig = Rig::with_game(s.game);
    rig.goto(nc3);
    rig.key(plain(Key::Char('z')), true, Some('z'));
    assert_eq!(rig.cursor(), nc3, "text focused: z is text");
    rig.press(ch('z'));
    assert_ne!(rig.cursor(), nc3, "board focused: z exits the variation");
    rig.goto(e5);
    rig.key(plain(Key::Char('v')), true, Some('v'));
    assert!(rig.shell.app.session.chooser().is_none());
    rig.press(ch('v'));
    assert!(rig.shell.app.session.chooser().is_some());
}

#[test]
fn m2_4_plain_keys_do_not_fire_with_a_text_field_focused() {
    let (mut rig, nodes) = dirty_game_rig();
    rig.goto(nodes[2]);
    for key in [
        Key::Named(NamedKey::Left),
        Key::Named(NamedKey::Right),
        Key::Named(NamedKey::Home),
        Key::Named(NamedKey::End),
        Key::Named(NamedKey::Up),
        Key::Named(NamedKey::Down),
        Key::Named(NamedKey::Delete),
        Key::Named(NamedKey::Backspace),
        Key::Named(NamedKey::Escape),
        Key::Char('v'),
        Key::Char('z'),
    ] {
        rig.key(plain(key), true, None);
        assert_eq!(rig.cursor(), nodes[2], "{key:?}");
        assert!(rig.shell.app.session.prompt().is_none(), "{key:?}");
        assert!(rig.shell.app.session.chooser().is_none(), "{key:?}");
    }
}

#[test]
fn m2_4_function_keys_work_even_with_a_text_field_focused() {
    let mut rig = Rig::new();
    assert!(!rig.shell.app.manual().is_open());
    rig.key(plain(Key::Named(NamedKey::F(1))), true, None);
    assert!(
        rig.shell.app.manual().is_open(),
        "F1 (Action::Help) is Always-scope and opens the manual"
    );
    assert!(!rig.shell.ui.show_about, "F1 no longer opens the About box");
}

#[test]
fn m2_4_command_z_with_a_text_field_focused_is_text_undo_not_game_undo() {
    let (mut rig, nodes) = dirty_game_rig();
    let count = rig.game().mainline().count();
    // An undoable edit.
    rig.app_msg(AppMessage::Game(GameMessage::SetComment {
        node: nodes[0],
        text: "x".to_owned(),
    }));
    rig.key(command(Key::Char('z')), true, None);
    assert_eq!(
        rig.game().comment(nodes[0]).unwrap(),
        Some("x"),
        "not undone"
    );
    rig.key(command(Key::Char('z')), false, None);
    assert_eq!(rig.game().comment(nodes[0]).unwrap(), None, "undone");
    assert_eq!(rig.game().mainline().count(), count);
    // Redo with Cmd+Y.
    rig.key(command(Key::Char('y')), false, None);
    assert_eq!(rig.game().comment(nodes[0]).unwrap(), Some("x"));
}

#[test]
fn m2_4_command_shift_z_is_redo_too() {
    let (mut rig, nodes) = dirty_game_rig();
    rig.app_msg(AppMessage::Game(GameMessage::SetComment {
        node: nodes[0],
        text: "x".to_owned(),
    }));
    rig.key(command(Key::Char('z')), false, None);
    rig.key(command_shift(Key::Char('z')), false, None);
    assert_eq!(
        rig.game().comment(nodes[0]).unwrap(),
        Some("x"),
        "table 3.10: Cmd+Shift+Z redoes"
    );
}

#[test]
fn m2_4_the_command_shortcuts_open_their_dialogs_and_panels() {
    let (mut rig, _) = dirty_game_rig();
    rig.key(command_shift(Key::Char('b')), false, None);
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::Setup)
    );
    rig.press(named(NamedKey::Escape));
    rig.key(command_shift(Key::Char('i')), false, None);
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::Import)
    );
    rig.press(named(NamedKey::Escape));
    rig.key(command(Key::Char('g')), false, None);
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::Goto)
    );
}

#[test]
fn m2_4_command_shift_h_opens_the_game_information_dialog() {
    // Table 3.10: "Cabeçalho da partida: Cmd+Shift+H".
    let (mut rig, _) = dirty_game_rig();
    rig.key(command_shift(Key::Char('h')), false, None);
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::Info)
    );
}

#[test]
fn m2_4_command_t_toggles_trial_mode_and_command_a_adds_a_variation() {
    let (mut rig, nodes) = dirty_game_rig();
    rig.goto(nodes[1]);
    rig.key(command(Key::Char('t')), false, None);
    assert!(rig.shell.app.session.in_trial());
    rig.key(command(Key::Char('t')), false, None);
    assert!(!rig.shell.app.session.in_trial());
    rig.key(command(Key::Char('a')), false, None);
    assert!(rig.shell.app.session.pending_variation().is_some());
}

#[test]
fn m2_4_command_a_with_a_text_field_focused_is_select_all_not_add_variation() {
    let (mut rig, nodes) = dirty_game_rig();
    rig.goto(nodes[1]);
    rig.key(command(Key::Char('a')), true, None);
    assert!(rig.shell.app.session.pending_variation().is_none());
}

#[test]
fn m2_4_command_i_and_command_p_toggle_the_panes() {
    let mut rig = Rig::new();
    // Information is a tab of the notation group (spec M2-6): the
    // shortcut selects it and, pressed again, goes back to the move list.
    assert!(rig.shell.ui.is_visible(PaneKind::Notation));
    rig.key(command(Key::Char('i')), false, None);
    assert!(
        rig.shell.ui.is_visible(PaneKind::Info),
        "the info tab opened"
    );
    rig.key(command(Key::Char('i')), false, None);
    assert!(rig.shell.ui.is_visible(PaneKind::Notation));
    assert!(rig.shell.ui.is_open(PaneKind::Info), "the tab stays open");
}

#[test]
fn m2_4_delete_shortcut_asks_before_truncating() {
    let (mut rig, nodes) = dirty_game_rig();
    rig.goto(nodes[1]);
    rig.press(named(NamedKey::Delete));
    assert!(matches!(
        rig.shell.app.session.prompt(),
        Some(Prompt::ConfirmStrip { .. })
    ));
}

#[test]
fn m2_4_ctrl_is_the_command_key_here_and_logo_is_not_on_this_platform() {
    // `Modifiers::command()` is Ctrl on Linux/Windows and Cmd on macOS.
    use iced::keyboard::{self, Key as IcedKey};
    use mychess_gui::keymap::key_input;
    let ctrl = key_input(&IcedKey::Character("g".into()), keyboard::Modifiers::CTRL).unwrap();
    let logo = key_input(&IcedKey::Character("g".into()), keyboard::Modifiers::LOGO).unwrap();
    if cfg!(target_os = "macos") {
        assert!(!ctrl.modifiers.command && logo.modifiers.command);
    } else {
        assert!(ctrl.modifiers.command && !logo.modifiers.command);
    }
}

#[test]
fn m2_4_a_key_does_not_fire_a_dialog_shortcut_when_another_dialog_has_the_keyboard() {
    let (mut rig, nodes) = dirty_game_rig();
    rig.goto(nodes[4]);
    rig.app_msg(AppMessage::Game(GameMessage::PlayMove {
        mv: rig.shell.app.session.position().parse_uci("a7a6").unwrap(),
        options: mychess_app::PlayOptions::default(),
    }));
    rig.app_msg(AppMessage::Command(mychess_app::Command::NewGame));
    assert!(rig.shell.app.discard_prompt.is_some());
    rig.key(command(Key::Char('g')), false, None);
    assert!(
        rig.shell.ui.dialogs.goto.is_none(),
        "the discard box owns the keyboard"
    );
}
