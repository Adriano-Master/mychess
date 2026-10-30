//! Extra contract tests of the files of the tree masks (spec M5-4, slice
//! M5-4b: D1814, D1815, D1816, D1819, D1820, D1861, D1869, D1871, cases 13 to
//! 15): hostile files, recent files, the options and the auto-load.
#![allow(
    clippy::format_push_string, // building the text of a file by hand
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::large_futures, // the rig owns the whole `AppState`
    clippy::cloned_ref_to_slice_refs
)]

mod m5_4b_common;

use std::path::{Path, PathBuf};

use m5_4b_common::*;
use mychess_app::{
    MaskEffect, MaskIssue, MaskPrefs, PickKind, PrefChange, PrefsIssue, PrefsMessage,
    load_preferences, render_preferences,
};
use mychess_db::tree::mask::stm;
use tempfile::TempDir;

/// The example of `docs/formatos/stm.md` 6.A (written by hand, canonical).
const EXAMPLE_A: &str = "set ::tree::mask::maskSerialized {{rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -} {{{null {} white {} {} {}} {e4 ! Green {King's pawn} ::rep::_tb_include {}} {d4 {} white {} {} {}}} {My repertoire}} {rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -} {{{e5 {} white {} {} {}} {c5 ?! Red {Sicilian: see line 2} ::rep::_tb_exclude ::tree::mask::imageMainLine}} {}} {rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq -} {{{Nf3 !? Yellow {Line one\nsecond line} tb_bkm {}}} {}}}\n";

const START_KEY: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -";

fn dir() -> TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn view(rig: &Rig) -> mychess_app::MaskView {
    rig.state.tree_view().mask.expect("a mask is open")
}

fn open_mask(rig: &mut Rig, path: &Path) {
    m(rig, MaskMessage::Open(Some(path.to_path_buf())));
}

/// A rig with a good mask (`good.stm`: only `e4`) already open.
fn rig_with_good(dir: &Path) -> Rig {
    let good = dir.join("good.stm");
    mask_with(&good, "e4");
    let mut rig = Rig::std_with_tree(dir);
    open_mask(&mut rig, &good);
    assert!(rig.state.mask_open());
    rig
}

fn assert_kept(rig: &Rig, what: &str) {
    let v = view(rig);
    assert_eq!(v.file_name.as_deref(), Some("good.stm"), "{what}");
    assert_eq!(v.positions, 1, "{what}");
    assert!(!v.dirty, "{what}");
}

// ---- hostile files (D1801, D1814, D1815, cases 13, 14) ----

#[test]
fn files_that_are_not_masks_fail_with_a_notice_and_keep_the_open_mask() {
    let dir = dir();
    let mut rig = rig_with_good(dir.path());
    let marker = dir.path().join("executed.txt");
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("spaces", b"  \n\t\n".to_vec()),
        ("text", b"just some text\n".to_vec()),
        (
            "binary",
            vec![0x00, 0x9f, 0x92, 0x96, 0xff, 0xfe, 0x00, 0x01, 0x80],
        ),
        ("pgn", b"[Event \"x\"]\n\n1. e4 e5 *\n".to_vec()),
        (
            "other command",
            b"puts hello\nset ::tree::mask::maskSerialized {}\n".to_vec(),
        ),
        (
            "extra command",
            format!(
                "set ::tree::mask::maskSerialized {{}}\nexec touch {}\n",
                marker.display()
            )
            .into_bytes(),
        ),
        (
            "source",
            b"source /etc/passwd\nset ::tree::mask::maskSerialized {}\n".to_vec(),
        ),
        (
            "substitution",
            format!(
                "set ::tree::mask::maskSerialized [exec touch {}]\n",
                marker.display()
            )
            .into_bytes(),
        ),
        (
            "variable",
            b"set ::tree::mask::maskSerialized $env(HOME)\n".to_vec(),
        ),
        (
            "unbalanced",
            b"set ::tree::mask::maskSerialized {{{\n".to_vec(),
        ),
        (
            "junk after",
            b"set ::tree::mask::maskSerialized {} junk\n".to_vec(),
        ),
        ("only set", b"set\n".to_vec()),
    ];
    for (name, bytes) in cases {
        let path = dir.path().join("hostile.stm");
        std::fs::write(&path, bytes).unwrap();
        open_mask(&mut rig, &path);
        match notice(&rig) {
            Some(MaskNotice::OpenFailed { path: p, .. }) => assert_eq!(p, path, "{name}"),
            other => panic!("{name}: {other:?}"),
        }
        assert_kept(&rig, name);
        assert!(!rig.state.prefs.mask.recent.contains(&path), "{name}");
        rig.state.notice = None;
    }
    assert!(!marker.exists(), "nothing of a file is ever executed");
}

