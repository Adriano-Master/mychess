//! M6-2a: the pattern evaluator on positions and on game lines.

#![allow(clippy::many_single_char_names, clippy::too_many_lines)]

use mychess_core::{Game, GameResult, Position, SanMode, Square};
use mychess_db::CancelToken;
use mychess_db::pattern::*;

const MATE_IN_2: &str = include_str!("m6_2a_fixtures/mate_in_2.toml");
const KNIGHT_FORK: &str = include_str!("m6_2a_fixtures/knight_fork.toml");
const ABSOLUTE_PIN: &str = include_str!("m6_2a_fixtures/absolute_pin.toml");
const QUEEN_SACRIFICE: &str = include_str!("m6_2a_fixtures/queen_sacrifice.toml");

const LEGAL: &str = "e4 e5 Nf3 d6 Bc4 Bg4 Nc3 g6 Nxe5 Bxd1 Bxf7+ Ke7 Nd5#";
const SCHOLAR: &str = "e4 e5 Bc4 Nc6 Qh5 Nf6 Qxf7#";

fn pat(text: &str) -> Pattern {
    PatternFile::from_toml_str(text).expect("fixture").pattern
}

fn game(fen: Option<&str>, sans: &str, result: GameResult) -> Game {
    let start = fen.map_or_else(Position::new, |f| Position::from_fen(f).expect("fen"));
    let mut g = Game::from_position(start);
    let mut node = g.root();
    for san in sans.split_whitespace() {
        let pos = g.position_at(node).expect("node").clone();
        let mv = pos.parse_san(san, SanMode::Strict).expect("san").mv;
        node = g.add_move(node, mv).expect("legal");
    }
    g.tags_mut().result = result;
    g
}

fn line(fen: Option<&str>, sans: &str, result: GameResult) -> Line {
    Line::from_game(&game(fen, sans, result), MAX_FRAMES)
}

fn root(cond: Condition) -> Pattern {
    Pattern {
        name: "t".into(),
        description: String::new(),
        scope: Scope::default(),
        root: cond,
    }
}

fn ply(p: &Pattern, l: &Line) -> Option<u16> {
    match_line(p, l, None).ply
}

#[test]
fn legal_queen_sacrifice_matches_at_ply_8() {
    let l = line(None, LEGAL, GameResult::White);
    assert_eq!(l.frames.len(), 14);
    assert!(l.ends_in_mate);
    let p = pat(QUEEN_SACRIFICE);
    let m = match_line(&p, &l, None);
    assert_eq!(m.ply, Some(8));
    // The filter value is ply + 1 = 9 (slice b).
    assert_eq!(matching_plies(&p, &l, None), vec![8]);

    let variant = |by, within, outcome, min_net| {
        let mut v = p.clone();
        let Condition::All { items } = &mut v.root else {
            panic!()
        };
        let Condition::Sacrifice(s) = &mut items[1] else {
            panic!()
        };
        s.by = by;
        s.within = within;
        s.outcome = outcome;
        s.min_net = min_net;
        v
    };
    // Within 1 move the mate is too late (it comes on the 2nd).
    assert_eq!(ply(&variant(Who::White, 1, Outcome::Mate, 5), &l), None);
    assert_eq!(ply(&variant(Who::White, 2, Outcome::Mate, 5), &l), Some(8));
    // Recovery never happens on this line.
    assert_eq!(
        ply(&variant(Who::White, 3, Outcome::Recovered, 5), &l),
        None
    );
    assert_eq!(
        ply(&variant(Who::White, 3, Outcome::Either, 5), &l),
        Some(8)
    );
    assert_eq!(ply(&variant(Who::White, 3, Outcome::Any, 9), &l), None);
    assert_eq!(ply(&variant(Who::White, 3, Outcome::Any, 8), &l), Some(8));
    // By Black: no match.
    assert_eq!(ply(&variant(Who::Black, 3, Outcome::Mate, 5), &l), None);
    // The result is a constant of the game: a drawn Legal does not match.
    let drawn = line(None, LEGAL, GameResult::Draw);
    assert_eq!(fold_constants(&p.root, drawn.result), Some(false));
    assert_eq!(match_line(&p, &drawn, None).examined, 0);
}

