//! Extra contract tests of the presets, the typed parameters, the editor of
//! the tree, the sentences of the cards and the preferences of the search
//! for patterns (spec `m6-2-taticas-e-padroes.md`: D3279, D3282, D3290 as
//! extended by D3299 items 27-29, rules 22 and 23 and the limits of the
//! model). They complement `m6_2c_editor.rs`.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_2c_common;

use m6_2c_common::*;
use mychess_app::pattern_search::model::{
    Condition, DiscoveredKind, MateCond, Phase, Region, RoleName, Scope, Sq, Status,
};
use mychess_app::{
    ConditionKind, EditRefusal, GameMessage, GroupKind, ParamValue, PatternEdit, PatternIssue,
    RegionSlot, SearchOp, Sentence, SentenceArg, load_preferences, render_preferences,
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

fn refused(rig: &Rig) -> Option<EditRefusal> {
    match issue(rig) {
        Some(PatternSearchNotice::EditRefused(why)) => Some(why),
        _ => None,
    }
}

fn root(rig: &Rig) -> Condition {
    rig.view().draft.root
}

fn leaf(kind: ConditionKind) -> Condition {
    kind.default_condition()
}

fn all(items: Vec<Condition>) -> Condition {
    Condition::All { items }
}

// ---------------------------------------------------------------- presets

#[test]
fn each_of_the_21_presets_is_selectable_with_its_scope_and_customizes_to_its_pattern() {
    let (mut rig, _dir) = window();
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    let entries = rig.view().presets;
    assert_eq!(entries.len(), 21);
    let mut ids = std::collections::BTreeSet::new();
    for entry in &entries {
        assert!(ids.insert(entry.id), "duplicate {}", entry.id);
        assert!(entry.name_key.starts_with("pattern-preset-"), "{entry:?}");
        rig.pat(Pat::SetParam(ParamValue::By(Who::White)));
        rig.pat(Pat::SelectPreset(entry.id.into()));
        let view = rig.view();
        assert_eq!(view.selected_preset.as_deref(), Some(entry.id));
        assert!(view.param_issue.is_none(), "{}", entry.id);
        assert!(view.issue.is_none(), "{}", entry.id);
        // The colour survives the choice (the rest goes back to default).
        assert_eq!(view.params.by, Who::White, "{}", entry.id);
        // The scope is the one the preset brings (D3279).
        let built = presets::build(
            entry.id,
            &Params {
                by: Who::White,
                ..Params::default()
            },
        )
        .unwrap();
        assert_eq!(view.scope, built.scope, "{}", entry.id);
        // Customize copies the pattern to the other tab.
        rig.pat(Pat::Customize {
            name: format!("Name of {}", entry.id),
            description: "Texto traduzido".into(),
        });
        let view = rig.view();
        assert_eq!(view.tab, PatternTab::Advanced, "{}", entry.id);
        assert_eq!(view.draft.root, built.root, "{}", entry.id);
        assert_eq!(view.draft.name, format!("Name of {}", entry.id));
        assert_eq!(view.draft.description, "Texto traduzido");
        assert!(view.tree_issue.is_none(), "{}", entry.id);
        assert!(!view.tree.is_empty());
        // The preset is intact: the first tab still builds the same.
        rig.pat(Pat::SelectTab(PatternTab::Presets));
        let Effect::PatternSearch { pattern, .. } = rig.start_effect() else {
            panic!("search")
        };
        assert_eq!(pattern.root, built.root, "{}", entry.id);
        rig.pat(Pat::Stop);
    }
}

#[test]
fn customizing_with_blank_texts_keeps_the_names_of_the_preset() {
    let (mut rig, _dir) = window();
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    rig.pat(Pat::SelectPreset("skewer".into()));
    let built = presets::build("skewer", &Params::default()).unwrap();
    rig.pat(Pat::Customize {
        name: "   ".into(),
        description: String::new(),
    });
    assert_eq!(rig.view().draft.name, built.name);
    assert_eq!(rig.view().draft.description, built.description);
}

#[test]
fn parameters_at_the_edge_of_the_range_are_accepted_and_one_step_out_is_refused() {
    let (mut rig, _dir) = window();
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    let cases: [(&str, ParamValue, &str, bool); 20] = [
        ("mate_in", ParamValue::N(1), "n", true),
        ("mate_in", ParamValue::N(5), "n", true),
        ("mate_in", ParamValue::N(0), "n", false),
        ("mate_in", ParamValue::N(6), "n", false),
        ("mate_in", ParamValue::N(255), "n", false),
        ("mate_in", ParamValue::MaxNodes(1000), "max_nodes", true),
        (
            "mate_in",
            ParamValue::MaxNodes(2_000_000),
            "max_nodes",
            true,
        ),
        ("mate_in", ParamValue::MaxNodes(999), "max_nodes", false),
        ("mate_in", ParamValue::MaxNodes(0), "max_nodes", false),
        (
            "mate_in",
            ParamValue::MaxNodes(2_000_001),
            "max_nodes",
            false,
        ),
        (
            "mate_in",
            ParamValue::MaxNodes(u32::MAX),
            "max_nodes",
            false,
        ),
        ("sacrifice", ParamValue::MinNet(1), "min_net", true),
        ("sacrifice", ParamValue::MinNet(20), "min_net", true),
        ("sacrifice", ParamValue::MinNet(0), "min_net", false),
        ("sacrifice", ParamValue::MinNet(21), "min_net", false),
        ("sacrifice", ParamValue::Within(1), "within", true),
        ("sacrifice", ParamValue::Within(8), "within", true),
        ("sacrifice", ParamValue::Within(0), "within", false),
        ("sacrifice", ParamValue::Within(9), "within", false),
        (
            "passed_pawn",
            ParamValue::MinOwnRank(Some(8)),
            "min_own_rank",
            false,
        ),
    ];
    for (id, value, field, ok) in cases {
        rig.pat(Pat::SelectPreset(id.into()));
        rig.pat(Pat::SetParam(value.clone()));
        let view = rig.view();
        let effects = rig.raw(Message::PatternSearch(Pat::Start));
        if ok {
            assert!(view.param_issue.is_none(), "{id} {value:?}");
            assert_eq!(effects.len(), 1, "{id} {value:?}");
            rig.pat(Pat::Stop);
        } else {
            let want = PatternSearchNotice::InvalidParam(field.into());
            assert_eq!(view.param_issue, Some(want.clone()), "{id} {value:?}");
            assert!(effects.is_empty(), "{id} {value:?}");
            assert_eq!(issue(&rig), Some(want.clone()));
            assert!(want.is_form_error());
            assert_eq!(want.message_key(), "pattern-error-invalid-param");
        }
    }
    // Ranks of the passed pawn: 2 to 7 or any.
    rig.pat(Pat::SelectPreset("passed_pawn".into()));
    for (rank, ok) in [
        (None, true),
        (Some(2), true),
        (Some(7), true),
        (Some(1), false),
        (Some(0), false),
        (Some(8), false),
        (Some(255), false),
    ] {
        rig.pat(Pat::SetParam(ParamValue::MinOwnRank(rank)));
        assert_eq!(rig.view().param_issue.is_none(), ok, "{rank:?}");
    }
}

#[test]
fn the_colour_of_a_preset_cannot_be_mover_or_opponent() {
    let (mut rig, _dir) = window();
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    rig.pat(Pat::SelectPreset("fork".into()));
    for who in [Who::Mover, Who::Opponent] {
        rig.pat(Pat::SetParam(ParamValue::By(who)));
        assert_eq!(
            rig.view().param_issue,
            Some(PatternSearchNotice::InvalidParam("by".into())),
            "{who:?}"
        );
        assert!(rig.raw(Message::PatternSearch(Pat::Start)).is_empty());
    }
    for who in [Who::White, Who::Black, Who::Any] {
        rig.pat(Pat::SetParam(ParamValue::By(who)));
        assert!(rig.view().param_issue.is_none(), "{who:?}");
    }
}

#[test]
fn picking_another_preset_clears_a_bad_value_of_the_previous_one() {
    let (mut rig, _dir) = window();
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    rig.pat(Pat::SelectPreset("mate_in".into()));
    rig.pat(Pat::SetParam(ParamValue::N(9)));
    assert!(rig.view().param_issue.is_some());
    rig.pat(Pat::SelectPreset("pin".into()));
    assert!(rig.view().param_issue.is_none());
    assert_eq!(rig.view().params.n, Params::default().n);
    assert_eq!(rig.raw(Message::PatternSearch(Pat::Start)).len(), 1);
}

// ------------------------------------------------------------ add / remove

#[test]
fn a_card_can_be_added_inside_any_group_at_any_depth() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Any);
    // The new OR group is at [0] and comes with one card inside.
    add(&mut rig, &[0], ConditionKind::Phase);
    assert_eq!(rig.view().selection, Some(vec![0, 1]));
    add(&mut rig, &[0], ConditionKind::Not);
    assert_eq!(rig.view().selection, Some(vec![0, 2]));
    // A NOT takes no new item; a leaf neither.
    add(&mut rig, &[0, 2], ConditionKind::Phase);
    assert_eq!(refused(&rig), Some(EditRefusal::NotAGroup));
    add(&mut rig, &[0, 1], ConditionKind::Phase);
    assert_eq!(refused(&rig), Some(EditRefusal::NotAGroup));
    assert_eq!(rig.view().tree.iter().filter(|n| n.depth == 2).count(), 3);
    assert_eq!(rig.view().tree[0].children, 1);
}

