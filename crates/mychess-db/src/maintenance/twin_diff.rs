// Derived from Scid vs PC 4.27, tcl/file/maint.tcl (`updateTwinChecker`
// 999-1434, the highlighting of the differences). Copyright (c) 1999-2004
// Shane Hudson; Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The side-by-side comparison of the twin checker (decision D3513, rule 25 of
//! `docs/specs/m7-1-manutencao.md`).
//!
//! Both games are written as PGN text (`WriteOptions::search_text`) and cut in
//! *tokens*: one per header line, then one per move number (`1.`, `1...`),
//! move, NAG (`!`, `?!`, `$14`), comment (`{...}`), `(` and `)`. A move that
//! differs only by its NAG marks the NAG, not the move. The tokens that are not in the longest common subsequence are
//! marked. This replaces the `regexp` of the Tcl code, which stops after 500
//! iterations per line (a defect not copied).

use mychess_core::Game;
use mychess_pgn::write::{WriteOptions, game_to_string};

use super::MaintError;

/// The most tokens (header lines plus movetext tokens) a side may have.
pub const MAX_DIFF_TOKENS: usize = 20_000;

/// Largest table of the alignment; beyond it the differing middle is marked
/// whole.
const MAX_TABLE: usize = 16_000_000;

/// A token and whether it differs from the other game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffToken {
    /// The text, as written in the PGN (`[Event "x"]` for a header line).
    pub text: String,
    /// Not in the common subsequence.
    pub differs: bool,
}

/// The result of [`diff_games`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameDiff {
    /// Both games compared.
    Compared {
        /// Header lines of the first game.
        header_a: Vec<DiffToken>,
        /// Header lines of the second game.
        header_b: Vec<DiffToken>,
        /// Movetext tokens of the first game.
        moves_a: Vec<DiffToken>,
        /// Movetext tokens of the second game.
        moves_b: Vec<DiffToken>,
    },
    /// A game has more than [`MAX_DIFF_TOKENS`] tokens: only the warning
    /// "games too large to compare" is shown.
    TooLarge {
        /// Tokens of the first game.
        tokens_a: usize,
        /// Tokens of the second game.
        tokens_b: usize,
    },
}

impl GameDiff {
    /// Whether any token is marked.
    pub fn has_differences(&self) -> bool {
        match self {
            GameDiff::Compared {
                header_a,
                header_b,
                moves_a,
                moves_b,
            } => [header_a, header_b, moves_a, moves_b]
                .iter()
                .any(|side| side.iter().any(|t| t.differs)),
            GameDiff::TooLarge { .. } => false,
        }
    }
}

/// Splits a whitespace-free word of the movetext: the move number (`12.`,
/// `12...`) and the `!`/`?` suffix of a move become tokens of their own.
fn push_word(word: &str, out: &mut Vec<String>) {
    let mut rest = word;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        let dots = rest
            .get(digits..)
            .map_or(0, |r| r.bytes().take_while(|&b| b == b'.').count());
        if dots > 0
            && let Some((number, tail)) = rest.split_at_checked(digits + dots)
        {
            out.push(number.to_owned());
            rest = tail;
        }
    }
    let body_len = rest.trim_end_matches(['!', '?']).len();
    match rest.split_at_checked(body_len) {
        Some((body, nag)) if !body.is_empty() && !nag.is_empty() => {
            out.push(body.to_owned());
            out.push(nag.to_owned());
        }
        _ if !rest.is_empty() => out.push(rest.to_owned()),
        _ => {}
    }
}

/// Splits the PGN of a game in header lines and movetext tokens.
fn tokens(game: &Game) -> Result<(Vec<String>, Vec<String>), MaintError> {
    let text = game_to_string(game, &WriteOptions::search_text())?;
    let (head, body) = text.split_once("\n\n").unwrap_or(("", text.as_str()));
    let header = head
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect();
    let mut out = Vec::new();
    let mut word = String::new();
    let mut chars = body.chars();
    let flush = |word: &mut String, out: &mut Vec<String>| {
        push_word(word, out);
        word.clear();
    };
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                flush(&mut word, &mut out);
                let mut comment = String::from('{');
                for d in chars.by_ref() {
                    comment.push(d);
                    if d == '}' {
                        break;
                    }
                }
                out.push(comment);
            }
            '(' | ')' => {
                flush(&mut word, &mut out);
                out.push(c.to_string());
            }
            c if c.is_whitespace() => flush(&mut word, &mut out),
            c => word.push(c),
        }
    }
    flush(&mut word, &mut out);
    Ok((header, out))
}