#[test]
fn knight_fork_is_found_when_it_happens_not_before() {
    let l = line(
        Some("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1"),
        "Nc7+",
        GameResult::Unknown,
    );
    let p = pat(KNIGHT_FORK);
    assert_eq!(ply(&p, &l), Some(1));
    // Position before the fork, as the initial position of a game.
    let before = line(
        Some("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1"),
        "",
        GameResult::Unknown,
    );
    let mut p0 = p.clone();
    p0.scope.min_move = 0;
    assert_eq!(ply(&p0, &before), None);
    // The fork "stands" at p = 0 of a game that starts right after it, but
    // new_this_move needs a previous position.
    let after = line(
        Some("r3k3/2N5/8/8/8/8/8/4K3 b - - 1 1"),
        "",
        GameResult::Unknown,
    );
    assert_eq!(ply(&p0, &after), None);
    let mut no_new = p0.clone();
    let Condition::Relation(r) = &mut no_new.root else {
        panic!()
    };
    r.new_this_move = false;
    assert_eq!(ply(&no_new, &after), Some(0));
}

#[test]
fn absolute_pin_in_the_initial_position_of_a_game() {
    let l = line(
        Some("4k3/8/8/4n3/8/8/8/4RK2 w - - 0 1"),
        "",
        GameResult::Unknown,
    );
    let mut p = pat(ABSOLUTE_PIN);
    // Pattern: white pinner; target roles exclude... knight included.
    p.scope.min_move = 0;
    assert_eq!(ply(&p, &l), Some(0));
    // With min_move = 1 the empty game has nothing to look at.
    p.scope.min_move = 1;
    assert_eq!(ply(&p, &l), None);
    // Highlights: the pinner and the pinned piece.
    p.scope.min_move = 0;
    let marks = highlights_at(&p, &l, 0);
    assert!(marks.contains(&Highlight {
        kind: HighlightKind::Square(Square::E1),
        role: HighlightRole::Actor
    }));
    assert!(marks.contains(&Highlight {
        kind: HighlightKind::Arrow(Square::E1, Square::E5),
        role: HighlightRole::Target
    }));
    assert!(marks.contains(&Highlight {
        kind: HighlightKind::Arrow(Square::E5, Square::E8),
        role: HighlightRole::Behind
    }));
}

#[test]
fn mate_in_one_available_and_played() {
    let one = |mode| {
        let mut p = pat(MATE_IN_2);
        let Condition::Mate(m) = &mut p.root else {
            panic!()
        };
        m.moves_min = 1;
        m.moves_max = 1;
        m.mode = mode;
        m.forcing_only = true;
        p
    };
    let l = line(None, SCHOLAR, GameResult::White);
    // Available: the queen on h5 and the bishop on c4 threaten Qxf7# after 3...Nf6? No:
    // only at ply 6 (white to move) does Qxf7# exist.
    let available = one(MateMode::Available);
    assert_eq!(matching_plies(&available, &l, None), vec![6]);
    let played = one(MateMode::Played);
    assert_eq!(matching_plies(&played, &l, None), vec![6]);
    // A game that does not end in mate: `played` never matches, `available` does.
    let cut = line(None, "e4 e5 Bc4 Nc6 Qh5 Nf6", GameResult::Unknown);
    assert_eq!(ply(&played, &cut), None);
    assert_eq!(ply(&available, &cut), Some(6));
    // Black's turn: `by = white` is false there.
    let mut black = available.clone();
    let Condition::Mate(m) = &mut black.root else {
        panic!()
    };
    m.by = Who::Black;
    assert_eq!(ply(&black, &l), None);
}

