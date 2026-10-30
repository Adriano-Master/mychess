//! The HTML output: the fragment of the Scid inside a complete UTF-8
//! document, with the names escaped (D3363).

use super::{Crosstable, CrosstableLabels, Kind, Style, heading, table_body};

pub(super) static STYLE: Style = Style {
    kind: Kind::Html,
    start_table: "\n<p><table border=1 cellspacing=0 cellpadding=4>\n",
    end_table: "</table></p>\n",
    start_row: "<tr>",
    end_row: "</tr>",
    newline: "\n",
    heading_break: "<br>\n",
    blank_row_line: "<tr></tr>\n",
    start_col: "<td>",
    end_col: "</td>",
    start_right: "<td align=right>",
    end_right: "</td>",
    start_bold: "<th>",
    end_bold: "</th>",
};

/// Escapes `& < > "` for HTML text.
pub(super) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// The table as a complete HTML document (`<!DOCTYPE html>`, UTF-8).
pub fn render_html(table: &Crosstable, labels: &CrosstableLabels) -> String {
    let mut out = String::from("<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<title>");
    out.push_str(&escape(&table.event));
    out.push_str("</title>\n</head>\n<body>\n");
    out.push_str(&heading(table, labels, &STYLE));
    out.push_str(&table_body(table, labels, &STYLE));
    out.push_str("</body>\n</html>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn escapes_the_four_characters() {
        assert_eq!(escape("A&B <x> \"q\""), "A&amp;B &lt;x&gt; &quot;q&quot;");
    }
}
