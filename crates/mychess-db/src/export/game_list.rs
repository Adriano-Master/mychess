// Behaviour derived from Scid vs PC 4.27, src/index.cpp
// (IndexEntry::PrintGameInfo, 365-675) and tcl/end.tcl (openExportGList,
// saveExportGList). Copyright (c) 1999-2003 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The game list as text: one line per game from a format string
//! (decisions D3719 of `docs/specs/m7-4-exportacao.md`).
//!
//! [`format_game_line`] is the one function behind the preview and the file
//! ([`export_game_list`]). A character that is not an ASCII letter is copied;
//! a letter is a field, optionally followed by `*` (variable width) and a
//! number (width, at least 1, at most [`MAX_FIELD_WIDTH`]). Widths count
//! **characters**, not bytes (Scid counts bytes). An unknown letter is
//! ignored.
//!
//! | Letter | Field |
//! |---|---|
//! | `g` `f` `G` `L` | game number; number in the filter; file offset; record length |
//! | `w` `b` `W` `B` | White, Black (cut or padded to the width); Elo (`%4d`; with `*`: `(nnnn)` estimated when missing) |
//! | `r` | result (width 1: `1 0 = *`; else `1-0`, `0-1`, `=-=`, `*` in 3 columns) |
//! | `m` | moves (`#` when over 9 in width 1, `##` when over 99 in width 2) |
//! | `y` `d` `E` | year, date, event date (unknown tail removed) |
//! | `n` `s` `e` `c` | round, site, event, country (last 3 letters of the site) |
//! | `o` `O` `T` `F` | ECO, stored line text, stored line code and text, final material |
//! | `S` `D` | starts from a position; deleted |
//! | `M` `U` | marked flags (custom flags by the first letter of their description) |
//! | `C` `V` `A` | comment, variation, NAG counts |

use std::path::{Path, PathBuf};

use super::text_file::{Append, Job, Step, push_fmt, run, with_extension_ci};
use super::{ExportError, ExportReport, ExportSkipReason};
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::{
    GameFlags, Header, IndexEntry, MaterialSignature, NameBase, NameKind, bytes_to_string,
    stored_lines::STORED_LINES,
};
use crate::source::GameSource;

/// The widest field; larger widths are cut to it.
pub const MAX_FIELD_WIDTH: usize = 1000;

/// The default format of the Scid dialog.
pub const DEFAULT_LIST_FORMAT: &str = "g6: w13 W4  b13 B4  r3:m2 y4 s11 o4";

const KNOWN_LETTERS: &str = "fgGLmOFrydEnwbWBseScoSDUMCVAT";

/// Whether `format` has at least one field letter this module knows.
pub fn format_has_fields(format: &str) -> bool {
    format.chars().any(|c| KNOWN_LETTERS.contains(c))
}

/// The first letter of each custom flag description of a database (what the
/// `M` and `U` fields show for the flags 1 to 6).
pub fn custom_flag_chars(header: &Header) -> [Option<char>; 6] {
    let mut out = [None; 6];
    for (slot, desc) in out.iter_mut().zip(header.custom_flag_descriptions()) {
        *slot = bytes_to_string(desc).chars().next();
    }
    out
}

fn pad(out: &mut String, text: &str, width: Option<usize>) {
    match width {
        None => out.push_str(text),
        Some(w) => {
            let mut n = 0usize;
            for c in text.chars().take(w) {
                out.push(c);
                n += 1;
            }
            out.extend(std::iter::repeat_n(' ', w - n));
        }
    }
}

fn int(out: &mut String, value: u64, width: Option<usize>) {
    let w = width.unwrap_or(1);
    push_fmt(out, format_args!("{value:>w$}"));
}

fn material(m: &MaterialSignature, white: bool) -> String {
    let (queens, rooks, bishops, knights, pawns) = if white {
        (
            m.white_queens,
            m.white_rooks,
            m.white_bishops,
            m.white_knights,
            m.white_pawns,
        )
    } else {
        (
            m.black_queens,
            m.black_rooks,
            m.black_bishops,
            m.black_knights,
            m.black_pawns,
        )
    };
    let mut text = String::new();
    for (count, big, small) in [
        (queens, 'Q', 'q'),
        (rooks, 'R', 'r'),
        (bishops, 'B', 'b'),
        (knights, 'N', 'n'),
    ] {
        text.extend(std::iter::repeat_n(big, usize::from(count / 2)));
        if count % 2 == 1 {
            text.push(small);
        }
    }
    if pawns > 0 {
        text.push(char::from(b'0'.saturating_add(pawns)));
    }
    text
}

fn name_of(names: &NameBase, kind: NameKind, id: u32) -> String {
    names
        .get(kind, id)
        .map(|b| bytes_to_string(b).into_owned())
        .unwrap_or_default()
}

