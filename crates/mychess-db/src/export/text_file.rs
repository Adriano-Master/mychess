//! The loop shared by the single-file text exports (HTML, LaTeX, EPD, game
//! list, CSV): temporary file in the destination directory, `rename` at the
//! end, progress, cancellation, skipped items.

use std::borrow::Cow;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{ExportError, ExportOptions, ExportSkipReason, REPORT_EVERY, io_error, open_output};
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, Ticker};

/// How an existing file is extended.
#[derive(Debug, Clone, Copy)]
pub(super) enum Append {
    /// Create a new file (or replace the old one when the job ends).
    No,
    /// Copy the old file, then add to its end.
    ToEnd,
    /// Copy the old file up to the last line that starts with this text
    /// (any letter case, leading white space ignored); the end text goes
    /// there. The text is lower case.
    BeforeMarker(&'static str),
}

pub(super) struct Job<'a> {
    pub(super) dest: &'a Path,
    pub(super) start: &'a str,
    pub(super) end: &'a str,
    pub(super) append: Append,
    pub(super) discard_on_cancel: bool,
    /// Throw the output away when no item was written.
    pub(super) discard_if_empty: bool,
    pub(super) total: u64,
}

/// What `render` did with an item.
pub(super) enum Step {
    /// Text was added to the buffer.
    Written,
    /// The item was left out for this reason (its game number, 0 if none).
    Skipped(u32, ExportSkipReason),
    /// The item was left out on purpose (not an error, not counted).
    Ignored,
}

pub(super) struct JobResult {
    pub(super) written: u64,
    pub(super) skipped: Vec<(u32, ExportSkipReason)>,
    pub(super) interrupted: bool,
    /// The output was thrown away; `dest` is as it was.
    pub(super) discarded: bool,
    pub(super) elapsed: Duration,
}

/// Appends formatted text to `out` (a `String` never fails to grow).
pub(super) fn push_fmt(out: &mut String, args: std::fmt::Arguments<'_>) {
    let _ = std::fmt::Write::write_fmt(out, args);
}

/// Position of the last line of `text` that starts (case-insensitively, after
/// white space) with `marker_lower`.
///
/// A byte order mark at the start of the file is not part of the first line:
/// when that line is the marker, the position is just after the mark, so the
/// mark stays in the part that is kept.
fn find_marker(text: &str, marker_lower: &str) -> Option<usize> {
    const BOM: char = '\u{feff}';
    let mut offset = 0usize;
    let mut found = None;
    for (n, raw) in text.split_inclusive('\n').enumerate() {
        let (line, skip) = match raw.strip_prefix(BOM) {
            Some(rest) if n == 0 => (rest, BOM.len_utf8()),
            _ => (raw, 0),
        };
        if line
            .trim_start()
            .get(..marker_lower.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(marker_lower))
        {
            found = Some(offset + skip);
        }
        offset += raw.len();
    }
    found
}

/// Whether the first line break of `bytes` is `\r\n`.
fn starts_with_crlf(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .position(|b| *b == b'\n')
        .is_some_and(|i| i > 0 && bytes.get(i - 1) == Some(&b'\r'))
}

/// `text` (made with `\n`) in the line ending of the file it is added to.
fn in_style(text: &str, crlf: bool) -> Cow<'_, str> {
    if crlf && text.contains('\n') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\n', "\r\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// Runs the job. `render` appends the text of one item to the buffer it is
