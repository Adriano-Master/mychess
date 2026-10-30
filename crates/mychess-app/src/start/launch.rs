//! What a file named on the command line is opened as (spec M9-4).

use std::path::{Path, PathBuf};

/// The most files one launch opens: the eight database slots (the Clipbase
/// is not a file).
pub const MAX_LAUNCH_FILES: usize = 8;

/// A file named on the command line and the way it is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchFile {
    /// A database: `.si4`/`.sg4`/`.sn4`, or PGN (anything else, as the Scid
    /// does for an unknown extension).
    Base(PathBuf),
    /// An EPD file, for the EPD window.
    Epd(PathBuf),
    /// A Polyglot opening book (`.bin`), loaded in the book window.
    Book(PathBuf),
}

impl LaunchFile {
    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            LaunchFile::Base(path) | LaunchFile::Epd(path) | LaunchFile::Book(path) => path,
        }
    }
}

/// How `path` is opened, from its extension (any case). Does not touch the
/// disk: a missing file is reported by the open itself.
#[must_use]
pub fn classify_launch_path(path: &Path) -> LaunchFile {
    if crate::epd_window::is_epd_path(path) {
        return LaunchFile::Epd(path.to_path_buf());
    }
    let is_book = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("bin"));
    if is_book {
        LaunchFile::Book(path.to_path_buf())
    } else {
        LaunchFile::Base(path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("games.pgn", "base")]
    #[case("GAMES.PGN", "base")]
    #[case("base.si4", "base")]
    #[case("base.SG4", "base")]
    #[case("noextension", "base")]
    #[case("weird.txt", "base")]
    #[case("pos.epd", "epd")]
    #[case("pos.EPD.gz", "epd")]
    #[case("book.bin", "book")]
    #[case("Livro Ação.BIN", "book")]
    fn classifies_by_extension(#[case] name: &str, #[case] kind: &str) {
        let got = match classify_launch_path(Path::new(name)) {
            LaunchFile::Base(_) => "base",
            LaunchFile::Epd(_) => "epd",
            LaunchFile::Book(_) => "book",
        };
        assert_eq!(got, kind);
    }

    #[test]
    fn keeps_the_path_untouched() {
        let path = Path::new("dir with space").join("é.pgn");
        assert_eq!(classify_launch_path(&path).path(), path);
    }
}
