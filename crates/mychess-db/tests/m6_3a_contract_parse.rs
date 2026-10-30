//! Contract tests of the reading of the `.ssp` grammar (M6-3a), line by line:
//! rules 1 to 15 of `docs/specs/m6-3-jogadores-torneios-spelling.md` and
//! `docs/formatos/spelling-ssp.md`. All data is synthetic (fictitious names).
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

#[path = "common/m6_3a_support.rs"]
mod support;

use std::io::Write as _;

use mychess_core::PartialDate;
use mychess_db::si4::NameKind;
use mychess_db::spelling::elo::EloTable;
use mychess_db::spelling::{
    MAX_LINE_BYTES, PlayerComment, ReadOptions, SpellingError, SpellingSet,
};
use mychess_db::{CancelToken, NoProgress};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;
use support::{EXAMPLE, opts, read};

fn players(text: &str) -> SpellingSet {
    read(&format!("@PLAYER \"\"\n{text}"))
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(bytes).unwrap();
    enc.finish().unwrap()
}

// ---------------------------------------------------------------------------
// Rule 2: splitting a line at the first `#`.
// ---------------------------------------------------------------------------

#[test]
fn comment_starts_after_the_first_hash_and_keeps_its_leading_space() {
    let set = read("@PLAYER\nAlfa, A # GM BRA [2500] 1980 # not a new comment\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(
        p.comment_exact("Alfa, A"),
        Some(" GM BRA [2500] 1980 # not a new comment"),
        "the space after the first # stays, a second # is plain text"
    );
}

