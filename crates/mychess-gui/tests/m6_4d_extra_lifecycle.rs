//! M6-4d extra (GUI): closing a panel cancels its computation, a quit with
//! computations in flight is quiet, text fields keep their keys from the
//! game, and saved layouts with the new and unknown panels load (spec M6-4,
//! D3430-D3439).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m6_4d_common;

/// Text shaping shares one font system: drawing tests run one at a time
/// (several simulators at once crash the process, see `m2_4_common`).
fn render_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

use m6_4d_common::*;
use mychess_app::LayoutNode;
use mychess_app::NamedKey::{Backspace, Delete, Down, End, Home, Left, Right, Space, Up};
use mychess_app::charts::ChartsEffect;
use mychess_app::crosstable::CrosstableEffect;
use mychess_app::piece_tracker::PieceTrackerEffect;
use mychess_gui::windows::dialog_ui::DialogMessage;

/// Reads whether the computation of an effect was cancelled.
type Cancelled = Box<dyn Fn() -> bool>;

fn control_of(effect: &Effect) -> Option<Cancelled> {
    match effect {
        Effect::Crosstable(CrosstableEffect::Compute { control, .. })
        | Effect::Charts(
            ChartsEffect::Stats { control, .. }
            | ChartsEffect::Graph { control, .. }
            | ChartsEffect::Rating { control, .. },
        )
        | Effect::PieceTracker(PieceTrackerEffect::Track { control, .. }) => {
            let control = control.clone();
            Some(Box::new(move || control.is_cancelled()))
        }
        _ => None,
    }
}

/// An open pane with a computation in flight: its control token.
fn in_flight(kind: PaneKind) -> (Rig, Cancelled) {
    let mut rig = Rig::new();
    let slot = rig.open_games(&GAMES);
    rig.load(slot, 1);
    rig.toggle(kind);
    let message: AppMessage = match kind {
        PaneKind::Crosstable => CrosstableMessage::Refresh.into(),
        PaneKind::FilterStats => ChartsMessage::RefreshStats.into(),
        PaneKind::RatingGraph => {
            rig.app(ChartsMessage::AddPlayer("A".into()));
            ChartsMessage::RefreshRating.into()
        }
        _ => PieceTrackerMessage::Refresh.into(),
    };
    let control = update(&mut rig.shell.app, message)
        .iter()
        .find_map(control_of)
        .unwrap_or_else(|| panic!("{kind:?}: a computation starts"));
    assert!(!control());
    (rig, control)
}

const PANES: [PaneKind; 4] = [
    PaneKind::Crosstable,
    PaneKind::FilterStats,
    PaneKind::RatingGraph,
    PaneKind::PieceTracker,
];

#[test]
fn closing_the_tab_cancels_the_computation_in_flight() {
    let _render = render_lock();
    for kind in PANES {
        let (mut rig, control) = in_flight(kind);
        rig.ui(Message::Ui(UiMessage::ClosePane(kind)));
        assert!(control(), "{kind:?}: close cancels");
    }
}

#[test]
fn toggling_the_pane_off_cancels_the_computation_in_flight() {
    let _render = render_lock();
    for kind in PANES {
        let (mut rig, control) = in_flight(kind);
        rig.toggle(kind);
        assert!(control(), "{kind:?}: toggle cancels");
    }
}

#[test]
fn closing_the_base_or_replacing_it_while_computing_does_not_panic() {
    let _render = render_lock();
    for kind in PANES {
        let (mut rig, _control) = in_flight(kind);
        rig.app(BasesMessage::Close(rig.shell.app.bases.current()));
        drop(rig.sim(1000.0, 700.0));
        rig.open_games(&GAMES);
        drop(rig.sim(1000.0, 700.0));
    }
}

#[test]
fn quitting_with_every_computation_in_flight_is_quiet() {
    let _render = render_lock();
    let mut rig = Rig::new();
    let slot = rig.open_games(&GAMES);
    rig.load(slot, 1);
    for kind in PANES {
        rig.toggle(kind);
    }
    let mut controls = Vec::new();
    for message in [
        AppMessage::from(CrosstableMessage::Refresh),
        ChartsMessage::RefreshStats.into(),
        PieceTrackerMessage::Refresh.into(),
    ] {
        controls.extend(
            update(&mut rig.shell.app, message)
                .iter()
                .filter_map(control_of),
        );
    }
    assert_eq!(controls.len(), 3);
    let host = rig.app(AppMessage::Quit);
    drop(host);
    // The panels can still be drawn and keys do not panic.
    drop(rig.sim(1200.0, 800.0));
    rig.ui(Message::Ui(UiMessage::FlipBoard));
}

