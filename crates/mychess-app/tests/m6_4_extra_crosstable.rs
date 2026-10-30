//! Extra contract tests of the crosstable controller (spec
//! `m6-4-tabela-estatisticas-rastreador.md`, section 2.1, rules T27, T28,
//! T30, D3352, D3357, D3363, D3369, D3410, D3412, D3414, D3420).
#![allow(
    let_underscore_drop,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_4_common;

use std::collections::BTreeSet;
use std::path::Path;

use m6_4_common::*;
use mychess_app::crosstable::{
    CrossFlag, CrossFormat, CrosstableBody, CrosstableNotice, ExportFormat,
};
use mychess_app::{GameInfoMessage, TagField};

fn compute_effects(effects: &[Effect]) -> Vec<Effect> {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Crosstable(CrosstableEffect::Compute { .. })))
        .cloned()
        .collect()
}

fn event_of(rig: &Rig) -> Option<String> {
    match rig.state.crosstable_view().body {
        CrosstableBody::Table(t) => Some(t.event.clone()),
        _ => None,
    }
}

fn names_in(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

/// Five games of `Club`/`Rio`, with the rounds `3`, `3.1`, `?`, empty and `2`.
fn rounds_base() -> [Spec; 5] {
    let g = |round, white, black| -> Spec {
        (
            "Club",
            "Rio",
            "2020.05.01",
            round,
            white,
            black,
            "1-0",
            2400,
            2300,
            "1. e4 e5 1-0",
        )
    };
    [
        g("3", "A", "B"),
        g("3.1", "B", "C"),
        g("?", "C", "A"),
        g("", "A", "B"),
        g("2", "B", "C"),
    ]
}

#[test]
fn rounds_are_the_number_at_the_start_of_the_tag_and_text_is_round_zero() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "r.pgn", &rounds_base());
    rig.load(slot, 1);
    rig.send(CrosstableMessage::Open);
    rig.send(CrosstableMessage::SetFilter(Some(3)));
    assert_eq!(rig.filter_numbers(slot), [1, 2], "3 and 3.1 are round 3");
    rig.send(CrosstableMessage::SetFilter(Some(0)));
    assert_eq!(rig.filter_numbers(slot), [3, 4], "? and empty are round 0");
    rig.send(CrosstableMessage::SetFilter(Some(2)));
    assert_eq!(rig.filter_numbers(slot), [5]);
    // A round nobody played empties the filter (it is a "set").
    rig.send(CrosstableMessage::SetFilter(Some(99)));
    assert_eq!(rig.filter_numbers(slot), Vec::<u32>::new());
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::FilterSet)
    );
}

#[test]
fn add_to_filter_adds_without_clearing_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "r.pgn", &rounds_base());
    rig.load(slot, 1);
    rig.send(CrosstableMessage::Open);
    rig.send(CrosstableMessage::SetFilter(Some(2)));
    rig.send(CrosstableMessage::AddToFilter(Some(3)));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
    rig.send(CrosstableMessage::AddToFilter(Some(3)));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
    // An empty set of games adds nothing.
    rig.send(CrosstableMessage::AddToFilter(Some(77)));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
    rig.send(CrosstableMessage::AddToFilter(None));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5]);
    // "Set filter" with every round is the whole tournament, not the base.
    rig.send(CrosstableMessage::SetFilter(None));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5]);
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::FilterSet)
    );
}

#[test]
fn set_and_add_to_filter_without_a_table_refuse_with_a_notice() {
    let (mut rig, slot, _dir) = sample();
    // The panel was never opened: no table.
    rig.send(CrosstableMessage::SetFilter(None));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5]);
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::NoTable)
    );
    rig.send(CrosstableMessage::AddToFilter(None));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5]);
}