#[test]
fn removing_the_only_child_of_a_not_removes_the_not() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    add(&mut rig, &[], ConditionKind::Status);
    edit(
        &mut rig,
        PatternEdit::Wrap {
            path: vec![1],
            kind: GroupKind::Not,
        },
    );
    assert_eq!(
        root(&rig),
        all(vec![
            leaf(ConditionKind::Phase),
            Condition::Not {
                item: Box::new(leaf(ConditionKind::Status))
            }
        ])
    );
    edit(&mut rig, PatternEdit::Remove(vec![1, 0]));
    assert!(issue(&rig).is_none(), "{:?}", issue(&rig));
    assert_eq!(root(&rig), all(vec![leaf(ConditionKind::Phase)]));
}

#[test]
fn removing_the_only_child_of_a_root_not_never_panics_and_leaves_a_valid_tree() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    edit(
        &mut rig,
        PatternEdit::SetGroupKind {
            path: vec![],
            kind: GroupKind::Not,
        },
    );
    assert!(matches!(root(&rig), Condition::Not { .. }));
    edit(&mut rig, PatternEdit::Remove(vec![0]));
    // Either refused or turned into an (empty) group: the tree is never
    // left as a NOT without its child.
    match root(&rig) {
        Condition::Not { item } => {
            assert_eq!(*item, leaf(ConditionKind::Phase));
            assert!(refused(&rig).is_some());
        }
        Condition::All { items } | Condition::Any { items } => assert!(items.is_empty()),
        other => panic!("{other:?}"),
    }
    assert!(!rig.view().tree.is_empty());
}

#[test]
fn removing_the_last_card_leaves_an_empty_group_that_blocks_search_and_test() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    edit(&mut rig, PatternEdit::Remove(vec![0]));
    assert_eq!(root(&rig), all(vec![]));
    let tree_issue = rig.view().tree_issue.expect("issue");
    assert_eq!(tree_issue.message_key(), "pattern-error-empty-group");
    assert!(rig.raw(Message::PatternSearch(Pat::Start)).is_empty());
    assert!(rig.raw(Message::PatternSearch(Pat::TestGame)).is_empty());
    assert_eq!(rig.view().issue_path, Some(vec![]));
}

// ---------------------------------------------------------------- limits

