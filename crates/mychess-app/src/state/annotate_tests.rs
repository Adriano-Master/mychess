//! Tests of the annotation flow: a fake engine answers the searches of the
//! run through synthetic events fed to `update` (spec M4-6, phase A).
//!
//! The fake evaluates by material with a two-ply search, so a hanging queen
//! is a real blunder in its eyes.

use mychess_core::{Game, Move, NodeId, Position};
use mychess_engine::{
    BestMove, Bound, Capabilities, EngineCommand, EngineEvent, EngineIdentity, EngineOption,
    OptionKind, OptionValue, Protocol, Score, SearchId, SearchInfo, SearchRequest,
};

use super::{AppState, update};
use crate::annotate::{
    AnnotateMessage, AnnotateNotice, AnnotateOptions, Control, Outcome, ScoreFormat, Side, WithVars,
};
use crate::effect::Effect;
use crate::engines::{EngineDraft, EngineMessage, EngineRunState, Hotkey, RunId};
use crate::flow::AppNotice;
use crate::message::Message;
use crate::session::{GameMessage, Nav};

// ---- the fake engine ----

/// What the fake says about a position.
pub(super) struct Reply {
    pub(super) cp: i32,
    pub(super) pv: Vec<Move>,
    pub(super) second: Option<i32>,
    /// A mate score (White's point of view) instead of `cp`.
    pub(super) mate: Option<i32>,
}

fn material(pos: &Position) -> i32 {
    let fen = pos.fen();
    let board = fen.split(' ').next().unwrap_or_default();
    board
        .chars()
        .map(|c| {
            let v = match c.to_ascii_lowercase() {
                'p' => 1,
                'n' | 'b' => 3,
                'r' => 5,
                'q' => 9,
                _ => 0,
            };
            if c.is_ascii_uppercase() { v } else { -v }
        })
        .sum::<i32>()
        * 100
}

/// The best move for the side to move and the best reply, by material at
/// two plies; the score is from White's point of view.
pub(super) fn material_reply(pos: &Position) -> Reply {
    let white = pos.turn() == mychess_core::Color::White;
    let better = |a: i32, b: i32| if white { a > b } else { a < b };
    let mut best: Option<(i32, Vec<Move>)> = None;
    for m in pos.legal_moves() {
        let mut after = pos.clone();
        after.play(m).expect("legal");
        let mut value = material(&after);
        let mut line = vec![m];
        let mut reply_best: Option<i32> = None;
        for r in after.legal_moves() {
            let mut then = after.clone();
            then.play(r).expect("legal");
            let v = material(&then);
            // The reply minimises for the mover (it is the other side).
            let reply_better = if white {
                v < reply_best.unwrap_or(i32::MAX)
            } else {
                v > reply_best.unwrap_or(i32::MIN)
            };
            if reply_best.is_none() || reply_better {
                reply_best = Some(v);
                line.truncate(1);
                line.push(r);
            }
        }
        if let Some(v) = reply_best {
            value = v;
        }
        if best.as_ref().is_none_or(|(b, _)| better(value, *b)) {
            best = Some((value, line));
        }
    }
    let (cp, pv) = best.unwrap_or((0, Vec::new()));
    Reply {
        cp,
        pv,
        second: None,
        mate: None,
    }
}

pub(super) fn position_of(request: &SearchRequest) -> Position {
    let mut pos = request.position.start.clone();
    for m in &request.position.moves {
        pos.play(*m).expect("the run sends legal moves");
    }
    pos
}

fn identity(extra: Vec<EngineOption>, multipv: bool) -> EngineIdentity {
    EngineIdentity {
        name: "Fake 1".to_owned(),
        author: None,
        options: extra,
        capabilities: Capabilities {
            multipv: multipv.then_some(1..=5),
            search_moves: true,
            ..Capabilities::default()
        },
    }
}

fn setup_with(game: Game, extra: Vec<EngineOption>, draft: EngineDraft) -> (AppState, RunId) {
    let mut state = AppState::default();
    let id = state.engines.add(draft).expect("add");
    let run = state.engine_runs.begin(id, true);
    let r = state.engine_runs.get_mut(run).expect("run");
    r.state = EngineRunState::Running;
    r.identity = Some(Box::new(identity(extra, true)));
    state.session.replace_game(game);
    (state, run)
}

pub(super) fn setup(game: Game) -> (AppState, RunId) {
    setup_with(game, Vec::new(), EngineDraft::new("Fake", "/fake"))
}

/// A game from the standard position with the moves `ucis`.
pub(super) fn game_of(ucis: &[&str]) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    for uci in ucis {
        let mv = game
            .position_at(node)
            .expect("position")
            .parse_uci(uci)
            .expect("uci");
        node = game.add_move(node, mv).expect("legal");
    }
    game
}

/// 20 plies; 3.Qxe5 hangs the queen.
const HANGING_QUEEN: [&str; 20] = [
    "e2e4", "e7e5", "d1h5", "b8c6", "h5e5", "c6e5", "g1f3", "e5f3", "g2f3", "d7d6", "f1c4", "g8f6",
    "d2d3", "f8e7", "c1e3", "e8g8", "b1c3", "c7c6", "e1c1", "b7b5",
];

pub(super) fn send(state: &mut AppState, message: impl Into<Message>) -> Vec<Effect> {
    update(state, message.into())
}

pub(super) fn event(state: &mut AppState, run: RunId, event: EngineEvent) -> Vec<Effect> {
    send(
        state,
        EngineMessage::Event {
            run,
            event: Box::new(event),
        },
    )
}

pub(super) fn commands(effects: &[Effect]) -> Vec<EngineCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand { command, .. } => Some((**command).clone()),
            _ => None,
        })
        .collect()
}

pub(super) fn last_search(effects: &[Effect]) -> Option<SearchRequest> {
    commands(effects).into_iter().rev().find_map(|c| match c {
        EngineCommand::Search(request) => Some(request),
        _ => None,
    })
}

/// Answers the search of `effects` the way the engine would: label,
/// start, one report and the best move. Returns the effects of the best
/// move and the request answered.
pub(super) fn answer(
    state: &mut AppState,
    run: RunId,
    effects: &[Effect],
    id: u64,
    reply: &Reply,
) -> Option<(Vec<Effect>, SearchRequest)> {
    let request = last_search(effects)?;
    let search = SearchId(id);
    event(
        state,
        run,
        EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        },
    );
    event(state, run, EngineEvent::SearchStarted(search));
    let info = |multipv: u8, cp: Score, pv: Vec<Move>| SearchInfo {
        search,
        multipv,
        depth: Some(13),
        score: Some((cp, Bound::Exact)),
        pv,
        ..SearchInfo::default()
    };
    event(
        state,
        run,
        EngineEvent::Info(info(
            1,
            reply.mate.map_or(Score::Cp(reply.cp), Score::Mate),
            reply.pv.clone(),
        )),
    );
    if let Some(second) = reply.second {
        event(
            state,
            run,
            EngineEvent::Info(info(2, Score::Cp(second), Vec::new())),
        );
    }
    let after = event(
        state,
        run,
        EngineEvent::BestMove(BestMove {
            search,
            mv: reply.pv.first().copied(),
            ponder: None,
        }),
    );
    Some((after, request))
}

/// Everything a drive saw.
#[derive(Default)]
pub(super) struct Log {
    pub(super) requests: Vec<SearchRequest>,
    pub(super) commands: Vec<EngineCommand>,
    /// The effects after the last answer.
    pub(super) last: Vec<Effect>,
}

/// Answers up to `max` searches with `engine`, starting from `effects`.
pub(super) fn drive(
    state: &mut AppState,
    run: RunId,
    mut effects: Vec<Effect>,
    max: usize,
    engine: &mut dyn FnMut(&SearchRequest, &Position) -> Reply,
) -> Log {
    let mut log = Log::default();
    log.commands.extend(commands(&effects));
    for n in 0..max {
        // Once the run ends, the board's analysis is the only search left.
        if !state.annotation_running() {
            break;
        }
        let Some(request) = last_search(&effects) else {
            break;
        };
        let reply = engine(&request, &position_of(&request));
        let id = u64::try_from(n).expect("small") + 100;
        let Some((after, request)) = answer(state, run, &effects, id, &reply) else {
            break;
        };
        log.requests.push(request);
        log.commands.extend(commands(&after));
        effects = after;
    }
    log.last = effects;
    log
}

