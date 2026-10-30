//! The state machine of the FICS play, driven by scripted lines (spec m8-9,
//! D2601-D2619). No network: the lines are the ones of the protocol.

use std::time::Instant;

use mychess_core::Square;
use mychess_online::fics::{Disconnect, FicsMessage as Net, SeekSpec, parse_line};

use super::{
    ConnectRequest, FicsCommand, FicsMessage, FicsNotice, FicsPrefs, FicsState, NetUpdate, Offer,
    Out, Phase, Role,
};

const START: &str = "<12> rnbqkbnr pppppppp -------- -------- -------- -------- PPPPPPPP RNBQKBNR W -1 1 1 1 1 0 7 me bob 1 5 0 39 39 300 300 1 none (0:00) none 0";
const E4: &str = "<12> rnbqkbnr pppppppp -------- -------- ----P--- -------- PPPP-PPP RNBQKBNR B 4 1 1 1 1 0 7 me bob -1 5 0 39 39 297 300 1 P/e2-e4 (0:03) e4 0";
const E5: &str = "<12> rnbqkbnr pppp-ppp -------- ----p--- ----P--- -------- PPPP-PPP RNBQKBNR W 4 1 1 1 1 0 7 me bob 1 5 0 39 39 297 296 2 P/e7-e5 (0:04) e5 0";

fn sq(s: &str) -> Square {
    s.parse().expect("square")
}

fn net(state: &FicsState, message: Net) -> FicsMessage {
    FicsMessage::Net(NetUpdate {
        epoch: state.epoch,
        at: Instant::now(),
        message,
    })
}

fn line(state: &mut FicsState, text: &str) -> Vec<Out> {
    let message = net(state, Net::Event(parse_line(text)));
    state.apply(message)
}

fn logged_in() -> FicsState {
    let mut s = FicsState::new();
    s.apply(FicsMessage::Connect(ConnectRequest { name: None }));
    let m = net(&s, Net::LoggedIn { name: "me".into() });
    s.apply(m);
    s
}

