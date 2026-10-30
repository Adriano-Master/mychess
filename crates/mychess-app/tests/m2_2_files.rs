//! Opening and saving PGN files, names and recent files (spec M2-2, rules
//! 24-35 and D124-D131). Everything runs in `tempfile` directories.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::default_trait_access,
    clippy::format_push_string,
    clippy::items_after_statements,
    clippy::assigning_clones,
    clippy::field_reassign_with_default,
    clippy::type_complexity,
    let_underscore_drop
)]

mod m2_2_common;

use std::path::{Path, PathBuf};

use m2_2_common::*;
use mychess_app::{
    AppNotice, EncodingPref, FileError, FileErrorKind, GameHeader, MAX_RECENT_FILES, PgnLoad,
    PgnSource, PickSpec, RecentFiles, SaveMode, ScanLimit, ScanLimits, ensure_pgn_extension,
    save_pgn, scan_pgn, suggest_file_name,
};
use mychess_core::{GameResult, Tags};
use mychess_pgn::read::{ReadOptions, parse_pgn_text};
use proptest::prelude::*;
use rstest::rstest;

const ACCENTED: &str = "[Event \"Torneio S\u{e3}o Paulo\"]\n[Site \"?\"]\n[Date \"2024.05.17\"]\n[Round \"1\"]\n[White \"Jos\u{e9}\"]\n[Black \"M\u{fc}ller\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n";

fn latin1(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap())
        .collect()
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A valid gzip stream with "stored" deflate blocks (no compression).
fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    let mut chunks = data.chunks(60_000).peekable();
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(chunk) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none()));
        let len = u16::try_from(chunk.len()).unwrap();
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out
}

fn scan_file(path: &Path) -> Result<PgnLoad, FileError> {
    scan_pgn(
        PgnSource::File(path.to_path_buf()),
        &ScanLimits::default(),
        &ReadOptions::default(),
    )
    .map(|(load, _)| load)
}

fn loaded(path: &Path) -> mychess_app::LoadedGame {
    match scan_file(path).unwrap() {
        PgnLoad::Single(g) => g,
        other => panic!("expected one game, got {other:?}"),
    }
}

fn open_in_harness(h: &mut Harness, path: &Path) {
    h.send(Command::Open);
    h.pick(Some(path));
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap()
}

// ------------------------------------------------------------------ rule 24

#[test]
fn rule24_one_game_file_loads_directly_with_origin_and_recent() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "one game.pgn", multi_pgn(1).as_bytes());
    let mut h = Harness::new();
    let eff = h.send(Command::Open);
    assert!(matches!(
        eff.as_slice(),
        [Effect::PickFile(PickSpec {
            kind: PickKind::OpenPgn,
            ..
        })]
    ));
    h.pick(Some(&path));
    assert_eq!(h.plies(), 3);
    assert_eq!(
        h.state.origin,
        GameOrigin::PgnFile {
            path: canonical(&path),
            index: 0,
            total: 1,
            partial: false,
        }
    );
    assert!(!h.state.is_dirty());
    assert_eq!(
        h.state.prefs.files.recent.iter().next(),
        Some(&canonical(&path))
    );
    assert_eq!(
        h.state.prefs.files.last_open_dir.as_deref(),
        Some(canonical(&path).parent().unwrap())
    );
    assert!(h.state.prefs_dirty);
}

#[test]
fn rule24_three_games_open_the_chooser_and_open_loads_the_chosen_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "three.pgn", multi_pgn(3).as_bytes());
    let mut h = Harness::new();
    open_in_harness(&mut h, &path);
    let chooser = h.state.chooser.as_ref().expect("chooser");
    assert_eq!(chooser.collection.len(), 3);
    assert_eq!(h.plies(), 0, "no game is loaded before choosing");
    let collection = chooser.collection.clone();
    h.send(mychess_app::ChooserMessage::Open(1));
    assert!(h.state.chooser.is_none());
    assert_eq!(h.tags().white, "P1");
    assert_eq!(
        h.state.origin,
        GameOrigin::PgnFile {
            path: canonical(&path),
            index: 1,
            total: 3,
            partial: false
        }
    );
    let parsed = collection.parse_game(1, &ReadOptions::default()).unwrap();
    assert!(h.state.session.game().same_content(&parsed.game));
}

#[test]
fn rule24_extension_is_not_required_and_case_is_irrelevant() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["games.PGN", "games.dat", "games", "GAMES.Pgn"] {
        let path = write_file(dir.path(), name, multi_pgn(2).as_bytes());
        assert!(
            matches!(scan_file(&path).unwrap(), PgnLoad::Many(_)),
            "{name}"
        );
    }
}

// ------------------------------------------------------------------ rule 25

#[rstest]
#[case::utf8("utf8")]
#[case::utf8_bom("bom")]
#[case::latin1("latin1")]
#[case::crlf("crlf")]
#[case::cr("cr")]
#[case::lf("lf")]
#[case::gzip("gzip")]
#[case::gzip_renamed("gzip-as-pgn")]
fn rule25_encodings_and_line_endings_give_the_same_tags(#[case] kind: &str) {
    let dir = tempfile::tempdir().unwrap();
    #[allow(clippy::match_same_arms)] // each case is spelled out on purpose
    let (name, bytes): (&str, Vec<u8>) = match kind {
        "utf8" => ("a.pgn", ACCENTED.as_bytes().to_vec()),
        "bom" => (
            "a.pgn",
            [&[0xef, 0xbb, 0xbf][..], ACCENTED.as_bytes()].concat(),
        ),
        "latin1" => ("a.pgn", latin1(ACCENTED)),
        "crlf" => ("a.pgn", ACCENTED.replace('\n', "\r\n").into_bytes()),
        "cr" => ("a.pgn", ACCENTED.replace('\n', "\r").into_bytes()),
        "lf" => ("a.pgn", ACCENTED.as_bytes().to_vec()),
        "gzip" => ("a.pgn.gz", gzip(ACCENTED.as_bytes())),
        "gzip-as-pgn" => ("a.pgn", gzip(ACCENTED.as_bytes())),
        _ => unreachable!(),
    };
    let path = write_file(dir.path(), name, &bytes);
    let game = loaded(&path).game;
    let tags = game.tags();
    assert_eq!(tags.white, "Jos\u{e9}", "{kind}");
    assert_eq!(tags.black, "M\u{fc}ller", "{kind}");
    assert_eq!(tags.event, "Torneio S\u{e3}o Paulo", "{kind}");
    assert_eq!(tags.result, GameResult::White);
    assert_eq!(game.mainline().count(), 3, "{kind}");
}

