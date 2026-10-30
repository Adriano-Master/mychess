//! What a line of the server means (spec m8-9, D2605-D2607).
//!
//! [`parse_line`] classifies one line (already framed, D2603) into a
//! [`FicsEvent`]. It is pure and total: anything it does not know is
//! [`FicsEvent::Text`], which the console shows as is. All text taken from
//! the server is cleaned (control characters out) and cut to a length that
//! cannot hurt (D2604).

use super::style12::Style12;

/// Longest free text kept (a tell, a console line).
pub const MAX_TEXT: usize = 2048;
/// Longest handle kept (FICS handles have 17 letters at most).
const MAX_HANDLE: usize = 32;

/// Colour wanted in a seek.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeekColor {
    /// Any colour.
    #[default]
    Any,
    /// White.
    White,
    /// Black.
    Black,
}

/// An advertisement of the seek graph (`<s>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seek {
    /// The index to `play`.
    pub index: u32,
    /// The advertiser.
    pub name: String,
    /// The titles, raw (`ti=`).
    pub titles: String,
    /// The rating; `None` for an unrated player (`0` or `++++`).
    pub rating: Option<u32>,
    /// The rating is provisional.
    pub provisional: bool,
    /// Minutes of each side.
    pub minutes: u32,
    /// Increment in seconds.
    pub increment: u32,
    /// Rated game.
    pub rated: bool,
    /// The kind: `blitz`, `lightning`, `standard`, `untimed`, a variant...
    pub kind: String,
    /// The colour wanted.
    pub color: SeekColor,
    /// Lowest rating accepted.
    pub min_rating: u32,
    /// Highest rating accepted.
    pub max_rating: u32,
    /// The game starts without the advertiser confirming it.
    pub automatic: bool,
    /// The formula of the advertiser is checked.
    pub formula: bool,
}

/// A match request, as `Challenge:` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchOffer {
    /// Who asks.
    pub challenger: String,
    /// His rating.
    pub challenger_rating: Option<u32>,
    /// Who is asked.
    pub opponent: String,
    /// His rating.
    pub opponent_rating: Option<u32>,
    /// Rated game.
    pub rated: bool,
    /// The kind.
    pub kind: String,
    /// Minutes of each side.
    pub minutes: u32,
    /// Increment in seconds.
    pub increment: u32,
}

/// A game that starts (`Creating:`) or that is listed (`Game N: ...`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSummary {
    /// The number of the game, when the line carries it.
    pub game: Option<u32>,
    /// White.
    pub white: String,
    /// White's rating.
    pub white_rating: Option<u32>,
    /// Black.
    pub black: String,
    /// Black's rating.
    pub black_rating: Option<u32>,
    /// Rated game.
    pub rated: bool,
    /// The kind.
    pub kind: String,
    /// Minutes of each side.
    pub minutes: u32,
    /// Increment in seconds.
    pub increment: u32,
}

/// The result at the end of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FicsResult {
    /// `1-0`.
    WhiteWins,
    /// `0-1`.
    BlackWins,
    /// `1/2-1/2`.
    Draw,
    /// `*`: aborted, adjourned or unknown.
    Unfinished,
}

