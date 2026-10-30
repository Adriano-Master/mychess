//! The catalogue of ready-made patterns (spec M6-2, D3279 and D3280).
//!
//! A [`Preset`] is a name, the list of parameters the window offers and a
//! pure function that turns [`Params`] into an ordinary [`Pattern`], which
//! the user may then customise: presets and the visual editor produce the
//! same type (D3251). [`build`] (and [`Preset::build_checked`]) refuse a
//! parameter outside its range with `InvalidValue` instead of approximating
//! (rule 22); the bare `fn` pointer [`Preset::build`] expects parameters that
//! passed [`check_params`].
//!
//! "Either colour" is [`Who::Any`], whose rule is "white **or** black" for
//! each condition. Where one colour must hold in several conditions at once
//! (pawn structures, the back-rank mate) the preset expands to an `Any`
//! group with one branch per colour.
//!
//! The pattern gets an English default name and description (the window
//! shows the localised one from [`Preset::name_key`]); [`Params::name`] and
//! [`Params::description`] override them, which is how the app saves a
//! pattern with its translated name.

use super::model::{
    Condition, DiscoveredCond, DiscoveredKind, DoubleAttackCond, Feature, FeatureCond, Gain,
    MateCond, MateMode, MoveCond, Outcome, Pattern, PatternError, PieceCond, PieceSel, Region,
    RelKind, RelationCond, ResultKind, RoleName, SacrificeCond, Scope, Sq, TargetSel, When,
    WhichMove, Who,
};
use mychess_core::Square;

/// The knobs of the presets. Every field has a default
/// ([`Params::default`]); a preset reads only the ones in its
/// [`Preset::params`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Params {
    /// The colour (`White`, `Black` or `Any`; `Mover` and `Opponent` are
    /// refused).
    pub by: Who,
    /// The N of "mate in N" (1 to 5).
    pub n: u8,
    /// Mate: `Available` looks for the mate in every position, `Played` only
    /// where the game ends in it.
    pub mate_mode: MateMode,
    /// Mate `Available`: the first move must give check.
    pub forcing_only: bool,
    /// Mate: node budget per position (1000 to 2 000 000).
    pub max_nodes: u32,
    /// The acting piece kinds (pinner, forker, sacrificed piece, promotion
    /// kinds); empty = any.
    pub roles: Vec<RoleName>,
    /// The victim kinds (pinned piece, hanging piece); empty = the preset's
    /// own default.
    pub target_roles: Vec<RoleName>,
    /// Pin: against the king (absolute); otherwise against any piece.
    pub against_king: bool,
    /// Discovered: attack, check or double check.
    pub discovery: DiscoveredKind,
    /// Sacrifice: net material lost (1 to 20).
    pub min_net: u8,
    /// Sacrifice: what must follow.
    pub outcome: Outcome,
    /// Sacrifice: moves of the sacrificer to wait for it (1 to 8).
    pub within: u8,
    /// Sacrifice: only in games the sacrificer won (needs a colour).
    pub require_win: bool,
    /// Passed pawn: advanced at least to this rank of its owner (2 to 7);
    /// `None` = anywhere.
    pub min_own_rank: Option<u8>,
    /// Name of the pattern (default: an English one).
    pub name: Option<String>,
    /// Description of the pattern.
    pub description: Option<String>,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            by: Who::Any,
            n: 2,
            mate_mode: MateMode::Available,
            forcing_only: false,
            max_nodes: 20_000,
            roles: Vec::new(),
            target_roles: Vec::new(),
            against_king: true,
            discovery: DiscoveredKind::Attack,
            min_net: 2,
            outcome: Outcome::Either,
            within: 3,
            require_win: false,
            min_own_rank: None,
            name: None,
            description: None,
        }
    }
}

