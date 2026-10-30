//! M5-4c GUI contract: what an open mask adds to the table of the tree
//! (spec `docs/specs/m5-4-mascaras-de-arvore.md`, D1827 to D1829, D1831).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::assert_is_empty
)]

mod m5_4c_gui_common;

use m5_4c_gui_common::*;
use tempfile::tempdir;

#[test]
fn without_a_mask_the_table_has_no_marks_and_no_comment_column() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    assert!(pane_shows(&rig, "Mask"), "the menu is always there");
    assert!(!pane_shows(&rig, "Comment"));
    assert!(!pane_shows(&rig, "Mask: (new mask)"));
    let dir = tempdir().unwrap();
    let mut pt = TreeRig::with_language(mychess_app::LanguagePref::PtBr);
    pt.open_pgn(dir.path(), "std.pgn", STD);
    pt.open_panel();
    assert!(pane_shows(&pt, "Máscara"));
    assert!(!pane_shows(&pt, "Mask"));
}

#[test]
fn an_open_mask_shows_its_name_the_dot_of_an_altered_one_and_the_comment_column() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    assert!(pane_shows(&rig, "Mask: (new mask)"));
    assert!(pane_shows(&rig, "Comment"));
    edit_row(&mut rig, "e4", MaskEdit::Add);
    assert!(
        pane_shows(&rig, "Mask: (new mask) • modified"),
        "an altered mask has the mark"
    );
    let file = stm_file(dir.path(), "rep", &[("", "e4")]);
    mask(&mut rig, MaskMessage::Open(Some(file)));
    // The unsaved one asks first; the answer is "don't save".
    rig.app_msg(mychess_app::Message::AnswerDiscard(
        mychess_app::DiscardChoice::DontSave,
    ));
}

#[test]
fn a_file_name_is_the_title_and_a_clean_mask_has_no_dot() {
    let dir = tempdir().unwrap();
    let file = stm_file(dir.path(), "rep", &[("", "e4")]);
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    mask(&mut rig, MaskMessage::Open(Some(file)));
    assert!(pane_shows(&rig, "Mask: rep.stm"));
    assert!(!pane_shows(&rig, "Mask: rep.stm • modified"));
}

#[test]
fn nag_color_markers_and_comment_are_drawn_on_the_row() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(&mut rig, "e4", MaskEdit::Nag(Some(MaskNag::Interesting)));
    edit_row(&mut rig, "e4", MaskEdit::Color(MaskColor::Green));
    edit_row(
        &mut rig,
        "e4",
        MaskEdit::Marker {
            slot: MarkerSlot::First,
            marker: Some(Marker::Include),
        },
    );
    edit_row(
        &mut rig,
        "e4",
        MaskEdit::Comment("book move\nsecond line".to_owned()),
    );
    let mut sim = pane_sim(&rig);
    assert!(sim.find("!?").is_ok(), "the NAG is text, not only a color");
    assert!(
        sim.find("book move…").is_ok(),
        "the first line, with an ellipsis for the rest"
    );
    assert!(sim.find("second line").is_err(), "only the first line");
    let cell = rig.row("e4").mask.expect("a cell");
    assert!(cell.in_mask && cell.comment_more);
}

#[test]
fn the_marks_the_header_and_the_total_line_up_with_the_move_text() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(&mut rig, "e4", MaskEdit::Nag(Some(MaskNag::Good)));
    for size in [
        iced::Size::new(1000.0, 500.0),
        iced::Size::new(360.0, 500.0),
    ] {
        let mut sim = pane_sim_at(&rig, size);
        let header = sim.find("Move").unwrap().bounds().x;
        let total = sim.find("TOTAL").unwrap().bounds().x;
        let row = sim.find("d4").unwrap().bounds().x;
        assert_eq!(header, total, "{size:?}: the total reserves the marks");
        assert_eq!(header, row, "{size:?}: so does a row with no mask cell");
    }
}

#[test]
fn hiding_the_markers_takes_the_group_out_and_keeps_the_comment_column() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    let shown = pane_sim(&rig).find("d4").unwrap().bounds().x;
    mask(&mut rig, MaskMessage::SetShowMarkers(false));
    let hidden = pane_sim(&rig).find("d4").unwrap().bounds().x;
    assert!(
        hidden < shown,
        "the group of marks left: {hidden} < {shown}"
    );
    assert!(pane_shows(&rig, "Comment"));
}

#[test]
fn the_short_display_keeps_the_marks_and_the_comment_column() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(&mut rig, "e4", MaskEdit::Comment("note".to_owned()));
    rig.app_msg(TreeMessage::SetShort(true));
    assert!(pane_shows(&rig, "note"));
    assert!(pane_shows(&rig, "Comment"));
    assert!(!pane_shows(&rig, "ECO"));
}

