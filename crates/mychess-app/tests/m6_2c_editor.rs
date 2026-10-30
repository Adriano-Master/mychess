//! Contract tests of the presets, the parameters, the editor of the tree,
//! the sentences of the cards and the preferences of the search for
//! patterns (spec `m6-2-taticas-e-padroes.md`: D3279, D3282, D3290 as
//! extended by slice c, rules 22 and 23, the limits of the model).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_2c_common;

use m6_2c_common::*;
use mychess_app::pattern_search::model::{
    Condition, DiscoveredKind, MateMode, Outcome, Region, RoleName, Scope, Sq,
};
use mychess_app::{
    ConditionKind, EditRefusal, GameMessage, GroupKind, ParamValue, PatternEdit, PatternIssue,
    RegionSlot, SearchOp, SentenceArg,
};
use mychess_core::Square;
use mychess_db::pattern::presets;

fn window() -> (Rig, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    rig.pat(Pat::Open);
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    (rig, dir)
}

fn edit(rig: &mut Rig, e: PatternEdit) {
    rig.pat(Pat::Edit(e));
}

fn add(rig: &mut Rig, parent: &[usize], kind: ConditionKind) {
    edit(
        rig,
        PatternEdit::Add {
            parent: parent.to_vec(),
            kind,
        },
    );
}

fn issue(rig: &Rig) -> Option<PatternSearchNotice> {
    rig.view().issue
}

// ---------------------------------------------------------------- presets

#[test]
fn the_list_of_presets_is_the_catalogue_of_the_database() {
    let (rig, _dir) = window();
    let view = rig.view();
    let all = presets::all();
    assert_eq!(view.presets.len(), all.len());
    assert_eq!(view.presets.len(), 21);
    for (entry, preset) in view.presets.iter().zip(&all) {
        assert_eq!(entry.id, preset.id);
        assert_eq!(entry.name_key, preset.name_key);
        assert_eq!(entry.params, preset.params);
        assert!(entry.name_key.starts_with("pattern-preset-"));
    }
    // The first one is picked when the window opens.
    assert_eq!(view.selected_preset.as_deref(), Some("mate_in"));
}

#[test]
fn picking_a_preset_resets_the_parameters_keeps_the_colour_and_takes_its_scope() {
    let (mut rig, _dir) = window();
    rig.pat(Pat::SelectPreset("mate_in".into()));
    assert_eq!(rig.view().scope.min_move, 0);
    rig.pat(Pat::SetParam(ParamValue::By(Who::Black)));
    rig.pat(Pat::SetParam(ParamValue::N(4)));
    rig.pat(Pat::SelectPreset("fork".into()));
    let view = rig.view();
    assert_eq!(view.selected_preset.as_deref(), Some("fork"));
    assert_eq!(view.params.by, Who::Black);
    assert_eq!(view.params.n, Params::default().n);
    assert_eq!(view.scope, Scope::default());
    assert_eq!(rig.state.prefs.pattern_search.last_preset, "fork");
}

#[test]
fn an_unknown_preset_is_refused_and_nothing_changes() {
    let (mut rig, _dir) = window();
    rig.pat(Pat::SelectPreset("fork".into()));
    rig.pat(Pat::SelectPreset("no_such".into()));
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::UnknownPreset("no_such".into()))
    );
    assert_eq!(rig.view().selected_preset.as_deref(), Some("fork"));
    assert_eq!(rig.state.prefs.pattern_search.last_preset, "fork");
}

#[test]
fn every_preset_runs_with_its_default_parameters() {
    let (mut rig, _dir) = window();
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    for entry in rig.view().presets {
        rig.pat(Pat::SelectPreset(entry.id.into()));
        assert!(rig.view().param_issue.is_none(), "{}", entry.id);
        // Reached `Start`, so the pattern built; no database open here
        // beyond the Clipbase, which is fine.
        let effects = rig.raw(Message::PatternSearch(Pat::Start));
        assert_eq!(effects.len(), 1, "{}: {effects:?}", entry.id);
        assert!(matches!(effects[0], Effect::PatternSearch { .. }));
        assert!(
            rig.view().issue.is_none(),
            "{}: {:?}",
            entry.id,
            rig.view().issue
        );
        rig.pat(Pat::Stop);
        rig.pat(Pat::Close);
        rig.pat(Pat::Open);
    }
}

