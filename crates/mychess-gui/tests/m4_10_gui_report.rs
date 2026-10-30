//! The game report panel seen from the window (spec M4-10,
//! `docs/specs/m4-10-relatorio-da-partida.md`, D1566 panel, D1567 curve and
//! markers, D1568 symbols and colours, D1569 guided review, D1571 write to
//! the game, D1572 recalculation without an engine, D1448 texts). Headless;
//! the engine answers with synthetic events, nothing is spawned.
#![allow(
    clippy::float_cmp,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::needless_range_loop
)]

mod m4_68_gui_common;

use iced::Rectangle;
use iced_test::selector::Candidate;
use m4_68_gui_common::*;
use mychess_app::report::{MoveClass, ReportMessage, ReportPreset, ReviewStep, SideFilter};
use mychess_app::{GameMessage, LanguagePref, PlayOptions};
use mychess_core::NodeId;
use mychess_gui::i18n::Language;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};

/// Scholar's mate. The engine's evaluations (white's point of view, in
/// centipawns) of the 8 positions, with its line: 3.Bc4 loses 12 points
/// (a mistake whose better move is 3.Nf3) and 3...Nf6 allows the mate (a
/// blunder whose better move is 3...g6).
const PLAN: [(i32, &[&str]); 8] = [
    (30, &["e2e4"]),
    (30, &["e7e5"]),
    (30, &["g1f3"]),
    (30, &["b8c6"]),
    (30, &["g1f3"]),
    (-120, &["g7g6"]),
    (3000, &["h5f7"]),
    (3000, &[]),
];

fn rmsg(m: ReportMessage) -> AppMessage {
    AppMessage::Report(m)
}

fn reports(messages: &[Message]) -> Vec<ReportMessage> {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::App(AppMessage::Report(r)) => Some(*r),
            _ => None,
        })
        .collect()
}

/// A rig with the Report pane open, Scholar's mate loaded and an engine.
fn base(language: LanguagePref) -> (Rig, RunId) {
    let mut rig = Rig::with_language(language);
    rig.set_game(game_of(&SCHOLAR));
    let run = uci_run(&mut rig);
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Report)));
    (rig, run)
}

/// Answers every search of a started run with `PLAN`; returns whether the
/// run ended.
fn answer_all(rig: &mut Rig, run: RunId, mut fx: Vec<Effect>) -> bool {
    let mut searches = Searches::new();
    for k in 0..30 {
        if !rig.shell.app.annotation_running() {
            return true;
        }
        let (cp, pv) = PLAN[k.min(7)];
        match searches.answer(rig, run, &fx, cp, pv) {
            Some(next) => fx = next,
            None => break,
        }
    }
    !rig.shell.app.annotation_running()
}

/// A rig with the finished report of Scholar's mate.
fn with_report(language: LanguagePref) -> (Rig, RunId) {
    let (mut rig, run) = base(language);
    let fx = up(&mut rig, rmsg(ReportMessage::Generate));
    assert!(rig.shell.app.report_running(), "the run started");
    assert!(answer_all(&mut rig, run, fx), "the run ended");
    assert!(rig.shell.app.report_data().is_some(), "a report");
    (rig, run)
}

fn english() -> (Rig, RunId) {
    with_report(LanguagePref::EnUs)
}

fn data(rig: &Rig) -> &mychess_app::report::ReportData {
    rig.shell.app.report_data().expect("a report")
}

/// The ply the cursor is after, on the main line (0 at the start).
fn cursor_ply(rig: &Rig) -> usize {
    let cursor = rig.cursor();
    if cursor == rig.game().root() {
        return 0;
    }
    rig.game()
        .mainline()
        .position(|n| n == cursor)
        .map_or(usize::MAX, |i| i + 1)
}

fn node_of_ply(rig: &Rig, ply: usize) -> NodeId {
    if ply == 0 {
        rig.game().root()
    } else {
        rig.game().mainline().nth(ply - 1).expect("ply")
    }
}

/// Applies the messages of a click to the application (effects dropped).
fn click_apply(rig: &mut Rig, label: &str) -> Vec<ReportMessage> {
    let sent = click_msgs(rig, label);
    let seen = reports(&sent);
    apply(rig, sent);
    seen
}

/// Clicks the `n`-th text (tree order) with this content.
fn click_nth(rig: &mut Rig, label: &str, n: usize) -> Vec<ReportMessage> {
    let at = nth_text_bounds(rig, label, n).center();
    let sent = click_at(rig, at);
    let seen = reports(&sent);
    apply(rig, sent);
    seen
}

