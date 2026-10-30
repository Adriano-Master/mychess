//! M2-6 (GUI): any sequence of user actions on the panels keeps the shell
//! consistent: valid layout, menu, application state, command bar and saved
//! layout agree (D1008, D1009, D1021).
#![allow(
    unused_qualifications,
    clippy::manual_midpoint,
    clippy::map_unwrap_or,
    clippy::large_stack_arrays,
    clippy::match_wildcard_for_single_variants,
    let_underscore_drop,
    clippy::needless_pass_by_value,
    clippy::redundant_closure_for_method_calls,
    clippy::items_after_statements,
    clippy::cast_precision_loss,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m2_6_gui_common;

use iced::widget::pane_grid::{DragEvent, Edge, Region, Target};
use m2_6_gui_common::*;
use mychess_app::Key;
use proptest::prelude::*;

#[derive(Debug, Clone)]
enum Op {
    Close(usize),
    Toggle(usize),
    Select(usize),
    Detach(usize),
    Drop {
        src: usize,
        dst: usize,
        region: usize,
    },
    Reset,
    CtrlE,
    CtrlI,
    OpenTabMenu(usize),
}

fn op() -> impl Strategy<Value = Op> {
    let k = 0..ALL.len();
    prop_oneof![
        k.clone().prop_map(Op::Close),
        k.clone().prop_map(Op::Toggle),
        k.clone().prop_map(Op::Select),
        k.clone().prop_map(Op::Detach),
        (0..8usize, 0..8usize, 0..6usize).prop_map(|(src, dst, region)| Op::Drop {
            src,
            dst,
            region
        }),
        Just(Op::Reset),
        Just(Op::CtrlE),
        Just(Op::CtrlI),
        k.prop_map(Op::OpenTabMenu),
    ]
}

fn apply(rig: &mut Rig, op: &Op) {
    match op {
        Op::Close(i) => ui(rig, UiMessage::ClosePane(ALL[*i])),
        Op::Toggle(i) => ui(rig, UiMessage::TogglePane(ALL[*i])),
        Op::Select(i) => ui(rig, UiMessage::SelectTab { kind: ALL[*i] }),
        Op::Detach(i) => ui(rig, UiMessage::DetachTab(ALL[*i])),
        Op::Drop { src, dst, region } => {
            let panes: Vec<_> = rig.shell.ui.panes.iter().map(|(p, _)| *p).collect();
            let (pane, onto) = (panes[src % panes.len()], panes[dst % panes.len()]);
            // The widget never reports a pane dropped on itself.
            let target = if pane == onto || *region == 5 {
                Target::Edge(Edge::Right)
            } else {
                Target::Pane(
                    onto,
                    match region {
                        0 | 1 => Region::Center,
                        2 => Region::Edge(Edge::Top),
                        3 => Region::Edge(Edge::Left),
                        _ => Region::Edge(Edge::Bottom),
                    },
                )
            };
            ui(
                rig,
                UiMessage::PaneDragged(DragEvent::Dropped { pane, target }),
            );
        }
        Op::Reset => ui(rig, UiMessage::ResetLayout),
        Op::CtrlE => rig.key(command(Key::Char('e')), false, None),
        Op::CtrlI => rig.key(command(Key::Char('i')), false, None),
        Op::OpenTabMenu(i) => ui(
            rig,
            UiMessage::OpenTabMenu {
                kind: ALL[*i],
                position: iced::Point::new(300.0, 300.0),
            },
        ),
    }
}

fn check(rig: &Rig) -> Result<(), TestCaseError> {
    let layout = layout(rig);
    prop_assert!(layout.is_valid(), "{layout:?}");
    prop_assert!(layout.contains(PaneKind::Board));
    prop_assert_eq!(layout.panes().len(), {
        let mut p = layout.panes();
        p.sort_by_key(|k| format!("{k:?}"));
        p.dedup();
        p.len()
    });
    for kind in CLOSABLE {
        prop_assert_eq!(is_open(rig, kind), layout.contains(kind), "{:?}", kind);
        prop_assert_eq!(pane_checked(rig, kind), is_open(rig, kind), "{:?}", kind);
    }
    prop_assert_eq!(rig.shell.app.tree().is_open(), is_open(rig, PaneKind::Tree));
    prop_assert_eq!(
        rig.shell.app.graph().is_open(),
        is_open(rig, PaneKind::Graph)
    );
    prop_assert_eq!(
        rig.shell.ui.command_bar_owner().is_some(),
        is_open(rig, PaneKind::Notation)
    );
    prop_assert_eq!(
        rig.shell.ui.comment_visible(),
        layout.visible_panes().contains(&PaneKind::Comment)
    );
    if let Some(saved) = &rig.shell.app.prefs.layout {
        prop_assert_eq!(saved, &rig.shell.ui.layout_to_save());
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn any_sequence_of_panel_actions_keeps_the_shell_consistent(
        ops in proptest::collection::vec(op(), 1..14)
    ) {
        let mut rig = rig();
        for op in &ops {
            apply(&mut rig, op);
            check(&rig)?;
        }
        // The window still draws, with the right controls.
        prop_assert_eq!(has(&rig, MOVE_FIELD_ID), is_open(&rig, PaneKind::Notation));
        prop_assert!(!has(&rig, close_id(PaneKind::Board)));
        for kind in CLOSABLE {
            prop_assert_eq!(
                has(&rig, close_id(kind)),
                is_open(&rig, kind) && (tabs(&rig, kind).len() == 1 || shown_in(&rig, kind).is_some()),
                "{:?}", kind
            );
        }
    }
}
