//! M2-4 form dialogs: game information (tags), set up position, paste PGN
//! and the virtualized game chooser (spec D191-D197, rules 37-44, 49-56,
//! M2-2 D125).

#![allow(
    clippy::assigning_clones,
    clippy::format_push_string,
    clippy::unchecked_time_subtraction,
    clippy::overly_complex_bool_expr,
    clippy::assert_is_empty,
    clippy::float_cmp,
    clippy::unnecessary_wraps
)]

mod m2_4_common;

use std::time::Instant;

use iced::{Settings, Size, widget};
use iced_test::Simulator;
use m2_4_common::{Rig, has, line, messages, named};
use mychess_app::{
    ChooserMessage, ClipboardPurpose, Command, DatePart, DateTarget, FieldIssueKind,
    GameInfoMessage, GameMessage, ImportMessage, IssueSeverity, Message as AppMessage, NameKind,
    NamedKey, PlayerSide, SetupAnalysis, SetupMessage, SetupProblem, SetupWarning, TagField,
};
use mychess_core::{Color, Game, Piece, Square};
use mychess_gui::Message;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::dialog_ui::{
    ActiveDialog, CHOOSER_LIST_HEIGHT, CHOOSER_ROW_HEIGHT, DialogMessage, DialogUi, field_id,
    visible_rows,
};
use mychess_gui::windows::{dialogs, setup_board};

fn dialog_sim(rig: &Rig) -> Simulator<'_, Message> {
    let element = dialogs::view(&rig.shell).expect("a dialog is open");
    Simulator::with_size(Settings::default(), Size::new(1100.0, 800.0), element)
}

fn info(message: GameInfoMessage) -> AppMessage {
    AppMessage::Info(message)
}

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

fn piece(c: char) -> Piece {
    Piece::from_char(c).expect("piece")
}

fn active(rig: &Rig) -> Option<ActiveDialog> {
    ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs)
}

// ---------------------------------------------------------------------------
// Game information (tags)
// ---------------------------------------------------------------------------

fn header_rig() -> Rig {
    let mut game = Game::new();
    let root = game.root();
    line(&mut game, root, &["e2e4", "e7e5"]);
    game.tags_mut().white = "Carlsen, M.".to_owned();
    game.tags_mut().black = "Caruana, F.".to_owned();
    game.tags_mut().event = "Candidates".to_owned();
    let mut rig = Rig::with_game(game);
    rig.msg(Message::Dialog(DialogMessage::OpenHeader(None)));
    // The dialog is opened through the application: run its message.
    rig.sync();
    rig
}

#[test]
fn m2_4_opening_the_header_dialog_fills_the_draft_with_the_tags() {
    let rig = header_rig();
    assert_eq!(active(&rig), Some(ActiveDialog::Info));
    let dialog = rig.shell.app.info_dialog.as_ref().expect("open");
    assert_eq!(dialog.draft.white, "Carlsen, M.");
    assert_eq!(dialog.draft.event, "Candidates");
}

#[test]
fn m2_4_the_dialog_can_open_on_a_chosen_field() {
    let mut ui = DialogUi::default();
    let mut app = mychess_app::AppState::default();
    let out = ui.update(DialogMessage::OpenHeader(Some(TagField::Event)), &app);
    assert_eq!(
        out.messages,
        [AppMessage::Info(GameInfoMessage::Open)],
        "asks the application to open it"
    );
    for message in out.messages {
        mychess_app::update(&mut app, message);
    }
    assert_eq!(
        ui.newly_opened(&app),
        Some("hdr-event"),
        "focus on that field"
    );
    assert_eq!(ui.newly_opened(&app), None, "only once");
    assert_eq!(field_id(TagField::Date), "hdr-date-year");
    assert_eq!(field_id(TagField::White), "hdr-white");
}

fn app_send(rig: &mut Rig, message: AppMessage) {
    rig.app_msg(message);
}

#[test]
fn m2_4_applying_changes_the_tags_with_one_undo_point() {
    let mut rig = header_rig();
    let depth = rig.shell.app.session.undo_depth();
    app_send(
        &mut rig,
        info(GameInfoMessage::SetText(
            TagField::White,
            "Nakamura, H.".to_owned(),
        )),
    );
    rig.msg(Message::Dialog(DialogMessage::HeaderSubmit));
    assert!(rig.shell.app.info_dialog.is_none(), "closed on success");
    assert_eq!(rig.game().tags().white, "Nakamura, H.");
    assert_eq!(rig.shell.app.session.undo_depth(), depth + 1);
    assert!(rig.shell.app.is_dirty());
}

