//! Pure edits of the tree of conditions (spec M6-2, D3282): every function
//! takes the tree and returns the edited **copy**, or the reason the edit is
//! refused. Nothing here panics: paths are followed with `get`, and the size
//! limits of the model are measured with an early cut-off, so even a hostile
//! `Replace` cannot make the walk recurse deeper than the limit.

use mychess_core::Square;
use mychess_db::pattern::model::{
    BishopColors, Condition, DiscoveredCond, DiscoveredKind, DoubleAttackCond, Feature,
    FeatureCond, Flag, Gain, MAX_DEPTH, MAX_GROUP_ITEMS, MAX_NODES, MateCond, MateMode,
    MaterialCond, MoveCond, Outcome, Phase, PieceCond, PieceSel, Region, RelKind, RelationCond,
    ResultKind, RoleName, SacrificeCond, Side, Sq, Status, TargetSel, When, WhichMove, Who,
};

use super::message::{EditRefusal, GroupKind, RegionSlot};

/// The kinds of condition the "Add condition" menu offers (spec, "Condições
/// disponíveis no editor").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConditionKind {
    /// A group: all of these.
    All,
    /// A group: any of these.
    Any,
    /// A group: not this.
    Not,
    /// Pieces on squares.
    Piece,
    /// Material.
    Material,
    /// Side to move.
    SideToMove,
    /// Game phase.
    Phase,
    /// Check, checkmate or stalemate.
    Status,
    /// The move that led to the position.
    PreviousMove,
    /// The move the game plays next.
    NextMove,
    /// Result of the game.
    Result,
    /// Passed pawns.
    PassedPawn,
    /// Isolated pawns.
    IsolatedPawn,
    /// Doubled pawns.
    DoubledPawns,
    /// Exposed king.
    ExposedKing,
    /// Bishop pair.
    BishopPair,
    /// Attacks.
    Attacks,
    /// Defends.
    Defends,
    /// Pins.
    Pins,
    /// Skewers.
    Skewers,
    /// Forks.
    Forks,
    /// Discovered attack, check or double check.
    Discovered,
    /// Double attack.
    DoubleAttack,
    /// Sacrifice.
    Sacrifice,
    /// Mate in N.
    Mate,
}

/// The submenus of "Add condition" (`pattern-cat-*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConditionGroup {
    /// Groups (AND / OR / NOT).
    Logic,
    /// Pieces and squares.
    Pieces,
    /// Relations between pieces.
    Relations,
    /// Tactics.
    Tactics,
    /// Structure and king safety.
    Structure,
    /// Move and state.
    Moves,
}

impl ConditionGroup {
    /// The Fluent key of the title of the submenu.
    pub fn title_key(self) -> &'static str {
        match self {
            ConditionGroup::Logic => "pattern-cat-logic",
            ConditionGroup::Pieces => "pattern-cat-pieces",
            ConditionGroup::Relations => "pattern-cat-relations",
            ConditionGroup::Tactics => "pattern-cat-tactics",
            ConditionGroup::Structure => "pattern-cat-structure",
            ConditionGroup::Moves => "pattern-cat-move",
        }
    }
}

impl ConditionKind {
    /// Every kind, in the order of the menu (grouped by [`Self::group`]).
    pub const ALL: [ConditionKind; 25] = [
        ConditionKind::All,
        ConditionKind::Any,
        ConditionKind::Not,
        ConditionKind::Piece,
        ConditionKind::Material,
        ConditionKind::Phase,
        ConditionKind::SideToMove,
        ConditionKind::Attacks,
        ConditionKind::Defends,
        ConditionKind::Pins,
        ConditionKind::Skewers,
        ConditionKind::Forks,
        ConditionKind::Discovered,
        ConditionKind::DoubleAttack,
        ConditionKind::Sacrifice,
        ConditionKind::Mate,
        ConditionKind::PassedPawn,
        ConditionKind::IsolatedPawn,
        ConditionKind::DoubledPawns,
        ConditionKind::ExposedKing,
        ConditionKind::BishopPair,
        ConditionKind::PreviousMove,
        ConditionKind::NextMove,
        ConditionKind::Status,
        ConditionKind::Result,
    ];

