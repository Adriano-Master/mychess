//! Header output: PGN tag pairs and Scid's short header
//! (`Game::WritePGN`, `src/game.cpp:3616-3757`).

use mychess_core::{CastlingMode, Game, Rating, Variant};

use super::options::{Header, TagSet, WriteOptions};

/// FEN of the standard start position.
const STANDARD_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// The seven mandatory tags, which come first in `Tags::pgn_pairs`.
const SEVEN_TAG_COUNT: usize = 7;

/// Whether the game starts from something other than the standard position.
pub(crate) fn has_custom_start(game: &Game) -> bool {
    let pos = game.start_position();
    pos.castling_mode() != CastlingMode::Standard || pos.fen() != STANDARD_FEN
}

/// Escapes a tag value for a quoted PGN string: `\` and `"` get a backslash
/// and line breaks and NUL become spaces (a tag is one line).
pub(crate) fn escape_tag_value(value: &str, out: &mut String) {
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            // NUL cannot live in a text file that tools read as C strings.
            '\n' | '\r' | '\0' => out.push(' '),
            c => out.push(c),
        }
    }
}

fn push_tag(out: &mut String, name: &str, value: &str, nl: &str) {
    out.push('[');
    out.push_str(name);
    out.push_str(" \"");
    escape_tag_value(value, out);
    out.push_str("\"]");
    out.push_str(nl);
}

/// Appends the header of `game` followed by the blank separator line.
pub(crate) fn write_header(game: &Game, opts: &WriteOptions, nl: &str, out: &mut String) {
    match opts.header {
        Header::Short => write_short_header(game, nl, out),
        Header::Pgn => write_pgn_header(game, opts, nl, out),
    }
    out.push_str(nl);
}

fn write_pgn_header(game: &Game, opts: &WriteOptions, nl: &str, out: &mut String) {
    let pairs = game.tags().pgn_pairs();
    let is_960 = game.variant() == Variant::Chess960;
    // Scid: game.cpp:3733 writes ScidFlags right after EventDate, before the
    // other extra tags. `pgn_pairs` lists the standard tags first, so the
    // flags go in front of the first extra.
    let t = game.tags();
    let standard = SEVEN_TAG_COUNT
        + [t.white_rating, t.black_rating]
            .iter()
            .flatten()
            .filter(|r| r.value.get() > 0)
            .count()
        + usize::from(t.eco.is_some())
        + usize::from(!t.event_date.is_unknown());
    let flags = pairs
        .iter()
        .find(|(n, v)| n == "ScidFlags" && !v.is_empty())
        .filter(|_| opts.scid_flags && opts.tags == TagSet::All);
    let mut flags_pending = flags;
    for (i, (name, value)) in pairs.iter().enumerate() {
        if i >= standard
            && let Some((n, v)) = flags_pending.take()
        {
            push_tag(out, n, v, nl);
        }
        match name.as_str() {
            // Derived from the start position, never copied from the extras.
            "SetUp" | "ScidFlags" if i >= standard => continue,
            // Chess960 games get their own Variant/SetUp/FEN at the end.
            "Variant" | "FEN" if i >= standard && is_960 => continue,
            _ => {}
        }
        let keep = match opts.tags {
            TagSet::All => true,
            TagSet::SevenTagRoster => i < SEVEN_TAG_COUNT || name == "Variant",
        };
        if keep {
            push_tag(out, name, value, nl);
        }
    }
    if let Some((n, v)) = flags_pending {
        push_tag(out, n, v, nl);
    }
    if is_960 {
        // D58: always Variant, SetUp and FEN, even for position 518.
        push_tag(out, "Variant", "Chess960", nl);
        push_tag(out, "SetUp", "1", nl);
        push_tag(
            out,
            "FEN",
            &game.start_position().fen_with_style(opts.fen_castling),
            nl,
        );
    } else if has_custom_start(game) {
        push_tag(out, "SetUp", "1", nl);
        push_tag(out, "FEN", &game.start_position().fen(), nl);
    }
}

