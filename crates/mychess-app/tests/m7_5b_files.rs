//! Contract tests of opening, creating, closing and saving EPD files (spec
//! `m7-5-janela-epd.md`, rules 1-5, criteria 5, 6 and 8).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]
mod m7_5b_common;

use m7_5b_common::*;
use mychess_app::FileErrorKind;
use mychess_pgn::epd::{EpdBook, EpdOptions};

#[test]
fn update_only_describes_the_read_and_the_answer_opens_the_tab() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "cinco.epd");
    let mut rig = Rig::new();
    let effects = rig.raw(Message::Epd(EpdMessage::OpenPath(path)));
    assert!(matches!(
        effects.as_slice(),
        [Effect::Epd(EpdEffect::Open { create: false, .. })]
    ));
    // Nothing is open until the answer arrives.
    assert!(rig.view().tabs.is_empty());
    assert!(rig.view().opening);
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    let tab = rig.tab();
    assert_eq!((tab.records, tab.rows), (5, 5));
    assert_eq!(tab.name, "cinco.epd");
    assert!(!tab.read_only && !tab.dirty);
    assert!(!rig.view().opening);
    assert_eq!(rig.view().recent.len(), 1);
}

#[test]
fn an_answer_of_an_old_request_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "cinco.epd");
    let mut rig = Rig::new();
    let effects = rig.raw(Message::Epd(EpdMessage::OpenPath(path)));
    let Message::Epd(EpdMessage::Opened { result, .. }) =
        rig.perform(effects.into_iter().next().unwrap())
    else {
        panic!("opened expected");
    };
    rig.raw(Message::Epd(EpdMessage::Opened {
        request: RequestId(9_999_999),
        result,
    }));
    assert!(rig.view().tabs.is_empty());
    assert!(rig.view().opening, "still waiting for the real answer");
}

#[test]
fn cancelling_the_open_drops_the_late_answer() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "cinco.epd");
    let mut rig = Rig::new();
    let effects = rig.raw(Message::Epd(EpdMessage::OpenPath(path)));
    rig.raw(Message::Epd(EpdMessage::CancelOpen));
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert!(rig.view().tabs.is_empty());
}

#[test]
fn the_fifth_file_is_refused_and_the_same_file_does_not_open_twice() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    for i in 0..4 {
        let path = write_cinco(dir.path(), &format!("f{i}.epd"));
        rig.open(&path);
    }
    assert_eq!(rig.view().tabs.len(), 4);
    let fifth = write_cinco(dir.path(), "f4.epd");
    let effects = rig.raw(Message::Epd(EpdMessage::OpenPath(fifth)));
    assert!(effects.is_empty(), "no read is started");
    assert_eq!(rig.notice(), Some(EpdNotice::TooMany { max: 4 }));
    // The same file, even with the limit reached: it is already open.
    let again = dir.path().join("f1.epd");
    rig.raw(Message::Epd(EpdMessage::OpenPath(again)));
    assert_eq!(rig.notice(), Some(EpdNotice::AlreadyOpen));
    assert_eq!(rig.view().tabs.len(), 4);
}

#[test]
fn the_same_file_opened_twice_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "a.epd");
    let mut rig = Rig::new();
    rig.open(&path);
    rig.epd(EpdMessage::OpenPath(path));
    assert_eq!(rig.notice(), Some(EpdNotice::AlreadyOpen));
    assert_eq!(rig.view().tabs.len(), 1);
}

#[cfg(unix)]
#[test]
fn the_same_file_through_a_symbolic_link_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "a.epd");
    let link = dir.path().join("link.epd");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let mut rig = Rig::new();
    rig.open(&path);
    rig.epd(EpdMessage::OpenPath(link));
    assert_eq!(rig.notice(), Some(EpdNotice::AlreadyOpen));
    assert_eq!(rig.view().tabs.len(), 1);
}

#[test]
fn a_missing_file_is_a_notice_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.epd(EpdMessage::OpenPath(dir.path().join("nope.epd")));
    assert!(matches!(
        rig.notice(),
        Some(EpdNotice::OpenFailed(e)) if e.kind == FileErrorKind::NotFound
    ));
    assert!(rig.view().tabs.is_empty());
    assert!(!rig.view().opening);
}

#[test]
fn a_dirty_file_reports_the_lines_it_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sujo.epd");
    let mut text = cinco_text();
    text.push_str("not a fen\n");
    text.push_str("# a comment\n\n");
    text.push_str(&four_fields(&five_positions()[0]));
    text.push_str(" id \"no end");
    std::fs::write(&path, text).unwrap();
    let mut rig = Rig::new();
    let id = rig.open(&path);
    let tab = rig.tab();
    assert_eq!(tab.records, 5);
    // The invalid FEN, the repeated position and the string without end.
    assert!(tab.skipped_lines >= 2, "{}", tab.skipped_lines);
    assert!(matches!(rig.notice(), Some(EpdNotice::SkippedLines { .. })));
    let (diagnostics, _) = rig.state.epd_diagnostics(id).expect("report");
    assert!(diagnostics.iter().any(|d| d.line == 6));
}

