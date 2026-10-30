//! A wrapper that reports where the right button was pressed (spec M3-4,
//! slice 4c, D563).
//!
//! `mouse_area` publishes a right press without saying where, and the
//! context menu of the game list needs the pointer in window coordinates to
//! place the card (see `widgets::context_menu`). The callback gets that
//! point and the point relative to the wrapped widget (to tell which row
//! was hit). The press is not captured, so the content still sees it.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, mouse};

/// See the [module](self) documentation.
pub struct RightClick<'a, Message> {
    content: Element<'a, Message>,
    on_right_click: Box<dyn Fn(Point, Point) -> Message + 'a>,
}

impl<'a, Message> RightClick<'a, Message> {
    /// Wraps `content`; `on_right_click(window_point, local_point)`.
    pub fn new(
        content: impl Into<Element<'a, Message>>,
        on_right_click: impl Fn(Point, Point) -> Message + 'a,
    ) -> RightClick<'a, Message> {
        RightClick {
            content: content.into(),
            on_right_click: Box::new(on_right_click),
        }
    }
}

impl<Message: Clone> Widget<Message, iced::Theme, iced::Renderer> for RightClick<'_, Message> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
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
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
        if !shell.is_event_captured()
            && let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event
            && let Some(local) = cursor.position_in(layout.bounds())
            && let Some(window) = cursor.position()
        {
            shell.publish((self.on_right_click)(window, local));
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
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
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

impl<'a, Message: Clone + 'a> From<RightClick<'a, Message>> for Element<'a, Message> {
    fn from(widget: RightClick<'a, Message>) -> Element<'a, Message> {
        Element::new(widget)
    }
}
