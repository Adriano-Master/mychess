//! The flow of the start-up (spec M9-4): the files named on the command
//! line, the first-run welcome and the tip of the day. The welcome and the
//! tip are drawn by the front-end; the disk work is a [`StartEffect`].

use super::AppState;
use crate::book::{BookMessage, BookSide};
use crate::effect::Effect;
use crate::engines::EngineMessage;
use crate::flow::AppNotice;
use crate::prefs::{PrefChange, PrefsMessage};
use crate::start::{
    LaunchFile, MAX_LAUNCH_FILES, StartEffect, StartInfo, StartMessage, StartNotice, StartPrefs,
    StartState, TIP_COUNT, WelcomeState,
};

/// How many engines found in `Engine/` the welcome registers.
const MAX_REGISTERED_ENGINES: usize = 4;

impl AppState {
    /// The start-up flow: the welcome and the tip of the day.
    #[must_use]
    pub fn start(&self) -> &StartState {
        &self.start
    }

    pub(crate) fn start_message(&mut self, message: StartMessage, effects: &mut Vec<Effect>) {
        match message {
            StartMessage::Configure(info) => {
                // Without a preferences file nothing can be remembered: the
                // files open now and the welcome and the tip stay away.
                if self.flow.prefs_path.is_none() {
                    self.start_begin(*info, effects);
                } else {
                    self.start.pending = Some(*info);
                }
            }
            StartMessage::Prepared { request, report } => {
                let Some(welcome) = self.start.welcome.as_mut() else {
                    return;
                };
                if welcome.request != Some(request) {
                    return;
                }
                welcome.request = None;
                let problem = report.problems.first().map(|p| p.path().to_path_buf());
                welcome.report = Some(*report);
                if let Some(path) = problem {
                    self.notice = Some(AppNotice::Start(StartNotice::FolderProblem(path)));
                }
            }
            StartMessage::CloseWelcome => self.close_welcome(effects),
            StartMessage::ShowTip => self.open_tip(effects),
            StartMessage::NextTip => self.step_tip(1),
            StartMessage::PreviousTip => self.step_tip(TIP_COUNT - 1),
            StartMessage::CloseTip => self.start.tip = None,
            StartMessage::SetShowTip(show) => {
                let prefs = StartPrefs {
                    show_tip: show,
                    ..self.prefs.start
                };
                self.prefs_message(PrefsMessage::Set(PrefChange::Start(prefs)), effects);
            }
        }
    }

    /// What the launch asked for, once the preferences are known.
    pub(crate) fn start_after_prefs(&mut self, effects: &mut Vec<Effect>) {
        if let Some(info) = self.start.pending.take() {
            self.start_begin(info, effects);
        }
    }

    fn start_begin(&mut self, info: StartInfo, effects: &mut Vec<Effect>) {
        if let Some(warning) = info.warnings.first() {
            self.notice = Some(AppNotice::Start(StartNotice::from(warning)));
        }
        let persistent = self.flow.prefs_path.is_some();
        if persistent && !self.prefs.start.welcome_done {
            let request = info.root.as_ref().map(|root| {
                let request = self.new_request();
                effects.push(Effect::Start(StartEffect::PrepareFolder {
                    request,
                    root: root.clone(),
                }));
                request
            });
            self.start.welcome = Some(WelcomeState {
                root: info.root,
                mode: info.mode,
                report: None,
                request,
            });
        } else if persistent && self.prefs.start.show_tip && !info.skip_tip {
            self.open_tip(effects);
        }
        self.start
            .queue
            .extend(info.files.into_iter().take(MAX_LAUNCH_FILES));
        self.start_open_next(effects);
    }

    /// Opens the next file of the command line. A database is read by an
    /// effect, so the walk goes on when its answer comes
    /// ([`BasesMessage::Opened`](crate::BasesMessage)).
    pub(crate) fn start_open_next(&mut self, effects: &mut Vec<Effect>) {
        while self.flow.open_base.is_none() {
            let Some(file) = self.start.queue.pop_front() else {
                return;
            };
            match file {
                LaunchFile::Base(path) => self.start_open_base(path, effects),
                LaunchFile::Epd(path) => self.epd_start_open(path, false, effects),
                LaunchFile::Book(path) => self.book_message(
                    BookMessage::Load {
                        side: BookSide::First,
                        path,
                    },
                    effects,
                ),
            }
        }
    }

