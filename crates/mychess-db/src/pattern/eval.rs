//! The pattern evaluator (spec M6-2, D3252-D3259, D3257, D3271, D3275).
//!
//! A [`Line`] is the main line of a game as positions. [`eval_position`]
//! evaluates a condition tree at one position of it with three-state logic
//! ([`Tri`]); [`match_line`] applies the scope rules (range, `hold`,
//! `end_only`, mirror); [`highlights_at`] explains an occurrence with arrows
//! and squares. Everything is pure and deterministic.

use mychess_core::{
    Color, Game, GameResult, Move, Position, Role, Square, tactics, tactics::GamePhase,
};

use super::model::{
    BishopColors, CastleKind, Condition, Counts, DiscoveredCond, DiscoveredKind, Feature,
    FeatureCond, Gain, MAX_DEPTH, MateCond, MateMode, MaterialCond, MinMax, MoveCond, NamedRegion,
    Outcome, Pattern, Phase, PieceCond, Region, RelKind, RelationCond, ResultKind, RoleName,
    SacrificeCond, Scope, Side, Status, TargetSel, When, WhichMove, Who,
};
use crate::progress::CancelToken;

/// A three-state truth value (Kleene logic). Only [`Tri::True`] matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tri {
    /// Holds.
    True,
    /// Does not hold.
    False,
    /// Could not be decided (a mate search ran out of nodes).
    Unknown,
}

impl Tri {
    /// From a plain boolean.
    pub const fn from_bool(b: bool) -> Tri {
        if b { Tri::True } else { Tri::False }
    }

    /// Kleene negation: `Not(Unknown) = Unknown`.
    #[must_use]
    pub const fn not(self) -> Tri {
        match self {
            Tri::True => Tri::False,
            Tri::False => Tri::True,
            Tri::Unknown => Tri::Unknown,
        }
    }

    /// Whether this is [`Tri::True`].
    pub const fn is_true(self) -> bool {
        matches!(self, Tri::True)
    }
}

/// One position of the line.
#[derive(Debug, Clone)]
pub struct Frame {
    /// The position.
    pub pos: Position,
    /// The move that led here (`None` for the initial position).
    pub mv: Option<Move>,
    /// Material balance, White minus Black (Q9 R5 B3 N3 P1).
    pub balance: i16,
}

/// The main line of a game as positions.
#[derive(Debug, Clone)]
pub struct Line {
    /// `frames[p]` is the position after `p` plies; `frames[0]` is the initial one.
    pub frames: Vec<Frame>,
    /// The result of the game.
    pub result: ResultKind,
    /// The line was cut, either at the frame limit or because a position or
    /// move of the game could not be read (see [`Line::broken`]). The last
    /// frame is then not the end of the game.
    pub truncated: bool,
    /// The game could not be followed to its end because a position or move
    /// failed (always implies `truncated`). The scan of a database (slice b)
    /// counts such a game as `unreadable`; `ends_in_mate` and the `played`
    /// mate mode never hold on a broken line.
    pub broken: bool,
    /// The last frame is a checkmate (and the line is complete).
    pub ends_in_mate: bool,
}

/// The most frames kept for a game.
pub const MAX_FRAMES: usize = 2048;

fn result_kind(r: GameResult) -> ResultKind {
    match r {
        GameResult::White => ResultKind::WhiteWins,
        GameResult::Black => ResultKind::BlackWins,
        GameResult::Draw => ResultKind::Draw,
        _ => ResultKind::Unknown,
    }
}

fn balance_of(pos: &Position) -> i16 {
    i16::try_from(tactics::material_balance(pos)).unwrap_or(0)
}

impl Line {
    /// Builds the main line of `game`, with at most `max_frames` positions
    /// (initial one included; more are cut and `truncated` is set). If a
    /// position or move of the main line fails, the line stops there with
    /// `truncated` and `broken` set.
    pub fn from_game(game: &Game, max_frames: usize) -> Line {
        let limit = max_frames.max(1);
        let start = game.start_position().clone();
        let mut frames = vec![Frame {
            balance: balance_of(&start),
            pos: start,
            mv: None,
        }];
        let mut truncated = false;
        let mut broken = false;
        for id in game.mainline() {
            if frames.len() >= limit {
                truncated = true;
                break;
            }
            let (Ok(pos), Ok(mv)) = (game.position_at(id), game.mv(id)) else {
                truncated = true;
                broken = true;
                break;
            };
            frames.push(Frame {
                balance: balance_of(pos),
                pos: pos.clone(),
                mv: Some(mv),
            });
        }
        let mut line = Line::new(frames, result_kind(game.tags().result), truncated);
        line.broken = broken;
        line
    }

    /// Builds a line from frames (the first one is the initial position).
    pub fn new(frames: Vec<Frame>, result: ResultKind, truncated: bool) -> Line {
        let ends_in_mate = !truncated && frames.last().is_some_and(|f| f.pos.is_checkmate());
        Line {
            frames,
            result,
            truncated,
            broken: false,
            ends_in_mate,
        }
    }