#[test]
fn three_states_unknown_never_matches_nor_excludes() {
    // A budget of 1000 nodes cannot settle a mate in 4 from the start position.
    let hard = Condition::Mate(MateCond {
        by: Who::Any,
        moves_min: 4,
        moves_max: 4,
        mode: MateMode::Available,
        forcing_only: false,
        max_nodes: 1_000,
    });
    let l = line(None, "e4 e5", GameResult::Unknown);
    assert_eq!(eval_position(&hard, &l, 0, None), Tri::Unknown);
    let not = Condition::Not {
        item: Box::new(hard.clone()),
    };
    assert_eq!(eval_position(&not, &l, 0, None), Tri::Unknown);
    let mut p = root(not.clone());
    p.scope.min_move = 0;
    let m = match_line(&p, &l, None);
    assert_eq!(m.ply, None);
    assert!(m.unknown_positions >= 1);
    // False dominates Unknown in AND; True dominates in OR.
    let false_leaf = Condition::Phase {
        phase: Phase::Endgame,
    };
    let true_leaf = Condition::Phase {
        phase: Phase::Opening,
    };
    let all = |a: &Condition, b: &Condition| Condition::All {
        items: vec![a.clone(), b.clone()],
    };
    let any = |a: &Condition, b: &Condition| Condition::Any {
        items: vec![a.clone(), b.clone()],
    };
    assert_eq!(
        eval_position(&all(&hard, &false_leaf), &l, 0, None),
        Tri::False
    );
    assert_eq!(
        eval_position(&all(&hard, &true_leaf), &l, 0, None),
        Tri::Unknown
    );
    assert_eq!(
        eval_position(&any(&hard, &true_leaf), &l, 0, None),
        Tri::True
    );
    assert_eq!(
        eval_position(&any(&hard, &false_leaf), &l, 0, None),
        Tri::Unknown
    );
    assert_eq!(
        eval_position(
            &Condition::Not {
                item: Box::new(false_leaf)
            },
            &l,
            0,
            None
        ),
        Tri::True
    );
    // A cancelled token makes `available` searches Unknown at once.
    let cancel = CancelToken::new();
    cancel.cancel();
    let easy = line(
        Some("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1"),
        "",
        GameResult::Unknown,
    );
    let mate1 = Condition::Mate(MateCond {
        by: Who::White,
        moves_min: 1,
        moves_max: 1,
        mode: MateMode::Available,
        forcing_only: true,
        max_nodes: 20_000,
    });
    assert_eq!(eval_position(&mate1, &easy, 0, None), Tri::True);
    assert_eq!(eval_position(&mate1, &easy, 0, Some(&cancel)), Tri::Unknown);
}

#[test]
fn ignore_colors_finds_the_inverted_pattern_and_swaps_the_result() {
    // The mirror of the fork position: Black's knight forks rook and king.
    let g = Some("4k3/8/8/8/3n4/8/8/R3K3 b - - 0 1");
    let l = line(g, "Nc2+", GameResult::Black);
    let mut p = pat(KNIGHT_FORK);
    assert_eq!(ply(&p, &l), None);
    p.scope.ignore_colors = true;
    assert_eq!(ply(&p, &l), Some(1));
    // Result: a pattern for "1-0" matches a "0-1" game only with the mirror.
    let both = Pattern {
        root: Condition::All {
            items: vec![
                Condition::Result {
                    results: vec![ResultKind::WhiteWins],
                },
                p.root.clone(),
            ],
        },
        ..p.clone()
    };
    assert_eq!(ply(&both, &l), Some(1));
    let white_game = line(g, "Nc2+", GameResult::White);
    assert_eq!(ply(&both, &white_game), None);
    // Highlights come back in the original orientation.
    let marks = highlights_at(&p, &l, 1);
    assert!(marks.contains(&Highlight {
        kind: HighlightKind::Square(Square::C2),
        role: HighlightRole::Actor
    }));
    // A symmetric pattern gives the same answer with or without the mirror.
    let sym = Pattern {
        root: Condition::Phase {
            phase: Phase::Opening,
        },
        ..root(leaf_true())
    };
    let mut sym_mirror = sym.clone();
    sym_mirror.scope.ignore_colors = true;
    let opening = line(None, "e4 e5 Nf3", GameResult::Unknown);
    assert_eq!(
        match_line(&sym, &opening, None).ply,
        match_line(&sym_mirror, &opening, None).ply
    );
}

