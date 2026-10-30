//! M2-4 notation panel: layout (indented versus in line), hit testing, the
//! context menu, clicks, hover preview and scrolling (spec D161-D173,
//! rules 9-22, acceptance criteria "NotationView" and "Menu de contexto").
//!
//! The expectations come from the specification; positions are read from a
//! `LayoutCache` over the same paragraph type, style and width as the
//! widget, never guessed.

#![allow(
    clippy::doc_markdown,
    clippy::cast_precision_loss,
    clippy::assert_is_empty,
    clippy::float_cmp,
    clippy::unnecessary_wraps
)]

mod m2_4_common;

use std::time::{Duration, Instant};

use iced::advanced::text::Renderer as TextRenderer;
use iced::{Point, Size, Theme};
use m2_4_common::{Rig, Sample, line, play, sample};
use mychess_app::{
    CommentTarget, GameMessage, HoverPreview, Message as AppMessage, NotationDoc, NotationLayout,
    NotationOptions, PrefChange, PrefsMessage, Prompt, StripKind, build_notation,
};
use mychess_core::{Game, Nag, NodeId};
use mychess_gui::widgets::notation::layout::{Inputs, LayoutCache};
use mychess_gui::widgets::notation::scroll::{BAR_WIDTH, Scroll};
use mychess_gui::widgets::notation::style::PADDING;
use mychess_gui::widgets::notation::{Hit, Labels, NotationEvent, text_style};
use mychess_gui::windows::notation::{
    self, MenuAction, MenuLabel, MenuRow, NotationMessage, menu_messages, menu_model,
    variation_head,
};
use mychess_gui::{Message, UiMessage};

type Paragraph = <iced::Renderer as TextRenderer>::Paragraph;

const WIDTH: f32 = 700.0;
const TEXT_SIZE: f32 = 15.0;

fn labels() -> Labels {
    Labels {
        round: "Round".to_owned(),
        position: "Position".to_owned(),
        empty: "No moves yet".to_owned(),
    }
}

/// A document and its shaped layout.
struct Lay {
    _render: m2_4_common::RenderGuard,
    cache: LayoutCache<Paragraph>,
    step: f32,
}

impl Lay {
    fn new(game: &Game, options: &NotationOptions, width: f32) -> Lay {
        let doc = build_notation(game, options, 1);
        Lay::of(&doc, width)
    }

    fn of(doc: &NotationDoc, width: f32) -> Lay {
        let guard = m2_4_common::render_lock();
        let labels = labels();
        let style = text_style(&Theme::Light, TEXT_SIZE, true, false);
        let mut cache = LayoutCache::<Paragraph>::default();
        cache.sync(&Inputs {
            doc,
            labels: &labels,
            style: &style,
            width: width - BAR_WIDTH,
        });
        Lay {
            _render: guard,
            cache,
            step: style.indent_step(),
        }
    }

    /// Left edge of the paragraph that holds `hit`.
    fn entry_x(&self, hit: Hit) -> f32 {
        let (entry, _) = self.cache.find_hit(hit).expect("shown");
        let mut xs = Vec::new();
        self.cache
            .for_each(0..self.cache.len(), |_, x, _| xs.push(x));
        xs[entry] - PADDING
    }

    fn entry_of(&self, hit: Hit) -> usize {
        self.cache.find_hit(hit).expect("shown").0
    }

    fn centre(&self, hit: Hit) -> Point {
        let (entry, span) = self.cache.find_hit(hit).expect("shown");
        let rect = self.cache.span_rects(entry, span)[0];
        Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }
}

fn mv(node: NodeId) -> Hit {
    Hit::Move(node)
}

// ---------------------------------------------------------------------------
// Indented versus in line (D163, rule 10)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_indented_layout_puts_each_variation_in_its_own_block_two_em_in() {
    let s = sample();
    let lay = Lay::new(&s.game, &NotationOptions::default(), WIDTH);
    let main = lay.entry_x(mv(s.e5));
    let nc3 = lay.entry_x(mv(s.nc3));
    assert_ne!(lay.entry_of(mv(s.e5)), lay.entry_of(mv(s.nc3)));
    assert!(
        (nc3 - main - lay.step).abs() < 0.5,
        "one level = 2 em = {}: got {}",
        lay.step,
        nc3 - main
    );
    assert!((lay.step - 2.0 * TEXT_SIZE).abs() < 0.01, "2 em per level");
}

#[test]
fn m2_4_a_variation_inside_a_variation_is_one_more_level_in() {
    let mut game = Game::new();
    let root = game.root();
    let a = line(&mut game, root, &["e2e4", "e7e5", "g1f3"]);
    // 2.Nc3 (variation of level 1) and inside it 2...Nf6 / 2...Bc5 (level 2).
    let nc3 = play(&mut game, a[1], "b1c3");
    let nf6 = play(&mut game, nc3, "g8f6");
    let bc5 = play(&mut game, nc3, "f8c5");
    let lay = Lay::new(&game, &NotationOptions::default(), WIDTH);
    let level0 = lay.entry_x(mv(a[1]));
    let level1 = lay.entry_x(mv(nc3));
    let level2 = lay.entry_x(mv(bc5));
    assert!((level1 - level0 - lay.step).abs() < 0.5);
    assert!((level2 - level0 - 2.0 * lay.step).abs() < 0.5);
    // The continuation of the level-1 line (nf6) is back at level 1 and not
    // in the block of the line it was split from.
    assert!((lay.entry_x(mv(nf6)) - level1).abs() < 0.5);
}

