//! M8-6 fixes after the review (spec D2350-D2362): castling in the typed
//! lines, the crucial move against the scale of the first stage, the file
//! that is read before it is written, the initiators of the other modes,
//! unusable engine answers and bounds.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::large_futures,
    clippy::assert_is_empty,
    clippy::print_stderr
)]

mod m4_3_common;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mychess_app::calvar::{
    Assessment, CalvarConfig, CalvarEffect, CalvarEngine, CalvarFailure, CalvarMessage,
    CalvarNotice, CalvarPhase, EngineFailure, Eval, LineProbes, LinesJob, LoadedStore, Probe,
    RootJob, RootLine, UserLine, analyze_root, compare, read_moves, store_path,
};
use mychess_app::engines::EngineDraft;
use mychess_app::tactics::Cancel;
use mychess_app::tournament::{Participant, TimeControl, TournamentConfig, TournamentMessage};
use mychess_app::{
    AppState, DiscardChoice, Effect, GameMessage, Message, PlayOptions, RequestId, perform, update,
};
use mychess_core::Position;

// ---- (1) castling (D2350, D2351) ----

#[test]
fn castling_written_with_zeros_or_letters_is_read() {
    let p = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    for (text, plies) in [
        ("0-0", 1),
        ("0-0-0", 1),
        ("O-O", 1),
        ("O-O-O", 1),
        ("o-o", 1),
        ("12.0-0", 1),
        ("12...O-O", 1),
        ("12. 0-0-0", 1),
        ("O-O+", 1),
        ("0-0 0-0-0", 2),
    ] {
        let read = read_moves(&p, text);
        assert_eq!(read.rejected, None, "{text}");
        assert_eq!(read.moves.len(), plies, "{text}");
    }
}

#[test]
fn the_two_castlings_are_the_right_moves() {
    let p = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    let short = read_moves(&p, "0-0").moves[0];
    let long = read_moves(&p, "0-0-0").moves[0];
    assert_eq!(p.san(short).unwrap(), "O-O");
    assert_eq!(p.san(long).unwrap(), "O-O-O");
}

#[test]
fn a_number_is_digits_followed_by_a_dot_and_digits_alone_are_ignored() {
    let p = Position::new();
    assert_eq!(read_moves(&p, "12 e4").moves.len(), 1);
    assert_eq!(read_moves(&p, "1. e4 1... e5").moves.len(), 2);
    let read = read_moves(&p, "1-0");
    assert_eq!(
        read.rejected.as_deref(),
        Some("1-0"),
        "not a number, not a move"
    );
    assert!(
        read_moves(&p, "00").rejected.is_none(),
        "digits alone are ignored"
    );
}

#[test]
fn an_impossible_short_castling_is_refused_not_turned_into_the_long_one() {
    // The knight on g1 blocks the short castling; the long one is possible.
    let p = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K1NR w KQkq - 0 1").unwrap();
    let read = read_moves(&p, "OO");
    assert!(read.moves.is_empty());
    assert_eq!(read.rejected.as_deref(), Some("OO"));
    assert!(read_moves(&p, "O-O").moves.is_empty());
    assert_eq!(read_moves(&p, "O-O-O").moves.len(), 1);
}

#[test]
fn zeros_and_letters_that_are_no_castling_are_refused() {
    let p = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    for text in ["O", "O-O-O-O", "-", "o-", "0-0-0-0"] {
        let read = read_moves(&p, text);
        assert!(read.moves.is_empty(), "{text}");
    }
}

// ---- (3) the crucial move (D2352) ----

fn root_line(p: &Position, n: u8, uci: &str, cp: i32) -> RootLine {
    RootLine {
        multipv: n,
        depth: 20,
        eval: Eval::centipawns(cp),
        pv: vec![p.parse_uci(uci).unwrap()],
    }
}

fn probe(cp: i32) -> Probe {
    Probe {
        eval: Eval::centipawns(cp),
        depth: 10,
        pv: Vec::new(),
    }
}