/// Clicks the widget with this id.
fn click_id(rig: &mut Rig, id: &'static str) -> Vec<ReportMessage> {
    let at = rig
        .sim(SIZE)
        .find(iced::widget::Id::new(id))
        .expect("the widget")
        .bounds()
        .center();
    let sent = click_at(rig, at);
    let seen = reports(&sent);
    apply(rig, sent);
    seen
}

/// Clicks the first text that ends with `suffix` (a moment of the list).
fn click_text_ending(rig: &mut Rig, suffix: &str) -> Vec<ReportMessage> {
    let mut s = rig.sim(SIZE);
    let suffix_owned = suffix.to_owned();
    let found: Option<Rectangle> = s
        .find(move |c: Candidate<'_>| match c {
            Candidate::Text {
                content,
                visible_bounds,
                ..
            } if content.ends_with(&suffix_owned) => Some(visible_bounds),
            _ => None,
        })
        .unwrap_or_else(|e| panic!("a text ending in {suffix:?}: {e:?}"));
    drop(s);
    let at = found.expect("visible").center();
    let sent = click_at(rig, at);
    let seen = reports(&sent);
    apply(rig, sent);
    seen
}

fn has_text_ending(rig: &Rig, suffix: &str) -> bool {
    let suffix_owned = suffix.to_owned();
    rig.sim(SIZE)
        .find(move |c: Candidate<'_>| match c {
            Candidate::Text { content, .. } if content.ends_with(&suffix_owned) => Some(()),
            _ => None,
        })
        .is_ok()
}

fn nags_of(rig: &Rig, node: NodeId) -> Vec<u8> {
    rig.game()
        .nags(node)
        .unwrap()
        .iter()
        .map(|n| n.value())
        .collect()
}

// ---- opening the panel (D1421, D1566) ----

#[test]
fn the_tools_menu_has_a_report_item_that_toggles_the_report_pane() {
    let rig = Rig::new();
    let tools = menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options)
        .into_iter()
        .find(|s| s.label_key == "menu-tools")
        .expect("a Tools menu");
    let action = tools
        .entries
        .iter()
        .find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key("menu-tools-report"),
                action,
                checked,
                ..
            } => Some((action.clone(), *checked)),
            _ => None,
        })
        .expect("the report item");
    assert_eq!(action.1, Some(false), "closed pane, unchecked item");
    assert!(matches!(
        action.0,
        Some(Message::Ui(UiMessage::TogglePane(PaneKind::Report)))
    ));
}

#[test]
fn toggling_the_report_pane_opens_and_closes_it() {
    let mut rig = Rig::new();
    assert!(!rig.shell.ui.is_open(PaneKind::Report));
    assert!(!window_has(&rig, "Generate report"));
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Report)));
    assert!(rig.shell.ui.is_open(PaneKind::Report));
    assert!(window_has(&rig, "Generate report"));
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Report)));
    assert!(!rig.shell.ui.is_open(PaneKind::Report));
}

#[test]
fn the_report_pane_has_a_tab_title_in_both_languages() {
    let (rig, _run) = base(LanguagePref::EnUs);
    assert!(window_has(&rig, "Report"));
    let (rig, _run) = base(LanguagePref::PtBr);
    assert!(window_has(&rig, "Relatório"));
}

// ---- the empty panel and the presets ----

#[test]
fn the_empty_panel_offers_presets_generate_and_the_game_evaluations() {
    let (rig, _run) = base(LanguagePref::EnUs);
    for label in ["Fast", "Standard", "Deep", "Generate report"] {
        assert!(window_has(&rig, label), "{label}");
    }
    assert!(window_has(&rig, "Use the game's evaluations"));
    assert!(!window_has(&rig, "Clear"), "nothing to clear");
    assert!(!window_has(&rig, "Stop"), "nothing to cancel");
    assert!(!window_has(&rig, "Write to game"), "nothing to write");
}

#[test]
fn clicking_a_preset_chooses_it_and_it_sets_the_depth_of_the_run() {
    for (label, preset, depth) in [
        ("Fast", ReportPreset::Fast, 12),
        ("Standard", ReportPreset::Standard, 16),
        ("Deep", ReportPreset::Deep, 22),
    ] {
        let (mut rig, _run) = base(LanguagePref::EnUs);
        if preset == ReportPreset::Standard {
            // The default: pick another one first so that the click matters.
            click_apply(&mut rig, "Deep");
        }
        let seen = click_apply(&mut rig, label);
        assert_eq!(seen, vec![ReportMessage::SetPreset(preset)], "{label}");
        assert_eq!(rig.shell.app.report.preset, preset);
        let fx = up(&mut rig, rmsg(ReportMessage::Generate));
        let request = last_search(&fx).expect("the first search");
        assert_eq!(request.limits.depth, Some(depth), "{label}");
    }
}