    /// The same game with colours swapped and the board flipped vertically;
    /// the result is swapped. `None` if a position cannot be mirrored.
    pub fn mirrored(&self) -> Option<Line> {
        let mut frames = Vec::with_capacity(self.frames.len());
        for f in &self.frames {
            frames.push(Frame {
                pos: tactics::mirror(&f.pos)?,
                mv: f.mv.map(tactics::mirror_move),
                balance: -f.balance,
            });
        }
        Some(Line {
            frames,
            result: self.result.swapped(),
            truncated: self.truncated,
            broken: self.broken,
            ends_in_mate: self.ends_in_mate,
        })
    }
}

// ---------------------------------------------------------------------------
// Highlights
// ---------------------------------------------------------------------------

/// What to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    /// An arrow between two squares.
    Arrow(Square, Square),
    /// A highlighted square.
    Square(Square),
}

/// The role of a highlight in the pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightRole {
    /// The acting piece.
    Actor,
    /// The target.
    Target,
    /// The piece behind (pins, skewers).
    Behind,
    /// Other information.
    Info,
}

/// One thing to draw on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
    /// Arrow or square.
    pub kind: HighlightKind,
    /// Its role.
    pub role: HighlightRole,
}

impl Highlight {
    fn mirrored(self) -> Highlight {
        let kind = match self.kind {
            HighlightKind::Arrow(a, b) => {
                HighlightKind::Arrow(tactics::mirror_square(a), tactics::mirror_square(b))
            }
            HighlightKind::Square(s) => HighlightKind::Square(tactics::mirror_square(s)),
        };
        Highlight { kind, ..self }
    }
}

// ---------------------------------------------------------------------------
// Regions
// ---------------------------------------------------------------------------

const DARK: u64 = 0xAA55_AA55_AA55_AA55;

fn bit(sq: Square) -> u64 {
    1u64 << u32::from(sq)
}

fn file_mask(file: u32) -> u64 {
    0x0101_0101_0101_0101u64 << file.min(7)
}

fn rank_mask(rank: u32) -> u64 {
    0xFFu64 << (8 * rank.min(7))
}

/// The squares of `region` as a bit mask (bit `i` is square index `i`, a1 = 0),
/// for a piece owned by `owner` (only `OwnRank` depends on it). Invalid
/// regions (which validation rejects) give the empty mask.
pub fn region_mask(region: &Region, owner: Color) -> u64 {
    match region {
        Region::Anywhere => u64::MAX,
        Region::Squares(list) => list.iter().fold(0, |m, s| m | bit(s.0)),
        Region::File(c) => match *c {
            'a'..='h' => file_mask(u32::from(*c) - u32::from('a')),
            _ => 0,
        },
        Region::Rank(r) => match *r {
            1..=8 => rank_mask(u32::from(*r) - 1),
            _ => 0,
        },
        Region::OwnRank(k) => match *k {
            1..=8 => {
                let k = u32::from(*k);
                rank_mask(if owner == Color::White { k - 1 } else { 8 - k })
            }
            _ => 0,
        },
        Region::Rect { from, to } => {
            let (f1, f2) = (u32::from(from.0.file()), u32::from(to.0.file()));
            let (r1, r2) = (u32::from(from.0.rank()), u32::from(to.0.rank()));
            let mut mask = 0;
            for f in f1.min(f2)..=f1.max(f2) {
                for r in r1.min(r2)..=r1.max(r2) {
                    mask |= file_mask(f) & rank_mask(r);
                }
            }
            mask
        }
        Region::Named(n) => match n {
            NamedRegion::Center => (1 << 27) | (1 << 28) | (1 << 35) | (1 << 36),
            NamedRegion::ExtendedCenter => 0x3C3C_3C3C_3C3C_3C3C & 0x0000_FFFF_FFFF_0000,
            NamedRegion::QueenSide => 0x0F0F_0F0F_0F0F_0F0F,
            NamedRegion::KingSide => 0xF0F0_F0F0_F0F0_F0F0,
            NamedRegion::WhiteHalf => 0x0000_0000_FFFF_FFFF,
            NamedRegion::BlackHalf => 0xFFFF_FFFF_0000_0000,
            NamedRegion::LightSquares => !DARK,
            NamedRegion::DarkSquares => DARK,
            NamedRegion::Edge => 0x8181_8181_8181_8181 | 0xFF | 0xFF00_0000_0000_0000,
            NamedRegion::LongDiagonals => 0x8040_2010_0804_0201 | 0x0102_0408_1020_4080,
        },
    }
}

fn in_mask(mask: u64, sq: Square) -> bool {
    mask & bit(sq) != 0
}

fn role_ok(roles: &[RoleName], role: Role) -> bool {
    roles.is_empty() || roles.iter().any(|r| r.role() == role)
}

const fn color_of(side: Side) -> Color {
    match side {
        Side::White => Color::White,
        Side::Black => Color::Black,
    }
}

// ---------------------------------------------------------------------------
// The evaluator
// ---------------------------------------------------------------------------

