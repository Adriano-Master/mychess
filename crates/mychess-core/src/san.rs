//! SAN reading (strict and tolerant, like the Scid) and writing.

use shakmaty::san::San;
use shakmaty::{CastlingSide, File, Rank, Role, Square};

use crate::error::{MoveError, SanError, clip};
use crate::moves::Move;
use crate::position::Position;

/// Longest text accepted as a single SAN move.
const MAX_SAN_LEN: usize = 32;

/// How strictly [`Position::parse_san`] reads a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SanMode {
    /// Canonical SAN only: `O-O`, `Nbd7`, `exd5`, `e8=Q`, `--`, optionally
    /// followed by `+` or `#`. Castling must be legal and a promotion piece
    /// is mandatory.
    Strict,
    /// Everything seen in real-world PGN files: `0-0`, `OO`, `Pe4`, `ed`,
    /// `e2e4`, `Ng1-f3`, `e8Q`, a missing promotion piece (queen), a lower
    /// case `b` for a bishop, trailing `e.p.`, `!`/`?` marks, `Z0` / `null` /
    /// `0000` for the null move, and castling after the right was lost when
    /// the king and rook are still on their squares and the move is
    /// otherwise possible (reported as [`SanWarning::IllegalCastling`]).
    ///
    /// An impossible short castling written without hyphens (`OO`, `00`) is
    /// read as long castling and reported as
    /// [`SanWarning::CastlingReinterpreted`]. The Scid applies this fallback
    /// to `O-O` as well; here it is limited to the unhyphenated spelling, a
    /// deliberate and safer restriction.
    Tolerant,
}

/// A non-fatal remark about a successfully read move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SanWarning {
    /// Castling was accepted although the position no longer grants the
    /// right. Use [`Position::play_lenient`] to play such a move.
    IllegalCastling,
    /// Short castling (`OO` / `00`, without hyphens) was impossible and was
    /// read as long castling instead, as the Scid does.
    CastlingReinterpreted,
}

/// The result of reading a SAN move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanParse {
    /// The move.
    pub mv: Move,
    /// Remarks about tolerated irregularities.
    pub warnings: Vec<SanWarning>,
}

/// Which check or mate marker [`Position::san_with`] appends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SanSuffix {
    /// No marker.
    None,
    /// `+` for check and for mate.
    Check,
    /// `+` for check and `#` for mate.
    CheckOrMate,
}

/// Mutable state while reading one move.
#[derive(Default)]
struct Flags {
    /// Non-canonical syntax was used.
    lenient: bool,
    /// The text contained an `x`.
    capture_marked: bool,
    warnings: Vec<SanWarning>,
}

/// Error construction with the text and position attached.
struct Ctx<'a> {
    pos: &'a Position,
    text: &'a str,
}

impl Ctx<'_> {
    fn syntax(&self) -> SanError {
        SanError::Syntax {
            text: clip(self.text),
        }
    }
    fn illegal(&self) -> SanError {
        SanError::Illegal {
            text: clip(self.text),
            fen: self.pos.fen(),
        }
    }
    fn ambiguous(&self) -> SanError {
        SanError::Ambiguous {
            text: clip(self.text),
            fen: self.pos.fen(),
        }
    }
    fn invalid_promotion(&self) -> SanError {
        SanError::InvalidPromotion {
            text: clip(self.text),
        }
    }
}

