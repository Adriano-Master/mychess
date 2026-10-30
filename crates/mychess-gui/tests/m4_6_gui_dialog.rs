//! The "Configure annotation" dialog (spec M4-6, "Comportamento observavel"
//! 1-2, "Textos de interface", D910, D912; Scid `initAnnotation`,
//! `analysis.tcl` 882-1165): how it opens, the controls and their ranges,
//! what the engine can do, `Enter`/`Esc`. Headless.
//!
//! The box is drawn alone (`annotate_dialog::view`) to click its controls;
//! keys and the window go through the `Rig`.
#![allow(clippy::float_cmp, clippy::cast_precision_loss)]

mod m4_68_gui_common;

use std::time::Duration;

use m4_68_gui_common::*;
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use rstest::rstest;

fn dialog_rig() -> (Rig, RunId) {
    let (mut rig, run) = rig_with_engine(&SCHOLAR);
    ann(&mut rig, AnnotateMessage::Open);
    (rig, run)
}

fn set_options(rig: &mut Rig, f: impl FnOnce(&mut AnnotateOptions)) {
    let mut o = dialog_options(rig);
    f(&mut o);
    ann(rig, AnnotateMessage::SetOptions(o));
}

/// The options sent by the one `SetOptions` among `messages`.
fn sent_options(messages: &[Message]) -> AnnotateOptions {
    let mut all: Vec<AnnotateOptions> = annotates(messages)
        .into_iter()
        .filter_map(|m| match m {
            AnnotateMessage::SetOptions(o) => Some(o),
            _ => None,
        })
        .collect();
    assert_eq!(all.len(), 1, "exactly one SetOptions in {messages:?}");
    all.remove(0)
}

// ---- opening ----

#[test]
fn the_tools_menu_item_annotate_sends_open() {
    let rig = Rig::new();
    let tools = menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options)
        .into_iter()
        .find(|s| s.label_key == "menu-tools")
        .expect("a Tools menu");
    let item = tools
        .entries
        .iter()
        .find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key("menu-tools-annotate"),
                action,
                ..
            } => Some(action.clone()),
            _ => None,
        })
        .expect("the annotate item");
    assert!(matches!(
        item,
        Some(Message::App(AppMessage::Annotate(AnnotateMessage::Open)))
    ));
}

#[test]
fn the_annotate_button_of_the_panel_opens_the_dialog() {
    let (mut rig, _run) = rig_with_engine(&SCHOLAR);
    let sent = click_msgs(&rig, "Annotate");
    assert!(
        annotates(&sent).contains(&AnnotateMessage::Open),
        "{sent:?}"
    );
    apply(&mut rig, sent);
    assert_eq!(active(&rig), Some(ActiveDialog::Annotate));
    assert!(rig.shell.app.annotate_dialog.is_some());
}

#[test]
fn the_window_shows_the_dialog_only_while_it_is_open() {
    let (mut rig, _run) = rig_with_engine(&SCHOLAR);
    assert!(!window_has(&rig, "Configure annotation"));
    ann(&mut rig, AnnotateMessage::Open);
    assert!(window_has(&rig, "Configure annotation"));
    esc(&mut rig);
    assert!(!window_has(&rig, "Configure annotation"));
}

#[test]
fn opening_without_an_engine_still_shows_the_dialog() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&SCHOLAR));
    ann(&mut rig, AnnotateMessage::Open);
    assert_eq!(active(&rig), Some(ActiveDialog::Annotate));
    assert!(dialog_has(&rig, "Start"));
}

#[test]
fn escape_closes_the_dialog_without_starting() {
    let (mut rig, _run) = dialog_rig();
    esc(&mut rig);
    assert_eq!(active(&rig), None);
    assert!(rig.shell.app.annotate_dialog.is_none());
    assert!(!rig.shell.app.annotation_running());
}