#[test]
fn rule25_gzip_with_a_non_gz_extension_and_a_renamed_gz_open_the_same() {
    let dir = tempfile::tempdir().unwrap();
    let data = gzip(multi_pgn(4).as_bytes());
    for name in ["x.pgn.gz", "x.gz", "x.bin", "x.PGN.GZ"] {
        let path = write_file(dir.path(), name, &data);
        match scan_file(&path).unwrap() {
            PgnLoad::Many(c) => assert_eq!(c.len(), 4, "{name}"),
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[test]
fn rule25_windows_1252_punctuation_is_decoded_not_mangled() {
    // 0x92 is the right single quote in Windows-1252 (a C1 control in Latin-1).
    let mut bytes = b"[Event \"Tournament\"]\n[Site \"Paris\"]\n[Date \"2024.01.02\"]\n[Round \"1\"]\n[White \"O".to_vec();
    bytes.push(0x92);
    bytes.extend_from_slice(b"Brien, Jos");
    bytes.push(0xe9);
    bytes.extend_from_slice(b"\"]\n[Black \"Fran");
    bytes.push(0xe7);
    bytes.extend_from_slice(b"ois\"]\n[Result \"*\"]\n\n1. e4 e5 *\n");
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "cp1252.pgn", &bytes);
    let game = loaded(&path).game;
    assert_eq!(game.tags().white, "O\u{2019}Brien, Jos\u{e9}");
    assert_eq!(game.tags().black, "Fran\u{e7}ois");
}

#[test]
fn rule25_collection_reports_the_detected_encoding() {
    let dir = tempfile::tempdir().unwrap();
    let text = multi_pgn(2).replace("P0", "Jos\u{e9}");
    let path = write_file(dir.path(), "l1.pgn", &latin1(&text));
    match scan_file(&path).unwrap() {
        PgnLoad::Many(c) => {
            assert!(!c.encoding().is_empty());
            assert_eq!(c.header(0).unwrap().white, "Jos\u{e9}");
        }
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------ rule 26

fn kind_of(result: Result<PgnLoad, FileError>) -> FileErrorKind {
    result.expect_err("an error").kind
}

#[rstest]
#[case::empty(b"".to_vec())]
#[case::plain_text(b"just some text without any header\n".to_vec())]
#[case::only_newlines(b"\n\n\n".to_vec())]
fn rule26_files_without_games_give_no_games_found(#[case] bytes: Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "x.pgn", &bytes);
    assert!(matches!(
        kind_of(scan_file(&path)),
        FileErrorKind::NoGamesFound { .. }
    ));
}

#[test]
fn rule26_no_games_found_reports_the_skipped_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let junk = b"nothing here, only junk text\n";
    let path = write_file(dir.path(), "x.pgn", junk);
    match kind_of(scan_file(&path)) {
        FileErrorKind::NoGamesFound { skipped_bytes } => assert!(skipped_bytes > 0),
        other => panic!("{other:?}"),
    }
}

#[test]
fn rule26_missing_file_and_directory_are_structured_errors() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.pgn");
    assert_eq!(kind_of(scan_file(&missing)), FileErrorKind::NotFound);
    // A directory: the exact kind is platform dependent, but it is an error.
    let err = scan_file(dir.path()).expect_err("directory");
    assert!(
        matches!(
            err.kind,
            FileErrorKind::Io(_) | FileErrorKind::NotFound | FileErrorKind::PermissionDenied
        ),
        "{err:?}"
    );
    assert_eq!(err.path.as_deref(), Some(dir.path()));
}

#[test]
fn rule26_utf16_is_refused_with_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = vec![0xff, 0xfe];
    for u in multi_pgn(1).encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    let path = write_file(dir.path(), "u16.pgn", &bytes);
    assert!(matches!(
        kind_of(scan_file(&path)),
        FileErrorKind::UnsupportedEncoding(_)
    ));
    let mut be = vec![0xfe, 0xff];
    for u in multi_pgn(1).encode_utf16() {
        be.extend_from_slice(&u.to_be_bytes());
    }
    let path = write_file(dir.path(), "u16be.pgn", &be);
    assert!(matches!(
        kind_of(scan_file(&path)),
        FileErrorKind::UnsupportedEncoding(_)
    ));
}

#[test]
fn rule26_truncated_gzip_is_corrupt_and_offers_no_game() {
    let dir = tempfile::tempdir().unwrap();
    let full = gzip(multi_pgn(30).as_bytes());
    let path = write_file(dir.path(), "cut.pgn.gz", &full[..full.len() / 2]);
    assert_eq!(kind_of(scan_file(&path)), FileErrorKind::CorruptGzip);
    // Header only.
    let path = write_file(dir.path(), "hdr.pgn.gz", &full[..10]);
    assert!(scan_file(&path).is_err());
}

#[test]
fn rule26_failed_open_leaves_the_game_and_removes_the_recent_entry() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone.pgn");
    let mut h = Harness::new();
    h.play("e2e4");
    h.state.prefs.files.recent.push(missing.clone());
    let game = h.state.session.game().clone();
    open_in_harness(&mut h, &missing);
    assert!(h.state.session.game().same_content(&game));
    assert!(h.state.discard_prompt.is_none());
    assert!(matches!(
        h.state.notice,
        Some(AppNotice::File(FileError {
            kind: FileErrorKind::NotFound,
            ..
        }))
    ));
    assert!(h.state.prefs.files.recent.iter().all(|p| p != &missing));
}

