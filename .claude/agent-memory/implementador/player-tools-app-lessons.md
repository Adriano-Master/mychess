---
name: player-tools-app-lessons
description: M6-3b lessons - ListJobKind is matched exhaustively by the GUI, [players] prefs table taken, panel_calc::Refresh reuse, hook reformat breaks scripted patches
metadata:
  type: feedback
---

- A new `ListJobKind` variant breaks the exhaustive `match` in `mychess-gui/.../toolbar.rs`; M6-3b reused `ListJobKind::Flags` for the name/Elo writes (it also gives `target_path` protection in `start_job`). A slice that may not touch the GUI must do the same and leave the label to the GUI slice.
- `[players]` in `preferences.toml` is the player strips (M2-5); the spec's `[players]` keys went to `[player_tools]`.
- `crate::panel_calc::Refresh<K>` (key, debounce, request, cancel) is the ready-made engine for any "recompute when the state changes" window; model: `state/crosstable_flow.rs`.
- Do not add effects to `PreferencesLoaded`/`Startup`: `flow_tests` compares those effect lists exactly. Use a host message (`SpellingMessage::LoadAtStart`).
- The rustfmt hook reflows imports right after a Write, so a later scripted `replace` on an import block silently misses; re-read before patching.
- `cargo test --test X` does not compile the lib's unit tests: `#[cfg(test)]` errors only show with `clippy --lib --tests` or `test --lib`.
