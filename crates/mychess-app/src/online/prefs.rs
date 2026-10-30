//! Preferences of the online play: the `[online]` table (spec M8-3, D1338).
//!
//! The token is **not** here (D1307): it lives in the keychain or, with the
//! explicit consent of the user, in a 0600 file kept by `mychess-online`.
//! Only the name of the last account and the choice of where the token is
//! kept are remembered.

use serde::Serialize;

/// The default server.
pub const DEFAULT_BASE_URL: &str = "https://lichess.org";

/// Longest value of a text preference, in characters.
const MAX_TEXT_CHARS: usize = 200;

macro_rules! pref_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
        #[non_exhaustive]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $text)] $variant),+
        }

        impl $name {
            /// The spelling used in the TOML file.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }

            /// Reads the TOML spelling.
            pub fn from_name(text: &str) -> Option<$name> {
                match text { $($text => Some($name::$variant),)+ _ => None }
            }
        }
    };
}

pref_enum! {
    /// Where the finished game goes (spec D1335, P7).
    SaveTo {
        /// The Clipbase (the default).
        Clipbase => "clipbase",
        /// Nowhere: the game stays in the editor only.
        Nowhere => "nowhere",
    }
}

pref_enum! {
    /// Where the token of the account is kept (spec D1307). A session-only
    /// token is not a preference: it is the answer to a question.
    TokenStorePref {
        /// The keychain of the system.
        Keyring => "keyring",
        /// A file with permission 0600 in the data directory (only after the
        /// user consented).
        File => "file",
    }
}

/// Everything the online dialogs remember.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OnlinePrefs {
    /// The last account used (the name only).
    pub login: String,
    /// The last seek asked for rated games (default: casual).
    pub rated: bool,
    /// Minutes of the last seek.
    pub time: u32,
    /// Increment of the last seek, in seconds.
    pub increment: u32,
    /// One premove at a time (default: on).
    pub premove: bool,
    /// Ask before resigning (default: on).
    pub confirm_resign: bool,
    /// Show the latency on the strip of the player (default: on).
    pub show_ping: bool,
    /// Where the finished game is stored.
    pub save_to: SaveTo,
    /// Where the token of the account is kept.
    pub token_store: TokenStorePref,
    /// The server (advanced). Only `https`, or `http` on the loopback
    /// address for the tests; anything else is replaced by the default so
    /// the token never travels in clear.
    pub base_url: String,
}

impl Default for OnlinePrefs {
    fn default() -> OnlinePrefs {
        OnlinePrefs {
            login: String::new(),
            rated: false,
            time: 10,
            increment: 0,
            premove: true,
            confirm_resign: true,
            show_ping: true,
            save_to: SaveTo::Clipbase,
            token_store: TokenStorePref::Keyring,
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }
}

/// Whether `url` may carry a token: `https`, or `http` to the loopback
/// interface. No user info, query or fragment.
pub fn base_url_is_acceptable(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    let host_part = rest.split('/').next().unwrap_or("");
    if host_part.is_empty()
        || host_part.contains('@')
        || rest.contains('?')
        || rest.contains('#')
        || url.chars().any(char::is_whitespace)
    {
        return false;
    }
    match scheme {
        "https" => true,
        "http" => {
            let host = host_part
                .rsplit_once(':')
                .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
                .map_or(host_part, |(host, _)| host);
            matches!(host, "127.0.0.1" | "localhost" | "[::1]")
        }
        _ => false,
    }
}

/// Whether `login` has the shape of a Lichess user name (nothing that could
/// escape a path or an entry of the keychain).
pub fn login_is_valid(login: &str) -> bool {
    (2..=30).contains(&login.len())
        && login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl OnlinePrefs {
    /// The same preferences with the numbers in range and the texts safe.
    #[must_use]
    pub fn sanitized(mut self) -> OnlinePrefs {
        let defaults = OnlinePrefs::default();
        self.time = self.time.min(180);
        self.increment = self.increment.min(180);
        if !login_is_valid(&self.login) {
            self.login = String::new();
        }
        self.login = self.login.chars().take(MAX_TEXT_CHARS).collect();
        if !base_url_is_acceptable(&self.base_url) {
            self.base_url = defaults.base_url;
        }
        self
    }
}

/// Whether the Board API accepts a seek of `minutes` + `increment` seconds
/// (rapid or slower, D1314); the front-end disables the button otherwise.
pub fn seek_time_is_valid(minutes: u32, increment: u32) -> bool {
    mychess_online::time_control::validate_seek(minutes, increment).is_ok()
}

/// Whether the Board API accepts a challenge with a clock of `limit_secs`
/// + `increment` seconds (blitz or slower, D1314).
pub fn challenge_time_is_valid(limit_secs: u32, increment: u32) -> bool {
    mychess_online::time_control::validate_challenge(limit_secs, increment).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_url_must_not_leak_the_token_in_clear() {
        for ok in [
            "https://lichess.org",
            "https://lichess.org/",
            "http://127.0.0.1:8080",
            "http://localhost",
            "http://[::1]:9",
        ] {
            assert!(base_url_is_acceptable(ok), "{ok}");
        }
        for bad in [
            "http://lichess.org",
            "http://example.com:80",
            "ftp://127.0.0.1",
            "https://user@lichess.org",
            "https://lichess.org/?x=1",
            "lichess.org",
            "https://",
            "https://a b",
            "http://127.0.0.1.evil.com",
        ] {
            assert!(!base_url_is_acceptable(bad), "{bad}");
        }
    }

    #[test]
    fn sanitizing_limits_numbers_and_replaces_unsafe_values() {
        let prefs = OnlinePrefs {
            time: 9999,
            increment: 9999,
            login: "../etc".to_owned(),
            base_url: "http://evil.example".to_owned(),
            ..OnlinePrefs::default()
        }
        .sanitized();
        assert_eq!((prefs.time, prefs.increment), (180, 180));
        assert_eq!(prefs.login, "");
        assert_eq!(prefs.base_url, DEFAULT_BASE_URL);
        assert_eq!(
            OnlinePrefs {
                login: "Ana_1".to_owned(),
                ..OnlinePrefs::default()
            }
            .sanitized()
            .login,
            "Ana_1"
        );
    }

    #[test]
    fn the_defaults_are_the_ones_of_the_spec() {
        let d = OnlinePrefs::default();
        assert!(d.premove && d.confirm_resign && d.show_ping && !d.rated);
        assert_eq!(
            (d.save_to, d.token_store),
            (SaveTo::Clipbase, TokenStorePref::Keyring)
        );
    }
}