#[test]
fn rule26_directory_chosen_as_file_is_a_notice_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    open_in_harness(&mut h, dir.path());
    assert!(matches!(h.state.notice, Some(AppNotice::File(_))));
    assert_eq!(h.plies(), 0);
}

// ------------------------------------------------------------------ rule 27

#[test]
fn rule27_game_limit_stops_reading_and_marks_the_collection() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "ten.pgn", multi_pgn(10).as_bytes());
    let limits = ScanLimits {
        max_bytes: 64 << 20,
        max_games: 4,
    };
    let (load, notices) =
        scan_pgn(PgnSource::File(path), &limits, &ReadOptions::default()).unwrap();
    let PgnLoad::Many(c) = load else {
        panic!("{load:?}")
    };
    assert_eq!(c.len(), 4);
    let (games_read, limit) = c.truncated().expect("truncated");
    assert_eq!(games_read, 4);
    assert_eq!(limit, ScanLimit::Games(4));
    assert!(
        notices.iter().any(|n| matches!(
            n.issue,
            mychess_app::ImportIssue::Truncated {
                limit: ScanLimit::Games(4),
                ..
            }
        )),
        "{notices:?}"
    );
}

#[test]
fn rule27_byte_limit_stops_reading_and_memory_stays_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let text = multi_pgn(200);
    let path = write_file(dir.path(), "big.pgn", text.as_bytes());
    let limit = 2_000u64;
    let limits = ScanLimits {
        max_bytes: limit,
        max_games: 50_000,
    };
    let (load, _) = scan_pgn(PgnSource::File(path), &limits, &ReadOptions::default()).unwrap();
    let PgnLoad::Many(c) = load else {
        panic!("{load:?}")
    };
    assert!(c.len() < 200 && !c.is_empty());
    let kept: usize = (0..c.len()).map(|i| c.raw_bytes(i).unwrap().len()).sum();
    // Allow one chunk of slack, but nowhere near the file size.
    assert!(
        kept as u64 <= limit + 512,
        "kept {kept} bytes for a {limit} byte limit"
    );
    assert!(matches!(c.truncated(), Some((_, ScanLimit::Bytes(2_000)))));
}

#[test]
fn rule27_default_limits_are_64_mib_and_50_000_games() {
    let l = ScanLimits::default();
    assert_eq!(l.max_bytes, 64 << 20);
    assert_eq!(l.max_games, 50_000);
}

#[test]
fn rule27_gzip_bomb_is_bounded_by_the_decompressed_limit() {
    // 3 MiB of PGN comments compressed as stored blocks, limit 1 MiB.
    let dir = tempfile::tempdir().unwrap();
    let mut text = String::new();
    while text.len() < 3 << 20 {
        text.push_str(&multi_pgn(50));
    }
    let path = write_file(dir.path(), "bomb.pgn.gz", &gzip(text.as_bytes()));
    let limits = ScanLimits {
        max_bytes: 1 << 20,
        max_games: 50_000,
    };
    let (load, _) = scan_pgn(PgnSource::File(path), &limits, &ReadOptions::default()).unwrap();
    let PgnLoad::Many(c) = load else {
        panic!("{load:?}")
    };
    let kept: usize = (0..c.len()).map(|i| c.raw_bytes(i).unwrap().len()).sum();
    assert!(kept < 1 << 21, "kept {kept}");
    assert!(c.truncated().is_some());
}

// ------------------------------------------------------------------ rule 28

fn tags(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn rule28_header_reading_is_lenient() {
    let h = GameHeader::from_scanned(&tags(&[
        ("Result", "1/2"),
        ("Date", "2024.??.??"),
        ("WhiteElo", "abc"),
        ("ECO", "zz"),
    ]));
    assert_eq!(h.result, GameResult::Draw);
    assert_eq!(h.date.year(), Some(2024));
    assert_eq!(h.date.month(), None);
    assert_eq!(h.white_elo, None);
    assert_eq!(h.eco, None);
    assert_eq!(h.white, "?");
    assert_eq!(h.event, "?");
    assert_eq!(h.round, "?");
}

#[test]
fn rule28_empty_tag_list_gives_all_unknown() {
    let h = GameHeader::from_scanned(&[]);
    assert_eq!(h.white, "?");
    assert_eq!(h.black, "?");
    assert_eq!(h.site, "?");
    assert_eq!(h.result, GameResult::Unknown);
    assert!(h.date.is_unknown());
    assert_eq!((h.white_elo, h.black_elo, h.eco), (None, None, None));
}

#[test]
fn rule28_elo_out_of_range_is_dropped_not_fatal() {
    for bad in ["99999", "-5", "4001", "", "  ", "12.5", "\u{0}"] {
        let h = GameHeader::from_scanned(&tags(&[("WhiteElo", bad), ("BlackElo", "2850")]));
        assert_eq!(h.white_elo, None, "{bad:?}");
        assert_eq!(h.black_elo, Some(2850));
    }
}

proptest! {
    #[test]
    fn rule66_from_scanned_never_panics(
        pairs in proptest::collection::vec(("\\PC{0,12}", "\\PC{0,30}"), 0..12)
    ) {
        let _ = GameHeader::from_scanned(&pairs);
    }
}

// ------------------------------------------------------------------ rule 29 / D126

#[test]
fn rule29_navigation_in_the_collection_and_no_wrap_at_the_ends() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "three.pgn", multi_pgn(3).as_bytes());
    let mut h = Harness::new();
    open_in_harness(&mut h, &path);
    h.send(mychess_app::ChooserMessage::Open(0));
    assert_eq!(h.tags().white, "P0");

    // Previous at the first game: no effect at all.
    assert!(h.raw(Command::PrevGame).is_empty());
    assert!(h.raw(Command::FirstGame).is_empty());
    assert_eq!(h.tags().white, "P0");

    h.send(Command::NextGame);
    assert_eq!(h.tags().white, "P1");
    h.send(Command::LastGame);
    assert_eq!(h.tags().white, "P2");
    assert!(matches!(
        h.state.origin,
        GameOrigin::PgnFile {
            index: 2,
            total: 3,
            ..
        }
    ));
    assert!(
        h.raw(Command::NextGame).is_empty(),
        "no rotation at the last game"
    );
    assert!(h.raw(Command::LastGame).is_empty());
    h.send(Command::PrevGame);
    assert_eq!(h.tags().white, "P1");
    h.send(Command::FirstGame);
    assert_eq!(h.tags().white, "P0");
    assert!(h.state.notice.is_none());
}

