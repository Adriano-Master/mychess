//! Tests of the reading of the move list.

use super::*;

fn san(text: &str) -> MoveToken {
    MoveToken::San(text.to_owned())
}

fn words(text: &str) -> Result<MovesSearch, SearchError> {
    MovesSearch::parse(text, SideFilter::Both, CheckTest::No, FilterOp::Reset)
}

#[test]
fn reads_plain_san_and_the_wildcard() {
    let q = words("e4 ? Nf3 O-O exd5 e8=Q Nbd7 R1a3 Qh1xg2 --").unwrap();
    assert_eq!(
        q.tokens,
        vec![
            san("e4"),
            MoveToken::Any,
            san("Nf3"),
            san("O-O"),
            san("exd5"),
            san("e8=Q"),
            san("Nbd7"),
            san("R1a3"),
            san("Qh1xg2"),
            san("--"),
        ]
    );
}

#[test]
fn reads_numbers_castling_zeros_and_annotations_tolerantly() {
    let q = words("1.e4 1...e5 12. 2.Nf3 0-0 0-0-0 exd5!? e8Q Nf3?! exd6e.p.").unwrap();
    assert_eq!(
        q.tokens,
        vec![
            san("e4"),
            san("e5"),
            san("Nf3"),
            san("O-O"),
            san("O-O-O"),
            san("exd5"),
            san("e8=Q"),
            san("Nf3"),
            san("exd6"),
        ]
    );
}

#[test]
fn a_check_marker_raises_the_check_test() {
    let q = words("Qh7+").unwrap();
    assert_eq!(q.check_test, CheckTest::Check);
    assert_eq!(q.tokens, vec![san("Qh7+")]);
    let q = words("Qh7+ Rh8#").unwrap();
    assert_eq!(q.check_test, CheckTest::Mate);
    let q = MovesSearch::parse("Qh7", SideFilter::Both, CheckTest::Mate, FilterOp::Or).unwrap();
    assert_eq!(q.check_test, CheckTest::Mate);
    assert_eq!(q.op, FilterOp::Or);
}

#[test]
fn refuses_what_is_not_a_move() {
    for bad in [
        "Ng1-f3", "Nf", "Z4", "e9", "i4", "Nxx3", "ee4", "e4e5", "O-O-O-O", "e8=K", "Pe4", "exd",
        "Raa1a", "!?", "é4",
    ] {
        assert!(
            matches!(words(bad), Err(SearchError::BadMoveToken { .. })),
            "{bad}"
        );
    }
    let long = "a".repeat(40);
    assert!(matches!(
        words(&long),
        Err(SearchError::BadMoveToken { .. })
    ));
}

#[test]
fn refuses_an_empty_list_and_a_leading_wildcard() {
    assert_eq!(words("").unwrap_err(), SearchError::EmptyMoveList);
    assert_eq!(words("  \n ").unwrap_err(), SearchError::EmptyMoveList);
    assert_eq!(words("1. 2...").unwrap_err(), SearchError::EmptyMoveList);
    assert_eq!(words("? e5").unwrap_err(), SearchError::FirstMoveIsWildcard);
}

#[test]
fn too_many_words_are_refused_and_the_limit_is_accepted() {
    let ok = vec!["e4"; MAX_MOVE_TOKENS].join(" ");
    assert!(words(&ok).is_ok());
    let too_many = vec!["e4"; MAX_MOVE_TOKENS + 1].join(" ");
    assert!(matches!(
        words(&too_many),
        Err(SearchError::BadMoveToken { .. })
    ));
}

#[test]
fn validate_checks_a_search_built_by_hand() {
    let mut q = words("e4 e5").unwrap();
    assert!(q.validate().is_ok());
    q.tokens = vec![MoveToken::Any];
    assert_eq!(q.validate(), Err(SearchError::FirstMoveIsWildcard));
    q.tokens = vec![];
    assert_eq!(q.validate(), Err(SearchError::EmptyMoveList));
    q.tokens = vec![san("Ng1-f3")];
    assert!(matches!(
        q.validate(),
        Err(SearchError::BadMoveToken { .. })
    ));
    q.tokens = vec![san("0-0")];
    assert!(matches!(
        q.validate(),
        Err(SearchError::BadMoveToken { .. })
    ));
}

#[test]
fn shapes_tell_the_piece_and_the_destination() {
    let Some(Shape::Move {
        role,
        to,
        promotion,
    }) = shape_of("Nbd7")
    else {
        panic!("shape");
    };
    assert_eq!((role, to, promotion), (Role::Knight, Square::D7, None));
    let Some(Shape::Move {
        role,
        to,
        promotion,
    }) = shape_of("exd8=Q+")
    else {
        panic!("shape");
    };
    assert_eq!(
        (role, to, promotion),
        (Role::Pawn, Square::D8, Some(Role::Queen))
    );
    assert!(matches!(
        shape_of("O-O-O"),
        Some(Shape::Castle { kingside: false })
    ));
    assert!(matches!(shape_of("--"), Some(Shape::Null)));
}

#[test]
fn validate_refuses_a_marker_the_check_test_does_not_include() {
    // Built by hand: parse would have raised the check test.
    let mut q = words("e4 Qh7+").unwrap();
    assert_eq!(q.check_test, CheckTest::Check);
    assert_eq!(q.validate(), Ok(()));
    q.check_test = CheckTest::No;
    assert!(matches!(
        q.validate(),
        Err(SearchError::CheckTestTooLow { token }) if token == "Qh7+"
    ));
    let mut m = words("e4 Qh7#").unwrap();
    assert_eq!(m.check_test, CheckTest::Mate);
    m.check_test = CheckTest::Check;
    assert!(matches!(
        m.validate(),
        Err(SearchError::CheckTestTooLow { .. })
    ));
    m.check_test = CheckTest::Mate;
    assert_eq!(m.validate(), Ok(()));
}

#[test]
fn only_one_final_marker_is_accepted() {
    for bad in ["Qh4++", "Qh4+#", "Qh4#+", "Qh4##"] {
        assert!(
            matches!(words(bad), Err(SearchError::BadMoveToken { .. })),
            "{bad}"
        );
    }
    assert_eq!(words("Qh4+").unwrap().tokens, vec![san("Qh4+")]);
    assert_eq!(words("Qh4#!").unwrap().tokens, vec![san("Qh4#")]);
}
