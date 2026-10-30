//! Contract tests of the files, the question about saving, the preferences
//! and the quit of the tree masks (spec M5-4, slice M5-4b: criteria 1 to 3,
//! the flow of D1817, D1818, D1873, D1874).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::cloned_ref_to_slice_refs
)]

mod m5_4b_common;

use m5_4b_common::*;
use mychess_app::{
    DiscardChoice, MaskIssue, MaskPending, MaskPrefs, PickKind, PrefChange, PrefsIssue,
    PrefsMessage, load_preferences, render_preferences,
};
use mychess_db::tree::mask::stm;

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn open_mask(rig: &mut Rig, path: &std::path::Path) {
    m(rig, MaskMessage::Open(Some(path.to_path_buf())));
}

fn view(rig: &Rig) -> mychess_app::MaskView {
    rig.state.tree_view().mask.expect("a mask is open")
}

fn positions_of(path: &std::path::Path) -> usize {
    stm::read(path).expect("readable").0.len()
}

#[test]
fn opening_a_file_shows_it_and_remembers_it() {
    let dir = dir();
    let path = dir.path().join("rep.stm");
    mask_with(&path, "e4");
    let mut rig = Rig::new();
    assert!(rig.state.tree_view().mask.is_none());
    open_mask(&mut rig, &path);
    let v = view(&rig);
    assert_eq!(v.file_name.as_deref(), Some("rep.stm"));
    assert_eq!(v.positions, 1);
    assert!(!v.dirty);
    assert_eq!(v.recent, [path.clone()]);
    assert_eq!(rig.state.prefs.mask.recent, [path]);
}

#[test]
fn a_file_that_is_not_a_mask_leaves_the_open_mask_alone() {
    let dir = dir();
    let good = dir.path().join("good.stm");
    mask_with(&good, "e4");
    let bad = dir.path().join("bad.stm");
    std::fs::write(&bad, "source /etc/passwd\n").expect("write");
    let mut rig = Rig::new();
    open_mask(&mut rig, &good);
    open_mask(&mut rig, &bad);
    assert_eq!(
        notice(&rig),
        Some(MaskNotice::OpenFailed {
            path: bad.clone(),
            issue: MaskIssue::NotAMask
        })
    );
    let v = view(&rig);
    assert_eq!(v.file_name.as_deref(), Some("good.stm"));
    assert!(!rig.state.prefs.mask.recent.contains(&bad));
    // A missing file: the same, with the kind of the error.
    open_mask(&mut rig, &dir.path().join("gone.stm"));
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::OpenFailed {
            issue: MaskIssue::Io(std::io::ErrorKind::NotFound),
            ..
        })
    ));
    assert_eq!(view(&rig).file_name.as_deref(), Some("good.stm"));
}

#[test]
fn warnings_of_the_reader_are_counted_and_kept() {
    let dir = dir();
    let path = dir.path().join("odd.stm");
    // One key twice: the reader keeps the last and warns.
    let key = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -";
    std::fs::write(
        &path,
        format!(
            "set ::tree::mask::maskSerialized {{{{{key}}} {{{{}} {{}}}} {{{key}}} {{{{}} {{}}}}}}\n"
        ),
    )
    .expect("write");
    let mut rig = Rig::new();
    open_mask(&mut rig, &path);
    let v = view(&rig);
    assert!(v.report.total_warnings() > 0, "{:?}", v.report);
    assert!(matches!(
        notice(&rig),
        Some(MaskNotice::Warnings { count }) if count == v.report.total_warnings()
    ));
}

