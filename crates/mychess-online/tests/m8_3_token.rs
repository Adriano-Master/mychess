//! Contract tests of the token and its storage (spec m8-3 D1307, D1343, D1350;
//! acceptance criteria on `FileStore`, `MemoryStore` and the token never being in
//! `Debug` or in errors; risk R3).

#![allow(clippy::assert_is_empty, clippy::type_complexity)]

mod m8_3_support;

use std::{sync::Arc, time::Duration};

use m8_3_support::{GAME, SECRET, fake};
use mychess_online::{
    Client, ClientConfig, Error, FileStore, MemoryStore, Token, TokenStore,
    models::{ChallengeParams, SeekParams},
    testing::{Response, Step},
};

fn tok(s: &str) -> Token {
    Token::new(s).expect("valid token")
}

fn assert_clean(text: &str, what: &str) {
    assert!(!text.contains(SECRET), "{what} leaks the token: {text}");
    assert!(
        !text.contains("SuperSecret"),
        "{what} leaks part of the token: {text}"
    );
}

// ------------------------------------------------------------------ Token

#[test]
fn token_accepts_only_letters_digits_and_underscore_up_to_1024() {
    for good in [
        "a",
        "lip_Abc123",
        "A_B_C",
        "0",
        &"x".repeat(512),
        &"x".repeat(600),
        &"x".repeat(1024),
    ] {
        assert!(Token::new(good).is_ok(), "{good:.20}");
    }
    for bad in [
        "",
        "   ",
        "\n",
        "has space",
        "tab\tinside",
        "new\nline",
        "semi;colon",
        "dash-ed",
        "dot.ted",
        "slash/ed",
        "per%cent",
        "quo\"te",
        "n\0ul",
        "t\u{f3}ken",
        "\u{ff10}\u{ff11}", // full-width digits are not ASCII
        &"x".repeat(1025),
        &"x".repeat(100_000),
    ] {
        assert!(
            matches!(Token::new(bad), Err(Error::InvalidArgument { .. })),
            "{bad:.20?}"
        );
    }
}

#[test]
fn token_surrounding_whitespace_is_trimmed_and_the_length_applies_after_trimming() {
    assert_eq!(tok("  lip_abc \r\n").expose(), "lip_abc");
    assert!(Token::new(&format!("  {}  ", "x".repeat(1024))).is_ok());
}

#[test]
fn token_duplicate_and_equality() {
    let a = tok("lip_one");
    let b = a.duplicate();
    assert_eq!(a, b);
    assert_eq!(b.expose(), "lip_one");
    assert_ne!(a, tok("lip_two"));
}

#[test]
fn token_debug_and_errors_are_redacted() {
    let t = tok(SECRET);
    for text in [
        format!("{t:?}"),
        format!("{t:#?}"),
        format!("{:?}", Some(&t)),
        format!("{:?}", vec![t.duplicate()]),
        format!("{:?}", Ok::<_, Error>(t.duplicate())),
    ] {
        assert_clean(&text, "Token Debug");
    }
    assert_eq!(format!("{t:?}"), "Token(<redacted>)");
    for rejected in [
        format!("{SECRET} with space"),
        format!("{SECRET}{}", "!".repeat(5)),
        format!("{SECRET}{}", "x".repeat(1100)),
    ] {
        let e = Token::new(&rejected).expect_err("rejected");
        assert_clean(&format!("{e} {e:?}"), "Token::new error");
    }
}

// ------------------------------------------------- shared TokenStore contract

fn contract(store: &dyn TokenStore) {
    assert!(store.load("bob").expect("empty load").is_none());
    store.delete("bob").expect("deleting nothing is fine");

    store.save("bob", &tok("lip_one")).expect("save");
    assert_eq!(store.load("bob").expect("load"), Some(tok("lip_one")));
    assert!(store.load("alice").expect("other login").is_none());
    assert!(
        store.load("Bob").expect("case differs").is_none(),
        "logins are compared exactly"
    );

    store.save("bob", &tok("lip_two")).expect("replace");
    assert_eq!(store.load("bob").expect("load"), Some(tok("lip_two")));

    store.delete("alice").expect("other login is a no-op");
    assert_eq!(store.load("bob").expect("load"), Some(tok("lip_two")));

    store.delete("bob").expect("delete");
    assert!(store.load("bob").expect("load").is_none());
    store.delete("bob").expect("delete twice");

    let long = tok(&"k".repeat(1024));
    store.save("bob", &long).expect("max size token");
    assert_eq!(store.load("bob").expect("load"), Some(long));
    store.delete("bob").expect("cleanup");
}

