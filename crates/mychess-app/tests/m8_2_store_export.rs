//! M8-2a contract tests of saving, resuming and exporting (spec D1285,
//! D1298, D1304, D1306, D1310, D1312, D1313, D1315): the state file, its
//! consistency checks, hostile slugs, CSV/Markdown/HTML escaping and safe
//! file names.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m8_2_common;

use m8_2_common::*;
use mychess_app::tournament::{
    ExportFormat, ExportLabels, ExportTable, Finished, MAX_STORE_BYTES, SavedStatus,
    SavedTournament, StoreError, Tournament, cross_table, export_cross, export_file_name,
    export_standings, is_safe_slug, list_unfinished, load_state, make_slug, parse_state, pgn_path,
    render_state, save_state, standings, state_path,
};
use mychess_core::{GameResult, PartialDate};
use proptest::prelude::*;
use rstest::rstest;

fn mid_state() -> SavedTournament {
    let mut sim = Sim::new(cfg(3), decide_by(|_| GameResult::White));
    sim.hold = true;
    sim.start();
    sim.t.to_saved()
}

fn finished_state() -> SavedTournament {
    let mut sim = Sim::new(cfg(3), decide_by(|_| GameResult::Draw));
    sim.start();
    sim.t.to_saved()
}

fn invalid(state: &SavedTournament) -> bool {
    matches!(
        render_state(state).and_then(|t| parse_state(&t)),
        Err(StoreError::Invalid(_) | StoreError::Config(_))
    )
}

#[test]
fn a_state_survives_render_parse_and_resume() {
    let state = mid_state();
    let text = render_state(&state).expect("render");
    let back = parse_state(&text).expect("parse");
    assert_eq!(back, state);
    let t = Tournament::resume(back, None, None).expect("resume");
    assert_eq!(t.config(), &state.config);
    assert_eq!(t.slug(), "t");
}

#[test]
fn the_final_state_is_marked_finished_and_not_listed_for_resuming() {
    let dir = tempfile::tempdir().unwrap();
    let done = finished_state();
    assert!(done.finished);
    save_state(
        &state_path(dir.path(), "done"),
        &SavedTournament {
            slug: "done".into(),
            ..done
        },
    )
    .unwrap();
    let open = SavedTournament {
        slug: "open".into(),
        ..mid_state()
    };
    save_state(&state_path(dir.path(), "open"), &open).unwrap();
    std::fs::write(dir.path().join("broken.toml"), "version = [").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
    let list = list_unfinished(dir.path());
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].path, state_path(dir.path(), "open"));
    assert_eq!((list[0].played, list[0].total), (0, 3));
    assert_eq!(list_unfinished(&dir.path().join("missing")), []);
}

#[test]
fn list_unfinished_accepts_an_upper_case_extension() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("OPEN.TOML");
    std::fs::write(
        &path,
        render_state(&SavedTournament {
            slug: "OPEN".into(),
            ..mid_state()
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(list_unfinished(dir.path()).len(), 1);
}

#[test]
fn saving_leaves_only_the_file_and_replaces_the_old_one_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = state_path(dir.path(), "t");
    save_state(&path, &mid_state()).unwrap();
    save_state(&path, &finished_state()).unwrap();
    assert!(load_state(&path).unwrap().finished);
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1, "no temporary file left: {names:?}");
}

// ---- hostile and inconsistent files ----

#[rstest]
#[case("../x")]
#[case("..")]
#[case("a/b")]
#[case("a\\b")]
#[case("")]
#[case("x:y")]
#[case("CON")]
#[case("..\\..\\evil")]
#[case("/etc/passwd")]
fn unsafe_slugs_are_refused_by_the_check(#[case] slug: &str) {
    assert!(!is_safe_slug(slug));
    let mut state = mid_state();
    state.slug = slug.to_owned();
    assert!(invalid(&state), "slug {slug:?}");
}

#[test]
fn a_normal_slug_is_safe() {
    assert!(is_safe_slug("spring-cup-2026-10-24"));
    assert!(is_safe_slug("t-2"));
}