#[test]
fn set_filter_acts_on_the_database_of_the_table_even_when_it_is_not_the_current_one() {
    let (mut rig, a, dir) = sample();
    // The localizer opens the tournament of game 4 and pins the target.
    rig.send(CrosstableMessage::OpenGame { slot: a, game: 4 });
    let b = rig.open_games(dir.path(), "b.pgn", &GAMES[..2]);
    assert_eq!(rig.state.bases.current(), b);
    rig.fire_delays();
    assert_eq!(event_of(&rig).as_deref(), Some("Open"), "still pinned to A");

    rig.send(CrosstableMessage::SetFilter(None));
    assert_eq!(rig.filter_numbers(a), [4]);
    assert_eq!(
        rig.filter_numbers(b),
        [1, 2],
        "the current base is untouched"
    );
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::FilterSet)
    );
    // The game on screen is marked only when the table's base is the current.
    let CrosstableBody::Table(t) = rig.state.crosstable_view().body else {
        panic!("table");
    };
    let marked = t
        .rows
        .iter()
        .flat_map(|r| r.cells.iter())
        .filter_map(|c| match c {
            mychess_app::crosstable::CrossCellView::Games(g) => Some(g),
            _ => None,
        })
        .flatten()
        .filter(|c| c.current)
        .count();
    assert_eq!(marked, 0);
    // "Update" unpins: base B has no game on screen.
    rig.send(CrosstableMessage::Refresh);
    assert_eq!(
        rig.state.crosstable_view().body,
        CrosstableBody::Empty(CrosstableNotice::NoGame)
    );
}

#[test]
fn a_pinned_target_survives_loading_other_games_and_is_released_by_update() {
    let (mut rig, slot, _dir) = sample();
    rig.send(CrosstableMessage::OpenGame { slot, game: 4 });
    assert_eq!(event_of(&rig).as_deref(), Some("Open"));
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    rig.fire_delays();
    assert_eq!(event_of(&rig).as_deref(), Some("Open"));
    assert!(!rig.state.crosstable_view().following);
    assert!(
        !rig.state.crosstable_view().stale,
        "a pinned table is not stale because the editor moved"
    );
    // Editing the tags of the editor does not move it either.
    rig.send(GameInfoMessage::Open);
    rig.send(GameInfoMessage::SetText(TagField::Event, "Zzz".into()));
    rig.send(GameInfoMessage::Apply);
    rig.fire_delays();
    assert_eq!(event_of(&rig).as_deref(), Some("Open"));
    rig.send(CrosstableMessage::Refresh);
    assert!(rig.state.crosstable_view().following);
}

#[test]
fn opening_a_game_that_does_not_exist_is_refused_and_keeps_the_target() {
    let (mut rig, slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    for game in [0, 6, u32::MAX] {
        rig.send(CrosstableMessage::OpenGame { slot, game });
        assert_eq!(
            rig.state.crosstable_view().notice,
            Some(CrosstableNotice::InvalidGame),
            "{game}"
        );
        assert!(rig.state.crosstable_view().following, "{game}");
    }
    assert_eq!(event_of(&rig).as_deref(), Some("Club"));
}

#[test]
fn editing_the_tags_of_the_editor_recomputes_with_the_new_tags_after_the_debounce() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    assert_eq!(event_of(&rig).as_deref(), Some("Club"));
    rig.send(GameInfoMessage::Open);
    rig.send(GameInfoMessage::SetText(TagField::Event, "Open".into()));
    rig.send(GameInfoMessage::SetText(TagField::Site, "Lima".into()));
    rig.send(GameInfoMessage::Apply);
    // The tags changed: the old table stays on screen, stale, and the
    // recomputation waits for the debounce.
    assert!(rig.state.crosstable_view().stale);
    assert!(
        rig.delays
            .iter()
            .any(|e| matches!(e, Effect::Crosstable(CrosstableEffect::Delay { .. })))
    );
    rig.fire_delays();
    // Open/Lima has no game within three months of 2020.05.01: the editor's
    // tags win over the bytes of the game stored in the base.
    assert_eq!(
        rig.state.crosstable_view().body,
        CrosstableBody::Empty(CrosstableNotice::NoTable)
    );
}

