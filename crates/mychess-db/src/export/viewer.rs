// Behaviour derived from Scid vs PC 4.27, tcl/misc/misc.tcl (namespace
// ::html: exportCurrentFilter, toHtml, recordElt). The viewer itself is new
// code of MyChess (docs/adr/0016-exportacao-visualizador-html-e-imagens.md);
// no file of the Scid is reused. Copyright (c) 1999-2003 Shane Hudson; Scid vs
// PC contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! Export of games as a self-contained HTML viewer, in volumes (decisions
//! D3703 to D3708 of `docs/specs/m7-4-exportacao.md`).
//!
//! [`viewer_volumes`] writes `X.html`, `X_2.html`, ... : each **volume** is a
//! single file with the stylesheet, the script, the piece symbols and the
//! games as JSON inside `<script type="application/json">`. A volume holds
//! at most [`ViewerOptions::per_file`] games and about
//! [`ViewerOptions::max_bytes`] bytes (a game too big for a volume still gets
//! one of its own). `X.pgn`, with the games of all finished volumes, is
//! optional. Nothing else is created or overwritten.
//!
//! The stylesheet, the script and the piece symbols come from the host as
//! [`ViewerAssets`] (the script and the style of slice M7-4d); they are
//! trusted text, but are refused when they would close their own element.
//!
//! # Safety of the data
//!
//! Everything that comes from the games is escaped: HTML text with
//! `& < > " '`; in the JSON, strings escape `"`, `\`, control characters,
//! `< > &` (as `<`, `>`, `&`) and U+2028/U+2029, so the data
//! block cannot contain `</script>` or `<!--`. A `<meta>` Content Security
//! Policy forbids any request (`default-src 'none'`), and the page has no
//! external reference but the relative links to the other volumes and the
//! PGN.
//!
//! # JSON model
//!
//! ```text
//! { "volume": 1, "title": "...", "prev": null, "next": "X_2.html", "pgn": "X.pgn",
//!   "games": [ { "n": 17, "white": "...", "black": "...", "event": "...", "site": "...",
//!                "date": "2024.05.??", "round": "3", "result": "1-0", "eco": "B20",
//!                "white_elo": 2500, "black_elo": null, "start_fen": null,
//!                "nodes": [ { "fen": "...", "san": null, "nags": [], "comment": null,
//!                             "depth": 0, "prev": -1, "next": 1, "variations": [] }, ... ] } ] }
//! ```
//!
//! `nodes[0]` is the start position; `next` is the main continuation of a
//! node and `variations` its alternatives (indices into `nodes`); `depth` is
//! the variation depth of the move. Comments have their `[%...]` blocks
//! removed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mychess_core::{Game, NodeId, strip_mark_codes};
use mychess_pgn::export::{HtmlOptions, NoDiagrams, game_to_html_string, html_escape};
use mychess_pgn::write::{WriteOptions, game_to_string};

use super::names::{stem_and_dir, volume_file_name};
use super::text_file::push_fmt;
use super::{ExportError, ExportItem, ExportOptions, ExportSkipReason, Output, open_output};
use crate::progress::{CancelToken, ProgressSink};

/// Default games per volume.
pub const DEFAULT_PER_FILE: usize = 500;
/// Largest value of [`ViewerOptions::per_file`].
pub const MAX_PER_FILE: usize = 5000;
/// Default size limit of a volume: 20 MiB.
pub const DEFAULT_MAX_BYTES: usize = 20 * 1024 * 1024;

/// The CSS, the script and the piece symbols of the viewer (from the host).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewerAssets {
    /// The stylesheet, placed inside `<style>`.
    pub css: String,
    /// The script, placed inside `<script>`.
    pub js: String,
    /// SVG `<symbol>` elements of the pieces, placed inside a hidden `<svg>`.
    pub pieces_svg: String,
}

