//! The favorite reports: a name and the moves of a position, kept in
//! `report_favorites.toml` (spec M7-2, section 8, decisions D3618, D3619).
//!
//! The model ([`Favorites`]) is pure; [`parse_favorites`] and
//! [`render_favorites`] turn it into the TOML text; [`load_file`] and
//! [`save_file`] are the blocking file work that only
//! [`perform`](crate::perform) and the tests call.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use toml::{Table, Value};

use crate::error::FileError;
use crate::fsutil::write_atomic;

/// Version of the file format written.
pub const FAVORITES_VERSION: i64 = 1;
/// The most favorites kept.
pub const MAX_FAVORITES: usize = 1000;
/// The longest name, in characters.
pub const MAX_NAME_CHARS: usize = 200;
/// The longest line of moves, in characters.
pub const MAX_MOVES_CHARS: usize = 4000;
/// Largest file that is read.
const MAX_FILE_BYTES: u64 = 1 << 20;
/// The UTF-8 byte order mark that some editors add.
const BOM: &[u8] = &[0xef, 0xbb, 0xbf];

/// A favorite report: a name and the moves from the start position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Favorite {
    /// The name shown in the menu (and the name of the generated file).
    pub name: String,
    /// The moves, `1.e4 c5 2.Nf3`.
    pub moves: String,
}

/// Why a favorite was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FavoriteError {
    /// The name is empty (or only blanks) or has a control character.
    EmptyName,
    /// Another favorite has the name.
    NameTaken,
    /// There are [`MAX_FAVORITES`] already.
    TooMany,
    /// The name or the moves are too long.
    TooLong,
    /// There is no favorite at the index.
    Missing,
}

/// What a favorites file held that could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FavoritesIssue {
    /// The file could not be read as TOML; it was kept as
    /// `report_favorites.toml.corrupt` and the list starts empty.
    Unreadable,
    /// The file was written by a newer version; what is understood was
    /// read.
    NewerVersion(i64),
    /// This many entries were invalid, repeated or over a limit and were
    /// left out.
    Dropped(usize),
}

/// The list of favorites, in the order of the menu.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Favorites {
    items: Vec<Favorite>,
}

fn clean_name(name: &str) -> Result<String, FavoriteError> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(FavoriteError::EmptyName);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(FavoriteError::TooLong);
    }
    Ok(name.to_owned())
}

impl Favorites {
    /// An empty list.
    pub fn new() -> Favorites {
        Favorites::default()
    }

    /// The favorites, in order.
    pub fn items(&self) -> &[Favorite] {
        &self.items
    }

    /// How many.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there is none.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The favorite at `index`.
    pub fn get(&self, index: usize) -> Option<&Favorite> {
        self.items.get(index)
    }

    /// Whether a favorite has this name.
    pub fn contains(&self, name: &str) -> bool {
        self.items.iter().any(|f| f.name == name)
    }

    /// Adds a favorite at the end. The name is trimmed.
    ///
    /// # Errors
    ///
    /// [`FavoriteError::EmptyName`], [`FavoriteError::NameTaken`],
    /// [`FavoriteError::TooMany`] and [`FavoriteError::TooLong`]; nothing
    /// changes.
    pub fn add(&mut self, name: &str, moves: &str) -> Result<(), FavoriteError> {
        let name = clean_name(name)?;
        if moves.chars().count() > MAX_MOVES_CHARS {
            return Err(FavoriteError::TooLong);
        }
        if self.contains(&name) {
            return Err(FavoriteError::NameTaken);
        }
        if self.items.len() >= MAX_FAVORITES {
            return Err(FavoriteError::TooMany);
        }
        self.items.push(Favorite {
            name,
            moves: moves.trim().to_owned(),
        });
        Ok(())
    }

    /// Renames the favorite at `index`; its own name is allowed (nothing
    /// changes then).
    ///
    /// # Errors
    ///
    /// The same as [`Favorites::add`], and [`FavoriteError::Missing`].
    pub fn rename(&mut self, index: usize, name: &str) -> Result<(), FavoriteError> {
        let name = clean_name(name)?;
        if index >= self.items.len() {
            return Err(FavoriteError::Missing);
        }
        if self
            .items
            .iter()
            .enumerate()
            .any(|(i, f)| i != index && f.name == name)
        {
            return Err(FavoriteError::NameTaken);
        }
        self.items[index].name = name;
        Ok(())
    }

    /// Removes the favorite at `index`.
    ///
    /// # Errors
    ///
    /// [`FavoriteError::Missing`].
    pub fn delete(&mut self, index: usize) -> Result<Favorite, FavoriteError> {
        if index >= self.items.len() {
            return Err(FavoriteError::Missing);
        }
        Ok(self.items.remove(index))
    }

