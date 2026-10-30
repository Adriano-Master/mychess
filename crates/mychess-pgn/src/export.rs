//! Document writers for exported games: HTML, LaTeX and the NAG to LaTeX
//! table (slice M7-4a, `docs/specs/m7-4-exportacao.md`).
//!
//! All writers are pure: they render a [`mychess_core::Game`] to text and
//! never touch the file system. Files, volumes and the append mode live in
//! `mychess-db`.
//!
//! - [`html`]: [`write_game_html`], [`write_html_document`], [`HtmlOptions`],
//!   [`DiagramSource`];
//! - [`latex`]: [`write_game_latex`], [`write_latex_document`], [`LatexOptions`];
//! - [`latex_nag`]: the NAG table of Scid's `naglatex.h`;
//! - [`html_escape`], [`latex_escape`]: the escapes used by the writers.

pub mod escape;
pub mod html;
pub mod latex;
pub mod nag_latex;
mod walk;

pub use escape::{html_escape, latex_escape, push_html_escaped, push_latex_escaped};
pub use html::{
    DiagramSource, HtmlOptions, NoDiagrams, default_html_end, default_html_start,
    game_to_html_string, write_game_html, write_html_document,
};
pub use latex::{
    LatexOptions, default_latex_end, default_latex_start, game_to_latex_string, write_game_latex,
    write_latex_document,
};
pub use nag_latex::latex_nag;
