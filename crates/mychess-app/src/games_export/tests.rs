//! Unit tests of the export of games (spec M3-9).

use std::path::Path;

use super::*;

#[test]
fn the_destination_gets_pgn_and_a_safe_name() {
    let fixed = |p: &str| fix_destination(Path::new(p)).unwrap();
    assert_eq!(fixed("a"), Path::new("a.pgn"));
    assert_eq!(fixed("a.pgn"), Path::new("a.pgn"));
    assert_eq!(fixed("A.PGN"), Path::new("A.pgn"));
    assert_eq!(fixed("a.txt"), Path::new("a.txt.pgn"));
    assert_eq!(fixed("dir/a:b|c.pgn"), Path::new("dir/a_b_c.pgn"));
    assert_eq!(fixed("dir/NUL.pgn"), Path::new("dir/_NUL.pgn"));
    assert_eq!(fixed("dir/com1"), Path::new("dir/_com1.pgn"));
}

#[test]
fn counts_say_which_scopes_exist() {
    let counts = ExportCounts {
        all: 3,
        filter: 0,
        selection: 0,
    };
    assert!(counts.available(ExportScope::All));
    assert!(!counts.available(ExportScope::Filter));
    assert!(!counts.available(ExportScope::Selection));
    assert!(counts.available(ExportScope::Current));
}

#[test]
fn notices_have_distinct_keys() {
    let notices = [
        GamesExportNotice::Exported {
            count: 1,
            name: String::new(),
        },
        GamesExportNotice::ExportedWithSkips {
            count: 1,
            skipped: 1,
            name: String::new(),
        },
        GamesExportNotice::Stopped,
        GamesExportNotice::StoppedAppended { count: 1 },
        GamesExportNotice::NothingToExport,
        GamesExportNotice::AllSkipped { skipped: 1 },
    ];
    let keys: std::collections::HashSet<_> =
        notices.iter().map(GamesExportNotice::message_key).collect();
    assert_eq!(keys.len(), notices.len());
}

#[cfg(unix)]
#[test]
fn a_name_that_is_not_unicode_is_refused() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let path = Path::new(OsStr::from_bytes(b"dir/a\xff.pgn"));
    assert_eq!(fix_destination(path), None);
}