#[test]
fn a_shallow_probe_below_the_first_stage_does_not_make_a_missed_move_crucial() {
    let p = Position::new();
    let root = [root_line(&p, 1, "e2e4", 44), root_line(&p, 2, "d2d4", 44)];
    let lines = [UserLine {
        moves: read_moves(&p, "d4").moves,
        assessment: Assessment::Equal,
    }];
    // The 10-second probe of d4 is far below what the first stage said.
    let probes = [LineProbes {
        first: Some(probe(10)),
        end: None,
    }];
    let report = compare(&p, &root, &lines, &probes);
    assert!(
        !report.has_crucial_miss(),
        "e4 is no better than d4 at depth 20"
    );
    assert!(report.parts.penalty.abs() < 1e-6);
}

#[test]
fn a_found_move_is_never_below_a_crucial_missed_one_in_the_first_stage() {
    let p = Position::new();
    let root = [
        root_line(&p, 1, "e2e4", 60),
        root_line(&p, 2, "d2d4", 40),
        root_line(&p, 3, "g1f3", 30),
    ];
    for probe_cp in [-500, 0, 40, 500] {
        let lines = [UserLine {
            moves: read_moves(&p, "d4").moves,
            assessment: Assessment::Equal,
        }];
        let probes = [LineProbes {
            first: Some(probe(probe_cp)),
            end: None,
        }];
        let report = compare(&p, &root, &lines, &probes);
        let found_cp = report
            .good
            .iter()
            .filter(|g| g.found)
            .map(|g| g.mover_cp)
            .max();
        for g in report.good.iter().filter(|g| g.crucial) {
            assert!(Some(g.mover_cp) > found_cp, "probe {probe_cp}: {g:?}");
        }
        assert!(
            report.good.iter().find(|g| g.san == "e4").unwrap().crucial,
            "e4 (60) is better than d4 (40) whatever the probe says ({probe_cp})"
        );
        assert!(
            !report.good.iter().find(|g| g.san == "Nf3").unwrap().crucial,
            "Nf3 (30) is worse than d4 ({probe_cp})"
        );
    }
}

#[test]
fn a_move_outside_the_multipv_uses_the_probe_but_never_above_the_worst_line() {
    let p = Position::new();
    let root = [root_line(&p, 1, "e2e4", 40), root_line(&p, 2, "d2d4", 30)];
    let lines = [UserLine {
        moves: read_moves(&p, "a3").moves,
        assessment: Assessment::Equal,
    }];
    let probes = [LineProbes {
        first: Some(probe(900)),
        end: None,
    }];
    let report = compare(&p, &root, &lines, &probes);
    assert!(report.has_crucial_miss(), "a3 is worth at most 30 here");
}

// ---- (4) the file is read before it is written (D2354) ----

const OLD_FILE: &str = "version = 1\n[config]\nmultipv = 8\n[stats]\ndone = 5\ntotal_score = 300\nbest_score = 90\n\
[[history]]\nwhen = 1\nscore = 60\nfound = 1\ngood = 2\nlines = 3\ncrucial = false\nwhite_to_move = true\nfen = \"x\"\n";

struct Rig {
    state: AppState,
    dir: tempfile::TempDir,
    held: VecDeque<Effect>,
    root: Vec<(RequestId, Box<RootJob>)>,
    lines: Vec<(RequestId, Box<LinesJob>)>,
    saves: usize,
}

