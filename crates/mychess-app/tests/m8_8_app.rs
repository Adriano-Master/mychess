//! M8-8b contract tests (spec D2562-D2579, D2583-D2586): the tablebase panel
//! and the endgame training in the application. `update` is driven with
//! stub sources; the delays are fired by hand.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::large_futures,
    clippy::assert_is_empty
)]

use std::path::PathBuf;
use std::sync::Arc;

use mychess_app::online::{OnlinePhase, PlayingState, family_of};
use mychess_app::tablebase::{
    DelayKind, LocalHandle, OnlineHandle, OpenedSet, TablebaseEffect, TablebaseMessage,
    TablebaseNotice, TablebaseStatus, Verdict,
};
use mychess_app::{
    Action, AppNotice, AppState, Command, DiscardChoice, Effect, GameMessage, Message, PlayOptions,
    RequestId, perform, update,
};
use mychess_core::{Game, Position};
use mychess_tablebase::testing::{StubProbe, report};
use mychess_tablebase::{Category, Evaluation, OpenReport, Source, TablebaseError, material_name};

const KQ: &str = "8/8/8/8/8/2k5/8/KQ6 w - - 0 1";

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).unwrap()
}

/// What a probe of `position` answers.
fn win_report(position: &Position, source: Source) -> mychess_tablebase::PositionReport {
    report(
        position,
        source,
        Evaluation::of(Category::Win),
        &[
            ("b1e4", Category::Win, Some(9)),
            ("b1d1", Category::Win, Some(3)),
            ("b1b2", Category::Draw, Some(0)),
            ("a1a2", Category::Loss, Some(-4)),
        ],
    )
}

struct Rig {
    state: AppState,
    _dir: tempfile::TempDir,
    local: Arc<StubProbe>,
    online: Arc<StubProbe>,
    /// Every effect of the tablebase that `update` asked for.
    seen: Vec<TablebaseEffect>,
    /// What the folders held, as the answer to `Open`.
    open_report: OpenReport,
    /// Delays that were asked for and not fired.
    delays: Vec<(RequestId, DelayKind)>,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
        let online = Arc::new(StubProbe::new(7));
        state.set_tablebase_online(OnlineHandle::from_probe(online.clone()));
        state.session.replace_game(Game::from_position(pos(KQ)));
        Rig {
            state,
            _dir: dir,
            local: Arc::new(StubProbe::new(5)),
            online,
            seen: Vec::new(),
            open_report: OpenReport {
                max_pieces: 5,
                wdl_files: 1,
                ..OpenReport::default()
            },
            delays: Vec::new(),
        }
    }

    /// Sends `message` and runs what it asks for, except the delays.
    async fn send(&mut self, message: impl Into<Message>) {
        let effects = update(&mut self.state, message.into());
        self.pump(effects).await;
    }

    async fn pump(&mut self, effects: Vec<Effect>) {
        let mut queue: std::collections::VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            let Effect::Tablebase(tb) = effect else {
                continue;
            };
            self.seen.push(tb.clone());
            let answer = match tb {
                TablebaseEffect::Open { request, .. } => {
                    Message::Tablebase(TablebaseMessage::Opened {
                        request,
                        result: Box::new(Ok(OpenedSet {
                            handle: LocalHandle::new(self.local.clone()),
                            report: self.open_report.clone(),
                        })),
                    })
                }
                TablebaseEffect::Delay { request, kind, .. } => {
                    self.delays.push((request, kind));
                    continue;
                }
                other => perform(Effect::Tablebase(other)).await,
            };
            queue.extend(update(&mut self.state, answer));
        }
    }

    fn probes(&self) -> usize {
        self.seen
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    TablebaseEffect::ProbeLocal { .. } | TablebaseEffect::ProbeOnline { .. }
                )
            })
            .count()
    }

    fn online_probes(&self) -> usize {
        self.seen
            .iter()
            .filter(|e| matches!(e, TablebaseEffect::ProbeOnline { .. }))
            .count()
    }

    async fn fire(&mut self, kind: DelayKind) -> bool {
        let Some(index) = self.delays.iter().position(|(_, k)| *k == kind) else {
            return false;
        };
        let (request, kind) = self.delays.remove(index);
        self.send(TablebaseMessage::DelayElapsed { request, kind })
            .await;
        true
    }

    /// Turns the local tables on, with a folder, and opens the panel.
    async fn local_panel(&mut self) {
        self.local
            .answer(&pos(KQ), Ok(win_report(&pos(KQ), Source::Local)));
        self.send(TablebaseMessage::AddDirectory(PathBuf::from("syzygy")))
            .await;
        self.send(TablebaseMessage::SetOpen(true)).await;
    }

    fn view(&self) -> mychess_app::tablebase::TablebaseView {
        self.state.tablebase_view()
    }

    async fn play(&mut self, uci: &str) {
        let mv = self.state.session.position().parse_uci(uci).unwrap();
        self.send(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        })
        .await;
    }
}