#[test]
fn replace_accepts_depth_six_and_refuses_depth_seven() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    let nested = |nots: usize| {
        let mut c = leaf(ConditionKind::Phase);
        for _ in 0..nots {
            c = Condition::Not { item: Box::new(c) };
        }
        c
    };
    // root (1) + 4 NOTs + the leaf = 6.
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(nested(4)),
        },
    );
    assert!(issue(&rig).is_none(), "{:?}", issue(&rig));
    let good = root(&rig);
    assert_eq!(good, all(vec![nested(4)]));
    // One NOT more is 7.
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(nested(5)),
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::TooDeep));
    assert_eq!(
        issue(&rig).unwrap().message_key(),
        "pattern-edit-refused-too-deep"
    );
    assert_eq!(root(&rig), good);
}

#[test]
fn a_hostile_deep_group_chain_is_refused_without_overflowing_the_stack() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    let mut deep = leaf(ConditionKind::Phase);
    for _ in 0..5_000 {
        deep = Condition::Any { items: vec![deep] };
    }
    let before = root(&rig);
    // Run on a thread with a big stack for the destructor of `deep`.
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            edit(
                &mut rig,
                PatternEdit::Replace {
                    path: vec![0],
                    condition: Box::new(deep),
                },
            );
            (refused(&rig), root(&rig), rig.view().tree.len(), rig)
        })
        .unwrap();
    let (why, after, len, _rig) = handle.join().unwrap();
    assert_eq!(why, Some(EditRefusal::TooDeep));
    assert_eq!(after, before);
    assert_eq!(len, 2);
}

#[test]
fn replace_with_too_many_nodes_or_items_is_refused_with_the_right_reason() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    let before = root(&rig);
    // 24 groups of two cards: 1 + 24 * 3 nodes, depth 3.
    let many_nodes = Condition::All {
        items: (0..24)
            .map(|_| {
                all(vec![
                    leaf(ConditionKind::Phase),
                    leaf(ConditionKind::Status),
                ])
            })
            .collect(),
    };
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(many_nodes),
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::TooManyNodes));
    assert_eq!(root(&rig), before);
    // 25 items in a group.
    let many_items = Condition::Any {
        items: (0..25).map(|_| leaf(ConditionKind::Phase)).collect(),
    };
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(many_items),
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::GroupFull));
    assert_eq!(root(&rig), before);
    // 24 items fit.
    let exact = Condition::Any {
        items: (0..24).map(|_| leaf(ConditionKind::Phase)).collect(),
    };
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(exact),
        },
    );
    assert!(issue(&rig).is_none());
    assert_eq!(rig.view().tree.len(), 26);
}

#[test]
fn wrapping_the_root_and_adding_past_depth_six_are_refused_and_the_tree_is_intact() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    // Wrap the root four times: depth 1 + 4 + leaf = 6.
    for _ in 0..4 {
        edit(
            &mut rig,
            PatternEdit::Wrap {
                path: vec![],
                kind: GroupKind::Not,
            },
        );
        assert!(issue(&rig).is_none());
    }
    let full = root(&rig);
    edit(
        &mut rig,
        PatternEdit::Wrap {
            path: vec![],
            kind: GroupKind::All,
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::TooDeep));
    assert_eq!(root(&rig), full);
}

// ------------------------------------------------------------------ move

#[test]
fn move_addresses_the_parent_before_the_move_and_inserts_at_the_index() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    add(&mut rig, &[], ConditionKind::All);
    // root = [Phase, All[Piece]]; the target group is at [1] *before*
    // the Phase leaves the root.
    edit(
        &mut rig,
        PatternEdit::Move {
            path: vec![0],
            parent: vec![1],
            index: 0,
        },
    );
    assert!(issue(&rig).is_none(), "{:?}", issue(&rig));
    assert_eq!(
        root(&rig),
        all(vec![all(vec![
            leaf(ConditionKind::Phase),
            leaf(ConditionKind::Piece)
        ])])
    );
}

#[test]
fn move_past_the_end_appends_and_moving_to_an_ancestor_reorders() {
    let (mut rig, _dir) = window();
    for kind in [
        ConditionKind::Phase,
        ConditionKind::SideToMove,
        ConditionKind::Status,
    ] {
        add(&mut rig, &[], kind);
    }
    edit(
        &mut rig,
        PatternEdit::Move {
            path: vec![0],
            parent: vec![],
            index: 99,
        },
    );
    assert!(issue(&rig).is_none(), "{:?}", issue(&rig));
    assert_eq!(
        root(&rig),
        all(vec![
            leaf(ConditionKind::SideToMove),
            leaf(ConditionKind::Status),
            leaf(ConditionKind::Phase),
        ])
    );
}

#[test]
fn move_refuses_descendant_leaf_target_depth_and_full_groups() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::All);
    add(&mut rig, &[0], ConditionKind::All);
    let tree = root(&rig);
    // Into its own descendant.
    edit(
        &mut rig,
        PatternEdit::Move {
            path: vec![0],
            parent: vec![0, 1],
            index: 0,
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::MoveIntoItself));
    assert_eq!(root(&rig), tree);
    // Into a leaf and into a NOT.
    edit(
        &mut rig,
        PatternEdit::Move {
            path: vec![0, 1],
            parent: vec![0, 0],
            index: 0,
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::NotAGroup));
    // Into a path that does not exist.
    edit(
        &mut rig,
        PatternEdit::Move {
            path: vec![0, 1],
            parent: vec![7],
            index: 0,
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::BadPath));
    edit(
        &mut rig,
        PatternEdit::Move {
            path: vec![8],
            parent: vec![],
            index: 0,
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::BadPath));
    assert_eq!(root(&rig), tree);

    // A full group (24 items) takes no one more.
    edit(&mut rig, PatternEdit::Clear);
    add(&mut rig, &[], ConditionKind::All);
    for _ in 0..23 {
        add(&mut rig, &[0], ConditionKind::Phase);
    }
    add(&mut rig, &[], ConditionKind::Status);
    assert_eq!(rig.view().tree[1].children, 24);
    let full = root(&rig);
    edit(
        &mut rig,
        PatternEdit::Move {
            path: vec![1],
            parent: vec![0],
            index: 0,
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::GroupFull));
    assert_eq!(root(&rig), full);

    // A group with a card moved under the group at depth 5 would put the
    // card at depth 7.
    edit(&mut rig, PatternEdit::Clear);
    add(&mut rig, &[], ConditionKind::Phase);
    for _ in 0..4 {
        edit(
            &mut rig,
            PatternEdit::Wrap {
                path: vec![0],
                kind: GroupKind::Any,
            },
        );
    }
    add(&mut rig, &[], ConditionKind::All);
    let deep = root(&rig);
    edit(
        &mut rig,
        PatternEdit::Move {
            path: vec![1],
            parent: vec![0, 0, 0, 0],
            index: 0,
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::TooDeep));
    assert_eq!(root(&rig), deep);
}