#[test]
fn a_gz_file_is_read_by_its_content() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cinco.epd.gz");
    let mut encoder = flate2_encoder(&path);
    encoder.write_all(cinco_text().as_bytes()).unwrap();
    encoder.finish().unwrap();
    let mut rig = Rig::new();
    rig.open(&path);
    assert_eq!(rig.tab().records, 5);
}

fn flate2_encoder(path: &std::path::Path) -> flate2::write::GzEncoder<std::fs::File> {
    flate2::write::GzEncoder::new(
        std::fs::File::create(path).unwrap(),
        flate2::Compression::default(),
    )
}

#[test]
fn open_by_the_file_menu_goes_to_the_epd_window_by_extension() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "viaopen.EPD");
    let mut rig = Rig::new();
    let host = rig.send(Command::Open);
    let Some(Effect::PickFile(spec)) = host.into_iter().next() else {
        panic!("a dialog is asked");
    };
    rig.send(Message::FilePicked(spec.request, Some(path)));
    assert_eq!(rig.view().tabs.len(), 1);
    assert_eq!(rig.tab().records, 5);
    // The PGN side was not touched.
    assert!(
        rig.state
            .bases
            .slot(rig.state.bases.current())
            .is_none_or(|s| s.path.is_none())
    );
}

#[test]
fn the_open_and_new_dialogs_use_their_own_kinds() {
    let mut rig = Rig::new();
    let host = rig.send(EpdMessage::Open);
    let Some(Effect::PickFile(spec)) = host.into_iter().next() else {
        panic!("dialog");
    };
    assert_eq!(spec.kind, PickKind::OpenEpd);
    rig.send(Message::FilePicked(spec.request, None));
    let host = rig.send(EpdMessage::New);
    let Some(Effect::PickFile(spec)) = host.into_iter().next() else {
        panic!("dialog");
    };
    assert_eq!(spec.kind, PickKind::NewEpd);
}

#[test]
fn new_creates_an_empty_file_with_the_extension_added() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let host = rig.send(EpdMessage::New);
    let Some(Effect::PickFile(spec)) = host.into_iter().next() else {
        panic!("dialog");
    };
    rig.send(Message::FilePicked(
        spec.request,
        Some(dir.path().join("novo")),
    ));
    assert!(dir.path().join("novo.epd").exists());
    let tab = rig.tab();
    assert_eq!(tab.records, 0);
    assert_eq!(tab.name, "novo.epd");
    assert!(!tab.dirty);
}

#[test]
fn closing_a_clean_file_closes_it_and_an_altered_one_asks() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "a.epd");
    let mut rig = Rig::new();
    let id = rig.open(&path);
    rig.epd(EpdMessage::Edit("bm d4\n".into()));
    rig.epd(EpdMessage::Commit);
    assert!(rig.tab().dirty);
    rig.epd(EpdMessage::Close(id));
    assert_eq!(rig.view().close_prompt, Some(id));
    assert_eq!(rig.view().tabs.len(), 1);
    rig.epd(EpdMessage::AnswerClose(EpdCloseChoice::Cancel));
    assert_eq!(rig.view().close_prompt, None);
    assert_eq!(rig.view().tabs.len(), 1, "cancel keeps the tab");
    rig.epd(EpdMessage::Close(id));
    rig.epd(EpdMessage::AnswerClose(EpdCloseChoice::Discard));
    assert!(rig.view().tabs.is_empty());
    // Nothing was written.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), cinco_text());
    // A clean file closes at once.
    let id = rig.open(&path);
    rig.epd(EpdMessage::Close(id));
    assert!(rig.view().tabs.is_empty());
    assert_eq!(rig.view().close_prompt, None);
}

#[test]
fn save_and_close_writes_then_closes() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "a.epd");
    let mut rig = Rig::new();
    let id = rig.open(&path);
    rig.epd(EpdMessage::Edit("bm d4\n".into()));
    rig.epd(EpdMessage::Commit);
    rig.epd(EpdMessage::Close(id));
    rig.epd(EpdMessage::AnswerClose(EpdCloseChoice::Save));
    assert!(rig.view().tabs.is_empty());
    let back = EpdBook::read_file(&path, EpdOptions::standard()).unwrap();
    let first = back.book.get(&five_positions()[0]).unwrap();
    assert_eq!(first.operations_text(), "bm d4\n");
    assert!(!dir.path().join("a.epd.tmp").exists());
}

