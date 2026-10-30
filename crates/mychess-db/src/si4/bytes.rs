//! Big-endian readers over `&[u8]` and a cursor for game records
//! (`docs/formatos/si4.md`, intro: all integers are big-endian).
//!
//! Every reader checks the bounds and returns `None` / a [`RecordError`]
//! instead of panicking.

use super::error::RecordError;

/// Reads a big-endian `u16` at `offset`, or `None` if it does not fit.
pub fn read_u16_be(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_be_bytes(bytes.try_into().ok()?))
}

/// Reads a big-endian 24-bit integer at `offset`, or `None` if it does not fit.
pub fn read_u24_be(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset.checked_add(3)?)?;
    Some(bytes.iter().fold(0u32, |acc, &b| (acc << 8) | u32::from(b)))
}

/// Reads a big-endian `u32` at `offset`, or `None` if it does not fit.
pub fn read_u32_be(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_be_bytes(bytes.try_into().ok()?))
}

/// A forward-only reader over one game record.
#[derive(Debug, Clone)]
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Cursor<'a> {
        Cursor { data, pos: 0 }
    }

    /// The bytes not read yet.
    pub(crate) fn rest(&self) -> &'a [u8] {
        self.data.get(self.pos..).unwrap_or(&[])
    }

    pub(crate) fn error(&self, reason: &'static str) -> RecordError {
        RecordError::new(self.pos, reason)
    }

    pub(crate) fn u8(&mut self, reason: &'static str) -> Result<u8, RecordError> {
        let byte = *self.data.get(self.pos).ok_or_else(|| self.error(reason))?;
        self.pos += 1;
        Ok(byte)
    }

    pub(crate) fn bytes(
        &mut self,
        n: usize,
        reason: &'static str,
    ) -> Result<&'a [u8], RecordError> {
        let end = self.pos.checked_add(n).ok_or_else(|| self.error(reason))?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| self.error(reason))?;
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn skip(&mut self, n: usize) {
        // `n` never exceeds the bytes already inspected through `rest()`.
        self.pos = self.pos.saturating_add(n).min(self.data.len());
    }

    /// Reads bytes up to (not including) a NUL and consumes the NUL.
    pub(crate) fn c_string(&mut self, reason: &'static str) -> Result<&'a [u8], RecordError> {
        let rest = self.rest();
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| self.error(reason))?;
        let text = rest.get(..len).ok_or_else(|| self.error(reason))?;
        self.pos += len + 1;
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_big_endian_integers() {
        let data = [0x12, 0x34, 0x56, 0x78, 0x9A];
        assert_eq!(read_u16_be(&data, 0), Some(0x1234));
        assert_eq!(read_u16_be(&data, 3), Some(0x789A));
        assert_eq!(read_u24_be(&data, 1), Some(0x0034_5678));
        assert_eq!(read_u32_be(&data, 1), Some(0x3456_789A));
    }

    #[test]
    fn readers_reject_out_of_bounds_offsets() {
        let data = [1, 2, 3];
        assert_eq!(read_u16_be(&data, 2), None);
        assert_eq!(read_u24_be(&data, 1), None);
        assert_eq!(read_u32_be(&data, 0), None);
        assert_eq!(read_u16_be(&data, usize::MAX), None);
        assert_eq!(read_u32_be(&[], 0), None);
    }

    #[test]
    fn cursor_reads_and_reports_position() {
        let mut cur = Cursor::new(b"ab\0cd");
        assert_eq!(cur.u8("x"), Ok(b'a'));
        assert_eq!(cur.c_string("x"), Ok(&b"b"[..]));
        assert_eq!(cur.rest(), b"cd");
        assert_eq!(cur.bytes(2, "x"), Ok(&b"cd"[..]));
        assert_eq!(cur.rest(), b"");
        assert_eq!(cur.u8("eof").map_err(|e| e.at()), Err(5));
    }

    #[test]
    fn cursor_string_without_nul_is_an_error() {
        let mut cur = Cursor::new(b"abc");
        assert!(cur.c_string("no nul").is_err());
        assert!(cur.bytes(4, "short").is_err());
    }
}
