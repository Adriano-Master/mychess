//! The state machine of the online session (spec M8-3, D1311):
//! `Disconnected -> Connecting -> Idle -> (Seeking | Challenging) -> Playing
//! -> Idle`, with the sub-states of `Playing`.

/// The sub-state of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlayingState {
    /// The game is on and the stream is alive.
    Live,
    /// The stream of the game was lost; it is being reopened (D1333).
    Reconnecting,
    /// The result is shown; the lock is already released (D1330).
    Finished,
}

/// Where the session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Phase {
    /// No account is connected.
    Disconnected,
    /// The token is being checked.
    Connecting,
    /// Connected, nothing going on.
    Idle,
    /// A seek is open.
    Seeking,
    /// A challenge was sent and waits.
    Challenging,
    /// A game (see [`PlayingState`]).
    Playing(PlayingState),
}

impl Phase {
    /// Whether an account is connected.
    pub fn is_connected(self) -> bool {
        !matches!(self, Phase::Disconnected | Phase::Connecting)
    }

    /// Whether a game is on (live or reconnecting): the lock of D1330.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Phase::Playing(PlayingState::Live | PlayingState::Reconnecting)
        )
    }

    /// Whether a seek, a challenge or a game occupies the session.
    pub fn is_busy(self) -> bool {
        matches!(self, Phase::Seeking | Phase::Challenging) || self.is_live()
    }

    /// Whether a seek, a challenge or an accepted challenge may start a
    /// game from here.
    pub fn can_start(self) -> bool {
        matches!(self, Phase::Idle | Phase::Playing(PlayingState::Finished))
    }

    /// Whether a connection may be requested.
    pub fn can_connect(self) -> bool {
        self == Phase::Disconnected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_live_game_locks_and_only_idle_or_finished_can_start() {
        use PlayingState::*;
        let all = [
            Phase::Disconnected,
            Phase::Connecting,
            Phase::Idle,
            Phase::Seeking,
            Phase::Challenging,
            Phase::Playing(Live),
            Phase::Playing(Reconnecting),
            Phase::Playing(Finished),
        ];
        let live: Vec<bool> = all.iter().map(|p| p.is_live()).collect();
        assert_eq!(live, [false, false, false, false, false, true, true, false]);
        let start: Vec<bool> = all.iter().map(|p| p.can_start()).collect();
        assert_eq!(
            start,
            [false, false, true, false, false, false, false, true]
        );
        assert!(Phase::Seeking.is_busy() && !Phase::Idle.is_busy());
        assert!(Phase::Disconnected.can_connect() && !Phase::Idle.can_connect());
        assert!(!Phase::Connecting.is_connected() && Phase::Seeking.is_connected());
    }
}
