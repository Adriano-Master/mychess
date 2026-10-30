//! M6-2d (GUI): the window "Tactics and patterns" opens from the action and
//! the menu, has its two tabs, turns the clicks into the messages of the
//! application, edits the tree of cards, asks about an expensive search,
//! navigates the occurrences and goes away on close (spec M6-2, D3281-D3285).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use iced::widget::Id;
use mychess_app::pattern_search::model::Condition;
use mychess_app::pattern_search::{
    ConditionKind, CostView, HitsView, PatternEdit, PatternSearchMessage, PatternTab,
};
use mychess_app::{Action, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::pattern_search_dialog::{
    PATTERN_ADD_ID, PATTERN_BAR_ID, PATTERN_BOX_ID, PATTERN_CLOSE_ID, PATTERN_COST_BOX_ID,
    PATTERN_COST_NO_ID, PATTERN_COST_YES_ID, PATTERN_HIDE_ID, PATTERN_HIT_CLEAR_ID,
    PATTERN_HIT_NEXT_ID, PATTERN_HIT_PREV_ID, PATTERN_REMOVE_ID, PATTERN_SHOW_ID, PATTERN_START_ID,
    PATTERN_STOP_ID, PatternSearchUi, PatternUiMessage, add_kind_id, card_id, cost_view, hits_row,
    preset_id, tab_id,
};
use mychess_gui::{Message, Shell, UiMessage, ViewOptions};

fn key(named: NamedKey) -> Message {
    Message::Dialog(DialogMessage::Key {
        input: KeyInput {
            key: Key::Named(named),
            modifiers: Modifiers::NONE,
        },
        text_focused: false,
        typed: None,
    })
}

fn open(shell: &mut Shell) {
    drop(shell.update(Message::Ui(UiMessage::Action(Action::SearchPattern))));
}

fn send(shell: &mut Shell, message: PatternSearchMessage) {
    drop(shell.update(Message::PatternUi(PatternUiMessage::Send(message))));
}

fn ui(shell: &mut Shell, message: PatternUiMessage) {
    drop(shell.update(Message::PatternUi(message)));
}

fn render(shell: &Shell) -> iced_test::Simulator<'_, Message> {
    let element = mychess_gui::windows::main_window::view(shell);
    iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1400.0, 1000.0),
        element,
    )
}

fn debug(messages: &[Message]) -> String {
    format!("{messages:?}")
}

fn click_messages(shell: &Shell, id: &str) -> String {
    let mut sim = render(shell);
    sim.click(Id::from(id.to_owned())).expect("the widget");
    debug(&sim.into_messages().collect::<Vec<_>>())
}

fn tab(shell: &Shell) -> PatternTab {
    shell.app.pattern_search_view().expect("open").tab
}

#[test]
fn the_action_opens_the_window_on_the_ready_made_tab() {
    let mut shell = Shell::new(None);
    assert!(!shell.modal_layer_open());
    open(&mut shell);
    assert_eq!(tab(&shell), PatternTab::Presets);
    assert!(shell.modal_layer_open());
    assert!(render(&shell).find(Id::new(PATTERN_BOX_ID)).is_ok());
}

#[test]
fn the_search_menu_item_opens_the_window() {
    let mut shell = Shell::new(None);
    let specs = menu_specs(&shell.app, &shell.ui, ViewOptions::default());
    let menu = specs
        .iter()
        .find(|s| s.label_key == "menu-search")
        .expect("the Search menu");
    let action = menu
        .entries
        .iter()
        .find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key("menu-search-pattern"),
                action,
                shortcut,
                ..
            } => {
                assert!(shortcut.is_none(), "no default key (D3289)");
                Some(action.clone())
            }
            _ => None,
        })
        .expect("the item")
        .expect("enabled");
    drop(shell.update(action));
    assert!(shell.app.pattern_search().is_open());
}

#[test]
fn a_tab_button_selects_its_tab() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    let messages = click_messages(&shell, tab_id(PatternTab::Advanced));
    assert!(messages.contains("SelectTab(Advanced)"), "{messages}");
    send(
        &mut shell,
        PatternSearchMessage::SelectTab(PatternTab::Advanced),
    );
    assert_eq!(tab(&shell), PatternTab::Advanced);
    assert!(render(&shell).find(Id::new(PATTERN_ADD_ID)).is_ok());
}