#[test]
fn the_default_preset_is_standard_depth_16() {
    let (mut rig, _run) = base(LanguagePref::EnUs);
    assert_eq!(rig.shell.app.report.preset, ReportPreset::Standard);
    let fx = up(&mut rig, rmsg(ReportMessage::Generate));
    assert_eq!(last_search(&fx).expect("search").limits.depth, Some(16));
}

// ---- Generate / Cancel / Clear ----

#[test]
fn the_generate_button_starts_the_run_and_shows_progress_and_the_cancel_button() {
    let (mut rig, _run) = base(LanguagePref::EnUs);
    let seen = click_apply(&mut rig, "Generate report");
    assert_eq!(seen, vec![ReportMessage::Generate]);
    // The shell hands the message to the application; here the effects are
    // dropped, but the run exists.
    assert!(rig.shell.app.report_running());
    assert!(window_has(&rig, "Stop"), "the Cancel button");
    assert!(window_has(
        &rig,
        "Analyzing the game with the engine... the game cannot be edited meanwhile."
    ));
    assert!(
        reports(&click_msgs(&rig, "Generate report")).is_empty(),
        "no second run: the button is disabled"
    );
}

#[test]
fn while_running_the_generate_button_and_the_presets_do_nothing() {
    let (mut rig, _run) = base(LanguagePref::EnUs);
    click_apply(&mut rig, "Generate report");
    assert!(rig.shell.app.report_running());
    for label in ["Fast", "Deep"] {
        let sent = click_msgs(&rig, label);
        assert!(reports(&sent).is_empty(), "{label} is disabled: {sent:?}");
    }
    assert_eq!(rig.shell.app.report.preset, ReportPreset::Standard);
}

#[test]
fn the_cancel_button_sends_cancel_and_keeps_a_partial_report() {
    let (mut rig, run) = base(LanguagePref::EnUs);
    let mut fx = up(&mut rig, rmsg(ReportMessage::Generate));
    let mut searches = Searches::new();
    for k in 0..4 {
        let (cp, pv) = PLAN[k];
        fx = searches.answer(&mut rig, run, &fx, cp, pv).expect("search");
    }
    assert!(rig.shell.app.report_running());
    let sent = click_msgs(&rig, "Stop");
    let seen = reports(&sent);
    assert!(
        seen.contains(&ReportMessage::Cancel),
        "Stop of the panel sends Cancel, got {sent:?}"
    );
    let cancel_fx = {
        let mut out = Vec::new();
        for m in sent {
            if let Message::App(a) = m {
                out.extend(mychess_app::update(&mut rig.shell.app, a));
            }
        }
        out
    };
    // The engine acknowledges the stop with its last search, if one is left.
    let mut pending = cancel_fx;
    if pending.is_empty() {
        pending = fx;
    }
    for _ in 0..4 {
        if !rig.shell.app.annotation_running() {
            break;
        }
        if let Some(next) = searches.answer(&mut rig, run, &pending, 30, &[]) {
            pending = next;
        } else {
            break;
        }
    }
    rig.sync();
    assert!(!rig.shell.app.report_running(), "the run ended");
    let kept = rig
        .shell
        .app
        .report_data()
        .expect("the positions analysed are kept");
    assert!(!kept.complete, "a partial report");
    assert!(kept.report.moves.len() < kept.plies);
    let text = format!(
        "Partial report: {} of {} moves analyzed.",
        kept.report.moves.len(),
        kept.plies
    );
    assert!(window_has(&rig, &text), "{text}");
}

#[test]
fn the_clear_button_forgets_the_report_and_the_empty_text_comes_back() {
    let (mut rig, _run) = english();
    assert!(!window_has(
        &rig,
        "Analyze the whole game with the engine to see how each move compares with the best one. The game is not changed."
    ));
    let seen = click_apply(&mut rig, "Clear");
    assert_eq!(seen, vec![ReportMessage::Clear]);
    assert!(rig.shell.app.report_data().is_none());
    assert!(window_has(
        &rig,
        "Analyze the whole game with the engine to see how each move compares with the best one. The game is not changed."
    ));
    assert!(window_has(&rig, "Generate report"));
    assert!(!window_has(&rig, "Clear"));
}