impl Rig {
    fn new(file: Option<&str>) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let prefs = dir.path().join("preferences.toml");
        if let Some(text) = file {
            std::fs::write(store_path(&prefs), text).unwrap();
        }
        let mut state = AppState::with_preferences_path(Some(prefs));
        state
            .engines
            .add(EngineDraft::new("Fake", "/fake"))
            .unwrap();
        Rig {
            state,
            dir,
            held: VecDeque::new(),
            root: Vec::new(),
            lines: Vec::new(),
            saves: 0,
        }
    }

    fn file(&self) -> PathBuf {
        store_path(&self.dir.path().join("preferences.toml"))
    }

    /// Sorts the effects: the reads of the file are held for the test to
    /// deliver, the rest is kept or run.
    async fn take(&mut self, effects: Vec<Effect>) {
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            match effect {
                Effect::Calvar(CalvarEffect::LoadStore { .. }) => self.held.push_back(effect),
                Effect::Calvar(CalvarEffect::AnalyzeRoot { request, job }) => {
                    self.root.push((request, job));
                }
                Effect::Calvar(CalvarEffect::AnalyzeLines { request, job }) => {
                    self.lines.push((request, job));
                }
                Effect::Calvar(CalvarEffect::SaveStore { .. }) => {
                    self.saves += 1;
                    let answer = perform(effect).await;
                    queue.extend(update(&mut self.state, answer));
                }
                _ => {}
            }
        }
    }

    async fn send(&mut self, m: CalvarMessage) {
        let effects = update(&mut self.state, Message::Calvar(m));
        self.take(effects).await;
    }

    /// Delivers the held reads.
    async fn deliver(&mut self) {
        while let Some(effect) = self.held.pop_front() {
            let answer = perform(effect).await;
            let effects = update(&mut self.state, answer);
            self.take(effects).await;
        }
    }

    async fn start(&mut self) {
        self.send(CalvarMessage::Start {
            config: Box::new(CalvarConfig {
                lines_required: 1,
                root_ms: 200,
                line_ms: 200,
                ..CalvarConfig::default()
            }),
            now: 9,
        })
        .await;
    }

    async fn finish(&mut self) {
        self.send(CalvarMessage::EnterText("e4 e5".into())).await;
        self.send(CalvarMessage::EndLine(Assessment::Equal)).await;
        let (request, job) = self.root.pop().unwrap();
        let p = job.position.clone();
        self.send(CalvarMessage::RootAnalyzed {
            request,
            result: Ok(vec![root_line(&p, 1, "e2e4", 30)]),
        })
        .await;
        self.send(CalvarMessage::Done).await;
        let (request, _) = self.lines.pop().unwrap();
        self.send(CalvarMessage::LinesAnalyzed {
            request,
            result: Ok(vec![LineProbes {
                first: Some(probe(30)),
                end: Some(probe(20)),
            }]),
        })
        .await;
    }
}

#[tokio::test]
async fn a_start_before_the_file_is_read_waits_and_keeps_the_old_history() {
    let mut rig = Rig::new(Some(OLD_FILE));
    rig.start().await;
    assert_eq!(
        rig.state.calvar_phase(),
        CalvarPhase::Closed,
        "waits for the file"
    );
    assert!(rig.root.is_empty());
    assert_eq!(rig.held.len(), 1, "the read was asked for");
    rig.deliver().await;
    assert_eq!(
        rig.state.calvar_phase(),
        CalvarPhase::Entering,
        "now it starts"
    );
    rig.finish().await;
    rig.send(CalvarMessage::Close).await;
    let text = std::fs::read_to_string(rig.file()).unwrap();
    assert!(
        text.contains("done = 6"),
        "the old five and the new one: {text}"
    );
    assert!(
        text.contains("when = 1\n"),
        "the old entry is still there: {text}"
    );
    assert!(text.contains("when = 9\n"), "{text}");
    assert!(text.contains("multipv = 8") || text.contains("multipv = 6"));
}

#[tokio::test]
async fn clearing_the_history_before_the_file_is_read_waits_and_keeps_the_rest() {
    let mut rig = Rig::new(Some(OLD_FILE));
    rig.send(CalvarMessage::ResetStats).await;
    assert_eq!(rig.saves, 0, "nothing is written before the file is read");
    assert_eq!(std::fs::read_to_string(rig.file()).unwrap(), OLD_FILE);
    rig.deliver().await;
    assert_eq!(rig.saves, 1);
    let text = std::fs::read_to_string(rig.file()).unwrap();
    assert!(!text.contains("[[history]]"), "{text}");
    assert!(
        text.contains("multipv = 8"),
        "the configuration stays: {text}"
    );
}

#[tokio::test]
async fn closing_before_the_file_is_read_writes_nothing() {
    let mut rig = Rig::new(Some(OLD_FILE));
    rig.send(CalvarMessage::Close).await;
    rig.send(CalvarMessage::CloseDialog).await;
    assert_eq!(rig.saves, 0);
    assert_eq!(std::fs::read_to_string(rig.file()).unwrap(), OLD_FILE);
}

#[tokio::test]
async fn a_file_of_a_newer_version_is_never_written() {
    let newer = "version = 99\n[stats]\ndone = 7\nfuture = true\n";
    let mut rig = Rig::new(Some(newer));
    rig.start().await;
    rig.deliver().await;
    assert_eq!(rig.state.calvar_phase(), CalvarPhase::Entering);
    rig.finish().await;
    rig.send(CalvarMessage::ResetStats).await;
    rig.send(CalvarMessage::Close).await;
    assert_eq!(rig.saves, 0);
    assert_eq!(std::fs::read_to_string(rig.file()).unwrap(), newer);
    assert_eq!(
        rig.state.calvar_view().store_issue,
        Some(CalvarNotice::StoreNewer)
    );
}