// ------------------------------------------------------ kinds of group

#[test]
fn group_kinds_convert_with_their_items_and_a_not_needs_exactly_one() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    add(&mut rig, &[], ConditionKind::Status);
    edit(
        &mut rig,
        PatternEdit::SetGroupKind {
            path: vec![],
            kind: GroupKind::Any,
        },
    );
    assert_eq!(
        root(&rig),
        Condition::Any {
            items: vec![leaf(ConditionKind::Phase), leaf(ConditionKind::Status)]
        }
    );
    // Two items cannot become a NOT.
    let two = root(&rig);
    edit(
        &mut rig,
        PatternEdit::SetGroupKind {
            path: vec![],
            kind: GroupKind::Not,
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::NeedsOneItem));
    assert_eq!(root(&rig), two);
    // One item can, and a NOT goes back to AND / OR with that item.
    edit(&mut rig, PatternEdit::Remove(vec![1]));
    edit(
        &mut rig,
        PatternEdit::SetGroupKind {
            path: vec![],
            kind: GroupKind::Not,
        },
    );
    assert_eq!(
        root(&rig),
        Condition::Not {
            item: Box::new(leaf(ConditionKind::Phase))
        }
    );
    edit(
        &mut rig,
        PatternEdit::SetGroupKind {
            path: vec![],
            kind: GroupKind::All,
        },
    );
    assert_eq!(root(&rig), all(vec![leaf(ConditionKind::Phase)]));
    // The same kind is not an error.
    edit(
        &mut rig,
        PatternEdit::SetGroupKind {
            path: vec![],
            kind: GroupKind::All,
        },
    );
    assert_eq!(root(&rig), all(vec![leaf(ConditionKind::Phase)]));
}

#[test]
fn wrap_and_unwrap_are_inverses_and_unwrap_needs_a_group_of_one() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    let start = root(&rig);
    for kind in GroupKind::ALL {
        edit(
            &mut rig,
            PatternEdit::Wrap {
                path: vec![0],
                kind,
            },
        );
        assert!(issue(&rig).is_none(), "{kind:?}");
        assert_eq!(rig.view().tree.len(), 3);
        assert_eq!(rig.view().tree[1].path, vec![0]);
        edit(&mut rig, PatternEdit::Unwrap(vec![0]));
        assert!(issue(&rig).is_none(), "{kind:?}");
        assert_eq!(root(&rig), start, "{kind:?}");
    }
    // A leaf is not a group; a group of two cannot be unwrapped.
    edit(&mut rig, PatternEdit::Unwrap(vec![0]));
    assert!(matches!(
        refused(&rig),
        Some(EditRefusal::NotAGroup | EditRefusal::NeedsOneItem)
    ));
    assert_eq!(root(&rig), start);
    add(&mut rig, &[], ConditionKind::Status);
    edit(&mut rig, PatternEdit::Unwrap(vec![]));
    assert_eq!(refused(&rig), Some(EditRefusal::NeedsOneItem));
    assert_eq!(
        issue(&rig).unwrap().message_key(),
        "pattern-edit-refused-needs-one-item"
    );
}

// --------------------------------------------------------------- squares

#[test]
fn regions_are_set_on_the_slot_the_node_has_and_refused_elsewhere() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Pins);
    add(&mut rig, &[], ConditionKind::PreviousMove);
    add(&mut rig, &[], ConditionKind::Phase);
    let region = Region::File('e');
    for (path, slot) in [
        (vec![0], RegionSlot::Actor),
        (vec![0], RegionSlot::Target),
        (vec![0], RegionSlot::Behind),
        (vec![1], RegionSlot::From),
        (vec![1], RegionSlot::To),
    ] {
        edit(
            &mut rig,
            PatternEdit::SetRegion {
                path: path.clone(),
                slot,
                region: region.clone(),
            },
        );
        assert!(issue(&rig).is_none(), "{slot:?}: {:?}", issue(&rig));
        assert_eq!(rig.view().selection, Some(path.clone()));
    }
    let tree = rig.view().tree;
    let Condition::Relation(rel) = &tree[1].condition else {
        panic!("relation")
    };
    assert_eq!(rel.actor.region, region);
    assert_eq!(rel.target.region, region);
    assert_eq!(
        rel.behind.as_ref().map(|b| b.region.clone()),
        Some(region.clone())
    );
    let Condition::Move(m) = &tree[2].condition else {
        panic!("move")
    };
    assert_eq!(
        (m.from.clone(), m.to.clone()),
        (region.clone(), region.clone())
    );
    // Slots a node does not have, and nodes that have none.
    let before = root(&rig);
    for (path, slot) in [
        (vec![0], RegionSlot::Main),
        (vec![0], RegionSlot::From),
        (vec![1], RegionSlot::Actor),
        (vec![1], RegionSlot::Main),
        (vec![2], RegionSlot::Main),
        (vec![2], RegionSlot::To),
        (vec![], RegionSlot::Main),
    ] {
        edit(
            &mut rig,
            PatternEdit::SetRegion {
                path: path.clone(),
                slot,
                region: Region::Anywhere,
            },
        );
        assert_eq!(
            refused(&rig),
            Some(EditRefusal::NoSuchRegion),
            "{path:?} {slot:?}"
        );
        assert_eq!(root(&rig), before);
    }
}