#[test]
fn a_finished_report_offers_analyse_again_instead_of_generate() {
    let (rig, _run) = english();
    assert!(window_has(&rig, "Analyze again"));
    assert!(!window_has(&rig, "Generate report"));
    assert!(window_has(&rig, "Write to game"));
}

#[test]
fn the_report_is_not_changing_the_game_by_itself() {
    let (rig, _run) = english();
    assert!(
        rig.game()
            .mainline()
            .all(|n| rig.game().comment(n).unwrap().is_none() && nags_of(&rig, n).is_empty()),
        "D1447: without Write to game the game is not altered"
    );
}

// ---- accuracy, counts, symbols (D1433, D1430, D1568) ----

#[test]
fn the_panel_shows_one_accuracy_box_per_side_with_one_decimal() {
    let (rig, _run) = english();
    let d = data(&rig);
    for accuracy in [d.report.white.accuracy, d.report.black.accuracy] {
        let value = accuracy.expect("a side with moves has an accuracy");
        assert!((0.0..=100.0).contains(&value), "{value}");
        assert!(window_has(&rig, &format!("{value:.1}")), "{value:.1}");
    }
    assert_eq!(count_text(&rig, "Accuracy"), 2);
    assert!(
        d.report.white.accuracy > d.report.black.accuracy,
        "the side that blundered into mate is less accurate"
    );
}

#[test]
fn in_portuguese_the_accuracy_uses_a_decimal_comma() {
    let (rig, _run) = with_report(LanguagePref::PtBr);
    let d = data(&rig);
    let value = d.report.black.accuracy.expect("accuracy");
    let shown = format!("{value:.1}").replace('.', ",");
    assert!(window_has(&rig, &shown), "{shown}");
    assert!(shown.contains(','));
}

#[test]
fn the_counts_table_lists_every_class_and_adds_up_to_the_moves_of_each_side() {
    let (rig, _run) = english();
    let d = data(&rig);
    for name in [
        "Brilliant",
        "Great",
        "Best",
        "Excellent",
        "Good",
        "Book",
        "Inaccuracy",
        "Mistake",
        "Miss",
        "Blunder",
    ] {
        assert!(window_has(&rig, name), "class {name}");
    }
    let total = |side: &mychess_app::report::SideReport| -> u32 {
        MoveClass::ALL.iter().map(|c| side.count(*c)).sum()
    };
    assert_eq!(total(&d.report.white), 4, "White played 4 moves");
    assert_eq!(total(&d.report.black), 3, "Black played 3 moves");
    assert_eq!(d.report.white.count(MoveClass::Mistake), 1);
    assert_eq!(d.report.black.count(MoveClass::Blunder), 1);
    assert_eq!(d.report.white.count(MoveClass::Blunder), 0);
}

#[test]
fn every_class_has_a_distinct_symbol_and_the_name_stays_next_to_it() {
    // D1568: symbols ASCII, never colour alone.
    let (rig, _run) = english();
    for glyph in ["??", "?", "?!"] {
        assert!(window_has(&rig, glyph), "symbol {glyph}");
    }
    assert!(window_has(&rig, "Blunder") && window_has(&rig, "Mistake"));
}

// ---- the list of moments (D1436, D1566) ----

#[test]
fn the_list_shows_the_faults_in_game_order_with_the_loss() {
    let (rig, _run) = english();
    assert!(window_has(&rig, "Moves to look at again"));
    assert!(has_text_ending(&rig, " Bc4"), "the mistake");
    assert!(has_text_ending(&rig, " Nf6"), "the blunder");
    assert!(!has_text_ending(&rig, " Qh5"), "a good move is not listed");
    let d = data(&rig);
    assert_eq!(d.moments(None), vec![5, 6]);
    let loss = format!("-{:.0} pts", d.report.moves[5].loss);
    assert!(window_has(&rig, &loss), "{loss}");
}

#[test]
fn clicking_a_moment_goes_to_its_move() {
    let (mut rig, _run) = english();
    rig.goto(rig.game().root());
    let seen = click_text_ending(&mut rig, " Nf6");
    assert_eq!(seen, vec![ReportMessage::GoTo(6)]);
    assert_eq!(cursor_ply(&rig), 6, "the board is after 3...Nf6");
    let seen = click_text_ending(&mut rig, " Bc4");
    assert_eq!(seen, vec![ReportMessage::GoTo(5)]);
    assert_eq!(cursor_ply(&rig), 5);
}