    /// Moves the favorite at `index` one place up. Returns the new index
    /// (the same one at the top).
    ///
    /// # Errors
    ///
    /// [`FavoriteError::Missing`].
    pub fn move_up(&mut self, index: usize) -> Result<usize, FavoriteError> {
        if index >= self.items.len() {
            return Err(FavoriteError::Missing);
        }
        if index == 0 {
            return Ok(0);
        }
        self.items.swap(index, index - 1);
        Ok(index - 1)
    }

    /// Moves the favorite at `index` one place down. Returns the new index
    /// (the same one at the bottom).
    ///
    /// # Errors
    ///
    /// [`FavoriteError::Missing`].
    pub fn move_down(&mut self, index: usize) -> Result<usize, FavoriteError> {
        if index >= self.items.len() {
            return Err(FavoriteError::Missing);
        }
        if index + 1 == self.items.len() {
            return Ok(index);
        }
        self.items.swap(index, index + 1);
        Ok(index + 1)
    }

    /// Adds the favorites that can be added; returns `(added, skipped)`.
    /// A favorite is skipped when its name is empty or taken, or the list
    /// is full.
    pub fn import(&mut self, favorites: impl IntoIterator<Item = (String, String)>) -> (u32, u32) {
        let (mut added, mut skipped) = (0u32, 0u32);
        for (name, moves) in favorites {
            if self.add(&name, &moves).is_ok() {
                added += 1;
            } else {
                skipped += 1;
            }
        }
        (added, skipped)
    }
}

/// The favorites file that goes with the preferences file `prefs`: the
/// same directory, `report_favorites.toml`.
pub fn favorites_path(prefs: &Path) -> PathBuf {
    prefs.with_file_name("report_favorites.toml")
}

/// The result of reading the favorites file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadedFavorites {
    /// The favorites.
    pub favorites: Favorites,
    /// What went wrong, if anything.
    pub issue: Option<FavoritesIssue>,
}

/// Reads the text of a favorites file, tolerantly: an invalid entry is
/// dropped and counted, never an error.
pub fn parse_favorites(text: &str) -> LoadedFavorites {
    let Ok(root) = text.parse::<Table>() else {
        return LoadedFavorites {
            favorites: Favorites::new(),
            issue: Some(FavoritesIssue::Unreadable),
        };
    };
    let version = root.get("version").and_then(Value::as_integer).unwrap_or(1);
    let mut favorites = Favorites::new();
    let mut dropped = 0usize;
    match root.get("favorite") {
        None => {}
        Some(Value::Array(entries)) => {
            for entry in entries {
                let name = entry.get("name").and_then(Value::as_str);
                let moves = entry.get("moves").and_then(Value::as_str);
                match (name, moves) {
                    (Some(name), Some(moves)) if favorites.add(name, moves).is_ok() => {}
                    _ => dropped += 1,
                }
            }
        }
        Some(_) => dropped += 1,
    }
    let issue = if version > FAVORITES_VERSION {
        Some(FavoritesIssue::NewerVersion(version))
    } else if dropped > 0 {
        Some(FavoritesIssue::Dropped(dropped))
    } else {
        None
    };
    LoadedFavorites { favorites, issue }
}

#[derive(Serialize)]
struct FileOut<'a> {
    version: i64,
    favorite: &'a [Favorite],
}

/// The text of the favorites file.
pub fn render_favorites(favorites: &Favorites) -> String {
    let out = FileOut {
        version: FAVORITES_VERSION,
        favorite: favorites.items(),
    };
    // The values are plain strings: the serializer cannot refuse them.
    toml::to_string(&out).unwrap_or_default()
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Reads `path`. A missing file is an empty list; an unreadable one is
/// kept as `.corrupt` and the list starts empty.
///
/// # Errors
///
/// The error of reading the file, when it is not that it does not exist.
pub(crate) fn load_file(path: &Path) -> Result<LoadedFavorites, FileError> {
    let mut bytes = Vec::new();
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LoadedFavorites::default());
        }
        Err(error) => return Err(FileError::from_io(&error, Some(path.to_path_buf()))),
    };
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| FileError::from_io(&error, Some(path.to_path_buf())))?;
    let too_big = u64::try_from(bytes.len()).map_or(true, |n| n > MAX_FILE_BYTES);
    let body = bytes.strip_prefix(BOM).unwrap_or(&bytes);
    let loaded = match std::str::from_utf8(body) {
        Ok(text) if !too_big => parse_favorites(text),
        _ => LoadedFavorites {
            favorites: Favorites::new(),
            issue: Some(FavoritesIssue::Unreadable),
        },
    };
    if loaded.issue == Some(FavoritesIssue::Unreadable) && !path.is_dir() {
        // Best effort: if it fails the file is simply rejected again.
        std::fs::rename(path, with_suffix(path, ".corrupt")).ok();
    }
    Ok(loaded)
}