#[test]
fn enter_starts_with_the_options_of_the_dialog() {
    let (mut rig, _run) = dialog_rig();
    set_options(&mut rig, |o| {
        o.control = Control::Time;
        o.movetime = Duration::from_secs(2);
        o.blunder = 0.5;
    });
    enter(&mut rig);
    assert!(rig.shell.app.annotation_running());
    assert_eq!(active(&rig), None, "the dialog closes when the run starts");
    let o = rig.shell.app.annotate_options;
    assert_eq!(o.control, Control::Time);
    assert_eq!(o.movetime, Duration::from_secs(2));
    assert_eq!(o.blunder, 0.5);
}

#[test]
fn start_without_an_engine_warns_and_does_not_run() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&SCHOLAR));
    ann(&mut rig, AnnotateMessage::Open);
    enter(&mut rig);
    assert!(!rig.shell.app.annotation_running());
    assert_eq!(annotate_notice(&rig), Some(AnnotateNotice::NoEngine));
    assert!(window_has(
        &rig,
        "No engine is running and none is set for F2."
    ));
}

#[test]
fn the_start_and_cancel_buttons_send_start_and_close() {
    let (rig, _run) = dialog_rig();
    assert!(annotates(&dialog_click(&rig, "Start")).contains(&AnnotateMessage::Start));
    assert!(annotates(&dialog_click(&rig, "Cancel")).contains(&AnnotateMessage::Close));
}

#[test]
fn start_through_the_button_uses_what_the_controls_chose() {
    let (mut rig, _run) = dialog_rig();
    let sent = dialog_click(&rig, "Time");
    apply(&mut rig, sent);
    let sent = dialog_click(&rig, "Start");
    apply(&mut rig, sent);
    assert!(rig.shell.app.annotation_running());
    assert_eq!(rig.shell.app.annotate_options.control, Control::Time);
}

// ---- the defaults of the Scid ----

#[test]
fn the_dialog_shows_every_label_and_default_of_the_scid() {
    let (rig, _run) = dialog_rig();
    let mut s = dialog_sim(&rig, false, &en());
    for text in [
        "Configure annotation",
        "Move control",
        "Depth per move",
        "13",
        "Seconds per move",
        "5.0",
        "Threshold",
        "1.0",
        "Cut off",
        "Add score",
        "Add variations",
        "Missed/shorter mates",
        "At end of game",
        "Which side",
        "Score format",
        "Use book",
        "Opening errors only",
        "Add annotator tag",
        "Process variations",
        "Mark tactical exercises",
        "Configure informant values...",
        "Start",
        "Cancel",
        "Fake",
        "Automatic",
    ] {
        assert!(s.find(text).is_ok(), "missing {text:?}");
    }
}

#[test]
fn the_default_options_of_the_dialog_are_the_scid_defaults() {
    let (rig, _run) = dialog_rig();
    let o = dialog_options(&rig);
    assert_eq!(o.control, Control::Depth);
    assert_eq!(o.depth, 13);
    assert_eq!(o.movetime, Duration::from_secs(5));
    assert_eq!(o.blunder, 1.0);
    assert_eq!(o.cutoff, 5.0);
    assert_eq!(o.with_score, WithScore::AllMoves);
    assert_eq!(o.with_vars, WithVars::Blunders);
    assert!(o.missed_mates && o.var_at_end && o.add_tag);
    assert!(!o.variations && !o.mark_exercises && o.opening_only.is_none());
}

#[test]
fn the_chosen_control_changes_what_is_drawn() {
    let (mut rig, _run) = dialog_rig();
    let f_depth = {
        let mut depth = dialog_sim(&rig, false, &en());
        fingerprint(&mut depth)
    };
    set_options(&mut rig, |o| o.control = Control::Time);
    let mut time = dialog_sim(&rig, false, &en());
    assert_ne!(f_depth, fingerprint(&mut time), "the pressed button moved");
}

// ---- exclusive choices ----