#[test]
fn clicking_a_class_filters_the_list_and_clicking_it_again_clears_the_filter() {
    let (mut rig, _run) = english();
    let seen = click_nth(&mut rig, "Blunder", 0);
    assert_eq!(seen, vec![ReportMessage::Filter(Some(MoveClass::Blunder))]);
    assert_eq!(rig.shell.app.report.filter, Some(MoveClass::Blunder));
    assert!(window_has(&rig, "Moves of the chosen class"));
    assert!(has_text_ending(&rig, " Nf6"));
    assert!(
        !has_text_ending(&rig, " Bc4"),
        "the mistake is filtered out"
    );
    let seen = click_nth(&mut rig, "Blunder", 0);
    assert_eq!(seen, vec![ReportMessage::Filter(None)]);
    assert!(has_text_ending(&rig, " Bc4"));
}

#[test]
fn filtering_a_class_with_no_move_says_so() {
    let (mut rig, _run) = english();
    let seen = click_nth(&mut rig, "Brilliant", 0);
    assert_eq!(
        seen,
        vec![ReportMessage::Filter(Some(MoveClass::Brilliant))]
    );
    assert!(window_has(&rig, "No move to show."));
}

// ---- the curve (D1567) ----

#[test]
fn the_curve_has_a_marker_for_each_fault_and_none_for_quiet_moves() {
    let (rig, _run) = english();
    let kinds = |ply: usize| -> Option<_> {
        let class = data(&rig).report.moves[ply - 1].class;
        mychess_app::report::marker_kind(class)
    };
    assert!(kinds(5).is_some(), "mistake");
    assert!(kinds(6).is_some(), "blunder");
    assert!(kinds(3).is_none(), "a quiet move draws no marker");
    let line = data(&rig).graph.line.as_ref().expect("a line");
    assert_eq!(line.markers.len(), 2, "{:?}", line.markers);
}

// ---- guided review (D1435, D1569) ----

fn start_both(rig: &mut Rig) {
    let seen = click_apply(rig, "Both");
    assert_eq!(
        seen,
        vec![ReportMessage::StartReview {
            side: SideFilter::Both,
            highlights: false
        }]
    );
}

#[test]
fn the_panel_offers_to_review_each_side_or_both() {
    let (mut rig, _run) = english();
    assert!(window_has(&rig, "Review the moves of:"));
    let seen = click_id(&mut rig, "report-review-white");
    assert_eq!(
        seen,
        vec![ReportMessage::StartReview {
            side: SideFilter::White,
            highlights: false
        }]
    );
    let review = rig.shell.app.report.review.clone().expect("a review");
    assert_eq!(review.moments.len(), 1, "White has one fault");
    assert_eq!(review.current().unwrap().ply, 5);
    click_apply(&mut rig, "Exit");
    let seen = click_id(&mut rig, "report-review-black");
    assert_eq!(
        seen,
        vec![ReportMessage::StartReview {
            side: SideFilter::Black,
            highlights: false
        }]
    );
    let review = rig.shell.app.report.review.clone().expect("a review");
    assert_eq!(review.moments.len(), 1, "Black has one fault");
    assert_eq!(review.current().unwrap().ply, 6);
}

#[test]
fn starting_the_review_shows_the_card_and_goes_to_the_position_before_the_move() {
    let (mut rig, _run) = english();
    // The walk has its own tests below.
    rig.shell.app.prefs.board.animation_ms = 0;
    start_both(&mut rig);
    assert!(window_has(&rig, "Moment 1 of 2"));
    assert!(
        !window_has(&rig, "Review the moves of:"),
        "the card replaces the buttons"
    );
    assert_eq!(cursor_ply(&rig), 4, "before 3.Bc4");
    for label in ["Try again", "See the best move", "Previous", "Next", "Exit"] {
        assert!(window_has(&rig, label), "{label}");
    }
    assert!(
        window_has(
            &rig,
            &format!(
                "Bc4 was played, Mistake: it loses about {:.0} points. The best move was Nf3.",
                data(&rig).report.moves[4].loss
            )
        ),
        "the sentence uses only computed facts (D1448)"
    );
}