fn key(named: mychess_app::NamedKey) -> mychess_app::KeyInput {
    mychess_app::KeyInput {
        key: mychess_app::Key::Named(named),
        modifiers: mychess_app::Modifiers::NONE,
    }
}

#[test]
fn keys_typed_in_a_field_of_a_panel_do_not_move_the_game_or_flip_the_board() {
    let _render = render_lock();
    let mut rig = Rig::new();
    let slot = rig.open_games(&GAMES);
    rig.load(slot, 1);
    for kind in PANES {
        rig.toggle(kind);
    }
    rig.ui(Message::Ui(UiMessage::ShowAbout(false)));
    let cursor = rig.shell.app.session.cursor();
    let flipped = rig.shell.app.board_flipped;
    let nodes = rig.shell.app.session.game().node_count();
    for named in [Left, Right, Up, Down, Home, End, Space, Delete, Backspace] {
        rig.ui(Message::Dialog(DialogMessage::Key {
            input: key(named),
            text_focused: true,
            typed: None,
        }));
    }
    for c in ['f', 'n', 'e', '1'] {
        rig.ui(Message::Dialog(DialogMessage::Key {
            input: mychess_app::KeyInput {
                key: mychess_app::Key::Char(c),
                modifiers: mychess_app::Modifiers::NONE,
            },
            text_focused: true,
            typed: Some(c),
        }));
    }
    assert_eq!(rig.shell.app.session.cursor(), cursor);
    assert_eq!(rig.shell.app.board_flipped, flipped);
    assert_eq!(rig.shell.app.session.game().node_count(), nodes);
    for kind in PANES {
        assert!(rig.shell.ui.is_open(kind), "{kind:?} stays open");
    }
}

#[test]
fn escape_does_not_close_a_data_panel() {
    let _render = render_lock();
    let mut rig = Rig::new();
    rig.open_games(&GAMES);
    for kind in PANES {
        rig.toggle(kind);
    }
    for named in [mychess_app::NamedKey::Escape, mychess_app::NamedKey::Tab] {
        rig.ui(Message::Dialog(DialogMessage::Key {
            input: key(named),
            text_focused: false,
            typed: None,
        }));
    }
    for kind in PANES {
        assert!(rig.shell.ui.is_open(kind), "{kind:?}");
    }
}

#[test]
fn a_saved_layout_with_the_new_and_unknown_panels_loads_and_the_app_follows() {
    let _render = render_lock();
    let text = "layout = { layout_version = 2, axis = \"horizontal\", ratio = 0.5, \
        a = { pane = \"board\" }, \
        b = { tabs = [\"crosstable\", \"from-the-future\", \"filter-stats\", \"rating-graph\", \"piece-tracker\"], active = 3 } }\n";
    let prefs = mychess_app::load_preferences(text).prefs;
    let layout = prefs.layout.clone().expect("layout read");
    assert!(layout.is_valid());
    for kind in PANES {
        assert!(layout.contains(kind), "{kind:?}");
    }
    assert_eq!(
        layout.panes().len(),
        5,
        "board + the four panels: unknown dropped"
    );
    let mut rig = Rig::new();
    // Through the shell: it builds the pane grid from the saved layout.
    rig.ui(Message::App(AppMessage::PreferencesLoaded(
        mychess_app::LoadedPreferences {
            prefs,
            issues: Vec::new(),
        },
    )));
    rig.ui(Message::Ui(UiMessage::ShowAbout(false)));
    for kind in PANES {
        assert!(rig.shell.ui.is_open(kind), "{kind:?} is open in the shell");
    }
    // The application learns the panels are open (it computes for them).
    assert!(rig.shell.app.crosstable().is_open());
    assert!(rig.shell.app.charts().stats_is_open());
    assert!(rig.shell.app.charts().rating_is_open());
    assert!(rig.shell.app.piece_tracker().is_open());
    drop(rig.sim(900.0, 600.0));
    // A split that loses a side to unknown names becomes the other side.
    let only_unknown = "layout = { layout_version = 2, axis = \"horizontal\", ratio = 0.5, \
        a = { pane = \"board\" }, b = { pane = \"nope\" } }\n";
    let loaded = mychess_app::load_preferences(only_unknown).prefs.layout;
    assert!(
        loaded.as_ref().is_none_or(LayoutNode::is_valid),
        "{loaded:?}"
    );
}
