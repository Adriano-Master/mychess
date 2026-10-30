//! The fields of the editor of the search for patterns: the controls that
//! change the card selected in the tree (spec M6-2, D3282) and the board of
//! 64 squares that sets a region.
//!
//! Every control builds its message when the view is built (the message
//! carries the whole edited card, `PatternEdit::Replace`), so nothing here
//! keeps state; the numeric text fields keep their drafts in
//! [`PatternSearchUi`].

use iced::widget::{button, checkbox, column, pick_list, row, text};
use iced::{Alignment, Element, Length};
use mychess_app::pattern_search::model::{
    BishopColors, CastleKind, Condition, Counts, DiscoveredKind, Feature, Flag, Gain, MateMode,
    MaterialCond, MinMax, MoveCond, NamedRegion, Outcome, Phase, PieceCond, Region, RelKind,
    RelationCond, ResultKind, RoleName, Side, Sq, Status, When, Who,
};
use mychess_app::pattern_search::{
    GroupKind, NodePath, NodeView, PatternEdit, PatternSearchMessage, RegionSlot, group_kind,
};
use mychess_core::{File, Rank, Square};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme::{self, ButtonLook};
use crate::windows::annotate_dialog::{choice, group};
use crate::windows::header_dialog::Labeled;
use crate::windows::pattern_search_dialog::{NumField, PatternSearchUi, PatternUiMessage};
use crate::windows::pattern_search_text::{capitalized, region_text, role_key, who_key};

const LABEL_WIDTH: f32 = 150.0;
const SQUARE_SIZE: f32 = 24.0;

/// The five values of "who".
pub const WHO_ALL: [Who; 5] = [Who::White, Who::Black, Who::Mover, Who::Opponent, Who::Any];
/// "Who" where the side to move makes no sense (after a move).
pub const WHO_AFTER: [Who; 4] = [Who::White, Who::Black, Who::Opponent, Who::Any];
/// "Who" for a mate (the other side makes no sense).
pub const WHO_MATE: [Who; 4] = [Who::White, Who::Black, Who::Mover, Who::Any];
/// Every kind of piece, in the order of the chips.
pub const ROLES: [RoleName; 6] = [
    RoleName::King,
    RoleName::Queen,
    RoleName::Rook,
    RoleName::Bishop,
    RoleName::Knight,
    RoleName::Pawn,
];
/// The kinds a pawn promotes to.
pub const PROMOTIONS: [RoleName; 4] = [
    RoleName::Queen,
    RoleName::Rook,
    RoleName::Bishop,
    RoleName::Knight,
];
const NAMED: [NamedRegion; 10] = [
    NamedRegion::Center,
    NamedRegion::ExtendedCenter,
    NamedRegion::QueenSide,
    NamedRegion::KingSide,
    NamedRegion::WhiteHalf,
    NamedRegion::BlackHalf,
    NamedRegion::LightSquares,
    NamedRegion::DarkSquares,
    NamedRegion::Edge,
    NamedRegion::LongDiagonals,
];

// ---- messages ----

/// A message for the application.
pub fn send(message: PatternSearchMessage) -> Message {
    Message::PatternUi(PatternUiMessage::Send(message))
}

/// An edit of the pattern.
pub fn edit(edit: PatternEdit) -> Message {
    send(PatternSearchMessage::Edit(edit))
}

/// Replaces the card at `path`.
pub fn replace(path: &NodePath, condition: Condition) -> Message {
    edit(PatternEdit::Replace {
        path: path.clone(),
        condition: Box::new(condition),
    })
}

fn to_u8(n: i64) -> u8 {
    u8::try_from(n.clamp(0, 255)).unwrap_or(u8::MAX)
}

// ---- small controls ----

/// `label  body`.
pub fn field<'a>(i18n: &I18n, key: &str, body: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(i18n.tr(key))
            .size(13)
            .width(Length::Fixed(LABEL_WIDTH)),
        body
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

/// `[-] value [+]`, the buttons off at the ends of the range.
pub fn num_stepper<'a>(
    value: i64,
    low: i64,
    high: i64,
    make: impl Fn(i64) -> Message,
) -> Element<'a, Message> {
    let step = |glyph: &'static str, message: Option<Message>| {
        button(text(glyph).size(13))
            .padding([2, 10])
            .style(button::secondary)
            .on_press_maybe(message)
    };
    row![
        step("\u{2212}", (value > low).then(|| make(value - 1))),
        text(value.to_string())
            .size(13)
            .width(Length::Fixed(34.0))
            .align_x(Alignment::Center),
        step("+", (value < high).then(|| make(value + 1))),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .into()
}