pub(super) fn material_engine(_: &SearchRequest, pos: &Position) -> Reply {
    material_reply(pos)
}

fn annotate_notice(state: &AppState) -> Option<AnnotateNotice> {
    match &state.notice {
        Some(AppNotice::Annotate(n)) => Some(*n),
        _ => None,
    }
}

fn start(state: &mut AppState) -> Vec<Effect> {
    send(state, AnnotateMessage::Start)
}

fn nth_main(game: &Game, ply: usize) -> NodeId {
    game.mainline().nth(ply - 1).expect("ply")
}

// ---- the tests ----

#[test]
fn annotates_a_whole_game_with_one_search_per_position_and_marks_the_blunder() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    let before = state.session.game().clone();
    let depth = state.session.undo_depth();
    let cursor = state.session.cursor();
    let effects = start(&mut state);
    assert!(state.annotation_running());
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert!(!state.annotation_running(), "the run ended");
    // The start position and every ply: 21 searches, each position once.
    assert_eq!(log.requests.len(), 21);
    let mut positions: Vec<String> = log.requests.iter().map(|r| position_of(r).fen()).collect();
    positions.sort();
    positions.dedup();
    assert_eq!(positions.len(), 21);
    let report = state.annotate_report.expect("report");
    assert_eq!(report.outcome, Outcome::Finished);
    assert_eq!((report.plies, report.total), (20, 20));
    assert!(report.variations >= 1);

    let game = state.session.game();
    // The queen move got `?` or `??`, the diagram goes on the move before.
    let blunder = nth_main(game, 5);
    let nags: Vec<u8> = game
        .nags(blunder)
        .expect("node")
        .iter()
        .map(|n| n.value())
        .collect();
    assert!(nags.iter().any(|n| *n == 2 || *n == 4), "mark: {nags:?}");
    let parent = nth_main(game, 4);
    assert!(
        game.nags(parent)
            .expect("node")
            .iter()
            .any(|n| n.value() == 201)
    );
    let text = game.comment(blunder).expect("node").expect("comment");
    assert!(text.contains(" / "), "both scores: {text}");
    assert!(
        game.children(parent).expect("children").len() >= 2,
        "the line"
    );
    // A quiet move got only its score as `[%eval ...]`.
    let quiet = game
        .comment(nth_main(game, 7))
        .expect("node")
        .expect("score");
    assert!(quiet.starts_with("[%eval "), "{quiet}");
    // Tags.
    assert_eq!(game.tags().get_extra("Annotator"), Some("Fake 1"));
    assert_eq!(game.tags().get_extra("Depth"), Some("13"));
    // The cursor did not move, one undo point for the whole run.
    assert_eq!(state.session.cursor(), cursor);
    assert_eq!(state.session.undo_depth(), depth + 1);
    assert!(state.is_dirty());
    send(&mut state, GameMessage::Undo);
    assert!(state.session.game().same_content(&before));
    assert_eq!(
        annotate_notice(&state),
        Some(AnnotateNotice::Finished {
            plies: 20,
            variations: report.variations
        })
    );
}

#[test]
fn the_engine_gets_depth_limits_one_line_and_is_put_back_afterwards() {
    let mut draft = EngineDraft::new("Fake", "/fake");
    draft.multipv = 3;
    let extra = vec![EngineOption {
        name: "OwnBook".to_owned(),
        kind: OptionKind::Check { default: true },
    }];
    let (mut state, run) = setup_with(game_of(&HANGING_QUEEN[..4]), extra, draft);
    let effects = start(&mut state);
    // The board's analysis is off while the run lasts.
    assert!(!state.engine_runs.get(run).expect("run").analyzing);
    let first = commands(&effects);
    assert!(first.contains(&EngineCommand::SetMultiPv(1)));
    assert!(first.contains(&EngineCommand::SetOption {
        name: "OwnBook".to_owned(),
        value: OptionValue::Bool(false),
    }));
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert!(log.requests.iter().all(|r| r.limits.depth == Some(13)));
    assert!(log.requests.iter().all(|r| r.limits.movetime.is_none()));
    assert!(log.commands.contains(&EngineCommand::SetMultiPv(3)));
    assert!(log.commands.contains(&EngineCommand::SetOption {
        name: "OwnBook".to_owned(),
        value: OptionValue::Bool(true),
    }));
    assert!(state.engine_runs.get(run).expect("run").analyzing);
}

#[test]
fn a_time_limit_sends_movetime() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..2]));
    state.annotate_options = AnnotateOptions {
        control: Control::Time,
        movetime: std::time::Duration::from_millis(1500),
        ..AnnotateOptions::default()
    };
    let effects = start(&mut state);
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert_eq!(log.requests.len(), 3);
    assert!(log.requests.iter().all(|r| r.limits.movetime
        == Some(std::time::Duration::from_millis(1500))
        && r.limits.depth.is_none()));
    // No `Depth` tag under a time limit.
    assert_eq!(state.session.game().tags().get_extra("Depth"), None);
}

#[test]
fn an_xboard_engine_is_limited_to_time_and_looks_for_no_mates() {
    let mut draft = EngineDraft::new("Old", "/old");
    draft.protocol = Protocol::XBoard;
    let (mut state, run) = setup_with(game_of(&HANGING_QUEEN[..2]), Vec::new(), draft);
    state.annotate_options.mark_exercises = true;
    assert!(!state.annotate_caps(Some(run)).depth);
    assert!(!state.annotate_caps(Some(run)).exercises);
    let effects = start(&mut state);
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert!(log.requests.iter().all(|r| r.limits.depth.is_none()));
    assert!(log.requests.iter().all(|r| r.limits.movetime.is_some()));
}

#[test]
fn cancelling_keeps_the_edits_and_one_undo_reverts_them() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    let before = state.session.game().clone();
    let depth = state.session.undo_depth();
    let effects = start(&mut state);
    // Answer up to and including the position after 3.Qxe5 (6 searches).
    let log = drive(&mut state, run, effects, 6, &mut material_engine);
    assert!(state.annotation_running());
    assert!(state.annotation_progress().expect("progress").done_plies >= 4);
    // The search in flight has started.
    let request = last_search(&log.last).expect("the search in flight");
    let search = SearchId(900);
    event(
        &mut state,
        run,
        EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        },
    );
    event(&mut state, run, EngineEvent::SearchStarted(search));
    let effects = send(&mut state, AnnotateMessage::Cancel);
    assert!(commands(&effects).contains(&EngineCommand::Stop));
    assert!(state.annotation_running(), "ends with the bestmove");
    // The search in flight ends: the run is over and its result is dropped.
    let effects = event(
        &mut state,
        run,
        EngineEvent::BestMove(BestMove {
            search,
            mv: None,
            ponder: None,
        }),
    );
    // The only search left is the board's analysis, given back to the engine.
    assert!(last_search(&effects).is_some_and(|r| r.tag & 0x4E4E_0000_0000_0000 == 0));
    assert!(!state.annotation_running());
    assert_eq!(
        state.annotate_report.expect("report").outcome,
        Outcome::Cancelled
    );
    assert!(state.is_dirty(), "the edits stay");
    assert!(!state.session.game().same_content(&before));
    assert_eq!(state.session.undo_depth(), depth + 1);
    send(&mut state, GameMessage::Undo);
    assert!(state.session.game().same_content(&before));
    assert!(
        state.engine_runs.get(run).expect("run").analyzing,
        "restored"
    );
}

#[test]
fn a_dying_engine_ends_the_run_and_keeps_the_edits() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    let effects = start(&mut state);
    drive(&mut state, run, effects, 6, &mut material_engine);
    event(
        &mut state,
        run,
        EngineEvent::Exited(mychess_engine::ExitInfo {
            reason: mychess_engine::ExitReason::Crashed,
            status: None,
            stderr_tail: Vec::new(),
        }),
    );
    assert!(!state.annotation_running());
    let report = state.annotate_report.expect("report");
    assert_eq!(
        report.outcome,
        Outcome::Failed(AnnotationError::EngineFailed)
    );
    assert!(report.plies >= 4);
    assert!(!state.session.can_redo());
    // The undo group is closed: a new edit has its own undo point.
    let depth = state.session.undo_depth();
    send(&mut state, GameMessage::Navigate(Nav::Start));
    let root = state.session.game().root();
    send(
        &mut state,
        GameMessage::SetComment {
            node: root,
            text: "x".to_owned(),
        },
    );
    assert_eq!(state.session.undo_depth(), depth + 1);
}

