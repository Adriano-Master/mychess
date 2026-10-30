//! Helpers of the M6-1a contract tests (advanced searches). Games are built in
//! code from UCI text, with variations in parentheses, and written with
//! `Si4Writer` through the M3-3a support module (which the including test
//! file must declare as `base`). The oracles of the property tests are
//! written here, on the decoded `mychess_core::Game`, independently of the
//! piece-list walkers of the crate.
#![allow(
    clippy::assert_is_empty,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    unused_qualifications,
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    clippy::cast_possible_truncation
)]

use std::path::PathBuf;

use mychess_core::{Color, Game, NodeId, Piece, Position, Role, Square};
use mychess_db::si4::{GameFlags, Si4Database};
use mychess_db::{
    BoardSearch, CancelToken, Filter, GameSource, MaterialSearch, MovesSearch, NoProgress,
    OpenOptions, PgnDatabase, SearchReport, search_board, search_material, search_moves,
};

use crate::base::{Fixture, build};

/// A game from UCI moves; `( ... )` opens a variation that replaces the last
/// move: `"e2e4 e7e5 (c7c5 g1f3) g1f3"`.
pub fn game(spec: &str) -> Game {
    game_from(Game::new(), spec)
}

/// Like [`game`] from a FEN (the game gets its own start position).
pub fn game_fen(fen: &str, spec: &str) -> Game {
    game_from(Game::from_position(Position::from_fen(fen).unwrap()), spec)
}

fn game_from(mut g: Game, spec: &str) -> Game {
    let mut at = g.root();
    let mut prev = g.root();
    let mut stack: Vec<(NodeId, NodeId)> = Vec::new();
    let spaced = spec.replace('(', " ( ").replace(')', " ) ");
    for token in spaced.split_whitespace() {
        match token {
            "(" => {
                stack.push((at, prev));
                at = prev;
            }
            ")" => {
                (at, prev) = stack.pop().unwrap();
            }
            uci => {
                let node = crate::base::play(&mut g, at, uci);
                prev = at;
                at = node;
            }
        }
    }
    g
}

/// Writes the games to a temporary SI4 database and opens it.
pub fn db_of(games: Vec<Game>) -> (Fixture, Si4Database) {
    let list: Vec<_> = games.into_iter().map(|g| (g, GameFlags::empty())).collect();
    let fx = build(&list);
    let db = fx.open();
    (fx, db)
}

/// Opens PGN text as a database (a temporary file).
pub fn pgn_db(text: &str) -> (tempfile::TempDir, PgnDatabase) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("games.pgn");
    std::fs::write(&path, text).unwrap();
    let (db, _) = PgnDatabase::open(
        &path,
        &OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    (dir, db)
}