#[test]
fn memory_store_follows_the_contract() {
    contract(&MemoryStore::new());
}

#[test]
fn file_store_follows_the_contract() {
    let dir = tempfile::tempdir().expect("tempdir");
    contract(&FileStore::at(dir.path().join("lichess-token")));
}

#[test]
fn stores_work_as_trait_objects_across_threads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stores: Vec<Arc<dyn TokenStore>> = vec![
        Arc::new(MemoryStore::new()),
        Arc::new(FileStore::at(dir.path().join("t"))),
    ];
    for store in stores {
        let s = store.clone();
        std::thread::spawn(move || s.save("bob", &tok("lip_thread")).expect("save"))
            .join()
            .expect("join");
        assert_eq!(store.load("bob").expect("load"), Some(tok("lip_thread")));
    }
}

#[test]
fn memory_store_keeps_one_login_at_a_time() {
    // Spec D1307: the store holds the session's token only.
    let s = MemoryStore::new();
    s.save("bob", &tok("lip_bob")).expect("save");
    s.save("alice", &tok("lip_alice")).expect("save");
    assert_eq!(s.load("alice").expect("load"), Some(tok("lip_alice")));
    // Whatever happened to bob, he must never get alice's token.
    assert_ne!(s.load("bob").expect("load"), Some(tok("lip_alice")));
}

#[test]
fn memory_store_debug_hides_the_secret() {
    let s = MemoryStore::new();
    s.save("bob", &tok(SECRET)).expect("save");
    assert_clean(&format!("{s:?} {s:#?}"), "MemoryStore Debug");
}

// -------------------------------------------------------------- FileStore

#[test]
fn file_store_writes_two_lines_login_then_token() {
    let dir = tempfile::tempdir().expect("tempdir");
    let s = FileStore::at(dir.path().join("lichess-token"));
    s.save("bob", &tok("lip_x")).expect("save");
    assert_eq!(
        std::fs::read_to_string(s.path()).expect("read"),
        "bob\nlip_x\n"
    );
    assert_eq!(FileStore::FILE_NAME, "lichess-token");
}

#[test]
fn file_store_in_data_dir_is_named_and_absolute() {
    if let Some(s) = FileStore::in_data_dir() {
        assert_eq!(
            s.path().file_name().and_then(|n| n.to_str()),
            Some("lichess-token")
        );
        assert!(s.path().is_absolute());
        // Must not be exercised for I/O here: it is the user's real directory.
    }
}

#[test]
fn file_store_rejects_logins_that_would_corrupt_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let s = FileStore::at(dir.path().join("t"));
    for login in ["", "a\nb", "a\rb", "\n", "bob\nlip_evil"] {
        assert!(
            matches!(
                s.save(login, &tok("lip_x")),
                Err(Error::InvalidArgument { .. })
            ),
            "{login:?}"
        );
    }
    assert!(!s.path().exists());
}

#[test]
fn file_store_creates_missing_parent_directories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let s = FileStore::at(dir.path().join("a").join("b").join("c").join("t"));
    s.save("bob", &tok("lip_x")).expect("save");
    assert_eq!(s.load("bob").expect("load"), Some(tok("lip_x")));
}