#[test]
fn a_parameter_out_of_range_is_refused_never_approximated() {
    let (mut rig, _dir) = window();
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    rig.pat(Pat::SelectPreset("mate_in".into()));
    for (value, field) in [
        (ParamValue::N(9), "n"),
        (ParamValue::N(0), "n"),
        (ParamValue::MaxNodes(10), "max_nodes"),
        (ParamValue::By(Who::Mover), "by"),
    ] {
        rig.pat(Pat::SetParam(value));
        let expected = PatternSearchNotice::InvalidParam(field.into());
        assert_eq!(rig.view().param_issue, Some(expected.clone()), "{field}");
        // The value is kept as typed ...
        assert!(rig.raw(Message::PatternSearch(Pat::Start)).is_empty());
        assert_eq!(issue(&rig), Some(expected));
        // ... and Customize refuses too, leaving the draft alone.
        let draft = rig.view().draft;
        rig.pat(Pat::Customize {
            name: String::new(),
            description: String::new(),
        });
        assert_eq!(rig.view().draft, draft);
        assert_eq!(rig.view().tab, PatternTab::Presets);
        rig.pat(Pat::SelectPreset("mate_in".into()));
    }
    assert_eq!(
        PatternSearchNotice::InvalidParam("n".into()).message_key(),
        "pattern-error-invalid-param"
    );
    // Another preset, another set of fields.
    rig.pat(Pat::SelectPreset("sacrifice".into()));
    rig.pat(Pat::SetParam(ParamValue::By(Who::White)));
    rig.pat(Pat::SetParam(ParamValue::MinNet(21)));
    assert_eq!(
        rig.view().param_issue,
        Some(PatternSearchNotice::InvalidParam("min_net".into()))
    );
    rig.pat(Pat::SetParam(ParamValue::MinNet(5)));
    rig.pat(Pat::SetParam(ParamValue::Within(0)));
    assert_eq!(
        rig.view().param_issue,
        Some(PatternSearchNotice::InvalidParam("within".into()))
    );
}

#[test]
fn every_kind_of_parameter_reaches_the_pattern() {
    let (mut rig, _dir) = window();
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    rig.pat(Pat::SelectPreset("sacrifice".into()));
    let values = [
        ParamValue::By(Who::White),
        ParamValue::Roles(vec![RoleName::Queen]),
        ParamValue::MinNet(5),
        ParamValue::Outcome(Outcome::Mate),
        ParamValue::Within(2),
        ParamValue::RequireWin(true),
    ];
    for value in values {
        rig.pat(Pat::SetParam(value));
    }
    let want = presets::build(
        "sacrifice",
        &Params {
            by: Who::White,
            roles: vec![RoleName::Queen],
            min_net: 5,
            outcome: Outcome::Mate,
            within: 2,
            require_win: true,
            ..Params::default()
        },
    )
    .unwrap();
    rig.pat(Pat::Customize {
        name: String::new(),
        description: String::new(),
    });
    let mut draft = rig.view().draft;
    draft.scope = want.scope.clone();
    assert_eq!(draft, want);
    // The remaining parameters are plain setters.
    rig.pat(Pat::SetParam(ParamValue::MateMode(MateMode::Played)));
    rig.pat(Pat::SetParam(ParamValue::ForcingOnly(true)));
    rig.pat(Pat::SetParam(ParamValue::MaxNodes(5000)));
    rig.pat(Pat::SetParam(ParamValue::AgainstKing(false)));
    rig.pat(Pat::SetParam(ParamValue::Discovery(DiscoveredKind::Check)));
    rig.pat(Pat::SetParam(ParamValue::TargetRoles(vec![RoleName::Rook])));
    rig.pat(Pat::SetParam(ParamValue::MinOwnRank(Some(6))));
    let p = rig.view().params;
    assert_eq!(p.mate_mode, MateMode::Played);
    assert!(p.forcing_only && !p.against_king);
    assert_eq!((p.max_nodes, p.min_own_rank), (5000, Some(6)));
    assert_eq!(p.discovery, DiscoveredKind::Check);
    assert_eq!(p.target_roles, vec![RoleName::Rook]);
}

