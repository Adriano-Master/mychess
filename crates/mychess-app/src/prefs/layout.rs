//! The arrangement of the panels, as data without any GUI toolkit (spec
//! M2-3 D135). The front-end converts it from and to its own pane grid.

use serde::{Serialize, Serializer};
use toml::{Table, Value};

mod ops;

/// Deepest nesting of splits accepted.
pub const MAX_LAYOUT_DEPTH: usize = 8;
/// Smallest split ratio accepted.
const MIN_RATIO: f32 = 0.1;
const MAX_RATIO: f32 = 0.9;
/// Ratios of the layout that was the default before M2-5, and how far a
/// saved ratio may be from them and still count as unchanged.
const LEGACY_OUTER_RATIO: f32 = 0.6;
const LEGACY_INNER_RATIO: f32 = 0.78;
const LEGACY_TOLERANCE: f32 = 0.005;
/// Version of the saved layout format (spec M2-6 D1010).
const LAYOUT_FORMAT_VERSION: u32 = 2;

/// A panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PaneKind {
    /// The board. Cannot be closed.
    Board,
    /// The game information panel.
    Info,
    /// The notation (move list) panel.
    Notation,
    /// The comment editor of the current move (spec M2-6 D1001).
    Comment,
    /// The game list of the databases (spec M3-4, slice 4b).
    GameList,
    /// The analysis panel: one section per running engine (spec M4-4).
    Analysis,
    /// The score and time graph of the game (spec M4-7).
    Graph,
    /// The opening tree (spec M5-2).
    Tree,
    /// The tournament between engines (spec M8-2b, D1360).
    Tournament,
    /// The game report (spec M4-10b, D1421).
    Report,
    /// The book window: two Polyglot books side by side (spec M5-6,
    /// D1650).
    Book,
    /// The tablebase panel: the result of every move (spec M8-8, D2571).
    Tablebase,
    /// The crosstable of the tournament of the game (spec M6-4).
    Crosstable,
    /// The statistics and graphs of the filter (spec M6-4).
    FilterStats,
    /// The rating graph of the players (spec M6-4).
    RatingGraph,
    /// The piece tracker (spec M6-4).
    PieceTracker,
    /// The EPD files: one tab per open file (spec M7-5c).
    Epd,
}

impl PaneKind {
    fn name(self) -> &'static str {
        match self {
            PaneKind::Board => "board",
            PaneKind::Info => "info",
            PaneKind::Notation => "notation",
            PaneKind::Comment => "comment",
            PaneKind::GameList => "games",
            PaneKind::Analysis => "analysis",
            PaneKind::Graph => "graph",
            PaneKind::Tree => "tree",
            PaneKind::Tournament => "tournament",
            PaneKind::Report => "report",
            PaneKind::Book => "book",
            PaneKind::Tablebase => "tablebase",
            PaneKind::Crosstable => "crosstable",
            PaneKind::FilterStats => "filter-stats",
            PaneKind::RatingGraph => "rating-graph",
            PaneKind::PieceTracker => "piece-tracker",
            PaneKind::Epd => "epd",
        }
    }

    fn from_name(name: &str) -> Option<PaneKind> {
        match name {
            "board" => Some(PaneKind::Board),
            "info" => Some(PaneKind::Info),
            "notation" => Some(PaneKind::Notation),
            "comment" => Some(PaneKind::Comment),
            "games" => Some(PaneKind::GameList),
            "analysis" => Some(PaneKind::Analysis),
            "graph" => Some(PaneKind::Graph),
            "tree" => Some(PaneKind::Tree),
            "tournament" => Some(PaneKind::Tournament),
            "report" => Some(PaneKind::Report),
            "book" => Some(PaneKind::Book),
            "tablebase" => Some(PaneKind::Tablebase),
            "crosstable" => Some(PaneKind::Crosstable),
            "filter-stats" => Some(PaneKind::FilterStats),
            "rating-graph" => Some(PaneKind::RatingGraph),
            "piece-tracker" => Some(PaneKind::PieceTracker),
            "epd" => Some(PaneKind::Epd),
            _ => None,
        }
    }
}

