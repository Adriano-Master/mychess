//! The flow of the manual window (spec M9-5, slice b): where the reader is,
//! the history, the search by title and the questions about links. The
//! texts are the front-end's; nothing is read here.

use super::AppState;
use crate::effect::Effect;
use crate::manual::{
    HISTORY_LIMIT, LinkTarget, MAX_TEXT_SCALE, ManualMessage, ManualNotice, ManualPrefs,
    ManualState, Place, resolve_link,
};
use crate::prefs::{PrefChange, PrefsMessage};

impl ManualState {
    /// Goes to a place: the forward branch is dropped, the history keeps
    /// [`HISTORY_LIMIT`] entries. The chapter is checked against the index.
    fn go(&mut self, chapter: usize, anchor: Option<String>) {
        let last = self.index().map_or(0, |i| i.len().saturating_sub(1));
        let chapter = chapter.min(last);
        let anchor = anchor.filter(|a| self.index().is_some_and(|i| i.has_anchor(chapter, a)));
        if !self.history.is_empty() {
            self.history.truncate(self.cursor + 1);
        }
        self.history.push(Place {
            chapter,
            anchor,
            scroll: 0.0,
        });
        if self.history.len() > HISTORY_LIMIT {
            let extra = self.history.len() - HISTORY_LIMIT;
            self.history.drain(..extra);
        }
        self.cursor = self.history.len() - 1;
        self.nav_serial += 1;
        self.notice = None;
    }

    fn step(&mut self, forward: bool) {
        let next = if forward {
            self.cursor
                .checked_add(1)
                .filter(|&n| n < self.history.len())
        } else {
            self.cursor.checked_sub(1)
        };
        if let Some(next) = next {
            self.cursor = next;
            self.nav_serial += 1;
            self.notice = None;
        }
    }

    /// Clicks on a link of the text of the chapter in view.
    fn follow(&mut self, uri: &str, effects: &mut Vec<Effect>) {
        match resolve_link(self.chapter(), uri) {
            LinkTarget::Chapter { chapter, anchor } => {
                let known = self.index().is_some_and(|i| {
                    chapter < i.len() && anchor.as_deref().is_none_or(|a| i.has_anchor(chapter, a))
                });
                if known {
                    self.go(chapter, anchor);
                } else {
                    self.notice = Some(ManualNotice::LinkNotFound);
                }
            }
            LinkTarget::External(url) => {
                if self.external_confirmed {
                    self.notice = None;
                    effects.push(Effect::OpenUrl(url));
                } else {
                    self.external = Some(url);
                }
            }
            LinkTarget::Ignored => self.notice = Some(ManualNotice::LinkIgnored),
            LinkTarget::NotFound => self.notice = Some(ManualNotice::LinkNotFound),
        }
    }

    fn set_query(&mut self, text: String) {
        self.results = self.index().map(|i| i.search(&text)).unwrap_or_default();
        self.query = text;
    }
}

impl AppState {
    /// The state of the manual window.
    #[must_use]
    pub fn manual(&self) -> &ManualState {
        &self.manual
    }