#[test]
fn a_foreign_command_is_told_apart_from_a_file_that_is_no_mask() {
    let dir = dir();
    let mut rig = rig_with_good(dir.path());
    let path = dir.path().join("x.stm");
    std::fs::write(&path, "set ::tree::mask::maskSerialized {}\nexec ls\n").unwrap();
    open_mask(&mut rig, &path);
    assert!(
        matches!(
            notice(&rig),
            Some(MaskNotice::OpenFailed {
                issue: MaskIssue::Foreign | MaskIssue::Syntax,
                ..
            })
        ),
        "{:?}",
        notice(&rig)
    );
    std::fs::write(&path, "hello world\n").unwrap();
    open_mask(&mut rig, &path);
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::OpenFailed {
            issue: MaskIssue::NotAMask,
            ..
        })
    ));
}

#[test]
fn a_file_cut_at_every_byte_never_loads_and_never_loses_the_open_mask() {
    let dir = dir();
    let mut rig = rig_with_good(dir.path());
    let path = dir.path().join("cut.stm");
    let text = EXAMPLE_A.replace("\\n", "\n");
    let whole = text.trim_end().len();
    for cut in 0..whole {
        // Cut on a character boundary only (the text is ASCII).
        std::fs::write(&path, &text.as_bytes()[..cut]).unwrap();
        open_mask(&mut rig, &path);
        assert!(
            matches!(notice(&rig), Some(MaskNotice::OpenFailed { .. })),
            "cut at {cut}: {:?}",
            notice(&rig)
        );
        assert_kept(&rig, &format!("cut at {cut}"));
        rig.state.notice = None;
    }
    // The whole file opens, and its line break at the end is not needed.
    std::fs::write(&path, text.trim_end()).unwrap();
    open_mask(&mut rig, &path);
    assert_eq!(view(&rig).file_name.as_deref(), Some("cut.stm"));
    assert_eq!(view(&rig).positions, 3);
}

#[test]
fn a_directory_in_place_of_the_file_is_an_error_not_a_panic() {
    let dir = dir();
    let mut rig = rig_with_good(dir.path());
    let sub = dir.path().join("sub.stm");
    std::fs::create_dir(&sub).unwrap();
    open_mask(&mut rig, &sub);
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::OpenFailed {
            issue: MaskIssue::Io(_),
            ..
        })
    ));
    assert_kept(&rig, "directory");
}

#[cfg(unix)]
#[test]
fn a_file_over_the_limit_is_refused_before_it_is_read() {
    let dir = dir();
    let mut rig = rig_with_good(dir.path());
    let path = dir.path().join("huge.stm");
    let file = std::fs::File::create(&path).unwrap();
    // A sparse file: no disk is used.
    file.set_len(256 * 1024 * 1024 + 1).unwrap();
    drop(file);
    open_mask(&mut rig, &path);
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::OpenFailed {
            issue: MaskIssue::TooLarge,
            ..
        })
    ));
    assert_kept(&rig, "too large");
}

#[test]
fn the_example_of_the_format_opens_with_no_warning_and_hides_the_null_move() {
    let dir = dir();
    let path = dir.path().join("a.stm");
    std::fs::write(&path, EXAMPLE_A.replace("\\n", "\n")).unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "e4.pgn", "[Result \"*\"]\n\n1. e4 e5 *\n\n");
    rig.send(TreeMessage::Open);
    open_mask(&mut rig, &path);
    let v = view(&rig);
    assert_eq!(v.positions, 3);
    assert_eq!(v.report.total_warnings(), 0);
    assert!(
        !matches!(notice(&rig), Some(MaskNotice::Warnings { .. })),
        "no warning to show"
    );
    assert_eq!(v.position_comment, "My repertoire");
    // The tree has `e4`; the mask also has `d4` (and the hidden `null`).
    let cell = rig.row("e4").mask.expect("cell");
    assert!(cell.in_mask);
    assert_eq!(cell.nag, Some(mychess_app::MaskNag::Good));
    assert_eq!(cell.color, mychess_app::MaskColor::Green);
    assert_eq!(cell.markers[0], Some(mychess_app::Marker::Include));
    assert_eq!(cell.comment, "King's pawn");
    let only: Vec<&str> = v.only_rows.iter().map(|r| r.san.as_str()).collect();
    assert_eq!(only, ["d4"], "`null` is never shown (D1810)");
    // A save keeps the three positions and the hidden move.
    m(&mut rig, MaskMessage::Save);
    let (back, _) = stm::read(&path).unwrap();
    assert_eq!(back.len(), 3);
    let start = back.position(&mychess_core::Position::new()).unwrap();
    assert!(
        start
            .moves
            .iter()
            .any(mychess_db::tree::mask::MaskMove::is_null)
    );
}