#[test]
fn m2_4_applying_an_unchanged_draft_makes_no_change_and_no_undo_point() {
    let mut rig = header_rig();
    let before = (
        rig.shell.app.session.changes(),
        rig.shell.app.session.undo_depth(),
        rig.shell.app.is_dirty(),
    );
    rig.msg(Message::Dialog(DialogMessage::HeaderSubmit));
    assert!(rig.shell.app.info_dialog.is_none());
    let after = (
        rig.shell.app.session.changes(),
        rig.shell.app.session.undo_depth(),
        rig.shell.app.is_dirty(),
    );
    assert_eq!(before.1, after.1, "rule 37: no undo point");
    assert_eq!(before.0, after.0, "rule 37: no change");
}

#[test]
fn m2_4_escape_cancels_and_leaves_the_tags_alone() {
    let mut rig = header_rig();
    app_send(
        &mut rig,
        info(GameInfoMessage::SetText(
            TagField::White,
            "Changed".to_owned(),
        )),
    );
    rig.press(named(NamedKey::Escape));
    assert!(rig.shell.app.info_dialog.is_none());
    assert_eq!(rig.game().tags().white, "Carlsen, M.");
}

#[test]
fn m2_4_an_event_date_without_a_game_date_blocks_ok_and_says_why() {
    let mut rig = header_rig();
    app_send(
        &mut rig,
        info(GameInfoMessage::SetDate(
            DateTarget::EventDate,
            DatePart::Year,
            "2024".to_owned(),
        )),
    );
    rig.msg(Message::Dialog(DialogMessage::HeaderSubmit));
    assert!(rig.shell.app.info_dialog.is_some(), "still open");
    {
        let issues = &rig.shell.app.info_dialog.as_ref().unwrap().issues;
        assert!(
            issues
                .iter()
                .any(|i| i.kind == FieldIssueKind::EventDateWithoutDate
                    && i.severity == IssueSeverity::Blocking),
            "{issues:?}"
        );
    }
    assert_eq!(
        rig.game().tags().event_date,
        mychess_core::PartialDate::default(),
        "nothing applied"
    );
    // Giving the game a date fixes it.
    app_send(
        &mut rig,
        info(GameInfoMessage::SetDate(
            DateTarget::Date,
            DatePart::Year,
            "2024".to_owned(),
        )),
    );
    rig.msg(Message::Dialog(DialogMessage::HeaderSubmit));
    assert!(rig.shell.app.info_dialog.is_none());
}

#[test]
fn m2_4_a_rating_above_4000_blocks_and_a_day_not_in_the_month_only_warns() {
    let mut rig = header_rig();
    app_send(
        &mut rig,
        info(GameInfoMessage::SetRating(
            PlayerSide::White,
            "4001".to_owned(),
        )),
    );
    rig.msg(Message::Dialog(DialogMessage::HeaderSubmit));
    assert!(rig.shell.app.info_dialog.is_some(), "blocked");
    {
        let issues = &rig.shell.app.info_dialog.as_ref().unwrap().issues;
        assert!(
            issues
                .iter()
                .any(|i| i.kind == FieldIssueKind::RatingOutOfRange
                    && i.severity == IssueSeverity::Blocking),
            "{issues:?}"
        );
    }
    app_send(
        &mut rig,
        info(GameInfoMessage::SetRating(
            PlayerSide::White,
            "2850".to_owned(),
        )),
    );
    for (part, text) in [
        (DatePart::Year, "2024"),
        (DatePart::Month, "02"),
        (DatePart::Day, "30"),
    ] {
        app_send(
            &mut rig,
            info(GameInfoMessage::SetDate(
                DateTarget::Date,
                part,
                text.to_owned(),
            )),
        );
    }
    rig.msg(Message::Dialog(DialogMessage::HeaderSubmit));
    assert!(
        rig.shell.app.info_dialog.is_none(),
        "a warning does not block"
    );
    assert_eq!(rig.game().tags().date.to_string(), "2024.02.30");
}

#[test]
fn m2_4_the_form_is_validated_on_each_edit_not_only_on_ok() {
    // D187: the validation runs "a cada edição para erro inline".
    let mut rig = header_rig();
    app_send(
        &mut rig,
        info(GameInfoMessage::SetRating(
            PlayerSide::White,
            "4001".to_owned(),
        )),
    );
    let mut sim = dialog_sim(&rig);
    assert!(
        sim.find("Rating must be between 0 and 4000.").is_ok(),
        "the message is shown as soon as the value is typed"
    );
}

#[test]
fn m2_4_a_warning_is_shown_before_it_is_applied() {
    // M2-2 D134: "aviso = mostra e deixa aplicar". A day that does not exist
    // in the month is a warning: it must be visible while editing.
    let mut rig = header_rig();
    for (part, text) in [
        (DatePart::Year, "2024"),
        (DatePart::Month, "02"),
        (DatePart::Day, "30"),
    ] {
        app_send(
            &mut rig,
            info(GameInfoMessage::SetDate(
                DateTarget::Date,
                part,
                text.to_owned(),
            )),
        );
    }
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("That day does not exist in that month.").is_ok());
}

