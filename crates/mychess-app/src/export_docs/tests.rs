//! Unit tests of the extra formats (spec M7-4, slice b).

use std::path::Path;

use super::*;

#[test]
fn the_destination_gets_the_extension_of_the_format() {
    let fixed = |p: &str, f: ExportFormat| {
        fix_destination_ext(Path::new(p), f.accepted_extensions(), f.extension()).unwrap()
    };
    assert_eq!(fixed("a", ExportFormat::Html), Path::new("a.html"));
    assert_eq!(fixed("a.HTM", ExportFormat::Html), Path::new("a.html"));
    assert_eq!(fixed("a.html", ExportFormat::Viewer), Path::new("a.html"));
    assert_eq!(fixed("a.ltx", ExportFormat::Latex), Path::new("a.tex"));
    assert_eq!(fixed("a.TEX", ExportFormat::Latex), Path::new("a.tex"));
    assert_eq!(fixed("a.txt", ExportFormat::Epd), Path::new("a.txt.epd"));
    assert_eq!(fixed("a", ExportFormat::List), Path::new("a.txt"));
    assert_eq!(
        fixed("dir/a:b.csv", ExportFormat::Csv),
        Path::new("dir/a_b.csv")
    );
    assert_eq!(
        fixed("dir/NUL", ExportFormat::Csv),
        Path::new("dir/_NUL.csv")
    );
}

#[test]
fn every_csv_column_has_a_distinct_key() {
    let keys: std::collections::HashSet<_> =
        CSV_COLUMNS.iter().map(|c| csv_column_key(*c)).collect();
    assert_eq!(keys.len(), CSV_COLUMNS.len());
}

#[test]
fn only_the_viewer_cannot_append_and_only_lists_need_a_database() {
    for format in ExportFormat::ALL {
        assert_eq!(format.can_append(), *format != ExportFormat::Viewer);
        assert_eq!(
            format.needs_database(),
            matches!(format, ExportFormat::List | ExportFormat::Csv)
        );
    }
}

#[cfg(unix)]
#[test]
fn a_name_that_is_not_unicode_is_refused() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let path = Path::new(OsStr::from_bytes(b"dir/a\xff"));
    assert_eq!(fix_destination_ext(path, &["html"], "html"), None);
}