/// A check box whose two messages are known now.
pub fn toggle<'a>(
    label: String,
    on: bool,
    if_true: Message,
    if_false: Message,
) -> Element<'a, Message> {
    checkbox(on)
        .label(label)
        .size(15)
        .text_size(13)
        .on_toggle(move |v| if v { if_true.clone() } else { if_false.clone() })
        .into()
}

/// Exclusive choices.
pub fn choices<'a, T: Copy + PartialEq + 'a>(
    options: &[(T, String)],
    current: T,
    make: impl Fn(T) -> Message,
) -> Element<'a, Message> {
    group(
        options
            .iter()
            .map(|(value, label)| choice(label.clone(), *value, current, true, &make))
            .collect(),
    )
}

/// "White / Black / ..." for the values in `allowed`.
pub fn who_choices<'a>(
    i18n: &I18n,
    current: Who,
    allowed: &[Who],
    make: impl Fn(Who) -> Message,
) -> Element<'a, Message> {
    let options: Vec<(Who, String)> = allowed.iter().map(|w| (*w, i18n.tr(who_key(*w)))).collect();
    choices(&options, current, make)
}

/// One toggle button per kind of piece; none chosen means any piece.
pub fn role_chips<'a>(
    i18n: &I18n,
    current: &[RoleName],
    all: &[RoleName],
    make: impl Fn(Vec<RoleName>) -> Message,
) -> Element<'a, Message> {
    group(
        all.iter()
            .map(|role| {
                let on = current.contains(role);
                let next: Vec<RoleName> = if on {
                    current.iter().copied().filter(|r| r != role).collect()
                } else {
                    let mut list = current.to_vec();
                    list.push(*role);
                    list
                };
                button(text(capitalized(&i18n.tr(role_key(*role)))).size(13))
                    .padding([3, 8])
                    .style(theme::command_button(ButtonLook::Secondary, on))
                    .on_press(make(next))
                    .into()
            })
            .collect(),
    )
}

fn flag_choices<'a>(
    i18n: &I18n,
    current: Flag,
    make: impl Fn(Flag) -> Message,
) -> Element<'a, Message> {
    choices(
        &[
            (Flag::Both, capitalized(&i18n.tr("pattern-flag-both"))),
            (Flag::Yes, capitalized(&i18n.tr("pattern-flag-yes"))),
            (Flag::No, capitalized(&i18n.tr("pattern-flag-no"))),
        ],
        current,
        make,
    )
}

/// A numeric text field with a draft.
pub fn number_input<'a>(
    ui: &PatternSearchUi,
    field: NumField,
    current: i64,
    width: f32,
) -> Element<'a, Message> {
    iced::widget::text_input("", &ui.shown(&field, current))
        .size(13)
        .padding([2, 4])
        .width(Length::Fixed(width))
        .on_input(move |t| Message::PatternUi(PatternUiMessage::Number(field.clone(), t)))
        .into()
}

// ---- regions ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum RegionKind {
    Squares,
    File,
    Rank,
    OwnRank,
    Rect,
    Named,
}

impl RegionKind {
    const ALL: [RegionKind; 6] = [
        RegionKind::Squares,
        RegionKind::File,
        RegionKind::Rank,
        RegionKind::OwnRank,
        RegionKind::Rect,
        RegionKind::Named,
    ];

    fn of(region: &Region) -> RegionKind {
        match region {
            Region::Anywhere | Region::Squares(_) => RegionKind::Squares,
            Region::File(_) => RegionKind::File,
            Region::Rank(_) => RegionKind::Rank,
            Region::OwnRank(_) => RegionKind::OwnRank,
            Region::Rect { .. } => RegionKind::Rect,
            Region::Named(_) => RegionKind::Named,
        }
    }

    fn key(self) -> &'static str {
        match self {
            RegionKind::Squares => "pattern-region-kind-squares",
            RegionKind::File => "pattern-region-kind-file",
            RegionKind::Rank => "pattern-region-kind-rank",
            RegionKind::OwnRank => "pattern-region-kind-own-rank",
            RegionKind::Rect => "pattern-region-kind-rect",
            RegionKind::Named => "pattern-region-kind-named",
        }
    }

    /// The region a click on the kind starts with.
    fn start(self, now: &Region) -> Region {
        match self {
            RegionKind::Squares => match now {
                Region::Squares(_) => now.clone(),
                _ => Region::Anywhere,
            },
            RegionKind::File => Region::File('a'),
            RegionKind::Rank => Region::Rank(1),
            RegionKind::OwnRank => Region::OwnRank(2),
            RegionKind::Rect => Region::Rect {
                from: Sq(Square::C3),
                to: Sq(Square::F6),
            },
            RegionKind::Named => Region::Named(NamedRegion::Center),
        }
    }
}