#[tokio::test]
async fn the_panel_without_configuration_asks_nothing() {
    let mut rig = Rig::new();
    rig.send(TablebaseMessage::SetOpen(true)).await;
    assert!(rig.seen.is_empty());
    let view = rig.view();
    assert!(view.open);
    assert_eq!(view.status, TablebaseStatus::NotConfigured);
    assert!(view.rows.is_empty());
}

#[tokio::test]
async fn local_tables_are_opened_and_the_position_is_probed() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    let opens = rig
        .seen
        .iter()
        .filter(|e| matches!(e, TablebaseEffect::Open { dirs, .. } if dirs == &[PathBuf::from("syzygy")]))
        .count();
    assert_eq!(opens, 1);
    assert_eq!(rig.probes(), 1);
    let view = rig.view();
    assert_eq!(view.status, TablebaseStatus::Ready);
    assert_eq!(view.source, Some(Source::Local));
    assert_eq!(view.position.map(|e| e.category), Some(Category::Win));
    let order: Vec<&str> = view.rows.iter().take(4).map(|r| r.san.as_str()).collect();
    assert_eq!(order, ["Qd1", "Qe4", "Qb2+", "Ka2"]);
    assert!(view.rows[0].best && !view.rows[1].best);
    assert_eq!(view.counts.wins, 2);
    assert!(!view.has_dtm && !view.stale);
    assert_eq!(view.summary.map(|s| s.max_pieces), Some(5));
}

#[tokio::test]
async fn a_new_position_asks_again_and_the_same_one_does_not() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    assert_eq!(rig.probes(), 1);
    // The same board again (a harmless message): no new query.
    rig.send(TablebaseMessage::SetOpen(true)).await;
    assert_eq!(rig.probes(), 1);
    // A move changes the position.
    let after = pos(KQ).after(pos(KQ).parse_uci("b1d1").unwrap()).unwrap();
    rig.local
        .answer(&after, Ok(win_report(&after, Source::Local)));
    rig.play("b1d1").await;
    assert_eq!(rig.probes(), 2);
    assert_eq!(rig.view().status, TablebaseStatus::Ready);
}

#[tokio::test]
async fn a_closed_panel_asks_nothing_and_a_reopened_one_asks_once() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    rig.send(TablebaseMessage::SetOpen(false)).await;
    let after = pos(KQ).after(pos(KQ).parse_uci("b1d1").unwrap()).unwrap();
    rig.local
        .answer(&after, Ok(win_report(&after, Source::Local)));
    rig.play("b1d1").await;
    assert_eq!(rig.probes(), 1);
    rig.send(TablebaseMessage::Toggle).await;
    assert_eq!(rig.probes(), 2);
}

#[tokio::test]
async fn an_answer_for_a_position_left_behind_is_discarded() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    // Hold the next answer: take the effect without running it.
    let after = pos(KQ).after(pos(KQ).parse_uci("b1d1").unwrap()).unwrap();
    rig.local
        .answer(&after, Ok(win_report(&after, Source::Local)));
    let mv = rig.state.session.position().parse_uci("b1d1").unwrap();
    let effects = update(
        &mut rig.state,
        Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    );
    let probe = effects
        .iter()
        .find_map(|e| match e {
            Effect::Tablebase(t @ TablebaseEffect::ProbeLocal { .. }) => Some(t.clone()),
            _ => None,
        })
        .expect("a probe for the new position");
    // The user moves on before the answer.
    let back = update(&mut rig.state, Message::Game(GameMessage::Undo));
    assert!(
        back.iter()
            .all(|e| !matches!(e, Effect::Tablebase(TablebaseEffect::ProbeLocal { .. }))),
        "one query at a time"
    );
    let answer = perform(Effect::Tablebase(probe)).await;
    let follow = update(&mut rig.state, answer);
    // The stale answer is not shown; the position now on the board is asked
    // again (it was resolved before: no new query is needed).
    let view = rig.view();
    assert!(view.rows.iter().any(|r| r.san == "Qd1"));
    assert!(!view.stale, "{view:?}");
    drop(follow);
}

