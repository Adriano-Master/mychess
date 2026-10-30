//! The SVG of a position (spec M7-4, rule 26, D3724, D3726).
//!
//! The board is drawn in a coordinate system where a square is 100 units, so
//! the text does not depend on the pixel size, except for the `width` and
//! `height` of the root element.

use std::fmt::Write as _;
use std::sync::Arc;

use mychess_core::{BoardMark, Color, MarkKind, Position, Role, Square};

use super::{ImageError, namespace_svg_ids};

/// Units of the side of a square.
const SQ: i32 = 100;
/// Units of the side of the board.
const BOARD: i32 = 8 * SQ;
/// The pieces in the order of [`PieceSvgs`]: white then black.
const ROLES: [Role; 6] = [
    Role::King,
    Role::Queen,
    Role::Rook,
    Role::Bishop,
    Role::Knight,
    Role::Pawn,
];

/// The name of a piece in the ids: `wK`, `bQ`...
pub(super) fn piece_name(color: Color, role: Role) -> &'static str {
    match (color, role) {
        (Color::White, Role::King) => "wK",
        (Color::White, Role::Queen) => "wQ",
        (Color::White, Role::Rook) => "wR",
        (Color::White, Role::Bishop) => "wB",
        (Color::White, Role::Knight) => "wN",
        (Color::White, Role::Pawn) => "wP",
        (Color::Black, Role::King) => "bK",
        (Color::Black, Role::Queen) => "bQ",
        (Color::Black, Role::Rook) => "bR",
        (Color::Black, Role::Bishop) => "bB",
        (Color::Black, Role::Knight) => "bN",
        (Color::Black, Role::Pawn) => "bP",
    }
}

/// A piece drawing split into the parts the board needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Symbol {
    view_box: String,
    inner: String,
}

/// The twelve drawings of a piece set, checked once (spec rule 28).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceSvgs {
    symbols: [Symbol; 12],
}

impl PieceSvgs {
    /// Builds the set from `draw(color, role)`, the SVG document of each
    /// piece (as in `assets/pieces/<set>/wK.svg`).
    ///
    /// # Errors
    ///
    /// [`ImageError::BadPiece`] for the first piece that is empty, has no
    /// `<svg>` element with a `viewBox` or contains an active element
    /// (`<script>`, `<foreignObject>`, an `on...=` attribute, `javascript:`).
    pub fn new(mut draw: impl FnMut(Color, Role) -> String) -> Result<PieceSvgs, ImageError> {
        let mut symbols = Vec::with_capacity(12);
        for color in [Color::White, Color::Black] {
            for role in ROLES {
                let name = piece_name(color, role);
                let symbol =
                    parse_symbol(&draw(color, role)).ok_or(ImageError::BadPiece { piece: name })?;
                symbols.push(symbol);
            }
        }
        let symbols: [Symbol; 12] = symbols
            .try_into()
            .map_err(|_| ImageError::BadPiece { piece: "wK" })?;
        Ok(PieceSvgs { symbols })
    }

    fn symbol(&self, color: Color, role: Role) -> &Symbol {
        let base = if color == Color::White { 0 } else { 6 };
        let offset = ROLES.iter().position(|r| *r == role).unwrap_or(0);
        // `symbols` always holds twelve entries and `base + offset < 12`.
        let [first, ..] = &self.symbols;
        self.symbols.get(base + offset).unwrap_or(first)
    }
}

/// Splits a piece document into its `viewBox` and its content. Rejects
/// active content: the drawing ends up inside HTML files.
fn parse_symbol(doc: &str) -> Option<Symbol> {
    let lower = doc.to_ascii_lowercase();
    for bad in [
        "<script",
        "<foreignobject",
        "javascript:",
        "<iframe",
        "<!entity",
    ] {
        if lower.contains(bad) {
            return None;
        }
    }
    if has_event_attribute(&lower) {
        return None;
    }
    let open = doc.find("<svg")?;
    let tag_end = tag_end(doc, open)?;
    let tag = doc.get(open..tag_end)?;
    if tag.ends_with('/') {
        return None;
    }
    let view_box = attribute(tag, "viewBox")?;
    if view_box.split_whitespace().count() != 4 || view_box.chars().any(|c| c == '"' || c == '<') {
        return None;
    }
    let close = doc.rfind("</svg>")?;
    let inner = doc.get(tag_end + 1..close)?.trim();
    (!inner.is_empty()).then(|| Symbol {
        view_box: view_box.to_owned(),
        inner: inner.to_owned(),
    })
}

