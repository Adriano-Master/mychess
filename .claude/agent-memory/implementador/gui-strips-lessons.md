---
name: gui-strips-lessons
description: Verified facts from M2-5b (player strips, column_metrics, parallel-agent build collisions)
metadata:
  type: project
---

- `BoardLayout::compute` must start its search at `floor(fit)+1`: with `Outside` margins `8s + 2*round(0.3 s)` can round down, so a canvas built for `s` would otherwise give back `s-1`. `column_metrics` relies on "canvas of side(s) gives back s" (strip width == board width).
- iced_test: a custom selector is a closure `FnMut(iced_test::selector::Candidate) -> Option<T>` (there is no `text_contains`); `Target::bounds()` gives layout bounds, and `container(..).id("x")` makes any block findable by `widget::Id`.
- Other agents (5c-gui) build from a mirror workspace that shares `target/`: test binaries of `mychess-gui` can be overwritten with a stale copy (symptom: panic line numbers from a source you already changed, path under scratchpad/ws). `touch crates/mychess-gui/src/lib.rs` and rerun.
- Text width without a measurer: estimate `chars * size * 0.66` for Noto Sans Bold and keep ~8 px of slack; 0.6 clipped the `+N` of compact strips in a pixel check.