#[test]
fn m2_4_the_indent_stops_growing_after_six_levels() {
    // 1.Nf3 (1.Nc3 Nf6 (1...a6 2.e4 (2.a3 b6 (2...b5 3.b3 (3.c3 ...)))))
    // Each variation head is a sibling of the main reply of the line above.
    let mut game = Game::new();
    let root = game.root();
    let _main = play(&mut game, root, "g1f3");
    let mut heads = vec![play(&mut game, root, "b1c3")];
    let steps = [
        ("g8f6", "a7a6"),
        ("e2e4", "a2a3"),
        ("b7b6", "b7b5"),
        ("b2b3", "d2d3"),
        ("c7c6", "c7c5"),
        ("e2e3", "e2e4"),
        ("d7d6", "d7d5"),
    ];
    for (reply, branch) in steps {
        let from = *heads.last().expect("a head");
        let _reply = play(&mut game, from, reply);
        heads.push(play(&mut game, from, branch));
    }
    assert_eq!(heads.len(), 8, "levels 1 to 8");
    let lay = Lay::new(&game, &NotationOptions::default(), WIDTH);
    for (index, head) in heads.iter().enumerate() {
        let level = index + 1;
        let expected = level.min(6) as f32 * lay.step;
        let x = lay.entry_x(mv(*head));
        assert!(
            (x - expected).abs() < 0.5,
            "level {level}: expected {expected}, got {x}"
        );
    }
}

#[test]
fn m2_4_inline_layout_keeps_variations_in_the_paragraph_of_the_line() {
    let s = sample();
    let options = NotationOptions {
        layout: NotationLayout::Inline,
        ..NotationOptions::default()
    };
    let lay = Lay::new(&s.game, &options, WIDTH);
    let main = lay.entry_of(mv(s.e5));
    for node in [s.nc3, s.nf6, s.c5, s.nf3, s.bb5] {
        assert_eq!(
            lay.entry_of(mv(node)),
            main,
            "one paragraph for the movetext"
        );
    }
    assert_eq!(lay.entry_x(mv(s.nc3)), lay.entry_x(mv(s.e5)), "no indent");
}

#[test]
fn m2_4_a_main_line_comment_is_an_indented_block_only_when_asked() {
    let mut s = sample();
    s.game.set_comment(s.e4, "king pawn").expect("comment");
    let target = Hit::Comment(CommentTarget::After(s.e4));
    // D163 is about the indented layout, which is no longer the default
    // (M2-5, D241).
    let indented = NotationOptions {
        layout: NotationLayout::Indented,
        ..NotationOptions::default()
    };
    let on = Lay::new(&s.game, &indented, WIDTH);
    assert_ne!(on.entry_of(target), on.entry_of(mv(s.e4)), "own block");
    assert!(
        (on.entry_x(target) - on.entry_x(mv(s.e4)) - on.step).abs() < 0.5,
        "indented by one level"
    );
    let off = Lay::new(
        &s.game,
        &NotationOptions {
            indent_comments: false,
            ..indented
        },
        WIDTH,
    );
    assert_eq!(off.entry_of(target), off.entry_of(mv(s.e4)), "in line");
}

#[test]
fn m2_4_a_comment_inside_a_variation_always_stays_in_line() {
    let mut s = sample();
    s.game.set_comment(s.nc3, "the Vienna").expect("comment");
    let lay = Lay::new(&s.game, &NotationOptions::default(), WIDTH);
    let comment = Hit::Comment(CommentTarget::After(s.nc3));
    assert_eq!(lay.entry_of(comment), lay.entry_of(mv(s.nc3)));
}

#[test]
fn m2_4_comment_before_a_variation_and_the_initial_comment_are_clickable_items() {
    let mut s = sample();
    s.game
        .set_comment_before(s.nc3, "an old idea")
        .expect("before");
    s.game.set_initial_comment("a classic");
    let lay = Lay::new(&s.game, &NotationOptions::default(), WIDTH);
    for target in [CommentTarget::Before(s.nc3), CommentTarget::Initial] {
        assert!(
            lay.cache.find_hit(Hit::Comment(target)).is_some(),
            "{target:?} is shown"
        );
    }
    // The initial comment comes before the first move, the other one before
    // the first move of its variation.
    let initial = lay
        .cache
        .find_hit(Hit::Comment(CommentTarget::Initial))
        .unwrap();
    let first = lay.cache.find_move(s.e4).unwrap();
    assert!(initial.0 <= first.0);
}

// ---------------------------------------------------------------------------
// Hit testing (rules 9, 14)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_points_on_moves_nags_and_comments_resolve_to_their_targets() {
    let mut s = sample();
    s.game.add_nag(s.e4, Nag::new(1).unwrap()).expect("nag");
    s.game.set_comment(s.e5, "a reply").expect("comment");
    let lay = Lay::new(&s.game, &NotationOptions::default(), WIDTH);
    for hit in [
        mv(s.e4),
        Hit::Nag(s.e4),
        mv(s.e5),
        Hit::Comment(CommentTarget::After(s.e5)),
        mv(s.nc3),
    ] {
        let at = lay.centre(hit);
        let located = lay.cache.locate(at).expect("something is there");
        assert_eq!(located.hit, Some(hit), "{hit:?}");
    }
}

