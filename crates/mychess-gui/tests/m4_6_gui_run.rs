//! A run of the annotation seen from the window (spec M4-6, "Comportamento
//! observavel" 3-5, D885, D895, D897, D911, D912; Scid `autoplay`/
//! `cancelAutoplay`): the progress in the status bar, the Stop button, `Esc`,
//! the final remark, the blocked edits and the timers. Headless; the engine
//! answers with synthetic events.
#![allow(clippy::float_cmp)]

mod m4_68_gui_common;

use m4_68_gui_common::*;
use mychess_app::engines::EngineMessage;
use mychess_gui::host::{HostWork, classify};

/// A run started from the defaults, plus the effects of the start.
fn started() -> (Rig, RunId, Vec<Effect>) {
    let (mut rig, run) = rig_with_engine(&SCHOLAR);
    let effects = ann(&mut rig, AnnotateMessage::Start);
    assert!(rig.shell.app.annotation_running(), "the run started");
    (rig, run, effects)
}

/// Answers searches with `cp`/no line until the run ends (at most 60).
fn finish(rig: &mut Rig, run: RunId, mut effects: Vec<Effect>) {
    let mut searches = Searches::new();
    for _ in 0..60 {
        if !rig.shell.app.annotation_running() {
            return;
        }
        match searches.answer(rig, run, &effects, 0, &[]) {
            Some(next) => effects = next,
            None => break,
        }
    }
    assert!(!rig.shell.app.annotation_running(), "the run did not end");
}

// ---- progress ----

#[test]
fn the_status_bar_shows_the_move_being_annotated_and_the_stop_button() {
    let (rig, _run, _fx) = started();
    assert!(window_has(&rig, "Annotating... move 1 of 7"));
    assert!(window_has(&rig, "Stop"));
}

#[test]
fn without_a_run_there_is_no_progress_and_no_stop_button() {
    let (rig, _run) = rig_with_engine(&SCHOLAR);
    assert!(!window_has(&rig, "Stop"));
    assert!(!window_has(&rig, "Annotating... move 1 of 7"));
}

#[test]
fn the_progress_text_follows_the_application_as_searches_end() {
    let (mut rig, run, mut effects) = started();
    let mut searches = Searches::new();
    let mut last_done = 0;
    for _ in 0..6 {
        effects = searches
            .answer(&mut rig, run, &effects, 0, &[])
            .expect("a search");
        let Some(p) = rig.shell.app.annotation_progress() else {
            break;
        };
        let shown = (p.done_plies + 1).min(p.total_plies);
        let text = format!("Annotating... move {shown} of {}", p.total_plies);
        assert!(window_has(&rig, &text), "{text}");
        assert!(p.done_plies >= last_done, "progress never goes back");
        last_done = p.done_plies;
    }
    assert!(last_done >= 1, "some move was annotated");
}

#[test]
fn the_progress_never_shows_a_move_past_the_total() {
    let (mut rig, run, effects) = started();
    let mut searches = Searches::new();
    let mut fx = effects;
    for _ in 0..60 {
        if let Some(p) = rig.shell.app.annotation_progress() {
            assert!(p.done_plies <= p.total_plies, "{p:?}");
        }
        match searches.answer(&mut rig, run, &fx, 0, &[]) {
            Some(next) if rig.shell.app.annotation_running() => fx = next,
            _ => break,
        }
    }
}

#[test]
fn while_the_engine_starts_the_progress_says_so() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&SCHOLAR));
    let id = add_engine(&mut rig, "Slow", Protocol::Uci);
    up(
        &mut rig,
        AppMessage::Engine(EngineMessage::SetHotkey(
            id,
            Some(mychess_app::engines::Hotkey::F2),
        )),
    );
    ann(&mut rig, AnnotateMessage::Start);
    assert!(rig.shell.app.annotation_running(), "waiting for the engine");
    assert!(window_has(&rig, "Annotating... starting the engine"));
    // Esc ends it at once, nothing was annotated.
    esc(&mut rig);
    assert!(!rig.shell.app.annotation_running());
    assert_eq!(
        annotate_notice(&rig),
        Some(AnnotateNotice::Cancelled { plies: 0 })
    );
}

#[test]
fn the_stop_button_sends_cancel_and_the_run_ends_with_a_remark() {
    let (mut rig, _run, _fx) = started();
    let sent = click_msgs(&rig, "Stop");
    assert!(
        annotates(&sent).contains(&AnnotateMessage::Cancel),
        "{sent:?}"
    );
    apply(&mut rig, sent);
    assert!(!rig.shell.app.annotation_running());
    assert_eq!(
        annotate_notice(&rig),
        Some(AnnotateNotice::Cancelled { plies: 0 })
    );
    assert!(window_has(
        &rig,
        "Annotation stopped after 0 moves; the edits were kept."
    ));
    assert!(!window_has(&rig, "Stop"), "the bar is gone");
}

