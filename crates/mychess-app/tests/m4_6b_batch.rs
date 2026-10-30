//! Contract tests of the batch annotation over the filter (spec M4-6,
//! phase B, D1520-D1526): a real SI4 database in a temporary directory and a
//! synthetic UCI engine that answers the searches `update` asks for.
#![allow(
    clippy::unwrap_used,
    clippy::assert_is_empty,
    clippy::nonminimal_bool,
    clippy::redundant_closure_for_method_calls,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::type_complexity
)]

mod m4_6_common;

use std::path::{Path, PathBuf};

use m4_6_common::{Reply, Rig, last_search, material_engine, ply_of};
use mychess_app::{
    AnalysisMessage, AnnotateMessage, AnnotateNotice, AppNotice, BaseError, BasesMessage,
    BasesNotice, Command, DiscardChoice, Effect, GameMessage, GameOrigin, ListMessage, Message,
    Nav, perform,
};
use mychess_core::{Game, Position};
use mychess_db::GameSource;
use mychess_db::si4::{GameFlags, Si4Database, Si4Writer};
use mychess_engine::{
    BestMove, EngineCommand, EngineEvent, ExitInfo, ExitReason, OptionValue, Score, SearchId,
    SearchRequest,
};

const THREE: &str = "[White \"Alpha\"]\n[Black \"X\"]\n\n1. e4 e5 2. Nf3 Nc6 *\n\n\
    [White \"Beta\"]\n[Black \"Y\"]\n\n1. d4 d5 *\n\n\
    [White \"Alpha\"]\n[Black \"Z\"]\n\n1. c4 c5 *\n";

struct Bt {
    rig: Rig,
    rt: tokio::runtime::Runtime,
    path: PathBuf,
    #[allow(dead_code, reason = "keeps the temp dir alive")]
    dir: tempfile::TempDir,
}

fn make_si4(dir: &Path, name: &str, pgn: &str) -> PathBuf {
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let base = dir.join(name);
    let mut writer = Si4Writer::create(&base, 0).expect("create si4");
    import_pgn(
        pgn.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("import");
    drop(writer);
    base.with_extension("si4")
}

fn is_engine_side(effect: &Effect) -> bool {
    matches!(
        effect,
        Effect::EngineCommand { .. }
            | Effect::StartEngine { .. }
            | Effect::StopEngine { .. }
            | Effect::ScheduleAnnotateTick { .. }
            | Effect::ScheduleAutoplayTick { .. }
            | Effect::ScheduleModeTick { .. }
    )
}

impl Bt {
    /// A rig with a running fake engine and the database of `pgn` open.
    fn new(pgn: &str) -> Bt {
        let dir = tempfile::tempdir().expect("dir");
        let path = make_si4(dir.path(), "b", pgn);
        let mut bt = Bt {
            rig: Rig::new(Game::new()),
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
            path: path.clone(),
            dir,
        };
        bt.pump(BasesMessage::Open(path));
        bt
    }

    /// `update`, then every effect that is not engine-side is performed and
    /// its answer fed back. Returns the engine-side effects.
    fn pump(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        let mut out = Vec::new();
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in self.rig.send(message) {
                if is_engine_side(&effect) {
                    out.push(effect);
                } else if effect.runs_on_host() {
                } else {
                    queue.push(self.rt.block_on(perform(effect)));
                }
            }
        }
        out
    }

    /// Performs the batch steps among `effects`, feeding the answers back.
    fn settle(&mut self, effects: Vec<Effect>) -> Vec<Effect> {
        let mut out = Vec::new();
        let mut queue = effects;
        while let Some(effect) = queue.pop() {
            if let Effect::AnnotateBatchStep { .. } = effect {
                let message = self.rt.block_on(perform(effect));
                let more = self.rig.send(message);
                queue.extend(more);
            } else if is_engine_side(&effect) {
                out.push(effect);
            }
        }
        out.reverse();
        out
    }

    fn filter_by(&mut self, text: &str) {
        let slot = self.rig.state.bases.current();
        self.pump(BasesMessage::SetSearchText {
            slot,
            text: text.into(),
        });
        self.pump(BasesMessage::List(ListMessage::FilterByText { slot }));
    }

    fn start(&mut self) -> Vec<Effect> {
        let fx = self.rig.send(AnnotateMessage::Start);
        self.settle(fx)
    }

    /// Answers searches until the annotation stops (or `max` searches).
    /// Returns the requests seen.
    fn drive(
        &mut self,
        mut effects: Vec<Effect>,
        max: usize,
        engine: &mut dyn FnMut(&SearchRequest, &Position) -> Reply,
    ) -> Vec<SearchRequest> {
        let mut seen = Vec::new();
        for _ in 0..max {
            if !self.rig.running() {
                break;
            }
            let Some(request) = last_search(&effects) else {
                break;
            };
            let reply = engine(&request, &m4_6_common::position_of(&request));
            let Some((after, request)) = self.rig.answer(&effects, &reply) else {
                break;
            };
            seen.push(request);
            effects = self.settle(after);
        }
        seen
    }

    fn run_all(&mut self) -> Vec<SearchRequest> {
        let fx = self.start();
        self.drive(fx, 500, &mut material_engine)
    }

    fn read(&self, number: u32) -> Game {
        let mut db = Si4Database::open(&self.path).expect("open");
        GameSource::game(&mut db, number).expect("game")
    }

    fn flags(&self, number: u32) -> GameFlags {
        let db = Si4Database::open(&self.path).expect("open");
        GameSource::entry(&db, number).expect("entry").flags
    }

    fn batch_notice(&self) -> Option<AnnotateNotice> {
        self.rig.notice()
    }
}

