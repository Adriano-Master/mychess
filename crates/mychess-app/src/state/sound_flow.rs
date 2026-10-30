//! The sounds: the watch of the cursor, the sounds folder and the window
//! (spec M8-11, D2803, D2811, D2815).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mychess_core::NodeId;

use super::AppState;
use crate::effect::Effect;
use crate::error::RequestId;
use crate::prefs::PrefsIssue;
use crate::prefs::{PrefChange, PrefsMessage};
#[cfg(test)]
use crate::sound::SoundFolderError;
use crate::sound::{
    FolderStatus, PackError, PackId, PackInfo, PackStatus, PackWarning, PlayRequest, SoundCue,
    SoundEffect, SoundFolder, SoundMessage, SoundPack, SoundPrefs, clips_for_pack, cue_for_move,
    gain, list_packs, user_packs_dir,
};

/// The sounds window, the folder and the pack in use and the watch.
#[derive(Debug)]
pub(crate) struct SoundFlow {
    open: bool,
    folder: Option<Arc<SoundFolder>>,
    status: FolderStatus,
    /// The folder the load in flight (or the one in use) is for.
    wanted: Option<String>,
    request: Option<RequestId>,
    last_san: Option<String>,
    /// The packs to choose from: the synthesized one, the embedded ones and
    /// the ones found in the user's folder.
    packs: Vec<PackInfo>,
    /// What the last scan of the user's folder found odd.
    packs_warnings: Vec<PackWarning>,
    scan_request: Option<RequestId>,
    /// The pack in use, once decoded (the synthesized sounds play until).
    pack: Option<Arc<SoundPack>>,
    pack_status: PackStatus,
    /// The pack the load in flight (or the one in use) is for.
    pack_wanted: PackId,
    pack_request: Option<RequestId>,
    /// The packs directory of the user.
    packs_dir: Option<PathBuf>,
    /// The packs directory after the user asked to open it (it exists).
    user_folder: Option<PathBuf>,
}

impl Default for SoundFlow {
    fn default() -> SoundFlow {
        SoundFlow {
            open: false,
            folder: None,
            status: FolderStatus::None,
            wanted: None,
            request: None,
            last_san: None,
            packs: list_packs(),
            packs_warnings: Vec::new(),
            scan_request: None,
            pack: None,
            pack_status: PackStatus::None,
            pack_wanted: PackId::BuiltinSynth,
            pack_request: None,
            packs_dir: user_packs_dir(),
            user_folder: None,
        }
    }
}

/// The cursor of the game before a message: all the watch needs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CursorMark {
    cursor: NodeId,
    nodes: usize,
}

/// What the cursor did.
enum Step {
    /// To a child; `new` when the move was added to the game.
    Forward {
        new: bool,
    },
    Back,
}

impl AppState {
    /// The mark to hand to [`AppState::sound_watch`] after the message; `None`
    /// when the sounds are off (nothing to watch) or the walk of the review
    /// is on (its last tick clears the walk before the watch runs, and the
    /// whole walk must stay silent).
    pub(crate) fn sound_mark(&self) -> Option<CursorMark> {
        (self.prefs.sound.audible() && self.report.walk.is_none()).then(|| CursorMark {
            cursor: self.session.cursor(),
            nodes: self.session.game().node_count(),
        })
    }

    /// Plays the sound of what the cursor did since `mark` (D2803): a move
    /// added, a step forward or a step back; a jump is silent, and so is
    /// everything while the annotation or the walk of the review runs.
    pub(crate) fn sound_watch(&mut self, mark: CursorMark, effects: &mut Vec<Effect>) {
        if self.annotation_running() || self.report.walk.is_some() {
            return;
        }
        let game = self.session.game();
        let now = self.session.cursor();
        if now == mark.cursor {
            return;
        }
        let step = if game.parent(now) == Ok(Some(mark.cursor)) {
            Step::Forward {
                new: game.node_count() > mark.nodes,
            }
        } else if game.parent(mark.cursor) == Ok(Some(now)) {
            Step::Back
        } else {
            return;
        };
        let (on_back, on_new, on_forward) = {
            let p = &self.prefs.sound;
            (p.on_back, p.on_new_move, p.on_forward)
        };
        match step {
            Step::Back => {
                if on_back {
                    self.sound_play(SoundCue::Back, None, effects);
                }
            }
            Step::Forward { new } => {
                if (new && on_new) || (!new && on_forward) {
                    let Some((cue, san)) = self.sound_move_at(now) else {
                        return;
                    };
                    self.sound_play(cue, Some(san), effects);
                }
            }
        }
    }

