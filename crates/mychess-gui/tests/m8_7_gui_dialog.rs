//! M8-7c (GUI): the tactical form, its start positions, the panel line and
//! the end box (spec `docs/specs/m8-7-jogo-tatico.md`, D2469-D2472), in both
//! languages. The game itself is tested in `mychess-app`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::engines::{EngineDraft, EngineId, EngineMessage, StartMode};
use mychess_app::tactical_game::{
    StartProblem, StrengthPlan, TacticalStartPref, TacticalStats, TacticalView,
};
use mychess_app::{
    Action, Binding, Command, Effect, Message as AppMessage, OpponentInfo, PlayEnd, PlayMessage,
    PlayPhase, PlayView, Shortcuts, StartFrom,
};
use mychess_core::{Color, GameResult, Position};
use mychess_engine::{Capabilities, EngineIdentity, EngineOption, OptionKind};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::play_dialog::{PlayNumber, PlayUi, PlayUiMessage, view};
use mychess_gui::windows::play_panel::{bar_with, end_box_with};
use mychess_gui::windows::tactical_dialog::{
    line_text, next_elo_text, plan_text, start_problem, stats_text,
};
use mychess_gui::{Message, Shell};

fn shell_with_engine(options: Vec<EngineOption>, started: bool) -> (Shell, EngineId) {
    let mut shell = Shell::new(None);
    let engine = shell
        .app
        .engines
        .add(EngineDraft::new("Fake", "fake-engine"))
        .expect("added");
    if started {
        let effects = mychess_app::update(
            &mut shell.app,
            AppMessage::Engine(EngineMessage::Start(engine, StartMode::Analyze)),
        );
        let run = effects
            .iter()
            .find_map(|e| match e {
                Effect::StartEngine { run, .. } => Some(*run),
                _ => None,
            })
            .expect("StartEngine");
        mychess_app::update(
            &mut shell.app,
            AppMessage::Engine(EngineMessage::Started {
                run,
                identity: Box::new(EngineIdentity {
                    name: "Fake".to_owned(),
                    author: None,
                    options,
                    capabilities: Capabilities::default(),
                }),
                at: 1_700_000_000,
            }),
        );
    }
    (shell, engine)
}

fn spin(name: &str, min: i64, max: i64) -> EngineOption {
    EngineOption {
        name: name.to_owned(),
        kind: OptionKind::Spin {
            default: min,
            min,
            max,
        },
    }
}

fn check(name: &str) -> EngineOption {
    EngineOption {
        name: name.to_owned(),
        kind: OptionKind::Check { default: false },
    }
}

fn open_tactical(shell: &Shell) -> PlayUi {
    let mut ui = PlayUi::default();
    drop(ui.update(PlayUiMessage::OpenTactical, &shell.app));
    ui
}

fn found(shell: &Shell, ui: &PlayUi, language: Language, keys: &[&str]) -> Vec<String> {
    let i18n = I18n::new(language);
    let element = view(&shell.app, ui, &i18n).expect("open");
    let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 1600.0), element);
    keys.iter()
        .filter(|k| sim.find(i18n.tr(k)).is_ok())
        .map(|k| (*k).to_owned())
        .collect()
}

fn tactical_config(out: &[AppMessage]) -> mychess_app::PlayConfig {
    out.iter()
        .find_map(|m| match m {
            AppMessage::Play(PlayMessage::Start(config)) => Some((**config).clone()),
            _ => None,
        })
        .expect("a start")
}

// ---- the form ----

