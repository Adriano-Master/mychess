//! The icons of the interface: Lucide SVG files embedded in the binary
//! (spec M2-5, D223, D224), colored by the theme.
//!
//! The renderer of iced applies [`svg::Style::color`] as a filter on the
//! pixels (it replaces the red, green and blue of every pixel and keeps the
//! alpha), so the `stroke="currentColor"` of the files, which resvg draws in
//! black, follows the theme without touching the files. The alpha of the
//! color is ignored by the filter: a dimmed icon uses the opacity of the
//! widget instead.

use std::sync::LazyLock;

use iced::widget::svg::{self, Svg};
use iced::{Color, Element, Length, Theme};

use crate::assets::{ICON_COUNT, LUCIDE_ICONS};
use crate::theme::Tokens;
use crate::theme::tokens::DISABLED_ALPHA;

/// An icon of the registry (Lucide names in the docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    /// `chevron-first`: go to the start of the game.
    First,
    /// `chevron-left`: one move back.
    Back,
    /// `play`: start the autoplay.
    Play,
    /// `pause`: stop the autoplay.
    Pause,
    /// `chevron-right`: one move forward.
    Forward,
    /// `chevron-last`: go to the end of the game.
    Last,
    /// `arrow-down-up`: flip the board.
    Flip,
    /// `undo-2`.
    Undo,
    /// `redo-2`.
    Redo,
    /// `git-branch-plus`: add a variation.
    AddVariation,
    /// `corner-left-up`: leave the variation.
    ExitVariation,
    /// `git-fork`: choose a variation.
    EnterVariation,
    /// `flask-conical`: trial mode.
    Trial,
    /// `ellipsis`: more commands.
    More,
    /// `sliders-horizontal`: display options.
    Options,
    /// `copy`.
    Copy,
    /// `save`.
    Save,
    /// `user-round`: the generic player avatar.
    Avatar,
    /// `grip-vertical`: the handle of a pane.
    Grip,
    /// `search`: Find.
    Search,
    /// `list-filter`: reduce the filter by text.
    FilterList,
    /// `rotate-ccw`: reset the filter.
    ResetFilter,
    /// `arrow-left-right`: negate the filter.
    Negate,
    /// `list-x`: remove from the filter.
    RemoveFromFilter,
    /// `list-checks`: keep only the selected games.
    KeepSelected,
    /// `columns-3`: choose the columns.
    Columns,
    /// `chevron-up`: ascending sort.
    SortUp,
    /// `chevron-down`: descending sort.
    SortDown,
    /// `trash`: flag the games as deleted.
    Trash,
    /// `x`: cancel or close.
    Close,
    /// `eye`: browse the games.
    Browse,
    /// `download`: load a game into the main board.
    Load,
    /// `zoom-in`: larger boards.
    ZoomIn,
    /// `zoom-out`: smaller boards.
    ZoomOut,
    /// `file-plus`: new game or file.
    FileNew,
    /// `folder-open`: open a file.
    FolderOpen,
    /// `clock`: recent files.
    Recent,
    /// `bookmark`: bookmarks.
    Bookmark,
    /// `bookmark-plus`: add a bookmark.
    BookmarkAdd,
    /// `settings-2`: manage a list.
    Manage,
    /// `database`: databases.
    Database,
    /// `eraser`: clear.
    Eraser,
    /// `lock`: read-only.
    Lock,
    /// `arrow-right`: next.
    ArrowRight,
    /// `arrow-left`: previous.
    ArrowLeft,
    /// `save-all`: save as.
    SaveAs,
    /// `file-down`: append to a PGN file.
    AppendFile,
    /// `log-out`: quit.
    Quit,
    /// `clipboard-copy`: copy to the clipboard.
    ClipboardCopy,
    /// `clipboard-paste`: paste from the clipboard.
    ClipboardPaste,
    /// `import`: import.
    Import,
    /// `square-pen`: edit the position.
    SetupBoard,
    /// `scissors`: strip.
    Scissors,
    /// `message-square-off`: remove comments.
    NoComments,
    /// `git-branch`: variations.
    Branch,
    /// `arrow-left-to-line`: truncate before.
    TruncateStart,
    /// `arrow-right-to-line`: truncate after.
    TruncateEnd,
    /// `skip-back`: first game.
    FirstGame,
    /// `step-back`: previous game.
    PrevGame,
    /// `step-forward`: next game.
    NextGame,
    /// `skip-forward`: last game.
    LastGame,
    /// `hash`: go to a move number.
    Hash,
    /// `info`: information.
    Info,
    /// `bot`: play an engine.
    Bot,
    /// `trophy`: tournament.
    Trophy,
    /// `list-ordered`: ordered list.
    ListOrdered,
    /// `sun-moon`: theme.
    ThemeMode,
    /// `palette`: colors.
    Palette,
    /// `layout-grid`: board theme.
    BoardGrid,
    /// `crown`: piece set.
    Pieces,
    /// `grid-3x3`: coordinates.
    Coords,
    /// `scale`: material.
    Material,
    /// `users`: players.
    Users,
    /// `scroll-text`: notation.
    Notation,
    /// `circle-dot`: side to move.
    SideToMove,
    /// `highlighter`: last move.
    Highlight,
    /// `target`: legal moves.
    Target,
    /// `pen-line`: marks.
    Marks,
    /// `timer`: animation.
    Timer,
    /// `chart-bar`: evaluation bar.
    EvalBar,
    /// `gauge`: traffic lights.
    Gauge,
    /// `move-up-right`: engine arrows.
    Arrow,
    /// `layout-panel-left`: panels.
    Panes,
    /// `layout-template`: reset the layout.
    ResetLayout,
    /// `panel-bottom`: status bar.
    StatusBar,
    /// `languages`: language.
    Language,
    /// `ruler`: scale or length.
    Ruler,
    /// `cpu`: engines.
    Engine,
    /// `zap`: engine analysis.
    Analysis,
    /// `wand-sparkles`: annotate.
    Annotate,
    /// `activity`: analysis window.
    Activity,
    /// `chart-line`: evaluation graph.
    Graph,
    /// `network`: opening tree.
    Tree,
    /// `file-text`: report.
    Report,
    /// `plug`: connect.
    Connect,
    /// `swords`: challenge.
    Challenge,
    /// `inbox`: incoming challenges.
    Inbox,
    /// `unplug`: disconnect.
    Disconnect,
    /// `lock-open`: remove the lock.
    Unlock,
    /// `upload`: export games to a file.
    Upload,
    /// `flag`: resign the game.
    Resign,
    /// `rotate-cw`: play again against the same opponent.
    Rematch,
    /// `git-merge`: merge a game into the current one.
    Merge,
    /// `star`: a favorite report.
    Star,
    /// `star-off`: no favorite report to use.
    StarOff,
    /// `book-open`: the manual.
    BookOpen,
}

