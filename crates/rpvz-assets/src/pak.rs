//! PopCap `.pak` archives as used by the PC release (`main.pak`).
//!
//! The whole file is XOR-ed with `0xF7`. Decoded, it is:
//!
//! ```text
//! u32 magic = 0xBAC04AC0, u32 version = 0
//! repeated { u8 flags; if flags & 0x80: end
//!            u8 name_len; name; u32 size; u64 filetime }
//! file data, back to back, in table order
//! ```
//! All integers are little-endian.

use std::io::{self, Read, Seek, SeekFrom, Write};

use crate::error::{Error, Result};

pub const XOR_KEY: u8 = 0xF7;
pub const MAGIC: u32 = 0xBAC0_4AC0;
pub const VERSION: u32 = 0;
const END_FLAG: u8 = 0x80;

/// Guards against garbage input making us allocate absurd tables.
const MAX_ENTRIES: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakEntry {
    /// Name as stored, usually with `\` separators.
    pub name: String,
    pub offset: u64,
    pub size: u32,
    pub filetime: u64,
}

/// Reader that XOR-decodes everything it reads.
struct XorReader<R> {
    inner: R,
}

impl<R: Read> Read for XorReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        buf[..n].iter_mut().for_each(|b| *b ^= XOR_KEY);
        Ok(n)
    }
}

impl<R: Seek> Seek for XorReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

pub struct PakReader<R> {
    inner: XorReader<R>,
    entries: Vec<PakEntry>,
}

impl<R: Read + Seek> PakReader<R> {
    pub fn new(inner: R) -> Result<Self> {
        let mut inner = XorReader { inner };
        let file_len = inner.seek(SeekFrom::End(0)).map_err(pak_io)?;
        inner.seek(SeekFrom::Start(0)).map_err(pak_io)?;

        let magic = read_u32(&mut inner)?;
        if magic != MAGIC {
            return Err(Error::Pak(format!("bad magic {magic:#010x}")));
        }
        let version = read_u32(&mut inner)?;
        if version != VERSION {
            return Err(Error::Pak(format!("unsupported version {version}")));
        }

        let mut raw = Vec::new();
        loop {
            let flags = read_u8(&mut inner)?;
            if flags & END_FLAG != 0 {
                break;
            }
            if raw.len() >= MAX_ENTRIES {
                return Err(Error::Pak("too many entries".into()));
            }
            let name_len = read_u8(&mut inner)? as usize;
            let mut name = vec![0; name_len];
            inner.read_exact(&mut name).map_err(pak_io)?;
            let size = read_u32(&mut inner)?;
            let filetime = read_u64(&mut inner)?;
            // Names are ASCII in practice; map bytes 1:1 (Latin-1) to stay lossless.
            let name: String = name.iter().map(|&b| b as char).collect();
            raw.push((name, size, filetime));
        }

        let mut offset = inner.stream_position().map_err(pak_io)?;
        let mut entries = Vec::with_capacity(raw.len());
        for (name, size, filetime) in raw {
            entries.push(PakEntry {
                name,
                offset,
                size,
                filetime,
            });
            offset += u64::from(size);
        }
        if offset > file_len {
            return Err(Error::Pak(format!(
                "table describes {offset} bytes but file has {file_len}"
            )));
        }
        Ok(Self { inner, entries })
    }

    pub fn entries(&self) -> &[PakEntry] {
        &self.entries
    }

    /// Decodes one entry into `out`.
    pub fn copy_entry(&mut self, entry: &PakEntry, out: &mut impl Write) -> Result<u64> {
        self.inner
            .seek(SeekFrom::Start(entry.offset))
            .map_err(pak_io)?;
        let mut limited = (&mut self.inner).take(u64::from(entry.size));
        let n = io::copy(&mut limited, out).map_err(pak_io)?;
        if n != u64::from(entry.size) {
            return Err(Error::Pak(format!("{} is truncated", entry.name)));
        }
        Ok(n)
    }
}

/// Builds a pak archive. Used for tests and tooling; RPvZ never ships one.
pub fn write_pak(files: &[(&str, &[u8])], out: &mut impl Write) -> io::Result<()> {
    let mut plain = Vec::new();
    plain.extend_from_slice(&MAGIC.to_le_bytes());
    plain.extend_from_slice(&VERSION.to_le_bytes());
    for (name, data) in files {
        let name_len = u8::try_from(name.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "name longer than 255 bytes")
        })?;
        let size = u32::try_from(data.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "file larger than 4 GiB"))?;
        plain.push(0);
        plain.push(name_len);
        plain.extend_from_slice(name.as_bytes());
        plain.extend_from_slice(&size.to_le_bytes());
        plain.extend_from_slice(&0u64.to_le_bytes());
    }
    plain.push(END_FLAG);
    for (_, data) in files {
        plain.extend_from_slice(data);
    }
    plain.iter_mut().for_each(|b| *b ^= XOR_KEY);
    out.write_all(&plain)
}

fn pak_io(e: io::Error) -> Error {
    if e.kind() == io::ErrorKind::UnexpectedEof {
        Error::Pak("unexpected end of file".into())
    } else {
        Error::Pak(e.to_string())
    }
}

fn read_u8(r: &mut impl Read) -> Result<u8> {
    let mut b = [0; 1];
    r.read_exact(&mut b).map_err(pak_io)?;
    Ok(b[0])
}

fn read_u32(r: &mut impl Read) -> Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b).map_err(pak_io)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b).map_err(pak_io)?;
    Ok(u64::from_le_bytes(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip() {
        let files: &[(&str, &[u8])] = &[
            ("properties\\resources.xml", b"<ResourceManifest/>"),
            ("images\\empty.png", b""),
            ("reanim\\Test.reanim", &[0u8, 1, 2, 0xF7, 0xFF]),
        ];
        let mut buf = Vec::new();
        write_pak(files, &mut buf).unwrap();
        assert_ne!(
            &buf[..4],
            &MAGIC.to_le_bytes(),
            "data must be XOR-ed on disk"
        );

        let mut pak = PakReader::new(Cursor::new(buf)).unwrap();
        let entries = pak.entries().to_vec();
        assert_eq!(entries.len(), 3);
        for ((name, data), entry) in files.iter().zip(&entries) {
            assert_eq!(entry.name, *name);
            let mut out = Vec::new();
            pak.copy_entry(entry, &mut out).unwrap();
            assert_eq!(&out, data);
        }
    }

    #[test]
    fn rejects_wrong_magic() {
        // Sixteen zero bytes once decoded.
        let buf = vec![XOR_KEY; 16];
        assert!(matches!(
            PakReader::new(Cursor::new(buf)),
            Err(Error::Pak(_))
        ));
    }

    #[test]
    fn rejects_truncated_data() {
        let mut buf = Vec::new();
        write_pak(&[("a", b"hello world")], &mut buf).unwrap();
        buf.truncate(buf.len() - 3);
        assert!(PakReader::new(Cursor::new(buf)).is_err());
    }
}
