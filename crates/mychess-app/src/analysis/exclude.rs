//! Excluding moves from the search (spec M4-4, D445; `excludeMove`,
//! `analysis.tcl` 2816-2842).

use mychess_core::{Move, Position, SanMode};

/// Why the text of the exclusion dialog was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExcludeError {
    /// The token (clipped to 64 characters) is not a legal move of the
    /// position.
    Invalid(String),
    /// Every legal move would be excluded: nothing would be searched.
    All,
}

/// Reads the moves to exclude: SAN moves separated by spaces or commas.
/// An empty text is an empty list (clear the exclusion); a move named twice
/// counts once.
///
/// # Errors
///
/// [`ExcludeError::Invalid`] for the first token that is not a legal move of
/// `position`; [`ExcludeError::All`] if the list holds every legal move.
pub fn parse_exclusion(position: &Position, text: &str) -> Result<Vec<Move>, ExcludeError> {
    let mut excluded: Vec<Move> = Vec::new();
    for token in text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
    {
        let parsed = position
            .parse_san(token, SanMode::Tolerant)
            .ok()
            .filter(|p| position.is_legal(p.mv))
            .ok_or_else(|| ExcludeError::Invalid(token.chars().take(64).collect()))?;
        if !excluded.contains(&parsed.mv) {
            excluded.push(parsed.mv);
        }
    }
    if !excluded.is_empty() && search_moves_without(position, &excluded).is_empty() {
        return Err(ExcludeError::All);
    }
    Ok(excluded)
}

/// The moves to search once `excluded` are left out: every legal move of
/// `position` that is not in the list (UCI `searchmoves`).
pub fn search_moves_without(position: &Position, excluded: &[Move]) -> Vec<Move> {
    position
        .legal_moves()
        .into_iter()
        .filter(|m| !excluded.contains(m))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn san_moves_split_on_spaces_and_commas() {
        let pos = Position::new();
        let moves = parse_exclusion(&pos, "e4, d4  Nf3,c4").expect("legal");
        let ucis: Vec<String> = moves.iter().map(|m| pos.uci(*m)).collect();
        assert_eq!(ucis, ["e2e4", "d2d4", "g1f3", "c2c4"]);
    }

    #[test]
    fn empty_text_clears_and_duplicates_count_once() {
        let pos = Position::new();
        assert_eq!(parse_exclusion(&pos, "  ,, "), Ok(Vec::new()));
        assert_eq!(parse_exclusion(&pos, "e4 e4").expect("legal").len(), 1);
    }

    #[test]
    fn an_illegal_or_unreadable_token_is_named() {
        let pos = Position::new();
        assert_eq!(
            parse_exclusion(&pos, "e4 Nf6"),
            Err(ExcludeError::Invalid("Nf6".to_owned()))
        );
        assert_eq!(
            parse_exclusion(&pos, "zzz"),
            Err(ExcludeError::Invalid("zzz".to_owned()))
        );
        let long = "x".repeat(200);
        let Err(ExcludeError::Invalid(text)) = parse_exclusion(&pos, &long) else {
            panic!("expected an invalid token");
        };
        assert_eq!(text.chars().count(), 64);
    }

    #[test]
    fn excluding_every_legal_move_is_refused() {
        // Black has two legal moves: the pawn push and Kb8.
        let pos = Position::from_fen("k7/8/1K5p/8/8/8/8/1R6 b - - 0 1").expect("fen");
        assert_eq!(pos.legal_moves().len(), 2);
        assert_eq!(parse_exclusion(&pos, "h5").map(|m| m.len()), Ok(1));
        assert_eq!(parse_exclusion(&pos, "h5, Kb8"), Err(ExcludeError::All));
    }

    #[test]
    fn the_search_moves_are_the_legal_moves_left() {
        let pos = Position::new();
        let excluded = parse_exclusion(&pos, "e4").expect("legal");
        let rest = search_moves_without(&pos, &excluded);
        assert_eq!(rest.len(), 19);
        assert!(!rest.contains(&excluded[0]));
    }
}
