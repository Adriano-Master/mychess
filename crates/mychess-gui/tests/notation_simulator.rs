//! Headless tests of the notation panel with `iced_test` and the real
//! renderer: pointer scripts in, events and messages out.
//!
//! The coordinates of the moves are not guessed: they come from a
//! `LayoutCache` over the same paragraph type, text style and width as the
//! widget under test.

use iced::advanced::text::Renderer as TextRenderer;
use iced::keyboard::{self, Key, key::Named};
use iced::{Event, Point, Settings, Size, Theme, mouse, widget};
use iced_test::Simulator;
use mychess_app::{
    CommentTarget, General, LanguagePref, LoadedPreferences, Message as AppMessage,
    NotationOptions, Preferences, ThemePref, build_notation,
};
use mychess_core::{Game, NodeId};
use mychess_gui::widgets::notation::layout::{Inputs, LayoutCache};
use mychess_gui::widgets::notation::scroll::BAR_WIDTH;
use mychess_gui::widgets::notation::{Hit, Labels, NotationEvent, NotationView, text_style};
use mychess_gui::windows::comment_editor::{self, EDITOR_ID, EditorMessage};
use mychess_gui::windows::layout::PaneKind;
use mychess_gui::windows::notation::{self, NotationMessage};
use mychess_gui::windows::notation_panel;
use mychess_gui::{Message, Shell};

type Paragraph = <iced::Renderer as TextRenderer>::Paragraph;

const SIZE: Size = Size::new(640.0, 360.0);
const TEXT_SIZE: f32 = 15.0;

fn labels() -> Labels {
    Labels {
        round: "Round".to_owned(),
        position: "Position".to_owned(),
        empty: "No moves yet".to_owned(),
    }
}

fn play(game: &mut Game, parent: NodeId, uci: &str) -> NodeId {
    let mv = game
        .position_at(parent)
        .expect("position")
        .parse_uci(uci)
        .expect("legal");
    game.add_move(parent, mv).expect("move")
}

/// 1.e4 e5 2.Nf3 (2.Nc3) 2...Nc6, with a comment after e4.
fn sample() -> (Game, [NodeId; 5]) {
    let mut game = Game::new();
    let root = game.root();
    let e4 = play(&mut game, root, "e2e4");
    let e5 = play(&mut game, e4, "e7e5");
    let nf3 = play(&mut game, e5, "g1f3");
    let nc3 = play(&mut game, e5, "b1c3");
    let nc6 = play(&mut game, nf3, "b8c6");
    game.set_comment(e4, "the king pawn").expect("comment");
    (game, [e4, e5, nf3, nc3, nc6])
}

struct Rig {
    doc: mychess_app::NotationDoc,
    labels: Labels,
    cache: LayoutCache<Paragraph>,
}

impl Rig {
    fn new(game: &Game) -> Rig {
        let doc = build_notation(game, &NotationOptions::default(), 1);
        let labels = labels();
        let style = text_style(&Theme::Light, TEXT_SIZE, true, false);
        let mut cache = LayoutCache::<Paragraph>::default();
        cache.sync(&Inputs {
            doc: &doc,
            labels: &labels,
            style: &style,
            width: SIZE.width - BAR_WIDTH,
        });
        Rig { doc, labels, cache }
    }

    fn center_of(&self, node: NodeId) -> Point {
        let (entry, span) = self.cache.find_move(node).expect("move is shown");
        let rect = self.cache.span_rects(entry, span)[0];
        Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    fn view(&self, current: Option<NodeId>) -> Simulator<'_, NotationEvent> {
        let style = text_style(&Theme::Light, TEXT_SIZE, true, false);
        let view = NotationView::new(&self.doc, &self.labels, style, current, |event| event)
            .into_element();
        Simulator::with_size(Settings::default(), SIZE, view)
    }
}

fn moved(position: Point) -> Event {
    Event::Mouse(mouse::Event::CursorMoved { position })
}

fn press(button: mouse::Button) -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(button))
}

fn release(button: mouse::Button) -> Event {
    Event::Mouse(mouse::Event::ButtonReleased(button))
}

