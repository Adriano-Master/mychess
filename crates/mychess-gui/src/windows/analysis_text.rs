//! The texts of the analysis panel that need arguments or logic: the
//! remarks of the status bar, the information lines, the description of a
//! locked position and the clickable pieces of a PV (spec M4-4).
//!
//! Everything is a pure function over data the application keeps, so the
//! tables of the spec are tested without a window.

use fluent_bundle::FluentArgs;
use mychess_app::{
    AnalysisNotice, AnalysisView, LockedPosition, Terminal, format_nodes, format_score,
    format_seconds, format_wdl, permille_percent,
};
use mychess_core::Color;

use crate::i18n::I18n;

/// What an unknown number looks like (`----` in the Scid).
const UNKNOWN: &str = "----";

/// The text of a remark of the analysis panel for the status bar.
pub fn notice_text(i18n: &I18n, notice: &AnalysisNotice) -> String {
    let mut args = FluentArgs::new();
    match notice {
        AnalysisNotice::LineTruncated(count) => {
            args.set("count", i64::try_from(*count).unwrap_or(i64::MAX));
        }
        AnalysisNotice::ExcludeInvalid(token) => args.set("token", token.as_str()),
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// The two information lines (rule 4): the score with depth, nodes and time,
/// then speed, hash, load, tablebase hits and the move being searched. The
/// second line is `None` for an engine that reports only the first set of
/// numbers (XBoard) and when `uci` is false.
pub fn info_lines(i18n: &I18n, view: &AnalysisView, uci: bool) -> (String, Option<String>) {
    let stats = &view.stats;
    let score = view
        .main_score()
        .map_or_else(|| UNKNOWN.to_owned(), |(s, b)| format_score(s, b));
    let depth = match stats.seldepth {
        Some(sel) if stats.depth > 0 => format!("{}/{sel}", stats.depth),
        _ => stats.depth.to_string(),
    };
    let mut args = FluentArgs::new();
    args.set("score", score);
    args.set("depth", depth);
    args.set("nodes", format_nodes(stats.nodes, stats.time_ms));
    args.set("time", format_seconds(stats.time_ms));
    let first = i18n.tr_args("analysis-info-main", &args);
    if !uci {
        return (first, None);
    }
    let percent = |permille: Option<u16>| {
        permille.map_or_else(
            || UNKNOWN.to_owned(),
            |p| format!("{}%", permille_percent(p)),
        )
    };
    let current = match (&stats.currmove, stats.currmove_number) {
        (Some(mv), Some(n)) => format!("{mv} ({n}/{})", stats.max_move_number),
        (Some(mv), None) => mv.clone(),
        _ => UNKNOWN.to_owned(),
    };
    let mut args = FluentArgs::new();
    args.set(
        "nps",
        stats
            .nps
            .map_or_else(|| UNKNOWN.to_owned(), |n| n.to_string()),
    );
    args.set("hash", percent(stats.hashfull_permille));
    args.set("load", percent(stats.cpuload_permille));
    args.set(
        "tb",
        stats
            .tbhits
            .map_or_else(|| UNKNOWN.to_owned(), |n| n.to_string()),
    );
    args.set("current", current);
    let mut second = i18n.tr_args("analysis-info-extra", &args);
    if let Some(wdl) = view.lines.first().and_then(|l| l.wdl) {
        second.push_str(&format_wdl(wdl));
    }
    (first, Some(second))
}

/// The state words of a terminal position (rule 12 of D450).
pub fn terminal_key(terminal: Terminal) -> &'static str {
    match terminal {
        Terminal::Checkmate => "analysis-state-checkmate",
        Terminal::Stalemate => "analysis-state-stalemate",
    }
}

/// The description of a locked position, as the tooltip of the lock button
/// writes it (`analysis.tcl` 3383-3397): `file, game N, 12... e5` and, in a
/// variation, ` (vardepth K)`.
pub fn lock_description(i18n: &I18n, lock: &LockedPosition) -> String {
    let mv = match &lock.last_move {
        // The move that led here: White's when Black is to move (same
        // number), Black's when White is to move (the number before).
        Some(san) if lock.side == Color::Black => format!("{}.{san}", lock.move_number),
        Some(san) => format!("{}...{san}", lock.move_number.saturating_sub(1)),
        None => i18n.tr("analysis-lock-start"),
    };
    let mut text = match (&lock.source, lock.game_number) {
        (Some(file), Some(game)) => {
            let mut args = FluentArgs::new();
            args.set("file", file.as_str());
            args.set("game", i64::try_from(game).unwrap_or(i64::MAX));
            args.set("move", mv);
            i18n.tr_args("analysis-lock-file-game", &args)
        }
        (None, Some(game)) => {
            let mut args = FluentArgs::new();
            args.set("game", i64::try_from(game).unwrap_or(i64::MAX));
            args.set("move", mv);
            i18n.tr_args("analysis-lock-game", &args)
        }
        _ => mv,
    };
    if lock.variation_depth > 0 {
        let mut args = FluentArgs::new();
        args.set("depth", i64::from(lock.variation_depth));
        text.push(' ');
        text.push_str(&i18n.tr_args("analysis-lock-vardepth", &args));
    }
    text
}

/// The pieces of the text of a PV: each space-separated word and, for a
/// word that holds a move (not a bare move number such as `12.` or `...`),
/// the index of that move in the PV.
///
/// The words come from `PvLine::text`, written by the application as
/// `1.e4 e5 2.Nf3` (or `1. e4 e5` with spaced numbers); a SAN move never
/// holds a dot, so what follows the last dot of a word is the move.
pub fn pv_words(text: &str) -> impl Iterator<Item = (&str, Option<usize>)> {
    let mut next = 0usize;
    text.split(' ').filter(|w| !w.is_empty()).map(move |word| {
        let move_part = word.rsplit('.').next().unwrap_or(word);
        if move_part.is_empty() {
            (word, None)
        } else {
            let index = next;
            next += 1;
            (word, Some(index))
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mychess_app::AnalysisPrefs;
    use mychess_app::engines::{AnalysisTarget, RunId};
    use mychess_core::Position;

    use super::*;
    use crate::i18n::Language;

    fn en() -> I18n {
        I18n::new(Language::EnUs)
    }

    #[test]
    fn words_of_a_pv_tell_moves_from_numbers() {
        let words: Vec<_> = pv_words("1.e4 e5 2.Nf3").collect();
        assert_eq!(
            words,
            [("1.e4", Some(0)), ("e5", Some(1)), ("2.Nf3", Some(2))]
        );
        let spaced: Vec<_> = pv_words("12. ... e5 13. Nf3 Nc6").collect();
        assert_eq!(
            spaced,
            [
                ("12.", None),
                ("...", None),
                ("e5", Some(0)),
                ("13.", None),
                ("Nf3", Some(1)),
                ("Nc6", Some(2))
            ]
        );
        let black: Vec<_> = pv_words("5...Nc6 6.Bb5+").collect();
        assert_eq!(black, [("5...Nc6", Some(0)), ("6.Bb5+", Some(1))]);
        assert_eq!(pv_words("").count(), 0);
    }

    #[test]
    fn notices_carry_their_arguments_in_both_languages() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            let invalid = notice_text(&i18n, &AnalysisNotice::ExcludeInvalid("Zz9".to_owned()));
            assert!(invalid.contains("Zz9"), "{invalid}");
            let cut = notice_text(&i18n, &AnalysisNotice::LineTruncated(7));
            assert!(cut.contains('7'), "{cut}");
        }
    }

    fn view() -> AnalysisView {
        AnalysisView::new(RunId(1), 1, &AnalysisPrefs::default())
    }

    fn every_notice() -> Vec<AnalysisNotice> {
        vec![
            AnalysisNotice::AtGameEnd,
            AnalysisNotice::LineTruncated(2),
            AnalysisNotice::ExcludeInvalid("Zz9".to_owned()),
            AnalysisNotice::ExcludeAll,
            AnalysisNotice::ExcludeUnsupported,
            AnalysisNotice::MultiPvUnsupported,
            AnalysisNotice::Locked,
            AnalysisNotice::LogSaved,
        ]
    }

    #[test]
    fn every_notice_has_a_text_in_both_languages() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for notice in every_notice() {
                let key = notice.message_key();
                assert!(i18n.has(key), "{key} in {language:?}");
                let text = notice_text(&i18n, &notice);
                assert!(!text.is_empty() && text != key, "{key}: {text}");
            }
        }
    }

    /// `AnalysisNotice` is `non_exhaustive`, so a variant added later would
    /// slip past the list above: every `analysis-notice-*` key the
    /// application names must exist in both languages.
    #[test]
    fn every_notice_key_the_application_names_is_translated() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mychess-app/src/analysis/message.rs");
        let text = std::fs::read_to_string(source).expect("the app source");
        let keys: Vec<&str> = text
            .split('"')
            .filter(|piece| piece.starts_with("analysis-notice-"))
            .collect();
        assert!(keys.len() >= every_notice().len(), "{keys:?}");
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for key in &keys {
                assert!(i18n.has(key), "{key} in {language:?}");
            }
        }
        let listed: Vec<&str> = every_notice()
            .iter()
            .map(AnalysisNotice::message_key)
            .collect();
        for key in &keys {
            assert!(listed.contains(key), "{key} is missing from the test list");
        }
    }

    #[test]
    fn unknown_numbers_read_as_four_dashes() {
        let (first, second) = info_lines(&en(), &view(), true);
        assert!(first.contains("Depth: 0"), "{first}");
        let second = second.expect("uci second line");
        assert!(second.contains("NPS: ----"), "{second}");
        assert!(second.contains("Hash: ----"), "{second}");
        assert!(second.contains("Current: ----"), "{second}");
        assert_eq!(info_lines(&en(), &view(), false).1, None);
    }

    #[test]
    fn known_numbers_are_formatted() {
        let mut v = view();
        v.stats.depth = 12;
        v.stats.seldepth = Some(20);
        v.stats.nodes = 1_234_567;
        v.stats.time_ms = 1_230;
        v.stats.nps = Some(987_000);
        v.stats.hashfull_permille = Some(123);
        v.stats.cpuload_permille = Some(400);
        v.stats.tbhits = Some(0);
        v.stats.currmove = Some("Nf3".to_owned());
        v.stats.currmove_number = Some(3);
        v.stats.max_move_number = 32;
        let (first, second) = info_lines(&en(), &v, true);
        assert!(first.contains("Depth: 12/20"), "{first}");
        assert!(first.contains("Time: 1.23"), "{first}");
        let second = second.expect("second");
        for part in [
            "NPS: 987000",
            "Hash: 12%",
            "Load: 40%",
            "TB: 0",
            "Current: Nf3 (3/32)",
        ] {
            assert!(second.contains(part), "{part} in {second}");
        }
    }

    fn lock(side: Color, last: Option<&str>, depth: u32) -> LockedPosition {
        LockedPosition {
            target: Arc::new(AnalysisTarget {
                start: Position::new(),
                moves: Vec::new(),
            }),
            move_number: 12,
            side,
            last_move: last.map(str::to_owned),
            variation_depth: depth,
            source: Some("games.pgn".to_owned()),
            game_number: Some(3),
        }
    }

    #[test]
    fn the_lock_description_names_file_game_move_and_variation() {
        let i18n = en();
        assert_eq!(
            lock_description(&i18n, &lock(Color::Black, Some("e4"), 0)),
            "games.pgn, game 3, 12.e4"
        );
        assert_eq!(
            lock_description(&i18n, &lock(Color::White, Some("e5"), 2)),
            "games.pgn, game 3, 11...e5 (vardepth 2)"
        );
        let mut start = lock(Color::White, None, 0);
        start.source = None;
        start.game_number = None;
        assert_eq!(lock_description(&i18n, &start), "start position");
    }
}
