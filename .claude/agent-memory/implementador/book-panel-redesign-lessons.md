---
name: book-panel-redesign-lessons
description: M5-6c book panel redesign: Id::new is &'static str, tooltips are invisible to iced_test, Row::wrap with Fill children, snapshot refresh
metadata:
  type: project
---

- `iced::advanced::widget::Id::new` takes `&'static str`; use `Id::from(String)` for dynamic ids (button ids via `container(..).id(..)`).
- iced_test cannot see tooltip text (`point_at` + `find` finds nothing): prove icon-only buttons have tips by construction (helper that requires the tip key) plus a keys-exist test.
- `Row::wrap` lays out children with loose limits: a `Fill` child takes the whole line, so never put `space::horizontal()` inside a wrapped row; a card with `width(Fill).max_width(N)` wraps under its sibling below ~2N.
- `snapshot().matches_image(path)` keeps an existing `<name>-tiny-skia.png`; to refresh use a new output dir (rm of `$VAR/*` is blocked by the safety hook).
- Another agent's red tests (m2_4_i18n export-games plural, mychess-app games_export clippy) can appear mid-task; report, don't fix.