fn leaf_true() -> Condition {
    Condition::Phase {
        phase: Phase::Opening,
    }
}

#[test]
fn range_hold_and_end_only() {
    let l = line(None, "e4 e5 Nf3 Nc6 Bc4 Bc5", GameResult::Unknown); // 7 frames
    let mut p = root(leaf_true());
    // min_move 1 starts after White's first move.
    assert_eq!(ply(&p, &l), Some(1));
    p.scope.min_move = 0;
    assert_eq!(ply(&p, &l), Some(0));
    p.scope.min_move = 2;
    assert_eq!(ply(&p, &l), Some(3));
    // hold: the first position of the run is reported.
    p.scope.hold = 3;
    assert_eq!(ply(&p, &l), Some(3));
    // The run must fit in the range: positions 3..=4 only (max_move 2) are too few.
    p.scope.max_move = 2;
    assert_eq!(ply(&p, &l), None);
    p.scope.max_move = 3;
    assert_eq!(ply(&p, &l), Some(3));
    // end_only ignores the range and hold, and `next` moves are false.
    p.scope = Scope {
        end_only: true,
        hold: 7,
        min_move: 50,
        max_move: 60,
        ..Scope::default()
    };
    assert_eq!(ply(&p, &l), Some(6));
    let next = Condition::Move(MoveCond {
        which: WhichMove::Next,
        by: Who::Any,
        roles: vec![],
        from: Region::Anywhere,
        to: Region::Anywhere,
        capture: Flag::Both,
        check: Flag::Both,
        promotion: vec![],
        castling: None,
    });
    let mut q = root(next);
    q.scope.end_only = true;
    assert_eq!(ply(&q, &l), None);
    // A game with no moves: only min_move = 0 looks at the initial position.
    let empty = line(None, "", GameResult::Unknown);
    let mut p = root(leaf_true());
    assert_eq!(ply(&p, &empty), None);
    p.scope.min_move = 0;
    assert_eq!(ply(&p, &empty), Some(0));
}

#[test]
fn occurrences_are_the_starts_of_runs() {
    let l = line(None, "e4 e5 Nf3 Nc6", GameResult::Unknown);
    // White to move holds at plies 0, 2, 4; runs of length 1.
    let mut p = root(Condition::SideToMove { side: Side::White });
    p.scope.min_move = 0;
    assert_eq!(matching_plies(&p, &l, None), vec![0, 2, 4]);
    p.scope.hold = 2;
    assert_eq!(matching_plies(&p, &l, None), Vec::<u16>::new());
    // A property that holds in a long run is one occurrence.
    let mut q = root(leaf_true());
    q.scope.min_move = 0;
    assert_eq!(matching_plies(&q, &l, None), vec![0]);
}

#[test]
fn move_conditions_look_at_the_previous_and_next_move() {
    let l = line(None, "e4 e5 Nf3", GameResult::Unknown);
    let mv = |which, by, to: &str| {
        Condition::Move(MoveCond {
            which,
            by,
            roles: vec![],
            from: Region::Anywhere,
            to: Region::Squares(vec![Sq(to.parse().expect("sq"))]),
            capture: Flag::Both,
            check: Flag::Both,
            promotion: vec![],
            castling: None,
        })
    };
    let prev_e4 = mv(WhichMove::Previous, Who::White, "e4");
    assert_eq!(eval_position(&prev_e4, &l, 1, None), Tri::True);
    assert_eq!(eval_position(&prev_e4, &l, 0, None), Tri::False);
    assert_eq!(eval_position(&prev_e4, &l, 2, None), Tri::False);
    // `opponent` of the side to move is the side that has just moved.
    let prev_opp = mv(WhichMove::Previous, Who::Opponent, "e4");
    assert_eq!(eval_position(&prev_opp, &l, 1, None), Tri::True);
    let next_e5 = mv(WhichMove::Next, Who::Mover, "e5");
    assert_eq!(eval_position(&next_e5, &l, 1, None), Tri::True);
    assert_eq!(eval_position(&next_e5, &l, 3, None), Tri::False);
    // Past the end there is no next move.
    assert_eq!(eval_position(&next_e5, &l, 99, None), Tri::False);
}

