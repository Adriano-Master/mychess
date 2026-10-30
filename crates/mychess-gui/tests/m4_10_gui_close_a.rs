//! M4-10 close-out seen from the window (spec
//! `docs/specs/m4-10-relatorio-da-partida.md`, D1434 marks, D1437 side of the
//! user, D1443 continue, D1444 time left, D2100-D2121): the "Configure
//! report" dialog, the panel, the arrow keys of the review and the class
//! badge of the board. Headless; the engine answers with synthetic events.
#![allow(
    clippy::float_cmp,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

mod m4_68_gui_common;

use iced_test::selector::Candidate;
use m4_68_gui_common::*;
use mychess_app::report::{MoveClass, ReportConfig, ReportMessage, SideFilter};
use mychess_app::{GameMessage, LanguagePref, Nav};
use mychess_core::Color as Side;
use mychess_gui::i18n::Language;
use mychess_gui::theme::contrast::{MIN_TEXT, contrast_ratio};
use mychess_gui::theme::report_colors::{class_color, class_glyph};
use mychess_gui::theme::{Tokens, contrast_on};

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

fn base(language: LanguagePref, white: &str, black: &str) -> (Rig, RunId) {
    let mut rig = Rig::with_language(language);
    let mut game = game_of(&SCHOLAR);
    white.clone_into(&mut game.tags_mut().white);
    black.clone_into(&mut game.tags_mut().black);
    rig.set_game(game);
    let run = uci_run(&mut rig);
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Report)));
    (rig, run)
}

/// Answers up to `max` searches with `PLAN`.
fn answer(rig: &mut Rig, run: RunId, mut fx: Vec<Effect>, max: usize) -> Vec<Effect> {
    let mut searches = Searches::new();
    for k in 0..max {
        if !rig.shell.app.annotation_running() {
            break;
        }
        let (cp, pv) = PLAN[k.min(7)];
        match searches.answer(rig, run, &fx, cp, pv) {
            Some(next) => fx = next,
            None => break,
        }
    }
    fx
}

fn with_report(language: LanguagePref, white: &str, black: &str) -> (Rig, RunId) {
    let (mut rig, run) = base(language, white, black);
    let fx = up(&mut rig, rmsg(ReportMessage::Generate));
    answer(&mut rig, run, fx, 30);
    assert!(!rig.shell.app.annotation_running());
    assert!(rig.shell.app.report_data().is_some(), "a report");
    (rig, run)
}