fn annotated(game: &Game) -> bool {
    game.tags().get_extra("Annotator") == Some("Fake 1")
        && game.mainline().all(|n| {
            game.comment(n)
                .unwrap()
                .is_some_and(|c| c.contains("[%eval"))
        })
}

fn same(a: &Game, b: &Game) -> bool {
    a.same_content(b)
}

// ---- where it applies (D1521) ----

#[test]
fn a_batch_walks_the_whole_filter_and_leaves_the_others_untouched() {
    let mut bt = Bt::new(THREE);
    let untouched = bt.read(2);
    bt.filter_by("Alpha");
    assert_eq!(
        bt.rig.state.bases.current_slot().unwrap().filtered(),
        2,
        "the filter holds games 1 and 3"
    );
    bt.rig.options(|o| o.batch = true);
    let requests = bt.run_all();
    assert!(!bt.rig.running());
    assert_eq!(
        bt.batch_notice(),
        Some(AnnotateNotice::BatchFinished {
            games: 2,
            skipped: 0
        })
    );
    // 4 plies + root, 2 plies + root.
    assert_eq!(requests.len(), 5 + 3);
    assert!(annotated(&bt.read(1)));
    assert!(annotated(&bt.read(3)));
    assert!(
        same(&bt.read(2), &untouched),
        "game 2 is outside the filter"
    );
    assert!(!bt.read(2).tags().get_extra("Annotator").is_some());
    // The last game is saved: the session shows it as not altered.
    assert!(!bt.rig.state.session.is_dirty());
}

#[test]
fn the_batch_starts_from_the_first_game_of_the_filter_whatever_the_current_one() {
    let mut bt = Bt::new(THREE);
    bt.rig.options(|o| o.batch = true);
    let requests = bt.run_all();
    assert_eq!(requests.len(), 5 + 3 + 3);
    for n in 1..=3 {
        assert!(annotated(&bt.read(n)), "game {n}");
    }
}

#[test]
fn a_pgn_slot_refuses_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.pgn");
    std::fs::write(&path, THREE).unwrap();
    let mut bt = Bt::new(THREE);
    bt.pump(BasesMessage::Open(path));
    bt.rig.options(|o| o.batch = true);
    let fx = bt.rig.start();
    assert!(!fx.iter().any(|e| matches!(
        e,
        Effect::AnnotateBatchStep { .. } | Effect::EngineCommand { .. }
    )));
    assert_eq!(bt.batch_notice(), Some(AnnotateNotice::BatchUnavailable));
    assert!(!bt.rig.running());
}