impl Position {
    /// Reads one SAN move (no move number, no comments) in this position.
    ///
    /// See [`SanMode`] for what each mode accepts. Trailing `+`, `#`, `!` and
    /// `?` are ignored, not validated. Never panics, whatever the input.
    ///
    /// ```
    /// use mychess_core::{Position, SanMode};
    ///
    /// let pos = Position::new();
    /// let parsed = pos.parse_san("Nf3", SanMode::Strict)?;
    /// assert_eq!(pos.uci(parsed.mv), "g1f3");
    /// assert!(pos.parse_san("Nf6", SanMode::Strict).is_err());
    /// # Ok::<(), mychess_core::SanError>(())
    /// ```
    pub fn parse_san(&self, text: &str, mode: SanMode) -> Result<SanParse, SanError> {
        let original = text.trim();
        if original.is_empty() {
            return Err(SanError::Empty);
        }
        if original.len() > MAX_SAN_LEN {
            return Err(SanError::TooLong {
                len: original.len(),
            });
        }
        let ctx = Ctx {
            pos: self,
            text: original,
        };
        if !original.is_ascii() {
            return Err(ctx.syntax());
        }
        let mut flags = Flags::default();
        let core = strip_decorations(original, &mut flags);
        if core.is_empty() || core.bytes().any(|b| b.is_ascii_whitespace()) {
            return Err(ctx.syntax());
        }

        let mv = if let Some(mv) = self.parse_null(&ctx, core, &mut flags)? {
            mv
        } else if let Some(mv) = self.parse_castle(&ctx, core, mode, &mut flags)? {
            mv
        } else {
            self.parse_regular(&ctx, core, mode, &mut flags)?
        };

        if mode == SanMode::Strict {
            let capture_mismatch = !matches!(mv, Move::Null | Move::Castle { .. })
                && flags.capture_marked != mv.is_capture();
            // Redundant disambiguation (`Ng1f3`, `Ngf3`) is not canonical: the
            // text must equal what `san_with` writes (without `+` / `#`).
            let redundant = !matches!(mv, Move::Null | Move::Castle { .. })
                && self
                    .san_with(mv, SanSuffix::None)
                    .map_or(true, |canonical| canonical != core);
            if flags.lenient || capture_mismatch || redundant {
                return Err(SanError::NonCanonical {
                    text: clip(original),
                });
            }
        }
        Ok(SanParse {
            mv,
            warnings: flags.warnings,
        })
    }