#[test]
fn picking_a_preset_and_a_parameter_sends_the_messages() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    let messages = click_messages(&shell, &preset_id("mate_in"));
    assert!(messages.contains("SelectPreset(\"mate_in\")"), "{messages}");
    send(
        &mut shell,
        PatternSearchMessage::SelectPreset("mate_in".to_owned()),
    );
    let messages = click_messages(&shell, "pattern-param-n-3");
    assert!(messages.contains("SetParam(N(3))"), "{messages}");
    send(
        &mut shell,
        PatternSearchMessage::SetParam(mychess_app::pattern_search::ParamValue::N(3)),
    );
    assert_eq!(shell.app.pattern_search_view().expect("open").params.n, 3);
}

#[test]
fn customize_copies_the_preset_with_translated_names_to_the_custom_tab() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    send(
        &mut shell,
        PatternSearchMessage::SelectPreset("fork".to_owned()),
    );
    ui(&mut shell, PatternUiMessage::Customize);
    let view = shell.app.pattern_search_view().expect("open");
    assert_eq!(view.tab, PatternTab::Advanced);
    assert!(
        ["Fork", "Garfo"].contains(&view.draft.name.as_str()),
        "{}",
        view.draft.name
    );
    assert_eq!(view.tree.len(), 1, "a preset is a single card");
}

#[test]
fn adding_and_removing_cards_edits_the_tree() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    send(
        &mut shell,
        PatternSearchMessage::SelectTab(PatternTab::Advanced),
    );
    ui(&mut shell, PatternUiMessage::ToggleAdd);
    let messages = click_messages(&shell, &add_kind_id(ConditionKind::Piece));
    assert!(messages.contains("Add(Piece)"), "{messages}");
    ui(&mut shell, PatternUiMessage::Add(ConditionKind::Piece));
    ui(&mut shell, PatternUiMessage::Add(ConditionKind::Mate));
    let view = shell.app.pattern_search_view().expect("open");
    assert_eq!(view.tree.len(), 3, "the group and two cards");
    // Select the first card and remove it.
    let messages = click_messages(&shell, &card_id(&[0]));
    assert!(messages.contains("Select(Some([0]))"), "{messages}");
    send(
        &mut shell,
        PatternSearchMessage::Edit(PatternEdit::Select(Some(vec![0]))),
    );
    let messages = click_messages(&shell, PATTERN_REMOVE_ID);
    assert!(messages.contains("Remove([0])"), "{messages}");
    send(
        &mut shell,
        PatternSearchMessage::Edit(PatternEdit::Remove(vec![0])),
    );
    let view = shell.app.pattern_search_view().expect("open");
    assert_eq!(view.tree.len(), 2);
}

#[test]
fn a_first_card_that_is_not_a_group_is_wrapped_before_adding() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    send(
        &mut shell,
        PatternSearchMessage::SelectPreset("fork".to_owned()),
    );
    ui(&mut shell, PatternUiMessage::Customize);
    assert!(!matches!(
        shell.app.pattern_search_view().expect("open").draft.root,
        Condition::All { .. }
    ));
    ui(&mut shell, PatternUiMessage::Add(ConditionKind::Phase));
    let view = shell.app.pattern_search_view().expect("open");
    assert!(matches!(view.draft.root, Condition::All { .. }));
    assert_eq!(view.tree.len(), 3);
}

#[test]
fn every_kind_of_card_has_an_editor_that_renders() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    send(
        &mut shell,
        PatternSearchMessage::SelectTab(PatternTab::Advanced),
    );
    for kind in ConditionKind::ALL {
        ui(&mut shell, PatternUiMessage::Add(kind));
    }
    let total = shell.app.pattern_search_view().expect("open").tree.len();
    assert!(total > ConditionKind::ALL.len());
    for index in 0..total {
        let path = shell.app.pattern_search_view().expect("open").tree[index]
            .path
            .clone();
        send(
            &mut shell,
            PatternSearchMessage::Edit(PatternEdit::Select(Some(path))),
        );
        assert!(render(&shell).find(Id::new(PATTERN_BOX_ID)).is_ok());
    }
}