#[test]
fn a_position_with_too_many_moves_and_a_huge_comment_load_with_a_warning() {
    let dir = dir();
    let path = dir.path().join("big.stm");
    let mut moves = String::new();
    let mut count = 0;
    'outer: for piece in ["Q", "R", "B", "N", "K"] {
        for x in ["", "x"] {
            for file in 'a'..='h' {
                for rank in '1'..='8' {
                    moves.push_str(&format!(
                        "{{{piece}{x}{file}{rank} {{}} white {{}} {{}} {{}}}} "
                    ));
                    count += 1;
                    if count == 600 {
                        break 'outer;
                    }
                }
            }
        }
    }
    let comment = "w".repeat(200_000);
    let text = format!(
        "set ::tree::mask::maskSerialized {{{{{START_KEY}}} {{{{{moves}}} {{{comment}}}}}}}\n"
    );
    std::fs::write(&path, text).unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    open_mask(&mut rig, &path);
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::Warnings { count }) if count >= 2
    ));
    let v = view(&rig);
    // `Nf3` is a row of the tree, so 499 are left for the strip of the mask.
    let in_tree = rig
        .view()
        .rows
        .iter()
        .filter(|r| r.mask.as_ref().is_some_and(|c| c.in_mask))
        .count();
    assert_eq!(v.only_rows.len() + in_tree, 500, "D1815: at most 500 moves");
    let whole = rig.state.mask_comment(None).expect("comment");
    assert!(whole.len() <= 64 * 1024, "{} bytes", whole.len());
    assert!(!whole.is_empty());
    assert!(v.position_comment.len() <= mychess_app::MAX_PREVIEW_BYTES);
    assert!(v.position_comment_more || v.position_comment.len() == whole.len());
}

#[test]
fn a_latin_1_file_and_a_file_with_bom_and_crlf_are_read() {
    let dir = dir();
    let mut rig = Rig::new();
    // `é` in Latin-1 (0xE9) is not valid UTF-8: read as Windows-1252.
    let latin = dir.path().join("latin.stm");
    let mut bytes =
        format!("set ::tree::mask::maskSerialized {{{{{START_KEY}}} {{{{}} {{caf").into_bytes();
    bytes.push(0xE9);
    bytes.extend_from_slice(b"}}}\n");
    std::fs::write(&latin, bytes).unwrap();
    open_mask(&mut rig, &latin);
    assert_eq!(rig.state.mask_comment(None).as_deref(), Some("café"));
    // UTF-8 with BOM and CRLF.
    let bom = dir.path().join("bom.stm");
    let text = format!(
        "\u{feff}set ::tree::mask::maskSerialized {{{{{START_KEY}}} {{{{}} {{one\r\ntwo ñ}}}}}}\r\n"
    );
    std::fs::write(&bom, text).unwrap();
    open_mask(&mut rig, &bom);
    assert_eq!(rig.state.mask_comment(None).as_deref(), Some("one\ntwo ñ"));
}

#[test]
fn the_report_of_the_open_file_replaces_the_one_of_the_last() {
    let dir = dir();
    let odd = dir.path().join("odd.stm");
    // A key with no value (odd number of elements).
    std::fs::write(
        &odd,
        format!("set ::tree::mask::maskSerialized {{{{{START_KEY}}}}}\n"),
    )
    .unwrap();
    let clean = dir.path().join("clean.stm");
    mask_with(&clean, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &odd);
    let odd_view = view(&rig);
    assert!(
        odd_view.report.total_warnings() > 0,
        "{:?}",
        odd_view.report
    );
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::Warnings { count }) if count == odd_view.report.total_warnings()
    ));
    open_mask(&mut rig, &clean);
    assert_eq!(view(&rig).report.total_warnings(), 0);
}