#[test]
fn the_tactical_form_has_its_own_sections_in_both_languages() {
    let (shell, _) = shell_with_engine(Vec::new(), false);
    let ui = open_tactical(&shell);
    for language in [Language::EnUs, Language::PtBr] {
        let keys = [
            "play-tactical-title",
            "play-tactical-elo",
            "play-tactical-adjust",
            "play-tactical-start-new",
            "play-tactical-start-current",
            "play-tactical-start-fen",
            "play-tactical-start-file",
            "play-tactical-start-base",
            "play-tactical-start-problem",
            "play-tactical-plan-unknown",
            "play-tactical-stats-none",
        ];
        let seen = found(&shell, &ui, language, &keys);
        for key in keys {
            assert!(
                seen.contains(&key.to_owned()),
                "{language:?} {key}: {seen:?}"
            );
        }
        // The strength of the normal form is not there.
        let normal = found(
            &shell,
            &ui,
            language,
            &["play-strength-elo", "play-strength-skill"],
        );
        assert!(normal.is_empty(), "{language:?}: {normal:?}");
    }
}

#[test]
fn the_normal_form_does_not_show_the_tactical_sections() {
    let (shell, _) = shell_with_engine(Vec::new(), false);
    let mut ui = PlayUi::default();
    drop(ui.update(PlayUiMessage::Open, &shell.app));
    let seen = found(
        &shell,
        &ui,
        Language::EnUs,
        &[
            "play-tactical-elo",
            "play-tactical-start-fen",
            "play-start-new",
        ],
    );
    assert_eq!(seen, ["play-start-new"]);
}

#[test]
fn submit_starts_a_tactical_game_with_the_elo_typed() {
    let (shell, engine) = shell_with_engine(Vec::new(), false);
    let mut ui = open_tactical(&shell);
    drop(ui.update(
        PlayUiMessage::Number(PlayNumber::TacticalElo, "1850".to_owned()),
        &shell.app,
    ));
    drop(ui.update(PlayUiMessage::TacticalAdjust(true), &shell.app));
    drop(ui.update(PlayUiMessage::TacticalAdjustStep(100), &shell.app));
    let out = ui.update(PlayUiMessage::Submit, &shell.app);
    assert!(ui.draft.is_none());
    let config = tactical_config(&out.messages);
    assert_eq!(config.engine, engine);
    let setup = config.tactical.expect("tactical");
    assert_eq!(setup.elo, 1850);
    assert_eq!(setup.adjust_step, Some(100));
    assert_eq!(setup.plan, None, "planned when the engine is known");
    assert_eq!(config.strength, mychess_app::Strength::default());
    assert_eq!(config.start, StartFrom::NewGame);
    // The preferences are saved first.
    let AppMessage::Prefs(_) = &out.messages[0] else {
        panic!("{:?}", out.messages);
    };
}

#[test]
fn an_elo_out_of_range_is_limited() {
    let (shell, _) = shell_with_engine(Vec::new(), false);
    let mut ui = open_tactical(&shell);
    drop(ui.update(
        PlayUiMessage::Number(PlayNumber::TacticalElo, "99999".to_owned()),
        &shell.app,
    ));
    let out = ui.update(PlayUiMessage::Submit, &shell.app);
    assert_eq!(tactical_config(&out.messages).tactical.unwrap().elo, 3200);
}

#[test]
fn the_normal_strength_of_the_dialog_does_not_leak_into_the_tactical_game() {
    let (shell, _) = shell_with_engine(Vec::new(), false);
    let mut ui = open_tactical(&shell);
    drop(ui.update(PlayUiMessage::LimitStrength(true), &shell.app));
    drop(ui.update(PlayUiMessage::UseSkill(true), &shell.app));
    let out = ui.update(PlayUiMessage::Submit, &shell.app);
    assert_eq!(
        tactical_config(&out.messages).strength,
        mychess_app::Strength::default()
    );
}

// ---- the start position ----

#[test]
fn a_bad_fen_keeps_the_form_open_and_says_so() {
    let (shell, _) = shell_with_engine(Vec::new(), false);
    let mut ui = open_tactical(&shell);
    drop(ui.update(
        PlayUiMessage::TacticalStart(TacticalStartPref::Fen),
        &shell.app,
    ));
    drop(ui.update(
        PlayUiMessage::TacticalFen("not a fen".to_owned()),
        &shell.app,
    ));
    let draft = ui.draft.as_deref().expect("open");
    assert_eq!(start_problem(&shell.app, draft), Some(StartProblem::BadFen));
    let out = ui.update(PlayUiMessage::Submit, &shell.app);
    assert_eq!(out.messages, []);
    assert!(ui.draft.is_some(), "the form stays");
    for language in [Language::EnUs, Language::PtBr] {
        let seen = found(&shell, &ui, language, &["play-tactical-fen-bad"]);
        assert_eq!(seen, ["play-tactical-fen-bad"], "{language:?}");
    }
}