fn squares_pick(options: &[Labeled<Square>], current: Square) -> Option<Labeled<Square>> {
    options.iter().find(|l| l.value == current).cloned()
}

#[allow(clippy::too_many_lines)] // one flat match over the kinds of region
/// The editor of one set of squares: the kind, then what the kind needs
/// (the board of 64 squares, a file, a rank, two corners or an area).
pub fn region_editor<'a>(
    i18n: &I18n,
    title: String,
    path: &NodePath,
    slot: RegionSlot,
    region: &Region,
) -> Element<'a, Message> {
    let set = |r: Region| {
        edit(PatternEdit::SetRegion {
            path: path.clone(),
            slot,
            region: r,
        })
    };
    let kinds: Vec<(RegionKind, String)> = RegionKind::ALL
        .iter()
        .map(|k| (*k, i18n.tr(k.key())))
        .collect();
    let mut body = column![
        text(title).size(13).style(theme::accent_text),
        choices(&kinds, RegionKind::of(region), |k| set(k.start(region)))
    ]
    .spacing(4);
    body = match region {
        Region::Anywhere | Region::Squares(_) => body.push(square_board(i18n, path, slot, region)),
        Region::File(now) => {
            let options: Vec<(char, String)> = File::ALL
                .iter()
                .map(|f| (f.char(), f.char().to_string()))
                .collect();
            body.push(choices(&options, *now, |c| set(Region::File(c))))
        }
        Region::Rank(now) | Region::OwnRank(now) => {
            let own = matches!(region, Region::OwnRank(_));
            let options: Vec<(u8, String)> = (1..=8).map(|n| (n, n.to_string())).collect();
            body.push(choices(&options, *now, |n| {
                set(if own {
                    Region::OwnRank(n)
                } else {
                    Region::Rank(n)
                })
            }))
        }
        Region::Rect { from, to } => {
            let options: Vec<Labeled<Square>> = Square::ALL
                .iter()
                .map(|s| Labeled {
                    value: *s,
                    label: s.to_string(),
                })
                .collect();
            let (a, b) = (*from, *to);
            let corner = |label: &str, now: Square, is_from: bool| {
                let options = options.clone();
                let owned_path = path.clone();
                row![
                    text(label.to_owned()).size(13),
                    pick_list(
                        options.clone(),
                        squares_pick(&options, now),
                        move |l: Labeled<Square>| {
                            let region = if is_from {
                                Region::Rect {
                                    from: Sq(l.value),
                                    to: b,
                                }
                            } else {
                                Region::Rect {
                                    from: a,
                                    to: Sq(l.value),
                                }
                            };
                            edit(PatternEdit::SetRegion {
                                path: owned_path.clone(),
                                slot,
                                region,
                            })
                        },
                    )
                    .text_size(12)
                    .padding(3)
                    .width(Length::Fixed(80.0))
                ]
                .spacing(6)
                .align_y(Alignment::Center)
            };
            body.push(
                row![
                    corner(&i18n.tr("pattern-region-from"), from.0, true),
                    corner(&i18n.tr("pattern-region-to"), to.0, false)
                ]
                .spacing(14),
            )
        }
        Region::Named(now) => {
            let options: Vec<(NamedRegion, String)> = NAMED
                .iter()
                .map(|n| (*n, capitalized(&region_text(i18n, &Region::Named(*n)))))
                .collect();
            body.push(choices(&options, *now, |n| set(Region::Named(n))))
        }
    };
    body.into()
}