/// An event of the connection, after parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FicsEvent {
    /// A `<12>` board.
    Style12(Box<Style12>),
    /// A seek appeared in the graph.
    SeekAdd(Box<Seek>),
    /// Seeks were removed (`<sr>`).
    SeekRemove(Vec<u32>),
    /// All the seeks were cleared (`<sc>`).
    SeekClear,
    /// The server names the guest account (`Press return ... as "X"`).
    GuestName(String),
    /// The session starts (`**** Starting FICS session as X ****`).
    SessionStart {
        /// The handle, without the `(U)` suffix.
        name: String,
    },
    /// The login was refused.
    LoginFailed(LoginFailure),
    /// A game is about to start (`Creating: ...`).
    Creating(Box<GameSummary>),
    /// `{Game N (A vs. B) Creating rated blitz match.}`
    GameBegan {
        /// The number of the game.
        game: u32,
        /// White.
        white: String,
        /// Black.
        black: String,
    },
    /// `{Game N (A vs. B) A resigns} 0-1`
    GameEnded {
        /// The number of the game.
        game: u32,
        /// White.
        white: String,
        /// Black.
        black: String,
        /// The reason, as the server said it.
        reason: String,
        /// The result.
        result: FicsResult,
    },
    /// A match request (to the user or from him).
    Challenge(Box<MatchOffer>),
    /// `X offers you a draw.`
    DrawOffer {
        /// Who offers.
        from: String,
    },
    /// `X would like to take back N half move(s).`
    TakebackOffer {
        /// Who asks.
        from: String,
        /// How many plies.
        plies: u32,
    },
    /// `X would like to abort the game`.
    AbortOffer {
        /// Who asks.
        from: String,
    },
    /// `You are now observing game N.`
    Observing {
        /// The game.
        game: u32,
    },
    /// `Game N: A (1500) B (1600) rated blitz 5 0`
    Listed(Box<GameSummary>),
    /// `Removing game N from observation list.`
    Unobserved {
        /// The game.
        game: u32,
    },
    /// `You are now examining game N.`
    Examining {
        /// The game.
        game: u32,
    },
    /// `You are no longer examining game N.`
    ExamineEnded,
    /// `X tells you: text`
    Tell {
        /// The sender.
        from: String,
        /// The text.
        text: String,
    },
    /// `X(12): text` (a channel, lectures included).
    ChannelTell {
        /// The channel.
        channel: u32,
        /// The sender.
        from: String,
        /// The text.
        text: String,
    },
    /// `X shouts: text`
    Shout {
        /// The sender.
        from: String,
        /// The text.
        text: String,
    },
    /// The server answered the `ping` command.
    Ping {
        /// The ping the server reports, in milliseconds.
        ms: u32,
    },
    /// Any other line.
    Text(String),
}

/// Why the login failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoginFailure {
    /// The password is wrong.
    InvalidPassword,
    /// The handle is not registered (and it was not a guest login).
    NotRegistered,
    /// The server is full or refuses the connection.
    Refused,
}

fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// A handle: letters, digits and the usual extras, 2-17 long, possibly with
/// a `(U)`, `(*)` or `(TM)` suffix that is removed.
fn handle(token: &str) -> Option<String> {
    let base = token.split('(').next().unwrap_or("");
    let ok = (2..=MAX_HANDLE).contains(&base.chars().count())
        && base.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then(|| base.to_owned())
}

fn rating(token: &str) -> Option<u32> {
    token
        .trim_matches(|c| c == '(' || c == ')')
        .parse::<u32>()
        .ok()
        .filter(|r| *r > 0)
}

/// Classifies one line of the server.
#[must_use]
pub fn parse_line(raw: &str) -> FicsEvent {
    // The prompt is written in front of the answers on the same line.
    let mut line = raw.trim_end();
    while let Some(rest) = line.trim_start().strip_prefix("fics%") {
        line = rest;
    }
    let line = line.trim_start();
    typed(line).unwrap_or_else(|| FicsEvent::Text(clean(line, MAX_TEXT)))
}