#[tokio::test]
async fn the_watchdog_abandons_a_query_and_the_late_answer_is_ignored() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    // A position the stub does not know would answer MissingTable; instead
    // take a probe effect and never answer it.
    let after = pos(KQ).after(pos(KQ).parse_uci("b1d1").unwrap()).unwrap();
    let mv = rig.state.session.position().parse_uci("b1d1").unwrap();
    let effects = update(
        &mut rig.state,
        Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    );
    let mut probe = None;
    let mut dog = None;
    for effect in effects {
        match effect {
            Effect::Tablebase(t @ TablebaseEffect::ProbeLocal { .. }) => probe = Some(t),
            Effect::Tablebase(TablebaseEffect::Delay {
                request,
                kind: DelayKind::Watchdog,
                ..
            }) => dog = Some(request),
            _ => {}
        }
    }
    let probe = probe.unwrap();
    let dog = dog.unwrap();
    rig.local
        .answer(&after, Ok(win_report(&after, Source::Local)));
    drop(update(
        &mut rig.state,
        Message::Tablebase(TablebaseMessage::DelayElapsed {
            request: dog,
            kind: DelayKind::Watchdog,
        }),
    ));
    let view = rig.view();
    assert!(view.issue.is_some(), "{view:?}");
    assert_eq!(
        view.issue.unwrap().message_key(),
        "tablebase-issue-timed-out"
    );
    // The late answer changes nothing.
    let late = perform(Effect::Tablebase(probe)).await;
    drop(update(&mut rig.state, late));
    assert_eq!(
        rig.view().issue.unwrap().message_key(),
        "tablebase-issue-timed-out"
    );
}

#[tokio::test]
async fn a_position_the_local_tables_do_not_cover_goes_online_after_a_wait() {
    let mut rig = Rig::new();
    let six = pos("8/8/8/8/3k4/8/2PPP3/KQ6 w - - 0 1");
    rig.state
        .session
        .replace_game(Game::from_position(six.clone()));
    rig.online
        .answer(&six, Ok(win_report(&six, Source::Online)));
    rig.send(TablebaseMessage::AddDirectory(PathBuf::from("syzygy")))
        .await;
    rig.send(TablebaseMessage::SetOnline(true)).await;
    rig.send(TablebaseMessage::SetOpen(true)).await;
    // The local tables (5 pieces) cannot answer a 6-piece position, so the
    // online one is asked, but only after the position stays still.
    assert_eq!(rig.online_probes(), 0);
    assert!(rig.delays.iter().any(|(_, k)| *k == DelayKind::Debounce));
    assert!(rig.fire(DelayKind::Debounce).await);
    assert_eq!(rig.online_probes(), 1);
    let view = rig.view();
    assert_eq!(view.source, Some(Source::Online));
    assert_eq!(view.status, TablebaseStatus::Ready);
    // The local stub was not troubled with the six pieces.
    assert_eq!(rig.local.calls(), 0);
}

#[tokio::test]
async fn the_wait_starts_over_when_the_position_changes() {
    let mut rig = Rig::new();
    rig.send(TablebaseMessage::SetOnline(true)).await;
    rig.send(TablebaseMessage::SetOpen(true)).await;
    let first = rig.delays.clone();
    assert_eq!(first.len(), 1);
    let after = pos(KQ).after(pos(KQ).parse_uci("b1d1").unwrap()).unwrap();
    rig.online
        .answer(&after, Ok(win_report(&after, Source::Online)));
    rig.play("b1d1").await;
    // The first wait is dead; a second one began.
    assert_eq!(rig.delays.len(), 2);
    let (old, _) = rig.delays[0];
    rig.delays.remove(0);
    rig.send(TablebaseMessage::DelayElapsed {
        request: old,
        kind: DelayKind::Debounce,
    })
    .await;
    assert_eq!(rig.online_probes(), 0, "the dead wait asks nothing");
    assert!(rig.fire(DelayKind::Debounce).await);
    assert_eq!(rig.online_probes(), 1);
    assert_eq!(rig.view().status, TablebaseStatus::Ready);
}

#[tokio::test]
async fn a_failure_is_a_warning_line_and_the_old_list_stays_dimmed() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    rig.local.answer(
        &pos("8/8/8/8/8/2k5/8/K1Q5 b - - 1 1"),
        Err(TablebaseError::Corrupt {
            material: "KQvK".to_owned(),
        }),
    );
    rig.play("b1c1").await;
    let view = rig.view();
    assert_eq!(view.status, TablebaseStatus::Stale);
    assert!(view.stale);
    assert!(!view.rows.is_empty(), "the previous list stays");
    assert_eq!(view.issue.unwrap().message_key(), "tablebase-issue-corrupt");
}

