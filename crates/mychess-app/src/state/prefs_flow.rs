//! Loading, changing and saving the preferences, and quitting (spec M2-2,
//! D137, D142-D147, rule 57).

use std::path::Path;
use std::time::Duration;

use super::AppState;
use crate::effect::Effect;
use crate::error::FileError;
use crate::flow::AppNotice;
use crate::prefs::{
    LoadedPreferences, PrefChange, PrefsIssue, PrefsMessage, WindowGeometry, WindowPrefs,
};
use crate::session::GameMessage;
use crate::shared::SharedEco;
use crate::shortcuts::Shortcuts;

impl AppState {
    pub(crate) fn startup(&mut self, effects: &mut Vec<Effect>) {
        match self.flow.prefs_path.clone() {
            Some(path) => {
                self.flow.bookmarks_loading = true;
                effects.push(Effect::LoadBookmarks {
                    path: crate::bookmarks::bookmarks_path(&path),
                });
                self.request_engines_load(effects);
                self.flow.prefs_loading = true;
                effects.push(Effect::LoadPreferences { path });
            }
            None => self.warn_no_config_dir(),
        }
    }

    fn warn_no_config_dir(&mut self) {
        if !self.flow.warned_no_config_dir {
            self.flow.warned_no_config_dir = true;
            self.prefs_issues.push(PrefsIssue::NoConfigDir);
        }
    }

    pub(crate) fn preferences_loaded(
        &mut self,
        loaded: LoadedPreferences,
        effects: &mut Vec<Effect>,
    ) {
        // The answer of the disk, not the early peek: the launch waits for it.
        let from_disk = self.flow.prefs_loading;
        self.flow.prefs_loading = false;
        self.prefs = loaded.prefs;
        self.prefs_issues = loaded.issues;
        // What the user changed while the file was being read wins over the
        // file, and must reach the disk (the file's other values stay).
        let early_changes = std::mem::take(&mut self.flow.early_changes);
        let early_geometry = self.flow.early_geometry.take();
        let early_tools = std::mem::take(&mut self.spelling.early_tools);
        let early_export = self.export_docs_take_early();
        let changed = !early_changes.is_empty()
            || early_geometry.is_some()
            || !early_tools.is_empty()
            || !early_export.is_empty();
        for change in early_changes {
            // The `[play]` that the dialog sent knew only the defaults of the
            // score and of the Elo: the ones of the file stay (the games
            // that ended meanwhile come as deltas, `RecordTactical`).
            let tactical = matches!(change, PrefChange::Play(_))
                .then(|| (self.prefs.play.tactical.stats, self.prefs.play.tactical.elo));
            change.apply(&mut self.prefs);
            if let Some((stats, elo)) = tactical {
                self.prefs.play.tactical.stats = stats;
                self.prefs.play.tactical.elo = elo;
            }
        }
        // `[player_tools]` changes come one field at a time: the history, the
        // spelling file and the rest of the file's table stay.
        for delta in early_tools {
            let mut tools = (*self.prefs.player_tools).clone();
            (delta.0)(&mut tools);
            *self.prefs.player_tools = tools.sanitized();
        }
        // `[export]` changes come one field at a time too (spec M7-4).
        self.export_docs_replay(early_export);
        if let Some(geometry) = early_geometry {
            self.set_window_prefs(geometry);
        }
        self.rebuild_shortcuts();
        self.apply_prefs_to_session();
        if let Some(path) = self.prefs.files.eco_file.clone() {
            effects.push(Effect::LoadEcoBook { path });
        }
        self.book_request_opening(effects);
        self.sound_prefs_changed(effects);
        self.fics_prefs_loaded();
        if changed {
            self.prefs_dirty = true;
        }
        if std::mem::take(&mut self.flow.save_after_load) {
            self.request_prefs_save(effects);
        }
        if from_disk {
            self.start_after_prefs(effects);
        }
    }

    /// Defaults plus the customisations of the preferences.
    fn rebuild_shortcuts(&mut self) {
        let (shortcuts, issues) = Shortcuts::defaults().with_overrides(&self.prefs.shortcuts);
        self.shortcuts = shortcuts;
        self.prefs_issues
            .retain(|i| !matches!(i, PrefsIssue::Shortcut(_)));
        self.prefs_issues
            .extend(issues.into_iter().map(PrefsIssue::Shortcut));
    }