/// Direction of a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SplitAxis {
    /// Side by side: `a` on the left, `b` on the right.
    Horizontal,
    /// Stacked: `a` on top, `b` below.
    Vertical,
}

/// A tree of panels: a panel, or a split in two.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutNode {
    /// A leaf: a group of panels shown as tabs, one active (spec M2-6
    /// D1000). A lone panel is a group of one tab.
    Group {
        /// The panels of the group, in tab order.
        tabs: Vec<PaneKind>,
        /// Index in `tabs` of the selected tab.
        active: usize,
    },
    /// Two subtrees.
    Split {
        /// Direction of the split.
        axis: SplitAxis,
        /// Share of the space given to `a`, in `0.1..=0.9`.
        ratio: f32,
        /// First part (left or top).
        a: Box<LayoutNode>,
        /// Second part (right or bottom).
        b: Box<LayoutNode>,
    },
}

impl LayoutNode {
    /// A group of one panel.
    pub fn pane(kind: PaneKind) -> LayoutNode {
        LayoutNode::Group {
            tabs: vec![kind],
            active: 0,
        }
    }

    /// A group of panels with `active` selected. Not checked: see
    /// [`is_valid`](LayoutNode::is_valid).
    pub fn group(tabs: Vec<PaneKind>, active: usize) -> LayoutNode {
        LayoutNode::Group { tabs, active }
    }