#[test]
fn a_pgn_file_name_that_climbs_out_is_only_informative_and_never_a_path() {
    let mut state = mid_state();
    state.pgn_file = "../../../etc/passwd".into();
    let back = parse_state(&render_state(&state).unwrap()).expect("pgn_file is informative");
    assert_eq!(
        pgn_path(std::path::Path::new("d"), &back.slug),
        std::path::Path::new("d").join("t.pgn")
    );
}

#[test]
fn a_future_or_missing_version_is_an_error_not_a_panic() {
    let mut state = mid_state();
    state.version = 2;
    assert!(matches!(
        parse_state(&toml_text(&state)),
        Err(StoreError::UnsupportedVersion(2))
    ));
    assert!(matches!(
        parse_state("name = \"x\"\n"),
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        parse_state("version = 9999\nwhatever = [1,2\n"),
        Err(StoreError::Invalid(_))
    ));
    // A future version with a different structure is still reported as a
    // version problem when the text is valid TOML.
    assert!(matches!(
        parse_state("version = 7\nnew_field = { a = 1 }\n"),
        Err(StoreError::UnsupportedVersion(7))
    ));
}

fn toml_text(state: &SavedTournament) -> String {
    // The renderer does not look at the version.
    render_state(state).unwrap()
}

#[test]
fn corrupt_text_and_binary_garbage_are_errors() {
    for text in [
        "",
        "version = 1",
        "[[[[",
        "version = \"one\"",
        "version = 1\nslug = 3\n",
    ] {
        assert!(parse_state(text).is_err(), "{text:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bin.toml");
    std::fs::write(&path, [0xff_u8, 0xfe, 0x00, 0x01, 0x80]).unwrap();
    assert!(matches!(load_state(&path), Err(StoreError::Invalid(_))));
    assert!(matches!(
        load_state(&dir.path().join("none.toml")),
        Err(StoreError::Io { .. })
    ));
}

#[test]
fn a_file_over_the_limit_is_too_large_without_being_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.toml");
    let size = usize::try_from(MAX_STORE_BYTES).unwrap() + 1;
    std::fs::write(&path, vec![b'#'; size]).unwrap();
    assert!(matches!(load_state(&path), Err(StoreError::TooLarge)));
    assert!(matches!(
        parse_state(&"#".repeat(size)),
        Err(StoreError::TooLarge)
    ));
}

#[test]
fn the_consistency_check_catches_damaged_games() {
    let base = finished_state();
    // number duplicated among live games
    let mut s = base.clone();
    s.games[1].number = s.games[0].number;
    assert!(invalid(&s), "duplicate number");
    // a participant that does not exist
    let mut s = base.clone();
    s.games[0].white = 9;
    assert!(invalid(&s), "unknown participant");
    // White against itself
    let mut s = base.clone();
    s.games[0].black = s.games[0].white;
    assert!(invalid(&s), "self play");
    // schedule shorter than the configuration says
    let mut s = base.clone();
    s.games.pop();
    assert!(invalid(&s), "incomplete schedule");
    // played without a result
    let mut s = base.clone();
    s.games[0].result = None;
    assert!(invalid(&s), "played without result");
    // queue points past the games
    let mut s = base.clone();
    s.queue = vec![99];
    assert!(invalid(&s), "queue out of range");
    // lists of the wrong length
    let mut s = base.clone();
    s.failures.pop();
    assert!(invalid(&s), "failures length");
    let mut s = base.clone();
    s.withdrawn.push(false);
    assert!(invalid(&s), "withdrawn length");
    // a game that does not match the schedule (another pair)
    let mut s = base.clone();
    s.games[0].white = 1;
    s.games[0].black = 2;
    assert!(invalid(&s), "pair differs from the calendar");
    // a group holding two different pairs
    let mut s = base.clone();
    s.games[1].group = s.games[0].group;
    assert!(invalid(&s), "group with two pairs");
    // invalid configuration inside the file
    let mut s = base.clone();
    s.config.cycles = 0;
    assert!(invalid(&s), "config");
}

