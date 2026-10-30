//! Assets embedded in the binary: Fluent files, fonts and piece SVGs.

use crate::i18n::Language;

/// `(file name, contents)` of every Fluent file of a language.
// One table of `include_str!` per language: it grows with every Fluent file.
#[allow(clippy::too_many_lines)]
pub(crate) fn ftl_sources(language: Language) -> &'static [(&'static str, &'static str)] {
    match language {
        Language::EnUs => &[
            (
                "common.ftl",
                include_str!("../../../assets/i18n/en-US/common.ftl"),
            ),
            (
                "menu.ftl",
                include_str!("../../../assets/i18n/en-US/menu.ftl"),
            ),
            (
                "board.ftl",
                include_str!("../../../assets/i18n/en-US/board.ftl"),
            ),
            (
                "panes.ftl",
                include_str!("../../../assets/i18n/en-US/panes.ftl"),
            ),
            (
                "status.ftl",
                include_str!("../../../assets/i18n/en-US/status.ftl"),
            ),
            (
                "dialogs.ftl",
                include_str!("../../../assets/i18n/en-US/dialogs.ftl"),
            ),
            (
                "notation.ftl",
                include_str!("../../../assets/i18n/en-US/notation.ftl"),
            ),
            (
                "comment.ftl",
                include_str!("../../../assets/i18n/en-US/comment.ftl"),
            ),
            (
                "gameinfo.ftl",
                include_str!("../../../assets/i18n/en-US/gameinfo.ftl"),
            ),
            (
                "header.ftl",
                include_str!("../../../assets/i18n/en-US/header.ftl"),
            ),
            (
                "setup.ftl",
                include_str!("../../../assets/i18n/en-US/setup.ftl"),
            ),
            (
                "prompt.ftl",
                include_str!("../../../assets/i18n/en-US/prompt.ftl"),
            ),
            (
                "nav.ftl",
                include_str!("../../../assets/i18n/en-US/nav.ftl"),
            ),
            (
                "keys.ftl",
                include_str!("../../../assets/i18n/en-US/keys.ftl"),
            ),
            (
                "pieces.ftl",
                include_str!("../../../assets/i18n/en-US/pieces.ftl"),
            ),
            (
                "players.ftl",
                include_str!("../../../assets/i18n/en-US/players.ftl"),
            ),
            (
                "appearance.ftl",
                include_str!("../../../assets/i18n/en-US/appearance.ftl"),
            ),
            (
                "games.ftl",
                include_str!("../../../assets/i18n/en-US/games.ftl"),
            ),
            (
                "bookmarks.ftl",
                include_str!("../../../assets/i18n/en-US/bookmarks.ftl"),
            ),
            (
                "engines.ftl",
                include_str!("../../../assets/i18n/en-US/engines.ftl"),
            ),
            (
                "analysis.ftl",
                include_str!("../../../assets/i18n/en-US/analysis.ftl"),
            ),
            (
                "graph.ftl",
                include_str!("../../../assets/i18n/en-US/graph.ftl"),
            ),
            (
                "tree.ftl",
                include_str!("../../../assets/i18n/en-US/tree.ftl"),
            ),
            (
                "book.ftl",
                include_str!("../../../assets/i18n/en-US/book.ftl"),
            ),
            (
                "annotate.ftl",
                include_str!("../../../assets/i18n/en-US/annotate.ftl"),
            ),
            (
                "play.ftl",
                include_str!("../../../assets/i18n/en-US/play.ftl"),
            ),
            (
                "opening_trainer.ftl",
                include_str!("../../../assets/i18n/en-US/opening_trainer.ftl"),
            ),
            (
                "calvar.ftl",
                include_str!("../../../assets/i18n/en-US/calvar.ftl"),
            ),
            (
                "tablebase.ftl",
                include_str!("../../../assets/i18n/en-US/tablebase.ftl"),
            ),
            (
                "sound.ftl",
                include_str!("../../../assets/i18n/en-US/sound.ftl"),
            ),
            (
                "eboard.ftl",
                include_str!("../../../assets/i18n/en-US/eboard.ftl"),
            ),
            (
                "tactics.ftl",
                include_str!("../../../assets/i18n/en-US/tactics.ftl"),
            ),
            (
                "tournament.ftl",
                include_str!("../../../assets/i18n/en-US/tournament.ftl"),
            ),
            (
                "online.ftl",
                include_str!("../../../assets/i18n/en-US/online.ftl"),
            ),
            (
                "fics.ftl",
                include_str!("../../../assets/i18n/en-US/fics.ftl"),
            ),
            (
                "eco.ftl",
                include_str!("../../../assets/i18n/en-US/eco.ftl"),
            ),
            (
                "advanced_search.ftl",
                include_str!("../../../assets/i18n/en-US/advanced_search.ftl"),
            ),
            (
                "pattern_search.ftl",
                include_str!("../../../assets/i18n/en-US/pattern_search.ftl"),
            ),
            (
                "crosstab.ftl",
                include_str!("../../../assets/i18n/en-US/crosstab.ftl"),
            ),
            (
                "stats.ftl",
                include_str!("../../../assets/i18n/en-US/stats.ftl"),
            ),
            (
                "ptracker.ftl",
                include_str!("../../../assets/i18n/en-US/ptracker.ftl"),
            ),
            (
                "player_tools.ftl",
                include_str!("../../../assets/i18n/en-US/player_tools.ftl"),
            ),
            (
                "epd.ftl",
                include_str!("../../../assets/i18n/en-US/epd.ftl"),
            ),
            (
                "merge.ftl",
                include_str!("../../../assets/i18n/en-US/merge.ftl"),
            ),
            (
                "maint.ftl",
                include_str!("../../../assets/i18n/en-US/maint.ftl"),
            ),
            (
                "oprep.ftl",
                include_str!("../../../assets/i18n/en-US/oprep.ftl"),
            ),
            (
                "preport.ftl",
                include_str!("../../../assets/i18n/en-US/preport.ftl"),
            ),
            (
                "export.ftl",
                include_str!("../../../assets/i18n/en-US/export.ftl"),
            ),
            (
                "manual.ftl",
                include_str!("../../../assets/i18n/en-US/manual.ftl"),
            ),
            (
                "start.ftl",
                include_str!("../../../assets/i18n/en-US/start.ftl"),
            ),
        ],
        Language::PtBr => &[
            (
                "common.ftl",
                include_str!("../../../assets/i18n/pt-BR/common.ftl"),
            ),
            (
                "menu.ftl",
                include_str!("../../../assets/i18n/pt-BR/menu.ftl"),
            ),
            (
                "board.ftl",
                include_str!("../../../assets/i18n/pt-BR/board.ftl"),
            ),
            (
                "panes.ftl",
                include_str!("../../../assets/i18n/pt-BR/panes.ftl"),
            ),
            (
                "status.ftl",
                include_str!("../../../assets/i18n/pt-BR/status.ftl"),
            ),
            (
                "dialogs.ftl",
                include_str!("../../../assets/i18n/pt-BR/dialogs.ftl"),
            ),
            (
                "notation.ftl",
                include_str!("../../../assets/i18n/pt-BR/notation.ftl"),
            ),
            (
                "comment.ftl",
                include_str!("../../../assets/i18n/pt-BR/comment.ftl"),
            ),
            (
                "gameinfo.ftl",
                include_str!("../../../assets/i18n/pt-BR/gameinfo.ftl"),
            ),
            (
                "header.ftl",
                include_str!("../../../assets/i18n/pt-BR/header.ftl"),
            ),
            (
                "setup.ftl",
                include_str!("../../../assets/i18n/pt-BR/setup.ftl"),
            ),
            (
                "prompt.ftl",
                include_str!("../../../assets/i18n/pt-BR/prompt.ftl"),
            ),
            (
                "nav.ftl",
                include_str!("../../../assets/i18n/pt-BR/nav.ftl"),
            ),
            (
                "keys.ftl",
                include_str!("../../../assets/i18n/pt-BR/keys.ftl"),
            ),
            (
                "pieces.ftl",
                include_str!("../../../assets/i18n/pt-BR/pieces.ftl"),
            ),
            (
                "players.ftl",
                include_str!("../../../assets/i18n/pt-BR/players.ftl"),
            ),
            (
                "appearance.ftl",
                include_str!("../../../assets/i18n/pt-BR/appearance.ftl"),
            ),
            (
                "games.ftl",
                include_str!("../../../assets/i18n/pt-BR/games.ftl"),
            ),
            (
                "bookmarks.ftl",
                include_str!("../../../assets/i18n/pt-BR/bookmarks.ftl"),
            ),
            (
                "engines.ftl",
                include_str!("../../../assets/i18n/pt-BR/engines.ftl"),
            ),
            (
                "analysis.ftl",
                include_str!("../../../assets/i18n/pt-BR/analysis.ftl"),
            ),
            (
                "graph.ftl",
                include_str!("../../../assets/i18n/pt-BR/graph.ftl"),
            ),
            (
                "tree.ftl",
                include_str!("../../../assets/i18n/pt-BR/tree.ftl"),
            ),
            (
                "book.ftl",
                include_str!("../../../assets/i18n/pt-BR/book.ftl"),
            ),
            (
                "annotate.ftl",
                include_str!("../../../assets/i18n/pt-BR/annotate.ftl"),
            ),
            (
                "play.ftl",
                include_str!("../../../assets/i18n/pt-BR/play.ftl"),
            ),
            (
                "opening_trainer.ftl",
                include_str!("../../../assets/i18n/pt-BR/opening_trainer.ftl"),
            ),
            (
                "calvar.ftl",
                include_str!("../../../assets/i18n/pt-BR/calvar.ftl"),
            ),
            (
                "tablebase.ftl",
                include_str!("../../../assets/i18n/pt-BR/tablebase.ftl"),
            ),
            (
                "sound.ftl",
                include_str!("../../../assets/i18n/pt-BR/sound.ftl"),
            ),
            (
                "eboard.ftl",
                include_str!("../../../assets/i18n/pt-BR/eboard.ftl"),
            ),
            (
                "tactics.ftl",
                include_str!("../../../assets/i18n/pt-BR/tactics.ftl"),
            ),
            (
                "tournament.ftl",
                include_str!("../../../assets/i18n/pt-BR/tournament.ftl"),
            ),
            (
                "online.ftl",
                include_str!("../../../assets/i18n/pt-BR/online.ftl"),
            ),
            (
                "fics.ftl",
                include_str!("../../../assets/i18n/pt-BR/fics.ftl"),
            ),
            (
                "eco.ftl",
                include_str!("../../../assets/i18n/pt-BR/eco.ftl"),
            ),
            (
                "advanced_search.ftl",
                include_str!("../../../assets/i18n/pt-BR/advanced_search.ftl"),
            ),
            (
                "pattern_search.ftl",
                include_str!("../../../assets/i18n/pt-BR/pattern_search.ftl"),
            ),
            (
                "crosstab.ftl",
                include_str!("../../../assets/i18n/pt-BR/crosstab.ftl"),
            ),
            (
                "stats.ftl",
                include_str!("../../../assets/i18n/pt-BR/stats.ftl"),
            ),
            (
                "ptracker.ftl",
                include_str!("../../../assets/i18n/pt-BR/ptracker.ftl"),
            ),
            (
                "player_tools.ftl",
                include_str!("../../../assets/i18n/pt-BR/player_tools.ftl"),
            ),
            (
                "epd.ftl",
                include_str!("../../../assets/i18n/pt-BR/epd.ftl"),
            ),
            (
                "merge.ftl",
                include_str!("../../../assets/i18n/pt-BR/merge.ftl"),
            ),
            (
                "maint.ftl",
                include_str!("../../../assets/i18n/pt-BR/maint.ftl"),
            ),
            (
                "oprep.ftl",
                include_str!("../../../assets/i18n/pt-BR/oprep.ftl"),
            ),
            (
                "preport.ftl",
                include_str!("../../../assets/i18n/pt-BR/preport.ftl"),
            ),
            (
                "export.ftl",
                include_str!("../../../assets/i18n/pt-BR/export.ftl"),
            ),
            (
                "manual.ftl",
                include_str!("../../../assets/i18n/pt-BR/manual.ftl"),
            ),
            (
                "start.ftl",
                include_str!("../../../assets/i18n/pt-BR/start.ftl"),
            ),
        ],
    }
}