    /// Writes `m` in SAN with `+` / `#` suffixes ([`SanSuffix::CheckOrMate`]).
    ///
    /// The move must be legal in this position, or a castling that
    /// [`Position::play_lenient`] accepts (a lost right, as kept by
    /// [`Game::add_move_lenient`](crate::Game::add_move_lenient)).
    ///
    /// ```
    /// use mychess_core::Position;
    ///
    /// let pos = Position::new();
    /// let m = pos.parse_uci("e2e4")?;
    /// assert_eq!(pos.san(m)?, "e4");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn san(&self, m: Move) -> Result<String, MoveError> {
        self.san_with(m, SanSuffix::CheckOrMate)
    }

    /// Writes `m` in SAN with the chosen suffix style.
    ///
    /// Pieces are disambiguated only against other *legal* moves, preferring
    /// the file, then the rank, then both. Castling is `O-O` / `O-O-O` and the
    /// null move is `--`. A castling whose right was lost (kept by
    /// `Game::add_move_lenient`) is written too.
    pub fn san_with(&self, m: Move, suffix: SanSuffix) -> Result<String, MoveError> {
        // A castling whose right was lost is writable too (PGN round-trip).
        let after = {
            let mut next = self.clone();
            next.play_lenient(m)
                .map(|()| next)
                .map_err(|_| MoveError::Illegal {
                    uci: self.uci(m),
                    fen: self.fen(),
                })?
        };
        let mut out = match m {
            Move::Null => return Ok("--".to_owned()),
            Move::Castle { .. } if m.castling_side() == Some(CastlingSide::KingSide) => {
                "O-O".to_owned()
            }
            Move::Castle { .. } => "O-O-O".to_owned(),
            other => match other.to_shakmaty() {
                Some(sm) => San::from_move(&self.inner, sm).to_string(),
                None => return Ok("--".to_owned()),
            },
        };
        if suffix != SanSuffix::None {
            if after.is_checkmate() {
                out.push(if suffix == SanSuffix::CheckOrMate {
                    '#'
                } else {
                    '+'
                });
            } else if after.is_check() {
                out.push('+');
            }
        }
        Ok(out)
    }

    fn parse_null(
        &self,
        ctx: &Ctx<'_>,
        core: &str,
        flags: &mut Flags,
    ) -> Result<Option<Move>, SanError> {
        let canonical = core == "--";
        let alias =
            core == "0000" || core.eq_ignore_ascii_case("null") || core.eq_ignore_ascii_case("z0");
        if !canonical && !alias {
            return Ok(None);
        }
        if alias {
            flags.lenient = true;
        }
        if self.is_check() {
            return Err(SanError::NullMoveInCheck {
                text: clip(ctx.text),
                fen: self.fen(),
            });
        }
        Ok(Some(Move::Null))
    }

    fn parse_castle(
        &self,
        ctx: &Ctx<'_>,
        core: &str,
        mode: SanMode,
        flags: &mut Flags,
    ) -> Result<Option<Move>, SanError> {
        if !core.bytes().all(|b| matches!(b, b'O' | b'o' | b'0' | b'-')) {
            return Ok(None);
        }
        let letters = core.bytes().filter(|b| *b != b'-').count();
        let hyphens = core.len() - letters;
        let side = match letters {
            2 => CastlingSide::KingSide,
            3 => CastlingSide::QueenSide,
            _ => return Ok(None),
        };
        if core != "O-O" && core != "O-O-O" {
            flags.lenient = true;
        }
        let find = |pos: &Position, side: CastlingSide| {
            pos.legal_moves()
                .into_iter()
                .find(|m| m.castling_side() == Some(side))
        };
        if let Some(m) = find(self, side) {
            return Ok(Some(m));
        }
        let unhyphenated_short = side == CastlingSide::KingSide && hyphens == 0;
        if mode == SanMode::Tolerant {
            // The Scid reads a failing "OO" as queen-side castling.
            if unhyphenated_short && let Some(m) = find(self, CastlingSide::QueenSide) {
                flags.warnings.push(SanWarning::CastlingReinterpreted);
                return Ok(Some(m));
            }
            let mut sides = vec![side];
            if unhyphenated_short {
                sides.push(CastlingSide::QueenSide);
            }
            for s in sides {
                if let Some(granted) = self.with_castling_right(s)
                    && let Some(m) = find(&granted, s)
                {
                    if s != side {
                        flags.warnings.push(SanWarning::CastlingReinterpreted);
                    }
                    flags.warnings.push(SanWarning::IllegalCastling);
                    return Ok(Some(m));
                }
            }
        }
        Err(SanError::IllegalCastling {
            text: clip(ctx.text),
            fen: self.fen(),
        })
    }

    fn parse_regular(
        &self,
        ctx: &Ctx<'_>,
        core: &str,
        mode: SanMode,
        flags: &mut Flags,
    ) -> Result<Move, SanError> {
        // Drop separators the Scid ignores inside a move.
        let cleaned: Vec<u8> = core
            .bytes()
            .filter(|b| !matches!(b, b'-' | b':' | b'.'))
            .collect();
        if cleaned.len() != core.len() {
            flags.lenient = true;
        }
        let moves = self.legal_moves();
        let first = *cleaned.first().ok_or_else(|| ctx.syntax())?;
        match first {
            b'K' | b'Q' | b'R' | b'B' | b'N' => {
                let role = role_from_upper(first).ok_or_else(|| ctx.syntax())?;
                piece_move(ctx, &moves, role, &cleaned[1..], flags)
            }
            b'P' => {
                flags.lenient = true;
                pawn_move(ctx, &moves, &cleaned[1..], flags)
            }
            b'a'..=b'h' => {
                let mut pawn_flags = Flags::default();
                match pawn_move(ctx, &moves, &cleaned, &mut pawn_flags) {
                    Ok(m) => {
                        merge(flags, pawn_flags);
                        Ok(m)
                    }
                    Err(err @ SanError::Illegal { .. })
                        if first == b'b' && mode == SanMode::Tolerant =>
                    {
                        let mut bishop_flags = Flags::default();
                        match piece_move(
                            ctx,
                            &moves,
                            Role::Bishop,
                            &cleaned[1..],
                            &mut bishop_flags,
                        ) {
                            Ok(m) => {
                                merge(flags, bishop_flags);
                                flags.lenient = true;
                                Ok(m)
                            }
                            Err(_) => Err(err),
                        }
                    }
                    Err(err) => Err(err),
                }
            }
            _ => Err(ctx.syntax()),
        }
    }
}

