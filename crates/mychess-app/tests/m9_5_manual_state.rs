//! The state of the manual window through the public API of the
//! application (spec M9-5, slice b, criteria 7 to 11 and 13).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mychess_app::manual::{
    CHAPTER_FILES, DEFAULT_TEXT_SCALE, MAX_TEXT_SCALE, ManualIndex, ManualMessage, ManualNotice,
    ManualPrefs, SharedIndex,
};
use mychess_app::{AppState, Effect, Message, load_preferences, render_preferences, update};

fn index() -> SharedIndex {
    let mut texts: Vec<String> = (0..15).map(|i| format!("# Capitulo {i}\n")).collect();
    "# Bancos\n\n## Os slots e a Clipbase\n\n## Abrir\n".clone_into(&mut texts[3]);
    "# Atalhos de teclado\n\n## Gerais\n".clone_into(&mut texts[12]);
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    SharedIndex::new(ManualIndex::build(&refs, false))
}

fn send(state: &mut AppState, message: ManualMessage) -> Vec<Effect> {
    update(state, Message::Manual(message))
}

fn open(state: &mut AppState, chapter: usize) {
    send(
        state,
        ManualMessage::Open {
            index: index(),
            chapter,
            anchor: None,
        },
    );
}

#[test]
fn catalog_has_fifteen_chapters_in_reading_order() {
    assert_eq!(CHAPTER_FILES.len(), 15);
    let mut sorted = CHAPTER_FILES;
    sorted.sort_unstable();
    assert_eq!(sorted, CHAPTER_FILES);
}

#[test]
fn a_closed_manual_is_the_default_and_open_shows_the_chapter() {
    let mut state = AppState::default();
    assert!(!state.manual().is_open());
    open(&mut state, 3);
    assert!(state.manual().is_open());
    assert_eq!(state.manual().chapter(), 3);
    send(&mut state, ManualMessage::Close);
    assert!(!state.manual().is_open());
}

#[test]
fn internal_links_navigate_and_back_forward_walk_the_trail() {
    let mut state = AppState::default();
    open(&mut state, 0);
    send(
        &mut state,
        ManualMessage::Link("03-bancos-de-dados.md#abrir".into()),
    );
    assert_eq!(state.manual().chapter(), 3);
    assert_eq!(state.manual().place().anchor.as_deref(), Some("abrir"));
    send(
        &mut state,
        ManualMessage::Link("#os-slots-e-a-clipbase".into()),
    );
    assert_eq!(
        state.manual().place().anchor.as_deref(),
        Some("os-slots-e-a-clipbase")
    );
    send(&mut state, ManualMessage::Back);
    assert_eq!(state.manual().place().anchor.as_deref(), Some("abrir"));
    send(&mut state, ManualMessage::Back);
    assert_eq!(state.manual().chapter(), 0);
    assert!(!state.manual().can_back());
    send(&mut state, ManualMessage::Forward);
    assert_eq!(state.manual().chapter(), 3);
}

#[test]
fn a_link_to_a_missing_heading_or_file_changes_nothing_and_says_so() {
    let mut state = AppState::default();
    open(&mut state, 3);
    let serial = state.manual().nav_serial();
    send(&mut state, ManualMessage::Link("#nao-existe".into()));
    assert_eq!(state.manual().notice(), Some(ManualNotice::LinkNotFound));
    send(&mut state, ManualMessage::Link("../x.md".into()));
    assert_eq!(state.manual().notice(), Some(ManualNotice::LinkNotFound));
    send(&mut state, ManualMessage::Link("file:///etc/passwd".into()));
    assert_eq!(state.manual().notice(), Some(ManualNotice::LinkIgnored));
    assert_eq!(state.manual().chapter(), 3);
    assert_eq!(state.manual().nav_serial(), serial);
}

#[test]
fn external_links_ask_once_and_only_then_emit_the_open_effect() {
    let mut state = AppState::default();
    open(&mut state, 0);
    let effects = send(
        &mut state,
        ManualMessage::Link("https://lichess.org".into()),
    );
    assert!(effects.is_empty(), "nothing opens before the answer");
    assert_eq!(
        state.manual().pending_external(),
        Some("https://lichess.org")
    );
    let effects = send(&mut state, ManualMessage::ConfirmExternal(true));
    assert_eq!(effects, [Effect::OpenUrl("https://lichess.org".into())]);
    let effects = send(
        &mut state,
        ManualMessage::Link("https://lichess.org".into()),
    );
    assert_eq!(effects, [Effect::OpenUrl("https://lichess.org".into())]);
    assert_eq!(state.manual().pending_external(), None);
}