#[test]
fn the_clipbase_refuses_the_batch() {
    let mut bt = Bt::new(THREE);
    let clip = mychess_app::SlotId::CLIPBASE;
    bt.pump(BasesMessage::Select(clip));
    assert_eq!(bt.rig.state.bases.current(), clip);
    bt.rig.options(|o| o.batch = true);
    let fx = bt.rig.start();
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::AnnotateBatchStep { .. }))
    );
    assert_eq!(bt.batch_notice(), Some(AnnotateNotice::BatchUnavailable));
    assert!(!bt.rig.running());
}

#[test]
fn a_read_only_si4_refuses_the_batch_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_si4(dir.path(), "ro", THREE);
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms.clone()).unwrap();
    let mut bt = Bt::new(THREE);
    bt.pump(BasesMessage::Open(path.clone()));
    let ro = bt.rig.state.bases.current_slot().unwrap().read_only;
    bt.rig.options(|o| o.batch = true);
    let fx = bt.rig.start();
    #[allow(clippy::permissions_set_readonly_false)]
    {
        perms.set_readonly(false);
        std::fs::set_permissions(&path, perms).unwrap();
    }
    if ro {
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::AnnotateBatchStep { .. }))
        );
        assert_eq!(bt.batch_notice(), Some(AnnotateNotice::BatchUnavailable));
        assert!(!bt.rig.running());
    }
}

#[test]
fn unsaved_changes_in_the_current_game_refuse_the_batch() {
    let mut bt = Bt::new(THREE);
    bt.pump(GameMessage::Navigate(Nav::GameEnd));
    let mv = bt.rig.state.session.position().parse_uci("a2a3");
    // The current game is the first of the base; add a move to dirty it.
    let mv = mv.or_else(|_| bt.rig.state.session.position().parse_uci("a7a6"));
    bt.pump(GameMessage::PlayMove {
        mv: mv.unwrap(),
        options: mychess_app::PlayOptions::default(),
    });
    assert!(bt.rig.state.session.is_dirty());
    let before = bt.rig.game().clone();
    bt.rig.options(|o| o.batch = true);
    let fx = bt.rig.start();
    assert!(fx.is_empty());
    assert_eq!(bt.batch_notice(), Some(AnnotateNotice::BatchDirty));
    assert!(!bt.rig.running());
    assert!(same(bt.rig.game(), &before), "the game in edition is kept");
}

#[test]
fn an_empty_filter_has_nothing_to_annotate() {
    let mut bt = Bt::new(THREE);
    bt.filter_by("NoSuchPlayerAnywhere");
    assert_eq!(bt.rig.state.bases.current_slot().unwrap().filtered(), 0);
    bt.rig.options(|o| o.batch = true);
    let fx = bt.rig.start();
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::AnnotateBatchStep { .. }))
    );
    assert_eq!(bt.batch_notice(), Some(AnnotateNotice::NothingToAnnotate));
    assert!(!bt.rig.running());
}

// ---- what is written (D1523, D1524, D1525) ----

#[test]
fn a_game_without_moves_is_annotated_not_skipped() {
    let pgn =
        "[White \"Alpha\"]\n[Black \"X\"]\n\n*\n\n[White \"Alpha\"]\n[Black \"Y\"]\n\n1. e4 *\n";
    let mut bt = Bt::new(pgn);
    bt.rig.options(|o| o.batch = true);
    bt.run_all();
    assert_eq!(
        bt.batch_notice(),
        Some(AnnotateNotice::BatchFinished {
            games: 2,
            skipped: 0
        })
    );
    let empty = bt.read(1);
    let root = empty.root();
    let text = empty
        .comment(root)
        .unwrap()
        .expect("initial comment (rule 11)");
    assert!(text.contains("13:"), "got {text:?}");
    assert_eq!(empty.tags().get_extra("Annotator"), Some("Fake 1"));
}

