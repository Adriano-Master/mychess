//! Shared helpers of the `m8_1_*` contract tests (spec
//! `docs/specs/m8-1-jogo-contra-engine.md`, phase A): controller set-up,
//! outputs filters, a Polyglot book built in memory and a ready UCI run for
//! the `AppState` glue.
//!
//! Only the public API of `mychess-app` is used.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::time::{Duration, Instant};

use mychess_app::engines::{EngineDraft, EngineId, EngineMessage, RunId, StartMode};
use mychess_app::{
    AppNotice, AppState, BookSelection, Effect, Message, OpponentInfo, PlayBook, PlayConfig,
    PlayController, PlayEnd, PlayMessage, PlayNotice, PlayOutput, PlayPhase, PlaySetup, PlayView,
    Think, ThinkLimits, TimeMode, update,
};
use mychess_book::Book;
use mychess_core::{Color, Game, GameResult, Move, PartialDate, Position, Square};
use mychess_engine::{
    BestMove, Capabilities, EngineCommand, EngineEvent, EngineIdentity, EngineOption, ExitInfo,
    ExitReason, OptionKind, SearchId, SearchRequest,
};

pub const SEC: Duration = Duration::from_secs(1);

pub fn timebonus(w_min: u32, w_inc: u32, b_min: u32, b_inc: u32) -> TimeMode {
    TimeMode::TimeBonus {
        white_minutes: w_min,
        white_bonus: w_inc,
        black_minutes: b_min,
        black_bonus: b_inc,
    }
}

pub fn blitz() -> TimeMode {
    timebonus(5, 3, 5, 3)
}

// ---------------------------------------------------------------------
// The pure controller
// ---------------------------------------------------------------------

pub fn setup(color: Color, mode: TimeMode) -> PlaySetup {
    let mut config = PlayConfig::new(EngineId(1));
    config.mode = mode;
    PlaySetup {
        config,
        player_color: color,
        start: Position::new(),
        opponent: OpponentInfo {
            name: "Fake 1".to_owned(),
            rating: None,
        },
        player_name: "Ana".to_owned(),
        date: PartialDate::new(Some(2026), Some(10), Some(24)).expect("date"),
        book: None,
        seed: 7,
    }
}

pub fn fen_setup(color: Color, mode: TimeMode, fen: &str) -> PlaySetup {
    let mut s = setup(color, mode);
    s.start = Position::from_fen(fen).expect("fen");
    s
}

/// A started game plus the instant it started at.
pub fn begin(s: PlaySetup) -> (PlayController, Vec<PlayOutput>, Instant) {
    let t0 = Instant::now();
    let (c, out) = PlayController::new(s, t0).expect("a valid setup");
    (c, out, t0)
}

pub fn uci(c: &PlayController, text: &str) -> Move {
    c.position().parse_uci(text).expect("uci")
}

