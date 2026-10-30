//! One EPD record: a position and its operations.

use std::cmp::Ordering;

use mychess_core::{Move, Position, SanError, SanMode};

/// One EPD operation: an opcode and its operands.
///
/// `operands` is the raw text, trimmed at both ends but otherwise untouched
/// (quotes and inner spaces included), so writing reproduces reading. It is
/// empty for an operation without operands such as `noop;`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EpdOperation {
    /// The opcode, normally `[A-Za-z][A-Za-z0-9_]*`.
    pub opcode: String,
    /// The raw operand text.
    pub operands: String,
}

impl EpdOperation {
    /// Creates an operation; the operands are trimmed. The opcode is stored
    /// as given (not trimmed or validated): writing rejects an opcode that
    /// is empty or has white space.
    pub fn new(opcode: impl Into<String>, operands: impl AsRef<str>) -> EpdOperation {
        EpdOperation {
            opcode: opcode.into(),
            operands: operands.as_ref().trim().to_owned(),
        }
    }

    /// True when the opcode matches `[A-Za-z][A-Za-z0-9_]*`.
    pub fn has_standard_opcode(&self) -> bool {
        is_standard_opcode(&self.opcode)
    }

    /// The operands without the surrounding double quotes, if both are
    /// there; the raw operands otherwise.
    pub fn unquoted(&self) -> &str {
        unquote(&self.operands)
    }

    /// Orders by the bytes of the text `opcode operands` (what the Scid's
    /// `lsort -ascii` compares).
    pub(crate) fn sort_cmp(&self, other: &EpdOperation) -> Ordering {
        self.sort_bytes().cmp(other.sort_bytes())
    }

    fn sort_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        let sep = (!self.operands.is_empty()).then_some(b' ');
        self.opcode.bytes().chain(sep).chain(self.operands.bytes())
    }
}

pub(crate) fn is_standard_opcode(opcode: &str) -> bool {
    let mut bytes = opcode.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn unquote(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text)
}

/// Parses the leading integer of `text` (optional sign, then digits), like
/// C's `strtol` stopping at the first other character.
pub(crate) fn leading_int(text: &str) -> Option<i64> {
    let text = text.trim_start();
    let digits_from = usize::from(text.starts_with(['+', '-']));
    let end = text
        .get(digits_from..)?
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .map_or(text.len(), |p| p + digits_from);
    text.get(..end)?.parse().ok()
}

/// A position with its operations, in file order.
///
/// The position is fixed (it is the key in the [`EpdBook`](crate::epd::EpdBook));
/// the operations can be edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpdRecord {
    position: Position,
    operations: Vec<EpdOperation>,
}

impl EpdRecord {
    /// A record with no operations.
    pub fn new(position: Position) -> EpdRecord {
        EpdRecord {
            position,
            operations: Vec::new(),
        }
    }

    /// A record with the given operations.
    pub fn with_operations(position: Position, operations: Vec<EpdOperation>) -> EpdRecord {
        EpdRecord {
            position,
            operations,
        }
    }

    /// The position.
    pub fn position(&self) -> &Position {
        &self.position
    }

    /// The operations, in order.
    pub fn operations(&self) -> &[EpdOperation] {
        &self.operations
    }

    /// Mutable access to the operations.
    pub fn operations_mut(&mut self) -> &mut Vec<EpdOperation> {
        &mut self.operations
    }

    /// The first operation with this opcode.
    pub fn operation(&self, opcode: &str) -> Option<&EpdOperation> {
        self.operations.iter().find(|op| op.opcode == opcode)
    }

