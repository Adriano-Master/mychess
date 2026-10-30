//! The game list as CSV (decision D3720 of `docs/specs/m7-4-exportacao.md`).
//!
//! One row per game of the filter, with the columns the caller chooses
//! ([`CsvColumn`], in the order of the list), fields quoted as RFC 4180 asks
//! (when they hold the separator, `"`, `\r` or `\n`; `"` is doubled), UTF-8,
//! lines ended by `\n`, an optional byte order mark for spreadsheets and an
//! optional header with the (translated) column names.
//!
//! # Spreadsheet formulas
//!
//! Names, events and sites come from third-party PGN files, and a spreadsheet
//! runs a cell that starts with `=`, `+`, `-` or `@` (or a tab or a carriage
//! return) as a formula. With [`CsvOptions::neutralize_formulas`] (on by
//! default) the text columns that start with one of those characters (and are
//! longer than that single character, which is plain text alone) get a
//! leading `'`, the convention of the spreadsheets for "this is text". The
//! numeric columns are made by the exporter and are never touched.

use std::path::{Path, PathBuf};

use super::text_file::{Append, Job, Step, run, with_extension_ci};
use super::{ExportError, ExportReport, ExportSkipReason};
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::{IndexEntry, NameBase, NameKind, bytes_to_string};
use crate::source::GameSource;

/// A column of the CSV file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CsvColumn {
    /// Game number in the database.
    Number,
    /// Position in the filter.
    FilterNumber,
    /// White player.
    White,
    /// Black player.
    Black,
    /// White Elo (empty when none).
    WhiteElo,
    /// Black Elo (empty when none).
    BlackElo,
    /// Result (`1-0`, `0-1`, `1/2-1/2`, `*`).
    Result,
    /// Date (`2024.05.??` without the unknown tail).
    Date,
    /// Year (empty when unknown).
    Year,
    /// Event name.
    Event,
    /// Site name.
    Site,
    /// Round.
    Round,
    /// ECO code (extended form).
    Eco,
    /// Number of moves.
    Moves,
    /// Letters of the flags set (`DWBMENPTKQ!?U123456`).
    Flags,
}

/// Options of [`export_game_csv`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvOptions {
    /// The columns, in order. Must not be empty.
    pub columns: Vec<CsvColumn>,
    /// The header row (one name per column, same length as `columns`), or
    /// `None` for no header.
    pub header: Option<Vec<String>>,
    /// Field separator: `,` or `;`.
    pub separator: char,
    /// Write a UTF-8 byte order mark first.
    pub bom: bool,
    /// Add to the end of an existing file (no header, no BOM).
    pub append: bool,
    /// Throw the output away when cancelled (default `true`).
    pub discard_on_cancel: bool,
    /// Put a `'` before the text fields (players, event, site, round) that
    /// start with `=`, `+`, `-`, `@`, a tab or a carriage return, so that a
    /// spreadsheet does not run them as formulas (default `true`).
    pub neutralize_formulas: bool,
}

impl Default for CsvOptions {
    fn default() -> CsvOptions {
        CsvOptions {
            columns: vec![
                CsvColumn::Number,
                CsvColumn::White,
                CsvColumn::Black,
                CsvColumn::Result,
                CsvColumn::Date,
            ],
            header: None,
            separator: ',',
            bom: false,
            append: false,
            discard_on_cancel: true,
            neutralize_formulas: true,
        }
    }
}

/// Appends `field` to `out`, quoted if RFC 4180 requires it.
pub fn push_csv_field(out: &mut String, field: &str, separator: char) {
    if field.contains([separator, '"', '\r', '\n']) {
        out.push('"');
        for c in field.chars() {
            if c == '"' {
                out.push('"');
            }
            out.push(c);
        }
        out.push('"');
    } else {
        out.push_str(field);
    }
}

/// `text` with a `'` in front when a spreadsheet would take it for a formula.
///
/// A field that is exactly one of `=`, `+`, `-` or `@` is left alone: alone,
/// the character is plain text for Excel and `LibreOffice` (and `-` is a common
/// PGN `Round`). A tab or a carriage return is always neutralized.
pub fn neutralize_formula(text: &str) -> std::borrow::Cow<'_, str> {
    if matches!(text, "=" | "+" | "-" | "@") {
        return std::borrow::Cow::Borrowed(text);
    }
    if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        std::borrow::Cow::Owned(format!("'{text}"))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Whether the column holds text taken from the game (not made by us).
fn is_text(col: CsvColumn) -> bool {
    matches!(
        col,
        CsvColumn::White | CsvColumn::Black | CsvColumn::Event | CsvColumn::Site | CsvColumn::Round
    )
}

fn name(names: &NameBase, kind: NameKind, id: u32) -> String {
    names
        .get(kind, id)
        .map(|b| bytes_to_string(b).into_owned())
        .unwrap_or_default()
}

