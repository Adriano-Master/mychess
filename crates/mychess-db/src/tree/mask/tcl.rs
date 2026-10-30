// Derived from the list rules of Tcl 8.6 (`generic/tclUtil.c`: `TclFindElement`,
// `TclScanElement`, `TclConvertElement`), which the Scid vs PC 4.27 uses to
// write and read its mask files (`tcl/windows/tree.tcl`, `::tree::mask::save`
// and `::tree::mask::open`). This file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).
//
// Attribution: the Tcl sources are Copyright (c) the Regents of the University
// of California, Sun Microsystems, Scriptics Corporation, ActiveState and
// other parties, distributed under the Tcl/Tk license (BSD-like: the notice
// and the permission text must stay with copies of the Tcl code). Only the
// list quoting *rules* were studied here; no Tcl code was copied.

//! A reader and a writer of Tcl lists, and a reader of the one-command script
//! that wraps a mask (`docs/formatos/stm.md`, sections 1, 3 and 4; decision
//! D1801). **Nothing is ever executed**: the script reader accepts only
//! `set ::tree::mask::maskSerialized <word>` and refuses any substitution.

use std::borrow::Cow;

/// A syntax error, with the byte offset in the text that was being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TclError {
    pub(crate) offset: usize,
    pub(crate) reason: &'static str,
}

impl TclError {
    fn new(offset: usize, reason: &'static str) -> TclError {
        TclError { offset, reason }
    }
}

/// Why the script of a mask file was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptError {
    /// No command at all (empty file, only blanks and comments).
    Empty,
    /// The first command is not `set`.
    NotSet,
    /// A command that is not the expected `set`, extra words, or a
    /// substitution (`$`, `[`) in a word.
    Foreign { offset: usize },
    /// The command or its word is malformed.
    Syntax(TclError),
}

/// The value of the word of the `set` command.
#[derive(Debug)]
pub(crate) struct ScriptValue<'a> {
    pub(crate) value: Cow<'a, str>,
    /// Offset in the text where `value` starts (approximate when the word had
    /// escapes).
    pub(crate) offset: usize,
}

/// Tcl white space inside lists.
pub(crate) const fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn slice(s: &str, from: usize, to: usize) -> &str {
    // Every offset passed here is next to an ASCII byte (a brace, a quote, a
    // space) or at the end of the text, so it is a character boundary.
    let part = s.get(from..to);
    debug_assert!(part.is_some(), "slice {from}..{to} is not on a boundary");
    part.unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// A lazy reader of the elements of a Tcl list: nothing is collected, so a
/// hostile list costs no memory beyond what the caller keeps. After an error
/// the iterator ends.
pub(crate) struct ListIter<'a> {
    s: &'a str,
    i: usize,
    failed: bool,
}

impl<'a> ListIter<'a> {
    pub(crate) fn new(s: &'a str) -> ListIter<'a> {
        ListIter {
            s,
            i: 0,
            failed: false,
        }
    }
}

impl<'a> Iterator for ListIter<'a> {
    type Item = Result<Cow<'a, str>, TclError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let b = self.s.as_bytes();
        while b.get(self.i).is_some_and(|&c| is_space(c)) {
            self.i += 1;
        }
        if self.i >= b.len() {
            return None;
        }
        match read_list_element(self.s, self.i) {
            Ok((element, next)) => {
                self.i = next;
                Some(Ok(element))
            }
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}

/// Splits a Tcl list into its elements (`Tcl_SplitList`).
///
/// Elements that need no change are borrowed from `s`.
#[cfg(test)]
pub(crate) fn split_list(s: &str) -> Result<Vec<Cow<'_, str>>, TclError> {
    ListIter::new(s).collect()
}

/// Checks a Tcl list and counts its elements, without keeping any.
pub(crate) fn count_list(s: &str) -> Result<usize, TclError> {
    let mut n = 0usize;
    for e in ListIter::new(s) {
        e?;
        n += 1;
    }
    Ok(n)
}

/// Reads a list keeping only its first `keep` elements; returns them and the
/// total count. The whole list is checked.
pub(crate) fn split_head(s: &str, keep: usize) -> Result<(Vec<Cow<'_, str>>, usize), TclError> {
    let mut head = Vec::with_capacity(keep.min(8));
    let mut n = 0usize;
    for e in ListIter::new(s) {
        let e = e?;
        if head.len() < keep {
            head.push(e);
        }
        n += 1;
    }
    Ok((head, n))
}

fn read_list_element(s: &str, start: usize) -> Result<(Cow<'_, str>, usize), TclError> {
    let b = s.as_bytes();
    match b.get(start) {
        Some(b'{') => {
            let (content, end) = read_braced(s, start)?;
            if b.get(end).is_some_and(|&c| !is_space(c)) {
                return Err(TclError::new(end, "garbage after a closing brace"));
            }
            Ok((content, end))
        }
        Some(b'"') => {
            let close = find_quote_end(b, start)?;
            let end = close + 1;
            if b.get(end).is_some_and(|&c| !is_space(c)) {
                return Err(TclError::new(end, "garbage after a closing quote"));
            }
            Ok((subst_backslash(slice(s, start + 1, close)), end))
        }
        _ => {
            let mut j = start;
            while let Some(&c) = b.get(j) {
                if is_space(c) {
                    break;
                }
                j += if c == b'\\' { 2 } else { 1 };
            }
            let j = j.min(b.len());
            Ok((subst_backslash(slice(s, start, j)), j))
        }
    }
}

/// Reads the braced word that starts at `start` (`{`). Returns the content
/// (with the backslash-newline collapse of Tcl) and the offset after the
/// closing brace.
fn read_braced(s: &str, start: usize) -> Result<(Cow<'_, str>, usize), TclError> {
    let b = s.as_bytes();
    let mut depth = 1usize;
    let mut j = start + 1;
    while let Some(&c) = b.get(j) {
        match c {
            b'\\' => j += 1,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let content = slice(s, start + 1, j);
                    return Ok((collapse_backslash_newline(content), j + 1));
                }
            }
            _ => {}
        }
        j += 1;
    }
    Err(TclError::new(start, "unmatched open brace"))
}

