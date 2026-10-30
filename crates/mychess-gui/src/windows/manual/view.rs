//! The layer of the manual window (spec M9-5, section 4.2).

use fluent_bundle::FluentArgs;
use iced::widget::{
    button, column, container, opaque, row, scrollable, space, stack, text, text_input, tooltip,
};
use iced::{Alignment, Element, Length};
use mychess_app::manual::{MAX_TEXT_SCALE, ManualIndex, ManualMessage};

use super::render::chapter_text;
use super::{
    MANUAL_FIND_ID, MANUAL_SCROLL_ID, MANUAL_TEXT_ID, MANUAL_WINDOW_ID, ManualUi, ManualUiMessage,
    chapter_entry_id, result_entry_id,
};
use crate::shell::{Message, Shell};
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::windows::panel_kit::{BODY_TEXT, action, muted};

/// The width of the table of contents.
const SIDEBAR_WIDTH: f32 = 250.0;
/// The widest the text goes, for comfortable reading.
const TEXT_WIDTH: f32 = 720.0;

fn nav(message: ManualMessage) -> Message {
    Message::ManualUi(ManualUiMessage::Nav(message))
}

fn plain(message: ManualMessage) -> Message {
    Message::ManualUi(ManualUiMessage::Plain(message))
}

/// A full-width entry of the table of contents or of the results; it is
/// filled with the accent while `active`.
fn entry<'a>(label: String, active: bool, message: Message) -> Element<'a, Message> {
    button(text(label).size(BODY_TEXT))
        .padding([3, 10])
        .width(Length::Fill)
        .style(theme::command_button(ButtonLook::Secondary, active))
        .on_press(message)
        .into()
}

/// An icon button with a tooltip; without a message it is disabled.
fn icon_button<'a>(
    shell: &'a Shell,
    glyph: Icon,
    tip_key: &str,
    message: Option<Message>,
    id: &'static str,
) -> Element<'a, Message> {
    let tone = if message.is_some() {
        IconTone::Text
    } else {
        IconTone::Disabled
    };
    let body = button(icon(glyph, 16.0, tone))
        .padding([4, 8])
        .style(theme::command_button(ButtonLook::Secondary, false))
        .on_press_maybe(message);
    let tip = container(text(shell.i18n.tr(tip_key)).size(12))
        .padding([4, 8])
        .style(theme::tooltip_box);
    container(tooltip(body, tip, tooltip::Position::Bottom))
        .id(id)
        .into()
}

/// The top bar: Back, Forward, Home, the title, the size of the text and
/// the close button.
fn top_bar(shell: &Shell, title: String) -> Element<'_, Message> {
    let i18n = &shell.i18n;
    let state = shell.app.manual();
    let step = shell.app.prefs.manual.text_scale;
    let smaller = (step > 0).then(|| plain(ManualMessage::Scale(-1)));
    let larger = (step < MAX_TEXT_SCALE).then(|| plain(ManualMessage::Scale(1)));
    row![
        icon_button(
            shell,
            Icon::Back,
            "manual-back-tip",
            state.can_back().then(|| nav(ManualMessage::Back)),
            "manual-back",
        ),
        icon_button(
            shell,
            Icon::Forward,
            "manual-forward-tip",
            state.can_forward().then(|| nav(ManualMessage::Forward)),
            "manual-forward",
        ),
        container(action(
            i18n.tr("manual-home"),
            Some(nav(ManualMessage::Home))
        ))
        .id("manual-home"),
        text(title).size(16).style(theme::accent_text),
        space::horizontal(),
        container(action(i18n.tr("manual-text-smaller"), smaller)).id("manual-smaller"),
        container(action(i18n.tr("manual-text-larger"), larger)).id("manual-larger"),
        container(action(
            i18n.tr("manual-close"),
            Some(plain(ManualMessage::Close))
        ))
        .id("manual-close"),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// The search field and, below it, the results or the table of contents.
fn sidebar<'a>(shell: &'a Shell, index: &'a ManualIndex) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let state = shell.app.manual();
    let find = text_input(&i18n.tr("manual-search-placeholder"), state.query())
        .id(MANUAL_FIND_ID)
        .on_input(|t| plain(ManualMessage::Query(t)))
        .padding(5)
        .size(BODY_TEXT)
        .width(Length::Fill);
    let mut list = column![].spacing(2);
    if state.query().trim().is_empty() {
        let current = state.chapter();
        let place = state.place();
        for chapter in 0..index.len() {
            let title = index.title(chapter).unwrap_or_default().to_owned();
            list = list.push(
                container(entry(
                    title,
                    chapter == current,
                    nav(ManualMessage::Goto {
                        chapter,
                        anchor: None,
                    }),
                ))
                .id(chapter_entry_id(chapter))
                .width(Length::Fill),
            );
            if chapter == current {
                for heading in index.headings(chapter).iter().filter(|h| h.level == 2) {
                    let active = place.anchor.as_deref() == Some(heading.slug.as_str());
                    list = list.push(
                        row![
                            space::horizontal().width(14),
                            entry(
                                heading.title.clone(),
                                active,
                                nav(ManualMessage::Goto {
                                    chapter,
                                    anchor: Some(heading.slug.clone()),
                                }),
                            ),
                        ]
                        .width(Length::Fill),
                    );
                }
            }
        }
    } else if state.results().is_empty() {
        list = list.push(muted(i18n.tr("manual-search-empty")));
    } else {
        for (i, hit) in state.results().iter().enumerate() {
            let label = if hit.anchor.is_some() {
                format!("{} / {}", hit.chapter_title, hit.title)
            } else {
                hit.title.clone()
            };
            list = list.push(
                container(entry(label, false, plain(ManualMessage::PickResult(i))))
                    .id(result_entry_id(i))
                    .width(Length::Fill),
            );
        }
    }
    column![
        text(i18n.tr("manual-contents"))
            .size(13)
            .style(theme::muted_text),
        find,
        scrollable(list.padding([0, 6])).height(Length::Fill),
    ]
    .spacing(8)
    .width(SIDEBAR_WIDTH)
    .into()
}