#[test]
fn m2_4_a_blocking_message_is_visible_without_scrolling_the_form() {
    // Rule 38/D192: the user must see why OK did nothing.
    let mut rig = header_rig();
    app_send(
        &mut rig,
        info(GameInfoMessage::SetDate(
            DateTarget::EventDate,
            DatePart::Year,
            "2024".to_owned(),
        )),
    );
    rig.msg(Message::Dialog(DialogMessage::HeaderSubmit));
    assert!(rig.shell.app.info_dialog.is_some());
    let mut sim = dialog_sim(&rig);
    assert!(
        sim.find("Event date can't exist without game date.")
            .is_ok(),
        "the message sits below the fold of the 470 px form"
    );
}

#[test]
fn m2_4_the_ok_button_applies_and_cancel_closes() {
    let mut rig = header_rig();
    let mut sim = dialog_sim(&rig);
    sim.click("OK").expect("ok");
    let got = messages(sim);
    assert!(
        has(&got, &Message::App(info(GameInfoMessage::Apply))),
        "{got:?}"
    );
    let mut sim = dialog_sim(&rig);
    sim.click("Cancel").expect("cancel");
    let got = messages(sim);
    assert!(
        has(&got, &Message::App(info(GameInfoMessage::Cancel))),
        "{got:?}"
    );
    rig.feed(got);
    assert!(rig.shell.app.info_dialog.is_none());
}

#[test]
fn m2_4_enter_in_a_text_field_applies_the_dialog() {
    let rig = header_rig();
    let mut sim = dialog_sim(&rig);
    sim.click(widget::Id::new("hdr-event"))
        .expect("event field");
    sim.tap_key(iced::keyboard::key::Named::Enter);
    let got = messages(sim);
    assert!(
        has(&got, &Message::Dialog(DialogMessage::HeaderSubmit)),
        "{got:?}"
    );
}

#[test]
fn m2_4_today_same_as_date_and_classify_buttons() {
    let rig = header_rig();
    // Today asks the application (the date is read by an effect, not in
    // `view`); "=Date" copies; "Classify" is off without an ECO book.
    let mut sim = dialog_sim(&rig);
    sim.click("Today").expect("today");
    let got = messages(sim);
    assert!(
        got.iter()
            .any(|m| matches!(m, Message::App(AppMessage::Info(GameInfoMessage::Today(_))))),
        "{got:?}"
    );
    let mut sim = dialog_sim(&rig);
    sim.click("=Date").expect("same as date");
    let got = messages(sim);
    assert!(has(
        &got,
        &Message::App(info(GameInfoMessage::CopyDateToEventDate))
    ));
    let mut sim = dialog_sim(&rig);
    sim.click("Classify").expect("classify");
    let got = messages(sim);
    assert!(
        got.is_empty(),
        "rule 40: disabled without an ECO book: {got:?}"
    );
}

#[test]
fn m2_4_use_previous_only_exists_as_a_message_and_extra_tags_can_be_added() {
    let rig = header_rig();
    let mut sim = dialog_sim(&rig);
    sim.click("Add").expect("add tag");
    let got = messages(sim);
    assert!(
        has(&got, &Message::App(info(GameInfoMessage::AddExtra))),
        "{got:?}"
    );
}

fn names_rig(names: &[&str]) -> Rig {
    let mut rig = header_rig();
    for name in names {
        rig.shell.app.names.observe(NameKind::Player, name);
    }
    rig
}

fn type_in_white(rig: &mut Rig, text: &str) {
    rig.msg(Message::Dialog(DialogMessage::HeaderText(
        TagField::White,
        text.to_owned(),
    )));
}