/// The offset of the closing `"` of the quoted word that starts at `start`.
fn find_quote_end(b: &[u8], start: usize) -> Result<usize, TclError> {
    let mut j = start + 1;
    while let Some(&c) = b.get(j) {
        match c {
            b'\\' => j += 1,
            b'"' => return Ok(j),
            _ => {}
        }
        j += 1;
    }
    Err(TclError::new(start, "unmatched open quote"))
}

/// Inside braces only the backslash-newline sequence changes (to one space,
/// swallowing the following blanks).
fn collapse_backslash_newline(s: &str) -> Cow<'_, str> {
    if !s.contains("\\\n") {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\n') => {
                out.push(' ');
                while chars.next_if(|&n| n == ' ' || n == '\t').is_some() {}
            }
            Some(n) => {
                out.push('\\');
                out.push(n);
            }
            None => out.push('\\'),
        }
    }
    Cow::Owned(out)
}

fn take_hex(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, max: usize) -> Option<u32> {
    let mut value: Option<u32> = None;
    let mut count = 0;
    while count < max {
        let Some(d) = chars
            .next_if(char::is_ascii_hexdigit)
            .and_then(|c| c.to_digit(16))
        else {
            break;
        };
        value = Some(value.unwrap_or(0).wrapping_mul(16).wrapping_add(d));
        count += 1;
    }
    value
}

/// The backslash substitutions of Tcl (`Tcl_UtfBackslash`).
pub(crate) fn subst_backslash(s: &str) -> Cow<'_, str> {
    if !s.contains('\\') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(n) = chars.next() else {
            out.push('\\');
            break;
        };
        match n {
            'a' => out.push('\u{7}'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'v' => out.push('\u{b}'),
            '\n' => {
                out.push(' ');
                while chars.next_if(|&c| c == ' ' || c == '\t').is_some() {}
            }
            '0'..='7' => {
                let mut v = n.to_digit(8).unwrap_or(0);
                for _ in 0..2 {
                    match chars.next_if(|c| ('0'..='7').contains(c)) {
                        Some(d) => v = v * 8 + d.to_digit(8).unwrap_or(0),
                        None => break,
                    }
                }
                out.push(char::from((v & 0xff) as u8));
            }
            'x' => match take_hex(&mut chars, usize::MAX) {
                Some(v) => out.push(char::from((v & 0xff) as u8)),
                None => out.push('x'),
            },
            'u' => match take_hex(&mut chars, 4) {
                Some(v) => out.push(char::from_u32(v).unwrap_or('\u{fffd}')),
                None => out.push('u'),
            },
            'U' => match take_hex(&mut chars, 8) {
                Some(v) => out.push(char::from_u32(v).unwrap_or('\u{fffd}')),
                None => out.push('U'),
            },
            other => out.push(other),
        }
    }
    Cow::Owned(out)
}