#[test]
fn a_burst_of_changes_computes_once() {
    let (mut rig, slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    for number in [4, 5, 4, 1, 5] {
        rig.send(BasesMessage::LoadGame { slot, number });
    }
    let delays = std::mem::take(&mut rig.delays);
    assert!(delays.len() >= 2, "{}", delays.len());
    let mut computes = 0;
    for delay in &delays {
        let message = Rig::elapsed(delay).unwrap();
        computes += compute_effects(&rig.raw(message)).len();
    }
    assert_eq!(computes, 1, "only the last debounce computes");
}

#[test]
fn an_option_asks_for_the_games_deleted_at_once() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    let effects = rig.raw(CrosstableMessage::SetFlag(CrossFlag::IncludeDeleted, true));
    let found = compute_effects(&effects);
    assert_eq!(found.len(), 1, "an option does not wait: {effects:?}");
    let Effect::Crosstable(CrosstableEffect::Compute { spec, .. }) = &found[0] else {
        unreachable!()
    };
    assert!(spec.options.include_deleted);
    // The same value again changes nothing: no computation.
    let effects = rig.raw(CrosstableMessage::SetFlag(CrossFlag::IncludeDeleted, true));
    assert!(compute_effects(&effects).is_empty());
}

#[test]
fn an_option_changed_while_the_panel_is_closed_is_used_when_it_opens() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(CrosstableMessage::SetFormat(CrossFormat::Swiss));
    assert!(compute_effects(&effects).is_empty(), "closed: no work");
    let effects = rig.raw(CrosstableMessage::Open);
    let found = compute_effects(&effects);
    let Effect::Crosstable(CrosstableEffect::Compute { spec, .. }) = &found[0] else {
        panic!("{effects:?}");
    };
    assert_eq!(spec.options.format, mychess_db::CrosstableFormat::Swiss);
}

#[test]
fn closing_the_pinned_base_leaves_no_table_and_nothing_breaks() {
    let (mut rig, a, dir) = sample();
    let b = rig.open_games(dir.path(), "b.pgn", &GAMES[..2]);
    rig.send(CrosstableMessage::OpenGame { slot: a, game: 4 });
    assert_eq!(event_of(&rig).as_deref(), Some("Open"));
    rig.send(BasesMessage::Close(a));
    assert!(!matches!(
        rig.state.crosstable_view().body,
        CrosstableBody::Table(_)
    ));
    rig.fire_delays();
    // The clicks of a table that is gone are ignored.
    rig.send(CrosstableMessage::LoadGame(4));
    rig.send(CrosstableMessage::SetFilter(None));
    rig.send(CrosstableMessage::AddToFilter(Some(1)));
    assert_eq!(rig.filter_numbers(b), [1, 2]);
    rig.send(CrosstableMessage::Refresh);
    let _ = rig.state.crosstable_view();
}

#[test]
fn clicks_on_games_out_of_range_are_ignored() {
    let (mut rig, slot, _dir) = sample();
    rig.send(CrosstableMessage::LoadGame(2));
    assert_eq!(
        rig.state.bases.slot(slot).unwrap().current_game,
        Some(1),
        "no table yet: ignored"
    );
    rig.send(CrosstableMessage::Open);
    for game in [0, 4, 5, 6, u32::MAX] {
        rig.send(CrosstableMessage::LoadGame(game));
        assert_eq!(
            rig.state.bases.slot(slot).unwrap().current_game,
            Some(1),
            "{game}"
        );
    }
}

#[test]
fn the_text_export_is_the_clipboard_text_in_utf8_with_line_feeds() {
    let (mut rig, _slot, dir) = sample();
    rig.send(CrosstableMessage::Open);
    let path = dir.path().join("t.txt");
    rig.send(CrosstableMessage::Export {
        format: ExportFormat::Text,
        path: path.clone(),
    });
    let bytes = std::fs::read(&path).unwrap();
    assert!(!bytes.contains(&b'\r'), "T30: \\n only");
    let text = String::from_utf8(bytes).expect("UTF-8");
    assert!(
        text.contains('\u{bd}'),
        "the draw is written as a half: {text}"
    );
    assert!(text.ends_with('\n'), "{text:?}");
    let host = rig.send(CrosstableMessage::Copy);
    let copied = host
        .iter()
        .find_map(|e| match e {
            Effect::WriteClipboard(t) => Some(t.clone()),
            _ => None,
        })
        .expect("clipboard");
    assert_eq!(copied, text);
}