/// A fixture of `tests/fixtures`.
pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub fn pgn_fixture(name: &str) -> PgnDatabase {
    let (db, _) = PgnDatabase::open(
        &fixture_path(name),
        &OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    db
}

pub fn pos_after(ucis: &str) -> Position {
    let mut pos = Position::new();
    for u in ucis.split_whitespace() {
        let mv = pos.parse_uci(u).unwrap();
        pos.play(mv).unwrap();
    }
    pos
}

/// `(game number, filter value)` of the games in the filter.
pub fn values(filter: &Filter) -> Vec<(u32, u8)> {
    filter.iter_games().map(|n| (n, filter.get(n))).collect()
}

pub fn numbers(filter: &Filter) -> Vec<u32> {
    filter.iter_games().collect()
}

pub fn run_board<D: GameSource + ?Sized>(db: &mut D, q: &BoardSearch) -> (Filter, SearchReport) {
    let mut filter = Filter::new(db.len());
    let report = search_board(db, q, &mut filter, &mut NoProgress, &CancelToken::new()).unwrap();
    (filter, report)
}

pub fn run_material<D: GameSource + ?Sized>(
    db: &mut D,
    q: &MaterialSearch,
) -> (Filter, SearchReport) {
    let mut filter = Filter::new(db.len());
    let report = search_material(db, q, &mut filter, &mut NoProgress, &CancelToken::new()).unwrap();
    (filter, report)
}

pub fn run_moves<D: GameSource + ?Sized>(db: &mut D, q: &MovesSearch) -> (Filter, SearchReport) {
    let mut filter = Filter::new(db.len());
    let report = search_moves(db, q, &mut filter, &mut NoProgress, &CancelToken::new()).unwrap();
    (filter, report)
}

// ------------------------------------------------------------- oracles

/// The positions of the main line of a decoded game, with their ply.
pub fn mainline_positions(game: &Game) -> Vec<Position> {
    let mut out = vec![game.start_position().clone()];
    let mut at = game.root();
    while let Ok(children) = game.children(at) {
        let Some(&next) = children.first() else { break };
        out.push(game.position_at(next).unwrap().clone());
        at = next;
    }
    out
}

/// The `(file, rank)` index of a square: 0..64.
pub fn sq_index(sq: Square) -> usize {
    sq.to_usize()
}

/// Counts of `(piece kind, colour)` of a position, kings included.
pub fn counts(pos: &Position) -> [[u8; 6]; 2] {
    let mut c = [[0u8; 6]; 2];
    for (_, p) in pos.pieces() {
        let r = match p.role {
            Role::Pawn => 0,
            Role::Knight => 1,
            Role::Bishop => 2,
            Role::Rook => 3,
            Role::Queen => 4,
            Role::King => 5,
        };
        c[usize::from(p.color == Color::Black)][r] += 1;
    }
    c
}

/// The board of a position as 64 optional pieces.
pub fn board(pos: &Position) -> [Option<Piece>; 64] {
    let mut b = [None; 64];
    for (sq, p) in pos.pieces() {
        b[sq_index(sq)] = Some(p);
    }
    b
}

/// The position with the colours swapped: ranks mirrored, colours and side
/// to move exchanged.
pub fn flipped_board(pos: &Position) -> ([Option<Piece>; 64], Color) {
    let mut b = [None; 64];
    for (sq, p) in pos.pieces() {
        let to = sq.flip_vertical();
        b[sq_index(to)] = Some(Piece {
            color: !p.color,
            role: p.role,
        });
    }
    (b, !pos.turn())
}

// ------------------------------------------------------- board oracle

use mychess_db::BoardMatch;

/// Squares and side to move of a position.
#[derive(Clone)]
pub struct Snap {
    pub board: [Option<Piece>; 64],
    pub turn: Color,
}

pub fn snap(pos: &Position) -> Snap {
    Snap {
        board: board(pos),
        turn: pos.turn(),
    }
}

pub fn snap_flipped(pos: &Position) -> Snap {
    let (board, turn) = flipped_board(pos);
    Snap { board, turn }
}

fn snap_counts(s: &Snap) -> [[u8; 6]; 2] {
    let mut c = [[0u8; 6]; 2];
    for p in s.board.iter().flatten() {
        let r = match p.role {
            Role::Pawn => 0,
            Role::Knight => 1,
            Role::Bishop => 2,
            Role::Rook => 3,
            Role::Queen => 4,
            Role::King => 5,
        };
        c[usize::from(p.color == Color::Black)][r] += 1;
    }
    c
}

fn pawn_files(s: &Snap) -> [[u8; 8]; 2] {
    let mut f = [[0u8; 8]; 2];
    for (i, p) in s.board.iter().enumerate() {
        if let Some(p) = p
            && p.role == Role::Pawn
        {
            f[usize::from(p.color == Color::Black)][i % 8] += 1;
        }
    }
    f
}

/// The rules of the spec (comportamento 8): same side to move, same count of
/// every kind, then the squares as the kind asks.
pub fn board_hit(kind: BoardMatch, target: &Snap, cur: &Snap) -> bool {
    if target.turn != cur.turn || snap_counts(target) != snap_counts(cur) {
        return false;
    }
    match kind {
        BoardMatch::Exact => target.board == cur.board,
        BoardMatch::Pawns => {
            let pawns = |s: &Snap| -> Vec<Option<Piece>> {
                s.board
                    .iter()
                    .map(|p| p.filter(|p| p.role == Role::Pawn))
                    .collect()
            };
            pawns(target) == pawns(cur)
        }
        BoardMatch::Files => pawn_files(target) == pawn_files(cur),
        BoardMatch::Material => true,
    }
}

/// The first hit of `check` in the depth-first order of `VarExactMatch`:
/// the position, the variations of the move played from it, then the move.
/// A hit inside a variation reports the ply where the outermost one starts.
fn dfs(
    game: &Game,
    node: NodeId,
    depth: u32,
    origin: Option<u32>,
    check: &dyn Fn(&Position) -> bool,
) -> Option<u32> {
    if check(game.position_at(node).unwrap()) {
        return Some(origin.unwrap_or(depth));
    }
    let children = game.children(node).unwrap();
    for &var in children.iter().skip(1) {
        if let Some(hit) = dfs(game, var, depth + 1, origin.or(Some(depth)), check) {
            return Some(hit);
        }
    }
    children
        .first()
        .and_then(|&next| dfs(game, next, depth + 1, origin, check))
}

/// The expected filter value of `game` for a position search, written on the
/// decoded game: main line normal, main line flipped, then (if asked, and if
/// the game has variations) the same in the variations.
pub fn oracle_board(
    game: &Game,
    kind: BoardMatch,
    target: &Position,
    flip: bool,
    vars: bool,
) -> Option<u8> {
    let targets: Vec<Snap> = if flip {
        vec![snap(target), snap_flipped(target)]
    } else {
        vec![snap(target)]
    };
    let value = |ply: u32| u8::try_from(ply + 1).unwrap_or(255);
    let main = mainline_positions(game);
    for t in &targets {
        if let Some(i) = main.iter().position(|p| board_hit(kind, t, &snap(p))) {
            return Some(value(i as u32));
        }
    }
    let has_variations = {
        let mut any = false;
        let mut stack = vec![game.root()];
        while let Some(n) = stack.pop() {
            let ch = game.children(n).unwrap();
            any |= ch.len() > 1;
            stack.extend_from_slice(ch);
        }
        any
    };
    if vars && has_variations {
        for t in &targets {
            let check = |p: &Position| board_hit(kind, t, &snap(p));
            if let Some(ply) = dfs(game, game.root(), 0, None, &check) {
                return Some(value(ply));
            }
        }
    }
    None
}

// ------------------------------------------------------- random games

use proptest::prelude::*;

const STARTS: [Option<&str>; 6] = [
    None,
    None,
    None,
    Some("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1"),
    Some("r3k2r/pppq1ppp/2n5/3pP3/8/2N5/PPP2PPP/R2QK2R w KQkq d6 0 1"),
    Some("7k/P6P/8/8/8/8/p6p/K7 w - - 0 1"),
];

fn random_game(start: usize, choices: &[u8], variations: &[(u8, u8, usize)]) -> Game {
    let mut g = match STARTS.get(start).copied().flatten() {
        Some(fen) => Game::from_position(Position::from_fen(fen).unwrap()),
        None => Game::new(),
    };
    let mut at = g.root();
    let mut nodes = vec![at];
    for &c in choices {
        let moves = g.position_at(at).unwrap().legal_moves();
        if moves.is_empty() {
            break;
        }
        let mv = moves[usize::from(c) % moves.len()];
        at = g.add_move(at, mv).unwrap();
        nodes.push(at);
    }
    for &(i, c, extra) in variations {
        let parent = nodes[usize::from(i) % nodes.len()];
        let taken: Vec<_> = g
            .children(parent)
            .unwrap()
            .iter()
            .map(|&n| g.mv(n).unwrap())
            .collect();
        let moves: Vec<_> = g
            .position_at(parent)
            .unwrap()
            .legal_moves()
            .into_iter()
            .filter(|m| !taken.contains(m))
            .collect();
        if moves.is_empty() {
            continue;
        }
        let mut at = g
            .add_move(parent, moves[usize::from(c) % moves.len()])
            .unwrap();
        for k in 0..extra {
            let moves = g.position_at(at).unwrap().legal_moves();
            if moves.is_empty() {
                break;
            }
            at = g
                .add_move(at, moves[(usize::from(c) + k * 7) % moves.len()])
                .unwrap();
        }
    }
    g
}

/// A random legal game: from the standard position or from one of a few
/// FENs, up to 60 plies, with up to 3 variations.
pub fn arb_game() -> impl Strategy<Value = Game> {
    (
        0usize..STARTS.len(),
        prop::collection::vec(any::<u8>(), 0..60),
        prop::collection::vec((any::<u8>(), any::<u8>(), 0usize..6), 0..3),
    )
        .prop_map(|(start, choices, vars)| random_game(start, &choices, &vars))
}

/// Between 1 and `max` random games.
pub fn arb_games(max: usize) -> impl Strategy<Value = Vec<Game>> {
    prop::collection::vec(arb_game(), 1..=max)
}

/// The position of `game` after `ply` main-line moves (limited to its end).
pub fn position_in(game: &Game, ply: usize) -> Position {
    let all = mainline_positions(game);
    all[ply % all.len()].clone()
}

// ---------------------------------------------------- material oracle

use mychess_db::{BishopColors, Pattern, PieceRange, SideRanges};

fn role_counts(pos: &Position) -> [[u8; 6]; 2] {
    // pawn, knight, bishop, rook, queen, king by colour
    counts(pos)
}

fn range_ok(range: PieceRange, n: u8) -> bool {
    n >= range.min && n <= range.max
}

fn side_ok(r: &SideRanges, c: &[u8; 6]) -> bool {
    range_ok(r.pawn, c[0])
        && range_ok(r.knight, c[1])
        && range_ok(r.bishop, c[2])
        && range_ok(r.rook, c[3])
        && range_ok(r.queen, c[4])
        && range_ok(r.minor, c[1] + c[2])
}

fn material_value(c: &[u8; 6]) -> i32 {
    i32::from(c[0])
        + 3 * i32::from(c[1])
        + 3 * i32::from(c[2])
        + 5 * i32::from(c[3])
        + 9 * i32::from(c[4])
}

fn pattern_flipped(p: &Pattern) -> Pattern {
    Pattern {
        piece: p.piece.map(|p| Piece {
            color: !p.color,
            role: p.role,
        }),
        file: p.file,
        rank: p.rank.map(mychess_core::Rank::flip_vertical),
        present: p.present,
    }
}

fn pattern_ok(p: &Pattern, pos: &Position) -> bool {
    let Some(piece) = p.piece else { return true };
    let found = pos.pieces().any(|(sq, q)| {
        q == piece && p.file.is_none_or(|f| sq.file() == f) && p.rank.is_none_or(|r| sq.rank() == r)
    });
    found == p.present
}

fn bishop_light(pos: &Position, color: Color) -> Option<bool> {
    pos.pieces()
        .find(|(_, p)| p.color == color && p.role == Role::Bishop)
        .map(|(sq, _)| sq.is_light())
}

/// The expected filter value of `game` for a material search, from the rules
/// of the spec (comportamento 20 to 24, D3215 to D3223), written on the
/// decoded positions.
pub fn oracle_material(game: &Game, query: &MaterialSearch) -> Option<u8> {
    let q = query.normalized();
    let positions = mainline_positions(game);
    let passes = if q.ignore_colors { 2 } else { 1 };
    for pass in 0..passes {
        let (white, black) = if pass == 0 {
            (q.white, q.black)
        } else {
            (q.black, q.white)
        };
        let patterns: Vec<Pattern> = q
            .patterns
            .iter()
            .map(|p| if pass == 0 { *p } else { pattern_flipped(p) })
            .collect();
        let diff = q.diff.map(|(a, b)| {
            let (a, b) = (i32::from(a.min(b)), i32::from(a.max(b)));
            if pass == 0 { (a, b) } else { (-b, -a) }
        });
        let sat = |pos: &Position| {
            let c = role_counts(pos);
            if !side_ok(&white, &c[0]) || !side_ok(&black, &c[1]) {
                return false;
            }
            if q.bishops != BishopColors::Either
                && c[0][2] == 1
                && c[1][2] == 1
                && let (Some(w), Some(b)) = (
                    bishop_light(pos, Color::White),
                    bishop_light(pos, Color::Black),
                )
                && ((q.bishops == BishopColors::Same) != (w == b))
            {
                return false;
            }
            if let Some((lo, hi)) = diff {
                let d = material_value(&c[0]) - material_value(&c[1]);
                if d < lo || d > hi {
                    return false;
                }
            }
            patterns.iter().all(|p| pattern_ok(p, pos))
        };
        if q.end_only {
            let last = positions.len() - 1;
            if last > 0 && sat(&positions[last]) {
                return Some(u8::try_from(last + 1).unwrap_or(255));
            }
            continue;
        }
        let lo = if q.moves.0 == 0 {
            0
        } else {
            usize::from(q.moves.0) * 2 - 1
        };
        let hi = usize::from(q.moves.1) * 2;
        let need = usize::from(q.min_plies).max(1);
        let (mut run, mut start) = (0usize, 0usize);
        for (p, pos) in positions.iter().enumerate() {
            if p > hi {
                break;
            }
            if p < lo {
                run = 0;
                continue;
            }
            if sat(pos) {
                if run == 0 {
                    start = p;
                }
                run += 1;
                if run >= need {
                    return Some(u8::try_from(start + 1).unwrap_or(255));
                }
            } else {
                run = 0;
            }
        }
    }
    None
}

/// A material search around `pos`: the ranges of the position, widened by
/// random slack, plus a few random patterns taken from it.
pub fn material_around(pos: &Position, seed: &[u8]) -> MaterialSearch {
    let mut q = MaterialSearch::from_position(pos);
    let at = |i: usize| usize::from(seed.get(i).copied().unwrap_or(0));
    for (i, side) in [&mut q.white, &mut q.black].into_iter().enumerate() {
        for (j, role) in [
            Role::Queen,
            Role::Rook,
            Role::Bishop,
            Role::Knight,
            Role::Pawn,
        ]
        .into_iter()
        .enumerate()
        {
            let k = i * 5 + j;
            if let Some(r) = side.get_mut(role) {
                let (lo, hi) = (at(k) % 3, at(k + 10) % 3);
                r.min = r.min.saturating_sub(lo as u8);
                r.max = r.max.saturating_add(hi as u8);
            }
        }
    }
    q.moves = (
        u16::from(seed.get(20).copied().unwrap_or(1) % 3),
        999 - u16::from(seed.get(21).copied().unwrap_or(0) % 2) * 997,
    );
    q.min_plies = u16::from(seed.get(22).copied().unwrap_or(1) % 4);
    q.end_only = at(23) % 6 == 0;
    q.ignore_colors = at(24) % 2 == 0;
    q.bishops = [
        BishopColors::Either,
        BishopColors::Same,
        BishopColors::Opposite,
    ][at(25) % 3];
    if at(26) % 3 == 0 {
        let d = (at(27) % 5) as i16;
        q.diff = Some((-d - 1, d + 1));
    }
    let pieces: Vec<_> = pos.pieces().collect();
    for k in 0..(at(28) % 3) {
        if pieces.is_empty() {
            break;
        }
        let (sq, piece) = pieces[at(29 + k) % pieces.len()];
        let present = at(32 + k) % 4 != 0;
        let (file, rank) = match at(35 + k) % 3 {
            0 => (Some(sq.file()), Some(sq.rank())),
            1 => (Some(sq.file()), None),
            _ => (None, Some(sq.rank())),
        };
        q.patterns.push(Pattern {
            piece: Some(piece),
            file,
            rank,
            present,
        });
    }
    q
}

// ------------------------------------------------------- moves oracle

use mychess_db::{CheckTest, MoveToken, SideFilter};

/// `(side to move, SAN)` of each move of a path of nodes, with the suffix
/// style of the check test.
fn path_sans(game: &Game, path: &[NodeId], test: CheckTest) -> Vec<(Color, String)> {
    let suffix = match test {
        CheckTest::No => mychess_core::SanSuffix::None,
        CheckTest::Check => mychess_core::SanSuffix::Check,
        CheckTest::Mate => mychess_core::SanSuffix::CheckOrMate,
    };
    let mut out = Vec::new();
    let mut before = game.root();
    for &node in path {
        let pos = game.position_at(before).unwrap();
        out.push((
            pos.turn(),
            pos.san_with(game.mv(node).unwrap(), suffix).unwrap(),
        ));
        before = node;
    }
    out
}

fn mainline_path(game: &Game) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut at = game.root();
    while let Some(&next) = game.children(at).unwrap().first() {
        out.push(next);
        at = next;
    }
    out
}