// ---------------------------------------------------------------------------
// The script (`set name word`)
// ---------------------------------------------------------------------------

fn is_blank(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | 0x0b | 0x0c | b'\r')
}

/// Skips blanks and backslash-newline continuations (not newlines).
fn skip_blanks(b: &[u8], mut i: usize) -> usize {
    loop {
        match b.get(i) {
            Some(&c) if is_blank(c) => i += 1,
            Some(b'\\') if b.get(i + 1) == Some(&b'\n') => i += 2,
            _ => return i,
        }
    }
}

/// Skips to the start of the next command: blanks, newlines, `;` and
/// comment lines (a `#` where a command would start).
fn skip_to_command(b: &[u8], mut i: usize) -> usize {
    loop {
        match b.get(i) {
            Some(&c) if is_space(c) || c == b';' => i += 1,
            Some(b'#') => {
                while let Some(&c) = b.get(i) {
                    if c == b'\\' {
                        i += 2;
                    } else if c == b'\n' {
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            _ => return i.min(b.len()),
        }
    }
}

fn ends_command(b: &[u8], i: usize) -> bool {
    matches!(b.get(i), None | Some(b'\n' | b';'))
}

/// Reads one command word. Substitutions (`$`, `[`) are refused.
fn read_script_word(s: &str, start: usize) -> Result<(ScriptValue<'_>, usize), ScriptError> {
    let b = s.as_bytes();
    let (value, offset, end) = match b.get(start) {
        Some(b'{') => {
            let (content, end) = read_braced(s, start).map_err(ScriptError::Syntax)?;
            (content, start + 1, end)
        }
        Some(b'"') => {
            let close = find_quote_end(b, start).map_err(ScriptError::Syntax)?;
            let inner = slice(s, start + 1, close);
            refuse_substitution(inner.as_bytes(), start + 1)?;
            (subst_backslash(inner), start + 1, close + 1)
        }
        _ => {
            let mut j = start;
            while let Some(&c) = b.get(j) {
                if is_blank(c) || c == b'\n' || c == b';' {
                    break;
                }
                j += if c == b'\\' { 2 } else { 1 };
            }
            let j = j.min(b.len());
            let raw = slice(s, start, j);
            refuse_substitution(raw.as_bytes(), start)?;
            (subst_backslash(raw), start, j)
        }
    };
    if !(ends_command(b, end) || b.get(end).is_some_and(|&c| is_blank(c) || c == b'\\')) {
        return Err(ScriptError::Syntax(TclError::new(
            end,
            "garbage after a closing brace or quote",
        )));
    }
    Ok((ScriptValue { value, offset }, end))
}

/// Errors on an unescaped `$` or `[`, which Tcl would substitute.
fn refuse_substitution(raw: &[u8], base: usize) -> Result<(), ScriptError> {
    let mut i = 0;
    while let Some(&c) = raw.get(i) {
        match c {
            b'\\' => i += 1,
            b'$' | b'[' => return Err(ScriptError::Foreign { offset: base + i }),
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

/// Reads the script `set ::tree::mask::maskSerialized <word>` and returns the
/// value of the word. Comments and blank lines may come before and after.
pub(crate) fn parse_set_script(text: &str) -> Result<ScriptValue<'_>, ScriptError> {
    let b = text.as_bytes();
    let mut i = skip_to_command(b, 0);
    if i >= b.len() {
        return Err(ScriptError::Empty);
    }
    let (cmd, end) = read_script_word(text, i)?;
    if cmd.value != "set" {
        return Err(ScriptError::NotSet);
    }
    i = skip_blanks(b, end);
    if ends_command(b, i) {
        return Err(ScriptError::Syntax(TclError::new(i, "set without a name")));
    }
    let (name, end) = read_script_word(text, i)?;
    if name.value != "::tree::mask::maskSerialized" && name.value != "tree::mask::maskSerialized" {
        return Err(ScriptError::Foreign { offset: i });
    }
    i = skip_blanks(b, end);
    if ends_command(b, i) {
        return Err(ScriptError::Syntax(TclError::new(i, "set without a value")));
    }
    let (value, end) = read_script_word(text, i)?;
    let i = skip_blanks(b, end);
    if !ends_command(b, i) {
        return Err(ScriptError::Foreign { offset: i });
    }
    let i = skip_to_command(b, i);
    if i < b.len() {
        return Err(ScriptError::Foreign { offset: i });
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// How an element is written (`TclScanElement`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Form {
    /// As is.
    Raw,
    /// Between braces.
    Braced,
    /// With a backslash before each special character.
    Backslash,
}

/// Decides the form of the element `s`. `first` is true for the first
/// element of a list (Tcl protects a leading `#` there).
pub(crate) fn scan_element(s: &str, first: bool) -> Form {
    if s.is_empty() {
        return Form::Braced;
    }
    let b = s.as_bytes();
    let mut level = 0i64;
    let mut quote = false;
    let mut slash = false;
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        match c {
            b'{' => {
                level += 1;
                quote = true;
            }
            b'}' => {
                level -= 1;
                if level < 0 {
                    slash = true;
                }
                quote = true;
            }
            b'\\' => match b.get(i + 1) {
                None | Some(b'\n' | b'\r') => slash = true,
                Some(_) => {
                    i += 1;
                    quote = true;
                }
            },
            // A raw carriage return would be read back as a line end (D1853).
            b'\r' => slash = true,
            b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'[' | b']' | b'$' | b';' | b'"' => {
                quote = true;
            }
            _ => {}
        }
        i += 1;
    }
    if level != 0 {
        slash = true;
    }
    if first && b.first() == Some(&b'#') {
        quote = true;
    }
    if slash {
        Form::Backslash
    } else if quote {
        Form::Braced
    } else {
        Form::Raw
    }
}

/// Appends `s` written with the given form to `out`.
pub(crate) fn push_with_form(out: &mut String, s: &str, form: Form, first: bool) {
    match form {
        Form::Raw => out.push_str(s),
        Form::Braced => {
            out.push('{');
            out.push_str(s);
            out.push('}');
        }
        Form::Backslash => {
            for (i, c) in s.chars().enumerate() {
                match c {
                    '{' | '}' | '[' | ']' | '$' | ';' | '"' | '\\' | ' ' => {
                        out.push('\\');
                        out.push(c);
                    }
                    '#' if first && i == 0 => out.push_str("\\#"),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    '\r' => out.push_str("\\r"),
                    '\u{c}' => out.push_str("\\f"),
                    '\u{b}' => out.push_str("\\v"),
                    other => out.push(other),
                }
            }
        }
    }
}

/// Appends `s` as an element of a list that is being built in `out`: a
/// separating space unless `out` is empty, then the element.
pub(crate) fn push_element(out: &mut String, s: &str) {
    let first = out.is_empty();
    if !first {
        out.push(' ');
    }
    if s.is_empty() {
        out.push_str("{}");
        return;
    }
    push_with_form(out, s, scan_element(s, first), first);
}

/// Appends `s` as a single word of a command (not the first element of a
/// list, so a leading `#` stays raw).
pub(crate) fn push_word(out: &mut String, s: &str) {
    if s.is_empty() {
        out.push_str("{}");
    } else {
        push_with_form(out, s, scan_element(s, false), false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::strategy::Strategy;

    fn quoted(s: &str, first: bool) -> String {
        let mut out = String::new();
        push_with_form(&mut out, s, scan_element(s, first), first);
        if s.is_empty() { "{}".to_owned() } else { out }
    }

    #[test]
    fn vectors_of_the_format_document() {
        for (input, first, expected) in [
            ("", false, "{}"),
            ("e4", false, "e4"),
            ("O-O", false, "O-O"),
            ("Qxf7#", false, "Qxf7#"),
            ("e8=Q+", false, "e8=Q+"),
            ("!?", false, "!?"),
            ("::rep::_tb_include", false, "::rep::_tb_include"),
            ("King's pawn", false, "{King's pawn}"),
            ("a { b", false, "a\\ \\{\\ b"),
            ("a } b", false, "a\\ \\}\\ b"),
            ("x\\", false, "x\\\\"),
            ("a$b", false, "{a$b}"),
            ("{a}", false, "{{a}}"),
            ("a\tb", false, "{a\tb}"),
            ("a\nb", false, "{a\nb}"),
            ("#x", true, "{#x}"),
            ("#x", false, "#x"),
            ("a\"b", false, "{a\"b}"),
            ("a\\\nb", false, "a\\\\\\nb"),
        ] {
            assert_eq!(quoted(input, first), expected, "input {input:?}");
        }
    }

    #[test]
    fn split_reads_every_style() {
        let got = split_list("a {b c} \"d e\" f\\ g {} {x {y} z}").unwrap();
        let got: Vec<&str> = got.iter().map(AsRef::as_ref).collect();
        assert_eq!(got, ["a", "b c", "d e", "f g", "", "x {y} z"]);
        assert_eq!(split_list("").unwrap().len(), 0);
        assert_eq!(split_list("  \n ").unwrap().len(), 0);
    }

    #[test]
    fn braces_keep_backslashes_but_collapse_continuations() {
        let got = split_list("{a\\{b} {c\\\n   d} {e\\\\f}").unwrap();
        let got: Vec<&str> = got.iter().map(AsRef::as_ref).collect();
        assert_eq!(got, ["a\\{b", "c d", "e\\\\f"]);
    }

    #[test]
    fn bare_words_substitute_escapes() {
        let got = split_list("a\\nb \\x41\\u00e9\\101 \\q x\\").unwrap();
        let got: Vec<&str> = got.iter().map(AsRef::as_ref).collect();
        assert_eq!(got, ["a\nb", "A\u{e9}A", "q", "x\\"]);
    }

    #[test]
    fn syntax_errors_have_offsets() {
        assert_eq!(split_list("a {b").unwrap_err().offset, 2);
        assert_eq!(split_list("{a}b").unwrap_err().offset, 3);
        assert_eq!(split_list("\"a").unwrap_err().offset, 0);
        assert!(split_list("\"a\"b").is_err());
        assert!(split_list("a}").is_ok());
    }

    #[test]
    fn script_accepts_only_the_set_command() {
        let ok = parse_set_script("# c\n\nset ::tree::mask::maskSerialized {a b}\n").unwrap();
        assert_eq!(ok.value, "a b");
        let ok = parse_set_script("set tree::mask::maskSerialized a\\ b").unwrap();
        assert_eq!(ok.value, "a b");
        assert_eq!(parse_set_script("").unwrap_err(), ScriptError::Empty);
        assert_eq!(parse_set_script("  # x\n").unwrap_err(), ScriptError::Empty);
        assert_eq!(
            parse_set_script("exec ls").unwrap_err(),
            ScriptError::NotSet
        );
        for bad in [
            "set ::tree::mask::maskSerialized {a}\nexec rm",
            "set ::tree::mask::maskSerialized {a} {b}",
            "set ::tree::mask::maskSerialized {a}; source x",
            "set other {a}",
            "set ::tree::mask::maskSerialized [exec ls]",
            "set ::tree::mask::maskSerialized $x",
            "set ::tree::mask::maskSerialized \"a [b]\"",
        ] {
            assert!(
                matches!(parse_set_script(bad), Err(ScriptError::Foreign { .. })),
                "{bad}"
            );
        }
        for bad in [
            "set",
            "set ::tree::mask::maskSerialized",
            "set ::tree::mask::maskSerialized {a",
            "set ::tree::mask::maskSerialized {a}b",
        ] {
            assert!(
                matches!(parse_set_script(bad), Err(ScriptError::Syntax(_))),
                "{bad}"
            );
        }
    }

    proptest::proptest! {
        #[test]
        fn push_element_then_split_list_is_identity(
            items in proptest::collection::vec(
                proptest::collection::vec(
                    proptest::sample::select(vec![
                        '{', '}', '\\', '[', ']', '$', ';', '"', '#', ' ', '\n', '\r',
                        '\t', '\u{b}', '\u{c}', '\u{e9}', '\u{1f600}', 'a', 'z',
                    ]),
                    0..10,
                ).prop_map(|v| v.into_iter().collect::<String>()),
                0..6,
            )
        ) {
            let mut list = String::new();
            for it in &items {
                push_element(&mut list, it);
            }
            let got = split_list(&list).unwrap();
            let got: Vec<&str> = got.iter().map(AsRef::as_ref).collect();
            proptest::prop_assert_eq!(got, items.iter().map(String::as_str).collect::<Vec<_>>());
            proptest::prop_assert_eq!(count_list(&list).unwrap(), items.len());
        }
    }

    #[test]
    fn head_keeps_the_first_elements_and_counts_all() {
        let (head, n) = split_head("a b {c d} e", 2).unwrap();
        assert_eq!(head, ["a", "b"]);
        assert_eq!(n, 4);
        assert!(split_head("a {b", 2).is_err());
    }

    #[test]
    fn carriage_return_is_escaped_so_it_survives_a_line_end_pass() {
        let mut out = String::new();
        push_element(&mut out, "a\r\nb");
        assert!(!out.contains('\r'), "{out:?}");
        assert_eq!(split_list(&out).unwrap()[0], "a\r\nb");
    }

    #[test]
    fn trailing_comments_and_separators_are_fine() {
        assert!(parse_set_script("set ::tree::mask::maskSerialized {a}\n\n# end\n;\n").is_ok());
    }
}