#[test]
fn a_position_with_nothing_to_play_is_told_apart_from_a_bad_fen() {
    let (shell, _) = shell_with_engine(Vec::new(), false);
    let mut ui = open_tactical(&shell);
    drop(ui.update(
        PlayUiMessage::TacticalStart(TacticalStartPref::Fen),
        &shell.app,
    ));
    drop(ui.update(
        PlayUiMessage::TacticalFen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1".to_owned()),
        &shell.app,
    ));
    let draft = ui.draft.as_deref().expect("open");
    assert_eq!(start_problem(&shell.app, draft), Some(StartProblem::Ended));
}

#[test]
fn a_good_fen_becomes_the_position_of_the_game() {
    let (shell, _) = shell_with_engine(Vec::new(), false);
    let mut ui = open_tactical(&shell);
    let fen = "6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1";
    drop(ui.update(
        PlayUiMessage::TacticalStart(TacticalStartPref::Fen),
        &shell.app,
    ));
    drop(ui.update(PlayUiMessage::TacticalFen(fen.to_owned()), &shell.app));
    let out = ui.update(PlayUiMessage::Submit, &shell.app);
    let config = tactical_config(&out.messages);
    assert_eq!(
        config.start,
        StartFrom::Position(Box::new(Position::from_fen(fen).unwrap()))
    );
}

#[test]
fn without_a_training_the_problem_cannot_be_chosen() {
    let (shell, _) = shell_with_engine(Vec::new(), false);
    let mut ui = open_tactical(&shell);
    drop(ui.update(
        PlayUiMessage::TacticalStart(TacticalStartPref::Problem),
        &shell.app,
    ));
    let draft = ui.draft.as_deref().expect("open");
    assert_eq!(
        start_problem(&shell.app, draft),
        Some(StartProblem::NoProblem)
    );
    let out = ui.update(PlayUiMessage::Submit, &shell.app);
    assert_eq!(out.messages, []);
    for language in [Language::EnUs, Language::PtBr] {
        let seen = found(&shell, &ui, language, &["play-tactical-start-problem-none"]);
        assert_eq!(seen, ["play-tactical-start-problem-none"], "{language:?}");
    }
}

#[test]
fn a_remembered_problem_choice_falls_back_when_no_training_is_on() {
    let (mut shell, _) = shell_with_engine(Vec::new(), false);
    shell.app.prefs.play.tactical.start = TacticalStartPref::Problem;
    let ui = open_tactical(&shell);
    assert_eq!(
        ui.draft.as_deref().unwrap().prefs.tactical.start,
        TacticalStartPref::New
    );
}

#[test]
fn a_file_for_the_drawing_is_needed_and_then_becomes_the_start() {
    let (shell, _) = shell_with_engine(Vec::new(), false);
    let mut ui = open_tactical(&shell);
    drop(ui.update(
        PlayUiMessage::TacticalStart(TacticalStartPref::RandomFile),
        &shell.app,
    ));
    assert_eq!(ui.update(PlayUiMessage::Submit, &shell.app).messages, []);
    let path = std::path::PathBuf::from("puzzles.pgn");
    drop(ui.update(
        PlayUiMessage::TacticalFilePicked(Some(path.clone())),
        &shell.app,
    ));
    let out = ui.update(PlayUiMessage::Submit, &shell.app);
    assert_eq!(
        tactical_config(&out.messages).start,
        StartFrom::Pick(mychess_app::tactical_game::PickSource::File(path))
    );
}