#[test]
fn file_store_corrupt_or_foreign_files_mean_no_token_not_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let s = FileStore::at(dir.path().join("t"));
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("one line", b"bob\n".to_vec()),
        ("no newline at all", b"bob".to_vec()),
        ("bad token chars", b"bob\nnot a token!\n".to_vec()),
        ("other login", b"alice\nlip_x\n".to_vec()),
        ("binary garbage", vec![0xff, 0xfe, 0x00, 0x80, 0x0a, 0xff]),
        ("blank token", b"bob\n\n".to_vec()),
        ("crlf endings", b"bob\r\nlip_x\r\n".to_vec()),
    ];
    for (name, bytes) in cases {
        write_private(s.path(), &bytes);
        let got = s.load("bob");
        match name {
            "crlf endings" => assert_eq!(got.expect("load"), Some(tok("lip_x")), "{name}"),
            _ => assert_eq!(got.expect("no error for content problems"), None, "{name}"),
        }
    }
}

#[test]
fn file_store_delete_removes_own_and_unreadable_files_but_keeps_foreign_ones() {
    let dir = tempfile::tempdir().expect("tempdir");
    let s = FileStore::at(dir.path().join("t"));
    write_private(s.path(), b"alice\nlip_x\n");
    s.delete("bob").expect("delete");
    assert!(s.path().exists(), "another login's file is kept");
    write_private(s.path(), b"bob\nlip_x\n");
    s.delete("bob").expect("delete");
    assert!(!s.path().exists());
    write_private(s.path(), &[0xff, 0xfe, 0x00]);
    s.delete("bob").expect("delete garbage");
    assert!(!s.path().exists(), "unreadable leftovers are removed");
}

#[test]
fn file_store_never_leaves_temporary_files_and_survives_concurrent_writers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("lichess-token");
    let handles: Vec<_> = (0..6)
        .map(|n| {
            let path = path.clone();
            std::thread::spawn(move || {
                let s = FileStore::at(path);
                for i in 0..15 {
                    s.save("bob", &tok(&format!("lip_w{n}_{i}"))).expect("save");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("join");
    }
    let s = FileStore::at(&path);
    let got = s.load("bob").expect("load").expect("a token");
    assert!(got.expose().starts_with("lip_w"), "{}", got.expose());
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(names.len(), 1, "no temporary files left: {names:?}");
}

#[test]
fn file_store_replacement_is_atomic_for_concurrent_readers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("lichess-token");
    let s = FileStore::at(&path);
    s.save("bob", &tok("lip_first")).expect("first");
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let (stop, path) = (stop.clone(), path.clone());
        std::thread::spawn(move || {
            let s = FileStore::at(path);
            let mut reads = 0u32;
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                match s.load("bob") {
                    Ok(Some(_)) => reads += 1,
                    other => panic!("a reader saw a torn or missing file: {other:?}"),
                }
            }
            reads
        })
    };
    for i in 0..200 {
        let t = format!("lip_{}_{i}", "z".repeat(if i % 2 == 0 { 900 } else { 5 }));
        s.save("bob", &tok(&t)).expect("save");
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(reader.join().expect("reader") > 0);
}

#[test]
fn file_store_errors_never_carry_the_token_or_its_file_content() {
    let dir = tempfile::tempdir().expect("tempdir");
    let s = FileStore::at(dir.path().join("t"));
    // Too large: a valid first line followed by padding.
    let mut big = format!("bob\n{SECRET}\n").into_bytes();
    big.extend(std::iter::repeat_n(b'#', 5000));
    write_private(s.path(), &big);
    let e = s.load("bob").expect_err("too large");
    assert!(matches!(e, Error::TokenStore(_)));
    assert_clean(&format!("{e} {e:?}"), "oversized file error");

    // A directory in place of the file.
    let sub = FileStore::at(dir.path().join("subdir"));
    std::fs::create_dir(sub.path()).expect("mkdir");
    let e = sub.load("bob").expect_err("directory");
    assert_clean(&format!("{e} {e:?}"), "directory error");
    assert_clean(&format!("{s:?}"), "FileStore Debug");
}

#[test]
fn file_store_refuses_a_file_over_the_size_limit_before_trusting_its_content() {
    let dir = tempfile::tempdir().expect("tempdir");
    let s = FileStore::at(dir.path().join("t"));
    write_private(s.path(), &vec![b'a'; 4096]);
    assert!(matches!(s.load("bob"), Err(Error::TokenStore(_))));
    // After a failed load the next save heals it.
    s.save("bob", &tok("lip_x")).expect("save");
    assert_eq!(s.load("bob").expect("load"), Some(tok("lip_x")));
}

#[test]
fn file_store_a_directory_where_the_file_should_be_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let s = FileStore::at(dir.path().join("t"));
    std::fs::create_dir(s.path()).expect("mkdir");
    assert!(matches!(s.load("bob"), Err(Error::TokenStore(_))));
}

