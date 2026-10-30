//! Comments and NAGs (spec rules 62-68).

use mychess_core::{Game, GameError, Nag, NodeId};

use super::GameSession;

/// Groups of NAGs that exclude each other when
/// [`EditSettings::exclusive_nags`](super::EditSettings::exclusive_nags) is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NagCategory {
    /// Move quality, `$1`-`$6` (`!`, `?`, `!!`, `??`, `!?`, `?!`).
    MoveQuality,
    /// Position evaluation, `$10`-`$21` (`=` to `-+`).
    Evaluation,
    /// Anything else: independent of the other NAGs.
    Other,
}

/// The category of a NAG.
pub fn nag_category(nag: Nag) -> NagCategory {
    match nag.value() {
        1..=6 => NagCategory::MoveQuality,
        10..=21 => NagCategory::Evaluation,
        _ => NagCategory::Other,
    }
}

/// The keyboard shortcuts of the Scid (`tcl/end.tcl` 1759-1777) as
/// `(symbol, NAG number)`. `Nag::parse(symbol)` gives the number.
pub const NAG_SHORTCUTS: &[(&str, u8)] = &[
    ("!", 1),
    ("?", 2),
    ("!!", 3),
    ("??", 4),
    ("!?", 5),
    ("?!", 6),
    ("=", 10),
    ("~", 13),
    ("+=", 14),
    ("=+", 15),
    ("+/-", 16),
    ("-/+", 17),
    ("+-", 18),
    ("-+", 19),
    ("~=", 44),
];

/// Reads the NAG field of the comment editor: tokens separated by
/// whitespace, each a symbol (`!?`, `+-`) or a number (`$14`, `14`).
/// Unknown tokens are ignored; duplicates are kept out.
pub fn parse_nag_list(text: &str) -> Vec<Nag> {
    let mut out: Vec<Nag> = Vec::new();
    for token in text.split_whitespace() {
        // `Nag::parse` accepts any text starting with `x` as a NAG; in this
        // field a symbol must match exactly, so only numbers and the known
        // symbols are accepted.
        let numeric = token
            .strip_prefix('$')
            .unwrap_or(token)
            .chars()
            .all(|c| c.is_ascii_digit());
        let known = numeric || NAG_SYMBOLS_OK.contains(&token);
        if !known {
            continue;
        }
        if let Some(nag) = Nag::parse(token)
            && !out.contains(&nag)
        {
            out.push(nag);
        }
    }
    out
}

/// A token of the NAG field of the comment editor that is not a NAG.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown annotation `{token}`")]
pub struct NagFieldError {
    /// The offending token, clipped to 32 characters.
    pub token: String,
}

/// Reads the NAG field of the comment editor strictly (spec M2-4, D179):
/// like [`parse_nag_list`], but the first token that is not a NAG is an
/// error, so the editor can refuse to save and show it.
///
/// # Errors
///
/// [`NagFieldError`] for an unknown symbol, `$0`, `$256` or text.
pub fn parse_nag_field(text: &str) -> Result<Vec<Nag>, NagFieldError> {
    let mut out: Vec<Nag> = Vec::new();
    for token in text.split_whitespace() {
        let digits = token.strip_prefix('$').unwrap_or(token);
        let nag = if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            digits
                .parse::<u16>()
                .ok()
                .and_then(|number| Nag::new(number).ok())
        } else if NAG_SYMBOLS_OK.contains(&token) {
            Nag::parse(token)
        } else {
            None
        };
        let Some(nag) = nag else {
            return Err(NagFieldError {
                token: token.chars().take(32).collect(),
            });
        };
        if !out.contains(&nag) {
            out.push(nag);
        }
    }
    Ok(out)
}

/// The NAG field of the comment editor for `nags`: symbols that read back
/// to the same NAG, `$n` for the others, separated by spaces.
pub fn nag_field_text(nags: &[Nag]) -> String {
    let mut out = String::new();
    for nag in nags {
        if !out.is_empty() {
            out.push(' ');
        }
        match nag.ascii().filter(|symbol| NAG_SYMBOLS_OK.contains(symbol)) {
            Some(symbol) => out.push_str(symbol),
            None => out.push_str(&nag.to_string()),
        }
    }
    out
}

impl GameSession {
    pub(super) fn set_comment(&mut self, node: NodeId, text: &str) {
        drop(self.edit(|game, _| game.set_comment(node, text)));
    }

