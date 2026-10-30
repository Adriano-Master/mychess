//! How the document is turned into styled text: spans, header lines and the
//! text of the annotation symbols (spec M2-4, D165, D167, D174, D176).

use std::borrow::Cow;

use iced::advanced::text::Span;
use iced::{Color, Font};
use mychess_app::{Block, BlockKind, HeaderLine, Inline, nag_text};
use mychess_core::{GameResult, Nag};

use super::figurine::{Part, san_parts};
use super::hit::Hit;
use crate::fonts;
use crate::theme::notation_colors::NotationPalette;
use crate::widgets::glyph::GLYPH_FONT;

/// Space left around the text, in logical pixels.
pub const PADDING: f32 = 8.0;

/// Indentation per nesting level, in em (spec D163).
pub const INDENT_EM: f32 = 2.0;

/// The text style the shaped paragraphs depend on. When any field changes
/// the paragraphs are shaped again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Font size in logical pixels.
    pub size: f32,
    /// Line height as a multiple of the size.
    pub line_height: f32,
    /// The colors.
    pub palette: NotationPalette,
    /// NAGs as symbols (`!`) or as `$n`.
    pub symbolic_nags: bool,
    /// Draw the main line in bold.
    pub bold_main: bool,
    /// Draw the piece letters of a move as figurines (spec M2-5, D243).
    pub figurines: bool,
}

impl TextStyle {
    /// The same style with the figurines on or off.
    #[must_use]
    pub fn with_figurines(mut self, on: bool) -> TextStyle {
        self.figurines = on;
        self
    }

    /// The width of one nesting level.
    pub fn indent_step(&self) -> f32 {
        self.size * INDENT_EM
    }

    /// The height of a line in pixels.
    pub fn line_height_px(&self) -> f32 {
        self.size * self.line_height
    }
}

/// The few words of the document that come from the translations.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Labels {
    /// "Round" (before the round number in the short header).
    pub round: String,
    /// "Position" (before the FEN of the short header).
    pub position: String,
    /// "No moves yet".
    pub empty: String,
}

/// A span with the toolkit's default link type.
pub type NotationSpan = Span<'static, (), Font>;

/// Whether the embedded fonts have a glyph for `c`. ASCII is in Noto Sans;
/// the other characters are the ones the NAG symbols use, covered by Noto
/// Sans, Noto Sans Math and Noto Sans Symbols 2 (a test checks the table
/// against the `cmap` of the fonts).
pub fn covered(c: char) -> bool {
    c.is_ascii() || NON_ASCII_GLYPHS.contains(&c)
}

/// The non-ASCII characters of the symbols of `Nag::glyph()` that the
/// embedded fonts draw. Four symbols of the Scid's table are missing from
/// all of them (`⌓` of `$142` and `⚨ ⚮ ⚯` of `$192`, `$193`, `$212`); those
/// annotations are shown as `$n`.
pub(super) const NON_ASCII_GLYPHS: &[char] = &[
    '×', 'ǁ', '—', '←', '↑', '→', '↓', '↖', '↗', '↹', '↺', '↻', '⇄', '⇆', '⇈', '⇊', '⇑', '⇓', '⇗',
    '⊕', '⊖', '⊙', '⊞', '⊥', '⊶', '⋘', '⋙', '⌊', '⌋', '□', '△', '◨', '◫', '◯', '⟨', '⟩', '⟪', '⟫',
    '⟺', '⡁',
];

/// The text shown for a NAG: its symbol when `symbolic` and the embedded
/// fonts can draw it, else `$n` (D176).
pub fn nag_label(nag: Nag, symbolic: bool) -> Cow<'static, str> {
    nag_label_with(nag, symbolic, covered)
}