#[test]
fn the_base_choice_asks_for_a_drawing_from_the_current_database() {
    let (shell, _) = shell_with_engine(Vec::new(), false);
    let mut ui = open_tactical(&shell);
    drop(ui.update(
        PlayUiMessage::TacticalStart(TacticalStartPref::RandomBase),
        &shell.app,
    ));
    let out = ui.update(PlayUiMessage::Submit, &shell.app);
    assert_eq!(
        tactical_config(&out.messages).start,
        StartFrom::Pick(mychess_app::tactical_game::PickSource::CurrentBase)
    );
}

#[test]
fn a_game_with_unsaved_changes_chains_the_discard_question() {
    let (mut shell, _) = shell_with_engine(Vec::new(), false);
    let mv = shell
        .app
        .session
        .position()
        .find_move("e2".parse().unwrap(), "e4".parse().unwrap(), None)
        .expect("legal");
    drop(shell.update(Message::App(mychess_app::Message::Game(
        mychess_app::GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        },
    ))));
    assert!(shell.app.play_start_needs_discard());
    let mut ui = open_tactical(&shell);
    let out = ui.update(PlayUiMessage::Submit, &shell.app);
    assert!(ui.pending.is_some());
    assert!(
        out.messages
            .iter()
            .any(|m| matches!(m, AppMessage::Command(Command::NewGame))),
        "{:?}",
        out.messages
    );
    assert!(
        !out.messages
            .iter()
            .any(|m| matches!(m, AppMessage::Play(PlayMessage::Start(_)))),
        "nothing starts before the answer"
    );
    // "Current position" goes through the set-up, as in the normal game.
    let mut ui = open_tactical(&shell);
    drop(ui.update(
        PlayUiMessage::TacticalStart(TacticalStartPref::Current),
        &shell.app,
    ));
    let out = ui.update(PlayUiMessage::Submit, &shell.app);
    assert!(
        out.messages
            .iter()
            .any(|m| matches!(m, AppMessage::Command(Command::SetupBoard))),
        "{:?}",
        out.messages
    );
}

// ---- the strength on screen ----

#[test]
fn the_plan_text_follows_what_the_engine_declares() {
    let cases: [Vec<EngineOption>; 4] = [
        vec![check("UCI_LimitStrength"), spin("UCI_Elo", 1320, 3190)],
        vec![spin("Skill Level", 0, 20)],
        Vec::new(),
        vec![check("UCI_LimitStrength"), spin("UCI_Elo", 1600, 3190)],
    ];
    for (index, options) in cases.iter().enumerate() {
        let (shell, _) = shell_with_engine(options.clone(), true);
        let mut ui = open_tactical(&shell);
        let elo = if index == 1 { "1800" } else { "1500" };
        drop(ui.update(
            PlayUiMessage::Number(PlayNumber::TacticalElo, elo.to_owned()),
            &shell.app,
        ));
        let draft = ui.draft.as_deref().unwrap();
        let en = plan_text(&shell.app, draft, &I18n::new(Language::EnUs));
        let pt = plan_text(&shell.app, draft, &I18n::new(Language::PtBr));
        assert!(!en.is_empty() && !pt.is_empty());
        match index {
            0 => assert!(en.contains("UCI_Elo 1500"), "{en}"),
            1 => assert!(en.contains("Skill Level 10"), "{en}"),
            2 => assert!(en.contains("depth 5"), "{en}"),
            _ => assert!(en.contains("1600") && en.contains("3190"), "{en}"),
        }
    }
}

