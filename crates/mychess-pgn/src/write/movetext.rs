//! Movetext output: a port of Scid's `Game::WriteMoveList`
//! (`src/game.cpp:3088-3552`) with the deliberate changes of spec D32-D35.
//!
//! Scid recurses once per variation level. Here the tree is walked with an
//! explicit stack of [`Frame`]s, one per line being written, so neither a
//! main line of tens of thousands of moves nor variations nested thousands of
//! levels deep can overflow the call stack.

use mychess_core::{Color, Game, Nag, NodeId, strip_mark_codes};

use super::WriteError;
use super::layout::Layout;
use super::options::{BraceHandling, NagOutput, NullMoves, WriteOptions};

/// Width of the white cell in column style.
const COLUMN_CELL: usize = 12;
/// Indentation step of comments and variations.
const INDENT_STEP: usize = 4;

/// One line (main line or variation) being written.
#[derive(Debug)]
struct Frame {
    /// Node whose position precedes `cur` (the parent of `cur`).
    prev: NodeId,
    /// Next node of this line.
    cur: Option<NodeId>,
    /// `cur` has not been written yet and is the first node of a variation.
    head: bool,
    /// Alternatives still to write after the node just written: the
    /// children of this node's parent from this index on.
    vars: Option<(NodeId, usize)>,
    /// The next move needs a number even if it is Black's.
    print_num: bool,
    /// Inside a `{-- ...}` comment made from a null move.
    in_region: bool,
    /// This line opened that comment and has to close it.
    closes_region: bool,
    depth: u32,
    /// The line was started on a new, deeper indentation level.
    indented: bool,
}

/// A column-style row waiting for Black's move.
#[derive(Debug)]
struct Row {
    text: String,
    cell_chars: usize,
}

struct Walker<'a> {
    game: &'a Game,
    opts: &'a WriteOptions,
    layout: Layout,
    row: Option<Row>,
    scratch: String,
}

/// Writes the movetext of `game` (comments, variations, NAGs and the final
/// result), ending with a line terminator.
pub(crate) fn write_movetext(
    game: &Game,
    opts: &WriteOptions,
    newline: &'static str,
) -> Result<String, WriteError> {
    let mut walker = Walker {
        game,
        opts,
        layout: Layout::new(opts.wrap_width(), newline),
        row: None,
        scratch: String::new(),
    };
    walker.run()?;
    Ok(walker.layout.finish())
}

/// Whether `symbol` can be written and read back safely as a bare token.
///
/// Letters would be taken for moves (`N`, `D`, `o-o`...), `--` is the null
/// move, and `(`, `[` or `%` would start variations, game headers or escape
/// lines. What is left are the typographic glyphs.
fn is_safe_symbol(symbol: &str) -> bool {
    if symbol == "ZZ" || symbol == "zz" {
        return true;
    }
    symbol != "--"
        && symbol.chars().all(|c| {
            matches!(
                c,
                '!' | '?' | '+' | '-' | '=' | '/' | '~' | '<' | '>' | '&' | '|' | '_' | '\\' | '^'
            )
        })
}

/// The text of one NAG in the chosen style.
fn nag_token(nag: Nag, style: NagOutput) -> String {
    if style == NagOutput::Symbolic
        && let Some(symbol) = nag.ascii().filter(|s| is_safe_symbol(s))
    {
        return symbol.to_owned();
    }
    nag.to_string()
}

/// Whether the first NAG is written touching the move (`e4!`).
fn sticks_to_move(token: &str) -> bool {
    token.starts_with(['!', '?'])
}

