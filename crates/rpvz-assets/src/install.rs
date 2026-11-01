//! Staging, atomic swap and status of the installed game data.

use std::path::Path;

use crate::error::{Error, IoContext, Result};
use crate::manifest::{MANIFEST_FILE, Manifest, VerifyReport};
use crate::paths::DataDirs;
use crate::progress::{Progress, ProgressFn, Stage};
use crate::source::{AssetSource, StageCtx};
use crate::steam::{self, SteamInstall};
use crate::util::remove_if_exists;

/// What is installed right now.
#[derive(Debug)]
pub enum Status {
    NotInstalled,
    /// Installed with an older layout; needs re-extraction.
    Outdated(Manifest),
    Installed(Manifest),
    /// The manifest exists but can't be read.
    Broken(Error),
}

pub struct Installer {
    dirs: DataDirs,
}

impl Installer {
    pub fn new(dirs: DataDirs) -> Self {
        Self { dirs }
    }

    pub fn dirs(&self) -> &DataDirs {
        &self.dirs
    }

    pub fn status(&self) -> Status {
        let path = self.dirs.manifest();
        if !path.exists() {
            return Status::NotInstalled;
        }
        match Manifest::load(&path) {
            Ok(m) if m.is_current() => Status::Installed(m),
            Ok(m) => Status::Outdated(m),
            Err(e) => Status::Broken(e),
        }
    }

    /// Re-hashes the installed files against the manifest.
    pub fn verify(&self, progress: &mut ProgressFn<'_>) -> Result<VerifyReport> {
        let manifest = Manifest::load(&self.dirs.manifest())?;
        progress(Progress::Stage(Stage::Verify));
        manifest.verify(&self.dirs.assets())
    }

    /// Stages `source` and swaps it in. The previous install stays untouched
    /// until the new one is complete.
    pub fn install(
        &self,
        source: &dyn AssetSource,
        progress: &mut ProgressFn<'_>,
    ) -> Result<Manifest> {
        let tmp = self.dirs.tmp();
        std::fs::create_dir_all(&tmp).at(&tmp)?;
        let staging = tmp.join(format!("staging-{}", std::process::id()));
        remove_if_exists(&staging)?;
        std::fs::create_dir_all(&staging).at(&staging)?;

        let result = self.stage_and_swap(source, &staging, &tmp, progress);
        // Clean up the staging dir on failure; on success it has been renamed.
        remove_if_exists(&staging)?;
        result
    }

    fn stage_and_swap(
        &self,
        source: &dyn AssetSource,
        staging: &Path,
        scratch: &Path,
        progress: &mut ProgressFn<'_>,
    ) -> Result<Manifest> {
        let staged = {
            let mut ctx = StageCtx {
                staging,
                scratch,
                progress,
            };
            source.stage(&mut ctx)?
        };
        progress(Progress::Stage(Stage::Finalize));
        if staged.files.is_empty() {
            return Err(Error::NotFound {
                what: "game files",
                detail: source.label(),
            });
        }
        let manifest = Manifest::new(staged.layout, staged.source, staged.files);
        manifest.save(&staging.join(MANIFEST_FILE))?;
        swap_in(staging, &self.dirs.assets(), scratch)?;
        Ok(manifest)
    }
}

/// Replaces `target` with `staging` using renames, restoring the old
/// directory if the second rename fails.
fn swap_in(staging: &Path, target: &Path, scratch: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).at(parent)?;
    }
    let old = scratch.join(format!("old-{}", std::process::id()));
    remove_if_exists(&old)?;
    let had_old = target.exists();
    if had_old {
        std::fs::rename(target, &old).at(target)?;
    }
    if let Err(e) = std::fs::rename(staging, target) {
        if had_old {
            let _ = std::fs::rename(&old, target);
        }
        return Err(e).at(target);
    }
    if had_old {
        remove_if_exists(&old)?;
    }
    Ok(())
}

/// Sources found without touching the network.
#[derive(Debug, Default)]
pub struct Detected {
    pub steam: Option<SteamInstall>,
}

pub fn detect() -> Detected {
    Detected {
        steam: steam::find_pvz(&steam::default_roots()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Layout;
    use crate::source::{ApkFileSource, tests::make_fake_apk};

    fn quiet() -> impl FnMut(Progress) {
        |_| {}
    }

    #[test]
    fn install_swap_and_reinstall() {
        let tmp = tempfile::tempdir().unwrap();
        let apk = make_fake_apk(tmp.path());
        let dirs = DataDirs::at(tmp.path().join("data"));
        let installer = Installer::new(dirs.clone());
        assert!(matches!(installer.status(), Status::NotInstalled));

        let mut src = ApkFileSource::new(&apk);
        src.known_digests.clear();
        let m = installer.install(&src, &mut quiet()).unwrap();
        assert_eq!(m.layout, Layout::Android);
        assert!(dirs.assets().join("main.rsb").is_file());
        assert!(matches!(installer.status(), Status::Installed(_)));
        assert!(installer.verify(&mut quiet()).unwrap().is_ok());

        // A stray file from the old install must not survive a reinstall.
        std::fs::write(dirs.assets().join("stray.txt"), b"x").unwrap();
        installer.install(&src, &mut quiet()).unwrap();
        assert!(!dirs.assets().join("stray.txt").exists());

        // Only the install remains: no staging or old dirs left behind.
        let leftovers: Vec<_> = std::fs::read_dir(dirs.tmp()).unwrap().collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn failed_install_keeps_previous() {
        let tmp = tempfile::tempdir().unwrap();
        let apk = make_fake_apk(tmp.path());
        let dirs = DataDirs::at(tmp.path().join("data"));
        let installer = Installer::new(dirs.clone());
        let mut good = ApkFileSource::new(&apk);
        good.known_digests.clear();
        installer.install(&good, &mut quiet()).unwrap();

        let bad = ApkFileSource {
            path: apk,
            delete_after: false,
            known_digests: vec!["nope".into()],
        };
        assert!(installer.install(&bad, &mut quiet()).is_err());
        assert!(matches!(installer.status(), Status::Installed(_)));
        assert!(dirs.assets().join("main.rsb").is_file());
    }

    #[test]
    fn verify_reports_tampering() {
        let tmp = tempfile::tempdir().unwrap();
        let apk = make_fake_apk(tmp.path());
        let dirs = DataDirs::at(tmp.path().join("data"));
        let installer = Installer::new(dirs.clone());
        let mut src = ApkFileSource::new(&apk);
        src.known_digests.clear();
        installer.install(&src, &mut quiet()).unwrap();
        std::fs::write(dirs.assets().join("main.rsb"), b"changed").unwrap();
        let report = installer.verify(&mut quiet()).unwrap();
        assert_eq!(report.corrupt, vec!["main.rsb"]);
    }
}