#[test]
fn m2_4_numbers_parentheses_and_empty_space_are_not_targets() {
    let s = sample();
    let options = NotationOptions {
        layout: NotationLayout::Inline,
        ..NotationOptions::default()
    };
    let lay = Lay::new(&s.game, &options, WIDTH);
    // The gap between two adjacent moves of the same paragraph is plain
    // text ("2." or "("): find a point left of a move that is not on a
    // clickable span.
    let (entry, span) = lay.cache.find_move(s.nc3).expect("nc3");
    let rect = lay.cache.span_rects(entry, span)[0];
    let before = Point::new(rect.x - 1.0, rect.y + rect.height / 2.0);
    let found = lay.cache.locate(before);
    assert!(
        found.is_none_or(|l| l.hit != Some(mv(s.nc3))),
        "the point just left of a move is not that move"
    );
    // Far below the content and far right of it: nothing.
    assert!(lay.cache.locate(Point::new(10.0, 1.0e7)).is_none());
    assert!(lay.cache.locate(Point::new(10.0, -50.0)).is_none());
    assert!(lay.cache.locate(Point::new(f32::NAN, f32::NAN)).is_none());
}

#[test]
fn m2_4_nags_do_not_become_clickable_when_shown_as_dollar_numbers() {
    let mut s = sample();
    s.game.add_nag(s.e4, Nag::new(1).unwrap()).expect("nag");
    let options = NotationOptions {
        symbolic_nags: false,
        ..NotationOptions::default()
    };
    // The doc still carries the NAG as its own inline: the hit is a NAG hit.
    let lay = Lay::new(&s.game, &options, WIDTH);
    assert!(lay.cache.find_hit(Hit::Nag(s.e4)).is_some());
}

// ---------------------------------------------------------------------------
// Scrolling (D170, rule 21)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_reveal_keeps_a_line_of_margin_and_does_nothing_when_visible() {
    let mut scroll = Scroll::default();
    scroll.set_extent(1000.0, 200.0);
    scroll.set_offset(100.0);
    // Already inside with margin: untouched.
    scroll.reveal(150.0, 170.0, 20.0);
    assert_eq!(scroll.offset(), 100.0);
    // Below the viewport: the bottom edge plus the margin is visible.
    scroll.reveal(400.0, 420.0, 20.0);
    assert_eq!(scroll.offset(), 440.0 - 200.0);
    // Above: the top edge minus the margin is visible.
    scroll.reveal(50.0, 70.0, 20.0);
    assert_eq!(scroll.offset(), 30.0);
}

#[test]
fn m2_4_a_block_taller_than_the_viewport_aligns_its_top() {
    let mut scroll = Scroll::default();
    scroll.set_extent(2000.0, 100.0);
    scroll.reveal(500.0, 900.0, 16.0);
    assert_eq!(scroll.offset(), 500.0 - 16.0);
}

#[test]
fn m2_4_scrolling_is_clamped_and_survives_hostile_numbers() {
    let mut scroll = Scroll::default();
    scroll.set_extent(300.0, 200.0);
    assert!(scroll.scroll_by(1.0e9));
    assert_eq!(scroll.offset(), 100.0);
    assert!(!scroll.scroll_by(1.0e9), "already at the end");
    assert!(scroll.scroll_by(-1.0e9));
    assert_eq!(scroll.offset(), 0.0);
    scroll.set_offset(f32::NAN);
    scroll.reveal(f32::INFINITY, f32::NAN, 5.0);
    scroll.set_extent(f32::NAN, f32::INFINITY);
    assert_eq!(scroll.offset(), 0.0);
    // Everything fits: there is no scroll bar.
    scroll.set_extent(100.0, 200.0);
    assert!(scroll.thumb(200.0).is_none());
}

// ---------------------------------------------------------------------------
// The context menu (D172, rules 17-19)
// ---------------------------------------------------------------------------

fn item(rows: &[MenuRow], label: MenuLabel) -> Option<MenuAction> {
    rows.iter().find_map(|row| match row {
        MenuRow::Item(l, action) if *l == label => Some(*action),
        _ => None,
    })?
}

fn listed(rows: &[MenuRow], label: MenuLabel) -> bool {
    rows.iter()
        .any(|row| matches!(row, MenuRow::Item(l, _) if *l == label))
}