    /// The default arrangement: `[ Board | Group{Notation, Info, Comment} ]`,
    /// ratio 0.60, the notation tab active (spec M2-6 D1001).
    pub fn default_layout() -> LayoutNode {
        LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio: LEGACY_OUTER_RATIO,
            a: Box::new(LayoutNode::pane(PaneKind::Board)),
            b: Box::new(LayoutNode::group(
                vec![PaneKind::Notation, PaneKind::Info, PaneKind::Comment],
                0,
            )),
        }
    }

    /// The default arrangement before M2-5: `[ [Board / Info] | Notation ]`,
    /// ratios 0.60 and 0.78 (M2-3 D136). Still a valid layout.
    pub fn legacy_default_layout() -> LayoutNode {
        LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.6,
            a: Box::new(LayoutNode::Split {
                axis: SplitAxis::Vertical,
                ratio: 0.78,
                a: Box::new(LayoutNode::pane(PaneKind::Board)),
                b: Box::new(LayoutNode::pane(PaneKind::Info)),
            }),
            b: Box::new(LayoutNode::pane(PaneKind::Notation)),
        }
    }

    /// Whether this is exactly the default arrangement of before M2-5
    /// (`Split(Horizontal, ~0.60, Split(Vertical, ~0.78, Board, Info),
    /// Notation)`, ratios within 0.005): the only layout replaced by the new
    /// default when the preferences are read (D213). Any other layout, even
    /// one with the same panels in another order, is false.
    pub fn is_legacy_default(&self) -> bool {
        let near = |ratio: f32, wanted: f32| (ratio - wanted).abs() <= LEGACY_TOLERANCE;
        let LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio,
            a,
            b,
        } = self
        else {
            return false;
        };
        let LayoutNode::Split {
            axis: SplitAxis::Vertical,
            ratio: inner,
            a: top,
            b: bottom,
        } = &**a
        else {
            return false;
        };
        near(*ratio, LEGACY_OUTER_RATIO)
            && near(*inner, LEGACY_INNER_RATIO)
            && **top == LayoutNode::pane(PaneKind::Board)
            && **bottom == LayoutNode::pane(PaneKind::Info)
            && **b == LayoutNode::pane(PaneKind::Notation)
    }

    /// The panels of the tree, left to right (every tab of every group).
    pub fn panes(&self) -> Vec<PaneKind> {
        match self {
            LayoutNode::Group { tabs, .. } => tabs.clone(),
            LayoutNode::Split { a, b, .. } => {
                let mut panes = a.panes();
                panes.extend(b.panes());
                panes
            }
        }
    }

    /// Whether the tree can be shown (spec M2-6 D1000): ratios in
    /// `0.1..=0.9`, depth up to 8, no panel twice, the board present and
    /// alone in its group, every group non-empty with `active < len`.
    pub fn is_valid(&self) -> bool {
        let panes = self.panes();
        let board = panes.iter().filter(|k| **k == PaneKind::Board).count() == 1;
        let unique = panes
            .iter()
            .enumerate()
            .all(|(i, kind)| !panes[..i].contains(kind));
        board && unique && self.shape_ok(0)
    }

    fn shape_ok(&self, depth: usize) -> bool {
        match self {
            LayoutNode::Group { tabs, active } => {
                !tabs.is_empty()
                    && *active < tabs.len()
                    && (tabs.len() == 1 || !tabs.contains(&PaneKind::Board))
            }
            LayoutNode::Split { ratio, a, b, .. } => {
                depth < MAX_LAYOUT_DEPTH
                    && (MIN_RATIO..=MAX_RATIO).contains(ratio)
                    && a.shape_ok(depth + 1)
                    && b.shape_ok(depth + 1)
            }
        }
    }

    /// The saved form (format v2, D1010), with the `layout_version` marker
    /// at the root.
    pub(super) fn to_value(&self) -> Value {
        let mut value = self.node_value();
        if let Value::Table(table) = &mut value {
            table.insert(
                "layout_version".to_owned(),
                Value::Integer(i64::from(LAYOUT_FORMAT_VERSION)),
            );
        }
        value
    }

    fn node_value(&self) -> Value {
        let mut table = Table::new();
        match self {
            LayoutNode::Group { tabs, active } => {
                if let [only] = tabs.as_slice() {
                    table.insert("pane".to_owned(), Value::String(only.name().to_owned()));
                } else {
                    let names = tabs
                        .iter()
                        .map(|k| Value::String(k.name().to_owned()))
                        .collect();
                    table.insert("tabs".to_owned(), Value::Array(names));
                    let active = i64::try_from(*active).unwrap_or(0);
                    table.insert("active".to_owned(), Value::Integer(active));
                }
            }
            LayoutNode::Split { axis, ratio, a, b } => {
                let axis = match axis {
                    SplitAxis::Horizontal => "horizontal",
                    SplitAxis::Vertical => "vertical",
                };
                table.insert("axis".to_owned(), Value::String(axis.to_owned()));
                // `f32` -> shortest text -> `f64`, so 0.6 is written as 0.6.
                let ratio = ratio.to_string().parse::<f64>().unwrap_or(0.5);
                table.insert("ratio".to_owned(), Value::Float(ratio));
                table.insert("a".to_owned(), a.node_value());
                table.insert("b".to_owned(), b.node_value());
            }
        }
        Value::Table(table)
    }

    /// Reads a tree; `None` when the value is not a tree. Does not check
    /// [`is_valid`](LayoutNode::is_valid) (but stops at a safe depth).
    ///
    /// Without the `layout_version = 2` marker the value is a v1 layout and
    /// is migrated (D1010). Unknown tabs and groups left empty are dropped
    /// (D1012); a split that loses one side becomes the other side.
    pub(super) fn from_value(value: &Value) -> Option<LayoutNode> {
        let is_v2 = matches!(
            value.as_table()?.get("layout_version"),
            Some(Value::Integer(n)) if *n >= i64::from(LAYOUT_FORMAT_VERSION)
        );
        let Parsed::Node(node) = Self::from_value_at(value, 0)? else {
            return None;
        };
        if is_v2 || node.is_legacy_default() {
            Some(node)
        } else {
            Some(node.migrated_from_v1())
        }
    }

    /// v1 -> v2: the lone `Notation` panel used to hold the comment and,
    /// when the `Info` panel was closed, the information as tabs (D1010).
    fn migrated_from_v1(self) -> LayoutNode {
        let panes = self.panes();
        if panes.contains(&PaneKind::Comment) {
            return self;
        }
        let tabs = if panes.contains(&PaneKind::Info) {
            vec![PaneKind::Notation, PaneKind::Comment]
        } else {
            vec![PaneKind::Notation, PaneKind::Info, PaneKind::Comment]
        };
        self.map_leaves(&|leaf| match leaf {
            LayoutNode::Group { tabs: t, .. } if t.as_slice() == [PaneKind::Notation] => {
                LayoutNode::group(tabs.clone(), 0)
            }
            other => other.clone(),
        })
    }

    pub(super) fn map_leaves(&self, f: &impl Fn(&LayoutNode) -> LayoutNode) -> LayoutNode {
        match self {
            LayoutNode::Group { .. } => f(self),
            LayoutNode::Split { axis, ratio, a, b } => LayoutNode::Split {
                axis: *axis,
                ratio: *ratio,
                a: Box::new(a.map_leaves(f)),
                b: Box::new(b.map_leaves(f)),
            },
        }
    }

    /// `None`: not a tree.
    fn from_value_at(value: &Value, depth: usize) -> Option<Parsed> {
        let table = value.as_table()?;
        if let Some(name) = table.get("pane") {
            return Some(Parsed::of(
                PaneKind::from_name(name.as_str()?).map(LayoutNode::pane),
            ));
        }
        if let Some(tabs) = table.get("tabs") {
            let names = tabs.as_array()?;
            let active = match table.get("active") {
                Some(Value::Integer(n)) => usize::try_from(*n).ok(),
                _ => None,
            };
            let kinds: Vec<Option<PaneKind>> = names
                .iter()
                .map(|v| v.as_str().and_then(PaneKind::from_name))
                .collect();
            // Keep the selected tab selected when unknown ones are dropped;
            // an index out of range selects the first.
            let selected = active.and_then(|i| kinds.get(i).copied().flatten());
            let known: Vec<PaneKind> = kinds.into_iter().flatten().collect();
            let active = selected
                .and_then(|k| known.iter().position(|x| *x == k))
                .unwrap_or(0);
            return Some(Parsed::of(
                (!known.is_empty()).then(|| LayoutNode::group(known, active)),
            ));
        }
        if depth >= MAX_LAYOUT_DEPTH {
            return None;
        }
        let axis = match table.get("axis")?.as_str()? {
            "horizontal" => SplitAxis::Horizontal,
            "vertical" => SplitAxis::Vertical,
            _ => return None,
        };
        let ratio = match table.get("ratio")? {
            Value::Float(f) => *f,
            #[allow(clippy::cast_precision_loss)]
            Value::Integer(i) => *i as f64,
            _ => return None,
        };
        #[allow(clippy::cast_possible_truncation)]
        let ratio = ratio as f32;
        let a = Self::from_value_at(table.get("a")?, depth + 1)?;
        let b = Self::from_value_at(table.get("b")?, depth + 1)?;
        Some(match (a, b) {
            (Parsed::Node(a), Parsed::Node(b)) => Parsed::Node(LayoutNode::Split {
                axis,
                ratio,
                a: Box::new(a),
                b: Box::new(b),
            }),
            (Parsed::Empty, only) | (only, Parsed::Empty) => only,
        })
    }
}