#[test]
fn a_new_mask_has_no_file_until_the_first_save_names_it() {
    let dir = dir();
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    assert_eq!(view(&rig).file_name, None);
    assert!(!dirty(&rig));
    edit(&mut rig, "e4", MaskEdit::Add);
    assert!(dirty(&rig));
    let host = m(&mut rig, MaskMessage::Save);
    let spec = match &host[0] {
        Effect::PickFile(spec) => spec.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(spec.kind, PickKind::SaveMask);
    assert_eq!(spec.file_name.as_deref(), Some("mask.stm"));
    // Nothing is written until the name comes; a name with no extension
    // gets `.stm`.
    pick(&mut rig, &host, Some(dir.path().join("rep")));
    let path = dir.path().join("rep.stm");
    assert_eq!(positions_of(&path), 1);
    assert!(!dirty(&rig));
    assert_eq!(view(&rig).file_name.as_deref(), Some("rep.stm"));
    assert_eq!(rig.state.prefs.mask.recent, [path]);
    assert_eq!(notice(&rig), Some(MaskNotice::Saved));
}

#[test]
fn save_writes_the_file_and_save_as_changes_the_destination() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "d4", MaskEdit::Add);
    assert!(dirty(&rig));
    assert!(m(&mut rig, MaskMessage::Save).is_empty());
    assert!(!dirty(&rig));
    assert_eq!(positions_of(&a), 1);
    let (read, _) = stm::read(&a).unwrap();
    let moves: Vec<String> = read
        .position(&mychess_core::Position::new())
        .unwrap()
        .moves
        .iter()
        .map(|mv| mv.san.clone())
        .collect();
    assert_eq!(moves, ["e4", "d4"]);
    let host = m(&mut rig, MaskMessage::SaveAs);
    let b = dir.path().join("b.stm");
    pick(&mut rig, &host, Some(b.clone()));
    assert!(b.is_file());
    assert_eq!(view(&rig).path, Some(b));
}

#[test]
fn a_failed_save_keeps_the_mask_altered_and_the_old_file() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "d4", MaskEdit::Add);
    let before = std::fs::read(&a).unwrap();
    // The destination is a directory: the write fails.
    let target = dir.path().join("d.stm");
    std::fs::create_dir(&target).unwrap();
    let host = m(&mut rig, MaskMessage::SaveAs);
    pick(&mut rig, &host, Some(target));
    assert!(matches!(notice(&rig), Some(MaskNotice::SaveFailed { .. })));
    assert!(dirty(&rig));
    assert_eq!(view(&rig).path, Some(a.clone()));
    assert_eq!(std::fs::read(&a).unwrap(), before);
}

#[test]
fn a_change_with_no_effect_does_not_alter_the_mask() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    m(&mut rig, MaskMessage::Answer(DiscardChoice::DontSave));
    // Save to make it clean, then repeat the same edits.
    let dir = dir();
    let host = m(&mut rig, MaskMessage::Save);
    pick(&mut rig, &host, Some(dir.path().join("m.stm")));
    assert!(!dirty(&rig));
    edit(&mut rig, "e4", MaskEdit::Add);
    edit(&mut rig, "e4+", MaskEdit::Add);
    edit(&mut rig, "e4", MaskEdit::Nag(None));
    edit(&mut rig, "d4", MaskEdit::Remove);
    m(&mut rig, MaskMessage::SetPositionComment(String::new()));
    assert!(!dirty(&rig));
    edit(
        &mut rig,
        "e4",
        MaskEdit::Nag(Some(mychess_app::MaskNag::Good)),
    );
    assert!(dirty(&rig));
}

#[test]
fn opening_new_or_closing_an_altered_mask_asks_first() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    let b = dir.path().join("b.stm");
    mask_with(&a, "e4");
    mask_with(&b, "d4");
    for then in [
        MaskMessage::Open(Some(b.clone())),
        MaskMessage::New,
        MaskMessage::Close,
    ] {
        let mut rig = Rig::new();
        open_mask(&mut rig, &a);
        edit(&mut rig, "c4", MaskEdit::Add);
        let host = m(&mut rig, then.clone());
        assert!(host.is_empty());
        let prompt = rig.state.mask_prompt().expect("the question").clone();
        assert_eq!(prompt.name.as_deref(), Some("a.stm"));
        assert!(rig.state.modal_open(), "the question is modal");
        // Cancel: nothing changes.
        m(&mut rig, MaskMessage::Answer(DiscardChoice::Cancel));
        assert!(rig.state.mask_prompt().is_none());
        assert_eq!(view(&rig).file_name.as_deref(), Some("a.stm"));
        assert!(dirty(&rig));
        // Don't save: it goes on, and the file is untouched.
        m(&mut rig, then.clone());
        m(&mut rig, MaskMessage::Answer(DiscardChoice::DontSave));
        assert_eq!(positions_of(&a), 1);
        let (read, _) = stm::read(&a).unwrap();
        assert_eq!(
            read.position(&mychess_core::Position::new())
                .unwrap()
                .moves
                .len(),
            1
        );
        match then {
            MaskMessage::Open(_) => {
                assert_eq!(view(&rig).file_name.as_deref(), Some("b.stm"));
            }
            MaskMessage::New => assert_eq!(view(&rig).file_name, None),
            _ => assert!(rig.state.tree_view().mask.is_none()),
        }
    }
}

