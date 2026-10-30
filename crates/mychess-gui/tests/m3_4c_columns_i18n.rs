//! Configurable columns, the narrow panel, the progress timer and the
//! translations of the game list actions (spec M3-4, slice 4c: D562, D568,
//! D569), headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m3_4c_common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use m3_4c_common::*;
use mychess_app::{
    AppNotice, BaseError, BasesNotice, ColumnAlign, ColumnSetup, GameListPrefs, ListColumn,
    MAX_COLUMN_CHARS, MIN_COLUMN_CHARS,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::game_list::{
    Column, ColumnEdit, GameListMessage, edit_columns, visible_setups,
};
use mychess_gui::windows::status_bar::app_notice_text;
use proptest::prelude::*;
use rstest::rstest;

fn defaults() -> GameListPrefs {
    GameListPrefs::default()
}

fn shown(prefs: &GameListPrefs) -> Vec<ListColumn> {
    prefs.columns.iter().map(|c| c.column).collect()
}

// ---- editing ----

#[test]
fn showing_a_hidden_column_appends_it_with_its_defaults_and_hiding_removes_it() {
    let added = edit_columns(&defaults(), &ColumnEdit::Toggle(Column::Site));
    assert_eq!(
        added.columns.last(),
        Some(&ColumnSetup::new(ListColumn::Site))
    );
    assert_eq!(added.columns.len(), 11);
    let removed = edit_columns(&added, &ColumnEdit::Toggle(Column::Site));
    assert_eq!(removed, defaults());
    let hidden = edit_columns(&defaults(), &ColumnEdit::Toggle(Column::White));
    assert!(!hidden.shows(ListColumn::White));
    assert_eq!(hidden.columns.len(), 9);
}

#[test]
fn the_last_column_cannot_be_hidden() {
    let mut prefs = defaults();
    for column in shown(&defaults()).into_iter().skip(1) {
        prefs = edit_columns(&prefs, &ColumnEdit::Toggle(Column::of(column)));
    }
    assert_eq!(prefs.columns.len(), 1);
    let again = edit_columns(
        &prefs,
        &ColumnEdit::Toggle(Column::of(prefs.columns[0].column)),
    );
    assert_eq!(again, prefs);
}

#[test]
fn alignment_is_set_for_the_chosen_column_only() {
    let prefs = edit_columns(
        &defaults(),
        &ColumnEdit::Align(Column::Result, ColumnAlign::Center),
    );
    assert_eq!(
        prefs.setup(ListColumn::Result).unwrap().align,
        ColumnAlign::Center
    );
    assert_eq!(
        prefs.setup(ListColumn::White).unwrap().align,
        ColumnAlign::Left
    );
    let right = edit_columns(
        &prefs,
        &ColumnEdit::Align(Column::White, ColumnAlign::Right),
    );
    assert_eq!(
        right.setup(ListColumn::White).unwrap().align,
        ColumnAlign::Right
    );
}

#[rstest]
#[case(14, 16)]
#[case(58, 60)]
#[case(59, 60)]
#[case(60, 60)]
fn wider_adds_two_characters_up_to_the_maximum(#[case] from: u16, #[case] to: u16) {
    let mut prefs = defaults();
    prefs.columns[1].chars = from;
    let out = edit_columns(&prefs, &ColumnEdit::Wider(Column::White));
    assert_eq!(out.setup(ListColumn::White).unwrap().chars, to);
}

#[rstest]
#[case(14, 12)]
#[case(4, 2)]
#[case(3, 2)]
#[case(2, 2)]
fn narrower_takes_two_characters_down_to_the_minimum(#[case] from: u16, #[case] to: u16) {
    let mut prefs = defaults();
    prefs.columns[1].chars = from;
    let out = edit_columns(&prefs, &ColumnEdit::Narrower(Column::White));
    assert_eq!(out.setup(ListColumn::White).unwrap().chars, to);
}

#[test]
fn moving_swaps_neighbours_and_stops_at_the_ends() {
    let right = edit_columns(&defaults(), &ColumnEdit::MoveRight(Column::Number));
    assert_eq!(shown(&right)[..2], [ListColumn::White, ListColumn::Number]);
    let left = edit_columns(&right, &ColumnEdit::MoveLeft(Column::Number));
    assert_eq!(left, defaults());
    assert_eq!(
        edit_columns(&defaults(), &ColumnEdit::MoveLeft(Column::Number)),
        defaults()
    );
    assert_eq!(
        edit_columns(&defaults(), &ColumnEdit::MoveRight(Column::Eco)),
        defaults()
    );
}

#[test]
fn editing_a_hidden_column_changes_nothing() {
    for edit in [
        ColumnEdit::Wider(Column::Site),
        ColumnEdit::Narrower(Column::Site),
        ColumnEdit::MoveLeft(Column::Site),
        ColumnEdit::MoveRight(Column::Site),
        ColumnEdit::Align(Column::Site, ColumnAlign::Center),
    ] {
        assert_eq!(edit_columns(&defaults(), &edit), defaults(), "{edit:?}");
    }
}

#[test]
fn reset_brings_back_the_default_columns_from_anywhere() {
    let mut prefs = defaults();
    prefs = edit_columns(&prefs, &ColumnEdit::Toggle(Column::Site));
    prefs = edit_columns(&prefs, &ColumnEdit::Toggle(Column::White));
    prefs = edit_columns(&prefs, &ColumnEdit::Wider(Column::Black));
    assert_ne!(prefs, defaults());
    assert_eq!(edit_columns(&prefs, &ColumnEdit::Reset), defaults());
}

fn edit_strategy() -> impl Strategy<Value = ColumnEdit> {
    let column = (0usize..22).prop_map(|i| Column::of(ListColumn::ALL[i]));
    prop_oneof![
        column.clone().prop_map(ColumnEdit::Toggle),
        (column.clone(), 0usize..3).prop_map(|(c, a)| ColumnEdit::Align(
            c,
            [ColumnAlign::Left, ColumnAlign::Center, ColumnAlign::Right][a]
        )),
        column.clone().prop_map(ColumnEdit::Wider),
        column.clone().prop_map(ColumnEdit::Narrower),
        column.clone().prop_map(ColumnEdit::MoveLeft),
        column.prop_map(ColumnEdit::MoveRight),
        Just(ColumnEdit::Reset),
    ]
}

proptest! {
    #[test]
    fn any_sequence_of_edits_keeps_a_sane_setup(edits in proptest::collection::vec(edit_strategy(), 0..60)) {
        let mut prefs = defaults();
        for edit in &edits {
            prefs = edit_columns(&prefs, edit);
            prop_assert!(!prefs.columns.is_empty());
            let mut seen = BTreeSet::new();
            for c in &prefs.columns {
                prop_assert!(seen.insert(c.column.name()), "{:?} twice", c.column);
                prop_assert!((MIN_COLUMN_CHARS..=MAX_COLUMN_CHARS).contains(&c.chars));
            }
        }
    }
}

// ---- the 22 columns of the screen ----

#[test]
fn the_screen_columns_map_to_the_application_columns_both_ways() {
    let mut keys = BTreeSet::new();
    for column in ListColumn::ALL {
        let screen = Column::of(column);
        assert_eq!(screen.list_column(), column);
        assert_eq!(screen.chars(), column.default_chars(), "{column:?}");
        assert!(
            keys.insert(screen.label_key()),
            "label key shared by two columns"
        );
        assert!(screen.label_key().starts_with("games-col-"));
    }
    assert_eq!(keys.len(), 22);
}

// ---- the narrow panel ----

#[test]
fn below_760_px_only_the_elo_columns_leave_the_setup() {
    let mut prefs = defaults();
    prefs = edit_columns(&prefs, &ColumnEdit::Toggle(Column::Site));
    prefs = edit_columns(&prefs, &ColumnEdit::Toggle(Column::Round));
    let wide = visible_setups(&prefs.columns, 760.0);
    assert_eq!(wide, prefs.columns, "760 px is wide enough");
    let narrow = visible_setups(&prefs.columns, 759.9);
    let names: Vec<ListColumn> = narrow.iter().map(|c| c.column).collect();
    assert!(!names.contains(&ListColumn::WhiteElo));
    assert!(!names.contains(&ListColumn::BlackElo));
    assert_eq!(names.len(), prefs.columns.len() - 2);
    assert!(names.contains(&ListColumn::Site) && names.contains(&ListColumn::Round));
    // Order is kept.
    let kept: Vec<ListColumn> = shown(&prefs)
        .into_iter()
        .filter(|c| names.contains(c))
        .collect();
    assert_eq!(kept, names);
}

#[test]
fn the_header_follows_the_width_of_the_panel() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    assert!(
        has_text(&rig, "WElo") && has_text(&rig, "BElo"),
        "wide panel"
    );
    viewport(&mut rig, 700.0, 400.0);
    assert!(
        !has_text(&rig, "WElo") && !has_text(&rig, "BElo"),
        "narrow panel"
    );
    assert!(has_text(&rig, "White") && has_text(&rig, "Black") && has_text(&rig, "ECO"));
}