/// [`nag_label`] with the glyph coverage injected.
pub(super) fn nag_label_with(
    nag: Nag,
    symbolic: bool,
    has_glyph: impl Fn(char) -> bool,
) -> Cow<'static, str> {
    let text = nag_text(nag, symbolic);
    if symbolic && !text.chars().all(has_glyph) {
        Cow::Owned(nag.to_string())
    } else {
        text
    }
}

/// Collects consecutive plain text of one color into a single span, so that
/// a paragraph has a handful of spans per move instead of one per blank.
struct Builder<'a> {
    style: &'a TextStyle,
    spans: Vec<NotationSpan>,
    hits: Vec<Option<Hit>>,
    plain: Option<(String, Color, bool)>,
}

impl Builder<'_> {
    fn flush(&mut self) {
        if let Some((text, color, bold)) = self.plain.take() {
            self.spans.push(styled(text, color, bold));
            self.hits.push(None);
        }
    }

    fn plain(&mut self, text: &str, color: Color, bold: bool) {
        match &mut self.plain {
            Some((buffer, c, b)) if *c == color && *b == bold => buffer.push_str(text),
            _ => {
                self.flush();
                self.plain = Some((text.to_owned(), color, bold));
            }
        }
    }

    fn clickable(&mut self, text: String, color: Color, bold: bool, hit: Hit) {
        self.flush();
        self.spans.push(styled(text, color, bold));
        self.hits.push(Some(hit));
    }

    /// A move: one span, or (with figurines) one per glyph and text run, all
    /// of them standing for the same `hit`.
    fn move_text(&mut self, san: &str, color: Color, bold: bool, hit: Hit) {
        if !self.style.figurines {
            self.clickable(san.to_owned(), color, bold, hit);
            return;
        }
        for part in san_parts(san, true) {
            match part {
                Part::Glyph(_) => {
                    self.flush();
                    self.spans
                        .push(Span::new(part.text()).color(color).font(GLYPH_FONT));
                    self.hits.push(Some(hit));
                }
                Part::Text(text) => self.clickable(text, color, bold, hit),
            }
        }
    }
}

fn styled(text: String, color: Color, bold: bool) -> NotationSpan {
    let span = Span::new(text).color(color);
    if bold {
        span.font(fonts::NOTO_SANS_BOLD)
    } else {
        span
    }
}

/// The spans of a block and, for each, what a click on it means.
pub fn block_spans(block: &Block, style: &TextStyle) -> (Vec<NotationSpan>, Vec<Option<Hit>>) {
    block_spans_with(block, style, false)
}

/// [`block_spans`], optionally without the outer parentheses of a variation
/// block (`framed`): a table card has its own frame (spec M2-5, D248).
pub fn block_spans_with(
    block: &Block,
    style: &TextStyle,
    framed: bool,
) -> (Vec<NotationSpan>, Vec<Option<Hit>>) {
    let palette = &style.palette;
    let skip_first = framed && matches!(block.inlines.first(), Some(Inline::Open));
    let skip_last = framed && matches!(block.inlines.last(), Some(Inline::Close));
    let last = block.inlines.len().saturating_sub(1);
    let mut b = Builder {
        style,
        spans: Vec::new(),
        hits: Vec::new(),
        plain: None,
    };
    // Variation blocks start inside a variation; the parentheses of an
    // inline layout nest further.
    let mut depth = u32::from(block.kind == BlockKind::Variation);
    let color_at = |depth: u32| {
        if depth > 0 {
            palette.variation
        } else {
            palette.main
        }
    };
    for (index, inline) in block.inlines.iter().enumerate() {
        if (skip_first && index == 0) || (skip_last && index == last) {
            continue;
        }
        match inline {
            Inline::Number(text) => {
                let bold = style.bold_main && depth == 0;
                b.plain(text, color_at(depth), bold);
            }
            Inline::Space => b.plain(" ", color_at(depth), false),
            Inline::Open => {
                depth += 1;
                b.plain("(", color_at(depth), false);
            }
            Inline::Close => {
                b.plain(")", color_at(depth), false);
                depth = depth.saturating_sub(1);
            }
            Inline::Pending { .. } => b.plain("( ... )", palette.variation, false),
            Inline::Move {
                node,
                san,
                in_variation,
                ..
            } => {
                let inside = *in_variation || depth > 0;
                let color = if inside {
                    palette.variation
                } else {
                    palette.main
                };
                let bold = style.bold_main && !inside;
                b.move_text(san, color, bold, Hit::Move(*node));
            }
            Inline::Nag { node, nag } => {
                b.clickable(
                    nag_label(*nag, b.style.symbolic_nags).into_owned(),
                    palette.nag_color(*nag),
                    false,
                    Hit::Nag(*node),
                );
            }
            Inline::Comment { target, text } => {
                b.clickable(
                    text.to_string(),
                    palette.comment,
                    false,
                    Hit::Comment(*target),
                );
            }
            // `Inline` is non-exhaustive: a newer application may add items;
            // they are skipped rather than guessed.
            _ => {}
        }
    }
    b.flush();
    (b.spans, b.hits)
}