fn write_private(path: &std::path::Path, bytes: &[u8]) {
    std::fs::write(path, bytes).expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    }
}

#[cfg(unix)]
mod unix_only {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    fn mode(p: &std::path::Path) -> u32 {
        std::fs::metadata(p).expect("meta").permissions().mode() & 0o777
    }

    #[test]
    fn file_and_new_directory_are_private() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = dir.path().join("fresh").join("MyChess");
        let s = FileStore::at(data.join("lichess-token"));
        s.save("bob", &tok("lip_x")).expect("save");
        assert_eq!(mode(s.path()), 0o600);
        assert_eq!(
            mode(&data),
            0o700,
            "directories created by the store are 0700"
        );
        s.save("bob", &tok("lip_y")).expect("replace");
        assert_eq!(mode(s.path()), 0o600, "a replaced file is private as well");
    }

    #[test]
    fn a_loose_mode_is_refused_with_a_clear_error_and_save_repairs_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let s = FileStore::at(dir.path().join("t"));
        for m in [0o644, 0o640, 0o660, 0o666, 0o604, 0o400, 0o700, 0o200] {
            std::fs::write(s.path(), format!("bob\n{SECRET}\n")).expect("write");
            std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(m)).expect("chmod");
            let e = s.load("bob");
            // mode 0o200 is unreadable for the owner, which is an error too.
            let err = e.expect_err(&format!("mode {m:o} must be refused"));
            assert!(matches!(err, Error::TokenStore(_)), "{m:o}");
            assert_clean(&format!("{err} {err:?}"), "mode error");
            s.save("bob", &tok("lip_new")).expect("save heals");
            assert_eq!(mode(s.path()), 0o600, "after healing from {m:o}");
            assert_eq!(s.load("bob").expect("load"), Some(tok("lip_new")));
        }
    }

    #[test]
    fn a_symlink_is_refused_on_load_and_never_written_through_on_save() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target");
        write_private(&target, format!("bob\n{SECRET}\n").as_bytes());
        let link = dir.path().join("lichess-token");
        symlink(&target, &link).expect("symlink");
        let s = FileStore::at(&link);

        let err = s.load("bob").expect_err("symlink refused");
        assert!(matches!(err, Error::TokenStore(_)));
        assert_clean(&format!("{err} {err:?}"), "symlink error");

        // Saving replaces the link itself and leaves the target alone.
        s.save("bob", &tok("lip_safe")).expect("save");
        assert!(
            !std::fs::symlink_metadata(&link)
                .expect("meta")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_to_string(&target).expect("target"),
            format!("bob\n{SECRET}\n")
        );
        assert_eq!(s.load("bob").expect("load"), Some(tok("lip_safe")));
    }

    #[test]
    fn a_dangling_symlink_is_refused_not_followed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let link = dir.path().join("lichess-token");
        symlink(dir.path().join("nowhere"), &link).expect("symlink");
        assert!(matches!(
            FileStore::at(&link).load("bob"),
            Err(Error::TokenStore(_))
        ));
    }

    #[test]
    fn deleting_a_symlink_removes_only_the_link() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target");
        write_private(&target, b"bob\nlip_x\n");
        let link = dir.path().join("lichess-token");
        symlink(&target, &link).expect("symlink");
        FileStore::at(&link).delete("bob").expect("delete");
        assert!(target.exists(), "the target must survive");
    }

    #[test]
    fn a_failed_save_keeps_the_previous_token() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = dir.path().join("d");
        std::fs::create_dir(&data).expect("mkdir");
        let s = FileStore::at(data.join("t"));
        s.save("bob", &tok("lip_old")).expect("first");
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o500)).expect("chmod");
        // Skip when the process can write anyway (running as root).
        let can_write_anyway = std::fs::File::create(data.join("probe")).is_ok();
        let result = s.save("bob", &tok("lip_new"));
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).expect("restore");
        if can_write_anyway {
            return;
        }
        let err = result.expect_err("read-only directory");
        assert!(matches!(err, Error::TokenStore(_)));
        assert_eq!(s.load("bob").expect("load"), Some(tok("lip_old")));
    }
}