fn push_rating(out: &mut String, rating: Option<Rating>) {
    if let Some(r) = rating.filter(|r| r.value.get() > 0) {
        out.push_str("  (");
        out.push_str(&r.value.get().to_string());
        out.push(')');
    }
}

/// Scid's three-line header (not PGN), `game.cpp:3616-3690`.
fn write_short_header(game: &Game, nl: &str, out: &mut String) {
    let t = game.tags();
    out.push_str(&t.white);
    push_rating(out, t.white_rating);
    out.push_str("   --   ");
    out.push_str(&t.black);
    push_rating(out, t.black_rating);
    out.push_str(nl);

    out.push_str(&t.event);
    if !t.round.is_empty() && t.round != "?" {
        out.push_str("  (Round ");
        out.push_str(&t.round);
        out.push(')');
    }
    out.push_str("  ");
    if !t.site.is_empty() && t.site != "?" {
        out.push_str(&t.site);
        out.push_str("  ");
    }
    let mut date = t.date.to_string();
    // Drop ".??" or ".??.??" from the end.
    if date.get(7..9) == Some(".?") {
        date.truncate(7);
    }
    if date.get(4..6) == Some(".?") {
        date.truncate(4);
    }
    out.push_str(&date);
    out.push_str("  ");
    out.push_str(t.result.as_str());
    if let Some(eco) = t.eco {
        out.push_str("  ");
        out.push_str(&eco.to_string());
    }
    out.push_str(nl);

    if has_custom_start(game) {
        out.push_str("Position: ");
        out.push_str(&game.start_position().fen());
        out.push_str(nl);
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one option at a time
mod tests {
    use super::*;
    use mychess_core::{Eco, Elo, GameResult, PartialDate, Position, RatingType};
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use std::str::FromStr;

    fn header(game: &Game, opts: &WriteOptions) -> String {
        let mut s = String::new();
        write_header(game, opts, "\n", &mut s);
        s
    }

    fn sample() -> Game {
        let mut g = Game::new();
        let t = g.tags_mut();
        t.event = "Open".into();
        t.site = "Rome".into();
        t.white = "Doe, J".into();
        t.black = "Roe, R".into();
        t.round = "3".into();
        t.date = PartialDate::from_str("2024.05.17").unwrap();
        t.result = GameResult::Draw;
        g
    }

    #[test]
    fn writes_the_seven_tags_in_order() {
        let h = header(&sample(), &WriteOptions::default());
        assert_eq!(
            h,
            "[Event \"Open\"]\n[Site \"Rome\"]\n[Date \"2024.05.17\"]\n[Round \"3\"]\n\
             [White \"Doe, J\"]\n[Black \"Roe, R\"]\n[Result \"1/2-1/2\"]\n\n"
        );
    }

    #[test]
    fn default_game_writes_question_marks() {
        let h = header(&Game::new(), &WriteOptions::default());
        assert!(h.starts_with("[Event \"?\"]\n[Site \"?\"]\n[Date \"????.??.??\"]\n"));
        assert!(h.ends_with("[Result \"*\"]\n\n"));
    }

    #[test]
    fn all_tags_adds_ratings_eco_event_date_and_extras() {
        let mut g = sample();
        let t = g.tags_mut();
        t.white_rating = Some(Rating::new(Elo::new(2700).unwrap(), RatingType::Elo));
        t.black_rating = Some(Rating::new(Elo::new(2650).unwrap(), RatingType::Rapid));
        t.eco = Some(Eco::from_str("B90").unwrap());
        t.event_date = PartialDate::from_str("2024.05.01").unwrap();
        t.set_extra("Annotator", "me").unwrap();
        let h = header(&g, &WriteOptions::default());
        let tail: Vec<&str> = h.lines().skip(7).collect();
        assert_eq!(
            tail,
            [
                "[WhiteElo \"2700\"]",
                "[BlackRapid \"2650\"]",
                "[ECO \"B90\"]",
                "[EventDate \"2024.05.01\"]",
                "[Annotator \"me\"]",
                ""
            ]
        );
    }

    #[test]
    fn seven_tag_roster_drops_the_rest_but_keeps_variant_and_fen() {
        let mut g =
            Game::from_position(Position::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap());
        g.tags_mut().set_extra("Annotator", "me").unwrap();
        g.tags_mut().set_extra("Variant", "Chess960").unwrap();
        g.tags_mut().eco = Some(Eco::from_str("B90").unwrap());
        let mut o = WriteOptions::default();
        o.tags = TagSet::SevenTagRoster;
        let h = header(&g, &o);
        assert!(!h.contains("Annotator") && !h.contains("ECO"));
        assert!(h.contains(
            "[Variant \"Chess960\"]\n[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/8/4K2R w K - 0 1\"]\n"
        ));
    }

    #[test]
    fn fen_start_writes_setup_then_fen_last() {
        let g = Game::from_position(Position::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap());
        let h = header(&g, &WriteOptions::default());
        assert!(h.ends_with("[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/8/4K2R w K - 0 1\"]\n\n"));
    }

    #[test]
    fn standard_start_has_no_fen() {
        assert!(!header(&Game::new(), &WriteOptions::default()).contains("FEN"));
    }

    #[test]
    fn stored_setup_extra_is_not_duplicated() {
        let mut g = Game::new();
        g.tags_mut().set_extra("SetUp", "0").unwrap();
        assert!(!header(&g, &WriteOptions::default()).contains("SetUp"));
    }

    #[rstest]
    #[case(false, false)]
    #[case(true, true)]
    fn scid_flags_only_when_requested(#[case] on: bool, #[case] expected: bool) {
        let mut g = Game::new();
        g.tags_mut().set_extra("ScidFlags", "WBM").unwrap();
        let mut o = WriteOptions::default();
        o.scid_flags = on;
        assert_eq!(header(&g, &o).contains("[ScidFlags \"WBM\"]"), expected);
    }

    #[rstest]
    #[case("plain", "plain")]
    #[case(r#"say "hi""#, r#"say \"hi\""#)]
    #[case(r"back\slash", r"back\\slash")]
    #[case("two\nlines\r\nhere", "two lines  here")]
    #[case("a\0b", "a b")]
    #[case("Müller ♟", "Müller ♟")]
    fn escapes_tag_values(#[case] input: &str, #[case] expected: &str) {
        let mut out = String::new();
        escape_tag_value(input, &mut out);
        assert_eq!(out, expected);
    }

    #[test]
    fn header_uses_the_requested_line_ending() {
        let mut s = String::new();
        write_header(&Game::new(), &WriteOptions::default(), "\r\n", &mut s);
        assert!(s.contains("[Event \"?\"]\r\n") && s.ends_with("\r\n\r\n"));
        assert!(!s.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn short_header_has_three_lines() {
        let mut g = sample();
        let t = g.tags_mut();
        t.white_rating = Some(Rating::new(Elo::new(2700).unwrap(), RatingType::Elo));
        t.eco = Some(Eco::from_str("B90").unwrap());
        let mut o = WriteOptions::default();
        o.header = Header::Short;
        assert_eq!(
            header(&g, &o),
            "Doe, J  (2700)   --   Roe, R\n\
             Open  (Round 3)  Rome  2024.05.17  1/2-1/2  B90\n\n"
        );
    }

    #[test]
    fn short_header_trims_unknown_date_parts_and_omits_unknowns() {
        let mut g = Game::new();
        g.tags_mut().date = PartialDate::from_str("2001.??.??").unwrap();
        let mut o = WriteOptions::default();
        o.header = Header::Short;
        assert_eq!(header(&g, &o), "?   --   ?\n?  2001  *\n\n");
    }

    #[test]
    fn short_header_shows_position_for_fen_games() {
        let g = Game::from_position(Position::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap());
        let mut o = WriteOptions::default();
        o.header = Header::Short;
        assert!(header(&g, &o).contains("Position: 4k3/8/8/8/8/8/8/4K2R w K - 0 1\n"));
    }
}