#[test]
fn clicking_a_move_publishes_its_node() {
    let (game, [e4, e5, nf3, nc3, nc6]) = sample();
    let rig = Rig::new(&game);
    for node in [e4, e5, nf3, nc3, nc6] {
        let mut sim = rig.view(None);
        let at = rig.center_of(node);
        sim.point_at(at);
        sim.simulate([moved(at)]);
        sim.simulate(iced_test::simulator::click());
        let events: Vec<_> = sim.into_messages().collect();
        assert!(
            events.contains(&NotationEvent::Click(node)),
            "{node}: {events:?}"
        );
    }
}

#[test]
fn clicking_a_comment_publishes_the_comment_target() {
    let (game, [e4, ..]) = sample();
    let rig = Rig::new(&game);
    let target = CommentTarget::After(e4);
    let (entry, span) = rig
        .cache
        .find_hit(Hit::Comment(target))
        .expect("the comment is shown");
    let rect = rig.cache.span_rects(entry, span)[0];
    let at = Point::new(rect.x + 3.0, rect.y + rect.height / 2.0);
    let mut sim = rig.view(None);
    sim.point_at(at);
    sim.simulate([moved(at)]);
    sim.simulate(iced_test::simulator::click());
    let events: Vec<_> = sim.into_messages().collect();
    assert!(
        events.contains(&NotationEvent::CommentClick(target)),
        "{events:?}"
    );
}

#[test]
fn the_right_button_asks_for_a_menu_at_the_pointer() {
    let (game, [_, e5, ..]) = sample();
    let rig = Rig::new(&game);
    let mut sim = rig.view(None);
    let at = rig.center_of(e5);
    sim.point_at(at);
    sim.simulate([moved(at), press(mouse::Button::Right)]);
    let events: Vec<_> = sim.into_messages().collect();
    assert!(
        events.contains(&NotationEvent::Menu {
            hit: Some(Hit::Move(e5)),
            position: at
        }),
        "{events:?}"
    );
}

#[test]
fn the_right_button_on_empty_space_asks_for_the_global_menu() {
    let (game, _) = sample();
    let rig = Rig::new(&game);
    let mut sim = rig.view(None);
    let at = Point::new(SIZE.width / 2.0, SIZE.height - 10.0);
    sim.point_at(at);
    sim.simulate([moved(at), press(mouse::Button::Right)]);
    let events: Vec<_> = sim.into_messages().collect();
    assert!(events.contains(&NotationEvent::Menu {
        hit: None,
        position: at
    }));
}

#[test]
fn hovering_a_move_and_leaving_it_are_reported() {
    let (game, [e4, ..]) = sample();
    let rig = Rig::new(&game);
    let mut sim = rig.view(None);
    let at = rig.center_of(e4);
    sim.point_at(at);
    sim.simulate([moved(at)]);
    sim.point_at(Point::new(SIZE.width / 2.0, SIZE.height - 5.0));
    sim.simulate([moved(Point::new(SIZE.width / 2.0, SIZE.height - 5.0))]);
    let events: Vec<_> = sim.into_messages().collect();
    assert_eq!(
        events,
        [
            NotationEvent::Hover(Some((e4, at))),
            NotationEvent::Hover(None)
        ]
    );
}

#[test]
fn the_middle_button_previews_while_held() {
    let (game, [_, _, nf3, ..]) = sample();
    let rig = Rig::new(&game);
    let mut sim = rig.view(None);
    let at = rig.center_of(nf3);
    sim.point_at(at);
    sim.simulate([
        moved(at),
        press(mouse::Button::Middle),
        release(mouse::Button::Middle),
    ]);
    let events: Vec<_> = sim.into_messages().collect();
    assert!(events.contains(&NotationEvent::Middle(Some((nf3, at)))));
    assert!(events.contains(&NotationEvent::Middle(None)));
}

#[test]
fn the_wheel_over_a_long_game_scrolls_the_list() {
    let mut game = Game::new();
    let mut at = game.root();
    for ply in 0..400 {
        at = play(&mut game, at, ["g1f3", "g8f6", "f3g1", "f6g8"][ply % 4]);
    }
    let rig = Rig::new(&game);
    let mut sim = rig.view(None);
    let over = Point::new(100.0, 100.0);
    sim.point_at(over);
    sim.simulate([Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 },
    })]);
    let events: Vec<_> = sim.into_messages().collect();
    assert_eq!(events, [NotationEvent::Scrolled]);
}

#[test]
fn the_wheel_does_nothing_when_everything_fits() {
    let (game, _) = sample();
    let rig = Rig::new(&game);
    let mut sim = rig.view(None);
    sim.point_at(Point::new(100.0, 100.0));
    sim.simulate([Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 },
    })]);
    assert_eq!(sim.into_messages().count(), 0);
}