    /// The number of moves of the main line of the game of the editor: what
    /// an online refresh compares to know whether a move arrived (D4400).
    pub(super) fn sound_mainline_len(&self) -> usize {
        self.session.game().mainline().count()
    }

    /// Plays the sound of the newest move of the editor game when its main
    /// line grew past `before` moves (D4400). The cursor watch cannot see
    /// the online games: each refresh replaces the whole game (another
    /// identity), which it treats as a silent jump. A refresh that adds
    /// nothing (the confirmation of a move already shown, a resynchronised
    /// `gameFull`) stays silent. Returns whether a move was played.
    pub(super) fn sound_after_refresh(&mut self, before: usize, effects: &mut Vec<Effect>) -> bool {
        if !self.prefs.sound.audible()
            || !self.prefs.sound.on_new_move
            || self.sound_mainline_len() <= before
        {
            return false;
        }
        let Some(last) = self.session.game().mainline().last() else {
            return false;
        };
        let Some((cue, san)) = self.sound_move_at(last) else {
            return false;
        };
        self.sound_play(cue, Some(san), effects);
        true
    }

    /// The sound of the start or of the end of an online game (D4401); the
    /// callers skip the end when the last move was just heard (it would cut
    /// the move sound).
    pub(super) fn sound_game_event(&mut self, cue: SoundCue, effects: &mut Vec<Effect>) {
        self.sound_play(cue, None, effects);
    }

    /// The cue and the SAN of the move that leads to `node`.
    fn sound_move_at(&self, node: NodeId) -> Option<(SoundCue, String)> {
        let game = self.session.game();
        let parent = game.parent(node).ok()??;
        let mv = game.mv(node).ok()?;
        let before = game.position_at(parent).ok()?;
        let san = before.san(mv).ok()?;
        Some((cue_for_move(before, mv), san))
    }

    /// Asks for the sound of `cue` (`san`: the move, for the announcement).
    /// Nothing happens when the sounds are off.
    pub(crate) fn sound_play(
        &mut self,
        cue: SoundCue,
        san: Option<String>,
        effects: &mut Vec<Effect>,
    ) {
        if !self.prefs.sound.audible() {
            return;
        }
        self.sound_play_now(cue, san, effects);
    }

    fn sound_play_now(&mut self, cue: SoundCue, san: Option<String>, effects: &mut Vec<Effect>) {
        let prefs = &self.prefs.sound;
        let cue = if prefs.tick_only && cue != SoundCue::Back && !is_event(cue) {
            SoundCue::Move
        } else {
            cue
        };
        let g = gain(prefs.volume);
        let clips = clips_for_pack(
            cue,
            san.as_deref(),
            prefs,
            self.flow.sound.folder.as_deref(),
            self.flow.sound.pack.as_deref(),
        );
        if prefs.announce {
            self.flow.sound.last_san = san;
        }
        effects.push(Effect::Sound(SoundEffect::Play(PlayRequest {
            clips,
            gain: g,
        })));
    }