/// The 64 squares; a click turns one on or off.
fn square_board<'a>(
    i18n: &I18n,
    path: &NodePath,
    slot: RegionSlot,
    region: &Region,
) -> Element<'a, Message> {
    let marked: Vec<Square> = match region {
        Region::Squares(list) => list.iter().map(|s| s.0).collect(),
        _ => Vec::new(),
    };
    let mut board = column![].spacing(1);
    for rank in Rank::ALL.iter().rev() {
        let mut line = row![
            text(rank.char().to_string())
                .size(11)
                .width(Length::Fixed(12.0))
        ]
        .spacing(1)
        .align_y(Alignment::Center);
        for file in File::ALL {
            let square = Square::from_coords(file, *rank);
            let on = marked.contains(&square);
            line = line.push(
                button(
                    text(if on { "\u{25cf}" } else { "" })
                        .size(11)
                        .align_x(Alignment::Center),
                )
                .padding(0)
                .width(Length::Fixed(SQUARE_SIZE))
                .height(Length::Fixed(SQUARE_SIZE))
                .style(theme::command_button(ButtonLook::Secondary, on))
                .on_press(edit(PatternEdit::ToggleSquare {
                    path: path.clone(),
                    slot,
                    square,
                })),
            );
        }
        board = board.push(line);
    }
    let mut files = row![iced::widget::space::horizontal().width(Length::Fixed(12.0))].spacing(1);
    for file in File::ALL {
        files = files.push(
            text(file.char().to_string())
                .size(11)
                .width(Length::Fixed(SQUARE_SIZE))
                .align_x(Alignment::Center),
        );
    }
    let side = column![
        text(i18n.tr("pattern-region-pick"))
            .size(12)
            .style(theme::muted_text),
        text(region_text(i18n, region)).size(12),
        button(text(i18n.tr("pattern-region-use-marks")).size(12))
            .padding([2, 8])
            .style(button::secondary)
            .on_press(edit(PatternEdit::UseMarks {
                path: path.clone(),
                slot
            })),
        button(text(i18n.tr("pattern-region-clear")).size(12))
            .padding([2, 8])
            .style(button::secondary)
            .on_press_maybe(matches!(region, Region::Squares(_)).then(|| {
                edit(PatternEdit::SetRegion {
                    path: path.clone(),
                    slot,
                    region: Region::Anywhere,
                })
            })),
    ]
    .spacing(6)
    .width(Length::Fixed(190.0));
    row![column![board, files].spacing(1), side]
        .spacing(12)
        .into()
}

// ---- the editor of a card ----