fn merge(into: &mut Flags, from: Flags) {
    into.lenient |= from.lenient;
    into.capture_marked |= from.capture_marked;
    into.warnings.extend(from.warnings);
}

/// Removes trailing check / mate / annotation marks and `e.p.` and records
/// whether anything non-canonical was removed.
fn strip_decorations<'a>(text: &'a str, flags: &mut Flags) -> &'a str {
    let mut s = text;
    loop {
        let before = s.len();
        let trimmed = s.trim_end_matches(['+', '#']);
        s = trimmed;
        let trimmed = s.trim_end_matches(['!', '?']);
        if trimmed.len() != s.len() {
            flags.lenient = true;
            s = trimmed;
        }
        let trimmed = s.trim_end();
        if trimmed.len() != s.len() {
            // Space between the move and its `+` / `#` / `!` / `?` marks.
            flags.lenient = true;
            s = trimmed;
        }
        for suffix in ["e.p.", "ep"] {
            if s.len() > suffix.len()
                && s.is_char_boundary(s.len() - suffix.len())
                && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
            {
                s = &s[..s.len() - suffix.len()];
                flags.lenient = true;
            }
        }
        if s.len() == before {
            return s;
        }
    }
}

fn role_from_upper(b: u8) -> Option<Role> {
    match b {
        b'K' => Some(Role::King),
        b'Q' => Some(Role::Queen),
        b'R' => Some(Role::Rook),
        b'B' => Some(Role::Bishop),
        b'N' => Some(Role::Knight),
        _ => None,
    }
}

fn file_of(b: u8) -> Option<File> {
    (b'a'..=b'h')
        .contains(&b)
        .then(|| File::new(u32::from(b - b'a')))
}

fn rank_of(b: u8) -> Option<Rank> {
    (b'1'..=b'8')
        .contains(&b)
        .then(|| Rank::new(u32::from(b - b'1')))
}

fn square_of(file: u8, rank: u8) -> Option<Square> {
    Some(Square::from_coords(file_of(file)?, rank_of(rank)?))
}

/// Picks the single move of `candidates`, or reports no / several matches.
fn unique(ctx: &Ctx<'_>, candidates: &[Move]) -> Result<Move, SanError> {
    match candidates {
        [] => Err(ctx.illegal()),
        [m] => Ok(*m),
        _ => Err(ctx.ambiguous()),
    }
}

/// Reads `[disambiguation][x]<square>` after a piece letter.
fn piece_move(
    ctx: &Ctx<'_>,
    moves: &[Move],
    role: Role,
    rest: &[u8],
    flags: &mut Flags,
) -> Result<Move, SanError> {
    let [body @ .., f, r] = rest else {
        return Err(ctx.syntax());
    };
    let to = square_of(*f, *r).ok_or_else(|| ctx.syntax())?;
    let body = match body {
        [head @ .., b'x'] => {
            flags.capture_marked = true;
            head
        }
        other => other,
    };
    let (from_file, from_rank) = match *body {
        [] => (None, None),
        [c] => match (file_of(c), rank_of(c)) {
            (Some(f), _) => (Some(f), None),
            (_, Some(r)) => (None, Some(r)),
            _ => return Err(ctx.syntax()),
        },
        [c, d] => (
            Some(file_of(c).ok_or_else(|| ctx.syntax())?),
            Some(rank_of(d).ok_or_else(|| ctx.syntax())?),
        ),
        _ => return Err(ctx.syntax()),
    };
    let candidates: Vec<Move> = moves
        .iter()
        .copied()
        .filter(|m| match m {
            Move::Normal {
                role: r,
                from,
                to: t,
                ..
            } => {
                *r == role
                    && *t == to
                    && from_file.is_none_or(|f| f == from.file())
                    && from_rank.is_none_or(|r| r == from.rank())
            }
            _ => false,
        })
        .collect();
    unique(ctx, &candidates)
}

/// The shape of a pawn move after promotion and `x` are removed.
enum PawnForm {
    /// `e4`: a push to a square.
    Push(Square),
    /// `ed`: capture from a file to a file.
    CaptureFiles(File, File),
    /// `ed5` / `exd5`: capture from a file onto a square.
    CaptureSquare(File, Square),
    /// `e2e4`: coordinates, any piece.
    Coordinates(Square, Square),
}