#[test]
fn customizing_copies_the_preset_with_the_translated_texts() {
    let (mut rig, _dir) = window();
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    rig.pat(Pat::SelectPreset("fork".into()));
    rig.pat(Pat::SetParam(ParamValue::By(Who::White)));
    rig.pat(Pat::SetParam(ParamValue::Roles(vec![RoleName::Knight])));
    rig.pat(Pat::Customize {
        name: "Garfo de cavalo".into(),
        description: "Um cavalo ataca duas peças.".into(),
    });
    let view = rig.view();
    assert_eq!(view.tab, PatternTab::Advanced);
    assert_eq!(view.draft.name, "Garfo de cavalo");
    assert_eq!(view.draft.description, "Um cavalo ataca duas peças.");
    let built = presets::build(
        "fork",
        &Params {
            by: Who::White,
            roles: vec![RoleName::Knight],
            ..Params::default()
        },
    )
    .unwrap();
    assert_eq!(view.draft.root, built.root);
    assert_eq!(view.tree.len(), 1);
    // The preset is untouched: editing the copy does not change what the
    // first tab builds.
    edit(
        &mut rig,
        PatternEdit::Wrap {
            path: vec![],
            kind: GroupKind::Not,
        },
    );
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    let Effect::PatternSearch { pattern, .. } = rig.start_effect() else {
        panic!("search")
    };
    assert_eq!(pattern.root, built.root);
    // A name that does not fit is refused.
    rig.pat(Pat::Customize {
        name: "x".repeat(81),
        description: String::new(),
    });
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::InvalidParam("name".into()))
    );
}

// ------------------------------------------------------- the tree editor

#[test]
fn adding_and_removing_cards_keeps_the_selection_on_the_new_one() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Piece);
    add(&mut rig, &[], ConditionKind::Phase);
    let view = rig.view();
    assert_eq!(view.tree.len(), 3);
    assert_eq!(view.tree[0].kind, ConditionKind::All);
    assert_eq!(view.tree[0].children, 2);
    assert_eq!(view.tree[2].path, vec![1]);
    assert!(view.tree[2].selected);
    assert_eq!(view.selection, Some(vec![1]));
    assert!(view.tree_issue.is_none());
    edit(&mut rig, PatternEdit::Remove(vec![0]));
    let view = rig.view();
    assert_eq!(view.tree.len(), 2);
    assert_eq!(view.tree[1].kind, ConditionKind::Phase);
    // The selection pointed at a node that moved: it is dropped, not left
    // dangling.
    assert_eq!(view.selection, None);
    edit(&mut rig, PatternEdit::Select(Some(vec![0])));
    assert_eq!(rig.view().selection, Some(vec![0]));
    edit(&mut rig, PatternEdit::Select(Some(vec![7])));
    assert_eq!(rig.view().selection, None);
}