#[tokio::test]
async fn an_online_game_blocks_the_panel_the_training_and_the_queries() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    assert_eq!(rig.view().status, TablebaseStatus::Ready);
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    assert!(rig.state.assist_locked());
    let probes = rig.probes();
    // The panel closes and reopens: nothing is asked, the notice appears.
    rig.send(TablebaseMessage::SetOpen(false)).await;
    rig.send(TablebaseMessage::Toggle).await;
    assert!(matches!(rig.state.notice, Some(AppNotice::Online(_))));
    assert_eq!(rig.view().status, TablebaseStatus::Locked);
    rig.send(TablebaseMessage::TrainStart).await;
    assert!(!rig.state.tablebase_training());
    assert!(rig.view().rows.is_empty());
    assert_eq!(rig.probes(), probes);
}

#[test]
fn the_messages_that_show_results_are_assistance() {
    use mychess_app::online::Family;
    for message in [
        Message::Tablebase(TablebaseMessage::Toggle),
        Message::Tablebase(TablebaseMessage::SetOpen(true)),
        Message::Tablebase(TablebaseMessage::PlayRow(0)),
        Message::Tablebase(TablebaseMessage::TrainStart),
        Message::Tablebase(TablebaseMessage::NewPosition {
            material: "KQvK".to_owned(),
            seed: 1,
        }),
        Message::Action(Action::ToggleTablebasePane),
    ] {
        assert_eq!(family_of(&message), Some(Family::Tablebases), "{message:?}");
    }
    for message in [
        Message::Tablebase(TablebaseMessage::SetOpen(false)),
        Message::Tablebase(TablebaseMessage::SetLocal(true)),
        Message::Tablebase(TablebaseMessage::SetOnline(true)),
        Message::Tablebase(TablebaseMessage::TrainStop),
        Message::Tablebase(TablebaseMessage::DismissOutcome),
        Message::Tablebase(TablebaseMessage::Reopen),
    ] {
        assert_eq!(family_of(&message), None, "{message:?}");
    }
}

#[tokio::test]
async fn folders_are_added_once_limited_and_removed() {
    let mut rig = Rig::new();
    rig.send(TablebaseMessage::AddDirectory(PathBuf::from("a")))
        .await;
    rig.send(TablebaseMessage::AddDirectory(PathBuf::from("a")))
        .await;
    rig.send(TablebaseMessage::AddDirectory(PathBuf::from("b")))
        .await;
    let prefs = &rig.state.prefs.tablebase;
    assert!(prefs.local);
    assert_eq!(prefs.directories, ["a", "b"]);
    for i in 0..20 {
        rig.send(TablebaseMessage::AddDirectory(PathBuf::from(format!(
            "d{i}"
        ))))
        .await;
    }
    assert_eq!(
        rig.state.prefs.tablebase.directories.len(),
        mychess_app::tablebase::MAX_DIRECTORIES
    );
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Tablebase(TablebaseNotice::TooManyFolders))
    );
    rig.send(TablebaseMessage::RemoveDirectory(0)).await;
    assert_eq!(rig.state.prefs.tablebase.directories[0], "b");
    rig.send(TablebaseMessage::RemoveDirectory(99)).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_folder_that_is_not_text_is_not_kept() {
    use std::os::unix::ffi::OsStringExt;
    let mut rig = Rig::new();
    let path = PathBuf::from(std::ffi::OsString::from_vec(vec![b'x', 0xFF, b'y']));
    rig.send(TablebaseMessage::AddDirectory(path)).await;
    assert!(rig.state.prefs.tablebase.directories.is_empty());
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Tablebase(TablebaseNotice::FolderNotSaved))
    );
}

// ---- training ----

/// The tablebase to move wins here (so the user, after the move, lost the win).
fn after_user_move_report(position: &Position) -> mychess_tablebase::PositionReport {
    report(
        position,
        Source::Local,
        Evaluation::of(Category::Win),
        &[
            ("c3d3", Category::Win, Some(5)),
            ("c3b3", Category::Draw, Some(0)),
        ],
    )
}

#[tokio::test]
async fn the_training_plays_the_move_of_the_tablebase_and_counts_a_slip() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    rig.send(TablebaseMessage::TrainStart).await;
    assert!(rig.state.tablebase_training());
    let view = rig.view();
    let training = view.training.unwrap();
    assert!(!training.tablebase_to_move);
    assert_eq!(training.start, Some(mychess_app::tablebase::Grade::Win));
    assert!(view.rows.is_empty(), "the results are hidden (D2574)");
    assert!(view.position.is_none(), "the summary is hidden too");
    assert_eq!(view.counts, mychess_tablebase::Counts::default());
    // The user plays a move that throws the win away: black to move wins.
    let after = pos(KQ).after(pos(KQ).parse_uci("b1b2").unwrap()).unwrap();
    rig.local.answer(&after, Ok(after_user_move_report(&after)));
    rig.play("b1b2").await;
    let training = rig.view().training.unwrap();
    assert!(training.tablebase_to_move);
    assert_eq!((training.user_moves, training.slips), (1, 1));
    // The reply waits its delay, then the best move is played.
    let reply = pos(KQ)
        .after(pos(KQ).parse_uci("b1b2").unwrap())
        .unwrap()
        .parse_uci("c3d3")
        .unwrap();
    let played_before = rig.state.session.position().fullmove_number();
    assert!(rig.fire(DelayKind::Reply).await);
    let position = rig.state.session.position();
    assert_eq!(position.turn(), mychess_core::Color::White);
    assert!(position.piece_at(reply.to().unwrap()).is_some());
    assert!(position.fullmove_number() > played_before);
}

