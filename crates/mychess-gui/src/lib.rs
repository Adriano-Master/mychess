//! MyChess front-end: the iced application (library part, so that the
//! integration tests can reach it; the `mychess` binary only calls [`run`]).
//!
//! Layers (spec `docs/specs/m2-3-casca-e-tabuleiro.md`):
//! - [`widgets::board`]: the board canvas and its pure geometry,
//!   interaction, animation and mark helpers (the player strips around it
//!   are widgets of [`windows::board_column`] and [`windows::player_strip`]);
//! - [`shell`]: `Shell { app, options, ui }` and the MVU functions;
//! - [`windows`]: menu bar, panels, status bar;
//! - `app_api` (private): the only module that touches `mychess-app`.

mod app_api;
mod assets;
pub mod audio;
pub mod cli;
mod fonts;
pub mod host;
pub mod i18n;
pub mod keymap;
mod platform;
pub mod shell;
#[cfg(test)]
mod test_support;
pub mod theme;
pub mod widgets;
pub mod windows;

pub use app_api::{ViewChange, ViewOptions, default_preferences_path, peek_preferences};
pub use shell::{Message, Shell, UiMessage};

use std::path::PathBuf;

use iced::Size;

/// Runs the application until the window is closed.
///
/// `prefs_path` is the preferences file ([`default_preferences_path`]);
/// `None` runs with the defaults and saves nothing. The file is peeked at
/// once, synchronously and before the window exists, to size the window and
/// to draw the first frame in the saved theme and language (D134); the
/// application loads it again, asynchronously, right after. `launch` is what
/// the command line asked for ([`cli::start_info`]).
///
/// # Errors
/// Returns the iced error when the window or the renderer cannot start.
pub fn run(
    prefs_path: Option<PathBuf>,
    launch: Option<mychess_app::start::StartInfo>,
) -> iced::Result {
    let preloaded = prefs_path.as_deref().and_then(peek_preferences);
    let size = preloaded
        .as_ref()
        .and_then(|loaded| app_api::saved_window_size(&loaded.prefs))
        .map_or(platform::DEFAULT_WINDOW_SIZE, |(w, h)| Size::new(w, h));
    let mut app = iced::application(
        move || Shell::boot_with(prefs_path.clone(), preloaded.clone(), launch.clone()),
        Shell::update,
        Shell::view,
    )
    .title(Shell::title)
    .theme(Shell::theme)
    .subscription(Shell::subscription)
    .default_font(fonts::NOTO_SANS)
    .window(platform::window_settings(size));
    for font in fonts::font_data() {
        app = app.font(font);
    }
    app.run()
}
