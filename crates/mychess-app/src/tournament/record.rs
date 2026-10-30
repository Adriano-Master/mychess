//! The PGN game of a finished tournament game: tags and comments (spec
//! M8-2, D1280, D1281).
//!
//! Scid: `comp.tcl` 554-560 (tags) and 830-841 (the note as a comment);
//! `comp.uwe` 65-90 (`[%clk]`, `[%emt]`).

use std::time::Duration;

use mychess_core::{Color, Elo, Game, GameError, PartialDate, Rating, RatingType};

use super::adjudicate::signed_pawns;
use super::config::{ScoreComment, TimeControl, TournamentConfig};
use super::game::{GameEnd, MoveScore, PlayedMove, TournamentGame};
use super::openings::Opening;
use super::schedule::Pairing;
use mychess_engine::Score as EngineScore;

/// Who plays, for the tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerInfo {
    /// The name of the participant.
    pub name: String,
    /// The rating.
    pub elo: Option<u16>,
    /// The `id name` the engine announced.
    pub engine_name: Option<String>,
}

/// What a game record is made of.
#[derive(Debug)]
pub struct RecordInput<'a> {
    /// The configuration of the tournament.
    pub config: &'a TournamentConfig,
    /// The game in the schedule.
    pub pairing: &'a Pairing,
    /// The player with White.
    pub white: &'a PlayerInfo,
    /// The player with Black.
    pub black: &'a PlayerInfo,
    /// Where the game started.
    pub opening: &'a Opening,
    /// The game, finished or not.
    pub game: &'a TournamentGame,
    /// How it ended.
    pub end: &'a GameEnd,
    /// Today.
    pub date: PartialDate,
    /// Games with the number of this one that were played before it: `0`
    /// for a game of the schedule, `k` for the `k`-th redo. A redo has the
    /// same pairing as the game it replaces, so its `Round` tag says so
    /// (`1.1 (redo 1)`) and the PGN keeps both games apart (D1312).
    pub redo: u32,
}