#[test]
fn rule29_navigation_without_a_collection_does_nothing() {
    let mut h = Harness::new();
    for c in [
        Command::NextGame,
        Command::PrevGame,
        Command::FirstGame,
        Command::LastGame,
    ] {
        assert!(h.raw(c).is_empty());
    }
    assert!(h.state.notice.is_none());
}

#[test]
fn d126_navigating_a_dirty_game_asks_before_replacing() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "three.pgn", multi_pgn(3).as_bytes());
    let mut h = Harness::new();
    open_in_harness(&mut h, &path);
    h.send(mychess_app::ChooserMessage::Open(0));
    h.send(GameMessage::Navigate(mychess_app::Nav::GameEnd));
    h.play("b8c6"); // an extra move on game P0 -> dirty
    h.send(Command::NextGame);
    assert!(h.state.discard_prompt.is_some());
    assert_eq!(h.tags().white, "P0");
    h.send(Message::AnswerDiscard(mychess_app::DiscardChoice::Cancel));
    assert_eq!(h.tags().white, "P0");
    h.send(Command::NextGame);
    h.send(Message::AnswerDiscard(mychess_app::DiscardChoice::DontSave));
    assert_eq!(h.tags().white, "P1");
}

// ------------------------------------------------------------------ rule 30

fn rich_game() -> Game {
    let mut game = game_from_ucis(&["e2e4", "e7e5", "g1f3", "b8c6"]);
    let root = game.root();
    let e4 = game.children(root).unwrap()[0];
    let e5 = game.children(e4).unwrap()[0];
    game.set_comment(e4, "King's pawn and \u{e9}").unwrap();
    game.add_nag(e5, mychess_core::Nag::new(1).unwrap())
        .unwrap();
    let mv = game.position_at(e4).unwrap().parse_uci("c7c5").unwrap();
    let c5 = game.add_move(e4, mv).unwrap();
    game.set_comment(c5, "Sicilian").unwrap();
    let mv = game.position_at(c5).unwrap().parse_uci("g1f3").unwrap();
    game.add_move(c5, mv).unwrap();
    let t = game.tags_mut();
    t.white = "Zé, A.".to_owned();
    t.black = "Ünal".to_owned();
    t.event = "Cup".to_owned();
    t.result = GameResult::Draw;
    t.set_extra("Annotator", "Anand").unwrap();
    game
}

#[test]
fn rule30_save_as_round_trips_the_whole_game() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.state.session.replace_game(rich_game());
    h.send(GameMessage::Navigate(mychess_app::Nav::GameEnd));
    h.play("a2a3"); // dirty
    let game = h.state.session.game().clone();
    let eff = h.send(Command::SaveAs);
    let [Effect::PickFile(spec)] = eff.as_slice() else {
        panic!("{eff:?}")
    };
    assert_eq!(spec.kind, PickKind::SavePgn);
    assert_eq!(
        spec.file_name.as_deref(),
        Some("Z\u{e9}, A. - \u{dc}nal.pgn")
    );
    // The dialog returns a name without extension: `.pgn` is added.
    let chosen = dir.path().join("my game");
    h.pick(Some(&chosen));
    let target = dir.path().join("my game.pgn");
    assert!(target.is_file(), "{:?}", dir_names(dir.path()));
    let bytes = std::fs::read(&target).unwrap();
    let parsed = parse_pgn_text(
        std::str::from_utf8(&bytes).unwrap(),
        &ReadOptions::default(),
    );
    assert_eq!(parsed.len(), 1);
    assert!(parsed[0].game.same_content(&game));
    assert!(!h.state.is_dirty());
    assert_eq!(
        h.state.origin,
        GameOrigin::SavedFile {
            path: canonical(&target)
        }
    );
    assert_eq!(
        h.state.prefs.files.recent.iter().next(),
        Some(&canonical(&target))
    );
    assert!(h.state.prefs.files.last_save_dir.is_some());
    assert_eq!(
        dir_names(dir.path()),
        vec!["my game.pgn"],
        "no temporary file left"
    );
}

#[test]
fn rule30_game_from_a_set_up_position_keeps_its_fen() {
    let dir = tempfile::tempdir().unwrap();
    let pos = fen("r3k2r/8/8/8/8/8/8/R3K2R b Kq - 12 30");
    let mut game = Game::from_position(pos);
    let mv = game
        .position_at(game.root())
        .unwrap()
        .parse_uci("e8c8")
        .unwrap();
    game.add_move(game.root(), mv).ok();
    let path = dir.path().join("fen.pgn");
    save_pgn(&path, SaveMode::Overwrite, &game, &Default::default()).unwrap();
    let back = loaded(&path).game;
    assert!(back.same_content(&game));
    assert_eq!(back.start_position().fen(), game.start_position().fen());
}

#[test]
fn rule30_save_writes_utf8_without_bom_and_lf_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("o.pgn");
    save_pgn(
        &path,
        SaveMode::Overwrite,
        &rich_game(),
        &Default::default(),
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]), "no BOM by default");
    assert!(!bytes.contains(&b'\r'), "LF on every platform");
    assert!(std::str::from_utf8(&bytes).is_ok());
    assert!(bytes.ends_with(b"\n"));
}

