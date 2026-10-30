//! Contract tests of the 22 columns and their persistence (spec M3-4, slice
//! 4c: D562; Scid `gamelist.tcl` `glistFields` 9-32).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

use mychess_app::{
    AppState, ColumnAlign, ColumnSetup, GameListPrefs, ListColumn, MAX_COLUMN_CHARS,
    MIN_COLUMN_CHARS, PrefChange, Preferences, PrefsMessage, load_preferences, render_preferences,
    update,
};
use proptest::prelude::*;
use rstest::rstest;

/// Applies a change the way the application does: through `update`.
#[allow(clippy::field_reassign_with_default)]
fn apply(prefs: &mut Preferences, change: PrefChange) {
    let mut state = AppState::default();
    state.prefs = prefs.clone();
    update(&mut state, PrefsMessage::Set(change).into());
    *prefs = state.prefs.clone();
}

// ---- the 22 columns of glistFields ----

/// (Scid code, column, anchor, width): `e` = right, `w` = left.
#[rstest]
#[case('g', ListColumn::Number, ColumnAlign::Right, 7)]
#[case('w', ListColumn::White, ColumnAlign::Left, 14)]
#[case('b', ListColumn::Black, ColumnAlign::Left, 14)]
#[case('r', ListColumn::Result, ColumnAlign::Right, 5)]
#[case('m', ListColumn::Length, ColumnAlign::Right, 5)]
#[case('d', ListColumn::Date, ColumnAlign::Left, 10)]
#[case('e', ListColumn::Event, ColumnAlign::Left, 10)]
#[case('W', ListColumn::WhiteElo, ColumnAlign::Right, 5)]
#[case('B', ListColumn::BlackElo, ColumnAlign::Right, 5)]
#[case('n', ListColumn::Round, ColumnAlign::Right, 5)]
#[case('s', ListColumn::Site, ColumnAlign::Left, 10)]
#[case('D', ListColumn::Deleted, ColumnAlign::Right, 3)]
#[case('V', ListColumn::Variations, ColumnAlign::Right, 3)]
#[case('C', ListColumn::Comments, ColumnAlign::Right, 3)]
#[case('A', ListColumn::Annos, ColumnAlign::Right, 3)]
#[case('o', ListColumn::Eco, ColumnAlign::Right, 5)]
#[case('O', ListColumn::Opening, ColumnAlign::Left, 6)]
#[case('U', ListColumn::Flags, ColumnAlign::Right, 3)]
#[case('S', ListColumn::Start, ColumnAlign::Right, 3)]
#[case('c', ListColumn::Country, ColumnAlign::Right, 3)]
#[case('E', ListColumn::EventDate, ColumnAlign::Left, 7)]
#[case('F', ListColumn::EndMaterial, ColumnAlign::Right, 7)]
fn each_column_has_the_default_alignment_and_width_of_the_scid(
    #[case] code: char,
    #[case] column: ListColumn,
    #[case] align: ColumnAlign,
    #[case] chars: u16,
) {
    assert_eq!(column.default_align(), align, "{code} {column:?}");
    assert_eq!(column.default_chars(), chars, "{code} {column:?}");
    let setup = ColumnSetup::new(column);
    assert_eq!((setup.align, setup.chars), (align, chars));
}

#[test]
fn all_lists_the_22_columns_in_the_order_of_glist_fields() {
    assert_eq!(ListColumn::ALL.len(), 22);
    let order = [
        ListColumn::Number,
        ListColumn::White,
        ListColumn::Black,
        ListColumn::Result,
        ListColumn::Length,
        ListColumn::Date,
        ListColumn::Event,
        ListColumn::WhiteElo,
        ListColumn::BlackElo,
        ListColumn::Round,
        ListColumn::Site,
        ListColumn::Deleted,
        ListColumn::Variations,
        ListColumn::Comments,
        ListColumn::Annos,
        ListColumn::Eco,
        ListColumn::Opening,
        ListColumn::Flags,
        ListColumn::Start,
        ListColumn::Country,
        ListColumn::EventDate,
        ListColumn::EndMaterial,
    ];
    assert_eq!(ListColumn::ALL, order);
}

#[test]
fn the_default_columns_are_the_ten_of_the_scid_order() {
    let prefs = GameListPrefs::default();
    let shown: Vec<ListColumn> = prefs.columns.iter().map(|c| c.column).collect();
    assert_eq!(
        shown,
        [
            ListColumn::Number,
            ListColumn::White,
            ListColumn::Black,
            ListColumn::Result,
            ListColumn::Length,
            ListColumn::Date,
            ListColumn::Event,
            ListColumn::WhiteElo,
            ListColumn::BlackElo,
            ListColumn::Eco,
        ]
    );
    assert!(
        prefs
            .columns
            .iter()
            .all(|c| *c == ColumnSetup::new(c.column)),
        "every default column has its default alignment and width"
    );
    assert_eq!(*Preferences::default().game_list, prefs);
}