    /// Passes the autoplay delay to the game session.
    fn apply_prefs_to_session(&mut self) {
        let delay = Duration::from_millis(u64::from(self.prefs.playback.autoplay_delay_ms));
        // Only a delay change; the session ignores the message while a
        // prompt is open, which cannot happen at load time.
        drop(self.session.update(GameMessage::SetAutoplayDelay(delay)));
        self.apply_editing_to_session();
    }

    /// Passes the "ask to replace" preference to the game session. The
    /// session accepts new settings while its prompt is open (the answer to
    /// "don't ask again" arrives with the prompt still showing).
    fn apply_editing_to_session(&mut self) {
        let mut settings = self.session.settings().clone();
        settings.ask_to_replace_moves = self.prefs.editing.ask_to_replace_moves;
        // The settings also hold the autoplay delay: keep the preference.
        settings.autoplay_delay =
            Duration::from_millis(u64::from(self.prefs.playback.autoplay_delay_ms));
        drop(self.session.update(GameMessage::ApplySettings(settings)));
    }

    pub(crate) fn prefs_message(&mut self, message: PrefsMessage, effects: &mut Vec<Effect>) {
        let PrefsMessage::Set(change) = message;
        let debounced = change.is_debounced();
        let eco_changed = matches!(change, PrefChange::EcoFile(_));
        let book_changed = matches!(change, PrefChange::Book(_));
        let shortcut_changed = matches!(change, PrefChange::Shortcut { .. });
        let delay_changed = matches!(change, PrefChange::AutoplayDelayMs(_));
        let editing_changed = matches!(change, PrefChange::AskToReplaceMoves(_));
        let sound_changed = matches!(change, PrefChange::Sound(_));
        if self.flow.prefs_loading {
            self.flow.early_changes.push(change.clone());
        }
        change.apply(&mut self.prefs);
        if shortcut_changed {
            self.rebuild_shortcuts();
        }
        if delay_changed {
            self.apply_prefs_to_session();
        }
        if editing_changed {
            self.apply_editing_to_session();
        }
        if book_changed {
            self.book_request_opening(effects);
        }
        if sound_changed {
            self.sound_prefs_changed(effects);
        }
        if eco_changed {
            match self.prefs.files.eco_file.clone() {
                Some(path) => effects.push(Effect::LoadEcoBook { path }),
                None => self.eco_book = None,
            }
        }
        if debounced {
            self.prefs_dirty = true;
        } else {
            self.request_prefs_save(effects);
        }
    }

    pub(crate) fn window_geometry(&mut self, geometry: WindowGeometry) {
        if self.flow.prefs_loading {
            self.flow.early_geometry = Some(geometry);
        }
        self.set_window_prefs(geometry);
        self.prefs_dirty = true;
    }

    fn set_window_prefs(&mut self, geometry: WindowGeometry) {
        self.prefs.window = WindowPrefs {
            width: Some(geometry.width),
            height: Some(geometry.height),
            x: geometry.x,
            y: geometry.y,
            maximized: geometry.maximized,
        };
    }

    pub(crate) fn flush_preferences(&mut self, effects: &mut Vec<Effect>) {
        if self.prefs_dirty {
            self.request_prefs_save(effects);
        }
    }

    /// Asks for a save; at most one is in flight (D145): a request made
    /// meanwhile becomes one more save, with the newest preferences.
    pub(crate) fn request_prefs_save(&mut self, effects: &mut Vec<Effect>) {
        let Some(path) = self.flow.prefs_path.clone() else {
            self.warn_no_config_dir();
            return;
        };
        if self.flow.prefs_loading {
            // Writing now would replace the file with the defaults before
            // it was read; wait for `PreferencesLoaded`.
            self.flow.save_after_load = true;
            return;
        }
        if self.flow.prefs_save_in_flight {
            self.flow.needs_resave = true;
            return;
        }
        self.flow.prefs_save_in_flight = true;
        self.prefs_dirty = false;
        effects.push(Effect::SavePreferences {
            path,
            prefs: Box::new(self.prefs.clone()),
        });
    }