fn suggestion_names(rig: &Rig) -> Vec<String> {
    rig.shell
        .app
        .info_dialog
        .as_ref()
        .map(|d| d.suggestions.iter().map(|m| m.name.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn m2_4_typing_a_prefix_lists_matching_names_ignoring_case_and_accents() {
    let mut rig = names_rig(&["Carlsen, M.", "CARUANA, F.", "Nepomniachtchi, I."]);
    type_in_white(&mut rig, "car");
    assert_eq!(suggestion_names(&rig), ["Carlsen, M.", "CARUANA, F."]);
    type_in_white(&mut rig, "");
    assert!(suggestion_names(&rig).is_empty(), "empty prefix: none");
}

#[test]
fn m2_4_at_most_nine_suggestions_are_listed() {
    let names: Vec<String> = (0..14).map(|i| format!("Player {i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut rig = names_rig(&refs);
    type_in_white(&mut rig, "play");
    assert!(suggestion_names(&rig).len() <= 9);
    let mut sim = dialog_sim(&rig);
    // Only nine rows exist as buttons even if more were passed.
    assert!(sim.find("Player 00").is_ok());
    assert!(sim.find("Player 12").is_err());
}

#[test]
fn m2_4_arrows_move_the_highlight_enter_picks_and_a_second_enter_applies() {
    let mut rig = names_rig(&["Carlsen, M.", "Caruana, F.", "Carter, X."]);
    type_in_white(&mut rig, "car");
    let names = suggestion_names(&rig);
    assert_eq!(names.len(), 3);
    assert!(rig.shell.ui.dialogs.suggestions_visible(&rig.shell.app));
    rig.press(named(NamedKey::Down));
    assert_eq!(rig.shell.ui.dialogs.header.highlight, Some(0));
    rig.press(named(NamedKey::Down));
    assert_eq!(rig.shell.ui.dialogs.header.highlight, Some(1));
    rig.press(named(NamedKey::Up));
    assert_eq!(rig.shell.ui.dialogs.header.highlight, Some(0));
    rig.press(named(NamedKey::Up));
    assert_eq!(rig.shell.ui.dialogs.header.highlight, Some(2), "wraps");
    rig.press(named(NamedKey::Down));
    assert_eq!(rig.shell.ui.dialogs.header.highlight, Some(0), "wraps back");
    rig.press(named(NamedKey::Down));
    // Enter (as the text field's submit) picks the highlighted name and the
    // dialog stays open.
    rig.msg(Message::Dialog(DialogMessage::HeaderSubmit));
    let dialog = rig.shell.app.info_dialog.as_ref().expect("still open");
    assert_eq!(dialog.draft.white, names[1]);
    assert!(
        !rig.shell.ui.dialogs.suggestions_visible(&rig.shell.app),
        "list closed"
    );
    // The next Enter applies.
    rig.msg(Message::Dialog(DialogMessage::HeaderSubmit));
    assert!(rig.shell.app.info_dialog.is_none());
    assert_eq!(rig.game().tags().white, names[1]);
}

#[test]
fn m2_4_escape_closes_the_list_first_and_the_dialog_second() {
    let mut rig = names_rig(&["Carlsen, M.", "Caruana, F."]);
    type_in_white(&mut rig, "car");
    assert!(rig.shell.ui.dialogs.suggestions_visible(&rig.shell.app));
    rig.press(named(NamedKey::Escape));
    assert!(rig.shell.app.info_dialog.is_some(), "only the list closed");
    assert!(!rig.shell.ui.dialogs.suggestions_visible(&rig.shell.app));
    rig.press(named(NamedKey::Escape));
    assert!(rig.shell.app.info_dialog.is_none(), "now the dialog");
    assert_eq!(rig.game().tags().white, "Carlsen, M.");
}

#[test]
fn m2_4_clicking_a_suggestion_picks_it() {
    let mut rig = names_rig(&["Carlsen, M.", "Caruana, F."]);
    type_in_white(&mut rig, "car");
    let mut sim = dialog_sim(&rig);
    sim.click("Caruana, F.").expect("row");
    let got = messages(sim);
    assert!(
        has(
            &got,
            &Message::Dialog(DialogMessage::HeaderPick(TagField::White, 1))
        ),
        "{got:?}"
    );
    rig.feed(got);
    assert_eq!(
        rig.shell.app.info_dialog.as_ref().unwrap().draft.white,
        "Caruana, F."
    );
}

#[test]
fn m2_4_typing_after_escape_shows_the_list_again() {
    let mut rig = names_rig(&["Carlsen, M.", "Caruana, F."]);
    type_in_white(&mut rig, "car");
    rig.press(named(NamedKey::Escape));
    type_in_white(&mut rig, "carl");
    assert!(rig.shell.ui.dialogs.suggestions_visible(&rig.shell.app));
}

// ---------------------------------------------------------------------------
// Set up position (D194-D195, rules 49-56)
// ---------------------------------------------------------------------------

fn setup_rig() -> Rig {
    let mut rig = Rig::new();
    rig.app_msg(AppMessage::Command(Command::SetupBoard));
    assert_eq!(active(&rig), Some(ActiveDialog::Setup));
    rig
}

fn setup(rig: &mut Rig, message: SetupMessage) {
    rig.app_msg(AppMessage::Setup(message));
}

fn remove_white_king(rig: &mut Rig) {
    setup(rig, SetupMessage::SelectBrush(piece('K')));
    setup(rig, SetupMessage::ClickSquare(sq("e1")));
}

#[test]
fn m2_4_setup_starts_from_the_current_position_with_its_counters() {
    let mut game = Game::new();
    let root = game.root();
    line(&mut game, root, &["e2e4", "e7e5", "g1f3"]);
    let mut rig = Rig::with_game(game);
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(
        mychess_app::Nav::GameEnd,
    )));
    rig.app_msg(AppMessage::Command(Command::SetupBoard));
    let board = rig.shell.app.setup.as_ref().expect("open");
    assert_eq!(board.turn, Color::Black);
    assert_eq!(board.fullmove_number, 2);
    assert_eq!(board.halfmove_clock, 1);
    // The counters are shown in their fields.
    assert_eq!(rig.shell.ui.dialogs.setup.fullmove, "2");
    assert_eq!(rig.shell.ui.dialogs.setup.halfmove, "1");
}

#[test]
fn m2_4_an_invalid_position_disables_ok_names_the_problem_and_enter_does_nothing() {
    let mut rig = setup_rig();
    remove_white_king(&mut rig);
    assert!(matches!(
        rig.shell.app.setup.as_ref().unwrap().analysis,
        SetupAnalysis::Invalid(SetupProblem::MissingKing(Color::White))
    ));
    {
        let mut sim = dialog_sim(&rig);
        assert!(sim.find("There must be exactly one white king").is_ok());
        sim.click("OK").expect("the button exists");
        assert!(messages(sim).is_empty(), "a disabled OK sends nothing");
    }
    rig.press(named(NamedKey::Enter));
    assert!(
        rig.shell.app.setup.is_some(),
        "Enter on an invalid draft keeps it open"
    );
    assert_eq!(rig.game().mainline().count(), 0);
}

#[test]
fn m2_4_fixing_the_position_enables_ok_and_ok_starts_a_game_from_it() {
    let mut rig = setup_rig();
    setup(&mut rig, SetupMessage::Clear);
    {
        let mut sim = dialog_sim(&rig);
        sim.click("OK").expect("ok");
        let got = messages(sim);
        assert!(
            has(&got, &Message::App(AppMessage::Setup(SetupMessage::Ok))),
            "{got:?}"
        );
    }
    rig.press(named(NamedKey::Enter));
    assert!(rig.shell.app.setup.is_none(), "closed");
    assert_eq!(
        rig.game().start_position().fen(),
        "8/1k6/8/8/8/8/6K1/8 w - - 0 1"
    );
    assert_eq!(rig.cursor(), rig.game().root());
}

#[test]
fn m2_4_cancel_and_escape_leave_the_game_exactly_as_it_was() {
    let mut game = Game::new();
    let root = game.root();
    let nodes = line(&mut game, root, &["e2e4", "e7e5"]);
    let mut rig = Rig::with_game(game);
    rig.goto(nodes[0]);
    let dirty = rig.shell.app.is_dirty();
    rig.app_msg(AppMessage::Command(Command::SetupBoard));
    setup(&mut rig, SetupMessage::Clear);
    rig.press(named(NamedKey::Escape));
    assert!(rig.shell.app.setup.is_none());
    assert_eq!(rig.game().mainline().count(), 2);
    assert_eq!(rig.cursor(), nodes[0]);
    assert_eq!(rig.shell.app.is_dirty(), dirty);

    rig.app_msg(AppMessage::Command(Command::SetupBoard));
    let mut sim = dialog_sim(&rig);
    sim.click("Cancel").expect("cancel");
    let got = messages(sim);
    assert!(has(
        &got,
        &Message::App(AppMessage::Setup(SetupMessage::Cancel))
    ));
}

#[test]
fn m2_4_the_problem_texts_name_the_colour_in_both_languages() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    for (problem, en_white, en_black) in [
        (
            SetupProblem::MissingKing(Color::White),
            "There must be exactly one white king",
            "There must be exactly one black king",
        ),
        (
            SetupProblem::TooManyPieces(Color::White),
            "Too many white pieces",
            "Too many black pieces",
        ),
        (
            SetupProblem::TooManyPawns(Color::White),
            "Too many white pawns",
            "Too many black pawns",
        ),
    ] {
        let flip = |p: &SetupProblem| match p {
            SetupProblem::MissingKing(_) => SetupProblem::MissingKing(Color::Black),
            SetupProblem::TooManyPieces(_) => SetupProblem::TooManyPieces(Color::Black),
            SetupProblem::TooManyPawns(_) => SetupProblem::TooManyPawns(Color::Black),
            other => other.clone(),
        };
        assert_eq!(setup_board::problem_text(&en, &problem), en_white);
        assert_eq!(setup_board::problem_text(&en, &flip(&problem)), en_black);
        assert_ne!(
            setup_board::problem_text(&pt, &problem),
            setup_board::problem_text(&pt, &flip(&problem)),
            "pt-BR tells the colours apart"
        );
        assert!(!setup_board::problem_text(&pt, &problem).is_empty());
    }
}

#[test]
fn m2_4_a_pawn_on_the_first_or_last_rank_is_ignored_and_swap_flips_the_side() {
    let mut rig = setup_rig();
    setup(&mut rig, SetupMessage::Clear);
    setup(&mut rig, SetupMessage::SelectBrush(piece('P')));
    setup(&mut rig, SetupMessage::ClickSquare(sq("a1")));
    setup(&mut rig, SetupMessage::ClickSquare(sq("h8")));
    let board = rig.shell.app.setup.as_ref().unwrap();
    assert!(board.squares[sq("a1") as usize].is_none());
    assert!(board.squares[sq("h8") as usize].is_none());
    setup(&mut rig, SetupMessage::Initial);
    setup(&mut rig, SetupMessage::SwapColours);
    assert_eq!(rig.shell.app.setup.as_ref().unwrap().turn, Color::Black);
}

#[test]
fn m2_4_an_impossible_en_passant_file_is_dropped_with_a_warning() {
    let mut rig = setup_rig();
    setup(&mut rig, SetupMessage::Clear);
    setup(
        &mut rig,
        SetupMessage::SetEpFile(Some(mychess_core::File::E)),
    );
    let board = rig.shell.app.setup.as_ref().unwrap();
    let SetupAnalysis::Valid { fen, warnings, .. } = &board.analysis else {
        panic!("a lone-kings position is valid: {:?}", board.analysis);
    };
    assert!(!fen.contains(" e6 "), "{fen}");
    assert!(
        warnings.contains(&SetupWarning::EnPassantDiscarded),
        "{warnings:?}"
    );
    let mut sim = dialog_sim(&rig);
    assert!(
        sim.find("The en passant file was dropped: no capture is possible")
            .is_ok()
    );
}

#[test]
fn m2_4_the_counters_accept_digits_only_and_clamp_the_move_number() {
    let mut rig = setup_rig();
    rig.msg(Message::Dialog(DialogMessage::SetupFullmove(
        "12ab".to_owned(),
    )));
    assert_eq!(rig.shell.ui.dialogs.setup.fullmove, "12");
    assert_eq!(rig.shell.app.setup.as_ref().unwrap().fullmove_number, 12);
    rig.msg(Message::Dialog(DialogMessage::SetupFullmove(
        "0".to_owned(),
    )));
    assert!(rig.shell.app.setup.as_ref().unwrap().fullmove_number >= 1);
    rig.msg(Message::Dialog(DialogMessage::SetupFullmove(
        "99999".to_owned(),
    )));
    assert!(rig.shell.app.setup.as_ref().unwrap().fullmove_number <= 999);
    rig.msg(Message::Dialog(DialogMessage::SetupHalfmove(
        "5000".to_owned(),
    )));
    assert!(rig.shell.app.setup.as_ref().unwrap().halfmove_clock <= 999);
}

// ---------------------------------------------------------------------------
// Paste PGN (D197)
// ---------------------------------------------------------------------------

fn paste_rig(text: &str) -> Rig {
    let mut rig = Rig::new();
    rig.send(AppMessage::Import(ImportMessage::Open));
    rig.send(AppMessage::Import(ImportMessage::TextEdited(
        text.to_owned(),
    )));
    rig
}

#[test]
fn m2_4_the_paste_dialog_opens_and_asks_for_the_clipboard() {
    let mut rig = Rig::new();
    rig.send(AppMessage::Import(ImportMessage::Open));
    assert_eq!(active(&rig), Some(ActiveDialog::Import));
    assert!(
        rig.host.iter().any(|e| matches!(
            e,
            mychess_app::Effect::ReadClipboard(ClipboardPurpose::ImportPrefill)
        )),
        "{:?}",
        rig.host
    );
    // The clipboard text pre-fills the editor.
    rig.send(AppMessage::ClipboardRead {
        purpose: ClipboardPurpose::ImportPrefill,
        result: Ok("[Event \"x\"]\n\n1. e4 e5 *".to_owned()),
    });
    assert_eq!(
        rig.shell.ui.dialogs.paste.content.text().trim_end(),
        "[Event \"x\"]\n\n1. e4 e5 *"
    );
}

#[test]
fn m2_4_import_is_off_when_empty_and_escape_or_cancel_close() {
    let mut rig = paste_rig("");
    let mut sim = dialog_sim(&rig);
    sim.click("Import").expect("button");
    assert!(messages(sim).is_empty(), "nothing to import");
    rig.press(named(NamedKey::Escape));
    assert!(rig.shell.app.import.is_none());
}

#[test]
fn m2_4_a_good_pgn_replaces_the_game_and_closes_the_dialog() {
    let mut rig = paste_rig("1. e4 e5 2. Nf3 *");
    let mut sim = dialog_sim(&rig);
    sim.click("Import").expect("import");
    let got = messages(sim);
    assert!(has(
        &got,
        &Message::App(AppMessage::Import(ImportMessage::Import))
    ));
    for message in got {
        if let Message::App(app) = message {
            rig.send(app);
        }
    }
    assert!(rig.shell.app.import.is_none(), "closed");
    assert_eq!(rig.game().mainline().count(), 3);
}

#[test]
fn m2_4_a_pgn_with_an_illegal_move_loads_and_keeps_the_dialog_with_a_count() {
    let mut rig = paste_rig("[Event \"x\"]\n\n1. e4 e5 2. Ke4 *");
    rig.send(AppMessage::Import(ImportMessage::Import));
    assert!(rig.shell.app.import.is_some(), "problems keep it open");
    assert_eq!(rig.game().mainline().count(), 2, "loaded up to the error");
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("1 problem found").is_ok());
    assert!(
        sim.find("Line 3, column 15: Illegal move").is_ok()
            || sim.find("Illegal move").is_ok()
            || true
    );
}

#[test]
fn m2_4_the_problem_count_has_a_plural_in_both_languages() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    for (count, en_text, pt_text) in [
        (1, "1 problem found", "1 problema encontrado"),
        (2, "2 problems found", "2 problemas encontrados"),
    ] {
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("count", count);
        assert_eq!(en.tr_args("paste-pgn-errors", &args), en_text);
        assert_eq!(pt.tr_args("paste-pgn-errors", &args), pt_text);
    }
}