    /// The submenu the kind belongs to.
    pub fn group(self) -> ConditionGroup {
        use ConditionKind as K;
        match self {
            K::All | K::Any | K::Not => ConditionGroup::Logic,
            K::Piece | K::Material | K::Phase | K::SideToMove => ConditionGroup::Pieces,
            K::Attacks | K::Defends | K::Pins | K::Skewers | K::Forks => ConditionGroup::Relations,
            K::Discovered | K::DoubleAttack | K::Sacrifice | K::Mate => ConditionGroup::Tactics,
            K::PassedPawn | K::IsolatedPawn | K::DoubledPawns | K::ExposedKing | K::BishopPair => {
                ConditionGroup::Structure
            }
            K::PreviousMove | K::NextMove | K::Status | K::Result => ConditionGroup::Moves,
        }
    }

    /// The Fluent key of the name of the kind.
    pub fn title_key(self) -> &'static str {
        use ConditionKind as K;
        match self {
            K::All => "pattern-group-all",
            K::Any => "pattern-group-any",
            K::Not => "pattern-group-not",
            K::Piece => "pattern-cond-piece",
            K::Material => "pattern-cond-material",
            K::SideToMove => "pattern-cond-side",
            K::Phase => "pattern-cond-phase",
            K::Status => "pattern-cond-status",
            K::PreviousMove => "pattern-cond-previous-move",
            K::NextMove => "pattern-cond-next-move",
            K::Result => "pattern-cond-result",
            K::PassedPawn => "pattern-cond-passed-pawn",
            K::IsolatedPawn => "pattern-cond-isolated-pawn",
            K::DoubledPawns => "pattern-cond-doubled-pawns",
            K::ExposedKing => "pattern-cond-exposed-king",
            K::BishopPair => "pattern-cond-bishop-pair",
            K::Attacks => "pattern-cond-attacks",
            K::Defends => "pattern-cond-defends",
            K::Pins => "pattern-cond-pins",
            K::Skewers => "pattern-cond-skewers",
            K::Forks => "pattern-cond-forks",
            K::Discovered => "pattern-cond-discovered",
            K::DoubleAttack => "pattern-cond-double-attack",
            K::Sacrifice => "pattern-cond-sacrifice",
            K::Mate => "pattern-cond-mate",
        }
    }

    /// The kind of a condition.
    pub fn of(condition: &Condition) -> ConditionKind {
        use ConditionKind as K;
        match condition {
            Condition::All { .. } => K::All,
            Condition::Any { .. } => K::Any,
            Condition::Not { .. } => K::Not,
            Condition::Piece(_) => K::Piece,
            Condition::Material(_) => K::Material,
            Condition::SideToMove { .. } => K::SideToMove,
            Condition::Phase { .. } => K::Phase,
            Condition::Status { .. } => K::Status,
            Condition::Move(m) => match m.which {
                WhichMove::Previous => K::PreviousMove,
                WhichMove::Next => K::NextMove,
            },
            Condition::Result { .. } => K::Result,
            Condition::Feature(f) => match f.feature {
                Feature::PassedPawn => K::PassedPawn,
                Feature::IsolatedPawn => K::IsolatedPawn,
                Feature::DoubledPawns => K::DoubledPawns,
                Feature::ExposedKing => K::ExposedKing,
                Feature::BishopPair => K::BishopPair,
            },
            Condition::Relation(r) => match r.kind {
                RelKind::Attacks => K::Attacks,
                RelKind::Defends => K::Defends,
                RelKind::Pins => K::Pins,
                RelKind::Skewers => K::Skewers,
                RelKind::Forks => K::Forks,
            },
            Condition::Discovered(_) => K::Discovered,
            Condition::DoubleAttack(_) => K::DoubleAttack,
            Condition::Sacrifice(_) => K::Sacrifice,
            Condition::Mate(_) => K::Mate,
        }
    }

    /// A new condition of this kind with sensible, **valid** values. A group
    /// comes with one piece condition inside (an empty group is not valid).
    /// "Mate in N" starts in the cheap `played` mode.
    #[allow(clippy::too_many_lines)] // one flat match over the kinds reads best
    pub fn default_condition(self) -> Condition {
        use ConditionKind as K;
        let relation = |kind, behind| {
            Condition::Relation(RelationCond {
                kind,
                actor: PieceSel {
                    side: Who::White,
                    roles: Vec::new(),
                    region: Region::Anywhere,
                },
                target: TargetSel {
                    roles: Vec::new(),
                    region: Region::Anywhere,
                },
                behind,
                gain: Gain::Any,
                min_targets: 2,
                when: When::Any,
                new_this_move: false,
            })
        };
        let feature = |feature| {
            Condition::Feature(FeatureCond {
                feature,
                side: Who::White,
                region: Region::Anywhere,
                min: 1,
            })
        };
        let movement = |which| {
            Condition::Move(MoveCond {
                which,
                by: Who::Any,
                roles: Vec::new(),
                from: Region::Anywhere,
                to: Region::Anywhere,
                capture: Flag::Both,
                check: Flag::Both,
                promotion: Vec::new(),
                castling: None,
            })
        };
        match self {
            K::All => Condition::All {
                items: vec![K::Piece.default_condition()],
            },
            K::Any => Condition::Any {
                items: vec![K::Piece.default_condition()],
            },
            K::Not => Condition::Not {
                item: Box::new(K::Piece.default_condition()),
            },
            K::Piece => Condition::Piece(PieceCond {
                side: Who::White,
                roles: Vec::new(),
                region: Region::Anywhere,
                min: 1,
                max: None,
            }),
            K::Material => Condition::Material(MaterialCond {
                bishops: BishopColors::Either,
                ..MaterialCond::default()
            }),
            K::SideToMove => Condition::SideToMove { side: Side::White },
            K::Phase => Condition::Phase {
                phase: Phase::Middlegame,
            },
            K::Status => Condition::Status {
                status: Status::Check,
            },
            K::PreviousMove => movement(WhichMove::Previous),
            K::NextMove => movement(WhichMove::Next),
            K::Result => Condition::Result {
                results: vec![ResultKind::WhiteWins],
            },
            K::PassedPawn => feature(Feature::PassedPawn),
            K::IsolatedPawn => feature(Feature::IsolatedPawn),
            K::DoubledPawns => feature(Feature::DoubledPawns),
            K::ExposedKing => feature(Feature::ExposedKing),
            K::BishopPair => feature(Feature::BishopPair),
            K::Attacks => relation(RelKind::Attacks, None),
            K::Defends => relation(RelKind::Defends, None),
            K::Pins => relation(
                RelKind::Pins,
                Some(TargetSel {
                    roles: vec![RoleName::King],
                    region: Region::Anywhere,
                }),
            ),
            K::Skewers => relation(
                RelKind::Skewers,
                Some(TargetSel {
                    roles: Vec::new(),
                    region: Region::Anywhere,
                }),
            ),
            K::Forks => relation(RelKind::Forks, None),
            K::Discovered => Condition::Discovered(DiscoveredCond {
                discovery: DiscoveredKind::Attack,
                by: Who::White,
                roles: Vec::new(),
            }),
            K::DoubleAttack => Condition::DoubleAttack(DoubleAttackCond { by: Who::White }),
            K::Sacrifice => Condition::Sacrifice(SacrificeCond {
                by: Who::White,
                roles: Vec::new(),
                min_net: 2,
                outcome: Outcome::Either,
                within: 3,
            }),
            K::Mate => Condition::Mate(MateCond {
                by: Who::White,
                moves_min: 1,
                moves_max: 1,
                mode: MateMode::Played,
                forcing_only: true,
                max_nodes: 20_000,
            }),
        }
    }
}

