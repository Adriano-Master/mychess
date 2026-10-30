//! The announcement of a move: the queue of sound files of the Scid and the
//! text in each language (D2807).

/// The longest SAN text looked at; a move is never longer.
const MAX_SAN_CHARS: usize = 16;

/// One word of the spoken move: one file of the sounds folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SoundToken {
    /// `King.wav`.
    King,
    /// `Queen.wav`.
    Queen,
    /// `Rook.wav`.
    Rook,
    /// `Bishop.wav`.
    Bishop,
    /// `Knight.wav`.
    Knight,
    /// `CastleK.wav`: short castling.
    CastleKingside,
    /// `CastleQ.wav`: long castling.
    CastleQueenside,
    /// `Back.wav`.
    Back,
    /// `Mate.wav`.
    Mate,
    /// `Promote.wav`.
    Promote,
    /// `Check.wav`.
    Check,
    /// `x.wav`: a capture.
    Takes,
    /// A file of the board, `a.wav` to `h.wav` (the letter, lower case).
    File(char),
    /// A rank of the board, `1.wav` to `8.wav` (the digit).
    Rank(char),
}

impl SoundToken {
    /// The name of the file (without the extension), as in the Scid.
    #[must_use]
    pub fn file_stem(self) -> String {
        match self {
            SoundToken::King => "King".to_owned(),
            SoundToken::Queen => "Queen".to_owned(),
            SoundToken::Rook => "Rook".to_owned(),
            SoundToken::Bishop => "Bishop".to_owned(),
            SoundToken::Knight => "Knight".to_owned(),
            SoundToken::CastleKingside => "CastleK".to_owned(),
            SoundToken::CastleQueenside => "CastleQ".to_owned(),
            SoundToken::Back => "Back".to_owned(),
            SoundToken::Mate => "Mate".to_owned(),
            SoundToken::Promote => "Promote".to_owned(),
            SoundToken::Check => "Check".to_owned(),
            SoundToken::Takes => "x".to_owned(),
            SoundToken::File(c) | SoundToken::Rank(c) => c.to_string(),
        }
    }

    /// Every token, for reading the folder.
    #[must_use]
    pub fn all() -> Vec<SoundToken> {
        let mut all = vec![
            SoundToken::King,
            SoundToken::Queen,
            SoundToken::Rook,
            SoundToken::Bishop,
            SoundToken::Knight,
            SoundToken::CastleKingside,
            SoundToken::CastleQueenside,
            SoundToken::Back,
            SoundToken::Mate,
            SoundToken::Promote,
            SoundToken::Check,
            SoundToken::Takes,
        ];
        all.extend(('a'..='h').map(SoundToken::File));
        all.extend(('1'..='8').map(SoundToken::Rank));
        all
    }
}

/// The files the Scid plays for the SAN move `san` (`sound.tcl`, `soundMap`):
/// each character is a word. `Nf3+` is Knight, f, 3, Check; `O-O` is the
/// short castling and `O-O-O` the long one. Unknown characters (the
/// `!`/`?` marks, the `-` of the null move) are skipped, so the result may
/// be empty. Unlike the Scid, a check or mate after a castling is also said.
#[must_use]
pub fn announce_tokens(san: &str) -> Vec<SoundToken> {
    let san: String = san.trim().chars().take(MAX_SAN_CHARS).collect();
    let mut tokens = Vec::new();
    let rest = if let Some(rest) = strip_castling(&san, true) {
        tokens.push(SoundToken::CastleQueenside);
        rest
    } else if let Some(rest) = strip_castling(&san, false) {
        tokens.push(SoundToken::CastleKingside);
        rest
    } else {
        san.as_str()
    };
    for c in rest.chars() {
        let token = match c {
            'K' => SoundToken::King,
            'Q' => SoundToken::Queen,
            'R' => SoundToken::Rook,
            'B' => SoundToken::Bishop,
            'N' => SoundToken::Knight,
            'x' => SoundToken::Takes,
            '#' => SoundToken::Mate,
            '=' => SoundToken::Promote,
            '+' => SoundToken::Check,
            'a'..='h' => SoundToken::File(c),
            '1'..='8' => SoundToken::Rank(c),
            _ => continue,
        };
        tokens.push(token);
    }
    tokens
}