#[test]
fn piece_material_and_structure_conditions() {
    let l = line(
        Some("4k3/8/8/3P4/8/8/8/R3K3 w - - 0 1"),
        "",
        GameResult::Unknown,
    );
    let piece = |side, roles: Vec<RoleName>, region, min, max| {
        Condition::Piece(PieceCond {
            side,
            roles,
            region,
            min,
            max,
        })
    };
    // A white pawn on d5 in the centre, on its own 5th rank.
    let c = piece(
        Who::White,
        vec![RoleName::Pawn],
        Region::Named(NamedRegion::Center),
        1,
        None,
    );
    assert_eq!(eval_position(&c, &l, 0, None), Tri::True);
    let c = piece(
        Who::Black,
        vec![RoleName::Pawn],
        Region::Anywhere,
        0,
        Some(0),
    );
    assert_eq!(eval_position(&c, &l, 0, None), Tri::True);
    // Black's own rank 4 is the absolute rank 5: black king is on rank 8 = own rank 1.
    let c = piece(
        Who::Black,
        vec![RoleName::King],
        Region::OwnRank(1),
        1,
        None,
    );
    assert_eq!(eval_position(&c, &l, 0, None), Tri::True);
    let c = piece(
        Who::Black,
        vec![RoleName::King],
        Region::OwnRank(8),
        1,
        None,
    );
    assert_eq!(eval_position(&c, &l, 0, None), Tri::False);
    // `Any` is white OR black.
    let c = piece(Who::Any, vec![RoleName::Rook], Region::File('a'), 1, None);
    assert_eq!(eval_position(&c, &l, 0, None), Tri::True);
    // Material: R + P vs nothing, diff +6.
    let m = Condition::Material(MaterialCond {
        diff: Some(MinMax { min: 6, max: 6 }),
        white: Counts {
            rooks: Some(MinMax { min: 1, max: 1 }),
            pawns: Some(MinMax { min: 1, max: 1 }),
            ..Counts::default()
        },
        ..MaterialCond::default()
    });
    assert_eq!(eval_position(&m, &l, 0, None), Tri::True);
    // Features: the d5 pawn is passed; no exposed king in the endgame.
    let f = |feature, side| {
        Condition::Feature(FeatureCond {
            feature,
            side,
            region: Region::Anywhere,
            min: 1,
        })
    };
    assert_eq!(
        eval_position(&f(Feature::PassedPawn, Who::White), &l, 0, None),
        Tri::True
    );
    assert_eq!(
        eval_position(&f(Feature::IsolatedPawn, Who::White), &l, 0, None),
        Tri::True
    );
    assert_eq!(
        eval_position(&f(Feature::ExposedKing, Who::Any), &l, 0, None),
        Tri::False
    );
    assert_eq!(
        eval_position(&f(Feature::BishopPair, Who::White), &l, 0, None),
        Tri::False
    );
}

#[test]
fn bishop_colours_status_and_side() {
    let opposite = line(
        Some("4k3/8/8/8/8/5b2/8/2B1K3 w - - 0 1"),
        "",
        GameResult::Unknown,
    );
    let bishops = |b| {
        Condition::Material(MaterialCond {
            bishops: b,
            ..MaterialCond::default()
        })
    };
    // Bc1 is dark, Bf3 is light.
    assert_eq!(
        eval_position(&bishops(BishopColors::Opposite), &opposite, 0, None),
        Tri::True
    );
    assert_eq!(
        eval_position(&bishops(BishopColors::Same), &opposite, 0, None),
        Tri::False
    );
    let mate = line(None, SCHOLAR, GameResult::White);
    let status = |s| Condition::Status { status: s };
    assert_eq!(
        eval_position(&status(Status::Checkmate), &mate, 7, None),
        Tri::True
    );
    assert_eq!(
        eval_position(&status(Status::Check), &mate, 7, None),
        Tri::True
    );
    assert_eq!(
        eval_position(&status(Status::Stalemate), &mate, 7, None),
        Tri::False
    );
    assert_eq!(
        eval_position(&Condition::SideToMove { side: Side::Black }, &mate, 7, None),
        Tri::True
    );
}