/// The group kind of a node, if it is a group.
pub fn group_kind(condition: &Condition) -> Option<GroupKind> {
    match condition {
        Condition::All { .. } => Some(GroupKind::All),
        Condition::Any { .. } => Some(GroupKind::Any),
        Condition::Not { .. } => Some(GroupKind::Not),
        _ => None,
    }
}

/// The empty tree of a new pattern: an empty AND group.
pub(crate) fn empty_tree() -> Condition {
    Condition::All { items: Vec::new() }
}

fn child(condition: &Condition, index: usize) -> Option<&Condition> {
    match condition {
        Condition::All { items } | Condition::Any { items } => items.get(index),
        Condition::Not { item } if index == 0 => Some(item),
        _ => None,
    }
}

fn child_mut(condition: &mut Condition, index: usize) -> Option<&mut Condition> {
    match condition {
        Condition::All { items } | Condition::Any { items } => items.get_mut(index),
        Condition::Not { item } if index == 0 => Some(item),
        _ => None,
    }
}

/// The node at `path`.
pub fn node<'a>(root: &'a Condition, path: &[usize]) -> Option<&'a Condition> {
    path.iter().try_fold(root, |c, i| child(c, *i))
}

fn node_mut<'a>(root: &'a mut Condition, path: &[usize]) -> Option<&'a mut Condition> {
    path.iter().try_fold(root, |c, i| child_mut(c, *i))
}