#[test]
fn m2_4_menu_on_a_main_line_move_disables_the_variation_items() {
    let s = sample();
    let rows = menu_model(&s.game, Some(Hit::Move(s.e5)));
    assert_eq!(
        item(&rows, MenuLabel::GoHere),
        Some(MenuAction::GoHere(s.e5))
    );
    for label in [
        MenuLabel::Promote,
        MenuLabel::MakeFirst,
        MenuLabel::DeleteVariation,
    ] {
        assert!(listed(&rows, label), "{label:?} is listed");
        assert_eq!(item(&rows, label), None, "{label:?} is disabled");
    }
    assert_eq!(
        item(&rows, MenuLabel::TruncateFrom),
        Some(MenuAction::TruncateFrom(s.e5))
    );
    assert_eq!(
        item(&rows, MenuLabel::TruncateBefore),
        Some(MenuAction::TruncateBefore(s.e5))
    );
    assert_eq!(
        item(&rows, MenuLabel::StripComments),
        Some(MenuAction::StripComments)
    );
    assert_eq!(
        item(&rows, MenuLabel::StripVariations),
        Some(MenuAction::StripVariations)
    );
    assert_eq!(
        item(&rows, MenuLabel::EditComment),
        Some(MenuAction::EditComment(CommentTarget::After(s.e5)))
    );
    assert!(rows.contains(&MenuRow::Nags(s.e5)), "the NAG strip");
}

#[test]
fn m2_4_menu_on_a_deep_variation_move_acts_on_the_head_that_contains_it() {
    let s = sample();
    // nf6 is the reply inside the variation 2.Nc3; its head is nc3.
    assert_eq!(variation_head(&s.game, s.nf6), Some(s.nc3));
    assert_eq!(variation_head(&s.game, s.nc3), Some(s.nc3));
    assert_eq!(variation_head(&s.game, s.c5), Some(s.c5));
    assert_eq!(variation_head(&s.game, s.bb5), None, "main line");
    let rows = menu_model(&s.game, Some(Hit::Move(s.nf6)));
    assert_eq!(
        item(&rows, MenuLabel::Promote),
        Some(MenuAction::Promote(s.nc3))
    );
    assert_eq!(
        item(&rows, MenuLabel::MakeFirst),
        Some(MenuAction::MakeFirst(s.nc3))
    );
    assert_eq!(
        item(&rows, MenuLabel::DeleteVariation),
        Some(MenuAction::DeleteVariation(s.nc3))
    );
    // The go-here and truncation items still point at the clicked move.
    assert_eq!(
        item(&rows, MenuLabel::GoHere),
        Some(MenuAction::GoHere(s.nf6))
    );
}

#[test]
fn m2_4_delete_comment_is_enabled_only_when_the_target_has_one() {
    let mut s = sample();
    let none = menu_model(&s.game, Some(Hit::Move(s.e4)));
    assert_eq!(item(&none, MenuLabel::DeleteComment), None);
    s.game.set_comment(s.e4, "text").expect("comment");
    let some = menu_model(&s.game, Some(Hit::Move(s.e4)));
    assert_eq!(
        item(&some, MenuLabel::DeleteComment),
        Some(MenuAction::DeleteComment(CommentTarget::After(s.e4)))
    );
    // A click on the comment itself targets that comment.
    s.game.set_comment_before(s.nc3, "before").expect("before");
    let before = menu_model(&s.game, Some(Hit::Comment(CommentTarget::Before(s.nc3))));
    assert_eq!(
        item(&before, MenuLabel::EditComment),
        Some(MenuAction::EditComment(CommentTarget::Before(s.nc3)))
    );
    assert_eq!(
        item(&before, MenuLabel::DeleteComment),
        Some(MenuAction::DeleteComment(CommentTarget::Before(s.nc3)))
    );
    s.game.set_initial_comment("start");
    let initial = menu_model(&s.game, Some(Hit::Comment(CommentTarget::Initial)));
    assert_eq!(
        item(&initial, MenuLabel::DeleteComment),
        Some(MenuAction::DeleteComment(CommentTarget::Initial))
    );
    assert_eq!(item(&initial, MenuLabel::GoHere), None, "no move to go to");
}

#[test]
fn m2_4_menu_on_empty_space_offers_only_the_global_items() {
    let s = sample();
    let rows = menu_model(&s.game, None);
    assert_eq!(item(&rows, MenuLabel::GoHere), None);
    assert_eq!(item(&rows, MenuLabel::Promote), None);
    assert_eq!(item(&rows, MenuLabel::DeleteComment), None);
    assert_eq!(
        item(&rows, MenuLabel::StripComments),
        Some(MenuAction::StripComments)
    );
    assert_eq!(
        item(&rows, MenuLabel::StripVariations),
        Some(MenuAction::StripVariations)
    );
    assert!(
        !rows.iter().any(|r| matches!(r, MenuRow::Nags(_))),
        "no annotation strip without a move"
    );
}

#[test]
fn m2_4_menu_for_a_node_that_no_longer_exists_does_not_panic_and_acts_on_nothing() {
    let mut s = sample();
    let stale = s.nf6;
    s.game.delete_variation(s.nc3).expect("deleted");
    let rows = menu_model(&s.game, Some(Hit::Move(stale)));
    assert_eq!(item(&rows, MenuLabel::GoHere), None);
    assert_eq!(item(&rows, MenuLabel::Promote), None);
}

#[test]
fn m2_4_removal_items_go_through_the_confirmation_prompt() {
    let s = sample();
    let root = s.game.root();
    for action in [
        MenuAction::StripComments,
        MenuAction::StripVariations,
        MenuAction::TruncateFrom(s.e5),
        MenuAction::TruncateBefore(s.e5),
    ] {
        let messages = menu_messages(action, root, root);
        assert!(
            matches!(
                messages.as_slice(),
                [AppMessage::Game(GameMessage::RequestStrip {
                    confirm: true,
                    ..
                })]
            ),
            "{action:?}: {messages:?}"
        );
    }
}