#[test]
fn regions_have_the_documented_definitions() {
    use mychess_core::Color::{Black, White};
    let has = |m: u64, s: &str| m & (1u64 << u32::from(s.parse::<Square>().expect("sq"))) != 0;
    let center = region_mask(&Region::Named(NamedRegion::Center), White);
    assert_eq!(center.count_ones(), 4);
    assert!(has(center, "d4") && has(center, "e5"));
    assert_eq!(
        region_mask(&Region::Named(NamedRegion::ExtendedCenter), White).count_ones(),
        16
    );
    let dark = region_mask(&Region::Named(NamedRegion::DarkSquares), White);
    assert!(has(dark, "a1") && !has(dark, "b1") && has(dark, "h2"));
    for i in 0..64u32 {
        let sq = Square::new(i);
        assert_eq!(has(dark, &sq.to_string()), sq.is_dark(), "{sq}");
    }
    let light = region_mask(&Region::Named(NamedRegion::LightSquares), White);
    assert_eq!(light, !dark);
    assert_eq!(
        region_mask(&Region::Named(NamedRegion::Edge), White).count_ones(),
        28
    );
    assert_eq!(
        region_mask(&Region::Named(NamedRegion::LongDiagonals), White).count_ones(),
        16
    );
    let q = region_mask(&Region::Named(NamedRegion::QueenSide), White);
    assert!(has(q, "d1") && !has(q, "e1"));
    assert!(has(
        region_mask(&Region::Named(NamedRegion::WhiteHalf), White),
        "h4"
    ));
    assert!(has(
        region_mask(&Region::Named(NamedRegion::BlackHalf), White),
        "a5"
    ));
    assert!(has(region_mask(&Region::OwnRank(2), White), "e2"));
    assert!(has(region_mask(&Region::OwnRank(2), Black), "e7"));
    assert!(has(region_mask(&Region::Rank(7), Black), "e7"));
    let rect = Region::Rect {
        from: Sq("f6".parse().expect("sq")),
        to: Sq("c3".parse().expect("sq")),
    };
    assert_eq!(region_mask(&rect, White).count_ones(), 16);
    assert_eq!(region_mask(&Region::File('z'), White), 0);
}

#[test]
fn double_attack_discovery_and_defends() {
    // Discovered check by the bishop move.
    let l = line(
        Some("4k3/7p/8/8/4B3/8/8/4RK2 w - - 0 1"),
        "Bxh7",
        GameResult::Unknown,
    );
    let disc = |d| {
        Condition::Discovered(DiscoveredCond {
            discovery: d,
            by: Who::White,
            roles: vec![RoleName::Bishop],
        })
    };
    assert_eq!(
        eval_position(&disc(DiscoveredKind::Check), &l, 1, None),
        Tri::True
    );
    assert_eq!(
        eval_position(&disc(DiscoveredKind::Attack), &l, 1, None),
        Tri::True
    );
    assert_eq!(
        eval_position(&disc(DiscoveredKind::DoubleCheck), &l, 1, None),
        Tri::False
    );
    assert_eq!(
        eval_position(&disc(DiscoveredKind::Check), &l, 0, None),
        Tri::False
    );
    // Double attack: the knight fork is also a double attack.
    let f = line(
        Some("r3r1k1/8/8/3N4/8/8/8/6K1 w - - 0 1"),
        "Nc7",
        GameResult::Unknown,
    );
    let da = Condition::DoubleAttack(DoubleAttackCond { by: Who::White });
    assert_eq!(eval_position(&da, &f, 1, None), Tri::True);
    assert_eq!(eval_position(&da, &f, 0, None), Tri::False);
    // Defends: the rook a1 defends the rook a8? Use R on a1 and a4.
    let d = line(
        Some("4k3/8/8/8/R7/8/8/R3K3 w - - 0 1"),
        "",
        GameResult::Unknown,
    );
    let defends = Condition::Relation(RelationCond {
        kind: RelKind::Defends,
        actor: PieceSel {
            side: Who::White,
            roles: vec![RoleName::Rook],
            region: Region::Rank(1),
        },
        target: TargetSel {
            roles: vec![RoleName::Rook],
            region: Region::Rank(4),
        },
        behind: None,
        gain: Gain::Any,
        min_targets: 2,
        when: When::Any,
        new_this_move: false,
    });
    assert_eq!(eval_position(&defends, &d, 0, None), Tri::True);
}