#[test]
fn toggling_squares_builds_a_set_without_duplicates_and_ends_anywhere() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Piece);
    let toggle = |rig: &mut Rig, sq: Square| {
        edit(
            rig,
            PatternEdit::ToggleSquare {
                path: vec![0],
                slot: RegionSlot::Main,
                square: sq,
            },
        );
    };
    let region = |rig: &Rig| match &rig.view().tree[1].condition {
        Condition::Piece(p) => p.region.clone(),
        other => panic!("{other:?}"),
    };
    toggle(&mut rig, Square::E4);
    assert_eq!(region(&rig), Region::Squares(vec![Sq(Square::E4)]));
    toggle(&mut rig, Square::A1);
    toggle(&mut rig, Square::H8);
    toggle(&mut rig, Square::A1);
    assert_eq!(
        region(&rig),
        Region::Squares(vec![Sq(Square::E4), Sq(Square::H8)])
    );
    assert!(rig.view().tree_issue.is_none());
    // All 64 squares are a valid set; one more toggle takes one away.
    edit(
        &mut rig,
        PatternEdit::SetRegion {
            path: vec![0],
            slot: RegionSlot::Main,
            region: Region::Anywhere,
        },
    );
    for sq in Square::ALL {
        toggle(&mut rig, sq);
    }
    let Region::Squares(list) = region(&rig) else {
        panic!("squares")
    };
    assert_eq!(list.len(), 64);
    assert!(rig.view().tree_issue.is_none());
    toggle(&mut rig, Square::E4);
    let Region::Squares(list) = region(&rig) else {
        panic!("squares")
    };
    assert_eq!(list.len(), 63);
    // Toggling on a node without that slot is refused.
    add(&mut rig, &[], ConditionKind::Phase);
    edit(
        &mut rig,
        PatternEdit::ToggleSquare {
            path: vec![1],
            slot: RegionSlot::Main,
            square: Square::E4,
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::NoSuchRegion));
}

#[test]
fn toggling_a_square_of_a_move_card_changes_only_that_slot() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::NextMove);
    edit(
        &mut rig,
        PatternEdit::ToggleSquare {
            path: vec![0],
            slot: RegionSlot::To,
            square: Square::G7,
        },
    );
    let Condition::Move(m) = &rig.view().tree[1].condition else {
        panic!("move")
    };
    assert_eq!(m.to, Region::Squares(vec![Sq(Square::G7)]));
    assert_eq!(m.from, Region::Anywhere);
}

#[test]
fn the_marks_come_from_the_comment_at_the_cursor_and_nowhere_else() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Piece);
    let use_marks = |rig: &mut Rig| {
        edit(
            rig,
            PatternEdit::UseMarks {
                path: vec![0],
                slot: RegionSlot::Main,
            },
        );
    };
    let region = |rig: &Rig| match &rig.view().tree[1].condition {
        Condition::Piece(p) => p.region.clone(),
        other => panic!("{other:?}"),
    };
    // A comment that marks nothing (plain text, arrows only, bad squares).
    for text in ["just a remark", "[%cal Ge2e4]", "[%csl Gz9,Gi1]", "[%csl ]"] {
        let node = rig.state.session.cursor();
        rig.send(Message::Game(GameMessage::SetComment {
            node,
            text: text.into(),
        }));
        use_marks(&mut rig);
        assert_eq!(refused(&rig), Some(EditRefusal::NoMarks), "{text:?}");
        assert_eq!(
            issue(&rig).unwrap().message_key(),
            "pattern-edit-refused-no-marks"
        );
        assert_eq!(region(&rig), Region::Anywhere, "{text:?}");
    }
    // Several colours, a repeated square and a bad one among good ones.
    let node = rig.state.session.cursor();
    rig.send(Message::Game(GameMessage::SetComment {
        node,
        text: "text [%csl Gz9][%csl Gc3,Rd5,Yc3] more [%cal Ra1h8]".into(),
    }));
    use_marks(&mut rig);
    assert!(issue(&rig).is_none(), "{:?}", issue(&rig));
    assert_eq!(
        region(&rig),
        Region::Squares(vec![Sq(Square::C3), Sq(Square::D5)])
    );
    // After a move the cursor is on another node, with no comment.
    rig.send(Message::Game(GameMessage::EnterMoveText("e4".into())));
    edit(
        &mut rig,
        PatternEdit::SetRegion {
            path: vec![0],
            slot: RegionSlot::Main,
            region: Region::Anywhere,
        },
    );
    use_marks(&mut rig);
    assert_eq!(refused(&rig), Some(EditRefusal::NoMarks));
    // The marks of the cursor's own comment.
    let node = rig.state.session.cursor();
    rig.send(Message::Game(GameMessage::SetComment {
        node,
        text: "[%csl Ge4]".into(),
    }));
    use_marks(&mut rig);
    assert_eq!(region(&rig), Region::Squares(vec![Sq(Square::E4)]));
    // Nothing of the game changed because of the edit.
    assert_eq!(
        rig.state
            .session
            .game()
            .comment(rig.state.session.cursor())
            .unwrap(),
        Some("[%csl Ge4]")
    );
}

#[test]
fn marks_on_a_node_without_that_slot_or_a_bad_path_are_refused() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    let root_node = rig.state.session.game().root();
    rig.send(Message::Game(GameMessage::SetComment {
        node: root_node,
        text: "[%csl Ge4]".into(),
    }));
    edit(
        &mut rig,
        PatternEdit::UseMarks {
            path: vec![0],
            slot: RegionSlot::Main,
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::NoSuchRegion));
    edit(
        &mut rig,
        PatternEdit::UseMarks {
            path: vec![5],
            slot: RegionSlot::Main,
        },
    );
    assert_eq!(refused(&rig), Some(EditRefusal::BadPath));
}

// ---------------------------------------------------- select, clear, issue