/// Noto Sans Regular (SIL OFL 1.1, see `assets/LICENSES.md`).
pub(crate) const NOTO_SANS_REGULAR: &[u8] =
    include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf");
/// Noto Sans Bold.
pub(crate) const NOTO_SANS_BOLD: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Bold.ttf");
/// Noto Sans Math: arrows and operators of the NAG symbols (SIL OFL 1.1).
pub(crate) const NOTO_SANS_MATH: &[u8] =
    include_bytes!("../../../assets/fonts/NotoSansMath-Regular.ttf");
/// Noto Sans Symbols 2: the geometric and braille NAG symbols (SIL OFL 1.1).
pub(crate) const NOTO_SANS_SYMBOLS2: &[u8] =
    include_bytes!("../../../assets/fonts/NotoSansSymbols2-Regular.ttf");

/// Expands to the twelve `(file stem, SVG bytes)` pairs of the piece set
/// stored in `assets/pieces/<set>/`, in the order white then black, king,
/// queen, rook, bishop, knight, pawn (the index order of `PieceSet`).
macro_rules! piece_files {
    ($set:literal) => {
        [
            (
                "wK",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/wK.svg")),
            ),
            (
                "wQ",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/wQ.svg")),
            ),
            (
                "wR",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/wR.svg")),
            ),
            (
                "wB",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/wB.svg")),
            ),
            (
                "wN",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/wN.svg")),
            ),
            (
                "wP",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/wP.svg")),
            ),
            (
                "bK",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/bK.svg")),
            ),
            (
                "bQ",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/bQ.svg")),
            ),
            (
                "bR",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/bR.svg")),
            ),
            (
                "bB",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/bB.svg")),
            ),
            (
                "bN",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/bN.svg")),
            ),
            (
                "bP",
                include_bytes!(concat!("../../../assets/pieces/", $set, "/bP.svg")),
            ),
        ]
    };
}