#[test]
fn the_header_shows_the_configured_columns_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    assert!(!has_text(&rig, "Site"));
    list(
        &mut rig,
        GameListMessage::Columns(ColumnEdit::Toggle(Column::Site)),
    );
    assert!(has_text(&rig, "Site"), "a shown column gets a header");
    list(
        &mut rig,
        GameListMessage::Columns(ColumnEdit::Toggle(Column::Eco)),
    );
    assert!(!has_text(&rig, "ECO"), "a hidden column loses it");
    list(&mut rig, GameListMessage::Columns(ColumnEdit::Reset));
    assert!(has_text(&rig, "ECO") && !has_text(&rig, "Site"));
}

#[test]
fn the_column_setup_is_an_application_preference_that_asks_to_be_saved() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    list(
        &mut rig,
        GameListMessage::Columns(ColumnEdit::Toggle(Column::Site)),
    );
    assert!(rig.shell.app.prefs.game_list.shows(ListColumn::Site));
    assert!(
        rig.shell.app.prefs_dirty,
        "the change is marked to be saved"
    );
}

// ---- progress timer ----

#[test]
fn a_timer_ticks_only_while_a_list_job_runs() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    let idle = rig.shell.subscription().units();
    let mut effects = mychess_app::update(
        &mut rig.shell.app,
        mychess_app::Message::Bases(mychess_app::BasesMessage::List(
            mychess_app::ListMessage::SortBy {
                slot: slot(1),
                column: ListColumn::White,
            },
        )),
    );
    assert!(rig.shell.app.list_job().is_some());
    assert_eq!(rig.shell.subscription().units(), idle + 1, "progress timer");
    let answer = b::runtime().block_on(mychess_app::perform(effects.remove(0)));
    rig.app_msg(answer);
    assert!(rig.shell.app.list_job().is_none());
    assert_eq!(rig.shell.subscription().units(), idle, "no job, no timer");
}

