//! Bounds-checked little-endian reading over a byte slice.

use std::io::Read;

use crate::error::{FormatError, Result};

#[derive(Clone)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    what: &'static str,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], what: &'static str) -> Self {
        Self { data, pos: 0, what }
    }

    pub fn at(data: &'a [u8], pos: usize, what: &'static str) -> Self {
        Self { data, pos, what }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn is_at_end(&self) -> bool {
        self.pos == self.data.len()
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let out = slice(self.data, self.pos, n, self.what)?;
        self.pos += n;
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    /// `u32` read as a length or offset.
    pub fn usize(&mut self) -> Result<usize> {
        Ok(self.u32()? as usize)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let b = self.bytes(N)?;
        let mut out = [0; N];
        out.copy_from_slice(b);
        Ok(out)
    }
}

/// `data[offset..offset + len]` or a `Truncated` error.
pub(crate) fn slice<'a>(
    data: &'a [u8],
    offset: usize,
    len: usize,
    what: &'static str,
) -> Result<&'a [u8]> {
    offset
        .checked_add(len)
        .and_then(|end| data.get(offset..end))
        .ok_or(FormatError::Truncated {
            what,
            offset,
            needed: len,
            len: data.len(),
        })
}

/// NUL-terminated string starting at `offset`. Bytes map 1:1 to chars (Latin-1).
pub(crate) fn cstr_at(data: &[u8], offset: usize, what: &'static str) -> Result<String> {
    let rest = data.get(offset..).ok_or(FormatError::Truncated {
        what,
        offset,
        needed: 1,
        len: data.len(),
    })?;
    let end = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| FormatError::Invalid(format!("{what}: unterminated string at {offset}")))?;
    Ok(latin1(&rest[..end]))
}

pub(crate) fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Inflates a zlib stream that must produce exactly `expected` bytes.
pub(crate) fn inflate(data: &[u8], expected: usize, what: &str) -> Result<Vec<u8>> {
    // Real files stay far below this; it guards against hostile size fields.
    const LIMIT: usize = 512 << 20;
    if expected > LIMIT {
        return Err(FormatError::Invalid(format!(
            "{what}: declared size {expected} too large"
        )));
    }
    let mut out = Vec::with_capacity(expected);
    flate2::read::ZlibDecoder::new(data)
        .take(expected as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| FormatError::Zlib(format!("{what}: {e}")))?;
    if out.len() != expected {
        return Err(FormatError::Invalid(format!(
            "{what}: inflated to {} bytes, expected {expected}",
            out.len()
        )));
    }
    Ok(out)
}