fn english() -> (Rig, RunId) {
    with_report(LanguagePref::EnUs, "Ana", "Bruno")
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

fn review_index(rig: &Rig) -> usize {
    rig.shell.app.report.review.as_ref().expect("review").index
}

// ---- the dialog (D2100) ----

#[test]
fn the_configure_button_opens_the_dialog_and_it_is_a_modal_layer() {
    let (mut rig, _) = base(LanguagePref::EnUs, "Ana", "Bruno");
    assert!(window_has(&rig, "Configure..."));
    assert_eq!(active(&rig), None);
    assert!(!rig.shell.modal_layer_open());
    let sent = click_msgs(&rig, "Configure...");
    assert_eq!(reports(&sent), [ReportMessage::OpenConfig]);
    apply(&mut rig, sent);
    assert_eq!(active(&rig), Some(ActiveDialog::ReportConfig));
    assert!(rig.shell.modal_layer_open(), "a modal layer");
    for text in [
        "Configure report",
        "Limit of each search",
        "Restore defaults",
        "Cancel",
        "Best: up to",
        "Inaccuracy: up to",
        "Show the class of the move on its square of the board",
    ] {
        assert!(window_has(&rig, text), "{text}");
    }
}

#[test]
fn the_dialog_speaks_portuguese_with_the_names_of_the_classes() {
    let (mut rig, _) = base(LanguagePref::PtBr, "Ana", "Bruno");
    up(&mut rig, rmsg(ReportMessage::OpenConfig));
    for text in [
        "Configurar relatório",
        "Limite de cada busca",
        "Restaurar padrões",
        "Melhor: até",
        "Imprecisão: até",
    ] {
        assert!(window_has(&rig, text), "{text}");
    }
}

#[test]
fn escape_closes_the_dialog_and_enter_starts_the_report() {
    let (mut rig, _) = base(LanguagePref::EnUs, "Ana", "Bruno");
    up(&mut rig, rmsg(ReportMessage::OpenConfig));
    esc(&mut rig);
    assert_eq!(active(&rig), None);
    assert!(!rig.shell.app.report_running());

    up(&mut rig, rmsg(ReportMessage::OpenConfig));
    enter(&mut rig);
    assert_eq!(active(&rig), None);
    assert!(
        rig.shell.app.report_running(),
        "Enter is the Generate button"
    );
}

#[test]
fn the_buttons_of_the_dialog_send_their_messages() {
    let (mut rig, _) = base(LanguagePref::EnUs, "Ana", "Bruno");
    up(&mut rig, rmsg(ReportMessage::OpenConfig));
    let sent = click_msgs(&rig, "Restore defaults");
    assert_eq!(reports(&sent), [ReportMessage::ConfigDefaults]);
    let sent = click_msgs(&rig, "Cancel");
    assert_eq!(reports(&sent), [ReportMessage::CloseConfig]);
}

#[test]
fn a_stepper_of_the_dialog_sends_the_limits_one_step_on() {
    let (mut rig, _) = base(LanguagePref::EnUs, "Ana", "Bruno");
    up(&mut rig, rmsg(ReportMessage::OpenConfig));
    // Some "+" of the window belong to other widgets: the one of the depth
    // is the first whose click asks for a new configuration.
    let (sent, config) = (0..6)
        .find_map(|n| {
            let at = nth_text_bounds(&rig, "+", n).center();
            let sent = click_at(&rig, at);
            let config = reports(&sent).into_iter().find_map(|m| match m {
                ReportMessage::ConfigSet(c) => Some(c),
                _ => None,
            })?;
            Some((sent, config))
        })
        .expect("a ConfigSet");
    assert_eq!(config.depth, ReportConfig::default().depth + 1);
    apply(&mut rig, sent);
    assert_eq!(
        rig.shell.app.report.config_dialog.unwrap().config.depth,
        config.depth
    );
}

// ---- the panel ----

#[test]
fn a_partial_report_offers_to_continue_and_a_complete_one_does_not() {
    let (mut rig, run) = base(LanguagePref::EnUs, "Ana", "Bruno");
    let fx = up(&mut rig, rmsg(ReportMessage::Generate));
    let fx = answer(&mut rig, run, fx, 4);
    assert!(rig.shell.app.report_running());
    assert!(!window_has(&rig, "Continue analysis"), "while it runs");
    drop(fx);
    up(&mut rig, rmsg(ReportMessage::Cancel));
    assert!(!rig.shell.app.report_running());
    assert!(rig.shell.app.report_data().is_some_and(|d| !d.complete));
    assert!(window_has(&rig, "Continue analysis"));
    let sent = click_msgs(&rig, "Continue analysis");
    assert_eq!(reports(&sent), [ReportMessage::Continue]);
    apply(&mut rig, sent);
    assert!(rig.shell.app.report_running(), "it went on");

    let (rig, _) = english();
    assert!(!window_has(&rig, "Continue analysis"));
}

#[test]
fn the_side_of_the_user_is_framed_and_named_in_both_languages() {
    let (mut rig, _) = english();
    rig.shell.app.report.user_side = None;
    assert!(window_has(&rig, "I played:"));
    assert!(window_has(
        &rig,
        "Choose your side to see your accuracy and your mistakes first."
    ));
    assert!(!window_has(&rig, "Black (you)"));
    up(
        &mut rig,
        rmsg(ReportMessage::SetUserSide(Some(Side::Black))),
    );
    assert!(window_has(&rig, "Black (you)"));
    assert!(!window_has(&rig, "White (you)"));
    assert!(!window_has(
        &rig,
        "Choose your side to see your accuracy and your mistakes first."
    ));

    let (mut pt, _) = with_report(LanguagePref::PtBr, "Ana", "Bruno");
    up(&mut pt, rmsg(ReportMessage::SetUserSide(Some(Side::White))));
    assert!(window_has(&pt, "Brancas (você)"));
    assert!(window_has(&pt, "Joguei de:"));
}

#[test]
fn the_names_of_the_game_choose_the_side_without_asking() {
    let (mut rig, run) = base(LanguagePref::EnUs, "Ana", "Bruno");
    rig.shell.app.prefs.play.player_name = "Ana".to_owned();
    let fx = up(&mut rig, rmsg(ReportMessage::Generate));
    answer(&mut rig, run, fx, 30);
    assert_eq!(rig.shell.app.report.user_side, Some(Side::White));
    assert!(window_has(&rig, "White (you)"));
    assert!(!window_has(
        &rig,
        "Choose your side to see your accuracy and your mistakes first."
    ));
}

#[test]
fn the_user_can_pick_the_side_with_the_buttons_of_the_row() {
    let (rig, _) = english();
    // "Not set" only exists in this row.
    let sent = click_msgs(&rig, "Not set");
    assert_eq!(reports(&sent), [ReportMessage::SetUserSide(None)]);
}

// ---- the time left (D2121) ----

#[test]
fn the_status_bar_shows_the_time_left_once_the_pace_is_known() {
    let (mut rig, run) = base(LanguagePref::EnUs, "Ana", "Bruno");
    let fx = up(&mut rig, rmsg(ReportMessage::Generate));
    let stamp = fx
        .iter()
        .find_map(|e| match e {
            Effect::ScheduleReviewTick { stamp, .. } => Some(*stamp),
            _ => None,
        })
        .expect("the clock timer");
    for _ in 0..4 {
        up(&mut rig, rmsg(ReportMessage::WalkTick { stamp }));
    }
    assert!(!has_text_ending(&rig, "left"), "no pace yet");
    answer(&mut rig, run, fx, 3);
    assert!(rig.shell.app.report_eta_seconds().is_some());
    assert!(has_text_ending(&rig, "left"), "about m:ss left");
}

// ---- the arrow keys of the review (D2118) ----

#[test]
fn the_arrow_keys_of_the_window_visit_the_moments_of_the_review() {
    let (mut rig, _) = english();
    rig.shell.app.prefs.board.animation_ms = 0;
    up(
        &mut rig,
        rmsg(ReportMessage::StartReview {
            side: SideFilter::Both,
            highlights: false,
        }),
    );
    assert!(rig.shell.app.report.review.as_ref().unwrap().moments.len() >= 2);
    assert_eq!(review_index(&rig), 0);
    let first = cursor_ply(&rig);
    rig.press(named(NamedKey::Right));
    assert_eq!(review_index(&rig), 1);
    assert_ne!(cursor_ply(&rig), first, "the board followed");
    rig.press(named(NamedKey::Left));
    assert_eq!(review_index(&rig), 0);
    assert_eq!(cursor_ply(&rig), first);
    assert!(window_has(
        &rig,
        "The Left and Right keys go to the previous and the next moment."
    ));
}

#[test]
fn the_arrow_keys_are_the_game_ones_when_there_is_no_review() {
    let (mut rig, _) = english();
    up(&mut rig, GameMessage::Navigate(Nav::Start));
    rig.press(named(NamedKey::Right));
    assert_eq!(cursor_ply(&rig), 1);
}

// ---- the class badge of the board (D1434) ----

#[test]
fn the_badge_is_the_glyph_and_the_colour_of_the_class_of_the_move() {
    let (mut rig, _) = english();
    up(&mut rig, GameMessage::Navigate(Nav::Start));
    assert_eq!(rig.shell.class_badge(), None, "no move at the start");
    // 3...Nf6 (ply 6) allows the mate: a blunder.
    up(&mut rig, GameMessage::Navigate(Nav::GoToMainlinePly(6)));
    let badge = rig.shell.class_badge().expect("a badge");
    let class = rig.shell.app.report_data().unwrap().report.moves[5].class;
    assert_eq!(class, MoveClass::Blunder);
    assert_eq!(badge.glyph, class_glyph(class));
    assert_eq!(badge.square.to_string(), "f6");
    let tokens = Tokens::of(&rig.shell.theme());
    assert_eq!(badge.color, class_color(&tokens, class));
    // The board is drawn with it.
    assert!(window_has(&rig, "Black"), "the window still draws");
}

#[test]
fn the_badge_goes_away_with_the_marks_the_report_and_the_review() {
    let (mut rig, _) = english();
    up(&mut rig, GameMessage::Navigate(Nav::GoToMainlinePly(6)));
    assert!(rig.shell.class_badge().is_some());
    rig.shell.app.prefs.report_config.board_marks = false;
    assert_eq!(rig.shell.class_badge(), None);
    rig.shell.app.prefs.report_config.board_marks = true;
    up(&mut rig, rmsg(ReportMessage::Clear));
    assert_eq!(rig.shell.class_badge(), None);
}

#[test]
fn the_glyph_of_every_class_reads_on_its_disc_in_both_themes() {
    for tokens in [Tokens::dark(), Tokens::light()] {
        for class in MoveClass::ALL {
            let disc = class_color(&tokens, class);
            let ratio = contrast_ratio(contrast_on(disc), disc);
            assert!(ratio >= MIN_TEXT, "{class:?}: {ratio}");
        }
    }
}

// ---- the texts (D1448) ----

#[test]
fn every_new_key_exists_in_both_languages_and_differs_between_them() {
    let keys = [
        "report-configure",
        "report-continue",
        "report-continue-tip",
        "report-progress-eta",
        "report-user-side",
        "report-user-none",
        "report-user-ask",
        "report-side-you",
        "report-review-keys",
        "report-config-title",
        "report-config-limit-label",
        "report-config-limits",
        "report-config-limit",
        "report-config-last",
        "report-config-marks",
        "report-config-defaults",
        "report-config-note",
    ];
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    for key in keys {
        let (a, b) = (en.tr(key), pt.tr(key));
        assert_ne!(a, key, "{key} is missing in en-US");
        assert_ne!(b, key, "{key} is missing in pt-BR");
        assert_ne!(a, b, "{key} is not translated");
    }
}

#[test]
fn the_text_of_the_report_never_says_variacao() {
    let source = include_str!("../../../assets/i18n/pt-BR/graph.ftl");
    let lower = source.to_lowercase();
    assert!(!lower.contains("variação") && !lower.contains("variações"));
}