#[test]
fn the_names_in_the_file_are_unique_and_round_trip() {
    let mut names = Vec::new();
    for column in ListColumn::ALL {
        assert_eq!(ListColumn::from_name(column.name()), Some(column));
        names.push(column.name());
    }
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 22);
    for align in [ColumnAlign::Left, ColumnAlign::Center, ColumnAlign::Right] {
        assert_eq!(ColumnAlign::from_name(align.name()), Some(align));
    }
    assert_eq!(ColumnAlign::from_name("justify"), None);
    assert_eq!(ListColumn::from_name(""), None);
    assert_eq!(ListColumn::from_name("White"), None, "names are lower case");
}

#[test]
fn width_is_limited_to_the_documented_range() {
    assert_eq!((MIN_COLUMN_CHARS, MAX_COLUMN_CHARS), (2, 60));
    for (given, expected) in [
        (0, 2),
        (1, 2),
        (2, 2),
        (14, 14),
        (60, 60),
        (61, 60),
        (u16::MAX, 60),
    ] {
        let mut setup = ColumnSetup::new(ListColumn::White);
        setup.chars = given;
        assert_eq!(setup.clamped().chars, expected, "{given}");
    }
}

#[test]
fn six_columns_do_not_sort() {
    let not_sortable: Vec<ListColumn> = ListColumn::ALL
        .into_iter()
        .filter(|c| !c.is_sortable())
        .collect();
    // Scid SortBy: Number, Opening, Flags, Annos, Start and EndMaterial do
    // not sort (listed here in the order of glistFields).
    assert_eq!(
        not_sortable,
        [
            ListColumn::Number,
            ListColumn::Annos,
            ListColumn::Opening,
            ListColumn::Flags,
            ListColumn::Start,
            ListColumn::EndMaterial,
        ]
    );
}

// ---- persistence ----

fn custom() -> GameListPrefs {
    let mut site = ColumnSetup::new(ListColumn::Site);
    site.align = ColumnAlign::Center;
    site.chars = 22;
    let mut white = ColumnSetup::new(ListColumn::White);
    white.align = ColumnAlign::Right;
    white.chars = 40;
    GameListPrefs {
        columns: vec![
            ColumnSetup::new(ListColumn::Number),
            site,
            white,
            ColumnSetup::new(ListColumn::EndMaterial),
        ],
    }
}

#[test]
fn a_custom_setup_survives_save_and_load_with_its_order_alignment_and_width() {
    let mut prefs = Preferences::default();
    apply(&mut prefs, PrefChange::GameList(custom()));
    let text = render_preferences(&prefs);
    let loaded = load_preferences(&text);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(*loaded.prefs.game_list, custom());
}

#[test]
fn the_default_setup_survives_save_and_load() {
    let text = render_preferences(&Preferences::default());
    let loaded = load_preferences(&text);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(*loaded.prefs.game_list, GameListPrefs::default());
}

#[test]
fn all_22_columns_with_every_alignment_survive_a_round_trip() {
    let columns: Vec<ColumnSetup> = ListColumn::ALL
        .iter()
        .enumerate()
        .map(|(i, c)| ColumnSetup {
            column: *c,
            align: [ColumnAlign::Left, ColumnAlign::Center, ColumnAlign::Right][i % 3],
            chars: 2 + u16::try_from(i).unwrap() * 2,
        })
        .collect();
    let mut prefs = Preferences::default();
    apply(
        &mut prefs,
        PrefChange::GameList(GameListPrefs {
            columns: columns.clone(),
        }),
    );
    let loaded = load_preferences(&render_preferences(&prefs));
    assert_eq!(loaded.prefs.game_list.columns, columns);
}