/// A part of a saved tree that was understood: a node, or nothing left
/// after dropping unknown tabs.
enum Parsed {
    Node(LayoutNode),
    Empty,
}

impl Parsed {
    fn of(node: Option<LayoutNode>) -> Parsed {
        node.map_or(Parsed::Empty, Parsed::Node)
    }
}

impl Serialize for LayoutNode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_is_valid_and_round_trips() {
        let layout = LayoutNode::default_layout();
        assert!(layout.is_valid());
        assert_eq!(LayoutNode::from_value(&layout.to_value()), Some(layout));
    }

    fn split(axis: SplitAxis, ratio: f32, a: LayoutNode, b: LayoutNode) -> LayoutNode {
        LayoutNode::Split {
            axis,
            ratio,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    #[test]
    fn new_default_is_board_and_a_notation_group() {
        let layout = LayoutNode::default_layout();
        assert_eq!(
            layout.panes(),
            [
                PaneKind::Board,
                PaneKind::Notation,
                PaneKind::Info,
                PaneKind::Comment
            ]
        );
        assert!(!layout.is_legacy_default());
    }

    #[test]
    fn only_the_exact_old_default_is_legacy() {
        let old = LayoutNode::legacy_default_layout();
        assert!(old.is_valid());
        assert!(old.is_legacy_default());
        let board = || LayoutNode::pane(PaneKind::Board);
        let info = || LayoutNode::pane(PaneKind::Info);
        let notation = || LayoutNode::pane(PaneKind::Notation);
        let inner = |r| split(SplitAxis::Vertical, r, board(), info());
        // Within the tolerance: still the old default.
        assert!(split(SplitAxis::Horizontal, 0.603, inner(0.783), notation()).is_legacy_default());
        // Dragged beyond it: customised.
        assert!(!split(SplitAxis::Horizontal, 0.65, inner(0.78), notation()).is_legacy_default());
        assert!(!split(SplitAxis::Horizontal, 0.6, inner(0.7), notation()).is_legacy_default());
        // Same ratios, panels swapped.
        assert!(!split(SplitAxis::Horizontal, 0.6, notation(), inner(0.78)).is_legacy_default());
        let swapped = split(SplitAxis::Vertical, 0.78, info(), board());
        assert!(!split(SplitAxis::Horizontal, 0.6, swapped, notation()).is_legacy_default());
        // Another axis.
        assert!(!split(SplitAxis::Vertical, 0.6, inner(0.78), notation()).is_legacy_default());
        assert!(!LayoutNode::pane(PaneKind::Board).is_legacy_default());
    }

    #[test]
    fn invalid_trees_are_detected() {
        let no_board = LayoutNode::pane(PaneKind::Info);
        assert!(!no_board.is_valid());
        let twice = LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            a: Box::new(LayoutNode::pane(PaneKind::Board)),
            b: Box::new(LayoutNode::pane(PaneKind::Board)),
        };
        assert!(!twice.is_valid());
        let bad_ratio = LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.95,
            a: Box::new(LayoutNode::pane(PaneKind::Board)),
            b: Box::new(LayoutNode::pane(PaneKind::Info)),
        };
        assert!(!bad_ratio.is_valid());
        let mut deep = LayoutNode::pane(PaneKind::Board);
        for _ in 0..=MAX_LAYOUT_DEPTH {
            deep = LayoutNode::Split {
                axis: SplitAxis::Vertical,
                ratio: 0.5,
                a: Box::new(deep),
                b: Box::new(LayoutNode::pane(PaneKind::Info)),
            };
        }
        assert!(!deep.is_valid());
    }

    #[test]
    fn the_game_list_pane_round_trips_and_is_optional() {
        let layout = LayoutNode::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.6,
            a: Box::new(LayoutNode::default_layout()),
            b: Box::new(LayoutNode::pane(PaneKind::GameList)),
        };
        assert!(layout.is_valid());
        assert!(!layout.is_legacy_default());
        assert_eq!(layout.panes().last(), Some(&PaneKind::GameList));
        assert_eq!(
            LayoutNode::from_value(&layout.to_value()),
            Some(layout.clone())
        );
        assert!(LayoutNode::default_layout().is_valid());
    }

    #[test]
    fn hostile_values_do_not_panic() {
        assert_eq!(LayoutNode::from_value(&Value::Integer(3)), None);
        let mut value = Value::Table(Table::new());
        for _ in 0..1000 {
            let mut t = Table::new();
            t.insert("axis".to_owned(), Value::String("vertical".to_owned()));
            t.insert("ratio".to_owned(), Value::Float(0.5));
            t.insert("a".to_owned(), value);
            t.insert("b".to_owned(), Value::Table(Table::new()));
            value = Value::Table(t);
        }
        assert_eq!(LayoutNode::from_value(&value), None);
    }

    fn parse(text: &str) -> Option<LayoutNode> {
        let value: Value = text.parse::<Table>().ok().map(Value::Table)?;
        LayoutNode::from_value(&value)
    }

    #[test]
    fn group_invariants_are_enforced() {
        let board = || LayoutNode::pane(PaneKind::Board);
        let with = |g| split(SplitAxis::Horizontal, 0.5, board(), g);
        let group = |tabs, active| LayoutNode::group(tabs, active);
        assert!(with(group(vec![PaneKind::Info, PaneKind::Comment], 1)).is_valid());
        assert!(!with(group(vec![], 0)).is_valid());
        assert!(!with(group(vec![PaneKind::Info], 1)).is_valid());
        assert!(!with(group(vec![PaneKind::Info, PaneKind::Info], 0)).is_valid());
        assert!(!with(group(vec![PaneKind::Info, PaneKind::Board], 0)).is_valid());
    }

    #[test]
    fn v2_round_trips_and_lone_panes_stay_short() {
        let layout = LayoutNode::default_layout();
        let text = toml::to_string(&layout).unwrap_or_default();
        assert!(text.contains("layout_version = 2"), "{text}");
        assert!(text.contains("pane = \"board\""), "{text}");
        assert!(
            text.contains("tabs = [\"notation\", \"info\", \"comment\"]"),
            "{text}"
        );
        assert_eq!(parse(&text), Some(layout));
    }

    #[test]
    fn v2_reading_is_tolerant() {
        let text = "layout_version = 2\naxis = \"horizontal\"\nratio = 0.6\n\
            [a]\npane = \"board\"\n[b]\ntabs = [\"info\", \"future\", \"comment\"]\nactive = 2\n";
        let layout = parse(text).expect("layout");
        assert_eq!(
            layout.group_of(PaneKind::Info),
            Some((&[PaneKind::Info, PaneKind::Comment][..], 1))
        );
        let text = text.replace("active = 2", "active = 9");
        let layout = parse(&text).expect("layout");
        assert_eq!(layout.group_of(PaneKind::Info).map(|g| g.1), Some(0));
        // A group with only unknown tabs disappears.
        let text = "layout_version = 2\naxis = \"horizontal\"\nratio = 0.6\n\
            [a]\npane = \"board\"\n[b]\ntabs = [\"x\"]\n";
        assert_eq!(parse(text), Some(LayoutNode::pane(PaneKind::Board)));
        // Garbage is not a layout.
        assert_eq!(parse("layout_version = 2\ntabs = 3\n"), None);
    }

    #[test]
    fn v1_notation_is_migrated() {
        let v1 =
            "axis = \"horizontal\"\nratio = 0.6\n[a]\npane = \"board\"\n[b]\npane = \"notation\"\n";
        assert_eq!(parse(v1), Some(LayoutNode::default_layout()));
        // `Info` elsewhere in the layout: only the comment joins.
        let with_info = "axis = \"horizontal\"\nratio = 0.7\n[a]\naxis = \"vertical\"\nratio = 0.5\n\
            [a.a]\npane = \"board\"\n[a.b]\npane = \"info\"\n[b]\npane = \"notation\"\n";
        let layout = parse(with_info).expect("layout");
        assert_eq!(
            layout.group_of(PaneKind::Notation),
            Some((&[PaneKind::Notation, PaneKind::Comment][..], 0))
        );
        assert!(layout.is_valid());
        // No `Notation`: nothing to migrate.
        let none =
            "axis = \"horizontal\"\nratio = 0.6\n[a]\npane = \"board\"\n[b]\npane = \"info\"\n";
        assert_eq!(
            parse(none).map(|l| l.panes()),
            Some(vec![PaneKind::Board, PaneKind::Info])
        );
    }
}
