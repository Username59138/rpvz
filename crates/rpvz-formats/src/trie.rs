//! PopCap's compressed string list, used for file and group names in RSB/RSG.
//!
//! Entries are runs of 4-byte units `[char: u8][branch: u24]`. A non-zero
//! `branch` says another entry starts at `begin + branch * 4` and shares the
//! name built so far. `char == 0` ends a name and is followed by its value.

use std::collections::HashMap;

use crate::error::Result;
use crate::read::{Reader, latin1, slice};

/// Decodes the list at `data[begin..begin + len]`. `read_value` parses the
/// value after each name.
pub(crate) fn decode<T>(
    data: &[u8],
    begin: usize,
    len: usize,
    mut read_value: impl FnMut(&mut Reader<'_>) -> Result<T>,
) -> Result<Vec<(String, T)>> {
    let region = slice(data, begin, len, "string list")?;
    let mut r = Reader::new(region, "string list");
    let mut pending: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut name = Vec::new();
    let mut out = Vec::new();
    while !r.is_at_end() {
        if let Some(prefix) = pending.remove(&r.pos()) {
            name = prefix;
        }
        let ch = r.u8()?;
        let b = r.bytes(3)?;
        let branch = usize::from(b[0]) | usize::from(b[1]) << 8 | usize::from(b[2]) << 16;
        if branch != 0 {
            pending.insert(branch * 4, name.clone());
        }
        if ch == 0 {
            let value = read_value(&mut r)?;
            out.push((latin1(&name), value));
            name.clear();
        } else {
            name.push(ch);
        }
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Encodes names without prefix sharing: always a valid list.
    pub fn encode_flat(entries: &[(&str, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, value) in entries {
            for b in name.bytes().chain([0]) {
                out.extend_from_slice(&[b, 0, 0, 0]);
            }
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    #[test]
    fn flat_list() {
        let data = encode_flat(&[("A\\B.PTX", 7), ("C", 9)]);
        let got = decode(&data, 0, data.len(), |r| r.u32()).unwrap();
        assert_eq!(got, vec![("A\\B.PTX".to_string(), 7), ("C".to_string(), 9)]);
    }

    #[test]
    fn shared_prefix() {
        // "AB" = 1 and "AC" = 2; the branch on 'B' points to the unit holding 'C'.
        let mut data = Vec::new();
        data.extend_from_slice(&[b'A', 0, 0, 0]); // 0
        data.extend_from_slice(&[b'B', 4, 0, 0]); // 1: branch -> unit 4
        data.extend_from_slice(&[0, 0, 0, 0]); // 2: end of "AB"
        data.extend_from_slice(&1u32.to_le_bytes()); // 3
        data.extend_from_slice(&[b'C', 0, 0, 0]); // 4
        data.extend_from_slice(&[0, 0, 0, 0]); // 5
        data.extend_from_slice(&2u32.to_le_bytes()); // 6
        let got = decode(&data, 0, data.len(), |r| r.u32()).unwrap();
        assert_eq!(got, vec![("AB".to_string(), 1), ("AC".to_string(), 2)]);
    }

    #[test]
    fn truncated_value_is_an_error() {
        let mut data = encode_flat(&[("A", 1)]);
        data.truncate(data.len() - 2);
        assert!(decode(&data, 0, data.len(), |r| r.u32()).is_err());
    }
}