    pub(crate) fn preferences_saved(
        &mut self,
        result: Result<(), FileError>,
        effects: &mut Vec<Effect>,
    ) {
        self.flow.prefs_save_in_flight = false;
        if let Err(error) = result {
            let issue = PrefsIssue::SaveFailed(error.kind);
            if !self.prefs_issues.contains(&issue) {
                self.prefs_issues.push(issue);
            }
        }
        if self.flow.needs_resave {
            self.flow.needs_resave = false;
            self.request_prefs_save(effects);
        } else if self.flow.exit_when_saved {
            effects.push(Effect::Exit);
        }
    }

    /// Whether `finish_quit` has taken the point of no return and is waiting
    /// for something before the exit: a job, a batch step, the bookmarks, the
    /// score of a trainer, the tournament writes, the caches of the trees,
    /// the engines or the preferences (D3509). Keep it in step with the
    /// waits of [`AppState::finish_quit`]: the safety net of the spelling
    /// flow must not undo a quit that is still going on.
    pub(crate) fn quit_finishing(&self) -> bool {
        self.flow.exit_when_saved
            || self.flow.quit_waiting_job
            || self.flow.quit_waiting_batch
            || self.flow.quit_waiting_bookmarks
            || self.tactics_quit_waiting()
            || self.opening_quit_waiting()
            || self.calvar_quit_waiting()
            || self.tournament_quit_pending()
            // `quit_started` is the point of no return of the caches (D1777).
            || self.tree.cache.quit_started
            || self.flow.engines.quit_waiting_save
            || self.flow.engines.shutdown == super::engines_flow::Shutdown::Waiting
    }

    /// Saves the preferences, then asks the front-end to close.
    pub(crate) fn finish_quit(&mut self, effects: &mut Vec<Effect>) {
        // The program ends: nothing cut by the quit starts again.
        self.spelling.cut_load = None;
        self.spelling.photos_cut = false;
        // A copy or a flag change in flight is cancelled and given a few
        // seconds to leave its files consistent (the batch being written
        // ends, the writer is dropped) before the program exits (D589).
        if let Some(job) = &self.flow.list_job {
            if !self.flow.quit_waiting_job {
                job.control.cancel();
                self.flow.quit_waiting_job = true;
                effects.push(Effect::QuitDeadline {
                    delay: Duration::from_secs(5),
                });
            }
            return;
        }
        // A batch step in flight (the game being written to the .si4 is not
        // journaled) finishes before the program exits (D1550).
        if self.flow.annotate_batch.is_some() {
            if !self.flow.quit_waiting_batch {
                if let Some(batch) = self.flow.annotate_batch.as_mut() {
                    batch.cancelled = true;
                }
                self.flow.quit_waiting_batch = true;
                effects.push(Effect::QuitDeadline {
                    delay: Duration::from_secs(5),
                });
            }
            return;
        }
        if self.flow.bookmarks_save_in_flight {
            // The bookmarks being written finish first; their answer calls
            // this again.
            self.flow.quit_waiting_bookmarks = true;
            return;
        }
        // The score of the tactics training too (D1958).
        if self.tactics_quit_wait(effects) {
            return;
        }
        // The caches of the trees are written before the program exits
        // (spec M5-3b, D1769).
        if self.tree_cache_quit_wait(effects) {
            return;
        }
        if self.quit_waits_for_engines(effects) {
            return;
        }
        if self.flow.prefs_path.is_none() {
            effects.push(Effect::Exit);
            return;
        }
        self.flow.exit_when_saved = true;
        self.request_prefs_save(effects);
    }

    pub(crate) fn eco_loaded(&mut self, path: &Path, result: Result<SharedEco, FileError>) {
        if self.prefs.files.eco_file.as_deref() != Some(path) {
            return;
        }
        match result {
            Ok(book) => self.eco_book = Some(book),
            Err(error) => {
                self.eco_book = None;
                self.notice = Some(AppNotice::File(error));
            }
        }
    }
}
