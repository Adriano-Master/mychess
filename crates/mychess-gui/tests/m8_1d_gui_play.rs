//! M8-1d (GUI): the ponder and coach boxes of the configuration dialog,
//! the line of the coach and the ponder mark in the panel (spec D2411,
//! D2425, D2429), in both languages.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::time::Duration;

use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::engines::{EngineDraft, EngineId};
use mychess_app::{
    COACH_DEPTH_RANGE, CoachGrade, CoachVerdict, CoachView, OpponentInfo, PlayPhase, PlayView,
};
use mychess_core::{Color, GameResult};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::play_dialog::{
    PlayDraft, PlayNumber, PlayUi, PlayUiMessage, coach_candidates, view,
};
use mychess_gui::windows::play_panel::{bar, coach_line};
use mychess_gui::{Message, Shell};

fn two_engines() -> (Shell, EngineId, EngineId) {
    let mut shell = Shell::new(None);
    let a = shell
        .app
        .engines
        .add(EngineDraft::new("Alpha", "alpha"))
        .expect("added");
    let b = shell
        .app
        .engines
        .add(EngineDraft::new("Bravo", "bravo"))
        .expect("added");
    (shell, a, b)
}

fn form(shell: &Shell) -> PlayUi {
    PlayUi {
        draft: Some(Box::new(PlayDraft::new(&shell.app))),
        ..PlayUi::default()
    }
}

fn sim_of(shell: &Shell, ui: &PlayUi, language: Language) -> Vec<String> {
    let i18n = I18n::new(language);
    let element = view(&shell.app, ui, &i18n).expect("open");
    let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 1400.0), element);
    // The labels the test looks for; found or not.
    let keys = [
        "play-ponder",
        "play-coach",
        "play-coach-engine",
        "play-coach-depth",
        "play-coach-seconds",
        "play-coach-hint",
        "play-coach-numbers",
        "play-coach-no-engine",
    ];
    keys.iter()
        .filter(|k| sim.find(i18n.tr(k)).is_ok())
        .map(|k| (*k).to_owned())
        .collect()
}

#[test]
fn the_form_offers_ponder_and_the_coach_in_both_languages() {
    let (shell, ..) = two_engines();
    for language in [Language::EnUs, Language::PtBr] {
        let found = sim_of(&shell, &form(&shell), language);
        assert!(
            found.contains(&"play-ponder".to_owned()),
            "{language:?}: {found:?}"
        );
        assert!(
            found.contains(&"play-coach".to_owned()),
            "{language:?}: {found:?}"
        );
        // The coach options appear only with the coach on.
        assert!(!found.contains(&"play-coach-depth".to_owned()), "{found:?}");
    }
}

#[test]
fn turning_the_coach_on_shows_its_options() {
    let (shell, ..) = two_engines();
    let mut ui = form(&shell);
    drop(ui.update(PlayUiMessage::Coach(true), &shell.app));
    for language in [Language::EnUs, Language::PtBr] {
        let found = sim_of(&shell, &ui, language);
        for key in [
            "play-coach-engine",
            "play-coach-depth",
            "play-coach-seconds",
            "play-coach-hint",
            "play-coach-numbers",
        ] {
            assert!(
                found.contains(&key.to_owned()),
                "{language:?} {key}: {found:?}"
            );
        }
    }
}

#[test]
fn with_a_single_engine_the_coach_cannot_be_chosen_and_the_form_says_why() {
    let mut shell = Shell::new(None);
    shell
        .app
        .engines
        .add(EngineDraft::new("Only", "only"))
        .expect("added");
    let found = sim_of(&shell, &form(&shell), Language::EnUs);
    assert!(
        found.contains(&"play-coach-no-engine".to_owned()),
        "{found:?}"
    );
    // Submitting leaves the coach off even if the preference said on.
    let mut ui = form(&shell);
    drop(ui.update(PlayUiMessage::Coach(true), &shell.app));
    drop(ui.update(PlayUiMessage::Submit, &shell.app));
    let prefs = form_prefs_after_submit(&mut shell, true);
    assert!(!prefs.coach, "no candidate: no coach");
}