/// Whether the tree respects the size limits of the model. The walk stops
/// at the first excess.
pub fn check_limits(root: &Condition) -> Result<(), EditRefusal> {
    fn walk(c: &Condition, depth: usize, nodes: &mut usize) -> Result<(), EditRefusal> {
        if depth > MAX_DEPTH {
            return Err(EditRefusal::TooDeep);
        }
        *nodes += 1;
        if *nodes > MAX_NODES {
            return Err(EditRefusal::TooManyNodes);
        }
        match c {
            Condition::All { items } | Condition::Any { items } => {
                if items.len() > MAX_GROUP_ITEMS {
                    return Err(EditRefusal::GroupFull);
                }
                items.iter().try_for_each(|i| walk(i, depth + 1, nodes))
            }
            Condition::Not { item } => walk(item, depth + 1, nodes),
            _ => Ok(()),
        }
    }
    walk(root, 1, &mut 0)
}

fn finish(root: Condition) -> Result<Condition, EditRefusal> {
    check_limits(&root)?;
    Ok(root)
}

/// Appends `condition` to the AND / OR group at `parent`.
pub(crate) fn add(
    root: &Condition,
    parent: &[usize],
    condition: Condition,
) -> Result<Condition, EditRefusal> {
    let mut out = root.clone();
    match node_mut(&mut out, parent).ok_or(EditRefusal::BadPath)? {
        Condition::All { items } | Condition::Any { items } => items.push(condition),
        _ => return Err(EditRefusal::NotAGroup),
    }
    finish(out)
}

/// Replaces the node at `path`.
pub(crate) fn replace(
    root: &Condition,
    path: &[usize],
    condition: Condition,
) -> Result<Condition, EditRefusal> {
    let mut out = root.clone();
    *node_mut(&mut out, path).ok_or(EditRefusal::BadPath)? = condition;
    finish(out)
}

/// Removes the node at `path` (the only child of a NOT takes the NOT with
/// it).
pub(crate) fn remove(root: &Condition, path: &[usize]) -> Result<Condition, EditRefusal> {
    let mut out = root.clone();
    detach(&mut out, path)?;
    finish(out)
}

/// Takes the node at `path` out of `root`.
fn detach(root: &mut Condition, path: &[usize]) -> Result<(), EditRefusal> {
    let mut path = path;
    if path.is_empty() {
        return Err(EditRefusal::RootCannotBeRemoved);
    }
    // A NOT without its child means nothing: remove the NOT instead.
    while let Some((_, parent)) = path.split_last() {
        let is_not = node(root, parent).is_some_and(|p| matches!(p, Condition::Not { .. }));
        if is_not {
            path = parent;
        } else {
            break;
        }
    }
    let Some((index, parent)) = path.split_last() else {
        return Err(EditRefusal::RootCannotBeRemoved);
    };
    match node_mut(root, parent).ok_or(EditRefusal::BadPath)? {
        Condition::All { items } | Condition::Any { items } => {
            if *index < items.len() {
                items.remove(*index);
                Ok(())
            } else {
                Err(EditRefusal::BadPath)
            }
        }
        _ => Err(EditRefusal::BadPath),
    }
}