#[rstest]
#[case("Time", 0, |o: &AnnotateOptions| o.control == Control::Time)]
#[case("All moves", 0, |o: &AnnotateOptions| o.with_score == WithScore::AllMoves)]
#[case("Blunders/Not best", 0, |o: &AnnotateOptions| o.with_score == WithScore::Blunders)]
#[case("Variations", 0, |o: &AnnotateOptions| o.with_score == WithScore::Variations)]
#[case("No", 0, |o: &AnnotateOptions| o.with_score == WithScore::No)]
#[case("When move is a blunder", 0, |o: &AnnotateOptions| o.with_vars == WithVars::Blunders)]
#[case("When move is not the best", 0, |o: &AnnotateOptions| o.with_vars == WithVars::NotBest)]
#[case("All moves", 1, |o: &AnnotateOptions| o.with_vars == WithVars::AllMoves)]
#[case("No", 1, |o: &AnnotateOptions| o.with_vars == WithVars::No)]
#[case("White", 0, |o: &AnnotateOptions| o.side == mychess_app::AnnotateSide::White)]
#[case("Black", 0, |o: &AnnotateOptions| o.side == mychess_app::AnnotateSide::Black)]
#[case("Both", 0, |o: &AnnotateOptions| o.side == mychess_app::AnnotateSide::All)]
#[case("+1.5", 0, |o: &AnnotateOptions| o.score_format == mychess_app::ScoreFormat::Plain)]
#[case("[% +1.5]", 0, |o: &AnnotateOptions| o.score_format == mychess_app::ScoreFormat::Bracket)]
#[case("[%eval +1.5]", 0, |o: &AnnotateOptions| o.score_format == mychess_app::ScoreFormat::Eval)]
fn an_exclusive_choice_sends_the_options_with_that_value(
    #[case] label: &str,
    #[case] nth: usize,
    #[case] holds: fn(&AnnotateOptions) -> bool,
) {
    let (rig, _run) = dialog_rig();
    let sent = dialog_click_nth(&rig, label, nth);
    let o = sent_options(&sent);
    assert!(holds(&o), "{label:?} #{nth}: {o:?}");
    // Everything else of the options is untouched.
    let d = AnnotateOptions::default();
    let changed = [
        o.control != d.control,
        o.with_score != d.with_score,
        o.with_vars != d.with_vars,
        o.side != d.side,
        o.score_format != d.score_format,
    ]
    .iter()
    .filter(|c| **c)
    .count();
    assert!(changed <= 1, "a choice changes one group only: {o:?}");
}

#[test]
fn choosing_a_value_updates_the_application_and_the_pressed_button() {
    let (mut rig, _run) = dialog_rig();
    let before = {
        let mut s = dialog_sim(&rig, false, &en());
        fingerprint(&mut s)
    };
    let sent = dialog_click_nth(&rig, "No", 1);
    apply(&mut rig, sent);
    assert_eq!(dialog_options(&rig).with_vars, WithVars::No);
    let mut s = dialog_sim(&rig, false, &en());
    assert_ne!(before, fingerprint(&mut s));
}

// ---- the "-" and "+" counters ----

/// The `n`-th "-" (U+2212) or "+" of the dialog, tree order: depth 0,
/// seconds 1, threshold 2, cut-off 3, opening moves 4.
fn step(rig: &Rig, glyph: &str, n: usize) -> Vec<Message> {
    dialog_click_nth(rig, glyph, n)
}

type Get = fn(&Informant) -> f32;
const MINUS: &str = "\u{2212}";

#[rstest]
#[case(13, "+", 14)]
#[case(13, MINUS, 12)]
#[case(10, MINUS, 10)]
#[case(100, "+", 100)]
fn the_depth_counter_moves_by_one_inside_10_to_100(
    #[case] from: u32,
    #[case] glyph: &str,
    #[case] to: u32,
) {
    let (mut rig, _run) = dialog_rig();
    set_options(&mut rig, |o| o.depth = from);
    let sent = step(&rig, glyph, 0);
    assert_eq!(sent_options(&sent).depth, to);
}