fn open_menu(rig: &mut Rig, hit: Option<Hit>) {
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Menu {
            hit,
            position: Point::new(120.0, 80.0),
        },
    )));
}

#[test]
fn m2_4_right_click_opens_the_menu_without_moving_the_cursor_and_marks_the_target() {
    let s = sample();
    let (e5, nf3) = (s.e5, s.nf3);
    let mut rig = Rig::with_game(s.game);
    rig.goto(nf3);
    open_menu(&mut rig, Some(Hit::Move(e5)));
    assert_eq!(rig.cursor(), nf3, "D172: the cursor stays");
    let menu = rig.shell.ui.notation.list.menu.expect("menu is open");
    assert_eq!(menu.node, Some(e5));
    assert_eq!(menu.hit, Some(Hit::Move(e5)));
    // The popup is part of the window while it is open.
    assert!(notation::overlay(&rig.shell).is_some());
}

#[test]
fn m2_4_the_menu_closes_with_escape_scroll_close_message_and_a_left_click() {
    let s = sample();
    let (e4, e5) = (s.e4, s.e5);
    let mut rig = Rig::with_game(s.game);
    // Escape.
    open_menu(&mut rig, Some(Hit::Move(e5)));
    rig.press(m2_4_common::named(mychess_app::NamedKey::Escape));
    assert!(rig.shell.ui.notation.list.menu.is_none(), "Esc");
    // Scrolling the list.
    open_menu(&mut rig, Some(Hit::Move(e5)));
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Scrolled,
    )));
    assert!(rig.shell.ui.notation.list.menu.is_none(), "scroll");
    // The click-outside layer.
    open_menu(&mut rig, Some(Hit::Move(e5)));
    rig.msg(Message::Notation(NotationMessage::CloseMenu));
    assert!(rig.shell.ui.notation.list.menu.is_none(), "click outside");
    // A left click on a move.
    open_menu(&mut rig, Some(Hit::Move(e5)));
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Click(e4),
    )));
    assert!(rig.shell.ui.notation.list.menu.is_none(), "left click");
    assert_eq!(rig.cursor(), e4);
}

#[test]
fn m2_4_choosing_an_item_closes_the_menu_and_acts_on_the_target() {
    let s = sample();
    let (e5, nc3) = (s.e5, s.nc3);
    let mut rig = Rig::with_game(s.game);
    open_menu(&mut rig, Some(Hit::Move(s.nf6)));
    rig.msg(Message::Notation(NotationMessage::Menu(
        MenuAction::Promote(nc3),
    )));
    assert!(rig.shell.ui.notation.list.menu.is_none());
    // The variation is now the main continuation of e5.
    assert_eq!(rig.game().children(e5).unwrap()[0], nc3);
}

#[test]
fn m2_4_delete_variation_through_the_menu_removes_exactly_that_variation() {
    let s = sample();
    let (e5, nc3, nf3) = (s.e5, s.nc3, s.nf3);
    let mut rig = Rig::with_game(s.game);
    rig.msg(Message::Notation(NotationMessage::Menu(
        MenuAction::DeleteVariation(nc3),
    )));
    assert_eq!(rig.game().children(e5).unwrap(), [nf3]);
}

#[test]
fn m2_4_strip_comments_through_the_menu_asks_with_the_count_before_removing() {
    let mut s = sample();
    s.game.set_comment(s.e4, "one").unwrap();
    s.game.set_comment(s.e5, "two").unwrap();
    let (e4, e5) = (s.e4, s.e5);
    let mut rig = Rig::with_game(s.game);
    rig.msg(Message::Notation(NotationMessage::Menu(
        MenuAction::StripComments,
    )));
    assert!(
        matches!(
            rig.shell.app.session.prompt(),
            Some(Prompt::ConfirmStrip {
                kind: StripKind::Comments,
                count: 2
            })
        ),
        "{:?}",
        rig.shell.app.session.prompt()
    );
    assert!(rig.game().comment(e4).unwrap().is_some(), "not removed yet");
    rig.app_msg(AppMessage::Game(GameMessage::AnswerStrip(true)));
    assert!(rig.game().comment(e4).unwrap().is_none());
    assert!(rig.game().comment(e5).unwrap().is_none());
}

#[test]
fn m2_4_truncating_from_the_menu_removes_the_move_and_what_follows_after_confirming() {
    let s = sample();
    let (e5, nf3, e4) = (s.e5, s.nf3, s.e4);
    let mut rig = Rig::with_game(s.game);
    rig.msg(Message::Notation(NotationMessage::Menu(
        MenuAction::TruncateFrom(nf3),
    )));
    assert!(rig.shell.app.session.prompt().is_some());
    rig.app_msg(AppMessage::Game(GameMessage::AnswerStrip(true)));
    // The Nc3 variation stays; nf3 and its descendants are gone.
    assert!(!rig.game().contains(nf3));
    assert!(rig.game().contains(e5) && rig.game().contains(e4));
}