    pub(crate) fn sound_message(&mut self, message: SoundMessage, effects: &mut Vec<Effect>) {
        match message {
            SoundMessage::Open => self.flow.sound.open = true,
            SoundMessage::Close => self.flow.sound.open = false,
            SoundMessage::Set(prefs) => {
                let prefs = prefs.sanitized();
                if prefs != *self.prefs.sound {
                    self.prefs_message(PrefsMessage::Set(PrefChange::Sound(prefs)), effects);
                }
            }
            SoundMessage::Cue(cue) => self.sound_play(cue, None, effects),
            SoundMessage::Test => {
                // A sample move, heard even when the sounds are off.
                let san = Some("Nf3".to_owned());
                if self.prefs.sound.volume > 0 {
                    self.sound_play_now(SoundCue::Move, san, effects);
                }
            }
            SoundMessage::FolderLoaded { request, result } => {
                if self.flow.sound.request != Some(request) {
                    return;
                }
                self.flow.sound.request = None;
                match result {
                    Ok(folder) => {
                        self.flow.sound.status = FolderStatus::Loaded {
                            found: folder.found(),
                            skipped: folder.skipped(),
                        };
                        self.flow.sound.folder = Some(folder);
                    }
                    Err(error) => {
                        self.flow.sound.folder = None;
                        self.flow.sound.status = FolderStatus::Failed(error);
                    }
                }
            }
            SoundMessage::Played => {}
            SoundMessage::ScanPacks => {
                self.sound_scan_packs(effects);
                self.sound_retry_failed_pack(effects);
            }
            SoundMessage::PacksScanned { request, result } => {
                if self.flow.sound.scan_request != Some(request) {
                    return;
                }
                self.flow.sound.scan_request = None;
                let mut packs = list_packs();
                match result {
                    Ok(scan) => {
                        packs.extend(scan.packs);
                        self.flow.sound.packs_warnings = scan.warnings;
                    }
                    Err(_) => self.flow.sound.packs_warnings.clear(),
                }
                self.flow.sound.packs = packs;
            }
            SoundMessage::SelectPack(id) => {
                if id.is_valid() && id != self.prefs.sound.pack {
                    let prefs = SoundPrefs {
                        pack: id,
                        ..(*self.prefs.sound).clone()
                    };
                    self.prefs_message(PrefsMessage::Set(PrefChange::Sound(prefs)), effects);
                } else if id == self.prefs.sound.pack {
                    // Choosing the same pack again reads it again.
                    self.sound_retry_failed_pack(effects);
                }
            }
            SoundMessage::PackLoaded {
                request,
                id,
                result,
            } => self.sound_pack_loaded(request, &id, result, effects),
            SoundMessage::OpenUserFolderRequested => {
                effects.push(Effect::Sound(SoundEffect::EnsureUserFolder {
                    dir: self.flow.sound.packs_dir.clone(),
                }));
            }
            SoundMessage::UserFolderReady(result) => {
                if let Ok(path) = result {
                    self.flow.sound.user_folder = Some(path);
                    // The user is about to copy packs there.
                    self.sound_scan_packs(effects);
                }
            }
        }
    }

    /// When the chosen pack failed to load (a transient I/O error or a
    /// timeout, say), forgets that it was asked for so the load is issued
    /// again; a pack that is loading or loaded is left alone.
    fn sound_retry_failed_pack(&mut self, effects: &mut Vec<Effect>) {
        if self.prefs.sound.pack != PackId::BuiltinSynth
            && matches!(self.flow.sound.pack_status, PackStatus::Failed(_))
        {
            self.flow.sound.pack_wanted = PackId::BuiltinSynth;
            self.sound_pack_changed(effects);
        }
    }

    fn sound_scan_packs(&mut self, effects: &mut Vec<Effect>) {
        let request = self.new_request();
        self.flow.sound.scan_request = Some(request);
        effects.push(Effect::Sound(SoundEffect::ScanPacks {
            request,
            dir: self.flow.sound.packs_dir.clone(),
        }));
    }