#[rstest]
#[case(5000, "+", 6000)]
#[case(5000, MINUS, 4000)]
#[case(500, "+", 600)]
#[case(1000, MINUS, 900)]
#[case(1000, "+", 2000)]
#[case(100, MINUS, 100)]
#[case(300_000, "+", 300_000)]
fn the_seconds_counter_steps_a_tenth_below_a_second_and_a_second_above(
    #[case] from_ms: u64,
    #[case] glyph: &str,
    #[case] to_ms: u64,
) {
    let (mut rig, _run) = dialog_rig();
    set_options(&mut rig, |o| {
        o.control = Control::Time;
        o.movetime = Duration::from_millis(from_ms);
    });
    let sent = step(&rig, glyph, 1);
    assert_eq!(sent_options(&sent).movetime, Duration::from_millis(to_ms));
}

#[rstest]
#[case(1.0, "+", 1.1)]
#[case(1.0, MINUS, 0.9)]
#[case(0.1, MINUS, 0.1)]
#[case(3.0, "+", 3.0)]
fn the_threshold_counter_moves_by_a_tenth_inside_0_1_to_3_0(
    #[case] from: f32,
    #[case] glyph: &str,
    #[case] to: f32,
) {
    let (mut rig, _run) = dialog_rig();
    set_options(&mut rig, |o| o.blunder = from);
    let sent = step(&rig, glyph, 2);
    assert!((sent_options(&sent).blunder - to).abs() < 1e-5, "{sent:?}");
}

#[rstest]
#[case(5.0, "+", 5.5)]
#[case(5.0, MINUS, 4.5)]
#[case(3.0, MINUS, 3.0)]
#[case(10.0, "+", 10.0)]
fn the_cutoff_counter_moves_by_half_a_pawn_inside_3_0_to_10_0(
    #[case] from: f32,
    #[case] glyph: &str,
    #[case] to: f32,
) {
    let (mut rig, _run) = dialog_rig();
    set_options(&mut rig, |o| o.cutoff = from);
    let sent = step(&rig, glyph, 3);
    assert!((sent_options(&sent).cutoff - to).abs() < 1e-5, "{sent:?}");
}

#[rstest]
#[case(10, "+", 11)]
#[case(10, MINUS, 9)]
#[case(5, MINUS, 5)]
#[case(20, "+", 20)]
fn the_opening_counter_moves_by_one_inside_5_to_20(
    #[case] from: u8,
    #[case] glyph: &str,
    #[case] to: u8,
) {
    let (mut rig, _run) = dialog_rig();
    set_options(&mut rig, |o| o.opening_only = Some(from));
    let sent = step(&rig, glyph, 4);
    assert_eq!(sent_options(&sent).opening_only, Some(to));
}

#[test]
fn the_counter_of_the_inactive_control_does_nothing() {
    // With the depth control the seconds counter is off, and the other way.
    let (mut rig, _run) = dialog_rig();
    assert!(step(&rig, "+", 1).is_empty(), "seconds off under Depth");
    set_options(&mut rig, |o| o.control = Control::Time);
    assert!(step(&rig, "+", 0).is_empty(), "depth off under Time");
    assert!(!step(&rig, "+", 1).is_empty(), "seconds on under Time");
}

#[test]
fn the_opening_counter_is_off_until_opening_errors_only_is_on() {
    let (mut rig, _run) = dialog_rig();
    assert!(step(&rig, "+", 4).is_empty());
    let sent = dialog_click(&rig, "Opening errors only");
    assert_eq!(sent_options(&sent).opening_only, Some(10));
    apply(&mut rig, sent);
    assert!(!step(&rig, "+", 4).is_empty());
    // Turning it off again clears the limit.
    let sent = dialog_click(&rig, "Opening errors only");
    assert_eq!(sent_options(&sent).opening_only, None);
}

#[test]
fn the_numbers_are_shown_after_a_change() {
    let (mut rig, _run) = dialog_rig();
    set_options(&mut rig, |o| {
        o.depth = 27;
        o.blunder = 2.5;
        o.cutoff = 7.5;
    });
    for text in ["27", "2.5", "7.5"] {
        assert!(dialog_has(&rig, text), "{text}");
    }
}