#[test]
fn the_search_button_sends_start_and_stop_is_off_until_it_runs() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    let messages = click_messages(&shell, PATTERN_START_ID);
    assert!(messages.contains("Send(Start)"), "{messages}");
    let mut sim = render(&shell);
    drop(sim.click(Id::new(PATTERN_STOP_ID)));
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(!messages.contains("Stop"), "{messages}");
    send(
        &mut shell,
        PatternSearchMessage::SelectPreset("fork".to_owned()),
    );
    send(&mut shell, PatternSearchMessage::Start);
    if shell.app.pattern_search().is_running() {
        let messages = click_messages(&shell, PATTERN_STOP_ID);
        assert!(messages.contains("Send(Stop)"), "{messages}");
    }
}

/// Two games: the fool's mate (checks) and a quiet one (none).
const GAMES: &str = "[Event \"1\"]\n[Result \"0-1\"]\n\n1. f3 e5 2. g4 Qh4# 0-1\n\n\
[Event \"2\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 Nc6 *\n\n";

/// The application with `games` open in a slot (the work of `perform` is
/// done here, on a runtime of the test), and the folder of its patterns in
/// a temporary directory.
struct Host {
    app: mychess_app::AppState,
    rt: tokio::runtime::Runtime,
    /// Kept so that the folder lives as long as the host.
    dir: tempfile::TempDir,
}

impl Host {
    fn new(games: &str) -> Host {
        let dir = tempfile::tempdir().expect("dir");
        let mut app = mychess_app::AppState::default();
        app.open_pgn_in_slots = true;
        app.set_pattern_dir(Some(dir.path().join("patterns")));
        let mut host = Host {
            app,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
            dir,
        };
        let path = host.dir.path().join("t.pgn");
        std::fs::write(&path, games).expect("write");
        host.send(AppMessage::Bases(mychess_app::BasesMessage::Open(path)));
        host
    }

    /// `update` plus every effect that is not for the host, answers fed back.
    fn send(&mut self, message: AppMessage) {
        let mut queue = vec![message];
        while let Some(message) = queue.pop() {
            for effect in mychess_app::update(&mut self.app, message) {
                if !effect.runs_on_host()
                    && !matches!(effect, mychess_app::Effect::ScheduleAutoplayTick { .. })
                {
                    queue.push(self.rt.block_on(mychess_app::perform(effect)));
                }
            }
        }
    }

    fn pat(&mut self, message: PatternSearchMessage) {
        self.send(AppMessage::PatternSearch(message));
    }

    /// `update` only: the search effect is returned, not run.
    fn plan(&mut self, message: PatternSearchMessage) -> Vec<mychess_app::Effect> {
        mychess_app::update(&mut self.app, AppMessage::PatternSearch(message))
    }

    /// The "side to move is in check" pattern in the Custom tab.
    fn pick_check(&mut self) {
        self.pat(PatternSearchMessage::SelectTab(PatternTab::Advanced));
        self.pat(PatternSearchMessage::Edit(PatternEdit::Clear));
        self.pat(PatternSearchMessage::Edit(PatternEdit::Add {
            parent: vec![],
            kind: ConditionKind::Status,
        }));
    }
}

fn shell_over(host: Host) -> (Shell, tokio::runtime::Runtime, tempfile::TempDir) {
    let Host { app, rt, dir } = host;
    let mut shell = Shell::new(None);
    shell.app = app;
    (shell, rt, dir)
}

#[test]
fn enter_starts_the_search() {
    let mut host = Host::new(GAMES);
    host.pat(PatternSearchMessage::Open);
    let (mut shell, _rt, _dir) = shell_over(host);
    assert!(!shell.app.pattern_search().is_running());
    drop(shell.update(key(NamedKey::Enter)));
    assert!(shell.app.pattern_search().is_open());
    assert!(
        shell.app.pattern_search().is_running(),
        "Enter planned a search that waits for its answer"
    );
}

#[test]
fn escape_stops_a_running_search_and_only_then_closes() {
    let mut host = Host::new(GAMES);
    host.pat(PatternSearchMessage::Open);
    let effects = host.plan(PatternSearchMessage::Start);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(host.app.pattern_search().is_running());
    let (mut shell, rt, _dir) = shell_over(host);
    // The first Esc only asks the search to stop: the window stays.
    drop(shell.update(key(NamedKey::Escape)));
    assert!(shell.app.pattern_search().is_open());
    assert!(shell.app.pattern_search().is_running());
    // The search answers; with nothing running the next Esc closes.
    let effect = effects.into_iter().next().expect("the effect");
    let answer = rt.block_on(mychess_app::perform(effect));
    drop(shell.update(Message::App(answer)));
    assert!(!shell.app.pattern_search().is_running());
    assert!(shell.app.pattern_search().is_open());
    drop(shell.update(key(NamedKey::Escape)));
    assert!(!shell.app.pattern_search().is_open());
}