/// The fields of the selected card.
#[allow(clippy::too_many_lines)] // one flat match over the kinds reads best
pub fn card_editor<'a>(ui: &PatternSearchUi, node: &NodeView, i18n: &I18n) -> Element<'a, Message> {
    let path = &node.path;
    match &node.condition {
        Condition::All { .. } | Condition::Any { .. } | Condition::Not { .. } => {
            group_editor(node, i18n)
        }
        Condition::Piece(p) => piece_editor(ui, path, p, i18n),
        Condition::Material(m) => material_editor(path, m, i18n),
        Condition::SideToMove { side } => {
            let options = [
                (Side::White, i18n.tr("pattern-who-white")),
                (Side::Black, i18n.tr("pattern-who-black")),
            ];
            field(
                i18n,
                "pattern-field-side",
                choices(&options, *side, |s| {
                    replace(path, Condition::SideToMove { side: s })
                }),
            )
        }
        Condition::Phase { phase } => {
            let options = [
                (Phase::Opening, "pattern-phase-opening"),
                (Phase::Middlegame, "pattern-phase-middlegame"),
                (Phase::Endgame, "pattern-phase-endgame"),
            ]
            .map(|(v, k)| (v, capitalized(&i18n.tr(k))));
            field(
                i18n,
                "pattern-field-phase",
                choices(&options, *phase, |p| {
                    replace(path, Condition::Phase { phase: p })
                }),
            )
        }
        Condition::Status { status } => {
            let options = [
                (Status::Check, "pattern-status-check"),
                (Status::Checkmate, "pattern-status-checkmate"),
                (Status::Stalemate, "pattern-status-stalemate"),
            ]
            .map(|(v, k)| (v, capitalized(&i18n.tr(k))));
            field(
                i18n,
                "pattern-field-status",
                choices(&options, *status, |s| {
                    replace(path, Condition::Status { status: s })
                }),
            )
        }
        Condition::Result { results } => {
            let chips = [
                (ResultKind::WhiteWins, "pattern-result-white-wins"),
                (ResultKind::Draw, "pattern-result-draw"),
                (ResultKind::BlackWins, "pattern-result-black-wins"),
                (ResultKind::Unknown, "pattern-result-unknown"),
            ]
            .map(|(kind, key)| {
                let on = results.contains(&kind);
                let next: Vec<ResultKind> = if on {
                    results.iter().copied().filter(|r| *r != kind).collect()
                } else {
                    let mut list = results.clone();
                    list.push(kind);
                    list
                };
                button(text(capitalized(&i18n.tr(key))).size(13))
                    .padding([3, 8])
                    .style(theme::command_button(ButtonLook::Secondary, on))
                    .on_press(replace(path, Condition::Result { results: next }))
                    .into()
            });
            field(i18n, "pattern-field-results", group(chips.into()))
        }
        Condition::Move(m) => move_editor(path, m, i18n),
        Condition::Feature(f) => {
            let set = |g: &dyn Fn(&mut mychess_app::pattern_search::model::FeatureCond)| {
                let mut c = f.clone();
                g(&mut c);
                replace(path, Condition::Feature(c))
            };
            let mut body = column![field(
                i18n,
                "pattern-field-side",
                who_choices(i18n, f.side, &WHO_ALL, |w| set(&|c| c.side = w))
            )]
            .spacing(8);
            if f.feature != Feature::BishopPair {
                body = body.push(field(
                    i18n,
                    "pattern-field-min",
                    num_stepper(i64::from(f.min), 1, 8, |n| set(&|c| c.min = to_u8(n))),
                ));
                body = body.push(region_editor(
                    i18n,
                    i18n.tr("pattern-field-where"),
                    path,
                    RegionSlot::Main,
                    &f.region,
                ));
            }
            body.into()
        }
        Condition::Relation(r) => relation_editor(path, r, i18n),
        Condition::Discovered(d) => {
            let set = |g: &dyn Fn(&mut mychess_app::pattern_search::model::DiscoveredCond)| {
                let mut c = d.clone();
                g(&mut c);
                replace(path, Condition::Discovered(c))
            };
            let kinds = [
                (DiscoveredKind::Attack, "pattern-discovery-attack"),
                (DiscoveredKind::Check, "pattern-discovery-check"),
                (
                    DiscoveredKind::DoubleCheck,
                    "pattern-discovery-double-check",
                ),
            ]
            .map(|(v, k)| (v, i18n.tr(k)));
            column![
                field(
                    i18n,
                    "pattern-field-discovery",
                    choices(&kinds, d.discovery, |k| set(&|c| c.discovery = k))
                ),
                field(
                    i18n,
                    "pattern-field-side",
                    who_choices(i18n, d.by, &WHO_AFTER, |w| set(&|c| c.by = w))
                ),
                field(
                    i18n,
                    "pattern-field-moved",
                    role_chips(i18n, &d.roles, &ROLES, |r| set(&|c| c.roles.clone_from(&r)))
                ),
            ]
            .spacing(8)
            .into()
        }
        Condition::DoubleAttack(d) => field(
            i18n,
            "pattern-field-side",
            who_choices(i18n, d.by, &WHO_AFTER, |w| {
                replace(
                    path,
                    Condition::DoubleAttack(mychess_app::pattern_search::model::DoubleAttackCond {
                        by: w,
                    }),
                )
            }),
        ),
        Condition::Sacrifice(s) => {
            let set = |g: &dyn Fn(&mut mychess_app::pattern_search::model::SacrificeCond)| {
                let mut c = s.clone();
                g(&mut c);
                replace(path, Condition::Sacrifice(c))
            };
            let outcomes = [
                (Outcome::Recovered, "pattern-outcome-recovered"),
                (Outcome::Mate, "pattern-outcome-mate"),
                (Outcome::Either, "pattern-outcome-either"),
                (Outcome::Any, "pattern-outcome-any"),
            ]
            .map(|(v, k)| (v, capitalized(&i18n.tr(k))));
            column![
                field(
                    i18n,
                    "pattern-field-side",
                    who_choices(i18n, s.by, &WHO_ALL, |w| set(&|c| c.by = w))
                ),
                field(
                    i18n,
                    "pattern-field-piece",
                    role_chips(i18n, &s.roles, &ROLES[1..], |r| set(&|c| c
                        .roles
                        .clone_from(&r)))
                ),
                field(
                    i18n,
                    "pattern-field-net",
                    num_stepper(i64::from(s.min_net), 1, 20, |n| set(
                        &|c| c.min_net = to_u8(n)
                    ))
                ),
                field(
                    i18n,
                    "pattern-field-within",
                    num_stepper(i64::from(s.within), 1, 8, |n| set(&|c| c.within = to_u8(n)))
                ),
                field(
                    i18n,
                    "pattern-field-outcome",
                    choices(&outcomes, s.outcome, |o| set(&|c| c.outcome = o))
                ),
            ]
            .spacing(8)
            .into()
        }
        Condition::Mate(m) => {
            let set = |g: &dyn Fn(&mut mychess_app::pattern_search::model::MateCond)| {
                let mut c = m.clone();
                g(&mut c);
                replace(path, Condition::Mate(c))
            };
            let modes = [
                (MateMode::Played, "pattern-mate-mode-played"),
                (MateMode::Available, "pattern-mate-mode-available"),
            ]
            .map(|(v, k)| (v, i18n.tr(k)));
            let mut body = column![
                field(
                    i18n,
                    "pattern-field-side",
                    who_choices(i18n, m.by, &WHO_MATE, |w| set(&|c| c.by = w))
                ),
                field(
                    i18n,
                    "pattern-field-mate-min",
                    num_stepper(i64::from(m.moves_min), 1, 5, |n| set(&|c| {
                        c.moves_min = to_u8(n);
                        c.moves_max = c.moves_max.max(c.moves_min);
                    }))
                ),
                field(
                    i18n,
                    "pattern-field-mate-max",
                    num_stepper(i64::from(m.moves_max), 1, 5, |n| set(&|c| {
                        c.moves_max = to_u8(n);
                        c.moves_min = c.moves_min.min(c.moves_max);
                    }))
                ),
                field(
                    i18n,
                    "pattern-param-mate-mode",
                    choices(&modes, m.mode, |v| set(&|c| c.mode = v))
                ),
            ]
            .spacing(8);
            if m.mode == MateMode::Available {
                let (yes, no) = (
                    set(&|c| c.forcing_only = true),
                    set(&|c| c.forcing_only = false),
                );
                body = body.push(toggle(
                    i18n.tr("pattern-mate-forcing-only"),
                    m.forcing_only,
                    yes,
                    no,
                ));
                body = body.push(field(
                    i18n,
                    "pattern-field-nodes",
                    number_input(
                        ui,
                        NumField::CardNodes(path.clone()),
                        i64::from(m.max_nodes),
                        80.0,
                    ),
                ));
            }
            body.into()
        }
    }
}