    /// Every operation with this opcode.
    pub fn operations_named<'a>(
        &'a self,
        opcode: &'a str,
    ) -> impl Iterator<Item = &'a EpdOperation> + 'a {
        self.operations.iter().filter(move |op| op.opcode == opcode)
    }

    /// Sets the first operation with this opcode (removing its other
    /// occurrences), or appends one when there is none.
    pub fn set_operation(&mut self, opcode: &str, operands: &str) {
        let operands = operands.trim();
        let mut found = false;
        self.operations.retain_mut(|op| {
            if op.opcode != opcode {
                return true;
            }
            if found {
                return false;
            }
            found = true;
            operands.clone_into(&mut op.operands);
            true
        });
        if !found {
            self.operations.push(EpdOperation::new(opcode, operands));
        }
    }

    /// Removes every operation with this opcode; returns how many.
    pub fn remove_opcode(&mut self, opcode: &str) -> usize {
        let before = self.operations.len();
        self.operations.retain(|op| op.opcode != opcode);
        before - self.operations.len()
    }

    /// The moves of `bm` (best moves), read as SAN in this position. Empty
    /// without the operation.
    pub fn bm(&self) -> Result<Vec<Move>, SanError> {
        self.san_list("bm")
    }

    /// The moves of `am` (avoid moves), read as SAN in this position.
    pub fn am(&self) -> Result<Vec<Move>, SanError> {
        self.san_list("am")
    }

    /// The moves of `pv` (predicted variation), played in sequence from this
    /// position. Empty without the operation.
    pub fn pv(&self) -> Result<Vec<Move>, SanError> {
        let Some(op) = self.operation("pv") else {
            return Ok(Vec::new());
        };
        let mut pos = self.position.clone();
        let mut moves = Vec::new();
        for token in op.operands.split_whitespace() {
            let mv = pos.parse_san(token, SanMode::Tolerant)?.mv;
            moves.push(mv);
            match pos.after(mv) {
                Ok(next) => pos = next,
                Err(_) => break,
            }
        }
        Ok(moves)
    }

    fn san_list(&self, opcode: &str) -> Result<Vec<Move>, SanError> {
        let Some(op) = self.operation(opcode) else {
            return Ok(Vec::new());
        };
        op.operands
            .split_whitespace()
            .map(|t| self.position.parse_san(t, SanMode::Tolerant).map(|p| p.mv))
            .collect()
    }

    /// `ce`: the evaluation in centipawns, from the point of view of the side
    /// to move.
    pub fn centipawns(&self) -> Option<i32> {
        self.int_operand("ce").and_then(|v| i32::try_from(v).ok())
    }

    /// `dm`: the distance to mate in moves.
    pub fn dm(&self) -> Option<i32> {
        self.int_operand("dm").and_then(|v| i32::try_from(v).ok())
    }

    /// `acd`: the analysis depth.
    pub fn acd(&self) -> Option<u64> {
        self.int_operand("acd").and_then(|v| u64::try_from(v).ok())
    }

    /// `acn`: the number of nodes searched.
    pub fn acn(&self) -> Option<u64> {
        self.int_operand("acn").and_then(|v| u64::try_from(v).ok())
    }

    /// `acs`: the analysis time in seconds.
    pub fn acs(&self) -> Option<u64> {
        self.int_operand("acs").and_then(|v| u64::try_from(v).ok())
    }

    /// `id`: the position identifier, without quotes.
    pub fn id(&self) -> Option<&str> {
        self.operation("id").map(EpdOperation::unquoted)
    }

    /// `eco`: the ECO code text, without quotes.
    pub fn eco(&self) -> Option<&str> {
        self.operation("eco").map(EpdOperation::unquoted)
    }

    /// `c0` to `c9`: a comment, without quotes. `None` when `n > 9`.
    pub fn comment(&self, n: u8) -> Option<&str> {
        if n > 9 {
            return None;
        }
        let opcode = format!("c{n}");
        self.operation(&opcode).map(EpdOperation::unquoted)
    }

    fn int_operand(&self, opcode: &str) -> Option<i64> {
        leading_int(&self.operation(opcode)?.operands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(ops: &[(&str, &str)]) -> EpdRecord {
        EpdRecord::with_operations(
            Position::new(),
            ops.iter().map(|(o, a)| EpdOperation::new(*o, a)).collect(),
        )
    }

    #[test]
    fn opcode_validation() {
        assert!(is_standard_opcode("bm"));
        assert!(is_standard_opcode("c0"));
        assert!(is_standard_opcode("draw_offer"));
        assert!(!is_standard_opcode(""));
        assert!(!is_standard_opcode("1x"));
        assert!(!is_standard_opcode("a-b"));
    }

    #[test]
    fn typed_helpers() {
        let r = rec(&[
            ("bm", "e4 Nf3"),
            ("am", "f3"),
            ("pv", "e4 e5 Nf3"),
            ("ce", "-25"),
            ("id", "\"start\""),
            ("c3", "\"third\""),
            ("acd", "12"),
            ("acn", "1000000"),
            ("dm", "3"),
        ]);
        assert_eq!(r.bm().map(|m| m.len()), Ok(2));
        assert_eq!(r.am().map(|m| m.len()), Ok(1));
        assert_eq!(r.pv().map(|m| m.len()), Ok(3));
        assert_eq!(r.centipawns(), Some(-25));
        assert_eq!(r.id(), Some("start"));
        assert_eq!(r.comment(3), Some("third"));
        assert_eq!(r.comment(10), None);
        assert_eq!(r.acd(), Some(12));
        assert_eq!(r.acn(), Some(1_000_000));
        assert_eq!(r.dm(), Some(3));
        assert_eq!(r.eco(), None);
    }

    #[test]
    fn san_errors_and_missing_ops() {
        let r = rec(&[("bm", "e4 Qh5")]);
        assert!(r.bm().is_err());
        assert_eq!(rec(&[]).bm(), Ok(Vec::new()));
        assert_eq!(rec(&[]).pv(), Ok(Vec::new()));
        // The second pv move is illegal after the first.
        assert!(rec(&[("pv", "e4 e4")]).pv().is_err());
    }

    #[test]
    fn leading_int_cases() {
        assert_eq!(leading_int(" 25 x"), Some(25));
        assert_eq!(leading_int("-7"), Some(-7));
        assert_eq!(leading_int("+7;"), Some(7));
        assert_eq!(leading_int("x1"), None);
        assert_eq!(leading_int("-"), None);
        assert_eq!(leading_int(""), None);
        assert_eq!(leading_int("99999999999999999999999"), None);
    }

    #[test]
    fn set_and_remove() {
        let mut r = rec(&[("id", "a"), ("bm", "e4"), ("id", "b")]);
        r.set_operation("id", " z ");
        assert_eq!(r.operations().len(), 2);
        assert_eq!(r.id(), Some("z"));
        r.set_operation("ce", "10");
        assert_eq!(r.centipawns(), Some(10));
        assert_eq!(r.remove_opcode("id"), 1);
        assert_eq!(r.remove_opcode("id"), 0);
    }

    #[test]
    fn sort_order_is_bytewise_on_the_joined_text() {
        let a = EpdOperation::new("bm", "e4");
        let b = EpdOperation::new("bm", "");
        let c = EpdOperation::new("bma", "");
        assert_eq!(b.sort_cmp(&a), Ordering::Less);
        assert_eq!(a.sort_cmp(&c), Ordering::Less);
        assert_eq!(a.sort_cmp(&a), Ordering::Equal);
    }
}