/// The files of a piece set: `(file stem, SVG bytes)` for the 12 pieces.
pub(crate) type PieceFiles = [(&'static str, &'static [u8]); 12];

/// Colin M.L. Burnett's set (GPL-2.0-or-later, see `assets/LICENSES.md`).
pub(crate) const CBURNETT: PieceFiles = piece_files!("cburnett");
/// Armando Hernandez Marroquin's Merida (GPL-2.0-or-later).
pub(crate) const MERIDA: PieceFiles = piece_files!("merida");
/// Alexis Luengas' Chessnut (Apache-2.0).
pub(crate) const CHESSNUT: PieceFiles = piece_files!("chessnut");
/// `RhosGFX` (CC0-1.0).
pub(crate) const RHOSGFX: PieceFiles = piece_files!("rhosgfx");
/// Maurizio Monge's Fantasy (MIT).
pub(crate) const FANTASY: PieceFiles = piece_files!("fantasy");
/// Maurizio Monge's Spatial (MIT).
pub(crate) const SPATIAL: PieceFiles = piece_files!("spatial");
/// Maurizio Monge's Celtic (MIT).
pub(crate) const CELTIC: PieceFiles = piece_files!("celtic");
/// Maxime Chupin's `MPChess` (GPL-3.0-or-later).
pub(crate) const MPCHESS: PieceFiles = piece_files!("mpchess");