#[test]
fn the_progress_is_in_portuguese_with_the_language_of_the_rig() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    rig.set_game(game_of(&SCHOLAR));
    uci_run(&mut rig);
    ann(&mut rig, AnnotateMessage::Start);
    assert!(window_has(&rig, "Anotando... lance 1 de 7"));
    assert!(window_has(&rig, "Parar"));
}

// ---- Esc ----

#[test]
fn escape_during_a_run_cancels_it_and_keeps_the_game() {
    let (mut rig, _run, _fx) = started();
    let before = rig.game().clone();
    esc(&mut rig);
    assert!(!rig.shell.app.annotation_running());
    assert_eq!(
        annotate_notice(&rig),
        Some(AnnotateNotice::Cancelled { plies: 0 })
    );
    assert_eq!(rig.game().mainline().count(), before.mainline().count());
}

#[test]
fn escape_with_a_modifier_does_not_cancel() {
    let (mut rig, _run, _fx) = started();
    rig.press(KeyInput {
        key: Key::Named(NamedKey::Escape),
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
    });
    assert!(rig.shell.app.annotation_running());
}

#[test]
fn escape_without_a_run_sends_no_annotate_message() {
    let (mut rig, _run) = rig_with_engine(&SCHOLAR);
    esc(&mut rig);
    assert!(annotate_notice(&rig).is_none());
    assert!(!rig.shell.app.annotation_running());
}

// D920 supersedes the order of D911: an Esc that has a layer to close
// (About, popups, search field, drag) closes it and does not stop the run.
#[test]
fn escape_closes_the_about_box_first_and_the_next_one_stops_the_run() {
    let (mut rig, _run, _fx) = started();
    rig.shell.ui.show_about = true;
    esc(&mut rig);
    assert!(!rig.shell.ui.show_about, "the box closed");
    assert!(rig.shell.app.annotation_running(), "the run goes on");
    esc(&mut rig);
    assert!(!rig.shell.app.annotation_running(), "the run was stopped");
}

#[test]
fn with_a_dialog_open_escape_goes_to_the_dialog_first() {
    let (mut rig, _run, _fx) = started();
    up(&mut rig, AppMessage::Engine(EngineMessage::OpenDialog));
    assert!(rig.shell.app.engine_dialog.is_some());
    esc(&mut rig);
    assert!(
        rig.shell.app.engine_dialog.is_none(),
        "the dialog took the key"
    );
    assert!(rig.shell.app.annotation_running(), "the run goes on");
    esc(&mut rig);
    assert!(
        !rig.shell.app.annotation_running(),
        "the second Esc stops it"
    );
}

#[test]
fn a_run_and_the_engines_dialog_can_both_be_on_screen() {
    let (mut rig, _run, _fx) = started();
    up(&mut rig, AppMessage::Engine(EngineMessage::OpenDialog));
    assert!(window_has(&rig, "Annotating... move 1 of 7"));
    assert!(window_has(&rig, "Stop"));
}

// ---- the end ----

#[test]
fn a_finished_run_leaves_the_remark_with_the_counts() {
    let (mut rig, run, fx) = started();
    finish(&mut rig, run, fx);
    match annotate_notice(&rig) {
        Some(AnnotateNotice::Finished { plies, variations }) => {
            assert_eq!(plies, 7);
            assert_eq!(variations, 0);
        }
        other => panic!("expected Finished, got {other:?}"),
    }
    tick_status(&mut rig);
    assert!(window_has(
        &rig,
        "Annotation finished: 7 moves, 0 variations."
    ));
    assert!(!window_has(&rig, "Stop"));
}

#[test]
fn the_final_remark_is_singular_for_one_move() {
    let (mut rig, run) = rig_with_engine(&["e2e4"]);
    let fx = ann(&mut rig, AnnotateMessage::Start);
    finish(&mut rig, run, fx);
    tick_status(&mut rig);
    assert!(window_has(
        &rig,
        "Annotation finished: 1 move, 0 variations."
    ));
}

#[test]
fn the_final_remark_is_in_portuguese_with_variante() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    rig.set_game(game_of(&["e2e4"]));
    let run = uci_run(&mut rig);
    let fx = ann(&mut rig, AnnotateMessage::Start);
    finish(&mut rig, run, fx);
    let text = annotate_dialog::notice_text(
        &pt(),
        &AnnotateNotice::Finished {
            plies: 1,
            variations: 1,
        },
    );
    assert!(text.contains("variante"), "{text}");
    assert!(!text.contains("variaç"), "{text}");
}