fn cell(col: CsvColumn, entry: &IndexEntry, names: &NameBase, numbers: (u32, u32)) -> String {
    let elo = |e: u16| if e == 0 { String::new() } else { e.to_string() };
    match col {
        CsvColumn::Number => numbers.0.to_string(),
        CsvColumn::FilterNumber => numbers.1.to_string(),
        CsvColumn::White => name(names, NameKind::Player, entry.white_id),
        CsvColumn::Black => name(names, NameKind::Player, entry.black_id),
        CsvColumn::WhiteElo => elo(entry.white_elo),
        CsvColumn::BlackElo => elo(entry.black_elo),
        CsvColumn::Result => entry.result.as_str().to_owned(),
        CsvColumn::Date => entry.date.short(),
        CsvColumn::Year => entry.date.year().map(|y| y.to_string()).unwrap_or_default(),
        CsvColumn::Event => name(names, NameKind::Event, entry.event_id),
        CsvColumn::Site => name(names, NameKind::Site, entry.site_id),
        CsvColumn::Round => name(names, NameKind::Round, entry.round_id),
        CsvColumn::Eco => entry.eco().map(|e| e.to_string()).unwrap_or_default(),
        CsvColumn::Moves => u32::from(entry.num_half_moves).div_ceil(2).to_string(),
        CsvColumn::Flags => entry.flags.letters(),
    }
}

/// One CSV row (with the final `\n`) for `entry`; `neutralize` is
/// [`CsvOptions::neutralize_formulas`].
pub fn format_csv_row(
    entry: &IndexEntry,
    names: &NameBase,
    numbers: (u32, u32),
    columns: &[CsvColumn],
    separator: char,
    neutralize: bool,
) -> String {
    let mut out = String::new();
    for (i, col) in columns.iter().enumerate() {
        if i > 0 {
            out.push(separator);
        }
        let text = cell(*col, entry, names, numbers);
        if neutralize && is_text(*col) {
            push_csv_field(&mut out, &neutralize_formula(&text), separator);
        } else {
            push_csv_field(&mut out, &text, separator);
        }
    }
    out.push('\n');
    out
}

/// Writes the games `numbers` (game numbers, base 1, in the order of the
/// filter) to the CSV file `dest` (`.csv` is added when missing).
///
/// # Errors
///
/// [`ExportError::InvalidOptions`] for no columns, a header of the wrong
/// length or a separator other than `,` and `;`;
/// [`ExportError::NothingToExport`] when no game was written (no file is
/// created or changed); [`ExportError::Io`] otherwise.
pub fn export_game_csv<S, I>(
    db: &S,
    numbers: I,
    dest: &Path,
    options: &CsvOptions,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<ExportReport, ExportError>
where
    S: GameSource + ?Sized,
    I: IntoIterator<Item = u32>,
{
    if options.columns.is_empty() {
        return Err(ExportError::InvalidOptions("no columns"));
    }
    if options
        .header
        .as_ref()
        .is_some_and(|h| h.len() != options.columns.len())
    {
        return Err(ExportError::InvalidOptions(
            "header and columns differ in length",
        ));
    }
    if !matches!(options.separator, ',' | ';') {
        return Err(ExportError::InvalidOptions("separator must be , or ;"));
    }
    let numbers = numbers.into_iter();
    let dest: PathBuf = with_extension_ci(dest, "csv");
    let mut start = String::new();
    if !options.append {
        if options.bom {
            start.push('\u{feff}');
        }
        if let Some(header) = &options.header {
            for (i, h) in header.iter().enumerate() {
                if i > 0 {
                    start.push(options.separator);
                }
                push_csv_field(&mut start, h, options.separator);
            }
            start.push('\n');
        }
    }
    let (low, high) = numbers.size_hint();
    let job = Job {
        dest: &dest,
        start: &start,
        end: "",
        append: if options.append {
            Append::ToEnd
        } else {
            Append::No
        },
        discard_on_cancel: options.discard_on_cancel,
        discard_if_empty: true,
        total: u64::try_from(high.unwrap_or(low)).unwrap_or(0),
    };
    let mut position = 0u32;
    let result = run(numbers, &job, "", progress, cancel, |number, buf| {
        position = position.saturating_add(1);
        match db.entry(number) {
            Ok(entry) => {
                buf.push_str(&format_csv_row(
                    &entry,
                    db.names(),
                    (number, position),
                    &options.columns,
                    options.separator,
                    options.neutralize_formulas,
                ));
                Step::Written
            }
            Err(e) => Step::Skipped(number, ExportSkipReason::Unreadable(e.to_string())),
        }
    })?;
    if result.written == 0 && !result.interrupted {
        return Err(ExportError::NothingToExport);
    }
    Ok(ExportReport {
        path: dest,
        written: if result.discarded { 0 } else { result.written },
        skipped: result.skipped,
        interrupted: result.interrupted,
        unrepresentable: 0,
        elapsed: result.elapsed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_triggers_get_a_quote_and_other_text_is_untouched() {
        for trigger in [
            "=1+1",
            "+1",
            "-1",
            "@SUM(A1)",
            "\tx",
            "\rx",
            "=HYPERLINK(\"u\")",
        ] {
            assert_eq!(neutralize_formula(trigger), format!("'{trigger}"));
        }
        for plain in [
            "", "Kasparov", "1-0", "a=b", "'=x", " =x", "-", "+", "=", "@",
        ] {
            assert_eq!(neutralize_formula(plain), plain);
        }
    }

    #[test]
    fn fields_are_quoted_only_when_needed() {
        let mut s = String::new();
        push_csv_field(&mut s, "plain", ',');
        s.push('|');
        push_csv_field(&mut s, "a,b", ',');
        s.push('|');
        push_csv_field(&mut s, "say \"hi\"", ',');
        s.push('|');
        push_csv_field(&mut s, "two\nlines", ',');
        s.push('|');
        push_csv_field(&mut s, "a,b", ';');
        s.push('|');
        push_csv_field(&mut s, "a;b", ';');
        assert_eq!(
            s,
            "plain|\"a,b\"|\"say \"\"hi\"\"\"|\"two\nlines\"|a,b|\"a;b\""
        );
    }
}
