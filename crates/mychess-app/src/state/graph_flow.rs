//! The score and time graph in the state (spec M4-7).
//!
//! The view is derived data, built at the end of [`update`](super::update)
//! when the panel is open and the game or the preferences changed since the
//! last build, so `view` only reads it. The series are `O(plies)` and read
//! the comments once; a game of thousands of plies builds in well under a
//! frame, so the build stays synchronous (D872). Navigation does not touch
//! the cache: only the bar of the current move follows the cursor.

use super::AppState;
use crate::effect::Effect;
use crate::graphs::{
    BarKey, GraphMessage, GraphPrefs, GraphState, Invert, ViewKey, current_x, ply_at_x,
    score_graph_view,
};
use crate::message::Message;
use crate::prefs::{PrefChange, PrefsMessage};
use crate::session::{GameMessage, Nav};

impl AppState {
    /// The graph of the game: the cached view and the bar of the current
    /// move. Empty while the panel is closed.
    pub fn graph(&self) -> &GraphState {
        &self.graph
    }

    pub(crate) fn graph_message(&mut self, message: GraphMessage, effects: &mut Vec<Effect>) {
        let prefs = self.prefs.graph;
        match message {
            GraphMessage::Toggle => self.graph.open = !self.graph.open,
            GraphMessage::SetOpen(open) => self.graph.open = open,
            GraphMessage::SetKind(kind) => {
                self.set_graph_prefs(GraphPrefs { kind, ..prefs }, effects);
            }
            GraphMessage::SetStyle(style) => {
                self.set_graph_prefs(GraphPrefs { style, ..prefs }, effects);
            }
            GraphMessage::ToggleInvertWhite => self.set_graph_prefs(
                GraphPrefs {
                    invert: Invert {
                        white: !prefs.invert.white,
                        ..prefs.invert
                    },
                    ..prefs
                },
                effects,
            ),
            GraphMessage::ToggleInvertBlack => self.set_graph_prefs(
                GraphPrefs {
                    invert: Invert {
                        black: !prefs.invert.black,
                        ..prefs.invert
                    },
                    ..prefs
                },
                effects,
            ),
            GraphMessage::ToggleHighlight => self.set_graph_prefs(
                GraphPrefs {
                    highlight_current: !prefs.highlight_current,
                    ..prefs
                },
                effects,
            ),
            GraphMessage::Click(x) => self.graph_click(x, effects),
            GraphMessage::GoToPly(ply) => self.graph_go_to_ply(ply, effects),
        }
    }

    fn set_graph_prefs(&mut self, prefs: GraphPrefs, effects: &mut Vec<Effect>) {
        if prefs == self.prefs.graph {
            return;
        }
        self.prefs_message(PrefsMessage::Set(PrefChange::Graph(prefs)), effects);
    }

    /// A click goes to the move under it, through the game's own navigation
    /// (so modal dialogs and an annotation run refuse it like any other
    /// move of the cursor).
    fn graph_click(&mut self, x: f32, effects: &mut Vec<Effect>) {
        let Some(view) = self.graph.view.as_ref() else {
            return;
        };
        let Some(ply) = ply_at_x(&view.x_axis, x, view.plies) else {
            return;
        };
        self.graph_navigate(ply, effects);
    }

    /// Goes straight to a ply of the view (style Line, D1556).
    fn graph_go_to_ply(&mut self, ply: usize, effects: &mut Vec<Effect>) {
        let Some(view) = self.graph.view.as_ref() else {
            return;
        };
        self.graph_navigate(ply.min(view.plies), effects);
    }