#[test]
fn a_game_with_nothing_to_analyse_is_skipped_and_left_as_it_was() {
    // Game 1 starts at move 30: with "opening only" (5 moves) no move is
    // inside the opening, so there is nothing to annotate.
    let late = "[White \"Alpha\"]\n[Black \"X\"]\n[SetUp \"1\"]\n\
        [FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 30\"]\n\n30. e4 *\n\n\
        [White \"Alpha\"]\n[Black \"Y\"]\n\n1. e4 e5 *\n";
    let mut bt = Bt::new(late);
    let original = bt.read(1);
    assert_eq!(original.start_position().fullmove_number(), 30, "fixture");
    bt.rig.options(|o| {
        o.batch = true;
        o.opening_only = Some(5);
    });
    bt.run_all();
    assert_eq!(
        bt.batch_notice(),
        Some(AnnotateNotice::BatchFinished {
            games: 1,
            skipped: 1
        })
    );
    assert!(same(&bt.read(1), &original), "skipped game untouched");
    assert!(bt.read(2).tags().get_extra("Annotator").is_some());
}

#[test]
fn the_tactics_flag_is_written_after_the_game_is_replaced() {
    let pgn = "[White \"Alpha\"]\n[Black \"X\"]\n\n1. e4 e5 2. Qh5 Nc6 3. Qxe5 *\n\n\
        [White \"Alpha\"]\n[Black \"Y\"]\n\n1. d4 d5 *\n";
    let mut bt = Bt::new(pgn);
    bt.rig.options(|o| {
        o.batch = true;
        o.mark_exercises = true;
        o.var_at_end = false;
    });
    // The blunder is ply 5; every probe at depth <= 2 or below 50 ms is wrong.
    let mut engine = |r: &SearchRequest, pos: &Position| -> Reply {
        let is_probe = r.limits.depth != Some(13);
        match ply_of(r) {
            4 => Reply::cp(30, vec![pos.parse_uci("g1f3").unwrap()]),
            5 => {
                let best = pos.parse_uci("c6e5").unwrap();
                let mut reply = Reply::cp(-800, vec![best]);
                reply.second = Some(Score::Cp(0));
                if is_probe {
                    let ms = r.limits.movetime.map(|d| d.as_millis());
                    if r.limits.depth == Some(2) || ms.is_some_and(|m| m < 50) {
                        reply.pv = pos
                            .legal_moves()
                            .into_iter()
                            .filter(|m| *m != best)
                            .take(1)
                            .collect();
                    }
                }
                reply
            }
            _ => Reply::cp(30, Vec::new()),
        }
    };
    let fx = bt.start();
    bt.drive(fx, 500, &mut engine);
    assert_eq!(
        bt.batch_notice(),
        Some(AnnotateNotice::BatchFinished {
            games: 2,
            skipped: 0
        })
    );
    assert!(bt.flags(1).contains(GameFlags::TACTICS));
    assert!(!bt.flags(2).contains(GameFlags::TACTICS), "context reset");
    // The annotation itself survived the flag write.
    assert!(bt.read(1).tags().get_extra("Annotator").is_some());
}

// ---- stopping (D1523) ----

fn cancel_now(bt: &mut Bt, effects: &[Effect]) {
    let request = last_search(effects).expect("a search in flight");
    bt.rig.send(AnnotateMessage::Cancel);
    bt.rig.event(EngineEvent::SearchTagged {
        search: SearchId(900),
        tag: request.tag,
    });
    bt.rig.event(EngineEvent::SearchStarted(SearchId(900)));
    let after = bt.rig.event(EngineEvent::BestMove(BestMove {
        search: SearchId(900),
        mv: None,
        ponder: None,
    }));
    bt.settle(after);
}

#[test]
fn cancelling_the_first_game_stops_the_batch_without_saving_it() {
    let mut bt = Bt::new(THREE);
    let before: Vec<Game> = (1..=3).map(|n| bt.read(n)).collect();
    bt.rig.options(|o| o.batch = true);
    let fx = bt.start();
    // Two searches answered, then Esc.
    let engine = material_engine;
    let mut effects = fx;
    for _ in 0..2 {
        let request = last_search(&effects).unwrap();
        let reply = engine(&request, &m4_6_common::position_of(&request));
        let (after, _) = bt.rig.answer(&effects, &reply).unwrap();
        effects = bt.settle(after);
    }
    cancel_now(&mut bt, &effects);
    assert!(!bt.rig.running(), "the batch is over");
    assert_eq!(
        bt.batch_notice(),
        Some(AnnotateNotice::BatchStopped { games: 0, total: 3 })
    );
    for (i, game) in before.iter().enumerate() {
        assert!(
            same(&bt.read(u32::try_from(i).unwrap() + 1), game),
            "game {}",
            i + 1
        );
    }
    // The edits stay in the session, which is altered (D469).
    assert!(bt.rig.state.session.is_dirty());
    assert!(bt.rig.game().tags().get_extra("Annotator").is_some());
}