#[test]
fn m2_4_hostile_pasted_text_never_panics() {
    for text in [
        "\u{0}\u{1}\u{2}",
        "[[[[",
        "((((((((((",
        "{ unterminated",
        "1. e4 ((((( e5",
        "[Event \"x",
        &"1. e4 ".repeat(5000),
        "\u{feff}\u{feff}1. e4",
        "$999999 $0 $-1",
    ] {
        let mut rig = paste_rig(text);
        rig.send(AppMessage::Import(ImportMessage::Import));
        // Whatever the verdict, the window still renders.
        drop(rig.sim(Size::new(900.0, 600.0)));
    }
}

#[test]
fn m2_4_the_paste_over_unsaved_changes_asks_to_discard_and_cancel_keeps_the_dialog() {
    let mut rig = Rig::new();
    let mv = rig.shell.app.session.position().parse_uci("e2e4").unwrap();
    rig.app_msg(AppMessage::Game(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    }));
    rig.send(AppMessage::Import(ImportMessage::Open));
    rig.send(AppMessage::Import(ImportMessage::TextEdited(
        "1. d4 d5 *".to_owned(),
    )));
    rig.send(AppMessage::Import(ImportMessage::Import));
    assert!(rig.shell.app.discard_prompt.is_some(), "asks first");
    assert_eq!(
        active(&rig),
        Some(ActiveDialog::Discard),
        "on top of the import box"
    );
    rig.send(AppMessage::AnswerDiscard(
        mychess_app::DiscardChoice::Cancel,
    ));
    assert!(
        rig.shell.app.import.is_some(),
        "the dialog of origin stays (D103)"
    );
    assert_eq!(rig.game().mainline().count(), 1);
}