    fn graph_navigate(&mut self, ply: usize, effects: &mut Vec<Effect>) {
        let Ok(ply) = u32::try_from(ply) else {
            return;
        };
        effects.extend(super::update(
            self,
            Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(ply))),
        ));
    }

    /// Rebuilds the view and the current-move bar when what they depend on
    /// changed. Called at the end of every `update`; closed, it only
    /// forgets what was built.
    pub(crate) fn refresh_graph(&mut self) {
        if !self.graph.open {
            if self.graph.view_key.is_some() || self.graph.bar_key.is_some() {
                self.graph.forget();
            }
            return;
        }
        let prefs = self.prefs.graph;
        let changes = self.session.changes();
        let key = ViewKey::new(changes, prefs);
        if self.graph.view_key != Some(key) {
            self.graph.view = Some(score_graph_view(self.session.game(), prefs));
            self.graph.view_key = Some(key);
            self.graph.stamp = self.graph.stamp.wrapping_add(1);
            self.graph.builds = self.graph.builds.wrapping_add(1);
        }
        let cursor = self.session.cursor();
        let bar_key = BarKey {
            changes,
            cursor,
            prefs,
        };
        if self.graph.bar_key != Some(bar_key) {
            self.graph.bar = current_x(self.session.game(), cursor, prefs);
            self.graph.bar_key = Some(bar_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use mychess_pgn::read::{ReadOptions, parse_pgn_text};

    use super::*;
    use crate::graphs::GraphKind;
    use crate::state::{AppState, update};

    const PGN: &str = "[Event \"t\"]\n[White \"Anna\"]\n[Black \"Boris\"]\n[Result \"1-0\"]\n\n\
        1. e4 { [%eval 0.30] [%clk 0:05:00] } e5 { [%eval 0.25] [%clk 0:05:00] } \
        2. Nf3 { [%eval 0.40] [%clk 0:04:55] } Nc6 { [%eval 0.30] [%clk 0:04:50] } \
        3. Bb5 { [%eval 0.50] [%clk 0:04:40] } a6 { [%eval 0.45] [%clk 0:04:44] } 1-0\n";

    fn loaded() -> AppState {
        let mut state = AppState::default();
        let parsed = parse_pgn_text(PGN, &ReadOptions::default());
        let game = parsed.into_iter().next().expect("one game").game;
        state.session.replace_game(game);
        state
    }

    fn send(state: &mut AppState, message: GraphMessage) {
        drop(update(state, Message::Graph(message)));
    }

    #[test]
    fn closed_panel_builds_nothing() {
        let mut state = loaded();
        drop(update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::End)),
        ));
        assert!(state.graph().view().is_none());
        assert_eq!(state.graph().builds(), 0);
    }

    #[test]
    fn opening_builds_the_view_from_the_real_game() {
        let mut state = loaded();
        send(&mut state, GraphMessage::SetOpen(true));
        let view = state.graph().view().expect("view");
        assert_eq!(view.bars.len(), 6);
        assert_eq!(view.plies, 6);
        assert!(!view.empty);
        assert_eq!(view.title, "Anna - Boris (1-0)");
    }

    #[test]
    fn navigation_and_repeated_messages_do_not_rebuild() {
        let mut state = loaded();
        send(&mut state, GraphMessage::SetOpen(true));
        let builds = state.graph().builds();
        let stamp = state.graph().stamp();
        drop(update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::Start)),
        ));
        drop(update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::End)),
        ));
        send(&mut state, GraphMessage::SetOpen(true));
        assert_eq!(state.graph().builds(), builds);
        assert_eq!(state.graph().stamp(), stamp);
        // The bar follows the cursor without a rebuild.
        assert_eq!(state.graph().current_x(), Some(3.5));
        drop(update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::Start)),
        ));
        assert_eq!(state.graph().current_x(), None);
        assert_eq!(state.graph().builds(), builds);
    }

    #[test]
    fn editing_the_game_rebuilds() {
        let mut state = loaded();
        send(&mut state, GraphMessage::SetOpen(true));
        let builds = state.graph().builds();
        drop(update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(2))),
        ));
        drop(update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(0))),
        ));
        assert_eq!(state.graph().builds(), builds);
        state.session.replace_game(mychess_core::Game::new());
        drop(update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::Start)),
        ));
        assert_eq!(state.graph().builds(), builds + 1);
        assert!(state.graph().view().is_some_and(|v| v.empty));
    }

    #[test]
    fn closing_forgets_and_reopening_rebuilds() {
        let mut state = loaded();
        send(&mut state, GraphMessage::SetOpen(true));
        send(&mut state, GraphMessage::SetOpen(false));
        assert!(state.graph().view().is_none());
        send(&mut state, GraphMessage::Toggle);
        assert!(state.graph().is_open());
        assert!(state.graph().view().is_some());
    }

    #[test]
    fn options_change_the_view_and_the_preferences() {
        let mut state = loaded();
        send(&mut state, GraphMessage::SetOpen(true));
        send(&mut state, GraphMessage::SetKind(GraphKind::Time));
        assert_eq!(state.prefs.graph.kind, GraphKind::Time);
        assert_eq!(state.graph().view().map(|v| v.kind), Some(GraphKind::Time));
        send(&mut state, GraphMessage::SetKind(GraphKind::Score));
        let before = state.graph().view().expect("view").bars.clone();
        send(&mut state, GraphMessage::ToggleInvertBlack);
        assert!(state.prefs.graph.invert.black);
        let after = state.graph().view().expect("view").bars.clone();
        // Black is inverted after the moves of White: bars 1, 3, 5.
        assert_eq!(after[0].1, -before[0].1);
        assert_eq!(after[1].1, before[1].1);
        send(&mut state, GraphMessage::ToggleInvertWhite);
        assert!(state.prefs.graph.invert.white);
        send(&mut state, GraphMessage::ToggleHighlight);
        assert!(!state.prefs.graph.highlight_current);
        drop(update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::End)),
        ));
        assert_eq!(state.graph().current_x(), None);
    }

    #[test]
    fn setting_the_same_option_changes_nothing() {
        let mut state = loaded();
        let mut effects = Vec::new();
        state.graph_message(GraphMessage::SetKind(GraphKind::Auto), &mut effects);
        assert!(effects.is_empty());
        assert!(!state.prefs_dirty);
    }

    #[test]
    fn a_click_goes_to_the_move() {
        let mut state = loaded();
        send(&mut state, GraphMessage::SetOpen(true));
        send(&mut state, GraphMessage::Click(2.0));
        // x = 2.0 is after White's second move: ply 3.
        assert_eq!(state.session.game().ply(state.session.cursor()), Ok(3));
        assert_eq!(state.graph().current_x(), Some(2.0));
        send(&mut state, GraphMessage::Click(0.0));
        assert_eq!(state.session.cursor(), state.session.game().root());
        // Outside the axis and not a number: ignored.
        send(&mut state, GraphMessage::Click(1.0));
        send(&mut state, GraphMessage::Click(500.0));
        send(&mut state, GraphMessage::Click(f32::NAN));
        assert_eq!(state.session.game().ply(state.session.cursor()), Ok(1));
    }

    #[test]
    fn a_click_with_the_panel_closed_is_ignored() {
        let mut state = loaded();
        send(&mut state, GraphMessage::Click(2.0));
        assert_eq!(state.session.cursor(), state.session.game().root());
    }

    #[test]
    fn a_long_game_builds_quickly() {
        let mut game = mychess_core::Game::new();
        let mut pos = mychess_core::Position::new();
        let mut node = game.root();
        for i in 0..6000 {
            let uci = ["g1f3", "g8f6", "f3g1", "f6g8"][i % 4];
            let mv = pos.parse_uci(uci).expect("legal");
            pos.play(mv).expect("play");
            node = game.add_move(node, mv).expect("add");
            game.set_comment(node, "[%eval 0.20] [%clk 1:00:00]")
                .expect("comment");
        }
        let mut state = AppState::default();
        state.session.replace_game(game);
        let start = std::time::Instant::now();
        send(&mut state, GraphMessage::SetOpen(true));
        let elapsed = start.elapsed();
        assert_eq!(state.graph().view().map(|v| v.plies), Some(6000));
        // Generous: a debug build takes a few milliseconds. The bound only
        // catches a quadratic regression.
        assert!(elapsed < std::time::Duration::from_secs(2), "{elapsed:?}");
    }
}