/// Sends the dialog's messages to the shell and returns the saved prefs.
fn form_prefs_after_submit(shell: &mut Shell, coach: bool) -> mychess_app::PlayPrefs {
    drop(shell.update(Message::PlayUi(PlayUiMessage::Open)));
    drop(shell.update(Message::PlayUi(PlayUiMessage::Coach(coach))));
    drop(shell.update(Message::PlayUi(PlayUiMessage::Submit)));
    (*shell.app.prefs.play).clone()
}

#[test]
fn the_coach_engine_is_never_the_opponents_and_is_saved_on_submit() {
    let (mut shell, a, b) = two_engines();
    assert_eq!(
        coach_candidates(&shell.app, Some(a))
            .iter()
            .map(|e| e.id)
            .collect::<Vec<_>>(),
        [b]
    );
    assert_eq!(coach_candidates(&shell.app, None).len(), 2);
    drop(shell.update(Message::PlayUi(PlayUiMessage::Open)));
    drop(shell.update(Message::PlayUi(PlayUiMessage::Engine(a))));
    drop(shell.update(Message::PlayUi(PlayUiMessage::Coach(true))));
    // The coach engine of the form was not chosen yet, or is the opponent:
    // it is fixed on submit.
    drop(shell.update(Message::PlayUi(PlayUiMessage::CoachEngine(a))));
    drop(shell.update(Message::PlayUi(PlayUiMessage::CoachHint(true))));
    drop(shell.update(Message::PlayUi(PlayUiMessage::CoachNumbers(false))));
    drop(shell.update(Message::PlayUi(PlayUiMessage::Ponder(true))));
    drop(shell.update(Message::PlayUi(PlayUiMessage::Number(
        PlayNumber::CoachDepth,
        "99".to_owned(),
    ))));
    drop(shell.update(Message::PlayUi(PlayUiMessage::Submit)));
    let p = &shell.app.prefs.play;
    assert!(p.coach && p.ponder && p.coach_hint && !p.coach_numbers);
    assert_eq!(p.coach_engine, Some(b.0), "the opponent cannot coach");
    assert_eq!(p.coach_depth, COACH_DEPTH_RANGE.1, "limited to the range");
    // The game began; once the opponent is up, the coach engine runs next
    // to it.
    assert!(shell.app.play_live());
    let run = shell.app.engine_runs.of_engine(a).expect("run").id;
    drop(shell.update(Message::App(mychess_app::Message::Engine(
        mychess_app::engines::EngineMessage::Started {
            run,
            identity: Box::new(mychess_engine::EngineIdentity {
                name: "Alpha".to_owned(),
                author: None,
                options: Vec::new(),
                capabilities: mychess_engine::Capabilities::default(),
            }),
            at: 1_700_000_000,
        },
    ))));
    assert!(shell.app.engine_runs.of_engine(b).is_some());
}