#[test]
fn copy_without_a_table_says_so_and_writes_nothing() {
    let mut rig = Rig::new();
    let host = rig.send(CrosstableMessage::Copy);
    assert!(host.is_empty());
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::NoTable)
    );
}

#[test]
fn export_replaces_an_existing_file_and_leaves_no_temporary_behind() {
    let (mut rig, _slot, dir) = sample();
    rig.send(CrosstableMessage::Open);
    let path = dir.path().join("t.html");
    std::fs::write(&path, "x".repeat(100_000)).unwrap();
    let before = names_in(dir.path());
    rig.send(CrosstableMessage::Export {
        format: ExportFormat::Html,
        path: path.clone(),
    });
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("<!DOCTYPE html>") || text.contains("<!DOCTYPE html>"));
    assert!(!text.contains("xxxx"), "the old content is gone");
    assert_eq!(names_in(dir.path()), before, "no stray temporary file");
}

#[test]
fn a_failed_export_changes_nothing_on_disk() {
    let (mut rig, _slot, dir) = sample();
    rig.send(CrosstableMessage::Open);
    // The destination is a directory.
    let target = dir.path().join("out.txt");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep"), "k").unwrap();
    let before = names_in(dir.path());
    rig.send(CrosstableMessage::Export {
        format: ExportFormat::Text,
        path: target.clone(),
    });
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::ExportFailed)
    );
    assert_eq!(names_in(dir.path()), before);
    assert_eq!(std::fs::read_to_string(target.join("keep")).unwrap(), "k");
    // A missing parent: nothing is created.
    let deep = dir.path().join("nope").join("t.tex");
    rig.send(CrosstableMessage::Export {
        format: ExportFormat::Latex,
        path: deep,
    });
    assert_eq!(names_in(dir.path()), before);
    // And the panel still works afterwards.
    let ok = dir.path().join("ok.txt");
    rig.send(CrosstableMessage::Export {
        format: ExportFormat::Text,
        path: ok.clone(),
    });
    assert!(ok.is_file());
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::Exported)
    );
}

#[test]
fn hostile_names_are_escaped_in_html_and_latex() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let spec: [Spec; 1] = [(
        "Ev&<b>",
        "Si%te_#",
        "2020.05.01",
        "1",
        "Tom & <Jerry>",
        "50%_#$ {x}",
        "1-0",
        2000,
        2000,
        "1. e4 1-0",
    )];
    let slot = rig.open_games(dir.path(), "h.pgn", &spec);
    rig.load(slot, 1);
    rig.send(CrosstableMessage::Open);
    let html = dir.path().join("h.html");
    let tex = dir.path().join("h.tex");
    for (format, path) in [(ExportFormat::Html, &html), (ExportFormat::Latex, &tex)] {
        rig.send(CrosstableMessage::Export {
            format,
            path: path.clone(),
        });
    }
    let html = std::fs::read_to_string(html).unwrap();
    assert!(html.contains("Tom &amp; &lt;Jerry&gt;"), "{html}");
    assert!(
        !html.contains("<Jerry>") && !html.contains("Tom & <"),
        "{html}"
    );
    assert!(html.contains("Ev&amp;&lt;b&gt;"), "{html}");
    assert!(html.to_lowercase().contains("charset"), "UTF-8 declared");
    let tex = std::fs::read_to_string(tex).unwrap();
    for needle in ["\\&", "\\%", "\\_", "\\#"] {
        assert!(tex.contains(needle), "{needle}: {tex}");
    }
    assert!(
        !tex.contains("Si%te"),
        "an unescaped % would comment the line"
    );
}