#[test]
fn select_and_clear_reset_what_they_should() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    add(&mut rig, &[], ConditionKind::Status);
    edit(&mut rig, PatternEdit::Select(Some(vec![1])));
    assert!(rig.view().tree[2].selected);
    assert!(!rig.view().tree[1].selected);
    edit(&mut rig, PatternEdit::Select(None));
    assert_eq!(rig.view().selection, None);
    assert!(rig.view().tree.iter().all(|n| !n.selected));
    edit(&mut rig, PatternEdit::Select(Some(vec![0, 0, 0, 0])));
    assert_eq!(rig.view().selection, None);
    // A selected node that disappears is dropped by Clear.
    edit(&mut rig, PatternEdit::Select(Some(vec![1])));
    edit(&mut rig, PatternEdit::Clear);
    assert_eq!(rig.view().selection, None);
    assert_eq!(root(&rig), all(vec![]));
    assert_eq!(rig.view().tree.len(), 1);
    // Selecting is not an edit: it is no refusal and no notice.
    assert!(issue(&rig).is_none());
}

#[test]
fn a_bad_value_points_at_the_exact_path_even_inside_a_not() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    add(&mut rig, &[], ConditionKind::Piece);
    edit(
        &mut rig,
        PatternEdit::Wrap {
            path: vec![1],
            kind: GroupKind::Not,
        },
    );
    let mut piece = leaf(ConditionKind::Piece);
    if let Condition::Piece(p) = &mut piece {
        p.min = 4;
        p.max = Some(2);
    }
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![1, 0],
            condition: Box::new(piece.clone()),
        },
    );
    // Wrong values inside a card are not refused: they are marked.
    assert!(issue(&rig).is_none());
    let view = rig.view();
    assert_eq!(view.issue_path, Some(vec![1, 0]));
    let marked: Vec<_> = view.tree.iter().filter(|n| n.has_issue).collect();
    assert_eq!(marked.len(), 1);
    assert_eq!(marked[0].path, vec![1, 0]);
    assert!(matches!(
        view.tree_issue,
        Some(PatternSearchNotice::InvalidPattern(
            PatternIssue::InvalidValue { .. }
        ))
    ));
    // Search, test, save and export are all refused.
    assert!(rig.raw(Message::PatternSearch(Pat::Start)).is_empty());
    assert!(rig.raw(Message::PatternSearch(Pat::TestGame)).is_empty());
    let dir = tempfile::tempdir().unwrap();
    let saved = rig.raw(Message::PatternSearch(Pat::SaveAs {
        name: "bad".into(),
        description: String::new(),
    }));
    assert!(saved.is_empty(), "{saved:?}");
    assert!(matches!(
        issue(&rig),
        Some(PatternSearchNotice::InvalidPattern(_))
    ));
    let export = rig.raw(Message::PatternSearch(Pat::Export {
        path: dir.path().join("bad.toml"),
        name: "bad".into(),
        description: String::new(),
    }));
    assert!(export.is_empty(), "{export:?}");
    assert!(!dir.path().join("bad.toml").exists());
    // Fixing the card unblocks everything.
    if let Condition::Piece(p) = &mut piece {
        p.max = Some(8);
    }
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![1, 0],
            condition: Box::new(piece),
        },
    );
    assert!(rig.view().issue_path.is_none());
    assert!(rig.view().tree_issue.is_none());
    assert_eq!(rig.raw(Message::PatternSearch(Pat::Start)).len(), 1);
}

#[test]
fn mover_where_it_can_never_match_is_marked_with_its_own_key() {
    let (mut rig, _dir) = window();
    for kind in [
        ConditionKind::Discovered,
        ConditionKind::DoubleAttack,
        ConditionKind::PreviousMove,
    ] {
        edit(&mut rig, PatternEdit::Clear);
        add(&mut rig, &[], kind);
        let mut card = rig.view().tree[1].condition.clone();
        match &mut card {
            Condition::Discovered(d) => d.by = Who::Mover,
            Condition::DoubleAttack(d) => d.by = Who::Mover,
            Condition::Move(m) => m.by = Who::Mover,
            other => panic!("{other:?}"),
        }
        edit(
            &mut rig,
            PatternEdit::Replace {
                path: vec![0],
                condition: Box::new(card),
            },
        );
        let view = rig.view();
        assert_eq!(view.issue_path, Some(vec![0]), "{kind:?}");
        let notice = view.tree_issue.expect("issue");
        assert_eq!(notice.message_key(), "pattern-error-mover-not-allowed");
        assert!(notice.is_form_error());
        assert!(rig.raw(Message::PatternSearch(Pat::Start)).is_empty());
        assert_eq!(
            issue(&rig).map(|n| n.message_key()),
            Some("pattern-error-mover-not-allowed")
        );
    }
}

#[test]
fn the_reversed_scope_and_the_empty_group_are_told_without_a_card_path() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Phase);
    rig.pat(Pat::SetScope(Scope {
        min_move: 9,
        max_move: 3,
        ..Scope::default()
    }));
    let view = rig.view();
    assert!(view.tree_issue.is_some());
    assert_eq!(view.issue_path, None);
    assert!(view.tree.iter().all(|n| !n.has_issue));
}

// -------------------------------------------------------------- sentences

fn only_squares(arg: &SentenceArg) {
    match arg {
        SentenceArg::Text(t) => assert!(
            t.chars().all(|c| c.is_ascii_alphanumeric()
                || c == ','
                || c == ' '
                || c == '-'
                || c == ':'),
            "{t:?}"
        ),
        SentenceArg::List(l) => l.iter().for_each(only_squares),
        SentenceArg::Phrase(p) => p.args.iter().for_each(|(_, a)| only_squares(a)),
        _ => {}
    }
}

fn keys_of(sentence: &Sentence, out: &mut Vec<&'static str>) {
    out.push(sentence.key);
    for (_, arg) in &sentence.args {
        walk_arg(arg, out);
    }
}

fn walk_arg(arg: &SentenceArg, out: &mut Vec<&'static str>) {
    match arg {
        SentenceArg::Key(k) => out.push(k),
        SentenceArg::List(list) => list.iter().for_each(|a| walk_arg(a, out)),
        SentenceArg::Phrase(p) => keys_of(p, out),
        _ => {}
    }
}