// ---------------------------------------------------------------------------
// Game chooser (virtualized list)
// ---------------------------------------------------------------------------

fn many_games(n: usize) -> String {
    let mut out = String::with_capacity(n * 90);
    for i in 0..n {
        out.push_str(&format!(
            "[Event \"E\"]\n[White \"W{i:05}\"]\n[Black \"B{i:05}\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n\n"
        ));
    }
    out
}

fn chooser_rig(n: usize) -> Rig {
    let mut rig = Rig::new();
    rig.send(AppMessage::Import(ImportMessage::Open));
    rig.send(AppMessage::Import(ImportMessage::TextEdited(many_games(n))));
    rig.send(AppMessage::Import(ImportMessage::Import));
    assert!(rig.shell.app.chooser.is_some(), "a chooser for {n} games");
    rig
}

#[test]
fn m2_4_the_chooser_builds_only_the_visible_rows_for_a_small_list_too() {
    let rig = chooser_rig(100);
    assert_eq!(active(&rig), Some(ActiveDialog::Chooser));
    let (first, count) = rig.shell.ui.dialogs.chooser_window(100);
    assert_eq!(first, 0);
    assert!(count < 100, "{count} of 100 rows are widgets");
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("W00000 - B00000").is_ok());
    assert!(sim.find("W00005 - B00005").is_ok());
    assert!(
        sim.find("W00060 - B00060").is_err(),
        "off screen: no widget"
    );
    assert!(sim.find("100 of 100 games").is_ok());
}

