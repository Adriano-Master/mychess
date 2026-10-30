//! Reading and writing `bookmarks.toml` (spec M3-6, D676, D677).
//!
//! Blocking: only [`perform`](crate::perform) and the tests call
//! [`load_file`] and [`save_file`].

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use super::tree::{
    BookmarkNode, Bookmarks, Folder, GameBookmark, MAX_BOOKMARK_DEPTH, MAX_BOOKMARKS,
    MAX_TITLE_CHARS, NEW_FOLDER_TITLE, NodeKind,
};
use crate::error::FileError;
use crate::fsutil::write_atomic;

/// Version of the file format written.
pub const BOOKMARKS_VERSION: i64 = 1;
/// Largest file that is read.
const MAX_FILE_BYTES: u64 = 1 << 20;
/// The UTF-8 byte order mark that some editors add.
const BOM: &[u8] = &[0xef, 0xbb, 0xbf];

/// A remark about the file that was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BookmarksIssue {
    /// The file could not be read or understood; it was kept as
    /// `bookmarks.toml.corrupt` and the list starts empty.
    Unreadable,
    /// The file was written by a newer version: what is understood was
    /// read, and a copy is kept before the file is replaced.
    NewerVersion(i64),
    /// This many entries were invalid or over a limit and were left out.
    Dropped(usize),
}

/// The result of reading `bookmarks.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadedBookmarks {
    /// The bookmarks.
    pub bookmarks: Bookmarks,
    /// What went wrong, if anything.
    pub issue: Option<BookmarksIssue>,
}

/// The bookmarks file that goes with the preferences file `prefs`: the same
/// directory, `bookmarks.toml`.
pub fn bookmarks_path(prefs: &Path) -> PathBuf {
    prefs.with_file_name("bookmarks.toml")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn quarantine(path: &Path) {
    // Best effort: if it fails the file is simply rejected again next time.
    if !path.is_dir() {
        std::fs::rename(path, with_suffix(path, ".corrupt")).ok();
    }
}

fn read_limited(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok((u64::try_from(bytes.len()).is_ok_and(|n| n <= MAX_FILE_BYTES)).then_some(bytes))
}

fn unreadable() -> LoadedBookmarks {
    LoadedBookmarks {
        bookmarks: Bookmarks::new(),
        issue: Some(BookmarksIssue::Unreadable),
    }
}

/// Reads the bookmarks at `path`. A missing file gives an empty list
/// without remarks; one that is unreadable, too large, not UTF-8 or not TOML
/// is renamed to `<name>.corrupt` and gives [`BookmarksIssue::Unreadable`].
pub(crate) fn load_file(path: &Path) -> LoadedBookmarks {
    let bytes = match read_limited(path) {
        Ok(Some(bytes)) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LoadedBookmarks::default();
        }
        Ok(None) | Err(_) => {
            quarantine(path);
            return unreadable();
        }
    };
    let content = bytes.strip_prefix(BOM).unwrap_or(&bytes);
    let Ok(text) = std::str::from_utf8(content) else {
        quarantine(path);
        return unreadable();
    };
    let loaded = parse_bookmarks(text);
    if loaded.issue == Some(BookmarksIssue::Unreadable) {
        quarantine(path);
    }
    loaded
}

