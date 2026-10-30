//! Text escaping for the HTML and LaTeX writers.

/// Appends formatted text to `out` (a `String` never fails to grow).
pub(crate) fn push_fmt(out: &mut String, args: std::fmt::Arguments<'_>) {
    let _ = std::fmt::Write::write_fmt(out, args);
}

/// Appends `text` to `out` escaped for HTML text and attribute values:
/// `& < > " '`. Control characters other than `\n` and `\t` are dropped
/// (they are not valid in HTML and would corrupt the JSON/XML around it).
pub fn push_html_escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
}

/// [`push_html_escaped`] into a new string.
pub fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    push_html_escaped(&mut out, text);
    out
}

/// Appends `text` escaped for LaTeX text mode: `# % & _ $ { } ~ ^ \` and
/// `<`/`>` (as `$<$`, `$>$`, decision D3709, rule 14 of the specification).
/// Control characters other than `\n` and `\t` are dropped.
pub fn push_latex_escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '#' => out.push_str("\\#"),
            '%' => out.push_str("\\%"),
            '&' => out.push_str("\\&"),
            '_' => out.push_str("\\_"),
            '$' => out.push_str("\\$"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            '\\' => out.push_str("\\textbackslash{}"),
            '<' => out.push_str("$<$"),
            '>' => out.push_str("$>$"),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
}

/// [`push_latex_escaped`] into a new string.
pub fn latex_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    push_latex_escaped(&mut out, text);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escapes_the_five_characters_and_drops_controls() {
        assert_eq!(
            html_escape("<a href=\"x\">'&'</a>\u{0}\u{7}\n"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;\n"
        );
    }

    #[test]
    fn latex_escapes_every_special_character() {
        assert_eq!(
            latex_escape("# % & _ $ { } ~ ^ \\ < >"),
            "\\# \\% \\& \\_ \\$ \\{ \\} \\textasciitilde{} \\textasciicircum{} \\textbackslash{} $<$ $>$"
        );
    }
}