/// Options of [`viewer_volumes`].
#[derive(Debug, Clone)]
pub struct ViewerOptions {
    /// Games per volume, 1 to [`MAX_PER_FILE`].
    pub per_file: usize,
    /// Approximate size limit of a volume, in bytes.
    pub max_bytes: usize,
    /// Also write `X.pgn` with the games of the finished volumes.
    pub include_pgn: bool,
    /// Title of the pages.
    pub title: String,
    /// Credit of the piece set, written as an HTML comment
    /// (`Pieces: <name> by <author>, <license>`).
    pub pieces_credit: Option<String>,
    /// CSS, script and pieces.
    pub assets: ViewerAssets,
    /// How the games go to `X.pgn`.
    pub pgn: WriteOptions,
}

impl Default for ViewerOptions {
    fn default() -> ViewerOptions {
        ViewerOptions {
            per_file: DEFAULT_PER_FILE,
            max_bytes: DEFAULT_MAX_BYTES,
            include_pgn: true,
            title: "MyChess".to_owned(),
            pieces_credit: None,
            assets: ViewerAssets::default(),
            pgn: WriteOptions::default(),
        }
    }
}

/// The result of [`viewer_volumes`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ViewerReport {
    /// The volume files written, in order.
    pub volumes: Vec<PathBuf>,
    /// `X.pgn`, if written.
    pub pgn: Option<PathBuf>,
    /// Games in the finished volumes.
    pub written: u64,
    /// Games that could not be read or rendered.
    pub skipped: Vec<(u32, ExportSkipReason)>,
    /// Cancelled: the volume in progress was thrown away.
    pub interrupted: bool,
    /// Time spent.
    pub elapsed: Duration,
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