#[test]
fn save_in_the_question_writes_then_goes_on() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    let b = dir.path().join("b.stm");
    mask_with(&a, "e4");
    mask_with(&b, "d4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "c4", MaskEdit::Add);
    m(&mut rig, MaskMessage::Open(Some(b.clone())));
    m(&mut rig, MaskMessage::Answer(DiscardChoice::Save));
    let (read, _) = stm::read(&a).unwrap();
    assert_eq!(
        read.position(&mychess_core::Position::new())
            .unwrap()
            .moves
            .len(),
        2
    );
    assert_eq!(view(&rig).file_name.as_deref(), Some("b.stm"));
    assert!(!dirty(&rig));
}

#[test]
fn a_failed_save_in_the_question_stops_what_waited() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "c4", MaskEdit::Add);
    // The file becomes a directory: the write fails.
    std::fs::remove_file(&a).unwrap();
    std::fs::create_dir(&a).unwrap();
    m(&mut rig, MaskMessage::New);
    m(&mut rig, MaskMessage::Answer(DiscardChoice::Save));
    assert!(matches!(notice(&rig), Some(MaskNotice::SaveFailed { .. })));
    assert_eq!(view(&rig).file_name.as_deref(), Some("a.stm"), "still open");
    assert!(dirty(&rig), "the work is not lost");
    assert!(rig.state.mask_prompt().is_none());
}

#[test]
fn closing_the_tree_panel_does_not_close_the_mask() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::std_with_tree(dir.path());
    open_mask(&mut rig, &a);
    rig.send(TreeMessage::Close);
    assert!(rig.state.mask_open());
    assert_eq!(view(&rig).file_name.as_deref(), Some("a.stm"));
    rig.send(TreeMessage::Open);
    assert!(rig.state.mask_open());
    // Neither does closing the base.
    let slot = rig.state.bases.current();
    rig.send(mychess_app::BasesMessage::Close(slot));
    assert!(rig.state.mask_open());
}

#[test]
fn a_late_answer_for_another_request_is_dropped() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    // The file is asked for; the answer of an older request arrives first.
    let effects = rig.raw(TreeMessage::Mask(MaskMessage::Open(Some(a.clone()))));
    let Effect::Mask(effect) = effects.into_iter().next().expect("the read") else {
        panic!("not a mask effect");
    };
    let answer = rig.perform(Effect::Mask(effect));
    let stale = Message::Tree(TreeMessage::Mask(MaskMessage::Opened {
        request: mychess_app::RequestId(9999),
        result: Box::new(Err(MaskIssue::NotAMask)),
    }));
    rig.send(stale);
    assert!(!rig.state.mask_open());
    rig.send(answer);
    assert!(rig.state.mask_open());
}

#[test]
fn recent_files_are_ten_newest_first_and_can_be_removed() {
    let dir = dir();
    let mut rig = Rig::new();
    let mut paths = Vec::new();
    for n in 0..12 {
        let path = dir.path().join(format!("m{n}.stm"));
        mask_with(&path, "e4");
        open_mask(&mut rig, &path);
        paths.push(path);
    }
    let recent = rig.state.prefs.mask.recent.clone();
    assert_eq!(recent.len(), 10);
    assert_eq!(recent[0], paths[11]);
    open_mask(&mut rig, &paths[5]);
    assert_eq!(rig.state.prefs.mask.recent[0], paths[5]);
    assert_eq!(
        rig.state
            .prefs
            .mask
            .recent
            .iter()
            .filter(|p| **p == paths[5])
            .count(),
        1
    );
    // A file that is gone stays in the list until it is removed.
    std::fs::remove_file(&paths[11]).unwrap();
    open_mask(&mut rig, &paths[11]);
    assert!(matches!(notice(&rig), Some(MaskNotice::OpenFailed { .. })));
    assert!(rig.state.prefs.mask.recent.contains(&paths[11]));
    m(&mut rig, MaskMessage::RemoveRecent(paths[11].clone()));
    assert!(!rig.state.prefs.mask.recent.contains(&paths[11]));
}