#[test]
fn cancelling_the_second_game_keeps_the_first_saved_and_the_rest_untouched() {
    let mut bt = Bt::new(THREE);
    let g2 = bt.read(2);
    let g3 = bt.read(3);
    bt.rig.options(|o| o.batch = true);
    let mut effects = bt.start();
    // Game 1 has 5 positions; answer them, then 1 search of game 2.
    for _ in 0..6 {
        let request = last_search(&effects).unwrap();
        let reply = material_engine(&request, &m4_6_common::position_of(&request));
        let (after, _) = bt.rig.answer(&effects, &reply).unwrap();
        effects = bt.settle(after);
    }
    assert!(bt.rig.running());
    cancel_now(&mut bt, &effects);
    assert!(!bt.rig.running());
    assert_eq!(
        bt.batch_notice(),
        Some(AnnotateNotice::BatchStopped { games: 1, total: 3 })
    );
    assert!(annotated(&bt.read(1)));
    assert!(same(&bt.read(2), &g2), "the cancelled game is not saved");
    assert!(same(&bt.read(3), &g3));
}

#[test]
fn an_engine_that_dies_stops_the_batch_without_saving_the_game_in_progress() {
    let mut bt = Bt::new(THREE);
    let g2 = bt.read(2);
    bt.rig.options(|o| o.batch = true);
    let mut effects = bt.start();
    for _ in 0..6 {
        let request = last_search(&effects).unwrap();
        let reply = material_engine(&request, &m4_6_common::position_of(&request));
        let (after, _) = bt.rig.answer(&effects, &reply).unwrap();
        effects = bt.settle(after);
    }
    let after = bt.rig.event(EngineEvent::Exited(ExitInfo {
        reason: ExitReason::Crashed,
        status: None,
        stderr_tail: Vec::new(),
    }));
    bt.settle(after);
    assert!(!bt.rig.running());
    assert_eq!(
        bt.batch_notice(),
        Some(AnnotateNotice::BatchStopped { games: 1, total: 3 })
    );
    assert!(annotated(&bt.read(1)));
    assert!(same(&bt.read(2), &g2));
}

#[test]
fn a_file_that_changed_during_the_batch_stops_it_with_a_stale_warning() {
    let mut bt = Bt::new(THREE);
    let g2 = bt.read(2);
    bt.rig.options(|o| o.batch = true);
    // The file changes behind the back of the slot (a new modification time).
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&bt.path)
        .unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
        .unwrap();
    drop(file);
    let mut effects = bt.start();
    // The very first step is refused (nothing was read yet): the batch never
    // starts an engine search.
    effects.retain(|e| matches!(e, Effect::EngineCommand { .. }));
    assert!(last_search(&effects).is_none());
    assert!(!bt.rig.running());
    assert_eq!(
        bt.rig.state.notice,
        Some(AppNotice::Bases(BasesNotice::Failed(BaseError::Stale)))
    );
    assert!(same(&bt.read(2), &g2));
}

// ---- during the batch ----