fn typed(line: &str) -> Option<FicsEvent> {
    if line.starts_with("<12>") {
        return Style12::parse(line)
            .ok()
            .map(|s| FicsEvent::Style12(Box::new(s)));
    }
    if line.starts_with("<sc>") {
        return Some(FicsEvent::SeekClear);
    }
    if let Some(rest) = line.strip_prefix("<sr>") {
        let ids: Vec<u32> = rest
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .take(1024)
            .collect();
        return Some(FicsEvent::SeekRemove(ids));
    }
    if let Some(rest) = line.strip_prefix("<s>") {
        return parse_seek(rest).map(|s| FicsEvent::SeekAdd(Box::new(s)));
    }
    if line.starts_with('{') {
        return parse_game_brace(line);
    }
    if let Some(rest) = line.strip_prefix("Creating:") {
        return parse_summary(rest).map(|g| FicsEvent::Creating(Box::new(g)));
    }
    if let Some(rest) = line.strip_prefix("Challenge:") {
        return parse_challenge(rest).map(|o| FicsEvent::Challenge(Box::new(o)));
    }
    if let Some(rest) = line.strip_prefix("Game ")
        && let Some((num, tail)) = rest.split_once(':')
        && let Ok(game) = num.trim().parse::<u32>()
        && let Some(mut g) = parse_summary(tail)
    {
        g.game = Some(game);
        return Some(FicsEvent::Listed(Box::new(g)));
    }
    if let Some(n) = number_in(line, "You are now observing game ") {
        return Some(FicsEvent::Observing { game: n });
    }
    if let Some(n) = number_in(line, "Removing game ") {
        return Some(FicsEvent::Unobserved { game: n });
    }
    if let Some(n) = number_in(line, "You are now examining game ") {
        return Some(FicsEvent::Examining { game: n });
    }
    if line.starts_with("You are no longer examining game") {
        return Some(FicsEvent::ExamineEnded);
    }
    if line.contains("Starting FICS session as ") {
        let after = line.split("Starting FICS session as ").nth(1)?;
        let name = handle(after.split_whitespace().next()?)?;
        return Some(FicsEvent::SessionStart { name });
    }
    if let Some(rest) = line.strip_prefix("Press return to enter the server as \"") {
        let name = handle(rest.split('"').next()?)?;
        return Some(FicsEvent::GuestName(name));
    }
    if line.contains("Invalid password") {
        return Some(FicsEvent::LoginFailed(LoginFailure::InvalidPassword));
    }
    if line.contains("is not a registered name") {
        return Some(FicsEvent::LoginFailed(LoginFailure::NotRegistered));
    }
    if line.starts_with("Sorry, the server is full")
        || line.contains("is banned")
        || line.contains("has been banned")
    {
        return Some(FicsEvent::LoginFailed(LoginFailure::Refused));
    }
    social(line)
}