#[test]
fn a_move_only_the_mask_has_comes_after_the_total_with_dashes() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    add_san(&mut rig, "Nc3");
    let mut sim = pane_sim(&rig);
    let total = y_of(&mut sim, "TOTAL");
    let only = y_of(&mut sim, "Nc3");
    assert!(only > total, "after the TOTAL: {only} > {total}");
    assert!(sim.find("–").is_ok(), "no statistics, dashes");
    let view = mask_view(&rig);
    assert_eq!(view.only_rows.len(), 1);
    assert!(view.only_rows[0].legal);
}

#[test]
fn the_strip_shows_the_first_line_of_the_comment_of_the_position() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    assert!(!pane_shows(&rig, "Open with care"));
    mask(
        &mut rig,
        MaskMessage::SetPositionComment("Open with care\nand patience".to_owned()),
    );
    let mut sim = pane_sim(&rig);
    let strip = y_of(&mut sim, "Open with care…");
    let header = y_of(&mut sim, "Move");
    assert!(strip < header, "above the table: {strip} < {header}");
}

#[test]
fn a_double_click_on_the_strip_opens_the_editor_of_the_comment() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    mask(
        &mut rig,
        MaskMessage::SetPositionComment("Plan A".to_owned()),
    );
    let messages = {
        let mut sim = pane_sim(&rig);
        let bounds = sim.find("Plan A").unwrap().bounds();
        let at = iced::Point::new(bounds.x + 4.0, bounds.y + 2.0);
        sim.point_at(at);
        let click =
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left));
        let release = iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        ));
        drop(sim.simulate([click.clone(), release.clone(), click, release]));
        sim.into_messages().collect::<Vec<_>>()
    };
    assert!(
        sent(
            &messages,
            &Message::MaskUi(MaskUiMessage::OpenPositionComment)
        ),
        "{messages:?}"
    );
}

#[test]
fn the_tooltip_asks_for_the_whole_comment_only_of_the_row_under_the_pointer() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(
        &mut rig,
        "e4",
        MaskEdit::Comment("one\ntwo\nthree".to_owned()),
    );
    let index = rig.row("e4").index;
    let view = rig.view();
    let ctx = mychess_gui::windows::mask::cells::MaskCtx::of(&rig.rig.shell, &view);
    assert!(
        ctx.hover.is_none(),
        "nothing under the pointer: nothing copied"
    );
    ui(
        &mut rig,
        MaskUiMessage::Hover(Some(Hover::Row(MaskTarget::Row(index)))),
    );
    let ctx = mychess_gui::windows::mask::cells::MaskCtx::of(&rig.rig.shell, &view);
    assert_eq!(
        ctx.hover,
        Some((
            Hover::Row(MaskTarget::Row(index)),
            "one\ntwo\nthree".to_owned()
        ))
    );
    // With the tool tips off the text is not even fetched.
    mask(&mut rig, MaskMessage::SetTooltips(false));
    let ctx = mychess_gui::windows::mask::cells::MaskCtx::of(&rig.rig.shell, &view);
    assert!(ctx.hover.is_none());
}

#[test]
fn closing_the_panel_keeps_the_mask_and_the_table_shows_it_again() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(&mut rig, "e4", MaskEdit::Comment("kept".to_owned()));
    rig.open_panel();
    assert!(!rig.panel_open());
    rig.open_panel();
    assert!(pane_shows(&rig, "kept"));
}

#[test]
fn chess960_keeps_the_mask_visible_and_turns_off_what_needs_the_position() {
    use mychess_core::{Game, Position};
    use mychess_gui::windows::mask::cells::MaskCtx;
    use mychess_gui::windows::mask::menu::entries;
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    edit_row(&mut rig, "e4", MaskEdit::Comment("kept".to_owned()));
    let pos = Position::chess960_start(519).unwrap();
    rig.rig
        .shell
        .app
        .session
        .replace_game(Game::from_position(pos));
    // Any message makes the application refresh what the mask says.
    mask(&mut rig, MaskMessage::CloseSearch);
    let view = rig.view();
    let ctx = MaskCtx::of(&rig.rig.shell, &view);
    assert!(!ctx.enabled && ctx.open);
    let menu = entries(&ctx);
    let disabled = menu
        .iter()
        .filter(|entry| {
            matches!(
                entry,
                mychess_gui::windows::menu_bar::MenuEntry::Item { action: None, .. }
            )
        })
        .count();
    assert!(
        disabled >= 4,
        "the fills and the outline are off: {disabled}"
    );
    assert!(
        pane_shows(&rig, "Mask: (new mask) • modified"),
        "still open"
    );
}