/// The remarks above the text: the language, a link that did not work.
fn banners(shell: &Shell) -> Element<'_, Message> {
    let i18n = &shell.i18n;
    let state = shell.app.manual();
    let mut bar = column![].spacing(2);
    if state.is_fallback() {
        bar = bar
            .push(container(muted(i18n.tr("manual-no-translation"))).id("manual-no-translation"));
    }
    if let Some(notice) = state.notice() {
        bar = bar.push(
            container(
                text(i18n.tr(notice.message_key()))
                    .size(12)
                    .style(text::danger),
            )
            .id("manual-notice"),
        );
    }
    bar.into()
}

/// The question "open this address in the browser?".
fn external_question<'a>(shell: &'a Shell, url: &'a str) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut args = FluentArgs::new();
    args.set("url", url);
    let card = container(
        column![
            text(i18n.tr("manual-external-title")).size(16),
            text(i18n.tr_args("manual-external-body", &args)).size(BODY_TEXT),
            row![
                container(action(
                    i18n.tr("manual-external-open"),
                    Some(plain(ManualMessage::ConfirmExternal(true)))
                ))
                .id("manual-external-open"),
                container(action(
                    i18n.tr("manual-external-cancel"),
                    Some(plain(ManualMessage::ConfirmExternal(false)))
                ))
                .id("manual-external-cancel"),
            ]
            .spacing(8),
        ]
        .spacing(10),
    )
    .id("manual-external-box")
    .padding(16)
    .max_width(460)
    .style(theme::pane_body);
    opaque(container(card).center(Length::Fill).style(theme::scrim))
}

/// The layer to put over the window, if the manual is open.
pub fn layer(shell: &Shell) -> Option<Element<'_, Message>> {
    if !ManualUi::layer_open(&shell.app) {
        return None;
    }
    let state = shell.app.manual();
    let index = state.index()?;
    let title = index
        .title(state.chapter())
        .map_or_else(|| shell.i18n.tr("manual-title"), str::to_owned);
    let text_area: Element<'_, Message> = match shell.ui.dialogs.manual.rendered() {
        Some(rendered) if rendered.item_count() > 0 => container(
            container(chapter_text(shell, rendered))
                .id(MANUAL_TEXT_ID)
                .max_width(TEXT_WIDTH)
                .padding([4, 12]),
        )
        .center_x(Length::Fill)
        .into(),
        // A chapter without text (missing from the build): say so.
        _ => container(
            text(shell.i18n.tr("manual-chapter-missing"))
                .size(BODY_TEXT)
                .style(theme::muted_text),
        )
        .id("manual-chapter-missing")
        .padding(16)
        .into(),
    };
    let scroll = scrollable(text_area)
        .id(MANUAL_SCROLL_ID)
        .on_scroll(|viewport| {
            Message::ManualUi(ManualUiMessage::Scrolled {
                offset: viewport.absolute_offset().y,
                height: viewport.bounds().height,
            })
        })
        .width(Length::Fill)
        .height(Length::Fill);
    let main = column![top_bar(shell, title), banners(shell), scroll]
        .spacing(8)
        .width(Length::Fill);
    let card = container(row![sidebar(shell, index), main].spacing(14))
        .id(MANUAL_WINDOW_ID)
        .padding(14)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::pane_body);
    let backdrop = opaque(
        container(card)
            .padding(18)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::scrim),
    );
    let mut layers = stack![backdrop];
    if let Some(url) = state.pending_external() {
        layers = layers.push(external_question(shell, url));
    }
    Some(layers.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;
    use crate::windows::manual::{Rendered, shared_index};

    #[test]
    fn a_chapter_without_text_says_so_in_both_languages() {
        for (language, expected) in [
            (
                Language::EnUs,
                "This chapter of the manual is not available.",
            ),
            (
                Language::PtBr,
                "Este capítulo do manual não está disponível.",
            ),
        ] {
            let dir = tempfile::tempdir().expect("dir");
            let mut shell = Shell::new(Some(dir.path().join("preferences.toml")));
            shell.i18n = crate::i18n::I18n::new(language);
            drop(mychess_app::update(
                &mut shell.app,
                mychess_app::Message::Manual(ManualMessage::Open {
                    index: shared_index(language),
                    chapter: 0,
                    anchor: None,
                }),
            ));
            let rendered =
                Rendered::build("", shell.app.manual().index().expect("index"), 0, language);
            shell.ui.dialogs.manual.rendered = Some(rendered);
            let element = layer(&shell).expect("the layer");
            let mut sim = iced_test::Simulator::with_size(
                iced::Settings::default(),
                iced::Size::new(1000.0, 700.0),
                element,
            );
            assert!(sim.find(expected).is_ok(), "{expected}");
        }
    }
}
