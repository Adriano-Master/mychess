//! The LaTeX output: the document of the Scid (`scrartcl`, `booktabs`,
//! `longtable`) with the names escaped (D3363).

use super::{Crosstable, CrosstableLabels, Kind, Style, heading, table_body};

pub(super) static STYLE: Style = Style {
    kind: Kind::Latex,
    start_table: "",
    end_table: "\\end{longtable}\n",
    start_row: "",
    end_row: " \\\\",
    newline: "\n",
    heading_break: "\\\\\n",
    blank_row_line: "\\\\\n",
    start_col: "",
    end_col: " &",
    start_right: "",
    end_right: " &",
    start_bold: "\\bfseries ",
    end_bold: " &",
};

/// Escapes `& % $ # _ { } ~ ^ \ < > |` for LaTeX text. The last three would
/// print as `¡`, `¿` and `—` in the default OT1 encoding, so the document
/// also loads `fontenc` T1.
pub(super) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' | '%' | '$' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            '\\' => out.push_str("\\textbackslash{}"),
            '<' => out.push_str("\\textless{}"),
            '>' => out.push_str("\\textgreater{}"),
            '|' => out.push_str("\\textbar{}"),
            c => out.push(c),
        }
    }
    out
}

/// The table as a complete LaTeX document.
///
/// The preamble loads `fontenc` (T1) and `inputenc` (UTF-8), which cover the
/// Latin scripts. Names in Cyrillic, Greek or CJK need `xelatex` or
/// `lualatex` with suitable fonts: `pdflatex` stops on those characters.
pub fn render_latex(table: &Crosstable, labels: &CrosstableLabels) -> String {
    let mut out = String::from(
        "\\documentclass{scrartcl}\n\n\
         \\usepackage[T1]{fontenc}\n\
         \\usepackage[utf8]{inputenc}\n\
         \\usepackage{booktabs}% provides toprule, midrule, bottomrule\n\
         \\usepackage{longtable}\n\n\
         \\begin{document}\n\n\
         \\setlength{\\parindent}{0cm}\n\
         \\small\n",
    );
    out.push_str(&heading(table, labels, &STYLE));
    out.push_str(&table_body(table, labels, &STYLE));
    out.push_str("\n\\end{document}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn escapes_the_special_characters() {
        assert_eq!(escape("a&b%c$d#e_f{g}h"), "a\\&b\\%c\\$d\\#e\\_f\\{g\\}h");
        assert_eq!(
            escape("~^\\"),
            "\\textasciitilde{}\\textasciicircum{}\\textbackslash{}"
        );
        assert_eq!(escape("<a|b>"), "\\textless{}a\\textbar{}b\\textgreater{}");
    }
}