#[test]
fn declining_the_question_opens_nothing_and_a_refusal_is_reported() {
    let mut state = AppState::default();
    open(&mut state, 0);
    send(
        &mut state,
        ManualMessage::Link("https://lichess.org".into()),
    );
    assert_eq!(
        send(&mut state, ManualMessage::ConfirmExternal(false)).len(),
        0
    );
    assert_eq!(state.manual().pending_external(), None);
    send(&mut state, ManualMessage::UrlOpened(false));
    assert_eq!(state.manual().notice(), Some(ManualNotice::UrlRefused));
    send(&mut state, ManualMessage::UrlOpened(true));
    assert_eq!(state.manual().notice(), None);
}

#[test]
fn search_by_title_finds_clipbase_and_atalhos() {
    let mut state = AppState::default();
    open(&mut state, 0);
    send(&mut state, ManualMessage::Query("clipbase".into()));
    let hits = state.manual().results();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        (hits[0].chapter, hits[0].anchor.as_deref()),
        (3, Some("os-slots-e-a-clipbase"))
    );
    send(&mut state, ManualMessage::Query("ATALHOS".into()));
    assert_eq!(state.manual().results()[0].chapter, 12);
    send(&mut state, ManualMessage::PickResult(0));
    assert_eq!(state.manual().chapter(), 12);
    send(&mut state, ManualMessage::PickResult(99));
    assert_eq!(
        state.manual().chapter(),
        12,
        "a result that is not there is ignored"
    );
}

#[test]
fn changing_the_language_keeps_chapter_and_heading_when_they_exist() {
    let mut state = AppState::default();
    open(&mut state, 3);
    send(&mut state, ManualMessage::Link("#abrir".into()));
    let serial = state.manual().nav_serial();
    send(&mut state, ManualMessage::Reindex(index()));
    assert_eq!(state.manual().chapter(), 3);
    assert_eq!(state.manual().place().anchor.as_deref(), Some("abrir"));
    assert!(
        state.manual().nav_serial() > serial,
        "the view is told to scroll again"
    );
    // Another index without that heading: the heading goes, the chapter stays.
    let texts = vec!["# Only\n"; 15];
    send(
        &mut state,
        ManualMessage::Reindex(SharedIndex::new(ManualIndex::build(&texts, true))),
    );
    assert_eq!(state.manual().chapter(), 3);
    assert_eq!(state.manual().place().anchor, None);
    assert!(state.manual().is_fallback());
}

#[test]
fn text_scale_steps_stay_inside_the_range_and_are_kept_in_the_preferences() {
    let mut state = AppState::default();
    assert_eq!(state.prefs.manual.text_scale, DEFAULT_TEXT_SCALE);
    send(&mut state, ManualMessage::Scale(1));
    assert_eq!(state.prefs.manual.text_scale, DEFAULT_TEXT_SCALE + 1);
    for _ in 0..20 {
        send(&mut state, ManualMessage::Scale(1));
    }
    assert_eq!(state.prefs.manual.text_scale, MAX_TEXT_SCALE);
    for _ in 0..20 {
        send(&mut state, ManualMessage::Scale(-1));
    }
    assert_eq!(state.prefs.manual.text_scale, 0);
}

#[test]
fn the_manual_table_of_the_preferences_round_trips_and_rejects_bad_values() {
    let mut prefs = mychess_app::Preferences::default();
    *prefs.manual = ManualPrefs { text_scale: 5 };
    let text = render_preferences(&prefs);
    assert!(text.contains("[manual]"), "{text}");
    let loaded = load_preferences(&text);
    assert_eq!(loaded.prefs.manual.text_scale, 5);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);

    let loaded = load_preferences("[manual]\ntext_scale = 99\n");
    assert_eq!(loaded.prefs.manual.text_scale, DEFAULT_TEXT_SCALE);
    assert_eq!(loaded.issues.len(), 1, "{:?}", loaded.issues);
    let loaded = load_preferences("[manual]\ntext_scale = \"big\"\n");
    assert_eq!(loaded.prefs.manual.text_scale, DEFAULT_TEXT_SCALE);
    assert_eq!(loaded.issues.len(), 1, "{:?}", loaded.issues);
}