fn sent(outs: &[Out]) -> Vec<String> {
    outs.iter()
        .filter_map(|o| match o {
            Out::Command(FicsCommand::Send(c)) => Some(c.line().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn connect_login_and_stale_epochs() {
    let mut s = FicsState::new();
    let outs = s.apply(FicsMessage::Connect(ConnectRequest { name: None }));
    assert!(matches!(
        outs.as_slice(),
        [Out::Command(FicsCommand::Connect {
            epoch: 1,
            name: None,
            ..
        })]
    ));
    assert_eq!(s.phase, Phase::Connecting);
    // A second connect while connecting is refused.
    let outs = s.apply(FicsMessage::Connect(ConnectRequest { name: None }));
    assert_eq!(outs, vec![Out::Notice(FicsNotice::WrongState)]);
    // A message of an older connection is ignored.
    let stale = FicsMessage::Net(NetUpdate {
        epoch: 99,
        at: Instant::now(),
        message: Net::LoggedIn { name: "x".into() },
    });
    assert!(s.apply(stale).is_empty());
    assert_eq!(s.phase, Phase::Connecting);
    let m = net(&s, Net::LoggedIn { name: "me".into() });
    let outs = s.apply(m);
    assert_eq!(s.phase, Phase::LoggedIn);
    assert_eq!(s.me.as_deref(), Some("me"));
    assert!(matches!(
        outs.as_slice(),
        [Out::Notice(FicsNotice::Connected { .. })]
    ));
}

#[test]
fn a_bad_handle_never_leaves() {
    let mut s = FicsState::new();
    for name in ["a", "bad name", "x\ny", "waytoolongforahandle"] {
        let outs = s.apply(FicsMessage::Connect(ConnectRequest {
            name: Some(name.into()),
        }));
        assert_eq!(outs, vec![Out::Notice(FicsNotice::BadInput)], "{name}");
    }
    assert_eq!(s.phase, Phase::Disconnected);
}

#[test]
fn commands_need_a_session_and_the_console_is_safe() {
    let mut s = FicsState::new();
    assert_eq!(
        s.apply(FicsMessage::Console("who".into())),
        vec![Out::Notice(FicsNotice::WrongState)]
    );
    let mut s = logged_in();
    let outs = s.apply(FicsMessage::Console("  who  ".into()));
    assert_eq!(sent(&outs), vec!["who"]);
    // A line break would carry a second command.
    assert_eq!(
        s.apply(FicsMessage::Console("who\nresign".into())),
        vec![Out::Notice(FicsNotice::BadInput)]
    );
    // The echo of a password command is masked.
    s.apply(FicsMessage::Console("password old new".into()));
    let echoed: Vec<String> = s.console.lines().map(|l| l.text.clone()).collect();
    assert!(echoed.iter().any(|t| t == "password ****"));
    assert!(echoed.iter().all(|t| !t.contains("old")));
}

#[test]
fn the_seek_graph_follows_the_server_and_variants_cannot_be_played() {
    let mut s = logged_in();
    line(
        &mut s,
        "<s> 16 w=bob ti=00 rt=1500E t=5 i=0 r=u tp=blitz c=? rr=0-9999 a=t f=f",
    );
    line(
        &mut s,
        "<s> 17 w=eve ti=00 rt=1600E t=5 i=0 r=u tp=crazyhouse c=? rr=0-9999 a=t f=f",
    );
    assert_eq!(s.seeks.len(), 2);
    assert_eq!(sent(&s.apply(FicsMessage::PlaySeek(16))), vec!["play 16"]);
    assert_eq!(
        s.apply(FicsMessage::PlaySeek(17)),
        vec![Out::Notice(FicsNotice::WrongState)]
    );
    line(&mut s, "<sr> 16");
    assert_eq!(s.seeks.len(), 1);
    line(&mut s, "<sc>");
    assert!(s.seeks.is_empty());
}

#[test]
fn seek_and_challenge_make_the_protocol_lines_and_remember_the_choice() {
    let mut s = logged_in();
    let spec = SeekSpec {
        minutes: 10,
        increment: 5,
        rated: true,
        color: mychess_online::fics::ColorChoice::Any,
        manual: false,
        range: None,
    };
    assert_eq!(
        sent(&s.apply(FicsMessage::Seek(spec))),
        vec!["seek 10 5 rated"]
    );
    assert_eq!(
        (s.prefs.minutes, s.prefs.increment, s.prefs.rated),
        (10, 5, true)
    );
    let outs = s.apply(FicsMessage::Challenge {
        player: "bob".into(),
        minutes: 3,
        increment: 2,
        rated: false,
        color: mychess_online::fics::ColorChoice::White,
    });
    assert_eq!(sent(&outs), vec!["match bob 3 2 unrated white"]);
    let outs = s.apply(FicsMessage::Challenge {
        player: "b o b".into(),
        minutes: 3,
        increment: 2,
        rated: false,
        color: mychess_online::fics::ColorChoice::Any,
    });
    assert_eq!(outs, vec![Out::Notice(FicsNotice::BadInput)]);
}

#[test]
fn a_challenge_to_the_user_becomes_an_offer_he_can_accept() {
    let mut s = logged_in();
    line(&mut s, "Challenge: bob (1500) me (----) unrated blitz 5 0.");
    assert!(matches!(
        s.offers.as_slice(),
        [Offer::Match { from, minutes: 5, .. }] if from == "bob"
    ));
    // A challenge by the user himself is not an offer to him.
    line(&mut s, "Challenge: me (----) eve (1500) unrated blitz 5 0.");
    assert_eq!(s.offers.len(), 1);
    let outs = s.apply(FicsMessage::Accept(Some("bob".into())));
    assert_eq!(sent(&outs), vec!["accept bob"]);
    assert!(s.offers.is_empty());
}

#[test]
fn a_game_locks_the_assistance_and_plays_to_the_end() {
    let mut s = logged_in();
    let outs = line(&mut s, START);
    assert!(matches!(outs[0], Out::Starting(_)));
    assert!(outs.contains(&Out::Opened));
    assert!(s.assist_locked() && s.playing());
    let game = s.game.as_ref().expect("a game");
    assert_eq!(
        (game.role, game.my_color),
        (Role::Playing, Some(mychess_core::Color::White))
    );

    // The move goes out as coordinates and shows at once.
    let outs = s.apply(FicsMessage::PlayerMove {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
    });
    assert_eq!(sent(&outs), vec!["e2e4"]);
    assert!(outs.contains(&Out::Changed));
    assert!(
        s.game
            .as_ref()
            .is_some_and(super::game::FicsGame::has_pending_move)
    );
    // The server confirms; the opponent answers.
    line(&mut s, E4);
    assert!(
        !s.game
            .as_ref()
            .is_some_and(super::game::FicsGame::has_pending_move)
    );
    line(&mut s, E5);
    assert_eq!(s.game.as_ref().map(super::game::FicsGame::plies), Some(2));

    let outs = line(&mut s, "{Game 7 (me vs. bob) bob resigns} 1-0");
    assert!(outs.contains(&Out::Finished));
    assert!(!s.assist_locked(), "the lock goes with the result");
}

#[test]
fn the_server_refusing_a_move_takes_it_out() {
    let mut s = logged_in();
    line(&mut s, START);
    s.apply(FicsMessage::PlayerMove {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
    });
    let outs = line(&mut s, "Illegal move (e2e4).");
    assert!(outs.contains(&Out::Notice(FicsNotice::MoveRejected)));
    assert_eq!(s.game.as_ref().map(super::game::FicsGame::plies), Some(0));
    assert!(s.game.as_ref().is_some_and(super::game::FicsGame::my_turn));
}

#[test]
fn a_move_out_of_turn_is_a_premove_sent_when_it_becomes_legal() {
    let mut s = logged_in();
    line(&mut s, START);
    s.apply(FicsMessage::PlayerMove {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
    });
    line(&mut s, E4);
    // Black to move: Nf3 is only kept, nothing goes out.
    let outs = s.apply(FicsMessage::PlayerMove {
        from: sq("g1"),
        to: sq("f3"),
        promotion: None,
    });
    assert!(sent(&outs).is_empty());
    assert!(s.premove.is_set());
    // The opponent moves: the premove is legal and goes out.
    let outs = line(&mut s, E5);
    assert_eq!(sent(&outs), vec!["g1f3"]);
    assert!(!s.premove.is_set());
    // Cancelling empties the slot.
    s.apply(FicsMessage::PlayerMove {
        from: sq("a2"),
        to: sq("a3"),
        promotion: None,
    });
    s.apply(FicsMessage::CancelPremove);
    assert!(!s.premove.is_set());
}

#[test]
fn an_illegal_premove_is_dropped_silently_and_the_switch_turns_it_off() {
    let mut s = logged_in();
    line(&mut s, START);
    s.apply(FicsMessage::PlayerMove {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
    });
    line(&mut s, E4);
    // Bishop takes e5-ish nonsense that the pawn move below blocks.
    s.apply(FicsMessage::PlayerMove {
        from: sq("f1"),
        to: sq("b5"),
        promotion: None,
    });
    assert!(s.premove.is_set());
    let outs = line(
        &mut s,
        "<12> rnbqkbnr pppp-ppp -------- -------p ----P--- -------- PPPP-PPP RNBQKBNR W -1 1 1 1 1 0 7 me bob 1 5 0 39 39 297 296 2 P/h7-h5 (0:04) h5 0",
    );
    // f1-b5 is legal after e4 (the diagonal is open): it goes out.
    assert_eq!(sent(&outs), vec!["f1b5"]);

    let prefs = FicsPrefs {
        premove: false,
        ..FicsPrefs::default()
    };
    let mut s = logged_in();
    s.apply(FicsMessage::SetPrefs(Box::new(prefs)));
    line(&mut s, START);
    s.apply(FicsMessage::PlayerMove {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
    });
    line(&mut s, E4);
    s.apply(FicsMessage::PlayerMove {
        from: sq("g1"),
        to: sq("f3"),
        promotion: None,
    });
    assert!(!s.premove.is_set());
}

#[test]
fn promotion_sends_the_piece_first_and_the_automatic_choice_wins() {
    // White pawn on e7, kings apart, White to move.
    let board = "<12> k------- ----P--- -------- -------- -------- -------- -------- ----K--- W -1 0 0 0 0 0 9 me bob 1 5 0 4 1 100 100 40 none (0:00) none 0";
    let mut s = logged_in();
    line(&mut s, board);
    let outs = s.apply(FicsMessage::PlayerMove {
        from: sq("e7"),
        to: sq("e8"),
        promotion: Some(mychess_core::Role::Knight),
    });
    assert_eq!(sent(&outs), vec!["promote n", "e7e8"]);

    let mut s = logged_in();
    s.prefs.auto_promote = super::AutoPromote::Rook;
    line(&mut s, board);
    let outs = s.apply(FicsMessage::PlayerMove {
        from: sq("e7"),
        to: sq("e8"),
        promotion: Some(mychess_core::Role::Knight),
    });
    assert_eq!(sent(&outs), vec!["promote r", "e7e8"]);
}

#[test]
fn offers_resign_draw_abort_and_takeback_need_a_game() {
    let mut s = logged_in();
    assert_eq!(
        s.apply(FicsMessage::Resign),
        vec![Out::Notice(FicsNotice::WrongState)]
    );
    line(&mut s, START);
    line(&mut s, "bob offers you a draw.");
    assert!(matches!(s.offers.as_slice(), [Offer::Draw { .. }]));
    assert_eq!(sent(&s.apply(FicsMessage::OfferDraw)), vec!["draw"]);
    assert!(s.offers.is_empty(), "accepting answers the offer");
    assert_eq!(sent(&s.apply(FicsMessage::Resign)), vec!["resign"]);
    assert_eq!(sent(&s.apply(FicsMessage::Abort)), vec!["abort"]);
    assert_eq!(sent(&s.apply(FicsMessage::Takeback(2))), vec!["takeback 2"]);
    assert_eq!(
        s.apply(FicsMessage::Takeback(0)),
        vec![Out::Notice(FicsNotice::BadInput)]
    );
}

fn observed_board(game: u32, white: &str) -> String {
    format!(
        "<12> rnbqkbnr pppppppp -------- -------- -------- -------- PPPPPPPP RNBQKBNR W -1 1 1 1 1 0 {game} {white} zed 0 5 0 39 39 300 300 1 none (0:00) none 0"
    )
}

#[test]
fn observed_games_are_listed_up_to_the_limit_and_do_not_lock() {
    let mut s = logged_in();
    for n in 1..=8 {
        line(&mut s, &observed_board(n, "ann"));
    }
    assert_eq!(s.observed.len(), 8);
    assert!(!s.assist_locked());
    // The ninth is refused at the server.
    let outs = line(&mut s, &observed_board(9, "ann"));
    assert_eq!(sent(&outs), vec!["unobserve 9"]);
    assert_eq!(s.observed.len(), 8);
    assert_eq!(
        s.apply(FicsMessage::Observe("12".into())),
        vec![Out::Notice(FicsNotice::TooManyObserved)]
    );
    // Show one in the editor, then it goes away with the server's word.
    assert_eq!(s.apply(FicsMessage::Show(3)), vec![Out::Opened]);
    assert_eq!(s.editor_game().map(|g| g.number), Some(3));
    let outs = line(&mut s, "Removing game 3 from observation list.");
    assert!(outs.contains(&Out::Closed));
    assert_eq!(s.observed.len(), 7);
    // Now there is room.
    assert_eq!(
        sent(&s.apply(FicsMessage::Observe("12".into()))),
        vec!["observe 12"]
    );
    assert_eq!(
        sent(&s.apply(FicsMessage::Observe("eve".into()))),
        vec!["observe eve"]
    );
    assert_eq!(
        s.apply(FicsMessage::Observe("e v e".into())),
        vec![Out::Notice(FicsNotice::BadInput)]
    );
}

#[test]
fn an_observed_game_ends_with_its_result_and_does_not_import() {
    let mut s = logged_in();
    line(&mut s, &observed_board(4, "ann"));
    let outs = line(&mut s, "{Game 4 (ann vs. zed) zed resigns} 1-0");
    assert!(!outs.contains(&Out::Finished));
    assert!(s.observed[0].result.is_some());
}

#[test]
fn examining_opens_a_game_without_the_lock() {
    let mut s = logged_in();
    let examined = START.replace(" 1 5 0 39 39", " 2 5 0 39 39");
    let outs = line(&mut s, &examined);
    assert!(outs.contains(&Out::Opened));
    assert!(!outs.iter().any(|o| matches!(o, Out::Starting(_))));
    assert_eq!(s.game.as_ref().map(|g| g.role), Some(Role::Examining));
    assert!(!s.assist_locked());
    assert_eq!(sent(&s.apply(FicsMessage::Forward(2))), vec!["forward 2"]);
    assert_eq!(sent(&s.apply(FicsMessage::Backward(1))), vec!["backward 1"]);
    let outs = line(&mut s, "You are no longer examining game 7.");
    assert!(outs.contains(&Out::Closed));
    assert!(s.game.is_none());
}

#[test]
fn follow_lectures_and_examine_commands() {
    let mut s = logged_in();
    assert_eq!(
        sent(&s.apply(FicsMessage::Follow("bob".into()))),
        vec!["follow bob"]
    );
    assert_eq!(s.following.as_deref(), Some("bob"));
    assert_eq!(sent(&s.apply(FicsMessage::Unfollow)), vec!["unfollow"]);
    assert_eq!(s.following, None);
    assert_eq!(
        sent(&s.apply(FicsMessage::Lectures(true))),
        vec!["+channel 24"]
    );
    assert!(s.lectures);
    assert_eq!(
        sent(&s.apply(FicsMessage::Lectures(false))),
        vec!["-channel 24"]
    );
    assert_eq!(sent(&s.apply(FicsMessage::Examine(None))), vec!["examine"]);
    assert_eq!(sent(&s.apply(FicsMessage::Unexamine)), vec!["unexamine"]);
    line(&mut s, START);
    assert_eq!(
        s.apply(FicsMessage::Follow("bob".into())),
        vec![Out::Notice(FicsNotice::FollowWhilePlaying)]
    );
    assert_eq!(
        s.apply(FicsMessage::Examine(None)),
        vec![Out::Notice(FicsNotice::WrongState)]
    );
    // Seeking in the middle of a game is refused too.
    assert_eq!(
        s.apply(FicsMessage::Unseek).len(),
        1,
        "unseek is harmless and allowed"
    );
}

#[test]
fn chat_ping_and_latency_reach_the_state() {
    let mut s = logged_in();
    line(&mut s, "bob tells you: hi");
    line(&mut s, "GM_Lecturer(TM)(24): welcome");
    line(&mut s, "Average ping time for me: 83 ms");
    let m = net(&s, Net::Latency(std::time::Duration::from_millis(90)));
    s.apply(m);
    assert_eq!(s.ping_ms, Some(83));
    assert_eq!(s.latency, Some(std::time::Duration::from_millis(90)));
    let texts: Vec<String> = s.console.lines().map(|l| l.text.clone()).collect();
    assert!(texts.iter().any(|t| t == "bob tells you: hi"));
    assert!(texts.iter().any(|t| t == "[24] GM_Lecturer: welcome"));
}

#[test]
fn a_dropped_connection_keeps_the_game_and_its_lock_until_forgotten() {
    let mut s = logged_in();
    line(&mut s, START);
    let m = net(&s, Net::Disconnected(Disconnect::Io));
    let outs = s.apply(m);
    assert!(outs.contains(&Out::Notice(FicsNotice::Disconnected(Disconnect::Io))));
    assert_eq!(s.phase, Phase::Disconnected);
    assert!(s.assist_locked(), "the server may still hold the game");
    // Forgetting is only allowed without a connection.
    let outs = s.apply(FicsMessage::ForgetGame);
    assert!(outs.contains(&Out::Notice(FicsNotice::LockReleased)));
    assert!(!s.assist_locked());

    // With a connection it is refused.
    let mut s = logged_in();
    line(&mut s, START);
    assert_eq!(
        s.apply(FicsMessage::ForgetGame),
        vec![Out::Notice(FicsNotice::WrongState)]
    );
}

#[test]
fn disconnecting_by_hand_says_quit_and_forgets_the_lists() {
    let mut s = logged_in();
    line(
        &mut s,
        "<s> 16 w=bob ti=00 rt=1500E t=5 i=0 r=u tp=blitz c=? rr=0-9999 a=t f=f",
    );
    line(&mut s, &observed_board(2, "ann"));
    let outs = s.apply(FicsMessage::Disconnect);
    assert!(matches!(
        outs.last(),
        Some(Out::Command(FicsCommand::Disconnect { quit: true, .. }))
    ));
    assert!(s.seeks.is_empty() && s.observed.is_empty());
    assert_eq!(s.phase, Phase::Disconnected);
    // The late messages of the closed connection are ignored.
    let late = FicsMessage::Net(NetUpdate {
        epoch: 1,
        at: Instant::now(),
        message: Net::LoggedIn { name: "me".into() },
    });
    assert!(s.apply(late).is_empty());
    assert_eq!(s.phase, Phase::Disconnected);
}

#[test]
fn another_game_while_one_is_on_is_not_taken() {
    let mut s = logged_in();
    line(&mut s, START);
    let other = START.replace(" 0 7 me bob", " 0 8 me eve");
    let outs = line(&mut s, &other);
    // The only effect is the notice (D2613); the game on is untouched.
    assert_eq!(outs, [Out::Notice(FicsNotice::AnotherGame)]);
    assert_eq!(s.game.as_ref().map(|g| g.number), Some(7));
}

#[test]
fn preferences_are_made_safe() {
    let mut s = FicsState::new();
    let prefs = FicsPrefs {
        host: "bad host/".into(),
        port: 0,
        minutes: 99_999,
        increment: 99_999,
        init_commands: vec!["set shout 0".into(), "bad\ncommand".into()],
        ..FicsPrefs::default()
    };
    s.apply(FicsMessage::SetPrefs(Box::new(prefs)));
    assert_eq!(s.prefs.host, "freechess.org");
    assert_eq!(s.prefs.port, 5000);
    assert_eq!((s.prefs.minutes, s.prefs.increment), (600, 600));
    assert_eq!(s.prefs.init_commands, vec!["set shout 0".to_owned()]);
}

#[test]
fn hostile_boards_never_panic() {
    let mut s = logged_in();
    for text in [
        "<12>",
        "<12> a b c",
        "<12> rnbqkbnr pppppppp -------- -------- -------- -------- PPPPPPPP RNBQKBKR W -1 0 0 0 0 0 7 a b 1 5 0 39 39 300 300 1 none (0:00) none 0",
        &START.replace(" 7 me bob", " 4294967295 me bob"),
    ] {
        line(&mut s, text);
    }
}

// ---- review fixes of the M8-9 (D2690-D2698) -------------------------------

fn finished_game() -> FicsState {
    let mut s = logged_in();
    line(&mut s, START);
    line(&mut s, "{Game 7 (me vs. bob) bob resigns} 1-0");
    assert!(!s.assist_locked());
    s
}

fn plies(s: &FicsState) -> Option<usize> {
    s.game.as_ref().map(super::game::FicsGame::plies)
}

#[test]
fn a_new_game_under_the_number_of_a_finished_one_is_a_new_game() {
    for new_start in [START.to_owned(), START.replace(" 7 me bob ", " 7 me eve ")] {
        let mut s = finished_game();
        let outs = line(&mut s, &new_start);
        assert!(matches!(outs[0], Out::Starting(_)), "{outs:?}");
        assert!(outs.contains(&Out::Opened));
        assert!(s.assist_locked(), "the lock is on again");
        assert!(s.game.as_ref().is_some_and(|g| g.result.is_none()));
        // The user's move goes out (no WrongState).
        let outs = s.apply(FicsMessage::PlayerMove {
            from: sq("e2"),
            to: sq("e4"),
            promotion: None,
        });
        assert_eq!(sent(&outs), vec!["e2e4"]);
        // The end of this game is stored once more.
        let name = if new_start == START { "bob" } else { "eve" };
        let outs = line(
            &mut s,
            &format!("{{Game 7 (me vs. {name}) {name} resigns}} 1-0"),
        );
        assert!(outs.contains(&Out::Finished));
    }
}

#[test]
fn the_end_of_another_game_with_the_same_number_is_not_the_end_of_this_one() {
    let mut s = logged_in();
    line(&mut s, START);
    let outs = line(&mut s, "{Game 7 (zed vs. eve) eve resigns} 1-0");
    assert!(!outs.contains(&Out::Finished));
    assert!(s.assist_locked());
}

#[test]
fn a_late_board_of_a_finished_game_is_not_applied_to_it() {
    let mut s = finished_game();
    // A board of the examined kind under the same number is a new game.
    let examined = START.replace(" 1 5 0 39 39", " 2 5 0 39 39");
    let outs = line(&mut s, &examined);
    assert!(outs.contains(&Out::Opened));
    assert_eq!(s.game.as_ref().map(|g| g.role), Some(Role::Examining));
}

#[test]
fn other_refusals_of_the_server_take_the_move_out_too() {
    for text in ["It is not your move.", "You are not playing a game."] {
        let mut s = logged_in();
        line(&mut s, START);
        s.apply(FicsMessage::PlayerMove {
            from: sq("e2"),
            to: sq("e4"),
            promotion: None,
        });
        let outs = line(&mut s, text);
        assert!(
            outs.contains(&Out::Notice(FicsNotice::MoveRejected)),
            "{text}"
        );
        assert_eq!(plies(&s), Some(0));
    }
}

#[test]
fn a_move_nobody_answers_asks_for_the_board_and_then_is_taken_out() {
    let mut s = logged_in();
    line(&mut s, START);
    s.apply(FicsMessage::PlayerMove {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
    });
    let t0 = Instant::now();
    assert!(s.needs_ticks());
    assert!(s.apply(FicsMessage::Tick(t0)).is_empty());
    let outs = s.apply(FicsMessage::Tick(t0 + super::state::MOVE_REFRESH_AFTER));
    assert_eq!(sent(&outs), vec!["refresh 7"]);
    // Asked once only.
    let outs = s.apply(FicsMessage::Tick(t0 + super::state::MOVE_REFRESH_AFTER));
    assert!(sent(&outs).is_empty());
    assert_eq!(plies(&s), Some(1));
    let outs = s.apply(FicsMessage::Tick(t0 + super::state::MOVE_UNDO_AFTER));
    assert!(outs.contains(&Out::Notice(FicsNotice::MoveRejected)));
    assert_eq!(plies(&s), Some(0));
    assert!(!s.needs_ticks());
    assert!(s.game.as_ref().is_some_and(super::game::FicsGame::my_turn));
}

#[test]
fn the_board_the_server_sends_resolves_the_move_before_the_watchdog() {
    let mut s = logged_in();
    line(&mut s, START);
    s.apply(FicsMessage::PlayerMove {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
    });
    let t0 = Instant::now();
    s.apply(FicsMessage::Tick(t0));
    line(&mut s, E4);
    let outs = s.apply(FicsMessage::Tick(t0 + super::state::MOVE_UNDO_AFTER));
    assert!(outs.is_empty());
    assert_eq!(plies(&s), Some(1));
}

#[test]
fn a_connection_that_falls_on_a_move_takes_the_move_out() {
    let mut s = logged_in();
    line(&mut s, START);
    s.apply(FicsMessage::PlayerMove {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
    });
    let m = net(&s, Net::Disconnected(Disconnect::Io));
    let outs = s.apply(m);
    assert!(outs.contains(&Out::Changed));
    assert_eq!(plies(&s), Some(0));
    assert!(s.assist_locked(), "the game is still parked with its lock");
}

#[test]
fn a_send_failure_takes_out_only_the_move_that_failed() {
    let mut s = logged_in();
    line(&mut s, START);
    s.apply(FicsMessage::PlayerMove {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
    });
    let epoch = s.epoch();
    // Another command failed: the move stays.
    let outs = s.apply(FicsMessage::SendFailed {
        epoch,
        line: "say hi".into(),
    });
    assert_eq!(outs, vec![Out::Notice(FicsNotice::SendFailed)]);
    assert_eq!(plies(&s), Some(1));
    // A failure of an old connection is ignored.
    let outs = s.apply(FicsMessage::SendFailed {
        epoch: epoch + 5,
        line: "e2e4".into(),
    });
    assert!(outs.is_empty());
    assert_eq!(plies(&s), Some(1));
    // The move itself failed.
    let outs = s.apply(FicsMessage::SendFailed {
        epoch,
        line: "e2e4".into(),
    });
    assert!(outs.contains(&Out::Notice(FicsNotice::MoveRejected)));
    assert_eq!(plies(&s), Some(0));
}

#[test]
fn a_connection_that_never_opens_is_given_up() {
    let mut s = FicsState::new();
    s.apply(FicsMessage::Connect(ConnectRequest { name: None }));
    assert!(s.needs_ticks());
    let t0 = Instant::now();
    assert!(s.apply(FicsMessage::Tick(t0)).is_empty());
    let outs = s.apply(FicsMessage::Tick(t0 + super::state::CONNECT_TIMEOUT));
    assert_eq!(s.phase, Phase::Disconnected);
    assert!(outs.contains(&Out::Notice(FicsNotice::Disconnected(Disconnect::Timeout))));
    assert!(matches!(
        outs.iter().find(|o| matches!(o, Out::Command(_))),
        Some(Out::Command(FicsCommand::Disconnect {
            quit: false,
            epoch: 1
        }))
    ));
    // A new attempt works.
    let outs = s.apply(FicsMessage::Connect(ConnectRequest { name: None }));
    assert!(matches!(
        outs.as_slice(),
        [Out::Command(FicsCommand::Connect { epoch: 3, .. })]
    ));
}

#[test]
fn the_watchdog_outlasts_the_client_timeouts() {
    // D2630: the state's backstop must not cut a slow but healthy login.
    let config =
        mychess_online::fics::FicsConfig::new("localhost", 1, mychess_online::fics::Login::Guest);
    assert!(super::state::CONNECT_TIMEOUT > config.connect_timeout + config.login_timeout);
    let mut s = FicsState::new();
    s.apply(FicsMessage::Connect(ConnectRequest { name: None }));
    let t0 = Instant::now();
    s.apply(FicsMessage::Tick(t0));
    let almost = config.connect_timeout + config.login_timeout;
    assert!(s.apply(FicsMessage::Tick(t0 + almost)).is_empty());
    assert_eq!(s.phase, Phase::Connecting);
}

#[test]
fn disconnect_names_the_connection_it_closes() {
    let mut s = logged_in();
    let outs = s.apply(FicsMessage::Disconnect);
    assert!(matches!(
        outs.last(),
        Some(Out::Command(FicsCommand::Disconnect {
            quit: true,
            epoch: 1
        }))
    ));
}

#[test]
fn a_finished_game_can_be_forgotten_while_connected() {
    let mut s = finished_game();
    let outs = s.apply(FicsMessage::ForgetGame);
    assert!(outs.contains(&Out::Closed));
    assert!(s.game.is_none());
    // A live game is still protected while connected.
    let mut s = logged_in();
    line(&mut s, START);
    assert_eq!(
        s.apply(FicsMessage::ForgetGame),
        vec![Out::Notice(FicsNotice::WrongState)]
    );
}

#[test]
fn show_and_restore_drop_a_finished_game_but_not_a_live_one() {
    let mut s = finished_game();
    line(&mut s, &observed_board(4, "ann"));
    assert_eq!(s.apply(FicsMessage::Show(4)), vec![Out::Opened]);
    assert!(s.game.is_none());
    assert_eq!(s.editor_game().map(|g| g.number), Some(4));

    let mut s = finished_game();
    assert_eq!(s.apply(FicsMessage::RestorePrevious), vec![Out::Closed]);
    assert!(s.game.is_none());

    // A live examination holds the editor.
    let mut s = logged_in();
    line(&mut s, &observed_board(4, "ann"));
    let examined = START.replace(" 1 5 0 39 39", " 2 5 0 39 39");
    line(&mut s, &examined);
    assert_eq!(
        s.apply(FicsMessage::Show(4)),
        vec![Out::Notice(FicsNotice::WrongState)]
    );
}

#[test]
fn variants_are_not_shown_as_observed_or_examined_games() {
    let mut s = logged_in();
    line(
        &mut s,
        "Creating: ann (1500) zed (1500) rated crazyhouse 5 0",
    );
    let outs = line(&mut s, &observed_board(5, "ann"));
    assert!(s.observed.is_empty(), "{outs:?}");
    assert_eq!(sent(&outs), vec!["unobserve 5"]);
    let examined = observed_board(5, "ann").replace(" 0 5 ann zed 0 ", " 0 5 ann zed 2 ");
    let outs = line(&mut s, &examined);
    assert!(s.game.is_none(), "{outs:?}");
}

#[test]
fn accepting_without_a_name_answers_every_kind_of_offer_and_the_end_clears_them() {
    let mut s = logged_in();
    line(&mut s, "Bob offers you a draw.");
    assert_eq!(s.offers.len(), 1, "{:?}", s.offers);
    s.apply(FicsMessage::Accept(None));
    assert!(s.offers.is_empty());

    let mut s = logged_in();
    line(&mut s, START);
    line(&mut s, "bob offers you a draw.");
    assert!(!s.offers.is_empty());
    line(
        &mut s,
        "{Game 7 (me vs. bob) Game drawn by mutual agreement} 1/2-1/2",
    );
    assert!(s.offers.is_empty());
}