    fn sound_pack_loaded(
        &mut self,
        request: RequestId,
        id: &PackId,
        result: Result<Arc<SoundPack>, PackError>,
        effects: &mut Vec<Effect>,
    ) {
        let flow = &mut self.flow.sound;
        if flow.pack_request != Some(request) || flow.pack_wanted != *id {
            return;
        }
        flow.pack_request = None;
        match result {
            Ok(pack) => {
                flow.pack_status = PackStatus::Loaded {
                    covered: pack.info().covered,
                    warnings: pack.info().warnings.len(),
                };
                flow.pack = Some(pack);
            }
            Err(error) => {
                flow.pack = None;
                flow.pack_status = PackStatus::Failed(error.clone());
                // A pack that is gone (or never was) goes back to the
                // default and is noted (D2902); a read error may pass.
                if matches!(error, PackError::NotFound(_) | PackError::InvalidId) {
                    let issue = PrefsIssue::InvalidValue {
                        key: "sound.pack".to_owned(),
                    };
                    if !self.prefs_issues.contains(&issue) {
                        self.prefs_issues.push(issue);
                    }
                    let prefs = SoundPrefs {
                        pack: PackId::BuiltinSynth,
                        ..(*self.prefs.sound).clone()
                    };
                    self.prefs_message(PrefsMessage::Set(PrefChange::Sound(prefs)), effects);
                    self.flow.sound.pack_status = PackStatus::Failed(error);
                }
            }
        }
    }

    /// Reads the sounds folder again when the preference changed (at load
    /// time and after every change of the `[sound]` table).
    pub(crate) fn sound_prefs_changed(&mut self, effects: &mut Vec<Effect>) {
        self.sound_folder_changed(effects);
        self.sound_pack_changed(effects);
    }

    /// Loads the pack when the choice changed (at start-up too).
    fn sound_pack_changed(&mut self, effects: &mut Vec<Effect>) {
        let wanted = self.prefs.sound.pack.clone();
        if wanted == self.flow.sound.pack_wanted {
            return;
        }
        self.flow.sound.pack = None;
        if wanted == PackId::BuiltinSynth {
            self.flow.sound.pack_request = None;
            self.flow.sound.pack_status = PackStatus::None;
        } else {
            let request = self.new_request();
            self.flow.sound.pack_request = Some(request);
            self.flow.sound.pack_status = PackStatus::Loading;
            effects.push(Effect::Sound(SoundEffect::LoadPack {
                request,
                id: wanted.clone(),
                dir: self.flow.sound.packs_dir.clone(),
            }));
        }
        self.flow.sound.pack_wanted = wanted;
    }

    fn sound_folder_changed(&mut self, effects: &mut Vec<Effect>) {
        let wanted = self
            .prefs
            .sound
            .folder_path()
            .map(|_| self.prefs.sound.folder.clone());
        if wanted == self.flow.sound.wanted {
            return;
        }
        self.flow.sound.wanted = wanted;
        self.flow.sound.folder = None;
        match self.prefs.sound.folder_path() {
            None => {
                self.flow.sound.request = None;
                self.flow.sound.status = FolderStatus::None;
            }
            Some(path) => {
                let request = self.new_request();
                self.flow.sound.request = Some(request);
                self.flow.sound.status = FolderStatus::Loading;
                effects.push(Effect::Sound(SoundEffect::LoadFolder { request, path }));
            }
        }
    }

    /// Whether the sounds window is open.
    #[must_use]
    pub fn sound_dialog_open(&self) -> bool {
        self.flow.sound.open
    }

    /// The packs to choose from (D2907): the synthesized one, the embedded
    /// ones and, after [`SoundMessage::ScanPacks`], the user's.
    #[must_use]
    pub fn sound_packs(&self) -> &[PackInfo] {
        &self.flow.sound.packs
    }

    /// What the last scan of the user's packs folder ignored.
    #[must_use]
    pub fn sound_packs_warnings(&self) -> &[PackWarning] {
        &self.flow.sound.packs_warnings
    }

    /// What is known about the pack in use.
    #[must_use]
    pub fn sound_pack_status(&self) -> &PackStatus {
        &self.flow.sound.pack_status
    }

    /// The pack in use, once loaded (its `info()` has the remarks found
    /// while decoding).
    #[must_use]
    pub fn sound_loaded_pack(&self) -> Option<&SoundPack> {
        self.flow.sound.pack.as_deref()
    }