#[tokio::test]
async fn the_training_ends_with_a_verdict_when_the_game_is_over() {
    let mut rig = Rig::new();
    let almost = pos("k7/8/1K6/8/8/8/7Q/8 w - - 0 1");
    rig.state
        .session
        .replace_game(Game::from_position(almost.clone()));
    rig.local.answer(
        &almost,
        Ok(report(
            &almost,
            Source::Local,
            Evaluation::of(Category::Win),
            &[("h2h8", Category::Win, Some(1))],
        )),
    );
    rig.send(TablebaseMessage::AddDirectory(PathBuf::from("s")))
        .await;
    rig.send(TablebaseMessage::TrainStart).await;
    assert!(rig.state.tablebase_training());
    // The mate.
    rig.play("h2h8").await;
    assert!(!rig.state.tablebase_training());
    let outcome = rig.view().outcome.expect("the result");
    assert_eq!(outcome.verdict, Verdict::Converted);
    assert_eq!(outcome.user_moves, 0);
    rig.send(TablebaseMessage::DismissOutcome).await;
    assert!(rig.view().outcome.is_none());
}

#[tokio::test]
async fn the_training_stops_when_the_user_leaves_the_end_of_the_line() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    let after = pos(KQ).after(pos(KQ).parse_uci("b1d1").unwrap()).unwrap();
    rig.local
        .answer(&after, Ok(win_report(&after, Source::Local)));
    rig.play("b1d1").await;
    rig.send(TablebaseMessage::TrainStart).await;
    assert!(rig.state.tablebase_training());
    rig.send(GameMessage::Navigate(mychess_app::Nav::Start))
        .await;
    assert!(!rig.state.tablebase_training());
    assert!(rig.view().outcome.is_none());
    // And it refuses to start where the line goes on.
    rig.send(TablebaseMessage::TrainStart).await;
    assert!(!rig.state.tablebase_training());
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Tablebase(TablebaseNotice::NotAtEndOfLine))
    );
}

#[tokio::test]
async fn a_new_position_replaces_the_game_and_starts_the_training() {
    let mut rig = Rig::new();
    let wanted = mychess_tablebase::generate::random_position("KRvKP", 7).unwrap();
    rig.online
        .answer(&wanted, Ok(win_report(&wanted, Source::Online)));
    rig.send(TablebaseMessage::SetOnline(true)).await;
    rig.send(TablebaseMessage::NewPosition {
        material: "KRvKP".to_owned(),
        seed: 7,
    })
    .await;
    assert_eq!(material_name(rig.state.session.position()), "KRvKP");
    assert!(rig.state.tablebase_training());
    assert!(rig.view().open);
    // The same seed gives the same position.
    let fen = rig.state.session.position().fen();
    rig.send(TablebaseMessage::TrainStop).await;
    rig.send(TablebaseMessage::NewPosition {
        material: "KRvKP".to_owned(),
        seed: 7,
    })
    .await;
    assert_eq!(rig.state.session.position().fen(), fen);
    // A text that is not a material changes nothing.
    rig.send(TablebaseMessage::NewPosition {
        material: "nonsense".to_owned(),
        seed: 7,
    })
    .await;
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Tablebase(TablebaseNotice::BadMaterial))
    );
    assert_eq!(rig.state.session.position().fen(), fen);
}

#[tokio::test]
async fn the_training_waits_for_the_tables_not_for_the_user() {
    // Without any source the training is refused.
    let mut rig = Rig::new();
    rig.send(TablebaseMessage::TrainStart).await;
    assert!(!rig.state.tablebase_training());
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Tablebase(TablebaseNotice::NothingToTrain))
    );
}

#[tokio::test]
async fn clicking_a_row_plays_its_move_but_not_during_the_training() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    let after = pos(KQ).after(pos(KQ).parse_uci("b1d1").unwrap()).unwrap();
    rig.local
        .answer(&after, Ok(win_report(&after, Source::Local)));
    // Row 0 is Qd1.
    rig.send(TablebaseMessage::PlayRow(0)).await;
    assert_eq!(rig.state.session.position().fen(), after.fen());
    // A row that is not there is ignored.
    rig.send(TablebaseMessage::PlayRow(999)).await;
    assert_eq!(rig.state.session.position().fen(), after.fen());
}