/// Expands to the `(file stem, SVG bytes)` pairs of the Lucide icons stored
/// in `assets/icons/lucide/` (ISC and MIT, see `assets/LICENSES.md`), in the
/// order of the `Icon` enum.
macro_rules! lucide_files {
    ($($name:literal),+ $(,)?) => {
        [$((
            $name,
            include_bytes!(concat!("../../../assets/icons/lucide/", $name, ".svg")).as_slice(),
        )),+]
    };
}

/// Number of embedded icons.
pub(crate) const ICON_COUNT: usize = 106;

/// The Lucide icons: `(file stem, SVG bytes)`.
pub(crate) const LUCIDE_ICONS: [(&str, &[u8]); ICON_COUNT] = lucide_files!(
    "chevron-first",
    "chevron-left",
    "play",
    "pause",
    "chevron-right",
    "chevron-last",
    "arrow-down-up",
    "undo-2",
    "redo-2",
    "git-branch-plus",
    "corner-left-up",
    "git-fork",
    "flask-conical",
    "ellipsis",
    "sliders-horizontal",
    "copy",
    "save",
    "user-round",
    "grip-vertical",
    "search",
    "list-filter",
    "rotate-ccw",
    "arrow-left-right",
    "list-x",
    "list-checks",
    "columns-3",
    "chevron-up",
    "chevron-down",
    "trash",
    "x",
    "eye",
    "download",
    "zoom-in",
    "zoom-out",
    "file-plus",
    "folder-open",
    "clock",
    "bookmark",
    "bookmark-plus",
    "settings-2",
    "database",
    "eraser",
    "lock",
    "arrow-right",
    "arrow-left",
    "save-all",
    "file-down",
    "log-out",
    "clipboard-copy",
    "clipboard-paste",
    "import",
    "square-pen",
    "scissors",
    "message-square-off",
    "git-branch",
    "arrow-left-to-line",
    "arrow-right-to-line",
    "skip-back",
    "step-back",
    "step-forward",
    "skip-forward",
    "hash",
    "info",
    "bot",
    "trophy",
    "list-ordered",
    "sun-moon",
    "palette",
    "layout-grid",
    "crown",
    "grid-3x3",
    "scale",
    "users",
    "scroll-text",
    "circle-dot",
    "highlighter",
    "target",
    "pen-line",
    "timer",
    "chart-bar",
    "gauge",
    "move-up-right",
    "layout-panel-left",
    "layout-template",
    "panel-bottom",
    "languages",
    "ruler",
    "cpu",
    "zap",
    "wand-sparkles",
    "activity",
    "chart-line",
    "network",
    "file-text",
    "plug",
    "swords",
    "inbox",
    "unplug",
    "lock-open",
    "upload",
    "flag",
    "rotate-cw",
    "git-merge",
    "star",
    "star-off",
    "book-open",
);