/// Splits a promotion suffix (`=Q`, `Q`, `q`) off a pawn move, returning the
/// length of the remaining text and the promotion piece.
fn split_promotion(
    ctx: &Ctx<'_>,
    text: &[u8],
    flags: &mut Flags,
) -> Result<(usize, Option<Role>), SanError> {
    let mut end = text.len();
    if end < 3 {
        return Ok((end, None));
    }
    let last = text[end - 1];
    let prev = text[end - 2];
    let upper = last.to_ascii_uppercase();
    if !matches!(upper, b'Q' | b'R' | b'B' | b'N' | b'K' | b'P')
        || !(prev.is_ascii_digit() || prev == b'=')
    {
        return Ok((end, None));
    }
    let role = match upper {
        b'Q' => Role::Queen,
        b'R' => Role::Rook,
        b'B' => Role::Bishop,
        b'N' => Role::Knight,
        _ => return Err(ctx.invalid_promotion()),
    };
    end -= 1;
    if prev == b'=' {
        end -= 1;
    } else {
        flags.lenient = true;
    }
    if last.is_ascii_lowercase() {
        flags.lenient = true;
    }
    Ok((end, Some(role)))
}

/// Reads a pawn move (or a coordinate move) written without a leading piece
/// letter.
fn pawn_move(
    ctx: &Ctx<'_>,
    moves: &[Move],
    text: &[u8],
    flags: &mut Flags,
) -> Result<Move, SanError> {
    let (end, promotion) = split_promotion(ctx, text, flags)?;
    let body: Vec<u8> = text[..end]
        .iter()
        .copied()
        .filter(|b| {
            if *b == b'x' {
                flags.capture_marked = true;
                false
            } else {
                true
            }
        })
        .collect();

    let form = match *body.as_slice() {
        [f, r] => match (square_of(f, r), file_of(f), file_of(r)) {
            (Some(sq), _, _) => PawnForm::Push(sq),
            (_, Some(a), Some(b)) => PawnForm::CaptureFiles(a, b),
            _ => return Err(ctx.syntax()),
        },
        [a, f, r] => PawnForm::CaptureSquare(
            file_of(a).ok_or_else(|| ctx.syntax())?,
            square_of(f, r).ok_or_else(|| ctx.syntax())?,
        ),
        [a, b, c, d] => {
            flags.lenient = true;
            PawnForm::Coordinates(
                square_of(a, b).ok_or_else(|| ctx.syntax())?,
                square_of(c, d).ok_or_else(|| ctx.syntax())?,
            )
        }
        _ => return Err(ctx.syntax()),
    };
    if matches!(form, PawnForm::CaptureFiles(..)) {
        flags.lenient = true;
    }
    if matches!(form, PawnForm::CaptureSquare(..)) && !flags.capture_marked {
        flags.lenient = true;
    }

    let candidates: Vec<Move> = moves
        .iter()
        .copied()
        .filter(|m| match (&form, m) {
            (
                PawnForm::Push(to),
                Move::Normal {
                    role: Role::Pawn,
                    from,
                    to: t,
                    ..
                },
            ) => t == to && from.file() == to.file(),
            (PawnForm::CaptureFiles(a, b), m) => {
                is_pawn_capture(*m)
                    && m.from().is_some_and(|f| f.file() == *a)
                    && m.to().is_some_and(|t| t.file() == *b)
            }
            (PawnForm::CaptureSquare(a, to), m) => {
                is_pawn_capture(*m)
                    && m.from().is_some_and(|f| f.file() == *a)
                    && m.to() == Some(*to)
            }
            (PawnForm::Coordinates(from, to), m) => {
                m.from() == Some(*from)
                    && (m.to() == Some(*to)
                        || matches!(m, Move::Castle { rook_from, .. } if rook_from == to))
            }
            _ => false,
        })
        .collect();

    if let Some(role) = promotion {
        if !candidates.iter().any(|m| m.is_promotion()) {
            return if candidates.is_empty() {
                Err(ctx.illegal())
            } else {
                Err(ctx.invalid_promotion())
            };
        }
        let chosen: Vec<Move> = candidates
            .into_iter()
            .filter(|m| m.promotion() == Some(role))
            .collect();
        return unique(ctx, &chosen);
    }
    if candidates.iter().all(|m| m.is_promotion()) && !candidates.is_empty() {
        // A pawn reaching the last rank without a piece letter: queen.
        flags.lenient = true;
        let chosen: Vec<Move> = candidates
            .into_iter()
            .filter(|m| m.promotion() == Some(Role::Queen))
            .collect();
        return unique(ctx, &chosen);
    }
    unique(ctx, &candidates)
}