#[test]
fn the_game_cannot_be_edited_while_the_run_goes_and_a_replaced_game_stops_it() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..6]));
    let effects = start(&mut state);
    let before = state.session.changes();
    send(&mut state, GameMessage::Navigate(Nav::Start));
    assert_eq!(annotate_notice(&state), Some(AnnotateNotice::Busy));
    assert_eq!(state.session.changes(), before);
    drive(&mut state, run, effects, 2, &mut material_engine);
    assert!(state.annotation_running());
    state.session.replace_game(Game::new());
    send(&mut state, GameMessage::AutoplayTick { generation: 0 });
    assert!(!state.annotation_running());
    assert_eq!(
        state.annotate_report.expect("report").outcome,
        Outcome::GameChanged
    );
    assert!(state.engine_runs.get(run).expect("run").analyzing);
}

#[test]
fn an_edit_by_someone_else_stops_the_run() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..6]));
    let effects = start(&mut state);
    drive(&mut state, run, effects, 2, &mut material_engine);
    let root = state.session.game().root();
    // A path that does not go through `Message::Game` (a command).
    state.session.update(GameMessage::SetComment {
        node: root,
        text: "someone".to_owned(),
    });
    send(&mut state, GameMessage::AutoplayTick { generation: 0 });
    assert!(!state.annotation_running());
    assert_eq!(
        state.annotate_report.expect("report").outcome,
        Outcome::GameChanged
    );
}

#[test]
fn a_game_without_moves_gets_the_score_as_initial_comment() {
    let (mut state, run) = setup(Game::new());
    let effects = start(&mut state);
    let log = drive(&mut state, run, effects, 5, &mut |_, _| Reply {
        cp: 20,
        pv: Vec::new(),
        second: None,
        mate: None,
    });
    assert_eq!(log.requests.len(), 1);
    assert_eq!(state.session.game().initial_comment(), Some("13:+0.20"));
    assert!(!state.annotation_running());
}

#[test]
fn a_cursor_in_the_middle_starts_there_and_the_move_before_it_is_left_alone() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..8]));
    send(&mut state, GameMessage::Navigate(Nav::Start));
    let third = nth_main(state.session.game(), 3);
    let cursor_pos = state.session.game().position_at(third).expect("p").fen();
    // Move the cursor to ply 3.
    for _ in 0..3 {
        send(&mut state, GameMessage::Navigate(Nav::Forward(1)));
    }
    assert_eq!(state.session.cursor(), third);
    let effects = start(&mut state);
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert_eq!(position_of(&log.requests[0]).fen(), cursor_pos);
    assert_eq!(log.requests.len(), 6, "the cursor position and five plies");
    let game = state.session.game();
    assert!(game.comment(nth_main(game, 3)).expect("n").is_none());
    assert!(game.comment(nth_main(game, 4)).expect("n").is_some());
}

#[test]
fn a_cursor_at_the_end_of_the_main_line_starts_from_the_beginning() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    send(&mut state, GameMessage::Navigate(Nav::GameEnd));
    let effects = start(&mut state);
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert_eq!(log.requests.len(), 5);
    assert_eq!(position_of(&log.requests[0]).fen(), Position::new().fen());
}

#[test]
fn only_one_side_is_annotated_but_every_position_is_searched() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..8]));
    state.annotate_options.side = Side::White;
    state.annotate_options.var_at_end = false;
    let effects = start(&mut state);
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert_eq!(log.requests.len(), 9);
    let game = state.session.game();
    for (ply, node) in game.mainline().enumerate() {
        let black = ply % 2 == 1;
        if black {
            // The diagram of a White blunder goes on the move before it.
            assert!(
                game.comment(node).expect("n").is_none()
                    && game.nags(node).expect("n").iter().all(|n| n.value() == 201),
                "ply {} is Black's",
                ply + 1
            );
        }
    }
}

#[test]
fn the_opening_limit_stops_after_the_moves_asked_for() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    state.annotate_options.opening_only = Some(5);
    let effects = start(&mut state);
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    // Plies 1 to 10 (five full moves) plus the start position.
    assert_eq!(log.requests.len(), 11);
    assert_eq!(state.annotate_report.expect("report").plies, 10);
}

#[test]
fn variations_are_annotated_recursively_when_asked() {
    let mut game = game_of(&HANGING_QUEEN[..6]);
    // 2.Qh5 gets the alternative 2.Nf3 with its own continuation 2...Nc6 3.Bc4.
    let e5 = nth_main(&game, 2);
    let nf3 = {
        let mv = game
            .position_at(e5)
            .expect("p")
            .parse_uci("g1f3")
            .expect("uci");
        game.add_move(e5, mv).expect("m")
    };
    let nc6 = {
        let mv = game
            .position_at(nf3)
            .expect("p")
            .parse_uci("b8c6")
            .expect("uci");
        game.add_move(nf3, mv).expect("m")
    };
    let bc4 = {
        let mv = game
            .position_at(nc6)
            .expect("p")
            .parse_uci("f1c4")
            .expect("uci");
        game.add_move(nc6, mv).expect("m")
    };
    let (mut state, run) = setup(game);
    state.annotate_options.variations = true;
    state.annotate_options.score_format = ScoreFormat::Plain;
    let effects = start(&mut state);
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    // Start + 6 main plies + 3 plies of the variation.
    assert_eq!(log.requests.len(), 10);
    let game = state.session.game();
    assert!(
        game.comment(bc4).expect("n").is_some(),
        "the last move of the variation"
    );
    // Without the option the variation is left alone.
    let (mut other, run2) = setup(state.session.game().clone());
    other.annotate_options.variations = false;
    let effects = start(&mut other);
    let log = drive(&mut other, run2, effects, 100, &mut material_engine);
    assert_eq!(log.requests.len(), 7);
}

#[test]
fn a_missed_mate_gets_the_mate_comment_on_the_line_and_a_diagram() {
    // After 2...Nc6 the engine sees a mate in 2 for White; White played 3.d3
    // and the mate is gone.
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5", "d1h5", "b8c6", "d2d3"]));
    let effects = start(&mut state);
    let mut engine = |request: &SearchRequest, pos: &Position| -> Reply {
        let mut reply = material_reply(pos);
        if request.position.moves.len() == 4 {
            reply.mate = Some(2);
            reply.cp = 32_748;
            reply.pv = vec![pos.parse_uci("h5f7").expect("uci")];
        } else {
            reply.cp = 0;
        }
        reply
    };
    drive(&mut state, run, effects, 100, &mut engine);
    let game = state.session.game();
    let parent = nth_main(game, 4);
    let children = game.children(parent).expect("children");
    assert_eq!(children.len(), 2, "the mate line is a variation of 3.d3");
    assert_eq!(
        game.comment_before(children[1]).expect("n"),
        Some("Mate in 2...")
    );
    assert!(
        game.nags(parent)
            .expect("n")
            .iter()
            .any(|n| n.value() == 201)
    );
    let nags: Vec<u8> = game
        .nags(nth_main(game, 5))
        .expect("n")
        .iter()
        .map(|n| n.value())
        .collect();
    assert!(nags.contains(&4), "a missed mate is a `??`: {nags:?}");
}

#[test]
fn a_mate_score_is_written_with_a_hash_in_the_eval_format() {
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5"]));
    let effects = start(&mut state);
    let mut engine = |request: &SearchRequest, pos: &Position| -> Reply {
        let mut reply = material_reply(pos);
        if request.position.moves.len() == 2 {
            reply.mate = Some(-3);
            reply.cp = -32_747;
        }
        reply
    };
    drive(&mut state, run, effects, 100, &mut engine);
    let game = state.session.game();
    let text = game
        .comment(nth_main(game, 2))
        .expect("n")
        .expect("comment");
    assert!(text.contains("[%eval #-3]"), "{text}");
}

