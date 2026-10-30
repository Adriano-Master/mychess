//! The sentence of a card (spec M6-2, D3282): a Fluent key plus arguments
//! generated from the model. No user-visible text lives here: the front-end
//! translates the key, translating nested [`SentenceArg::Key`] and
//! [`SentenceArg::Phrase`] values first and joining a [`SentenceArg::List`]
//! with its own conjunction ("or" for a list of pieces).
//!
//! Numbers that stand for flags are `0` / `1` so that Fluent can `select`
//! on them.

use mychess_db::pattern::model::{
    BishopColors, CastleKind, Condition, DiscoveredKind, Feature, Flag, Gain, MateCond, MateMode,
    MaterialCond, MinMax, MoveCond, NamedRegion, Outcome, Phase, PieceCond, PieceSel, Region,
    RelKind, RelationCond, ResultKind, RoleName, SacrificeCond, Side, Status, TargetSel, When,
    WhichMove, Who,
};

/// A Fluent message and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    /// The key of the message (`pattern-sentence-...`).
    pub key: &'static str,
    /// The arguments, by name.
    pub args: Vec<(&'static str, SentenceArg)>,
}

impl Sentence {
    /// The argument called `name`.
    pub fn arg(&self, name: &str) -> Option<&SentenceArg> {
        self.args.iter().find(|(n, _)| *n == name).map(|(_, a)| a)
    }
}

/// An argument of a [`Sentence`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SentenceArg {
    /// A message without arguments to translate (`pattern-who-white`).
    Key(&'static str),
    /// Text that needs no translation (square names, files).
    Text(String),
    /// A number (a count, a rank, a flag as 0 / 1).
    Number(i64),
    /// Several values to join ("a rook, a bishop").
    List(Vec<SentenceArg>),
    /// A nested sentence, translated with its own arguments.
    Phrase(Box<Sentence>),
}

fn s(key: &'static str, args: Vec<(&'static str, SentenceArg)>) -> Sentence {
    Sentence { key, args }
}

fn n(value: impl Into<i64>) -> SentenceArg {
    SentenceArg::Number(value.into())
}

fn count(len: usize) -> SentenceArg {
    SentenceArg::Number(i64::try_from(len).unwrap_or(i64::MAX))
}

fn flag(value: bool) -> SentenceArg {
    n(i64::from(value))
}

fn who(who: Who) -> SentenceArg {
    SentenceArg::Key(match who {
        Who::White => "pattern-who-white",
        Who::Black => "pattern-who-black",
        Who::Mover => "pattern-who-mover",
        Who::Opponent => "pattern-who-opponent",
        Who::Any => "pattern-who-any",
    })
}

fn role_key(role: RoleName) -> &'static str {
    match role {
        RoleName::King => "pattern-role-king",
        RoleName::Queen => "pattern-role-queen",
        RoleName::Rook => "pattern-role-rook",
        RoleName::Bishop => "pattern-role-bishop",
        RoleName::Knight => "pattern-role-knight",
        RoleName::Pawn => "pattern-role-pawn",
    }
}

/// The roles as a list; none means any piece.
fn roles(list: &[RoleName]) -> SentenceArg {
    if list.is_empty() {
        SentenceArg::Key("pattern-role-any")
    } else {
        SentenceArg::List(
            list.iter()
                .map(|r| SentenceArg::Key(role_key(*r)))
                .collect(),
        )
    }
}