/// Writes the bookmarks atomically, creating the directory. A file written
/// by a newer version is copied to `<name>.bak-v<N>` first (once).
pub(crate) fn save_file(path: &Path, bookmarks: &Bookmarks) -> Result<(), FileError> {
    let fail = |e: &std::io::Error| FileError::from_io(e, Some(path.to_path_buf()));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| fail(&e))?;
    }
    if let Some(version) = newer_version_on_disk(path) {
        let backup = with_suffix(path, &format!(".bak-v{version}"));
        if !backup.exists() {
            std::fs::copy(path, &backup).map_err(|e| fail(&e))?;
        }
    }
    // A failed serialization must never replace the good file with an empty one.
    let text = try_render(bookmarks)
        .map_err(|e| fail(&std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
    write_atomic(path, |file| {
        file.write_all(text.as_bytes())?;
        file.flush()
    })
    .map_err(|e| fail(&e))
}

fn newer_version_on_disk(path: &Path) -> Option<i64> {
    let bytes = read_limited(path).ok()??;
    let text = std::str::from_utf8(bytes.strip_prefix(BOM).unwrap_or(&bytes)).ok()?;
    let version = text.parse::<Table>().ok()?.get("version")?.as_integer()?;
    (version > BOOKMARKS_VERSION).then_some(version)
}

// ---- text <-> tree ----

/// Renders the tree as TOML. Games whose database path is not UTF-8 are
/// left out (TOML holds text only; `AppState` refuses to create them).
pub fn render_bookmarks(bookmarks: &Bookmarks) -> String {
    try_render(bookmarks).unwrap_or_default()
}

fn try_render(bookmarks: &Bookmarks) -> Result<String, toml::ser::Error> {
    let mut table = Table::new();
    table.insert("version".into(), Value::Integer(BOOKMARKS_VERSION));
    let items = items_to_value(bookmarks.roots());
    if !items.is_empty() {
        table.insert("items".into(), Value::Array(items));
    }
    toml::to_string(&table)
}

fn items_to_value(nodes: &[BookmarkNode]) -> Vec<Value> {
    nodes
        .iter()
        .filter_map(|node| {
            let mut t = Table::new();
            match &node.kind {
                NodeKind::Game(game) => {
                    t.insert("kind".into(), "game".into());
                    t.insert("title".into(), game.title.clone().into());
                    t.insert("base".into(), game.base.to_str()?.to_owned().into());
                    t.insert("game".into(), Value::Integer(i64::from(game.game)));
                    t.insert("ply".into(), Value::Integer(i64::from(game.ply)));
                    t.insert("white".into(), game.white.clone().into());
                    t.insert("black".into(), game.black.clone().into());
                    t.insert("year".into(), Value::Integer(i64::from(game.year)));
                    t.insert("site".into(), game.site.clone().into());
                    t.insert("round".into(), game.round.clone().into());
                    t.insert("result".into(), game.result.clone().into());
                }
                NodeKind::Folder(folder) => {
                    t.insert("kind".into(), "folder".into());
                    t.insert("title".into(), folder.title.clone().into());
                    let children = items_to_value(&folder.children);
                    if !children.is_empty() {
                        t.insert("children".into(), Value::Array(children));
                    }
                }
            }
            Some(Value::Table(t))
        })
        .collect()
}

/// Reads bookmarks from TOML text, tolerantly: invalid entries are left
/// out and counted. Never fails.
pub fn parse_bookmarks(text: &str) -> LoadedBookmarks {
    let Ok(table) = text.parse::<Table>() else {
        return unreadable();
    };
    let version = table
        .get("version")
        .and_then(Value::as_integer)
        .unwrap_or(BOOKMARKS_VERSION);
    let mut bookmarks = Bookmarks::new();
    let mut reader = Reader {
        dropped: 0,
        count: 0,
    };
    let roots = match table.get("items") {
        Some(Value::Array(items)) => reader.read(&mut bookmarks, items, 1),
        Some(_) => {
            reader.dropped += 1;
            Vec::new()
        }
        None => Vec::new(),
    };
    bookmarks.set_roots(roots);
    let issue = if version > BOOKMARKS_VERSION {
        Some(BookmarksIssue::NewerVersion(version))
    } else if reader.dropped > 0 {
        Some(BookmarksIssue::Dropped(reader.dropped))
    } else {
        None
    };
    LoadedBookmarks { bookmarks, issue }
}

struct Reader {
    dropped: usize,
    count: usize,
}

impl Reader {
    fn read(&mut self, tree: &mut Bookmarks, items: &[Value], depth: usize) -> Vec<BookmarkNode> {
        let mut nodes = Vec::new();
        for item in items {
            if self.count >= MAX_BOOKMARKS {
                self.dropped += 1;
                continue;
            }
            let Some(table) = item.as_table() else {
                self.dropped += 1;
                continue;
            };
            let kind = match table.get("kind").and_then(Value::as_str) {
                Some("folder") => {
                    if depth > MAX_BOOKMARK_DEPTH {
                        self.dropped += 1;
                        continue;
                    }
                    self.count += 1;
                    let children = match table.get("children") {
                        Some(Value::Array(children)) => self.read(tree, children, depth + 1),
                        Some(_) => {
                            self.dropped += 1;
                            Vec::new()
                        }
                        None => Vec::new(),
                    };
                    let title = text_of(table, "title");
                    NodeKind::Folder(Folder {
                        title: if title.is_empty() {
                            NEW_FOLDER_TITLE.to_owned()
                        } else {
                            title
                        },
                        children,
                    })
                }
                Some("game") => {
                    if let Some(game) = read_game(table) {
                        self.count += 1;
                        NodeKind::Game(game)
                    } else {
                        self.dropped += 1;
                        continue;
                    }
                }
                _ => {
                    self.dropped += 1;
                    continue;
                }
            };
            nodes.push(tree.node(kind));
        }
        nodes
    }
}

fn text_of(table: &Table, key: &str) -> String {
    table
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect()
}

fn number_of<T: TryFrom<i64>>(table: &Table, key: &str) -> Option<T> {
    table
        .get(key)
        .and_then(Value::as_integer)
        .and_then(|n| T::try_from(n).ok())
}

fn read_game(table: &Table) -> Option<GameBookmark> {
    let base = table.get("base").and_then(Value::as_str)?;
    if base.is_empty() {
        return None;
    }
    let game: u32 = number_of(table, "game").filter(|n| *n >= 1)?;
    let mut bookmark = GameBookmark {
        title: text_of(table, "title"),
        base: PathBuf::from(base),
        game,
        ply: number_of(table, "ply").unwrap_or(0),
        white: text_of(table, "white"),
        black: text_of(table, "black"),
        year: number_of(table, "year").unwrap_or(0),
        site: text_of(table, "site"),
        round: text_of(table, "round"),
        result: text_of(table, "result"),
    };
    if bookmark.result.is_empty() {
        "*".clone_into(&mut bookmark.result);
    }
    if bookmark.title.is_empty() {
        bookmark.title = super::default_title(&bookmark);
    }
    Some(bookmark)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(title: &str, base: &str, number: u32) -> NodeKind {
        NodeKind::Game(GameBookmark {
            title: title.to_owned(),
            base: PathBuf::from(base),
            game: number,
            ply: 12,
            white: "Kasparov, Garry".to_owned(),
            black: "Müller \"Max\"".to_owned(),
            year: 1999,
            site: "Wijk aan Zee NED".to_owned(),
            round: "5".to_owned(),
            result: "1-0".to_owned(),
        })
    }

    fn sample() -> Bookmarks {
        let mut tree = Bookmarks::new();
        let folder = tree
            .insert_after(
                None,
                NodeKind::Folder(Folder {
                    title: "Sicilian — Najdorf".to_owned(),
                    children: Vec::new(),
                }),
            )
            .expect("folder");
        tree.insert_after(Some(folder), game("A", "/db/a b/ü.si4", 3))
            .expect("child");
        tree.insert_after(None, game("B", "C:\\bases\\x.si4", 9))
            .expect("root");
        tree
    }

    fn shape(tree: &Bookmarks) -> Vec<(usize, String)> {
        tree.rows()
            .into_iter()
            .map(|r| (r.depth, r.title))
            .collect()
    }

    #[test]
    fn a_tree_survives_a_round_trip_with_nesting() {
        let tree = sample();
        let text = render_bookmarks(&tree);
        let loaded = parse_bookmarks(&text);
        assert_eq!(loaded.issue, None, "{text}");
        assert_eq!(shape(&loaded.bookmarks), shape(&tree));
        let first = loaded
            .bookmarks
            .roots()
            .iter()
            .find_map(|n| match &n.kind {
                NodeKind::Folder(f) => f.children.first().and_then(BookmarkNode::game),
                NodeKind::Game(_) => None,
            })
            .expect("nested game");
        assert_eq!(first.base, PathBuf::from("/db/a b/ü.si4"));
        assert_eq!(first.black, "Müller \"Max\"");
        assert_eq!((first.game, first.ply, first.year), (3, 12, 1999));
    }

    #[test]
    fn invalid_entries_are_dropped_and_counted() {
        let text = r#"
version = 1
[[items]]
kind = "game"
title = "ok"
base = "/a.si4"
game = 2
[[items]]
kind = "game"
title = "no base"
game = 2
[[items]]
kind = "game"
base = "/b.si4"
game = 0
[[items]]
kind = "weird"
"#;
        let loaded = parse_bookmarks(text);
        assert_eq!(loaded.bookmarks.len(), 1);
        assert_eq!(loaded.issue, Some(BookmarksIssue::Dropped(3)));
    }

    #[test]
    fn a_missing_title_is_made_from_the_game() {
        let loaded = parse_bookmarks(
            "[[items]]\nkind = \"game\"\nbase = \"/d/x.si4\"\ngame = 1\nwhite = \"A, B\"\nblack = \"C, D\"\nresult = \"0-1\"\nyear = 2001\nsite = \"Rome, Italy\"\n",
        );
        let title = loaded.bookmarks.roots()[0].title().to_owned();
        assert_eq!(title, "x:  A - C, 0-1,  Rome 2001");
    }

    #[test]
    fn nesting_deeper_than_the_limit_is_dropped() {
        let mut text = String::new();
        let mut prefix = "items".to_owned();
        for level in 0..=MAX_BOOKMARK_DEPTH + 1 {
            text = format!("{text}[[{prefix}]]\nkind = \"folder\"\ntitle = \"f{level}\"\n");
            prefix.push_str(".children");
        }
        let loaded = parse_bookmarks(&text);
        assert!(loaded.bookmarks.rows().len() <= MAX_BOOKMARK_DEPTH + 1);
        assert!(matches!(loaded.issue, Some(BookmarksIssue::Dropped(_))));
    }

    #[test]
    fn a_newer_version_is_read_with_a_remark() {
        let loaded = parse_bookmarks("version = 7\n[[items]]\nkind = \"folder\"\ntitle = \"x\"\n");
        assert_eq!(loaded.issue, Some(BookmarksIssue::NewerVersion(7)));
        assert_eq!(loaded.bookmarks.len(), 1);
    }

    #[test]
    fn broken_toml_is_unreadable() {
        assert_eq!(
            parse_bookmarks("= = [").issue,
            Some(BookmarksIssue::Unreadable)
        );
    }

    #[test]
    fn missing_file_is_an_empty_list_and_a_corrupt_one_is_set_aside() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bookmarks.toml");
        assert_eq!(load_file(&path), LoadedBookmarks::default());
        std::fs::write(&path, "= = broken").expect("write");
        let loaded = load_file(&path);
        assert_eq!(loaded.issue, Some(BookmarksIssue::Unreadable));
        assert!(!path.exists());
        assert!(dir.path().join("bookmarks.toml.corrupt").exists());
    }

    #[test]
    fn non_utf8_and_oversized_files_are_set_aside() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bookmarks.toml");
        std::fs::write(&path, [0xff, 0xfe, 0x00]).expect("write");
        assert_eq!(load_file(&path).issue, Some(BookmarksIssue::Unreadable));
        let mut big = b"# ".to_vec();
        big.resize(usize::try_from(MAX_FILE_BYTES).expect("fits") + 10, b'x');
        std::fs::write(&path, big).expect("write");
        assert_eq!(load_file(&path).issue, Some(BookmarksIssue::Unreadable));
        assert!(!path.exists());
    }

    #[test]
    fn saving_creates_the_directory_and_replaces_without_leftovers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("x").join("bookmarks.toml");
        let tree = sample();
        save_file(&path, &tree).expect("first");
        save_file(&path, &tree).expect("second");
        let loaded = load_file(&path);
        assert_eq!(shape(&loaded.bookmarks), shape(&tree));
        assert_eq!(
            std::fs::read_dir(path.parent().expect("parent"))
                .expect("dir")
                .count(),
            1
        );
    }

    #[test]
    fn a_file_of_a_newer_version_is_backed_up_once_before_it_is_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bookmarks.toml");
        std::fs::write(&path, "version = 9\n").expect("write");
        save_file(&path, &sample()).expect("save");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("bookmarks.toml.bak-v9")).expect("backup"),
            "version = 9\n"
        );
    }

    #[test]
    fn the_file_sits_next_to_the_preferences() {
        let prefs = Path::new("conf").join("preferences.toml");
        assert_eq!(
            bookmarks_path(&prefs),
            Path::new("conf").join("bookmarks.toml")
        );
    }
}