fn rating_suffix(rating: Option<u16>) -> String {
    rating.map_or_else(String::new, |r| format!(" ({r})"))
}

/// The text of a header line (D174).
pub fn header_text(line: &HeaderLine, labels: &Labels) -> String {
    match line {
        HeaderLine::Tag { name, value } => format!("[{name} \"{value}\"]"),
        HeaderLine::Players {
            white,
            white_rating,
            black,
            black_rating,
        } => format!(
            "{white}{}  --  {black}{}",
            rating_suffix(*white_rating),
            rating_suffix(*black_rating)
        ),
        HeaderLine::Event {
            event,
            round,
            site,
            date,
            result,
            eco,
        } => {
            let mut parts = vec![event.clone()];
            if let Some(round) = round {
                parts.push(format!("({} {round})", labels.round));
            }
            parts.push(site.clone());
            parts.push(date.clone());
            parts.push(result.as_str().to_owned());
            if let Some(eco) = eco {
                parts.push(eco.to_string());
            }
            parts.join("  ")
        }
        HeaderLine::Position { fen } => format!("{}: {fen}", labels.position),
        // A newer application may add lines; nothing is shown for them.
        _ => String::new(),
    }
}

/// The spans of the header: one line per header line, in the header color
/// (the players line in bold).
pub fn header_spans(lines: &[HeaderLine], labels: &Labels, style: &TextStyle) -> Vec<NotationSpan> {
    let mut spans = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        let mut text = header_text(line, labels);
        if index + 1 < lines.len() {
            text.push('\n');
        }
        let bold = matches!(line, HeaderLine::Players { .. });
        spans.push(styled(text, style.palette.header, bold));
    }
    spans
}

/// The span of the result at the end of the game.
pub fn result_span(result: GameResult, style: &TextStyle) -> NotationSpan {
    styled(result.as_str().to_owned(), style.palette.header, false)
}

/// The span shown for a game without moves.
pub fn empty_span(labels: &Labels, style: &TextStyle) -> NotationSpan {
    styled(labels.empty.clone(), style.palette.variation, false)
}

#[cfg(test)]
mod tests {
    use mychess_app::{NotationOptions, build_notation};
    use mychess_core::{Game, Position};
    use pretty_assertions::assert_eq;

    use super::*;

    fn style() -> TextStyle {
        TextStyle {
            size: 14.0,
            line_height: 1.3,
            palette: NotationPalette::of(&iced::Theme::Light),
            symbolic_nags: true,
            bold_main: false,
            figurines: false,
        }
    }

    fn game_with_variation() -> Game {
        let mut game = Game::new();
        let start = Position::new();
        let e4 = game
            .add_move(game.root(), start.parse_uci("e2e4").expect("uci"))
            .expect("e4");
        let after = game.position_at(e4).expect("position").clone();
        game.add_move(e4, after.parse_uci("e7e5").expect("uci"))
            .expect("e5");
        game.add_move(e4, after.parse_uci("c7c5").expect("uci"))
            .expect("c5");
        game.add_nag(e4, Nag::new(1).expect("nag")).expect("nag");
        game.set_comment(e4, "best by test").expect("comment");
        game
    }