/// The sentence of a set of squares (`pattern-region-*`).
pub fn describe_region(region: &Region) -> Sentence {
    match region {
        Region::Anywhere => s("pattern-region-anywhere", vec![]),
        Region::Squares(list) => {
            let text = list
                .iter()
                .map(|q| q.0.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            s(
                "pattern-region-squares",
                vec![("squares", SentenceArg::Text(text))],
            )
        }
        Region::File(c) => s(
            "pattern-region-file",
            vec![("file", SentenceArg::Text(c.to_string()))],
        ),
        Region::Rank(r) => s("pattern-region-rank", vec![("rank", n(*r))]),
        Region::OwnRank(r) => s("pattern-region-own-rank", vec![("rank", n(*r))]),
        Region::Rect { from, to } => s(
            "pattern-region-rect",
            vec![
                ("from", SentenceArg::Text(from.0.to_string())),
                ("to", SentenceArg::Text(to.0.to_string())),
            ],
        ),
        Region::Named(named) => s(
            match named {
                NamedRegion::Center => "pattern-region-named-center",
                NamedRegion::ExtendedCenter => "pattern-region-named-extended-center",
                NamedRegion::QueenSide => "pattern-region-named-queen-side",
                NamedRegion::KingSide => "pattern-region-named-king-side",
                NamedRegion::WhiteHalf => "pattern-region-named-white-half",
                NamedRegion::BlackHalf => "pattern-region-named-black-half",
                NamedRegion::LightSquares => "pattern-region-named-light-squares",
                NamedRegion::DarkSquares => "pattern-region-named-dark-squares",
                NamedRegion::Edge => "pattern-region-named-edge",
                NamedRegion::LongDiagonals => "pattern-region-named-long-diagonals",
            },
            vec![],
        ),
    }
}

fn region(region: &Region) -> SentenceArg {
    SentenceArg::Phrase(Box::new(describe_region(region)))
}

fn actor(sel: &PieceSel) -> SentenceArg {
    SentenceArg::Phrase(Box::new(s(
        "pattern-ref-actor",
        vec![
            ("side", who(sel.side)),
            ("piece", roles(&sel.roles)),
            ("where", region(&sel.region)),
        ],
    )))
}

fn target(sel: &TargetSel) -> SentenceArg {
    SentenceArg::Phrase(Box::new(s(
        "pattern-ref-target",
        vec![("piece", roles(&sel.roles)), ("where", region(&sel.region))],
    )))
}

fn flag_key(f: Flag) -> SentenceArg {
    SentenceArg::Key(match f {
        Flag::Yes => "pattern-flag-yes",
        Flag::No => "pattern-flag-no",
        Flag::Both => "pattern-flag-both",
    })
}

fn describe_piece(p: &PieceCond) -> Sentence {
    let mut args = vec![
        ("side", who(p.side)),
        ("piece", roles(&p.roles)),
        ("where", region(&p.region)),
    ];
    let key = match p.max {
        Some(0) if p.min == 0 => "pattern-sentence-piece-none",
        Some(max) if max == p.min => {
            args.push(("count", n(p.min)));
            "pattern-sentence-piece-exactly"
        }
        Some(max) => {
            args.push(("min", n(p.min)));
            args.push(("max", n(max)));
            "pattern-sentence-piece-between"
        }
        None if p.min > 1 => {
            args.push(("min", n(p.min)));
            "pattern-sentence-piece-at-least"
        }
        None => "pattern-sentence-piece",
    };
    s(key, args)
}

fn describe_material(m: &MaterialCond) -> Sentence {
    let mut parts = Vec::new();
    let mut count = |side: Who, name: &'static str, range: Option<MinMax<u8>>| {
        if let Some(r) = range {
            parts.push(SentenceArg::Phrase(Box::new(s(
                "pattern-material-count",
                vec![
                    ("side", who(side)),
                    ("piece", SentenceArg::Key(name)),
                    ("min", n(r.min)),
                    ("max", n(r.max)),
                ],
            ))));
        }
    };
    for (side, c) in [(Who::White, &m.white), (Who::Black, &m.black)] {
        count(side, "pattern-role-queen", c.queens);
        count(side, "pattern-role-rook", c.rooks);
        count(side, "pattern-role-bishop", c.bishops);
        count(side, "pattern-role-knight", c.knights);
        count(side, "pattern-role-minor", c.minors);
        count(side, "pattern-role-pawn", c.pawns);
    }
    if let Some(d) = m.diff {
        parts.push(SentenceArg::Phrase(Box::new(s(
            "pattern-material-diff",
            vec![("min", n(d.min)), ("max", n(d.max))],
        ))));
    }
    match m.bishops {
        BishopColors::Either => {}
        BishopColors::Same => parts.push(SentenceArg::Key("pattern-bishops-same")),
        BishopColors::Opposite => parts.push(SentenceArg::Key("pattern-bishops-opposite")),
    }
    if parts.is_empty() {
        s("pattern-sentence-material-any", vec![])
    } else {
        s(
            "pattern-sentence-material",
            vec![("parts", SentenceArg::List(parts))],
        )
    }
}

