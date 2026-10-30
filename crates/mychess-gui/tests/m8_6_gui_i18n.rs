//! M8-6c (GUI, spec D2328): the Fluent files of the calculation exercise
//! and the texts chosen from the state of the application.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mychess_app::calvar::{
    Assessment, CalvarFailure, CalvarNotice, CalvarPhase, CalvarStats, Eval, GoodMove, LineReport,
    Report, ScoreParts,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::calvar_text::{
    crucial_text, eval_tip, found_text, line_result_text, meaning_key, missed_text, notice_text,
    parts_text, recent_text, score_text, stats_text,
};

fn ftl(language: &str) -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("calvar.ftl");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with(' '))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (k.trim().to_owned(), v.to_owned()))
        .collect()
}

fn variables(text: &str) -> BTreeSet<String> {
    text.split("{ $")
        .skip(1)
        .filter_map(|rest| rest.split([' ', '}']).next())
        .map(str::to_owned)
        .collect()
}

#[test]
fn both_languages_have_the_same_keys() {
    let (en, pt) = (ftl("en-US"), ftl("pt-BR"));
    assert!(en.len() > 80, "the file was read: {}", en.len());
    assert_eq!(en.keys().collect::<Vec<_>>(), pt.keys().collect::<Vec<_>>());
}

#[test]
fn the_single_line_messages_use_the_same_variables_in_both_languages() {
    let (en, pt) = (ftl("en-US"), ftl("pt-BR"));
    for (key, text) in &en {
        // The messages with a selector are checked by the renderer below.
        if text.is_empty() {
            continue;
        }
        assert_eq!(variables(text), variables(&pt[key]), "variables of {key}");
    }
}

#[test]
fn portuguese_says_variante_never_variacao() {
    for (key, text) in ftl("pt-BR") {
        let lower = text.to_lowercase();
        assert!(
            !lower.contains("variação")
                && !lower.contains("variacao")
                && !lower.contains("variações"),
            "{key}: {text}"
        );
    }
    assert!(ftl("pt-BR")["menu-play-calvar"].contains("variantes"));
}

#[test]
fn every_notice_has_a_text_in_both_languages() {
    let notices = [
        CalvarNotice::Busy,
        CalvarNotice::NoEngine,
        CalvarNotice::BadFen,
        CalvarNotice::NoMoves,
        CalvarNotice::IllegalMove,
        CalvarNotice::UseKeyboard,
        CalvarNotice::LineOpen,
        CalvarNotice::NeedMoreLines,
        CalvarNotice::EmptyLine,
        CalvarNotice::TooManyLines,
        CalvarNotice::LineTooLong,
        CalvarNotice::Locked,
        CalvarNotice::TimedOut,
        CalvarNotice::EngineFailed,
        CalvarNotice::TimeUp,
        CalvarNotice::StoreNotSaved,
        CalvarNotice::StoreUnreadable,
        CalvarNotice::StoreNewer,
        CalvarNotice::CannotAdd,
    ];
    let known = ftl("en-US");
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in notices {
            assert!(known.contains_key(notice.message_key()), "{notice:?}");
            let text = notice_text(notice, &i18n);
            assert!(
                !text.is_empty() && !text.contains("calvar-"),
                "{notice:?}: {text}"
            );
        }
    }
}

#[test]
fn the_seven_evaluations_have_a_meaning_and_a_tip() {
    let known = ftl("en-US");
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for a in Assessment::ALL {
            assert!(known.contains_key(meaning_key(a)));
            let tip = eval_tip(a, &i18n);
            assert!(tip.contains(a.symbol()), "{tip}");
            assert!(!tip.contains("calvar-"), "{tip}");
        }
    }
    let _ = (CalvarPhase::Closed, CalvarFailure::TimeUp);
}

fn report() -> Report {
    let p = mychess_core::Position::new();
    let e4 = p.parse_uci("e2e4").unwrap();
    let d4 = p.parse_uci("d2d4").unwrap();
    let good = |mv, san: &str, cp, found, crucial| GoodMove {
        mv,
        san: san.to_owned(),
        eval: Eval::centipawns(cp),
        mover_cp: cp,
        depth: 14,
        pv: vec![mv],
        found,
        crucial,
    };
    let line = |ok: bool| LineReport {
        san: "1. d4 d5".to_owned(),
        moves: vec![d4],
        assessment: Assessment::Equal,
        first: Some(Eval::centipawns(30)),
        end: ok.then(|| Eval::centipawns(35)),
        engine_assessment: ok.then_some(Assessment::WhiteSlightly),
        good_candidate: ok,
        agree_plies: 2,
        accuracy: 0.5,
        sound: ok,
        depth_credit: 0.4,
        quality: 0.4,
    };
    Report {
        good: vec![
            good(e4, "e4", 40, false, true),
            good(d4, "d4", 30, true, false),
        ],
        lines: vec![line(true), line(false)],
        found: 1,
        good_total: 2,
        score: 55,
        parts: ScoreParts {
            candidates: 0.5,
            accuracy: 0.5,
            quality: 0.4,
            penalty: 15.0,
        },
    }
}

#[test]
fn the_result_is_written_in_both_languages_without_raw_keys() {
    let report = report();
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let texts = [
            score_text(&report, &i18n),
            found_text(&report, &i18n),
            crucial_text(&report, &i18n).expect("crucial"),
            parts_text(&report, &i18n),
            line_result_text(&report.lines[0], &i18n),
            line_result_text(&report.lines[1], &i18n),
        ];
        for text in &texts {
            assert!(
                !text.is_empty() && !text.contains("calvar-") && !text.contains("{ $"),
                "{text}"
            );
        }
        assert!(texts[0].contains("55"));
        assert!(texts[1].contains('1') && texts[1].contains('2'));
        assert!(texts[2].contains("e4") && texts[2].contains("+0.40"));
        assert!(texts[3].contains("15"), "the penalty: {}", texts[3]);
        assert!(texts[4].contains("+=") && texts[4].contains("+0.35"));
        assert!(
            !texts[5].contains("+0."),
            "an unknown line shows no evaluation"
        );
    }
}

#[test]
fn the_penalty_is_left_out_of_the_parts_when_there_is_none() {
    let mut report = report();
    report.parts.penalty = 0.0;
    let en = parts_text(&report, &I18n::new(Language::EnUs));
    let pt = parts_text(&report, &I18n::new(Language::PtBr));
    assert!(
        !en.contains("minus") && !pt.contains("menos"),
        "{en} / {pt}"
    );
}

#[test]
fn the_missed_moves_list_leaves_out_the_crucial_one_already_named() {
    let report = report();
    let i18n = I18n::new(Language::EnUs);
    assert_eq!(missed_text(&report, &i18n), None);
}

#[test]
fn the_counters_and_the_recent_scores_have_texts() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let empty = stats_text(&CalvarStats::default(), &i18n);
        assert!(!empty.is_empty() && !empty.contains("calvar-"));
        let stats = CalvarStats {
            done: 3,
            total_score: 210,
            best_score: 90,
            lines: 9,
            found: 5,
            good: 8,
        };
        let text = stats_text(&stats, &i18n);
        assert!(text.contains("70") && text.contains("90"), "{text}");
        assert_eq!(recent_text(&[], &i18n), None);
        assert!(recent_text(&[70, 80], &i18n).unwrap().contains("70, 80"));
    }
}