/// given (cleared before each call).
#[allow(clippy::too_many_lines)] // one pass: read the old file, copy, add, finish
pub(super) fn run<T>(
    items: impl Iterator<Item = T>,
    job: &Job<'_>,
    marker_name: &'static str,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
    mut render: impl FnMut(T, &mut String) -> Step,
) -> Result<JobResult, ExportError> {
    let started = Instant::now();
    if job.dest.file_name().is_none() {
        return Err(ExportError::NoFileName {
            path: job.dest.to_owned(),
        });
    }
    // The old contents (append modes) are read before the temporary file is
    // created, so a refusal leaves nothing behind.
    let mut prefix: Option<String> = None;
    // The new text follows the line ending of the old file (LF or CRLF).
    let mut crlf = false;
    let mut copy_from: Option<File> = None;
    match job.append {
        Append::No => {}
        Append::ToEnd => {
            copy_from = Some(File::open(job.dest).map_err(io_error(job.dest))?);
        }
        Append::BeforeMarker(marker) => {
            let bytes = fs::read(job.dest).map_err(io_error(job.dest))?;
            let text = String::from_utf8(bytes).map_err(|_| ExportError::NotUtf8 {
                path: job.dest.to_owned(),
            })?;
            let at = find_marker(&text, marker).ok_or_else(|| ExportError::NoAppendMarker {
                path: job.dest.to_owned(),
                marker: marker_name,
            })?;
            crlf = starts_with_crlf(text.as_bytes());
            prefix = Some(text.get(..at).unwrap_or("").to_owned());
        }
    }
    let (mut output, _, _) = open_output(job.dest, &ExportOptions::default())?;
    let result = (|| -> Result<JobResult, ExportError> {
        if let Some(mut file) = copy_from {
            let mut buf = vec![0u8; 64 * 1024];
            let mut first = true;
            loop {
                let n = file.read(&mut buf).map_err(io_error(job.dest))?;
                if n == 0 {
                    break;
                }
                if first {
                    crlf = starts_with_crlf(buf.get(..n).unwrap_or(&[]));
                    first = false;
                }
                output.write(buf.get(..n).unwrap_or(&[]))?;
            }
        }
        if let Some(prefix) = &prefix {
            output.write(prefix.as_bytes())?;
        } else if matches!(job.append, Append::No) {
            output.write(job.start.as_bytes())?;
        }
        let mut written = 0u64;
        let mut skipped = Vec::new();
        let mut interrupted = false;
        let mut ticker = Ticker::new(REPORT_EVERY, CHECK_INTERVAL);
        let mut handled = 0u64;
        let mut buf = String::new();
        let mut items = items;
        loop {
            if cancel.is_cancelled() {
                interrupted = true;
                break;
            }
            let Some(item) = items.next() else {
                break;
            };
            buf.clear();
            match render(item, &mut buf) {
                Step::Written => {
                    output.write(in_style(&buf, crlf).as_bytes())?;
                    written += 1;
                }
                Step::Skipped(number, reason) => skipped.push((number, reason)),
                Step::Ignored => {}
            }
            handled += 1;
            if ticker.tick() {
                progress.progress(handled, job.total.max(handled));
            }
        }
        progress.progress(handled, job.total.max(handled));
        let discard =
            (interrupted && job.discard_on_cancel) || (written == 0 && job.discard_if_empty);
        if !discard {
            output.write(in_style(job.end, crlf).as_bytes())?;
        }
        Ok(JobResult {
            written,
            skipped,
            interrupted,
            discarded: discard,
            elapsed: Duration::ZERO,
        })
    })();
    match result {
        Ok(mut r) => {
            if r.discarded {
                output.abandon();
            } else {
                output.finish()?;
            }
            r.elapsed = started.elapsed();
            Ok(r)
        }
        Err(e) => {
            output.abandon();
            Err(e)
        }
    }
}

/// `path` with `.ext` added unless the name already ends in it (any letter
/// case). A path without a file name is returned as it is.
pub(super) fn with_extension_ci(path: &Path, ext: &str) -> PathBuf {
    let Some(name) = path.file_name() else {
        return path.to_owned();
    };
    let dotted = format!(".{ext}");
    if name.to_string_lossy().to_lowercase().ends_with(&dotted) {
        return path.to_owned();
    }
    let mut name = name.to_owned();
    name.push(dotted);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_the_last_matching_line_in_any_case() {
        let text = "<body>\n  </BODY>\nx\n</body>\n</html>\n";
        assert_eq!(
            find_marker(text, "</body>"),
            Some(text.rfind("</body>").unwrap())
        );
        assert_eq!(find_marker("a\n  </BODY>\n", "</body>"), Some(2));
        assert_eq!(find_marker("nothing\n", "</body>"), None);
    }

    #[test]
    fn marker_after_a_byte_order_mark_keeps_the_mark_in_the_prefix() {
        let text = "\u{feff}</body>\r\n</html>\r\n";
        assert_eq!(find_marker(text, "</body>"), Some(3));
        assert_eq!(find_marker("\u{feff}x\r\n</BODY>", "</body>"), Some(6));
    }

    #[test]
    fn line_ending_is_detected_and_applied() {
        assert!(starts_with_crlf(b"a\r\nb\n"));
        assert!(!starts_with_crlf(b"a\nb\r\n"));
        assert!(!starts_with_crlf(b"\nb"));
        assert!(!starts_with_crlf(b"no break"));
        assert_eq!(in_style("a\nb\r\nc", true), "a\r\nb\r\nc");
        assert_eq!(in_style("a\nb", false), "a\nb");
    }

    #[test]
    fn extension_is_added_without_looking_at_the_case() {
        assert_eq!(
            with_extension_ci(Path::new("a"), "html"),
            Path::new("a.html")
        );
        assert_eq!(
            with_extension_ci(Path::new("a.HTML"), "html"),
            Path::new("a.HTML")
        );
        assert_eq!(
            with_extension_ci(Path::new("a.txt"), "tex"),
            Path::new("a.txt.tex")
        );
    }
}