#[test]
fn the_sentence_of_each_kind_has_its_key_and_arguments_and_no_fixed_text() {
    let (mut rig, _dir) = window();
    let table: [(ConditionKind, &str, &[&str]); 22] = [
        (
            ConditionKind::Piece,
            "pattern-sentence-piece",
            &["side", "piece", "where"],
        ),
        (
            ConditionKind::Material,
            "pattern-sentence-material-any",
            &[],
        ),
        (
            ConditionKind::SideToMove,
            "pattern-sentence-side-to-move",
            &["side"],
        ),
        (ConditionKind::Phase, "pattern-sentence-phase", &["phase"]),
        (
            ConditionKind::Status,
            "pattern-sentence-status",
            &["status"],
        ),
        (
            ConditionKind::PreviousMove,
            "pattern-sentence-move-previous",
            &[
                "by",
                "piece",
                "from",
                "to",
                "capture",
                "check",
                "promotion",
                "castling",
            ],
        ),
        (
            ConditionKind::NextMove,
            "pattern-sentence-move-next",
            &[
                "by",
                "piece",
                "from",
                "to",
                "capture",
                "check",
                "promotion",
                "castling",
            ],
        ),
        (
            ConditionKind::Result,
            "pattern-sentence-result",
            &["results"],
        ),
        (
            ConditionKind::PassedPawn,
            "pattern-sentence-passed-pawn",
            &["side", "where"],
        ),
        (
            ConditionKind::IsolatedPawn,
            "pattern-sentence-isolated-pawn",
            &["side", "where"],
        ),
        (
            ConditionKind::DoubledPawns,
            "pattern-sentence-doubled-pawns",
            &["side", "where"],
        ),
        (
            ConditionKind::ExposedKing,
            "pattern-sentence-exposed-king",
            &["side", "where"],
        ),
        (
            ConditionKind::BishopPair,
            "pattern-sentence-bishop-pair",
            &["side", "where"],
        ),
        (
            ConditionKind::Attacks,
            "pattern-sentence-attacks",
            &["actor", "target", "gain", "when", "new"],
        ),
        (
            ConditionKind::Defends,
            "pattern-sentence-defends",
            &["actor", "target"],
        ),
        (
            ConditionKind::Pins,
            "pattern-sentence-pin",
            &["actor", "target", "behind"],
        ),
        (
            ConditionKind::Skewers,
            "pattern-sentence-skewer",
            &["actor", "target", "behind"],
        ),
        (
            ConditionKind::Forks,
            "pattern-sentence-fork",
            &["actor", "target", "gain", "when", "new", "count"],
        ),
        (
            ConditionKind::Discovered,
            "pattern-sentence-discovered-attack",
            &["by", "piece"],
        ),
        (
            ConditionKind::DoubleAttack,
            "pattern-sentence-double-attack",
            &["by"],
        ),
        (
            ConditionKind::Sacrifice,
            "pattern-sentence-sacrifice",
            &["by", "piece", "net", "outcome", "within"],
        ),
        (
            ConditionKind::Mate,
            "pattern-sentence-mate",
            &["by", "min", "max", "mode", "forcing"],
        ),
    ];
    for (kind, key, args) in table {
        edit(&mut rig, PatternEdit::Clear);
        add(&mut rig, &[], kind);
        let sentence = rig.view().tree[1].sentence.clone();
        assert_eq!(sentence.key, key, "{kind:?}");
        for name in args {
            assert!(sentence.arg(name).is_some(), "{kind:?} lacks {name}");
        }
        let mut keys = Vec::new();
        keys_of(&sentence, &mut keys);
        assert!(
            keys.iter().all(|k| k.starts_with("pattern-")),
            "{kind:?}: {keys:?}"
        );
        // No argument carries prose: text is only for names of squares.
        sentence.args.iter().for_each(|(_, a)| only_squares(a));
    }
}

#[test]
fn the_sentence_changes_with_each_field_of_the_card() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Mate);
    let set = |rig: &mut Rig, mate: MateCond| {
        edit(
            rig,
            PatternEdit::Replace {
                path: vec![0],
                condition: Box::new(Condition::Mate(mate)),
            },
        );
        rig.view().tree[1].sentence.clone()
    };
    let Condition::Mate(base) = rig.view().tree[1].condition.clone() else {
        panic!("mate")
    };
    let a = set(
        &mut rig,
        MateCond {
            by: Who::Black,
            moves_min: 2,
            moves_max: 4,
            ..base.clone()
        },
    );
    assert_eq!(a.arg("by"), Some(&SentenceArg::Key("pattern-who-black")));
    assert_eq!(a.arg("min"), Some(&SentenceArg::Number(2)));
    assert_eq!(a.arg("max"), Some(&SentenceArg::Number(4)));
    let b = set(
        &mut rig,
        MateCond {
            mode: mychess_app::pattern_search::model::MateMode::Available,
            forcing_only: false,
            ..base
        },
    );
    assert_eq!(
        b.arg("mode"),
        Some(&SentenceArg::Key("pattern-mate-mode-available"))
    );
    assert_eq!(b.arg("forcing"), Some(&SentenceArg::Number(0)));
    // Other kinds follow their model too.
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(Condition::Phase {
                phase: Phase::Endgame,
            }),
        },
    );
    assert_eq!(
        rig.view().tree[1].sentence.arg("phase"),
        Some(&SentenceArg::Key("pattern-phase-endgame"))
    );
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(Condition::Status {
                status: Status::Checkmate,
            }),
        },
    );
    assert_eq!(
        rig.view().tree[1].sentence.arg("status"),
        Some(&SentenceArg::Key("pattern-status-checkmate"))
    );
}

#[test]
fn the_sentences_of_a_discovery_have_one_key_for_each_discovery() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Discovered);
    let mut keys = Vec::new();
    for discovery in [
        DiscoveredKind::Attack,
        DiscoveredKind::Check,
        DiscoveredKind::DoubleCheck,
    ] {
        let mut card = rig.view().tree[1].condition.clone();
        if let Condition::Discovered(d) = &mut card {
            d.discovery = discovery;
            d.roles = vec![RoleName::Knight];
        }
        edit(
            &mut rig,
            PatternEdit::Replace {
                path: vec![0],
                condition: Box::new(card),
            },
        );
        let sentence = rig.view().tree[1].sentence.clone();
        assert_eq!(
            sentence.arg("piece"),
            Some(&SentenceArg::List(vec![SentenceArg::Key(
                "pattern-role-knight"
            )]))
        );
        keys.push(sentence.key);
    }
    assert_eq!(
        keys,
        [
            "pattern-sentence-discovered-attack",
            "pattern-sentence-discovered-check",
            "pattern-sentence-double-check"
        ]
    );
}

