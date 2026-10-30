//! Platform-specific window settings, kept small and in one place.

use iced::{Size, window};

/// Minimum window size (logical pixels).
pub(crate) const MIN_WINDOW_SIZE: Size = Size::new(800.0, 560.0);
/// Default window size when no size was saved.
pub(crate) const DEFAULT_WINDOW_SIZE: Size = Size::new(1280.0, 800.0);
/// Identifier used by Linux desktops to match the window to its launcher.
pub(crate) const APPLICATION_ID: &str = "mychess";

/// Side of the square window icon, in pixels.
const ICON_SIDE: u32 = 128;
/// The window icon (the MyChess knight, `assets/brand/mychess.svg`) rendered
/// to raw RGBA, so no image decoder is needed at run time.
const ICON_RGBA: &[u8] = include_bytes!("../../../assets/brand/icon/mychess-128.rgba");

/// The icon of the window; `None` if the bytes were not a valid RGBA image
/// (the window then keeps the default icon, never a failure).
pub(crate) fn window_icon() -> Option<window::Icon> {
    window::icon::from_rgba(ICON_RGBA.to_vec(), ICON_SIDE, ICON_SIDE).ok()
}

/// The window settings for the main window.
pub(crate) fn window_settings(size: Size) -> window::Settings {
    // `mut` is only needed on Linux (application id); the other targets warn.
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut settings = window::Settings {
        size: Size::new(
            size.width.max(MIN_WINDOW_SIZE.width),
            size.height.max(MIN_WINDOW_SIZE.height),
        ),
        min_size: Some(MIN_WINDOW_SIZE),
        exit_on_close_request: false,
        icon: window_icon(),
        ..window::Settings::default()
    };
    // Linux (X11 and Wayland) groups windows by application id; Windows and
    // macOS take the identity from the executable and need nothing here.
    #[cfg(target_os = "linux")]
    {
        APPLICATION_ID.clone_into(&mut settings.platform_specific.application_id);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = APPLICATION_ID;
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_has_the_mychess_icon() {
        assert_eq!(
            ICON_RGBA.len(),
            (ICON_SIDE * ICON_SIDE * 4) as usize,
            "the RGBA file must be {ICON_SIDE}x{ICON_SIDE} pixels of 4 bytes"
        );
        assert!(window_icon().is_some());
        assert!(window_settings(DEFAULT_WINDOW_SIZE).icon.is_some());
        // Rounded corners are transparent, the knight is opaque.
        assert_eq!(ICON_RGBA[3], 0, "top-left corner pixel");
        let opaque = ICON_RGBA
            .iter()
            .skip(3)
            .step_by(4)
            .filter(|alpha| **alpha == 255)
            .count();
        assert!(opaque > 10_000, "only {opaque} opaque pixels");
    }

    #[test]
    fn size_is_never_below_the_minimum() {
        let s = window_settings(Size::new(100.0, 100.0));
        assert_eq!(s.size, MIN_WINDOW_SIZE);
        assert!(!s.exit_on_close_request);
    }

    #[test]
    fn saved_size_is_kept_when_large_enough() {
        let s = window_settings(Size::new(1500.0, 900.0));
        assert_eq!(s.size, Size::new(1500.0, 900.0));
    }
}