fn group_editor<'a>(node: &NodeView, i18n: &I18n) -> Element<'a, Message> {
    let current = group_kind(&node.condition);
    let options: Vec<(GroupKind, String)> = GroupKind::ALL
        .iter()
        .map(|k| (*k, i18n.tr(k.title_key())))
        .collect();
    let mut body = column![field(
        i18n,
        "pattern-change-group",
        choices(&options, current.unwrap_or(GroupKind::All), |k| {
            edit(PatternEdit::SetGroupKind {
                path: node.path.clone(),
                kind: k,
            })
        })
    )]
    .spacing(8);
    if node.children == 1 {
        body = body.push(
            button(text(i18n.tr("pattern-unwrap")).size(13))
                .padding([3, 8])
                .style(button::secondary)
                .on_press(edit(PatternEdit::Unwrap(node.path.clone()))),
        );
    }
    body.into()
}

fn piece_editor<'a>(
    ui: &PatternSearchUi,
    path: &NodePath,
    p: &PieceCond,
    i18n: &I18n,
) -> Element<'a, Message> {
    let set = |g: &dyn Fn(&mut PieceCond)| {
        let mut c = p.clone();
        g(&mut c);
        replace(path, Condition::Piece(c))
    };
    let _ = ui;
    let limited = p.max.is_some();
    let mut count = row![
        text(i18n.tr("pattern-field-min")).size(13),
        num_stepper(i64::from(p.min), 0, 64, |n| set(&|c| c.min = to_u8(n))),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    count = count.push(toggle(
        i18n.tr("pattern-field-max"),
        limited,
        set(&|c| c.max = Some(c.min.max(1))),
        set(&|c| c.max = None),
    ));
    if let Some(max) = p.max {
        count = count.push(num_stepper(i64::from(max), 0, 64, |n| {
            set(&|c| c.max = Some(to_u8(n)))
        }));
    }
    column![
        field(
            i18n,
            "pattern-field-side",
            who_choices(i18n, p.side, &WHO_ALL, |w| set(&|c| c.side = w))
        ),
        field(
            i18n,
            "pattern-field-piece",
            role_chips(i18n, &p.roles, &ROLES, |r| set(&|c| c.roles.clone_from(&r)))
        ),
        field(i18n, "pattern-field-count", count.into()),
        region_editor(
            i18n,
            i18n.tr("pattern-field-where"),
            path,
            RegionSlot::Main,
            &p.region
        ),
    ]
    .spacing(8)
    .into()
}

fn selector_editor<'a>(
    i18n: &I18n,
    title_key: &str,
    path: &NodePath,
    slot: RegionSlot,
    roles: &[RoleName],
    region: &Region,
    make_roles: impl Fn(Vec<RoleName>) -> Message,
) -> Element<'a, Message> {
    column![
        field(i18n, title_key, role_chips(i18n, roles, &ROLES, make_roles)),
        region_editor(i18n, i18n.tr(title_key), path, slot, region),
    ]
    .spacing(6)
    .into()
}