#[test]
fn previous_is_disabled_on_the_first_moment_and_next_on_the_last() {
    let (mut rig, _run) = english();
    start_both(&mut rig);
    assert!(reports(&click_msgs(&rig, "Previous")).is_empty());
    let seen = click_apply(&mut rig, "Next");
    assert_eq!(seen, vec![ReportMessage::ReviewNext]);
    assert!(window_has(&rig, "Moment 2 of 2"));
    assert_eq!(cursor_ply(&rig), 5, "before 3...Nf6");
    assert!(reports(&click_msgs(&rig, "Next")).is_empty());
    let seen = click_apply(&mut rig, "Previous");
    assert_eq!(seen, vec![ReportMessage::ReviewPrevious]);
    assert!(window_has(&rig, "Moment 1 of 2"));
    assert_eq!(cursor_ply(&rig), 4);
}

#[test]
fn see_the_best_move_reveals_it_without_changing_the_game() {
    let (mut rig, _run) = english();
    start_both(&mut rig);
    let before = rig.game().mainline().count();
    let seen = click_apply(&mut rig, "See the best move");
    assert_eq!(seen, vec![ReportMessage::ReviewShowBest]);
    assert_eq!(
        rig.shell.app.report.review.as_ref().unwrap().step,
        ReviewStep::Revealed
    );
    assert!(window_has(&rig, "The best move was Nf3."));
    assert_eq!(rig.game().mainline().count(), before);
}

#[test]
fn try_again_accepts_only_the_best_move_and_wrong_attempts_never_enter_the_game() {
    let (mut rig, _run) = english();
    start_both(&mut rig);
    let seen = click_apply(&mut rig, "Try again");
    assert_eq!(seen, vec![ReportMessage::ReviewTry]);
    assert!(window_has(
        &rig,
        "Find a better move than Bc4 on the board."
    ));
    assert!(
        reports(&click_msgs(&rig, "Try again")).is_empty(),
        "already trying"
    );

    let ply = 5;
    let best = data(&rig).report.moves[ply - 1].best.expect("best");
    let played = data(&rig).report.moves[ply - 1].played;
    let position = rig.game().position_at(rig.cursor()).unwrap();
    let wrong = position
        .legal_moves()
        .into_iter()
        .find(|m| *m != best && *m != played)
        .expect("another legal move");
    let size_before = rig.game().mainline().count();
    let nodes_before = rig.game().node_count();

    for _ in 0..2 {
        up(
            &mut rig,
            AppMessage::Game(GameMessage::PlayMove {
                mv: wrong,
                options: PlayOptions::default(),
            }),
        );
    }
    assert_eq!(rig.game().mainline().count(), size_before);
    assert_eq!(rig.game().node_count(), nodes_before, "no variation either");
    assert_eq!(rig.shell.app.report.review.as_ref().unwrap().attempts, 2);
    assert_eq!(cursor_ply(&rig), 4, "the board stays at the moment");
    assert!(
        window_has(&rig, "Not that one. Hint: move the piece on g1."),
        "two failures give the hint"
    );

    up(
        &mut rig,
        AppMessage::Game(GameMessage::PlayMove {
            mv: best,
            options: PlayOptions::default(),
        }),
    );
    assert_eq!(
        rig.shell.app.report.review.as_ref().unwrap().step,
        ReviewStep::Solved
    );
    assert!(window_has(&rig, "Correct! Nf3 is the best move."));
    assert_eq!(
        rig.game().node_count(),
        nodes_before,
        "still not in the game"
    );
}

#[test]
fn exit_closes_the_card_and_the_start_buttons_come_back() {
    let (mut rig, _run) = english();
    start_both(&mut rig);
    let seen = click_apply(&mut rig, "Exit");
    assert_eq!(seen, vec![ReportMessage::ReviewExit]);
    assert!(rig.shell.app.report.review.is_none());
    assert!(window_has(&rig, "Review the moves of:"));
    assert!(!window_has(&rig, "Moment 1 of 2"));
}

#[test]
fn a_review_started_in_portuguese_uses_the_portuguese_texts() {
    let (mut rig, _run) = with_report(LanguagePref::PtBr);
    let pt = I18n::new(Language::PtBr);
    let both = pt.tr("report-review-both");
    let sent = click_msgs(&rig, &both);
    apply(&mut rig, sent);
    for key in [
        "report-review-try",
        "report-review-show-best",
        "report-review-previous",
        "report-review-next",
        "report-review-exit",
    ] {
        let label = pt.tr(key);
        assert!(window_has(&rig, &label), "{key} = {label}");
    }
    assert_ne!(pt.tr("report-review-try"), "Try again");
}

// ---- write to the game (D1571, D1440, D1447) ----

