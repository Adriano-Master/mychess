---
name: report-close-lessons
description: M4-10 close-out lessons - D-number ranges are shared with crates, Rig effects always include board analysis, disk budget per test batch, ETA clock via existing ticks
metadata:
  type: project
---

- The D-number band check must grep `crates/` too, not only `docs/specs`: D2130-D2175 were already used in code comments by the tactics and book agents; D2100-D2129 was free (M4-10 close-out took it).
- In app tests built on `m4_6_common::Rig`, `rig.send(..)` can return analysis `Search` effects of the board engine; never assert `fx.is_empty()` for a message that "does nothing" - assert the state instead.
- `Rig::drive(fx, n, engine)` counts searches, and `annotation_progress().done_plies` is `n - 1` (the base position does not count).
- Disk is tight (a full `target` of test binaries was 43G and the link failed with "Bus error"): run test binaries in batches of about 10 and delete executables in `target/m410c/debug/deps` after each batch.
- A python one-liner `open(p,'w').write(open(p).read())` truncates the file before reading; it emptied `report_panel.rs` once (untracked, no git copy). Always rewrite from a read.
- The ETA of a run needs a clock but `update` has none: reuse `Effect::ScheduleReviewTick` + `ReportMessage::WalkTick { stamp }` with a separate stamp (`ReportUi.clock`), 1 s per tick; the watchdog is `report_guard` plus the late tick finding no run.
- A non-tiny field of `Preferences` must be `Box`ed (`PreferencesLoaded` size test).
- Plain Left/Right are `Action::Back/Forward` (user-rebindable); remapping them inside `AppState::action` for the review avoided a new Action (and the exhaustive lists of keys, menus and online families).