/// `text` without the castling it starts with (`O-O-O` when `long`).
fn strip_castling(text: &str, long: bool) -> Option<&str> {
    let word = if long {
        ["O-O-O", "0-0-0"]
    } else {
        ["O-O", "0-0"]
    };
    word.iter().find_map(|w| text.strip_prefix(w))
}

/// The languages of the text of the announcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AnnounceLang {
    /// English (en-US).
    EnUs,
    /// Brazilian Portuguese (pt-BR).
    PtBr,
}

struct Words {
    pieces: [&'static str; 5],
    takes: &'static str,
    castle_short: &'static str,
    castle_long: &'static str,
    promotes: &'static str,
    check: &'static str,
    mate: &'static str,
    back: &'static str,
}

impl AnnounceLang {
    const fn words(self) -> Words {
        match self {
            AnnounceLang::EnUs => Words {
                pieces: ["King", "Queen", "Rook", "Bishop", "Knight"],
                takes: "takes",
                castle_short: "castles kingside",
                castle_long: "castles queenside",
                promotes: "promotes to",
                check: ", check",
                mate: ", checkmate",
                back: "back",
            },
            AnnounceLang::PtBr => Words {
                pieces: ["Rei", "Dama", "Torre", "Bispo", "Cavalo"],
                takes: "toma",
                castle_short: "roque pequeno",
                castle_long: "roque grande",
                promotes: "promove a",
                check: ", xeque",
                mate: ", xeque-mate",
                back: "voltar",
            },
        }
    }

    /// The word for the move taken back.
    #[must_use]
    pub const fn back_word(self) -> &'static str {
        self.words().back
    }
}

fn piece_index(c: char) -> Option<usize> {
    match c {
        'K' => Some(0),
        'Q' => Some(1),
        'R' => Some(2),
        'B' => Some(3),
        'N' => Some(4),
        _ => None,
    }
}

/// The move `san` in words: "Knight takes e5, check", "Cavalo toma e5,
/// xeque", "e8 promotes to Queen". Meant for the "last announcement" of the
/// sounds window, for a screen reader and for a future speech engine; the
/// text never fails: what is not understood is skipped.
#[must_use]
pub fn announce_text(san: &str, lang: AnnounceLang) -> String {
    let words = lang.words();
    let san: String = san.trim().chars().take(MAX_SAN_CHARS).collect();
    let mut parts: Vec<String> = Vec::new();
    if let Some(rest) = strip_castling(&san, true) {
        parts.push(words.castle_long.to_owned());
        return finish(&parts, rest, &words);
    }
    if let Some(rest) = strip_castling(&san, false) {
        parts.push(words.castle_short.to_owned());
        return finish(&parts, rest, &words);
    }
    let mut chars = san.chars().peekable();
    if let Some(index) = chars.peek().copied().and_then(piece_index) {
        parts.push(words.pieces[index].to_owned());
        chars.next();
    }
    let mut squares = String::new();
    let mut promotion = None;
    let mut takes = false;
    let mut after_equals = false;
    for c in chars {
        match c {
            'x' => takes = true,
            '=' => after_equals = true,
            c if after_equals => {
                if let Some(i) = piece_index(c) {
                    promotion = Some(i);
                }
            }
            'a'..='h' | '1'..='8' => squares.push(c),
            _ => {}
        }
    }
    // The last two characters are the destination; what comes before is the
    // file or rank that tells two pieces apart (or the file of a pawn that
    // captures).
    let (from, to) = squares.split_at(squares.len().saturating_sub(2));
    if !from.is_empty() {
        parts.push(from.to_owned());
    }
    if takes {
        parts.push(words.takes.to_owned());
    }
    if !to.is_empty() {
        parts.push(to.to_owned());
    }
    if let Some(i) = promotion {
        parts.push(format!("{} {}", words.promotes, words.pieces[i]));
    }
    finish(&parts, &san, &words)
}