#[tokio::test]
async fn a_read_that_could_not_run_protects_the_file() {
    // What the effect answers when the blocking task dies.
    let mut rig = Rig::new(Some(OLD_FILE));
    rig.send(CalvarMessage::Open).await;
    rig.held.clear();
    rig.send(CalvarMessage::StoreLoaded(Box::new(LoadedStore {
        unreadable: true,
        read_only: true,
        ..LoadedStore::default()
    })))
    .await;
    rig.start().await;
    rig.finish().await;
    rig.send(CalvarMessage::Close).await;
    assert_eq!(rig.saves, 0);
    assert_eq!(std::fs::read_to_string(rig.file()).unwrap(), OLD_FILE);
}

#[tokio::test]
async fn closing_without_any_change_does_not_write_the_file() {
    let mut rig = Rig::new(Some(OLD_FILE));
    rig.send(CalvarMessage::Open).await;
    rig.deliver().await;
    // The stored configuration is the one used: nothing changes.
    let config = rig.state.calvar_view().config;
    rig.send(CalvarMessage::Start {
        config: Box::new(config),
        now: 1,
    })
    .await;
    assert_eq!(rig.state.calvar_phase(), CalvarPhase::Entering);
    rig.send(CalvarMessage::Close).await;
    assert_eq!(rig.saves, 0, "no result, no new configuration, no write");
}

#[tokio::test]
async fn a_changed_configuration_is_written_on_close() {
    let mut rig = Rig::new(Some(OLD_FILE));
    rig.start().await;
    rig.deliver().await;
    rig.send(CalvarMessage::Close).await;
    assert_eq!(rig.saves, 1);
    assert!(
        std::fs::read_to_string(rig.file())
            .unwrap()
            .contains("line_ms = 200")
    );
}

// ---- (5) the initiators of the other modes (D2358) ----

async fn with_a_user_game_and_an_exercise() -> Rig {
    let mut rig = Rig::new(None);
    let mv = rig.state.session.position().parse_uci("e2e4").unwrap();
    let effects = update(
        &mut rig.state,
        Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    );
    rig.take(effects).await;
    rig.start().await;
    rig.deliver().await;
    let effects = update(
        &mut rig.state,
        Message::AnswerDiscard(DiscardChoice::DontSave),
    );
    rig.take(effects).await;
    assert_eq!(rig.state.calvar_phase(), CalvarPhase::Entering);
    assert_eq!(rig.state.session.game().tags().event, "Calculation");
    rig
}

#[tokio::test]
async fn starting_a_tournament_ends_the_exercise_and_gives_the_game_back() {
    let mut rig = with_a_user_game_and_an_exercise().await;
    let a = rig.state.engines.add(EngineDraft::new("A", "/a")).unwrap();
    let b = rig.state.engines.add(EngineDraft::new("B", "/b")).unwrap();
    let config = TournamentConfig {
        participants: vec![Participant::new(a.0, "A"), Participant::new(b.0, "B")],
        cycles: 1,
        time: TimeControl::Depth(5),
        ..TournamentConfig::default()
    };
    let effects = update(
        &mut rig.state,
        Message::Tournament(TournamentMessage::Start {
            config: Box::new(config),
            at: std::time::Instant::now(),
        }),
    );
    rig.take(effects).await;
    assert_eq!(rig.state.calvar_phase(), CalvarPhase::Closed);
    assert_eq!(
        rig.state.session.game().mainline().count(),
        1,
        "the user's e4"
    );
    assert!(rig.state.is_dirty());
    assert_ne!(rig.state.session.game().tags().event, "Calculation");
}

/// The body of `fn name` in a source file (up to the next function at the
/// same indentation).
fn function_body(source: &str, name: &str) -> String {
    let needle = format!("fn {name}(");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("no fn {name}"));
    let rest = &source[start..];
    let end = rest[needle.len()..]
        .find("\n    fn ")
        .or_else(|| rest[needle.len()..].find("\n    pub"))
        .map_or(rest.len(), |e| e + needle.len());
    rest[..end].to_owned()
}