fn relation_editor<'a>(path: &NodePath, r: &RelationCond, i18n: &I18n) -> Element<'a, Message> {
    let set = |g: &dyn Fn(&mut RelationCond)| {
        let mut c = r.clone();
        g(&mut c);
        replace(path, Condition::Relation(c))
    };
    let mut body = column![
        field(
            i18n,
            "pattern-field-side",
            who_choices(i18n, r.actor.side, &WHO_ALL, |w| set(&|c| c.actor.side = w))
        ),
        selector_editor(
            i18n,
            "pattern-field-actor",
            path,
            RegionSlot::Actor,
            &r.actor.roles,
            &r.actor.region,
            |v| set(&|c| c.actor.roles.clone_from(&v)),
        ),
        selector_editor(
            i18n,
            "pattern-field-target",
            path,
            RegionSlot::Target,
            &r.target.roles,
            &r.target.region,
            |v| set(&|c| c.target.roles.clone_from(&v)),
        ),
    ]
    .spacing(8);
    if let Some(behind) = &r.behind {
        body = body.push(selector_editor(
            i18n,
            "pattern-field-behind",
            path,
            RegionSlot::Behind,
            &behind.roles,
            &behind.region,
            |v| {
                set(&|c| {
                    if let Some(b) = c.behind.as_mut() {
                        b.roles.clone_from(&v);
                    }
                })
            },
        ));
    }
    if r.kind == RelKind::Attacks {
        let gains = [
            (Gain::Any, "pattern-gain-any"),
            (Gain::Undefended, "pattern-gain-undefended"),
            (Gain::WinsMaterial, "pattern-gain-wins-material"),
        ]
        .map(|(v, k)| (v, capitalized(&i18n.tr(k))));
        body = body.push(field(
            i18n,
            "pattern-field-gain",
            choices(&gains, r.gain, |g| set(&|c| c.gain = g)),
        ));
    }
    if r.kind == RelKind::Forks {
        body = body.push(field(
            i18n,
            "pattern-field-targets",
            num_stepper(i64::from(r.min_targets), 2, 8, |n| {
                set(&|c| c.min_targets = to_u8(n))
            }),
        ));
    }
    let whens = [
        (When::Any, "pattern-when-any"),
        (When::ActorJustMoved, "pattern-when-actor-just-moved"),
        (When::ActorToMove, "pattern-when-actor-to-move"),
    ]
    .map(|(v, k)| (v, capitalized(&i18n.tr(k))));
    body = body.push(field(
        i18n,
        "pattern-field-when",
        choices(&whens, r.when, |w| set(&|c| c.when = w)),
    ));
    body = body.push(toggle(
        i18n.tr("pattern-field-new"),
        r.new_this_move,
        set(&|c| c.new_this_move = true),
        set(&|c| c.new_this_move = false),
    ));
    body.into()
}

fn move_editor<'a>(path: &NodePath, m: &MoveCond, i18n: &I18n) -> Element<'a, Message> {
    let set = |g: &dyn Fn(&mut MoveCond)| {
        let mut c = m.clone();
        g(&mut c);
        replace(path, Condition::Move(c))
    };
    let allowed: &[Who] = if m.which == mychess_app::pattern_search::model::WhichMove::Previous {
        &WHO_AFTER
    } else {
        &WHO_ALL
    };
    let castles = [
        (None, "pattern-castle-none"),
        (Some(CastleKind::Short), "pattern-castle-short"),
        (Some(CastleKind::Long), "pattern-castle-long"),
        (Some(CastleKind::Any), "pattern-castle-any"),
    ]
    .map(|(v, k)| (v, capitalized(&i18n.tr(k))));
    column![
        field(
            i18n,
            "pattern-field-side",
            who_choices(i18n, m.by, allowed, |w| set(&|c| c.by = w))
        ),
        field(
            i18n,
            "pattern-field-piece",
            role_chips(i18n, &m.roles, &ROLES, |r| set(&|c| c.roles.clone_from(&r)))
        ),
        region_editor(
            i18n,
            i18n.tr("pattern-region-from"),
            path,
            RegionSlot::From,
            &m.from
        ),
        region_editor(
            i18n,
            i18n.tr("pattern-region-to"),
            path,
            RegionSlot::To,
            &m.to
        ),
        field(
            i18n,
            "pattern-field-capture",
            flag_choices(i18n, m.capture, |f| set(&|c| c.capture = f))
        ),
        field(
            i18n,
            "pattern-field-check",
            flag_choices(i18n, m.check, |f| set(&|c| c.check = f))
        ),
        field(
            i18n,
            "pattern-field-promotion",
            role_chips(i18n, &m.promotion, &PROMOTIONS, |r| set(&|c| c
                .promotion
                .clone_from(&r)))
        ),
        field(
            i18n,
            "pattern-field-castling",
            choices(&castles, m.castling, |k| set(&|c| c.castling = k))
        ),
    ]
    .spacing(8)
    .into()
}

