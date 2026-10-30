//! The modal layer: a card over a dimmed backdrop that blocks everything
//! under it (spec M2-4, D186).
//!
//! [`modal`] builds the layer to push on a `stack`. Mouse events never reach
//! the page below (`opaque`). The keyboard is trapped by the shell: `Esc`
//! cancels, `Enter` runs the default button and `Tab` cycles only through
//! the widgets of the card ([`cycle_focus`]).

use iced::advanced::widget::operation::{self, Focusable, Operation, Outcome};
use iced::advanced::widget::{self, Id};
use iced::widget::{column, container, opaque, row, space, text};
use iced::{Element, Length, Rectangle, Task, Theme};

use crate::theme;

/// What a modal box shows.
pub struct Card<'a, M> {
    /// Id of the card container: the focus scope and the handle for tests.
    pub id: &'static str,
    /// The title, already translated.
    pub title: String,
    /// The content.
    pub body: Element<'a, M>,
    /// The buttons, in order, pushed to the right.
    pub buttons: Vec<Element<'a, M>>,
    /// The width of the card in logical pixels.
    pub width: f32,
}

/// The modal layer for `card`: backdrop, centered card, title, body and a
/// row of buttons.
pub fn modal<'a, M: 'a>(card: Card<'a, M>) -> Element<'a, M> {
    let buttons = row![space::horizontal()]
        .extend(card.buttons)
        .spacing(8)
        .width(Length::Fill);
    let content = column![text(card.title).size(18), card.body, buttons]
        .spacing(14)
        .width(Length::Fixed(card.width));
    let body = container(content)
        .id(card.id)
        .padding(20)
        .style(theme::pane_body);
    opaque(container(body).center(Length::Fill).style(theme::scrim))
}

/// How far a pass of [`cycle_focus`] counted.
#[derive(Debug, Clone)]
struct Tally {
    scope: Id,
    backwards: bool,
    total: usize,
    focused: Option<usize>,
}

/// Follows the traversal and tells whether the widgets seen are inside the
/// scope container.
#[derive(Debug)]
struct Scope {
    id: Id,
    inside: bool,
    entering: bool,
}

impl Scope {
    fn new(id: Id) -> Scope {
        Scope {
            id,
            inside: false,
            entering: false,
        }
    }

    fn enter(&mut self, id: Option<&Id>) {
        self.entering = id == Some(&self.id);
    }

    /// Starts the children of the container announced by [`Scope::enter`]:
    /// they are inside when it is the scope. Returns what to give back to
    /// [`Scope::leave`].
    fn descend(&mut self) -> bool {
        let entering = std::mem::take(&mut self.entering);
        let was = self.inside;
        self.inside = was || entering;
        was
    }

    fn leave(&mut self, was: bool) {
        self.inside = was;
    }
}

struct CountPass {
    scope: Scope,
    tally: Tally,
}

impl Operation<Tally> for CountPass {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Tally>)) {
        let was = self.scope.descend();
        operate(self);
        self.scope.leave(was);
    }

    fn container(&mut self, id: Option<&Id>, _bounds: Rectangle) {
        self.scope.enter(id);
    }

    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if self.scope.inside {
            if state.is_focused() {
                self.tally.focused = Some(self.tally.total);
            }
            self.tally.total += 1;
        }
    }

    fn finish(&self) -> Outcome<Tally> {
        Outcome::Some(self.tally.clone())
    }
}

struct FocusPass {
    scope: Scope,
    tally: Tally,
    current: usize,
}

impl Operation<()> for FocusPass {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
        let was = self.scope.descend();
        operate(self);
        self.scope.leave(was);
    }

    fn container(&mut self, id: Option<&Id>, _bounds: Rectangle) {
        self.scope.enter(id);
    }

    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if !self.scope.inside {
            return;
        }
        let at = self.current;
        self.current += 1;
        let total = self.tally.total;
        if total == 0 {
            return;
        }
        let target = match (self.tally.focused, self.tally.backwards) {
            (None, false) => 0,
            (None, true) => total - 1,
            (Some(focused), false) => (focused + 1) % total,
            (Some(focused), true) => (focused + total - 1) % total,
        };
        if Some(at) == self.tally.focused && at != target {
            state.unfocus();
        }
        if at == target {
            state.focus();
        }
    }
}

/// Moves the keyboard focus to the next (or previous) focusable widget
/// *inside* the container with id `scope`, wrapping around: the focus trap
/// of a modal box. The page below is never reached.
pub fn cycle_focus<T: Send + 'static>(scope: &'static str, backwards: bool) -> Task<T> {
    let scope = Id::new(scope);
    let first = CountPass {
        scope: Scope::new(scope.clone()),
        tally: Tally {
            scope,
            backwards,
            total: 0,
            focused: None,
        },
    };
    let chained = operation::then(first, |tally: Tally| FocusPass {
        scope: Scope::new(tally.scope.clone()),
        tally,
        current: 0,
    });
    widget::operate(chained).discard()
}

/// Takes the keyboard focus away from whatever widget has it (a text field
/// under a layer that must get the keys).
pub fn unfocus_all<T: Send + 'static>() -> Task<T> {
    widget::operate(operation::focusable::unfocus::<()>()).discard()
}

/// The style of a card button that is the default answer (`Enter`).
pub fn default_button(
    theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    iced::widget::button::primary(theme, status)
}

#[cfg(test)]
mod tests {
    use iced::widget::{button, text_input};
    use iced::{Settings, Size};
    use iced_test::Simulator;

    use super::*;

    #[derive(Debug, Clone)]
    enum Msg {
        Ok,
        Edited(String),
    }

    fn sample<'a>() -> Element<'a, Msg> {
        modal(Card {
            id: "sample-card",
            title: "Title".to_owned(),
            body: text_input("hint", "")
                .id("sample-field")
                .on_input(Msg::Edited)
                .into(),
            buttons: vec![button(text("Done")).on_press(Msg::Ok).into()],
            width: 320.0,
        })
    }

    #[test]
    fn the_card_shows_title_body_and_buttons_and_answers_clicks() {
        let mut sim = Simulator::with_size(Settings::default(), Size::new(600.0, 400.0), sample());
        assert!(sim.find("Title").is_ok());
        assert!(sim.find(Id::new("sample-card")).is_ok());
        sim.click("Done").expect("button");
        sim.click(Id::new("sample-field")).expect("field");
        sim.typewrite("x");
        let sent: Vec<Msg> = sim.into_messages().collect();
        assert!(matches!(sent.as_slice(), [Msg::Ok, Msg::Edited(t)] if t == "x"));
    }

    #[test]
    fn scope_flags_follow_the_container_that_announced_them() {
        let mut scope = Scope::new(Id::new("a"));
        scope.enter(Some(&Id::new("b")));
        let was = scope.descend();
        assert!(!scope.inside, "another container does not open the scope");
        scope.leave(was);
        scope.enter(Some(&Id::new("a")));
        let was = scope.descend();
        assert!(scope.inside);
        scope.leave(was);
        assert!(!scope.inside, "leaving restores the flag");
    }
}