#[test]
fn bad_paths_and_wrong_targets_are_refused_without_panic() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Piece);
    let cases = [
        (PatternEdit::Remove(vec![9, 9]), EditRefusal::BadPath),
        (
            PatternEdit::Remove(vec![]),
            EditRefusal::RootCannotBeRemoved,
        ),
        (
            PatternEdit::Add {
                parent: vec![0],
                kind: ConditionKind::Piece,
            },
            EditRefusal::NotAGroup,
        ),
        (
            PatternEdit::Add {
                parent: vec![3],
                kind: ConditionKind::Piece,
            },
            EditRefusal::BadPath,
        ),
        (
            PatternEdit::Replace {
                path: vec![0, 0, 0],
                condition: Box::new(Condition::Phase {
                    phase: mychess_app::pattern_search::model::Phase::Endgame,
                }),
            },
            EditRefusal::BadPath,
        ),
        (
            PatternEdit::Move {
                path: vec![],
                parent: vec![],
                index: 0,
            },
            EditRefusal::RootCannotBeRemoved,
        ),
        (
            PatternEdit::Move {
                path: vec![0],
                parent: vec![0],
                index: 0,
            },
            EditRefusal::MoveIntoItself,
        ),
        (
            PatternEdit::SetGroupKind {
                path: vec![0],
                kind: GroupKind::Any,
            },
            EditRefusal::NotAGroup,
        ),
        (PatternEdit::Unwrap(vec![]), EditRefusal::NeedsOneItem),
        (
            PatternEdit::SetRegion {
                path: vec![0],
                slot: RegionSlot::From,
                region: Region::Anywhere,
            },
            EditRefusal::NoSuchRegion,
        ),
        (
            PatternEdit::ToggleSquare {
                path: vec![9],
                slot: RegionSlot::Main,
                square: Square::E4,
            },
            EditRefusal::BadPath,
        ),
    ];
    // A second card so that `Unwrap([])` sees two items.
    add(&mut rig, &[], ConditionKind::Phase);
    let tree_two = rig.view().draft.root;
    for (e, why) in cases {
        edit(&mut rig, e);
        assert_eq!(
            issue(&rig),
            Some(PatternSearchNotice::EditRefused(why)),
            "{why:?}"
        );
        assert_eq!(rig.view().draft.root, tree_two, "{why:?}");
        assert!(why.message_key().starts_with("pattern-edit-refused-"));
        rig.pat(Pat::SelectTab(PatternTab::Advanced));
    }
}

#[test]
fn the_depth_limit_of_six_is_enforced() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Piece);
    // A leaf alone is depth 1: the root group makes 2; four wraps reach 6.
    let mut path = vec![0];
    for _ in 0..4 {
        edit(
            &mut rig,
            PatternEdit::Wrap {
                path: path.clone(),
                kind: GroupKind::Any,
            },
        );
        assert!(issue(&rig).is_none(), "{:?}", issue(&rig));
    }
    let deepest = rig.view().draft.root;
    // One more level is refused and the tree is as it was.
    edit(
        &mut rig,
        PatternEdit::Wrap {
            path: path.clone(),
            kind: GroupKind::Any,
        },
    );
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::EditRefused(EditRefusal::TooDeep))
    );
    assert_eq!(rig.view().draft.root, deepest);
    // Adding a group below the deepest group is refused too.
    path.extend([0, 0, 0]);
    edit(
        &mut rig,
        PatternEdit::Add {
            parent: path,
            kind: ConditionKind::All,
        },
    );
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::EditRefused(EditRefusal::TooDeep))
    );
    assert_eq!(rig.view().draft.root, deepest);
}

#[test]
fn the_limits_of_nodes_and_items_are_enforced() {
    let (mut rig, _dir) = window();
    // Each AND card comes with one piece inside: two nodes. The root and
    // 23 of them are 47 nodes; the 24th would make 49.
    for _ in 0..23 {
        add(&mut rig, &[], ConditionKind::All);
        assert!(issue(&rig).is_none());
    }
    assert_eq!(rig.view().tree.len(), 47);
    add(&mut rig, &[], ConditionKind::All);
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::EditRefused(EditRefusal::TooManyNodes))
    );
    assert_eq!(rig.view().tree.len(), 47);
    // One leaf more is exactly 48 nodes ...
    add(&mut rig, &[], ConditionKind::Phase);
    assert!(issue(&rig).is_none());
    assert_eq!(rig.view().tree.len(), 48);
    // ... and the 25th item of a group is over the limit of items.
    edit(&mut rig, PatternEdit::Clear);
    for _ in 0..24 {
        add(&mut rig, &[], ConditionKind::Phase);
    }
    add(&mut rig, &[], ConditionKind::Phase);
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::EditRefused(EditRefusal::GroupFull))
    );
    assert_eq!(rig.view().tree.len(), 25);
}