#[test]
fn save_writes_the_book_and_clears_the_altered_mark() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "a.epd");
    let mut rig = Rig::new();
    rig.open(&path);
    rig.epd(EpdMessage::Edit("bm Nf3\nid \"new\"\n".into()));
    rig.epd(EpdMessage::Commit);
    assert!(rig.tab().dirty);
    rig.epd(EpdMessage::Save);
    assert!(!rig.tab().dirty);
    let back = EpdBook::read_file(&path, EpdOptions::standard()).unwrap();
    assert_eq!(back.book.len(), 5);
    assert_eq!(
        back.book
            .get(&five_positions()[0])
            .unwrap()
            .operations_text(),
        "bm Nf3\nid \"new\"\n"
    );
}

#[test]
fn save_as_gz_writes_compressed_and_renames_the_tab() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "a.epd");
    let mut rig = Rig::new();
    rig.open(&path);
    let host = rig.send(EpdMessage::SaveAs);
    let Some(Effect::PickFile(spec)) = host.into_iter().next() else {
        panic!("dialog");
    };
    assert_eq!(spec.kind, PickKind::SaveEpd);
    let target = dir.path().join("b.epd.gz");
    rig.send(Message::FilePicked(spec.request, Some(target.clone())));
    assert_eq!(rig.tab().name, "b.epd.gz");
    let raw = std::fs::read(&target).unwrap();
    assert_eq!(&raw[..2], &[0x1F, 0x8B]);
    let back = EpdBook::read_file(&target, EpdOptions::standard()).unwrap();
    assert_eq!(back.book.len(), 5);
    // The original is untouched.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), cinco_text());
}

#[test]
fn a_record_that_cannot_be_written_is_a_clear_notice_and_nothing_is_touched() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "a.epd");
    let mut rig = Rig::new();
    rig.open(&path);
    // A `;` typed at the end of the line (the editor shows no `;`).
    rig.epd(EpdMessage::Edit("id \"x\";\n".into()));
    rig.epd(EpdMessage::Save);
    let Some(EpdNotice::Unwritable { opcode, kind }) = rig.notice() else {
        panic!("unwritable expected, got {:?}", rig.notice());
    };
    assert_eq!(opcode, "id");
    assert_eq!(kind, mychess_app::epd_window::UnwritableKind::Semicolon);
    assert_eq!(
        EpdNotice::Unwritable { opcode, kind }.message_key(),
        "epd-unwritable-semicolon"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), cinco_text());
    assert!(rig.tab().dirty, "the changes are still there");
    // A string that never closes.
    rig.epd(EpdMessage::Edit("c0 \"never closed\n".into()));
    rig.epd(EpdMessage::Save);
    assert!(matches!(
        rig.notice(),
        Some(EpdNotice::Unwritable {
            kind: mychess_app::epd_window::UnwritableKind::UnbalancedQuote,
            ..
        })
    ));
}

#[test]
fn an_operand_the_reader_would_drop_is_refused_on_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "a.epd");
    let mut rig = Rig::new();
    rig.open(&path);
    let huge = format!("c0 \"{}\"\n", "x".repeat(1_200_000));
    rig.epd(EpdMessage::Edit(huge));
    rig.epd(EpdMessage::Save);
    assert!(matches!(
        rig.notice(),
        Some(EpdNotice::Unwritable {
            kind: mychess_app::epd_window::UnwritableKind::TooLong,
            ..
        })
    ));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), cinco_text());
}

#[cfg(unix)]
#[test]
fn a_read_only_file_opens_read_only_refuses_edits_and_saves_but_save_as_works() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "ro.epd");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    // A user that can write anyway (root) cannot show the behaviour.
    if std::fs::OpenOptions::new().write(true).open(&path).is_ok() {
        return;
    }
    let mut rig = Rig::new();
    rig.open(&path);
    let tab = rig.tab();
    assert!(tab.read_only);
    assert!(!tab.editor_enabled);
    rig.epd(EpdMessage::Edit("bm e4\n".into()));
    assert_eq!(rig.notice(), Some(EpdNotice::ReadOnly));
    assert!(!rig.tab().editor_draft);
    rig.epd(EpdMessage::Save);
    assert_eq!(rig.notice(), Some(EpdNotice::ReadOnly));
    let host = rig.send(EpdMessage::SaveAs);
    let Some(Effect::PickFile(spec)) = host.into_iter().next() else {
        panic!("save as is available");
    };
    let target = dir.path().join("copy.epd");
    rig.send(Message::FilePicked(spec.request, Some(target.clone())));
    assert!(target.exists());
    assert!(!rig.tab().read_only);
}

#[test]
fn quit_drops_the_files_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cinco(dir.path(), "a.epd");
    let mut rig = Rig::new();
    let effects = rig.raw(Message::Epd(EpdMessage::OpenPath(path)));
    rig.raw(Message::Quit);
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    assert!(rig.view().tabs.is_empty());
}
