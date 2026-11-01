//! Small file helpers shared by the sources.

use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{IoContext, Result};

/// Copies `reader` into a new file at `dest` (creating parent dirs) and returns
/// the byte count and lowercase hex SHA-256 of what was written.
pub fn write_hashed(mut reader: impl Read, dest: &Path) -> Result<(u64, String)> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).at(parent)?;
    }
    let file = File::create(dest).at(dest)?;
    let mut writer = HashingWriter::new(BufWriter::new(file));
    io::copy(&mut reader, &mut writer).at(dest)?;
    let (inner, size, hash) = writer.finish();
    inner
        .into_inner()
        .map_err(|e| e.into_error())
        .and_then(|f| f.sync_all())
        .at(dest)?;
    Ok((size, hash))
}

/// Lowercase hex SHA-256 of a file.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).at(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).at(path)?;
    Ok(hex::encode(hasher.finalize()))
}

/// Writer adapter that hashes and counts everything passing through it.
pub struct HashingWriter<W> {
    inner: W,
    hasher: Sha256,
    size: u64,
}

impl<W: Write> HashingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            size: 0,
        }
    }

    pub fn finish(self) -> (W, u64, String) {
        (self.inner, self.size, hex::encode(self.hasher.finalize()))
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Removes a file or directory tree if it exists.
pub fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path).at(path),
        Ok(_) => std::fs::remove_file(path).at(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).at(path),
    }
}