/// Whether the (lower-cased) text has an attribute `on<name>=`.
fn has_event_attribute(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    lower.match_indices("on").any(|(i, _)| {
        let after_space = i
            .checked_sub(1)
            .and_then(|p| bytes.get(p))
            .is_some_and(u8::is_ascii_whitespace);
        let rest = lower.get(i + 2..).unwrap_or("");
        let name_len = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        after_space
            && name_len > 0
            && rest
                .get(name_len..)
                .is_some_and(|r| r.trim_start().starts_with('='))
    })
}

/// The byte offset of the `>` that ends the tag opened at `open` (quotes
/// respected).
fn tag_end(doc: &str, open: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (offset, byte) in doc.as_bytes().get(open..)?.iter().enumerate() {
        match (quote, *byte) {
            (None, b'"' | b'\'') => quote = Some(*byte),
            (Some(q), b) if b == q => quote = None,
            (None, b'>') => return Some(open + offset),
            _ => {}
        }
    }
    None
}

/// The value of attribute `name` in the text of a tag.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    for (at, _) in tag.match_indices(name) {
        let before = at
            .checked_sub(1)
            .and_then(|i| tag.as_bytes().get(i))
            .copied();
        if !before.is_some_and(|b| b.is_ascii_whitespace()) {
            continue;
        }
        let rest = tag.get(at + name.len()..)?.trim_start();
        let rest = rest.strip_prefix('=')?.trim_start();
        let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let value = rest.get(1..)?;
        let end = value.find(quote)?;
        return value.get(..end);
    }
    None
}

/// Colors of the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardPalette {
    /// Light squares, `#rrggbb`.
    pub light: String,
    /// Dark squares, `#rrggbb`.
    pub dark: String,
    /// The squares of the last move, `#rrggbb` (drawn with some opacity).
    pub highlight: String,
    /// The margin around the board, `#rrggbb`.
    pub margin: String,
}

impl Default for BoardPalette {
    /// The brown board of Lichess, with a green highlight.
    fn default() -> BoardPalette {
        BoardPalette {
            light: "#f0d9b5".to_owned(),
            dark: "#b58863".to_owned(),
            highlight: "#9bc700".to_owned(),
            margin: "#ffffff".to_owned(),
        }
    }
}

/// A color that is safe to write into an attribute: `#rgb`, `#rrggbb`,
/// `#rrggbbaa` or a
/// plain name; anything else gives `fallback`.
fn safe_color<'a>(color: &'a str, fallback: &'a str) -> &'a str {
    let hex = color
        .strip_prefix('#')
        .is_some_and(|h| matches!(h.len(), 3 | 6 | 8) && h.bytes().all(|b| b.is_ascii_hexdigit()));
    let name =
        !color.is_empty() && color.len() <= 24 && color.bytes().all(|b| b.is_ascii_alphabetic());
    if hex || name { color } else { fallback }
}

/// What to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardImage {
    /// The position.
    pub position: Position,
    /// Seen from Black's side.
    pub flip: bool,
    /// Side of the board in pixels (the nominal size of an SVG).
    pub size_px: u32,
    /// Margin around the board in pixels.
    pub margin_px: u32,
    /// File and rank letters inside the squares.
    pub coordinates: bool,
    /// Squares of the last move to highlight.
    pub last_move: Option<(Square, Square)>,
    /// Square marks and arrows.
    pub marks: Vec<BoardMark>,
    /// Colors.
    pub palette: BoardPalette,
    /// The piece drawings.
    pub pieces: Arc<PieceSvgs>,
    /// Prefix of every id of the document, so that several boards can share
    /// a page (D3726). Letters, digits and `-` only.
    pub id_prefix: String,
    /// Text of the `<title>` element (the FEN of a diagram).
    pub title: Option<String>,
}