#[test]
fn starting_without_an_engine_says_so_and_f2_is_started_when_there_is_one() {
    let mut state = AppState::default();
    send(&mut state, AnnotateMessage::Start);
    assert_eq!(annotate_notice(&state), Some(AnnotateNotice::NoEngine));
    assert!(!state.annotation_running());

    let id = state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("add");
    state.engines.set_hotkey(id, Some(Hotkey::F2)).expect("key");
    state.session.replace_game(game_of(&HANGING_QUEEN[..2]));
    let effects = send(&mut state, AnnotateMessage::Start);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::StartEngine { .. }))
    );
    assert!(state.annotation_running());
    let run = state.engine_runs.as_slice()[0].id;
    // The engine comes up: the run begins.
    let effects = send(
        &mut state,
        EngineMessage::Started {
            run,
            identity: Box::new(identity(Vec::new(), true)),
            at: 0,
        },
    );
    assert!(last_search(&effects).is_some());
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert_eq!(log.requests.len(), 3);
    assert!(!state.annotation_running());
}

#[test]
fn a_second_start_is_refused_and_open_cancels() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    start(&mut state);
    send(&mut state, AnnotateMessage::Start);
    assert_eq!(
        annotate_notice(&state),
        Some(AnnotateNotice::AlreadyRunning)
    );
    let effects = send(&mut state, AnnotateMessage::Open);
    assert!(commands(&effects).contains(&EngineCommand::Stop));
    let _ = run;
}

#[test]
fn nothing_to_annotate_is_reported_and_the_engine_is_left_alone() {
    let mut game = game_of(&HANGING_QUEEN[..4]);
    // A cursor at the end of a variation has nothing after it.
    let e5 = nth_main(&game, 2);
    let mv = game
        .position_at(e5)
        .expect("p")
        .parse_uci("g1f3")
        .expect("uci");
    let nf3 = game.add_move(e5, mv).expect("m");
    let (mut state, run) = setup(game);
    state
        .session
        .update(GameMessage::Navigate(Nav::GoToNode(nf3)));
    assert_eq!(state.session.cursor(), nf3);
    let effects = start(&mut state);
    assert_eq!(
        annotate_notice(&state),
        Some(AnnotateNotice::NothingToAnnotate)
    );
    assert!(!commands(&effects).iter().any(|c| matches!(
        c,
        EngineCommand::Stop | EngineCommand::SetMultiPv(_) | EngineCommand::SetOption { .. }
    )));
    assert!(state.engine_runs.get(run).expect("run").analyzing);
    assert!(!state.annotation_running());
}

#[test]
fn the_dialog_opens_with_the_last_options_and_start_uses_them() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    send(&mut state, AnnotateMessage::Open);
    let dialog = state.annotate_dialog.clone().expect("dialog");
    assert_eq!(dialog.options, AnnotateOptions::default());
    assert_eq!(dialog.moves, 4);
    assert!(!dialog.book_available);
    let mut options = dialog.options;
    options.with_vars = WithVars::No;
    options.depth = 99;
    send(&mut state, AnnotateMessage::SetOptions(options));
    send(&mut state, AnnotateMessage::SetEngine(Some(run)));
    let effects = send(&mut state, AnnotateMessage::Start);
    assert!(state.annotate_dialog.is_none());
    assert_eq!(state.annotate_options.depth, 99);
    let request = last_search(&effects).expect("search");
    assert_eq!(request.limits.depth, Some(99));
}

#[test]
fn exercises_are_probed_with_the_engine_and_flag_the_game() {
    // 1.e4 e5 2.Qh5 Nc6 3.Qxe5??: the position after the blunder is a shot
    // for Black: Nxe5.
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5", "d1h5", "b8c6", "h5e5"]));
    state.annotate_options.mark_exercises = true;
    let effects = start(&mut state);
    assert!(commands(&effects).contains(&EngineCommand::SetMultiPv(4)));
    let mut probes: Vec<(Option<u32>, Option<std::time::Duration>)> = Vec::new();
    let mut engine = |request: &SearchRequest, pos: &Position| -> Reply {
        let mut reply = material_reply(pos);
        let after_blunder = request.position.moves.len() == 5;
        if after_blunder {
            reply.second = Some(reply.cp + 700);
            probes.push((request.limits.depth, request.limits.movetime));
            // Obvious at depth 1 but not at depth 2: not obvious overall;
            // at times below 50 ms the engine plays something else.
            let ms = request.limits.movetime.map(|d| d.as_millis());
            let wrong = request.limits.depth == Some(2) || ms.is_some_and(|m| m < 50);
            if wrong {
                let other = pos
                    .legal_moves()
                    .into_iter()
                    .find(|m| Some(*m) != reply.pv.first().copied())
                    .expect("another move");
                reply.pv = vec![other];
            }
        }
        reply
    };
    let log = drive(&mut state, run, effects, 100, &mut engine);
    assert!(!state.annotation_running());
    // 6 positions + 11 probes of the position after the blunder.
    assert_eq!(log.requests.len(), 6 + 11);
    // The first search of the position is the run's own (depth 13).
    let depths: Vec<u32> = probes
        .iter()
        .filter_map(|p| p.0)
        .filter(|d| *d != 13)
        .collect();
    assert_eq!(depths, vec![1, 2, 3]);
    let times: Vec<u128> = probes
        .iter()
        .filter_map(|p| p.1.map(|d| d.as_millis()))
        .collect();
    assert_eq!(times, vec![1, 2, 5, 10, 50, 100, 200, 1000]);
    // MultiPV was put back.
    assert!(log.commands.contains(&EngineCommand::SetMultiPv(1)));
    let game = state.session.game();
    let blunder = nth_main(game, 5);
    let text = game.comment(blunder).expect("n").expect("comment");
    // Misses at 1, 2, 5 and 10 ms: the last miss is index 3, difficulty 5.
    // The score is put in front of it afterwards, as in the Scid.
    assert!(text.contains("****D5 0.0->-8.0"), "{text}");
    assert!(text.starts_with("-8.00 / +0.00 "), "{text}");
    assert!(state.session.tactics_pending());
    assert_eq!(state.annotate_report.expect("report").exercises, 1);
}

#[test]
fn an_obvious_move_is_not_an_exercise() {
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5", "d1h5", "b8c6", "h5e5"]));
    state.annotate_options.mark_exercises = true;
    let effects = start(&mut state);
    let mut engine = |request: &SearchRequest, pos: &Position| -> Reply {
        let mut reply = material_reply(pos);
        if request.position.moves.len() == 5 {
            reply.second = Some(reply.cp + 700);
        }
        reply
    };
    let log = drive(&mut state, run, effects, 100, &mut engine);
    // Found at depths 1, 2 and 3: the probes stop after three.
    assert_eq!(log.requests.len(), 6 + 3);
    assert!(!state.session.tactics_pending());
    let game = state.session.game();
    let text = game
        .comment(nth_main(game, 5))
        .expect("n")
        .expect("comment");
    assert!(!text.contains("****D"));
}

// ---- round 2: cancel before the search starts, watchdog, blocked edits ----

use super::annotate_flow::{CANCEL_GRACE, DEPTH_CEILING, DEPTH_STALL, STALL_GRACE};
use crate::annotate::AnnotationError;
use crate::engines::{CommandError, CommandKind};
use crate::flow::{Command, Replacement};
use crate::game_info::GameInfoMessage;

/// The watchdog timers of `effects`: run, generation, delay.
fn ticks(effects: &[Effect]) -> Vec<(RunId, u64, std::time::Duration)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ScheduleAnnotateTick {
                run,
                generation,
                delay,
            } => Some((*run, *generation, *delay)),
            _ => None,
        })
        .collect()
}

fn last_tick(effects: &[Effect]) -> (RunId, u64, std::time::Duration) {
    *ticks(effects).last().expect("a watchdog timer")
}

fn outcome(state: &AppState) -> Outcome {
    state.annotate_report.expect("report").outcome
}

/// The undo group is closed: the next edit leaves its own undo point.
fn assert_group_closed(state: &mut AppState) {
    let depth = state.session.undo_depth();
    let root = state.session.game().root();
    state.session.update(GameMessage::SetComment {
        node: root,
        text: format!("after {depth}"),
    });
    assert_eq!(state.session.undo_depth(), depth + 1, "the group is closed");
}

fn fake_with_everything() -> (AppState, RunId) {
    let mut draft = EngineDraft::new("Fake", "/fake");
    draft.multipv = 3;
    let extra = vec![
        EngineOption {
            name: "OwnBook".to_owned(),
            kind: OptionKind::Check { default: true },
        },
        EngineOption {
            name: "Clear Hash".to_owned(),
            kind: OptionKind::Button,
        },
    ];
    setup_with(game_of(&HANGING_QUEEN[..6]), extra, draft)
}

