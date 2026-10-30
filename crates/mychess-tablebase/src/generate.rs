//! Random positions of a given material for the training (spec m8-8, D2577),
//! the `Random` button of the Scid.
//!
//! The generator is deterministic: the same material and seed give the same
//! position. It does not need the tables; it only guarantees that the position
//! is legal and playable.

use mychess_core::{Position, Role};

use crate::error::TablebaseError;

/// Most pieces of a material, kings included.
pub const MAX_PIECES: usize = 7;

/// Why a position could not be generated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GenerateError {
    /// The text is not a material like `KRvKP`.
    #[error("not a material: {0}")]
    InvalidMaterial(String),
    /// More pieces than the tablebases cover.
    #[error("{0} pieces: at most 7")]
    TooManyPieces(usize),
    /// Fewer than three pieces.
    #[error("{0} pieces: at least 3")]
    TooFewPieces(usize),
    /// More than eight pawns of a side.
    #[error("more than eight pawns")]
    TooManyPawns,
    /// No legal position was found (should not happen for a sensible material).
    #[error("no legal position found")]
    NoLegalPosition,
}

impl From<GenerateError> for TablebaseError {
    fn from(error: GenerateError) -> TablebaseError {
        TablebaseError::Protocol(error.to_string())
    }
}

/// The pieces of each side besides the king.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Material {
    /// White pieces other than the king.
    pub white: Vec<Role>,
    /// Black pieces other than the king.
    pub black: Vec<Role>,
}

impl Material {
    /// Pieces on the board, kings included.
    #[must_use]
    pub fn pieces(&self) -> usize {
        2 + self.white.len() + self.black.len()
    }
}

fn parse_side(text: &str, whole: &str) -> Result<Vec<Role>, GenerateError> {
    let invalid = || GenerateError::InvalidMaterial(clip(whole));
    let mut kings = 0;
    let mut roles = Vec::new();
    for c in text.chars() {
        match c.to_ascii_uppercase() {
            'K' => kings += 1,
            'Q' => roles.push(Role::Queen),
            'R' => roles.push(Role::Rook),
            'B' => roles.push(Role::Bishop),
            'N' => roles.push(Role::Knight),
            'P' => roles.push(Role::Pawn),
            _ => return Err(invalid()),
        }
        if roles.len() > MAX_PIECES {
            return Err(GenerateError::TooManyPieces(roles.len() + 1));
        }
    }
    if kings != 1 {
        return Err(invalid());
    }
    Ok(roles)
}

fn clip(text: &str) -> String {
    text.chars().take(24).collect()
}

/// Reads a material such as `KQvK`, `krvkp` or `KRP-KR`: two sides separated
/// by `v` or `-`, each with exactly one king.
///
/// # Errors
/// [`GenerateError`].
pub fn parse_material(text: &str) -> Result<Material, GenerateError> {
    let trimmed = text.trim();
    let Some(split) = trimmed.find(['v', 'V', '-']) else {
        return Err(GenerateError::InvalidMaterial(clip(trimmed)));
    };
    let (white, black) = trimmed.split_at(split);
    let black = black.get(1..).unwrap_or("");
    let material = Material {
        white: parse_side(white, trimmed)?,
        black: parse_side(black, trimmed)?,
    };
    let pieces = material.pieces();
    if pieces > MAX_PIECES {
        return Err(GenerateError::TooManyPieces(pieces));
    }
    if pieces < 3 {
        return Err(GenerateError::TooFewPieces(pieces));
    }
    for side in [&material.white, &material.black] {
        if side.iter().filter(|r| **r == Role::Pawn).count() > 8 {
            return Err(GenerateError::TooManyPawns);
        }
    }
    Ok(material)
}

/// `SplitMix64`: a tiny, well-mixed generator (no dependency needed).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`n > 0`).
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

fn letter(role: Role) -> char {
    match role {
        Role::Pawn => 'p',
        Role::Knight => 'n',
        Role::Bishop => 'b',
        Role::Rook => 'r',
        Role::Queen => 'q',
        Role::King => 'k',
    }
}

/// How many times a position is tried before giving up.
const ATTEMPTS: usize = 2000;

/// A random legal position of `material` (D2577): the kings are not adjacent,
/// the side that does not move is not in check, there are no pawns on the
/// first or last rank, there are no castling rights and the game is not over.
///
/// # Errors
/// [`GenerateError`].
pub fn random_position(material: &str, seed: u64) -> Result<Position, GenerateError> {
    let material = parse_material(material)?;
    let mut rng = Rng(seed);
    for _ in 0..ATTEMPTS {
        if let Some(position) = attempt(&material, &mut rng) {
            return Ok(position);
        }
    }
    Err(GenerateError::NoLegalPosition)
}

fn attempt(material: &Material, rng: &mut Rng) -> Option<Position> {
    let mut board = [[None::<char>; 8]; 8];
    let mut place = |rng: &mut Rng, role: Role, white: bool| -> Option<(usize, usize)> {
        for _ in 0..64 {
            let file = usize::try_from(rng.below(8)).ok()?;
            let rank = if role == Role::Pawn {
                1 + usize::try_from(rng.below(6)).ok()?
            } else {
                usize::try_from(rng.below(8)).ok()?
            };
            let cell = board.get_mut(rank)?.get_mut(file)?;
            if cell.is_none() {
                let c = letter(role);
                *cell = Some(if white { c.to_ascii_uppercase() } else { c });
                return Some((file, rank));
            }
        }
        None
    };
    let white_king = place(rng, Role::King, true)?;
    let black_king = place(rng, Role::King, false)?;
    if white_king.0.abs_diff(black_king.0) <= 1 && white_king.1.abs_diff(black_king.1) <= 1 {
        return None;
    }
    for role in &material.white {
        place(rng, *role, true)?;
    }
    for role in &material.black {
        place(rng, *role, false)?;
    }
    let white_to_move = rng.below(2) == 0;
    let placement = (0..8)
        .rev()
        .map(|rank| {
            let mut text = String::new();
            let mut empty = 0u8;
            for file in 0..8 {
                match board.get(rank).and_then(|r| r.get(file)).copied().flatten() {
                    Some(c) => {
                        if empty > 0 {
                            text.push_str(&empty.to_string());
                            empty = 0;
                        }
                        text.push(c);
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                text.push_str(&empty.to_string());
            }
            text
        })
        .collect::<Vec<_>>()
        .join("/");
    let (mover, other) = if white_to_move {
        ("w", "b")
    } else {
        ("b", "w")
    };
    let position = Position::from_fen(&format!("{placement} {mover} - - 0 1")).ok()?;
    // The side that does not move must not be attacked: look at the same
    // board with the other side to move.
    let flipped = Position::from_fen(&format!("{placement} {other} - - 0 1")).ok()?;
    if flipped.is_check()
        || position.legal_moves().is_empty()
        || position.is_insufficient_material()
    {
        return None;
    }
    Some(position)
}
