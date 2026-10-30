---
name: gui-evalbar-lessons
description: M4-5 GUI: contract tests build BoardData/BoardColumn/RunInfo by literal, so extend with builders or defaulted wrappers
metadata:
  type: project
---

- Tests in `crates/mychess-gui/tests/` build `BoardData {..}`, `BoardColumn {..}` and match `BoardEvent` exhaustively: add data through builders (`BoardWidget::with_engine_arrows`, `BoardColumn::layout_with`) rather than new struct fields; a new `BoardEvent` variant breaks `m2_3_interaction.rs` (one-line fix).
- Calling `canvas::Program::update` directly in unit tests works (`Action::into_inner().0` gives the published message); a press taken with `Action::capture()` keeps the interaction idle.
- Other agents' WIP (`windows/graph*`, tools menu shape, `Command+Shift+L` vs `Command+L` shortcut conflict in `m3_4b_keys_layout`) can turn the suite red; list them, do not fix.