/// A search label and start for the in-flight search of `effects`.
fn start_search(state: &mut AppState, run: RunId, effects: &[Effect], id: u64) -> SearchId {
    let request = last_search(effects).expect("search in flight");
    let search = SearchId(id);
    event(
        state,
        run,
        EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        },
    );
    event(state, run, EngineEvent::SearchStarted(search));
    search
}

fn assert_put_back(commands: &[EngineCommand]) {
    assert!(
        commands.contains(&EngineCommand::SetMultiPv(3)),
        "{commands:?}"
    );
    assert!(
        commands.contains(&EngineCommand::SetOption {
            name: "OwnBook".to_owned(),
            value: OptionValue::Bool(true),
        }),
        "{commands:?}"
    );
}

#[test]
fn cancel_before_the_first_search_starts_ends_at_once_and_restores_everything() {
    let (mut state, run) = fake_with_everything();
    let before = state.session.game().clone();
    let effects = start(&mut state);
    assert!(last_search(&effects).is_some());
    // No `SearchTagged`, no `SearchStarted`: the search waits behind the
    // `isready` of the setup commands.
    let effects = send(&mut state, AnnotateMessage::Cancel);
    let sent = commands(&effects);
    assert!(sent.contains(&EngineCommand::Stop), "{sent:?}");
    assert_put_back(&sent);
    assert!(!state.annotation_running());
    assert_eq!(outcome(&state), Outcome::Cancelled);
    assert!(state.engine_runs.get(run).expect("run").analyzing);
    assert_group_closed(&mut state);
    // The tags (the first edit) stay: the group closed with them.
    assert!(!state.session.game().same_content(&before));
}

#[test]
fn what_a_cancelled_unstarted_search_reports_late_is_ignored() {
    let (mut state, run) = fake_with_everything();
    let effects = start(&mut state);
    let request = last_search(&effects).expect("search");
    send(&mut state, AnnotateMessage::Cancel);
    let notice = state.notice.clone();
    let report = state.annotate_report;
    let search = SearchId(4242);
    event(
        &mut state,
        run,
        EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        },
    );
    event(&mut state, run, EngineEvent::SearchStarted(search));
    event(
        &mut state,
        run,
        EngineEvent::Info(SearchInfo {
            search,
            multipv: 1,
            depth: Some(3),
            score: Some((Score::Cp(10), Bound::Exact)),
            ..SearchInfo::default()
        }),
    );
    assert_eq!(state.flow.annotate_stale.len(), 1, "remembered");
    event(
        &mut state,
        run,
        EngineEvent::BestMove(BestMove {
            search,
            mv: None,
            ponder: None,
        }),
    );
    assert!(state.flow.annotate_stale.is_empty(), "forgotten at the end");
    assert_eq!(state.notice, notice);
    assert_eq!(state.annotate_report, report);
    assert!(!state.annotation_running());
}

#[test]
fn cancel_among_the_probes_after_clear_hash_ends_at_once() {
    let game = game_of(&["e2e4", "e7e5", "d1h5", "b8c6", "h5e5"]);
    let mut draft = EngineDraft::new("Fake", "/fake");
    draft.multipv = 3;
    let extra = vec![EngineOption {
        name: "Clear Hash".to_owned(),
        kind: OptionKind::Button,
    }];
    let (mut state, run) = setup_with(game, extra, draft);
    state.annotate_options.mark_exercises = true;
    let effects = start(&mut state);
    let mut engine = |request: &SearchRequest, pos: &Position| -> Reply {
        let mut reply = material_reply(pos);
        if request.position.moves.len() == 5 {
            reply.second = Some(reply.cp + 700);
        }
        reply
    };
    // The six positions are answered; the first probe is sent behind a
    // `Clear Hash` and a one-line setting and has not started.
    let log = drive(&mut state, run, effects, 6, &mut engine);
    let sent = commands(&log.last);
    assert!(sent.contains(&EngineCommand::SetMultiPv(1)), "{sent:?}");
    assert!(
        sent.iter()
            .any(|c| matches!(c, EngineCommand::SetOption { name, .. } if name == "Clear Hash"))
    );
    assert!(state.annotation_running());
    let effects = send(&mut state, AnnotateMessage::Cancel);
    let sent = commands(&effects);
    assert!(sent.contains(&EngineCommand::Stop));
    // The lines of the engine come back to what the user had.
    assert!(sent.contains(&EngineCommand::SetMultiPv(3)), "{sent:?}");
    assert!(!state.annotation_running());
    assert_eq!(outcome(&state), Outcome::Cancelled);
    assert_group_closed(&mut state);
}

#[test]
fn the_probes_use_one_line_and_the_run_gets_its_lines_back() {
    let game = game_of(&["e2e4", "e7e5", "d1h5", "b8c6", "h5e5"]);
    let extra = vec![EngineOption {
        name: "Clear Hash".to_owned(),
        kind: OptionKind::Button,
    }];
    let mut draft = EngineDraft::new("Fake", "/fake");
    draft.multipv = 2;
    let (mut state, run) = setup_with(game, extra, draft);
    state.annotate_options.mark_exercises = true;
    let effects = start(&mut state);
    let mut engine = |request: &SearchRequest, pos: &Position| -> Reply {
        let mut reply = material_reply(pos);
        if request.position.moves.len() == 5 {
            reply.second = Some(reply.cp + 700);
        }
        reply
    };
    let log = drive(&mut state, run, effects, 100, &mut engine);
    // Four lines for the run, one for the three probes, four again after
    // them, the user's two at the end.
    let lines: Vec<u8> = log
        .commands
        .iter()
        .filter_map(|c| match c {
            EngineCommand::SetMultiPv(n) => Some(*n),
            _ => None,
        })
        .collect();
    assert_eq!(lines, vec![4, 1, 4, 2]);
    // `Clear Hash` before each probe (D470 keeps it; D893).
    let clears = log
        .commands
        .iter()
        .filter(|c| matches!(c, EngineCommand::SetOption { name, .. } if name == "Clear Hash"))
        .count();
    assert_eq!(clears, 3);
}

#[test]
fn the_watchdog_is_armed_with_each_search_and_fails_a_mute_engine() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    let effects = start(&mut state);
    let (timer_run, generation, delay) = last_tick(&effects);
    assert_eq!(timer_run, run);
    assert_eq!(delay, DEPTH_STALL, "a depth limit has no time to add to");
    // A stale timer does nothing.
    send(
        &mut state,
        AnnotateMessage::Tick {
            run,
            generation: generation + 1,
        },
    );
    assert!(state.annotation_running());
    // The engine never answers the `go`: the timer ends the run.
    let effects = send(&mut state, AnnotateMessage::Tick { run, generation });
    assert!(!state.annotation_running());
    assert_eq!(
        outcome(&state),
        Outcome::Failed(AnnotationError::EngineFailed)
    );
    assert!(commands(&effects).contains(&EngineCommand::Stop));
    assert_group_closed(&mut state);
}

#[test]
fn under_a_time_limit_the_watchdog_waits_the_grace_past_the_movetime() {
    let (mut state, _) = setup(game_of(&HANGING_QUEEN[..2]));
    let movetime = std::time::Duration::from_secs(5);
    state.annotate_options = AnnotateOptions {
        control: Control::Time,
        movetime,
        ..AnnotateOptions::default()
    };
    let effects = start(&mut state);
    assert_eq!(last_tick(&effects).2, movetime + STALL_GRACE);
}

#[test]
fn a_search_that_reports_is_alive_and_the_watchdog_waits_again() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    let effects = start(&mut state);
    let (_, generation, _) = last_tick(&effects);
    let search = start_search(&mut state, run, &effects, 7);
    let effects = send(&mut state, AnnotateMessage::Tick { run, generation });
    assert!(state.annotation_running(), "the engine answered");
    let (_, next, _) = last_tick(&effects);
    assert_ne!(next, generation);
    // Silent for a whole period now: given up.
    send(
        &mut state,
        AnnotateMessage::Tick {
            run,
            generation: next,
        },
    );
    assert!(!state.annotation_running());
    // What the dead search says late is not lost on anyone.
    event(
        &mut state,
        run,
        EngineEvent::BestMove(BestMove {
            search,
            mv: None,
            ponder: None,
        }),
    );
    assert!(!state.annotation_running());
}