// ---- translations ----

fn ftl_keys(language: &str, file: &str) -> BTreeMap<String, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut out = BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(" = ")
            && !key.starts_with('#')
            && !key.contains(' ')
        {
            out.insert(key.to_owned(), value.to_owned());
        }
    }
    out
}

fn all_keys(language: &str) -> BTreeMap<String, String> {
    let mut out = ftl_keys(language, "games.ftl");
    out.extend(ftl_keys(language, "status.ftl"));
    out
}

fn variables(text: &str) -> BTreeSet<String> {
    text.split("{ $")
        .skip(1)
        .filter_map(|rest| rest.split([' ', '}']).next())
        .map(str::to_owned)
        .collect()
}

#[test]
fn games_and_base_keys_exist_in_both_languages_with_the_same_variables() {
    let en = all_keys("en-US");
    let pt = all_keys("pt-BR");
    let relevant = |k: &&String| k.starts_with("games-") || k.starts_with("base-");
    let en_keys: BTreeSet<&String> = en.keys().filter(relevant).collect();
    let pt_keys: BTreeSet<&String> = pt.keys().filter(relevant).collect();
    assert_eq!(en_keys, pt_keys, "same keys in en-US and pt-BR");
    for key in &en_keys {
        assert_eq!(
            variables(&en[*key]),
            variables(&pt[*key]),
            "variables of {key}"
        );
        assert!(!pt[*key].trim().is_empty(), "{key} is empty in pt-BR");
    }
}