#[test]
fn m2_4_an_annotation_chosen_in_the_menu_acts_on_the_clicked_move_without_moving_the_cursor() {
    // D172 and resolved question 4: the menu acts on the node clicked,
    // without moving the cursor.
    let s = sample();
    let (e4, nf3) = (s.e4, s.nf3);
    let mut rig = Rig::with_game(s.game);
    rig.goto(nf3);
    let good = Nag::new(1).unwrap();
    rig.msg(Message::Notation(NotationMessage::Menu(
        MenuAction::ToggleNag(e4, good),
    )));
    assert_eq!(
        rig.game().nags(e4).unwrap(),
        [good],
        "the clicked move got it"
    );
    assert_eq!(rig.cursor(), nf3, "the cursor did not move");
}

// ---------------------------------------------------------------------------
// Clicks (D173, rule 15)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_a_left_click_on_a_move_goes_there_and_on_a_nag_too() {
    let s = sample();
    let (e5, nf3) = (s.e5, s.nf3);
    let mut rig = Rig::with_game(s.game);
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Click(e5),
    )));
    assert_eq!(rig.cursor(), e5);
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Click(nf3),
    )));
    assert_eq!(rig.cursor(), nf3);
}

#[test]
fn m2_4_a_click_on_a_comment_opens_the_editor_on_that_comment() {
    let mut s = sample();
    s.game.set_comment(s.e5, "reply").unwrap();
    s.game.set_comment_before(s.nc3, "before").unwrap();
    s.game.set_initial_comment("game");
    let (e5, nc3, nf3) = (s.e5, s.nc3, s.nf3);
    let mut rig = Rig::with_game(s.game);
    rig.goto(nf3);
    assert!(!rig.shell.ui.comment_visible());

    // After(n): goes to n and opens the editor on it.
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::CommentClick(CommentTarget::After(e5)),
    )));
    assert!(rig.shell.ui.comment_visible());
    assert_eq!(rig.cursor(), e5);
    assert_eq!(
        rig.shell.ui.notation.editor.target(),
        CommentTarget::After(e5)
    );
    assert_eq!(rig.shell.ui.notation.editor.text(), "reply");

    // Before(n): the cursor does not move.
    rig.goto(nf3);
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::CommentClick(CommentTarget::Before(nc3)),
    )));
    assert_eq!(
        rig.cursor(),
        nf3,
        "a comment before a variation keeps the cursor"
    );
    assert_eq!(
        rig.shell.ui.notation.editor.target(),
        CommentTarget::Before(nc3)
    );
    assert_eq!(rig.shell.ui.notation.editor.text(), "before");

    // Initial.
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::CommentClick(CommentTarget::Initial),
    )));
    assert_eq!(
        rig.shell.ui.notation.editor.target(),
        CommentTarget::Initial
    );
    assert_eq!(rig.shell.ui.notation.editor.text(), "game");
}

#[test]
fn m2_4_clicks_on_a_stale_node_do_not_panic_and_leave_the_cursor() {
    let s = sample();
    let (nf3, nf6) = (s.nf3, s.nf6);
    let mut rig = Rig::with_game(s.game);
    rig.goto(nf3);
    // Delete the variation behind the app's back, then click its old move.
    rig.app_msg(AppMessage::Game(GameMessage::DeleteVariation(
        mychess_app::VariationTarget::Node(s.nc3),
    )));
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Click(nf6),
    )));
    assert_eq!(rig.cursor(), nf3);
}

// ---------------------------------------------------------------------------
// Hover preview (D171, rule 20)
// ---------------------------------------------------------------------------

fn hover(rig: &mut Rig, node: NodeId) {
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Hover(Some((node, Point::new(50.0, 50.0)))),
    )));
}

fn tick(rig: &mut Rig, at: Instant) {
    rig.msg(Message::Notation(NotationMessage::HoverTick(at)));
}

fn preview_open(rig: &Rig) -> bool {
    rig.shell.ui.notation.list.preview_open()
}

#[test]
fn m2_4_the_preview_opens_only_after_600_ms_of_hover() {
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    let t0 = Instant::now();
    hover(&mut rig, s.e5);
    tick(&mut rig, t0);
    tick(&mut rig, t0 + Duration::from_millis(300));
    tick(&mut rig, t0 + Duration::from_millis(599));
    assert!(!preview_open(&rig), "599 ms is too early");
    tick(&mut rig, t0 + Duration::from_millis(600));
    assert!(preview_open(&rig), "600 ms");
}

#[test]
fn m2_4_moving_to_another_move_restarts_the_wait() {
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    let t0 = Instant::now();
    hover(&mut rig, s.e4);
    tick(&mut rig, t0);
    tick(&mut rig, t0 + Duration::from_millis(500));
    hover(&mut rig, s.e5);
    tick(&mut rig, t0 + Duration::from_millis(550));
    tick(&mut rig, t0 + Duration::from_millis(1100));
    assert!(!preview_open(&rig), "only 550 ms on the new move");
    tick(&mut rig, t0 + Duration::from_millis(1150));
    assert!(preview_open(&rig));
}

#[test]
fn m2_4_leaving_the_move_cancels_a_pending_preview_and_closes_an_open_one() {
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    let t0 = Instant::now();
    hover(&mut rig, s.e4);
    tick(&mut rig, t0);
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Hover(None),
    )));
    tick(&mut rig, t0 + Duration::from_secs(5));
    assert!(!preview_open(&rig));
    hover(&mut rig, s.e4);
    tick(&mut rig, t0 + Duration::from_secs(6));
    tick(&mut rig, t0 + Duration::from_secs(7));
    assert!(preview_open(&rig));
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Hover(None),
    )));
    assert!(!preview_open(&rig), "leaves with the pointer");
}