    /// The user's packs directory (it may not exist yet), if the system has
    /// a data directory.
    #[must_use]
    pub fn sound_packs_dir(&self) -> Option<&Path> {
        self.flow.sound.packs_dir.as_deref()
    }

    /// Replaces the user's packs directory (tests and embedders).
    pub fn set_sound_packs_dir(&mut self, dir: Option<PathBuf>) {
        self.flow.sound.packs_dir = dir;
    }

    /// The packs directory, once the user asked to open it and it was
    /// created.
    #[must_use]
    pub fn sound_user_folder(&self) -> Option<&Path> {
        self.flow.sound.user_folder.as_deref()
    }

    /// The sound preferences.
    #[must_use]
    pub fn sound_prefs(&self) -> &SoundPrefs {
        &self.prefs.sound
    }

    /// What is known about the sounds folder.
    #[must_use]
    pub fn sound_folder_status(&self) -> &FolderStatus {
        &self.flow.sound.status
    }

    /// The SAN of the last move announced, for the "last announcement" of
    /// the window (the front-end writes it in its language with
    /// [`announce_text`](crate::sound::announce_text)).
    #[must_use]
    pub fn sound_last_announcement(&self) -> Option<&str> {
        self.flow.sound.last_san.as_deref()
    }
}

/// Whether `cue` is not about a move (game start and end, clock).
fn is_event(cue: SoundCue) -> bool {
    matches!(
        cue,
        SoundCue::GameStart | SoundCue::GameEnd | SoundCue::LowTime
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mychess_core::Position;

    use super::*;
    use crate::message::Message;
    use crate::session::{GameMessage, Nav};
    use crate::sound::{SoundPrefs, builtin_clip};
    use crate::state::update;

    fn on() -> AppState {
        let mut state = AppState::default();
        *state.prefs.sound = SoundPrefs {
            enabled: true,
            ..SoundPrefs::default()
        };
        state
    }

    fn play(state: &mut AppState, uci: &str) -> Vec<Effect> {
        let pos: Position = state
            .session
            .game()
            .position_at(state.session.cursor())
            .expect("position")
            .clone();
        let mv = pos.parse_uci(uci).expect("uci");
        update(
            state,
            Message::Game(GameMessage::PlayMove {
                mv,
                options: crate::PlayOptions::default(),
            }),
        )
    }

    fn plays(effects: &[Effect]) -> Vec<&PlayRequest> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Sound(SoundEffect::Play(r)) => Some(r),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn nothing_is_asked_while_the_sounds_are_off() {
        let mut state = AppState::default();
        let effects = play(&mut state, "e2e4");
        assert!(plays(&effects).is_empty());
        let effects = update(
            &mut state,
            Message::Sound(SoundMessage::Cue(SoundCue::Move)),
        );
        assert!(effects.is_empty());
    }

    #[test]
    fn a_new_move_plays_its_sound_with_the_volume_applied() {
        let mut state = on();
        state.prefs.sound.volume = 50;
        let effects = play(&mut state, "e2e4");
        let requests = plays(&effects);
        assert_eq!(requests.len(), 1);
        // The samples are shared, not copied; the gain travels beside them.
        assert!(Arc::ptr_eq(
            &requests[0].clips[0],
            &builtin_clip(SoundCue::Move)
        ));
        assert!((requests[0].gain - 0.25).abs() < 1e-6);
    }

    #[test]
    fn the_cue_follows_the_kind_of_move() {
        let mut state = on();
        for uci in ["e2e4", "d7d5"] {
            drop(play(&mut state, uci));
        }
        let effects = play(&mut state, "e4d5");
        let request = plays(&effects)[0];
        assert!(Arc::ptr_eq(
            &request.clips[0],
            &builtin_clip(SoundCue::Capture)
        ));
        assert!((request.gain - gain(70)).abs() < 1e-6);
    }

    #[test]
    fn navigation_is_silent_unless_chosen() {
        let mut state = on();
        drop(play(&mut state, "e2e4"));
        let back = update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::Back(1))),
        );
        assert!(plays(&back).is_empty());
        let fwd = update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::Forward(1))),
        );
        assert!(plays(&fwd).is_empty());
        state.prefs.sound.on_forward = true;
        state.prefs.sound.on_back = true;
        let back = update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::Back(1))),
        );
        assert!(Arc::ptr_eq(
            &plays(&back)[0].clips[0],
            &builtin_clip(SoundCue::Back)
        ));
        let fwd = update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::Forward(1))),
        );
        assert_eq!(plays(&fwd).len(), 1);
    }

    #[test]
    fn a_jump_over_several_moves_is_silent() {
        let mut state = on();
        state.prefs.sound.on_forward = true;
        state.prefs.sound.on_back = true;
        for uci in ["e2e4", "e7e5", "g1f3"] {
            drop(play(&mut state, uci));
        }
        let jump = update(&mut state, Message::Game(GameMessage::Navigate(Nav::Start)));
        assert!(plays(&jump).is_empty());
    }

    #[test]
    fn tick_only_turns_every_move_into_the_plain_sound() {
        let mut state = on();
        state.prefs.sound.tick_only = true;
        for uci in ["e2e4", "d7d5"] {
            drop(play(&mut state, uci));
        }
        let effects = play(&mut state, "e4d5");
        assert!(Arc::ptr_eq(
            &plays(&effects)[0].clips[0],
            &builtin_clip(SoundCue::Move)
        ));
    }

    #[test]
    fn nothing_plays_while_an_annotation_runs() {
        let mut state = on();
        state.prefs.sound.on_forward = true;
        state.prefs.sound.on_back = true;
        state.fake_annotation_batch();
        assert!(state.annotation_running());
        let effects = play(&mut state, "e2e4");
        assert!(plays(&effects).is_empty());
        state.flow.annotate_batch = None;
        // The first move was refused (the annotation commands the game).
        let effects = play(&mut state, "e2e4");
        assert_eq!(plays(&effects).len(), 1, "the same step plays when idle");
    }

    #[test]
    fn nothing_plays_while_the_review_walks() {
        let mut state = AppState::default();
        play(&mut state, "e2e4");
        play(&mut state, "e7e5");
        update(&mut state, Message::Game(GameMessage::Navigate(Nav::Start)));
        *state.prefs.sound = SoundPrefs {
            enabled: true,
            on_forward: true,
            on_back: true,
            ..SoundPrefs::default()
        };
        state.report.walk = Some(crate::report::ReviewWalk {
            target_ply: 2,
            next_ply: 1,
            direction: crate::report::WalkDirection::Forward,
            interval_ms: 100,
            stamp: 1,
        });
        // A tick in the middle and the last one (which clears the walk).
        for _ in 0..2 {
            let effects = update(
                &mut state,
                Message::Report(crate::report::ReportMessage::WalkTick { stamp: 1 }),
            );
            assert!(plays(&effects).is_empty(), "the walk is silent");
        }
        assert!(state.report.walk.is_none(), "the walk ended");
        assert_eq!(state.session.game().mainline().count(), 2);
        let effects = update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::Back(1))),
        );
        assert_eq!(plays(&effects).len(), 1, "idle again: the step back plays");
    }

    #[test]
    fn the_test_button_plays_even_when_off_but_not_at_volume_zero() {
        let mut state = AppState::default();
        let effects = update(&mut state, Message::Sound(SoundMessage::Test));
        assert_eq!(plays(&effects).len(), 1);
        state.prefs.sound.volume = 0;
        let effects = update(&mut state, Message::Sound(SoundMessage::Test));
        assert!(plays(&effects).is_empty());
    }

    #[test]
    fn setting_the_preferences_limits_them_and_reads_a_new_folder() {
        let mut state = AppState::default();
        let prefs = SoundPrefs {
            volume: 200,
            folder: "/some/sounds".to_owned(),
            ..SoundPrefs::default()
        };
        let effects = update(
            &mut state,
            Message::Sound(SoundMessage::Set(Box::new(prefs))),
        );
        assert_eq!(state.prefs.sound.volume, 100);
        assert_eq!(state.sound_folder_status(), &FolderStatus::Loading);
        let request = effects
            .iter()
            .find_map(|e| match e {
                Effect::Sound(SoundEffect::LoadFolder { request, path }) => {
                    assert_eq!(path, &PathBuf::from("/some/sounds"));
                    Some(*request)
                }
                _ => None,
            })
            .expect("a folder load");
        // An old answer is ignored; the right one is kept.
        let stale = RequestId(request.0.wrapping_sub(1));
        drop(update(
            &mut state,
            Message::Sound(SoundMessage::FolderLoaded {
                request: stale,
                result: Err(SoundFolderError::Io(std::io::ErrorKind::Other)),
            }),
        ));
        assert_eq!(state.sound_folder_status(), &FolderStatus::Loading);
        let folder = SoundFolder::from_clips(
            PathBuf::from("/some/sounds"),
            vec![(
                "Check",
                crate::sound::Clip {
                    sample_rate: 8_000,
                    channels: 1,
                    samples: vec![0.1],
                },
            )],
        );
        drop(update(
            &mut state,
            Message::Sound(SoundMessage::FolderLoaded {
                request,
                result: Ok(Arc::new(folder)),
            }),
        ));
        assert_eq!(
            state.sound_folder_status(),
            &FolderStatus::Loaded {
                found: 1,
                skipped: 0
            }
        );
        // Removing the folder forgets it without a read.
        let cleared = SoundPrefs {
            folder: String::new(),
            ..(*state.prefs.sound).clone()
        };
        let effects = update(
            &mut state,
            Message::Sound(SoundMessage::Set(Box::new(cleared))),
        );
        assert_eq!(state.sound_folder_status(), &FolderStatus::None);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::Sound(SoundEffect::LoadFolder { .. })))
        );
    }

    #[test]
    fn announcing_with_a_folder_queues_the_words_and_remembers_the_move() {
        let mut state = on();
        // Changed only through `Set`, so the folder change triggers the load.
        let mut prefs = (*state.prefs.sound).clone();
        prefs.announce = true;
        prefs.folder = "/s".to_owned();
        let effects = update(
            &mut state,
            Message::Sound(SoundMessage::Set(Box::new(prefs))),
        );
        drop(effects);
        // Pretend the folder was read: Knight, g, 1 are missing, "f" and "3"
        // exist; Nf3 says f, 3.
        let tone = |v| crate::sound::Clip {
            sample_rate: 8_000,
            channels: 1,
            samples: vec![v],
        };
        let request = state.flow.sound.request;
        let folder = SoundFolder::from_clips(
            PathBuf::from("/s"),
            vec![("f", tone(0.1)), ("3", tone(0.2))],
        );
        drop(update(
            &mut state,
            Message::Sound(SoundMessage::FolderLoaded {
                request: request.expect("loading"),
                result: Ok(Arc::new(folder)),
            }),
        ));
        for uci in ["e2e4", "e7e5"] {
            drop(play(&mut state, uci));
        }
        let effects = play(&mut state, "g1f3");
        let requests = plays(&effects);
        assert_eq!(requests[0].clips.len(), 2);
        assert_eq!(state.sound_last_announcement(), Some("Nf3"));
    }

    #[test]
    fn the_window_opens_and_closes() {
        let mut state = AppState::default();
        assert!(!state.sound_dialog_open());
        drop(update(&mut state, Message::Sound(SoundMessage::Open)));
        assert!(state.sound_dialog_open());
        drop(update(&mut state, Message::Sound(SoundMessage::Close)));
        assert!(!state.sound_dialog_open());
    }

    #[test]
    fn sounds_are_not_assistance_online() {
        assert_eq!(
            crate::online::family_of(&Message::Sound(SoundMessage::Cue(SoundCue::Move))),
            None
        );
    }
}