#[test]
fn write_to_game_adds_evals_and_nags_in_one_undo_step() {
    let (mut rig, _run) = english();
    let seen = click_apply(&mut rig, "Write to game");
    assert_eq!(seen, vec![ReportMessage::WriteToGame]);
    let n5 = node_of_ply(&rig, 5);
    let n6 = node_of_ply(&rig, 6);
    assert_eq!(nags_of(&rig, n5), vec![2], "? for the mistake");
    assert_eq!(nags_of(&rig, n6), vec![4], "?? for the blunder");
    let comment = rig.game().comment(n6).unwrap().unwrap_or_default();
    assert!(comment.contains("[%eval"), "{comment:?}");
    let best = node_of_ply(&rig, 1);
    assert!(
        nags_of(&rig, best).is_empty(),
        "D1571: a best move gets no mark"
    );
    // One undo step puts the game back.
    up(&mut rig, AppMessage::Game(GameMessage::Undo));
    for n in rig.game().mainline() {
        assert!(nags_of(&rig, n).is_empty());
        assert!(rig.game().comment(n).unwrap().is_none());
    }
}

#[test]
fn writing_twice_does_not_repeat_the_evals() {
    let (mut rig, _run) = english();
    click_apply(&mut rig, "Write to game");
    let comments: Vec<_> = rig
        .game()
        .mainline()
        .map(|n| rig.game().comment(n).unwrap().map(str::to_owned))
        .collect();
    click_apply(&mut rig, "Write to game");
    let again: Vec<_> = rig
        .game()
        .mainline()
        .map(|n| rig.game().comment(n).unwrap().map(str::to_owned))
        .collect();
    assert_eq!(comments, again, "[%eval] is omitted when already present");
    for c in again.iter().flatten() {
        assert!(c.matches("[%eval").count() <= 1, "{c:?}");
    }
}

// ---- recalculation without an engine (D1572, D1573) ----

#[test]
fn the_game_evaluations_button_rebuilds_the_classes_without_an_engine() {
    let (mut rig, _run) = english();
    let before: Vec<MoveClass> = data(&rig).report.moves.iter().map(|m| m.class).collect();
    click_apply(&mut rig, "Write to game");
    click_apply(&mut rig, "Clear");
    assert!(window_has(&rig, "Use the game's evaluations"));
    let seen = click_apply(&mut rig, "Use the game's evaluations");
    assert_eq!(seen, vec![ReportMessage::FromGame]);
    assert!(!rig.shell.app.annotation_running(), "no run, no engine");
    let after: Vec<MoveClass> = data(&rig).report.moves.iter().map(|m| m.class).collect();
    assert_eq!(after.len(), before.len());
    assert_eq!(after[4], MoveClass::Mistake);
    assert_eq!(after[5], MoveClass::Blunder);
    assert_eq!(
        before[4], after[4],
        "D1573: the trip through the comments keeps the loss classes"
    );
    assert_eq!(before[5], after[5]);
}

#[test]
fn a_review_from_the_game_evaluations_cannot_try_or_show_the_best_move() {
    let (mut rig, _run) = english();
    click_apply(&mut rig, "Write to game");
    click_apply(&mut rig, "Clear");
    click_apply(&mut rig, "Use the game's evaluations");
    start_both(&mut rig);
    assert!(window_has(&rig, "Moment 1 of 2"));
    assert!(reports(&click_msgs(&rig, "Try again")).is_empty());
    assert!(reports(&click_msgs(&rig, "See the best move")).is_empty());
    assert!(
        window_has(
            &rig,
            &format!(
                "Bc4 was played, Mistake: it loses about {:.0} points.",
                data(&rig).report.moves[4].loss
            )
        ),
        "D1570: only the verdict"
    );
}

#[test]
fn without_evaluations_in_the_game_there_is_no_report_and_a_notice() {
    let (mut rig, _run) = base(LanguagePref::EnUs);
    let seen = click_apply(&mut rig, "Use the game's evaluations");
    assert_eq!(seen, vec![ReportMessage::FromGame]);
    assert!(rig.shell.app.report_data().is_none());
    assert!(
        window_has(
            &rig,
            "The game has no evaluations ([%eval]) to build a report from."
        ),
        "the status bar says why"
    );
}

// ---- stale reports ----

#[test]
fn an_edit_of_the_main_line_discards_the_report_and_the_panel_goes_back_to_empty() {
    let (mut rig, _run) = english();
    rig.set_game(game_of(&SCHOLAR[..4]));
    assert!(rig.shell.app.report_data().is_none());
    assert!(window_has(&rig, "Generate report"));
}

// ---- Fluent parity (D1448) ----