#[test]
fn comment_is_trimmed_on_the_right_only() {
    let set = read("@PLAYER\nAlfa, A #GM BRA [2500]  \t \r\nBeta, B #  X Y\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.comment_exact("Alfa, A"), Some("GM BRA [2500]"));
    assert_eq!(p.comment_exact("Beta, B"), Some("  X Y"));
}

#[test]
fn name_part_is_trimmed_on_both_ends_with_tabs_and_blanks() {
    let set = read("@PLAYER\n \t Alfa, A \t #x\n\tBeta, B\t\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.comment_exact("Alfa, A"), Some("x"));
    assert_eq!(p.comment_exact("Beta, B"), Some(""));
    assert_eq!(set.report().players, 2);
}

#[test]
fn a_name_without_hash_has_an_empty_comment_and_still_matches() {
    let set = read("@PLAYER\nAlfa, A\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.comment_exact("Alfa, A"), Some(""));
    assert_eq!(p.comment_loose("Alfa"), Some(""), "matched, no comment");
    assert_eq!(p.comment_loose("Zzz"), None);
}

#[test]
fn comment_with_leading_space_is_still_parsed_as_fields() {
    // Rule 16: fields are separated by one or more spaces; the space that the
    // file keeps after '#' must not shift them.
    let c = PlayerComment::parse(" GM RUS [2812] 1963.04.13 i");
    assert_eq!(c.last_country(), "RUS");
    assert_eq!(c.peak(), 2812);
    assert_eq!(c.born().to_string(), "1963.04.13");
    let c = PlayerComment::parse("GM    RUS   [2812]     1963.04.13");
    assert_eq!(c.last_country(), "RUS");
    assert_eq!(c.born().to_string(), "1963.04.13");
}

// ---------------------------------------------------------------------------
// Rule 3: classes of lines.
// ---------------------------------------------------------------------------

#[rstest]
#[case::empty("")]
#[case::blanks("   \t ")]
#[case::comment_only("# a comment")]
#[case::comment_after_blanks("   # a comment")]
#[case::old_bio("> old biography line")]
#[case::old_bio_with_hash("> old # biography")]
#[case::unknown_percent("%Foo bar")]
#[case::lowercase_directive("%bio note")]
#[case::bio_without_space("%Bio")]
#[case::elo_without_space("%Elo")]
fn ignored_lines_change_nothing(#[case] line: &str) {
    let with = read(&format!("@PLAYER\nAlfa, A #c\n{line}\nBeta, B\n"));
    let without = read("@PLAYER\nAlfa, A #c\nBeta, B\n");
    assert_eq!(with.report(), without.report());
    let (a, b) = (
        with.checker(NameKind::Player),
        without.checker(NameKind::Player),
    );
    assert_eq!(a.num_names(), b.num_names());
    assert_eq!(a.num_aliases(), 0);
    assert_eq!(a.has_elo_data(), false);
    assert_eq!(a.bio("Alfa, A").len(), 0);
}

#[test]
fn old_biography_lines_do_not_become_names_or_bio() {
    let set = read("@PLAYER\nAlfa, A\n> some old text\n%Bio real note\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(set.report().players, 1);
    assert_eq!(p.bio("Alfa, A"), ["real note"]);
}

// ---------------------------------------------------------------------------
// Rule 4: section lines.
// ---------------------------------------------------------------------------

#[rstest]
#[case::p("@P", NameKind::Player)]
#[case::pl("@pl", NameKind::Player)]
#[case::pl_quotes("@pl \"x\"", NameKind::Player)]
#[case::full_upper("@PLAYER", NameKind::Player)]
#[case::full_lower("@player", NameKind::Player)]
#[case::mixed_case("@PlAyEr", NameKind::Player)]
#[case::quotes("@PLAYER \"., -_*\"", NameKind::Player)]
#[case::leading_spaces("@   player", NameKind::Player)]
#[case::longer_than_word("@Playerx", NameKind::Player)]
#[case::e("@E", NameKind::Event)]
#[case::ev("@ev", NameKind::Event)]
#[case::event("@EVENT \",. -_\"", NameKind::Event)]
#[case::s("@S", NameKind::Site)]
#[case::site("@site", NameKind::Site)]
#[case::site_quotes("@SITE \"., -_()\"", NameKind::Site)]
#[case::r("@R", NameKind::Round)]
#[case::round("@ROUND \"\"", NameKind::Round)]
#[case::rou("@rou", NameKind::Round)]
fn section_selects_the_kind(#[case] header: &str, #[case] kind: NameKind) {
    let set = read(&format!("{header}\nSome Name\n"));
    for k in NameKind::ALL {
        let expected = u32::from(k == kind);
        assert_eq!(set.report().names(k), expected, "{header}: {k:?}");
        assert_eq!(set.checker(k).num_names(), expected, "{header}: {k:?}");
    }
}

#[rstest]
#[case::empty("@")]
#[case::space("@ ")]
#[case::unknown("@FOO")]
#[case::digit("@123")]
#[case::other_letter("@Q")]
#[case::unrelated_prefix("@Playground")]
#[case::unknown_with_quotes("@FOO \"abc\"")]
#[case::unknown_with_open_quote("@FOO \"abc")]
fn unknown_section_has_no_owner_and_is_not_an_error(#[case] header: &str) {
    let set = read(&format!("{header}\nSome Name\n= alias\n%Bio note\n"));
    for k in NameKind::ALL {
        assert_eq!(set.report().names(k), 0, "{header}: {k:?}");
        assert_eq!(set.checker(k).num_aliases(), 0, "{header}: {k:?}");
    }
}

#[test]
fn an_unknown_section_swallows_names_until_the_next_section() {
    let set = read("@PLAYER\nA1\n@FOO\nHidden\n= hidden alias\n@PLAYER\nA2\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(set.report().players, 2);
    assert_eq!(p.correct("Hidden"), None);
    assert_eq!(p.correct("hidden alias"), None);
    assert_eq!(p.correct("A2"), Some("A2"));
}

#[test]
fn sections_of_the_same_kind_accumulate_names_and_rules() {
    let set = read(
        "@PLAYER\nA1\n%Prefix \"x\" \"y\"\n@SITE\nS1\n@PLAYER\nA2\n%Prefix \"p\" \"q\"\n= a2alias\n",
    );
    let p = set.checker(NameKind::Player);
    assert_eq!(set.report().players, 2);
    assert_eq!(p.prefix_fix("xa").as_deref(), Some("ya"));
    assert_eq!(p.prefix_fix("pa").as_deref(), Some("qa"));
    assert_eq!(
        p.correct("a2alias"),
        Some("A2"),
        "alias of the last PLAYER name"
    );
}

#[test]
fn lines_before_the_first_section_belong_to_no_kind() {
    let set = read("Orphan\n= orphan alias\n%Prefix \"a\" \"b\"\n%Bio x\n@PLAYER\nReal\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(set.report().players, 1);
    assert_eq!(p.num_aliases(), 0);
    assert_eq!(
        p.prefix_fix("abc"),
        None,
        "a rule outside any section is ignored"
    );
    assert_eq!(p.bio("Real").len(), 0);
}

#[test]
fn a_rule_in_an_unknown_section_is_ignored() {
    let set = read("@PLAYER\nA\n@FOO\n%Prefix \"a\" \"b\"\n");
    assert_eq!(set.checker(NameKind::Player).prefix_fix("abc"), None);
}

// ---------------------------------------------------------------------------
// Rule 5: excluded characters.
// ---------------------------------------------------------------------------

#[test]
fn excluded_characters_are_the_text_between_the_first_two_quotes() {
    let set = read("@PLAYER \"., -_*\"\nKova, A. B-C\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.excluded_chars(), ['.', ',', ' ', '-', '_', '*']);
    // Both sides are compared without the excluded characters.
    assert_eq!(p.correct("KovaABC"), Some("Kova, A. B-C"));
    assert_eq!(p.correct("Kova A B C"), Some("Kova, A. B-C"));
    assert_eq!(p.correct("K.o.v.a"), Some("Kova, A. B-C"));
}

#[test]
fn text_after_the_closing_quote_is_ignored() {
    let set = read("@PLAYER \"-\" junk \"more\"\nA-B\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.excluded_chars(), ['-']);
    assert_eq!(p.correct("AB"), Some("A-B"));
}

#[test]
fn no_quotes_keeps_the_previous_set_and_empty_quotes_clear_it() {
    let set = read("@PLAYER \"-\"\nA-B\n@PLAYER\nC-D\n");
    assert_eq!(set.checker(NameKind::Player).excluded_chars(), ['-']);
    let set = read("@PLAYER \"-\"\nA-B\n@PLAYER \"\"\nC-D\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.excluded_chars().len(), 0);
    // `A-B` was stored without the dash while the first set was in force;
    // the query now keeps its dash.
    assert_eq!(p.correct("AB"), Some("A-B"));
    assert_eq!(p.correct("A-B"), None);
    assert_eq!(p.correct("C-D"), Some("C-D"));
    assert_eq!(p.correct("CD"), None);
}

#[test]
fn keys_use_the_set_in_force_when_read_and_queries_the_final_set() {
    // Rule 5: "as consultas usam o conjunto vigente ao fim da leitura, e as
    // chaves já gravadas usam o vigente quando foram lidas".
    let set = read("@PLAYER \"-\"\nAb-c\n@PLAYER \"_\"\nDe_f\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.excluded_chars(), ['_']);
    assert_eq!(p.correct("Abc"), Some("Ab-c"), "key stored as Abc");
    assert_eq!(p.correct("Ab-c"), None, "the query keeps its dash now");
    assert_eq!(p.correct("De_f"), Some("De_f"));
    assert_eq!(p.correct("Def"), Some("De_f"));
}

#[test]
fn excluded_characters_may_be_non_ascii() {
    let set = read("@PLAYER \"é\"\nJosé\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.correct("Jos"), Some("José"));
    assert_eq!(p.correct("José"), Some("José"));
}

#[test]
fn each_kind_has_its_own_excluded_set() {
    let set =
        read("@PLAYER \"-\"\nA-B\n@SITE \"()\"\nRio (BRA)\n@EVENT \",.\"\nOpen, 1.\n@ROUND\n1\n");
    assert_eq!(set.checker(NameKind::Player).excluded_chars(), ['-']);
    assert_eq!(set.checker(NameKind::Site).excluded_chars(), ['(', ')']);
    assert_eq!(set.checker(NameKind::Event).excluded_chars(), [',', '.']);
    assert_eq!(set.checker(NameKind::Round).excluded_chars().len(), 0);
    assert_eq!(
        set.checker(NameKind::Site).correct("Rio BRA"),
        Some("Rio (BRA)")
    );
}

#[rstest]
#[case::player("@PLAYER \"abc\nName\n", 1)]
#[case::event("Orphan\n@EVENT \"abc\n", 2)]
#[case::site("\n\n@SITE \"\n", 3)]
#[case::round("@ROUND \"x\"\nA\n@ROUND \"y\nB\n", 3)]
#[case::abbreviated("@p \"x\n", 1)]
#[case::lone_quote("@PLAYER \"\n", 1)]
fn an_unclosed_quote_in_a_known_section_is_the_only_syntax_error(
    #[case] text: &str,
    #[case] line: u64,
) {
    let err = SpellingSet::read_bytes(text.as_bytes(), &opts(2020)).unwrap_err();
    match err {
        SpellingError::UnclosedQuote { line: got } => assert_eq!(got, line),
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn an_unclosed_quote_in_a_file_is_an_error_for_every_kind_at_once() {
    // One pass, but the load is atomic: nothing is returned.
    let text = "@PLAYER\nA\n@SITE\nS\n@EVENT \"oops\nE\n";
    assert!(SpellingSet::read_bytes(text.as_bytes(), &opts(2020)).is_err());
}

// ---------------------------------------------------------------------------
// Rules 6 to 8: correct names and aliases.
// ---------------------------------------------------------------------------

#[test]
fn correct_names_are_numbered_in_file_order_and_the_name_keeps_everything() {
    let set = read("@PLAYER \"., \"\nZeta, Z\nAlfa, A\nMid, M\n");
    let p = set.checker(NameKind::Player);
    let names: Vec<&str> = p.correct_names().collect();
    assert_eq!(names, ["Zeta, Z", "Alfa, A", "Mid, M"]);
    assert_eq!(p.num_names(), 3);
}

#[test]
fn an_alias_inherits_name_and_comment_and_does_not_count() {
    let set = read("@PLAYER \"., \"\nAlfa, Anna #GM BRA [2500] 1980\n  = Alfa, A\n=Alfa An\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.num_names(), 1);
    assert_eq!(p.num_aliases(), 2);
    assert_eq!(p.correct("Alfa, A"), Some("Alfa, Anna"));
    assert_eq!(p.comment_loose("Alfa An"), Some("GM BRA [2500] 1980"));
    assert_eq!(
        p.comment_exact("Alfa, A"),
        None,
        "an alias has no exact comment"
    );
}

#[test]
fn an_alias_points_to_the_last_correct_name_of_its_kind_across_other_sections() {
    let set = read("@PLAYER\nAlfa\n@SITE\nRio\n= Rio de Janeiro\n@PLAYER\n= Alfa2\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.correct("Alfa2"), Some("Alfa"));
    assert_eq!(
        set.checker(NameKind::Site).correct("Rio de Janeiro"),
        Some("Rio")
    );
    assert_eq!(p.correct("Rio de Janeiro"), None);
    assert_eq!(p.num_aliases(), 1);
    assert_eq!(set.checker(NameKind::Site).num_aliases(), 1);
}

#[test]
fn an_alias_in_a_kind_without_names_yet_is_ignored() {
    let set = read("@PLAYER\nAlfa\n@SITE\n= Orphan alias\nRio\n");
    assert_eq!(set.checker(NameKind::Site).num_aliases(), 0);
    assert_eq!(set.checker(NameKind::Site).correct("Orphan alias"), None);
}

#[test]
fn only_spaces_are_stripped_after_the_equal_sign() {
    let set = read("@PLAYER \"\"\nAlfa\n=   spaced\n=\ttabbed\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.correct("spaced"), Some("Alfa"));
    // The line was trimmed of tabs at both ends, so "\ttabbed" -> "=\ttabbed":
    // the tab is not a space and stays in the alias.
    assert_eq!(p.correct("\ttabbed"), Some("Alfa"));
}

#[test]
fn empty_keys_are_kept_but_never_match() {
    let set = read("@PLAYER \"., \"\n...\nReal\n=\n= ,.\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(
        set.report().players,
        2,
        "the name with an empty key is stored"
    );
    assert_eq!(p.correct("..."), None);
    assert_eq!(p.correct(",,"), None);
    assert_eq!(p.correct(""), None);
    assert_eq!(p.corrections("...", 10).len(), 0);
    assert_eq!(p.comment_loose("..."), None);
    // Exact (whole name) lookups still see the stored name.
    assert_eq!(p.comment_exact("..."), Some(""));
    assert_eq!(p.correct("Real"), Some("Real"));
}

#[test]
fn a_repeated_correct_name_counts_twice_and_the_last_one_wins_for_exact_data() {
    let set = read("@PLAYER\nDup #first\nDup #second\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(set.report().players, 2);
    assert_eq!(p.comment_exact("Dup"), Some("second"));
    assert_eq!(p.comment_loose("Dup"), Some("second"));
}

// ---------------------------------------------------------------------------
// Rule 9: directives.
// ---------------------------------------------------------------------------

#[test]
fn bio_notes_keep_the_text_after_one_space_and_order() {
    let set = players("Alfa\n%Bio first\n%Bio   two spaces\n%Bio last note \n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.bio("Alfa"), ["first", "  two spaces", "last note"]);
}

#[test]
fn a_hash_ends_the_text_of_a_bio_note() {
    let set = players("Alfa\n%Bio Rating #1 em 1990\n%Bio #only comment\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(
        p.bio("Alfa"),
        ["Rating"],
        "a note that is all comment is not a %Bio line"
    );
}

#[test]
fn bio_and_elo_before_any_name_are_ignored() {
    let set = read("@PLAYER\n%Bio early\n%Elo 2013:2300\nAlfa\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.bio("Alfa").len(), 0);
    assert!(!p.has_elo_data());
    assert!(p.elo_table("Alfa").is_none());
}

#[test]
fn an_elo_line_marks_elo_data_even_when_its_values_are_invalid() {
    let set = read("@PLAYER\nAlfa\n%Elo garbage\n");
    let p = set.checker(NameKind::Player);
    assert!(p.has_elo_data());
    assert_eq!(
        p.elo_at("Alfa", PartialDate::parse_lenient("2013.03.01"), false),
        None
    );
    assert_eq!(set.report().elo_lines, 1);
}

#[test]
fn a_bare_elo_line_is_trimmed_to_a_bare_keyword_and_ignored() {
    let set = read("@PLAYER\nAlfa\n%Elo   \n%Bio \t\n");
    let p = set.checker(NameKind::Player);
    assert!(!p.has_elo_data());
    assert_eq!(p.bio("Alfa").len(), 0);
}

#[test]
fn elo_lines_of_one_name_are_limited_and_the_rest_counted() {
    let mut text = String::from("@PLAYER\nAlfa\n");
    for i in 0..70 {
        text.push_str(&format!("%Elo {}:2000\n", 1970 + i % 40));
    }
    let set = read(&text);
    assert_eq!(set.report().elo_lines, 64);
    assert_eq!(set.report().elo_dropped, 6);
    assert!(set.checker(NameKind::Player).has_elo_data());
}

#[test]
fn render_text_is_stored_without_leading_spaces_and_has_no_other_effect() {
    let set = players("Jose, A\n%Render    José, A\n= Jose A\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.render_name("Jose, A"), Some("José, A"));
    assert_eq!(
        p.render_name("Jose A"),
        None,
        "an alias has no render text of its own"
    );
    assert_eq!(p.correct("Jose A"), Some("Jose, A"));
}

#[test]
fn directives_apply_to_the_last_correct_name_of_each_kind_in_any_section() {
    // D3350: the Scid reads the file once per kind; a %Bio that follows a
    // site belongs to the site and, in the player pass, to the last player.
    let set = read("@PLAYER\nP1\n@SITE\nS1\n%Bio shared note\n%Elo 2013:2300\n@EVENT\nE1\n");
    let player = set.checker(NameKind::Player);
    let site = set.checker(NameKind::Site);
    assert_eq!(player.bio("P1"), ["shared note"]);
    assert_eq!(site.bio("S1"), ["shared note"]);
    assert!(player.has_elo_data());
    assert!(site.has_elo_data());
    assert!(!set.checker(NameKind::Event).has_elo_data());
    assert_eq!(set.checker(NameKind::Event).bio("E1").len(), 0);
}

#[test]
fn directives_after_an_unknown_section_still_reach_the_last_names() {
    let set = read("@PLAYER\nP1\n@FOO\n%Bio from unknown\n");
    assert_eq!(set.checker(NameKind::Player).bio("P1"), ["from unknown"]);
}

#[test]
fn an_alias_is_between_a_name_and_its_directive_without_stealing_it() {
    let set = players("Alfa\n= Alfa alias\n%Bio note after alias\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.bio("Alfa"), ["note after alias"]);
    assert_eq!(
        p.bio("Alfa alias"),
        ["note after alias"],
        "exact alias: the notes of the main name"
    );
}

// ---------------------------------------------------------------------------
// Rule 10: replacement rules.
// ---------------------------------------------------------------------------

#[rstest]
#[case::plain("%Prefix \"GM \" \"\"", "GM Braga", Some("Braga"))]
#[case::replacement("%Prefix \"VIII \" \"8th \"", "VIII Round", Some("8th Round"))]
#[case::extra_text_after("%Prefix \"A\" \"B\" trailing junk", "Ax", Some("Bx"))]
#[case::spaces_between_pairs("%Prefix \"A\"     \"B\"", "Ax", Some("Bx"))]
#[case::text_between_pairs("%Prefix \"A\" => \"B\"", "Ax", Some("Bx"))]
#[case::case_exact("%Prefix \"gm \" \"\"", "GM Braga", None)]
#[case::only_two_quotes("%Prefix \"A\"", "Ax", None)]
#[case::three_quotes("%Prefix \"A\" \"B", "Ax", None)]
#[case::no_quotes("%Prefix A B", "Ax", None)]
#[case::empty_from("%Prefix \"\" \"x\"", "Ax", None)]
#[case::whole_name("%Prefix \"Ax\" \"Z\"", "Ax", Some("Z"))]
#[case::name_shorter("%Prefix \"Axx\" \"Z\"", "Ax", None)]
#[case::no_space_after_keyword("%Prefix\"A\" \"B\"", "Ax", None)]
#[case::tab_after_keyword("%Prefix\t\"A\" \"B\"", "Ax", None)]
fn prefix_rule_grammar(#[case] line: &str, #[case] name: &str, #[case] expected: Option<&str>) {
    let set = read(&format!("@PLAYER\nName\n{line}\n"));
    assert_eq!(
        set.checker(NameKind::Player).prefix_fix(name).as_deref(),
        expected,
        "{line}"
    );
}

#[test]
fn spaces_inside_the_quotes_are_preserved() {
    let set = read("@PLAYER\n%Prefix \" \" \"\"\n%Suffix \" \" \"\"\n%Infix \" - \" \"-\"\nName\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.prefix_fix(" Braga").as_deref(), Some("Braga"));
    assert_eq!(p.prefix_fix("Braga"), None);
    assert_eq!(p.suffix_fix("Braga ").as_deref(), Some("Braga"));
    assert_eq!(
        p.infix_fix("A - B - C").as_deref(),
        Some("A-B - C"),
        "first occurrence only"
    );
}

#[test]
fn suffix_and_infix_rules_follow_their_grammar() {
    let set = read(
        "@EVENT\nE\n%Suffix \" ch\" \" Ch\"\n%Infix \"-ch \" \" Ch \"\n%Infix \"zz\" \"\"\n%Suffix \"\" \"x\"\n",
    );
    let e = set.checker(NameKind::Event);
    assert_eq!(e.suffix_fix("Brasil ch").as_deref(), Some("Brasil Ch"));
    assert_eq!(
        e.suffix_fix("ch"),
        None,
        "shorter than the suffix with its space"
    );
    assert_eq!(e.suffix_fix("Brasil"), None, "an empty suffix is ignored");
    assert_eq!(e.infix_fix("Rio-ch 2020").as_deref(), Some("Rio Ch 2020"));
    assert_eq!(e.infix_fix("azzb").as_deref(), Some("ab"));
    assert_eq!(e.infix_fix("none"), None);
    assert_eq!(e.infix_fix(""), None);
}

#[test]
fn rules_are_scoped_to_their_kind() {
    let set = read("@PLAYER\nP\n%Suffix \"(x)\" \"X\"\n@SITE\nS\n%Suffix \"(Brazil)\" \"BRA\"\n");
    let player = set.checker(NameKind::Player);
    let site = set.checker(NameKind::Site);
    assert_eq!(player.suffix_fix("Rio (x)").as_deref(), Some("Rio X"));
    assert_eq!(player.suffix_fix("Rio (Brazil)"), None);
    assert_eq!(site.suffix_fix("Rio (Brazil)").as_deref(), Some("Rio BRA"));
    assert_eq!(site.suffix_fix("Rio (x)"), None);
}

#[test]
fn the_rule_defined_last_is_tried_first_for_suffix_and_infix_too() {
    let set = read(
        "@PLAYER\n%Suffix \"c\" \"1\"\n%Suffix \"bc\" \"2\"\n%Infix \"a\" \"3\"\n%Infix \"ab\" \"4\"\nN\n",
    );
    let p = set.checker(NameKind::Player);
    assert_eq!(p.suffix_fix("abc").as_deref(), Some("a2"));
    assert_eq!(p.suffix_fix("xc").as_deref(), Some("x1"));
    assert_eq!(p.infix_fix("xaby").as_deref(), Some("x4y"));
    assert_eq!(p.infix_fix("xay").as_deref(), Some("x3y"));
}

#[test]
fn rules_apply_to_the_original_name_not_to_the_key() {
    let set = read("@PLAYER \"., \"\n%Prefix \"Mr. \" \"\"\nN\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.prefix_fix("Mr. Smith").as_deref(), Some("Smith"));
    assert_eq!(
        p.prefix_fix("Mr Smith"),
        None,
        "the excluded characters do not count"
    );
}

#[test]
fn rules_do_not_panic_on_multibyte_text() {
    let set = read("@PLAYER\n%Prefix \"é\" \"e\"\n%Suffix \"ã\" \"a\"\n%Infix \"ç\" \"c\"\nN\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.prefix_fix("éa").as_deref(), Some("ea"));
    assert_eq!(p.suffix_fix("maçã").as_deref(), Some("maça"));
    assert_eq!(p.infix_fix("maçã").as_deref(), Some("macã"));
    assert_eq!(p.prefix_fix("\u{301}"), None);
}

// ---------------------------------------------------------------------------
// Rule 1: bytes to text, gzip, line ends.
// ---------------------------------------------------------------------------

#[test]
fn utf8_bom_is_removed_and_utf8_is_kept() {
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice("@PLAYER\nJosé Ângelo\n".as_bytes());
    let set = SpellingSet::read_bytes(&bytes, &opts(2020)).unwrap();
    assert_eq!(
        set.checker(NameKind::Player).correct("José"),
        Some("José Ângelo")
    );
}

#[test]
fn latin1_and_windows_1252_bytes_are_decoded() {
    // 0xE9 = é (both), 0x80 = € and 0x92 = right quote only in Windows-1252.
    let set =
        SpellingSet::read_bytes(b"@PLAYER\nJos\xE9\nPre\x80o\nO\x92Neil\n", &opts(2020)).unwrap();
    let p = set.checker(NameKind::Player);
    assert_eq!(p.correct("Jos\u{e9}"), Some("Jos\u{e9}"));
    assert_eq!(p.correct("Pre\u{20ac}o"), Some("Pre\u{20ac}o"));
    assert_eq!(p.correct("O\u{2019}Neil"), Some("O\u{2019}Neil"));
}

#[test]
fn a_large_latin1_file_is_decoded_with_its_accents() {
    // More than 32 non-ASCII bytes: the encoding detector is consulted.
    let mut bytes = b"@PLAYER\n".to_vec();
    for i in 0..60 {
        bytes.extend_from_slice(
            &format!("Jos\u{e9}-{i:02} \u{c2}ngelo, \u{c7}elso\n").as_bytes_latin1(),
        );
    }
    let set = SpellingSet::read_bytes(&bytes, &opts(2020)).unwrap();
    assert_eq!(set.report().players, 60);
    let p = set.checker(NameKind::Player);
    assert!(
        p.correct_names()
            .all(|n| n.contains('\u{e9}') && n.contains('\u{c7}')),
        "{:?}",
        p.correct_names().next()
    );
}

/// Helper: Latin-1 encoding of a string whose characters are all below U+0100.
trait AsLatin1 {
    fn as_bytes_latin1(&self) -> Vec<u8>;
}

impl AsLatin1 for String {
    fn as_bytes_latin1(&self) -> Vec<u8> {
        self.chars()
            .map(|c| u8::try_from(u32::from(c)).unwrap())
            .collect()
    }
}

#[test]
fn invalid_utf8_at_the_end_does_not_panic() {
    for tail in [
        &b"\xC3"[..],
        b"\xE2\x82",
        b"\xF0\x9F\x99",
        b"\xFF\xFE",
        b"\x80",
    ] {
        let mut bytes = b"@PLAYER\nAlfa\n".to_vec();
        bytes.extend_from_slice(tail);
        let set = SpellingSet::read_bytes(&bytes, &opts(2020)).unwrap();
        assert!(set.report().players >= 1);
    }
}

#[test]
fn line_terminators_all_work_and_mix() {
    let set = read("@PLAYER\r\nA\nB\rC\r\nD\n\rE");
    assert_eq!(set.report().players, 5);
    let set = read("@PLAYER\n\n\n\r\r\nA");
    assert_eq!(set.report().players, 1);
}

#[test]
fn the_last_line_needs_no_terminator_for_every_class() {
    for (tail, check) in [
        ("Last", "name"),
        ("= alias", "alias"),
        ("%Bio note", "bio"),
        ("%Prefix \"a\" \"b\"", "rule"),
        ("%Elo 2013:2300", "elo"),
    ] {
        let set = read(&format!("@PLAYER\nFirst\n{tail}"));
        let p = set.checker(NameKind::Player);
        match check {
            "name" => assert_eq!(set.report().players, 2),
            "alias" => assert_eq!(p.correct("alias"), Some("First")),
            "bio" => assert_eq!(p.bio("First"), ["note"]),
            "rule" => assert_eq!(p.prefix_fix("ax").as_deref(), Some("bx")),
            "elo" => assert!(p.has_elo_data()),
            _ => unreachable!(),
        }
    }
}

#[test]
fn gzip_is_detected_by_content_and_handles_multiple_members() {
    let mut two = gzip(b"@PLAYER\nA\n");
    two.extend_from_slice(&gzip(b"B\n"));
    let set = SpellingSet::read_bytes(&two, &opts(2020)).unwrap();
    assert_eq!(set.report().players, 2);

    let dir = tempfile::tempdir().unwrap();
    // A gzip stream in a file without the .gz suffix, and plain text in a .gz.
    let plain_named_gz = dir.path().join("plain.ssp.gz");
    std::fs::write(&plain_named_gz, EXAMPLE).unwrap();
    let gz_named_plain = dir.path().join("zipped.ssp");
    std::fs::write(&gz_named_plain, gzip(EXAMPLE)).unwrap();
    for path in [plain_named_gz, gz_named_plain] {
        let set = SpellingSet::read(&path, &opts(2020), &CancelToken::new(), &mut NoProgress);
        assert_eq!(set.unwrap().report().players, 3, "{}", path.display());
    }
}

#[test]
fn gzip_with_bom_and_latin1_inside() {
    let mut inner = b"\xEF\xBB\xBF@PLAYER\nJos\xE9\n".to_vec();
    inner.extend_from_slice(b"\n");
    let set = SpellingSet::read_bytes(&gzip(&inner), &opts(2020)).unwrap();
    assert_eq!(
        set.checker(NameKind::Player).correct("Jos\u{e9}"),
        Some("Jos\u{e9}")
    );
}

#[test]
fn every_truncation_of_a_gzip_stream_is_an_error_not_a_partial_load() {
    let gz = gzip(EXAMPLE);
    for cut in 2..gz.len() {
        let result = SpellingSet::read_bytes(&gz[..cut], &opts(2020));
        assert!(
            matches!(result, Err(SpellingError::Gzip(_))),
            "cut at {cut} of {}: {:?}",
            gz.len(),
            result.map(|s| s.report().players)
        );
    }
}

#[test]
fn a_gzip_stream_with_a_flipped_byte_never_panics() {
    let gz = gzip(EXAMPLE);
    for at in 0..gz.len() {
        let mut bad = gz.clone();
        bad[at] ^= 0xFF;
        drop(SpellingSet::read_bytes(&bad, &opts(2020)));
    }
}

#[test]
fn a_gzip_bomb_is_stopped_by_the_decompressed_limit() {
    let bomb = gzip(&vec![b'\n'; 8 << 20]);
    assert!(bomb.len() < 64 << 10, "the stream itself is tiny");
    let small = ReadOptions {
        max_bytes: 1 << 20,
        ..opts(2020)
    };
    assert!(matches!(
        SpellingSet::read_bytes(&bomb, &small),
        Err(SpellingError::TooLarge { limit }) if limit == 1 << 20
    ));
    // The same through a file.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bomb.ssp.gz");
    std::fs::write(&path, &bomb).unwrap();
    assert!(matches!(
        SpellingSet::read(&path, &small, &CancelToken::new(), &mut NoProgress),
        Err(SpellingError::TooLarge { .. })
    ));
}

// ---------------------------------------------------------------------------
// Rule 14: limits.
// ---------------------------------------------------------------------------

#[test]
fn max_bytes_is_inclusive_for_plain_text() {
    let len = EXAMPLE.len() as u64;
    let exact = ReadOptions {
        max_bytes: len,
        ..opts(2020)
    };
    assert!(SpellingSet::read_bytes(EXAMPLE, &exact).is_ok());
    let one_less = ReadOptions {
        max_bytes: len - 1,
        ..opts(2020)
    };
    assert!(matches!(
        SpellingSet::read_bytes(EXAMPLE, &one_less),
        Err(SpellingError::TooLarge { .. })
    ));
}

#[test]
fn max_bytes_through_a_file_is_inclusive_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.ssp");
    std::fs::write(&path, EXAMPLE).unwrap();
    let len = EXAMPLE.len() as u64;
    let ok = SpellingSet::read(
        &path,
        &ReadOptions {
            max_bytes: len,
            ..opts(2020)
        },
        &CancelToken::new(),
        &mut NoProgress,
    );
    assert!(ok.is_ok());
    let err = SpellingSet::read(
        &path,
        &ReadOptions {
            max_bytes: len - 1,
            ..opts(2020)
        },
        &CancelToken::new(),
        &mut NoProgress,
    );
    assert!(matches!(err, Err(SpellingError::TooLarge { .. })));
}

#[test]
fn a_gzip_file_whose_text_fits_the_limit_loads_even_if_the_gzip_is_bigger() {
    // Incompressible text: the gzip stream is a little larger than the text.
    let mut text = String::from("@PLAYER\n");
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    while text.len() < 4000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let name: String = (0..12)
            .map(|i| char::from(b'A' + u8::try_from((x >> (i * 5)) % 26).unwrap()))
            .collect();
        text.push_str(&name);
        text.push('\n');
    }
    let gz = gzip(text.as_bytes());
    let limit = text.len() as u64;
    let opts = ReadOptions {
        max_bytes: limit,
        ..opts(2020)
    };
    assert!(
        SpellingSet::read_bytes(&gz, &opts).is_ok(),
        "from memory the limit applies to the decompressed size"
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rand.ssp.gz");
    std::fs::write(&path, &gz).unwrap();
    let from_file = SpellingSet::read(&path, &opts, &CancelToken::new(), &mut NoProgress);
    assert!(
        from_file.is_ok(),
        "the file path must agree with read_bytes (gzip {} bytes > limit {limit}): {:?}",
        gz.len(),
        from_file.err()
    );
}

#[test]
fn the_line_limit_is_in_bytes_and_inclusive() {
    assert_eq!(MAX_LINE_BYTES, 64 * 1024);
    let ok = format!("@PLAYER\n{}\n", "a".repeat(MAX_LINE_BYTES));
    let long = format!("@PLAYER\n{}\n", "a".repeat(MAX_LINE_BYTES + 1));
    assert_eq!(read(&ok).report().skipped_lines, 0);
    assert_eq!(read(&ok).report().players, 1);
    assert_eq!(read(&long).report().skipped_lines, 1);
    assert_eq!(read(&long).report().players, 0);
    // The terminator is not part of the line.
    let crlf = format!("@PLAYER\r\n{}\r\n", "a".repeat(MAX_LINE_BYTES));
    assert_eq!(read(&crlf).report().skipped_lines, 0);
    // Bytes, not characters.
    let two_byte_ok = format!("@PLAYER\n{}\n", "é".repeat(MAX_LINE_BYTES / 2));
    let two_byte_long = format!(
        "@PLAYER\n{}x\n{}\n",
        "é".repeat(MAX_LINE_BYTES / 2),
        "é".repeat(MAX_LINE_BYTES / 2)
    );
    assert_eq!(read(&two_byte_ok).report().skipped_lines, 0);
    assert_eq!(read(&two_byte_long).report().skipped_lines, 1);
    assert_eq!(read(&two_byte_long).report().players, 1);
}

#[test]
fn an_over_long_line_of_any_class_is_skipped_without_side_effects() {
    let junk = "x".repeat(MAX_LINE_BYTES + 10);
    let text = format!(
        "@PLAYER\nAlfa\n%Bio {junk}\n= {junk}\n%Elo {junk}\n%Prefix \"{junk}\" \"y\"\n@SITE \"{junk}\n> {junk}\nBeta\n"
    );
    let set = read(&text);
    let p = set.checker(NameKind::Player);
    assert_eq!(set.report().skipped_lines, 6);
    assert_eq!(
        set.report().players,
        2,
        "the long @SITE line did not switch the section"
    );
    assert_eq!(p.bio("Alfa").len(), 0);
    assert_eq!(p.num_aliases(), 0);
    assert!(!p.has_elo_data());
}

#[test]
fn a_comment_may_be_as_long_as_the_line_limit_and_is_kept_whole() {
    let comment = "c".repeat(MAX_LINE_BYTES - 10);
    let set = read(&format!("@PLAYER\nAlfa #{comment}\n"));
    assert_eq!(
        set.checker(NameKind::Player)
            .comment_exact("Alfa")
            .map(str::len),
        Some(comment.len())
    );
}

#[test]
fn a_directory_and_a_missing_file_are_read_errors() {
    let dir = tempfile::tempdir().unwrap();
    for path in [dir.path().to_path_buf(), dir.path().join("nope.ssp")] {
        let err = SpellingSet::read(&path, &opts(2020), &CancelToken::new(), &mut NoProgress)
            .unwrap_err();
        assert!(matches!(
            err,
            SpellingError::Io { .. } | SpellingError::NotAFile { .. }
        ));
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn progress_is_monotonic_and_ends_at_the_total() {
    let mut text = String::from("@PLAYER\n");
    for i in 0..20_000 {
        text.push_str(&format!("Name {i}\n"));
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.ssp");
    std::fs::write(&path, &text).unwrap();
    let mut calls: Vec<(u64, u64)> = Vec::new();
    let set = SpellingSet::read(&path, &opts(2020), &CancelToken::new(), &mut |d, t| {
        calls.push((d, t));
    })
    .unwrap();
    assert_eq!(set.report().players, 20_000);
    assert!(
        calls.len() >= 2,
        "at least one report in the middle and the final one"
    );
    assert!(calls.windows(2).all(|w| w[0].0 <= w[1].0), "monotonic");
    assert!(calls.iter().all(|&(d, t)| d <= t));
    let last = calls.last().unwrap();
    assert_eq!(last.0, last.1);
}

#[test]
fn cancelling_in_the_middle_yields_cancelled_and_no_set() {
    let text = "@PLAYER\nA\n".repeat(50_000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.ssp");
    std::fs::write(&path, text).unwrap();
    let token = CancelToken::new();
    let t2 = token.clone();
    let result = SpellingSet::read(&path, &opts(2020), &token, &mut move |d, _| {
        if d > 100 {
            t2.cancel();
        }
    });
    assert!(matches!(result, Err(SpellingError::Cancelled)));
}

// ---------------------------------------------------------------------------
// Truncation and corruption sweeps.
// ---------------------------------------------------------------------------

fn exercise(set: &SpellingSet) {
    for kind in NameKind::ALL {
        let c = set.checker(kind);
        for name in c.correct_names().take(20) {
            std::hint::black_box((
                c.correct(name),
                c.corrections(name, 10),
                c.comment_loose(name),
                c.comment_exact(name),
                c.bio(name).len(),
                c.elo_history(name, 1970, 0),
                c.elo_at(name, PartialDate::parse_lenient("2013.03.01"), false),
            ));
        }
    }
}

#[test]
fn every_truncation_of_the_example_either_loads_or_reports_an_unclosed_quote() {
    for cut in 0..=EXAMPLE.len() {
        match SpellingSet::read_bytes(&EXAMPLE[..cut], &opts(2020)) {
            Ok(set) => exercise(&set),
            Err(SpellingError::UnclosedQuote { .. }) => {}
            Err(other) => panic!("cut {cut}: unexpected {other:?}"),
        }
    }
}

#[test]
fn a_cut_inside_the_header_quote_is_the_unclosed_quote_error() {
    let at = EXAMPLE.windows(2).position(|w| w == b" \"").unwrap() + 2;
    // `@PLAYER "` with nothing after the opening quote.
    let err = SpellingSet::read_bytes(&EXAMPLE[..at], &opts(2020)).unwrap_err();
    assert!(
        matches!(err, SpellingError::UnclosedQuote { line: 2 }),
        "{err:?}"
    );
}

#[test]
fn every_single_corrupted_byte_of_the_example_is_survivable() {
    for at in 0..EXAMPLE.len() {
        for value in [
            0x00u8, 0x80, 0xFF, b'"', b'#', b'\n', b'\r', b'%', b'@', b'=', b':', b',',
        ] {
            let mut bytes = EXAMPLE.to_vec();
            bytes[at] = value;
            match SpellingSet::read_bytes(&bytes, &opts(2020)) {
                Ok(set) => exercise(&set),
                Err(SpellingError::UnclosedQuote { .. }) => {}
                Err(other) => panic!("byte {at} = {value:#x}: unexpected {other:?}"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rule 60 and the hostile `%Elo` digits (behaviour of D3308).
// ---------------------------------------------------------------------------

fn elo_year(line: &str, year: u16) -> Option<Vec<u16>> {
    let table = EloTable::parse([line], 2020);
    table.year(year).map(|v| v.to_vec())
}

#[rstest]
#[case::plain("2013:2300,2301", 2013, Some(vec![2300, 2301]))]
#[case::question_marks("2013:?,2301,?", 2013, Some(vec![0, 2301, 0]))]
#[case::one_digit("2013:5", 2013, Some(vec![5]))]
#[case::leading_zeros("2013:0012", 2013, Some(vec![12]))]
#[case::five_digit_value_is_invalid_data("2013:2300,23000,2400", 2013, Some(vec![2300]))]
#[case::letter_stops_the_line("2013:2300,x,2400", 2013, Some(vec![2300]))]
#[case::signed_value_stops("2013:2300,-5", 2013, Some(vec![2300]))]
#[case::decimal_point_stops("2013:2300.5,2400", 2013, Some(vec![2300]))]
#[case::semicolon_stops("2013:2300;2400", 2013, Some(vec![2300]))]
#[case::no_colon_drops_the_group("2013 2300", 2013, None)]
#[case::five_digit_year("20130:2300", 2013, None)]
#[case::year_before_1970("1969:2300", 1969, None)]
#[case::year_after_current("2021:2300", 2021, None)]
#[case::year_equal_to_current("2020:2300", 2020, Some(vec![2300]))]
#[case::first_year("1970:2300", 1970, Some(vec![2300]))]
#[case::thirteenth_value_dropped("2013:1,2,3,4,5,6,7,8,9,10,11,12,13", 2013, Some(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]))]
#[case::empty_value_between_commas_is_invalid_data("2013:5,,7", 2013, Some(vec![5, 0]))]
fn elo_line_values(#[case] line: &str, #[case] year: u16, #[case] expected: Option<Vec<u16>>) {
    let got = elo_year(line, year).map(|mut v| {
        // Compare only up to the number of values written.
        let n = expected.as_ref().map_or(0, Vec::len);
        v.truncate(n);
        v
    });
    assert_eq!(got, expected, "{line}");
}

#[test]
fn elo_groups_after_an_invalid_one_are_dropped_with_it() {
    // "interrompe a leitura da linha, conservando o que já foi lido"
    let t = EloTable::parse(["2012:2200 2013:x 2014:2400"], 2020);
    assert!(t.year(2012).is_some());
    assert!(t.year(2014).is_none());
}

#[test]
fn elo_groups_may_be_in_any_order_and_repeat() {
    let t = EloTable::parse(["2014:2400 2012:2200,2201 2014:9,2401"], 2020);
    let years: Vec<u16> = t.years().map(|(y, _)| y).collect();
    assert_eq!(years, [2012, 2014], "years come out sorted");
    assert_eq!(
        t.year(2014).unwrap()[..2],
        [9, 2401],
        "a repeat overwrites the positions it defines (a ? defines a 0)"
    );
}

#[test]
fn elo_lines_accumulate_across_lines_of_the_same_name() {
    let t = EloTable::parse(["2013:2300,2301,2302", "2013:9", "2014:5"], 2020);
    assert_eq!(t.year(2013).unwrap()[..3], [9, 2301, 2302]);
    assert_eq!(t.year(2014).unwrap()[0], 5);
}

#[test]
fn elo_text_with_huge_runs_of_digits_does_not_panic_or_overflow() {
    let huge = "9".repeat(10_000);
    for line in [
        format!("{huge}:1"),
        format!("2013:{huge}"),
        format!("2013:1,{huge},3"),
        huge.clone(),
        format!("2013:{}", "1,".repeat(30_000)),
        ":::".to_owned(),
        String::new(),
        "\u{ff12}\u{ff10}\u{ff11}\u{ff13}:2300".to_owned(),
    ] {
        let t = EloTable::parse([line.as_str()], 2020);
        for (_, values) in t.years() {
            assert_eq!(values.len(), 12);
        }
    }
}

// ---------------------------------------------------------------------------
// Properties.
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn arbitrary_unicode_text_never_panics(text in "\\PC{0,400}") {
        if let Ok(set) = SpellingSet::read_bytes(text.as_bytes(), &opts(2020)) {
            exercise(&set);
        }
    }

    #[test]
    fn arbitrary_gzip_payloads_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let mut stream = vec![0x1f, 0x8b];
        stream.extend_from_slice(&bytes);
        drop(SpellingSet::read_bytes(&stream, &opts(2020)));
        drop(SpellingSet::read_bytes(&gzip(&bytes), &opts(2020)));
    }

    #[test]
    fn elo_text_is_always_survivable(tail in "[0-9:,? \\-.;a-z\u{ff10}-\u{ff19}]{0,120}") {
        let t = EloTable::parse([tail.as_str()], 2020);
        for (year, values) in t.years() {
            prop_assert!((1970..=2020).contains(&year));
            prop_assert!(values.iter().all(|&v| v <= 9999));
        }
    }

    #[test]
    fn reading_is_independent_of_the_line_terminator(names in proptest::collection::vec("[A-Za-z]{1,8}( #[A-Z ]{0,8})?", 1..12)) {
        let body = names.join("\n");
        let lf = read(&format!("@PLAYER\n{body}\n"));
        let crlf = read(&format!("@PLAYER\r\n{}\r\n", names.join("\r\n")));
        let cr = read(&format!("@PLAYER\r{}\r", names.join("\r")));
        prop_assert_eq!(lf.report(), crlf.report());
        prop_assert_eq!(lf.report(), cr.report());
        let a: Vec<&str> = lf.checker(NameKind::Player).correct_names().collect();
        let b: Vec<&str> = cr.checker(NameKind::Player).correct_names().collect();
        prop_assert_eq!(a, b);
    }

    #[test]
    fn a_name_with_a_non_empty_key_finds_itself(
        names in proptest::collection::vec("[A-Z][a-z]{1,5}[,. -]{0,2}[A-Z]?[a-z]{1,4}", 1..16)
    ) {
        let mut text = String::from("@PLAYER \",. -\"\n");
        for n in &names {
            text.push_str(n);
            text.push('\n');
        }
        let set = read(&text);
        let p = set.checker(NameKind::Player);
        for n in &names {
            prop_assert_eq!(p.correct(n).map(|c| c.replace([',', '.', ' ', '-'], "")), Some(n.replace([',', '.', ' ', '-'], "")));
            prop_assert!(p.comment_exact(n).is_some());
        }
    }
}

// ===========================================================================
// Photos: `.spf` reading, keys, signatures, the library (rules 101 to 106,
// `docs/formatos/spf-fotos.md`).
// ===========================================================================

mod photos {
    use std::path::{Path, PathBuf};

    use mychess_db::CancelToken;
    use mychess_db::si4::NameKind;
    use mychess_db::spelling::photo::{
        MAX_PHOTO_BYTES, MAX_PHOTOS, MAX_SPF_BYTES, PhotoKind, PhotoLibrary, PhotoSource,
        detect_kind, photo_key, read_spf,
    };
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::read;

    /// The 1x1 GIF of the specification.
    const GIF_B64: &str = "R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";

    fn b64(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |acc, (i, &b)| acc | u32::from(b) << (16 - 8 * i));
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(char::from(
                        A[usize::try_from((n >> (18 - 6 * i)) & 63).unwrap()],
                    ));
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    fn gif() -> Vec<u8> {
        let mut v = b"GIF89a".to_vec();
        v.extend_from_slice(&[1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x3B]);
        v
    }

    fn png() -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend_from_slice(b"rest");
        v
    }

    fn jpeg() -> Vec<u8> {
        vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0]
    }

    // ---- detect_kind ----

    #[rstest]
    #[case::gif87(b"GIF87a....".to_vec(), Some(PhotoKind::Gif))]
    #[case::gif89(b"GIF89a....".to_vec(), Some(PhotoKind::Gif))]
    #[case::gif_exact_signature(b"GIF89a".to_vec(), Some(PhotoKind::Gif))]
    #[case::gif_truncated(b"GIF89".to_vec(), None)]
    #[case::gif_wrong_version(b"GIF88a....".to_vec(), None)]
    #[case::gif_lowercase(b"gif89a....".to_vec(), None)]
    #[case::png(png(), Some(PhotoKind::Png))]
    #[case::png_truncated_signature(vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A], None)]
    #[case::png_wrong_tail(vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0B], None)]
    #[case::jpeg(jpeg(), Some(PhotoKind::Jpeg))]
    #[case::jpeg_three_bytes(vec![0xFF, 0xD8, 0xFF], Some(PhotoKind::Jpeg))]
    #[case::jpeg_two_bytes(vec![0xFF, 0xD8], None)]
    #[case::jpeg_wrong_third(vec![0xFF, 0xD8, 0x00], None)]
    #[case::empty(Vec::new(), None)]
    #[case::text(b"hello world".to_vec(), None)]
    #[case::html(b"<html><img>".to_vec(), None)]
    #[case::svg(b"<svg xmlns=\"\"/>".to_vec(), None)]
    #[case::zip(b"PK\x03\x04".to_vec(), None)]
    #[case::bmp(b"BM\x00\x00".to_vec(), None)]
    #[case::zeros(vec![0; 64], None)]
    fn signature_detection(#[case] bytes: Vec<u8>, #[case] kind: Option<PhotoKind>) {
        assert_eq!(detect_kind(&bytes), kind);
    }

    // ---- photo_key ----

    #[rstest]
    #[case("Carlsen, Magnus", "carlsen,magnus")]
    #[case("carlsen,magnus", "carlsen,magnus")]
    #[case("CARLSEN,   MAGNUS", "carlsen,magnus")]
    #[case("Deep Fritz 11", "fritz11")]
    #[case("DEEP FRITZ 11", "fritz11")]
    #[case("deep fritz", "fritz")]
    #[case("Deep", "deep")]
    #[case("Deeper Blue", "deeperblue")]
    #[case("  Deep Fritz", "deepfritz")]
    #[case("Green Light Chess", "greenlightchess")]
    #[case("A\tB\u{a0}C\u{3000}D", "abcd")]
    #[case("ÉVA Ç", "évaç")]
    #[case("", "")]
    #[case("   ", "")]
    fn keys_without_a_spelling_file(#[case] name: &str, #[case] key: &str) {
        assert_eq!(photo_key(name, None), key);
    }

    #[test]
    fn keys_go_through_the_correct_spelling_when_there_is_a_checker() {
        let set = read("@PLAYER \"., \"\nCarlsen, Magnus\n= Carlsen, M\n= Magnus Carlsen\n");
        let p = set.checker(NameKind::Player);
        let want = photo_key("Carlsen, Magnus", None);
        for alias in [
            "Carlsen, M",
            "Magnus Carlsen",
            "Carlsen, Magn",
            "Carlsen,M.",
        ] {
            assert_eq!(photo_key(alias, Some(p)), want, "{alias}");
        }
        assert_eq!(
            photo_key("Nobody, N", Some(p)),
            "nobody,n",
            "no correction: the name itself"
        );
        assert_eq!(
            photo_key("Nobody, N", None),
            photo_key("Nobody, N", Some(p))
        );
    }

    // ---- read_spf ----

    fn names(photos: &[(String, Vec<u8>)]) -> Vec<&str> {
        photos.iter().map(|(n, _)| n.as_str()).collect()
    }

    #[test]
    fn spf_accepts_every_word_style_and_separator_of_the_specification() {
        let text = format!(
            "# Fotos\nphoto \"Almeida, Beatriz Costa\" {{{GIF_B64}}}\n\
             photo {{Braga, Carlos Eduardo}} \"{GIF_B64}\"; photo Solo {GIF_B64}\n\
             photo \"A, B\" \\\n   {GIF_B64}\n\
             photo {{Nested {{braces}} name}} {GIF_B64}\r\n\
             photo \"Esc \\\"quoted\\\" name\" {GIF_B64}\n\
             \n   # indented comment\n   photo   Spaced    {GIF_B64}   \n"
        );
        let (photos, report) = read_spf(text.as_bytes());
        assert_eq!(
            names(&photos),
            [
                "Almeida, Beatriz Costa",
                "Braga, Carlos Eduardo",
                "Solo",
                "A, B",
                "Nested {braces} name",
                "Esc \"quoted\" name",
                "Spaced"
            ]
        );
        assert!(photos.iter().all(|(_, img)| img.starts_with(b"GIF89a")));
        assert_eq!(
            (report.photos, report.invalid, report.ignored_commands),
            (7, 0, 0)
        );
        assert!(!report.truncated);
    }

    #[test]
    fn spf_text_is_never_interpreted() {
        let text = format!("photo {{$x [cmd] \\n name}} {GIF_B64}\nphoto \"$HOME\" {GIF_B64}\n");
        let (photos, _) = read_spf(text.as_bytes());
        assert_eq!(photos[0].0, "$x [cmd] \\n name", "braces keep everything");
        assert_eq!(photos[1].0, "$HOME", "no variable substitution");
    }

    #[test]
    fn spf_commands_other_than_photo_are_counted_and_never_executed() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker.txt");
        std::fs::write(&marker, b"keep me").unwrap();
        let created = dir.path().join("created-by-spf.txt");
        let p = |path: &Path| path.display().to_string().replace('\\', "/");
        let text = format!(
            "exec touch {{{created}}}\n\
             file delete {{{marker}}}\n\
             file delete -force {{{marker}}}\n\
             set x [exec touch {{{created}}}]\n\
             source {{{created}}}\n\
             eval {{exec touch {created}}}\n\
             open {{{created}}} w\n\
             puts [exec touch {created}]\n\
             exit\n\
             rm -rf /\n\
             photo \"Ok\" {GIF_B64}\n\
             proc photo {{a b}} {{exec touch {created}}}\n\
             Photo \"Upper\" {GIF_B64}\n\
             photos \"Plural\" {GIF_B64}\n\
             {{photo}} \"Braced\" {GIF_B64}\n\
             \"photo\" \"Quoted\" {GIF_B64}\n",
            created = p(&created),
            marker = p(&marker),
        );
        let (photos, report) = read_spf(text.as_bytes());
        assert!(marker.exists(), "file delete must not run");
        assert_eq!(std::fs::read(&marker).unwrap(), b"keep me");
        assert!(!created.exists(), "exec/open/source must not run");
        // The first `photo` is used; `proc photo` is just another command.
        assert_eq!(photos[0].0, "Ok");
        assert_eq!(
            report.ignored_commands, 13,
            "11 foreign commands, `Photo` and `photos`: {report:?}"
        );
        assert_eq!(
            report.photos, 3,
            "`photo`, {{photo}} and \"photo\" are the same Tcl word"
        );
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "nothing was created"
        );
    }

    #[test]
    fn spf_commands_with_the_wrong_number_of_words_are_invalid_not_executed() {
        let cases = [
            "photo".to_owned(),
            "photo OnlyName".to_owned(),
            format!("photo A B {GIF_B64}"),
            format!("photo A {GIF_B64} extra"),
        ];
        for text in cases {
            let (photos, report) = read_spf(text.as_bytes());
            assert!(photos.is_empty(), "{text}");
            assert_eq!(report.invalid, 1, "{text}");
        }
    }

    #[rstest]
    #[case::bad_alphabet("photo A {R0lG*ODlh}")]
    #[case::url_safe_alphabet("photo A {R0lGODlhAQAB-_8=}")]
    #[case::not_an_image("photo A {aGVsbG8gd29ybGQ=}")]
    #[case::empty_data("photo A {}")]
    #[case::empty_data_quotes("photo A \"\"")]
    #[case::truncated_signature("photo A {R0lGOD}")]
    #[case::padding_in_the_middle("photo A {R0lG=ODlhAQAB}")]
    #[case::data_after_padding("photo A {R0lGODlhAQ==AQAB}")]
    #[case::empty_name("photo {} {R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7}")]
    fn spf_invalid_photos_are_dropped_and_counted(#[case] text: &str) {
        let (photos, report) = read_spf(text.as_bytes());
        assert!(photos.is_empty(), "{text}");
        assert_eq!((report.photos, report.invalid), (0, 1), "{text}");
    }

    #[test]
    fn spf_base64_ignores_spaces_and_line_breaks_and_accepts_every_padding() {
        let image = gif();
        for len in 0..4 {
            let mut data = image.clone();
            data.extend(std::iter::repeat_n(0u8, len));
            let enc = b64(&data);
            let spaced: String = enc
                .as_bytes()
                .chunks(7)
                .map(|c| std::str::from_utf8(c).unwrap())
                .collect::<Vec<_>>()
                .join(" \n\t");
            let text = format!("photo A {{{spaced}}}\nphoto B \"{enc}\"\nphoto C {enc}\n");
            let (photos, report) = read_spf(text.as_bytes());
            assert_eq!(report.photos, 3, "padding case {len}: {report:?}");
            assert_eq!(photos[0].1, data);
        }
    }

    #[test]
    fn spf_photos_at_and_over_the_size_limit() {
        let mut ok = b"GIF89a".to_vec();
        ok.resize(MAX_PHOTO_BYTES, 0);
        let mut big = ok.clone();
        big.push(0);
        let text = format!("photo ok {{{}}}\nphoto big {{{}}}\n", b64(&ok), b64(&big));
        let (photos, report) = read_spf(text.as_bytes());
        assert_eq!(names(&photos), ["ok"]);
        assert_eq!((report.photos, report.invalid), (1, 1));
    }

    #[test]
    fn spf_stops_at_an_unterminated_word_keeping_what_came_before() {
        for tail in [
            "photo \"Open quote {x}",
            "photo {Open brace x",
            "photo A \"unterminated",
        ] {
            let text = format!("photo Good {GIF_B64}\n{tail}");
            let (photos, report) = read_spf(text.as_bytes());
            assert_eq!(names(&photos), ["Good"], "{tail}");
            assert_eq!(report.invalid, 1, "{tail}");
        }
    }

    #[test]
    fn spf_bom_blank_files_and_binary_garbage() {
        let mut with_bom = b"\xEF\xBB\xBF".to_vec();
        with_bom.extend_from_slice(format!("photo A {GIF_B64}\n").as_bytes());
        assert_eq!(read_spf(&with_bom).0.len(), 1);
        for text in [
            &b""[..],
            b"\n\n\n",
            b"# only a comment",
            b";;;;",
            b"\\",
            b"\\\n",
            b"{",
            b"}",
            b"\"",
            b"\xFF\xFE\x00",
        ] {
            let (photos, _) = read_spf(text);
            assert!(photos.is_empty());
        }
        // Names that are not UTF-8 do not panic.
        let mut bytes = b"photo J\xE9 ".to_vec();
        bytes.extend_from_slice(GIF_B64.as_bytes());
        bytes.push(b'\n');
        assert_eq!(read_spf(&bytes).0.len(), 1);
    }

    #[test]
    fn spf_limit_of_photos_per_file_marks_it_truncated() {
        let line = format!("photo p{{}} {GIF_B64}\n");
        let mut text = String::with_capacity((MAX_PHOTOS + 10) * 70);
        for i in 0..MAX_PHOTOS + 10 {
            text.push_str(&line.replace("{}", &i.to_string()));
        }
        let (photos, report) = read_spf(text.as_bytes());
        assert_eq!(photos.len(), MAX_PHOTOS);
        assert!(report.truncated);
        assert_eq!(report.photos as usize, MAX_PHOTOS);
    }

    #[test]
    fn spf_file_over_the_size_limit_is_refused_at_once() {
        let big = vec![b'#'; MAX_SPF_BYTES + 1];
        let (photos, report) = read_spf(&big);
        assert!(photos.is_empty());
        assert!(report.truncated);
    }

    // ---- PhotoLibrary ----

    fn library(names: &[&str]) -> PhotoLibrary {
        let mut lib = PhotoLibrary::new();
        for n in names {
            lib.insert(n, PhotoSource::Bytes(gif().into()));
        }
        lib
    }

    #[test]
    fn lookup_ignores_case_and_spaces_and_the_deep_prefix() {
        let lib = library(&["Carlsen, Magnus", "Fritz"]);
        for q in ["carlsen,magnus", "CARLSEN,   MAGNUS", "Carlsen,Magnus"] {
            assert!(lib.lookup(q, None).is_some(), "{q}");
        }
        assert!(lib.lookup("Deep Fritz", None).is_some());
        assert!(
            lib.lookup("Carlsen, Magnu", None).is_none(),
            "a name with a comma has no prefix search"
        );
        assert!(lib.lookup("", None).is_none());
        assert_eq!(lib.len(), 2);
    }

    #[test]
    fn lookup_of_a_name_without_comma_takes_the_longest_prefix_of_at_least_three() {
        let lib = library(&["fri", "fritz", "ab"]);
        let key = |q: &str| lib.lookup(q, None).map(|s| format!("{s:?}"));
        assert!(key("Deep Fritz 11").is_some());
        let mut only_short = PhotoLibrary::new();
        only_short.insert("fri", PhotoSource::File(PathBuf::from("short")));
        only_short.insert("fritz", PhotoSource::File(PathBuf::from("long")));
        assert_eq!(
            only_short.lookup("Fritz 11", None),
            Some(&PhotoSource::File(PathBuf::from("long")))
        );
        assert_eq!(
            only_short.lookup("Fridge", None),
            Some(&PhotoSource::File(PathBuf::from("short")))
        );
        assert!(
            only_short.lookup("Fr", None).is_none(),
            "two characters: no prefix search"
        );
        assert!(
            only_short.lookup("abx", None).is_none(),
            "a prefix of two characters is not tried"
        );
        assert!(lib.lookup("ab", None).is_some(), "but the exact key is");
    }

    #[test]
    fn lookup_goes_through_the_correct_spelling() {
        let set = read("@PLAYER \"., \"\nPina, Paula\n= Pina, P\n");
        let p = set.checker(NameKind::Player);
        let lib = library(&["Pina, Paula"]);
        assert!(lib.lookup("Pina, P", Some(p)).is_some());
        assert!(lib.lookup("Pina, P", None).is_none());
        assert!(lib.lookup("Pina, Paula", Some(p)).is_some());
    }

    #[test]
    fn insert_replaces_ignores_empty_keys_and_respects_the_limit() {
        let mut lib = PhotoLibrary::new();
        lib.insert("A", PhotoSource::File("one".into()));
        lib.insert("a", PhotoSource::File("two".into()));
        assert_eq!(lib.len(), 1);
        assert_eq!(
            lib.lookup("A", None),
            Some(&PhotoSource::File("two".into()))
        );
        lib.insert("   ", PhotoSource::File("empty".into()));
        lib.insert("", PhotoSource::File("empty".into()));
        assert_eq!(lib.len(), 1);
        for i in 0..MAX_PHOTOS + 5 {
            lib.insert(&format!("player{i}"), PhotoSource::File("x".into()));
        }
        assert_eq!(lib.len(), MAX_PHOTOS);
        lib.insert("a", PhotoSource::File("three".into()));
        assert_eq!(
            lib.lookup("a", None),
            Some(&PhotoSource::File("three".into())),
            "replacing is allowed when full"
        );
        assert!(!lib.is_empty());
    }

    fn spf_file(dir: &Path, name: &str, entries: &[(&str, &[u8])]) {
        let mut text = String::new();
        for (n, img) in entries {
            text.push_str(&format!("photo {{{n}}} {{{}}}\n", b64(img)));
        }
        std::fs::write(dir.join(name), text).unwrap();
    }

    fn source_len(lib: &PhotoLibrary, name: &str) -> Option<usize> {
        match lib.lookup(name, None)? {
            PhotoSource::Bytes(b) => Some(b.len()),
            PhotoSource::File(p) => {
                Some(usize::try_from(std::fs::metadata(p).unwrap().len()).unwrap())
            }
        }
    }

    #[test]
    fn load_dir_reads_spf_in_alphabetical_order_then_loose_images_that_win() {
        let dir = tempfile::tempdir().unwrap();
        let mut long_gif = gif();
        long_gif.extend_from_slice(&[0; 50]);
        let mut longer_gif = gif();
        longer_gif.extend_from_slice(&[0; 100]);
        spf_file(
            dir.path(),
            "a.spf",
            &[("Same, Player", &gif()), ("Only, A", &gif())],
        );
        spf_file(
            dir.path(),
            "b.spf",
            &[("Same, Player", &long_gif), ("Loose, Wins", &gif())],
        );
        spf_file(dir.path(), "C.SPF", &[("Upper, Ext", &gif())]);
        std::fs::write(dir.path().join("Loose, Wins.GIF"), &longer_gif).unwrap();
        std::fs::write(dir.path().join("Png, Image.Png"), png()).unwrap();
        std::fs::write(dir.path().join("Jpg, Image.jpg"), jpeg()).unwrap();
        std::fs::write(dir.path().join("Jpeg, Image.JPEG"), jpeg()).unwrap();
        std::fs::write(dir.path().join("Dots, M. Jr.gif"), gif()).unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a photo").unwrap();
        std::fs::write(dir.path().join("README"), "x").unwrap();
        std::fs::create_dir(dir.path().join("Sub, Dir.gif")).unwrap();
        let mut lib = PhotoLibrary::new();
        let report = lib.load_dir(dir.path(), &CancelToken::new());
        assert_eq!(report.from_spf, 5);
        assert_eq!(report.loose, 5);
        assert_eq!(report.errors, 0);
        assert_eq!(
            source_len(&lib, "Same, Player"),
            Some(long_gif.len()),
            "the last .spf wins"
        );
        assert_eq!(
            source_len(&lib, "Loose, Wins"),
            Some(longer_gif.len()),
            "a loose image beats an .spf"
        );
        for n in [
            "Only, A",
            "Upper, Ext",
            "Png, Image",
            "Jpg, Image",
            "Jpeg, Image",
            "Dots, M. Jr",
        ] {
            assert!(lib.lookup(n, None).is_some(), "{n}");
        }
        assert!(lib.lookup("notes", None).is_none());
        assert!(
            lib.lookup("Sub, Dir", None).is_none(),
            "a directory is not a photo"
        );
        // Loose images are referenced by path, not read into memory.
        assert!(matches!(
            lib.lookup("Png, Image", None),
            Some(PhotoSource::File(_))
        ));
        assert!(matches!(
            lib.lookup("Only, A", None),
            Some(PhotoSource::Bytes(_))
        ));
    }

    #[test]
    fn load_dir_counts_unusable_files_without_failing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("fake.gif"), "this is text").unwrap();
        std::fs::write(dir.path().join("empty.png"), "").unwrap();
        std::fs::write(dir.path().join("short.jpg"), [0xFF]).unwrap();
        let mut huge = b"GIF89a".to_vec();
        huge.resize(MAX_PHOTO_BYTES + 1, 0);
        std::fs::write(dir.path().join("huge.gif"), &huge).unwrap();
        spf_file(dir.path(), "good.spf", &[("Good, G", &gif())]);
        std::fs::write(
            dir.path().join("script.spf"),
            "exec rm -rf /\nphoto A {not base64!}\nphoto\n",
        )
        .unwrap();
        let mut lib = PhotoLibrary::new();
        let report = lib.load_dir(dir.path(), &CancelToken::new());
        assert_eq!(report.errors, 4, "{report:?}");
        assert_eq!(report.loose, 0);
        assert_eq!(report.from_spf, 1);
        assert_eq!(
            report.spf_ignored, 3,
            "one exec, one bad photo, one bare photo"
        );
        assert_eq!(lib.len(), 1);
    }

    #[test]
    fn load_dir_of_nothing_is_an_empty_report() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = PhotoLibrary::new();
        for path in [dir.path().join("missing"), dir.path().to_path_buf()] {
            let r = lib.load_dir(&path, &CancelToken::new());
            assert_eq!((r.from_spf, r.loose, r.errors, r.spf_ignored), (0, 0, 0, 0));
        }
        let file = dir.path().join("a.spf");
        std::fs::write(&file, "").unwrap();
        assert_eq!(
            lib.load_dir(&file, &CancelToken::new()).loose,
            0,
            "a file is not a directory"
        );
        assert!(lib.is_empty());
    }

    #[test]
    fn load_dir_cancelled_returns_without_panic() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..20 {
            std::fs::write(dir.path().join(format!("p{i}.gif")), gif()).unwrap();
        }
        let token = CancelToken::new();
        token.cancel();
        let mut lib = PhotoLibrary::new();
        let r = lib.load_dir(dir.path(), &token);
        assert!(r.loose < 20);
    }

    #[test]
    fn load_dir_a_file_named_only_by_its_extension_adds_no_photo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gif"), gif()).unwrap();
        let mut lib = PhotoLibrary::new();
        let _ = lib.load_dir(dir.path(), &CancelToken::new());
        assert!(lib.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn load_dir_does_not_hang_on_fifos_and_survives_non_utf8_names() {
        use std::os::unix::ffi::OsStrExt;
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let made = |name: &str| {
            std::process::Command::new("mkfifo")
                .arg(dir.path().join(name))
                .status()
                .is_ok_and(|s| s.success())
        };
        let fifos = made("pipe.gif") && made("pipe.spf") && made("pipe.png");
        if !fifos {
            eprintln!("mkfifo is not available: skipping the FIFO part");
        }
        let odd = std::ffi::OsStr::from_bytes(b"J\xE9\xFF.gif");
        std::fs::write(dir.path().join(odd), gif()).unwrap();
        spf_file(dir.path(), "ok.spf", &[("Ok, O", &gif())]);
        let path = dir.path().to_path_buf();
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut lib = PhotoLibrary::new();
            let report = lib.load_dir(&path, &CancelToken::new());
            tx.send((report, lib.len())).ok();
        });
        let (report, len) = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("load_dir blocked on a FIFO");
        worker.join().unwrap();
        assert_eq!(report.from_spf, 1);
        assert_eq!(
            report.errors, 0,
            "FIFOs are not regular files: skipped, not errors"
        );
        assert!(len >= 1);
    }

    #[cfg(unix)]
    #[test]
    fn load_dir_follows_a_symlink_to_an_image_but_not_one_to_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        std::fs::write(target.path().join("real.gif"), gif()).unwrap();
        std::os::unix::fs::symlink(
            target.path().join("real.gif"),
            dir.path().join("Linked, L.gif"),
        )
        .unwrap();
        std::os::unix::fs::symlink(target.path(), dir.path().join("dirlink.gif")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("nowhere"), dir.path().join("dangling.gif"))
            .unwrap();
        let mut lib = PhotoLibrary::new();
        let report = lib.load_dir(dir.path(), &CancelToken::new());
        assert!(report.errors <= 2, "{report:?}");
        assert!(lib.lookup("dirlink", None).is_none());
        assert!(lib.lookup("dangling", None).is_none());
    }
}
