//! Shared helpers of the `m2_1_*` integration tests (spec `m2-1-edicao-de-partida.md`).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use mychess_app::{GameMessage, GameSession, Nav, PlayOptions, ReplaceChoice};
pub use mychess_core::{Game, Move, Nag, NodeId, Position, Role, Square};

/// Adds `uci` below `parent` in `game` and returns the new node.
pub fn add(game: &mut Game, parent: NodeId, uci: &str) -> NodeId {
    let mv = game.position_at(parent).unwrap().parse_uci(uci).unwrap();
    game.add_move(parent, mv).unwrap()
}

/// A main line from the standard start.
pub fn line(ucis: &[&str]) -> Game {
    let mut game = Game::new();
    let mut cur = game.root();
    for uci in ucis {
        cur = add(&mut game, cur, uci);
    }
    game
}

/// Named nodes of [`nested_game`].
///
/// ```text
/// main:  1.e4(m1) e5(m2) 2.Nf3(m3) Nc6(m4) 3.Bb5(m5) a6(m6)
/// var A: 1...c5(a1) 2.Nf3(a2) d6(a3)            -- child of m1
/// var B: 2.Nc3(b1) Nc6(b2)                      -- child of a1 (inside A)
/// var C: 2...e6(c1) 3.g3(c2)                    -- child of b1 (inside B, 3rd level)
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Nested {
    pub m1: NodeId,
    pub m2: NodeId,
    pub m3: NodeId,
    pub m4: NodeId,
    pub m5: NodeId,
    pub m6: NodeId,
    pub a1: NodeId,
    pub a2: NodeId,
    pub a3: NodeId,
    pub b1: NodeId,
    pub b2: NodeId,
    pub c1: NodeId,
    pub c2: NodeId,
}

pub fn nested_game() -> (Game, Nested) {
    let mut g = Game::new();
    let root = g.root();
    let m1 = add(&mut g, root, "e2e4");
    let m2 = add(&mut g, m1, "e7e5");
    let m3 = add(&mut g, m2, "g1f3");
    let m4 = add(&mut g, m3, "b8c6");
    let m5 = add(&mut g, m4, "f1b5");
    let m6 = add(&mut g, m5, "a7a6");
    let a1 = add(&mut g, m1, "c7c5");
    let a2 = add(&mut g, a1, "g1f3");
    let a3 = add(&mut g, a2, "d7d6");
    let b1 = add(&mut g, a1, "b1c3");
    let b2 = add(&mut g, b1, "b8c6");
    let c1 = add(&mut g, b1, "e7e6");
    let c2 = add(&mut g, c1, "g2g3");
    (
        g,
        Nested {
            m1,
            m2,
            m3,
            m4,
            m5,
            m6,
            a1,
            a2,
            a3,
            b1,
            b2,
            c1,
            c2,
        },
    )
}

pub fn nested_session() -> (GameSession, Nested) {
    let (g, n) = nested_game();
    (GameSession::new(g), n)
}

pub fn nav(s: &mut GameSession, n: Nav) {
    s.update(GameMessage::Navigate(n));
}

pub fn goto(s: &mut GameSession, node: NodeId) {
    nav(s, Nav::GoToNode(node));
    assert_eq!(s.cursor(), node, "setup: GoToNode failed");
}

pub fn mv_at(game: &Game, node: NodeId, uci: &str) -> Move {
    game.position_at(node).unwrap().parse_uci(uci).unwrap()
}

/// Plays `uci` at the cursor with default options.
pub fn play(s: &mut GameSession, uci: &str) {
    let mv = mv_at(s.game(), s.cursor(), uci);
    s.update(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
}

pub fn play_forced(s: &mut GameSession, uci: &str) {
    let mv = mv_at(s.game(), s.cursor(), uci);
    s.update(GameMessage::PlayMove {
        mv,
        options: PlayOptions {
            force_variation: true,
        },
    });
}

pub fn answer(s: &mut GameSession, c: ReplaceChoice) {
    s.update(GameMessage::AnswerReplace(c));
}

pub fn children(s: &GameSession, n: NodeId) -> Vec<NodeId> {
    s.game().children(n).unwrap().to_vec()
}

pub fn sq(name: &str) -> Square {
    name.parse().unwrap()
}

pub fn nag(n: u16) -> Nag {
    Nag::new(n).unwrap()
}

/// Session on `fen`.
pub fn session_fen(fen: &str) -> GameSession {
    GameSession::new(Game::from_position(Position::from_fen(fen).unwrap()))
}

pub fn send(s: &mut GameSession, m: GameMessage) {
    s.update(m);
}

/// Types every char of `text` into the keyboard entry.
pub fn type_text(s: &mut GameSession, text: &str) {
    for ch in text.chars() {
        s.update(GameMessage::MoveEntryChar(ch));
    }
}