fn report_ids(language: &str) -> std::collections::BTreeSet<String> {
    ids(&ftl_text(language, "graph.ftl"))
        .into_iter()
        .filter(|id| id.starts_with("report-") || id.ends_with("-report"))
        .collect()
}

#[test]
fn the_report_keys_exist_in_both_languages_and_match() {
    let en_ids = report_ids("en-US");
    let pt_ids = report_ids("pt-BR");
    assert!(en_ids.len() >= 40, "{} keys", en_ids.len());
    assert_eq!(
        en_ids.difference(&pt_ids).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "missing in pt-BR"
    );
    assert_eq!(
        pt_ids.difference(&en_ids).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "missing in en-US"
    );
}

#[test]
fn every_key_the_panel_uses_resolves_in_both_languages() {
    let mut keys: Vec<String> = [
        "menu-tools-report",
        "pane-report",
        "report-side-white",
        "report-side-black",
        "report-accuracy",
        "report-generate",
        "report-regenerate",
        "report-clear",
        "report-empty",
        "report-running",
        "report-partial",
        "report-moments",
        "report-moments-filtered",
        "report-no-moments",
        "report-moment-loss",
        "report-estimate-note",
        "report-progress",
        "report-progress-starting",
        "report-write",
        "report-write-tip",
        "report-from-game",
        "report-from-game-tip",
        "report-review-start",
        "report-review-both",
        "report-review-title",
        "report-review-looking-best",
        "report-review-looking",
        "report-review-trying",
        "report-review-hint",
        "report-review-solved",
        "report-review-revealed",
        "report-review-try",
        "report-review-show-best",
        "report-review-previous",
        "report-review-next",
        "report-review-exit",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    keys.extend(ReportPreset::ALL.iter().map(|p| p.label_key().to_owned()));
    keys.extend(MoveClass::ALL.iter().map(|c| c.name_key().to_owned()));
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in &keys {
            assert!(i18n.has(key), "{language:?}: missing {key}");
            assert_ne!(i18n.tr(key), *key, "{language:?}: {key} resolves to itself");
        }
    }
}

#[test]
fn the_portuguese_texts_are_translated_not_copied_from_english() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    for key in [
        "report-generate",
        "report-clear",
        "report-write",
        "report-review-try",
        "report-review-next",
        "report-class-blunder",
        "report-preset-deep",
    ] {
        assert_ne!(en.tr(key), pt.tr(key), "{key}");
    }
}

// ---- the walk and the arrow of the review (M4-11) ----

#[test]
fn the_review_walks_to_the_moment_and_draws_the_arrow_when_it_arrives() {
    let (mut rig, _run) = english();
    rig.shell.app.prefs.board.animation_ms = 200;
    start_both(&mut rig);
    // The effects were dropped by the rig, so the walk is waiting for its
    // timer: no arrow yet.
    assert!(rig.shell.app.report.walk.is_some(), "a walk is in progress");
    assert!(rig.shell.app.board_arrows().is_empty());
    // An action of the user ends it at the moment, and the arrow shows.
    rig.msg(Message::App(AppMessage::Report(
        ReportMessage::ReviewPrevious,
    )));
    assert!(rig.shell.app.report.walk.is_none());
    assert_eq!(cursor_ply(&rig), 4, "before 3.Bc4");
    let arrows = rig.shell.app.board_arrows();
    assert_eq!(arrows.len(), 1);
    assert_eq!(arrows[0].kind, mychess_app::ArrowKind::ReviewBest);
    // "Try again" hides it; "See the best move" brings it back.
    click_apply(&mut rig, "Try again");
    assert!(rig.shell.app.board_arrows().is_empty());
    click_apply(&mut rig, "See the best move");
    assert_eq!(rig.shell.app.board_arrows().len(), 1);
}

#[test]
fn the_card_has_the_arrow_checkbox_and_it_edits_the_preference() {
    let (mut rig, _run) = english();
    rig.shell.app.prefs.board.animation_ms = 0;
    start_both(&mut rig);
    assert!(rig.shell.app.prefs.report.review_arrow);
    assert_eq!(rig.shell.app.board_arrows().len(), 1);
    let mut sim = rig.sim(SIZE);
    sim.click("Show the arrow of the best move")
        .expect("checkbox");
    let sent: Vec<Message> = sim.into_messages().collect();
    for m in sent {
        rig.msg(m);
    }
    assert!(!rig.shell.app.prefs.report.review_arrow);
    assert!(
        rig.shell.app.board_arrows().is_empty(),
        "off: no arrow in the verdict"
    );
}