#[test]
fn an_engine_that_dies_ends_the_run_with_the_failure_remark() {
    let (mut rig, run, _fx) = started();
    exit_engine(&mut rig, run);
    assert!(!rig.shell.app.annotation_running());
    assert_eq!(
        annotate_notice(&rig),
        Some(AnnotateNotice::EngineFailed { plies: 0 })
    );
    tick_status(&mut rig);
    assert!(window_has(
        &rig,
        "The engine failed; the annotation stopped after 0 moves."
    ));
}

#[test]
fn a_new_run_can_start_after_the_end() {
    let (mut rig, run, fx) = started();
    finish(&mut rig, run, fx);
    ann(&mut rig, AnnotateMessage::Start);
    assert!(rig.shell.app.annotation_running());
}

// ---- the game cannot be edited meanwhile (D885, D897) ----

#[test]
fn navigation_during_a_run_is_refused_with_the_busy_remark() {
    let (mut rig, _run, _fx) = started();
    let cursor = rig.cursor();
    rig.press(named(NamedKey::Left));
    assert!(rig.shell.app.annotation_running(), "the run goes on");
    assert_eq!(rig.cursor(), cursor, "the cursor did not move");
    assert_eq!(annotate_notice(&rig), Some(AnnotateNotice::Busy));
}

#[test]
fn the_busy_remark_is_visible_in_the_status_bar_during_a_run() {
    let (mut rig, _run, _fx) = started();
    rig.press(named(NamedKey::Left));
    tick_status(&mut rig);
    assert!(window_has(
        &rig,
        "The game cannot be edited while it is being annotated. Press Esc to stop."
    ));
}

#[test]
fn a_move_played_during_a_run_is_refused() {
    let (mut rig, _run, _fx) = started();
    let before = rig.game().mainline().count();
    let mv = rig.shell.app.session.position().legal_moves()[0];
    up(
        &mut rig,
        AppMessage::Game(GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        }),
    );
    assert_eq!(rig.game().mainline().count(), before);
    assert_eq!(annotate_notice(&rig), Some(AnnotateNotice::Busy));
}

#[test]
fn opening_the_dialog_again_during_a_run_stops_it() {
    let (mut rig, _run, _fx) = started();
    ann(&mut rig, AnnotateMessage::Open);
    assert!(!rig.shell.app.annotation_running());
    assert!(rig.shell.app.annotate_dialog.is_none(), "no dialog opens");
    assert_eq!(
        annotate_notice(&rig),
        Some(AnnotateNotice::Cancelled { plies: 0 })
    );
}

#[test]
fn the_annotate_button_of_the_panel_stays_usable_during_a_run_to_stop_it() {
    let (rig, _run, _fx) = started();
    let sent = click_msgs(&rig, "Annotate");
    assert!(
        annotates(&sent).contains(&AnnotateMessage::Open),
        "{sent:?}"
    );
}

#[test]
fn a_second_start_during_a_run_is_refused() {
    let (mut rig, _run, _fx) = started();
    ann(&mut rig, AnnotateMessage::Start);
    assert!(rig.shell.app.annotation_running());
    assert_eq!(annotate_notice(&rig), Some(AnnotateNotice::AlreadyRunning));
}

#[test]
fn starting_in_trial_mode_is_refused_with_a_remark() {
    let (mut rig, _run) = rig_with_engine(&SCHOLAR);
    up(
        &mut rig,
        AppMessage::Game(GameMessage::EnterTrial { null_move: false }),
    );
    ann(&mut rig, AnnotateMessage::Start);
    assert!(!rig.shell.app.annotation_running());
    assert_eq!(annotate_notice(&rig), Some(AnnotateNotice::TrialMode));
    tick_status(&mut rig);
    assert!(window_has(&rig, "Leave trial mode before annotating."));
}

// ---- the timers (D895, D918): no timer of the GUI ----

#[test]
fn the_watchdog_of_the_run_is_an_effect_that_goes_to_perform() {
    let (_rig, _run, effects) = started();
    let ticks: Vec<&Effect> = effects
        .iter()
        .filter(|e| matches!(e, Effect::ScheduleAnnotateTick { .. }))
        .collect();
    assert!(!ticks.is_empty(), "the run arms its watchdog");
    for effect in ticks {
        assert!(!effect.runs_on_host());
        assert_eq!(
            classify(effect.clone(), &en()),
            HostWork::Perform(effect.clone())
        );
    }
}

#[test]
fn a_tick_of_an_old_generation_is_harmless() {
    let (mut rig, run, _fx) = started();
    ann(&mut rig, AnnotateMessage::Tick { run, generation: 0 });
    assert!(rig.shell.app.annotation_running());
}

// ---- hostile geometry ----

#[test]
fn a_tiny_window_draws_the_progress_without_panicking() {
    let (rig, _run, _fx) = started();
    for size in [
        iced::Size::new(1.0, 1.0),
        iced::Size::new(150.0, 100.0),
        iced::Size::new(320.0, 240.0),
    ] {
        drop(rig.sim(size));
    }
}