#[test]
fn a_hostile_deep_replacement_is_refused_without_overflowing_the_stack() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Piece);
    let mut deep = ConditionKind::Piece.default_condition();
    for _ in 0..5_000 {
        deep = Condition::Not {
            item: Box::new(deep),
        };
    }
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(deep),
        },
    );
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::EditRefused(EditRefusal::TooDeep))
    );
    assert_eq!(rig.view().tree.len(), 2);
    // Dropping the deep value is left to the thread that built it; keep the
    // test from recursing in the destructor.
}

#[test]
fn groups_can_be_turned_wrapped_unwrapped_and_cards_moved() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Piece);
    add(&mut rig, &[], ConditionKind::Phase);
    add(&mut rig, &[], ConditionKind::Status);
    // OR instead of AND.
    edit(
        &mut rig,
        PatternEdit::SetGroupKind {
            path: vec![],
            kind: GroupKind::Any,
        },
    );
    assert_eq!(rig.view().tree[0].kind, ConditionKind::Any);
    // NOT needs exactly one item.
    edit(
        &mut rig,
        PatternEdit::SetGroupKind {
            path: vec![],
            kind: GroupKind::Not,
        },
    );
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::EditRefused(EditRefusal::NeedsOneItem))
    );
    // Wrap the phase in a NOT, then move the status into an AND group.
    edit(
        &mut rig,
        PatternEdit::Wrap {
            path: vec![1],
            kind: GroupKind::Not,
        },
    );
    let kinds: Vec<_> = rig.view().tree.iter().map(|n| n.kind).collect();
    assert_eq!(
        kinds,
        [
            ConditionKind::Any,
            ConditionKind::Piece,
            ConditionKind::Not,
            ConditionKind::Phase,
            ConditionKind::Status
        ]
    );
    add(&mut rig, &[], ConditionKind::All);
    // The new AND is item 3 (with one piece inside); move the status in.
    edit(
        &mut rig,
        PatternEdit::Move {
            path: vec![2],
            parent: vec![3],
            index: 0,
        },
    );
    assert!(issue(&rig).is_none(), "{:?}", issue(&rig));
    let root = rig.view().draft.root;
    let Condition::Any { items } = &root else {
        panic!("root");
    };
    assert_eq!(items.len(), 3);
    let Condition::All { items: inner } = &items[2] else {
        panic!("inner");
    };
    assert!(matches!(inner[0], Condition::Status { .. }));
    // Removing the child of a NOT removes the NOT.
    edit(&mut rig, PatternEdit::Remove(vec![1, 0]));
    assert_eq!(
        rig.view()
            .tree
            .iter()
            .filter(|n| n.kind == ConditionKind::Not)
            .count(),
        0
    );
    // Unwrap an AND that has one item.
    edit(&mut rig, PatternEdit::Remove(vec![1, 0]));
    edit(&mut rig, PatternEdit::Unwrap(vec![1]));
    assert!(issue(&rig).is_none(), "{:?}", issue(&rig));
}

#[test]
fn the_fields_of_a_card_are_edited_by_replacing_the_card() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Forks);
    let mut card = rig.view().tree[1].condition.clone();
    if let Condition::Relation(r) = &mut card {
        r.actor.roles = vec![RoleName::Knight];
        r.min_targets = 3;
    }
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(card.clone()),
        },
    );
    assert_eq!(rig.view().tree[1].condition, card);
    assert!(rig.view().tree_issue.is_none());
    // A value the model rejects is accepted here (so that the user can keep
    // typing) but flagged and not runnable.
    let mut wrong = card;
    if let Condition::Relation(r) = &mut wrong {
        r.min_targets = 12;
    }
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(wrong),
        },
    );
    assert_eq!(rig.view().issue_path, Some(vec![0]));
    let Some(PatternSearchNotice::InvalidPattern(PatternIssue::InvalidValue { path })) =
        rig.view().tree_issue
    else {
        panic!("{:?}", rig.view().tree_issue);
    };
    assert_eq!(path, "root.items[0].min_targets");
    assert!(rig.raw(Message::PatternSearch(Pat::Start)).is_empty());
}