/// The first number after `prefix` in a line that starts with it.
fn number_in(line: &str, prefix: &str) -> Option<u32> {
    let rest = line.strip_prefix(prefix)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn social(line: &str) -> Option<FicsEvent> {
    if let Some((who, text)) = line.split_once(" tells you: ") {
        return Some(FicsEvent::Tell {
            from: handle(who)?,
            text: clean(text, MAX_TEXT),
        });
    }
    if let Some((who, text)) = line.split_once(" shouts: ") {
        return Some(FicsEvent::Shout {
            from: handle(who)?,
            text: clean(text, MAX_TEXT),
        });
    }
    if let Some(who) = line.strip_suffix(" offers you a draw.") {
        return Some(FicsEvent::DrawOffer { from: handle(who)? });
    }
    if let Some((who, tail)) = line.split_once(" would like to take back ") {
        let plies = tail
            .split_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .filter(|n| (1..=1000).contains(n))?;
        return Some(FicsEvent::TakebackOffer {
            from: handle(who)?,
            plies,
        });
    }
    if let Some((who, _)) = line.split_once(" would like to abort the game") {
        return Some(FicsEvent::AbortOffer { from: handle(who)? });
    }
    if let Some(ev) = channel_tell(line) {
        return Some(ev);
    }
    ping(line)
}

/// `Name(12): text`, `Name(U)(12): text`.
fn channel_tell(line: &str) -> Option<FicsEvent> {
    let (head, text) = line.split_once(": ")?;
    if head.contains(' ') || !head.ends_with(')') {
        return None;
    }
    let open = head.rfind('(')?;
    let channel: u32 = head[open + 1..head.len() - 1].parse().ok()?;
    Some(FicsEvent::ChannelTell {
        channel,
        from: handle(&head[..open])?,
        text: clean(text, MAX_TEXT),
    })
}

/// Lines such as `Average ping time for X: 83 ms` (D2610 (confirmar)).
fn ping(line: &str) -> Option<FicsEvent> {
    let lower = line.to_ascii_lowercase();
    if !lower.contains("ping time") {
        return None;
    }
    let ms = lower
        .split(|c: char| !c.is_ascii_digit())
        .find(|t| !t.is_empty() && t.len() <= 6)?
        .parse()
        .ok()?;
    Some(FicsEvent::Ping { ms })
}

fn parse_seek(rest: &str) -> Option<Seek> {
    let mut tokens = rest.split_whitespace();
    let index: u32 = tokens.next()?.parse().ok()?;
    let mut seek = Seek {
        index,
        name: String::new(),
        titles: String::new(),
        rating: None,
        provisional: false,
        minutes: 0,
        increment: 0,
        rated: false,
        kind: String::new(),
        color: SeekColor::Any,
        min_rating: 0,
        max_rating: 9999,
        automatic: true,
        formula: false,
    };
    for token in tokens {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        match key {
            "w" => seek.name = handle(value)?,
            "ti" => seek.titles = clean(value, 8),
            "rt" => {
                seek.provisional = value.ends_with('P');
                let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
                seek.rating = digits.parse().ok().filter(|r| *r > 0);
            }
            "t" => seek.minutes = value.parse().ok()?,
            "i" => seek.increment = value.parse().ok()?,
            "r" => seek.rated = value == "r",
            "tp" => seek.kind = clean(value, 24),
            "c" => {
                seek.color = match value {
                    "W" => SeekColor::White,
                    "B" => SeekColor::Black,
                    _ => SeekColor::Any,
                }
            }
            "rr" => {
                let (lo, hi) = value.split_once('-')?;
                seek.min_rating = lo.parse().ok()?;
                seek.max_rating = hi.parse().ok()?;
            }
            "a" => seek.automatic = value == "t",
            "f" => seek.formula = value == "t",
            _ => {}
        }
    }
    if seek.name.is_empty() || seek.kind.is_empty() {
        return None;
    }
    Some(seek)
}

/// `{Game N (A vs. B) text} result`
fn parse_game_brace(line: &str) -> Option<FicsEvent> {
    let inner = line.strip_prefix("{Game ")?;
    let (inside, after) = inner.split_once('}')?;
    let (num, rest) = inside.split_once(' ')?;
    let game: u32 = num.parse().ok()?;
    let rest = rest.strip_prefix('(')?;
    let (names, text) = rest.split_once(')')?;
    let (white, black) = names.split_once(" vs. ")?;
    let (white, black) = (handle(white)?, handle(black)?);
    let text = text.trim();
    if text.starts_with("Creating ") {
        return Some(FicsEvent::GameBegan { game, white, black });
    }
    let result = match after.trim() {
        "1-0" => FicsResult::WhiteWins,
        "0-1" => FicsResult::BlackWins,
        "1/2-1/2" => FicsResult::Draw,
        "*" => FicsResult::Unfinished,
        _ => return None,
    };
    Some(FicsEvent::GameEnded {
        game,
        white,
        black,
        reason: clean(text, 256),
        result,
    })
}

/// `name (1500) name (1600) rated blitz 5 0` with the optional trailing dot.
/// `( 696)` and `(++++)` are accepted.
fn parse_summary(rest: &str) -> Option<GameSummary> {
    let normalized = rest.replace("( ", "(");
    let t: Vec<&str> = normalized.split_whitespace().collect();
    // name (r) name (r) rated|unrated kind minutes inc [flags]
    if t.len() < 7 {
        return None;
    }
    let rated = match t[4] {
        "rated" => true,
        "unrated" => false,
        _ => return None,
    };
    Some(GameSummary {
        game: None,
        white: handle(t[0])?,
        white_rating: rating(t[1]),
        black: handle(t[2])?,
        black_rating: rating(t[3]),
        rated,
        kind: clean(t[5], 24),
        minutes: t[6].parse().ok()?,
        increment: t.get(7)?.trim_end_matches('.').parse().ok()?,
    })
}

fn parse_challenge(rest: &str) -> Option<MatchOffer> {
    let g = parse_summary(rest)?;
    Some(MatchOffer {
        challenger: g.white,
        challenger_rating: g.white_rating,
        opponent: g.black,
        opponent_rating: g.black_rating,
        rated: g.rated,
        kind: g.kind,
        minutes: g.minutes,
        increment: g.increment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_line() {
        let ev = parse_line(
            "<s> 16 w=GuestXWHP ti=01 rt=1500P t=5 i=3 r=r tp=blitz c=W rr=1200-1800 a=f f=t",
        );
        let FicsEvent::SeekAdd(s) = ev else {
            panic!("not a seek");
        };
        assert_eq!((s.index, s.name.as_str()), (16, "GuestXWHP"));
        assert_eq!((s.rating, s.provisional), (Some(1500), true));
        assert_eq!((s.minutes, s.increment, s.rated), (5, 3, true));
        assert_eq!(s.color, SeekColor::White);
        assert_eq!((s.min_rating, s.max_rating), (1200, 1800));
        assert!(!s.automatic && s.formula);
    }

    #[test]
    fn unrated_seek_and_removals() {
        let FicsEvent::SeekAdd(s) =
            parse_line("<s> 9 w=bob ti=00 rt=0P t=1 i=0 r=u tp=lightning c=? rr=0-9999 a=t f=f")
        else {
            panic!("not a seek");
        };
        assert_eq!(s.rating, None);
        assert!(!s.rated);
        assert_eq!(
            parse_line("<sr> 12 13 x 14"),
            FicsEvent::SeekRemove(vec![12, 13, 14])
        );
        assert_eq!(parse_line("<sc>"), FicsEvent::SeekClear);
        // Broken seek lines are plain text, not errors.
        assert!(matches!(parse_line("<s> 9 w=bob"), FicsEvent::Text(_)));
        assert!(matches!(parse_line("<s> x"), FicsEvent::Text(_)));
    }

    #[test]
    fn game_begin_and_end() {
        assert_eq!(
            parse_line("{Game 112 (Libertie vs. stevenaaus) Creating rated blitz match.}"),
            FicsEvent::GameBegan {
                game: 112,
                white: "Libertie".into(),
                black: "stevenaaus".into()
            }
        );
        assert_eq!(
            parse_line("{Game 331 (AmorVerus vs. killerbie) killerbie checkmated} 1-0"),
            FicsEvent::GameEnded {
                game: 331,
                white: "AmorVerus".into(),
                black: "killerbie".into(),
                reason: "killerbie checkmated".into(),
                result: FicsResult::WhiteWins
            }
        );
        let FicsEvent::GameEnded { result, .. } =
            parse_line("{Game 3 (a1 vs. b2) Game drawn by mutual agreement} 1/2-1/2")
        else {
            panic!("not an end");
        };
        assert_eq!(result, FicsResult::Draw);
        assert!(matches!(
            parse_line("{Game 3 (a1 vs. b2) Game aborted} *"),
            FicsEvent::GameEnded {
                result: FicsResult::Unfinished,
                ..
            }
        ));
        assert!(matches!(
            parse_line("{Game 3 (a1 vs. b2) odd} banana"),
            FicsEvent::Text(_)
        ));
    }

    #[test]
    fn creating_challenge_and_listed() {
        let FicsEvent::Creating(g) =
            parse_line("Creating: Libertie (1263) stevenaaus (1092) rated blitz 6 12")
        else {
            panic!("not creating");
        };
        assert_eq!((g.white.as_str(), g.white_rating), ("Libertie", Some(1263)));
        assert_eq!((g.minutes, g.increment, g.rated), (6, 12, true));
        let FicsEvent::Challenge(c) =
            parse_line("Challenge: GuestAB (----) bob ( 696) unrated blitz 5 0.")
        else {
            panic!("not a challenge");
        };
        assert_eq!(c.challenger_rating, None);
        assert_eq!(c.opponent_rating, Some(696));
        assert!(!c.rated);
        let FicsEvent::Listed(g) =
            parse_line("Game 193: franky (1758) homeomorphism (1722) rated lightning 1 0")
        else {
            panic!("not listed");
        };
        assert_eq!(g.game, Some(193));
        assert_eq!(g.kind, "lightning");
    }

    #[test]
    fn observe_examine_and_session() {
        assert_eq!(
            parse_line("You are now observing game 193."),
            FicsEvent::Observing { game: 193 }
        );
        assert_eq!(
            parse_line("Removing game 193 from observation list."),
            FicsEvent::Unobserved { game: 193 }
        );
        assert_eq!(
            parse_line("You are now examining game 7."),
            FicsEvent::Examining { game: 7 }
        );
        assert_eq!(
            parse_line("**** Starting FICS session as GuestABCD(U) ****"),
            FicsEvent::SessionStart {
                name: "GuestABCD".into()
            }
        );
        assert_eq!(
            parse_line("Press return to enter the server as \"GuestQQQQ\":"),
            FicsEvent::GuestName("GuestQQQQ".into())
        );
    }

    #[test]
    fn offers_tells_and_channels() {
        assert_eq!(
            parse_line("fics% bob offers you a draw."),
            FicsEvent::DrawOffer { from: "bob".into() }
        );
        assert_eq!(
            parse_line("bob would like to take back 2 half move(s)."),
            FicsEvent::TakebackOffer {
                from: "bob".into(),
                plies: 2
            }
        );
        assert_eq!(
            parse_line("bob(*) tells you: hi there"),
            FicsEvent::Tell {
                from: "bob".into(),
                text: "hi there".into()
            }
        );
        assert_eq!(
            parse_line("GM_Lecturer(TM)(24): welcome to the lecture"),
            FicsEvent::ChannelTell {
                channel: 24,
                from: "GM_Lecturer".into(),
                text: "welcome to the lecture".into()
            }
        );
        assert!(matches!(
            parse_line("bob shouts: hello all"),
            FicsEvent::Shout { .. }
        ));
        assert!(matches!(
            parse_line("bob would like to abort the game; type \"abort\" to accept."),
            FicsEvent::AbortOffer { .. }
        ));
    }

    #[test]
    fn login_failures_ping_and_text() {
        assert_eq!(
            parse_line("**** Invalid password! ****"),
            FicsEvent::LoginFailed(LoginFailure::InvalidPassword)
        );
        assert_eq!(
            parse_line("\"Zed\" is not a registered name."),
            FicsEvent::LoginFailed(LoginFailure::NotRegistered)
        );
        assert_eq!(
            parse_line("Average ping time for bob: 83 ms"),
            FicsEvent::Ping { ms: 83 }
        );
        assert_eq!(
            parse_line("Something else\u{7} entirely"),
            FicsEvent::Text("Something else entirely".into())
        );
    }

    #[test]
    fn style12_goes_through() {
        let line = "<12> rnbqkbnr pppppppp -------- -------- -------- -------- PPPPPPPP RNBQKBNR W -1 1 1 1 1 0 7 a b 0 2 12 39 39 120 120 1 none (0:00) none 0";
        assert!(matches!(parse_line(line), FicsEvent::Style12(_)));
        assert!(matches!(parse_line("<12> broken"), FicsEvent::Text(_)));
    }

    #[test]
    fn hostile_lines_never_panic_and_stay_bounded() {
        let long = "x".repeat(10_000);
        let FicsEvent::Text(t) = parse_line(&long) else {
            panic!("not text");
        };
        assert_eq!(t.len(), MAX_TEXT);
        for line in [
            "{",
            "{Game ",
            "{Game 9 (",
            "{Game 9 (a vs. ) x} 1-0",
            "Game :",
            "Game 5: ",
            "Challenge:",
            "(): ",
            "x(99999999999999): y",
            "<sr>",
            "<s>",
            " tells you: ",
            "Creating: a (1) b (1) rated blitz 5",
        ] {
            drop(parse_line(line));
        }
    }
}