#[test]
fn between_two_games_the_annotation_still_runs_and_edits_are_refused() {
    let mut bt = Bt::new(THREE);
    bt.rig.options(|o| o.batch = true);
    let mut effects = bt.start();
    // Answer the five searches of game 1 but keep the last step unperformed.
    for _ in 0..4 {
        let request = last_search(&effects).unwrap();
        let reply = material_engine(&request, &m4_6_common::position_of(&request));
        let (after, _) = bt.rig.answer(&effects, &reply).unwrap();
        effects = bt.settle(after);
    }
    let request = last_search(&effects).unwrap();
    let reply = material_engine(&request, &m4_6_common::position_of(&request));
    let (after, _) = bt.rig.answer(&effects, &reply).unwrap();
    assert!(
        after
            .iter()
            .any(|e| matches!(e, Effect::AnnotateBatchStep { .. })),
        "game 1 is done: one step saves it and reads the next"
    );
    assert!(bt.rig.running(), "running between the games");
    assert!(bt.rig.state.annotation_progress().is_some());
    let before = bt.rig.game().clone();
    bt.rig.send(GameMessage::Navigate(Nav::Start));
    assert_eq!(bt.batch_notice(), Some(AnnotateNotice::Busy));
    assert!(same(bt.rig.game(), &before));
    // The step runs: game 2 of the batch is on the board.
    let effects = bt.settle(after);
    assert!(last_search(&effects).is_some());
    let progress = bt.rig.state.annotation_progress().unwrap();
    assert_eq!((progress.game_index, progress.game_total), (2, 3));
}

#[test]
fn the_progress_counts_games_of_the_filter() {
    let mut bt = Bt::new(THREE);
    bt.rig.options(|o| o.batch = true);
    let effects = bt.start();
    let progress = bt.rig.state.annotation_progress().unwrap();
    assert_eq!((progress.game_index, progress.game_total), (1, 3));
    assert!(last_search(&effects).is_some());
    // Edits are refused during the first game too.
    bt.rig.send(GameMessage::Navigate(Nav::GameEnd));
    assert_eq!(bt.batch_notice(), Some(AnnotateNotice::Busy));
}

#[test]
fn the_engine_is_prepared_again_for_every_game_and_restored_at_the_end() {
    let mut bt = Bt::new(THREE);
    bt.rig.options(|o| o.batch = true);
    let mut effects = bt.start();
    let mut all = m4_6_common::commands(&effects);
    for _ in 0..40 {
        if !bt.rig.running() {
            break;
        }
        let request = last_search(&effects).unwrap();
        let reply = material_engine(&request, &m4_6_common::position_of(&request));
        let (after, _) = bt.rig.answer(&effects, &reply).unwrap();
        effects = bt.settle(after);
        all.extend(m4_6_common::commands(&effects));
    }
    assert!(!bt.rig.running());
    let own_book: Vec<bool> = all
        .iter()
        .filter_map(|c| match c {
            EngineCommand::SetOption { name, value } if name == "OwnBook" => {
                Some(*value == OptionValue::Bool(true))
            }
            _ => None,
        })
        .collect();
    let off = own_book.iter().filter(|on| !**on).count();
    assert!(
        off >= 1,
        "the own book is off during the games: {own_book:?}"
    );
    assert_eq!(own_book.last(), Some(&true), "restored at the end");
}

// ---- review fixes (D1550-D1556) ----

/// Plays game 1 to its end and returns with the step that saves it and reads
/// game 2 still unperformed.
fn between_games(bt: &mut Bt) -> Vec<Effect> {
    bt.rig.options(|o| o.batch = true);
    let mut effects = bt.start();
    for _ in 0..4 {
        let request = last_search(&effects).unwrap();
        let reply = material_engine(&request, &m4_6_common::position_of(&request));
        let (after, _) = bt.rig.answer(&effects, &reply).unwrap();
        effects = bt.settle(after);
    }
    let request = last_search(&effects).unwrap();
    let reply = material_engine(&request, &m4_6_common::position_of(&request));
    let (after, _) = bt.rig.answer(&effects, &reply).unwrap();
    assert!(
        after
            .iter()
            .any(|e| matches!(e, Effect::AnnotateBatchStep { .. }))
    );
    after
}

/// Performs the batch step among `effects` and returns what its answer asks.
fn run_step(bt: &mut Bt, effects: Vec<Effect>) -> Vec<Effect> {
    let step = effects
        .into_iter()
        .find(|e| matches!(e, Effect::AnnotateBatchStep { .. }))
        .expect("a batch step");
    let message = bt.rt.block_on(perform(step));
    bt.rig.send(message)
}