/// A parameter the window offers for a preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParamKey {
    /// [`Params::by`].
    By,
    /// [`Params::n`].
    N,
    /// [`Params::mate_mode`].
    MateMode,
    /// [`Params::forcing_only`].
    ForcingOnly,
    /// [`Params::max_nodes`].
    MaxNodes,
    /// [`Params::roles`].
    Roles,
    /// [`Params::target_roles`].
    TargetRoles,
    /// [`Params::against_king`].
    AgainstKing,
    /// [`Params::discovery`].
    Discovery,
    /// [`Params::min_net`].
    MinNet,
    /// [`Params::outcome`].
    Outcome,
    /// [`Params::within`].
    Within,
    /// [`Params::require_win`].
    RequireWin,
    /// [`Params::min_own_rank`].
    MinOwnRank,
}

/// One ready-made pattern.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    /// Stable identifier (`mate_in`, `pin`, `fork`...).
    pub id: &'static str,
    /// Fluent key of its name (`pattern-preset-...`).
    pub name_key: &'static str,
    /// The parameters the window shows for it.
    pub params: &'static [ParamKey],
    /// Builds the pattern. Pure; expects parameters that passed
    /// [`check_params`] (use [`Preset::build_checked`] otherwise).
    pub build: fn(&Params) -> Pattern,
}

impl Preset {
    /// Checks `params` (the ones this preset uses), builds the pattern and
    /// validates it.
    ///
    /// # Errors
    ///
    /// `InvalidValue` for a parameter out of range, or the validation error
    /// of the result.
    pub fn build_checked(&self, params: &Params) -> Result<Pattern, PatternError> {
        check_params(self, params)?;
        let pattern = (self.build)(params);
        pattern.validate()?;
        Ok(pattern)
    }
}

fn bad(key: &str, reason: &str) -> PatternError {
    PatternError::InvalidValue {
        path: format!("params.{key}"),
        reason: reason.to_owned(),
    }
}