/// Moves the node at `path` into the group `parent` at `index`; `parent`
/// is an address before the move.
pub(crate) fn move_node(
    root: &Condition,
    path: &[usize],
    parent: &[usize],
    index: usize,
) -> Result<Condition, EditRefusal> {
    if path.is_empty() {
        return Err(EditRefusal::RootCannotBeRemoved);
    }
    if parent.starts_with(path) {
        return Err(EditRefusal::MoveIntoItself);
    }
    let moving = node(root, path).ok_or(EditRefusal::BadPath)?.clone();
    let mut out = root.clone();
    let at = match node_mut(&mut out, parent).ok_or(EditRefusal::BadPath)? {
        Condition::All { items } | Condition::Any { items } => {
            let at = index.min(items.len());
            items.insert(at, moving);
            at
        }
        _ => return Err(EditRefusal::NotAGroup),
    };
    // The insertion shifted the later siblings under `parent`.
    let mut original = path.to_vec();
    if original.len() > parent.len()
        && original.starts_with(parent)
        && let Some(slot) = original.get_mut(parent.len())
        && *slot >= at
    {
        *slot += 1;
    }
    detach(&mut out, &original)?;
    finish(out)
}

/// Turns the group at `path` into another kind.
pub(crate) fn set_group_kind(
    root: &Condition,
    path: &[usize],
    kind: GroupKind,
) -> Result<Condition, EditRefusal> {
    let mut out = root.clone();
    let target = node_mut(&mut out, path).ok_or(EditRefusal::BadPath)?;
    let current = std::mem::replace(target, empty_tree());
    let result = match (current, kind) {
        (Condition::All { items } | Condition::Any { items }, GroupKind::All) => {
            Condition::All { items }
        }
        (Condition::All { items } | Condition::Any { items }, GroupKind::Any) => {
            Condition::Any { items }
        }
        (Condition::All { mut items } | Condition::Any { mut items }, GroupKind::Not) => {
            if items.len() == 1 {
                match items.pop() {
                    Some(item) => Condition::Not {
                        item: Box::new(item),
                    },
                    None => return Err(EditRefusal::NeedsOneItem),
                }
            } else {
                return Err(EditRefusal::NeedsOneItem);
            }
        }
        (Condition::Not { item }, GroupKind::Not) => Condition::Not { item },
        (Condition::Not { item }, GroupKind::All) => Condition::All { items: vec![*item] },
        (Condition::Not { item }, GroupKind::Any) => Condition::Any { items: vec![*item] },
        _ => return Err(EditRefusal::NotAGroup),
    };
    *target = result;
    finish(out)
}

/// Puts the node at `path` inside a new group of `kind`.
pub(crate) fn wrap(
    root: &Condition,
    path: &[usize],
    kind: GroupKind,
) -> Result<Condition, EditRefusal> {
    let mut out = root.clone();
    let target = node_mut(&mut out, path).ok_or(EditRefusal::BadPath)?;
    let inner = std::mem::replace(target, empty_tree());
    *target = match kind {
        GroupKind::All => Condition::All { items: vec![inner] },
        GroupKind::Any => Condition::Any { items: vec![inner] },
        GroupKind::Not => Condition::Not {
            item: Box::new(inner),
        },
    };
    finish(out)
}

