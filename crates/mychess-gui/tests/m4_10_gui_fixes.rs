//! M4-10 review fixes in the GUI: decimal separator of the configuration
//! dialog (D2194) and the stable ids of the review buttons (D2195).
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

mod m4_68_gui_common;

use m4_68_gui_common::*;
use mychess_app::LanguagePref;
use mychess_app::report::ReportMessage;

fn base(language: LanguagePref) -> Rig {
    let mut rig = Rig::with_language(language);
    rig.set_game(game_of(&SCHOLAR));
    let _run = uci_run(&mut rig);
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Report)));
    up(&mut rig, AppMessage::Report(ReportMessage::OpenConfig));
    rig
}

#[test]
fn the_dialog_writes_decimals_with_a_point_in_english() {
    let rig = base(LanguagePref::EnUs);
    assert!(window_has(&rig, "0.5"), "Best: up to 0.5");
    assert!(window_has(&rig, "2.0"), "2 seconds per search");
    assert!(!window_has(&rig, "0,5"));
}

#[test]
fn the_dialog_writes_decimals_with_a_comma_in_portuguese() {
    let rig = base(LanguagePref::PtBr);
    assert!(window_has(&rig, "0,5"), "Melhor: até 0,5");
    assert!(window_has(&rig, "2,0"), "2 segundos por busca");
    assert!(!window_has(&rig, "0.5"));
    assert!(!window_has(&rig, "2.0"));
}