#[tokio::test]
async fn reopening_the_panel_tries_a_failed_query_again() {
    let mut rig = Rig::new();
    rig.send(TablebaseMessage::SetOnline(true)).await;
    rig.online
        .answer(&pos(KQ), Err(TablebaseError::Network("down".to_owned())));
    rig.send(TablebaseMessage::SetOpen(true)).await;
    assert!(rig.fire(DelayKind::Debounce).await);
    assert_eq!(rig.online_probes(), 1);
    assert_eq!(
        rig.view().issue.unwrap().message_key(),
        "tablebase-issue-network"
    );
    // The same position is not asked again while the panel stays open...
    rig.send(TablebaseMessage::SetOpen(true)).await;
    assert!(!rig.delays.iter().any(|(_, k)| *k == DelayKind::Debounce));
    // ...but closing and opening it tries anew.
    rig.online
        .answer(&pos(KQ), Ok(win_report(&pos(KQ), Source::Online)));
    rig.send(TablebaseMessage::SetOpen(false)).await;
    rig.send(TablebaseMessage::SetOpen(true)).await;
    assert!(rig.fire(DelayKind::Debounce).await);
    assert_eq!(rig.online_probes(), 2);
    assert_eq!(rig.view().status, TablebaseStatus::Ready);
}

/// The probe effects among `effects`.
fn probes_in(effects: &[Effect]) -> Vec<TablebaseEffect> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Tablebase(t @ TablebaseEffect::ProbeLocal { .. }) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

// ---- the review round ----

#[tokio::test]
async fn a_new_position_with_an_unrelated_question_open_does_not_arm_the_training() {
    let mut rig = Rig::new();
    rig.send(TablebaseMessage::SetOnline(true)).await;
    // The game is altered, and "new game" asks what to do with the changes.
    rig.play("b1d1").await;
    rig.send(Message::Command(Command::NewGame)).await;
    assert!(rig.state.discard_prompt.is_some());
    // The question is about another replacement: this one must not start a
    // training behind the user's back.
    rig.send(TablebaseMessage::NewPosition {
        material: "KRvKP".to_owned(),
        seed: 7,
    })
    .await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    assert!(!rig.state.tablebase_training());
}

#[tokio::test]
async fn the_old_training_stays_until_the_replacement_happens() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    let after = pos(KQ).after(pos(KQ).parse_uci("b1d1").unwrap()).unwrap();
    rig.local.answer(&after, Ok(after_user_move_report(&after)));
    rig.send(TablebaseMessage::TrainStart).await;
    rig.play("b1d1").await; // alters the game
    assert!(rig.state.tablebase_training());
    let wanted = mychess_tablebase::generate::random_position("KRvKP", 7).unwrap();
    rig.local
        .answer(&wanted, Ok(win_report(&wanted, Source::Local)));
    rig.send(TablebaseMessage::NewPosition {
        material: "KRvKP".to_owned(),
        seed: 7,
    })
    .await;
    // The question is open: nothing was thrown away yet.
    assert!(rig.state.discard_prompt.is_some());
    assert!(rig.state.tablebase_training());
    assert_eq!(material_name(rig.state.session.position()), "KQvK");
    // Cancelling keeps everything.
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel))
        .await;
    assert!(rig.state.tablebase_training());
    assert_eq!(material_name(rig.state.session.position()), "KQvK");
    // Saying yes replaces the game and starts the new training.
    rig.send(TablebaseMessage::NewPosition {
        material: "KRvKP".to_owned(),
        seed: 7,
    })
    .await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    assert_eq!(material_name(rig.state.session.position()), "KRvKP");
    assert!(rig.state.tablebase_training());
    assert_eq!(rig.view().training.unwrap().user_moves, 0);
}

#[tokio::test]
async fn a_new_position_needs_a_source_before_it_replaces_the_game() {
    let mut rig = Rig::new();
    let fen = rig.state.session.position().fen();
    rig.send(TablebaseMessage::NewPosition {
        material: "KRvKP".to_owned(),
        seed: 7,
    })
    .await;
    assert_eq!(rig.state.session.position().fen(), fen);
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Tablebase(TablebaseNotice::NothingToTrain))
    );
}