/// Replaces the group at `path` by its only item.
pub(crate) fn unwrap_group(root: &Condition, path: &[usize]) -> Result<Condition, EditRefusal> {
    let mut out = root.clone();
    let target = node_mut(&mut out, path).ok_or(EditRefusal::BadPath)?;
    let only = match target {
        Condition::Not { item } => (**item).clone(),
        Condition::All { items } | Condition::Any { items } if items.len() == 1 => {
            items.first().cloned().ok_or(EditRefusal::NeedsOneItem)?
        }
        Condition::All { .. } | Condition::Any { .. } => return Err(EditRefusal::NeedsOneItem),
        _ => return Err(EditRefusal::NotAGroup),
    };
    *target = only;
    finish(out)
}

/// The set of squares of `slot` in `condition`.
pub(crate) fn region_mut(condition: &mut Condition, slot: RegionSlot) -> Option<&mut Region> {
    match (condition, slot) {
        (Condition::Piece(p), RegionSlot::Main) => Some(&mut p.region),
        (Condition::Feature(f), RegionSlot::Main) => Some(&mut f.region),
        (Condition::Move(m), RegionSlot::From) => Some(&mut m.from),
        (Condition::Move(m), RegionSlot::To) => Some(&mut m.to),
        (Condition::Relation(r), RegionSlot::Actor) => Some(&mut r.actor.region),
        (Condition::Relation(r), RegionSlot::Target) => Some(&mut r.target.region),
        (Condition::Relation(r), RegionSlot::Behind) => r.behind.as_mut().map(|b| &mut b.region),
        _ => None,
    }
}

/// Sets the set of squares `slot` of the node at `path`.
pub(crate) fn set_region(
    root: &Condition,
    path: &[usize],
    slot: RegionSlot,
    region: Region,
) -> Result<Condition, EditRefusal> {
    let mut out = root.clone();
    let node = node_mut(&mut out, path).ok_or(EditRefusal::BadPath)?;
    *region_mut(node, slot).ok_or(EditRefusal::NoSuchRegion)? = region;
    Ok(out)
}