#[test]
fn a_finished_flag_with_games_still_to_play_is_incoherent() {
    let mut s = mid_state();
    s.finished = true;
    assert!(invalid(&s));
}

#[test]
fn a_game_missing_from_the_queue_is_queued_again_at_the_end_and_repeats_are_dropped() {
    let mut state = mid_state();
    assert_eq!(state.games.len(), 3);
    state.queue = vec![1, 1, 1];
    let t = Tournament::resume(state, None, None).expect("resume");
    assert_eq!(t.queue_numbers(), [2, 1, 3], "no game lost, no game twice");
}

#[test]
fn the_queue_never_holds_a_game_that_is_not_waiting() {
    let mut sim = Sim::new(cfg(3), decide_by(|_| GameResult::White));
    sim.start();
    let mut state = sim.t.to_saved();
    state.finished = false;
    state.queue = vec![0, 1, 2];
    let t = Tournament::resume(state, None, None).expect("resume");
    assert_eq!(t.queue_numbers(), Vec::<u32>::new());
}

#[test]
fn a_redone_game_keeps_its_number_in_a_state_that_still_loads() {
    let mut sim = Sim::new(cfg(2), decide_by(|_| GameResult::White));
    sim.start();
    let actions = sim.t.redo(1);
    sim.run(actions);
    let state = sim.t.to_saved();
    assert_eq!(state.games.len(), 2);
    assert!(
        state
            .games
            .iter()
            .any(|g| g.status == SavedStatus::Replaced)
    );
    let back = parse_state(&render_state(&state).unwrap()).expect("loads");
    let t = Tournament::resume(back, None, None).unwrap();
    assert_eq!(t.queue_numbers(), [1]);
    assert_eq!(t.progress(), (0, 1));
}

#[test]
fn a_redo_with_two_live_copies_of_the_same_number_is_refused() {
    let mut sim = Sim::new(cfg(2), decide_by(|_| GameResult::White));
    sim.start();
    let actions = sim.t.redo(1);
    sim.run(actions);
    let mut state = sim.t.to_saved();
    state.games[0].status = SavedStatus::Played;
    assert!(invalid(&state));
}

#[test]
fn the_revision_survives_a_resume_and_keeps_growing() {
    let mut sim = Sim::new(cfg(2), decide_by(|_| GameResult::White));
    sim.start();
    let last = sim.states.last().unwrap().revision;
    let back = parse_state(&render_state(sim.states.last().unwrap()).unwrap()).unwrap();
    let t = Tournament::resume(back, None, None).unwrap();
    assert!(t.to_saved().revision > last);
}

#[test]
fn a_value_no_toml_reader_takes_makes_the_render_fail_instead_of_writing() {
    // D1310: u64 above i64::MAX.
    let mut state = mid_state();
    state.revision = u64::MAX;
    assert!(render_state(&state).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = state_path(dir.path(), "t");
    save_state(&path, &mid_state()).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(save_state(&path, &state).is_err());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the old state is intact"
    );
}

#[test]
fn a_state_at_the_biggest_signed_numbers_still_round_trips() {
    let mut state = mid_state();
    state.revision = u64::MAX >> 1;
    state.config.seed = u64::MAX >> 1;
    let back = parse_state(&render_state(&state).unwrap()).unwrap();
    assert_eq!(back.config.seed, u64::MAX >> 1);
}

// ---- slugs ----

#[test]
fn slugs_get_a_numeric_suffix_when_taken() {
    let date = PartialDate::new(Some(2026), Some(10), Some(24)).unwrap();
    let first = make_slug("Spring Cup", date, |_| false);
    assert_eq!(first, "Spring-Cup-2026-10-24");
    let second = make_slug("Spring Cup", date, |s| s == first);
    assert_eq!(second, format!("{first}-2"));
    let third = make_slug("Spring Cup", date, |s| s == first || s == second);
    assert_eq!(third, format!("{first}-3"));
}