// ---- the whole panel in a shell ----

fn shell() -> Shell {
    let mut shell = Shell::new(None);
    let prefs = Preferences {
        general: General {
            language: LanguagePref::EnUs,
            theme: ThemePref::Light,
            ..General::default()
        },
        ..Preferences::default()
    };
    drop(shell.update(Message::App(AppMessage::PreferencesLoaded(
        LoadedPreferences {
            prefs,
            issues: Vec::new(),
        },
    ))));
    shell
}

fn feed(shell: &mut Shell, messages: impl IntoIterator<Item = Message>) {
    for message in messages {
        drop(shell.update(message));
    }
}

#[test]
fn a_click_in_the_panel_moves_the_cursor_of_the_game() {
    let (game, [_, e5, ..]) = sample();
    let mut shell = shell();
    shell.app.session.replace_game(game);
    shell.app.refresh_notation();
    // The pane is a row with the options toggle above the list: sweep the
    // area and collect what the list reports.
    let mut sim = Simulator::with_size(
        Settings::default(),
        Size::new(480.0, 360.0),
        notation::moves_tab(&shell),
    );
    for row in 0..60u16 {
        for column in 0..80u16 {
            let p = Point::new(4.0 + f32::from(column) * 6.0, 2.0 + f32::from(row) * 6.0);
            sim.point_at(p);
            sim.simulate([moved(p)]);
            sim.simulate(iced_test::simulator::click());
        }
    }
    let messages: Vec<Message> = sim.into_messages().collect();
    let click = Message::Notation(NotationMessage::Event(NotationEvent::Click(e5)));
    assert!(
        messages
            .iter()
            .any(|m| format!("{m:?}") == format!("{click:?}")),
        "the click on e5 was reported"
    );
    feed(&mut shell, [click]);
    assert_eq!(shell.app.session.cursor(), e5);
}

fn editor_sim(shell: &Shell) -> Simulator<'_, Message> {
    Simulator::with_size(
        Settings::default(),
        Size::new(520.0, 420.0),
        comment_editor::view(shell),
    )
}

#[test]
fn typing_in_the_editor_and_pressing_the_save_shortcut_writes_the_comment() {
    let mut shell = shell();
    // The editor is on the comment of the game at the start.
    let mut sim = editor_sim(&shell);
    sim.click(widget::Id::new(EDITOR_ID)).expect("text area");
    sim.typewrite("Hello");
    let command = if cfg!(target_os = "macos") {
        keyboard::Modifiers::LOGO
    } else {
        keyboard::Modifiers::CTRL
    };
    sim.simulate([Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(Named::Enter),
        modified_key: Key::Named(Named::Enter),
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers: command,
        repeat: false,
        text: None,
    })]);
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(
            m,
            Message::Notation(NotationMessage::Editor(EditorMessage::Commit))
        )),
        "{messages:?}"
    );
    feed(&mut shell, messages);
    assert_eq!(shell.app.session.game().initial_comment(), Some("Hello"));
}

#[test]
fn the_save_button_and_the_clear_button_work() {
    let mut shell = shell();
    let mut sim = editor_sim(&shell);
    sim.click(widget::Id::new(EDITOR_ID)).expect("text area");
    sim.typewrite("note");
    sim.click("Save").expect("save button");
    let messages: Vec<Message> = sim.into_messages().collect();
    feed(&mut shell, messages);
    assert_eq!(shell.app.session.game().initial_comment(), Some("note"));
    // The editor now shows the saved text; Clear + Save removes it.
    let mut sim = editor_sim(&shell);
    sim.click("Clear").expect("clear button");
    sim.click("Save").expect("save button");
    let messages: Vec<Message> = sim.into_messages().collect();
    feed(&mut shell, messages);
    assert_eq!(shell.app.session.game().initial_comment(), None);
}

#[test]
fn the_editor_names_the_comment_it_edits_in_the_current_language() {
    let shell = shell();
    let mut sim = editor_sim(&shell);
    assert!(sim.find("Comment at game start").is_ok());
    assert!(sim.find("Save").is_ok());
    assert!(sim.find(widget::Id::new(EDITOR_ID)).is_ok());
}

