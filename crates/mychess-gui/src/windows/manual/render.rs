//! The chapter parsed once, and the viewer that gives the headings their
//! ids (spec M9-5, sections 4.4 and 5.4).

use iced::widget::markdown::{self, HeadingLevel, Item, Settings, Style, Viewer};
use iced::widget::{Id, container};
use iced::{Element, Font, Theme};
use mychess_app::manual::ManualIndex;

use crate::i18n::Language;
use crate::shell::{Message, Shell};
use crate::windows::manual::ManualUiMessage;

/// The sizes of the text, one per step of the preference (D4060). Step
/// `DEFAULT_TEXT_SCALE` is the size of the interface.
const TEXT_SIZES: [f32; 7] = [12.0, 13.5, 15.0, 17.0, 19.0, 22.0, 26.0];

/// The base size of the text for a step of the preference.
#[must_use]
pub fn text_size(step: u8) -> f32 {
    TEXT_SIZES
        .get(usize::from(step))
        .copied()
        .unwrap_or(TEXT_SIZES[2])
}

/// The id of the container of a heading.
#[must_use]
pub fn heading_id(slug: &str) -> String {
    format!("manual-h-{slug}")
}

/// A chapter parsed by the markdown widget.
#[derive(Debug)]
pub struct Rendered {
    /// The chapter.
    pub chapter: usize,
    /// The language of the interface it was made for.
    pub language: Language,
    content: markdown::Content,
    ids: Vec<(usize, Id)>,
    slugs: Vec<String>,
}

impl Rendered {
    /// Parses a chapter. The `i`-th heading of the text is the `i`-th
    /// `Item::Heading` of the parsed content; the slugs come from the index.
    #[must_use]
    pub fn build(text: &str, index: &ManualIndex, chapter: usize, language: Language) -> Rendered {
        let content = markdown::Content::parse(text);
        let slugs: Vec<String> = index
            .headings(chapter)
            .iter()
            .map(|h| h.slug.clone())
            .collect();
        let ids = content
            .items()
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, Item::Heading(..)))
            .zip(&slugs)
            .map(|((position, _), slug)| (position, Id::from(heading_id(slug))))
            .collect();
        Rendered {
            chapter,
            language,
            content,
            ids,
            slugs,
        }
    }

    /// How many items the parser produced.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.content.items().len()
    }

    /// How many items are headings.
    #[must_use]
    pub fn heading_count(&self) -> usize {
        self.content
            .items()
            .iter()
            .filter(|item| matches!(item, Item::Heading(..)))
            .count()
    }

    /// The slugs of the headings, in order.
    #[must_use]
    pub fn slugs(&self) -> &[String] {
        &self.slugs
    }

    /// Whether the parser produced a table or an image (the manual has
    /// none, D4050).
    #[must_use]
    pub fn has_table_or_image(&self) -> bool {
        self.content
            .items()
            .iter()
            .any(|item| matches!(item, Item::Table { .. } | Item::Image { .. }))
    }

    /// The level of the first item, if it is a heading.
    #[must_use]
    pub fn first_heading_level(&self) -> Option<HeadingLevel> {
        match self.content.items().first() {
            Some(Item::Heading(level, _)) => Some(*level),
            _ => None,
        }
    }

    /// The text of the chapter, drawn with the style of the theme.
    pub fn view<'a>(&'a self, theme: &Theme, step: u8) -> Element<'a, Message> {
        let viewer = ChapterViewer { ids: &self.ids };
        markdown::view_with(self.content.items(), settings(theme, step), &viewer)
    }
}

/// The settings of the markdown widget, with the colors of the theme.
fn settings(theme: &Theme, step: u8) -> Settings {
    let palette = theme.extended_palette();
    let mut style = Style::from_palette(theme.palette());
    style.inline_code_highlight.background = palette.background.weak.color.into();
    style.inline_code_color = palette.background.weak.text;
    style.link_color = palette.primary.base.color;
    style.inline_code_font = Font::MONOSPACE;
    style.code_block_font = Font::MONOSPACE;
    Settings::with_text_size(text_size(step), style)
}

/// The message the viewer produces when a link of the text is clicked.
#[must_use]
pub fn link_message(uri: String) -> Message {
    Message::ManualUi(ManualUiMessage::Nav(
        mychess_app::manual::ManualMessage::Link(uri),
    ))
}

/// Gives the headings an id the shell can find.
struct ChapterViewer<'a> {
    ids: &'a [(usize, Id)],
}

impl<'a> Viewer<'a, Message> for ChapterViewer<'a> {
    fn on_link_click(url: markdown::Uri) -> Message {
        link_message(url)
    }

    fn heading(
        &self,
        settings: Settings,
        level: &'a HeadingLevel,
        text: &'a markdown::Text,
        index: usize,
    ) -> Element<'a, Message> {
        let body = markdown::heading(settings, level, text, index, Self::on_link_click);
        match self.ids.iter().find(|(position, _)| *position == index) {
            Some((_, id)) => container(body).id(id.clone()).into(),
            None => body,
        }
    }
}

/// The text of the chapter of the shell, centered and limited in width.
pub(super) fn chapter_text<'a>(shell: &'a Shell, rendered: &'a Rendered) -> Element<'a, Message> {
    let step = shell.app.prefs.manual.text_scale;
    rendered.view(&shell.theme(), step)
}