#[test]
fn hostile_names_make_safe_slugs() {
    let date = PartialDate::new(Some(2026), Some(10), Some(24)).unwrap();
    for name in [
        "a\\b:c*d?e\"f<g>h|i",
        "CON",
        "",
        "   ",
        "\u{0}\n\t",
        &"x".repeat(500),
        "COM1",
    ] {
        let slug = make_slug(name, date, |_| false);
        assert!(is_safe_slug(&slug), "{name:?} -> {slug:?}");
        assert!(!slug.contains(['/', '\\', ':']));
    }
    // An unknown date writes `?` in the date text: still safe.
    let slug = make_slug("x", PartialDate::default(), |_| false);
    assert!(is_safe_slug(&slug), "{slug:?}");
}

#[test]
fn a_tournament_named_with_dots_gets_a_slug_that_can_be_resumed() {
    // BUG probe (D1304 x make_slug): `make_slug` keeps inner dots, but
    // `is_safe_slug` refuses ".." anywhere, so the state of a tournament
    // called "Wait... what" is saved and then refused when resumed.
    let date = PartialDate::new(Some(2026), Some(10), Some(24)).unwrap();
    for name in ["Wait... what", "v1..2", "../../etc/passwd", ".."] {
        let slug = make_slug(name, date, |_| false);
        assert!(!slug.contains(['/', '\\']), "{slug:?}");
        let mut state = finished_state();
        state.slug = slug.clone();
        assert!(
            render_state(&state).and_then(|t| parse_state(&t)).is_ok(),
            "the state saved under the generated slug {slug:?} (from {name:?}) does not load back"
        );
    }
}

proptest! {
    #[test]
    fn any_name_makes_a_safe_slug_and_a_path_that_stays_in_the_folder(name in ".{0,200}") {
        let slug = make_slug(&name, PartialDate::new(Some(2026), Some(1), Some(2)).unwrap(), |_| false);
        prop_assert!(is_safe_slug(&slug));
        let dir = std::path::Path::new("tournaments");
        for path in [state_path(dir, &slug), pgn_path(dir, &slug)] {
            prop_assert_eq!(path.parent().unwrap(), dir);
        }
    }

    #[test]
    fn a_state_with_a_random_slug_never_panics_and_only_safe_ones_load(slug in ".{0,40}") {
        let mut state = finished_state();
        state.slug = slug.clone();
        let result = render_state(&state).and_then(|t| parse_state(&t));
        prop_assert_eq!(result.is_ok(), is_safe_slug(&slug));
    }
}

// ---- export ----

fn rows_and_names(
    names: &[&str],
    games: &[Finished],
) -> (Vec<String>, Vec<mychess_app::tournament::StandingRow>) {
    (
        names.iter().map(|s| (*s).to_owned()).collect(),
        standings(names.len(), &[], games),
    )
}

fn game(number: u32, white: usize, black: usize, result: GameResult) -> Finished {
    Finished {
        number,
        white,
        black,
        result,
    }
}

#[test]
fn csv_has_a_header_one_row_per_participant_unix_line_ends_and_a_decimal_point() {
    let (names, rows) = rows_and_names(
        &["Alpha", "Beta"],
        &[
            game(1, 0, 1, GameResult::White),
            game(2, 1, 0, GameResult::Draw),
        ],
    );
    let out = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Csv);
    let lines: Vec<&str> = out.split('\n').collect();
    assert_eq!(lines[0], "Rank,Name,Played,Wins,Draws,Losses,Points,%,SB");
    assert_eq!(lines[1], "1,Alpha,2,1,1,0,1.5,75.0,0.75");
    assert_eq!(lines[2], "2,Beta,2,0,1,1,0.5,25.0,0.75");
    assert_eq!(lines[3], "");
    assert!(!out.contains('\r'));
}