#[tokio::test]
async fn a_move_before_the_first_answer_ends_the_training_without_a_verdict() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    // A position whose answer has not come: the training does not know what
    // it is worth.
    let other = pos("8/8/8/8/8/2k5/8/KR6 w - - 0 1");
    rig.state
        .session
        .replace_game(Game::from_position(other.clone()));
    drop(update(
        &mut rig.state,
        Message::Tablebase(TablebaseMessage::TrainStart),
    ));
    assert!(rig.state.tablebase_training());
    assert_eq!(rig.view().training.unwrap().start, None);
    // The user moves; the answer for the position after his move arrives.
    let mv = other.parse_uci("b1b2").unwrap();
    let after = other.after(mv).unwrap();
    rig.local.answer(&after, Ok(after_user_move_report(&after)));
    rig.play("b1b2").await;
    assert!(!rig.state.tablebase_training(), "nothing to compare with");
    let outcome = rig.view().outcome.expect("a result is shown");
    assert_eq!(outcome.verdict, Verdict::Inconclusive);
    assert!(!rig.delays.iter().any(|(_, k)| *k == DelayKind::Reply));
}

#[tokio::test]
async fn a_query_in_flight_for_a_position_left_behind_does_not_block_the_new_one() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    let first = pos(KQ).after(pos(KQ).parse_uci("b1d1").unwrap()).unwrap();
    rig.local
        .answer(&first, Ok(win_report(&first, Source::Local)));
    let mv = rig.state.session.position().parse_uci("b1d1").unwrap();
    let sent = update(
        &mut rig.state,
        Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    );
    assert_eq!(
        probes_in(&sent).len(),
        1,
        "the query for the first position"
    );
    // It is never answered; the user goes back and plays another move.
    drop(update(&mut rig.state, Message::Game(GameMessage::Undo)));
    let mv = rig.state.session.position().parse_uci("b1e4").unwrap();
    let second = pos(KQ).after(mv).unwrap();
    rig.local
        .answer(&second, Ok(win_report(&second, Source::Local)));
    let sent = update(
        &mut rig.state,
        Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    );
    let probes = probes_in(&sent);
    assert_eq!(
        probes.len(),
        1,
        "the new position is asked at once: {sent:?}"
    );
    let TablebaseEffect::ProbeLocal { position, .. } = &probes[0] else {
        unreachable!()
    };
    assert_eq!(position.fen(), second.fen());
}

#[tokio::test]
async fn a_closed_panel_lets_go_of_the_table_files_and_reopening_reads_them_again() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    // The set is held by the state (and the stub we keep). The log of the
    // effects holds handles too: forget it.
    rig.seen.clear();
    assert!(Arc::strong_count(&rig.local) > 1);
    rig.send(TablebaseMessage::SetOpen(false)).await;
    rig.seen.clear();
    assert_eq!(Arc::strong_count(&rig.local), 1, "no handle kept");
    assert!(rig.view().summary.is_none());
    rig.send(TablebaseMessage::SetOpen(true)).await;
    let opens = rig
        .seen
        .iter()
        .filter(|e| matches!(e, TablebaseEffect::Open { .. }))
        .count();
    assert_eq!(opens, 1, "the folders are read again");
    assert_eq!(rig.view().status, TablebaseStatus::Ready);
}

#[tokio::test]
async fn the_training_keeps_the_tables_open_while_the_panel_is_closed() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    rig.send(TablebaseMessage::TrainStart).await;
    rig.send(TablebaseMessage::SetOpen(false)).await;
    rig.seen.clear();
    assert!(rig.state.tablebase_training());
    assert!(Arc::strong_count(&rig.local) > 1);
    rig.send(TablebaseMessage::TrainStop).await;
    // Nothing wants the tables now.
    rig.send(TablebaseMessage::SetOpen(false)).await;
    rig.seen.clear();
    assert_eq!(Arc::strong_count(&rig.local), 1);
}

#[tokio::test]
async fn the_training_does_not_play_into_a_game_something_else_holds() {
    let mut rig = Rig::new();
    rig.local_panel().await;
    rig.send(TablebaseMessage::TrainStart).await;
    let after = pos(KQ).after(pos(KQ).parse_uci("b1b2").unwrap()).unwrap();
    rig.local.answer(&after, Ok(after_user_move_report(&after)));
    rig.play("b1b2").await;
    assert!(rig.state.tablebase_training());
    let fen = rig.state.session.position().fen();
    // A question about the changes is on the screen when the delay ends.
    rig.send(Message::Command(Command::NewGame)).await;
    assert!(rig.state.discard_prompt.is_some());
    assert!(rig.fire(DelayKind::Reply).await);
    assert_eq!(
        rig.state.session.position().fen(),
        fen,
        "no move was played"
    );
    assert!(!rig.state.tablebase_training());
}