fn describe_move(m: &MoveCond) -> Sentence {
    let castling = match m.castling {
        None => "pattern-castle-none",
        Some(CastleKind::Short) => "pattern-castle-short",
        Some(CastleKind::Long) => "pattern-castle-long",
        Some(CastleKind::Any) => "pattern-castle-any",
    };
    s(
        match m.which {
            WhichMove::Previous => "pattern-sentence-move-previous",
            WhichMove::Next => "pattern-sentence-move-next",
        },
        vec![
            ("by", who(m.by)),
            ("piece", roles(&m.roles)),
            ("from", region(&m.from)),
            ("to", region(&m.to)),
            ("capture", flag_key(m.capture)),
            ("check", flag_key(m.check)),
            ("promotion", roles(&m.promotion)),
            ("castling", SentenceArg::Key(castling)),
        ],
    )
}

fn describe_relation(r: &RelationCond) -> Sentence {
    let key = match r.kind {
        RelKind::Attacks => "pattern-sentence-attacks",
        RelKind::Defends => "pattern-sentence-defends",
        RelKind::Pins => "pattern-sentence-pin",
        RelKind::Skewers => "pattern-sentence-skewer",
        RelKind::Forks => "pattern-sentence-fork",
    };
    let gain = match r.gain {
        Gain::Any => "pattern-gain-any",
        Gain::Undefended => "pattern-gain-undefended",
        Gain::WinsMaterial => "pattern-gain-wins-material",
    };
    let when = match r.when {
        When::Any => "pattern-when-any",
        When::ActorJustMoved => "pattern-when-actor-just-moved",
        When::ActorToMove => "pattern-when-actor-to-move",
    };
    let mut args = vec![
        ("actor", actor(&r.actor)),
        ("target", target(&r.target)),
        ("gain", SentenceArg::Key(gain)),
        ("when", SentenceArg::Key(when)),
        ("new", flag(r.new_this_move)),
        ("count", n(r.min_targets)),
    ];
    if let Some(b) = &r.behind {
        args.push(("behind", target(b)));
    }
    s(key, args)
}

fn describe_sacrifice(c: &SacrificeCond) -> Sentence {
    let outcome = match c.outcome {
        Outcome::Recovered => "pattern-outcome-recovered",
        Outcome::Mate => "pattern-outcome-mate",
        Outcome::Either => "pattern-outcome-either",
        Outcome::Any => "pattern-outcome-any",
    };
    s(
        "pattern-sentence-sacrifice",
        vec![
            ("by", who(c.by)),
            ("piece", roles(&c.roles)),
            ("net", n(c.min_net)),
            ("outcome", SentenceArg::Key(outcome)),
            ("within", n(c.within)),
        ],
    )
}

fn describe_mate(c: &MateCond) -> Sentence {
    let mode = match c.mode {
        MateMode::Played => "pattern-mate-mode-played",
        MateMode::Available => "pattern-mate-mode-available",
    };
    s(
        "pattern-sentence-mate",
        vec![
            ("by", who(c.by)),
            ("min", n(c.moves_min)),
            ("max", n(c.moves_max)),
            ("mode", SentenceArg::Key(mode)),
            ("forcing", flag(c.forcing_only)),
        ],
    )
}