/// Writes the favorites atomically. With `backup` the file that is there
/// is first copied to `<file>.bak` (it was written by a newer version or
/// held entries that were dropped); if the copy fails nothing is written.
///
/// # Errors
///
/// The error of copying or writing the file.
pub(crate) fn save_file(path: &Path, favorites: &Favorites, backup: bool) -> Result<(), FileError> {
    if backup {
        match std::fs::copy(path, with_suffix(path, ".bak")) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(FileError::from_io(&error, Some(path.to_path_buf()))),
        }
    }
    let text = render_favorites(favorites);
    write_atomic(path, |file| {
        std::io::Write::write_all(file, text.as_bytes())
    })
    .map_err(|error| FileError::from_io(&error, Some(path.to_path_buf())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Favorites {
        let mut f = Favorites::new();
        f.add("Najdorf", "1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6")
            .expect("add");
        f.add("Ruy Lopez", "1.e4 e5 2.Nf3 Nc6 3.Bb5").expect("add");
        f.add("Queen's Gambit", "1.d4 d5 2.c4").expect("add");
        f
    }

    #[test]
    fn names_are_trimmed_and_must_be_unique_and_not_empty() {
        let mut f = list();
        assert_eq!(f.add("  ", "1.e4"), Err(FavoriteError::EmptyName));
        assert_eq!(f.add("a\nb", "1.e4"), Err(FavoriteError::EmptyName));
        assert_eq!(f.add(" Najdorf ", "1.e4"), Err(FavoriteError::NameTaken));
        assert_eq!(
            f.add(&"x".repeat(MAX_NAME_CHARS + 1), ""),
            Err(FavoriteError::TooLong)
        );
        assert_eq!(f.len(), 3);
        f.add("  Sicilian ", "1.e4 c5").expect("add");
        assert_eq!(f.get(3).map(|x| x.name.as_str()), Some("Sicilian"));
    }

    #[test]
    fn rename_delete_and_reorder() {
        let mut f = list();
        assert_eq!(f.rename(1, "Najdorf"), Err(FavoriteError::NameTaken));
        assert_eq!(f.rename(1, "Ruy Lopez"), Ok(()));
        assert_eq!(f.rename(9, "x"), Err(FavoriteError::Missing));
        f.rename(1, "Spanish").expect("rename");
        assert_eq!(f.move_up(1), Ok(0));
        assert_eq!(f.items()[0].name, "Spanish");
        assert_eq!(f.move_up(0), Ok(0));
        assert_eq!(f.move_down(0), Ok(1));
        assert_eq!(f.move_down(2), Ok(2));
        assert_eq!(f.move_down(7), Err(FavoriteError::Missing));
        let gone = f.delete(0).expect("delete");
        assert_eq!(gone.name, "Najdorf");
        assert_eq!(f.delete(5), Err(FavoriteError::Missing));
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn the_text_round_trips() {
        let f = list();
        let text = render_favorites(&f);
        assert!(text.starts_with("version = 1"), "{text}");
        let loaded = parse_favorites(&text);
        assert_eq!(loaded.favorites, f);
        assert_eq!(loaded.issue, None);
    }

    #[test]
    fn bad_entries_are_dropped_and_counted() {
        let text = "version = 1\n[[favorite]]\nname = \"A\"\nmoves = \"1.e4\"\n\
                    [[favorite]]\nname = \"A\"\nmoves = \"1.d4\"\n\
                    [[favorite]]\nname = \"\"\nmoves = \"1.d4\"\n\
                    [[favorite]]\nname = 3\n";
        let loaded = parse_favorites(text);
        assert_eq!(loaded.favorites.len(), 1);
        assert_eq!(loaded.issue, Some(FavoritesIssue::Dropped(3)));
        assert_eq!(
            parse_favorites("= = =").issue,
            Some(FavoritesIssue::Unreadable)
        );
        assert_eq!(
            parse_favorites("version = 7\n").issue,
            Some(FavoritesIssue::NewerVersion(7))
        );
    }

    #[test]
    fn import_skips_what_cannot_be_added() {
        let mut f = list();
        let (added, skipped) = f.import([
            ("New".to_owned(), "1.c4".to_owned()),
            ("Najdorf".to_owned(), "1.e4".to_owned()),
            (String::new(), "1.e4".to_owned()),
        ]);
        assert_eq!((added, skipped), (1, 2));
        assert_eq!(f.len(), 4);
    }

    #[test]
    fn the_file_is_written_and_read_back_and_a_bad_one_is_kept_aside() {
        let dir = tempfile::tempdir().expect("dir");
        let path = favorites_path(&dir.path().join("preferences.toml"));
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("report_favorites.toml")
        );
        assert_eq!(load_file(&path).expect("missing").favorites.len(), 0);
        save_file(&path, &list(), false).expect("save");
        assert_eq!(load_file(&path).expect("load").favorites, list());
        std::fs::write(&path, "= = =").expect("write");
        let loaded = load_file(&path).expect("load");
        assert_eq!(loaded.issue, Some(FavoritesIssue::Unreadable));
        assert!(!path.exists());
        assert!(with_suffix(&path, ".corrupt").exists());
    }
}