/// Node cost class for ordering (D3271): 0 cheap, 1 detectors, 2 mate.
/// Meant for patterns that passed [`Pattern::validate`](super::Pattern::validate).
fn cost(c: &Condition) -> u8 {
    match c {
        Condition::All { items } | Condition::Any { items } => {
            items.iter().map(cost).max().unwrap_or(0)
        }
        Condition::Not { item } => cost(item),
        Condition::Piece(_)
        | Condition::Material(_)
        | Condition::SideToMove { .. }
        | Condition::Phase { .. }
        | Condition::Status { .. }
        | Condition::Move(_)
        | Condition::Result { .. } => 0,
        Condition::Feature(_)
        | Condition::Relation(_)
        | Condition::Discovered(_)
        | Condition::DoubleAttack(_)
        | Condition::Sacrifice(_) => 1,
        Condition::Mate(_) => 2,
    }
}

/// An instance of a relation, for `new_this_move` and highlights.
#[derive(Debug, Clone)]
struct Inst {
    actor: Square,
    targets: Vec<Square>,
    behind: Option<Square>,
}

struct Env<'a> {
    line: &'a Line,
    i: usize,
    cancel: Option<&'a CancelToken>,
    hl: Option<Vec<Highlight>>,
}

impl Env<'_> {
    fn cur(&self) -> Option<&Frame> {
        self.line.frames.get(self.i)
    }

    fn pos(&self) -> Option<&Position> {
        self.cur().map(|f| &f.pos)
    }

    fn prev(&self) -> Option<&Frame> {
        self.i.checked_sub(1).and_then(|p| self.line.frames.get(p))
    }

    fn turn(&self) -> Color {
        self.pos().map_or(Color::White, Position::turn)
    }

    fn colors(&self, who: Who) -> &'static [Color] {
        const W: &[Color] = &[Color::White];
        const B: &[Color] = &[Color::Black];
        const BOTH: &[Color] = &[Color::White, Color::Black];
        let turn = self.turn();
        match who {
            Who::White => W,
            Who::Black => B,
            Who::Any => BOTH,
            Who::Mover => {
                if turn == Color::White {
                    W
                } else {
                    B
                }
            }
            Who::Opponent => {
                if turn == Color::White {
                    B
                } else {
                    W
                }
            }
        }
    }

    fn mark(&mut self, kind: HighlightKind, role: HighlightRole) {
        if let Some(v) = &mut self.hl {
            v.push(Highlight { kind, role });
        }
    }

    fn hl_len(&self) -> usize {
        self.hl.as_ref().map_or(0, Vec::len)
    }

    fn hl_cut(&mut self, len: usize) {
        if let Some(v) = &mut self.hl {
            v.truncate(len);
        }
    }

    fn eval(&mut self, cond: &Condition, depth: usize) -> Tri {
        if depth > MAX_DEPTH + 2 || self.pos().is_none() {
            return Tri::False;
        }
        match cond {
            Condition::All { items } => self.eval_all(items, depth),
            Condition::Any { items } => self.eval_any(items, depth),
            Condition::Not { item } => {
                let len = self.hl_len();
                let r = self.eval(item, depth + 1).not();
                self.hl_cut(len);
                r
            }
            Condition::Piece(p) => Tri::from_bool(self.piece(p)),
            Condition::Material(m) => Tri::from_bool(self.material(m)),
            Condition::SideToMove { side } => Tri::from_bool(self.turn() == color_of(*side)),
            Condition::Phase { phase } => {
                let Some(pos) = self.pos() else {
                    return Tri::False;
                };
                let now = tactics::phase(pos);
                Tri::from_bool(matches!(
                    (phase, now),
                    (Phase::Opening, GamePhase::Opening)
                        | (Phase::Middlegame, GamePhase::Middlegame)
                        | (Phase::Endgame, GamePhase::Endgame)
                ))
            }
            Condition::Status { status } => {
                let Some(pos) = self.pos() else {
                    return Tri::False;
                };
                Tri::from_bool(match status {
                    Status::Check => pos.is_check(),
                    Status::Checkmate => pos.is_checkmate(),
                    Status::Stalemate => pos.is_stalemate(),
                })
            }
            Condition::Move(m) => Tri::from_bool(self.move_cond(m)),
            Condition::Result { results } => Tri::from_bool(results.contains(&self.line.result)),
            Condition::Feature(f) => Tri::from_bool(self.feature(f)),
            Condition::Relation(r) => Tri::from_bool(self.relation(r)),
            Condition::Discovered(d) => Tri::from_bool(self.discovered(d)),
            Condition::DoubleAttack(d) => Tri::from_bool(self.double_attack(d.by)),
            Condition::Sacrifice(s) => Tri::from_bool(self.sacrifice(s)),
            Condition::Mate(m) => self.mate(m),
        }
    }

    fn eval_all(&mut self, items: &[Condition], depth: usize) -> Tri {
        let start = self.hl_len();
        let mut unknown = false;
        for pass in 0..=2u8 {
            for item in items.iter().filter(|c| cost(c) == pass) {
                match self.eval(item, depth + 1) {
                    Tri::False => {
                        self.hl_cut(start);
                        return Tri::False;
                    }
                    Tri::Unknown => unknown = true,
                    Tri::True => {}
                }
            }
        }
        if unknown {
            self.hl_cut(start);
            Tri::Unknown
        } else {
            Tri::True
        }
    }

    fn eval_any(&mut self, items: &[Condition], depth: usize) -> Tri {
        let mut unknown = false;
        for pass in 0..=2u8 {
            for item in items.iter().filter(|c| cost(c) == pass) {
                let len = self.hl_len();
                match self.eval(item, depth + 1) {
                    Tri::True => return Tri::True,
                    Tri::Unknown => unknown = true,
                    Tri::False => {}
                }
                self.hl_cut(len);
            }
        }
        if unknown { Tri::Unknown } else { Tri::False }
    }

    // --- pieces and material ------------------------------------------------

    fn piece(&mut self, p: &PieceCond) -> bool {
        let Some(pos) = self.pos() else { return false };
        let mut found: Option<Vec<Square>> = None;
        for &c in self.colors(p.side) {
            let mask = region_mask(&p.region, c);
            let squares: Vec<Square> = pos
                .pieces()
                .filter(|(sq, pc)| {
                    pc.color == c && role_ok(&p.roles, pc.role) && in_mask(mask, *sq)
                })
                .map(|(sq, _)| sq)
                .collect();
            let n = squares.len();
            if n >= usize::from(p.min) && p.max.is_none_or(|m| n <= usize::from(m)) {
                found = Some(squares);
                break;
            }
        }
        match found {
            Some(squares) => {
                for sq in squares {
                    self.mark(HighlightKind::Square(sq), HighlightRole::Actor);
                }
                true
            }
            None => false,
        }
    }

    fn material(&self, m: &MaterialCond) -> bool {
        let Some(pos) = self.pos() else { return false };
        // counts[color][role index]; bishop colour bits per side (1 light, 2 dark).
        let mut counts = [[0u32; 6]; 2];
        let mut bishop_bits = [0u8; 2];
        for (sq, pc) in pos.pieces() {
            let ci = usize::from(pc.color == Color::Black);
            let ri = match pc.role {
                Role::Queen => 0,
                Role::Rook => 1,
                Role::Bishop => 2,
                Role::Knight => 3,
                Role::Pawn => 4,
                Role::King => continue,
            };
            counts[ci][ri] += 1;
            if pc.role == Role::Bishop {
                bishop_bits[ci] |= if sq.is_light() { 1 } else { 2 };
            }
        }
        let side_ok = |ci: usize, want: &Counts| {
            let c = counts[ci];
            let check = |r: &Option<MinMax<u8>>, v: u32| {
                r.is_none_or(|r| u32::from(r.min) <= v && v <= u32::from(r.max))
            };
            check(&want.queens, c[0])
                && check(&want.rooks, c[1])
                && check(&want.bishops, c[2])
                && check(&want.knights, c[3])
                && check(&want.minors, c[2] + c[3])
                && check(&want.pawns, c[4])
        };
        if !side_ok(0, &m.white) || !side_ok(1, &m.black) {
            return false;
        }
        if let Some(d) = m.diff {
            let value = |ci: usize| {
                let c = counts[ci];
                9 * c[0] + 5 * c[1] + 3 * c[2] + 3 * c[3] + c[4]
            };
            let diff = i32::try_from(value(0)).unwrap_or(0) - i32::try_from(value(1)).unwrap_or(0);
            if !d.contains(diff) {
                return false;
            }
        }
        let single = |b: u8| b == 1 || b == 2;
        match m.bishops {
            BishopColors::Either => true,
            BishopColors::Same => {
                single(bishop_bits[0]) && single(bishop_bits[1]) && bishop_bits[0] == bishop_bits[1]
            }
            BishopColors::Opposite => {
                single(bishop_bits[0]) && single(bishop_bits[1]) && bishop_bits[0] != bishop_bits[1]
            }
        }
    }

    // --- moves ----------------------------------------------------------------

    fn move_cond(&mut self, m: &MoveCond) -> bool {
        let (mv, mover, after) = match m.which {
            WhichMove::Previous => {
                let (Some(prev), Some(cur)) = (self.prev(), self.cur()) else {
                    return false;
                };
                (cur.mv, prev.pos.turn(), &cur.pos)
            }
            WhichMove::Next => {
                let Some(next) = self.line.frames.get(self.i + 1) else {
                    return false;
                };
                (next.mv, self.turn(), &next.pos)
            }
        };
        let Some(mv) = mv else { return false };
        let (Some(from), Some(to), Some(role)) = (mv.from(), mv.to(), mv.role()) else {
            return false;
        };
        if !self.colors(m.by).contains(&mover)
            || !role_ok(&m.roles, role)
            || !in_mask(region_mask(&m.from, mover), from)
            || !in_mask(region_mask(&m.to, mover), to)
            || !m.capture.allows(mv.is_capture())
            || !m.check.allows(after.is_check())
        {
            return false;
        }
        if !m.promotion.is_empty() && !mv.promotion().is_some_and(|p| role_ok(&m.promotion, p)) {
            return false;
        }
        if let Some(kind) = m.castling {
            let ok = match (kind, mv.castling_side()) {
                (_, None) => false,
                (CastleKind::Any, Some(_)) => true,
                (CastleKind::Short, Some(s)) => s == mychess_core::CastlingSide::KingSide,
                (CastleKind::Long, Some(s)) => s == mychess_core::CastlingSide::QueenSide,
            };
            if !ok {
                return false;
            }
        }
        self.mark(HighlightKind::Arrow(from, to), HighlightRole::Info);
        true
    }

    // --- structure ------------------------------------------------------------

    fn feature(&mut self, f: &FeatureCond) -> bool {
        let Some(pos) = self.pos() else { return false };
        for &c in self.colors(f.side) {
            let mask = region_mask(&f.region, c);
            let squares: Vec<Square> = match f.feature {
                Feature::PassedPawn => tactics::passed_pawns(pos, c),
                Feature::IsolatedPawn => tactics::isolated_pawns(pos, c),
                Feature::DoubledPawns => tactics::doubled_pawns(pos, c),
                Feature::ExposedKing => match pos.king_square(c) {
                    Some(k) if tactics::exposed_king(pos, c) => vec![k],
                    _ => Vec::new(),
                },
                Feature::BishopPair => {
                    let bishops: Vec<Square> = pos
                        .pieces()
                        .filter(|(sq, p)| {
                            p.color == c && p.role == Role::Bishop && in_mask(mask, *sq)
                        })
                        .map(|(sq, _)| sq)
                        .collect();
                    let light = bishops.iter().any(|s| s.is_light());
                    let dark = bishops.iter().any(|s| s.is_dark());
                    if light && dark { bishops } else { Vec::new() }
                }
            }
            .into_iter()
            .filter(|s| in_mask(mask, *s))
            .collect();
            let needed = if f.feature == Feature::BishopPair {
                1
            } else {
                usize::from(f.min)
            };
            if squares.len() >= needed && !squares.is_empty() {
                for sq in squares {
                    self.mark(HighlightKind::Square(sq), HighlightRole::Actor);
                }
                return true;
            }
        }
        false
    }

    // --- relations --------------------------------------------------------------

    fn relation(&mut self, r: &RelationCond) -> bool {
        let turn = self.turn();
        for &c in self.colors(r.actor.side) {
            let ok_when = match r.when {
                When::Any => true,
                When::ActorJustMoved => c == turn.other(),
                When::ActorToMove => c == turn,
            };
            if !ok_when {
                continue;
            }
            let (Some(cur), prev) = (self.pos(), self.prev()) else {
                return false;
            };
            let mut found = relation_instances(r, cur, c);
            if r.new_this_move {
                let Some(prev) = prev else { continue };
                let before = relation_instances(r, &prev.pos, c);
                found.retain(|inst| !existed(inst, &before, &prev.pos, cur));
            }
            if let Some(inst) = found.into_iter().next() {
                self.mark(HighlightKind::Square(inst.actor), HighlightRole::Actor);
                for t in &inst.targets {
                    self.mark(HighlightKind::Arrow(inst.actor, *t), HighlightRole::Target);
                }
                if let (Some(b), Some(t)) = (inst.behind, inst.targets.first()) {
                    self.mark(HighlightKind::Arrow(*t, b), HighlightRole::Behind);
                }
                return true;
            }
        }
        false
    }

    fn discovered(&mut self, d: &DiscoveredCond) -> bool {
        let (Some(prev), Some(cur)) = (self.prev(), self.cur()) else {
            return false;
        };
        let Some(mv) = cur.mv else { return false };
        let mover = prev.pos.turn();
        if !self.colors(d.by).contains(&mover) || !mv.role().is_some_and(|r| role_ok(&d.roles, r)) {
            return false;
        }
        let list = tactics::discovered_attacks(&prev.pos, mv, &cur.pos);
        let (ok, marks): (bool, Vec<(Square, Square)>) = match d.discovery {
            DiscoveredKind::Attack => (
                !list.is_empty(),
                list.iter().map(|x| (x.slider, x.target)).collect(),
            ),
            DiscoveredKind::Check => {
                let king = cur.pos.king_square(mover.other());
                let hits: Vec<_> = list
                    .iter()
                    .filter(|x| Some(x.target) == king)
                    .map(|x| (x.slider, x.target))
                    .collect();
                (!hits.is_empty(), hits)
            }
            DiscoveredKind::DoubleCheck => {
                let checkers = tactics::checkers(&cur.pos);
                let king = cur.pos.king_square(cur.pos.turn());
                let marks = king
                    .map(|k| checkers.iter().map(|c| (*c, k)).collect())
                    .unwrap_or_default();
                (checkers.len() >= 2, marks)
            }
        };
        if ok {
            for (a, b) in marks {
                self.mark(HighlightKind::Arrow(a, b), HighlightRole::Target);
            }
        }
        ok
    }

    fn double_attack(&mut self, by: Who) -> bool {
        let (Some(prev), Some(cur)) = (self.prev(), self.cur()) else {
            return false;
        };
        let mover = prev.pos.turn();
        if !self.colors(by).contains(&mover) {
            return false;
        }
        let victims = tactics::double_attack(&prev.pos, &cur.pos, mover);
        if victims.is_empty() {
            return false;
        }
        for v in victims {
            self.mark(HighlightKind::Square(v), HighlightRole::Target);
        }
        true
    }

    // --- sacrifice --------------------------------------------------------------

    fn sacrifice(&mut self, s: &SacrificeCond) -> bool {
        let k = self.i;
        if k == 0 {
            return false;
        }
        let frames = &self.line.frames;
        let turn = self.turn();
        if !self.colors(s.by).contains(&turn) {
            return false;
        }
        let sign = |f: &Frame| -> i32 {
            let b = i32::from(f.balance);
            if turn == Color::White { b } else { -b }
        };
        let (Some(f0), Some(f2)) = (frames.get(k), frames.get(k + 2)) else {
            return false;
        };
        let Some(reply) = f2.mv else { return false };
        let Some(given) = reply.capture() else {
            return false;
        };
        if given == Role::King || !role_ok(&s.roles, given) {
            return false;
        }
        let base = sign(f0);
        if base - sign(f2) < i32::from(s.min_net) {
            return false;
        }
        let w = usize::from(s.within);
        let recovered =
            (1..=w).any(|i2| frames.get(k + 2 + 2 * i2).is_some_and(|f| sign(f) >= base));
        let mate = (1..=w).any(|i2| {
            frames
                .get(k + 1 + 2 * i2)
                .is_some_and(|f| f.pos.is_checkmate())
        });
        let ok = match s.outcome {
            Outcome::Recovered => recovered,
            Outcome::Mate => mate,
            Outcome::Either => recovered || mate,
            Outcome::Any => true,
        };
        if ok && let Some(to) = reply.to() {
            self.mark(HighlightKind::Square(to), HighlightRole::Info);
        }
        ok
    }

    // --- mate -------------------------------------------------------------------

    fn mate(&mut self, m: &MateCond) -> Tri {
        let Some(pos) = self.pos() else {
            return Tri::False;
        };
        let turn = pos.turn();
        if !self.colors(m.by).contains(&turn) {
            return Tri::False;
        }
        let query = tactics::MateQuery {
            min_moves: m.moves_min,
            max_moves: m.moves_max,
            forcing_only: m.forcing_only,
            max_nodes: m.max_nodes,
        };
        let verdict = match m.mode {
            MateMode::Played => {
                if !self.line.ends_in_mate {
                    return Tri::False;
                }
                let last = self.line.frames.len() - 1;
                // The side mated at the end is the opponent of the one to move here
                // exactly when the distance is odd.
                let Some(distance) = last.checked_sub(self.i) else {
                    return Tri::False;
                };
                let Ok(plies) = u32::try_from(distance) else {
                    return Tri::False;
                };
                let stop = || self.cancel.is_some_and(CancelToken::is_cancelled);
                tactics::mate_in_played_with(pos, plies, query, Some(&stop))
            }
            MateMode::Available => {
                let stop = || self.cancel.is_some_and(CancelToken::is_cancelled);
                if stop() {
                    return Tri::Unknown;
                }
                tactics::mate_in_available_with(pos, query, Some(&stop))
            }
        };
        match verdict {
            tactics::MateVerdict::Mate => Tri::True,
            tactics::MateVerdict::NoMate => Tri::False,
            tactics::MateVerdict::Unknown => Tri::Unknown,
        }
    }
}

