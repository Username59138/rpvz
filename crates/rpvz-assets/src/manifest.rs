//! `manifest.json`: what is installed, where it came from, and file hashes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, IoContext, Result};
use crate::util::sha256_file;

pub const MANIFEST_FILE: &str = "manifest.json";

/// Bump when the on-disk layout produced by the installer changes, so older
/// installs get re-extracted.
pub const LAYOUT_VERSION: u32 = 2;

/// What the install holds. PC data lives under `pc/` (`main.pak` contents),
/// GOTY additions converted from Android data under `goty/`, and a raw Android
/// install keeps the OBB layout (`compiled32/`, `main.rsb`, `music/*.sps`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// Full PC data from Steam (already GOTY).
    Pc,
    /// PC 1.0 data from the demo plus GOTY additions from the Android release.
    PcGoty,
    /// Android data only.
    Android,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Steam {
        install_dir: PathBuf,
    },
    ApkPure {
        package: String,
        version: String,
        sha256: String,
    },
    File {
        path: PathBuf,
        sha256: String,
    },
    /// The PC demo plus GOTY content converted from the Android release.
    DemoGoty {
        demo_sha256: String,
        apk_version: String,
        apk_sha256: String,
    },
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Steam { install_dir } => write!(f, "Steam ({})", install_dir.display()),
            Source::ApkPure {
                package, version, ..
            } => write!(f, "APKPure ({package} {version})"),
            Source::File { path, .. } => write!(f, "file ({})", path.display()),
            Source::DemoGoty { apk_version, .. } => {
                write!(f, "PC demo + GOTY from Android {apk_version}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative path with `/` separators.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub layout_version: u32,
    pub layout: Layout,
    pub source: Source,
    /// Hash over the sorted file list; identical game data gives the same digest
    /// regardless of which APK version or download it came from.
    pub content_digest: String,
    pub installed_unix: u64,
    pub files: Vec<FileEntry>,
}

impl Manifest {
    pub fn new(layout: Layout, source: Source, mut files: Vec<FileEntry>) -> Self {
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let installed_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            layout_version: LAYOUT_VERSION,
            layout,
            source,
            content_digest: content_digest(&files),
            installed_unix,
            files,
        }
    }

    pub fn is_current(&self) -> bool {
        self.layout_version == LAYOUT_VERSION
    }

    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).at(path)?;
        serde_json::from_str(&text).map_err(|e| Error::Manifest {
            path: path.to_owned(),
            reason: e.to_string(),
        })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(|e| Error::Manifest {
            path: path.to_owned(),
            reason: e.to_string(),
        })?;
        std::fs::write(path, text).at(path)
    }

    /// Re-hashes every file under `root` and reports anything missing or changed.
    pub fn verify(&self, root: &Path) -> Result<VerifyReport> {
        let mut report = VerifyReport::default();
        for entry in &self.files {
            let path = root.join(&entry.path);
            match std::fs::metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    report.missing.push(entry.path.clone());
                }
                Err(e) => return Err(e).at(&path),
                Ok(meta) if meta.len() != entry.size => report.corrupt.push(entry.path.clone()),
                Ok(_) => {
                    if sha256_file(&path)? != entry.sha256 {
                        report.corrupt.push(entry.path.clone());
                    } else {
                        report.ok += 1;
                    }
                }
            }
        }
        Ok(report)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub ok: usize,
    pub missing: Vec<String>,
    pub corrupt: Vec<String>,
}

impl VerifyReport {
    pub fn is_ok(&self) -> bool {
        self.missing.is_empty() && self.corrupt.is_empty()
    }
}

/// SHA-256 over `path\tsize\tsha256\n` lines of the files sorted by path.
pub fn content_digest(files: &[FileEntry]) -> String {
    let mut sorted: Vec<&FileEntry> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut hasher = Sha256::new();
    for f in sorted {
        hasher.update(format!("{}\t{}\t{}\n", f.path, f.size, f.sha256).as_bytes());
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, sha: &str) -> FileEntry {
        FileEntry {
            path: path.into(),
            size: 1,
            sha256: sha.into(),
        }
    }

    #[test]
    fn digest_ignores_order() {
        let a = vec![entry("a", "1"), entry("b", "2")];
        let b = vec![entry("b", "2"), entry("a", "1")];
        assert_eq!(content_digest(&a), content_digest(&b));
        assert_ne!(content_digest(&a), content_digest(&[entry("a", "1")]));
    }

    #[test]
    fn verify_detects_missing_and_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("good"), b"x").unwrap();
        std::fs::write(dir.path().join("bad"), b"y").unwrap();
        let x_hash = crate::util::sha256_file(&dir.path().join("good")).unwrap();
        let m = Manifest::new(
            Layout::Pc,
            Source::File {
                path: "t".into(),
                sha256: String::new(),
            },
            vec![
                FileEntry {
                    path: "good".into(),
                    size: 1,
                    sha256: x_hash.clone(),
                },
                FileEntry {
                    path: "bad".into(),
                    size: 1,
                    sha256: x_hash.clone(),
                },
                FileEntry {
                    path: "gone".into(),
                    size: 1,
                    sha256: x_hash,
                },
            ],
        );
        let r = m.verify(dir.path()).unwrap();
        assert_eq!(r.ok, 1);
        assert_eq!(r.corrupt, vec!["bad"]);
        assert_eq!(r.missing, vec!["gone"]);
    }
}