#[test]
fn the_panel_shows_the_header_the_option_bar_and_the_move_list() {
    let (game, _) = sample();
    let mut shell = shell();
    shell.app.session.replace_game(game);
    shell.app.refresh_notation();
    drop(shell.update(Message::Notation(NotationMessage::ToggleOptions)));
    let mut sim = Simulator::with_size(
        Settings::default(),
        Size::new(520.0, 420.0),
        notation_panel::view(&shell, PaneKind::Notation, true),
    );
    // The header has the icon button of the display options (M2-5, D249),
    // and the open bar offers the layouts and the new options.
    assert!(sim.find(widget::Id::new("notation-options")).is_ok());
    // (The radio buttons of the layout have no text the selector reaches.)
    assert!(sim.find("Piece symbols").is_ok());
    assert!(sim.find("Time column").is_ok());
    assert!(sim.find("Show result").is_ok());
    assert!(sim.find(widget::Id::new("notation-pane")).is_ok());
    // Toggling an option reaches the preferences of the application.
    sim.click("Show result").expect("option");
    let messages: Vec<Message> = sim.into_messages().collect();
    feed(&mut shell, messages);
    assert!(!shell.app.notation_options().show_result);
    assert_eq!(shell.app.notation().result, None);
}

#[test]
fn the_panel_renders_in_both_themes() {
    for theme in [Theme::Light, Theme::Dark] {
        let (game, _) = sample();
        let mut shell = shell();
        shell.app.session.replace_game(game);
        shell.app.refresh_notation();
        let mut sim = Simulator::with_size(
            Settings::default(),
            Size::new(480.0, 360.0),
            notation_panel::view(&shell, PaneKind::Notation, true),
        );
        assert!(sim.snapshot(&theme).is_ok());
    }
}

// ---- measurements (run by hand: `cargo test -p mychess-gui --test
// notation_simulator -- --ignored --nocapture measure`) ----

/// 300 plies on the main line and 150 variations of 4 plies each.
fn big_game() -> (Game, Vec<NodeId>) {
    let mut game = Game::new();
    let mut at = game.root();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let mut line = Vec::new();
    for ply in 0..300 {
        if ply % 2 == 0 {
            let mut v = at;
            for uci in ["a2a3", "a7a6", "a3a4", "a6a5"] {
                v = play(&mut game, v, uci);
            }
        }
        at = play(&mut game, at, cycle[ply % 4]);
        line.push(at);
    }
    (game, line)
}

#[test]
#[ignore = "timing report, run by hand"]
fn measure_the_document_and_the_layout() {
    use std::time::Instant;
    let (game, line) = big_game();
    let t = Instant::now();
    let doc = build_notation(&game, &NotationOptions::default(), 1);
    println!(
        "build_notation: {:?} ({} blocks)",
        t.elapsed(),
        doc.blocks.len()
    );
    let labels = labels();
    let style = text_style(&Theme::Light, TEXT_SIZE, true, false);
    let mut cache = LayoutCache::<Paragraph>::default();
    let inputs = Inputs {
        doc: &doc,
        labels: &labels,
        style: &style,
        width: 600.0,
    };
    let t = Instant::now();
    cache.sync(&inputs);
    println!(
        "first layout: {:?} ({} paragraphs shaped)",
        t.elapsed(),
        cache.shaped()
    );
    let t = Instant::now();
    for _ in 0..1000 {
        cache.sync(&inputs);
    }
    println!(
        "sync with nothing changed, per call: {:?}",
        t.elapsed() / 1000
    );
    // One comment changes: only its block is shaped again.
    let mut edited = game.clone();
    edited.set_comment(line[10], "edited").expect("comment");
    let mut doc2 = build_notation(&edited, &NotationOptions::default(), 2);
    doc2.keep_revisions(&doc);
    let before = cache.shaped();
    let t = Instant::now();
    cache.sync(&Inputs {
        doc: &doc2,
        ..inputs
    });
    println!(
        "one comment edited: {:?} ({} paragraphs shaped)",
        t.elapsed(),
        cache.shaped() - before
    );
    // A narrower window lays out again without shaping.
    let before = cache.shaped();
    let t = Instant::now();
    cache.sync(&Inputs {
        doc: &doc2,
        width: 420.0,
        ..inputs
    });
    println!(
        "width change: {:?} ({} paragraphs shaped, {} resized)",
        t.elapsed(),
        cache.shaped() - before,
        cache.resized()
    );
}