impl BoardImage {
    /// A board with the usual choices: coordinates, no marks, no margin.
    pub fn new(position: Position, pieces: Arc<PieceSvgs>, size_px: u32) -> BoardImage {
        BoardImage {
            position,
            flip: false,
            size_px,
            margin_px: 0,
            coordinates: true,
            last_move: None,
            marks: Vec::new(),
            palette: BoardPalette::default(),
            pieces,
            id_prefix: String::new(),
            title: None,
        }
    }
}

fn escape(text: &str) -> String {
    mychess_pgn::export::html_escape(text)
}

/// Display column and row (0 at the top left) of `sq`.
fn cell(sq: Square, flip: bool) -> (i32, i32) {
    let file = i32::from(u8::from(sq.file()));
    let rank = i32::from(u8::from(sq.rank()));
    if flip {
        (7 - file, rank)
    } else {
        (file, 7 - rank)
    }
}

/// The `<symbol>` elements of the twelve pieces, with the ids of each piece
/// prefixed (`<prefix>wK`, and inside it `<prefix>wK-<id>`), for a document
/// that keeps the pieces in one place (the viewer of the HTML export).
pub fn piece_symbols(pieces: &PieceSvgs, prefix: &str) -> String {
    let mut out = String::new();
    for color in [Color::White, Color::Black] {
        for role in ROLES {
            push_symbol(&mut out, pieces, color, role, prefix);
        }
    }
    out
}

fn push_symbol(out: &mut String, pieces: &PieceSvgs, color: Color, role: Role, prefix: &str) {
    let name = piece_name(color, role);
    let symbol = pieces.symbol(color, role);
    let inner = namespace_svg_ids(&symbol.inner, &format!("{prefix}{name}-"));
    let _ = write!(
        out,
        "<symbol id=\"{prefix}{name}\" viewBox=\"{}\">{inner}</symbol>",
        symbol.view_box
    );
}

/// The SVG document of `img`.
///
/// Layout, in drawing order: the margin, 64 squares (`class="sq"`), the
/// highlights of the last move (`class="hl"`), square marks, coordinates
/// (`class="co"`), pieces (`<use>`), arrows. Pixel sizes are not checked
/// here; [`board_png`](super::board_png) does.
pub fn board_svg(img: &BoardImage) -> String {
    svg_document(img, true)
}

/// As [`board_svg`]; without `squares` the squares and the margin are left
/// out (a transparent PNG).
pub(super) fn svg_document(img: &BoardImage, squares: bool) -> String {
    let margin =
        i32::try_from(img.margin_px.saturating_mul(BOARD as u32) / img.size_px.max(1)).unwrap_or(0);
    let total = BOARD + 2 * margin;
    let side = img.size_px.saturating_add(img.margin_px.saturating_mul(2));
    let prefix = &img.id_prefix;
    let mut out = String::with_capacity(16 * 1024);
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{side}\" height=\"{side}\" viewBox=\"{} {} {total} {total}\">",
        -margin, -margin
    );
    if let Some(title) = &img.title {
        let _ = write!(out, "<title>{}</title>", escape(title));
    }
    // Only the pieces on the board get a symbol.
    out.push_str("<defs>");
    let mut seen = [false; 12];
    for (_, piece) in img.position.pieces() {
        let index = usize::from(piece.color == Color::Black) * 6
            + ROLES.iter().position(|r| *r == piece.role).unwrap_or(0);
        if let Some(flag) = seen.get_mut(index)
            && !*flag
        {
            *flag = true;
            push_symbol(&mut out, &img.pieces, piece.color, piece.role, prefix);
        }
    }
    out.push_str("</defs>");
    if squares {
        if margin > 0 {
            let _ = write!(
                out,
                "<rect class=\"bg\" x=\"{}\" y=\"{}\" width=\"{total}\" height=\"{total}\" fill=\"{}\"/>",
                -margin,
                -margin,
                safe_color(&img.palette.margin, "#ffffff")
            );
        }
        push_squares(&mut out, img);
    }
    push_highlights(&mut out, img);
    push_square_marks(&mut out, img);
    if img.coordinates {
        push_coordinates(&mut out, img);
    }
    push_pieces(&mut out, img);
    push_arrows(&mut out, img);
    out.push_str("</svg>");
    out
}