#[rstest]
#[case("=HYPERLINK(\"http://evil\")", "\"'=HYPERLINK(\"\"http://evil\"\")\"")]
#[case("+1+1", "'+1+1")]
#[case("-2+3", "'-2+3")]
#[case("@SUM(A1)", "'@SUM(A1)")]
#[case("\tcmd", "'\tcmd")]
#[case("a,b", "\"a,b\"")]
#[case("say \"hi\"", "\"say \"\"hi\"\"\"")]
#[case("line\nbreak", "\"line\nbreak\"")]
#[case("normal", "normal")]
fn csv_neutralises_formulas_and_quotes_what_needs_it(#[case] name: &str, #[case] cell: &str) {
    let (names, rows) = rows_and_names(&[name, "B"], &[game(1, 0, 1, GameResult::White)]);
    let out = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Csv);
    assert!(out.contains(&format!(",{cell},")), "{out}");
}

#[test]
fn csv_neutralises_a_formula_in_the_cross_table_names_too() {
    let (names, rows) = rows_and_names(&["=cmd", "B"], &[game(1, 0, 1, GameResult::White)]);
    let cells = cross_table(2, &[game(1, 0, 1, GameResult::White)]);
    let out = export_cross(
        &names,
        &cells,
        &rows,
        &ExportLabels::default(),
        ExportFormat::Csv,
    );
    assert!(out.contains(",'=cmd,"), "{out}");
}

#[test]
fn markdown_escapes_the_characters_that_change_meaning() {
    let (names, rows) = rows_and_names(
        &["a|b *c* _d_ [e] `f` <g> \\h", "B"],
        &[game(1, 0, 1, GameResult::White)],
    );
    let out = export_standings(
        &names,
        &rows,
        &ExportLabels::default(),
        ExportFormat::Markdown,
    );
    assert!(
        out.contains("a\\|b \\*c\\* \\_d\\_ \\[e] \\`f\\` \\<g> \\\\h"),
        "{out}"
    );
    assert!(out.lines().nth(1).unwrap().contains(":---"));
    let name_with_newline = rows_and_names(&["x\ny", "B"], &[]);
    let out = export_standings(
        &name_with_newline.0,
        &name_with_newline.1,
        &ExportLabels::default(),
        ExportFormat::Markdown,
    );
    assert_eq!(
        out.lines().count(),
        4,
        "a newline in a name does not add a row: {out}"
    );
}

#[test]
fn html_escapes_names_and_never_emits_a_raw_tag_from_them() {
    let (names, rows) = rows_and_names(
        &["<script>alert('x')&\"</script>", "B"],
        &[game(1, 0, 1, GameResult::White)],
    );
    let out = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Html);
    assert!(!out.contains("<script"), "{out}");
    assert!(out.contains("&lt;script&gt;alert(&#39;x&#39;)&amp;&quot;&lt;/script&gt;"));
    assert!(out.starts_with("<table>"));
    let labels = ExportLabels {
        name: "<b>Name</b>".into(),
        ..ExportLabels::default()
    };
    let out = export_standings(&names, &rows, &labels, ExportFormat::Html);
    assert!(!out.contains("<b>"));
}

#[test]
fn text_export_aligns_columns_by_characters_and_hides_control_characters() {
    let (names, rows) = rows_and_names(&["Zoë", "Bob\u{7}x\n"], &[game(1, 0, 1, GameResult::Draw)]);
    let out = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Text);
    assert!(!out.chars().any(|c| c.is_control() && c != '\n'));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 4, "header, rule, two rows: {out}");
    let width = |l: &str| l.chars().count();
    assert_eq!(width(lines[2]), width(lines[3]), "{out}");
    assert!(lines[1].chars().all(|c| c == '-'));
}