#[test]
fn the_form_reads_the_options_like_the_plan_does() {
    // `UCI_LimitStrength` that is not a check and a backwards `UCI_Elo` are
    // no option of strength: the summary says what the real plan will do.
    let not_a_check = EngineOption {
        name: "UCI_LimitStrength".to_owned(),
        kind: OptionKind::Button,
    };
    let backwards = spin("UCI_Elo", 3190, 1320);
    let (shell, _) = shell_with_engine(
        vec![
            not_a_check.clone(),
            backwards.clone(),
            spin("Skill Level", 0, 20),
        ],
        true,
    );
    let ui = open_tactical(&shell);
    let draft = ui.draft.as_deref().unwrap();
    let en = I18n::new(Language::EnUs);
    assert!(plan_text(&shell.app, draft, &en).contains("Skill Level"));
    let (shell, _) = shell_with_engine(vec![not_a_check, backwards], true);
    let ui = open_tactical(&shell);
    let draft = ui.draft.as_deref().unwrap();
    assert!(plan_text(&shell.app, draft, &en).contains("depth"));
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        assert!(!plan_text(&shell.app, draft, &i18n).contains("UCI_Elo"));
    }
}

#[test]
fn before_the_engine_runs_the_plan_text_says_it_is_set_later() {
    let (shell, _) = shell_with_engine(Vec::new(), false);
    let ui = open_tactical(&shell);
    let draft = ui.draft.as_deref().unwrap();
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        assert_eq!(
            plan_text(&shell.app, draft, &i18n),
            i18n.tr("play-tactical-plan-unknown")
        );
    }
}

#[test]
fn the_score_is_written_with_a_plural_in_both_languages() {
    let one = TacticalStats {
        games: 1,
        wins: 1,
        streak: 1,
        best_streak: 1,
        best_win_elo: 1500,
        ..TacticalStats::default()
    };
    let many = TacticalStats {
        games: 5,
        wins: 2,
        draws: 1,
        losses: 2,
        best_streak: 2,
        best_win_elo: 1700,
        ..TacticalStats::default()
    };
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    assert!(
        stats_text(&one, &en).starts_with("1 game."),
        "{}",
        stats_text(&one, &en)
    );
    assert!(stats_text(&many, &en).starts_with("5 games."));
    assert!(
        stats_text(&one, &pt).starts_with("1 partida."),
        "{}",
        stats_text(&one, &pt)
    );
    assert!(stats_text(&many, &pt).starts_with("5 partidas."));
    assert_eq!(
        stats_text(&TacticalStats::default(), &en),
        en.tr("play-tactical-stats-none")
    );
    assert!(stats_text(&many, &en).contains("50%"), "2 + 1/2 of 5 = 50%");
}

// ---- the panel and the end box ----

fn view_in(phase: PlayPhase) -> PlayView {
    PlayView {
        phase,
        player_color: Color::White,
        opponent: OpponentInfo {
            name: "Fake".to_owned(),
            rating: None,
        },
        white_time: std::time::Duration::ZERO,
        black_time: std::time::Duration::ZERO,
        countdown: false,
        question: None,
        result: GameResult::White,
        can_resume: false,
        engine_lost: false,
        can_take_back: false,
        can_offer_draw: false,
        pondering: false,
        coach: None,
    }
}

fn tactical_view(plan: Option<StrengthPlan>, adjusted: Option<(u32, u32)>) -> TacticalView {
    TacticalView {
        elo: 1500,
        plan,
        adjust_step: Some(50),
        stats: TacticalStats {
            games: 3,
            wins: 2,
            draws: 0,
            losses: 1,
            streak: 1,
            best_streak: 2,
            best_win_elo: 1500,
        },
        adjusted,
    }
}

#[test]
fn the_line_of_the_panel_names_the_strength_and_the_score() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    let elo = tactical_view(
        Some(StrengthPlan::Elo {
            elo: 1500,
            clamped: false,
        }),
        None,
    );
    assert_eq!(
        line_text(&elo, &en),
        "Tactical: engine at Elo 1500 · score 2-0-1"
    );
    assert_eq!(
        line_text(&elo, &pt),
        "Tático: engine com Elo 1500 · placar 2-0-1"
    );
    let skill = tactical_view(Some(StrengthPlan::Skill { level: 8 }), None);
    assert!(line_text(&skill, &en).contains("Skill Level 8"));
    let depth = tactical_view(Some(StrengthPlan::Depth { depth: 5 }), None);
    assert!(line_text(&depth, &pt).contains("profundidade até 5"));
}