#[test]
fn the_22_column_headers_have_text_in_both_languages() {
    for language in ["en-US", "pt-BR"] {
        let keys = ftl_keys(language, "games.ftl");
        for column in ListColumn::ALL {
            let key = Column::of(column).label_key();
            assert!(
                keys.get(key).is_some_and(|v| !v.trim().is_empty()),
                "{language}: {key}"
            );
        }
    }
}

#[test]
fn every_games_or_base_key_used_by_the_code_exists() {
    let en = all_keys("en-US");
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let app_src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mychess-app/src");
    let mut used = BTreeSet::new();
    for root in [src, app_src] {
        collect_literals(&root, &mut used);
    }
    assert!(used.len() > 40, "the scan found the keys: {}", used.len());
    for key in used {
        assert!(
            en.contains_key(&key),
            "key {key:?} is used in the code but not defined"
        );
    }
}

fn collect_literals(dir: &Path, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_literals(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            for piece in text.split('"').skip(1).step_by(2) {
                let ok = (piece.starts_with("games-") || piece.starts_with("base-"))
                    && piece
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                    && !piece.ends_with('-');
                // Widget ids share the prefix but are not Fluent keys.
                let widget_id = ["games-header", "games-list", "games-pane"].contains(&piece);
                if ok && !widget_id {
                    out.insert(piece.to_owned());
                }
            }
        }
    }
}

#[test]
fn every_notice_and_error_of_the_slots_has_a_message_in_both_languages() {
    let notices = [
        BasesNotice::NoFreeSlot,
        BasesNotice::OpenInProgress,
        BasesNotice::AlreadyInClipbase,
        BasesNotice::ClipbaseEmpty,
        BasesNotice::NotActivated,
        BasesNotice::GameNotLoaded,
        BasesNotice::ListBusy,
        BasesNotice::NotFound,
        BasesNotice::Filtered { removed: 3 },
        BasesNotice::Copied { count: 4 },
        BasesNotice::ReadOnly,
        BasesNotice::NothingSelected,
        BasesNotice::UnreadableEntries { count: 5 },
        BasesNotice::Failed(BaseError::AlreadyOpen),
        BasesNotice::Failed(BaseError::ReadOnly),
        BasesNotice::Failed(BaseError::ClipbaseLimit { limit: 2 }),
        BasesNotice::Failed(BaseError::Cancelled),
        BasesNotice::Failed(BaseError::GameUnreadable { number: 1 }),
        BasesNotice::Failed(BaseError::Internal),
    ];
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in &notices {
            let key = notice.message_key();
            let text = app_notice_text(&i18n, &AppNotice::Bases(notice.clone()));
            assert!(
                !text.is_empty() && text != key && !text.contains("{ $"),
                "{language:?}: {key} -> {text:?}"
            );
        }
    }
}

#[test]
fn counted_notices_carry_their_numbers_in_both_languages() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let removed = app_notice_text(
            &i18n,
            &AppNotice::Bases(BasesNotice::Filtered { removed: 37 }),
        );
        assert!(removed.contains("37"), "{removed}");
        let copied = app_notice_text(&i18n, &AppNotice::Bases(BasesNotice::Copied { count: 12 }));
        assert!(copied.contains("12"), "{copied}");
        let unreadable = app_notice_text(
            &i18n,
            &AppNotice::Bases(BasesNotice::UnreadableEntries { count: 9 }),
        );
        assert!(unreadable.contains('9'), "{unreadable}");
    }
}

#[test]
fn portuguese_panel_shows_translated_headers_and_buttons_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    let path = numbered(dir.path(), "a.pgn", "Branco", 20);
    open(&mut rig, &path);
    viewport(&mut rig, 1280.0, 400.0);
    let pt = I18n::new(Language::PtBr);
    for column in [Column::White, Column::Black, Column::Result, Column::Date] {
        let label = pt.tr(column.label_key());
        assert!(has_text(&rig, &label), "pt-BR header {label}");
    }
    assert!(has_text(&rig, &pt.tr("games-ignore-case")));
    assert!(
        !has_text(&rig, "Ignore case"),
        "no English left in the toolbar"
    );
}
