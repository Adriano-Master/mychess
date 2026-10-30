---
name: gui-analysis-lessons
description: Non-obvious facts from the M4-4 analysis panel GUI (pane_grid on_click side effect, Fluent leading blanks, EngineRun not constructible)
metadata:
  type: project
---

- `pane_grid::on_click` publishes a message on every left press in any pane, which adds a stray message to the many full-window contract tests (`m3_4c_toolbar`, `m3_4e_keys`) that assert "a disabled button sends nothing": use pointer hover (`mouse_area` enter/exit) for panel focus instead.
- `EngineRun` has private fields: GUI sections take a plain `RunInfo` struct so tests can build one. `Protocol::XBoard` (capital B). The GUI crate needs `mychess-engine` as a dev-dependency to build `Score`/`LogEntry` in tests (arch check does not restrict dev-deps).
- Fluent trims leading blanks of a value: `key =  (text)` loses the space; push the space in code.
- `tests/m4_3_gui_i18n.rs` scans `menu_bar.rs` for `menu-tools*` keys and only looks in engines/menu/dialogs/common/status `.ftl`: new menu-tools keys go in `menu.ftl`. `m4_3_gui_keys_menu.rs` asserts the exact Tools menu shape.
- Python edits bypass the rustfmt hook: run `rustfmt --edition 2024 <files>` on touched files.