#[test]
fn escape_closes_the_window_when_nothing_runs() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    drop(shell.update(key(NamedKey::Escape)));
    assert!(!shell.app.pattern_search().is_open());
    assert!(!shell.modal_layer_open());
}

#[test]
fn the_close_button_closes_the_window() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    let mut sim = render(&shell);
    sim.click(Id::new(PATTERN_CLOSE_ID)).expect("the button");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(debug(&messages).contains("Send(Close)"));
    for message in messages {
        drop(shell.update(message));
    }
    assert!(shell.app.pattern_search_view().is_none());
    assert!(render(&shell).find(Id::new(PATTERN_BOX_ID)).is_err());
}

#[test]
fn the_question_about_the_cost_answers_yes_and_no() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    let cost = CostView {
        estimated_positions: 3_000_000,
        games: 50_000,
    };
    let i18n = I18n::new(Language::EnUs);
    let element = cost_view(cost, &i18n);
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(900.0, 600.0),
        element,
    );
    assert!(sim.find(Id::new(PATTERN_COST_BOX_ID)).is_ok());
    assert!(sim.find("3000000").is_ok() || sim.find(Id::new(PATTERN_COST_YES_ID)).is_ok());
    sim.click(Id::new(PATTERN_COST_YES_ID)).expect("yes");
    sim.click(Id::new(PATTERN_COST_NO_ID)).expect("no");
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(messages.contains("AnswerCost(true)"), "{messages}");
    assert!(messages.contains("AnswerCost(false)"), "{messages}");
}

#[test]
fn the_occurrence_buttons_navigate_and_clear() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    let mut view = shell.app.pattern_search_view().expect("open");
    assert!(
        hits_row(&view, &I18n::new(Language::EnUs)).is_none(),
        "nothing to show without occurrences"
    );
    view.hits = Some(HitsView {
        plies: vec![3, 9, 14],
        current: 1,
        from_test: false,
        game: Some(1),
    });
    let i18n = I18n::new(Language::EnUs);
    let row = hits_row(&view, &i18n).expect("occurrences");
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(900.0, 100.0),
        row,
    );
    assert!(sim.find("Occurrence 2 of 3").is_ok());
    sim.click(Id::new(PATTERN_HIT_PREV_ID)).expect("previous");
    sim.click(Id::new(PATTERN_HIT_NEXT_ID)).expect("next");
    sim.click(Id::new(PATTERN_HIT_CLEAR_ID)).expect("clear");
    let messages = debug(&sim.into_messages().collect::<Vec<_>>());
    for expected in ["PrevHit", "NextHit", "ClearHits"] {
        assert!(messages.contains(expected), "{expected}: {messages}");
    }
}

#[test]
fn hiding_the_window_leaves_a_bar_that_brings_it_back() {
    let mut shell = Shell::new(None);
    open(&mut shell);
    let messages = click_messages(&shell, PATTERN_HIDE_ID);
    assert!(messages.contains("Hide(true)"), "{messages}");
    ui(&mut shell, PatternUiMessage::Hide(true));
    assert!(shell.app.pattern_search().is_open());
    assert!(!shell.modal_layer_open(), "the board is free");
    assert!(render(&shell).find(Id::new(PATTERN_BAR_ID)).is_ok());
    let messages = click_messages(&shell, PATTERN_SHOW_ID);
    assert!(messages.contains("Hide(false)"), "{messages}");
    // Opening it again from the menu also brings the window back.
    open(&mut shell);
    assert!(shell.modal_layer_open());
    assert!(render(&shell).find(Id::new(PATTERN_BOX_ID)).is_ok());
}