    fn texts(spans: &[NotationSpan]) -> Vec<String> {
        spans.iter().map(|s| s.text.to_string()).collect()
    }

    #[test]
    fn spans_follow_the_inlines_and_only_moves_nags_and_comments_are_hits() {
        let game = game_with_variation();
        let opts = NotationOptions {
            layout: mychess_app::NotationLayout::Inline,
            ..NotationOptions::default()
        };
        let doc = build_notation(&game, &opts, 1);
        let block = doc.blocks.first().expect("one block");
        let (spans, hits) = block_spans(block, &style());
        assert_eq!(spans.len(), hits.len());
        let joined: String = texts(&spans).concat();
        assert_eq!(joined, "1.e4! best by test 1...e5 (1...c5)");
        let clickable: Vec<_> = hits.iter().flatten().collect();
        assert!(matches!(clickable[0], Hit::Move(_)));
        assert!(matches!(clickable[1], Hit::Nag(_)));
        assert!(matches!(clickable[2], Hit::Comment(_)));
        assert!(clickable[3..].iter().all(|h| matches!(h, Hit::Move(_))));
    }

    #[test]
    fn plain_runs_of_one_color_share_a_span() {
        let game = game_with_variation();
        let opts = NotationOptions {
            layout: mychess_app::NotationLayout::Inline,
            ..NotationOptions::default()
        };
        let doc = build_notation(&game, &opts, 1);
        let block = doc.blocks.first().expect("block");
        let (spans, _) = block_spans(block, &style());
        assert!(
            spans.len() < block.inlines.len(),
            "{} spans for {} inlines",
            spans.len(),
            block.inlines.len()
        );
    }

    #[test]
    fn variation_text_uses_the_variation_color_and_main_line_the_main_one() {
        let game = game_with_variation();
        let opts = NotationOptions {
            layout: mychess_app::NotationLayout::Inline,
            ..NotationOptions::default()
        };
        let doc = build_notation(&game, &opts, 1);
        let st = style();
        let (spans, hits) = block_spans(doc.blocks.first().expect("block"), &st);
        let colors: Vec<_> = spans
            .iter()
            .zip(&hits)
            .filter(|(_, h)| matches!(h, Some(Hit::Move(_))))
            .map(|(s, _)| s.color)
            .collect();
        assert_eq!(colors[0], Some(st.palette.main), "e4");
        assert_eq!(colors[1], Some(st.palette.main), "e5");
        assert_eq!(colors[2], Some(st.palette.variation), "c5");
    }

    #[test]
    fn bold_main_line_only_touches_the_main_line() {
        let game = game_with_variation();
        let opts = NotationOptions {
            layout: mychess_app::NotationLayout::Inline,
            ..NotationOptions::default()
        };
        let doc = build_notation(&game, &opts, 1);
        let st = TextStyle {
            bold_main: true,
            ..style()
        };
        let (spans, hits) = block_spans(doc.blocks.first().expect("block"), &st);
        let bold: Vec<_> = spans
            .iter()
            .zip(&hits)
            .filter(|(_, h)| matches!(h, Some(Hit::Move(_))))
            .map(|(s, _)| s.font.is_some())
            .collect();
        assert_eq!(bold, [true, true, false]);
    }

    #[test]
    fn indented_variation_blocks_start_in_the_variation_color() {
        let game = game_with_variation();
        let doc = build_notation(&game, &NotationOptions::default(), 1);
        let st = style();
        let variation = doc
            .blocks
            .iter()
            .find(|b| b.kind == BlockKind::Variation)
            .expect("variation block");
        let (spans, _) = block_spans(variation, &st);
        assert!(spans.iter().all(|s| s.color == Some(st.palette.variation)));
    }