fn read_src(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every place that starts a mode which takes the editor, the engines or
/// the game of the user, with the file and the guard that keeps it from
/// running over the exercise. A new initiator must be added here: the test
/// is the list (D2358).
#[test]
fn every_initiator_of_a_mode_ends_or_refuses_the_calculation_exercise() {
    const GUARDS: [&str; 11] = [
        "tactics_halt()",
        "calvar_live()",
        "tactics_blocked()",
        "tactics_start_refused()",
        "opening_blocked()",
        "opening_start_refused()",
        "calvar_start_refused()",
        "calvar_blocked()",
        "tactics_stop(",
        "calvar_halt()",
        "calvar_stop(",
    ];
    let initiators = [
        ("state/play_flow.rs", "play_start"),
        ("state/annotate_flow.rs", "annotate_preflight"),
        ("state/report_flow.rs", "review_start"),
        ("state/tournament_flow.rs", "tournament_precheck"),
        ("state/training_flow.rs", "mode_can_start"),
        ("state/online_flow.rs", "online_stop_everything"),
        ("state/replace.rs", "proceed_or_ask"),
        ("tactics/flow.rs", "tactics_start"),
        ("opening_trainer/flow.rs", "opening_start"),
    ];
    for (file, name) in initiators {
        let body = function_body(&read_src(file), name);
        assert!(
            GUARDS.iter().any(|g| body.contains(g)),
            "{file}: `{name}` neither ends nor refuses the exercise"
        );
    }
    // The annotation run and the tournament resume go through the checks
    // above; the three modes of the analysis (threat, shoot-out, automove)
    // share `mode_can_start`.
    let training = read_src("state/training_flow.rs");
    for name in ["threat_toggle", "shootout_toggle", "automove_toggle"] {
        assert!(
            function_body(&training, name).contains("mode_can_start("),
            "{name} skips the common gate"
        );
    }
    let tournament = read_src("state/tournament_flow.rs");
    for name in ["tournament_start", "tournament_resume"] {
        assert!(
            function_body(&tournament, name).contains("tournament_precheck()"),
            "{name} skips the common gate"
        );
    }
}

// ---- (6) unusable engine answers (D2355) ----

#[tokio::test]
async fn an_answer_without_a_single_probe_is_a_failed_engine_not_a_report_of_zeros() {
    let mut rig = Rig::new(None);
    rig.start().await;
    rig.deliver().await;
    rig.send(CalvarMessage::EnterText("e4 e5".into())).await;
    rig.send(CalvarMessage::EndLine(Assessment::Equal)).await;
    let (request, job) = rig.root.pop().unwrap();
    let p = job.position.clone();
    rig.send(CalvarMessage::RootAnalyzed {
        request,
        result: Ok(vec![root_line(&p, 1, "e2e4", 30)]),
    })
    .await;
    rig.send(CalvarMessage::Done).await;
    let (request, _) = rig.lines.pop().unwrap();
    rig.send(CalvarMessage::LinesAnalyzed {
        request,
        result: Ok(vec![LineProbes::default()]),
    })
    .await;
    assert_eq!(
        rig.state.calvar_phase(),
        CalvarPhase::Failed(CalvarFailure::EngineUnavailable)
    );
    assert_eq!(rig.state.calvar_view().stats.done, 0, "not in the history");
    assert_eq!(
        rig.state.calvar_view().notice,
        Some(CalvarNotice::EngineFailed)
    );
}

#[tokio::test]
async fn one_usable_probe_is_enough_for_a_report() {
    let mut rig = Rig::new(None);
    rig.start().await;
    rig.deliver().await;
    rig.send(CalvarMessage::EnterText("e4".into())).await;
    rig.send(CalvarMessage::EndLine(Assessment::Equal)).await;
    let (request, job) = rig.root.pop().unwrap();
    let p = job.position.clone();
    rig.send(CalvarMessage::RootAnalyzed {
        request,
        result: Ok(vec![root_line(&p, 1, "e2e4", 30)]),
    })
    .await;
    rig.send(CalvarMessage::Done).await;
    let (request, _) = rig.lines.pop().unwrap();
    rig.send(CalvarMessage::LinesAnalyzed {
        request,
        result: Ok(vec![LineProbes {
            first: Some(probe(30)),
            end: None,
        }]),
    })
    .await;
    assert!(matches!(rig.state.calvar_phase(), CalvarPhase::Reported(_)));
    assert_eq!(rig.state.calvar_view().stats.done, 1);
}

// ---- (6) bounds, with the fake engine ----

#[tokio::test]
async fn the_first_stage_still_works_with_exact_reports() {
    let Some(binary) = m4_3_common::fake_binary() else {
        eprintln!("SKIPPED: mychess-fake-engine is not built");
        return;
    };
    let job = RootJob {
        engine: CalvarEngine(mychess_engine::EngineConfig::new(binary)),
        position: Position::new(),
        multipv: 3,
        think: Duration::from_millis(300),
        cancel: Cancel::new(),
    };
    let lines = analyze_root(&job).await.expect("lines");
    assert!(!lines.is_empty());
    let _ = EngineFailure::Failed;
}

// ---- (7) the collection of the game of the user ----

#[tokio::test]
async fn the_game_comes_back_with_its_collection() {
    use mychess_app::{PgnLoad, PgnSource, ScanLimits, scan_pgn};
    use mychess_pgn::read::ReadOptions;
    let mut rig = Rig::new(None);
    let text = "[Event \"a\"]\n\n1. e4 *\n\n[Event \"b\"]\n\n1. d4 *\n".to_owned();
    let (load, _) = scan_pgn(
        PgnSource::Text(text),
        &ScanLimits::default(),
        &ReadOptions::default(),
    )
    .unwrap();
    let PgnLoad::Many(collection) = load else {
        panic!("two games make a collection");
    };
    rig.state.collection = Some(collection.clone());
    rig.start().await;
    rig.deliver().await;
    assert_eq!(rig.state.calvar_phase(), CalvarPhase::Entering);
    assert!(
        rig.state.collection.is_none(),
        "the exercise has no collection"
    );
    rig.send(CalvarMessage::Close).await;
    assert!(
        rig.state
            .collection
            .as_ref()
            .is_some_and(|c| std::sync::Arc::ptr_eq(c, &collection)),
        "next/previous game still work"
    );
}

// ---- (7) a dialog on top of the board gets the keys first (D2361) ----

#[tokio::test]
async fn a_typed_move_goes_to_the_dialog_on_top_not_to_the_line() {
    use mychess_app::{DiscardPrompt, Pending};
    let mut rig = Rig::new(None);
    rig.start().await;
    rig.deliver().await;
    assert_eq!(rig.state.calvar_phase(), CalvarPhase::Entering);
    rig.state.discard_prompt = Some(DiscardPrompt {
        pending: Pending::Quit,
    });
    let effects = update(
        &mut rig.state,
        Message::Game(GameMessage::EnterMoveText("e4".into())),
    );
    rig.take(effects).await;
    assert_eq!(
        rig.state.calvar_view().current_plies,
        0,
        "nothing was typed"
    );
    rig.state.discard_prompt = None;
    let effects = update(
        &mut rig.state,
        Message::Game(GameMessage::EnterMoveText("e4".into())),
    );
    rig.take(effects).await;
    assert_eq!(rig.state.calvar_view().current_plies, 1);
}

#[tokio::test]
async fn the_cached_view_follows_every_change_of_the_lines() {
    let mut rig = Rig::new(None);
    rig.start().await;
    rig.deliver().await;
    rig.send(CalvarMessage::EnterText("e4 e5".into())).await;
    assert_eq!(rig.state.calvar_view().current_san, "1. e4 e5");
    rig.send(CalvarMessage::Undo).await;
    assert_eq!(rig.state.calvar_view().current_san, "1. e4");
    rig.send(CalvarMessage::EndLine(Assessment::Equal)).await;
    rig.send(CalvarMessage::EnterText("d4".into())).await;
    rig.send(CalvarMessage::EndLine(Assessment::WhiteSlightly))
        .await;
    let view = rig.state.calvar_view();
    assert_eq!(view.current_san, "");
    assert_eq!(
        view.lines
            .iter()
            .map(|l| l.san.as_str())
            .collect::<Vec<_>>(),
        ["1. e4", "1. d4"]
    );
    rig.send(CalvarMessage::RemoveLine(0)).await;
    let view = rig.state.calvar_view();
    assert_eq!(view.lines.len(), 1);
    assert_eq!(view.lines[0].san, "1. d4");
}