    fn close_welcome(&mut self, effects: &mut Vec<Effect>) {
        let Some(welcome) = self.start.welcome.take() else {
            return;
        };
        let prefs = StartPrefs {
            welcome_done: true,
            ..self.prefs.start
        };
        self.prefs_message(PrefsMessage::Set(PrefChange::Start(prefs)), effects);
        // The engines of `Engine/` join the list (D4010). The probe answers
        // later; one already in the list is skipped by the message itself.
        if !self.flow.engines.loading
            && let Some(report) = welcome.report
        {
            for engine in report.engines.into_iter().take(MAX_REGISTERED_ENGINES) {
                self.engine_message(EngineMessage::AddFound(engine.path), effects);
            }
        }
    }

    /// Shows the tip the next start would show, and moves on to the next.
    fn open_tip(&mut self, effects: &mut Vec<Effect>) {
        let index = self.prefs.start.next_tip % TIP_COUNT;
        self.start.tip = Some(index);
        let prefs = StartPrefs {
            next_tip: (index + 1) % TIP_COUNT,
            ..self.prefs.start
        };
        self.prefs_message(PrefsMessage::Set(PrefChange::Start(prefs)), effects);
    }

    fn step_tip(&mut self, by: u32) {
        if let Some(index) = self.start.tip {
            self.start.tip = Some((index + by) % TIP_COUNT);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::error::RequestId;
    use crate::message::Message;
    use crate::prefs::load_preferences;
    use crate::start::LayoutMode;
    use crate::update;

    fn info(root: Option<PathBuf>, files: Vec<LaunchFile>) -> StartInfo {
        StartInfo {
            root,
            mode: LayoutMode::Normal,
            warnings: Vec::new(),
            files,
            skip_tip: false,
        }
    }

    /// A state that has a preferences file, asked to load it, with the
    /// launch handed over.
    fn launched(prefs_text: &str, info: StartInfo) -> (AppState, Vec<Effect>) {
        let mut state = AppState::with_preferences_path(Some(PathBuf::from("prefs.toml")));
        update(&mut state, Message::Startup);
        update(
            &mut state,
            Message::Start(StartMessage::Configure(Box::new(info))),
        );
        assert!(!state.start().is_open(), "nothing before the preferences");
        let effects = update(
            &mut state,
            Message::PreferencesLoaded(load_preferences(prefs_text)),
        );
        (state, effects)
    }

    fn prepare_request(effects: &[Effect]) -> Option<(RequestId, PathBuf)> {
        effects.iter().find_map(|e| match e {
            Effect::Start(StartEffect::PrepareFolder { request, root }) => {
                Some((*request, root.clone()))
            }
            _ => None,
        })
    }

    #[tokio::test]
    async fn first_run_prepares_the_folder_and_closing_remembers_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("Home x").join("MyChess");
        let (mut state, effects) = launched("", info(Some(root.clone()), vec![]));
        let welcome = state.start().welcome().expect("welcome open");
        assert!(welcome.report.is_none(), "folders are made by an effect");
        assert!(!root.exists(), "update itself touches no disk");
        let (request, asked) = prepare_request(&effects).expect("effect");
        assert_eq!(asked, root);
        // The disk work, as the front-end would run it.
        let answer = crate::perform(Effect::Start(StartEffect::PrepareFolder {
            request,
            root: root.clone(),
        }))
        .await;
        update(&mut state, answer);
        let report = state
            .start()
            .welcome()
            .and_then(|w| w.report.clone())
            .expect("report");
        assert!(report.is_complete());
        for name in crate::start::SUBFOLDERS {
            assert!(root.join(name).is_dir(), "{name}");
        }
        assert!(!state.start().is_open() || state.start().tip().is_none());
        update(&mut state, Message::Start(StartMessage::CloseWelcome));
        assert!(state.start().welcome().is_none());
        assert!(state.prefs.start.welcome_done);
    }

    #[test]
    fn a_stale_answer_is_ignored() {
        let root = PathBuf::from("MyChess");
        let (mut state, effects) = launched("", info(Some(root.clone()), vec![]));
        let (request, _) = prepare_request(&effects).expect("effect");
        let stale = RequestId(request.0.wrapping_add(7));
        let report = crate::start::FolderReport {
            root,
            created: vec![],
            existing: vec![],
            problems: vec![],
            engines: vec![],
        };
        update(
            &mut state,
            Message::Start(StartMessage::Prepared {
                request: stale,
                report: Box::new(report),
            }),
        );
        assert!(state.start().welcome().is_some_and(|w| w.report.is_none()));
    }

    #[test]
    fn after_the_welcome_the_tip_of_the_day_opens_and_moves_on() {
        let (mut state, effects) = launched(
            "[start]\nwelcome_done = true\nnext_tip = 3\n",
            info(None, vec![]),
        );
        assert!(prepare_request(&effects).is_none(), "no second welcome");
        assert_eq!(state.start().tip(), Some(3));
        assert_eq!(state.prefs.start.next_tip, 4, "the next start shows 4");
        update(&mut state, Message::Start(StartMessage::NextTip));
        assert_eq!(state.start().tip(), Some(4));
        update(&mut state, Message::Start(StartMessage::CloseTip));
        assert_eq!(state.start().tip(), None);
    }

    #[test]
    fn tips_wrap_in_both_directions() {
        let (mut state, _) = launched("[start]\nwelcome_done = true\n", info(None, vec![]));
        assert_eq!(state.start().tip(), Some(0));
        update(&mut state, Message::Start(StartMessage::PreviousTip));
        assert_eq!(state.start().tip(), Some(TIP_COUNT - 1));
        update(&mut state, Message::Start(StartMessage::NextTip));
        assert_eq!(state.start().tip(), Some(0));
    }

    #[test]
    fn tips_stay_away_when_turned_off_or_skipped() {
        let (state, _) = launched(
            "[start]\nwelcome_done = true\nshow_tip = false\n",
            info(None, vec![]),
        );
        assert_eq!(state.start().tip(), None);
        let mut skipped = info(None, vec![]);
        skipped.skip_tip = true;
        let (state, _) = launched("[start]\nwelcome_done = true\n", skipped);
        assert_eq!(state.start().tip(), None);
    }

    #[test]
    fn the_menu_can_open_the_tip_again_and_the_box_is_saved() {
        let (mut state, _) = launched(
            "[start]\nwelcome_done = true\nshow_tip = false\n",
            info(None, vec![]),
        );
        update(&mut state, Message::Start(StartMessage::ShowTip));
        assert!(state.start().tip().is_some());
        update(&mut state, Message::Start(StartMessage::SetShowTip(true)));
        assert!(state.prefs.start.show_tip);
    }

    #[test]
    fn without_a_preferences_file_files_open_but_no_welcome_or_tip() {
        let mut state = AppState::with_preferences_path(None);
        let file = LaunchFile::Base(PathBuf::from("a.pgn"));
        let effects = update(
            &mut state,
            Message::Start(StartMessage::Configure(Box::new(info(
                Some(PathBuf::from("MyChess")),
                vec![file],
            )))),
        );
        assert!(!state.start().is_open());
        assert!(effects.iter().any(|e| matches!(e, Effect::OpenBase { .. })));
    }

    #[test]
    fn command_line_files_open_one_at_a_time() {
        let files = vec![
            LaunchFile::Base(PathBuf::from("a.pgn")),
            LaunchFile::Base(PathBuf::from("b.pgn")),
            LaunchFile::Epd(PathBuf::from("c.epd")),
            LaunchFile::Book(PathBuf::from("d.bin")),
        ];
        let (state, effects) = launched("[start]\nwelcome_done = true\n", info(None, files));
        let opens = effects
            .iter()
            .filter(|e| matches!(e, Effect::OpenBase { .. }))
            .count();
        assert_eq!(opens, 1, "the second base waits for the first");
        assert_eq!(state.start().queued_files(), 3);
    }

    #[test]
    fn more_files_than_slots_are_cut() {
        let files = (0..20)
            .map(|n| LaunchFile::Base(PathBuf::from(format!("{n}.pgn"))))
            .collect();
        let (state, _) = launched("[start]\nwelcome_done = true\n", info(None, files));
        assert_eq!(state.start().queued_files(), MAX_LAUNCH_FILES - 1);
    }

    #[test]
    fn a_layout_warning_becomes_a_notice() {
        let mut with_warning = info(None, vec![]);
        with_warning.warnings = vec![crate::start::LayoutWarning::NoHomeFolder];
        let (state, _) = launched(
            "[start]\nwelcome_done = true\nshow_tip = false\n",
            with_warning,
        );
        assert_eq!(
            state.notice,
            Some(AppNotice::Start(StartNotice::NoHomeFolder))
        );
    }
}