// ---- the check boxes ----

#[rstest]
#[case("Missed/shorter mates", |o: &AnnotateOptions| !o.missed_mates)]
#[case("At end of game", |o: &AnnotateOptions| !o.var_at_end)]
#[case("Add annotator tag", |o: &AnnotateOptions| !o.add_tag)]
#[case("Process variations", |o: &AnnotateOptions| o.variations)]
#[case("Mark tactical exercises", |o: &AnnotateOptions| o.mark_exercises)]
fn a_check_box_flips_its_option(#[case] label: &str, #[case] holds: fn(&AnnotateOptions) -> bool) {
    let (rig, _run) = dialog_rig();
    let sent = dialog_click(&rig, label);
    assert!(holds(&sent_options(&sent)), "{label}");
}

// ---- the engine ----

#[test]
fn only_running_engines_are_offered_and_a_click_chooses_one() {
    let (mut rig, run) = rig_with_engine(&SCHOLAR);
    // A second engine that is still starting is not offered.
    let id = add_engine(&mut rig, "Starting One", Protocol::Uci);
    up(
        &mut rig,
        AppMessage::Engine(EngineMessage::Start(id, StartMode::Analyze)),
    );
    ann(&mut rig, AnnotateMessage::Open);
    assert!(dialog_has(&rig, "Fake"));
    assert!(!dialog_has(&rig, "Starting One"));
    let sent = dialog_click(&rig, "Fake");
    assert!(annotates(&sent).contains(&AnnotateMessage::SetEngine(Some(run))));
    let sent = dialog_click(&rig, "Automatic");
    assert!(annotates(&sent).contains(&AnnotateMessage::SetEngine(None)));
}

#[test]
fn eight_engines_are_all_offered_and_each_click_names_its_own_run() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&SCHOLAR));
    let mut runs = Vec::new();
    for n in 0..8 {
        let id = add_engine(&mut rig, &format!("Engine {n}"), Protocol::Uci);
        runs.push(start_with(
            &mut rig,
            id,
            identity(&format!("Engine {n}"), Some(1..=5), true),
        ));
    }
    ann(&mut rig, AnnotateMessage::Open);
    for (n, run) in runs.iter().enumerate() {
        let sent = dialog_click(&rig, &format!("Engine {n}"));
        assert!(
            annotates(&sent).contains(&AnnotateMessage::SetEngine(Some(*run))),
            "engine {n}: {sent:?}"
        );
    }
}

#[test]
fn a_hostile_engine_name_is_shown_verbatim() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&SCHOLAR));
    // Fluent syntax, an RLO override, CJK and an emoji-like symbol.
    let name = "{ $x } \u{202e}gnp \u{65e5}\u{672c}\u{8a9e} -> [one] *";
    let id = add_engine(&mut rig, name, Protocol::Uci);
    let run = start_with(&mut rig, id, identity(name, Some(1..=5), true));
    ann(&mut rig, AnnotateMessage::Open);
    let sent = dialog_click(&rig, name);
    assert!(annotates(&sent).contains(&AnnotateMessage::SetEngine(Some(run))));
}

#[test]
fn a_huge_engine_name_does_not_break_the_dialog() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&SCHOLAR));
    let name = "W".repeat(120);
    let id = add_engine(&mut rig, &name, Protocol::Uci);
    start_with(&mut rig, id, identity(&name, Some(1..=5), true));
    ann(&mut rig, AnnotateMessage::Open);
    assert!(dialog_has(&rig, "Start"));
}

// ---- XBoard (D477): time only, no mates, no exercises ----