/// Appends `text` as a JSON string that is also safe inside `<script>`.
pub fn push_json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if u32::from(c) < 0x20 || c == '\u{7f}' => {
                push_fmt(out, format_args!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn push_opt_str(out: &mut String, text: Option<&str>) {
    match text {
        Some(t) => push_json_string(out, t),
        None => out.push_str("null"),
    }
}

fn unknown_to_none(text: &str) -> Option<&str> {
    (!text.is_empty() && text != "?").then_some(text)
}

fn cleaned_comment(text: Option<&str>) -> Option<String> {
    let stripped = strip_mark_codes(text?);
    let t = stripped.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// The JSON object of one game (see the module documentation).
#[allow(clippy::too_many_lines)] // one pass that writes the whole object
fn game_json(game: &Game, number: u32) -> Result<String, String> {
    let root = game.root();
    let mut ids: Vec<NodeId> = vec![root];
    ids.extend(game.descendants(root).map_err(|e| e.to_string())?);
    let index: HashMap<NodeId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut depths: Vec<u32> = Vec::with_capacity(ids.len());
    let tags = game.tags();
    let mut out = String::new();
    out.push_str("{\"n\":");
    out.push_str(&number.to_string());
    for (key, value) in [
        ("white", unknown_to_none(&tags.white)),
        ("black", unknown_to_none(&tags.black)),
        ("event", unknown_to_none(&tags.event)),
        ("site", unknown_to_none(&tags.site)),
    ] {
        push_fmt(&mut out, format_args!(",\"{key}\":"));
        push_opt_str(&mut out, value);
    }
    out.push_str(",\"date\":");
    push_json_string(&mut out, &tags.date.short());
    out.push_str(",\"round\":");
    push_opt_str(&mut out, unknown_to_none(&tags.round));
    out.push_str(",\"result\":");
    push_json_string(&mut out, tags.result.as_str());
    out.push_str(",\"eco\":");
    push_opt_str(&mut out, tags.eco.map(|e| e.to_string()).as_deref());
    for (key, rating) in [
        ("white_elo", tags.white_rating),
        ("black_elo", tags.black_rating),
    ] {
        push_fmt(&mut out, format_args!(",\"{key}\":"));
        match rating.map(|r| r.value.get()).filter(|v| *v > 0) {
            Some(v) => out.push_str(&v.to_string()),
            None => out.push_str("null"),
        }
    }
    let start = game.start_position();
    out.push_str(",\"start_fen\":");
    let start_fen = start.fen();
    push_opt_str(
        &mut out,
        (start_fen != mychess_core::Position::new().fen()).then_some(start_fen.as_str()),
    );
    out.push_str(",\"nodes\":[");
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let parent = game.parent(*id).map_err(|e| e.to_string())?;
        let children = game.children(*id).map_err(|e| e.to_string())?;
        let (depth, san) = match parent {
            None => (0, None),
            Some(p) => {
                let p_idx = *index.get(&p).ok_or("parent not indexed")?;
                let siblings = game.children(p).map_err(|e| e.to_string())?;
                let base = *depths.get(p_idx).ok_or("parent depth missing")?;
                let depth = if siblings.first() == Some(id) {
                    base
                } else {
                    base + 1
                };
                let mv = game.mv(*id).map_err(|e| e.to_string())?;
                let before = game.position_at(p).map_err(|e| e.to_string())?;
                (depth, Some(before.san(mv).map_err(|e| e.to_string())?))
            }
        };
        depths.push(depth);
        let pos = game.position_at(*id).map_err(|e| e.to_string())?;
        out.push_str("{\"fen\":");
        push_json_string(&mut out, &pos.fen());
        out.push_str(",\"san\":");
        push_opt_str(&mut out, san.as_deref());
        out.push_str(",\"nags\":[");
        let nags = game.nags(*id).map_err(|e| e.to_string())?;
        for (k, nag) in nags.iter().enumerate() {
            if k > 0 {
                out.push(',');
            }
            out.push_str(&nag.value().to_string());
        }
        out.push_str("],\"comment\":");
        let comment = if i == 0 {
            game.initial_comment()
        } else {
            game.comment(*id).map_err(|e| e.to_string())?
        };
        push_opt_str(&mut out, cleaned_comment(comment).as_deref());
        push_fmt(&mut out, format_args!(",\"depth\":{depth},\"prev\":"));
        match parent {
            Some(p) => out.push_str(&index.get(&p).copied().unwrap_or(0).to_string()),
            None => out.push_str("-1"),
        }
        out.push_str(",\"next\":");
        match children.first().and_then(|c| index.get(c)) {
            Some(n) => out.push_str(&n.to_string()),
            None => out.push_str("-1"),
        }
        out.push_str(",\"variations\":[");
        for (k, c) in children.iter().skip(1).enumerate() {
            if k > 0 {
                out.push(',');
            }
            out.push_str(&index.get(c).copied().unwrap_or(0).to_string());
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    Ok(out)
}

// ---------------------------------------------------------------------------
// Volumes
// ---------------------------------------------------------------------------

struct Rendered {
    json: String,
    noscript: String,
    pgn: String,
}

impl Rendered {
    fn size(&self) -> usize {
        self.json.len() + self.noscript.len()
    }
}

/// Percent-encodes a relative file name for an `href`.
fn href_encode(name: &str) -> String {
    let mut out = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            push_fmt(&mut out, format_args!("%{b:02X}"));
        }
    }
    out
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn validate(options: &ViewerOptions) -> Result<(), ExportError> {
    if options.per_file == 0 || options.per_file > MAX_PER_FILE {
        return Err(ExportError::InvalidOptions("per_file must be 1 to 5000"));
    }
    if options.max_bytes == 0 {
        return Err(ExportError::InvalidOptions("max_bytes must be positive"));
    }
    if contains_ci(&options.assets.js, "</script") || contains_ci(&options.assets.js, "<!--") {
        return Err(ExportError::InvalidOptions("script closes its own element"));
    }
    if contains_ci(&options.assets.css, "</style") {
        return Err(ExportError::InvalidOptions("style closes its own element"));
    }
    if let Some(why) = unsafe_svg(&options.assets.pieces_svg) {
        return Err(ExportError::InvalidOptions(why));
    }
    Ok(())
}

/// Why `svg` (the inside of the hidden `<svg>` of the page) cannot be trusted,
/// or `None`. The text is compared in lower case and with white space and
/// control characters removed, so `</SVG >` and `<ScRiPt` do not get through;
/// an event attribute (`onload=`, `onclick =`...) is found by its shape.
fn unsafe_svg(svg: &str) -> Option<&'static str> {
    const FORBIDDEN: [(&str, &str); 13] = [
        ("</svg", "pieces close the svg element"),
        ("<script", "pieces hold a script element"),
        ("<!--", "pieces hold a comment"),
        ("<![cdata[", "pieces hold a CDATA section"),
        ("<foreignobject", "pieces hold a foreignObject"),
        ("<iframe", "pieces hold an iframe"),
        ("<embed", "pieces hold an embed"),
        ("<object", "pieces hold an object"),
        ("<math", "pieces hold a math element"),
        ("<style", "pieces hold a style element"),
        ("javascript:", "pieces hold a javascript URL"),
        ("&#", "pieces hold a character reference"),
        ("<?", "pieces hold a processing instruction"),
    ];
    let squeezed: String = svg
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .flat_map(char::to_lowercase)
        .collect();
    if let Some((_, why)) = FORBIDDEN.iter().find(|(bad, _)| squeezed.contains(bad)) {
        return Some(why);
    }
    // `on<name>=` at the start of an attribute.
    let lower = svg.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    for (i, pair) in chars.windows(2).enumerate() {
        let starts_attribute = i == 0
            || chars.get(i - 1).is_some_and(|c| {
                c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '/')
            });
        if pair != ['o', 'n'] || !starts_attribute {
            continue;
        }
        let rest = chars.get(i + 2..).unwrap_or(&[]);
        let letters = rest.iter().take_while(|c| c.is_ascii_alphabetic()).count();
        let after = rest.get(letters..).unwrap_or(&[]);
        let next = after.iter().find(|c| !c.is_whitespace() && !c.is_control());
        if letters > 0 && next == Some(&'=') {
            return Some("pieces hold an event attribute");
        }
    }
    None
}

const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'";

struct VolumeInfo<'a> {
    number: u32,
    prev: Option<String>,
    next: Option<String>,
    pgn: Option<&'a str>,
}

