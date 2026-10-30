//! The plain text output: the format of the Scid's window and of its "Save
//! as text".

use super::{Crosstable, CrosstableLabels, Kind, Style, heading, table_body};

pub(super) static STYLE: Style = Style {
    kind: Kind::Plain,
    start_table: "",
    end_table: "",
    start_row: "",
    end_row: "",
    newline: "\n",
    heading_break: "\n",
    blank_row_line: "\n",
    start_col: "",
    end_col: "",
    start_right: "",
    end_right: "",
    start_bold: "",
    end_bold: "",
};

/// The table as plain text, like the Scid: the heading (event, site and
/// dates, average rating) and the table, `\n` line ends, `½` for a draw.
pub fn render_text(table: &Crosstable, labels: &CrosstableLabels) -> String {
    let mut out = heading(table, labels, &STYLE);
    out.push_str(&table_body(table, labels, &STYLE));
    out
}