#[test]
fn squares_are_chosen_one_by_one_by_rectangle_or_from_the_marks() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Piece);
    for sq in [Square::E4, Square::D4] {
        edit(
            &mut rig,
            PatternEdit::ToggleSquare {
                path: vec![0],
                slot: RegionSlot::Main,
                square: sq,
            },
        );
    }
    let region = |rig: &Rig| match &rig.view().tree[1].condition {
        Condition::Piece(p) => p.region.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        region(&rig),
        Region::Squares(vec![Sq(Square::E4), Sq(Square::D4)])
    );
    edit(
        &mut rig,
        PatternEdit::ToggleSquare {
            path: vec![0],
            slot: RegionSlot::Main,
            square: Square::E4,
        },
    );
    edit(
        &mut rig,
        PatternEdit::ToggleSquare {
            path: vec![0],
            slot: RegionSlot::Main,
            square: Square::D4,
        },
    );
    // No square left: anywhere (an empty set is not valid).
    assert_eq!(region(&rig), Region::Anywhere);
    edit(
        &mut rig,
        PatternEdit::SetRegion {
            path: vec![0],
            slot: RegionSlot::Main,
            region: Region::Rect {
                from: Sq(Square::C3),
                to: Sq(Square::F6),
            },
        },
    );
    assert!(matches!(region(&rig), Region::Rect { .. }));
    // No marks on the board: refused.
    edit(
        &mut rig,
        PatternEdit::UseMarks {
            path: vec![0],
            slot: RegionSlot::Main,
        },
    );
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::EditRefused(EditRefusal::NoMarks))
    );
    // The squares marked in the comment at the cursor (arrows are not
    // squares).
    let root = rig.state.session.game().root();
    rig.send(Message::Game(GameMessage::SetComment {
        node: root,
        text: "[%csl Gd4,Re5][%cal Ge2e4]".into(),
    }));
    edit(
        &mut rig,
        PatternEdit::UseMarks {
            path: vec![0],
            slot: RegionSlot::Main,
        },
    );
    assert_eq!(
        region(&rig),
        Region::Squares(vec![Sq(Square::D4), Sq(Square::E5)])
    );
}

#[test]
fn a_new_pattern_keeps_the_scope_and_set_meta_names_it() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Piece);
    edit(
        &mut rig,
        PatternEdit::SetMeta {
            name: "N".into(),
            description: "D".into(),
        },
    );
    assert_eq!(rig.view().draft.name, "N");
    rig.pat(Pat::SetScope(Scope {
        hold: 3,
        ..Scope::default()
    }));
    edit(&mut rig, PatternEdit::Clear);
    let view = rig.view();
    assert_eq!(view.tree.len(), 1);
    assert_eq!(view.draft.name, "N");
    assert_eq!(view.draft.scope.hold, 3);
}

// -------------------------------------------------------------- sentences