#[test]
fn the_suggested_file_name_is_safe_on_every_platform() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let spec: [Spec; 1] = [(
        "a<b>:c|d?e*f",
        "x/y\\z",
        "2020.05.01",
        "1",
        "A",
        "B",
        "1-0",
        0,
        0,
        "1. e4 1-0",
    )];
    let slot = rig.open_games(dir.path(), "n.pgn", &spec);
    rig.load(slot, 1);
    assert!(
        rig.state
            .crosstable_export_name(ExportFormat::Text)
            .is_none(),
        "no table, no name"
    );
    rig.send(CrosstableMessage::Open);
    for format in [ExportFormat::Text, ExportFormat::Html, ExportFormat::Latex] {
        let name = rig.state.crosstable_export_name(format).expect("name");
        assert!(
            !name.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']),
            "{name}"
        );
        assert!(
            name.ends_with(&format!(".{}", format.extension())),
            "{name}"
        );
        assert!(name.len() > format.extension().len() + 1, "{name}");
    }
    // The name really can be created.
    let name = rig
        .state
        .crosstable_export_name(ExportFormat::Text)
        .unwrap();
    let path = dir.path().join(name);
    rig.send(CrosstableMessage::Export {
        format: ExportFormat::Text,
        path: path.clone(),
    });
    assert!(path.is_file());
}

#[test]
fn quit_cancels_the_computation_and_a_pending_debounce_starts_nothing() {
    let (mut rig, slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    rig.send(BasesMessage::LoadGame { slot, number: 4 });
    let delay = rig.delays.pop().expect("a debounce");
    rig.raw(Message::Quit);
    let effects = rig.raw(Rig::elapsed(&delay).unwrap());
    assert!(
        compute_effects(&effects).is_empty(),
        "a computation after Quit: {effects:?}"
    );
}

#[test]
fn closing_and_reopening_drops_the_answer_of_the_first_computation() {
    let (mut rig, _slot, _dir) = sample();
    let first = rig.raw(CrosstableMessage::Open);
    let old = compute_effects(&first).remove(0);
    rig.raw(CrosstableMessage::Close);
    let second = rig.raw(CrosstableMessage::Open);
    let new = compute_effects(&second).remove(0);
    let answer = rig.perform(old);
    rig.raw(answer);
    assert!(!matches!(
        rig.state.crosstable_view().body,
        CrosstableBody::Table(_)
    ));
    let answer = rig.perform(new);
    rig.raw(answer);
    assert!(matches!(
        rig.state.crosstable_view().body,
        CrosstableBody::Table(_)
    ));
}

#[test]
fn a_late_cancel_after_the_answer_keeps_the_table() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    rig.send(CrosstableMessage::Cancel);
    assert!(matches!(
        rig.state.crosstable_view().body,
        CrosstableBody::Table(_)
    ));
}

#[test]
fn a_base_without_the_game_in_its_own_tournament_and_a_lone_game_have_clear_notices() {
    // A one-game base: two players, a table of one game.
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "one.pgn", &GAMES[3..4]);
    rig.load(slot, 1);
    rig.send(CrosstableMessage::Open);
    let CrosstableBody::Table(t) = rig.state.crosstable_view().body else {
        panic!("table");
    };
    assert_eq!((t.rows.len(), t.game_count), (2, 1));
    // The same base with the pinned target out of range is refused.
    rig.send(CrosstableMessage::OpenGame { slot, game: 2 });
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::InvalidGame)
    );
}

#[test]
fn filter_buttons_refuse_when_the_database_of_the_table_changed_under_it() {
    // D3414: a table of the Clipbase goes stale when a game is added to it;
    // "Set filter" must not mark games of a table that no longer matches.
    let (mut rig, slot, _dir) = sample();
    rig.send(BasesMessage::ClipbaseCopy);
    rig.send(CrosstableMessage::OpenGame {
        slot: SlotId::CLIPBASE,
        game: 1,
    });
    assert!(matches!(
        rig.state.crosstable_view().body,
        CrosstableBody::Table(_)
    ));
    // Another game goes to the Clipbase.
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total(), 2);
    let before = rig.filter_numbers(SlotId::CLIPBASE);
    // The recomputation is still waiting for its debounce.
    assert!(rig.state.crosstable_view().stale);
    rig.send(CrosstableMessage::SetFilter(None));
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::Stale)
    );
    assert_eq!(rig.filter_numbers(SlotId::CLIPBASE), before.clone());
    rig.send(CrosstableMessage::AddToFilter(None));
    assert_eq!(rig.filter_numbers(SlotId::CLIPBASE), before);
    // After the recomputation the buttons work again.
    rig.fire_delays();
    rig.send(CrosstableMessage::SetFilter(None));
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::FilterSet)
    );
}