impl Walker<'_> {
    fn run(&mut self) -> Result<(), WriteError> {
        let game = self.game;
        let root = game.root();

        if self.opts.comments
            && let Some(cleaned) = game.initial_comment().and_then(|c| self.clean_comment(c))
        {
            self.put_comment(&cleaned, false);
            self.layout.break_line();
        }

        let mut stack = vec![Frame {
            prev: root,
            cur: game.children(root)?.first().copied(),
            head: false,
            vars: None,
            print_num: true,
            in_region: false,
            closes_region: false,
            depth: 0,
            indented: false,
        }];

        while let Some(top) = stack.len().checked_sub(1) {
            if let Some((parent, index)) = stack[top].vars {
                let next = game.children(parent)?.get(index).copied();
                if let Some(variation) = next {
                    stack[top].vars = Some((parent, index + 1));
                    stack[top].print_num = true;
                    let child = self.open_variation(&stack[top], parent, variation)?;
                    stack.push(child);
                } else {
                    stack[top].vars = None;
                }
                continue;
            }
            if let Some(node) = stack[top].cur {
                self.write_node(&mut stack[top], node)?;
                continue;
            }
            if let Some(frame) = stack.pop() {
                self.close_line(&frame);
            }
        }

        self.flush_row();
        self.layout.word(game.tags().result.as_str());
        Ok(())
    }

    /// Opens the variation starting at `variation`, an alternative to the
    /// main continuation after `parent`.
    fn open_variation(
        &mut self,
        outer: &Frame,
        parent: NodeId,
        variation: NodeId,
    ) -> Result<Frame, WriteError> {
        if outer.depth == 0 && !outer.in_region {
            self.flush_row();
        }
        let indented = self.opts.effective_indent_variations() && !outer.in_region;
        if indented {
            self.layout.break_line();
            self.layout
                .set_indent(self.layout.indent().saturating_add(INDENT_STEP));
        }
        self.layout.add_prefix("(");
        if self.opts.comments
            && let Some(cleaned) = self
                .game
                .comment_before(variation)?
                .and_then(|c| self.clean_comment(c))
        {
            self.put_comment(&cleaned, outer.in_region);
        }
        Ok(Frame {
            prev: parent,
            cur: Some(variation),
            head: true,
            vars: None,
            print_num: true,
            in_region: outer.in_region,
            closes_region: false,
            depth: outer.depth.saturating_add(1),
            indented,
        })
    }

    /// Ends a line: closes the null-move comment and the parenthesis.
    fn close_line(&mut self, frame: &Frame) {
        if frame.closes_region {
            self.layout.glue("}");
        }
        if frame.depth > 0 {
            self.layout.glue(")");
            if frame.indented {
                self.layout.break_line();
                self.layout
                    .set_indent(self.layout.indent().saturating_sub(INDENT_STEP));
            }
        }
    }

    /// Writes the move `node` with its NAGs and comment, then queues its
    /// alternatives and advances the line.
    fn write_node(&mut self, frame: &mut Frame, node: NodeId) -> Result<(), WriteError> {
        let game = self.game;
        let opts = self.opts;
        let before = game.position_at(frame.prev)?;
        let mv = game.mv(node)?;
        let white = before.turn() == Color::White;
        let fullmove = before.fullmove_number();
        let is_main = !frame.head;
        frame.head = false;
        let is_null = mv.is_null();

        let mut starts_region = false;
        if is_null && opts.null_moves == NullMoves::ConvertToComment && !frame.in_region {
            if frame.depth == 0 {
                self.flush_row();
            }
            frame.in_region = true;
            frame.closes_region = true;
            starts_region = true;
        }

        let san = if is_null {
            "--".to_owned()
        } else {
            before.san(mv).map_err(|source| WriteError::Move {
                ply: game.ply(node).unwrap_or(0),
                source,
            })?
        };
        let nags = if opts.nags == NagOutput::None {
            &[]
        } else {
            game.nags(node)?
        };
        let need_number = white || frame.print_num;
        frame.print_num = false;
        let column = opts.column && frame.depth == 0 && !frame.in_region;

        if column {
            self.column_move(white, fullmove, &san, nags);
        } else {
            self.scratch.clear();
            if need_number {
                self.scratch.push_str(&fullmove.to_string());
                self.scratch.push_str(if white { "." } else { "..." });
                if opts.move_number_space {
                    self.scratch.push(' ');
                }
            }
            self.scratch.push_str(&san);
            if starts_region {
                self.layout.add_prefix("{");
            }
            self.layout.word(&self.scratch);
            for (i, nag) in nags.iter().enumerate() {
                let token = nag_token(*nag, opts.nags);
                if i == 0 && sticks_to_move(&token) {
                    self.layout.glue(&token);
                } else {
                    self.layout.word(&token);
                }
            }
        }

        if opts.comments
            && let Some(cleaned) = game.comment(node)?.and_then(|c| self.clean_comment(c))
        {
            let indent = opts.effective_indent_comments() && frame.depth == 0 && !frame.in_region;
            if column {
                self.flush_row();
            }
            if indent {
                self.layout.break_line();
                self.layout
                    .set_indent(self.layout.indent().saturating_add(INDENT_STEP));
            }
            self.put_comment(&cleaned, frame.in_region);
            if indent {
                self.layout.break_line();
                self.layout
                    .set_indent(self.layout.indent().saturating_sub(INDENT_STEP));
            }
            frame.print_num = true;
        }

        if opts.variations && is_main && game.children(frame.prev)?.len() > 1 {
            frame.vars = Some((frame.prev, 1));
        }
        frame.prev = node;
        frame.cur = game.children(node)?.first().copied();
        Ok(())
    }

    /// Adds a main-line move in column style (spec rule 34).
    fn column_move(&mut self, white: bool, fullmove: u32, san: &str, nags: &[Nag]) {
        let mut cell = san.to_owned();
        for (i, nag) in nags.iter().enumerate() {
            let token = nag_token(*nag, self.opts.nags);
            if !(i == 0 && sticks_to_move(&token)) {
                cell.push(' ');
            }
            cell.push_str(&token);
        }
        let cell_chars = cell.chars().count();
        if white {
            self.flush_row();
            self.row = Some(Row {
                text: format!("\t{fullmove:>4}.\t{cell}"),
                cell_chars,
            });
        } else if let Some(mut row) = self.row.take() {
            let pad = COLUMN_CELL.saturating_sub(row.cell_chars).max(1);
            row.text.extend(std::iter::repeat_n(' ', pad));
            row.text.push('\t');
            row.text.push_str(&cell);
            self.layout.raw_line(&row.text);
        } else {
            self.layout
                .raw_line(&format!("\t{fullmove:>4}.\t...        \t{cell}"));
        }
    }

    /// Ends the pending column row, if any.
    fn flush_row(&mut self) {
        if let Some(row) = self.row.take() {
            self.layout.raw_line(&row.text);
        }
    }

    /// Applies the comment options. `None` when nothing is left to write
    /// (Scid's `CommentEmpty`, `game.cpp:2168`).
    fn clean_comment(&self, text: &str) -> Option<String> {
        let stripped = if self.opts.strip_marks {
            strip_mark_codes(text)
        } else {
            std::borrow::Cow::Borrowed(text)
        };
        // Braces would end the comment early, so they never survive.
        let cleaned: String = match self.opts.braces {
            BraceHandling::ReplaceWithParens => stripped
                .chars()
                .map(|c| match c {
                    '{' => '(',
                    '}' => ')',
                    c => c,
                })
                .collect(),
            BraceHandling::Strip => stripped
                .chars()
                .filter(|c| !matches!(c, '{' | '}'))
                .collect(),
        };
        let blank = cleaned.bytes().all(|b| b.is_ascii_whitespace());
        (!blank).then_some(cleaned)
    }

    /// Writes cleaned comment text as words. `plain` leaves out the braces
    /// (inside a null-move comment). Explicit line breaks are kept; runs of
    /// blanks collapse and blank lines vanish.
    fn put_comment(&mut self, cleaned: &str, plain: bool) {
        let mut first = true;
        for line in cleaned.split(['\n', '\r']) {
            let mut words = line.split_ascii_whitespace().peekable();
            if words.peek().is_none() {
                continue;
            }
            if first {
                if !plain {
                    self.layout.add_prefix("{");
                }
            } else {
                self.layout.break_line();
            }
            first = false;
            for word in words {
                self.layout.word(word);
            }
        }
        if !first && !plain {
            self.layout.glue("}");
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one option at a time
mod tests {
    use super::*;
    use crate::write::options::{Encoding, LineWidth};
    use mychess_core::{Move, NodeId, Position, SanMode};
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    /// Appends `san` as a child of `parent`.
    fn mv(g: &mut Game, parent: NodeId, san: &str) -> NodeId {
        let m = g
            .position_at(parent)
            .unwrap()
            .parse_san(san, SanMode::Strict)
            .unwrap()
            .mv;
        g.add_move(parent, m).unwrap()
    }

    /// Appends a line of moves, returning every node.
    fn line(g: &mut Game, parent: NodeId, sans: &str) -> Vec<NodeId> {
        let mut at = parent;
        sans.split_whitespace()
            .map(|s| {
                at = mv(g, at, s);
                at
            })
            .collect()
    }

    fn mv_root(g: &mut Game, san: &str) -> NodeId {
        let root = g.root();
        mv(g, root, san)
    }

    fn line_root(g: &mut Game, sans: &str) -> Vec<NodeId> {
        let root = g.root();
        line(g, root, sans)
    }

    fn text(g: &Game, o: &WriteOptions) -> String {
        write_movetext(g, o, "\n").unwrap()
    }

    fn opts() -> WriteOptions {
        WriteOptions::default()
    }

    fn nag(n: u16) -> Nag {
        Nag::new(n).unwrap()
    }

    /// 1. e4 e5 (1... c5 2. Nf3) 2. Nf3, result `*`.
    fn with_variation() -> (Game, Vec<NodeId>) {
        let mut g = Game::new();
        let main = line_root(&mut g, "e4 e5 Nf3");
        let c5 = mv(&mut g, main[0], "c5");
        let nf3 = mv(&mut g, c5, "Nf3");
        (g, vec![main[0], main[1], main[2], c5, nf3])
    }

    #[test]
    fn empty_game_is_just_the_result() {
        assert_eq!(text(&Game::new(), &opts()), "*\n");
    }

    #[test]
    fn main_line_with_result() {
        let mut g = Game::new();
        line_root(&mut g, "e4 e5 Nf3 Nc6");
        g.tags_mut().result = mychess_core::GameResult::Draw;
        assert_eq!(text(&g, &opts()), "1. e4 e5 2. Nf3 Nc6 1/2-1/2\n");
    }

    #[test]
    fn variation_without_inner_spaces_and_number_after_it() {
        let (g, _) = with_variation();
        assert_eq!(text(&g, &opts()), "1. e4 e5 (1... c5 2. Nf3) 2. Nf3 *\n");
    }

    #[test]
    fn variations_option_off_drops_them() {
        let (g, _) = with_variation();
        let mut o = opts();
        o.variations = false;
        assert_eq!(text(&g, &o), "1. e4 e5 2. Nf3 *\n");
    }

    #[test]
    fn variation_on_the_first_move_has_number() {
        let mut g = Game::new();
        let e4 = mv_root(&mut g, "e4");
        mv(&mut g, e4, "e5");
        mv_root(&mut g, "d4");
        assert_eq!(text(&g, &opts()), "1. e4 (1. d4) 1... e5 *\n");
    }

    #[test]
    fn several_variations_keep_insertion_order() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5");
        line(&mut g, m[0], "c5");
        line(&mut g, m[0], "e6");
        assert_eq!(text(&g, &opts()), "1. e4 e5 (1... c5) (1... e6) *\n");
    }

    #[test]
    fn nested_variation() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5 Nf3");
        let c5 = mv(&mut g, m[0], "c5");
        let nf3 = mv(&mut g, c5, "Nf3");
        let d6 = mv(&mut g, nf3, "d6");
        mv(&mut g, nf3, "Nc6");
        mv(&mut g, d6, "d4");
        assert_eq!(
            text(&g, &opts()),
            "1. e4 e5 (1... c5 2. Nf3 d6 (2... Nc6) 3. d4) 2. Nf3 *\n"
        );
    }

    #[test]
    fn move_number_space_off() {
        let (g, _) = with_variation();
        let mut o = opts();
        o.move_number_space = false;
        assert_eq!(text(&g, &o), "1.e4 e5 (1...c5 2.Nf3) 2.Nf3 *\n");
    }

    #[test]
    fn comments_nags_and_number_after_black_comment() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5 Nf3 Nc6");
        g.set_comment(m[0], "best by test").unwrap();
        g.add_nag(m[0], nag(1)).unwrap();
        g.add_nag(m[2], nag(14)).unwrap();
        g.set_comment(m[2], "bom").unwrap();
        g.set_comment(m[1], "after black").unwrap();
        // The NAG comes right after the move, then the comment.
        assert_eq!(
            text(&g, &opts()),
            "1. e4 $1 {best by test} 1... e5 {after black} 2. Nf3 $14 {bom} 2... Nc6 *\n"
        );
    }

    #[test]
    fn initial_comment_has_its_own_line() {
        let mut g = Game::new();
        line_root(&mut g, "e4 e5");
        g.set_initial_comment("Before it all");
        assert_eq!(text(&g, &opts()), "{Before it all}\n1. e4 e5 *\n");
    }

    #[test]
    fn initial_comment_on_empty_game() {
        let mut g = Game::new();
        g.set_initial_comment("hi");
        assert_eq!(text(&g, &opts()), "{hi}\n*\n");
    }

    #[test]
    fn comment_before_goes_right_after_the_paren() {
        let (mut g, ids) = with_variation();
        g.set_comment_before(ids[3], "Sicilian").unwrap();
        assert_eq!(
            text(&g, &opts()),
            "1. e4 e5 ({Sicilian} 1... c5 2. Nf3) 2. Nf3 *\n"
        );
    }

    #[test]
    fn comments_option_off_drops_comments_but_keeps_nags() {
        let (mut g, ids) = with_variation();
        g.set_comment(ids[0], "x").unwrap();
        g.set_initial_comment("y");
        g.set_comment_before(ids[3], "z").unwrap();
        g.add_nag(ids[0], nag(1)).unwrap();
        let mut o = opts();
        o.comments = false;
        assert_eq!(text(&g, &o), "1. e4 $1 e5 (1... c5 2. Nf3) 2. Nf3 *\n");
    }

    #[rstest]
    #[case(NagOutput::None, "1. e4 e5 *\n")]
    #[case(NagOutput::Numeric, "1. e4 $1 $14 e5 $2 *\n")]
    #[case(NagOutput::Symbolic, "1. e4! += e5? *\n")]
    fn nag_styles(#[case] style: NagOutput, #[case] expected: &str) {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5");
        g.add_nag(m[0], nag(1)).unwrap();
        g.add_nag(m[0], nag(14)).unwrap();
        g.add_nag(m[1], nag(2)).unwrap();
        let mut o = opts();
        o.nags = style;
        assert_eq!(text(&g, &o), expected);
    }

    #[test]
    fn symbolic_nags_keep_unsafe_symbols_numeric() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4");
        // 146 is `N`, 210 is `--`, 8 is `[]`, 142 is `(_)`: none is safe.
        for n in [146, 210, 8, 142, 22, 18] {
            g.add_nag(m[0], nag(n)).unwrap();
        }
        let mut o = opts();
        o.nags = NagOutput::Symbolic;
        assert_eq!(text(&g, &o), "1. e4 $146 $210 $8 $142 ZZ +- *\n");
    }

    #[test]
    fn symbolic_first_nag_not_exclamation_is_separate() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4");
        g.add_nag(m[0], nag(10)).unwrap();
        g.add_nag(m[0], nag(1)).unwrap();
        let mut o = opts();
        o.nags = NagOutput::Symbolic;
        assert_eq!(text(&g, &o), "1. e4 = ! *\n");
    }

    #[rstest]
    #[case(BraceHandling::ReplaceWithParens, "{a (b) c}")]
    #[case(BraceHandling::Strip, "{a b c}")]
    fn braces_inside_comments(#[case] mode: BraceHandling, #[case] expected: &str) {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4");
        g.set_comment(m[0], "a {b} c").unwrap();
        let mut o = opts();
        o.braces = mode;
        assert_eq!(text(&g, &o), format!("1. e4 {expected} *\n"));
    }

    #[test]
    fn strip_marks_removes_commands_and_empty_comments() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5 Nf3");
        g.set_comment(m[0], "[%csl Ge4] strong [%clk 0:01:00]")
            .unwrap();
        g.set_comment(m[1], "[%cal Ge2e4]").unwrap();
        let mut o = opts();
        o.strip_marks = true;
        // The second comment vanishes entirely and does not force a number.
        assert_eq!(text(&g, &o), "1. e4 {strong} 1... e5 2. Nf3 *\n");
        o.strip_marks = false;
        assert!(text(&g, &o).contains("[%cal Ge2e4]"));
    }

    #[test]
    fn comment_that_becomes_blank_is_not_written() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5");
        g.set_comment(m[0], "[%clk 0:00:01]").unwrap();
        let mut o = opts();
        o.strip_marks = true;
        assert_eq!(text(&g, &o), "1. e4 e5 *\n");
    }

    #[test]
    fn explicit_comment_newlines_are_kept_and_blanks_collapse() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4");
        g.set_comment(m[0], "one   two\n\n\nthree\r\nfour").unwrap();
        assert_eq!(text(&g, &opts()), "1. e4 {one two\nthree\nfour} *\n");
    }

    #[test]
    fn null_move_kept_by_default() {
        let mut g = Game::new();
        let e4 = mv_root(&mut g, "e4");
        let n = g.add_move(e4, Move::Null).unwrap();
        line(&mut g, n, "d4");
        assert_eq!(text(&g, &opts()), "1. e4 -- 2. d4 *\n");
    }

    #[test]
    fn null_move_converted_to_comment_with_nested_variation() {
        let mut g = Game::new();
        let e4 = mv_root(&mut g, "e4");
        let n = g.add_move(e4, Move::Null).unwrap();
        let d4 = mv(&mut g, n, "d4");
        mv(&mut g, n, "Nf3");
        mv(&mut g, d4, "d5");
        g.set_comment(d4, "ok").unwrap();
        let mut o = opts();
        o.null_moves = NullMoves::ConvertToComment;
        // The comment of d4 is plain text inside the big comment; the
        // variation stays in parentheses.
        assert_eq!(text(&g, &o), "1. e4 {-- 2. d4 ok (2. Nf3) 2... d5} *\n");
    }

    #[test]
    fn null_move_comment_inside_a_variation_closes_before_the_paren() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5");
        let c5 = mv(&mut g, m[0], "c5");
        let n = g.add_move(c5, Move::Null).unwrap();
        line(&mut g, n, "Nf6");
        let mut o = opts();
        o.null_moves = NullMoves::ConvertToComment;
        assert_eq!(text(&g, &o), "1. e4 e5 (1... c5 {2. -- Nf6}) *\n");
    }

    #[test]
    fn wide_enough_stays_on_one_line() {
        let mut g = Game::new();
        line_root(
            &mut g,
            "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7 Re1 b5 Bb3 d6 c3 O-O h3 Nb8 d4 Nbd7",
        );
        let mut o = opts();
        o.width = LineWidth::Columns(200);
        assert_eq!(
            text(&g, &o),
            "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 Nb8 10. d4 Nbd7 *\n"
        );
    }

    #[rstest]
    #[case(75)]
    #[case(80)]
    #[case(20)]
    fn lines_respect_the_width_and_have_no_trailing_space(#[case] width: u16) {
        let mut g = Game::new();
        let m = line_root(
            &mut g,
            "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7 Re1 b5 Bb3 d6 c3 O-O h3 Nb8 d4 Nbd7",
        );
        g.set_comment(
            m[4],
            "the Ruy Lopez, a long classical opening with many ideas",
        )
        .unwrap();
        let c5 = mv(&mut g, m[0], "c5");
        line(&mut g, c5, "Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6");
        let mut o = opts();
        o.width = LineWidth::Columns(width);
        let out = text(&g, &o);
        for l in out.lines() {
            assert!(l.chars().count() <= usize::from(width), "too long: {l:?}");
            assert_eq!(l, l.trim_end(), "trailing space: {l:?}");
            assert!(!l.is_empty(), "blank line");
            assert!(!l.starts_with(['}', ')']), "closer at line start: {l:?}");
        }
        // Wrapping changes only the white space.
        let mut one = opts();
        one.width = LineWidth::Unlimited;
        let joined: Vec<&str> = out.split_whitespace().collect();
        let flat = text(&g, &one);
        assert_eq!(joined, flat.split_whitespace().collect::<Vec<_>>());
        assert_eq!(flat.lines().count(), 1);
    }

    #[test]
    fn exact_width_stays_and_one_more_wraps() {
        let mut g = Game::new();
        line_root(&mut g, "e4 e5");
        let mut o = opts();
        // "1. e4 e5 *" is 10 characters.
        o.width = LineWidth::Columns(10);
        assert_eq!(text(&g, &o), "1. e4 e5 *\n");
        o.width = LineWidth::Columns(9);
        assert_eq!(text(&g, &o), "1. e4 e5\n*\n");
    }

    #[test]
    fn indent_variations_puts_each_on_its_own_line() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5 Nf3");
        let c5 = mv(&mut g, m[0], "c5");
        let nf3 = mv(&mut g, c5, "Nf3");
        let d6 = mv(&mut g, nf3, "d6");
        mv(&mut g, nf3, "Nc6");
        mv(&mut g, d6, "d4");
        mv(&mut g, m[0], "e6");
        let mut o = opts();
        o.indent_variations = true;
        assert_eq!(
            text(&g, &o),
            "1. e4 e5\n    (1... c5 2. Nf3 d6\n        (2... Nc6)\n    3. d4)\n    (1... e6)\n2. Nf3 *\n"
        );
    }

    #[test]
    fn indent_comments_applies_to_level_zero_only() {
        let (mut g, ids) = with_variation();
        g.set_comment(ids[0], "first").unwrap();
        g.set_comment(ids[3], "inside").unwrap();
        let mut o = opts();
        o.indent_comments = true;
        assert_eq!(
            text(&g, &o),
            "1. e4\n    {first}\n1... e5 (1... c5 {inside} 2. Nf3) 2. Nf3 *\n"
        );
    }

    #[test]
    fn column_style_rows() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5 Nf3 Nc6 Bb5");
        g.add_nag(m[2], nag(1)).unwrap();
        let mut o = opts();
        o.nags = NagOutput::Symbolic;
        o.column = true;
        assert_eq!(
            text(&g, &o),
            "\t   1.\te4          \te5\n\t   2.\tNf3!        \tNc6\n\t   3.\tBb5\n*\n"
        );
    }

    #[test]
    fn column_style_comment_and_variation_end_the_table() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5 Nf3 Nc6");
        g.set_comment(m[0], "king pawn").unwrap();
        mv_root(&mut g, "d4");
        let mut o = opts();
        o.column = true;
        assert_eq!(
            text(&g, &o),
            "\t   1.\te4\n    {king pawn}\n    (1. d4)\n\t   1.\t...        \te5\n\t   2.\tNf3         \tNc6\n*\n"
        );
    }

    #[test]
    fn long_comment_wraps_between_words_with_braces_glued() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5");
        g.set_comment(m[0], "alpha beta gamma delta epsilon")
            .unwrap();
        let mut o = opts();
        o.width = LineWidth::Columns(16);
        assert_eq!(
            text(&g, &o),
            "1. e4 {alpha\nbeta gamma delta\nepsilon} 1... e5\n*\n"
        );
    }

    #[test]
    fn scid_export_layout_of_a_nested_game() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5 Nf3 Nc6");
        let c5 = mv(&mut g, m[0], "c5");
        let nf3 = mv(&mut g, c5, "Nf3");
        g.set_comment(nf3, "Sicilian").unwrap();
        g.add_nag(m[1], nag(1)).unwrap();
        assert_eq!(
            text(&g, &WriteOptions::scid_export()),
            "1. e4 e5 $1\n    (1... c5 2. Nf3 {Sicilian})\n2. Nf3 Nc6 *\n"
        );
    }

    #[test]
    fn column_style_null_move_comment_leaves_the_table() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4");
        let n = g.add_move(m[0], Move::Null).unwrap();
        line(&mut g, n, "d4");
        let mut o = opts();
        o.column = true;
        o.null_moves = NullMoves::ConvertToComment;
        assert_eq!(text(&g, &o), "\t   1.\te4\n{-- 2. d4} *\n");
    }

    #[test]
    fn long_main_line_does_not_overflow_the_stack() {
        // Alternate knight shuffles; 30 000 plies, legal by repetition rules
        // not being enforced by Game.
        let mut g = Game::new();
        let mut at = g.root();
        for i in 0..30_000 {
            let san = ["Nf3", "Nf6", "Ng1", "Ng8"][i % 4];
            at = mv(&mut g, at, san);
        }
        let out = text(&g, &opts());
        assert!(out.ends_with("*\n"));
        assert!(out.lines().all(|l| l.chars().count() <= 80));
    }

    #[test]
    fn deeply_nested_variations_do_not_overflow_the_stack() {
        // Each level adds `Nf3` as the main move and `Nc3 Nf6 Nb1 Ng8` as a
        // variation, which brings the position back and holds the next level.
        let mut g = Game::new();
        let mut parent = g.root();
        let depth = 2_000;
        for _ in 0..depth {
            mv(&mut g, parent, "Nf3");
            let alt = mv(&mut g, parent, "Nc3");
            let reply = mv(&mut g, alt, "Nf6");
            let back = mv(&mut g, reply, "Nb1");
            parent = mv(&mut g, back, "Ng8");
        }
        let mut o = opts();
        o.width = LineWidth::Unlimited;
        let out = text(&g, &o);
        assert_eq!(out.matches('(').count(), depth);
        assert_eq!(out.matches(')').count(), depth);
    }

    #[test]
    fn crlf_newlines_in_movetext() {
        let mut g = Game::new();
        line_root(&mut g, "e4");
        g.set_comment(g.mainline().next().unwrap(), "a\nb").unwrap();
        let out = write_movetext(&g, &opts(), "\r\n").unwrap();
        assert_eq!(out, "1. e4 {a\r\nb} *\r\n");
    }

    #[test]
    fn fen_start_numbers_from_the_position() {
        let pos = Position::from_fen("4k3/8/8/8/8/8/8/4K2R b K - 0 17").unwrap();
        let mut g = Game::from_position(pos);
        line_root(&mut g, "Kd7 Kf1");
        assert_eq!(text(&g, &opts()), "17... Kd7 18. Kf1 *\n");
    }

    #[test]
    fn encoding_option_does_not_affect_text() {
        let (g, _) = with_variation();
        let mut o = opts();
        o.encoding = Encoding::Latin1;
        assert_eq!(text(&g, &o), text(&g, &opts()));
    }

    #[rstest]
    #[case(0)]
    #[case(1)]
    fn width_one_or_zero_puts_each_word_on_its_own_line(#[case] cols: u16) {
        let mut g = Game::new();
        line_root(&mut g, "e4 e5");
        let mut o = opts();
        o.width = LineWidth::Columns(cols);
        assert_eq!(text(&g, &o), "1. e4\ne5\n*\n");
    }

    #[test]
    fn comment_words_starting_with_percent_never_start_a_line() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5");
        g.set_comment(m[0], "%a %b c").unwrap();
        for w in 1..=20u16 {
            let mut o = opts();
            o.width = LineWidth::Columns(w);
            let out = text(&g, &o);
            assert!(out.lines().all(|l| !l.starts_with('%')), "{w}: {out}");
        }
        assert_eq!(text(&g, &opts()), "1. e4 {%a %b c} 1... e5 *\n");
    }

    #[test]
    fn comment_line_that_looks_like_a_tag_is_kept_inside_the_braces() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4");
        g.set_comment(m[0], "[Event \"x\"]").unwrap();
        assert_eq!(text(&g, &opts()), "1. e4 {[Event \"x\"]} *\n");
    }

    #[test]
    fn column_style_with_a_nested_variation_is_indented() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5 Nf3");
        let c5 = mv(&mut g, m[0], "c5");
        mv(&mut g, c5, "Nf3");
        let mut o = opts();
        o.column = true;
        let out = text(&g, &o);
        assert!(out.contains("\n    (1... c5 2. Nf3)\n"), "{out}");
    }

    #[test]
    fn black_null_move_is_written_as_dashes() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4");
        g.add_move(m[0], Move::Null).unwrap();
        assert_eq!(text(&g, &opts()), "1. e4 -- *\n");
    }

    #[test]
    fn nested_variations_close_on_one_line_when_indented() {
        let mut g = Game::new();
        let m = line_root(&mut g, "e4 e5 Nf3");
        let c5 = mv(&mut g, m[0], "c5");
        let n = mv(&mut g, c5, "Nf3");
        mv(&mut g, c5, "d4");
        let _ = n;
        let mut o = opts();
        o.indent_variations = true;
        let out = text(&g, &o);
        assert!(
            out.lines().all(|l| !l.trim_start().starts_with(')')),
            "{out}"
        );
    }
}