#[test]
fn quitting_between_two_games_waits_for_the_step_and_does_not_relaunch() {
    let mut bt = Bt::new(THREE);
    let g3 = bt.read(3);
    let step = between_games(&mut bt);
    let mut out = bt.rig.send(Message::Quit);
    if bt.rig.state.discard_prompt.is_some() {
        out.extend(bt.rig.send(Message::AnswerDiscard(DiscardChoice::DontSave)));
    }
    assert!(
        !out.iter().any(|e| matches!(e, Effect::Exit)),
        "the step in flight finishes first: {out:?}"
    );
    assert!(out.iter().any(|e| matches!(e, Effect::QuitDeadline { .. })));
    let after = run_step(&mut bt, step);
    // The quit goes on: the engines are shut down before the exit.
    assert!(
        after
            .iter()
            .any(|e| matches!(e, Effect::Exit | Effect::ShutdownEngines { .. })),
        "{after:?}"
    );
    assert!(
        !after.iter().any(is_engine_side_start),
        "the next game is not launched: {after:?}"
    );
    assert!(!bt.rig.running());
    assert!(same(&bt.read(3), &g3));
}

fn is_engine_side_start(effect: &Effect) -> bool {
    matches!(effect, Effect::EngineCommand { .. })
}

#[test]
fn declining_the_quit_lets_the_cancelled_batch_stop_by_itself() {
    let mut bt = Bt::new(THREE);
    let step = between_games(&mut bt);
    bt.rig.send(Message::Quit);
    assert!(bt.rig.state.discard_prompt.is_some(), "the game is altered");
    bt.rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    let after = run_step(&mut bt, step);
    assert!(!after.iter().any(|e| matches!(e, Effect::Exit)));
    assert!(!bt.rig.running(), "the batch ended");
    assert_eq!(
        bt.batch_notice(),
        Some(AnnotateNotice::BatchStopped { games: 1, total: 3 })
    );
}

#[test]
fn list_jobs_are_refused_while_the_batch_owns_the_database() {
    let mut bt = Bt::new(THREE);
    let _step = between_games(&mut bt);
    let slot = bt.rig.state.bases.current();
    bt.rig.send(BasesMessage::SetSearchText {
        slot,
        text: "Alpha".into(),
    });
    let fx = bt
        .rig
        .send(BasesMessage::List(ListMessage::FilterByText { slot }));
    assert!(fx.is_empty(), "no job starts: {fx:?}");
    assert_eq!(bt.batch_notice(), Some(AnnotateNotice::Busy));
}

#[test]
fn saving_is_refused_during_the_batch_and_the_old_origin_is_dropped() {
    let mut bt = Bt::new(THREE);
    bt.rig.state.origin = GameOrigin::SavedFile {
        path: bt.dir.path().join("old.pgn"),
    };
    let step = between_games(&mut bt);
    assert_eq!(bt.rig.state.origin, GameOrigin::New);
    for command in [Command::Save, Command::SaveAs] {
        let fx = bt.rig.send(Message::Command(command));
        assert!(fx.is_empty(), "{command:?}: {fx:?}");
        assert_eq!(bt.batch_notice(), Some(AnnotateNotice::Busy));
    }
    // The next game of the batch does not bring an origin back.
    run_step(&mut bt, step);
    assert_eq!(bt.rig.state.origin, GameOrigin::New);
}

#[test]
fn the_batch_option_is_cleared_when_the_batch_ends() {
    let mut bt = Bt::new(THREE);
    bt.rig.options(|o| o.batch = true);
    let fx = bt.start();
    bt.drive(fx, 500, &mut material_engine);
    assert!(!bt.rig.running());
    assert!(!bt.rig.state.annotate_options.batch);
}

#[test]
fn the_engine_of_the_batch_is_not_the_boards_between_two_games() {
    let mut bt = Bt::new(THREE);
    let _step = between_games(&mut bt);
    let run = bt.rig.run;
    let analyzing = bt
        .rig
        .state
        .engine_runs()
        .iter()
        .find(|r| r.id == run)
        .map(|r| r.analyzing);
    let fx = bt.rig.send(AnalysisMessage::Toggle(run));
    assert!(fx.is_empty(), "{fx:?}");
    let now = bt
        .rig
        .state
        .engine_runs()
        .iter()
        .find(|r| r.id == run)
        .map(|r| r.analyzing);
    assert_eq!(analyzing, now);
}