#[test]
fn a_file_from_before_the_section_loads_the_default_without_a_remark() {
    let loaded = load_preferences("version = 1\n[board]\nanimation_ms = 120\n");
    assert_eq!(*loaded.prefs.game_list, GameListPrefs::default());
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn an_empty_file_gets_the_default_columns() {
    let loaded = load_preferences("");
    assert_eq!(*loaded.prefs.game_list, GameListPrefs::default());
}

fn sane(prefs: &GameListPrefs) {
    assert!(!prefs.columns.is_empty(), "never empty");
    let mut seen = Vec::new();
    for c in &prefs.columns {
        assert!(!seen.contains(&c.column), "{:?} twice", c.column);
        seen.push(c.column);
        assert!(
            (MIN_COLUMN_CHARS..=MAX_COLUMN_CHARS).contains(&c.chars),
            "{c:?}"
        );
    }
}

#[rstest]
#[case::unknown_column("[[game_list.columns]]\ncolumn = \"nope\"\n")]
#[case::duplicate(
    "[[game_list.columns]]\ncolumn = \"white\"\n[[game_list.columns]]\ncolumn = \"white\"\nchars = 9\n"
)]
#[case::zero_width("[[game_list.columns]]\ncolumn = \"white\"\nchars = 0\n")]
#[case::negative_width("[[game_list.columns]]\ncolumn = \"white\"\nchars = -5\n")]
#[case::huge_width("[[game_list.columns]]\ncolumn = \"white\"\nchars = 4000000000\n")]
#[case::absurd_width("[[game_list.columns]]\ncolumn = \"white\"\nchars = 9223372036854775807\n")]
#[case::width_text("[[game_list.columns]]\ncolumn = \"white\"\nchars = \"wide\"\n")]
#[case::width_float("[[game_list.columns]]\ncolumn = \"white\"\nchars = 7.5\n")]
#[case::unknown_align("[[game_list.columns]]\ncolumn = \"white\"\nalign = \"diagonal\"\n")]
#[case::align_number("[[game_list.columns]]\ncolumn = \"white\"\nalign = 3\n")]
#[case::missing_column("[[game_list.columns]]\nalign = \"left\"\n")]
#[case::column_number("[[game_list.columns]]\ncolumn = 7\n")]
#[case::columns_is_text("[game_list]\ncolumns = \"white\"\n")]
#[case::columns_is_number("[game_list]\ncolumns = 3\n")]
#[case::section_is_number("game_list = 5\n")]
#[case::section_is_array("game_list = [1, 2, 3]\n")]
#[case::empty_columns("[game_list]\ncolumns = []\n")]
#[case::entries_not_tables("[game_list]\ncolumns = [1, \"x\", [2]]\n")]
#[case::unknown_keys(
    "[game_list]\nfoo = 1\n[[game_list.columns]]\ncolumn = \"date\"\nbar = true\n"
)]
fn a_damaged_game_list_section_is_sanitized_without_a_panic(#[case] text: &str) {
    let loaded = load_preferences(text);
    sane(&loaded.prefs.game_list);
}

#[test]
fn absurd_widths_in_the_file_are_clamped() {
    let loaded = load_preferences(
        "[[game_list.columns]]\ncolumn = \"white\"\nchars = 99999\n\
         [[game_list.columns]]\ncolumn = \"black\"\nchars = 1\n",
    );
    let columns = &loaded.prefs.game_list.columns;
    assert_eq!(columns[0].chars, MAX_COLUMN_CHARS);
    assert_eq!(columns[1].chars, MIN_COLUMN_CHARS);
}

#[test]
fn unreadable_file_text_keeps_the_default_columns() {
    let loaded = load_preferences("[[game_list.columns\ncolumn = \n");
    assert_eq!(*loaded.prefs.game_list, GameListPrefs::default());
}

#[test]
fn applying_a_bad_change_never_stores_a_broken_setup() {
    let mut prefs = Preferences::default();
    let mut big = ColumnSetup::new(ListColumn::Site);
    big.chars = u16::MAX;
    apply(
        &mut prefs,
        PrefChange::GameList(GameListPrefs {
            columns: vec![big, ColumnSetup::new(ListColumn::Site)],
        }),
    );
    // Whatever is stored, the saved file loads back sane.
    let loaded = load_preferences(&render_preferences(&prefs));
    sane(&loaded.prefs.game_list);
}

proptest! {
    #[test]
    fn any_sanitized_setup_round_trips_through_the_file(
        picks in proptest::collection::vec((0usize..22, 0usize..3, 0u16..200), 1..30)
    ) {
        let raw = GameListPrefs {
            columns: picks
                .into_iter()
                .map(|(c, a, w)| ColumnSetup {
                    column: ListColumn::ALL[c],
                    align: [ColumnAlign::Left, ColumnAlign::Center, ColumnAlign::Right][a],
                    chars: w,
                })
                .collect(),
        };
        let clean = raw.sanitized();
        sane(&clean);
        let mut prefs = Preferences::default();
        apply(&mut prefs, PrefChange::GameList(clean.clone()));
        let loaded = load_preferences(&render_preferences(&prefs));
        prop_assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        prop_assert_eq!(&*loaded.prefs.game_list, &clean);
    }

    #[test]
    fn sanitizing_is_idempotent_and_keeps_the_first_of_each_column(
        picks in proptest::collection::vec((0usize..22, 0u16..200), 0..30)
    ) {
        let raw = GameListPrefs {
            columns: picks
                .iter()
                .map(|(c, w)| {
                    let mut s = ColumnSetup::new(ListColumn::ALL[*c]);
                    s.chars = *w;
                    s
                })
                .collect(),
        };
        let once = raw.clone().sanitized();
        prop_assert_eq!(once.clone().sanitized(), once.clone());
        if let Some((c, w)) = picks.first() {
            let first = once.setup(ListColumn::ALL[*c]).expect("first is kept");
            prop_assert_eq!(first.chars, (*w).clamp(MIN_COLUMN_CHARS, MAX_COLUMN_CHARS));
        }
    }
}