    #[test]
    fn nags_fall_back_to_dollar_numbers_without_symbols_or_without_glyphs() {
        let nag = |n| Nag::new(n).expect("nag");
        assert_eq!(nag_label(nag(1), true), "!");
        assert_eq!(nag_label(nag(1), false), "$1");
        assert_eq!(nag_label(nag(22), true), "⊙");
        // A glyph the fonts cannot draw becomes `$n`.
        assert_eq!(nag_label_with(nag(22), true, |c| c.is_ascii()), "$22");
        assert_eq!(nag_label_with(nag(18), true, |c| c.is_ascii()), "+-");
        // A NAG without a symbol is `$n` either way.
        assert_eq!(nag_label(nag(250), true), "$250");
    }

    #[test]
    fn header_lines_follow_d174() {
        let labels = Labels {
            round: "Round".to_owned(),
            position: "Position".to_owned(),
            empty: String::new(),
        };
        let tag = HeaderLine::Tag {
            name: "Event".to_owned(),
            value: "Test".to_owned(),
        };
        assert_eq!(header_text(&tag, &labels), "[Event \"Test\"]");
        let players = HeaderLine::Players {
            white: "A".to_owned(),
            white_rating: Some(2700),
            black: "B".to_owned(),
            black_rating: None,
        };
        assert_eq!(header_text(&players, &labels), "A (2700)  --  B");
        let event = HeaderLine::Event {
            event: "Ev".to_owned(),
            round: Some("3".to_owned()),
            site: "Here".to_owned(),
            date: "2024.05".to_owned(),
            result: GameResult::Draw,
            eco: None,
        };
        assert_eq!(
            header_text(&event, &labels),
            "Ev  (Round 3)  Here  2024.05  1/2-1/2"
        );
        let position = HeaderLine::Position {
            fen: "8/8/8/8/8/8/8/K6k w - - 0 1".to_owned(),
        };
        assert_eq!(
            header_text(&position, &labels),
            "Position: 8/8/8/8/8/8/8/K6k w - - 0 1"
        );
    }

