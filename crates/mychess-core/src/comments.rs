//! Commands embedded in comments: `[%draw ...]`, `[%eval ...]`, `[%clk ...]`.
//!
//! Comments keep their commands as plain text (the core never erases them
//! when saving). This module reads them without allocating a regex engine:
//! the scanners are hand-written, linear in the text size and never panic.
//!
//! A command block is `[%name arguments]`. As in Scid, a block that is not
//! closed by `]` before the next `[` (or the end of the text) is ordinary
//! text. The formats follow Scid's `::board::mark` (`tcl/board.tcl`) and the
//! `ChessBase` `[%csl ...]`/`[%cal ...]` extensions.

use std::borrow::Cow;

use shakmaty::Square;

/// Color used by marks that name none (Scid's default).
pub const DEFAULT_MARK_COLOR: &str = "firebrick";

/// Score that Scid gives to a mate (`[%eval #3]`), in pawns.
pub const MATE_SCORE: f32 = 327.0;

/// What a [`BoardMark`] draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MarkKind {
    /// A filled square (`full`; also what the old `[%mark ...]` means).
    Full,
    /// A square outline (`square`).
    Square,
    /// An arrow from one square to another (`arrow`).
    Arrow,
    /// A circle (`circle`).
    Circle,
    /// A disk (`disk`).
    Disk,
    /// A tux icon (`tux`).
    Tux,
    /// A single character written on the square (`[-+=?!A-Za-z0-9]`).
    Text(char),
}

impl MarkKind {
    /// Whether the kind is a character valid for [`MarkKind::Text`].
    fn is_text_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '=' | '?' | '!')
    }

    fn from_word(word: &str) -> Option<MarkKind> {
        Some(match word {
            "full" => MarkKind::Full,
            "square" => MarkKind::Square,
            "arrow" => MarkKind::Arrow,
            "circle" => MarkKind::Circle,
            "disk" => MarkKind::Disk,
            "tux" => MarkKind::Tux,
            _ => {
                let mut chars = word.chars();
                let c = chars.next()?;
                if chars.next().is_some() || !Self::is_text_char(c) {
                    return None;
                }
                MarkKind::Text(c)
            }
        })
    }

    /// The token used in `[%draw token,square,color]`.
    fn token(self) -> String {
        match self {
            MarkKind::Full => "full".to_owned(),
            MarkKind::Square => "square".to_owned(),
            MarkKind::Arrow => "arrow".to_owned(),
            MarkKind::Circle => "circle".to_owned(),
            MarkKind::Disk => "disk".to_owned(),
            MarkKind::Tux => "tux".to_owned(),
            MarkKind::Text(c) => c.to_string(),
        }
    }
}

/// A mark drawn on the board: a square marker or an arrow.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BoardMark {
    /// What to draw.
    pub kind: MarkKind,
    /// The marked square, or the arrow's origin.
    pub from: Square,
    /// The arrow's destination. Invariant: `Some` only for
    /// [`MarkKind::Arrow`] (the parsers uphold it; [`toggle_mark`] ignores a
    /// mark that breaks it). Prefer [`BoardMark::square`] and
    /// [`BoardMark::arrow`], which uphold it by construction.
    pub to: Option<Square>,
    /// The color, as written (a Tk color name such as `green` or `#ff0000`).
    pub color: String,
}

impl BoardMark {
    /// A square marker of `kind` (anything but an arrow) on `sq`.
    pub fn square(kind: MarkKind, sq: Square, color: impl Into<String>) -> BoardMark {
        BoardMark {
            kind,
            from: sq,
            to: None,
            color: color.into(),
        }
    }

    /// An arrow from `from` to `to`.
    pub fn arrow(from: Square, to: Square, color: impl Into<String>) -> BoardMark {
        BoardMark {
            kind: MarkKind::Arrow,
            from,
            to: Some(to),
            color: color.into(),
        }
    }
}