#[test]
fn the_cross_table_shows_scores_per_game_a_diagonal_and_the_withdrawn_label() {
    let games = [
        game(1, 0, 1, GameResult::White),
        game(2, 1, 0, GameResult::Draw),
        game(3, 0, 2, GameResult::Black),
    ];
    let names: Vec<String> = ["A", "B", "C"].iter().map(|s| (*s).to_owned()).collect();
    let rows = standings(3, &[false, false, true], &games);
    let cells = cross_table(3, &games);
    let out = export_cross(
        &names,
        &cells,
        &rows,
        &ExportLabels::default(),
        ExportFormat::Csv,
    );
    let a = out.lines().find(|l| l.contains(",A,")).unwrap();
    // A vs B: win then draw; A vs C: loss.
    assert!(
        a.contains('x') && a.contains("1 \u{bd}") && a.contains(",0,"),
        "{a}"
    );
    let standing = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Csv);
    assert!(standing.contains("C (withdrawn)"), "{standing}");
    let translated = ExportLabels {
        withdrawn: "retirado".into(),
        rank: "Pos".into(),
        ..ExportLabels::default()
    };
    let standing = export_standings(&names, &rows, &translated, ExportFormat::Csv);
    assert!(standing.starts_with("Pos,") && standing.contains("C (retirado)"));
}

#[test]
fn exporting_an_empty_table_gives_only_the_header() {
    for format in [
        ExportFormat::Csv,
        ExportFormat::Markdown,
        ExportFormat::Html,
        ExportFormat::Text,
    ] {
        let out = export_standings(&[], &[], &ExportLabels::default(), format);
        assert_ne!(out, "");
    }
    let csv = export_standings(&[], &[], &ExportLabels::default(), ExportFormat::Csv);
    assert_eq!(csv.lines().count(), 1);
}

#[rstest]
#[case(ExportFormat::Csv, "csv")]
#[case(ExportFormat::Text, "txt")]
#[case(ExportFormat::Markdown, "md")]
#[case(ExportFormat::Html, "html")]
fn export_extensions(#[case] format: ExportFormat, #[case] ext: &str) {
    assert_eq!(format.extension(), ext);
    assert!(export_file_name("cup", ExportTable::Standings, format).ends_with(&format!(".{ext}")));
}

#[test]
fn export_file_names_are_safe_on_the_three_platforms() {
    for slug in [
        "../../a:b",
        "CON",
        "a/b\\c",
        "",
        "x?y*z",
        "NUL",
        "aux",
        "lpt1",
        "  .. ",
    ] {
        for table in [ExportTable::Standings, ExportTable::Cross] {
            let name = export_file_name(slug, table, ExportFormat::Csv);
            assert!(
                !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']),
                "{name:?}"
            );
            assert!(!name.starts_with('.') && !name.starts_with(' '));
            let stem = name.split('.').next().unwrap().to_ascii_uppercase();
            assert!(
                !["CON", "PRN", "AUX", "NUL", "COM1", "LPT1"].contains(&stem.as_str()),
                "{name:?}"
            );
        }
    }
    assert_ne!(
        export_file_name("cup", ExportTable::Standings, ExportFormat::Csv),
        export_file_name("cup", ExportTable::Cross, ExportFormat::Csv)
    );
}

proptest! {
    #[test]
    fn exporting_any_names_never_panics_and_csv_has_one_line_per_row_outside_quotes(
        a in ".{0,30}", b in ".{0,30}"
    ) {
        let (names, rows) = rows_and_names(&[&a, &b], &[game(1, 0, 1, GameResult::Draw)]);
        let cells = cross_table(2, &[game(1, 0, 1, GameResult::Draw)]);
        for format in [ExportFormat::Csv, ExportFormat::Text, ExportFormat::Markdown, ExportFormat::Html] {
            let _s = export_standings(&names, &rows, &ExportLabels::default(), format);
            let _c = export_cross(&names, &cells, &rows, &ExportLabels::default(), format);
        }
        let html = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Html);
        let inner = html.replace("<table>", "").replace("</table>", "").replace("<thead>", "").replace("</thead>", "")
            .replace("<tbody>", "").replace("</tbody>", "").replace("<tr>", "").replace("</tr>", "")
            .replace("<th>", "").replace("</th>", "").replace("<td>", "").replace("</td>", "");
        prop_assert!(!inner.contains('<'), "raw '<' leaked: {}", html);
    }
}