#[test]
fn deterministic_and_pure() {
    let l = line(None, LEGAL, GameResult::White);
    let p = pat(QUEEN_SACRIFICE);
    let a = match_line(&p, &l, None);
    let b = match_line(&p, &l, None);
    assert_eq!(a, b);
    assert_eq!(
        Line::from_game(&game(None, LEGAL, GameResult::White), 5)
            .frames
            .len(),
        5
    );
    assert!(Line::from_game(&game(None, LEGAL, GameResult::White), 5).truncated);
}

#[test]
fn end_only_on_a_cut_line_matches_nothing_and_counts_unknown() {
    // 2049 frames do not fit in MAX_FRAMES: the last kept frame is not the end.
    let mut g = Game::new();
    let mut node = g.root();
    let mut plies = 0usize;
    // Knights shuffle: a legal game with no mate, more than 2048 plies.
    let cycle = ["Nf3", "Nf6", "Ng1", "Ng8"];
    while plies < 2060 {
        let pos = g.position_at(node).expect("node").clone();
        let mv = pos
            .parse_san(cycle[plies % 4], SanMode::Strict)
            .expect("san")
            .mv;
        node = g.add_move(node, mv).expect("legal");
        plies += 1;
    }
    let l = Line::from_game(&g, MAX_FRAMES);
    assert_eq!(l.frames.len(), MAX_FRAMES);
    assert!(l.truncated);
    assert!(!l.broken, "a frame limit is not a broken game");
    let mut p = root(Condition::Phase {
        phase: Phase::Opening,
    });
    // Without `end_only` the opening matches at once.
    assert_eq!(match_line(&p, &l, None).ply, Some(1));
    p.scope.end_only = true;
    let m = match_line(&p, &l, None);
    assert_eq!(m.ply, None);
    assert_eq!(m.unknown_positions, 1, "the undecidable end is counted");
    assert_eq!(matching_plies(&p, &l, None), Vec::<u16>::new());
    // With `ignore_colors` the mirrored line is not tried (nor counted again).
    p.scope.ignore_colors = true;
    assert_eq!(match_line(&p, &l, None).unknown_positions, 1);
    // A complete line still evaluates its last position.
    let full = line(None, SCHOLAR, GameResult::White);
    assert!(!full.truncated);
    assert!(!full.broken);
    assert!(full.ends_in_mate);
    let mut q = root(Condition::Status {
        status: Status::Checkmate,
    });
    q.scope.end_only = true;
    assert_eq!(match_line(&q, &full, None).ply, Some(7));
}

#[test]
fn a_broken_line_is_cut_and_never_ends_in_mate() {
    // A `Game` built through the API cannot hold an unreadable node, so the
    // flag is exercised on a hand-made line: cut, broken, no mate.
    let full = line(None, SCHOLAR, GameResult::White);
    let mut broken = Line::new(full.frames[..4].to_vec(), full.result, true);
    broken.broken = true;
    assert!(broken.truncated && broken.broken && !broken.ends_in_mate);
    assert!(broken.mirrored().expect("mirror").broken);
    let mut p = root(Condition::Status {
        status: Status::Checkmate,
    });
    p.scope.end_only = true;
    let m = match_line(&p, &broken, None);
    assert_eq!((m.ply, m.unknown_positions), (None, 1));
}