fn volume_html(options: &ViewerOptions, info: &VolumeInfo<'_>, games: &[Rendered]) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"");
    out.push_str(&html_escape(CSP));
    out.push_str(
        "\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>",
    );
    out.push_str(&html_escape(&options.title));
    push_fmt(&mut out, format_args!(" - {}", info.number));
    out.push_str("</title>\n<style>\n");
    out.push_str(&options.assets.css);
    out.push_str("\n</style>\n</head>\n<body>\n");
    if let Some(credit) = &options.pieces_credit {
        // A comment cannot hold `--`: split every run of dashes, repeating
        // until none is left (`"---"` becomes `"- - -"`).
        let mut safe: String = credit
            .chars()
            .filter(|c| *c != '>' && *c != '<' && !c.is_control())
            .collect();
        while safe.contains("--") {
            safe = safe.replace("--", "- -");
        }
        push_fmt(&mut out, format_args!("<!-- Pieces: {safe} -->\n"));
    }
    if !options.assets.pieces_svg.is_empty() {
        out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" style=\"position:absolute;width:0;height:0;overflow:hidden\" aria-hidden=\"true\">");
        out.push_str(&options.assets.pieces_svg);
        out.push_str("</svg>\n");
    }
    out.push_str("<nav class=\"volumes\">");
    for (label, target) in [("prev", &info.prev), ("next", &info.next)] {
        if let Some(name) = target {
            push_fmt(
                &mut out,
                format_args!(
                    "<a rel=\"{label}\" href=\"{}\">{}</a> ",
                    html_escape(&href_encode(name)),
                    html_escape(name)
                ),
            );
        }
    }
    if let Some(pgn) = info.pgn {
        push_fmt(
            &mut out,
            format_args!(
                "<a rel=\"alternate\" href=\"{}\">{}</a>",
                html_escape(&href_encode(pgn)),
                html_escape(pgn)
            ),
        );
    }
    out.push_str("</nav>\n<main id=\"viewer\"></main>\n<noscript>\n");
    for g in games {
        out.push_str(&g.noscript);
    }
    out.push_str("</noscript>\n<script type=\"application/json\" id=\"mychess-data\">");
    push_fmt(
        &mut out,
        format_args!("{{\"volume\":{},\"title\":", info.number),
    );
    push_json_string(&mut out, &options.title);
    out.push_str(",\"prev\":");
    push_opt_str(&mut out, info.prev.as_deref());
    out.push_str(",\"next\":");
    push_opt_str(&mut out, info.next.as_deref());
    out.push_str(",\"pgn\":");
    push_opt_str(&mut out, info.pgn);
    out.push_str(",\"games\":[");
    for (i, g) in games.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&g.json);
    }
    out.push_str("]}</script>\n<script>\n");
    out.push_str(&options.assets.js);
    out.push_str("\n</script>\n</body>\n</html>\n");
    out
}