#[test]
fn auto_load_opens_the_newest_recent_when_the_panel_opens() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::TreeMask(
        Box::new(MaskPrefs {
            recent: vec![a.clone()],
            auto_load: true,
            ..MaskPrefs::default()
        }),
    ))));
    assert!(!rig.state.mask_open());
    rig.send(TreeMessage::Open);
    assert_eq!(view(&rig).file_name.as_deref(), Some("a.stm"));
    // Off: the panel opens with no mask.
    let mut rig = Rig::new();
    rig.send(TreeMessage::Open);
    assert!(!rig.state.mask_open());
}

#[test]
fn a_failed_auto_load_is_a_notice_never_a_dialog() {
    let dir = dir();
    let mut rig = Rig::new();
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::TreeMask(
        Box::new(MaskPrefs {
            recent: vec![dir.path().join("gone.stm")],
            auto_load: true,
            ..MaskPrefs::default()
        }),
    ))));
    let host = rig.send(TreeMessage::Open);
    assert!(host.iter().all(|e| !matches!(e, Effect::PickFile(_))));
    assert!(matches!(notice(&rig), Some(MaskNotice::OpenFailed { .. })));
    assert!(!rig.state.mask_open());
    assert_eq!(rig.state.prefs.mask.recent.len(), 1);
}

#[test]
fn the_options_are_saved_and_read_back() {
    let mut rig = Rig::new();
    assert!(rig.state.prefs.mask.show_markers, "D1831: on by default");
    m(&mut rig, MaskMessage::SetShowMarkers(false));
    m(&mut rig, MaskMessage::SetTooltips(false));
    m(&mut rig, MaskMessage::SetAutoLoad(true));
    let prefs = rig.state.prefs.clone();
    assert!(!prefs.mask.show_markers && !prefs.mask.tooltips && prefs.mask.auto_load);
    let text = render_preferences(&prefs);
    assert!(text.contains("[tree_mask]"), "{text}");
    let loaded = load_preferences(&text);
    assert_eq!(loaded.prefs.mask, prefs.mask);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn the_reader_of_the_options_is_tolerant() {
    let base = std::env::temp_dir();
    let (a, b) = (base.join("a.stm"), base.join("b.stm"));
    let text = format!(
        "[tree_mask]\nshow_markers = 3\nauto_load = true\nfuture_key = 1\n\
         recent = ['{a}', '{a}', '{b}']\nlast_dir = 'x'\n",
        a = a.display(),
        b = b.display()
    );
    let loaded = load_preferences(&text);
    let prefs = &loaded.prefs.mask;
    assert!(
        prefs.show_markers,
        "a wrong value falls back to its default"
    );
    assert!(prefs.auto_load);
    assert_eq!(prefs.recent, [a, b]);
    assert_eq!(
        loaded.issues,
        [PrefsIssue::InvalidValue {
            key: "tree_mask.show_markers".to_owned()
        }]
    );
    let loaded = load_preferences("tree_mask = 7\n");
    assert_eq!(*loaded.prefs.mask, MaskPrefs::default());
}

#[test]
fn quit_asks_about_an_altered_mask() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    let host = rig.send(Message::Quit);
    assert!(!host.iter().any(|e| matches!(e, Effect::Exit)));
    assert_eq!(
        rig.state.mask_prompt().map(|p| p.then.clone()),
        Some(MaskPending::Quit)
    );
    // Cancel: the program stays, the work stays.
    m(&mut rig, MaskMessage::Answer(DiscardChoice::Cancel));
    assert!(rig.state.mask_prompt().is_none());
    assert!(dirty(&rig));
    // Quit again: asked again; "don't save" quits.
    rig.send(Message::Quit);
    assert!(rig.state.mask_prompt().is_some());
    let host = m(&mut rig, MaskMessage::Answer(DiscardChoice::DontSave));
    assert!(host.iter().any(|e| matches!(e, Effect::Exit)), "{host:?}");
}