fn all_paths(game: &Game) -> Vec<Vec<NodeId>> {
    fn walk(game: &Game, at: NodeId, acc: &mut Vec<NodeId>, out: &mut Vec<Vec<NodeId>>) {
        let children = game.children(at).unwrap();
        if children.is_empty() {
            out.push(acc.clone());
        }
        for &c in children {
            acc.push(c);
            walk(game, c, acc, out);
            acc.pop();
        }
    }
    let mut out = Vec::new();
    walk(game, game.root(), &mut Vec::new(), &mut out);
    out
}

fn first_start(sans: &[(Color, String)], q: &mychess_db::MovesSearch) -> Option<usize> {
    let last = usize::from(q.start_plies.1).min(sans.len().checked_sub(1)?);
    (usize::from(q.start_plies.0)..=last).find(|&start| {
        let allowed = match q.side {
            SideFilter::Both => true,
            SideFilter::White => sans[start].0 == Color::White,
            SideFilter::Black => sans[start].0 == Color::Black,
        };
        allowed
            && q.tokens
                .iter()
                .enumerate()
                .all(|(j, t)| match (t, sans.get(start + j)) {
                    (_, None) => false,
                    (MoveToken::Any, Some(_)) => true,
                    (MoveToken::San(text), Some((_, san))) => text == san,
                })
    })
}

/// What a search by moves must answer for `game`: the filter value if the
/// main line has the sequence, and whether any line (variations included)
/// has it. All the SAN of every move are compared (no pre-test).
pub fn oracle_moves(game: &Game, q: &mychess_db::MovesSearch) -> (Option<u8>, bool) {
    let main = path_sans(game, &mainline_path(game), q.check_test);
    let value = first_start(&main, q).map(|s| u8::try_from(s + 2).unwrap_or(255));
    let any = all_paths(game)
        .iter()
        .any(|p| first_start(&path_sans(game, p, q.check_test), q).is_some());
    (value, any)
}

/// The SAN of the main line with the suffix style of `test`.
pub fn mainline_san_list(game: &Game, test: CheckTest) -> Vec<String> {
    path_sans(game, &mainline_path(game), test)
        .into_iter()
        .map(|(_, s)| s)
        .collect()
}