#[test]
fn d129_encoding_preferences_utf8_bom_and_latin1() {
    let dir = tempfile::tempdir().unwrap();
    let game = rich_game();
    let mut prefs = mychess_app::PgnExportPrefs::default();
    prefs.encoding = EncodingPref::Utf8Bom;
    let p1 = dir.path().join("bom.pgn");
    save_pgn(&p1, SaveMode::Overwrite, &game, &prefs).unwrap();
    assert!(std::fs::read(&p1).unwrap().starts_with(&[0xef, 0xbb, 0xbf]));

    prefs.encoding = EncodingPref::Latin1;
    let p2 = dir.path().join("l1.pgn");
    let mut g = game.clone();
    g.tags_mut().black = "Ünal \u{265f}".to_owned();
    save_pgn(&p2, SaveMode::Overwrite, &g, &prefs).unwrap();
    let bytes = std::fs::read(&p2).unwrap();
    assert!(
        std::str::from_utf8(&bytes).is_err(),
        "Latin-1 bytes are not UTF-8"
    );
    assert!(bytes.windows(4).any(|w| w == b"\xdcnal"));
    assert!(bytes.windows(7).any(|w| w == b"\xdcnal ?\"") || bytes.contains(&b'?'));
}

// ------------------------------------------------------------------ rule 31

#[test]
fn rule31_append_keeps_old_bytes_and_adds_exactly_one_blank_line() {
    let dir = tempfile::tempdir().unwrap();
    let game = game_from_ucis(&["d2d4", "d7d5"]);
    for old in [
        "[Event \"A\"]\n\n1. e4 *\n".to_owned(),
        "[Event \"A\"]\n\n1. e4 *".to_owned(),
        "[Event \"A\"]\n\n1. e4 *\n\n".to_owned(),
    ] {
        let path = write_file(dir.path(), "a.pgn", old.as_bytes());
        save_pgn(&path, SaveMode::Append, &game, &Default::default()).unwrap();
        let after = std::fs::read(&path).unwrap();
        assert!(
            after.starts_with(old.as_bytes()),
            "old bytes unchanged for {old:?}"
        );
        let tail = &after[old.len()..];
        let added = std::str::from_utf8(tail).unwrap();
        let sep_len = if old.ends_with("\n\n") {
            0
        } else if old.ends_with('\n') {
            1
        } else {
            2
        };
        assert!(
            !added[sep_len..].starts_with('\n'),
            "no extra blank line in {added:?}"
        );
        assert!(added[sep_len..].starts_with("[Event"), "{added:?}");
        let text = String::from_utf8(after).unwrap();
        // Exactly one blank line between the games.
        assert_eq!(text.matches("\n\n\n").count(), 0, "{text:?}");
        let parsed = parse_pgn_text(&text, &ReadOptions::default());
        assert_eq!(parsed.len(), 2);
        assert!(parsed[1].game.same_content(&game));
    }
}

#[test]
fn rule31_append_creates_a_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.pgn");
    let game = game_from_ucis(&["e2e4"]);
    save_pgn(&path, SaveMode::Append, &game, &Default::default()).unwrap();
    assert!(loaded(&path).game.same_content(&game));
    assert_eq!(dir_names(dir.path()), vec!["new.pgn"]);
}

#[test]
fn rule31_append_to_an_empty_file_behaves_like_a_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "empty.pgn", b"");
    let game = game_from_ucis(&["e2e4"]);
    save_pgn(&path, SaveMode::Append, &game, &Default::default()).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.starts_with('\n'), "no leading blank lines: {text:?}");
    assert_eq!(parse_pgn_text(&text, &ReadOptions::default()).len(), 1);
}

#[test]
fn rule31_append_does_not_repeat_the_bom() {
    let dir = tempfile::tempdir().unwrap();
    let old = [&[0xef, 0xbb, 0xbf][..], b"[Event \"A\"]\n\n1. e4 *\n"].concat();
    let path = write_file(dir.path(), "bom.pgn", &old);
    let mut prefs = mychess_app::PgnExportPrefs::default();
    prefs.encoding = EncodingPref::Utf8Bom; // even if the preference asks for one
    save_pgn(&path, SaveMode::Append, &game_from_ucis(&["d2d4"]), &prefs).unwrap();
    let after = std::fs::read(&path).unwrap();
    let boms = after
        .windows(3)
        .filter(|w| *w == [0xef, 0xbb, 0xbf])
        .count();
    assert_eq!(boms, 1);
    assert!(after.starts_with(&old));
}

#[test]
fn rule31_crlf_file_is_kept_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let old = b"[Event \"A\"]\r\n\r\n1. e4 *\r\n".to_vec();
    let path = write_file(dir.path(), "crlf.pgn", &old);
    save_pgn(
        &path,
        SaveMode::Append,
        &game_from_ucis(&["d2d4"]),
        &Default::default(),
    )
    .unwrap();
    let after = std::fs::read(&path).unwrap();
    assert!(after.starts_with(&old));
    let parsed = parse_pgn_text(&String::from_utf8(after).unwrap(), &ReadOptions::default());
    assert_eq!(parsed.len(), 2);
}

// ------------------------------------------------------------------ rule 32

#[test]
fn rule32_appending_unrepresentable_text_to_latin1_is_refused_and_the_file_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let old = latin1("[Event \"S\u{e3}o\"]\n\n1. e4 *\n");
    let path = write_file(dir.path(), "l1.pgn", &old);
    let mut game = game_from_ucis(&["d2d4"]);
    game.tags_mut().white = "\u{265f}".to_owned();
    let err = save_pgn(&path, SaveMode::Append, &game, &Default::default()).unwrap_err();
    assert_eq!(err.kind, FileErrorKind::EncodingLossy);
    assert_eq!(std::fs::read(&path).unwrap(), old);
    assert_eq!(dir_names(dir.path()), vec!["l1.pgn"]);
}