#[tokio::test]
async fn the_view_tells_the_folders_that_need_renaming() {
    use mychess_tablebase::{DirReport, DirStatus};
    let mut rig = Rig::new();
    rig.open_report = OpenReport {
        directories: vec![DirReport {
            path: PathBuf::from("syzygy"),
            status: DirStatus::Read,
            wdl_files: 1,
            dtz_files: 0,
            max_pieces: 3,
            uppercase_files: 3,
            issues: Vec::new(),
        }],
        wdl_files: 1,
        dtz_files: 0,
        max_pieces: 3,
    };
    rig.send(TablebaseMessage::AddDirectory(PathBuf::from("syzygy")))
        .await;
    rig.send(TablebaseMessage::SetOpen(true)).await;
    assert_eq!(rig.view().directories[0].uppercase_files, 3);
}

#[tokio::test]
async fn the_watchdog_of_the_open_gives_up_and_the_late_answer_is_ignored() {
    let mut rig = Rig::new();
    // `Open` is asked and never answered.
    let mut held = Vec::new();
    for message in [
        Message::Tablebase(TablebaseMessage::AddDirectory(PathBuf::from("syzygy"))),
        Message::Tablebase(TablebaseMessage::SetOpen(true)),
    ] {
        held.extend(update(&mut rig.state, message));
    }
    let mut open = None;
    let mut dog = None;
    for effect in held {
        match effect {
            Effect::Tablebase(TablebaseEffect::Open { request, .. }) => open = Some(request),
            Effect::Tablebase(TablebaseEffect::Delay {
                request,
                kind: DelayKind::OpenWatchdog,
                ..
            }) => dog = Some(request),
            _ => {}
        }
    }
    let (open, dog) = (open.unwrap(), dog.unwrap());
    assert_eq!(open, dog, "the watchdog is for the open in flight");
    assert_eq!(rig.view().status, TablebaseStatus::Loading);
    drop(update(
        &mut rig.state,
        Message::Tablebase(TablebaseMessage::DelayElapsed {
            request: dog,
            kind: DelayKind::OpenWatchdog,
        }),
    ));
    let view = rig.view();
    assert_ne!(view.status, TablebaseStatus::Loading);
    assert_eq!(
        view.issue.unwrap().message_key(),
        "tablebase-issue-timed-out"
    );
    // The late answer changes nothing.
    drop(update(
        &mut rig.state,
        Message::Tablebase(TablebaseMessage::Opened {
            request: open,
            result: Box::new(Ok(OpenedSet {
                handle: LocalHandle::new(rig.local.clone()),
                report: rig.open_report.clone(),
            })),
        }),
    ));
    assert!(rig.view().summary.is_none());
}

#[tokio::test]
async fn turning_a_source_off_drops_its_query_in_flight_and_its_report() {
    let mut rig = Rig::new();
    rig.send(TablebaseMessage::SetOnline(true)).await;
    rig.send(TablebaseMessage::SetOpen(true)).await;
    rig.online
        .answer(&pos(KQ), Ok(win_report(&pos(KQ), Source::Online)));
    // The wait ends and the probe goes out, but its answer is held.
    let (request, kind) = rig.delays.remove(0);
    let effects = update(
        &mut rig.state,
        Message::Tablebase(TablebaseMessage::DelayElapsed { request, kind }),
    );
    let probe = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::Tablebase(t @ TablebaseEffect::ProbeOnline { .. }) => Some(t),
            _ => None,
        })
        .unwrap();
    rig.send(TablebaseMessage::SetOnline(false)).await;
    let late = perform(Effect::Tablebase(probe)).await;
    drop(update(&mut rig.state, late));
    let view = rig.view();
    assert_eq!(view.source, None, "{view:?}");
    assert!(view.rows.is_empty());
    assert_eq!(view.status, TablebaseStatus::NotConfigured);
}

#[tokio::test]
async fn a_late_online_answer_after_the_assistance_lock_shows_nothing() {
    let mut rig = Rig::new();
    rig.send(TablebaseMessage::SetOnline(true)).await;
    rig.send(TablebaseMessage::SetOpen(true)).await;
    rig.online
        .answer(&pos(KQ), Ok(win_report(&pos(KQ), Source::Online)));
    let (request, kind) = rig.delays.remove(0);
    let effects = update(
        &mut rig.state,
        Message::Tablebase(TablebaseMessage::DelayElapsed { request, kind }),
    );
    let probe = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::Tablebase(t @ TablebaseEffect::ProbeOnline { .. }) => Some(t),
            _ => None,
        })
        .unwrap();
    // An online game starts while the probe is out.
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    rig.send(TablebaseMessage::SetOpen(true)).await;
    let late = perform(Effect::Tablebase(probe)).await;
    drop(update(&mut rig.state, late));
    let view = rig.view();
    assert_eq!(view.status, TablebaseStatus::Locked);
    assert_eq!(view.source, None);
    assert!(view.rows.is_empty());
}