fn push_squares(out: &mut String, img: &BoardImage) {
    for sq in Square::ALL {
        let (col, row) = cell(sq, img.flip);
        // a1 is dark.
        let dark = (u8::from(sq.file()) + u8::from(sq.rank())) % 2 == 0;
        let fill = if dark {
            safe_color(&img.palette.dark, "#b58863")
        } else {
            safe_color(&img.palette.light, "#f0d9b5")
        };
        let _ = write!(
            out,
            "<rect class=\"sq\" x=\"{}\" y=\"{}\" width=\"{SQ}\" height=\"{SQ}\" fill=\"{fill}\"/>",
            col * SQ,
            row * SQ
        );
    }
}

fn push_highlights(out: &mut String, img: &BoardImage) {
    let Some((from, to)) = img.last_move else {
        return;
    };
    let fill = safe_color(&img.palette.highlight, "#9bc700");
    for sq in [from, to] {
        let (col, row) = cell(sq, img.flip);
        let _ = write!(
            out,
            "<rect class=\"hl\" x=\"{}\" y=\"{}\" width=\"{SQ}\" height=\"{SQ}\" fill=\"{fill}\" fill-opacity=\"0.41\"/>",
            col * SQ,
            row * SQ
        );
    }
}

fn mark_color(mark: &BoardMark) -> &str {
    safe_color(&mark.color, "firebrick")
}

fn push_square_marks(out: &mut String, img: &BoardImage) {
    for mark in &img.marks {
        let (col, row) = cell(mark.from, img.flip);
        let (x, y) = (col * SQ, row * SQ);
        let color = mark_color(mark);
        match mark.kind {
            MarkKind::Full => {
                let _ = write!(
                    out,
                    "<rect class=\"mk\" x=\"{x}\" y=\"{y}\" width=\"{SQ}\" height=\"{SQ}\" fill=\"{color}\" fill-opacity=\"0.5\"/>"
                );
            }
            MarkKind::Square => {
                let _ = write!(
                    out,
                    "<rect class=\"mk\" x=\"{}\" y=\"{}\" width=\"90\" height=\"90\" fill=\"none\" stroke=\"{color}\" stroke-width=\"8\"/>",
                    x + 5,
                    y + 5
                );
            }
            MarkKind::Circle => {
                let _ = write!(
                    out,
                    "<circle class=\"mk\" cx=\"{}\" cy=\"{}\" r=\"44\" fill=\"none\" stroke=\"{color}\" stroke-width=\"8\"/>",
                    x + 50,
                    y + 50
                );
            }
            MarkKind::Disk => {
                let _ = write!(
                    out,
                    "<circle class=\"mk\" cx=\"{}\" cy=\"{}\" r=\"30\" fill=\"{color}\" fill-opacity=\"0.6\"/>",
                    x + 50,
                    y + 50
                );
            }
            MarkKind::Text(c) => {
                let _ = write!(
                    out,
                    "<text class=\"mk\" x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-family=\"Noto Sans, sans-serif\" font-size=\"64\" font-weight=\"bold\" fill=\"{color}\" fill-opacity=\"0.8\">{}</text>",
                    x + 50,
                    y + 70,
                    escape(&c.to_string())
                );
            }
            // Arrows are drawn above the pieces; the tux has no drawing.
            _ => {}
        }
    }
}