#[test]
fn the_numeric_fields_of_the_scope_reach_the_application() {
    use mychess_gui::windows::pattern_search_dialog::NumField;
    let mut shell = Shell::new(None);
    open(&mut shell);
    for (field, text) in [
        (NumField::MoveMin, "5"),
        (NumField::MoveMax, "40"),
        (NumField::Hold, "3"),
    ] {
        ui(&mut shell, PatternUiMessage::Number(field, text.to_owned()));
    }
    let scope = shell.app.pattern_search_view().expect("open").scope;
    assert_eq!((scope.min_move, scope.max_move, scope.hold), (5, 40, 3));
    // Not a number: nothing changes.
    ui(
        &mut shell,
        PatternUiMessage::Number(NumField::Hold, "x".to_owned()),
    );
    assert_eq!(shell.app.pattern_search_view().expect("open").scope.hold, 3);
}

#[test]
fn the_progress_tick_message_is_the_one_the_application_reads() {
    let mut host = Host::new(GAMES);
    host.pat(PatternSearchMessage::Open);
    let mut effects = host.plan(PatternSearchMessage::Start);
    let effect = effects.pop().expect("the search");
    let control = match &effect {
        mychess_app::Effect::PatternSearch { control, .. } => control.clone(),
        other => panic!("not a search: {other:?}"),
    };
    let (mut shell, rt, _dir) = shell_over(host);
    let progress = |shell: &Shell| {
        shell
            .app
            .pattern_search_view()
            .unwrap()
            .running
            .unwrap()
            .progress
    };
    assert_eq!(progress(&shell), (0, 0));
    // The work reports to its control; the window shows it after a tick.
    let _answer = rt.block_on(mychess_app::perform(effect));
    let reported = control.progress();
    assert_ne!(reported, (0, 0), "the work reported its progress");
    assert_eq!(progress(&shell), (0, 0), "not before the tick");
    drop(shell.update(Message::App(AppMessage::PatternSearch(
        PatternSearchMessage::Progress,
    ))));
    assert_eq!(progress(&shell), reported);
}

#[test]
fn the_window_hides_by_itself_when_a_search_ends_with_occurrences() {
    let mut host = Host::new(GAMES);
    host.pat(PatternSearchMessage::Open);
    host.pick_check();
    let mut ui = PatternSearchUi::default();
    ui.sync(&host.app);
    assert!(!ui.is_hidden());
    let mut effects = host.plan(PatternSearchMessage::Start);
    ui.sync(&host.app);
    assert!(host.app.pattern_search().is_running());
    assert!(!ui.is_hidden(), "not while it runs");
    // The answer arrives, the first game is loaded and its occurrences are
    // computed.
    let effect = effects.pop().expect("the search");
    let answer = host.rt.block_on(mychess_app::perform(effect));
    host.send(answer);
    assert!(!host.app.pattern_search().is_running());
    let hits = host.app.pattern_search_hits().expect("occurrences");
    assert!(!hits.from_test && !hits.plies.is_empty(), "{hits:?}");
    ui.sync(&host.app);
    assert!(ui.is_hidden());
    // Bringing it back is the user's choice and sticks.
    ui.set_hidden(false);
    ui.sync(&host.app);
    assert!(!ui.is_hidden());
}

#[test]
fn the_window_stays_when_nothing_matched() {
    let mut host = Host::new(GAMES);
    host.pat(PatternSearchMessage::Open);
    // No white mate in one in these games (the fool's mate is Black's).
    host.pat(PatternSearchMessage::SelectPreset("mate_in".to_owned()));
    host.pat(PatternSearchMessage::SetParam(mychess_app::ParamValue::By(
        mychess_app::pattern_search::model::Who::White,
    )));
    host.pat(PatternSearchMessage::SetParam(mychess_app::ParamValue::N(
        1,
    )));
    let mut ui = PatternSearchUi::default();
    ui.sync(&host.app);
    let mut effects = host.plan(PatternSearchMessage::Start);
    ui.sync(&host.app);
    assert!(host.app.pattern_search().is_running());
    let answer = host
        .rt
        .block_on(mychess_app::perform(effects.pop().expect("search")));
    host.send(answer);
    assert_eq!(host.app.pattern_search().last_matched(), Some(0));
    ui.sync(&host.app);
    assert!(!ui.is_hidden(), "0 games: nothing to look at");
    // Nothing is pending afterwards either.
    ui.sync(&host.app);
    assert!(!ui.is_hidden());
}
