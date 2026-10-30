//! M3-9 review fixes, GUI side: the plural remarks in both languages with 0,
//! 1 and 5 (pt-BR `one` covers 0 and 1 in CLDR; spec
//! `docs/specs/m3-9-exportar-partidas.md`, D2020).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mychess_app::GamesExportNotice as N;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::export_games_dialog::notice_text;

fn text(language: Language, notice: &N) -> String {
    notice_text(&I18n::new(language), notice)
}

fn exported(count: u64) -> N {
    N::Exported {
        count,
        name: "a.pgn".into(),
    }
}

fn skips(count: u64, skipped: u64) -> N {
    N::ExportedWithSkips {
        count,
        skipped,
        name: "a.pgn".into(),
    }
}

#[test]
fn d2020_exported_has_its_own_text_for_zero_one_and_many() {
    let pt = |n| text(Language::PtBr, &exported(n));
    assert_eq!(pt(0), "Nenhuma partida exportada");
    assert_eq!(pt(1), "Uma partida exportada para a.pgn");
    assert_eq!(pt(5), "5 partidas exportadas para a.pgn");
    let en = |n| text(Language::EnUs, &exported(n));
    assert_eq!(en(0), "0 games exported to a.pgn");
    assert_eq!(en(1), "One game exported to a.pgn");
    assert_eq!(en(5), "5 games exported to a.pgn");
}

#[test]
fn d2020_the_skip_remark_agrees_with_both_counts() {
    let pt = |c, s| text(Language::PtBr, &skips(c, s));
    assert_eq!(
        pt(0, 2),
        "Nenhuma partida exportada para a.pgn; 2 não exportadas"
    );
    assert_eq!(
        pt(1, 1),
        "Uma partida exportada para a.pgn; uma não exportada"
    );
    assert_eq!(
        pt(5, 1),
        "5 partidas exportadas para a.pgn; uma não exportada"
    );
    assert_eq!(
        pt(5, 3),
        "5 partidas exportadas para a.pgn; 3 não exportadas"
    );
    let en = |c, s| text(Language::EnUs, &skips(c, s));
    assert_eq!(en(1, 1), "One game exported to a.pgn; one not exported");
    assert_eq!(en(5, 3), "5 games exported to a.pgn; 3 not exported");
}

#[test]
fn d2020_the_stopped_append_remark_has_zero_one_and_many() {
    let pt = |n| text(Language::PtBr, &N::StoppedAppended { count: n });
    assert_eq!(
        pt(0),
        "Exportação interrompida: nenhuma partida foi acrescentada ao arquivo"
    );
    assert!(pt(1).contains("uma partida"));
    assert!(pt(5).contains("5 partidas"));
    let en = |n| text(Language::EnUs, &N::StoppedAppended { count: n });
    assert!(en(0).contains("0 games"));
    assert!(en(1).contains("one game"));
    assert!(en(5).contains("5 games"));
}

#[test]
fn d2021_the_all_skipped_remark_says_the_file_was_not_changed() {
    let all = |skipped| N::AllSkipped { skipped };
    assert_eq!(
        text(Language::PtBr, &all(1)),
        "A partida não pôde ser exportada: o arquivo não foi alterado"
    );
    assert_eq!(
        text(Language::PtBr, &all(4)),
        "Nenhuma das 4 partidas pôde ser exportada: o arquivo não foi alterado"
    );
    assert_eq!(
        text(Language::EnUs, &all(4)),
        "None of the 4 games could be exported: the file was not changed"
    );
}