#[test]
fn an_xboard_engine_disables_depth_mates_and_exercises_and_says_why() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&SCHOLAR));
    xboard_run(&mut rig);
    ann(&mut rig, AnnotateMessage::Open);
    assert!(dialog_has(&rig, "This engine can only be limited by time."));
    for label in ["Depth", "Missed/shorter mates", "Mark tactical exercises"] {
        assert!(dialog_click(&rig, label).is_empty(), "{label} must be off");
    }
    // Time and everything else keep working.
    assert!(!dialog_click(&rig, "Time").is_empty());
    assert!(!dialog_click(&rig, "Add annotator tag").is_empty());
    // With a depth option, the seconds counter is the live one.
    assert!(!step(&rig, "+", 1).is_empty(), "seconds are on");
    assert!(step(&rig, "+", 0).is_empty(), "depth is off");
}

#[test]
fn an_xboard_engine_shows_the_time_estimate_even_with_a_depth_option() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&SCHOLAR));
    xboard_run(&mut rig);
    ann(&mut rig, AnnotateMessage::Open);
    assert_eq!(dialog_options(&rig).control, Control::Depth);
    // 7 moves x 5 s = 35 s.
    assert!(dialog_has(
        &rig,
        "7 moves to annotate  Estimated time: 0 min 35 s"
    ));
}

#[test]
fn choosing_another_engine_changes_what_is_enabled() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&SCHOLAR));
    let uci = {
        let id = add_engine(&mut rig, "Uci", Protocol::Uci);
        start_with(&mut rig, id, identity("Uci", Some(1..=5), true))
    };
    let xb = xboard_run(&mut rig);
    ann(&mut rig, AnnotateMessage::Open);
    assert!(
        !dialog_click(&rig, "Depth").is_empty(),
        "the default is the UCI one"
    );
    ann(&mut rig, AnnotateMessage::SetEngine(Some(xb)));
    assert!(dialog_click(&rig, "Depth").is_empty());
    ann(&mut rig, AnnotateMessage::SetEngine(Some(uci)));
    assert!(!dialog_click(&rig, "Depth").is_empty());
}

#[test]
fn the_exercises_need_multipv_from_the_engine() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&SCHOLAR));
    let id = add_engine(&mut rig, "Single", Protocol::Uci);
    start_with(&mut rig, id, identity("Single", None, false));
    ann(&mut rig, AnnotateMessage::Open);
    assert!(dialog_click(&rig, "Mark tactical exercises").is_empty());
    assert!(!dialog_click(&rig, "Missed/shorter mates").is_empty());
}

// ---- the book (D472, spec M5-6 D1664): needs an opening book ----

#[test]
fn the_book_is_off_and_says_it_needs_an_opening_book() {
    let (rig, _run) = dialog_rig();
    assert!(dialog_has(
        &rig,
        "Choose an opening book in Tools > Opening books"
    ));
    assert!(dialog_click(&rig, "Use book").is_empty());
}

#[test]
fn the_book_can_be_used_when_the_application_says_it_is_available() {
    let (mut rig, _run) = dialog_rig();
    rig.shell
        .app
        .annotate_dialog
        .as_mut()
        .expect("open")
        .book_available = true;
    assert!(!dialog_has(
        &rig,
        "Choose an opening book in Tools > Opening books"
    ));
    let sent = dialog_click(&rig, "Use book");
    assert!(sent_options(&sent).use_book, "{sent:?}");
}

// ---- the informant values (D474, D910) ----

#[test]
fn the_informant_block_opens_with_its_button_and_lists_the_eight_values() {
    let (mut rig, _run) = dialog_rig();
    let sent = dialog_click(&rig, "Configure informant values...");
    assert!(
        sent.iter()
            .any(|m| matches!(m, Message::Dialog(DialogMessage::ToggleAnnotateInformant)))
    );
    assert!(!rig.shell.ui.dialogs.annotate_informant_open);
    apply(&mut rig, sent);
    assert!(rig.shell.ui.dialogs.annotate_informant_open);
    let mut s = dialog_sim(&rig, true, &en());
    for text in [
        "Interesting move (!?)",
        "Poor move (?)",
        "Blunder (??)",
        "Dubious move (?!)",
        "Slight advantage (+=)",
        "Moderate advantage (+/-)",
        "Decisive advantage (+-)",
        "Won game (++-)",
        "Defaults",
        "Hide informant values",
    ] {
        assert!(s.find(text).is_ok(), "missing {text:?}");
    }
}