    pub(super) fn set_comment_before(&mut self, node: NodeId, text: &str) {
        drop(self.edit(|game, _| game.set_comment_before(node, text)));
    }

    pub(super) fn add_nag(&mut self, nag: Nag) {
        let exclusive = self.settings.exclusive_nags;
        drop(self.edit(|game, cursor| apply_nag(game, *cursor, nag, exclusive)));
    }

    pub(super) fn toggle_nag(&mut self, nag: Nag) {
        let node = self.cursor;
        self.toggle_nag_at(node, nag);
    }

    /// Toggles a NAG of `node`, which need not be the cursor move; the
    /// cursor stays where it is. A stale id gives `Notice::StaleNode`.
    pub(super) fn toggle_nag_at(&mut self, node: NodeId, nag: Nag) {
        let exclusive = self.settings.exclusive_nags;
        drop(self.edit(|game, _| {
            if game.remove_nag(node, nag)? {
                Ok(())
            } else {
                apply_nag(game, node, nag, exclusive)
            }
        }));
    }

    pub(super) fn remove_nag(&mut self, nag: Nag) {
        drop(self.edit(|game, cursor| game.remove_nag(*cursor, nag).map(drop)));
    }

    pub(super) fn clear_nags(&mut self) {
        drop(self.edit(|game, cursor| game.clear_nags(*cursor)));
    }

    pub(super) fn set_nags(&mut self, nags: &[Nag]) {
        drop(self.edit(|game, cursor| {
            game.clear_nags(*cursor)?;
            for &nag in nags {
                game.add_nag(*cursor, nag)?;
            }
            Ok(())
        }));
    }
}

/// Adds `nag` at the end; with `exclusive`, first removes the NAGs of its
/// category. Nothing happens if it is already there.
fn apply_nag(game: &mut Game, node: NodeId, nag: Nag, exclusive: bool) -> Result<(), GameError> {
    if game.nags(node)?.contains(&nag) {
        // Still reports the root as an error (it has no NAGs).
        return game.add_nag(node, nag).map(drop);
    }
    let category = nag_category(nag);
    if exclusive && category != NagCategory::Other {
        let same: Vec<Nag> = game
            .nags(node)?
            .iter()
            .copied()
            .filter(|&other| nag_category(other) == category)
            .collect();
        for other in same {
            game.remove_nag(node, other)?;
        }
    }
    game.add_nag(node, nag).map(drop)
}

/// Symbols accepted by [`parse_nag_list`] besides numbers.
const NAG_SYMBOLS_OK: &[&str] = &[
    "!", "?", "!!", "??", "!?", "?!", "[]", "=", "~", "+=", "+/=", "=+", "=/+", "+/-", "-/+", "+-",
    "-+", "+--", "--+", "ZZ", "zz", "[+]", "[++]", ">", ">>", ">>>", "<", "<<", "<<<", "<+>",
    "<->", "<++>", "<-->", "<+++>", "<--->", "/\\", "(_)", "R", "RR", "N", "_|_", "<=>", "/",
    "x...", "BB", "Bb", "||", "o-o", "oo", "D", "--", "o^", "|_", "_|", "->", "+>", "=/&", "~=",
];

#[cfg(test)]
mod field_tests {
    use super::*;

    fn nag(n: u16) -> Nag {
        Nag::new(n).unwrap()
    }

    #[test]
    fn field_accepts_symbols_and_numbers_without_duplicates() {
        assert_eq!(
            parse_nag_field("! +- $14 18 7").unwrap(),
            [nag(1), nag(18), nag(14), nag(7)]
        );
        assert_eq!(parse_nag_field("  ").unwrap(), []);
    }

    #[test]
    fn field_rejects_the_first_bad_token() {
        for bad in ["$0", "256", "$256", "hello", "!x", "$"] {
            let text = format!("! {bad} ?");
            let err = parse_nag_field(&text).unwrap_err();
            assert_eq!(err.token, bad, "{text}");
        }
    }

    #[test]
    fn field_text_reads_back() {
        let nags = [nag(1), nag(18), nag(14), nag(7), nag(201), nag(146)];
        let text = nag_field_text(&nags);
        assert_eq!(parse_nag_field(&text).unwrap(), nags);
        assert!(text.starts_with("! +- +="));
    }
}