fn stored_line_text(code: u8) -> &'static str {
    usize::from(code)
        .checked_sub(1)
        .and_then(|i| STORED_LINES.get(i))
        .copied()
        .unwrap_or("")
}

/// Formats one line of the game list as Scid's `PrintGameInfo`, without
/// Elo estimates (see [`format_game_line_with`]).
///
/// `numbers` is (game number, number in the filter). `custom_flags` are the
/// letters that stand for the custom flags 1 to 6 (see
/// [`custom_flag_chars`]).
///
pub fn format_game_line(
    entry: &IndexEntry,
    names: &NameBase,
    numbers: (u32, u32),
    format: &str,
    custom_flags: [Option<char>; 6],
) -> String {
    format_game_line_with(entry, names, numbers, format, custom_flags, &|_| None)
}

/// [`format_game_line`] with a source of estimated Elo ratings by player
/// name, used by `W*` and `B*` when the game has none (Scid takes it from
/// the spelling file).
#[allow(clippy::too_many_lines)] // one flat `match` over the field letters
pub fn format_game_line_with(
    entry: &IndexEntry,
    names: &NameBase,
    numbers: (u32, u32),
    format: &str,
    custom_flags: [Option<char>; 6],
    estimate: &dyn Fn(&str) -> Option<u16>,
) -> String {
    let mut out = String::new();
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        i += 1;
        if !c.is_ascii_alphabetic() {
            out.push(c);
            continue;
        }
        let mut variable = false;
        if chars.get(i) == Some(&'*') {
            variable = true;
            i += 1;
        }
        let mut width = 0usize;
        while let Some(d) = chars.get(i).and_then(|c| c.to_digit(10)) {
            width = width.saturating_mul(10).saturating_add(d as usize);
            i += 1;
        }
        if width == 0 {
            width = 1;
        }
        let width = width.min(MAX_FIELD_WIDTH);
        // `None` means variable width.
        let w = if variable { None } else { Some(width) };
        let text = |kind, id| name_of(names, kind, id);
        match c {
            'f' => int(&mut out, u64::from(numbers.1), w),
            'g' => int(&mut out, u64::from(numbers.0), w),
            'G' => int(&mut out, u64::from(entry.offset), w),
            'L' => int(&mut out, u64::from(entry.length), w),
            'm' => {
                let m = u32::from(entry.num_half_moves).div_ceil(2);
                if w == Some(2) && m > 99 {
                    out.push_str("##");
                } else if w == Some(1) && m > 9 {
                    out.push('#');
                } else {
                    int(&mut out, u64::from(m), w);
                }
            }
            'O' => pad(&mut out, stored_line_text(entry.stored_line_code), w),
            'T' => push_fmt(
                &mut out,
                format_args!(
                    "{:>3} {}",
                    entry.stored_line_code,
                    stored_line_text(entry.stored_line_code)
                ),
            ),
            'F' => {
                let m = entry.final_material();
                out.push_str(&material(&m, true));
                out.push(':');
                out.push_str(&material(&m, false));
            }
            'r' => {
                let (ch, long) = match entry.result {
                    mychess_core::GameResult::White => ('1', "1-0"),
                    mychess_core::GameResult::Black => ('0', "0-1"),
                    mychess_core::GameResult::Draw => ('=', "=-="),
                    _ => ('*', "*"),
                };
                if w == Some(1) {
                    out.push(ch);
                } else {
                    push_fmt(&mut out, format_args!("{long:<3}"));
                }
            }
            'y' => int(&mut out, u64::from(entry.date.year().unwrap_or(0)), Some(4)),
            'd' => pad(&mut out, &entry.date.short(), w),
            'E' => pad(&mut out, &entry.event_date.short(), w),
            'n' => pad(&mut out, &text(NameKind::Round, entry.round_id), w),
            'w' => pad(&mut out, &text(NameKind::Player, entry.white_id), w),
            'b' => pad(&mut out, &text(NameKind::Player, entry.black_id), w),
            'W' | 'B' => {
                let (elo, id) = if c == 'W' {
                    (entry.white_elo, entry.white_id)
                } else {
                    (entry.black_elo, entry.black_id)
                };
                if variable {
                    if elo == 0 {
                        match estimate(&text(NameKind::Player, id)).filter(|e| *e > 0) {
                            Some(e) => push_fmt(&mut out, format_args!("({e:>4})")),
                            None => out.push_str("     "),
                        }
                    } else {
                        push_fmt(&mut out, format_args!("{elo:>4} "));
                    }
                } else {
                    push_fmt(&mut out, format_args!("{elo:>4}"));
                }
            }
            's' => pad(&mut out, &text(NameKind::Site, entry.site_id), w),
            'e' => pad(&mut out, &text(NameKind::Event, entry.event_id), w),
            'c' => {
                let site: Vec<char> = text(NameKind::Site, entry.site_id).chars().collect();
                let country = site
                    .len()
                    .checked_sub(4)
                    .filter(|_| site.len() > 3)
                    .and_then(|start| {
                        let sp = site.get(start)?;
                        let first = site.get(start + 1)?;
                        (*sp == ' ' && first.is_ascii_uppercase()).then(|| {
                            site.get(start + 1..)
                                .unwrap_or(&[])
                                .iter()
                                .collect::<String>()
                        })
                    });
                pad(&mut out, country.as_deref().unwrap_or("?"), w);
            }
            'o' => {
                let eco = entry.eco().map(|e| e.to_string()).unwrap_or_default();
                pad(&mut out, &eco, w);
            }
            'S' => out.push(if entry.flags.contains(GameFlags::START) {
                'S'
            } else {
                ' '
            }),
            'D' => out.push(if entry.flags.contains(GameFlags::DELETE) {
                'D'
            } else {
                ' '
            }),
            'M' | 'U' => {
                let letters = entry.flags.letters();
                let flags: String = letters
                    .strip_prefix('D')
                    .unwrap_or(&letters)
                    .chars()
                    .map(|f| {
                        f.to_digit(10)
                            .and_then(|d| usize::try_from(d).ok())
                            .and_then(|d| d.checked_sub(1))
                            .and_then(|d| custom_flags.get(d).copied().flatten())
                            .unwrap_or(f)
                    })
                    .collect();
                pad(&mut out, &flags, w);
            }
            'C' | 'V' | 'A' => {
                let (count, letter) = match c {
                    'C' => (entry.comment_count, 'C'),
                    'V' => (entry.variation_count, 'V'),
                    _ => (entry.nag_count, 'A'),
                };
                if w == Some(1) {
                    out.push(if count > 0 { letter } else { ' ' });
                } else if count > 0 {
                    push_fmt(&mut out, format_args!("{count:>2}"));
                }
            }
            _ => {}
        }
    }
    out
}