fn push_coordinates(out: &mut String, img: &BoardImage) {
    // 12% of a square (rule 26).
    let size = SQ * 12 / 100;
    for sq in Square::ALL {
        let (col, row) = cell(sq, img.flip);
        let bottom = row == 7;
        let left = col == 0;
        if !bottom && !left {
            continue;
        }
        let dark = (u8::from(sq.file()) + u8::from(sq.rank())) % 2 == 0;
        // Each label uses the color of the opposite square.
        let fill = if dark {
            safe_color(&img.palette.light, "#f0d9b5")
        } else {
            safe_color(&img.palette.dark, "#b58863")
        };
        let (x, y) = (col * SQ, row * SQ);
        if bottom {
            let _ = write!(
                out,
                "<text class=\"co\" x=\"{}\" y=\"{}\" text-anchor=\"end\" font-family=\"Noto Sans, sans-serif\" font-size=\"{size}\" fill=\"{fill}\">{}</text>",
                x + SQ - 4,
                y + SQ - 4,
                sq.file().char()
            );
        }
        if left {
            let _ = write!(
                out,
                "<text class=\"co\" x=\"{}\" y=\"{}\" font-family=\"Noto Sans, sans-serif\" font-size=\"{size}\" fill=\"{fill}\">{}</text>",
                x + 4,
                y + size + 2,
                sq.rank().char()
            );
        }
    }
}

fn push_pieces(out: &mut String, img: &BoardImage) {
    let prefix = &img.id_prefix;
    // In board order so the text does not depend on the iteration of the
    // position.
    for sq in Square::ALL {
        let Some(piece) = img.position.piece_at(sq) else {
            continue;
        };
        let (col, row) = cell(sq, img.flip);
        let _ = write!(
            out,
            "<use class=\"pc\" href=\"#{prefix}{}\" x=\"{}\" y=\"{}\" width=\"{SQ}\" height=\"{SQ}\"/>",
            piece_name(piece.color, piece.role),
            col * SQ,
            row * SQ
        );
    }
}