// ------------------------------------------- redaction sweep on public types

#[tokio::test]
async fn debug_and_display_of_every_public_value_are_free_of_the_token() {
    let fake = fake().await;
    fake.fallback("GET", "/api/account", Response::json(401, "{}"));
    fake.fallback(
        "GET",
        "/api/stream/event",
        Response::stream(vec![Step::Hold]),
    );
    fake.fallback(
        "GET",
        &format!("/api/board/game/stream/{GAME}"),
        Response::stream(vec![Step::Hold]),
    );
    fake.fallback(
        "POST",
        "/api/board/seek",
        Response::stream(vec![Step::Hold]),
    );
    fake.fallback(
        "POST",
        "/api/challenge/alice",
        Response::stream(vec![Step::Hold]),
    );

    let cfg = ClientConfig {
        base_url: fake.base_url(),
        token: Token::new(SECRET).ok(),
        ..ClientConfig::default()
    };
    let mut texts = vec![format!("{cfg:?}"), format!("{cfg:#?}")];
    let c = Client::new(cfg).expect("client");
    texts.push(format!("{c:?}"));
    texts.push(format!("{c:#?}"));
    texts.push(format!("{:?}", c.gate()));
    texts.push(format!("{:?}", c.clone()));

    let ev = c.open_event_stream().expect("events");
    let gm = c.open_game_stream(GAME).expect("game");
    let sk = c.seek(&SeekParams::new(10, 0)).expect("seek");
    let ch = c
        .challenge("alice", &ChallengeParams::new(600, 0))
        .expect("challenge");
    assert!(fake.wait_requests("POST", "/api/challenge/alice", 1).await);
    texts.extend([
        format!("{ev:?}"),
        format!("{gm:?}"),
        format!("{sk:?}"),
        format!("{ch:?}"),
    ]);
    texts.push(format!("{fake:?}"));
    texts.push(format!("{:?}", c.account().await));

    // Errors from every kind of failure of an authenticated client.
    let failures = vec![
        c.account().await.err(),
        c.play_move("short", "e2e4", false).await.err(),
        c.play_move(GAME, "zz", false).await.err(),
        c.user_exists("a/b").await.err(),
        c.decline_challenge("chal0001", "").await.err(),
    ];
    for e in failures.into_iter().flatten() {
        texts.push(format!("{e}"));
        texts.push(format!("{e:?}"));
    }
    for t in texts {
        assert_clean(&t, "public value");
    }
}

#[tokio::test]
async fn failures_of_the_network_stack_never_mention_the_token_or_the_url() {
    let dead = {
        let f = fake().await;
        f.base_url()
    };
    let probe = fake().await;
    let c = m8_3_support::client_with(&probe, |cfg| cfg.base_url = dead.clone());
    let mut texts = Vec::new();
    texts.push(format!("{:?}", c.account().await));
    texts.push(format!("{:?}", c.play_move(GAME, "e2e4", false).await));
    let mut s = c.open_event_stream().expect("stream");
    for _ in 0..3 {
        if let Ok(Some(m)) = tokio::time::timeout(Duration::from_secs(5), s.next()).await {
            texts.push(format!("{m:?}"));
        }
    }
    let mut sk = c.seek(&SeekParams::new(10, 0)).expect("seek");
    texts.push(format!("{:?}", sk.outcome().await));
    for t in texts {
        assert_clean(&t, "network failure text");
        assert!(!t.contains(&dead), "the request URL leaks into: {t}");
    }
}
