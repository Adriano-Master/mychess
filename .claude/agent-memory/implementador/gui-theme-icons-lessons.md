---
name: gui-theme-icons-lessons
description: Verified iced 0.14 facts from M2-5a (svg color filter, button ids, responsive height, aw menu)
metadata:
  type: project
---

- `svg::Style::color` is a pixel filter (tiny-skia and wgpu replace RGB, keep alpha, ignore the color's alpha): Lucide `stroke="currentColor"` recolors fine; a dimmed icon needs `Svg::opacity`, not a translucent color. No plan B needed.
- iced 0.14 `button` has no `.id()`: wrap it in `container(..).id(Id)` and `Simulator::click(Id::new(..))` clicks the container center.
- `responsive(..).height(Length::Shrink)` sizes to the content when the parent has a bounded height; default is Fill.
- `Theme::custom` is `PartialEq` by name+palette; old shell tests comparing with `Theme::Dark` break. `Tokens::of` decides dark/light by `palette().background` luminance so built-in themes still map.
- Rendering tests: pixels via `tests/render_common` (the renderer draws on `Theme::Light`); resvg is not a direct dependency, so SVG parse checks go through the iced renderer.
- `I18n` is not `Clone` (Mutex): build tooltips outside `responsive` closures.