#[test]
fn each_informant_counter_moves_its_own_threshold_by_a_tenth() {
    let (rig, _run) = dialog_rig();
    let d = Informant::default();
    let rows: [(usize, Get, f32); 8] = [
        (5, |i| i.interesting, d.interesting),
        (6, |i| i.poor, d.poor),
        (7, |i| i.blunder, d.blunder),
        (8, |i| i.dubious, d.dubious),
        (9, |i| i.slight, d.slight),
        (10, |i| i.moderate, d.moderate),
        (11, |i| i.decisive, d.decisive),
        (12, |i| i.won, d.won),
    ];
    for (n, get, base) in rows {
        let mut s = dialog_sim(&rig, true, &en());
        let at = nth_text_point(&mut s, "+", n);
        s.point_at(at);
        drop(s.simulate([
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Left,
            )),
        ]));
        let sent: Vec<Message> = s.into_messages().collect();
        let informant = annotates(&sent)
            .into_iter()
            .find_map(|m| match m {
                AnnotateMessage::SetInformant(i) => Some(i),
                _ => None,
            })
            .unwrap_or_else(|| panic!("row {n}: no SetInformant in {sent:?}"));
        assert!(
            (get(&informant) - (base + 0.1)).abs() < 1e-5,
            "row {n}: {informant:?}"
        );
        // The other seven are untouched.
        let same = [
            informant.interesting == d.interesting,
            informant.poor == d.poor,
            informant.blunder == d.blunder,
            informant.dubious == d.dubious,
            informant.slight == d.slight,
            informant.moderate == d.moderate,
            informant.decisive == d.decisive,
            informant.won == d.won,
        ];
        assert_eq!(
            same.iter().filter(|s| !**s).count(),
            1,
            "row {n}: {informant:?}"
        );
    }
}

#[test]
fn the_informant_defaults_button_restores_the_scid_values() {
    let (mut rig, _run) = dialog_rig();
    open_informant(&mut rig);
    let sent = dialog_click(&rig, "Defaults");
    assert!(annotates(&sent).contains(&AnnotateMessage::SetInformant(Informant::default())));
}

#[test]
fn the_informant_values_are_clamped_to_0_0_and_9_9() {
    let (mut rig, _run) = dialog_rig();
    ann(
        &mut rig,
        AnnotateMessage::SetInformant(Informant {
            interesting: 0.0,
            won: 9.9,
            ..Informant::default()
        }),
    );
    let mut s = dialog_sim(&rig, true, &en());
    let down = nth_text_point(&mut s, MINUS, 5);
    s.point_at(down);
    drop(s.simulate([
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        )),
    ]));
    let sent: Vec<Message> = s.into_messages().collect();
    let i = annotates(&sent)
        .into_iter()
        .find_map(|m| match m {
            AnnotateMessage::SetInformant(i) => Some(i),
            _ => None,
        })
        .expect("a SetInformant");
    assert!(i.interesting >= 0.0, "{i:?}");
    let mut s = dialog_sim(&rig, true, &en());
    let up_won = nth_text_point(&mut s, "+", 12);
    s.point_at(up_won);
    drop(s.simulate([
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        )),
    ]));
    let sent: Vec<Message> = s.into_messages().collect();
    let i = annotates(&sent)
        .into_iter()
        .find_map(|m| match m {
            AnnotateMessage::SetInformant(i) => Some(i),
            _ => None,
        })
        .expect("a SetInformant");
    assert!(i.won <= 9.9 + 1e-5, "{i:?}");
}

#[test]
fn the_informant_values_survive_into_the_run() {
    let (mut rig, _run) = dialog_rig();
    let custom = Informant {
        won: 7.0,
        ..Informant::default()
    };
    ann(&mut rig, AnnotateMessage::SetInformant(custom));
    enter(&mut rig);
    assert_eq!(rig.shell.app.annotate_informant, custom);
}