#[test]
fn an_opaque_key_is_kept_by_a_save_and_never_shown() {
    let dir = dir();
    let path = dir.path().join("opaque.stm");
    std::fs::write(
        &path,
        format!(
            "set ::tree::mask::maskSerialized {{{{not a position}} {{{{{{Qh5 {{}} white {{}} {{}} {{}}}}}} {{kept}}}} {{{START_KEY}}} {{{{{{e4 {{}} white {{}} {{}} {{}}}}}} {{}}}}}}\n"
        ),
    )
    .unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    open_mask(&mut rig, &path);
    assert_eq!(view(&rig).positions, 2);
    assert!(rig.row("e4").mask.expect("cell").in_mask);
    edit(&mut rig, "d4", MaskEdit::Add);
    m(&mut rig, MaskMessage::Save);
    let (back, _) = stm::read(&path).unwrap();
    assert_eq!(back.len(), 2, "the opaque key is still there");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("not a position") && text.contains("kept"),
        "{text}"
    );
}

// ---- saving (D1816, D1869, case 15) ----

#[test]
fn a_read_only_file_is_not_overwritten_and_the_work_stays() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let before = std::fs::read(&a).unwrap();
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "d4", MaskEdit::Add);
    let mut perms = std::fs::metadata(&a).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&a, perms).unwrap();
    m(&mut rig, MaskMessage::Save);
    let notice_now = notice(&rig);
    // Restore, so the temporary directory can be removed everywhere.
    let mut perms = std::fs::metadata(&a).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&a, perms).unwrap();
    assert!(
        matches!(
            notice_now,
            Some(MaskNotice::SaveFailed {
                issue: MaskIssue::ReadOnly,
                ..
            })
        ),
        "{notice_now:?}"
    );
    assert!(dirty(&rig));
    assert_eq!(std::fs::read(&a).unwrap(), before);
}

#[test]
fn saving_into_a_directory_that_does_not_exist_fails_and_keeps_the_work() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "d4", MaskEdit::Add);
    let host = m(&mut rig, MaskMessage::SaveAs);
    let elsewhere = dir.path().join("no").join("such").join("dir").join("b.stm");
    pick(&mut rig, &host, Some(elsewhere.clone()));
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::SaveFailed {
            issue: MaskIssue::Io(std::io::ErrorKind::NotFound),
            ..
        })
    ));
    assert!(dirty(&rig));
    assert_eq!(view(&rig).path, Some(a));
    assert!(!rig.state.prefs.mask.recent.contains(&elsewhere));
}

#[test]
fn only_a_name_with_no_extension_gets_the_stm_one() {
    let dir = dir();
    for (given, written) in [
        ("rep", "rep.stm"),
        ("rep.STM", "rep.STM"),
        ("rep.txt", "rep.txt"),
        ("a.b", "a.b"),
    ] {
        let mut rig = Rig::new();
        m(&mut rig, MaskMessage::New);
        edit(&mut rig, "e4", MaskEdit::Add);
        let host = m(&mut rig, MaskMessage::Save);
        pick(&mut rig, &host, Some(dir.path().join(given)));
        assert!(dir.path().join(written).is_file(), "{given} -> {written}");
    }
}

