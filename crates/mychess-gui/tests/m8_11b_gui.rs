//! M8-11b (GUI, spec D2907, D2914): the sound pack chooser: the shell rescans
//! the packs when the window opens, opens the user's folder through an
//! injected opener, and both languages have the texts.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use mychess_app::Message as AppMessage;
use mychess_app::sound::{PackError, SoundMessage};
use mychess_gui::shell::{Message, Shell};

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

#[allow(clippy::unnecessary_wraps)] // must match `FolderOpener`
fn record(path: &Path) -> std::io::Result<()> {
    OPENED.lock().unwrap().push(path.to_path_buf());
    Ok(())
}

fn ready(path: &str) -> Message {
    Message::App(AppMessage::Sound(SoundMessage::UserFolderReady(Ok(
        PathBuf::from(path),
    ))))
}

#[test]
fn opening_the_window_rescans_the_packs() {
    let mut shell = Shell::new(None);
    let open = Message::App(AppMessage::Sound(SoundMessage::Open));
    let task = shell.update(open);
    assert!(task.units() >= 1, "a ScanPacks effect is launched");
    assert!(shell.app.sound_dialog_open());
}

#[test]
fn the_ready_folder_is_handed_to_the_opener_and_an_error_is_not() {
    let mut shell = Shell::new(None);
    shell.folder_opener = record;
    drop(shell.update(Message::App(AppMessage::Sound(
        SoundMessage::UserFolderReady(Err(PackError::InvalidId)),
    ))));
    drop(shell.update(ready("m8-11b-folder")));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if OPENED
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == Path::new("m8-11b-folder"))
        {
            break;
        }
        assert!(Instant::now() < deadline, "the opener was not called");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(OPENED.lock().unwrap().len(), 1, "only the Ok answer opens");
    assert_eq!(
        shell.app.sound_user_folder(),
        Some(Path::new("m8-11b-folder"))
    );
}

fn keys(language: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("sound.ftl");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, _)| k.trim().to_owned())
        .collect()
}

#[test]
fn the_pack_keys_exist_in_both_languages() {
    let (en, pt) = (keys("en-US"), keys("pt-BR"));
    assert_eq!(en, pt);
    for key in [
        "sound-pack",
        "sound-pack-default",
        "sound-pack-user",
        "sound-pack-covered",
        "sound-pack-complete",
        "sound-pack-loading",
        "sound-pack-error-io",
        "sound-open-folder",
        "sound-user-folder-hint",
        "sound-advanced",
    ] {
        assert!(en.contains(key), "{key}");
    }
}
