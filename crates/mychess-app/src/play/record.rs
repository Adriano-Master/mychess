//! The record of the game: tags, result and comments (spec M8-1, D1231,
//! D1232, D1235, D1237).
//!
//! Scid: `sergame.tcl` `play` (328-519) writes the tags; the end-of-game
//! comments come from `checkEndOfGame`/`checkRepetition`.

use mychess_core::{Color, Elo, Game, GameError, NodeId, PartialDate, Rating, RatingType};

use super::config::{Strength, TimeMode};
use super::rules::PlayEnd;
use crate::tactical_game::TacticalSetup;

/// The `Event` tag of the games against an engine.
pub const EVENT: &str = "UCI Game";

/// What the tags of the game are made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagSetup {
    /// Name of the opponent.
    pub opponent: String,
    /// Name of the player; empty is written as `?`.
    pub player: String,
    /// The colour of the player.
    pub player_color: Color,
    /// The limits (for `Timemode` and `TimeControl`).
    pub mode: TimeMode,
    /// The limited strength actually sent to the engine.
    pub strength: Strength,
    /// The name of the specific opening, if any.
    pub opening: Option<String>,
    /// Today.
    pub date: PartialDate,
}

/// Writes the tags that are known when the game starts.
pub fn write_start_tags(game: &mut Game, setup: &TagSetup) {
    let player = if setup.player.trim().is_empty() {
        "?".to_owned()
    } else {
        setup.player.clone()
    };
    let tags = game.tags_mut();
    EVENT.clone_into(&mut tags.event);
    tags.date = setup.date;
    let (white, black) = match setup.player_color {
        Color::White => (player, setup.opponent.clone()),
        Color::Black => (setup.opponent.clone(), player),
    };
    tags.white = white;
    tags.black = black;
    // A value the tags refuse (a line break) is dropped: the game goes on.
    tags.set_extra("Timemode", &setup.mode.timemode_tag()).ok();
    if let Some(control) = setup.mode.time_control_tag() {
        tags.set_extra("TimeControl", &control).ok();
    }
    if let Some(elo) = setup.strength.elo {
        tags.set_extra("UCI_Elo", &elo.to_string()).ok();
    }
    if let Some(skill) = setup.strength.skill {
        tags.set_extra("SkillLevel", &skill.to_string()).ok();
    }
    if let Some(name) = &setup.opening {
        tags.set_extra("Opening", name).ok();
    }
}

/// The `Event` tag of a tactical game (M8-7, D2461).
pub const TACTICAL_EVENT: &str = "Tactical game";

/// Rewrites the tags of a tactical game on top of the common ones (M8-7,
/// D2461): the event, the Elo of the engine, the Elo that was asked for and
/// what the engine was told.
pub(crate) fn write_tactical_tags(game: &mut Game, setup: &TacticalSetup, engine_color: Color) {
    let tags = game.tags_mut();
    TACTICAL_EVENT.clone_into(&mut tags.event);
    tags.set_extra("TacticalElo", &setup.elo.to_string()).ok();
    let Some(plan) = setup.plan else {
        return;
    };
    tags.set_extra("TacticalLimit", &plan.limit_tag()).ok();
    let shown = u16::try_from(plan.effective_elo(setup.elo))
        .ok()
        .and_then(|value| Elo::new(value).ok())
        .map(|value| Rating::new(value, RatingType::Elo));
    match engine_color {
        Color::White => tags.white_rating = shown,
        Color::Black => tags.black_rating = shown,
    }
}

/// Records how the game ended: the `Result` tag and the comment of the
/// automatic draws on the last move.
///
/// # Errors
///
/// [`GameError`] when `last` is not a node of the game.
pub fn write_result(game: &mut Game, end: PlayEnd, last: NodeId) -> Result<(), GameError> {
    game.tags_mut().result = end.result();
    if last != game.root()
        && let Some(text) = end.comment()
    {
        game.append_comment(last, text)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::GameResult;

    fn setup(color: Color) -> TagSetup {
        TagSetup {
            opponent: "Fake 1".to_owned(),
            player: "Ana".to_owned(),
            player_color: color,
            mode: TimeMode::TimeBonus {
                white_minutes: 5,
                white_bonus: 3,
                black_minutes: 5,
                black_bonus: 3,
            },
            strength: Strength::default(),
            opening: None,
            date: PartialDate::new(Some(2026), Some(10), Some(24)).expect("date"),
        }
    }

    #[test]
    fn writes_event_players_date_and_time_tags() {
        let mut game = Game::new();
        write_start_tags(&mut game, &setup(Color::Black));
        let tags = game.tags();
        assert_eq!(tags.event, "UCI Game");
        assert_eq!(
            (tags.white.as_str(), tags.black.as_str()),
            ("Fake 1", "Ana")
        );
        assert_eq!(tags.date.to_string(), "2026.10.24");
        assert_eq!(tags.result, GameResult::Unknown);
        assert_eq!(tags.get_extra("Timemode"), Some("Time+Bonus 5+3/5+3"));
        assert_eq!(tags.get_extra("TimeControl"), Some("300+3"));
        assert_eq!(tags.get_extra("UCI_Elo"), None);
    }

    #[test]
    fn an_unnamed_player_is_a_question_mark() {
        let mut game = Game::new();
        let mut s = setup(Color::White);
        s.player = "  ".to_owned();
        write_start_tags(&mut game, &s);
        assert_eq!(game.tags().white, "?");
    }

    #[test]
    fn strength_and_opening_become_extra_tags() {
        let mut game = Game::new();
        let mut s = setup(Color::White);
        s.strength = Strength {
            elo: Some(1500),
            skill: Some(7),
        };
        s.opening = Some("Sicilian".to_owned());
        write_start_tags(&mut game, &s);
        let tags = game.tags();
        assert_eq!(tags.get_extra("UCI_Elo"), Some("1500"));
        assert_eq!(tags.get_extra("SkillLevel"), Some("7"));
        assert_eq!(tags.get_extra("Opening"), Some("Sicilian"));
    }

    #[test]
    fn the_result_and_the_draw_comment_are_written() {
        let mut game = Game::new();
        let mv = game.start_position().parse_uci("e2e4").expect("uci");
        let node = game.add_move(game.root(), mv).expect("move");
        write_result(&mut game, PlayEnd::ThreefoldRepetition, node).expect("write");
        assert_eq!(game.tags().result, GameResult::Draw);
        assert_eq!(game.comment(node).expect("node"), Some("3 fold repetition"));
        write_result(
            &mut game,
            PlayEnd::Resignation {
                loser: Color::White,
            },
            node,
        )
        .expect("write");
        assert_eq!(game.tags().result, GameResult::Black);
    }
}
