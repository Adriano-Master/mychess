// Behaviour derived from Scid vs PC 4.27, src/game.cpp (Game::WritePGN,
// Game::WriteMoveList in HTML format) and tcl/start.tcl (default HTML start
// and end text). Copyright (c) 1999-2003 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! HTML writer for games (decision D3702 of `docs/specs/m7-4-exportacao.md`).
//!
//! [`write_game_html`] writes one `<article class="game">`; the document
//! around the games is the *start* and *end* text of [`HtmlOptions`]
//! ([`write_html_document`] puts them together). Everything that comes from
//! the game is escaped (`& < > " '`); the only raw text in the output is the
//! start/end text and the SVG that the [`DiagramSource`] returns, both
//! supplied by the host.
//!
//! Layout: the main line is in `<b>`, variations in `<div class="var">`,
//! comments in `<span class="c">`, diagrams in `<figure class="dg">`.

use std::io::Write;

use mychess_core::{Game, Nag, Position, strip_mark_codes};

use super::escape::push_html_escaped;
use super::walk::{Ev, MoveEv, walk};
use crate::write::WriteError;

/// The NAG of a diagram (`D`).
const NAG_DIAGRAM: u8 = 201;

/// Draws the diagrams of a game.
pub trait DiagramSource {
    /// An SVG document (or fragment) for the position `fen`, `px` pixels
    /// wide, seen from Black when `flip` is set. It is copied **as it is**
    /// into the output, so it must be well formed; it should carry a
    /// `<title>` with the FEN.
    fn svg(&mut self, fen: &str, flip: bool, px: u16) -> String;
}

/// A [`DiagramSource`] that draws nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDiagrams;

impl DiagramSource for NoDiagrams {
    fn svg(&mut self, _fen: &str, _flip: bool, _px: u16) -> String {
        String::new()
    }
}

/// Options of [`write_game_html`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlOptions {
    /// Write comments (`[%...]` blocks are always removed).
    pub comments: bool,
    /// Write variations.
    pub variations: bool,
    /// NAGs as symbols (`!`) instead of numbers (`$1`).
    pub symbols: bool,
    /// Width of the diagrams in pixels; `0` turns diagrams off (the NAG `D`
    /// is then written as `(D)`).
    pub diagram_px: u16,
    /// Mark variations with the `ind` class (indented by the stylesheet).
    pub indent_variations: bool,
    /// Text written before the first game by [`write_html_document`].
    pub start: String,
    /// Text written after the last game by [`write_html_document`].
    pub end: String,
}

impl Default for HtmlOptions {
    fn default() -> HtmlOptions {
        HtmlOptions {
            comments: true,
            variations: true,
            symbols: false,
            diagram_px: 200,
            indent_variations: true,
            start: default_html_start().to_owned(),
            end: default_html_end().to_owned(),
        }
    }
}

/// The default start text: an HTML5 head with an embedded stylesheet,
/// ending with `<body>`.
pub fn default_html_start() -> &'static str {
    concat!(
        "<!DOCTYPE html>\n",
        "<html lang=\"en\">\n",
        "<head>\n",
        "<meta charset=\"utf-8\">\n",
        "<title>MyChess export</title>\n",
        "<style>\n",
        "body { font-family: sans-serif; max-width: 52em; margin: 1em auto; }\n",
        "article.game { margin-bottom: 2em; }\n",
        "header.gh { text-align: center; }\n",
        "div.moves { line-height: 1.5; }\n",
        "div.var { margin-left: 1.5em; color: #333; }\n",
        "div.var:not(.ind) { margin-left: 0; display: inline; }\n",
        "span.c { color: #006; font-style: italic; }\n",
        "figure.dg { margin: 0.5em 0; }\n",
        "</style>\n",
        "</head>\n",
        "<body>\n"
    )
}

/// The default end text: closes `<body>` and `<html>` (the markers the
/// append mode of the export looks for).
pub fn default_html_end() -> &'static str {
    "</body>\n</html>\n"
}

fn standard_fen() -> String {
    Position::new().fen()
}

fn push_header(out: &mut String, game: &Game) {
    let tags = game.tags();
    let elo = |r: Option<mychess_core::Rating>| {
        r.filter(|r| r.value.get() > 0)
            .map(|r| format!(" ({})", r.value.get()))
            .unwrap_or_default()
    };
    out.push_str("<header class=\"gh\">\n<h2>");
    push_html_escaped(out, &tags.white);
    out.push_str(&elo(tags.white_rating));
    out.push_str(" &ndash; ");
    push_html_escaped(out, &tags.black);
    out.push_str(&elo(tags.black_rating));
    out.push_str("</h2>\n<p>");
    push_html_escaped(out, &tags.event);
    if !tags.round.is_empty() && tags.round != "?" {
        out.push_str(" (Round ");
        push_html_escaped(out, &tags.round);
        out.push(')');
    }
    if !tags.site.is_empty() && tags.site != "?" {
        out.push_str(" &middot; ");
        push_html_escaped(out, &tags.site);
    }
    out.push_str(" &middot; ");
    push_html_escaped(out, &tags.date.short());
    out.push_str(" &middot; ");
    push_html_escaped(out, tags.result.as_str());
    if let Some(eco) = tags.eco {
        out.push_str(" &middot; ");
        push_html_escaped(out, &eco.to_string());
    }
    out.push_str("</p>\n</header>\n");
}

struct Renderer<'a> {
    opts: &'a HtmlOptions,
    diagrams: &'a mut dyn DiagramSource,
    out: String,
    /// Per open line: the next move must show its number.
    need_number: Vec<bool>,
}