fn render(game: &Game, number: u32, options: &ViewerOptions) -> Result<Rendered, ExportSkipReason> {
    let json = game_json(game, number).map_err(ExportSkipReason::Unwritable)?;
    let html_opts = HtmlOptions {
        diagram_px: 0,
        symbols: true,
        ..HtmlOptions::default()
    };
    let noscript = game_to_html_string(game, &html_opts, &mut NoDiagrams)
        .map_err(|e| ExportSkipReason::Unwritable(e.to_string()))?;
    let pgn = if options.include_pgn {
        let mut text = game_to_string(game, &options.pgn)
            .map_err(|e| ExportSkipReason::Unwritable(e.to_string()))?;
        text.push('\n');
        text
    } else {
        String::new()
    };
    Ok(Rendered {
        json,
        noscript,
        pgn,
    })
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), ExportError> {
    let (mut output, _, _) = open_output(path, &ExportOptions::default())?;
    if let Err(e) = output.write(bytes) {
        output.abandon();
        return Err(e);
    }
    output.finish()
}

/// Where and how the volume files of one export are written.
struct VolumeTarget<'a> {
    options: &'a ViewerOptions,
    dir: &'a Path,
    stem: &'a str,
    pgn_name: Option<&'a str>,
}

impl VolumeTarget<'_> {
    /// Writes (or rewrites) volume `number`; `has_next` adds the link to the
    /// following volume.
    fn write(
        &self,
        number: u32,
        has_next: bool,
        batch: &[Rendered],
    ) -> Result<PathBuf, ExportError> {
        let info = VolumeInfo {
            number,
            prev: (number > 1).then(|| volume_file_name(self.stem, number - 1)),
            next: has_next.then(|| volume_file_name(self.stem, number + 1)),
            pgn: self.pgn_name,
        };
        let path = self.dir.join(volume_file_name(self.stem, number));
        write_file(&path, volume_html(self.options, &info, batch).as_bytes())?;
        Ok(path)
    }
}