/// Turns `square` on or off in the set of squares `slot` of the node at
/// `path`.
pub(crate) fn toggle_square(
    root: &Condition,
    path: &[usize],
    slot: RegionSlot,
    square: Square,
) -> Result<Condition, EditRefusal> {
    let mut out = root.clone();
    let node = node_mut(&mut out, path).ok_or(EditRefusal::BadPath)?;
    let region = region_mut(node, slot).ok_or(EditRefusal::NoSuchRegion)?;
    let mut list = match region {
        Region::Squares(list) => std::mem::take(list),
        _ => Vec::new(),
    };
    if let Some(at) = list.iter().position(|s| s.0 == square) {
        list.remove(at);
    } else {
        list.push(Sq(square));
    }
    *region = if list.is_empty() {
        Region::Anywhere
    } else {
        Region::Squares(list)
    };
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece() -> Condition {
        ConditionKind::Piece.default_condition()
    }

    fn all_of(items: Vec<Condition>) -> Condition {
        Condition::All { items }
    }

    #[test]
    fn every_kind_default_round_trips_its_kind() {
        for kind in ConditionKind::ALL {
            assert_eq!(ConditionKind::of(&kind.default_condition()), kind);
            assert!(check_limits(&kind.default_condition()).is_ok());
        }
    }

    #[test]
    fn add_remove_and_paths() {
        let root = all_of(vec![piece()]);
        let root = add(&root, &[], piece()).expect("add");
        assert_eq!(node(&root, &[1]), Some(&piece()));
        assert_eq!(add(&root, &[9], piece()), Err(EditRefusal::BadPath));
        assert_eq!(add(&root, &[0], piece()), Err(EditRefusal::NotAGroup));
        let root = remove(&root, &[0]).expect("remove");
        assert_eq!(root, all_of(vec![piece()]));
        assert_eq!(remove(&root, &[]), Err(EditRefusal::RootCannotBeRemoved));
        assert_eq!(remove(&root, &[5]), Err(EditRefusal::BadPath));
    }

    #[test]
    fn removing_the_child_of_a_not_removes_the_not() {
        let root = all_of(vec![
            piece(),
            Condition::Not {
                item: Box::new(piece()),
            },
        ]);
        let root = remove(&root, &[1, 0]).expect("remove");
        assert_eq!(root, all_of(vec![piece()]));
    }

    #[test]
    fn depth_and_node_limits_are_enforced() {
        // Depth: a leaf alone is 1, so six nested groups with a leaf is 7.
        let mut tree = piece();
        for _ in 0..5 {
            tree = all_of(vec![tree]);
        }
        assert!(check_limits(&tree).is_ok(), "depth 6 is allowed");
        assert_eq!(wrap(&tree, &[], GroupKind::Any), Err(EditRefusal::TooDeep));
        // Nodes: 1 root + 47 leaves = 48 is the limit.
        let full = all_of(vec![piece(); 24]);
        let two = all_of(vec![full.clone(), all_of(vec![piece(); 21])]);
        assert!(check_limits(&two).is_ok());
        assert_eq!(add(&two, &[1], piece()), Err(EditRefusal::TooManyNodes));
        assert_eq!(add(&full, &[], piece()), Err(EditRefusal::GroupFull));
    }

    #[test]
    fn move_reorders_and_refuses_moving_into_itself() {
        let a = Condition::Status {
            status: Status::Check,
        };
        let b = Condition::Phase {
            phase: Phase::Endgame,
        };
        let root = all_of(vec![a.clone(), b.clone()]);
        let moved = move_node(&root, &[0], &[], 2).expect("move");
        assert_eq!(moved, all_of(vec![b.clone(), a.clone()]));
        let nested = all_of(vec![all_of(vec![a.clone()]), b.clone()]);
        assert_eq!(
            move_node(&nested, &[0], &[0], 0),
            Err(EditRefusal::MoveIntoItself)
        );
        let out = move_node(&nested, &[1], &[0], 0).expect("move");
        assert_eq!(out, all_of(vec![all_of(vec![b, a])]));
        assert_eq!(
            move_node(&nested, &[], &[0], 0),
            Err(EditRefusal::RootCannotBeRemoved)
        );
        assert_eq!(
            move_node(&nested, &[0], &[1], 0),
            Err(EditRefusal::NotAGroup)
        );
    }

    #[test]
    fn group_kinds_wrap_and_unwrap() {
        let root = all_of(vec![piece()]);
        let any = set_group_kind(&root, &[], GroupKind::Any).expect("any");
        assert!(matches!(any, Condition::Any { .. }));
        let not = set_group_kind(&any, &[], GroupKind::Not).expect("not");
        assert!(matches!(not, Condition::Not { .. }));
        assert_eq!(unwrap_group(&not, &[]), Ok(piece()));
        let two = all_of(vec![piece(), piece()]);
        assert_eq!(
            set_group_kind(&two, &[], GroupKind::Not),
            Err(EditRefusal::NeedsOneItem)
        );
        assert_eq!(unwrap_group(&two, &[]), Err(EditRefusal::NeedsOneItem));
        assert_eq!(
            set_group_kind(&two, &[0], GroupKind::Any),
            Err(EditRefusal::NotAGroup)
        );
        let wrapped = wrap(&two, &[0], GroupKind::Not).expect("wrap");
        assert!(matches!(node(&wrapped, &[0]), Some(Condition::Not { .. })));
    }

    #[test]
    fn squares_toggle_on_and_off() {
        let root = all_of(vec![piece()]);
        let e4 = Square::E4;
        let on = toggle_square(&root, &[0], RegionSlot::Main, e4).expect("on");
        let Some(Condition::Piece(p)) = node(&on, &[0]) else {
            panic!("piece");
        };
        assert_eq!(p.region, Region::Squares(vec![Sq(e4)]));
        let off = toggle_square(&on, &[0], RegionSlot::Main, e4).expect("off");
        assert_eq!(off, root);
        assert_eq!(
            toggle_square(&root, &[0], RegionSlot::From, e4),
            Err(EditRefusal::NoSuchRegion)
        );
        assert_eq!(
            set_region(&root, &[3], RegionSlot::Main, Region::Anywhere),
            Err(EditRefusal::BadPath)
        );
    }
}