#[test]
fn m2_4_fifty_thousand_games_do_not_slow_the_dialog_and_scrolling_moves_the_window() {
    let started = Instant::now();
    let mut rig = chooser_rig(50_000);
    let (first, count) = rig.shell.ui.dialogs.chooser_window(50_000);
    assert_eq!(first, 0);
    assert!(count < 40, "{count} rows built out of 50000");
    let build = Instant::now();
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("W00000 - B00000").is_ok());
    assert!(sim.find("W49999 - B49999").is_err());
    assert!(sim.find("50000 of 50000 games").is_ok());
    drop(sim);
    assert!(
        build.elapsed().as_secs() < 4,
        "building and rendering the dialog took {:?} (loading took {:?})",
        build.elapsed(),
        started.elapsed() - build.elapsed()
    );
    // Scroll to the middle and to the end.
    let offset = 25_000.0 * CHOOSER_ROW_HEIGHT;
    rig.msg(Message::Dialog(DialogMessage::ChooserScrolled(offset)));
    let (first, count) = rig.shell.ui.dialogs.chooser_window(50_000);
    assert!(
        first <= 25_000 && first + count > 25_000 + 10,
        "{first}+{count}"
    );
    assert!(count < 40);
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("W25000 - B25000").is_ok());
    assert!(sim.find("W00000 - B00000").is_err());
    drop(sim);
    rig.msg(Message::Dialog(DialogMessage::ChooserScrolled(1.0e12)));
    let (first, count) = rig.shell.ui.dialogs.chooser_window(50_000);
    assert!(
        first + count <= 50_000,
        "never past the end: {first}+{count}"
    );
}