    /// Reads the code points a TrueType font has glyphs for (`cmap` formats
    /// 4 and 12), so the coverage table is checked against the real fonts.
    fn cmap_coverage(font: &[u8]) -> std::collections::BTreeSet<u32> {
        fn u16_at(d: &[u8], at: usize) -> usize {
            usize::from(u16::from_be_bytes([d[at], d[at + 1]]))
        }
        fn u32_at(d: &[u8], at: usize) -> usize {
            u32::from_be_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]) as usize
        }
        let tables = u16_at(font, 4);
        let cmap = (0..tables)
            .map(|i| 12 + 16 * i)
            .find(|&at| &font[at..at + 4] == b"cmap")
            .map(|at| u32_at(font, at + 8))
            .expect("cmap table");
        let mut out = std::collections::BTreeSet::new();
        for i in 0..u16_at(font, cmap + 2) {
            let sub = cmap + u32_at(font, cmap + 4 + 8 * i + 4);
            match u16_at(font, sub) {
                4 => {
                    let n = u16_at(font, sub + 6) / 2;
                    for s in 0..n {
                        let end = u16_at(font, sub + 14 + 2 * s);
                        let start = u16_at(font, sub + 16 + 2 * n + 2 * s);
                        for c in start..=end {
                            if c != 0xFFFF {
                                out.insert(u32::try_from(c).expect("u16"));
                            }
                        }
                    }
                }
                12 => {
                    for g in 0..u32_at(font, sub + 12) {
                        let at = sub + 16 + 12 * g;
                        let (start, end) = (u32_at(font, at), u32_at(font, at + 4));
                        for c in start..=end {
                            out.insert(u32::try_from(c).expect("code point"));
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    #[test]
    fn the_embedded_fonts_draw_every_symbol_of_the_table() {
        let mut union = std::collections::BTreeSet::new();
        for font in [
            crate::assets::NOTO_SANS_REGULAR,
            crate::assets::NOTO_SANS_MATH,
            crate::assets::NOTO_SANS_SYMBOLS2,
        ] {
            union.extend(cmap_coverage(font));
        }
        // The figurines (D244) and the fold chevrons of the variation cards
        // come from the same fonts.
        let chevrons = ['\u{25B8}', '\u{25BE}'];
        for &c in NON_ASCII_GLYPHS
            .iter()
            .chain(&crate::widgets::glyph::GLYPHS)
            .chain(&chevrons)
        {
            assert!(
                union.contains(&(c as u32)),
                "{c} (U+{:04X}) not drawn",
                c as u32
            );
        }
    }

    #[test]
    fn only_four_rare_symbols_fall_back_to_dollar_numbers() {
        let mut fallback = Vec::new();
        for n in 1..=255u16 {
            let nag = Nag::new(n).expect("nag");
            let glyph = nag.glyph();
            if glyph.chars().all(covered) {
                assert_eq!(nag_label(nag, true), glyph, "NAG {n}");
            } else {
                fallback.push(n);
                assert_eq!(nag_label(nag, true), nag.to_string(), "NAG {n}");
            }
        }
        assert_eq!(fallback, [142, 192, 193, 212]);
    }

    fn indented_with(layout: mychess_app::NotationLayout, game: &Game) -> mychess_app::NotationDoc {
        let opts = NotationOptions {
            layout,
            ..NotationOptions::default()
        };
        build_notation(game, &opts, 1)
    }

    #[test]
    fn figurines_split_a_piece_move_into_spans_of_one_hit() {
        let mut game = Game::new();
        let start = Position::new();
        let nf3 = game
            .add_move(game.root(), start.parse_uci("g1f3").expect("uci"))
            .expect("nf3");
        let doc = indented_with(mychess_app::NotationLayout::Inline, &game);
        let st = style().with_figurines(true);
        let (spans, hits) = block_spans(doc.blocks.first().expect("block"), &st);
        let moves: Vec<_> = spans
            .iter()
            .zip(&hits)
            .filter(|(_, h)| **h == Some(Hit::Move(nf3)))
            .map(|(s, _)| (s.text.to_string(), s.font.is_some()))
            .collect();
        assert_eq!(
            moves,
            [("\u{265E}".to_owned(), true), ("f3".to_owned(), false)]
        );
        // Without figurines it is one span of English SAN.
        let (plain, _) = block_spans(doc.blocks.first().expect("block"), &style());
        assert!(plain.iter().any(|s| s.text == "Nf3"));
    }

    #[test]
    fn a_framed_card_drops_only_the_outer_parentheses() {
        let game = game_with_variation();
        let doc = indented_with(mychess_app::NotationLayout::Indented, &game);
        let variation = doc
            .blocks
            .iter()
            .find(|b| b.kind == BlockKind::Variation)
            .expect("variation block");
        let st = style();
        let joined = |framed| -> String {
            block_spans_with(variation, &st, framed)
                .0
                .iter()
                .map(|s| s.text.to_string())
                .collect()
        };
        assert_eq!(joined(false), "(1...c5)");
        assert_eq!(joined(true), "1...c5");
    }

    #[test]
    fn nags_are_colored_by_tone_in_the_text() {
        let game = game_with_variation();
        let doc = indented_with(mychess_app::NotationLayout::Inline, &game);
        let st = style();
        let (spans, hits) = block_spans(doc.blocks.first().expect("block"), &st);
        let nag = spans
            .iter()
            .zip(&hits)
            .find(|(_, h)| matches!(h, Some(Hit::Nag(_))))
            .map(|(s, _)| s.color)
            .expect("the nag span");
        assert_eq!(nag, Some(st.palette.tones.good));
    }
}