// ------------------------------------------------------------ preferences

fn prefs_text_after(f: impl FnOnce(&mut Rig)) -> String {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    rig.pat(Pat::Open);
    f(&mut rig);
    render_preferences(&rig.state.prefs)
}

#[test]
fn each_operation_and_scope_survives_render_and_load() {
    for (op, scope, preset) in [
        (SearchOp::Reset, Scope::default(), "mate_in"),
        (
            SearchOp::And,
            Scope {
                min_move: 0,
                max_move: 0,
                hold: 1,
                end_only: true,
                ignore_colors: false,
            },
            "pin",
        ),
        (
            SearchOp::Or,
            Scope {
                min_move: 999,
                max_move: 999,
                hold: 40,
                end_only: false,
                ignore_colors: true,
            },
            "opposite_castling",
        ),
    ] {
        let text = prefs_text_after(|rig| {
            rig.pat(Pat::SelectPreset(preset.into()));
            rig.pat(Pat::SetOp(op));
            rig.pat(Pat::SetScope(scope.clone()));
        });
        assert!(text.contains("[pattern_search]"), "{text}");
        let loaded = load_preferences(&text);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        let dir = tempfile::tempdir().unwrap();
        let mut fresh = Rig::new(dir.path());
        fresh.state.prefs = loaded.prefs;
        fresh.pat(Pat::Open);
        let view = fresh.view();
        assert_eq!(view.op, op);
        assert_eq!(view.scope, scope);
        assert_eq!(view.selected_preset.as_deref(), Some(preset));
        // The rendering is stable (what is loaded renders the same).
        assert_eq!(render_preferences(&fresh.state.prefs), text);
    }
}

#[test]
fn bad_values_in_the_preferences_fall_back_and_the_window_still_opens() {
    let cases = [
        "[pattern_search]\nhold = 0\n",
        "[pattern_search]\nhold = -5\n",
        "[pattern_search]\nhold = \"x\"\n",
        "[pattern_search]\nmin_move = 70000\nmax_move = 70000\n",
        "[pattern_search]\nmin_move = -1\n",
        "[pattern_search]\nop = \"xor\"\n",
        "[pattern_search]\nop = 3\n",
        "[pattern_search]\nlast_preset = \"nope\"\n",
        "[pattern_search]\nlast_preset = 7\n",
        "[pattern_search]\nlast_preset = \"\"\n",
        "[pattern_search]\nend_only = \"yes\"\nignore_colors = 1\n",
        "[pattern_search]\nunknown = 1\n",
        "pattern_search = \"text\"\n",
        "[pattern_search]\nmin_move = 50\nmax_move = 10\n",
    ];
    for text in cases {
        let loaded = load_preferences(text);
        let p = &loaded.prefs.pattern_search;
        assert!((1..=40).contains(&p.hold), "{text:?}: {p:?}");
        assert!(p.min_move <= 999 && p.max_move <= 999, "{text:?}: {p:?}");
        let dir = tempfile::tempdir().unwrap();
        let mut rig = Rig::new(dir.path());
        rig.state.prefs = loaded.prefs;
        rig.pat(Pat::Open);
        let view = rig.view();
        let id = view.selected_preset.expect("a preset");
        assert!(presets::find(&id).is_some(), "{text:?}: {id}");
        assert!((1..=40).contains(&view.scope.hold), "{text:?}");
    }
    // The bad value falls back to the default, the good ones stay.
    let loaded = load_preferences("[pattern_search]\nhold = 3\nop = \"xor\"\nmax_move = 40\n");
    let p = &loaded.prefs.pattern_search;
    assert_eq!((p.hold, p.max_move, p.op), (3, 40, SearchOp::Reset));
    assert!(!loaded.issues.is_empty());
}

#[test]
fn a_changed_scope_is_stored_when_the_search_runs_and_a_failed_start_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let scope = Scope {
        min_move: 2,
        max_move: 20,
        hold: 2,
        end_only: false,
        ignore_colors: true,
    };
    rig.pat(Pat::SetScope(scope.clone()));
    assert_eq!(rig.state.prefs.pattern_search.scope(), scope);
    // An out-of-range parameter refuses the start and the scope stays.
    rig.pat(Pat::SetParam(ParamValue::N(9)));
    assert!(rig.raw(Message::PatternSearch(Pat::Start)).is_empty());
    assert_eq!(rig.state.prefs.pattern_search.scope(), scope);
}

#[test]
fn a_mate_of_the_opponent_is_marked_and_refused() {
    let (mut rig, _dir) = window();
    add(&mut rig, &[], ConditionKind::Mate);
    let Condition::Mate(mut mate) = rig.view().tree[1].condition.clone() else {
        panic!("mate")
    };
    mate.by = Who::Opponent;
    edit(
        &mut rig,
        PatternEdit::Replace {
            path: vec![0],
            condition: Box::new(Condition::Mate(mate.clone())),
        },
    );
    assert_eq!(rig.view().issue_path, Some(vec![0]));
    assert!(rig.raw(Message::PatternSearch(Pat::Start)).is_empty());
    // The range of moves is 1 to 8 and must not be reversed.
    for (min, max, ok) in [(1, 8, true), (0, 3, false), (1, 9, false), (4, 2, false)] {
        mate.by = Who::Mover;
        mate.moves_min = min;
        mate.moves_max = max;
        edit(
            &mut rig,
            PatternEdit::Replace {
                path: vec![0],
                condition: Box::new(Condition::Mate(mate.clone())),
            },
        );
        assert_eq!(rig.view().tree_issue.is_none(), ok, "{min}..{max}");
    }
}
