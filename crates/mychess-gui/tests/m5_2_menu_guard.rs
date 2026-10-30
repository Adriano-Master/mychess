//! Regression for D992: `iced_aw` 0.14.1 panics in `MenuBarOverlay::operate`
//! when a widget operation runs with a menu open that has a submenu never
//! opened. `menu_guard::guard` must absorb it.

use std::panic::{AssertUnwindSafe, catch_unwind};

use iced::advanced::widget::{Id, operation};
use iced::widget::text;
use iced_aw::menu::{Item, Menu, MenuBar};
use iced_test::core::renderer::Headless;
use iced_test::core::{Element, Event, Font, Pixels, Point, Size, Theme, mouse, time, window};
use iced_test::renderer::Renderer;
use iced_test::runtime::UserInterface;
use iced_test::runtime::user_interface::Cache;
use mychess_gui::widgets::menu_guard::guard;

type El = Element<'static, (), Theme, Renderer>;

fn bar() -> MenuBar<'static, (), Theme, Renderer> {
    let sub = Menu::new(vec![Item::new(text("leaf"))]);
    let menu = Menu::new(vec![
        Item::new(text("plain")),
        Item::with_menu(text("with submenu"), sub),
    ]);
    MenuBar::new(vec![Item::with_menu(text("Root"), menu)])
}

/// Opens the menu, runs a `focus` operation and reports whether it panicked.
fn operate_with_open_menu(element: El) -> bool {
    let mut renderer = iced_test::futures::futures::executor::block_on(Renderer::new(
        Font::DEFAULT,
        Pixels(16.0),
        std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
    ))
    .expect("a headless renderer");
    let size = Size::new(400.0, 300.0);
    let mut ui = UserInterface::build(element, size, Cache::default(), &mut renderer);
    let at = mouse::Cursor::Available(Point::new(10.0, 8.0));
    let mut messages = Vec::new();
    for event in [
        Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(10.0, 8.0),
        }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        Event::Window(window::Event::RedrawRequested(time::Instant::now())),
    ] {
        drop(ui.update(
            &[event],
            at,
            &mut renderer,
            &mut iced_test::core::clipboard::Null,
            &mut messages,
        ));
    }
    let mut op = operation::focusable::focus::<()>(Id::new("nobody"));
    catch_unwind(AssertUnwindSafe(|| ui.operate(&renderer, &mut op))).is_err()
}

#[test]
fn operating_with_an_open_menu_does_not_panic_when_guarded() {
    assert!(!operate_with_open_menu(guard(bar())));
}

/// Documents the upstream bug (and proves the test above would catch it).
/// If this starts failing, `iced_aw` was fixed: drop `menu_guard`.
#[test]
fn the_unguarded_bar_still_panics_upstream() {
    assert!(operate_with_open_menu(bar().into()));
}
