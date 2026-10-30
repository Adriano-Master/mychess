//! The tree of bookmarks: games and nested folders (spec M3-6, D671, D672;
//! Scid: `tcl/file/bookmark.tcl`).

use std::path::PathBuf;

/// Deepest nesting of folders (the top level is level 1).
pub const MAX_BOOKMARK_DEPTH: usize = 8;
/// Most entries (games and folders) in the tree.
pub const MAX_BOOKMARKS: usize = 5000;
/// Longest title, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
/// Title of a folder created without a name (the Scid's own text).
pub const NEW_FOLDER_TITLE: &str = "New folder";

/// Identifies an entry for the length of the session; it is not saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BookmarkId(u32);

/// A bookmarked game: where it is and how to recognise it when its number
/// has changed (D672).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameBookmark {
    /// The text shown in the menu.
    pub title: String,
    /// The database file (canonical path).
    pub base: PathBuf,
    /// The game number when the bookmark was made, from 1.
    pub game: u32,
    /// The ply of the main line to show (0 is the start position).
    pub ply: u32,
    /// `White`.
    pub white: String,
    /// `Black`.
    pub black: String,
    /// The year of the date, 0 when unknown.
    pub year: u16,
    /// `Site`.
    pub site: String,
    /// `Round`.
    pub round: String,
    /// `Result` as in PGN.
    pub result: String,
}

/// A folder of bookmarks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// The text shown in the menu.
    pub title: String,
    /// The entries inside, in order.
    pub children: Vec<BookmarkNode>,
}

/// What an entry is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// A game.
    Game(GameBookmark),
    /// A folder.
    Folder(Folder),
}

/// An entry of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkNode {
    /// Session identity.
    pub id: BookmarkId,
    /// The content.
    pub kind: NodeKind,
}

impl BookmarkNode {
    /// The title.
    pub fn title(&self) -> &str {
        match &self.kind {
            NodeKind::Game(game) => &game.title,
            NodeKind::Folder(folder) => &folder.title,
        }
    }

    /// Whether this is a folder.
    pub fn is_folder(&self) -> bool {
        matches!(self.kind, NodeKind::Folder(_))
    }

    /// The game, if this is one.
    pub fn game(&self) -> Option<&GameBookmark> {
        match &self.kind {
            NodeKind::Game(game) => Some(game),
            NodeKind::Folder(_) => None,
        }
    }

    /// Levels of folders from this entry down, itself included.
    fn height(&self) -> usize {
        match &self.kind {
            NodeKind::Game(_) => 1,
            NodeKind::Folder(folder) => {
                1 + folder.children.iter().map(Self::height).max().unwrap_or(0)
            }
        }
    }

    fn count(&self) -> usize {
        match &self.kind {
            NodeKind::Game(_) => 1,
            NodeKind::Folder(folder) => 1 + folder.children.iter().map(Self::count).sum::<usize>(),
        }
    }
}

/// Why an edit of the tree was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TreeError {
    /// The entry does not exist.
    #[error("no such bookmark")]
    NotFound,
    /// The tree is full.
    #[error("too many bookmarks")]
    TooMany,
    /// Folders would nest deeper than the limit.
    #[error("folders are nested too deeply")]
    TooDeep,
    /// The move has nowhere to go (first entry moved up, and so on).
    #[error("the entry cannot be moved there")]
    CannotMove,
}

/// One line of the flattened tree, as the editor lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkRow {
    /// The entry.
    pub id: BookmarkId,
    /// Nesting level, 0 at the top.
    pub depth: usize,
    /// The title.
    pub title: String,
    /// Whether it is a folder.
    pub is_folder: bool,
}

/// The bookmarks: a forest of games and folders.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bookmarks {
    roots: Vec<BookmarkNode>,
    next_id: u32,
}

/// Index path of an entry: the positions from the top level down.
type Path = Vec<usize>;

impl Bookmarks {
    /// An empty tree.
    pub fn new() -> Bookmarks {
        Bookmarks::default()
    }

    /// The top-level entries.
    pub fn roots(&self) -> &[BookmarkNode] {
        &self.roots
    }

    /// Whether there is nothing in the tree.
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// How many entries (games and folders) there are.
    pub fn len(&self) -> usize {
        self.roots.iter().map(BookmarkNode::count).sum()
    }