// ---- material ----

fn range_cell<'a>(
    current: Option<MinMax<u8>>,
    high: u8,
    make: impl Fn(Option<MinMax<u8>>) -> Message,
) -> Element<'a, Message> {
    match current {
        None => button(text("\u{2013}").size(12))
            .padding([2, 8])
            .style(button::secondary)
            .on_press(make(Some(MinMax { min: 0, max: high })))
            .into(),
        Some(r) => row![
            num_stepper(i64::from(r.min), 0, i64::from(r.max), |n| make(Some(
                MinMax {
                    min: to_u8(n),
                    max: r.max
                }
            ))),
            text("\u{2013}").size(12),
            num_stepper(i64::from(r.max), i64::from(r.min), i64::from(high), |n| {
                make(Some(MinMax {
                    min: r.min,
                    max: to_u8(n),
                }))
            }),
            button(text("\u{d7}").size(12))
                .padding([2, 6])
                .style(button::secondary)
                .on_press(make(None)),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into(),
    }
}

fn material_editor<'a>(path: &NodePath, m: &MaterialCond, i18n: &I18n) -> Element<'a, Message> {
    type Getter = fn(&mut Counts) -> &mut Option<MinMax<u8>>;
    let rows: [(&str, Getter, u8); 6] = [
        ("pattern-role-queen", |c| &mut c.queens, 9),
        ("pattern-role-rook", |c| &mut c.rooks, 10),
        ("pattern-role-bishop", |c| &mut c.bishops, 10),
        ("pattern-role-knight", |c| &mut c.knights, 10),
        ("pattern-role-minor", |c| &mut c.minors, 10),
        ("pattern-role-pawn", |c| &mut c.pawns, 8),
    ];
    let mut counts = column![
        row![
            text("").width(Length::Fixed(90.0)),
            text(i18n.tr("pattern-who-white"))
                .size(13)
                .width(Length::Fixed(210.0)),
            text(i18n.tr("pattern-who-black")).size(13),
        ]
        .spacing(8)
    ]
    .spacing(4);
    for (label, get, high) in rows {
        let cell = |white: bool| {
            let mut probe = m.clone();
            let counts = if white {
                &mut probe.white
            } else {
                &mut probe.black
            };
            let current = *get(counts);
            range_cell(current, high, |value| {
                let mut c = m.clone();
                let counts = if white { &mut c.white } else { &mut c.black };
                *get(counts) = value;
                replace(path, Condition::Material(c))
            })
        };
        counts = counts.push(
            row![
                text(capitalized(&i18n.tr(label)))
                    .size(13)
                    .width(Length::Fixed(90.0)),
                iced::widget::container(cell(true)).width(Length::Fixed(210.0)),
                cell(false)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    let set = |g: &dyn Fn(&mut MaterialCond)| {
        let mut c = m.clone();
        g(&mut c);
        replace(path, Condition::Material(c))
    };
    let diff: Element<'a, Message> = match m.diff {
        None => button(text("\u{2013}").size(12))
            .padding([2, 8])
            .style(button::secondary)
            .on_press(set(&|c| c.diff = Some(MinMax { min: -3, max: 3 })))
            .into(),
        Some(d) => row![
            num_stepper(i64::from(d.min), -39, i64::from(d.max), |n| set(&|c| {
                c.diff = Some(MinMax {
                    min: i32::try_from(n).unwrap_or(0),
                    max: d.max,
                });
            })),
            text("\u{2013}").size(12),
            num_stepper(i64::from(d.max), i64::from(d.min), 39, |n| set(&|c| {
                c.diff = Some(MinMax {
                    min: d.min,
                    max: i32::try_from(n).unwrap_or(0),
                });
            })),
            button(text("\u{d7}").size(12))
                .padding([2, 6])
                .style(button::secondary)
                .on_press(set(&|c| c.diff = None)),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into(),
    };
    let bishops = [
        (BishopColors::Either, "pattern-bishops-either"),
        (BishopColors::Same, "pattern-bishops-same"),
        (BishopColors::Opposite, "pattern-bishops-opposite"),
    ]
    .map(|(v, k)| (v, capitalized(&i18n.tr(k))));
    column![
        counts,
        field(i18n, "pattern-field-diff", diff),
        field(
            i18n,
            "pattern-field-bishops",
            choices(&bishops, m.bishops, |b| set(&|c| c.bishops = b))
        ),
    ]
    .spacing(8)
    .into()
}