#[test]
fn a_refused_or_busy_search_fails_the_run_but_a_refused_option_only_warns() {
    for (kind, error, fails) in [
        (CommandKind::Search, CommandError::Refused, true),
        (CommandKind::Search, CommandError::Busy, true),
        (CommandKind::SetMultiPv, CommandError::Refused, false),
        (CommandKind::SetOption, CommandError::Busy, false),
        (CommandKind::Stop, CommandError::Refused, false),
    ] {
        let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
        start(&mut state);
        send(
            &mut state,
            EngineMessage::CommandFailed { run, kind, error },
        );
        assert_eq!(!state.annotation_running(), fails, "{kind:?} {error:?}");
        if fails {
            assert_eq!(
                outcome(&state),
                Outcome::Failed(AnnotationError::EngineFailed)
            );
            assert_group_closed(&mut state);
        }
    }
}

#[test]
fn a_search_refused_by_the_engine_itself_fails_the_run() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    start(&mut state);
    event(
        &mut state,
        run,
        EngineEvent::Warning(mychess_engine::EngineWarning::CommandRefused {
            command: "go",
            reason: "no".to_owned(),
        }),
    );
    assert!(!state.annotation_running());
    assert_eq!(
        outcome(&state),
        Outcome::Failed(AnnotationError::EngineFailed)
    );
}

#[test]
fn the_user_cancels_a_search_the_engine_refused_or_never_started() {
    // Refused (no answer will come): the cancel ends the run.
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    start(&mut state);
    send(&mut state, AnnotateMessage::Cancel);
    assert_eq!(outcome(&state), Outcome::Cancelled);
    assert!(!state.annotation_running());
    // Started but the engine is mute after the `stop`: the grace ends it.
    let (mut state, run2) = setup(game_of(&HANGING_QUEEN[..4]));
    let effects = start(&mut state);
    start_search(&mut state, run2, &effects, 1);
    let effects = send(&mut state, AnnotateMessage::Cancel);
    assert!(state.annotation_running(), "waits for the bestmove");
    let (_, generation, delay) = last_tick(&effects);
    assert_eq!(delay, CANCEL_GRACE);
    send(
        &mut state,
        AnnotateMessage::Tick {
            run: run2,
            generation,
        },
    );
    assert!(!state.annotation_running());
    assert_eq!(outcome(&state), Outcome::Cancelled);
    assert_group_closed(&mut state);
    let _ = run;
}

fn running_state() -> (AppState, RunId) {
    let (mut state, run) = fake_with_everything();
    let effects = start(&mut state);
    drive(&mut state, run, effects, 2, &mut material_engine);
    assert!(state.annotation_running());
    (state, run)
}

#[test]
fn every_way_to_replace_or_edit_the_game_is_refused_while_the_run_goes() {
    let (mut state, _) = running_state();
    let before = state.session.game().clone();
    let (generation, changes) = (state.session.generation(), state.session.changes());
    let mut effects = Vec::new();
    for command in [
        Command::NewGame,
        Command::Open,
        Command::PasteFen,
        Command::PasteMoves,
        Command::NextGame,
        Command::SetupBoard,
    ] {
        state.notice = None;
        update(&mut state, Message::Command(command));
        assert_eq!(annotate_notice(&state), Some(AnnotateNotice::Busy));
    }
    state.notice = None;
    state.begin_replace(Replacement::New, &mut effects);
    assert_eq!(annotate_notice(&state), Some(AnnotateNotice::Busy));
    state.notice = None;
    let root = state.session.game().root();
    state.apply_game(
        GameMessage::SetComment {
            node: root,
            text: "x".to_owned(),
        },
        &mut effects,
    );
    assert_eq!(annotate_notice(&state), Some(AnnotateNotice::Busy));
    assert_eq!(
        (state.session.generation(), state.session.changes()),
        (generation, changes)
    );
    assert!(state.session.game().same_content(&before));
    assert!(state.annotation_running(), "none of them stopped the run");
}

#[test]
fn the_tag_dialog_cannot_overwrite_the_tags_of_the_run() {
    let (mut state, run) = fake_with_everything();
    send(&mut state, GameInfoMessage::Open);
    assert!(state.info_dialog.is_some());
    let effects = start(&mut state);
    drive(&mut state, run, effects, 2, &mut material_engine);
    assert!(state.annotation_running());
    let annotator = state
        .session
        .game()
        .tags()
        .get_extra("Annotator")
        .map(str::to_owned);
    assert!(annotator.is_some());
    state.notice = None;
    send(&mut state, GameInfoMessage::Apply);
    assert_eq!(annotate_notice(&state), Some(AnnotateNotice::Busy));
    assert!(state.annotation_running());
    assert_eq!(
        state
            .session
            .game()
            .tags()
            .get_extra("Annotator")
            .map(str::to_owned),
        annotator
    );
}

#[test]
fn quitting_cancels_the_run_first_and_puts_the_engine_back() {
    let (mut state, run) = running_state();
    let effects = send(&mut state, Message::Quit);
    assert!(!state.annotation_running());
    assert_eq!(outcome(&state), Outcome::Cancelled);
    assert_put_back(&commands(&effects));
    assert!(state.engine_runs.get(run).is_some());
    assert_group_closed(&mut state);
}

#[test]
fn a_game_replaced_while_the_engine_starts_stops_the_run_before_it_begins() {
    let mut state = AppState::default();
    let id = state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("add");
    state.engines.set_hotkey(id, Some(Hotkey::F2)).expect("key");
    state.session.replace_game(game_of(&HANGING_QUEEN[..4]));
    send(&mut state, AnnotateMessage::Start);
    assert!(state.annotation_running());
    let run = state.engine_runs.as_slice()[0].id;
    state.session.replace_game(game_of(&["d2d4"]));
    send(&mut state, GameMessage::AutoplayTick { generation: 0 });
    assert!(!state.annotation_running());
    assert_eq!(outcome(&state), Outcome::GameChanged);
    // The engine comes up late: nothing starts, the new game is untouched.
    let effects = send(
        &mut state,
        EngineMessage::Started {
            run,
            identity: Box::new(identity(Vec::new(), true)),
            at: 0,
        },
    );
    assert!(commands(&effects).iter().all(
        |c| !matches!(c, EngineCommand::Search(r) if crate::annotate::is_annotation_tag(r.tag))
    ));
    assert!(state.session.game().tags().get_extra("Annotator").is_none());
}

#[test]
fn the_engine_failing_to_start_or_being_stopped_while_starting_ends_the_run() {
    for kind in 0..2 {
        let mut state = AppState::default();
        let id = state
            .engines
            .add(EngineDraft::new("Fake", "/fake"))
            .expect("add");
        state.engines.set_hotkey(id, Some(Hotkey::F2)).expect("key");
        state.session.replace_game(game_of(&HANGING_QUEEN[..4]));
        send(&mut state, AnnotateMessage::Start);
        let run = state.engine_runs.as_slice()[0].id;
        let before = state.session.game().clone();
        if kind == 0 {
            send(
                &mut state,
                EngineMessage::StartFailed {
                    run,
                    failure: crate::engines::EngineFailure::from_error(
                        &mychess_engine::EngineError::Closed,
                    ),
                },
            );
        } else {
            send(&mut state, EngineMessage::Stopped(run));
        }
        assert!(!state.annotation_running(), "kind {kind}");
        assert_eq!(
            outcome(&state),
            Outcome::Failed(AnnotationError::EngineFailed)
        );
        assert!(state.session.game().same_content(&before));
        assert_group_closed(&mut state);
    }
}

#[test]
fn a_best_move_of_another_search_is_not_the_runs_and_the_walk_does_not_advance() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    let effects = start(&mut state);
    start_search(&mut state, run, &effects, 1);
    let done = state.annotation_progress().expect("progress").done_plies;
    // An old search, out of order.
    event(
        &mut state,
        run,
        EngineEvent::BestMove(BestMove {
            search: SearchId(555),
            mv: None,
            ponder: None,
        }),
    );
    assert!(state.annotation_running());
    assert_eq!(
        state.annotation_progress().expect("progress").done_plies,
        done
    );
    // The run's own search still finishes the run in the end.
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert!(!log.requests.is_empty());
}