#[test]
fn the_dialog_of_a_save_suggests_the_name_of_the_open_file() {
    let dir = dir();
    let a = dir.path().join("rep.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    let host = m(&mut rig, MaskMessage::SaveAs);
    let Effect::PickFile(spec) = &host[0] else {
        panic!("{host:?}");
    };
    assert_eq!(spec.kind, PickKind::SaveMask);
    assert_eq!(spec.file_name.as_deref(), Some("rep.stm"));
    assert_eq!(spec.initial_dir.as_deref(), Some(dir.path()));
}

#[test]
fn save_and_save_as_with_no_mask_say_so() {
    let mut rig = Rig::new();
    for message in [MaskMessage::Save, MaskMessage::SaveAs] {
        let out = rig.raw(TreeMessage::Mask(message.clone()));
        assert!(out.is_empty(), "{message:?}");
        assert_eq!(notice(&rig), Some(MaskNotice::NoMask), "{message:?}");
        rig.state.notice = None;
    }
}

#[test]
fn a_file_dialog_answer_of_another_request_does_nothing_for_the_mask() {
    let mut rig = Rig::new();
    let host = m(&mut rig, MaskMessage::Open(None));
    let Effect::PickFile(spec) = &host[0] else {
        panic!("{host:?}");
    };
    let other = mychess_app::RequestId(spec.request.0 + 1000);
    let out = rig.raw(Message::FilePicked(other, Some(PathBuf::from("x.stm"))));
    assert!(out.iter().all(|e| !matches!(e, Effect::Mask(_))), "{out:?}");
    assert!(!rig.state.mask_open());
    // The right one still works afterwards.
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    rig.send(Message::FilePicked(spec.request, Some(a)));
    assert!(rig.state.mask_open());
}

// ---- recent files (D1819) ----

#[test]
fn saving_puts_the_file_first_and_remembers_its_directory() {
    let dir = dir();
    let mut rig = Rig::new();
    let mut paths = Vec::new();
    for n in 0..12 {
        m(&mut rig, MaskMessage::New);
        edit(&mut rig, "e4", MaskEdit::Add);
        let host = m(&mut rig, MaskMessage::Save);
        let path = dir.path().join(format!("s{n}.stm"));
        pick(&mut rig, &host, Some(path.clone()));
        paths.push(path);
    }
    let prefs = &rig.state.prefs.mask;
    assert_eq!(prefs.recent.len(), 10);
    assert_eq!(prefs.recent[0], paths[11]);
    assert_eq!(prefs.recent[9], paths[2]);
    assert_eq!(prefs.last_dir.as_deref(), Some(dir.path()));
    assert_eq!(view(&rig).recent, prefs.recent);
}

#[test]
fn a_file_that_does_not_open_does_not_enter_the_list_and_removing_an_absent_one_is_a_no_op() {
    let dir = dir();
    let mut rig = Rig::new();
    let missing = dir.path().join("missing.stm");
    open_mask(&mut rig, &missing);
    assert!(rig.state.prefs.mask.recent.is_empty());
    assert!(rig.state.prefs.mask.last_dir.is_none());
    let before = rig.state.prefs.mask.clone();
    m(&mut rig, MaskMessage::RemoveRecent(missing));
    assert_eq!(rig.state.prefs.mask, before);
}

#[test]
fn opening_the_same_file_by_two_paths_keeps_the_compare_by_path() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    open_mask(&mut rig, &a);
    assert_eq!(rig.state.prefs.mask.recent, [a.clone()]);
    let dotted = dir.path().join(".").join("a.stm");
    open_mask(&mut rig, &dotted);
    // D1819: the comparison is by path; a different spelling is another
    // entry, but never a crash and never more than ten.
    assert!(rig.state.prefs.mask.recent.len() <= 2);
    assert_eq!(rig.state.prefs.mask.recent[0], dotted);
}

// ---- the options (D1861) ----

/// A TOML basic string with the path inside (absolute on every platform).
fn toml_path(path: &Path) -> String {
    let text = path.to_str().expect("utf-8 path");
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn abs(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

fn prefs_of(text: &str) -> (MaskPrefs, Vec<PrefsIssue>) {
    let loaded = load_preferences(text);
    ((*loaded.prefs.mask).clone(), loaded.issues)
}

fn invalid(key: &str) -> PrefsIssue {
    PrefsIssue::InvalidValue {
        key: key.to_owned(),
    }
}

#[test]
fn a_wrong_type_resets_only_its_own_field_and_is_reported() {
    let defaults = MaskPrefs::default();
    let cases = [
        ("auto_load = \"yes\"", "tree_mask.auto_load"),
        ("auto_load = 1", "tree_mask.auto_load"),
        ("show_markers = []", "tree_mask.show_markers"),
        ("tooltips = 0.5", "tree_mask.tooltips"),
        ("recent = 5", "tree_mask.recent"),
        ("recent = \"a.stm\"", "tree_mask.recent"),
        ("last_dir = 7", "tree_mask.last_dir"),
        ("last_dir = [\"a\"]", "tree_mask.last_dir"),
    ];
    for (line, key) in cases {
        let keep = abs("keep.stm");
        let recent_line = if key == "tree_mask.recent" {
            String::new()
        } else {
            format!("recent = [{}]\n", toml_path(&keep))
        };
        let (prefs, issues) = prefs_of(&format!("[tree_mask]\n{recent_line}{line}\n"));
        assert!(issues.contains(&invalid(key)), "{line}: {issues:?}");
        // The other fields keep what the file says, or the default.
        assert_eq!(prefs.show_markers, defaults.show_markers, "{line}");
        if key != "tree_mask.recent" {
            assert_eq!(prefs.recent, [keep], "{line}");
        }
    }
}

#[test]
fn the_list_of_recent_files_is_cleaned_when_it_is_read() {
    let mut text = String::from("[tree_mask]\nrecent = [");
    for n in 0..200 {
        text.push_str(&toml_path(&abs(&format!("f{}.stm", n % 50))));
        text.push_str(", ");
    }
    text.push_str(&format!(
        "\"\", 3, true, \"relative.stm\", {}]\n",
        toml_path(&abs("ok.stm"))
    ));
    let (prefs, issues) = prefs_of(&text);
    assert!(prefs.recent.len() <= 10);
    let mut unique = prefs.recent.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), prefs.recent.len(), "no repeated path");
    assert!(prefs.recent.iter().all(|p| p.is_absolute()));
    assert!(
        issues.contains(&invalid("tree_mask.recent")),
        "the entries that are no path are reported: {issues:?}"
    );
}

