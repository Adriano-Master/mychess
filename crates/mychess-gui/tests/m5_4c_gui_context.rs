//! M5-4c GUI contract: the context menu of a row of the table (spec
//! `docs/specs/m5-4-mascaras-de-arvore.md`, D1822, D1830).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_4c_gui_common;

use m5_4c_gui_common::*;
use mychess_app::Nav;
use tempfile::tempdir;

fn context_open(rig: &TreeRig) -> bool {
    rig.rig.shell.ui.dialogs.mask.context.is_some()
}

#[test]
fn a_right_click_on_a_row_opens_the_menu_with_the_items_of_the_scid() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    // The row sends the message the right button produces.
    let row = rig.row("e4");
    let sent_by_row = {
        let mut sim = pane_sim(&rig);
        let bounds = sim.find("e4").unwrap().bounds();
        let at = iced::Point::new(bounds.x + 2.0, bounds.y + 2.0);
        sim.point_at(at);
        drop(sim.simulate([
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: at }),
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Right,
            )),
        ]));
        sim.into_messages().collect::<Vec<_>>()
    };
    let opened = sent_by_row.iter().any(|m| {
        matches!(
            m,
            Message::MaskUi(MaskUiMessage::OpenContext {
                target: MaskTarget::Row(i),
                san,
                in_mask: false,
                ..
            }) if *i == row.index && san == "e4"
        )
    });
    assert!(opened, "{sent_by_row:?}");
    open_context(&mut rig, "e4", false);
    assert!(context_open(&rig));
    for label in [
        "Add to the mask: e4",
        "Remove from the mask: e4",
        "Comment the move",
        "Comment the position",
        "Add this move to the mask  ▸",
        "Marker 1  ▸",
        "Marker 2  ▸",
        "Color  ▸",
        "NAG  ▸",
        "Apply to the line",
    ] {
        assert!(rig.shows(label), "{label}");
    }
}

#[test]
fn without_a_mask_no_context_menu_opens() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    // The panel without a mask never opens the menu (a row with no mask
    // cell, such as `[end]`, draws no right click either).
    mask(&mut rig, MaskMessage::Close);
    open_context(&mut rig, "e4", false);
    assert!(!context_open(&rig), "no mask, no menu");
}

#[test]
fn add_and_remove_are_enabled_by_whether_the_move_is_in_the_mask() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    open_context(&mut rig, "e4", false);
    assert!(rig.click_messages("Remove from the mask: e4").is_empty());
    assert!(!rig.click_messages("Add to the mask: e4").is_empty());
    rig.click("Add to the mask: e4");
    assert!(rig.row("e4").mask.unwrap().in_mask);
    assert!(!context_open(&rig), "the menu closes after the choice");
    open_context(&mut rig, "e4", false);
    assert!(rig.click_messages("Add to the mask: e4").is_empty());
    rig.click("Remove from the mask: e4");
    assert!(!rig.row("e4").mask.unwrap().in_mask);
}

#[test]
fn the_marker_submenu_lists_the_eleven_markers_with_names_and_sets_one() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    open_context(&mut rig, "e4", false);
    rig.click("Marker 2  ▸");
    for name in [
        "Include",
        "Exclude",
        "Main line",
        "Bookmark",
        "White",
        "Black",
        "New line",
        "To be verified",
        "To train",
        "Dubious",
        "To remove",
        "No marker",
    ] {
        assert!(rig.shows(name), "{name}");
    }
    rig.click("To train");
    let cell = rig.row("e4").mask.unwrap();
    assert_eq!(cell.markers, [None, Some(Marker::ToTrain)]);
    assert!(!context_open(&rig));
}

#[test]
fn the_color_and_nag_submenus_set_them_and_none_clears_the_nag() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    open_context(&mut rig, "e4", false);
    rig.click("Color  ▸");
    for name in ["White", "Green", "Yellow", "Blue", "Red"] {
        assert!(rig.shows(name), "{name}");
    }
    rig.click("Red");
    assert_eq!(rig.row("e4").mask.unwrap().color, MaskColor::Red);
    open_context(&mut rig, "e4", false);
    rig.click("NAG  ▸");
    for name in [
        "!! Brilliant",
        "! Good",
        "!? Interesting",
        "?! Dubious",
        "? Poor",
        "?? Blunder",
        "~ Unclear",
        "No NAG",
    ] {
        assert!(rig.shows(name), "{name}");
    }
    rig.click("?? Blunder");
    assert_eq!(rig.row("e4").mask.unwrap().nag, Some(MaskNag::Blunder));
    open_context(&mut rig, "e4", false);
    rig.click("NAG  ▸");
    rig.click("No NAG");
    assert_eq!(rig.row("e4").mask.unwrap().nag, None);
}

#[test]
fn the_submenus_of_marks_are_off_when_the_markers_are_hidden() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    mask(&mut rig, MaskMessage::SetShowMarkers(false));
    open_context(&mut rig, "e4", false);
    for label in ["Marker 1  ▸", "Marker 2  ▸", "Color  ▸", "NAG  ▸"] {
        assert!(rig.click_messages(label).is_empty(), "{label}");
    }
    assert!(
        !rig.click_messages("Add this move to the mask  ▸")
            .is_empty()
    );
}