#[test]
fn the_engine_is_put_back_and_the_group_closed_in_every_ending() {
    // Game changed.
    let (mut state, _) = running_state();
    state.session.replace_game(Game::new());
    let effects = send(&mut state, GameMessage::AutoplayTick { generation: 0 });
    assert_eq!(outcome(&state), Outcome::GameChanged);
    assert_put_back(&commands(&effects));
    assert_group_closed(&mut state);
    // Engine failed while it is alive (a refused search).
    let (mut state, run) = fake_with_everything();
    start(&mut state);
    let effects = send(
        &mut state,
        EngineMessage::CommandFailed {
            run,
            kind: CommandKind::Search,
            error: CommandError::Busy,
        },
    );
    assert_put_back(&commands(&effects));
    assert_group_closed(&mut state);
    // The end of the game.
    let (mut state, run) = fake_with_everything();
    let effects = start(&mut state);
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert_eq!(outcome(&state), Outcome::Finished);
    assert_put_back(&log.commands);
    assert_group_closed(&mut state);
}

#[test]
fn the_autoplay_tick_passes_and_does_not_disturb_the_run() {
    let (mut state, _) = running_state();
    state.notice = None;
    send(&mut state, GameMessage::AutoplayTick { generation: 99 });
    assert!(state.annotation_running());
    assert_eq!(annotate_notice(&state), None);
}

// ---- D900: late reports of a stopped search, absolute deadlines ----

/// Gives the run an analysis view, to see whether anything leaks into it.
fn with_analysis_view(state: &mut AppState, run: RunId) {
    let view = crate::analysis::AnalysisView::new(run, 1, &crate::prefs::AnalysisPrefs::default());
    state.analysis.push(view);
}

/// What the window shows of a search (not the bookkeeping of its own
/// restarts, which the end of the run legitimately triggers).
fn view_of(
    state: &AppState,
    run: RunId,
) -> (Option<SearchId>, bool, usize, Option<(Score, Bound)>) {
    let view = state.analysis.get(run).expect("view");
    (
        view.search,
        view.finished,
        view.lines.len(),
        view.main_score(),
    )
}

fn late_reports(state: &mut AppState, run: RunId, search: SearchId) {
    event(state, run, EngineEvent::SearchStarted(search));
    event(
        state,
        run,
        EngineEvent::Info(SearchInfo {
            search,
            multipv: 1,
            depth: Some(5),
            score: Some((Score::Cp(33), Bound::Exact)),
            ..SearchInfo::default()
        }),
    );
    event(
        state,
        run,
        EngineEvent::BestMove(BestMove {
            search,
            mv: None,
            ponder: None,
        }),
    );
}

#[test]
fn a_search_adopted_when_the_run_ends_does_not_leak_into_the_analysis_window() {
    for how in ["watchdog", "game changed", "engine failed"] {
        let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
        with_analysis_view(&mut state, run);
        let effects = start(&mut state);
        let (_, generation, _) = last_tick(&effects);
        let search = start_search(&mut state, run, &effects, 9);
        match how {
            "watchdog" => {
                // Alive at the first tick, mute at the second.
                let effects = send(&mut state, AnnotateMessage::Tick { run, generation });
                let (_, next, _) = last_tick(&effects);
                send(
                    &mut state,
                    AnnotateMessage::Tick {
                        run,
                        generation: next,
                    },
                );
            }
            "game changed" => {
                let root = state.session.game().root();
                state.session.update(GameMessage::SetComment {
                    node: root,
                    text: "someone".to_owned(),
                });
                send(&mut state, GameMessage::AutoplayTick { generation: 0 });
            }
            _ => {
                let mut effects = Vec::new();
                state.annotate_engine_gone(run, true, &mut effects);
                assert!(commands(&effects).contains(&EngineCommand::Stop));
            }
        }
        assert!(!state.annotation_running(), "{how}");
        // After the run's own restoring of the board's analysis.
        let before = view_of(&state, run);
        late_reports(&mut state, run, search);
        assert_eq!(
            view_of(&state, run),
            before,
            "{how}: nothing reached the window"
        );
        assert!(
            state.flow.annotate_stale.is_empty(),
            "{how}: forgotten at the bestmove"
        );
    }
}

#[test]
fn an_unlabelled_start_after_the_stop_of_the_run_is_dropped() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    with_analysis_view(&mut state, run);
    start(&mut state);
    // Sent, never labelled nor started: the engine lost the label.
    let mut effects = Vec::new();
    state.annotate_engine_gone(run, true, &mut effects);
    assert!(commands(&effects).contains(&EngineCommand::Stop));
    let before = view_of(&state, run);
    late_reports(&mut state, run, SearchId(77));
    assert_eq!(view_of(&state, run), before);
    // The next start is the window's again.
    event(&mut state, run, EngineEvent::SearchStarted(SearchId(78)));
    assert_eq!(view_of(&state, run).0, Some(SearchId(78)));
}

#[test]
fn a_chatty_engine_without_bestmove_is_given_up_at_the_absolute_deadline() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    let effects = start(&mut state);
    let search = start_search(&mut state, run, &effects, 3);
    let (_, mut generation, mut delay) = last_tick(&effects);
    let mut waited = std::time::Duration::ZERO;
    while state.annotation_running() {
        waited += delay;
        assert!(waited <= DEPTH_CEILING, "gave up past the ceiling");
        // The engine reports before every tick.
        event(
            &mut state,
            run,
            EngineEvent::Info(SearchInfo {
                search,
                multipv: 1,
                depth: Some(2),
                score: Some((Score::Cp(1), Bound::Exact)),
                ..SearchInfo::default()
            }),
        );
        let effects = send(&mut state, AnnotateMessage::Tick { run, generation });
        if let Some(&(_, next, d)) = ticks(&effects).last() {
            generation = next;
            delay = d;
        }
    }
    assert_eq!(waited, DEPTH_CEILING);
    assert_eq!(
        outcome(&state),
        Outcome::Failed(AnnotationError::EngineFailed)
    );
}

#[test]
fn a_chatty_engine_under_a_time_limit_fails_at_movetime_plus_grace() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    let movetime = std::time::Duration::from_secs(5);
    state.annotate_options = AnnotateOptions {
        control: Control::Time,
        movetime,
        ..AnnotateOptions::default()
    };
    let effects = start(&mut state);
    let search = start_search(&mut state, run, &effects, 3);
    let (_, generation, _) = last_tick(&effects);
    event(
        &mut state,
        run,
        EngineEvent::Info(SearchInfo {
            search,
            multipv: 1,
            depth: Some(2),
            score: Some((Score::Cp(1), Bound::Exact)),
            ..SearchInfo::default()
        }),
    );
    send(&mut state, AnnotateMessage::Tick { run, generation });
    assert!(!state.annotation_running(), "alive, but past the deadline");
}

#[test]
fn a_chatty_engine_cannot_extend_a_cancel() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..4]));
    let effects = start(&mut state);
    let search = start_search(&mut state, run, &effects, 3);
    let effects = send(&mut state, AnnotateMessage::Cancel);
    let (_, generation, delay) = last_tick(&effects);
    assert_eq!(delay, CANCEL_GRACE);
    event(
        &mut state,
        run,
        EngineEvent::Info(SearchInfo {
            search,
            multipv: 1,
            depth: Some(2),
            score: Some((Score::Cp(1), Bound::Exact)),
            ..SearchInfo::default()
        }),
    );
    assert!(state.annotation_running());
    send(&mut state, AnnotateMessage::Tick { run, generation });
    assert!(!state.annotation_running());
    assert_eq!(outcome(&state), Outcome::Cancelled);
}

#[test]
fn the_run_does_not_start_while_a_base_is_opening_and_the_dialog_stays() {
    use crate::bases::{BasesMessage, BasesNotice};
    let (mut state, _run) = setup(game_of(&HANGING_QUEEN[..6]));
    send(&mut state, AnnotateMessage::Open);
    assert!(state.annotate_dialog.is_some());
    let path = std::env::temp_dir().join("mychess-annotate-opening-test.si4");
    send(&mut state, BasesMessage::Open(path));
    assert!(state.base_opening().is_some(), "the opening is in flight");
    let effects = start(&mut state);
    assert!(!state.annotation_running());
    assert!(effects.is_empty(), "no search was sent");
    assert!(
        matches!(
            state.notice,
            Some(AppNotice::Bases(BasesNotice::OpenInProgress))
        ),
        "{:?}",
        state.notice
    );
    assert!(
        state.annotate_dialog.is_some(),
        "the dialog was not consumed"
    );
    assert!(state.base_opening().is_some(), "the opening is untouched");
}

