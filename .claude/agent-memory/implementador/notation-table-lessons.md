---
name: notation-table-lessons
description: Verified facts from the M2-5 5c-gui notation table work (figurines, iced_test snapshots, hook, clippy)
metadata:
  type: project
---

- To SEE a widget: `Simulator::with_size(Settings { fonts, default_font: Font::with_name("Noto Sans"), .. }, size, el).snapshot(&theme)?.matches_image(path)` writes `<path>-wgpu.png` when the file does not exist (delete it first to refresh); open it with Read. Fonts must be passed in `Settings.fonts` (read `assets/fonts/*.ttf` by path in an integration test; `assets` is `pub(crate)`). Result at 15 px: Noto Sans Symbols 2 chess glyphs are legible but small and sit high; a size of 1.2x plus a 1.5 px downward nudge looks right, so no cburnett SVG was needed.
- A pill that recolors the text (accent + on_accent) without shaping on cursor moves means one tiny paragraph per part (figurine, SAN run, each NAG) drawn with `fill_paragraph(color)`; spans with explicit colors cannot be overridden at draw time.
- Move rows have fixed height, so only the comment/card rows need shaping at sync; move rows are shaped lazily in `ensure_shaped` (called after prepare and after every scroll change: wheel, thumb drag, track click).
- `large_types_passed_by_value` fires on `TextStyle` (palette makes it >256 bytes) for `NotationView::new`; kept by-value for API compat with `#[allow]`.
- The rustfmt hook formats one file as a crate root: a `mod x;` inside a non-`mod.rs` file needs `#[path = "dir/x.rs"]` or the hook reports "failed to resolve mod". `cargo fmt` itself is fine either way.
- Tests that shape real text must hold `m2_4_common::render_lock()` (parallel simulators SIGSEGV).
- Dark-theme card hover must stay as light as the `hover` token: the blunder tone (#ff7a70) drops under 4.5:1 on a 7% mix over `raised`.
- When another agent leaves the tree not compiling, a `tar` copy of the workspace into the scratchpad plus `CARGO_TARGET_DIR=<real target>` rebuilds only the workspace crates (deps are reused).