fn push_arrows(out: &mut String, img: &BoardImage) {
    for mark in &img.marks {
        let (MarkKind::Arrow, Some(to)) = (mark.kind, mark.to) else {
            continue;
        };
        if to == mark.from {
            continue;
        }
        let (c1, r1) = cell(mark.from, img.flip);
        let (c2, r2) = cell(to, img.flip);
        let (x1, y1) = (f64::from(c1 * SQ + 50), f64::from(r1 * SQ + 50));
        let (x2, y2) = (f64::from(c2 * SQ + 50), f64::from(r2 * SQ + 50));
        let (dx, dy) = (x2 - x1, y2 - y1);
        let len = dx.hypot(dy);
        let (ux, uy) = (dx / len, dy / len);
        let head = 36.0;
        // The shaft stops where the head starts.
        let (bx, by) = (x2 - ux * head, y2 - uy * head);
        let (px, py) = (-uy, ux);
        let color = mark_color(mark);
        let _ = write!(
            out,
            "<line class=\"ar\" x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{bx:.1}\" y2=\"{by:.1}\" stroke=\"{color}\" stroke-width=\"14\" stroke-opacity=\"0.8\"/>\
             <polygon class=\"ar\" points=\"{x2:.1},{y2:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"{color}\" fill-opacity=\"0.8\"/>",
            bx + px * 26.0,
            by + py * 26.0,
            bx - px * 26.0,
            by - py * 26.0,
        );
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A piece drawing with a gradient whose id repeats in every piece.
    pub(crate) fn fake_piece(color: &str) -> String {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 45 45"><defs><linearGradient id="a"><stop offset="0" stop-color="{color}"/></linearGradient></defs><rect x="5" y="5" width="35" height="35" fill="url(#a)"/></svg>"#
        )
    }

    pub(crate) fn fake_set() -> Arc<PieceSvgs> {
        Arc::new(
            PieceSvgs::new(|c, _| {
                fake_piece(if c == Color::White {
                    "#ff0000"
                } else {
                    "#0000ff"
                })
            })
            .expect("set"),
        )
    }

    fn start(pieces: &Arc<PieceSvgs>) -> BoardImage {
        BoardImage::new(Position::new(), pieces.clone(), 512)
    }

    #[test]
    fn the_start_position_has_64_squares_32_pieces_and_16_coordinates() {
        let svg = board_svg(&start(&fake_set()));
        assert_eq!(svg.matches("class=\"sq\"").count(), 64);
        assert_eq!(svg.matches("<use ").count(), 32);
        assert_eq!(svg.matches("class=\"co\"").count(), 16);
        assert!(svg.starts_with("<svg xmlns=") && svg.ends_with("</svg>"));
        assert!(svg.contains("viewBox=\"0 0 800 800\""), "{}", &svg[..300]);
    }

    #[test]
    fn flipping_moves_the_coordinates_and_the_pieces() {
        let set = fake_set();
        let mut img = start(&set);
        let normal = board_svg(&img);
        img.flip = true;
        let flipped = board_svg(&img);
        assert_ne!(normal, flipped);
        // The white king (e1) is at the bottom in one view and at the top in
        // the other.
        assert!(
            normal.contains("href=\"#wK\" x=\"400\" y=\"700\""),
            "{normal}"
        );
        assert!(
            flipped.contains("href=\"#wK\" x=\"300\" y=\"0\""),
            "{flipped}"
        );
        // The first label of the bottom row is `h` when flipped.
        let first_label = |s: &str| {
            let at = s.find("class=\"co\"").expect("label");
            s[at..].split('>').nth(1).and_then(|t| t.chars().next())
        };
        assert_ne!(first_label(&normal), first_label(&flipped));
    }

    #[test]
    fn last_move_and_arrows_are_drawn() {
        let set = fake_set();
        let mut img = start(&set);
        img.last_move = Some((Square::E2, Square::E4));
        img.marks = vec![
            BoardMark::arrow(Square::G1, Square::F3, "green"),
            BoardMark::square(MarkKind::Full, Square::D4, "#ff0000"),
        ];
        let svg = board_svg(&img);
        assert_eq!(svg.matches("class=\"hl\"").count(), 2);
        assert_eq!(svg.matches("class=\"ar\"").count(), 2, "line and head");
        assert_eq!(svg.matches("class=\"mk\"").count(), 1);
    }

    #[test]
    fn output_is_deterministic() {
        let set = fake_set();
        assert_eq!(board_svg(&start(&set)), board_svg(&start(&set)));
    }

    #[test]
    fn piece_ids_are_prefixed_per_piece_and_per_board() {
        let set = fake_set();
        let mut img = start(&set);
        img.id_prefix = "dg3-".to_owned();
        let svg = board_svg(&img);
        assert!(svg.contains("id=\"dg3-wK\""), "{svg}");
        assert!(svg.contains("id=\"dg3-wK-a\"") && svg.contains("id=\"dg3-bQ-a\""));
        assert!(svg.contains("url(#dg3-wK-a)"));
        // No bare gradient id is left to collide with another piece.
        assert!(!svg.contains("id=\"a\""));
    }

    #[test]
    fn a_hostile_color_is_not_written() {
        let set = fake_set();
        let mut img = start(&set);
        img.marks = vec![BoardMark::square(
            MarkKind::Full,
            Square::D4,
            "red\"/><script>",
        )];
        let svg = board_svg(&img);
        assert!(!svg.contains("<script"), "{svg}");
        assert!(svg.contains("fill=\"firebrick\""));
    }

    #[test]
    fn pieces_with_active_content_or_no_viewbox_are_refused() {
        let bad = |doc: &str| PieceSvgs::new(|_, _| doc.to_owned());
        assert!(matches!(bad(""), Err(ImageError::BadPiece { piece: "wK" })));
        assert!(bad("<svg><rect/></svg>").is_err(), "no viewBox");
        assert!(bad(r#"<svg viewBox="0 0 1 1"><script>x</script></svg>"#).is_err());
        assert!(bad(r#"<svg viewBox="0 0 1 1"><rect onload="x()"/></svg>"#).is_err());
        assert!(bad(r#"<svg viewBox="0 0 1 1"/>"#).is_err(), "self closed");
        assert!(bad(&fake_piece("red")).is_ok());
    }
}