/// A command found in a comment by [`embedded_commands`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EmbeddedCommand {
    /// A board mark (`[%draw ...]`, `[%mark ...]`, `[%arrow ...]`; one entry
    /// per marker of a `[%csl ...]`/`[%cal ...]`).
    Mark(BoardMark),
    /// `[%eval X]`, in pawns (mate is [`MATE_SCORE`]).
    Eval(f32),
    /// `[%clk h:m:s]`, in seconds.
    Clk(u32),
    /// `[%emt m:s]` or `[%emt h:m:s]`, in seconds.
    Emt(u32),
    /// A well-formed block this module does not interpret (or malformed
    /// arguments of a known command).
    Other {
        /// The command name (`ct`, `eval`...).
        name: String,
        /// The text after the name, trimmed.
        args: String,
    },
}

/// The color Scid assigns to a `ChessBase` color letter (`G R Y B O C`).
pub fn chessbase_color(letter: char) -> Option<&'static str> {
    Some(match letter {
        'G' => "green",
        'R' => "firebrick",
        'Y' => "yellow2",
        'B' => "blue",
        'O' => "darkOrange1",
        'C' => "turquoise",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Block scanning
// ---------------------------------------------------------------------------

/// A `[%...]` block: byte range in the text (brackets included) and the
/// text between `[%` and `]`.
struct Block<'a> {
    start: usize,
    end: usize,
    body: &'a str,
}

/// Iterator over the well-formed `[%...]` blocks of a text, in order.
struct Blocks<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Blocks<'a> {
    fn new(text: &'a str) -> Self {
        Blocks { text, pos: 0 }
    }
}

impl<'a> Iterator for Blocks<'a> {
    type Item = Block<'a>;

    fn next(&mut self) -> Option<Block<'a>> {
        let bytes = self.text.as_bytes();
        loop {
            let start = self.pos + self.text.get(self.pos..)?.find("[%")?;
            let body_start = start + 2;
            // The scan stops at the next '[', so failed scans never overlap
            // and the whole iteration is linear.
            let close = bytes[body_start..]
                .iter()
                .position(|b| matches!(b, b']' | b'['));
            match close {
                Some(k) if bytes[body_start + k] == b']' => {
                    let end = body_start + k + 1;
                    self.pos = end;
                    return Some(Block {
                        start,
                        end,
                        body: &self.text[body_start..end - 1],
                    });
                }
                _ => self.pos = start + 1,
            }
        }
    }
}

/// Splits a block body into its name and trimmed arguments.
fn split_body(body: &str) -> (&str, &str) {
    match body.find(|c: char| c.is_ascii_whitespace()) {
        Some(i) => (&body[..i], body[i..].trim()),
        None => (body, ""),
    }
}

/// The arguments of the first `[%name ...]` block, if any.
fn command_arg<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    Blocks::new(text).find_map(|b| {
        let (n, args) = split_body(b.body);
        (n == name).then_some(args)
    })
}

fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Removes every `[%...]` block and the white space that follows it (Scid's
/// `strTrimMarkCodes`, but for any command name). A block not closed before
/// the next `[` or the end of the text stays as text.
///
/// Returns the input borrowed when nothing was removed.
pub fn strip_mark_codes(text: &str) -> Cow<'_, str> {
    let mut out = String::new();
    let mut copied = 0;
    let mut any = false;
    for b in Blocks::new(text) {
        any = true;
        out.push_str(&text[copied..b.start]);
        copied = b.end;
        while text.as_bytes().get(copied).is_some_and(|c| is_c_space(*c)) {
            copied += 1;
        }
    }
    if !any {
        return Cow::Borrowed(text);
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
}

// ---------------------------------------------------------------------------
// Marks
// ---------------------------------------------------------------------------

fn square(s: &str) -> Option<Square> {
    if s.len() != 2 {
        return None;
    }
    Square::from_ascii(s.as_bytes()).ok()
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// A color as the Scid grammar allows: starts with a word character or `#`.
fn color_text(s: &str) -> Option<String> {
    let s = s.trim();
    let first = s.bytes().next()?;
    (is_word_byte(first) || first == b'#').then(|| s.to_owned())
}

fn mark(kind: MarkKind, from: Square, to: Option<Square>, color: String) -> EmbeddedCommand {
    EmbeddedCommand::Mark(BoardMark {
        kind,
        from,
        to,
        color,
    })
}

/// Old Scid syntax: `[%mark SQ [SQ] [color]]`, `[%arrow SQ SQ [color]]`.
fn parse_scid_mark(name: &str, args: &str) -> Option<EmbeddedCommand> {
    let square_at = |s: &str| -> Option<(Square, usize)> {
        let sq = square(s.get(..2)?)?;
        let boundary = s.as_bytes().get(2).is_none_or(|b| !is_word_byte(*b));
        boundary.then_some((sq, 2))
    };
    let (from, n) = square_at(args)?;
    let mut rest = args[n..].trim_start();
    let mut to = None;
    if let Some((sq, n)) = square_at(rest) {
        to = Some(sq);
        rest = rest[n..].trim_start();
    }
    let color = if rest.is_empty() {
        DEFAULT_MARK_COLOR.to_owned()
    } else {
        color_text(rest)?
    };
    if name == "arrow" {
        Some(mark(MarkKind::Arrow, from, Some(to?), color))
    } else {
        Some(mark(MarkKind::Full, from, None, color))
    }
}

/// `[%draw [type,]SQ[,SQ][,color]]`.
fn parse_draw(args: &str) -> Option<EmbeddedCommand> {
    let (first, rest) = args
        .split_once(',')
        .map_or((args, None), |(a, b)| (a, Some(b)));
    let (kind, from, mut rest) = if let Some(sq) = square(first) {
        (None, sq, rest)
    } else {
        let kind = MarkKind::from_word(first)?;
        let (sq_text, after) = rest?
            .split_once(',')
            .map_or((rest?, None), |(a, b)| (a, Some(b)));
        (Some(kind), square(sq_text)?, after)
    };
    let mut to = None;
    if let Some(r) = rest {
        let (head, tail) = r.split_once(',').map_or((r, None), |(a, b)| (a, Some(b)));
        if let Some(sq) = square(head) {
            to = Some(sq);
            rest = tail;
        }
    }
    let color = match rest {
        None => DEFAULT_MARK_COLOR.to_owned(),
        Some(c) => color_text(c)?,
    };
    let kind = match (kind, to) {
        (None | Some(MarkKind::Arrow), Some(_)) => MarkKind::Arrow,
        (None, None) => MarkKind::Full,
        (Some(MarkKind::Arrow), None) | (Some(_), Some(_)) => return None,
        (Some(k), None) => k,
    };
    Some(mark(kind, from, to, color))
}

/// `ChessBase` `[%csl <C><SQ>,...]` (circles) and `[%cal <C><SQ><SQ>,...]`
/// (arrows). All items must be well formed.
fn parse_chessbase(name: &str, args: &str) -> Option<Vec<EmbeddedCommand>> {
    let arrows = name == "cal";
    let width = if arrows { 5 } else { 3 };
    let mut out = Vec::new();
    for item in args.split(',') {
        if item.len() != width || !item.is_ascii() {
            return None;
        }
        let color = chessbase_color(item.chars().next()?)?.to_owned();
        let from = square(&item[1..3])?;
        if arrows {
            out.push(mark(
                MarkKind::Arrow,
                from,
                Some(square(&item[3..5])?),
                color,
            ));
        } else {
            out.push(mark(MarkKind::Circle, from, None, color));
        }
    }
    Some(out)
}

/// Finds and decodes every embedded command, in order of appearance.
///
/// Known commands with malformed arguments are returned as
/// [`EmbeddedCommand::Other`].
pub fn embedded_commands(text: &str) -> Vec<EmbeddedCommand> {
    let mut out = Vec::new();
    for block in Blocks::new(text) {
        let (name, args) = split_body(block.body);
        let parsed = match name {
            "mark" | "arrow" => parse_scid_mark(name, args).map(|c| vec![c]),
            "draw" => parse_draw(args).map(|c| vec![c]),
            "csl" | "cal" => parse_chessbase(name, args),
            "eval" => parse_eval_value(args).map(|v| vec![EmbeddedCommand::Eval(v)]),
            "clk" => parse_clock(args).map(|v| vec![EmbeddedCommand::Clk(v)]),
            "emt" => parse_clock(args).map(|v| vec![EmbeddedCommand::Emt(v)]),
            _ => None,
        };
        match parsed {
            Some(commands) => out.extend(commands),
            None => out.push(EmbeddedCommand::Other {
                name: name.to_owned(),
                args: args.to_owned(),
            }),
        }
    }
    out
}

/// Tokens of the kinds that replace each other on one square (Scid's
/// `typeRegsub`).
fn replaces_on_square(token: &str) -> bool {
    matches!(
        token,
        "tux"
            | "circle"
            | "disk"
            | "full"
            | "+"
            | "-"
            | "="
            | "?"
            | "!"
            | "1"
            | "2"
            | "3"
            | "4"
            | "5"
            | "6"
            | "7"
            | "8"
            | "9"
    )
}

/// A color that [`toggle_mark`] writes and removes: ASCII alphanumerics and
/// `_`, or `#` followed by hexadecimal digits. A color that looks like a
/// square (`a1`...`h8`) is refused, since [`embedded_commands`] would read it
/// as an arrow destination.
fn is_mark_color(s: &str) -> bool {
    let looks_like_square = matches!(s.as_bytes(), [b'a'..=b'h', b'1'..=b'8']);
    if looks_like_square {
        return false;
    }
    match s.strip_prefix('#') {
        Some(hex) => !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        None => !s.is_empty() && s.bytes().all(is_word_byte),
    }
}

/// Toggles `mark` in `comment`, like Scid's `addMarker` and `drawArrow`.
///
/// The mark is written as ` [%draw type,square,color]` (or
/// `[%draw arrow,from,to,color]`) at the end of the comment. If the identical
/// mark is already there, it is removed instead, so applying the same mark
/// twice restores the comment (trimmed). Before adding, a previous mark on
/// the same square of the kinds `tux circle disk full + - = ? ! 1..9` is
/// removed, whatever its color; an arrow is removed in both directions and
/// with any color. The result is trimmed.
///
/// An arrow with `from == to`, a mark whose `to` does not match its kind, a
/// text character outside `[-+=?!A-Za-z0-9]` or an invalid color leaves the
/// comment unchanged. A valid color is made of ASCII letters, digits and `_`,
/// or is `#` followed by hexadecimal digits; names that look like a square
/// (`a1`...`h8`) are refused because they would read back as an arrow
/// destination, and `,`, `]` and spaces are never allowed. An empty color
/// means [`DEFAULT_MARK_COLOR`].
pub fn toggle_mark(comment: &str, mark: &BoardMark) -> String {
    let unchanged = || comment.to_owned();
    let color = if mark.color.is_empty() {
        DEFAULT_MARK_COLOR
    } else {
        mark.color.as_str()
    };
    if !is_mark_color(color) {
        return unchanged();
    }
    if let MarkKind::Text(c) = mark.kind
        && !MarkKind::is_text_char(c)
    {
        return unchanged();
    }
    let token = mark.kind.token();
    let from = mark.from.to_string();
    let (to, canonical) = match (mark.kind, mark.to) {
        (MarkKind::Arrow, Some(to)) if to != mark.from => {
            let to = to.to_string();
            let text = format!("[%draw arrow,{from},{to},{color}]");
            (Some(to), text)
        }
        (kind, None) if kind != MarkKind::Arrow => {
            (None, format!("[%draw {token},{from},{color}]"))
        }
        _ => return unchanged(),
    };

    let mut out = String::with_capacity(comment.len() + canonical.len() + 1);
    let mut copied = 0;
    let mut erase = false;
    for block in Blocks::new(comment) {
        let Some(args) = block.body.strip_prefix("draw ") else {
            continue;
        };
        let parts: Vec<&str> = args.split(',').collect();
        let removable = match &to {
            Some(to) => {
                parts.len() == 4
                    && parts[0] == "arrow"
                    && ((parts[1] == from && parts[2] == *to)
                        || (parts[1] == *to && parts[2] == from))
                    && is_mark_color(parts[3])
            }
            None => {
                parts.len() == 3
                    && parts[1] == from
                    && is_mark_color(parts[2])
                    && (parts[0] == token
                        || (replaces_on_square(&token) && replaces_on_square(parts[0])))
            }
        };
        if removable {
            erase |= comment[block.start..block.end] == canonical;
            out.push_str(&comment[copied..block.start]);
            copied = block.end;
        }
    }
    out.push_str(&comment[copied..]);
    let mut result = out.trim().to_owned();
    if !erase {
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(&canonical);
    }
    result
}

// ---------------------------------------------------------------------------
// Evaluations and clocks
// ---------------------------------------------------------------------------

/// Reads an evaluation at the start of `s`: `#` (mate; `#-` for Black),
/// or a number with optional sign and decimals.
fn parse_eval_value(s: &str) -> Option<f32> {
    let b = s.as_bytes();
    if b.first() == Some(&b'#') {
        return Some(if b.get(1) == Some(&b'-') {
            -MATE_SCORE
        } else {
            MATE_SCORE
        });
    }
    let sign = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let int_len = b[sign..].iter().take_while(|c| c.is_ascii_digit()).count();
    if int_len == 0 {
        return None;
    }
    let mut end = sign + int_len;
    if b.get(end) == Some(&b'.') {
        let frac = b[end + 1..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if frac > 0 {
            end += 1 + frac;
        }
    }
    s[..end].parse::<f32>().ok().filter(|v| v.is_finite())
}

/// The evaluation stored in a comment, in pawns, as Scid's `sc_game scores`.
///
/// With a `[%eval X]` command, `X` is the answer: `#` means mate
/// ([`MATE_SCORE`], negative for `#-`), otherwise a number with an optional
/// sign; anything else gives `None`. Without it, the first `+` or `-`
/// followed by a digit is read as a number, skipping a `-` that follows a
/// digit (game results such as `1-0`). Scid clamps scores to +-10 only for
/// its graph; this function does not.
pub fn eval_of(text: &str) -> Option<f32> {
    if let Some(arg) = command_arg(text, "eval") {
        return parse_eval_value(arg);
    }
    let b = text.as_bytes();
    (0..b.len().saturating_sub(1))
        .filter(|&i| matches!(b[i], b'+' | b'-') && b[i + 1].is_ascii_digit())
        .find(|&i| !(b[i] == b'-' && i > 0 && b[i - 1].is_ascii_digit()))
        .and_then(|i| parse_eval_value(&text[i..]))
}

/// Reads `h:m:s` or `m:s` (the seconds may have decimals, which are
/// dropped) as a number of seconds.
fn parse_clock(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.trim().splitn(4, ':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let number = |p: &str| -> Option<u64> {
        if p.is_empty() || p.len() > 9 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        p.parse().ok()
    };
    let (last, init) = parts.split_last()?;
    let seconds = match last.split_once('.') {
        Some((whole, frac)) if frac.bytes().all(|b| b.is_ascii_digit()) => number(whole)?,
        Some(_) => return None,
        None => number(last)?,
    };
    let mut total = seconds;
    let mut unit = 60;
    for p in init.iter().rev() {
        total += number(p)? * unit;
        unit *= 60;
    }
    u32::try_from(total).ok()
}

/// The time on the clock from the first `[%clk h:m:s]` (or `m:s`) command,
/// in seconds.
pub fn clock_of(text: &str) -> Option<u32> {
    command_arg(text, "clk").and_then(parse_clock)
}

/// The elapsed move time from the first `[%emt m:s]` (or `h:m:s`) command,
/// in seconds.
pub fn emt_of(text: &str) -> Option<u32> {
    command_arg(text, "emt").and_then(parse_clock)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(s: &str) -> Square {
        s.parse().unwrap()
    }

    fn bm(kind: MarkKind, from: &str, to: Option<&str>, color: &str) -> BoardMark {
        BoardMark {
            kind,
            from: sq(from),
            to: to.map(sq),
            color: color.to_owned(),
        }
    }

    fn marks(text: &str) -> Vec<BoardMark> {
        embedded_commands(text)
            .into_iter()
            .filter_map(|c| match c {
                EmbeddedCommand::Mark(m) => Some(m),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn strips_codes_and_following_space() {
        assert_eq!(strip_mark_codes("a [%draw e4] b"), "a b");
        assert_eq!(strip_mark_codes("[%eval 0.3]  \n x [%clk 0:01:00]"), "x ");
        assert_eq!(strip_mark_codes("a [%b c"), "a [%b c");
        assert_eq!(strip_mark_codes("a [%b [%c d] e"), "a [%b e");
        assert_eq!(strip_mark_codes("[x] [%] é"), "[x] é");
        assert!(matches!(strip_mark_codes("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn old_scid_formats() {
        assert_eq!(
            marks("[%mark e4 green]"),
            [bm(MarkKind::Full, "e4", None, "green")]
        );
        assert_eq!(
            marks("[%mark e4]"),
            [bm(MarkKind::Full, "e4", None, "firebrick")]
        );
        assert_eq!(
            marks("[%arrow c4 f7]"),
            [bm(MarkKind::Arrow, "c4", Some("f7"), "firebrick")]
        );
        assert_eq!(
            marks("[%arrow c4 f7 blue]"),
            [bm(MarkKind::Arrow, "c4", Some("f7"), "blue")]
        );
        assert_eq!(marks("[%arrow c4]").len(), 0);
        assert_eq!(marks("[%mark e44]").len(), 0);
    }

    #[test]
    fn draw_format() {
        assert_eq!(
            marks("[%draw e4]"),
            [bm(MarkKind::Full, "e4", None, "firebrick")]
        );
        assert_eq!(
            marks("[%draw circle,f7,blue]"),
            [bm(MarkKind::Circle, "f7", None, "blue")]
        );
        assert_eq!(
            marks("[%draw e2,e4]"),
            [bm(MarkKind::Arrow, "e2", Some("e4"), "firebrick")]
        );
        assert_eq!(
            marks("[%draw arrow,e2,e4,green]"),
            [bm(MarkKind::Arrow, "e2", Some("e4"), "green")]
        );
        assert_eq!(
            marks("[%draw 1,e4]"),
            [bm(MarkKind::Text('1'), "e4", None, "firebrick")]
        );
        assert_eq!(
            marks("[%draw ?,e4,#ff0000]"),
            [bm(MarkKind::Text('?'), "e4", None, "#ff0000")]
        );
        for bad in [
            "[%draw]",
            "[%draw arrow,e4]",
            "[%draw circle,e2,e4]",
            "[%draw 1,e2,e4]",
            "[%draw bogus,e4]",
            "[%draw e9]",
            "[%draw e4,]",
            "[%draw e4,,x]",
        ] {
            assert_eq!(marks(bad).len(), 0, "{bad}");
        }
    }

    #[test]
    fn chessbase_formats() {
        assert_eq!(
            marks("[%csl Ye5,Gd4]"),
            [
                bm(MarkKind::Circle, "e5", None, "yellow2"),
                bm(MarkKind::Circle, "d4", None, "green")
            ]
        );
        assert_eq!(
            marks("[%cal Rd5e4,Bb1c3]"),
            [
                bm(MarkKind::Arrow, "d5", Some("e4"), "firebrick"),
                bm(MarkKind::Arrow, "b1", Some("c3"), "blue")
            ]
        );
        assert_eq!(marks("[%csl Xe5]").len(), 0);
        assert_eq!(marks("[%csl Ye5,Gd]").len(), 0);
        assert_eq!(marks("[%cal Rd5]").len(), 0);
    }

    #[test]
    fn chessbase_color_table() {
        let table = [
            ('G', "green"),
            ('R', "firebrick"),
            ('Y', "yellow2"),
            ('B', "blue"),
            ('O', "darkOrange1"),
            ('C', "turquoise"),
        ];
        for (c, name) in table {
            assert_eq!(chessbase_color(c), Some(name));
        }
        assert_eq!(chessbase_color('g'), None);
    }

    #[test]
    fn mixed_commands_in_order() {
        let cmds =
            embedded_commands("x [%eval 0.5] [%clk 1:02:03] [%emt 0:07] [%ct foo bar] [%eval zz]");
        assert_eq!(
            cmds,
            [
                EmbeddedCommand::Eval(0.5),
                EmbeddedCommand::Clk(3723),
                EmbeddedCommand::Emt(7),
                EmbeddedCommand::Other {
                    name: "ct".into(),
                    args: "foo bar".into()
                },
                EmbeddedCommand::Other {
                    name: "eval".into(),
                    args: "zz".into()
                },
            ]
        );
    }

    #[test]
    fn toggle_adds_and_removes() {
        let m = bm(MarkKind::Circle, "e4", None, "green");
        let once = toggle_mark("good move", &m);
        assert_eq!(once, "good move [%draw circle,e4,green]");
        assert_eq!(toggle_mark(&once, &m), "good move");
        assert_eq!(toggle_mark("", &m), "[%draw circle,e4,green]");
        assert_eq!(toggle_mark(&toggle_mark("", &m), &m), "");
    }

    #[test]
    fn toggle_replaces_marks_of_the_same_square() {
        let green = bm(MarkKind::Circle, "e4", None, "green");
        let red = bm(MarkKind::Full, "e4", None, "red");
        let c = toggle_mark("t [%draw circle,e4,green] [%draw circle,d4,green]", &red);
        assert_eq!(c, "t  [%draw circle,d4,green] [%draw full,e4,red]");
        // A different color of the same mark is replaced, not toggled off.
        let c = toggle_mark("[%draw circle,e4,blue]", &green);
        assert_eq!(c, "[%draw circle,e4,green]");
        // `square` is not in the replace list.
        let sqm = bm(MarkKind::Square, "e4", None, "red");
        let c = toggle_mark("[%draw circle,e4,blue]", &sqm);
        assert_eq!(c, "[%draw circle,e4,blue] [%draw square,e4,red]");
    }

    #[test]
    fn toggle_arrows() {
        let a = bm(MarkKind::Arrow, "e2", Some("e4"), "green");
        let c = toggle_mark("", &a);
        assert_eq!(c, "[%draw arrow,e2,e4,green]");
        assert_eq!(toggle_mark(&c, &a), "");
        // Reverse direction and other color are removed, then re-added.
        let c = toggle_mark("[%draw arrow,e4,e2,blue]", &a);
        assert_eq!(c, "[%draw arrow,e2,e4,green]");
        // Origin equal to destination is ignored.
        let same = bm(MarkKind::Arrow, "e2", Some("e2"), "green");
        assert_eq!(toggle_mark("x", &same), "x");
    }

    #[test]
    fn toggle_ignores_invalid_marks() {
        let c = "keep";
        assert_eq!(toggle_mark(c, &bm(MarkKind::Arrow, "e2", None, "red")), c);
        assert_eq!(
            toggle_mark(c, &bm(MarkKind::Circle, "e2", Some("e4"), "red")),
            c
        );
        assert_eq!(toggle_mark(c, &bm(MarkKind::Circle, "e2", None, "re]d")), c);
        // Colors that read back as a square, or have forbidden characters.
        for bad in ["a1", "h8", "#", "#xyz", "a,b", "a b", "#ff 00"] {
            let m = bm(MarkKind::Circle, "e2", None, bad);
            assert_eq!(toggle_mark(c, &m), c, "{bad}");
        }
        assert_eq!(
            toggle_mark(c, &bm(MarkKind::Text('#'), "e2", None, "red")),
            c
        );
        let default = toggle_mark(c, &bm(MarkKind::Full, "e2", None, ""));
        assert_eq!(default, "keep [%draw full,e2,firebrick]");
    }

    #[test]
    fn toggle_accepts_hex_and_underscore_colors() {
        for color in ["#ff0000", "#FFF", "dark_green", "_x", "a9", "i1"] {
            let m = bm(MarkKind::Circle, "e4", None, color);
            let once = toggle_mark("x", &m);
            assert_eq!(once, format!("x [%draw circle,e4,{color}]"));
            assert_eq!(marks(&once), std::slice::from_ref(&m), "{color}");
            assert_eq!(toggle_mark(&once, &m), "x", "{color}");
        }
    }

    #[test]
    fn mark_constructors_uphold_the_invariant() {
        let c = BoardMark::square(MarkKind::Disk, sq("d4"), "red");
        assert_eq!(c, bm(MarkKind::Disk, "d4", None, "red"));
        let a = BoardMark::arrow(sq("e2"), sq("e4"), String::from("blue"));
        assert_eq!(a, bm(MarkKind::Arrow, "e2", Some("e4"), "blue"));
    }

    #[test]
    fn clock_with_too_many_parts_is_rejected() {
        assert_eq!(parse_clock("1:2:3:4"), None);
        assert_eq!(parse_clock(&":1".repeat(100_000)), None);
        assert_eq!(parse_clock("1:02:03"), Some(3723));
    }

    #[test]
    fn toggled_marks_are_read_back() {
        let m = bm(MarkKind::Text('!'), "a1", None, "red");
        assert_eq!(marks(&toggle_mark("", &m)), [m]);
        let a = bm(MarkKind::Arrow, "a1", Some("h8"), "red");
        assert_eq!(marks(&toggle_mark("", &a)), [a]);
    }

    #[test]
    fn eval_with_command() {
        assert_eq!(eval_of("[%eval 0.25]"), Some(0.25));
        assert_eq!(eval_of("[%eval -1.5] +9"), Some(-1.5));
        assert_eq!(eval_of("[%eval +3]"), Some(3.0));
        assert_eq!(eval_of("[%eval #3]"), Some(MATE_SCORE));
        assert_eq!(eval_of("[%eval #-2]"), Some(-MATE_SCORE));
        assert_eq!(eval_of("[%eval abc] +1.0"), None);
        assert_eq!(eval_of("[%eval -.5]"), None);
        assert_eq!(eval_of("[%eval 2.]"), Some(2.0));
    }

    #[test]
    fn eval_without_command() {
        assert_eq!(eval_of("Black is better -0.75 here"), Some(-0.75));
        assert_eq!(eval_of("+0.13: Nf3"), Some(0.13));
        assert_eq!(eval_of("1-0"), None);
        assert_eq!(eval_of("1-0 then +2.1"), Some(2.1));
        assert_eq!(eval_of("no score - here"), None);
        assert_eq!(eval_of(""), None);
        assert_eq!(eval_of("-"), None);
    }

    #[test]
    fn clock_and_emt() {
        assert_eq!(clock_of("[%clk 1:30:05]"), Some(5405));
        assert_eq!(clock_of("[%clk 0:00:09.7]"), Some(9));
        assert_eq!(emt_of("a [%emt 2:30] b"), Some(150));
        assert_eq!(emt_of("[%emt 0:00:03]"), Some(3));
        assert_eq!(clock_of("[%clk abc]"), None);
        assert_eq!(clock_of("[%clk 5]"), None);
        assert_eq!(clock_of("[%clk 1:2:3:4]"), None);
        assert_eq!(clock_of("[%clk 999999999:99:99]"), None);
        assert_eq!(clock_of("no command"), None);
    }

    #[test]
    fn hostile_input_does_not_panic() {
        let mut nasty = "[%".repeat(5000);
        nasty.push_str("é[%draw ,,,]]]%[[ [%csl ,] [%eval #] [%clk ::] [%draw é,é]");
        drop(strip_mark_codes(&nasty));
        drop(embedded_commands(&nasty));
        let _ = eval_of(&nasty);
        let _ = clock_of(&nasty);
        drop(toggle_mark(
            &nasty,
            &bm(MarkKind::Circle, "e4", None, "red"),
        ));
        for text in ["[", "[%", "[%]", "]", "é[%é]é", "[%draw é"] {
            drop(strip_mark_codes(text));
            drop(embedded_commands(text));
            let _ = eval_of(text);
        }
    }
}