#[test]
fn unknown_keys_and_tables_are_ignored_without_a_report() {
    let (prefs, issues) = prefs_of(
        "[tree_mask]\nfuture = 1\nauto_load = true\n[tree_mask.sub]\nx = 2\n[other]\ntree_mask = 1\n",
    );
    assert!(prefs.auto_load);
    assert!(
        !issues.iter().any(|i| matches!(
            i,
            PrefsIssue::InvalidValue { key } if key.starts_with("tree_mask.f")
        )),
        "{issues:?}"
    );
}

#[test]
fn extreme_values_load_without_an_error_and_come_back_from_a_save() {
    let long = "d".repeat(100_000);
    let long_file = abs(&format!("{long}.stm"));
    let weird = abs("\u{1F600}/é ü ' [x] # \"q\" $ {}.stm");
    let text = format!(
        "[tree_mask]\nrecent = [{}, {}]\nlast_dir = {}\nauto_load = false\n",
        toml_path(&long_file),
        toml_path(&weird),
        toml_path(&abs(&long)),
    );
    let (prefs, issues) = prefs_of(&text);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(prefs.recent, [long_file, weird]);
    // Render and read again: the same.
    let mut loaded = load_preferences(&text).prefs;
    *loaded.mask = prefs.clone();
    let again = load_preferences(&render_preferences(&loaded));
    assert!(again.issues.is_empty(), "{:?}", again.issues);
    assert_eq!(*again.prefs.mask, prefs);
}

#[test]
fn the_whole_table_of_the_wrong_kind_gives_the_defaults() {
    for text in [
        "tree_mask = 1\n",
        "tree_mask = \"x\"\n",
        "tree_mask = [1, 2]\n",
        "[[tree_mask]]\nauto_load = true\n",
    ] {
        let (prefs, _) = prefs_of(text);
        assert_eq!(prefs, MaskPrefs::default(), "{text}");
    }
}

#[test]
fn changing_the_options_through_the_state_is_the_same_as_the_message() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::SetAutoLoad(true));
    assert!(rig.state.prefs.mask.auto_load);
    // The same value again produces no work.
    let out = rig.raw(TreeMessage::Mask(MaskMessage::SetAutoLoad(true)));
    assert!(out.is_empty(), "{out:?}");
    // The generic message reaches the same field and keeps the list.
    let mut prefs = (*rig.state.prefs.mask).clone();
    prefs.recent = vec![PathBuf::from("x.stm")];
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::TreeMask(
        Box::new(prefs),
    ))));
    m(&mut rig, MaskMessage::SetTooltips(false));
    assert_eq!(rig.state.prefs.mask.recent, [PathBuf::from("x.stm")]);
    assert!(!rig.state.prefs.mask.tooltips);
}

// ---- the auto-load (D1820, D1871) ----

fn rig_with_auto_load(recent: Vec<PathBuf>) -> Rig {
    let mut rig = Rig::new();
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::TreeMask(
        Box::new(MaskPrefs {
            recent,
            auto_load: true,
            ..MaskPrefs::default()
        }),
    ))));
    rig
}