/// The words joined, then ", check" or ", checkmate" when `marked` has the
/// marker.
fn finish(parts: &[String], marked: &str, words: &Words) -> String {
    let mut text = parts.join(" ");
    if marked.contains('#') {
        text.push_str(words.mate);
    } else if marked.contains('+') {
        text.push_str(words.check);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use SoundToken::*;

    #[test]
    fn tokens_follow_the_scid_sound_map() {
        assert_eq!(
            announce_tokens("Nf3+"),
            [Knight, File('f'), Rank('3'), Check]
        );
        assert_eq!(
            announce_tokens("exd5"),
            [File('e'), Takes, File('d'), Rank('5')]
        );
        assert_eq!(
            announce_tokens("e8=Q#"),
            [File('e'), Rank('8'), Promote, Queen, Mate]
        );
        assert_eq!(
            announce_tokens("Nbd7"),
            [Knight, File('b'), File('d'), Rank('7')]
        );
        assert_eq!(
            announce_tokens("R1a3"),
            [Rook, Rank('1'), File('a'), Rank('3')]
        );
        // A lower case b is the file, an upper case B is the bishop.
        assert_eq!(announce_tokens("Bb5"), [Bishop, File('b'), Rank('5')]);
        assert_eq!(announce_tokens("Kxe2"), [King, Takes, File('e'), Rank('2')]);
    }

    #[test]
    fn castling_replaces_the_letters() {
        assert_eq!(announce_tokens("O-O"), [CastleKingside]);
        assert_eq!(announce_tokens("O-O-O"), [CastleQueenside]);
        assert_eq!(announce_tokens("O-O+"), [CastleKingside, Check]);
        assert_eq!(announce_tokens("O-O-O#"), [CastleQueenside, Mate]);
    }

    #[test]
    fn nothing_to_say_gives_an_empty_queue() {
        assert!(announce_tokens("--").is_empty());
        assert!(announce_tokens("").is_empty());
        assert!(announce_tokens("!?").is_empty());
        assert!(announce_tokens(&"x".repeat(1000)).len() <= MAX_SAN_CHARS);
    }

    #[test]
    fn every_token_has_a_distinct_file_stem() {
        let mut stems: Vec<String> = SoundToken::all()
            .into_iter()
            .map(SoundToken::file_stem)
            .collect();
        let total = stems.len();
        assert_eq!(total, 12 + 8 + 8);
        stems.sort();
        stems.dedup();
        assert_eq!(stems.len(), total);
    }

    #[test]
    fn text_in_english() {
        let t = |s| announce_text(s, AnnounceLang::EnUs);
        assert_eq!(t("Nxe5+"), "Knight takes e5, check");
        assert_eq!(t("e4"), "e4");
        assert_eq!(t("exd5"), "e takes d5");
        assert_eq!(t("Nbd7"), "Knight b d7");
        assert_eq!(t("O-O"), "castles kingside");
        assert_eq!(t("O-O-O+"), "castles queenside, check");
        assert_eq!(t("e8=Q#"), "e8 promotes to Queen, checkmate");
        assert_eq!(t("Qh5#"), "Queen h5, checkmate");
    }

    #[test]
    fn text_in_portuguese() {
        let t = |s| announce_text(s, AnnounceLang::PtBr);
        assert_eq!(t("Nxe5+"), "Cavalo toma e5, xeque");
        assert_eq!(t("Bb5"), "Bispo b5");
        assert_eq!(t("O-O"), "roque pequeno");
        assert_eq!(t("O-O-O"), "roque grande");
        assert_eq!(t("a8=N+"), "a8 promove a Cavalo, xeque");
        assert_eq!(t("Qh5#"), "Dama h5, xeque-mate");
        assert_eq!(AnnounceLang::PtBr.back_word(), "voltar");
    }

    #[test]
    fn text_never_fails_on_garbage() {
        assert_eq!(announce_text("", AnnounceLang::EnUs), "");
        assert_eq!(announce_text("--", AnnounceLang::PtBr), "");
        drop(announce_text("\u{1F600}x=+#", AnnounceLang::EnUs));
    }
}