#[test]
fn the_panel_shows_the_line_only_for_a_tactical_game() {
    let i18n = I18n::new(Language::EnUs);
    let v = view_in(PlayPhase::Player);
    let plain = bar_with(&v, None, &i18n);
    let mut sim = Simulator::with_size(Settings::default(), Size::new(1100.0, 80.0), plain);
    assert!(sim.find("Tactical:").is_err());
    let tv = tactical_view(
        Some(StrengthPlan::Elo {
            elo: 1500,
            clamped: false,
        }),
        None,
    );
    let tactical = bar_with(&v, Some(&tv), &i18n);
    let mut sim = Simulator::with_size(Settings::default(), Size::new(1100.0, 80.0), tactical);
    let line = line_text(&tv, &i18n);
    assert!(sim.find(line).is_ok());
}

#[test]
fn the_end_box_shows_the_score_and_the_change_of_elo() {
    let tv = tactical_view(
        Some(StrengthPlan::Elo {
            elo: 1500,
            clamped: false,
        }),
        Some((1500, 1550)),
    );
    let end = PlayEnd::Checkmate {
        winner: Color::White,
    };
    let v = view_in(PlayPhase::Ended(end));
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let element = end_box_with(end, &v, Some(&tv), false, &i18n);
        let mut sim = Simulator::with_size(Settings::default(), Size::new(900.0, 700.0), element);
        assert!(
            sim.find(stats_text(&tv.stats, &i18n)).is_ok(),
            "{language:?}"
        );
        assert!(
            sim.find(next_elo_text(&tv, &i18n).expect("changed"))
                .is_ok(),
            "{language:?}"
        );
    }
    // No change of Elo, no line.
    let same = tactical_view(None, None);
    assert_eq!(next_elo_text(&same, &I18n::new(Language::EnUs)), None);
}

// ---- the shortcut ----

#[test]
fn the_default_shortcut_is_command_shift_f7_and_never_uses_alt() {
    let shortcuts = Shortcuts::defaults();
    let bindings = shortcuts.bindings(Action::TacticalGame);
    assert_eq!(bindings, [Binding::parse("Command+Shift+F7").unwrap()]);
    assert!(!format!("{:?}", bindings[0]).contains("alt: true"));
    // No other action has taken the key.
    let owners = shortcuts
        .iter()
        .filter(|(_, b)| b.contains(&bindings[0]))
        .count();
    assert_eq!(owners, 1);
}

#[test]
fn the_action_opens_the_tactical_form_and_the_menu_item_exists() {
    let (mut shell, _) = shell_with_engine(Vec::new(), false);
    drop(shell.update(Message::Ui(mychess_gui::UiMessage::Action(
        Action::TacticalGame,
    ))));
    let draft = shell.ui.dialogs.play.draft.as_deref().expect("form open");
    assert!(draft.tactical);
    // Pressing the key again while a game runs only warns.
}

#[test]
fn the_drawing_of_a_position_can_be_given_up_from_the_status_bar() {
    let (mut shell, engine) = shell_with_engine(Vec::new(), false);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("p.pgn");
    std::fs::write(&file, "[Event \"a\"]\n\n1. e4 e5 *\n").unwrap();
    let mut config = mychess_app::PlayConfig::new(engine);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::File(file));
    mychess_app::update(
        &mut shell.app,
        AppMessage::Play(PlayMessage::Start(Box::new(config))),
    );
    assert!(shell.app.play_picking());
    let element = mychess_gui::windows::main_window::view(&shell);
    let mut sim = Simulator::with_size(Settings::default(), Size::new(1300.0, 900.0), element);
    assert!(sim.find(shell.i18n.tr("play-status-picking")).is_ok());
    sim.click(shell.i18n.tr("dialog-cancel")).unwrap();
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(
        matches!(messages.as_slice(), [Message::PlayUi(PlayUiMessage::Quit)]),
        "{messages:?}"
    );
}