impl Icon {
    /// Every icon, in registry order.
    pub const ALL: [Icon; ICON_COUNT] = [
        Icon::First,
        Icon::Back,
        Icon::Play,
        Icon::Pause,
        Icon::Forward,
        Icon::Last,
        Icon::Flip,
        Icon::Undo,
        Icon::Redo,
        Icon::AddVariation,
        Icon::ExitVariation,
        Icon::EnterVariation,
        Icon::Trial,
        Icon::More,
        Icon::Options,
        Icon::Copy,
        Icon::Save,
        Icon::Avatar,
        Icon::Grip,
        Icon::Search,
        Icon::FilterList,
        Icon::ResetFilter,
        Icon::Negate,
        Icon::RemoveFromFilter,
        Icon::KeepSelected,
        Icon::Columns,
        Icon::SortUp,
        Icon::SortDown,
        Icon::Trash,
        Icon::Close,
        Icon::Browse,
        Icon::Load,
        Icon::ZoomIn,
        Icon::ZoomOut,
        Icon::FileNew,
        Icon::FolderOpen,
        Icon::Recent,
        Icon::Bookmark,
        Icon::BookmarkAdd,
        Icon::Manage,
        Icon::Database,
        Icon::Eraser,
        Icon::Lock,
        Icon::ArrowRight,
        Icon::ArrowLeft,
        Icon::SaveAs,
        Icon::AppendFile,
        Icon::Quit,
        Icon::ClipboardCopy,
        Icon::ClipboardPaste,
        Icon::Import,
        Icon::SetupBoard,
        Icon::Scissors,
        Icon::NoComments,
        Icon::Branch,
        Icon::TruncateStart,
        Icon::TruncateEnd,
        Icon::FirstGame,
        Icon::PrevGame,
        Icon::NextGame,
        Icon::LastGame,
        Icon::Hash,
        Icon::Info,
        Icon::Bot,
        Icon::Trophy,
        Icon::ListOrdered,
        Icon::ThemeMode,
        Icon::Palette,
        Icon::BoardGrid,
        Icon::Pieces,
        Icon::Coords,
        Icon::Material,
        Icon::Users,
        Icon::Notation,
        Icon::SideToMove,
        Icon::Highlight,
        Icon::Target,
        Icon::Marks,
        Icon::Timer,
        Icon::EvalBar,
        Icon::Gauge,
        Icon::Arrow,
        Icon::Panes,
        Icon::ResetLayout,
        Icon::StatusBar,
        Icon::Language,
        Icon::Ruler,
        Icon::Engine,
        Icon::Analysis,
        Icon::Annotate,
        Icon::Activity,
        Icon::Graph,
        Icon::Tree,
        Icon::Report,
        Icon::Connect,
        Icon::Challenge,
        Icon::Inbox,
        Icon::Disconnect,
        Icon::Unlock,
        Icon::Upload,
        Icon::Resign,
        Icon::Rematch,
        Icon::Merge,
        Icon::Star,
        Icon::StarOff,
        Icon::BookOpen,
    ];

    fn index(self) -> usize {
        self as usize
    }