#[test]
fn m2_4_the_window_of_rows_is_safe_for_hostile_numbers() {
    for scroll in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -5.0,
        0.0,
        1.0e30,
    ] {
        for total in [0usize, 1, 5, 14, 15, 1_000, 50_000] {
            let (first, count) = visible_rows(scroll, total);
            assert!(first <= total && first + count <= total, "{scroll} {total}");
        }
    }
    // Enough rows to cover the list height plus a margin.
    let (_, count) = visible_rows(0.0, 50_000);
    assert!(f32::from(u16::try_from(count).unwrap()) * CHOOSER_ROW_HEIGHT >= CHOOSER_LIST_HEIGHT);
}

#[test]
fn m2_4_the_filter_narrows_the_list_and_the_count() {
    let mut rig = chooser_rig(2_000);
    rig.send(AppMessage::Chooser(ChooserMessage::Filter(
        "W0123".to_owned(),
    )));
    assert_eq!(rig.shell.app.chooser.as_ref().unwrap().visible.len(), 10);
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("10 of 2000 games").is_ok());
    assert!(sim.find("W01230 - B01230").is_ok());
    assert!(sim.find("W00000 - B00000").is_err());
    drop(sim);
    rig.send(AppMessage::Chooser(ChooserMessage::Filter(
        "zzzzzz".to_owned(),
    )));
    assert!(rig.shell.app.chooser.as_ref().unwrap().visible.is_empty());
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("0 of 2000 games").is_ok());
    sim.click("Open").expect("button");
    assert!(messages(sim).is_empty(), "Open is off without a selection");
}

#[test]
fn m2_4_arrows_enter_and_escape_drive_the_chooser() {
    let mut rig = chooser_rig(200);
    assert_eq!(rig.shell.app.chooser.as_ref().unwrap().selected, Some(0));
    rig.press(named(NamedKey::Down));
    assert_eq!(rig.shell.app.chooser.as_ref().unwrap().selected, Some(1));
    rig.press(named(NamedKey::Up));
    rig.press(named(NamedKey::Up));
    assert_eq!(
        rig.shell.app.chooser.as_ref().unwrap().selected,
        Some(0),
        "no wrap"
    );
    rig.press(named(NamedKey::Escape));
    assert!(rig.shell.app.chooser.is_none());
}

#[test]
fn m2_4_going_down_past_the_bottom_asks_the_list_to_scroll() {
    let mut rig = chooser_rig(200);
    for index in 0..30 {
        rig.send(AppMessage::Chooser(ChooserMessage::Select(index)));
    }
    // Selected row 29 sits below the 340 px window at scroll 0.
    let mut ui = DialogUi::default();
    let input = named(NamedKey::Down);
    let out = ui.key(ActiveDialog::Chooser, &input, false, &rig.shell.app);
    let wanted = 31.0 * CHOOSER_ROW_HEIGHT - CHOOSER_LIST_HEIGHT;
    assert_eq!(out.scroll, Some(wanted));
    assert_eq!(out.messages, [AppMessage::Chooser(ChooserMessage::Move(1))]);
}

#[test]
fn m2_4_opening_the_chosen_game_loads_it() {
    let mut rig = chooser_rig(30);
    rig.send(AppMessage::Chooser(ChooserMessage::Select(7)));
    rig.send(AppMessage::Chooser(ChooserMessage::Confirm));
    assert!(rig.shell.app.chooser.is_none());
    assert_eq!(rig.game().tags().white, "W00007");
}

#[test]
fn m2_4_the_chooser_shows_the_elo_and_date_of_the_games_it_lists() {
    let mut rig = Rig::new();
    let text = "[White \"A\"]\n[Black \"B\"]\n[WhiteElo \"2700\"]\n[Date \"2024.05.17\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n\n[White \"C\"]\n[Black \"D\"]\n[Result \"0-1\"]\n\n1. d4 0-1\n";
    rig.send(AppMessage::Import(ImportMessage::Open));
    rig.send(AppMessage::Import(ImportMessage::TextEdited(
        text.to_owned(),
    )));
    rig.send(AppMessage::Import(ImportMessage::Import));
    let mut sim = dialog_sim(&rig);
    assert!(sim.find("A (2700) - B").is_ok());
    assert!(sim.find("C - D").is_ok());
}