#[test]
fn add_this_move_lists_every_legal_move_in_columns_and_adds_the_one_clicked() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    open_context(&mut rig, "e4", false);
    rig.click("Add this move to the mask  ▸");
    let legal = rig.app().mask_legal_moves();
    assert_eq!(legal.len(), 20);
    for san in &legal {
        assert!(rig.shows(san), "{san}");
    }
    rig.click("Nc3");
    assert_eq!(mask_view(&rig).only_rows.len(), 1);
    assert_eq!(mask_view(&rig).only_rows[0].san, "Nc3");
}

#[test]
fn a_second_click_on_the_item_of_a_submenu_closes_it() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    open_context(&mut rig, "e4", false);
    rig.click("Color  ▸");
    assert!(rig.shows("Yellow"));
    rig.click("Color  ▸");
    assert!(!rig.shows("Yellow"));
    rig.click("NAG  ▸");
    assert!(rig.shows("Unclear") || rig.shows("~ Unclear"));
    assert!(!rig.shows("Yellow"));
}

fn play_line(rig: &mut TreeRig) {
    rig.play("e2e4");
    rig.play("e7e5");
}

#[test]
fn apply_to_the_line_reaches_the_moves_that_led_to_the_position_in_one_step() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    play_line(&mut rig);
    // The cursor is after 1.e4 e5; the row is a move of the mask table there.
    let row = rig.sans();
    assert!(row.contains(&"Nf3".to_owned()), "{row:?}");
    open_context(&mut rig, "Nf3", false);
    rig.click("Apply to the line");
    assert!(rig.rig.shell.ui.dialogs.mask.apply_line);
    rig.click("Add to the mask: Nf3");
    assert!(
        !rig.rig.shell.ui.dialogs.mask.apply_line,
        "the box turns itself off after the use"
    );
    // The moves before were added too: at the start e4, after 1.e4 e5.
    rig.nav(Nav::Start);
    assert!(rig.row("e4").mask.unwrap().in_mask, "1.e4 was added");
    rig.nav(Nav::Forward(1));
    assert!(rig.row("e5").mask.unwrap().in_mask, "1...e5 was added");
    // One change for "altered": a single save clears it.
    assert!(mask_view(&rig).dirty);
}

#[test]
fn the_command_modifier_on_the_click_does_the_same_without_the_box() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    play_line(&mut rig);
    open_context(&mut rig, "Nf3", true);
    rig.click("Add to the mask: Nf3");
    rig.nav(Nav::Start);
    assert!(rig.row("e4").mask.unwrap().in_mask);
}

#[test]
fn without_the_modifier_only_the_clicked_move_changes() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    play_line(&mut rig);
    open_context(&mut rig, "Nf3", false);
    rig.click("Add to the mask: Nf3");
    rig.nav(Nav::Start);
    assert!(!rig.row("e4").mask.unwrap().in_mask);
}

#[test]
fn escape_and_a_click_outside_close_the_menu_without_touching_the_mask() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    open_context(&mut rig, "e4", false);
    rig.key(named(NamedKey::Escape));
    assert!(!context_open(&rig));
    assert!(!mask_view(&rig).dirty);
    open_context(&mut rig, "e4", false);
    ui(&mut rig, MaskUiMessage::CloseContext);
    assert!(!context_open(&rig));
}

#[test]
fn the_menu_goes_away_with_the_mask() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    open_context(&mut rig, "e4", false);
    mask(&mut rig, MaskMessage::Close);
    assert!(!context_open(&rig));
}

#[test]
fn a_row_only_the_mask_has_has_its_own_menu_and_can_be_removed() {
    let dir = tempdir().unwrap();
    let mut rig = new_mask_rig(dir.path());
    add_san(&mut rig, "Nc3");
    ui(
        &mut rig,
        MaskUiMessage::OpenContext {
            target: MaskTarget::MaskOnly(0),
            san: "Nc3".to_owned(),
            in_mask: true,
            position: iced::Point::new(200.0, 200.0),
            command: false,
        },
    );
    rig.click("Remove from the mask: Nc3");
    assert!(mask_view(&rig).only_rows.is_empty());
}

#[test]
fn the_menu_is_in_portuguese_without_the_word_variacao() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_language(mychess_app::LanguagePref::PtBr);
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.open_panel();
    mask(&mut rig, MaskMessage::New);
    open_context(&mut rig, "e4", false);
    for label in [
        "Adicionar à máscara: e4",
        "Comentar o lance",
        "Comentar a posição",
        "Aplicar à linha",
        "Cor  ▸",
    ] {
        assert!(rig.shows(label), "{label}");
    }
    rig.click("Marcador 1  ▸");
    assert!(rig.shows("Linha principal") && rig.shows("A verificar"));
}