#[test]
fn every_card_has_a_sentence_made_of_keys_and_arguments() {
    let (mut rig, _dir) = window();
    for kind in ConditionKind::ALL {
        if matches!(
            kind,
            ConditionKind::All | ConditionKind::Any | ConditionKind::Not
        ) {
            continue;
        }
        add(&mut rig, &[], kind);
    }
    edit(&mut rig, PatternEdit::Clear);
    // One at a time, to stay under the limits.
    for kind in ConditionKind::ALL {
        edit(&mut rig, PatternEdit::Clear);
        add(&mut rig, &[], kind);
        let card = rig.view().tree[1].clone();
        assert_eq!(card.kind, kind);
        assert!(card.sentence.key.starts_with("pattern-"), "{kind:?}");
        assert!(!kind.title_key().is_empty());
        assert!(rig.view().tree_issue.is_none(), "{kind:?}");
    }
    // A pin names the actor, the target and the piece behind.
    edit(&mut rig, PatternEdit::Clear);
    add(&mut rig, &[], ConditionKind::Pins);
    let sentence = rig.view().tree[1].sentence.clone();
    assert_eq!(sentence.key, "pattern-sentence-pin");
    let Some(SentenceArg::Phrase(behind)) = sentence.arg("behind") else {
        panic!("behind");
    };
    assert_eq!(
        behind.arg("piece"),
        Some(&SentenceArg::List(vec![SentenceArg::Key(
            "pattern-role-king"
        )]))
    );
    // A group's sentence is its title.
    assert_eq!(rig.view().tree[0].sentence.key, "pattern-group-all");
    assert_eq!(
        rig.view().tree[0].sentence.arg("count"),
        Some(&SentenceArg::Number(1))
    );
}

#[test]
fn the_sentence_follows_the_model_not_a_fixed_text() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Piece);
    let mut card = rig.view().tree[1].condition.clone();
    if let Condition::Piece(p) = &mut card {
        p.roles = vec![RoleName::Rook, RoleName::Queen];
        p.region = Region::Squares(vec![Sq(Square::A1), Sq(Square::H1)]);
        p.min = 0;
        p.max = Some(0);
    }
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(card),
        },
    );
    let sentence = rig.view().tree[1].sentence.clone();
    assert_eq!(sentence.key, "pattern-sentence-piece-none");
    assert_eq!(
        sentence.arg("piece"),
        Some(&SentenceArg::List(vec![
            SentenceArg::Key("pattern-role-rook"),
            SentenceArg::Key("pattern-role-queen"),
        ]))
    );
    let Some(SentenceArg::Phrase(place)) = sentence.arg("where") else {
        panic!("where");
    };
    assert_eq!(place.key, "pattern-region-squares");
    assert_eq!(
        place.arg("squares"),
        Some(&SentenceArg::Text("a1, h1".to_owned()))
    );
}

// ------------------------------------------------------------ preferences

#[test]
fn the_operation_the_scope_and_the_last_preset_are_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    let mut rig = Rig::new(dir.path());
    rig.state = {
        let mut s = AppState::with_preferences_path(Some(path));
        s.set_pattern_dir(Some(dir.path().join("patterns")));
        s
    };
    rig.pat(Pat::Open);
    rig.pat(Pat::SetOp(SearchOp::Or));
    rig.pat(Pat::SelectPreset("pin".into()));
    rig.pat(Pat::SetScope(Scope {
        min_move: 5,
        max_move: 30,
        hold: 99,
        end_only: true,
        ignore_colors: true,
    }));
    let prefs = &rig.state.prefs.pattern_search;
    assert_eq!(prefs.op, SearchOp::Or);
    assert_eq!(prefs.last_preset, "pin");
    assert_eq!(
        (prefs.min_move, prefs.max_move, prefs.hold),
        (5, 30, 40),
        "hold is clamped"
    );
    assert!(prefs.end_only && prefs.ignore_colors);
    // The text of the preferences file has the table.
    let text = mychess_app::render_preferences(&rig.state.prefs);
    assert!(text.contains("[pattern_search]"));
    assert!(text.contains("last_preset = \"pin\""));
    // A new window of a state built from those preferences shows them.
    let loaded = mychess_app::load_preferences(&text);
    let mut fresh = Rig::new(dir.path());
    fresh.state.prefs = loaded.prefs;
    fresh.pat(Pat::Open);
    let view = fresh.view();
    assert_eq!(view.op, SearchOp::Or);
    assert_eq!(view.scope.max_move, 30);
    assert_eq!(view.selected_preset.as_deref(), Some("pin"));
    // Starting a search remembers the scope in use as well.
    fresh.pat(Pat::SetScope(Scope::default()));
    assert_eq!(fresh.state.prefs.pattern_search.max_move, 999);
}