fn is_pawn_capture(m: Move) -> bool {
    matches!(
        m,
        Move::EnPassant { .. }
            | Move::Normal {
                role: Role::Pawn,
                capture: Some(_),
                ..
            }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).unwrap()
    }

    fn tolerant(p: &Position, text: &str) -> Result<Move, SanError> {
        p.parse_san(text, SanMode::Tolerant).map(|r| r.mv)
    }

    fn uci_of(p: &Position, text: &str) -> String {
        p.uci(tolerant(p, text).unwrap())
    }

    #[rstest]
    #[case("e4", "e2e4")]
    #[case("Nf3", "g1f3")]
    #[case("Pe4", "e2e4")]
    #[case("e2e4", "e2e4")]
    #[case("e2-e4", "e2e4")]
    #[case("Ng1-f3", "g1f3")]
    #[case("Ng1f3", "g1f3")]
    #[case("Nf3+", "g1f3")]
    #[case("e4!?", "e2e4")]
    fn opening_moves(#[case] text: &str, #[case] uci: &str) {
        assert_eq!(uci_of(&Position::new(), text), uci);
    }

    #[test]
    fn strict_accepts_canonical_and_rejects_the_rest() {
        let p = Position::new();
        for ok in ["e4", "Nf3", "Nf3+", "e4#"] {
            assert!(p.parse_san(ok, SanMode::Strict).is_ok(), "{ok}");
        }
        for bad in ["Pe4", "e2e4", "Ng1-f3", "e4!", "Z0"] {
            assert!(
                matches!(
                    p.parse_san(bad, SanMode::Strict),
                    Err(SanError::NonCanonical { .. } | SanError::NullMoveInCheck { .. })
                ),
                "{bad}"
            );
        }
        let c = pos("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1");
        assert!(c.parse_san("exd5", SanMode::Strict).is_ok());
        assert!(c.parse_san("ed", SanMode::Strict).is_err());
        assert!(c.parse_san("ed5", SanMode::Strict).is_err());
        assert_eq!(uci_of(&c, "ed"), "e4d5");
        assert_eq!(uci_of(&c, "ed5"), "e4d5");
        assert_eq!(uci_of(&c, "exd5"), "e4d5");
    }

    #[test]
    fn strict_rejects_redundant_disambiguation_but_tolerant_accepts() {
        let p = Position::new();
        for bad in ["Ng1f3", "Ngf3", "N1f3"] {
            assert!(
                matches!(
                    p.parse_san(bad, SanMode::Strict),
                    Err(SanError::NonCanonical { .. })
                ),
                "{bad}"
            );
            let parsed = p.parse_san(bad, SanMode::Tolerant).unwrap();
            assert!(parsed.warnings.is_empty(), "{bad}");
        }
        assert!(p.parse_san("Nf3", SanMode::Strict).is_ok());
    }

    #[test]
    fn castling_spellings() {
        let p = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        for text in ["O-O", "0-0", "o-o", "OO", "00", "O-O+", "0-0#"] {
            assert_eq!(uci_of(&p, text), "e1g1", "{text}");
        }
        for text in ["O-O-O", "0-0-0", "o-o-o", "OOO", "000"] {
            assert_eq!(uci_of(&p, text), "e1c1", "{text}");
        }
        assert!(p.parse_san("O-O", SanMode::Strict).is_ok());
        assert!(p.parse_san("0-0", SanMode::Strict).is_err());
    }

    #[test]
    fn oo_falls_back_to_long_castling_like_the_scid() {
        let p = pos("r3k2r/8/8/8/8/8/8/R3K2R w Qkq - 0 1");
        assert_eq!(uci_of(&p, "OO"), "e1c1");
        assert!(matches!(
            p.parse_san("O-O", SanMode::Strict),
            Err(SanError::IllegalCastling { .. })
        ));
    }

    #[test]
    fn tolerant_castling_without_right_warns() {
        let p = pos("r3k2r/8/8/8/8/8/8/R3K2R w kq - 0 1");
        assert!(matches!(
            p.parse_san("O-O", SanMode::Strict),
            Err(SanError::IllegalCastling { .. })
        ));
        let r = p.parse_san("O-O", SanMode::Tolerant).unwrap();
        assert_eq!(r.warnings, vec![SanWarning::IllegalCastling]);
        let mut q = p.clone();
        assert!(q.play(r.mv).is_err());
        q.play_lenient(r.mv).unwrap();
        assert_eq!(q.fen(), "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1");
    }

    #[test]
    fn tolerant_castling_still_refuses_the_impossible() {
        // Rook missing, path blocked, in check, through an attacked square.
        for fen in [
            "r3k2r/8/8/8/8/8/8/4K3 w - - 0 1",
            "r3k2r/8/8/8/8/8/8/R3K1NR w - - 0 1",
            "r3k2r/8/8/8/8/8/4r3/R3K2R w - - 0 1",
            "r3k2r/8/8/8/8/8/5r2/R3K2R w - - 0 1",
        ] {
            let p = pos(fen);
            assert!(
                matches!(
                    p.parse_san("O-O", SanMode::Tolerant),
                    Err(SanError::IllegalCastling { .. })
                ),
                "{fen}"
            );
        }
    }

    #[test]
    fn promotions() {
        let p = pos("3r2k1/4P3/8/8/8/8/8/4K3 w - - 0 1");
        for (text, uci) in [
            ("e8=Q", "e7e8q"),
            ("e8Q", "e7e8q"),
            ("e8q", "e7e8q"),
            ("e8=N", "e7e8n"),
            ("e8", "e7e8q"),
            ("exd8=R", "e7d8r"),
            ("exd8B", "e7d8b"),
            ("ed8=N+", "e7d8n"),
            ("e7e8q", "e7e8q"),
        ] {
            assert_eq!(uci_of(&p, text), uci, "{text}");
        }
        assert!(matches!(
            p.parse_san("e8=K", SanMode::Tolerant),
            Err(SanError::InvalidPromotion { .. })
        ));
        assert!(matches!(
            p.parse_san("e8=P", SanMode::Tolerant),
            Err(SanError::InvalidPromotion { .. })
        ));
        assert!(matches!(
            p.parse_san("e8", SanMode::Strict),
            Err(SanError::NonCanonical { .. })
        ));
        let q = Position::new();
        assert!(matches!(
            q.parse_san("e4=Q", SanMode::Tolerant),
            Err(SanError::InvalidPromotion { .. })
        ));
    }

    #[test]
    fn disambiguation_and_errors() {
        let p = pos("4k3/8/8/8/8/2N3N1/8/4K3 w - - 0 1");
        assert!(matches!(
            p.parse_san("Ne2", SanMode::Tolerant),
            Err(SanError::Ambiguous { .. })
        ));
        assert_eq!(uci_of(&p, "Nge2"), "g3e2");
        assert_eq!(uci_of(&p, "Nce2"), "c3e2");
        assert_eq!(uci_of(&p, "Nc3e2"), "c3e2");
        assert_eq!(uci_of(&p, "Nc3-e2"), "c3e2");
        assert!(matches!(
            p.parse_san("Ng7", SanMode::Tolerant),
            Err(SanError::Illegal { .. })
        ));
    }

    #[test]
    fn lowercase_b_can_be_a_bishop() {
        let p = pos("4k3/8/8/8/8/2p5/3B4/4K3 w - - 0 1");
        assert_eq!(uci_of(&p, "bxc3"), "d2c3");
        assert!(p.parse_san("bxc3", SanMode::Strict).is_err());
    }

    #[test]
    fn null_move_spellings() {
        let p = Position::new();
        for text in ["--", "Z0", "z0", "null", "0000"] {
            assert_eq!(tolerant(&p, text), Ok(Move::Null), "{text}");
        }
        assert_eq!(p.parse_san("--", SanMode::Strict).unwrap().mv, Move::Null);
        assert!(p.parse_san("Z0", SanMode::Strict).is_err());
        let check = pos("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1");
        assert!(matches!(
            tolerant(&check, "--"),
            Err(SanError::NullMoveInCheck { .. })
        ));
    }

    #[test]
    fn en_passant_suffixes_are_ignored() {
        let p = pos("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1");
        for text in ["exd6", "exd6ep", "exd6 e.p.", "exd6e.p.+"] {
            assert_eq!(uci_of(&p, text), "e5d6", "{text}");
        }
    }

    #[test]
    fn hostile_input_is_an_error_not_a_panic() {
        let p = Position::new();
        let long = "x".repeat(10_000);
        for text in [
            "", " ", "+", "#", "=", "x", "xx", "Q", "N", "Nx", "e", "e9", "i4", "\u{0}", "é4", "O",
            "OOOO", "--x", "ep", "e.p.", "=Q", "e8=", "ed=", "N@e4", &long,
        ] {
            assert!(tolerant(&p, text).is_err(), "{text:?}");
            assert!(p.parse_san(text, SanMode::Strict).is_err(), "{text:?}");
        }
    }

    #[test]
    fn writes_san_with_suffixes() {
        let p = pos("rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq - 0 2");
        let m = tolerant(&p, "Qh4").unwrap();
        assert_eq!(p.san(m).unwrap(), "Qh4#");
        assert_eq!(p.san_with(m, SanSuffix::Check).unwrap(), "Qh4+");
        assert_eq!(p.san_with(m, SanSuffix::None).unwrap(), "Qh4");
        let q = pos("4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
        let m = tolerant(&q, "Ra8").unwrap();
        assert_eq!(q.san(m).unwrap(), "Ra8+");
    }

    #[test]
    fn writes_pawn_castle_promotion_and_null() {
        let p = pos("r3k2r/1P6/8/3pP3/8/8/8/R3K2R w KQkq d6 0 1");
        let cases = [
            ("e5d6", "exd6"),
            ("e5e6", "e6"),
            ("e1g1", "O-O"),
            ("e1c1", "O-O-O"),
        ];
        for (uci, san) in cases {
            let m = p.parse_uci(uci).unwrap();
            assert_eq!(p.san(m).unwrap(), san);
        }
        let m = p.parse_uci("b7b8q").unwrap();
        assert_eq!(p.san(m).unwrap(), "b8=Q+");
        let m = p.parse_uci("b7a8n").unwrap();
        assert_eq!(p.san_with(m, SanSuffix::None).unwrap(), "bxa8=N");
        assert_eq!(p.san(Move::Null).unwrap(), "--");
    }

    #[test]
    fn writes_disambiguation_like_the_scid() {
        // Three queens reach c3: a1 shares a file and a rank with the others.
        let p = pos("4k3/8/8/8/8/Q7/8/Q1Q4K w - - 0 1");
        let san = |uci: &str| p.san(p.parse_uci(uci).unwrap()).unwrap();
        assert_eq!(san("a1c3"), "Qa1c3");
        assert_eq!(san("a3c3"), "Q3c3");
        assert_eq!(san("c1c3"), "Qcc3");
        // Only one of two knights can move (the other is pinned): no marker.
        let q = pos("3k4/8/8/8/8/1n6/3n4/3RK3 b - - 0 1");
        assert_eq!(q.san(q.parse_uci("b3c1").unwrap()).unwrap(), "Nc1");
    }

    #[test]
    fn illegal_move_cannot_be_written() {
        let p = Position::new();
        let bad = Move::Normal {
            role: Role::Pawn,
            from: Square::E2,
            to: Square::E5,
            capture: None,
            promotion: None,
        };
        assert!(matches!(p.san(bad), Err(MoveError::Illegal { .. })));
    }
}