/// Whether the same piece, on the same squares, already stood in the same
/// relation in the previous position.
fn existed(inst: &Inst, before: &[Inst], prev: &Position, cur: &Position) -> bool {
    let same = |sq: Square| prev.piece_at(sq) == cur.piece_at(sq);
    if !same(inst.actor) || inst.behind.is_some_and(|b| !same(b)) {
        return false;
    }
    before.iter().any(|b| {
        b.actor == inst.actor
            && b.behind == inst.behind
            && inst
                .targets
                .iter()
                .all(|t| same(*t) && b.targets.contains(t))
    })
}

/// `mask` is `region_mask(&sel.region, owner)`, computed once by the caller.
fn target_ok(sel: &TargetSel, mask: u64, pos: &Position, sq: Square, owner: Color) -> bool {
    pos.piece_at(sq)
        .is_some_and(|p| p.color == owner && role_ok(&sel.roles, p.role) && in_mask(mask, sq))
}

/// All instances of a relation for the actors of colour `c` in `pos`
/// (ignoring `when` and `new_this_move`).
#[allow(clippy::too_many_lines)] // one flat arm per relation kind reads best
fn relation_instances(r: &RelationCond, pos: &Position, c: Color) -> Vec<Inst> {
    let actor_mask = region_mask(&r.actor.region, c);
    let actors: Vec<Square> = pos
        .pieces()
        .filter(|(sq, p)| {
            p.color == c && role_ok(&r.actor.roles, p.role) && in_mask(actor_mask, *sq)
        })
        .map(|(sq, _)| sq)
        .collect();
    let enemy = c.other();
    let is_actor = |sq: Square| actors.contains(&sq);
    let mut out = Vec::new();
    match r.kind {
        RelKind::Attacks => {
            let target_mask = region_mask(&r.target.region, enemy);
            for &a in &actors {
                for (t, _) in pos.pieces().filter(|(_, p)| p.color == enemy) {
                    if !target_ok(&r.target, target_mask, pos, t, enemy)
                        || !tactics::legal_attack(pos, a, t)
                    {
                        continue;
                    }
                    let king = pos.piece_at(t).is_some_and(|p| p.role == Role::King);
                    let gain_ok = match r.gain {
                        Gain::Any => true,
                        Gain::Undefended => !king && tactics::defenders(pos, t).is_empty(),
                        Gain::WinsMaterial => tactics::worth(pos, a, t),
                    };
                    if gain_ok {
                        out.push(Inst {
                            actor: a,
                            targets: vec![t],
                            behind: None,
                        });
                    }
                }
            }
        }
        RelKind::Defends => {
            let target_mask = region_mask(&r.target.region, c);
            for (t, _) in pos.pieces().filter(|(_, p)| p.color == c) {
                if !target_ok(&r.target, target_mask, pos, t, c) {
                    continue;
                }
                for d in tactics::defenders(pos, t) {
                    if d != t && is_actor(d) {
                        out.push(Inst {
                            actor: d,
                            targets: vec![t],
                            behind: None,
                        });
                    }
                }
            }
        }
        RelKind::Pins => {
            let target_mask = region_mask(&r.target.region, enemy);
            let behind_mask = r.behind.as_ref().map(|b| region_mask(&b.region, enemy));
            for p in tactics::pins(pos, c) {
                let behind_ok = r
                    .behind
                    .as_ref()
                    .zip(behind_mask)
                    .is_none_or(|(b, m)| target_ok(b, m, pos, p.behind, enemy));
                if is_actor(p.pinner)
                    && target_ok(&r.target, target_mask, pos, p.pinned, enemy)
                    && behind_ok
                {
                    out.push(Inst {
                        actor: p.pinner,
                        targets: vec![p.pinned],
                        behind: Some(p.behind),
                    });
                }
            }
        }
        RelKind::Skewers => {
            let target_mask = region_mask(&r.target.region, enemy);
            let behind_mask = r.behind.as_ref().map(|b| region_mask(&b.region, enemy));
            for s in tactics::skewers(pos, c) {
                let behind_ok = r
                    .behind
                    .as_ref()
                    .zip(behind_mask)
                    .is_none_or(|(b, m)| target_ok(b, m, pos, s.back, enemy));
                if is_actor(s.attacker)
                    && target_ok(&r.target, target_mask, pos, s.front, enemy)
                    && behind_ok
                {
                    out.push(Inst {
                        actor: s.attacker,
                        targets: vec![s.front],
                        behind: Some(s.back),
                    });
                }
            }
        }
        RelKind::Forks => {
            let target_mask = region_mask(&r.target.region, enemy);
            for &a in &actors {
                let victims: Vec<Square> = tactics::fork_victims(pos, a)
                    .into_iter()
                    .filter(|v| target_ok(&r.target, target_mask, pos, *v, enemy))
                    .collect();
                if victims.len() >= usize::from(r.min_targets).max(2) && tactics::safe(pos, a) {
                    out.push(Inst {
                        actor: a,
                        targets: victims,
                        behind: None,
                    });
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Evaluates `cond` at position `i` of `line` (three-state). A position
/// outside the line is [`Tri::False`].
///
/// `cond` must come from a pattern that passed
/// [`Pattern::validate`](super::Pattern::validate); ranges, budgets and
/// `Who` values are not re-checked here.
pub fn eval_position(cond: &Condition, line: &Line, i: usize, cancel: Option<&CancelToken>) -> Tri {
    Env {
        line,
        i,
        cancel,
        hl: None,
    }
    .eval(cond, 1)
}

/// Decides a tree from the game result alone, when possible (the pattern
/// must have passed [`Pattern::validate`](super::Pattern::validate)): `Some(false)`
/// means the tree cannot hold in any position of a game with that result
/// (the game can be skipped without building its line), `Some(true)` that
/// it holds whatever the position, `None` that positions matter. Pure
/// three-valued folding of the `Result` leaves (D3258).
pub fn fold_constants(cond: &Condition, result: ResultKind) -> Option<bool> {
    match cond {
        Condition::Result { results } => Some(results.contains(&result)),
        Condition::Not { item } => fold_constants(item, result).map(|b| !b),
        Condition::All { items } => {
            let mut all_true = true;
            for item in items {
                match fold_constants(item, result) {
                    Some(false) => return Some(false),
                    Some(true) => {}
                    None => all_true = false,
                }
            }
            if all_true { Some(true) } else { None }
        }
        Condition::Any { items } => {
            let mut all_false = true;
            for item in items {
                match fold_constants(item, result) {
                    Some(true) => return Some(true),
                    Some(false) => {}
                    None => all_false = false,
                }
            }
            if all_false { Some(false) } else { None }
        }
        _ => None,
    }
}

/// The outcome of [`match_line`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineMatch {
    /// Plies played before the first position of the first matching run
    /// (`0` = the initial position), or `None`.
    pub ply: Option<u16>,
    /// Positions examined (both orientations counted).
    pub examined: u32,
    /// Positions in which an `Unknown` decided the answer.
    pub unknown_positions: u32,
}

fn range_of(scope: &Scope, frames: usize) -> Option<(usize, usize)> {
    let last = frames.checked_sub(1)?;
    if scope.end_only {
        return Some((last, last));
    }
    let lo = (2 * usize::from(scope.min_move)).saturating_sub(1);
    let hi = (2 * usize::from(scope.max_move)).min(last);
    (lo <= hi).then_some((lo, hi))
}

/// Scans one orientation: the starts of the runs of at least `hold`
/// consecutive true positions (stopping at the first when `first_only`).
fn scan_runs(
    pattern: &Pattern,
    line: &Line,
    cancel: Option<&CancelToken>,
    first_only: bool,
) -> (Vec<u16>, u32, u32) {
    let scope = &pattern.scope;
    let mut starts = Vec::new();
    let (mut examined, mut unknown) = (0u32, 0u32);
    if fold_constants(&pattern.root, line.result) == Some(false) {
        return (starts, examined, unknown);
    }
    // `end_only` asks about the last position of the game; a cut line does
    // not end there, so nothing can be decided (counted as unknown).
    if scope.end_only && line.truncated {
        return (starts, 1, 1);
    }
    let Some((lo, hi)) = range_of(scope, line.frames.len()) else {
        return (starts, examined, unknown);
    };
    let hold = if scope.end_only {
        1
    } else {
        usize::from(scope.hold.max(1))
    };
    let mut run_start = 0usize;
    let mut run_len = 0usize;
    for p in lo..=hi {
        // D3296: a cancelled scan stops at the next position (the solver
        // itself stops within 4096 nodes); what is left is not examined.
        if cancel.is_some_and(CancelToken::is_cancelled) {
            unknown += 1;
            break;
        }
        examined += 1;
        match eval_position(&pattern.root, line, p, cancel) {
            Tri::True => {
                if run_len == 0 {
                    run_start = p;
                }
                run_len += 1;
                if run_len == hold {
                    if let Ok(ply) = u16::try_from(run_start) {
                        starts.push(ply);
                    }
                    if first_only {
                        break;
                    }
                }
            }
            other => {
                if other == Tri::Unknown {
                    unknown += 1;
                }
                run_len = 0;
            }
        }
    }
    (starts, examined, unknown)
}

fn scan_with_mirror(
    pattern: &Pattern,
    line: &Line,
    cancel: Option<&CancelToken>,
    first_only: bool,
) -> (Vec<u16>, u32, u32) {
    let (mut starts, mut examined, mut unknown) = scan_runs(pattern, line, cancel, first_only);
    if starts.is_empty()
        && pattern.scope.ignore_colors
        && !(pattern.scope.end_only && line.truncated)
        && fold_constants(&pattern.root, line.result.swapped()) != Some(false)
        && let Some(mirror) = line.mirrored()
    {
        let (s, e, u) = scan_runs(pattern, &mirror, cancel, first_only);
        starts = s;
        examined += e;
        unknown += u;
    }
    (starts, examined, unknown)
}

/// Applies the scope rules of `pattern` to `line` and returns the first
/// matching run (D3252, D3259; rules 5-8 of the spec): positions from
/// `2*min_move - 1` (at least 0) to `2*max_move`; `hold` consecutive true
/// positions, reported at the first of them; `end_only` examines only the
/// last position; with `ignore_colors`, if nothing matched, the mirrored
/// line is tried too (same ply).
pub fn match_line(pattern: &Pattern, line: &Line, cancel: Option<&CancelToken>) -> LineMatch {
    let (starts, examined, unknown) = scan_with_mirror(pattern, line, cancel, true);
    LineMatch {
        ply: starts.first().copied(),
        examined,
        unknown_positions: unknown,
    }
}

/// All occurrences in the line: the first position of every maximal run of
/// at least `hold` consecutive true positions, in ply order. With
/// `ignore_colors` the mirrored line is used only if the original has none.
pub fn matching_plies(pattern: &Pattern, line: &Line, cancel: Option<&CancelToken>) -> Vec<u16> {
    scan_with_mirror(pattern, line, cancel, false).0
}

/// The highlights that explain the pattern at position `i` (empty if it does
/// not hold there). With `ignore_colors`, when the pattern holds only in the
/// mirrored line, the squares are mirrored back.
pub fn highlights_at(pattern: &Pattern, line: &Line, i: usize) -> Vec<Highlight> {
    let run = |l: &Line| {
        let mut env = Env {
            line: l,
            i,
            cancel: None,
            hl: Some(Vec::new()),
        };
        let t = env.eval(&pattern.root, 1);
        t.is_true().then(|| env.hl.unwrap_or_default())
    };
    if let Some(found) = run(line) {
        return found;
    }
    if pattern.scope.ignore_colors
        && let Some(mirror) = line.mirrored()
        && let Some(found) = run(&mirror)
    {
        return found.into_iter().map(Highlight::mirrored).collect();
    }
    Vec::new()
}
