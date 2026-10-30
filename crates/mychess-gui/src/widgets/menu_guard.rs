//! Guard for the `iced_aw` menu bar (spec M5-2, D992).
//!
//! `iced_aw` 0.14.1 (the latest release) panics in
//! `MenuBarOverlay::operate` (`menu_bar_overlay.rs`, `tree.children[1]`)
//! whenever a widget operation (`focus`, `scroll_to`, `snap_to`, ...) runs
//! while a menu is open and the open menu has a submenu that was never
//! opened: the state of a submenu only exists once it was opened. Menus hold
//! no focusable or scrollable widget, so [`guard`] wraps the bar and its
//! overlay delegates everything except `operate`, which does nothing.
//!
//! Remove this module once `iced_aw` fixes the traversal upstream.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay::Element as OverlayElement;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Theme, Vector, mouse};

/// Wraps `bar` (an `iced_aw` `MenuBar` or anything that shows menus as an
/// overlay) so that widget operations skip its menu overlay.
pub fn guard<'a, Message: 'a>(bar: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(MenuGuard { inner: bar.into() })
}

struct MenuGuard<'a, Message> {
    inner: Element<'a, Message>,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for MenuGuard<'_, Message> {
    fn tag(&self) -> tree::Tag {
        self.inner.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.inner.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.inner.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.inner.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.inner.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.inner.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.inner
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.inner.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.inner
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.inner
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<OverlayElement<'b, Message, Theme, iced::Renderer>> {
        let inner =
            self.inner
                .as_widget_mut()
                .overlay(tree, layout, renderer, viewport, translation)?;
        Some(OverlayElement::new(Box::new(NoOperate { inner })))
    }
}

/// An overlay that delegates everything but `operate`.
struct NoOperate<'a, Message> {
    inner: OverlayElement<'a, Message, Theme, iced::Renderer>,
}

impl<Message> overlay::Overlay<Message, Theme, iced::Renderer> for NoOperate<'_, Message> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        self.inner.as_overlay_mut().layout(renderer, bounds)
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.inner
            .as_overlay()
            .draw(renderer, theme, style, layout, cursor);
    }

    // `operate` is intentionally the default no-op (see the module docs).

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        self.inner
            .as_overlay_mut()
            .update(event, layout, cursor, renderer, clipboard, shell);
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.inner
            .as_overlay()
            .mouse_interaction(layout, cursor, renderer)
    }

    fn overlay<'a>(
        &'a mut self,
        layout: Layout<'a>,
        renderer: &iced::Renderer,
    ) -> Option<OverlayElement<'a, Message, Theme, iced::Renderer>> {
        self.inner.as_overlay_mut().overlay(layout, renderer)
    }

    fn index(&self) -> f32 {
        self.inner.as_overlay().index()
    }
}
