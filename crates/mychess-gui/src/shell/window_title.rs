//! The title of the main window.

use fluent_bundle::FluentArgs;

use crate::i18n::I18n;

/// `{*}{file name or "Untitled"} - MyChess`, followed by
/// ` - Board: connected` while an electronic board is connected (D2743).
pub(super) fn window_title(
    i18n: &I18n,
    file_name: Option<&str>,
    modified: bool,
    board_connected: bool,
) -> String {
    let name = file_name.map_or_else(|| i18n.tr("window-untitled"), str::to_owned);
    let mut args = FluentArgs::new();
    args.set("name", name);
    args.set("modified", if modified { "yes" } else { "no" });
    let mut title = i18n.tr_args("window-title", &args);
    if board_connected {
        title.push_str(" - ");
        title.push_str(&i18n.tr("eboard-status-connected"));
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn titles_mark_modified_games_and_translate_untitled() {
        let en = I18n::new(Language::EnUs);
        assert_eq!(window_title(&en, None, false, false), "Untitled - MyChess");
        assert_eq!(
            window_title(&en, Some("a.pgn"), true, false),
            "*a.pgn - MyChess"
        );
        let pt = I18n::new(Language::PtBr);
        assert_eq!(
            window_title(&pt, None, true, false),
            "*Sem título - MyChess"
        );
    }

    #[test]
    fn titles_repeat_the_connected_board() {
        let en = I18n::new(Language::EnUs);
        assert_eq!(
            window_title(&en, None, false, true),
            "Untitled - MyChess - Board: connected"
        );
        let pt = I18n::new(Language::PtBr);
        assert_eq!(
            window_title(&pt, None, false, true),
            "Sem título - MyChess - Tabuleiro: conectado"
        );
    }
}