#[test]
fn m2_4_the_preview_can_be_switched_off_and_then_never_opens() {
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    let options = NotationOptions {
        hover_preview: HoverPreview::Off,
        ..*rig.shell.app.notation_options()
    };
    rig.app_msg(AppMessage::Prefs(PrefsMessage::Set(PrefChange::Notation(
        options,
    ))));
    let t0 = Instant::now();
    hover(&mut rig, s.e5);
    tick(&mut rig, t0);
    tick(&mut rig, t0 + Duration::from_secs(10));
    assert!(!preview_open(&rig));
    assert!(
        !rig.shell
            .ui
            .notation
            .list
            .hover_pending(rig.shell.app.notation_options()),
        "no timer while the option is off"
    );
}

#[test]
fn m2_4_the_hover_timer_runs_only_while_a_hover_is_pending() {
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    assert!(notation::subscriptions(&rig.shell).is_empty(), "idle");
    hover(&mut rig, s.e5);
    assert_eq!(notation::subscriptions(&rig.shell).len(), 1, "pending");
    let t0 = Instant::now();
    tick(&mut rig, t0);
    tick(&mut rig, t0 + Duration::from_millis(700));
    assert!(preview_open(&rig));
    assert!(
        notation::subscriptions(&rig.shell).is_empty(),
        "no timer once the preview is open"
    );
}

#[test]
fn m2_4_the_preview_never_opens_while_a_menu_is_open() {
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    open_menu(&mut rig, Some(Hit::Move(s.e4)));
    hover(&mut rig, s.e5);
    let t0 = Instant::now();
    tick(&mut rig, t0);
    tick(&mut rig, t0 + Duration::from_secs(3));
    assert!(!preview_open(&rig));
}

#[test]
fn m2_4_the_middle_button_previews_at_once_and_release_closes_it() {
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Middle(Some((s.e5, Point::new(20.0, 20.0)))),
    )));
    assert!(preview_open(&rig), "no waiting");
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Middle(None),
    )));
    assert!(!preview_open(&rig));
}

#[test]
fn m2_4_escape_scrolling_and_dialogs_close_the_preview() {
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    let middle = |rig: &mut Rig, node| {
        rig.msg(Message::Notation(NotationMessage::Event(
            NotationEvent::Middle(Some((node, Point::new(20.0, 20.0)))),
        )));
    };
    middle(&mut rig, s.e5);
    rig.press(m2_4_common::named(mychess_app::NamedKey::Escape));
    assert!(!preview_open(&rig), "Esc");
    middle(&mut rig, s.e5);
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Scrolled,
    )));
    assert!(!preview_open(&rig), "scroll");
}

#[cfg(target_os = "linux")]
#[test]
fn m2_4_a_dialog_on_top_hides_the_preview_in_the_window() {
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    let size = Size::new(1000.0, 700.0);
    rig.msg(Message::Ui(UiMessage::WindowResized(size)));
    rig.app_msg(AppMessage::Game(GameMessage::RequestStrip {
        kind: StripKind::Variations,
        confirm: true,
    }));
    assert!(
        rig.shell.app.session.prompt().is_some(),
        "the question is open"
    );
    let without = m2_4_common::fingerprint(&mut rig.sim(size));
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Middle(Some((s.e5, Point::new(300.0, 300.0)))),
    )));
    assert!(preview_open(&rig));
    let with = m2_4_common::fingerprint(&mut rig.sim(size));
    assert_eq!(without, with, "no preview under a dialog (rule 20)");
}

#[test]
fn m2_4_changing_the_game_under_a_preview_or_menu_drops_them() {
    // Node ids kept by the panel must not survive an edit (architecture
    // 2.2.1): undo is such an edit.
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    rig.app_msg(AppMessage::Game(GameMessage::StripComments));
    open_menu(&mut rig, Some(Hit::Move(s.e5)));
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Middle(Some((s.e5, Point::new(20.0, 20.0)))),
    )));
    rig.app_msg(AppMessage::Game(GameMessage::DeleteVariation(
        mychess_app::VariationTarget::Node(s.nc3),
    )));
    assert!(rig.shell.ui.notation.list.menu.is_none());
    assert!(!preview_open(&rig));
}

// ---------------------------------------------------------------------------
// Reveal token (D170)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_the_reveal_token_grows_for_outside_moves_and_not_for_clicks_in_the_panel() {
    let s = sample();
    let (e4, e5) = (s.e4, s.e5);
    let mut rig = Rig::with_game(s.game);
    let token = |rig: &Rig| rig.shell.ui.notation.list.reveal_token;
    let t0 = token(&rig);
    // A click in the panel: no jump.
    rig.msg(Message::Notation(NotationMessage::Event(
        NotationEvent::Click(e4),
    )));
    assert_eq!(token(&rig), t0, "clicking does not scroll");
    // Navigation from elsewhere: the panel must follow.
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(
        mychess_app::Nav::Forward(1),
    )));
    assert_eq!(rig.cursor(), e5);
    assert!(token(&rig) > t0, "forward key reveals the move");
    let t1 = token(&rig);
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(
        mychess_app::Nav::Back(1),
    )));
    assert!(token(&rig) > t1, "back reveals too");
}