// ---- the estimate (rule 22) ----

#[test]
fn the_estimate_counts_the_moves_and_is_singular_for_one() {
    let (mut rig, _run) = rig_with_engine(&["e2e4"]);
    ann(&mut rig, AnnotateMessage::Open);
    assert!(dialog_has(&rig, "1 move to annotate"));
    rig.set_game(game_of(&SCHOLAR));
    let o = dialog_options(&rig);
    ann(&mut rig, AnnotateMessage::SetOptions(o));
    assert!(dialog_has(&rig, "7 moves to annotate"));
}

#[test]
fn the_estimate_counts_from_the_cursor_to_the_end() {
    let (mut rig, _run) = rig_with_engine(&SCHOLAR);
    let third = rig.game().mainline().nth(2).expect("ply 3");
    rig.goto(third);
    ann(&mut rig, AnnotateMessage::Open);
    // D465: the move that leads to the cursor is not annotated.
    assert!(dialog_has(&rig, "4 moves to annotate"), "plies after ply 3");
}

#[test]
fn the_time_estimate_is_moves_times_seconds_per_move() {
    let (mut rig, _run) = dialog_rig();
    set_options(&mut rig, |o| {
        o.control = Control::Time;
        o.movetime = Duration::from_secs(300);
    });
    // 7 x 300 s = 35 min 0 s: no overflow, no rounding.
    assert!(dialog_has(
        &rig,
        "7 moves to annotate  Estimated time: 35 min 0 s"
    ));
    set_options(&mut rig, |o| o.movetime = Duration::from_millis(100));
    assert!(
        dialog_has(&rig, "7 moves to annotate  Estimated time: 0 min 0 s"),
        "0.7 s shows as 0"
    );
}

#[test]
fn a_depth_limit_shows_no_time_estimate() {
    let (rig, _run) = dialog_rig();
    assert!(dialog_has(&rig, "7 moves to annotate"));
    assert!(!dialog_has(
        &rig,
        "7 moves to annotate  Estimated time: 0 min 35 s"
    ));
}

#[test]
fn opening_errors_only_shrinks_the_estimate() {
    let (mut rig, _run) = rig_with_engine(&[
        "e2e4", "e7e5", "d1h5", "b8c6", "h5e5", "c6e5", "g1f3", "e5f3", "g2f3", "d7d6", "f1c4",
        "g8f6", "d2d3", "f8e7", "c1e3", "e8g8", "b1c3", "c7c6", "e1c1", "b7b5",
    ]);
    ann(&mut rig, AnnotateMessage::Open);
    assert!(dialog_has(&rig, "20 moves to annotate"));
    set_options(&mut rig, |o| o.opening_only = Some(5));
    let moves = rig.shell.app.annotate_dialog.as_ref().expect("open").moves;
    assert!(
        (10..=11).contains(&moves),
        "5 opening moves are 10 plies (11 with the crossing one): {moves}"
    );
}

// ---- portuguese and tiny windows ----

#[test]
fn the_dialog_is_drawn_in_portuguese_with_the_scid_words() {
    let (rig, _run) = dialog_rig();
    let mut s = dialog_sim(&rig, false, &pt());
    for text in [
        "Configurar anotação",
        "Profundidade por lance",
        "Iniciar",
        "Cancelar",
        "Escolha um livro de abertura em Ferramentas > Livros de abertura",
    ] {
        assert!(s.find(text).is_ok(), "missing {text:?}");
    }
}

#[test]
fn a_tiny_window_with_the_dialog_open_does_not_panic() {
    let (rig, _run) = dialog_rig();
    for size in [
        iced::Size::new(1.0, 1.0),
        iced::Size::new(120.0, 80.0),
        iced::Size::new(300.0, 200.0),
    ] {
        drop(rig.sim(size));
    }
    let element = annotate_dialog::view(&rig.shell.app, true, &en()).expect("open");
    drop(iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(50.0, 50.0),
        element,
    ));
}