impl Renderer<'_> {
    fn diagram(&mut self, pos: &Position) {
        let fen = pos.fen();
        let svg = self.diagrams.svg(&fen, false, self.opts.diagram_px);
        self.out.push_str("<figure class=\"dg\">");
        self.out.push_str(&svg);
        self.out.push_str("</figure>\n");
    }

    fn mv(&mut self, m: &MoveEv<'_>) {
        let want_diagram = self.opts.diagram_px > 0;
        let comment_raw = if self.opts.comments { m.comment } else { None };
        let mut diagram = false;
        let need = self.need_number.last().copied().unwrap_or(true);
        let main = m.depth == 0;
        if main {
            self.out.push_str("<b>");
        }
        if m.white || need {
            self.out.push_str(&m.number.to_string());
            self.out.push_str(if m.white { "." } else { "..." });
        }
        push_html_escaped(&mut self.out, &m.san);
        if let Some(n) = self.need_number.last_mut() {
            *n = false;
        }
        for nag in m.nags {
            if nag.value() == NAG_DIAGRAM {
                if want_diagram {
                    diagram = true;
                } else {
                    self.out.push_str(" <i>(D)</i>");
                }
                continue;
            }
            self.push_nag(*nag);
        }
        if main {
            self.out.push_str("</b>");
        }
        self.out.push(' ');
        if let Some(text) = comment_raw {
            if want_diagram && text.starts_with('#') {
                diagram = true;
            }
            let stripped = strip_mark_codes(text);
            if !stripped.trim().is_empty() {
                self.out.push_str("<span class=\"c\">{");
                push_html_escaped(&mut self.out, stripped.trim());
                self.out.push_str("}</span> ");
                self.set_need();
            }
        }
        if diagram {
            self.diagram(m.after);
            self.set_need();
        }
    }

    fn set_need(&mut self) {
        if let Some(n) = self.need_number.last_mut() {
            *n = true;
        }
    }

    fn push_nag(&mut self, nag: Nag) {
        if self.opts.symbols {
            push_html_escaped(&mut self.out, &nag.glyph());
        } else {
            self.out.push_str(&nag.to_string());
        }
    }
}

/// Renders one game as an `<article>` string.
pub fn game_to_html_string(
    game: &Game,
    opts: &HtmlOptions,
    diagrams: &mut dyn DiagramSource,
) -> Result<String, WriteError> {
    let mut r = Renderer {
        opts,
        diagrams,
        out: String::new(),
        need_number: vec![true],
    };
    r.out.push_str("<article class=\"game\">\n");
    push_header(&mut r.out, game);
    let start = game.start_position();
    let start_fen = start.fen();
    if start_fen != standard_fen() {
        r.out.push_str("<p class=\"fen\">FEN: ");
        push_html_escaped(&mut r.out, &start_fen);
        r.out.push_str("</p>\n");
        if opts.diagram_px > 0 {
            r.diagram(start);
        }
    }
    if opts.comments
        && let Some(text) = game.initial_comment()
    {
        let stripped = strip_mark_codes(text);
        if !stripped.trim().is_empty() {
            r.out.push_str("<p><span class=\"c\">{");
            push_html_escaped(&mut r.out, stripped.trim());
            r.out.push_str("}</span></p>\n");
        }
    }
    r.out.push_str("<div class=\"moves\">\n");
    let indent = if opts.indent_variations { " ind" } else { "" };
    {
        // The walk callback needs the renderer; collect events lazily by
        // running the walk with a closure over `r`.
        let rr = &mut r;
        walk(game, opts.variations, |ev| match ev {
            Ev::Move(m) => {
                if m.first_in_line
                    && m.depth > 0
                    && rr.opts.comments
                    && let Some(text) = m.comment_before
                {
                    let stripped = strip_mark_codes(text);
                    if !stripped.trim().is_empty() {
                        rr.out.push_str("<span class=\"c\">{");
                        push_html_escaped(&mut rr.out, stripped.trim());
                        rr.out.push_str("}</span> ");
                    }
                }
                rr.mv(&m);
            }
            Ev::VarStart => {
                rr.out.push_str("<div class=\"var");
                rr.out.push_str(indent);
                rr.out.push_str("\">(");
                rr.need_number.push(true);
            }
            Ev::VarEnd => {
                while rr.out.ends_with(' ') {
                    rr.out.pop();
                }
                rr.out.push_str(")</div>\n");
                rr.need_number.pop();
                rr.set_need();
            }
        })?;
    }
    r.out.push_str("\n</div>\n<p class=\"res\"><b>");
    push_html_escaped(&mut r.out, game.tags().result.as_str());
    r.out.push_str("</b></p>\n</article>\n");
    Ok(r.out)
}

/// Writes one game as an `<article>`.
///
/// The text is built in memory first, so nothing is written when the game
/// cannot be rendered.
pub fn write_game_html<W: Write>(
    w: &mut W,
    game: &Game,
    opts: &HtmlOptions,
    diagrams: &mut dyn DiagramSource,
) -> Result<(), WriteError> {
    let text = game_to_html_string(game, opts, diagrams)?;
    w.write_all(text.as_bytes())?;
    Ok(())
}

/// Writes a complete document: the start text, every game and the end text.
pub fn write_html_document<W: Write>(
    w: &mut W,
    games: &[&Game],
    opts: &HtmlOptions,
    diagrams: &mut dyn DiagramSource,
) -> Result<(), WriteError> {
    let mut text = opts.start.clone();
    for game in games {
        text.push_str(&game_to_html_string(game, opts, diagrams)?);
    }
    text.push_str(&opts.end);
    w.write_all(text.as_bytes())?;
    Ok(())
}