/// A source of estimated Elo ratings by player name.
pub type EloEstimate = std::sync::Arc<dyn Fn(&str) -> Option<u16> + Send + Sync>;

/// Options of [`export_game_list`].
#[derive(Clone)]
pub struct GameListOptions {
    /// The format string (see the module documentation).
    pub format: String,
    /// Letters for the custom flags (default: from the database header).
    pub custom_flags: Option<[Option<char>; 6]>,
    /// Elo estimate by player name, for `W*` and `B*`.
    pub estimate: Option<EloEstimate>,
    /// Add to the end of an existing file.
    pub append: bool,
    /// Throw the output away when cancelled (default `true`).
    pub discard_on_cancel: bool,
}

impl std::fmt::Debug for GameListOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameListOptions")
            .field("format", &self.format)
            .field("custom_flags", &self.custom_flags)
            .field("append", &self.append)
            .field("discard_on_cancel", &self.discard_on_cancel)
            .finish_non_exhaustive()
    }
}

impl Default for GameListOptions {
    fn default() -> GameListOptions {
        GameListOptions {
            format: DEFAULT_LIST_FORMAT.to_owned(),
            custom_flags: None,
            estimate: None,
            append: false,
            discard_on_cancel: true,
        }
    }
}

/// Writes one line per game of `numbers` (game numbers, base 1, in the order
/// of the filter) to the text file `dest` (`.txt` is added when the name has
/// no such extension), UTF-8 without BOM, lines ended by `\n`.
///
/// # Errors
///
/// [`ExportError::EmptyFormat`] when the format has no known field (before
/// any file is touched); [`ExportError::NothingToExport`] when no game was
/// written (no file is created or changed); [`ExportError::Io`] otherwise.
pub fn export_game_list<S, I>(
    db: &S,
    numbers: I,
    dest: &Path,
    options: &GameListOptions,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<ExportReport, ExportError>
where
    S: GameSource + ?Sized,
    I: IntoIterator<Item = u32>,
{
    if !format_has_fields(&options.format) {
        return Err(ExportError::EmptyFormat);
    }
    let numbers = numbers.into_iter();
    let dest: PathBuf = with_extension_ci(dest, "txt");
    let custom = options
        .custom_flags
        .unwrap_or_else(|| custom_flag_chars(db.header()));
    let estimate = |name: &str| options.estimate.as_ref().and_then(|f| f(name));
    let (low, high) = numbers.size_hint();
    let job = Job {
        dest: &dest,
        start: "",
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
                buf.push_str(&format_game_line_with(
                    &entry,
                    db.names(),
                    (number, position),
                    &options.format,
                    custom,
                    &estimate,
                ));
                buf.push('\n');
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