/// The chapters of the manual in a language, in the order of
/// `mychess_app::manual::CHAPTER_FILES`; `None` when the language has no
/// manual yet (spec M9-5, D4054, D4063).
pub(crate) fn manual_chapters(language: Language) -> Option<&'static [&'static str; 15]> {
    match language {
        Language::EnUs => None,
        Language::PtBr => Some(&[
            include_str!("../../../assets/manual/pt-BR/00-indice.md"),
            include_str!("../../../assets/manual/pt-BR/01-primeiros-passos.md"),
            include_str!("../../../assets/manual/pt-BR/02-tabuleiro-e-partida.md"),
            include_str!("../../../assets/manual/pt-BR/03-bancos-de-dados.md"),
            include_str!("../../../assets/manual/pt-BR/04-buscas.md"),
            include_str!("../../../assets/manual/pt-BR/05-aberturas.md"),
            include_str!("../../../assets/manual/pt-BR/06-engines-e-analise.md"),
            include_str!("../../../assets/manual/pt-BR/07-jogar.md"),
            include_str!("../../../assets/manual/pt-BR/08-treinar.md"),
            include_str!("../../../assets/manual/pt-BR/09-jogadores-e-torneios.md"),
            include_str!("../../../assets/manual/pt-BR/10-manutencao-e-relatorios.md"),
            include_str!("../../../assets/manual/pt-BR/11-aparencia-e-preferencias.md"),
            include_str!("../../../assets/manual/pt-BR/12-atalhos-de-teclado.md"),
            include_str!("../../../assets/manual/pt-BR/13-perguntas-frequentes.md"),
            include_str!("../../../assets/manual/pt-BR/14-glossario.md"),
        ]),
    }
}