/// The sentence of a node. A group's sentence is its title
/// (`pattern-group-*`) with the number of items.
pub fn describe(condition: &Condition) -> Sentence {
    match condition {
        Condition::All { items } => s("pattern-group-all", vec![("count", count(items.len()))]),
        Condition::Any { items } => s("pattern-group-any", vec![("count", count(items.len()))]),
        Condition::Not { .. } => s("pattern-group-not", vec![]),
        Condition::Piece(p) => describe_piece(p),
        Condition::Material(m) => describe_material(m),
        Condition::SideToMove { side } => s(
            "pattern-sentence-side-to-move",
            vec![(
                "side",
                who(match side {
                    Side::White => Who::White,
                    Side::Black => Who::Black,
                }),
            )],
        ),
        Condition::Phase { phase } => s(
            "pattern-sentence-phase",
            vec![(
                "phase",
                SentenceArg::Key(match phase {
                    Phase::Opening => "pattern-phase-opening",
                    Phase::Middlegame => "pattern-phase-middlegame",
                    Phase::Endgame => "pattern-phase-endgame",
                }),
            )],
        ),
        Condition::Status { status } => s(
            "pattern-sentence-status",
            vec![(
                "status",
                SentenceArg::Key(match status {
                    Status::Check => "pattern-status-check",
                    Status::Checkmate => "pattern-status-checkmate",
                    Status::Stalemate => "pattern-status-stalemate",
                }),
            )],
        ),
        Condition::Move(m) => describe_move(m),
        Condition::Result { results } => s(
            "pattern-sentence-result",
            vec![(
                "results",
                SentenceArg::List(
                    results
                        .iter()
                        .map(|r| {
                            SentenceArg::Key(match r {
                                ResultKind::WhiteWins => "pattern-result-white-wins",
                                ResultKind::Draw => "pattern-result-draw",
                                ResultKind::BlackWins => "pattern-result-black-wins",
                                ResultKind::Unknown => "pattern-result-unknown",
                            })
                        })
                        .collect(),
                ),
            )],
        ),
        Condition::Feature(f) => s(
            match f.feature {
                Feature::PassedPawn => "pattern-sentence-passed-pawn",
                Feature::IsolatedPawn => "pattern-sentence-isolated-pawn",
                Feature::DoubledPawns => "pattern-sentence-doubled-pawns",
                Feature::ExposedKing => "pattern-sentence-exposed-king",
                Feature::BishopPair => "pattern-sentence-bishop-pair",
            },
            vec![
                ("side", who(f.side)),
                ("where", region(&f.region)),
                ("min", n(f.min)),
            ],
        ),
        Condition::Relation(r) => describe_relation(r),
        Condition::Discovered(d) => s(
            match d.discovery {
                DiscoveredKind::Attack => "pattern-sentence-discovered-attack",
                DiscoveredKind::Check => "pattern-sentence-discovered-check",
                DiscoveredKind::DoubleCheck => "pattern-sentence-double-check",
            },
            vec![("by", who(d.by)), ("piece", roles(&d.roles))],
        ),
        Condition::DoubleAttack(d) => s("pattern-sentence-double-attack", vec![("by", who(d.by))]),
        Condition::Sacrifice(c) => describe_sacrifice(c),
        Condition::Mate(c) => describe_mate(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_search::edit::ConditionKind;
    use mychess_db::pattern::model::Sq;

    #[test]
    fn every_default_card_has_a_sentence_with_keys_only() {
        for kind in ConditionKind::ALL {
            let sentence = describe(&kind.default_condition());
            assert!(
                sentence.key.starts_with("pattern-"),
                "{kind:?}: {}",
                sentence.key
            );
        }
    }

    #[test]
    fn piece_sentences_follow_the_counts() {
        let mut p = PieceCond {
            side: Who::White,
            roles: vec![RoleName::Rook],
            region: Region::Squares(vec![Sq(mychess_core::Square::E4)]),
            min: 1,
            max: None,
        };
        assert_eq!(describe_piece(&p).key, "pattern-sentence-piece");
        p.min = 2;
        assert_eq!(describe_piece(&p).key, "pattern-sentence-piece-at-least");
        p.min = 0;
        p.max = Some(0);
        assert_eq!(describe_piece(&p).key, "pattern-sentence-piece-none");
        p.min = 2;
        p.max = Some(2);
        assert_eq!(describe_piece(&p).key, "pattern-sentence-piece-exactly");
        p.max = Some(4);
        assert_eq!(describe_piece(&p).key, "pattern-sentence-piece-between");
        let squares = describe_piece(&p);
        let Some(SentenceArg::Phrase(r)) = squares.arg("where") else {
            panic!("region phrase");
        };
        assert_eq!(r.key, "pattern-region-squares");
        assert_eq!(r.arg("squares"), Some(&SentenceArg::Text("e4".to_owned())));
    }

    #[test]
    fn a_pin_names_actor_target_and_the_piece_behind() {
        let sentence = describe(&ConditionKind::Pins.default_condition());
        assert_eq!(sentence.key, "pattern-sentence-pin");
        for name in ["actor", "target", "behind"] {
            assert!(sentence.arg(name).is_some(), "{name}");
        }
        assert!(
            describe(&ConditionKind::Attacks.default_condition())
                .arg("behind")
                .is_none()
        );
    }

    #[test]
    fn material_without_limits_has_its_own_key() {
        assert_eq!(
            describe(&ConditionKind::Material.default_condition()).key,
            "pattern-sentence-material-any"
        );
    }
}