    /// The file stem in `assets/icons/lucide/`.
    pub fn file_stem(self) -> &'static str {
        LUCIDE_ICONS.get(self.index()).map_or("", |(name, _)| name)
    }

    /// The SVG source.
    pub fn bytes(self) -> &'static [u8] {
        LUCIDE_ICONS
            .get(self.index())
            .map_or(&[], |(_, bytes)| bytes)
    }

    /// The handle of the icon, created once.
    pub fn handle(self) -> svg::Handle {
        static HANDLES: LazyLock<Vec<svg::Handle>> = LazyLock::new(|| {
            LUCIDE_ICONS
                .iter()
                .map(|(_, bytes)| svg::Handle::from_memory(*bytes))
                .collect()
        });
        HANDLES
            .get(self.index())
            .cloned()
            .unwrap_or_else(|| svg::Handle::from_memory(&[][..]))
    }
}

/// The role of an icon color, resolved against the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconTone {
    /// The main text color.
    Text,
    /// The secondary color.
    Muted,
    /// The accent.
    Accent,
    /// Over an accent background.
    OnAccent,
    /// A disabled control: muted at half opacity (D227).
    Disabled,
    /// The danger tint.
    Danger,
}

impl IconTone {
    /// The color of the tone.
    pub fn color(self, tokens: &Tokens) -> Color {
        match self {
            IconTone::Text => tokens.text,
            IconTone::Muted | IconTone::Disabled => tokens.muted,
            IconTone::Accent => tokens.accent,
            IconTone::OnAccent => tokens.on_accent,
            IconTone::Danger => tokens.danger,
        }
    }

    /// The opacity of the tone.
    pub fn opacity(self) -> f32 {
        if self == IconTone::Disabled {
            DISABLED_ALPHA
        } else {
            1.0
        }
    }
}

/// An icon of `size` logical pixels, colored by `tone`.
pub fn icon<'a, M: 'a>(icon: Icon, size: f32, tone: IconTone) -> Element<'a, M> {
    Svg::<Theme>::new(icon.handle())
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .opacity(tone.opacity())
        .style(move |theme: &Theme, _status| svg::Style {
            color: Some(tone.color(&Tokens::of(theme))),
        })
        .into()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn licenses() -> String {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/LICENSES.md");
        std::fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    #[allow(clippy::too_many_lines)] // a flat list of file stems
    fn registry_order_matches_the_enum() {
        let expected = [
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
        ];
        for (icon, name) in Icon::ALL.into_iter().zip(expected) {
            assert_eq!(icon.file_stem(), name);
        }
        let unique: HashSet<_> = Icon::ALL.iter().map(|i| i.file_stem()).collect();
        assert_eq!(unique.len(), Icon::ALL.len());
    }

    #[test]
    fn every_icon_is_a_plain_svg_without_scripts_or_external_references() {
        for icon in Icon::ALL {
            let text = std::str::from_utf8(icon.bytes()).expect("utf-8 svg");
            assert!(text.contains("<svg"), "{icon:?}");
            assert!(text.contains("stroke=\"currentColor\""), "{icon:?}");
            let lower = text.to_ascii_lowercase();
            assert!(!lower.contains("<script"), "{icon:?}");
            assert!(!lower.contains("href"), "{icon:?}");
            assert!(!lower.contains("xlink"), "{icon:?}");
        }
    }

    #[test]
    fn every_file_of_the_folder_is_registered_and_licensed() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/lucide");
        let licenses = licenses();
        let mut on_disk = HashSet::new();
        for entry in std::fs::read_dir(&dir)
            .expect("icon folder")
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("svg"))
            {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_owned();
                assert!(
                    licenses.contains(&format!("`icons/lucide/{stem}.svg`")),
                    "{stem} has no row in assets/LICENSES.md"
                );
                on_disk.insert(stem);
            }
        }
        let registered: HashSet<String> =
            Icon::ALL.iter().map(|i| i.file_stem().to_owned()).collect();
        assert_eq!(on_disk, registered);
        assert!(dir.join("LICENSE").is_file());
    }

    #[test]
    fn the_license_text_has_both_notices() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/lucide");
        let text = std::fs::read_to_string(dir.join("LICENSE")).unwrap_or_default();
        assert!(text.contains("ISC License"));
        assert!(text.contains("The MIT License (MIT)"));
    }

    #[test]
    fn tones_resolve_against_the_tokens() {
        let t = Tokens::dark();
        assert_eq!(IconTone::Text.color(&t), t.text);
        assert_eq!(IconTone::OnAccent.color(&t), t.on_accent);
        assert_eq!(IconTone::Disabled.color(&t), t.muted);
        assert!((IconTone::Disabled.opacity() - 0.5).abs() < f32::EPSILON);
        assert!((IconTone::Text.opacity() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn handles_are_shared() {
        assert_eq!(Icon::Play.handle(), Icon::Play.handle());
        assert_ne!(Icon::Play.handle(), Icon::Pause.handle());
    }
}
