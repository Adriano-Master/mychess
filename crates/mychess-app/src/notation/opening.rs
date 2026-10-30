//! The opening name for the notation header (M2-5, D249).

use mychess_core::{EcoBook, Game, NodeId};

use crate::game_info::OpeningName;

/// The name of the **last** book entry found on the path from the start of
/// the game to `cursor`.
///
/// It walks from the cursor towards the root and stops at the first known
/// position, so a game that left the book keeps the last opening it was in.
/// This differs from [`GameInfoView::opening`](crate::GameInfoView), which
/// only looks at the exact position. The start position (the root) never
/// names an opening, and a stale `cursor` counts as the root. Without a book
/// the result is `None`.
pub fn opening_along_path(
    game: &Game,
    cursor: NodeId,
    book: Option<&EcoBook>,
) -> Option<OpeningName> {
    let book = book?;
    let root = game.root();
    let mut at = if game.contains(cursor) { cursor } else { root };
    while at != root {
        if let Some(entry) = game
            .position_at(at)
            .ok()
            .and_then(|position| book.lookup(position))
        {
            return Some(OpeningName {
                eco: entry.eco(),
                name: entry.name().to_owned(),
            });
        }
        at = game.parent(at).ok().flatten()?;
    }
    None
}