#[test]
fn quit_with_save_writes_the_file_first() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "d4", MaskEdit::Add);
    rig.send(Message::Quit);
    let host = m(&mut rig, MaskMessage::Answer(DiscardChoice::Save));
    assert!(host.iter().any(|e| matches!(e, Effect::Exit)), "{host:?}");
    let (read, _) = stm::read(&a).unwrap();
    assert_eq!(
        read.position(&mychess_core::Position::new())
            .unwrap()
            .moves
            .len(),
        2
    );
}

#[test]
fn quit_with_save_of_a_new_mask_asks_for_the_name() {
    let dir = dir();
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    rig.send(Message::Quit);
    let host = m(&mut rig, MaskMessage::Answer(DiscardChoice::Save));
    assert!(matches!(host.as_slice(), [Effect::PickFile(_)]));
    // The user gives up the dialog: the quit is off.
    let host2 = pick(&mut rig, &host, None);
    assert!(!host2.iter().any(|e| matches!(e, Effect::Exit)));
    assert!(dirty(&rig));
    // Again, with a name this time.
    rig.send(Message::Quit);
    let host = m(&mut rig, MaskMessage::Answer(DiscardChoice::Save));
    let host = pick(&mut rig, &host, Some(dir.path().join("n.stm")));
    assert!(host.iter().any(|e| matches!(e, Effect::Exit)), "{host:?}");
    assert!(dir.path().join("n.stm").is_file());
}

#[test]
fn quit_with_a_failed_save_stays_open_with_the_work() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    edit(&mut rig, "d4", MaskEdit::Add);
    std::fs::remove_file(&a).unwrap();
    std::fs::create_dir(&a).unwrap();
    rig.send(Message::Quit);
    let host = m(&mut rig, MaskMessage::Answer(DiscardChoice::Save));
    assert!(!host.iter().any(|e| matches!(e, Effect::Exit)));
    assert!(dirty(&rig));
    assert!(matches!(notice(&rig), Some(MaskNotice::SaveFailed { .. })));
}

#[test]
fn quit_without_an_altered_mask_does_not_ask() {
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    let mut rig = Rig::new();
    let host = rig.send(Message::Quit);
    assert!(host.iter().any(|e| matches!(e, Effect::Exit)));
    let mut rig = Rig::new();
    open_mask(&mut rig, &a);
    let host = rig.send(Message::Quit);
    assert!(host.iter().any(|e| matches!(e, Effect::Exit)));
    assert!(rig.state.mask_prompt().is_none());
}

#[test]
fn quit_asks_about_the_game_first_then_about_the_mask() {
    let mut rig = Rig::new();
    m(&mut rig, MaskMessage::New);
    edit(&mut rig, "e4", MaskEdit::Add);
    rig.play("e2e4");
    rig.send(Message::Quit);
    assert!(rig.state.discard_prompt.is_some());
    assert!(rig.state.mask_prompt().is_none());
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert!(rig.state.mask_prompt().is_some());
    let host = m(&mut rig, MaskMessage::Answer(DiscardChoice::DontSave));
    assert!(host.iter().any(|e| matches!(e, Effect::Exit)));
}

#[test]
fn the_file_dialog_of_open_is_asked_with_its_own_kind() {
    // The dialog kinds exist for the front-end (D1869).
    let mut rig = Rig::new();
    let host = m(&mut rig, MaskMessage::Open(None));
    let Effect::PickFile(spec) = &host[0] else {
        panic!("{host:?}");
    };
    assert_eq!(spec.kind, PickKind::OpenMask);
    let dir = dir();
    let a = dir.path().join("a.stm");
    mask_with(&a, "e4");
    pick(&mut rig, &host, Some(a));
    assert!(rig.state.mask_open());
}