/// `h:mm:ss` for a clock or a move time (seconds, rounded down).
pub fn clock_text(time: Duration) -> String {
    let secs = time.as_secs();
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Seconds as text with up to three decimals and no trailing zeros.
fn seconds_text(ms: u64) -> String {
    let (whole, frac) = (ms / 1000, ms % 1000);
    if frac == 0 {
        whole.to_string()
    } else {
        let decimals = format!("{frac:03}");
        format!("{whole}.{}", decimals.trim_end_matches('0'))
    }
}

/// The PGN `TimeControl` tag (D1280): `60+1`, `1/3` (one move in 3 s) or
/// `-` when there is no time.
pub fn time_control_tag(time: TimeControl) -> String {
    match time {
        TimeControl::Clock {
            base_ms,
            increment_ms,
        } => {
            if increment_ms == 0 {
                seconds_text(base_ms)
            } else {
                format!("{}+{}", seconds_text(base_ms), seconds_text(increment_ms))
            }
        }
        TimeControl::MoveTime { ms } => format!("1/{}", seconds_text(ms)),
        TimeControl::Depth(_) | TimeControl::Nodes(_) => "-".to_owned(),
    }
}

/// The note as the comment of one move, from the point of view of White
/// (the convention of the graphs, M4-7).
fn score_comment(style: ScoreComment, side: Color, note: &MoveScore) -> Option<String> {
    let white_pov = |s: EngineScore| match (s, side) {
        (s, Color::White) => s,
        (EngineScore::Cp(cp), Color::Black) => EngineScore::Cp(cp.saturating_neg()),
        (EngineScore::Mate(n), Color::Black) => EngineScore::Mate(n.saturating_neg()),
    };
    let score = white_pov(note.score);
    let value = |plus: bool| match score {
        EngineScore::Mate(n) if plus => format!("{}M{}", if n < 0 { '-' } else { '+' }, n.abs()),
        EngineScore::Mate(n) => format!("#{n}"),
        EngineScore::Cp(cp) if plus => signed_pawns(cp),
        EngineScore::Cp(cp) => {
            let sign = if cp < 0 { "-" } else { "" };
            format!(
                "{sign}{}.{:02}",
                cp.unsigned_abs() / 100,
                cp.unsigned_abs() % 100
            )
        }
    };
    match style {
        ScoreComment::None => None,
        ScoreComment::Text => Some(value(true)),
        ScoreComment::Pct => Some(format!("[% {}]", value(true))),
        ScoreComment::Eval => Some(match note.depth {
            Some(depth) => format!("[%eval {},{depth}]", value(false)),
            None => format!("[%eval {}]", value(false)),
        }),
    }
}

fn move_comment(config: &TournamentConfig, played: &PlayedMove) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if !played.free {
        if let Some(note) = &played.score
            && let Some(text) = score_comment(config.comments.score, played.side, note)
        {
            parts.push(text);
        }
        if config.comments.clock
            && let Some(left) = played.left
        {
            parts.push(format!("[%clk {}]", clock_text(left)));
        }
        if config.comments.move_time
            && let Some(spent) = played.spent
        {
            parts.push(format!("[%emt {}]", clock_text(spent)));
        }
    }
    if let Some(note) = &played.note {
        parts.push(note.clone());
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn rating(elo: Option<u16>) -> Option<Rating> {
    elo.filter(|&e| e > 0)
        .and_then(|e| Elo::new(e).ok())
        .map(|e| Rating::new(e, RatingType::Elo))
}

/// Builds the game of the PGN of the tournament (D1279-D1281).
///
/// # Errors
///
/// [`GameError`] if a move does not fit the game (never for a game that was
/// played by [`TournamentGame`]).
pub fn build_game(input: &RecordInput<'_>) -> Result<Game, GameError> {
    let RecordInput {
        config,
        pairing,
        white,
        black,
        opening,
        game: played,
        end,
        date,
        redo,
    } = input;
    let mut game = Game::from_position(opening.start.clone());
    let mut node = game.root();
    let last_ply = played.played().len();
    for (i, entry) in played.played().iter().enumerate() {
        node = game.add_move(node, entry.mv)?;
        let mut text = move_comment(config, entry);
        if i + 1 == last_ply
            && let Some(final_comment) = &end.comment
        {
            text = Some(match text {
                Some(t) => format!("{t} {final_comment}"),
                None => final_comment.clone(),
            });
        }
        if let Some(text) = text {
            game.set_comment(node, &text)?;
        }
    }
    if last_ply == 0
        && let Some(final_comment) = &end.comment
    {
        game.set_initial_comment(final_comment);
    }
    let tags = game.tags_mut();
    tags.event.clone_from(&config.name);
    tags.site.clone_from(&config.site);
    tags.date = *date;
    tags.round = if *redo == 0 {
        pairing.round_label()
    } else {
        format!("{} (redo {redo})", pairing.round_label())
    };
    tags.white.clone_from(&white.name);
    tags.black.clone_from(&black.name);
    tags.white_rating = rating(white.elo);
    tags.black_rating = rating(black.elo);
    tags.result = end.result;
    // A value the tags refuse (a line break) is dropped: the game goes on.
    let mut extra = |name: &str, value: &str| {
        tags.set_extra(name, value).ok();
    };
    extra("WhiteType", "program");
    extra("BlackType", "program");
    extra("Termination", end.termination.tag());
    extra("TimeControl", &time_control_tag(config.time));
    match config.time {
        TimeControl::MoveTime { ms } => extra("Movetime", &seconds_text(ms)),
        TimeControl::Depth(depth) => extra("Depth", &depth.to_string()),
        TimeControl::Nodes(nodes) => extra("Nodes", &nodes.to_string()),
        TimeControl::Clock { .. } => {}
    }
    if let Some(name) = &opening.name {
        extra("Opening", name);
    }
    if let Some(name) = &white.engine_name {
        extra("WhiteEngine", name);
    }
    if let Some(name) = &black.engine_name {
        extra("BlackEngine", name);
    }
    extra("PlyCount", &played.moves().len().to_string());
    Ok(game)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use mychess_core::{GameResult, Position};
    use mychess_engine::Score;
    use mychess_pgn::write::{WriteOptions, game_to_string};

    use super::*;
    use crate::tournament::config::{Adjudication, Participant, Scheduling};
    use crate::tournament::game::{GameSetup, Reply, Step, Termination};
    use crate::tournament::schedule::build_schedule;

    fn config() -> TournamentConfig {
        TournamentConfig {
            participants: vec![Participant::new(1, "Alpha"), Participant::new(2, "Beta")],
            time: TimeControl::Clock {
                base_ms: 60_000,
                increment_ms: 1_000,
            },
            ..TournamentConfig::default()
        }
    }

    fn played_game(opening: Opening, ucis: &[&str], config: &TournamentConfig) -> TournamentGame {
        let now = Instant::now();
        let setup = GameSetup {
            opening,
            opening_is_book: false,
            time: config.time,
            leeway_percent: 175,
            margin: Duration::from_secs(2),
            depth_limit: Duration::from_secs(600),
            claim_draws: true,
            adjudication: Adjudication::default(),
            book: None,
            book_ply_limit: None,
            ponder: [false, false],
            seed: 0,
            number: 1,
            tablebase: None,
        };
        let mut game = TournamentGame::new(setup, now);
        for uci in ucis {
            let Step::Think(_) = game.advance(now) else {
                panic!("ended early");
            };
            let mv = game.position().parse_uci(uci).expect("uci");
            game.reply(
                &Reply {
                    mv: Some(mv),
                    spent: Duration::from_millis(1_500),
                    score: Some(Score::Cp(35)),
                    depth: Some(14),
                    ponder: None,
                },
                now,
            );
        }
        game
    }

    fn player(name: &str, elo: Option<u16>) -> PlayerInfo {
        PlayerInfo {
            name: name.to_owned(),
            elo,
            engine_name: Some(format!("{name} 1.0")),
        }
    }

    fn build(config: &TournamentConfig, game: &TournamentGame, end: &GameEnd) -> Game {
        let pairing = build_schedule(2, Scheduling::RoundRobin, 2, 0).pairings[1];
        build_game(&RecordInput {
            config,
            pairing: &pairing,
            white: &player("Beta", Some(2800)),
            black: &player("Alpha", None),
            opening: &Opening::initial(),
            game,
            end,
            date: PartialDate::new(Some(2026), Some(10), Some(24)).expect("date"),
            redo: 0,
        })
        .expect("game")
    }

    #[test]
    fn writes_the_tags_of_the_spec() {
        let config = config();
        let game = played_game(Opening::initial(), &["e2e4", "e7e5"], &config);
        let end = GameEnd {
            result: GameResult::Draw,
            termination: Termination::Adjudication,
            comment: Some("Manual adjudication".to_owned()),
        };
        let out = build(&config, &game, &end);
        let tags = out.tags();
        assert_eq!(tags.event, "MyChess Tournament");
        assert_eq!(tags.site, "MyChess");
        assert_eq!(tags.round, "2.1");
        assert_eq!(
            (tags.white.as_str(), tags.black.as_str()),
            ("Beta", "Alpha")
        );
        assert_eq!(tags.result, GameResult::Draw);
        assert_eq!(tags.get_extra("WhiteType"), Some("program"));
        assert_eq!(tags.get_extra("BlackType"), Some("program"));
        assert_eq!(tags.get_extra("Termination"), Some("adjudication"));
        assert_eq!(tags.get_extra("TimeControl"), Some("60+1"));
        assert_eq!(tags.get_extra("WhiteEngine"), Some("Beta 1.0"));
        assert_eq!(tags.get_extra("PlyCount"), Some("2"));
        assert_eq!(tags.white_rating.map(|r| r.value.get()), Some(2800));
        assert!(tags.black_rating.is_none());
    }

    #[test]
    fn writes_the_comments_of_each_move() {
        let config = config();
        let game = played_game(Opening::initial(), &["e2e4", "e7e5"], &config);
        let end = GameEnd {
            result: GameResult::Unknown,
            termination: Termination::Unterminated,
            comment: None,
        };
        let out = build(&config, &game, &end);
        let nodes: Vec<_> = out.mainline().collect();
        assert_eq!(
            out.comment(nodes[0]).expect("node"),
            Some("[%eval 0.35,14] [%clk 0:00:59] [%emt 0:00:01]")
        );
        // The engine layer reports +35 for White after Black's move too, so
        // Black stands at -35 and the comment (White's view) stays +0.35.
        assert_eq!(
            out.comment(nodes[1]).expect("node"),
            Some("[%eval 0.35,14] [%clk 0:00:59] [%emt 0:00:01]")
        );
    }

    #[test]
    fn a_white_point_of_view_score_after_a_black_move_is_written_as_is() {
        // Regression (D1302): Cp(-700) from White's point of view on a move
        // of Black means Black is winning, and the comment shows -7.00.
        let config = config();
        let now = Instant::now();
        let mut s = GameSetup {
            opening: Opening::initial(),
            opening_is_book: false,
            time: config.time,
            leeway_percent: 175,
            margin: Duration::from_secs(2),
            depth_limit: Duration::from_secs(600),
            claim_draws: true,
            adjudication: Adjudication::default(),
            book: None,
            book_ply_limit: None,
            ponder: [false, false],
            seed: 0,
            number: 1,
            tablebase: None,
        };
        s.time = config.time;
        let mut game = TournamentGame::new(s, now);
        for (uci, cp) in [("e2e4", 0), ("e7e5", -700)] {
            let Step::Think(_) = game.advance(now) else {
                panic!("ended early");
            };
            let mv = game.position().parse_uci(uci).expect("uci");
            game.reply(
                &Reply {
                    mv: Some(mv),
                    spent: Duration::from_millis(10),
                    score: Some(Score::Cp(cp)),
                    depth: None,
                    ponder: None,
                },
                now,
            );
        }
        // For the mover, Black, the note is +700.
        assert_eq!(
            game.played()[1].score.map(|n| n.score),
            Some(Score::Cp(700))
        );
        let end = GameEnd {
            result: GameResult::Unknown,
            termination: Termination::Unterminated,
            comment: None,
        };
        let out = build(&config, &game, &end);
        let nodes: Vec<_> = out.mainline().collect();
        let text = out.comment(nodes[1]).expect("node").expect("comment");
        assert!(text.starts_with("[%eval -7.00]"), "{text}");
    }

    #[test]
    fn the_comment_options_turn_parts_off_and_change_the_style() {
        let mut config = config();
        config.comments.score = ScoreComment::Text;
        config.comments.clock = false;
        config.comments.move_time = false;
        let game = played_game(Opening::initial(), &["e2e4"], &config);
        let end = GameEnd {
            result: GameResult::Draw,
            termination: Termination::Normal,
            comment: Some("3 fold repetition".to_owned()),
        };
        let out = build(&config, &game, &end);
        let node = out.mainline().next().expect("node");
        assert_eq!(
            out.comment(node).expect("node"),
            Some("+0.35 3 fold repetition")
        );
        config.comments.score = ScoreComment::Pct;
        let out = build(&config, &game, &end);
        let node = out.mainline().next().expect("node");
        assert_eq!(
            out.comment(node).expect("node"),
            Some("[% +0.35] 3 fold repetition")
        );
        config.comments.score = ScoreComment::None;
        let out = build(&config, &game, &end);
        let node = out.mainline().next().expect("node");
        assert_eq!(out.comment(node).expect("node"), Some("3 fold repetition"));
    }

    #[test]
    fn a_mate_note_and_a_game_without_moves() {
        let note = MoveScore {
            score: Score::Mate(3),
            depth: None,
        };
        assert_eq!(
            score_comment(ScoreComment::Eval, Color::White, &note).as_deref(),
            Some("[%eval #3]")
        );
        assert_eq!(
            score_comment(ScoreComment::Eval, Color::Black, &note).as_deref(),
            Some("[%eval #-3]")
        );
        assert_eq!(
            score_comment(ScoreComment::Text, Color::White, &note).as_deref(),
            Some("+M3")
        );
        let config = config();
        let game = played_game(Opening::initial(), &[], &config);
        let end = GameEnd {
            result: GameResult::White,
            termination: Termination::Abandoned,
            comment: Some("Black engine failed".to_owned()),
        };
        let out = build(&config, &game, &end);
        assert_eq!(out.initial_comment(), Some("Black engine failed"));
    }

    #[test]
    fn book_and_opening_moves_carry_no_measurements() {
        let config = config();
        let start = Position::new();
        let e4 = start.parse_uci("e2e4").expect("uci");
        let opening = Opening {
            name: Some("King pawn".to_owned()),
            start,
            moves: vec![e4],
        };
        let game = played_game(opening.clone(), &["e7e5"], &config);
        let end = GameEnd {
            result: GameResult::Draw,
            termination: Termination::Normal,
            comment: None,
        };
        let pairing = build_schedule(2, Scheduling::RoundRobin, 1, 0).pairings[0];
        let out = build_game(&RecordInput {
            config: &config,
            pairing: &pairing,
            white: &player("Alpha", None),
            black: &player("Beta", None),
            opening: &opening,
            game: &game,
            end: &end,
            date: PartialDate::default(),
            redo: 0,
        })
        .expect("game");
        let first = out.mainline().next().expect("node");
        assert_eq!(out.comment(first).expect("node"), None);
        assert_eq!(out.tags().get_extra("Opening"), Some("King pawn"));
    }

    #[test]
    fn a_non_standard_start_writes_fen_and_setup() {
        let config = config();
        let opening = Opening {
            name: None,
            start: Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").expect("fen"),
            moves: Vec::new(),
        };
        let game = played_game(opening.clone(), &[], &config);
        let end = GameEnd {
            result: GameResult::Unknown,
            termination: Termination::Unterminated,
            comment: None,
        };
        let pairing = build_schedule(2, Scheduling::RoundRobin, 1, 0).pairings[0];
        let out = build_game(&RecordInput {
            config: &config,
            pairing: &pairing,
            white: &player("A", None),
            black: &player("B", None),
            opening: &opening,
            game: &game,
            end: &end,
            date: PartialDate::default(),
            redo: 0,
        })
        .expect("game");
        let text = game_to_string(&out, &WriteOptions::default()).expect("pgn");
        assert!(text.contains("[SetUp \"1\"]"), "{text}");
        assert!(
            text.contains("[FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\"]"),
            "{text}"
        );
    }

    #[test]
    fn formats_time_control_tags() {
        let clock = |b, i| TimeControl::Clock {
            base_ms: b,
            increment_ms: i,
        };
        assert_eq!(time_control_tag(clock(60_000, 1_000)), "60+1");
        assert_eq!(time_control_tag(clock(300_000, 0)), "300");
        assert_eq!(time_control_tag(clock(500, 100)), "0.5+0.1");
        assert_eq!(time_control_tag(TimeControl::MoveTime { ms: 3_000 }), "1/3");
        assert_eq!(time_control_tag(TimeControl::Depth(8)), "-");
        assert_eq!(clock_text(Duration::from_secs(3_725)), "1:02:05");
    }
}