#[test]
fn rule32_utf8_file_gets_utf8_and_latin1_file_gets_latin1() {
    let dir = tempfile::tempdir().unwrap();
    let mut game = game_from_ucis(&["d2d4"]);
    game.tags_mut().white = "Jos\u{e9}".to_owned();

    let utf8 = write_file(
        dir.path(),
        "u.pgn",
        "[Event \"S\u{e3}o\"]\n\n1. e4 *\n".as_bytes(),
    );
    save_pgn(&utf8, SaveMode::Append, &game, &Default::default()).unwrap();
    let text = std::fs::read(&utf8).unwrap();
    assert!(std::str::from_utf8(&text).is_ok());
    assert!(
        !text.starts_with(&[0xef, 0xbb, 0xbf]),
        "UTF-8 without BOM stays without BOM"
    );
    assert!(text.windows(5).any(|w| w == "Jos\u{e9}".as_bytes()));

    let l1 = write_file(
        dir.path(),
        "l.pgn",
        &latin1("[Event \"S\u{e3}o\"]\n\n1. e4 *\n"),
    );
    save_pgn(&l1, SaveMode::Append, &game, &Default::default()).unwrap();
    let bytes = std::fs::read(&l1).unwrap();
    assert!(std::str::from_utf8(&bytes).is_err());
    assert!(bytes.windows(4).any(|w| w == b"Jos\xe9"));
}

#[test]
fn rule32_append_refuses_utf16_and_gzip_targets() {
    let dir = tempfile::tempdir().unwrap();
    let game = game_from_ucis(&["d2d4"]);
    let u16 = write_file(dir.path(), "u16.pgn", &[0xff, 0xfe, b'[', 0]);
    let err = save_pgn(&u16, SaveMode::Append, &game, &Default::default()).unwrap_err();
    assert!(
        matches!(err.kind, FileErrorKind::UnsupportedEncoding(_)),
        "{err:?}"
    );
    assert_eq!(std::fs::read(&u16).unwrap(), [0xff, 0xfe, b'[', 0]);
    let gz = write_file(dir.path(), "x.pgn", &gzip(b"[Event \"x\"]\n\n*\n"));
    let before = std::fs::read(&gz).unwrap();
    let err = save_pgn(&gz, SaveMode::Append, &game, &Default::default());
    assert!(err.is_err());
    assert_eq!(
        std::fs::read(&gz).unwrap(),
        before,
        "a gzip file is never corrupted by appending text"
    );
}

// ------------------------------------------------------------------ rule 33

#[test]
fn rule33_missing_directory_fails_with_not_found_and_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("no such dir").join("x.pgn");
    let err = save_pgn(
        &path,
        SaveMode::Overwrite,
        &rich_game(),
        &Default::default(),
    )
    .unwrap_err();
    assert_eq!(err.kind, FileErrorKind::NotFound);
    assert_eq!(err.path.as_deref(), Some(path.as_path()));
    assert!(dir_names(dir.path()).is_empty());
}

#[test]
fn rule33_destination_that_is_a_directory_keeps_it_and_leaves_no_temporary() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("x.pgn");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("inner.txt"), b"keep").unwrap();
    for mode in [SaveMode::Overwrite, SaveMode::Append] {
        let result = save_pgn(&target, mode, &rich_game(), &Default::default());
        assert!(result.is_err(), "{mode:?}");
        assert!(target.is_dir());
        assert_eq!(std::fs::read(target.join("inner.txt")).unwrap(), b"keep");
        assert_eq!(
            dir_names(dir.path()),
            vec!["x.pgn"],
            "{mode:?}: no temporary left"
        );
    }
}

#[cfg(unix)]
#[test]
fn rule33_read_only_directory_fails_and_keeps_the_original() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "x.pgn", b"original\n");
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root ignores directory permissions: skip when the directory is still writable.
    let writable = std::fs::write(dir.path().join("probe"), b"x").is_ok();
    let result = save_pgn(
        &path,
        SaveMode::Overwrite,
        &rich_game(),
        &Default::default(),
    );
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        return;
    }
    assert_eq!(result.unwrap_err().kind, FileErrorKind::PermissionDenied);
    assert_eq!(std::fs::read(&path).unwrap(), b"original\n");
    assert_eq!(dir_names(dir.path()), vec!["x.pgn"]);
}

#[test]
fn rule33_failed_save_in_the_app_keeps_dirty_and_origin_and_reports_a_notice() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SaveAs);
    h.pick(Some(&dir.path().join("missing dir").join("x.pgn")));
    assert!(h.state.is_dirty());
    assert_eq!(h.state.origin, GameOrigin::New);
    assert!(matches!(h.state.notice, Some(AppNotice::File(_))));
    assert_eq!(
        h.state.prefs.files.recent.len(),
        0,
        "no recent entry for a failed save"
    );
}

#[cfg(unix)]
#[test]
fn d128_permissions_of_the_replaced_file_are_kept_and_new_files_are_0644() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "x.pgn", b"[Event \"x\"]\n\n*\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    save_pgn(
        &path,
        SaveMode::Overwrite,
        &rich_game(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    save_pgn(&path, SaveMode::Append, &rich_game(), &Default::default()).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    let fresh = dir.path().join("fresh.pgn");
    save_pgn(
        &fresh,
        SaveMode::Overwrite,
        &rich_game(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
        0o644
    );
}

#[test]
fn rule33_overwrite_replaces_the_previous_content_completely() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "x.pgn", &vec![b'x'; 100_000]);
    let game = game_from_ucis(&["e2e4"]);
    save_pgn(&path, SaveMode::Overwrite, &game, &Default::default()).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains('x') || text.contains("[Event"));
    assert!(text.len() < 1000);
    assert_eq!(parse_pgn_text(&text, &ReadOptions::default()).len(), 1);
}

// ------------------------------------------------------------------ rule 34

#[rstest]
#[case("a", "a.pgn")]
#[case("a.PGN", "a.PGN")]
#[case("a.Pgn", "a.Pgn")]
#[case("a.txt", "a.txt")]
#[case("a.TXT", "a.TXT")]
#[case("a.foo", "a.foo.pgn")]
#[case("a.pgn.gz", "a.pgn.gz.pgn")]
#[case("a.gz", "a.gz.pgn")]
fn rule34_ensure_pgn_extension(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(
        ensure_pgn_extension(PathBuf::from(input)),
        PathBuf::from(expected)
    );
}