#[test]
fn the_auto_load_happens_once_for_each_opening_of_the_panel() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = rig_with_auto_load(vec![a]);
    rig.send(TreeMessage::Open);
    assert!(rig.state.mask_open());
    // The user closes the mask: the panel is still open and it stays closed.
    m(&mut rig, MaskMessage::Close);
    m(&mut rig, MaskMessage::SetTooltips(false));
    rig.send(TreeMessage::SetTraining(false));
    assert!(
        !rig.state.mask_open(),
        "no second auto-load in the same opening"
    );
    // Closing the panel and opening it again is another opening.
    rig.send(TreeMessage::Close);
    rig.send(TreeMessage::Open);
    assert!(rig.state.mask_open());
}

#[test]
fn the_auto_load_does_not_replace_a_mask_that_is_open() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = rig_with_auto_load(vec![a]);
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "d4", MaskEdit::Add);
    rig.send(TreeMessage::Open);
    assert_eq!(view(&rig).file_name, None);
    assert!(dirty(&rig));
    assert!(rig.state.mask_prompt().is_none());
}

#[test]
fn a_failed_auto_load_is_tried_once_and_leaves_the_list_alone() {
    let dir = dir();
    let gone = dir.path().join("gone.stm");
    let mut rig = rig_with_auto_load(vec![gone.clone()]);
    let first = rig.raw(TreeMessage::Open);
    assert_eq!(
        first
            .iter()
            .filter(|e| matches!(e, Effect::Mask(MaskEffect::Read { .. })))
            .count(),
        1
    );
    let read = first
        .into_iter()
        .find(|e| matches!(e, Effect::Mask(MaskEffect::Read { .. })))
        .unwrap();
    let answer = rig.perform(read);
    let after = rig.raw(answer);
    assert!(
        after
            .iter()
            .all(|e| !matches!(e, Effect::Mask(MaskEffect::Read { .. })))
    );
    assert!(matches!(notice(&rig), Some(MaskNotice::OpenFailed { .. })));
    // Other messages do not try again.
    for message in [
        TreeMessage::SetTraining(false),
        TreeMessage::Mask(MaskMessage::SetTooltips(false)),
    ] {
        let out = rig.raw(message);
        assert!(
            out.iter()
                .all(|e| !matches!(e, Effect::Mask(MaskEffect::Read { .. }))),
            "{out:?}"
        );
    }
    assert_eq!(rig.state.prefs.mask.recent, [gone]);
}

#[test]
fn the_auto_load_with_an_empty_list_does_nothing() {
    let mut rig = rig_with_auto_load(Vec::new());
    let out = rig.raw(TreeMessage::Open);
    assert!(out.iter().all(|e| !matches!(e, Effect::Mask(_))), "{out:?}");
    assert!(!rig.state.mask_open());
    assert_eq!(notice(&rig), None);
}

// ---- update does no I/O (rule of gold 3) ----

#[test]
fn update_only_asks_for_the_files_and_never_touches_them() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    // The read is an effect: the state has nothing before it is performed,
    // even if the file is gone when it runs.
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::Open(Some(a.clone()))));
    assert!(matches!(
        effects.as_slice(),
        [Effect::Mask(MaskEffect::Read { .. })]
    ));
    assert!(!rig.state.mask_open());
    std::fs::remove_file(&a).unwrap();
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert!(!rig.state.mask_open());
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::OpenFailed {
            issue: MaskIssue::Io(std::io::ErrorKind::NotFound),
            ..
        })
    ));
    // The write too: nothing is on disk before the effect runs.
    let b = dir.path().join("b.stm");
    mask_with(&b, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &b);
    edit(&mut rig, "d4", MaskEdit::Add);
    let before = std::fs::read(&b).unwrap();
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::Save));
    assert!(matches!(
        effects.as_slice(),
        [Effect::Mask(MaskEffect::Write { .. })]
    ));
    assert_eq!(std::fs::read(&b).unwrap(), before, "update wrote nothing");
    assert!(dirty(&rig), "and it is not clean before the answer");
}

#[test]
fn a_search_and_an_outline_do_not_edit_and_do_not_mark() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    m(&mut rig, MaskMessage::Search(Box::default()));
    m(&mut rig, MaskMessage::BuildOutline);
    assert!(!dirty(&rig));
    assert!(rig.state.mask_search().is_some());
    assert!(rig.state.mask_outline().is_some());
    // Closing the mask forgets both.
    m(&mut rig, MaskMessage::Close);
    assert!(rig.state.mask_search().is_none() && rig.state.mask_outline().is_none());
}
