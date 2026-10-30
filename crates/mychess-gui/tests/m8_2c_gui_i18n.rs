//! M8-2c (GUI): every text the new tabs and notices ask for exists in both
//! languages (spec D2511, D2515, D2521-D2522).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mychess_app::tournament::{Reliability, TimeVerdict, TournamentNotice};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::tournament_text::{notice_text, reliability_key};

#[test]
fn verdicts_and_sample_labels_have_texts_in_both_languages() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for verdict in [
            TimeVerdict::NoData,
            TimeVerdict::Respects,
            TimeVerdict::Overruns,
            TimeVerdict::Forfeits,
        ] {
            let text = i18n.tr(verdict.label_key());
            assert!(
                !text.is_empty() && !text.starts_with("tournament-"),
                "{text}"
            );
        }
        for sample in [
            Reliability::Empty,
            Reliability::VeryUncertain,
            Reliability::Low,
            Reliability::Fair,
        ] {
            let text = i18n.tr(reliability_key(sample));
            assert!(
                !text.is_empty() && !text.starts_with("tournament-"),
                "{text}"
            );
        }
    }
}

#[test]
fn the_new_notices_carry_their_arguments() {
    let i18n = I18n::new(Language::EnUs);
    let names = vec!["A".to_owned()];
    let reduced = notice_text(
        &i18n,
        &TournamentNotice::ConcurrencyReduced { from: 16, to: 3 },
        &names,
    );
    assert!(reduced.contains("16") && reduced.contains('3'), "{reduced}");
    let xboard = notice_text(
        &i18n,
        &TournamentNotice::UnsupportedTime("Crafty".to_owned()),
        &names,
    );
    assert!(xboard.contains("Crafty"), "{xboard}");
    let pt = I18n::new(Language::PtBr);
    let failed = notice_text(&pt, &TournamentNotice::DestinationFailed, &names);
    assert!(failed.contains("PGN"), "{failed}");
}

#[test]
fn the_estimate_says_one_game_in_the_singular_in_both_languages() {
    use fluent_bundle::FluentArgs;
    let estimate = |language: Language, key: &str, games: i64| {
        let mut args = FluentArgs::new();
        args.set("games", games);
        args.set("lanes", 2_i64);
        args.set("time", "5min");
        I18n::new(language).tr_args(key, &args)
    };
    for key in ["tournament-estimate", "tournament-estimate-parallel"] {
        assert!(estimate(Language::PtBr, key, 1).starts_with("1 partida,"));
        assert!(estimate(Language::PtBr, key, 0).starts_with("0 partida,"));
        assert!(estimate(Language::PtBr, key, 6).starts_with("6 partidas,"));
        assert!(estimate(Language::EnUs, key, 1).starts_with("1 game,"));
        assert!(estimate(Language::EnUs, key, 6).starts_with("6 games,"));
    }
}

#[test]
fn the_new_remarks_of_the_destination_have_texts() {
    let names = vec!["A".to_owned()];
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in [
            TournamentNotice::DestinationNotReopened,
            TournamentNotice::DestinationUnavailable,
        ] {
            let text = notice_text(&i18n, &notice, &names);
            assert!(
                !text.is_empty() && !text.starts_with("tournament-"),
                "{text}"
            );
        }
    }
}