#[test]
fn m2_4_a_new_game_asks_the_panel_to_scroll_back() {
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    let before = rig.shell.ui.notation.list.reveal_token;
    rig.set_game(Game::new());
    assert!(rig.shell.ui.notation.list.reveal_token > before);
}

// ---------------------------------------------------------------------------
// Render relations (Linux only; compared within one run)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod render {
    use super::*;
    use iced::Settings;
    use iced_test::Simulator;
    use m2_4_common::fingerprint;
    use mychess_gui::widgets::notation::NotationView;

    fn view_hash(doc: &NotationDoc, current: Option<NodeId>, token: u64) -> String {
        let _guard = m2_4_common::render_lock();
        let labels = labels();
        let style = text_style(&Theme::Light, TEXT_SIZE, true, false);
        let element = NotationView::new(doc, &labels, style, current, |e| e)
            .reveal_token(token)
            .into_element();
        let mut sim = Simulator::with_size(Settings::default(), Size::new(560.0, 300.0), element);
        fingerprint(&mut sim)
    }

    #[test]
    fn m2_4_the_current_move_is_drawn_differently_from_the_others() {
        let s = sample();
        let doc = build_notation(&s.game, &NotationOptions::default(), 1);
        let none = view_hash(&doc, None, 0);
        let at_e4 = view_hash(&doc, Some(s.e4), 0);
        let at_e5 = view_hash(&doc, Some(s.e5), 0);
        let again = view_hash(&doc, Some(s.e4), 0);
        assert_ne!(none, at_e4, "the highlight is visible");
        assert_ne!(at_e4, at_e5, "and follows the cursor");
        assert_eq!(at_e4, again, "deterministic");
    }

    #[test]
    fn m2_4_indented_and_in_line_layouts_look_different() {
        let s = sample();
        let indented = build_notation(&s.game, &NotationOptions::default(), 1);
        let inline = build_notation(
            &s.game,
            &NotationOptions {
                layout: NotationLayout::Inline,
                ..NotationOptions::default()
            },
            1,
        );
        assert_ne!(view_hash(&indented, None, 0), view_hash(&inline, None, 0));
    }

    #[test]
    fn m2_4_a_long_game_scrolls_to_the_current_move_when_the_token_says_so() {
        let mut game = Game::new();
        let root = game.root();
        let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
        let mut at = root;
        let mut nodes = Vec::new();
        for ply in 0..400 {
            at = play(&mut game, at, cycle[ply % 4]);
            nodes.push(at);
        }
        let doc = build_notation(&game, &NotationOptions::default(), 1);
        let top = view_hash(&doc, Some(nodes[0]), 0);
        let end = view_hash(&doc, Some(nodes[399]), 1);
        assert_ne!(
            top, end,
            "the last move is not at the top: the view scrolled"
        );
        // Same cursor, but at the start of the game the view shows the top.
        let start = view_hash(&doc, None, 0);
        assert_ne!(start, end);
    }

    #[test]
    fn m2_4_the_menu_target_is_highlighted_while_the_menu_is_open() {
        let _guard = m2_4_common::render_lock();
        let s = sample();
        let doc = build_notation(&s.game, &NotationOptions::default(), 1);
        let labels = labels();
        let make = |target: Option<NodeId>| {
            let style = text_style(&Theme::Light, TEXT_SIZE, true, false);
            let element = NotationView::new(&doc, &labels, style, None, |e| e)
                .menu_target(target)
                .into_element();
            let mut sim =
                Simulator::with_size(Settings::default(), Size::new(560.0, 300.0), element);
            fingerprint(&mut sim)
        };
        assert_ne!(make(None), make(Some(s.e5)));
        assert_ne!(make(Some(s.e4)), make(Some(s.e5)));
    }

    #[test]
    fn m2_4_both_themes_draw_the_panel() {
        let _guard = m2_4_common::render_lock();
        let s: Sample = sample();
        let doc = build_notation(&s.game, &NotationOptions::default(), 1);
        let labels = labels();
        let draw = |theme: &Theme| {
            let style = text_style(theme, TEXT_SIZE, true, false);
            let element = NotationView::new(&doc, &labels, style, Some(s.e4), |e| e).into_element();
            let mut sim =
                Simulator::with_size(Settings::default(), Size::new(560.0, 300.0), element);
            sim.snapshot(theme).is_ok()
        };
        assert!(draw(&Theme::Light) && draw(&Theme::Dark));
    }
}

#[test]
fn m2_4_the_window_view_shows_the_menu_card_with_translated_items() {
    let s = sample();
    let mut rig = Rig::with_game(s.game);
    rig.msg(Message::Ui(UiMessage::WindowResized(Size::new(
        1280.0, 800.0,
    ))));
    open_menu(&mut rig, Some(Hit::Move(s.nc3)));
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    for label in [
        "Go to this move",
        "Promote variation",
        "Make first variation",
        "Delete variation",
        "Strip all comments",
        "Edit comment",
    ] {
        assert!(sim.find(label).is_ok(), "{label}");
    }
}
