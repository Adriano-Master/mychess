//! Headless rasterizer for the pixel tests: renders an element with the same
//! renderer as `iced_test::Simulator` but hands back the RGBA bytes (the
//! simulator only exposes hashes). Compared within one run, so differences
//! between machines do not matter.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use std::path::Path;
use std::sync::{Mutex, Once, PoisonError};

use iced_test::core::renderer::{Headless, Style};
use iced_test::core::{Element, Event, Font, Pixels, Size, Theme, clipboard, mouse, time, window};
use iced_test::renderer::Renderer;
use iced_test::runtime::UserInterface;
use iced_test::runtime::user_interface::Cache;

/// Device pixels per logical pixel in [`render`].
pub(crate) const SCALE: f32 = 2.0;

/// A rendered frame.
pub(crate) struct Shot {
    /// Width in device pixels.
    pub width: u32,
    rgba: Vec<u8>,
}

impl Shot {
    /// The RGBA value of the device pixel `(x, y)`.
    pub(crate) fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = 4 * (y as usize * self.width as usize + x as usize);
        let mut out = [0; 4];
        out.copy_from_slice(&self.rgba[at..at + 4]);
        out
    }
}

/// One renderer at a time: creating headless renderers on several test
/// threads at once crashes the graphics backend.
static RENDER_LOCK: Mutex<()> = Mutex::new(());
static FONT: Once = Once::new();

/// Registers the bold interface font, so labels do not depend on the fonts
/// of the machine.
fn load_interface_font() {
    FONT.call_once(register_interface_font);
}

fn register_interface_font() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/NotoSans-Bold.ttf");
    let bytes = std::fs::read(&path).expect("the embedded font file");
    iced_test::renderer::graphics::text::font_system()
        .write()
        .expect("font system")
        .load_font(bytes.into());
}

/// Renders `element` in a window of `size` logical pixels.
pub(crate) fn render<'a, M>(
    element: impl Into<Element<'a, M, Theme, Renderer>>,
    size: Size,
) -> Shot {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    load_interface_font();
    let mut renderer = iced_test::futures::futures::executor::block_on(Renderer::new(
        Font::with_name("Noto Sans"),
        Pixels(16.0),
        std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
    ))
    .expect("a headless renderer");
    let mut ui = UserInterface::build(element, size, Cache::default(), &mut renderer);
    let mut messages = Vec::new();
    drop(ui.update(
        &[Event::Window(window::Event::RedrawRequested(
            time::Instant::now(),
        ))],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    ));
    let theme = Theme::Light;
    ui.draw(
        &mut renderer,
        &theme,
        &Style {
            text_color: theme.palette().text,
        },
        mouse::Cursor::Unavailable,
    );
    let physical = Size::new(
        (size.width * SCALE).round() as u32,
        (size.height * SCALE).round() as u32,
    );
    let rgba = renderer.screenshot(physical, SCALE, theme.palette().background);
    Shot {
        width: physical.width,
        rgba,
    }
}