    /// Handles a [`ManualMessage`].
    pub(crate) fn manual_message(&mut self, message: ManualMessage, effects: &mut Vec<Effect>) {
        let manual = &mut self.manual;
        match message {
            ManualMessage::Open {
                index,
                chapter,
                anchor,
            } => {
                let first = !manual.open;
                manual.index = Some(index);
                manual.open = true;
                if first {
                    // A new session of reading: the old trail and search go.
                    manual.history.clear();
                    manual.cursor = 0;
                    manual.query.clear();
                    manual.results.clear();
                    manual.external = None;
                }
                manual.go(chapter, anchor);
            }
            ManualMessage::Reindex(index) => {
                manual.index = Some(index);
                let query = std::mem::take(&mut manual.query);
                manual.set_query(query);
                // Entries of the trail that name a heading the new texts do
                // not have lose the heading; chapters beyond the end go.
                let known = manual.index().map(|i| (i.len(), i.clone()));
                if let Some((len, index)) = known {
                    for place in &mut manual.history {
                        place.chapter = place.chapter.min(len.saturating_sub(1));
                        if place
                            .anchor
                            .as_deref()
                            .is_some_and(|a| !index.has_anchor(place.chapter, a))
                        {
                            place.anchor = None;
                        }
                    }
                }
                manual.nav_serial += 1;
            }
            ManualMessage::Close => {
                manual.open = false;
                manual.external = None;
                manual.notice = None;
            }
            ManualMessage::Link(uri) => manual.follow(&uri, effects),
            ManualMessage::Goto { chapter, anchor } => manual.go(chapter, anchor),
            ManualMessage::RememberScroll(offset) => {
                if let Some(place) = manual.history.get_mut(manual.cursor) {
                    place.scroll = if offset.is_finite() {
                        offset.max(0.0)
                    } else {
                        0.0
                    };
                }
            }
            ManualMessage::Back => manual.step(false),
            ManualMessage::Forward => manual.step(true),
            ManualMessage::Home => manual.go(0, None),
            ManualMessage::Query(text) => manual.set_query(text),
            ManualMessage::PickResult(i) => {
                if let Some(hit) = manual.results.get(i).cloned() {
                    manual.go(hit.chapter, hit.anchor);
                }
            }
            ManualMessage::Scale(steps) => {
                let current = i16::from(self.prefs.manual.text_scale);
                let next = (current + i16::from(steps)).clamp(0, i16::from(MAX_TEXT_SCALE));
                let next = ManualPrefs {
                    text_scale: u8::try_from(next).unwrap_or(self.prefs.manual.text_scale),
                };
                if next != *self.prefs.manual {
                    self.prefs_message(PrefsMessage::Set(PrefChange::Manual(next)), effects);
                }
            }
            ManualMessage::ConfirmExternal(yes) => {
                if let Some(url) = manual.external.take()
                    && yes
                {
                    manual.external_confirmed = true;
                    effects.push(Effect::OpenUrl(url));
                }
            }
            ManualMessage::UrlOpened(ok) => {
                manual.notice = (!ok).then_some(ManualNotice::UrlRefused);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manual::{ManualIndex, SharedIndex};

    fn index() -> SharedIndex {
        let mut texts = vec!["# Zero\n"; 15];
        texts[3] = "# Bases\n\n## Os slots e a Clipbase\n\n## Abrir\n";
        texts[4] = "# Buscas\n";
        SharedIndex::new(ManualIndex::build(&texts, false))
    }

    fn open(state: &mut ManualState, chapter: usize, anchor: Option<&str>) {
        let mut effects = Vec::new();
        run(
            state,
            ManualMessage::Open {
                index: index(),
                chapter,
                anchor: anchor.map(str::to_owned),
            },
            &mut effects,
        );
    }

    /// Runs a message on a bare state (the scale needs the preferences, so
    /// tests of it go through `AppState` in the integration tests).
    fn run(state: &mut ManualState, message: ManualMessage, effects: &mut Vec<Effect>) {
        // Only the messages that touch no preference.
        let mut app = AppState::default();
        std::mem::swap(&mut app.manual, state);
        app.manual_message(message, effects);
        std::mem::swap(&mut app.manual, state);
    }

    #[test]
    fn open_goes_to_the_chapter_and_checks_the_anchor() {
        let mut state = ManualState::default();
        open(&mut state, 3, Some("abrir"));
        assert_eq!(state.chapter(), 3);
        assert_eq!(state.place().anchor.as_deref(), Some("abrir"));
        open(&mut state, 3, Some("nao-existe"));
        assert_eq!(state.place().anchor, None);
    }

    #[test]
    fn back_forward_and_new_navigation_drop_the_forward_branch() {
        let mut state = ManualState::default();
        let mut fx = Vec::new();
        open(&mut state, 3, None);
        run(
            &mut state,
            ManualMessage::Link("04-buscas.md".into()),
            &mut fx,
        );
        assert_eq!(state.chapter(), 4);
        run(&mut state, ManualMessage::Back, &mut fx);
        assert_eq!(state.chapter(), 3);
        assert!(state.can_forward());
        run(&mut state, ManualMessage::Forward, &mut fx);
        assert_eq!(state.chapter(), 4);
        run(&mut state, ManualMessage::Back, &mut fx);
        run(&mut state, ManualMessage::Home, &mut fx);
        assert_eq!(state.chapter(), 0);
        assert!(
            !state.can_forward(),
            "navigating after Back drops the branch"
        );
    }

    #[test]
    fn history_keeps_at_most_one_hundred_entries() {
        let mut state = ManualState::default();
        let mut fx = Vec::new();
        open(&mut state, 3, None);
        for i in 0..250 {
            let uri = if i % 2 == 0 {
                "04-buscas.md"
            } else {
                "03-bancos-de-dados.md"
            };
            run(&mut state, ManualMessage::Link(uri.into()), &mut fx);
        }
        assert_eq!(state.history.len(), HISTORY_LIMIT);
        assert_eq!(state.cursor, HISTORY_LIMIT - 1);
    }

    #[test]
    fn unknown_link_sets_a_notice_and_does_not_move() {
        let mut state = ManualState::default();
        let mut fx = Vec::new();
        open(&mut state, 3, None);
        let serial = state.nav_serial();
        run(
            &mut state,
            ManualMessage::Link("#nao-existe".into()),
            &mut fx,
        );
        assert_eq!(state.notice(), Some(ManualNotice::LinkNotFound));
        run(
            &mut state,
            ManualMessage::Link("javascript:x".into()),
            &mut fx,
        );
        assert_eq!(state.notice(), Some(ManualNotice::LinkIgnored));
        assert_eq!(state.nav_serial(), serial);
        assert!(fx.is_empty());
    }

    #[test]
    fn external_link_asks_once_then_emits_the_effect() {
        let mut state = ManualState::default();
        let mut fx = Vec::new();
        open(&mut state, 3, None);
        run(
            &mut state,
            ManualMessage::Link("https://lichess.org".into()),
            &mut fx,
        );
        assert_eq!(state.pending_external(), Some("https://lichess.org"));
        assert!(fx.is_empty(), "nothing opens before the answer");
        run(&mut state, ManualMessage::ConfirmExternal(true), &mut fx);
        assert_eq!(fx, [Effect::OpenUrl("https://lichess.org".into())]);
        run(
            &mut state,
            ManualMessage::Link("https://lichess.org".into()),
            &mut fx,
        );
        assert_eq!(fx.len(), 2, "asked only once per session");
        assert_eq!(state.pending_external(), None);
    }

    #[test]
    fn declining_the_external_link_opens_nothing() {
        let mut state = ManualState::default();
        let mut fx = Vec::new();
        open(&mut state, 3, None);
        run(
            &mut state,
            ManualMessage::Link("https://lichess.org".into()),
            &mut fx,
        );
        run(&mut state, ManualMessage::ConfirmExternal(false), &mut fx);
        assert!(fx.is_empty());
        assert_eq!(state.pending_external(), None);
    }

    #[test]
    fn search_finds_headings_and_a_result_navigates() {
        let mut state = ManualState::default();
        let mut fx = Vec::new();
        open(&mut state, 0, None);
        run(&mut state, ManualMessage::Query("clipbase".into()), &mut fx);
        assert_eq!(state.results().len(), 1);
        run(&mut state, ManualMessage::PickResult(0), &mut fx);
        assert_eq!(state.chapter(), 3);
        assert_eq!(
            state.place().anchor.as_deref(),
            Some("os-slots-e-a-clipbase")
        );
        run(&mut state, ManualMessage::Query(String::new()), &mut fx);
        assert!(state.results().is_empty());
    }

    #[test]
    fn remembered_scroll_returns_with_back() {
        let mut state = ManualState::default();
        let mut fx = Vec::new();
        open(&mut state, 3, None);
        run(&mut state, ManualMessage::RememberScroll(420.0), &mut fx);
        run(
            &mut state,
            ManualMessage::Link("04-buscas.md".into()),
            &mut fx,
        );
        run(&mut state, ManualMessage::Back, &mut fx);
        assert!((state.place().scroll - 420.0).abs() < f32::EPSILON);
    }

    #[test]
    fn reopening_a_closed_window_starts_a_new_trail() {
        let mut state = ManualState::default();
        let mut fx = Vec::new();
        open(&mut state, 3, None);
        run(&mut state, ManualMessage::Close, &mut fx);
        assert!(!state.is_open());
        open(&mut state, 5, None);
        assert_eq!(state.history.len(), 1, "the old trail is gone");
        assert!(!state.can_back());
        assert_eq!(state.chapter(), 5);
    }
}
