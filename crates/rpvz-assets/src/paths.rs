//! Where RPvZ keeps its data on disk.

use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// Environment variable that overrides the data directory.
pub const DATA_DIR_ENV: &str = "RPVZ_DATA_DIR";

/// Directory layout under the RPvZ data root.
#[derive(Debug, Clone)]
pub struct DataDirs {
    root: PathBuf,
}

impl DataDirs {
    /// Uses `RPVZ_DATA_DIR` if set, otherwise the platform data directory
    /// (`~/.local/share/rpvz` on Linux).
    pub fn from_env() -> Result<Self> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::at(dir));
        }
        let dirs = directories::ProjectDirs::from("", "", "rpvz").ok_or(Error::NoDataDir)?;
        Ok(Self::at(dirs.data_dir()))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Installed game data. Holds `manifest.json` next to the files.
    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }

    /// Scratch space for downloads and staging. Lives on the same filesystem as
    /// [`Self::assets`] so the final swap is an atomic rename.
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    pub fn manifest(&self) -> PathBuf {
        self.assets().join(crate::manifest::MANIFEST_FILE)
    }
}

/// Turns an archive entry name (`a\b/c.txt`) into a relative path, rejecting
/// absolute paths and `..` so archives can't write outside the target dir.
pub fn sanitize_relative(name: &str) -> Result<PathBuf> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(Error::UnsafePath(name.to_owned()));
    }
    let mut out = PathBuf::new();
    for part in normalized.split('/').filter(|p| !p.is_empty() && *p != ".") {
        // Drive letters and alternate data streams are only meaningful on Windows,
        // but reject them everywhere so behaviour doesn't depend on the host.
        if part.contains(':') {
            return Err(Error::UnsafePath(name.to_owned()));
        }
        match Path::new(part).components().next() {
            Some(Component::Normal(c)) if Path::new(part).components().count() == 1 => out.push(c),
            _ => return Err(Error::UnsafePath(name.to_owned())),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(Error::UnsafePath(name.to_owned()));
    }
    Ok(out)
}

/// Relative path rendered with `/` on every platform, for manifests.
pub fn to_slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_accepts_nested_and_backslashes() {
        assert_eq!(
            sanitize_relative(r"reanim\Zombie.reanim").unwrap(),
            Path::new("reanim").join("Zombie.reanim")
        );
        assert_eq!(
            sanitize_relative("./a//b/").unwrap(),
            Path::new("a").join("b")
        );
    }

    #[test]
    fn sanitize_rejects_escapes() {
        for bad in ["../x", "a/../../x", "/etc/passwd", "", "C:\\x", ".."] {
            assert!(
                sanitize_relative(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }
}