/// Writes `games` as HTML viewer volumes next to `dest`: `dest` names the
/// first volume (a `.html`/`.htm` extension is dropped and added back; the
/// name is sanitized), the others add `_2`, `_3`... before the extension.
///
/// Progress is `(games handled, expected)`; the token is looked at before
/// each game. Each volume is written to a temporary file and renamed when it
/// is complete, so a cancelled export leaves the finished volumes (and a
/// `X.pgn` with their games) and no partial file.
///
/// # Errors
///
/// [`ExportError::InvalidOptions`] for options out of range or an asset that
/// closes its own element; [`ExportError::NothingToExport`] when no game was
/// written and nothing was cancelled (no file is created; [`ExportError::AllSkipped`]
/// when games were read but all were left out, with the counts);
/// [`ExportError::Io`] for files. After an I/O error, volumes already
/// finished stay on disk.
#[allow(clippy::too_many_lines)] // the volume loop reads best in one piece
pub fn viewer_volumes<I>(
    games: I,
    dest: &Path,
    options: &ViewerOptions,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<ViewerReport, ExportError>
where
    I: IntoIterator<Item = ExportItem>,
{
    let started = Instant::now();
    validate(options)?;
    let (stem, dir) = stem_and_dir(dest).ok_or_else(|| ExportError::NoFileName {
        path: dest.to_owned(),
    })?;
    let mut games = games.into_iter();
    let (low, high) = games.size_hint();
    let total = u64::try_from(high.unwrap_or(low)).unwrap_or(0);
    let pgn_name = options.include_pgn.then(|| format!("{stem}.pgn"));
    let pgn_path = pgn_name.as_ref().map(|n| dir.join(n));

    let mut handled = 0u64;
    let mut skipped: Vec<(u32, ExportSkipReason)> = Vec::new();
    let mut interrupted = false;
    // Next rendered game, read ahead so that "next volume" links are exact.
    let mut fetch = |progress: &mut dyn ProgressSink| -> Option<Rendered> {
        loop {
            if cancel.is_cancelled() {
                interrupted = true;
                return None;
            }
            let item = games.next()?;
            let (number, game) = match item {
                ExportItem::Game(g) => (0, Ok(g)),
                ExportItem::Stored { number, game } => (
                    number,
                    game.map_err(|e| ExportSkipReason::Unreadable(e.to_string())),
                ),
            };
            handled += 1;
            progress.progress(handled, total.max(handled));
            match game.and_then(|g| render(&g, number, options)) {
                Ok(r) => return Some(r),
                Err(reason) => skipped.push((number, reason)),
            }
        }
    };

    let base = options.assets.css.len() + options.assets.js.len() + options.assets.pieces_svg.len();
    let mut pending = fetch(progress);
    let mut volumes: Vec<PathBuf> = Vec::new();
    let mut pgn_output: Option<Output> = None;
    let mut written = 0u64;
    let mut number = 0u32;
    // The last volume written, kept so that its `next` link can be taken out
    // again when the volume it points to never gets written.
    let mut last: Option<(u32, Vec<Rendered>, bool)> = None;
    let mut abandoned_next = false;
    let target = VolumeTarget {
        options,
        dir: &dir,
        stem: &stem,
        pgn_name: pgn_name.as_deref(),
    };
    let outcome: Result<(), ExportError> = (|| {
        while pending.is_some() {
            number += 1;
            let mut batch: Vec<Rendered> = Vec::new();
            let mut bytes = base;
            while let Some(r) = pending.take() {
                if !batch.is_empty()
                    && (batch.len() >= options.per_file
                        || bytes.saturating_add(r.size()) > options.max_bytes)
                {
                    pending = Some(r);
                    break;
                }
                bytes = bytes.saturating_add(r.size());
                batch.push(r);
                pending = fetch(progress);
            }
            // A cancel seen while filling the volume throws it away.
            if cancel.is_cancelled() {
                abandoned_next = true;
                return Ok(());
            }
            let has_next = pending.is_some();
            let path = target.write(number, has_next, &batch)?;
            volumes.push(path);
            written += batch.len() as u64;
            let (_, kept, _) = last.insert((number, batch, has_next));
            if let Some(pgn_path) = &pgn_path {
                if pgn_output.is_none() {
                    pgn_output = Some(open_output(pgn_path, &ExportOptions::default())?.0);
                }
                if let Some(out) = pgn_output.as_mut() {
                    for g in kept.iter() {
                        out.write(g.pgn.as_bytes())?;
                    }
                }
            }
        }
        Ok(())
    })();
    // Whatever stopped the export before the volume the last file points to,
    // that file must not link to a volume that does not exist.
    let dangling = outcome.is_err() || abandoned_next;
    if dangling
        && let Some((n, batch, true)) = &last
        && let Err(e) = target.write(*n, false, batch)
        && outcome.is_ok()
    {
        if let Some(out) = pgn_output.take() {
            out.abandon();
        }
        return Err(e);
    }
    if let Err(e) = outcome {
        if let Some(out) = pgn_output.take() {
            out.abandon();
        }
        return Err(e);
    }
    let interrupted = interrupted || cancel.is_cancelled();
    let mut pgn = None;
    if let Some(out) = pgn_output.take() {
        out.finish()?;
        pgn = pgn_path;
    }
    if written == 0 && !interrupted {
        return Err(if skipped.is_empty() {
            ExportError::NothingToExport
        } else {
            ExportError::AllSkipped {
                handled,
                skipped: u64::try_from(skipped.len()).unwrap_or(u64::MAX),
                first: skipped.first().cloned(),
            }
        });
    }
    Ok(ViewerReport {
        volumes,
        pgn,
        written,
        skipped,
        interrupted,
        elapsed: started.elapsed(),
    })
}