/// Marks the tokens outside the longest common subsequence of `a` and `b`.
fn mark(a: &[String], b: &[String]) -> (Vec<DiffToken>, Vec<DiffToken>) {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (ma, mb) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let mut keep_a = vec![false; ma.len()];
    let mut keep_b = vec![false; mb.len()];
    if !ma.is_empty() && !mb.is_empty() && ma.len().saturating_mul(mb.len()) <= MAX_TABLE {
        // table[i][j] = LCS length of ma[i..] and mb[j..]. Lengths fit in u16
        // because each side has at most MAX_DIFF_TOKENS tokens.
        let width = mb.len() + 1;
        let mut table = vec![0u16; (ma.len() + 1) * width];
        for i in (0..ma.len()).rev() {
            for j in (0..mb.len()).rev() {
                let value = if ma[i] == mb[j] {
                    table[(i + 1) * width + j + 1] + 1
                } else {
                    table[(i + 1) * width + j].max(table[i * width + j + 1])
                };
                table[i * width + j] = value;
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < ma.len() && j < mb.len() {
            if ma[i] == mb[j] {
                keep_a[i] = true;
                keep_b[j] = true;
                i += 1;
                j += 1;
            } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    let build = |all: &[String], prefix: usize, suffix: usize, keep: &[bool]| {
        let mut out = Vec::with_capacity(all.len());
        let mid = all.len() - suffix - prefix;
        for (k, text) in all.iter().enumerate() {
            let differs = k >= prefix && k < prefix + mid && !keep[k - prefix];
            out.push(DiffToken {
                text: text.clone(),
                differs,
            });
        }
        out
    };
    (
        build(a, prefix, suffix, &keep_a),
        build(b, prefix, suffix, &keep_b),
    )
}

/// Compares two games token by token (rule 25). Identical games have no mark;
/// a side with more than [`MAX_DIFF_TOKENS`] tokens gives
/// [`GameDiff::TooLarge`].
///
/// # Errors
///
/// [`MaintError::Write`] if a game cannot be written as PGN (an inconsistent
/// tree, which a valid [`Game`] never has). The error is never turned into an
/// empty text, which would make two broken games look identical.
pub fn diff_games(a: &Game, b: &Game) -> Result<GameDiff, MaintError> {
    let (header_a, moves_a) = tokens(a)?;
    let (header_b, moves_b) = tokens(b)?;
    let size_a = header_a.len() + moves_a.len();
    let size_b = header_b.len() + moves_b.len();
    if size_a > MAX_DIFF_TOKENS || size_b > MAX_DIFF_TOKENS {
        return Ok(GameDiff::TooLarge {
            tokens_a: size_a,
            tokens_b: size_b,
        });
    }
    let (ha, hb) = mark(&header_a, &header_b);
    let (ma, mb) = mark(&moves_a, &moves_b);
    Ok(GameDiff::Compared {
        header_a: ha,
        header_b: hb,
        moves_a: ma,
        moves_b: mb,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|&x| x.to_owned()).collect()
    }

    #[test]
    fn marks_only_the_tokens_outside_the_common_subsequence() {
        let (a, b) = mark(
            &s(&["1.", "e4", "e5", "2."]),
            &s(&["1.", "e4", "{c}", "e5", "2."]),
        );
        assert_eq!(
            a.iter().filter(|t| t.differs).count(),
            0,
            "the shorter side has nothing extra"
        );
        let marked: Vec<&str> = b
            .iter()
            .filter(|t| t.differs)
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(marked, ["{c}"]);
    }

    #[test]
    fn tokenizer_keeps_comments_whole() {
        let mut game = Game::new();
        let mv = game.start_position().parse_uci("e2e4").unwrap();
        let id = game.add_move(game.root(), mv).unwrap();
        game.set_comment(id, "a (b) c").unwrap();
        let (header, moves) = tokens(&game).unwrap();
        assert!(
            header.iter().any(|l| l.starts_with("[Event ")),
            "{header:?}"
        );
        assert!(
            moves
                .iter()
                .any(|t| t.starts_with("{a") && t.ends_with('}')),
            "{moves:?}"
        );
        assert!(!moves.iter().any(|t| t == "("), "{moves:?}");
    }

    #[test]
    fn move_numbers_and_nag_suffixes_are_tokens_of_their_own() {
        let mut out = Vec::new();
        for word in ["1.e4!?", "12...Nf3", "e5", "1-0", "$14", "+="] {
            push_word(word, &mut out);
        }
        assert_eq!(
            out,
            ["1.", "e4", "!?", "12...", "Nf3", "e5", "1-0", "$14", "+="]
        );
    }

    #[test]
    fn a_move_that_differs_only_by_its_nag_marks_the_nag() {
        let build = |nag: bool| {
            let mut game = Game::new();
            let mv = game.start_position().parse_uci("e2e4").unwrap();
            let id = game.add_move(game.root(), mv).unwrap();
            if nag {
                game.add_nag(id, mychess_core::Nag::new(1).unwrap())
                    .unwrap();
            }
            game
        };
        let (_, a) = tokens(&build(false)).unwrap();
        let (_, b) = tokens(&build(true)).unwrap();
        let (_, marked_b) = mark(&a, &b);
        let marked: Vec<&str> = marked_b
            .iter()
            .filter(|t| t.differs)
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(marked, ["!"]);
    }

    #[test]
    fn the_alignment_beyond_the_table_limit_marks_the_middle_whole() {
        // 4001 x 4001 tokens exceed MAX_TABLE: no common subsequence is
        // searched, but the common prefix and suffix are still kept.
        let mut a: Vec<String> = vec!["start".into()];
        let mut b: Vec<String> = vec!["start".into()];
        a.extend((0..4001).map(|i| format!("a{i}")));
        b.extend((0..4001).map(|i| format!("b{i}")));
        a.push("end".into());
        b.push("end".into());
        const { assert!(4001usize * 4001 > MAX_TABLE) };
        let (ma, mb) = mark(&a, &b);
        assert!(!ma[0].differs && !mb[0].differs);
        assert!(!ma[ma.len() - 1].differs && !mb[mb.len() - 1].differs);
        assert!(ma[1..ma.len() - 1].iter().all(|t| t.differs));
        assert!(mb[1..mb.len() - 1].iter().all(|t| t.differs));
    }
}