#[test]
fn rule34_extension_check_looks_at_the_file_name_not_a_directory() {
    let p = Path::new("dir.pgn").join("a");
    assert_eq!(
        ensure_pgn_extension(p.clone()),
        Path::new("dir.pgn").join("a.pgn")
    );
}

fn named(white: &str, black: &str) -> Tags {
    let mut t = Tags::default();
    t.white = white.to_owned();
    t.black = black.to_owned();
    t
}

#[rstest]
#[case("Kasparov, G.", "Karpov, A.", "Kasparov, G. - Karpov, A.pgn")]
#[case("a:b", "c", "a_b - c.pgn")]
#[case("a/b", "c\\d", "a_b - c_d.pgn")]
#[case("?", "?", "game.pgn")]
#[case("", "", "game.pgn")]
#[case("  ", "x", "game.pgn")]
#[case("What?", "Who*", "What_ - Who_.pgn")]
#[case("a<b>", "c|d\"e", "a_b_ - c_d_e.pgn")]
fn rule34_suggest_file_name(#[case] w: &str, #[case] b: &str, #[case] expected: &str) {
    assert_eq!(suggest_file_name(&named(w, b)), expected);
}

#[test]
fn rule34_reserved_windows_names_are_prefixed() {
    for name in ["con", "CON", "Prn", "aux", "NUL", "com1", "LPT9", "con.txt"] {
        let suggested = mychess_app::sanitize_file_name(name);
        assert!(suggested.starts_with('_'), "{name} -> {suggested}");
    }
    // Not reserved: similar names.
    for name in ["console", "com10", "lpt0", "nul x"] {
        assert!(
            !mychess_app::sanitize_file_name(name).starts_with('_'),
            "{name}"
        );
    }
}

#[test]
fn rule34_long_names_are_cut_to_100_chars_before_the_extension() {
    let long = "x".repeat(300);
    let name = suggest_file_name(&named(&long, &long));
    let stem = name.strip_suffix(".pgn").unwrap();
    assert!(stem.chars().count() <= 100, "{}", stem.chars().count());
    assert!(!stem.is_empty());
    let accents = "\u{e9}".repeat(300);
    let name = suggest_file_name(&named(&accents, "b"));
    assert!(name.strip_suffix(".pgn").unwrap().chars().count() <= 100);
}

proptest! {
    #[test]
    fn rule34_suggested_names_are_always_safe(w in "\\PC{0,150}", b in "\\PC{0,150}") {
        let name = suggest_file_name(&named(&w, &b));
        let stem = name.strip_suffix(".pgn").expect("always ends with .pgn");
        prop_assert!(!stem.is_empty());
        prop_assert!(stem.chars().count() <= 100);
        prop_assert!(!stem.ends_with(' ') && !stem.ends_with('.'));
        prop_assert!(!stem.starts_with(' '));
        prop_assert!(!stem.chars().any(|c| "<>:\"/\\|?*".contains(c) || c.is_control()), "{name:?}");
        let upper = stem.split('.').next().unwrap().to_ascii_uppercase();
        let reserved = ["CON", "PRN", "AUX", "NUL"];
        prop_assert!(!reserved.contains(&upper.as_str()));
        prop_assert!(!(upper.len() == 4 && (upper.starts_with("COM") || upper.starts_with("LPT")) && upper.ends_with(|c: char| ('1'..='9').contains(&c))));
    }
}

// ------------------------------------------------------------------ rule 35

#[test]
fn rule35_push_moves_to_front_without_duplicates() {
    let mut r = RecentFiles::default();
    for n in ["a", "b", "c"] {
        r.push(PathBuf::from(n));
    }
    r.push(PathBuf::from("a"));
    let v: Vec<_> = r.iter().cloned().collect();
    assert_eq!(v, ["a", "c", "b"].map(PathBuf::from));
    r.push(PathBuf::from("a"));
    assert_eq!(r.len(), 3, "stable");
}

#[test]
fn rule35_eleventh_push_drops_the_oldest() {
    let mut r = RecentFiles::default();
    for i in 0..11 {
        r.push(PathBuf::from(format!("f{i}")));
    }
    assert_eq!(r.len(), MAX_RECENT_FILES);
    assert_eq!(MAX_RECENT_FILES, 10);
    assert!(r.iter().all(|p| p != Path::new("f0")));
    assert_eq!(r.get(0), Some(&PathBuf::from("f10")));
    assert_eq!(r.get(9), Some(&PathBuf::from("f1")));
}

#[test]
fn rule35_remove_takes_the_entry_out_and_ignores_unknown_ones() {
    let mut r = RecentFiles::default();
    r.push(PathBuf::from("a"));
    r.push(PathBuf::from("b"));
    r.remove(Path::new("a"));
    r.remove(Path::new("zzz"));
    assert_eq!(r.iter().collect::<Vec<_>>(), [&PathBuf::from("b")]);
    assert!(!r.is_empty());
}

#[test]
fn d131_recent_files_after_open_save_and_append_are_updated_with_canonical_paths() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_file(dir.path(), "a.pgn", multi_pgn(1).as_bytes());
    let mut h = Harness::new();
    open_in_harness(&mut h, &a);
    // Same file via a different spelling: no duplicate.
    let sneaky = dir.path().join(".").join("a.pgn");
    open_in_harness(&mut h, &sneaky);
    assert_eq!(h.state.prefs.files.recent.len(), 1);

    h.play("a2a3");
    h.send(Command::Append);
    let b = dir.path().join("b.pgn");
    h.pick(Some(&b));
    assert_eq!(
        h.state.prefs.files.recent.iter().next(),
        Some(&canonical(&b))
    );
    assert_eq!(h.state.prefs.files.recent.len(), 2);
}

// ------------------------------------------------------------------ D127 flows