#[test]
fn the_remark_of_the_coach_reads_well_in_both_languages() {
    let v = |grade, loss, decisive, best: Option<&str>| CoachVerdict {
        ply: 5,
        played_san: "a3".to_owned(),
        best_san: best.map(str::to_owned),
        grade,
        loss_cp: loss,
        decisive,
    };
    let coach = |last: Option<CoachVerdict>, busy, lost, numbers| CoachView {
        engine: "Bravo".to_owned(),
        busy,
        lost,
        numbers,
        last,
    };
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    let cases = [
        (
            coach(None, false, false, true),
            "Coach: watching",
            "Coach: observando",
        ),
        (
            coach(None, true, false, true),
            "Coach: judging your move...",
            "Coach: avaliando o seu lance...",
        ),
        (
            coach(None, false, true, true),
            "Coach: unavailable (the engine stopped)",
            "Coach: indisponível (a engine parou)",
        ),
        (
            coach(
                Some(v(CoachGrade::Best, 0, false, None)),
                false,
                false,
                true,
            ),
            "Coach: best move",
            "Coach: melhor lance",
        ),
        (
            coach(
                Some(v(CoachGrade::Good, 30, false, None)),
                false,
                false,
                true,
            ),
            "Coach: good move",
            "Coach: boa jogada",
        ),
        (
            coach(
                Some(v(CoachGrade::Inaccuracy, 85, false, None)),
                false,
                false,
                true,
            ),
            "Coach: inaccuracy (-0.9)",
            "Coach: imprecisão (-0,9)",
        ),
        (
            coach(
                Some(v(CoachGrade::Mistake, 210, false, Some("Nf3"))),
                false,
                false,
                true,
            ),
            "Coach: mistake (-2.1), better was Nf3",
            "Coach: erro (-2,1), o melhor era Nf3",
        ),
        (
            coach(
                Some(v(CoachGrade::Blunder, 400, false, None)),
                false,
                false,
                false,
            ),
            "Coach: blunder",
            "Coach: erro grave",
        ),
        (
            coach(
                Some(v(CoachGrade::Blunder, 32_000, true, None)),
                false,
                false,
                true,
            ),
            "Coach: blunder",
            "Coach: erro grave",
        ),
    ];
    for (view, en_text, pt_text) in &cases {
        assert_eq!(&coach_line(view, &en), en_text);
        assert_eq!(&coach_line(view, &pt), pt_text);
    }
    // While judging the old remark is not shown.
    let busy = coach(Some(v(CoachGrade::Good, 0, false, None)), true, false, true);
    assert_eq!(coach_line(&busy, &en), "Coach: judging your move...");
}

fn play_view(pondering: bool, coach: Option<CoachView>) -> PlayView {
    PlayView {
        phase: PlayPhase::Player,
        player_color: Color::White,
        opponent: OpponentInfo {
            name: "Alpha".to_owned(),
            rating: None,
        },
        white_time: Duration::ZERO,
        black_time: Duration::ZERO,
        countdown: false,
        question: None,
        result: GameResult::Unknown,
        can_resume: false,
        engine_lost: false,
        can_take_back: coach.is_none(),
        can_offer_draw: true,
        pondering,
        coach,
    }
}

#[test]
fn the_panel_shows_the_ponder_mark_and_the_line_of_the_coach() {
    for (language, ponder, coach) in [
        (Language::EnUs, "Thinking on your time", "Coach: watching"),
        (Language::PtBr, "Pensando no seu tempo", "Coach: observando"),
    ] {
        let i18n = I18n::new(language);
        let view = play_view(
            true,
            Some(CoachView {
                engine: "Bravo".to_owned(),
                busy: false,
                lost: false,
                numbers: true,
                last: None,
            }),
        );
        let mut sim = Simulator::with_size(
            Settings::default(),
            Size::new(1000.0, 200.0),
            bar(&view, &i18n),
        );
        assert!(sim.find(ponder).is_ok(), "{language:?}: ponder mark");
        assert!(sim.find(coach).is_ok(), "{language:?}: coach line");
    }
    let i18n = I18n::new(Language::EnUs);
    let view = play_view(false, None);
    let mut sim = Simulator::with_size(
        Settings::default(),
        Size::new(1000.0, 200.0),
        bar(&view, &i18n),
    );
    assert!(sim.find("Thinking on your time").is_err());
    assert!(sim.find("Coach: watching").is_err());
}

#[test]
fn the_numeric_fields_of_the_coach_are_digits_limited_to_their_range() {
    let (shell, ..) = two_engines();
    let mut ui = form(&shell);
    drop(ui.update(
        PlayUiMessage::Number(PlayNumber::CoachSeconds, "9x9".to_owned()),
        &shell.app,
    ));
    let draft = ui.draft.as_ref().unwrap();
    assert_eq!(draft.text(PlayNumber::CoachSeconds), "99");
    assert_eq!(draft.to_prefs().coach_seconds, 30);
    assert_eq!(draft.text(PlayNumber::CoachDepth), "12");
}