#[test]
fn a_base_cannot_be_opened_while_the_run_goes() {
    use crate::bases::BasesMessage;
    let (mut state, run) = setup(game_of(&HANGING_QUEEN[..6]));
    let effects = start(&mut state);
    drive(&mut state, run, effects, 2, &mut material_engine);
    assert!(state.annotation_running());
    let path = std::env::temp_dir().join("mychess-annotate-opening-test2.si4");
    send(&mut state, BasesMessage::Open(path));
    assert!(state.base_opening().is_none());
    assert_eq!(annotate_notice(&state), Some(AnnotateNotice::Busy));
    assert!(state.annotation_running());
}

#[test]
fn a_tournament_does_not_start_during_an_annotation_and_the_reverse() {
    // Regression (D1311): both commanded the analyses and the game.
    let (mut state, _run) = setup(game_of(&HANGING_QUEEN));
    start(&mut state);
    assert!(state.annotation_running());
    let effects = send(
        &mut state,
        crate::tournament::TournamentMessage::Start {
            config: Box::default(),
            at: std::time::Instant::now(),
        },
    );
    assert!(effects.is_empty());
    assert_eq!(
        state.notice,
        Some(AppNotice::Tournament(
            crate::tournament::TournamentNotice::Busy
        ))
    );
    assert!(!state.tournament_live());
}

// ---- the batch over the filter (phase B) ----

mod batch {
    use super::*;
    use crate::bases::{BasesMessage, SlotId};
    use crate::effect::perform;
    use mychess_db::GameSource;

    const THREE_GAMES: &str = "[Event \"A\"]\n[Result \"*\"]\n\n1. e4 e5 *\n\n\
        [Event \"B\"]\n[Result \"*\"]\n\n*\n\n\
        [Event \"C\"]\n[Result \"*\"]\n\n1. d4 d5 *\n";

    /// A `.si4` with the three games, opened in a slot of `state`.
    fn open_base(state: &mut AppState, dir: &std::path::Path) -> (SlotId, std::path::PathBuf) {
        use mychess_db::si4::Si4Writer;
        use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
        let base = dir.join("batch");
        let mut writer = Si4Writer::create(&base, 0).expect("create");
        import_pgn(
            THREE_GAMES.as_bytes(),
            &mut writer,
            &ImportOptions::default(),
            None,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .expect("import");
        drop(writer);
        let path = base.with_extension("si4");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let mut queue = update(state, Message::Bases(BasesMessage::Open(path.clone())));
        while let Some(effect) = queue.pop() {
            if matches!(effect, Effect::OpenBase { .. }) {
                let message = rt.block_on(perform(effect));
                queue.extend(update(state, message));
            }
        }
        (state.bases.current(), path)
    }

    type Step = Result<crate::annotate::BatchStep, crate::bases::BaseError>;

    /// The disk step among `effects`, performed now.
    fn take_step(effects: &[Effect]) -> Option<Step> {
        effects.iter().find_map(|e| match e {
            Effect::AnnotateBatchStep {
                path,
                save,
                next,
                expected_total,
                stamp,
            } => Some(crate::annotate::batch_step(
                path,
                save.as_ref(),
                *next,
                *expected_total,
                *stamp,
            )),
            _ => None,
        })
    }

    /// Runs the batch to its end, answering the engine and the disk steps.
    fn run_batch(state: &mut AppState, run: RunId, mut effects: Vec<Effect>) -> usize {
        let mut searches = 0;
        for _ in 0..200 {
            if let Some(result) = take_step(&effects) {
                effects = send(state, AnnotateMessage::BatchStep(result));
                if !state.annotation_running() {
                    break;
                }
                continue;
            }
            let Some(request) = last_search(&effects) else {
                break;
            };
            let reply = material_reply(&position_of(&request));
            let Some((after, _)) = answer(state, run, &effects, 100, &reply) else {
                break;
            };
            searches += 1;
            effects = after;
        }
        searches
    }

    fn start_batch(state: &mut AppState) -> Vec<Effect> {
        state.annotate_options = AnnotateOptions {
            batch: true,
            ..AnnotateOptions::default()
        };
        start(state)
    }

    #[test]
    fn annotates_the_filter_and_saves_each_game() {
        let dir = tempfile::tempdir().expect("dir");
        let (mut state, run) = setup(Game::new());
        let (slot, path) = open_base(&mut state, dir.path());
        let effects = start_batch(&mut state);
        assert!(state.annotation_running());
        let searches = run_batch(&mut state, run, effects);
        assert!(!state.annotation_running());
        // A: start + 2 plies; B: the start position (rule 11); C: like A.
        // The last answer may be the board's analysis coming back.
        assert!(searches >= 7, "{searches}");
        assert_eq!(
            state.notice,
            Some(AppNotice::Annotate(AnnotateNotice::BatchFinished {
                games: 3,
                skipped: 0
            }))
        );
        let mut db = mychess_db::si4::Si4Database::open(&path).expect("reopen");
        for (n, tagged) in [(1, true), (2, true), (3, true)] {
            let game = GameSource::game(&mut db, n).expect("game");
            assert_eq!(
                game.tags().get_extra("Annotator").is_some(),
                tagged,
                "game {n}"
            );
        }
        // The slot reads the new file; the last game is not "altered".
        assert!(state.bases.slot(slot).is_some());
        assert!(!state.session.is_dirty());
    }

    #[test]
    fn games_outside_the_filter_are_left_alone() {
        let dir = tempfile::tempdir().expect("dir");
        let (mut state, run) = setup(Game::new());
        let (slot, path) = open_base(&mut state, dir.path());
        if let Some(s) = state.bases.slot_mut(slot) {
            s.filter.set(3, 0);
        }
        let effects = start_batch(&mut state);
        run_batch(&mut state, run, effects);
        let mut db = mychess_db::si4::Si4Database::open(&path).expect("reopen");
        let first = GameSource::game(&mut db, 1).expect("game");
        let third = GameSource::game(&mut db, 3).expect("game");
        assert!(first.tags().get_extra("Annotator").is_some());
        assert!(third.tags().get_extra("Annotator").is_none());
    }

    #[test]
    fn a_pgn_base_cannot_take_a_batch() {
        let dir = tempfile::tempdir().expect("dir");
        let (mut state, _run) = setup(Game::new());
        let pgn = dir.path().join("g.pgn");
        std::fs::write(&pgn, THREE_GAMES).expect("write");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let mut queue = update(&mut state, Message::Bases(BasesMessage::Open(pgn)));
        while let Some(effect) = queue.pop() {
            if matches!(effect, Effect::OpenBase { .. }) {
                let message = rt.block_on(perform(effect));
                queue.extend(update(&mut state, message));
            }
        }
        let effects = start_batch(&mut state);
        assert!(effects.is_empty());
        assert_eq!(
            annotate_notice(&state),
            Some(AnnotateNotice::BatchUnavailable)
        );
        assert!(!state.annotation_running());
    }

    #[test]
    fn a_dirty_game_is_not_discarded_by_the_batch() {
        let dir = tempfile::tempdir().expect("dir");
        let (mut state, _run) = setup(game_of(&HANGING_QUEEN[..2]));
        open_base(&mut state, dir.path());
        state.session.replace_game(game_of(&["e2e4"]));
        drop(state.session.edit(|game, _| {
            game.tags_mut().set_extra("X", "1").ok();
            Ok(())
        }));
        assert!(state.session.is_dirty());
        start_batch(&mut state);
        assert_eq!(annotate_notice(&state), Some(AnnotateNotice::BatchDirty));
        assert!(!state.annotation_running());
    }

    #[test]
    fn cancelling_between_games_stops_after_the_step_in_flight() {
        let dir = tempfile::tempdir().expect("dir");
        let (mut state, _run) = setup(Game::new());
        open_base(&mut state, dir.path());
        let effects = start_batch(&mut state);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::AnnotateBatchStep { .. }))
        );
        send(&mut state, AnnotateMessage::Cancel);
        let step = take_step(&effects);
        send(&mut state, AnnotateMessage::BatchStep(step.expect("step")));
        assert!(!state.annotation_running());
        assert!(matches!(
            annotate_notice(&state),
            Some(AnnotateNotice::BatchStopped { games: 0, total: 3 })
        ));
    }
}