#[test]
fn d127_save_on_a_saved_game_rewrites_the_same_file_without_asking() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SaveAs);
    h.pick(Some(&dir.path().join("g.pgn")));
    h.play("e7e5");
    assert!(h.state.is_dirty());
    let eff = h.send(Command::Save);
    assert!(eff.is_empty(), "no dialog: {eff:?}");
    assert!(!h.state.is_dirty());
    let back = loaded(&dir.path().join("g.pgn")).game;
    assert_eq!(back.mainline().count(), 2);
}

#[test]
fn d127_save_of_a_new_game_or_a_collection_game_asks_for_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    let eff = h.send(Command::Save);
    assert!(matches!(
        eff.as_slice(),
        [Effect::PickFile(PickSpec {
            kind: PickKind::SavePgn,
            ..
        })]
    ));

    let path = write_file(dir.path(), "three.pgn", multi_pgn(3).as_bytes());
    let before = std::fs::read(&path).unwrap();
    open_in_harness(&mut h, &path);
    h.send(mychess_app::ChooserMessage::Open(1));
    let eff = h.send(Command::Save);
    assert!(
        matches!(
            eff.as_slice(),
            [Effect::PickFile(PickSpec {
                kind: PickKind::SavePgn,
                ..
            })]
        ),
        "a game of a multi-game file is never replaced in place: {eff:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn d127_cancelled_dialog_does_nothing() {
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SaveAs);
    let eff = h.pick(None);
    assert!(eff.is_empty());
    assert!(h.state.is_dirty());
}

#[test]
fn d127_save_as_over_the_open_multi_game_file_asks_for_extra_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "three.pgn", multi_pgn(3).as_bytes());
    let before = std::fs::read(&path).unwrap();
    let mut h = Harness::new();
    open_in_harness(&mut h, &path);
    h.send(mychess_app::ChooserMessage::Open(0));
    h.send(Command::SaveAs);
    let eff = h.pick(Some(&canonical(&path)));
    assert!(eff.is_empty());
    let prompt = h.state.overwrite_prompt.as_ref().expect("overwrite prompt");
    assert_eq!(prompt.games, 3);
    assert_eq!(std::fs::read(&path).unwrap(), before, "nothing written yet");

    // "No" leaves the file alone.
    h.send(Message::AnswerOverwrite(false));
    assert!(h.state.overwrite_prompt.is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);

    // "Yes" writes just this game.
    h.send(Command::SaveAs);
    h.pick(Some(&canonical(&path)));
    h.send(Message::AnswerOverwrite(true));
    assert_ne!(std::fs::read(&path).unwrap(), before);
    assert!(matches!(loaded(&path).game.tags().white.as_str(), "P0"));
    assert_eq!(
        h.state.origin,
        GameOrigin::SavedFile {
            path: canonical(&path)
        }
    );
}

#[test]
fn d127_save_as_over_a_single_game_file_does_not_ask() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "one.pgn", multi_pgn(1).as_bytes());
    let mut h = Harness::new();
    open_in_harness(&mut h, &path);
    h.send(Command::SaveAs);
    h.pick(Some(&canonical(&path)));
    assert!(h.state.overwrite_prompt.is_none());
}

#[test]
fn d127_append_keeps_dirty_false_and_never_turns_the_file_into_the_save_target() {
    let dir = tempfile::tempdir().unwrap();
    let multi = write_file(dir.path(), "multi.pgn", multi_pgn(3).as_bytes());
    let mut h = Harness::new();
    h.play_all(&["e2e4", "e7e5"]);
    h.send(Command::Append);
    h.pick(Some(&multi));
    assert!(!h.state.is_dirty());
    let text = std::fs::read_to_string(&multi).unwrap();
    assert_eq!(parse_pgn_text(&text, &ReadOptions::default()).len(), 4);
    // A later Save must not clobber the multi-game file with one game.
    let before = std::fs::read(&multi).unwrap();
    h.play("g1f3");
    let eff = h.send(Command::Save);
    if eff.is_empty() {
        // It re-saved somewhere: it must not be the appended file.
        assert_eq!(std::fs::read(&multi).unwrap(), before);
    } else {
        assert!(matches!(eff.as_slice(), [Effect::PickFile(_)]));
    }
}

#[test]
fn d127_append_origin_is_unchanged() {
    // The literal D127 says "origin = SavedFile" for any successful save,
    // but for Append that would make Ctrl+S overwrite a multi-game file.
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::Append);
    h.pick(Some(&dir.path().join("log.pgn")));
    assert_eq!(h.state.origin, GameOrigin::New);
    assert!(!h.state.is_dirty());
}

// ------------------------------------------------------------------ trial mode

#[test]
fn saving_in_trial_mode_writes_the_game_from_before_the_trial() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(GameMessage::ToggleTrial { null_move: false });
    assert!(h.state.session.in_trial());
    h.play("e7e5");
    h.play("g1f3");
    h.send(Command::SaveAs);
    let eff = h.raw(Message::FilePicked(
        match h.host.iter().rev().find_map(|e| match e {
            Effect::PickFile(s) => Some(s.request),
            _ => None,
        }) {
            Some(r) => r,
            None => panic!("no dialog"),
        },
        Some(dir.path().join("t.pgn")),
    ));
    let [Effect::SavePgn { game, .. }] = eff.as_slice() else {
        panic!("{eff:?}")
    };
    assert_eq!(game.mainline().count(), 1, "trial moves are not saved");
}

// ------------------------------------------------------------------ misc hostile

#[test]
fn hostile_files_never_panic() {
    let dir = tempfile::tempdir().unwrap();
    let samples: Vec<Vec<u8>> = vec![
        vec![0; 1000],
        vec![0xff; 1000],
        b"[".repeat(5000),
        b"[Event \"".repeat(1000),
        b"1. e4 ".repeat(20_000),
        [&[0x1f, 0x8b][..], &[0u8; 50][..]].concat(),
        (0u8..=255).cycle().take(5000).collect(),
    ];
    for (i, bytes) in samples.iter().enumerate() {
        let path = write_file(dir.path(), &format!("h{i}.pgn"), bytes);
        let _ = scan_file(&path);
        let mut h = Harness::new();
        open_in_harness(&mut h, &path);
    }
}