/// Checks the parameters that `preset` uses against their ranges (rule 22).
///
/// # Errors
///
/// `InvalidValue` with `path = "params.<field>"`.
pub fn check_params(preset: &Preset, p: &Params) -> Result<(), PatternError> {
    if matches!(p.by, Who::Mover | Who::Opponent) {
        return Err(bad("by", "must be white, black or any"));
    }
    if let Some(name) = &p.name
        && (name.is_empty() || name.chars().count() > 80)
    {
        return Err(bad("name", "1 to 80 characters"));
    }
    for key in preset.params {
        match key {
            ParamKey::N if !(1..=5).contains(&p.n) => return Err(bad("n", "must be 1 to 5")),
            ParamKey::MaxNodes if !(1_000..=2_000_000).contains(&p.max_nodes) => {
                return Err(bad("max_nodes", "must be 1000 to 2000000"));
            }
            ParamKey::MinNet if !(1..=20).contains(&p.min_net) => {
                return Err(bad("min_net", "must be 1 to 20"));
            }
            ParamKey::Within if !(1..=8).contains(&p.within) => {
                return Err(bad("within", "must be 1 to 8"));
            }
            ParamKey::MinOwnRank if p.min_own_rank.is_some_and(|k| !(2..=7).contains(&k)) => {
                return Err(bad("min_own_rank", "must be 2 to 7"));
            }
            ParamKey::RequireWin if p.require_win && p.by == Who::Any => {
                return Err(bad("require_win", "needs white or black"));
            }
            ParamKey::Roles | ParamKey::TargetRoles => {
                let list = if *key == ParamKey::Roles {
                    &p.roles
                } else {
                    &p.target_roles
                };
                if list.len() > 6 {
                    return Err(bad("roles", "at most 6 kinds"));
                }
                if preset.id == "promotion"
                    && list
                        .iter()
                        .any(|r| matches!(r, RoleName::King | RoleName::Pawn))
                {
                    return Err(bad("roles", "a pawn promotes to Q, R, B or N"));
                }
                if preset.id == "sacrifice" && list.contains(&RoleName::King) {
                    return Err(bad("roles", "the king cannot be sacrificed"));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A square from the name of a constant of this file. A bad name is a bug of
/// the table, not an input: debug builds (and so the tests) panic on it
/// instead of silently using a1; `every_square_constant_parses` covers them.
fn sq(name: &str) -> Sq {
    if let Ok(square) = name.parse::<Square>() {
        Sq(square)
    } else {
        debug_assert!(false, "bad square constant {name:?} in the presets");
        Sq(Square::A1)
    }
}

fn colour_word(by: Who) -> &'static str {
    match by {
        Who::White => "white",
        Who::Black => "black",
        _ => "either colour",
    }
}

fn finish(p: &Params, title: &str, description: &str, scope: Scope, root: Condition) -> Pattern {
    Pattern {
        name: p
            .name
            .clone()
            .unwrap_or_else(|| format!("{title} ({})", colour_word(p.by))),
        description: p
            .description
            .clone()
            .unwrap_or_else(|| description.to_owned()),
        scope,
        root,
    }
}

fn finish_fixed(p: &Params, title: &str, description: &str, root: Condition) -> Pattern {
    Pattern {
        name: p.name.clone().unwrap_or_else(|| title.to_owned()),
        description: p
            .description
            .clone()
            .unwrap_or_else(|| description.to_owned()),
        scope: Scope::default(),
        root,
    }
}

/// `f(white)` / `f(black)`, or an `Any` group with both for "either colour".
fn per_side(by: Who, f: impl Fn(Who) -> Condition) -> Condition {
    match by {
        Who::White | Who::Black => f(by),
        _ => Condition::Any {
            items: vec![f(Who::White), f(Who::Black)],
        },
    }
}

fn piece(side: Who, role: RoleName, region: Region, min: u8, max: Option<u8>) -> Condition {
    Condition::Piece(PieceCond {
        side,
        roles: vec![role],
        region,
        min,
        max,
    })
}

fn pawns_on(side: Who, region: Region) -> Condition {
    piece(side, RoleName::Pawn, region, 1, None)
}

fn no_pawns_on(side: Who, region: Region) -> Condition {
    piece(side, RoleName::Pawn, region, 0, Some(0))
}

fn file(c: char) -> Region {
    Region::File(c)
}

fn squares(names: &[&str]) -> Region {
    Region::Squares(names.iter().map(|n| sq(n)).collect())
}

fn target(roles: &[RoleName]) -> TargetSel {
    TargetSel {
        roles: roles.to_vec(),
        region: Region::Anywhere,
    }
}

fn relation(kind: RelKind, p: &Params, targets: TargetSel) -> RelationCond {
    RelationCond {
        kind,
        actor: PieceSel {
            side: p.by,
            roles: p.roles.clone(),
            region: Region::Anywhere,
        },
        target: targets,
        behind: None,
        gain: Gain::Any,
        min_targets: 2,
        when: When::Any,
        new_this_move: false,
    }
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

fn build_mate_in(p: &Params) -> Pattern {
    let scope = Scope {
        min_move: 0,
        ..Scope::default()
    };
    let root = Condition::Mate(MateCond {
        by: p.by,
        moves_min: p.n,
        moves_max: p.n,
        mode: p.mate_mode,
        forcing_only: p.forcing_only,
        max_nodes: p.max_nodes,
    });
    let title = format!("Mate in {}", p.n);
    finish(
        p,
        &title,
        "Positions with a forced mate in exactly this many moves.",
        scope,
        root,
    )
}

fn build_pin(p: &Params) -> Pattern {
    let mut r = relation(RelKind::Pins, p, target(&p.target_roles));
    r.behind = Some(if p.against_king {
        target(&[RoleName::King])
    } else {
        target(&[])
    });
    finish(
        p,
        "Pin",
        if p.against_king {
            "A piece pins an enemy piece against the king."
        } else {
            "A piece pins an enemy piece against an undefended or more valuable piece."
        },
        Scope::default(),
        Condition::Relation(r),
    )
}

fn build_fork(p: &Params) -> Pattern {
    let mut r = relation(RelKind::Forks, p, target(&[]));
    r.when = When::ActorJustMoved;
    r.new_this_move = true;
    finish(
        p,
        "Fork",
        "A piece attacks two enemy pieces at once and cannot be captured to advantage.",
        Scope::default(),
        Condition::Relation(r),
    )
}

fn build_skewer(p: &Params) -> Pattern {
    let mut r = relation(RelKind::Skewers, p, target(&[]));
    r.when = When::ActorJustMoved;
    r.new_this_move = true;
    finish(
        p,
        "Skewer",
        "A piece attacks a valuable piece with another one behind it.",
        Scope::default(),
        Condition::Relation(r),
    )
}

fn build_discovered(p: &Params) -> Pattern {
    let root = Condition::Discovered(DiscoveredCond {
        discovery: p.discovery,
        by: p.by,
        roles: p.roles.clone(),
    });
    let title = match p.discovery {
        DiscoveredKind::Attack => "Discovered attack",
        DiscoveredKind::Check => "Discovered check",
        DiscoveredKind::DoubleCheck => "Double check",
    };
    finish(
        p,
        title,
        "A move that uncovers an attack by another piece.",
        Scope::default(),
        root,
    )
}

fn build_double_attack(p: &Params) -> Pattern {
    finish(
        p,
        "Double attack",
        "A move after which one piece attacks two enemy pieces.",
        Scope::default(),
        Condition::DoubleAttack(DoubleAttackCond { by: p.by }),
    )
}

fn build_sacrifice(p: &Params) -> Pattern {
    let sacrifice = Condition::Sacrifice(SacrificeCond {
        by: p.by,
        roles: p.roles.clone(),
        min_net: p.min_net,
        outcome: p.outcome,
        within: p.within,
    });
    let root = if p.require_win {
        let wins = match p.by {
            Who::Black => ResultKind::BlackWins,
            _ => ResultKind::WhiteWins,
        };
        Condition::All {
            items: vec![
                Condition::Result {
                    results: vec![wins],
                },
                sacrifice,
            ],
        }
    } else {
        sacrifice
    };
    finish(
        p,
        "Sacrifice",
        "Material given up that comes back or leads to mate.",
        Scope::default(),
        root,
    )
}

fn build_hanging(p: &Params) -> Pattern {
    let victims = if p.target_roles.is_empty() {
        vec![
            RoleName::Knight,
            RoleName::Bishop,
            RoleName::Rook,
            RoleName::Queen,
        ]
    } else {
        p.target_roles.clone()
    };
    let mut r = relation(RelKind::Attacks, p, target(&victims));
    r.gain = Gain::Undefended;
    r.when = When::ActorJustMoved;
    r.new_this_move = true;
    finish(
        p,
        "Hanging piece",
        "A move that attacks an undefended piece.",
        Scope::default(),
        Condition::Relation(r),
    )
}

fn build_passed_pawn(p: &Params) -> Pattern {
    let feature = |region: Region| {
        Condition::Feature(FeatureCond {
            feature: Feature::PassedPawn,
            side: p.by,
            region,
            min: 1,
        })
    };
    let root = match p.min_own_rank {
        None => feature(Region::Anywhere),
        Some(k) => Condition::Any {
            items: (k..=7).map(|rank| feature(Region::OwnRank(rank))).collect(),
        },
    };
    finish(
        p,
        "Passed pawn",
        "A pawn with no enemy pawn in front of it or on the adjacent files.",
        Scope::default(),
        root,
    )
}

fn build_exposed_king(p: &Params) -> Pattern {
    finish(
        p,
        "Exposed king",
        "A king with almost no pawn shield outside the endgame.",
        Scope::default(),
        Condition::Feature(FeatureCond {
            feature: Feature::ExposedKing,
            side: p.by,
            region: Region::Anywhere,
            min: 1,
        }),
    )
}

fn build_back_rank_mate(p: &Params) -> Pattern {
    let branch = |by: Who| {
        let (rank, victim) = if by == Who::White {
            (8, Who::Black)
        } else {
            (1, Who::White)
        };
        let mut r = relation(
            RelKind::Attacks,
            &Params {
                by,
                roles: vec![RoleName::Rook, RoleName::Queen],
                ..Params::default()
            },
            target(&[RoleName::King]),
        );
        r.when = When::ActorJustMoved;
        Condition::All {
            items: vec![
                Condition::Status {
                    status: super::model::Status::Checkmate,
                },
                piece(victim, RoleName::King, Region::Rank(rank), 1, None),
                Condition::Relation(r),
            ],
        }
    };
    finish(
        p,
        "Back-rank mate",
        "Checkmate by a rook or queen on the last rank.",
        Scope::default(),
        per_side(p.by, branch),
    )
}

fn build_promotion(p: &Params) -> Pattern {
    let kinds = if p.roles.is_empty() {
        vec![
            RoleName::Queen,
            RoleName::Rook,
            RoleName::Bishop,
            RoleName::Knight,
        ]
    } else {
        p.roles.clone()
    };
    finish(
        p,
        "Promotion",
        "A pawn promotes to one of these pieces.",
        Scope::default(),
        Condition::Move(MoveCond {
            which: WhichMove::Previous,
            by: p.by,
            roles: vec![RoleName::Pawn],
            from: Region::Anywhere,
            to: Region::Anywhere,
            capture: super::model::Flag::Both,
            check: super::model::Flag::Both,
            promotion: kinds,
            castling: None,
        }),
    )
}

/// An isolated queen's pawn for `side`: a pawn on the d file with none on the
/// c and e files. The regions are whole files, which is the same as the
/// rank range 2..7 (a pawn never stands on the 1st or 8th rank), so "a pawn
/// on d with none on c and e" and the Scid pattern for an isolated d pawn
/// match the same positions.
fn iqp_items(side: Who) -> Vec<Condition> {
    vec![
        pawns_on(side, file('d')),
        no_pawns_on(side, file('c')),
        no_pawns_on(side, file('e')),
    ]
}

fn build_iqp(p: &Params) -> Pattern {
    finish_fixed(
        p,
        "Isolated queen's pawn (white)",
        "A white pawn on the d file with no white pawns on the c and e files.",
        Condition::All {
            items: iqp_items(Who::White),
        },
    )
}

fn build_iqp_black(p: &Params) -> Pattern {
    finish_fixed(
        p,
        "Isolated queen's pawn (black)",
        "A black pawn on the d file with no black pawns on the c and e files.",
        Condition::All {
            items: iqp_items(Who::Black),
        },
    )
}

fn build_iqp_both(p: &Params) -> Pattern {
    let mut items = iqp_items(Who::White);
    items.extend(iqp_items(Who::Black));
    finish_fixed(
        p,
        "Isolated queen's pawns (both sides)",
        "Both sides have an isolated pawn on the d file.",
        Condition::All { items },
    )
}

fn build_hanging_pawns(p: &Params) -> Pattern {
    let rect = |c: &str| Region::Rect {
        from: sq(&format!("{c}4")),
        to: sq(&format!("{c}5")),
    };
    let branch = |by: Who| Condition::All {
        items: vec![
            pawns_on(by, rect("c")),
            pawns_on(by, rect("d")),
            no_pawns_on(by, file('b')),
            no_pawns_on(by, file('e')),
        ],
    };
    finish(
        p,
        "Hanging pawns",
        "Pawns on c and d, side by side on the 4th or 5th rank, with none on b and e.",
        Scope::default(),
        per_side(p.by, branch),
    )
}

fn build_maroczy(p: &Params) -> Pattern {
    finish_fixed(
        p,
        "Maroczy bind",
        "White pawns on c4 and e4 and none on the d file.",
        Condition::All {
            items: vec![
                pawns_on(Who::White, squares(&["c4"])),
                pawns_on(Who::White, squares(&["e4"])),
                no_pawns_on(Who::White, file('d')),
            ],
        },
    )
}

fn bishop_on(by: Who, square: &str) -> Condition {
    piece(by, RoleName::Bishop, squares(&[square]), 1, None)
}

fn build_fianchetto_light(p: &Params) -> Pattern {
    finish(
        p,
        "Fianchetto (light-squared bishop)",
        "A bishop on g2 (white) or b7 (black).",
        Scope::default(),
        per_side(p.by, |by| {
            bishop_on(by, if by == Who::White { "g2" } else { "b7" })
        }),
    )
}

fn build_fianchetto_dark(p: &Params) -> Pattern {
    finish(
        p,
        "Fianchetto (dark-squared bishop)",
        "A bishop on b2 (white) or g7 (black).",
        Scope::default(),
        per_side(p.by, |by| {
            bishop_on(by, if by == Who::White { "b2" } else { "g7" })
        }),
    )
}

fn build_fianchetto_four(p: &Params) -> Pattern {
    finish(
        p,
        "Fianchetto of both bishops",
        "Both bishops fianchettoed: g2 and b2 (white) or b7 and g7 (black).",
        Scope::default(),
        per_side(p.by, |by| {
            let (a, b) = if by == Who::White {
                ("g2", "b2")
            } else {
                ("b7", "g7")
            };
            Condition::All {
                items: vec![bishop_on(by, a), bishop_on(by, b)],
            }
        }),
    )
}

fn build_opposite_castling(p: &Params) -> Pattern {
    let kings = |w: &str, b: &str| Condition::All {
        items: vec![
            piece(Who::White, RoleName::King, squares(&[w]), 1, None),
            piece(Who::Black, RoleName::King, squares(&[b]), 1, None),
        ],
    };
    finish_fixed(
        p,
        "Opposite-side castling",
        "The kings castled on opposite wings: c1 against g8, or g1 against c8.",
        Condition::Any {
            items: vec![kings("c1", "g8"), kings("g1", "c8")],
        },
    )
}

// ---------------------------------------------------------------------------
// The catalogue
// ---------------------------------------------------------------------------

use ParamKey as K;

/// The catalogue, in the order of the window.
static PRESETS: [Preset; 21] = [
    Preset {
        id: "mate_in",
        name_key: "pattern-preset-mate-in",
        params: &[K::By, K::N, K::MateMode, K::ForcingOnly, K::MaxNodes],
        build: build_mate_in,
    },
    Preset {
        id: "pin",
        name_key: "pattern-preset-pin",
        params: &[K::By, K::Roles, K::TargetRoles, K::AgainstKing],
        build: build_pin,
    },
    Preset {
        id: "fork",
        name_key: "pattern-preset-fork",
        params: &[K::By, K::Roles],
        build: build_fork,
    },
    Preset {
        id: "skewer",
        name_key: "pattern-preset-skewer",
        params: &[K::By],
        build: build_skewer,
    },
    Preset {
        id: "discovered",
        name_key: "pattern-preset-discovered",
        params: &[K::By, K::Discovery],
        build: build_discovered,
    },
    Preset {
        id: "double_attack",
        name_key: "pattern-preset-double-attack",
        params: &[K::By],
        build: build_double_attack,
    },
    Preset {
        id: "sacrifice",
        name_key: "pattern-preset-sacrifice",
        params: &[
            K::By,
            K::Roles,
            K::MinNet,
            K::Outcome,
            K::Within,
            K::RequireWin,
        ],
        build: build_sacrifice,
    },
    Preset {
        id: "hanging",
        name_key: "pattern-preset-hanging",
        params: &[K::By, K::TargetRoles],
        build: build_hanging,
    },
    Preset {
        id: "passed_pawn",
        name_key: "pattern-preset-passed-pawn",
        params: &[K::By, K::MinOwnRank],
        build: build_passed_pawn,
    },
    Preset {
        id: "exposed_king",
        name_key: "pattern-preset-exposed-king",
        params: &[K::By],
        build: build_exposed_king,
    },
    Preset {
        id: "back_rank_mate",
        name_key: "pattern-preset-back-rank",
        params: &[K::By],
        build: build_back_rank_mate,
    },
    Preset {
        id: "promotion",
        name_key: "pattern-preset-promotion",
        params: &[K::By, K::Roles],
        build: build_promotion,
    },
    Preset {
        id: "iqp",
        name_key: "pattern-preset-iqp",
        params: &[],
        build: build_iqp,
    },
    Preset {
        id: "iqp_black",
        name_key: "pattern-preset-iqp-black",
        params: &[],
        build: build_iqp_black,
    },
    Preset {
        id: "iqp_both",
        name_key: "pattern-preset-iqp-both",
        params: &[],
        build: build_iqp_both,
    },
    Preset {
        id: "hanging_pawns",
        name_key: "pattern-preset-hanging-pawns",
        params: &[K::By],
        build: build_hanging_pawns,
    },
    Preset {
        id: "maroczy",
        name_key: "pattern-preset-maroczy",
        params: &[],
        build: build_maroczy,
    },
    Preset {
        id: "fianchetto_light",
        name_key: "pattern-preset-fianchetto-light",
        params: &[K::By],
        build: build_fianchetto_light,
    },
    Preset {
        id: "fianchetto_dark",
        name_key: "pattern-preset-fianchetto-dark",
        params: &[K::By],
        build: build_fianchetto_dark,
    },
    Preset {
        id: "fianchetto_four",
        name_key: "pattern-preset-fianchetto-four",
        params: &[K::By],
        build: build_fianchetto_four,
    },
    Preset {
        id: "opposite_castling",
        name_key: "pattern-preset-opposite-castling",
        params: &[],
        build: build_opposite_castling,
    },
];

/// Every preset, in the order of the window.
pub fn all() -> Vec<Preset> {
    PRESETS.to_vec()
}

/// The preset called `id`.
pub fn find(id: &str) -> Option<Preset> {
    all().into_iter().find(|p| p.id == id)
}

/// Builds the preset `id` with `params`, checked and validated (spec
/// "Catálogo dos presets").
///
/// # Errors
///
/// `InvalidValue` for an unknown id (`path = "id"`) or a parameter out of
/// range, or the validation error of the result.
pub fn build(id: &str, params: &Params) -> Result<Pattern, PatternError> {
    match find(id) {
        Some(preset) => preset.build_checked(params),
        None => Err(PatternError::InvalidValue {
            path: "id".to_owned(),
            reason: format!("unknown preset {id:?}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_builds_and_validates_with_defaults() {
        let list = all();
        assert_eq!(list.len(), 21);
        for preset in &list {
            let p = preset
                .build_checked(&Params::default())
                .unwrap_or_else(|e| panic!("{}: {e}", preset.id));
            assert!(!p.name.is_empty(), "{}", preset.id);
        }
    }

    #[test]
    fn every_square_constant_parses() {
        // `sq` panics in debug builds on a bad constant; build every preset in
        // all its variants that reach it, then the literal tables.
        for preset in all() {
            for against_king in [true, false] {
                let params = Params {
                    against_king,
                    ..Params::default()
                };
                preset.build_checked(&params).expect("builds");
            }
        }
        for name in ["c4", "e4", "c5", "d4", "d5", "g2", "b7", "e1", "e8"] {
            assert_eq!(sq(name).0.to_string(), name);
        }
    }

    #[test]
    fn pin_description_follows_the_target() {
        let king = build("pin", &Params::default()).expect("pin");
        let other = build(
            "pin",
            &Params {
                against_king: false,
                ..Params::default()
            },
        )
        .expect("pin");
        assert!(king.description.contains("the king"));
        assert!(other.description.contains("undefended or more valuable"));
    }

    #[test]
    fn ids_and_keys_are_unique() {
        let list = all();
        for (i, a) in list.iter().enumerate() {
            for b in list.iter().skip(i + 1) {
                assert_ne!(a.id, b.id);
                assert_ne!(a.name_key, b.name_key);
            }
        }
    }

    #[test]
    fn out_of_range_is_an_error_not_an_approximation() {
        let bad_n = Params {
            n: 9,
            ..Params::default()
        };
        assert!(matches!(
            build("mate_in", &bad_n),
            Err(PatternError::InvalidValue { .. })
        ));
        let bad_rank = Params {
            min_own_rank: Some(0),
            ..Params::default()
        };
        assert!(build("passed_pawn", &bad_rank).is_err());
        assert!(build("nope", &Params::default()).is_err());
    }
}