    fn fresh_id(&mut self) -> BookmarkId {
        self.next_id = self.next_id.wrapping_add(1);
        BookmarkId(self.next_id)
    }

    /// Builds a node with a new id (used by the reader and by the inserts).
    pub(crate) fn node(&mut self, kind: NodeKind) -> BookmarkNode {
        BookmarkNode {
            id: self.fresh_id(),
            kind,
        }
    }

    /// Replaces the top level (the reader builds nodes with [`Bookmarks::node`]).
    pub(crate) fn set_roots(&mut self, roots: Vec<BookmarkNode>) {
        self.roots = roots;
    }

    /// The entry `id`.
    pub fn get(&self, id: BookmarkId) -> Option<&BookmarkNode> {
        let path = self.locate(id)?;
        self.at(&path)
    }

    /// The entries in display order with their depth (folders before their
    /// children).
    pub fn rows(&self) -> Vec<BookmarkRow> {
        fn walk(nodes: &[BookmarkNode], depth: usize, out: &mut Vec<BookmarkRow>) {
            for node in nodes {
                out.push(BookmarkRow {
                    id: node.id,
                    depth,
                    title: node.title().to_owned(),
                    is_folder: node.is_folder(),
                });
                if let NodeKind::Folder(folder) = &node.kind {
                    walk(&folder.children, depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.roots, 0, &mut out);
        out
    }

    fn locate(&self, id: BookmarkId) -> Option<Path> {
        fn walk(nodes: &[BookmarkNode], id: BookmarkId, path: &mut Path) -> bool {
            for (index, node) in nodes.iter().enumerate() {
                path.push(index);
                if node.id == id {
                    return true;
                }
                if let NodeKind::Folder(folder) = &node.kind
                    && walk(&folder.children, id, path)
                {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Path::new();
        walk(&self.roots, id, &mut path).then_some(path)
    }

    fn at(&self, path: &[usize]) -> Option<&BookmarkNode> {
        let (first, rest) = path.split_first()?;
        let mut node = self.roots.get(*first)?;
        for index in rest {
            match &node.kind {
                NodeKind::Folder(folder) => node = folder.children.get(*index)?,
                NodeKind::Game(_) => return None,
            }
        }
        Some(node)
    }

    fn at_mut(&mut self, path: &[usize]) -> Option<&mut BookmarkNode> {
        let (first, rest) = path.split_first()?;
        let mut node = self.roots.get_mut(*first)?;
        for index in rest {
            match &mut node.kind {
                NodeKind::Folder(folder) => node = folder.children.get_mut(*index)?,
                NodeKind::Game(_) => return None,
            }
        }
        Some(node)
    }

    /// The list that holds the entries at `parent` (empty path: the top).
    fn siblings_mut(&mut self, parent: &[usize]) -> Option<&mut Vec<BookmarkNode>> {
        if parent.is_empty() {
            return Some(&mut self.roots);
        }
        match &mut self.at_mut(parent)?.kind {
            NodeKind::Folder(folder) => Some(&mut folder.children),
            NodeKind::Game(_) => None,
        }
    }

    /// Adds `kind` after the entry `after` at the same level; an empty
    /// folder takes it as its first child; `None` appends at the top level.
    ///
    /// # Errors
    ///
    /// [`TreeError::NotFound`], [`TreeError::TooMany`] or
    /// [`TreeError::TooDeep`].
    pub fn insert_after(
        &mut self,
        after: Option<BookmarkId>,
        kind: NodeKind,
    ) -> Result<BookmarkId, TreeError> {
        if self.len() >= MAX_BOOKMARKS {
            return Err(TreeError::TooMany);
        }
        let (parent, index) = match after {
            None => (Path::new(), self.roots.len()),
            Some(id) => {
                let mut path = self.locate(id).ok_or(TreeError::NotFound)?;
                let empty_folder = matches!(
                    self.at(&path).map(|n| &n.kind),
                    Some(NodeKind::Folder(folder)) if folder.children.is_empty()
                );
                if empty_folder {
                    (path, 0)
                } else {
                    let last = path.pop().ok_or(TreeError::NotFound)?;
                    (path, last + 1)
                }
            }
        };
        // A new entry has no children: it is one level below its parent.
        if parent.len() + 1 > MAX_BOOKMARK_DEPTH {
            return Err(TreeError::TooDeep);
        }
        let node = self.node(kind);
        let id = node.id;
        let siblings = self.siblings_mut(&parent).ok_or(TreeError::NotFound)?;
        siblings.insert(index.min(siblings.len()), node);
        Ok(id)
    }

    /// Adds `kind` at the top level just before the first folder, or at the
    /// end when there is none (Scid's `::bookmarks::AddCurrent`, D705).
    ///
    /// # Errors
    ///
    /// [`TreeError::TooMany`].
    pub fn insert_before_first_folder(&mut self, kind: NodeKind) -> Result<BookmarkId, TreeError> {
        if self.len() >= MAX_BOOKMARKS {
            return Err(TreeError::TooMany);
        }
        let index = self
            .roots
            .iter()
            .position(BookmarkNode::is_folder)
            .unwrap_or(self.roots.len());
        let node = self.node(kind);
        let id = node.id;
        self.roots.insert(index, node);
        Ok(id)
    }

    /// Gives every entry with an empty title a default one (D678).
    pub fn fill_empty_titles(&mut self) {
        fn walk(nodes: &mut [BookmarkNode]) {
            for node in nodes {
                match &mut node.kind {
                    NodeKind::Game(game) if game.title.is_empty() => {
                        game.title = super::default_title(game);
                    }
                    NodeKind::Folder(folder) => {
                        if folder.title.is_empty() {
                            NEW_FOLDER_TITLE.clone_into(&mut folder.title);
                        }
                        walk(&mut folder.children);
                    }
                    NodeKind::Game(_) => {}
                }
            }
        }
        walk(&mut self.roots);
    }

    /// Changes the title (cut to [`MAX_TITLE_CHARS`]).
    ///
    /// # Errors
    ///
    /// [`TreeError::NotFound`].
    pub fn rename(&mut self, id: BookmarkId, title: &str) -> Result<(), TreeError> {
        let path = self.locate(id).ok_or(TreeError::NotFound)?;
        let title: String = title.chars().take(MAX_TITLE_CHARS).collect();
        match &mut self.at_mut(&path).ok_or(TreeError::NotFound)?.kind {
            NodeKind::Game(game) => game.title = title,
            NodeKind::Folder(folder) => folder.title = title,
        }
        Ok(())
    }

    /// Removes the entry and everything inside it.
    ///
    /// # Errors
    ///
    /// [`TreeError::NotFound`].
    pub fn remove(&mut self, id: BookmarkId) -> Result<BookmarkNode, TreeError> {
        let mut path = self.locate(id).ok_or(TreeError::NotFound)?;
        let index = path.pop().ok_or(TreeError::NotFound)?;
        let siblings = self.siblings_mut(&path).ok_or(TreeError::NotFound)?;
        Ok(siblings.remove(index))
    }

    /// Swaps the entry with its previous sibling.
    ///
    /// # Errors
    ///
    /// [`TreeError::CannotMove`] for the first entry.
    pub fn move_up(&mut self, id: BookmarkId) -> Result<(), TreeError> {
        self.step(id, false)
    }

    /// Swaps the entry with its next sibling.
    ///
    /// # Errors
    ///
    /// [`TreeError::CannotMove`] for the last entry.
    pub fn move_down(&mut self, id: BookmarkId) -> Result<(), TreeError> {
        self.step(id, true)
    }

    fn step(&mut self, id: BookmarkId, down: bool) -> Result<(), TreeError> {
        let mut path = self.locate(id).ok_or(TreeError::NotFound)?;
        let index = path.pop().ok_or(TreeError::NotFound)?;
        let siblings = self.siblings_mut(&path).ok_or(TreeError::NotFound)?;
        let other = if down {
            index.checked_add(1).filter(|i| *i < siblings.len())
        } else {
            index.checked_sub(1)
        }
        .ok_or(TreeError::CannotMove)?;
        siblings.swap(index, other);
        Ok(())
    }

    /// Moves the entry into the folder that precedes it, as its last child.
    ///
    /// # Errors
    ///
    /// [`TreeError::CannotMove`] when the previous sibling is not a folder;
    /// [`TreeError::TooDeep`] when it would nest too deeply.
    pub fn indent(&mut self, id: BookmarkId) -> Result<(), TreeError> {
        let mut path = self.locate(id).ok_or(TreeError::NotFound)?;
        let index = path.pop().ok_or(TreeError::NotFound)?;
        let previous = index.checked_sub(1).ok_or(TreeError::CannotMove)?;
        let height = self
            .at(&[path.clone(), vec![index]].concat())
            .map_or(1, BookmarkNode::height);
        let mut folder_path = path;
        folder_path.push(previous);
        if !self.at(&folder_path).is_some_and(BookmarkNode::is_folder) {
            return Err(TreeError::CannotMove);
        }
        // The folder is at depth `folder_path.len()`; the entry goes below.
        if folder_path.len() + height > MAX_BOOKMARK_DEPTH {
            return Err(TreeError::TooDeep);
        }
        let mut node_path = folder_path.clone();
        node_path.pop();
        let node = {
            let siblings = self.siblings_mut(&node_path).ok_or(TreeError::NotFound)?;
            siblings.remove(index)
        };
        match &mut self.at_mut(&folder_path).ok_or(TreeError::NotFound)?.kind {
            NodeKind::Folder(folder) => folder.children.push(node),
            NodeKind::Game(_) => return Err(TreeError::CannotMove),
        }
        Ok(())
    }

    /// Moves the entry out of its folder, to the position after it.
    ///
    /// # Errors
    ///
    /// [`TreeError::CannotMove`] for a top-level entry.
    pub fn outdent(&mut self, id: BookmarkId) -> Result<(), TreeError> {
        let mut path = self.locate(id).ok_or(TreeError::NotFound)?;
        if path.len() < 2 {
            return Err(TreeError::CannotMove);
        }
        let index = path.pop().ok_or(TreeError::NotFound)?;
        let folder_index = path.pop().ok_or(TreeError::NotFound)?;
        let node = {
            let mut folder_path = path.clone();
            folder_path.push(folder_index);
            let siblings = self.siblings_mut(&folder_path).ok_or(TreeError::NotFound)?;
            siblings.remove(index)
        };
        let siblings = self.siblings_mut(&path).ok_or(TreeError::NotFound)?;
        siblings.insert(folder_index + 1, node);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(title: &str) -> NodeKind {
        NodeKind::Folder(Folder {
            title: title.to_owned(),
            children: Vec::new(),
        })
    }

    fn game(title: &str) -> NodeKind {
        NodeKind::Game(GameBookmark {
            title: title.to_owned(),
            base: PathBuf::from("db.si4"),
            game: 1,
            ply: 0,
            white: String::new(),
            black: String::new(),
            year: 0,
            site: String::new(),
            round: String::new(),
            result: "*".to_owned(),
        })
    }

    fn titles(tree: &Bookmarks) -> Vec<(usize, String)> {
        tree.rows()
            .into_iter()
            .map(|r| (r.depth, r.title))
            .collect()
    }

    fn d(depth: usize, title: &str) -> (usize, String) {
        (depth, title.to_owned())
    }

    #[test]
    fn inserting_after_an_entry_keeps_the_level_and_an_empty_folder_takes_the_child() {
        let mut tree = Bookmarks::new();
        let f = tree.insert_after(None, folder("F")).expect("f");
        let a = tree.insert_after(Some(f), game("A")).expect("a");
        tree.insert_after(Some(a), game("B")).expect("b");
        tree.insert_after(None, game("C")).expect("c");
        assert_eq!(
            titles(&tree),
            vec![d(0, "F"), d(1, "A"), d(1, "B"), d(0, "C")]
        );
        assert_eq!(tree.len(), 4);
    }

    #[test]
    fn moving_up_and_down_swaps_siblings_and_stops_at_the_ends() {
        let mut tree = Bookmarks::new();
        let a = tree.insert_after(None, game("A")).expect("a");
        let b = tree.insert_after(None, game("B")).expect("b");
        assert_eq!(tree.move_up(a), Err(TreeError::CannotMove));
        assert_eq!(tree.move_down(b), Err(TreeError::CannotMove));
        tree.move_down(a).expect("down");
        assert_eq!(titles(&tree), vec![d(0, "B"), d(0, "A")]);
        tree.move_up(a).expect("up");
        assert_eq!(titles(&tree), vec![d(0, "A"), d(0, "B")]);
    }

    #[test]
    fn indent_enters_the_folder_before_and_outdent_leaves_it() {
        let mut tree = Bookmarks::new();
        tree.insert_after(None, folder("F")).expect("f");
        let a = tree.insert_after(None, game("A")).expect("a");
        let b = tree.insert_after(None, game("B")).expect("b");
        tree.indent(a).expect("indent a");
        assert_eq!(titles(&tree), vec![d(0, "F"), d(1, "A"), d(0, "B")]);
        // B follows a folder, so it can go in too, after A.
        tree.indent(b).expect("indent b");
        assert_eq!(titles(&tree), vec![d(0, "F"), d(1, "A"), d(1, "B")]);
        tree.outdent(a).expect("outdent");
        assert_eq!(titles(&tree), vec![d(0, "F"), d(1, "B"), d(0, "A")]);
        assert_eq!(tree.outdent(a), Err(TreeError::CannotMove));
        // A follows the folder again, so it can go back in; B, now the
        // first child, has nothing before it.
        tree.indent(a).expect("back in");
        assert_eq!(tree.indent(b), Err(TreeError::CannotMove));
    }

    #[test]
    fn folders_cannot_nest_past_the_limit() {
        let mut tree = Bookmarks::new();
        let mut last = tree.insert_after(None, folder("0")).expect("root");
        for level in 1..MAX_BOOKMARK_DEPTH {
            last = tree
                .insert_after(Some(last), folder(&level.to_string()))
                .expect("nest");
        }
        assert_eq!(
            tree.insert_after(Some(last), game("deep")),
            Err(TreeError::TooDeep)
        );
        // Indenting a folder that has children under the deepest level too.
        let top = tree.insert_after(None, folder("x")).expect("x");
        let first = tree.roots()[0].id;
        tree.move_up(top).expect("move");
        assert_eq!(tree.indent(first), Err(TreeError::TooDeep));
    }

    fn root_titles(tree: &Bookmarks) -> Vec<String> {
        tree.roots().iter().map(|n| n.title().to_owned()).collect()
    }

    #[test]
    fn insert_before_first_folder_goes_ahead_of_the_first_folder() {
        let mut tree = Bookmarks::new();
        tree.insert_after(None, game("A")).expect("a");
        tree.insert_after(None, folder("F1")).expect("f1");
        tree.insert_after(None, folder("F2")).expect("f2");
        tree.insert_before_first_folder(game("X")).expect("x");
        tree.insert_before_first_folder(game("Y")).expect("y");
        assert_eq!(root_titles(&tree), ["A", "X", "Y", "F1", "F2"]);
    }

    #[test]
    fn insert_before_first_folder_appends_without_folders_and_respects_the_limit() {
        let mut tree = Bookmarks::new();
        tree.insert_before_first_folder(game("A")).expect("a");
        tree.insert_before_first_folder(game("B")).expect("b");
        assert_eq!(root_titles(&tree), ["A", "B"]);
        while tree.len() < MAX_BOOKMARKS {
            tree.insert_before_first_folder(game("g")).expect("room");
        }
        assert_eq!(
            tree.insert_before_first_folder(game("g")),
            Err(TreeError::TooMany)
        );
    }

    #[test]
    fn deleting_a_folder_removes_what_is_inside_and_unknown_ids_are_refused() {
        let mut tree = Bookmarks::new();
        let f = tree.insert_after(None, folder("F")).expect("f");
        tree.insert_after(Some(f), game("A")).expect("a");
        tree.remove(f).expect("remove");
        assert!(tree.is_empty());
        assert_eq!(tree.remove(f).map(|_| ()), Err(TreeError::NotFound));
        assert_eq!(tree.rename(f, "x"), Err(TreeError::NotFound));
    }

    #[test]
    fn rename_cuts_long_titles_and_empty_ones_get_a_default() {
        let mut tree = Bookmarks::new();
        let f = tree.insert_after(None, folder("F")).expect("f");
        tree.rename(f, &"x".repeat(MAX_TITLE_CHARS + 50))
            .expect("rename");
        assert_eq!(
            tree.get(f).expect("f").title().chars().count(),
            MAX_TITLE_CHARS
        );
        tree.rename(f, "").expect("empty");
        tree.fill_empty_titles();
        assert_eq!(tree.get(f).expect("f").title(), NEW_FOLDER_TITLE);
    }

    #[test]
    fn the_tree_refuses_entries_past_the_limit() {
        let mut tree = Bookmarks::new();
        for _ in 0..MAX_BOOKMARKS {
            tree.insert_after(None, game("g")).expect("room");
        }
        assert_eq!(tree.insert_after(None, game("g")), Err(TreeError::TooMany));
    }
}