pub fn thinks(out: &[PlayOutput]) -> Vec<Think> {
    out.iter()
        .filter_map(|o| match o {
            PlayOutput::Think(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

pub fn ended(out: &[PlayOutput]) -> Option<PlayEnd> {
    out.iter().find_map(|o| match o {
        PlayOutput::Ended(e) => Some(*e),
        _ => None,
    })
}

pub fn moved_count(out: &[PlayOutput]) -> usize {
    out.iter()
        .filter(|o| matches!(o, PlayOutput::Moved(_)))
        .count()
}

pub fn has_stop(out: &[PlayOutput]) -> bool {
    out.iter().any(|o| matches!(o, PlayOutput::StopThinking))
}

/// Plays `ucis` alternately, starting with the side to move; the player's
/// moves go through `player_move`, the opponent's through `opponent_moved`.
/// Time does not advance. Returns the outputs of the last move.
pub fn play_ucis(c: &mut PlayController, ucis: &[&str], t: Instant) -> Vec<PlayOutput> {
    let mut last = Vec::new();
    for text in ucis {
        let mv = uci(c, text);
        last = if c.position().turn() == c.player_color() {
            c.player_move(mv, t)
        } else {
            c.opponent_moved(Some(mv), None, t, None)
        };
    }
    last
}

pub fn mainline_ucis(game: &Game) -> Vec<String> {
    let mut pos = game.start_position().clone();
    game.mainline()
        .filter(|id| *id != game.root())
        .map(|id| {
            let mv = game.mv(id).expect("mv");
            let u = pos.uci(mv);
            pos.play(mv).expect("legal");
            u
        })
        .collect()
}

pub fn comment_at_tip(c: &PlayController) -> Option<String> {
    c.game().comment(c.tip()).expect("node").map(str::to_owned)
}

// ---------------------------------------------------------------------
// Books
// ---------------------------------------------------------------------

/// A line of the book: UCI moves from the standard start; every position of
/// the line gets the next move with `weight`.
pub fn book_with_lines(lines: &[(&[&str], u16)], selection: BookSelection) -> PlayBook {
    use std::collections::BTreeMap;
    let mut per_position: BTreeMap<String, (Position, Vec<(Move, u16)>)> = BTreeMap::new();
    for (line, weight) in lines {
        let mut pos = Position::new();
        for text in *line {
            let mv = pos.parse_uci(text).expect("uci");
            let entry = per_position
                .entry(pos.fen())
                .or_insert_with(|| (pos.clone(), Vec::new()));
            if !entry.1.iter().any(|(m, _)| *m == mv) {
                entry.1.push((mv, *weight));
            }
            pos.play(mv).expect("legal");
        }
    }
    let mut book = Book::new();
    for (_, (pos, moves)) in per_position {
        book.set_position_moves(&pos, &moves).expect("set");
    }
    PlayBook::new(book, selection)
}

/// `e4` (70) and `d4` (30) at the start; `e5` after `e4`; `Nf3` after that.
pub fn mini_book(selection: BookSelection) -> PlayBook {
    book_with_lines(
        &[(&["e2e4", "e7e5", "g1f3"], 70), (&["d2d4"], 30)],
        selection,
    )
}

// ---------------------------------------------------------------------
// The AppState glue
// ---------------------------------------------------------------------

pub fn identity(options: Vec<EngineOption>) -> EngineIdentity {
    EngineIdentity {
        name: "Fake 1".to_owned(),
        author: None,
        options,
        capabilities: Capabilities::default(),
    }
}

pub fn spin(name: &str, min: i64, max: i64) -> EngineOption {
    EngineOption {
        name: name.to_owned(),
        kind: OptionKind::Spin {
            default: min,
            min,
            max,
        },
    }
}

pub fn check(name: &str) -> EngineOption {
    EngineOption {
        name: name.to_owned(),
        kind: OptionKind::Check { default: false },
    }
}

pub fn button(name: &str) -> EngineOption {
    EngineOption {
        name: name.to_owned(),
        kind: OptionKind::Button,
    }
}

/// A UCI engine that finished its handshake, in an `AppState` that has not
/// started a game yet.
pub struct Rig {
    pub state: AppState,
    pub run: RunId,
    pub engine: EngineId,
    next_search: u64,
}

impl Rig {
    pub fn new() -> Rig {
        Rig::with_options(Vec::new())
    }

    pub fn with_options(options: Vec<EngineOption>) -> Rig {
        let mut state = AppState::default();
        let engine = state
            .engines
            .add(EngineDraft::new("Fake", "/fake"))
            .expect("add engine");
        let fx = update(
            &mut state,
            Message::Engine(EngineMessage::Start(engine, StartMode::Analyze)),
        );
        let run = fx
            .iter()
            .find_map(|e| match e {
                Effect::StartEngine { run, .. } => Some(*run),
                _ => None,
            })
            .expect("StartEngine");
        update(
            &mut state,
            Message::Engine(EngineMessage::Started {
                run,
                identity: Box::new(identity(options)),
                at: 1_700_000_000,
            }),
        );
        Rig {
            state,
            run,
            engine,
            next_search: 1000,
        }
    }

    pub fn config(&self, color: mychess_app::ColorChoice, mode: TimeMode) -> PlayConfig {
        let mut c = PlayConfig::new(self.engine);
        c.color = color;
        c.mode = mode;
        c
    }

    pub fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        update(&mut self.state, message.into())
    }

    pub fn start(&mut self, config: PlayConfig) -> Vec<Effect> {
        self.send(PlayMessage::Start(Box::new(config)))
    }

    pub fn event(&mut self, event: EngineEvent) -> Vec<Effect> {
        let run = self.run;
        self.send(EngineMessage::Event {
            run,
            event: Box::new(event),
        })
    }

    pub fn player_move(&mut self, from: &str, to: &str, at: Instant) -> Vec<Effect> {
        self.send(PlayMessage::PlayerMove {
            from: sq(from),
            to: sq(to),
            promotion: None,
            at,
        })
    }

    /// The engine answers the search in `effects` with `uci` (`None`: no
    /// move).
    pub fn reply(&mut self, effects: &[Effect], uci: Option<&str>) -> Vec<Effect> {
        let request = play_search(effects).expect("a game search was sent");
        let mut pos = request.position.start.clone();
        for m in &request.position.moves {
            pos.play(*m).expect("legal");
        }
        let mv = uci.map(|u| pos.parse_uci(u).expect("uci"));
        self.answer(&request, mv)
    }

    pub fn answer(&mut self, request: &SearchRequest, mv: Option<Move>) -> Vec<Effect> {
        self.next_search += 1;
        let search = SearchId(self.next_search);
        self.event(EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        });
        self.event(EngineEvent::SearchStarted(search));
        self.event(EngineEvent::BestMove(BestMove {
            search,
            mv,
            ponder: None,
        }))
    }

    pub fn view(&self) -> PlayView {
        self.state.play_view(Instant::now()).expect("a game")
    }

    pub fn view_at(&self, at: Instant) -> Option<PlayView> {
        self.state.play_view(at)
    }

    pub fn phase(&self) -> PlayPhase {
        self.view().phase
    }

    pub fn notice(&self) -> Option<PlayNotice> {
        match &self.state.notice {
            Some(AppNotice::Play(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn mainline(&self) -> Vec<String> {
        mainline_ucis(self.state.session.game())
    }
}

pub fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

pub fn commands(effects: &[Effect]) -> Vec<EngineCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand { command, .. } => Some((**command).clone()),
            _ => None,
        })
        .collect()
}

/// The last search of the game against the engine: the analysis of the
/// board is infinite, the game always carries a limit.
pub fn play_search(effects: &[Effect]) -> Option<SearchRequest> {
    commands(effects).into_iter().rev().find_map(|c| match c {
        EngineCommand::Search(r)
            if r.limits.depth.is_some()
                || r.limits.nodes.is_some()
                || r.limits.movetime.is_some()
                || r.limits.clock.is_some() =>
        {
            Some(r)
        }
        _ => None,
    })
}

pub fn set_options(effects: &[Effect]) -> Vec<(String, mychess_engine::OptionValue)> {
    commands(effects)
        .into_iter()
        .filter_map(|c| match c {
            EngineCommand::SetOption { name, value } => Some((name, value)),
            _ => None,
        })
        .collect()
}

pub fn load_book_request(effects: &[Effect]) -> Option<u64> {
    effects.iter().find_map(|e| match e {
        Effect::LoadPlayBook { request, .. } => Some(*request),
        _ => None,
    })
}

pub fn exited() -> EngineEvent {
    EngineEvent::Exited(ExitInfo {
        reason: ExitReason::Crashed,
        status: None,
        stderr_tail: Vec::new(),
    })
}
