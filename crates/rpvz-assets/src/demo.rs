//! The official 2009 PC demo of Plants vs. Zombies.
//!
//! PopCap gave the demo away for free; its time limit lives in the executable,
//! which RPvZ never runs. The data inside (`main.pak`) is the complete 1.0 game.
//! PopCap's own download link is gone, so the installer comes from the Internet
//! Archive copy, pinned to a SHA-256. That file is Authenticode-signed by
//! "PopCap Games" (VeriSign, timestamped 2009-07-28).
//!
//! The installer is an InstallShield stub followed by a Microsoft cabinet
//! (LZX), from which only `main.pak` is taken.

use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::path::{Path, PathBuf};

use crate::apkpure::{agent, download, verify_and_finish};
use crate::error::{Error, IoContext, Result};
use crate::manifest::FileEntry;
use crate::progress::{Progress, ProgressFn, Stage};
use crate::source::unpack_pc;
use crate::util::{remove_if_exists, sha256_file, write_hashed};

pub const DEMO_URL: &str =
    "https://archive.org/download/PlantsVsZombies_EN/PlantsVsZombiesSetup-en_2009.exe";
pub const DEMO_SHA256: &str = "2f1249af3848a6bc02d75039e285fb0fa7ca0a53d7e5c6bdb81e38b31d5864d2";
pub const DEMO_SIZE: u64 = 27_951_128;

const CAB_SIGNATURE: &[u8] = b"MSCF\0\0\0\0";

#[derive(Debug, Clone)]
pub struct DemoSource {
    /// Use this installer instead of downloading.
    pub file: Option<PathBuf>,
    /// Expected SHA-256 of the installer; `None` skips the check (tests only).
    pub sha256: Option<String>,
}

impl Default for DemoSource {
    fn default() -> Self {
        Self {
            file: None,
            sha256: Some(DEMO_SHA256.into()),
        }
    }
}

impl DemoSource {
    pub fn label(&self) -> String {
        match &self.file {
            Some(f) => format!("PvZ demo ({})", f.display()),
            None => "PvZ demo (Internet Archive)".into(),
        }
    }

    /// Returns a verified installer path, downloading it into `scratch` if
    /// needed, and whether it may be deleted afterwards.
    pub fn fetch(&self, scratch: &Path, progress: &mut ProgressFn<'_>) -> Result<(PathBuf, bool)> {
        if let Some(file) = &self.file {
            if let Some(expected) = &self.sha256 {
                progress(Progress::Stage(Stage::Verify));
                let actual = sha256_file(file)?;
                if &actual != expected {
                    return Err(Error::HashMismatch {
                        what: format!("demo installer {}", file.display()),
                        expected: expected.clone(),
                        actual,
                    });
                }
            }
            return Ok((file.clone(), false));
        }
        let expected = self.sha256.as_deref().unwrap_or(DEMO_SHA256);
        let done = scratch.join("PlantsVsZombiesSetup.exe");
        if done.is_file() && sha256_file(&done)? == expected {
            return Ok((done, true));
        }
        remove_if_exists(&done)?;
        let part = done.with_extension("exe.part");
        progress(Progress::Stage(Stage::Download));
        download(&agent(), DEMO_URL, &part, progress)?;
        verify_and_finish(&part, &done, expected, "PvZ demo installer", progress)?;
        Ok((done, true))
    }

    /// Fetches the installer, unpacks its `main.pak` into `dest` and returns
    /// the installed files and the installer's SHA-256.
    pub fn install_into(
        &self,
        dest: &Path,
        scratch: &Path,
        progress: &mut ProgressFn<'_>,
    ) -> Result<(Vec<FileEntry>, String)> {
        let (installer, owned) = self.fetch(scratch, progress)?;
        let installer_sha = sha256_file(&installer)?;
        progress(Progress::Stage(Stage::Extract));
        let pak = scratch.join("demo-main.pak.tmp");
        let result =
            extract_main_pak(&installer, &pak).and_then(|()| unpack_pc(&pak, None, dest, progress));
        remove_if_exists(&pak)?;
        let files = result?;
        if owned {
            remove_if_exists(&installer)?;
        }
        Ok((files, installer_sha))
    }
}

/// Copies `main.pak` out of the cabinet embedded in the installer.
pub fn extract_main_pak(installer: &Path, out: &Path) -> Result<()> {
    let mut data = Vec::new();
    BufReader::new(File::open(installer).at(installer)?)
        .read_to_end(&mut data)
        .at(installer)?;
    let start = data
        .windows(CAB_SIGNATURE.len())
        .position(|w| w == CAB_SIGNATURE)
        .ok_or_else(|| Error::NotFound {
            what: "cabinet",
            detail: installer.display().to_string(),
        })?;
    let mut cab = cab::Cabinet::new(Cursor::new(&data[start..]))
        .map_err(|e| Error::Pak(format!("cabinet: {e}")))?;
    let name = cab
        .folder_entries()
        .flat_map(|f| f.file_entries())
        .map(|f| f.name().to_owned())
        .find(|n| {
            n.replace('\\', "/")
                .to_ascii_lowercase()
                .ends_with("/main.pak")
                || n.eq_ignore_ascii_case("main.pak")
        })
        .ok_or_else(|| Error::NotFound {
            what: "main.pak",
            detail: format!("{} has no main.pak", installer.display()),
        })?;
    let reader = cab
        .read_file(&name)
        .map_err(|e| Error::Pak(format!("cabinet: {e}")))?;
    write_hashed(reader, out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Fake installer: some PE-ish bytes, then a cabinet holding a tiny pak.
    pub fn make_fake_installer(dir: &Path) -> PathBuf {
        let mut pak = Vec::new();
        crate::pak::write_pak(
            &[
                ("Reanim\\Test.png", b"png"),
                ("Properties\\Resources.xml", b"<x/>"),
            ],
            &mut pak,
        )
        .unwrap();
        let mut builder = cab::CabinetBuilder::new();
        builder
            .add_folder(cab::CompressionType::None)
            .add_file("Plants vs. Zombies\\main.pak");
        let mut writer = builder.build(Cursor::new(Vec::new())).unwrap();
        while let Some(mut f) = writer.next_file().unwrap() {
            f.write_all(&pak).unwrap();
        }
        let cab = writer.finish().unwrap().into_inner();
        let mut exe = b"MZ\x90\x00 not really a PE ".to_vec();
        exe.extend_from_slice(&cab);
        let path = dir.join("setup.exe");
        std::fs::write(&path, exe).unwrap();
        path
    }

    #[test]
    fn unpacks_pak_from_installer() {
        let tmp = tempfile::tempdir().unwrap();
        let installer = make_fake_installer(tmp.path());
        let dest = tmp.path().join("pc");
        let src = DemoSource {
            file: Some(installer.clone()),
            sha256: None,
        };
        let mut sink = |_: Progress| {};
        let (files, sha) = src.install_into(&dest, tmp.path(), &mut sink).unwrap();
        assert_eq!(sha, sha256_file(&installer).unwrap());
        let mut paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        paths.sort();
        assert_eq!(
            paths,
            ["properties/resources.xml", "reanim/test.png"],
            "lower-cased"
        );
        assert_eq!(std::fs::read(dest.join("reanim/test.png")).unwrap(), b"png");
        assert!(installer.exists(), "user-supplied installer is kept");
        assert!(!tmp.path().join("demo-main.pak.tmp").exists());
    }

    #[test]
    fn rejects_wrong_hash_and_non_installers() {
        let tmp = tempfile::tempdir().unwrap();
        let installer = make_fake_installer(tmp.path());
        let mut sink = |_: Progress| {};
        let src = DemoSource {
            file: Some(installer),
            sha256: Some("00".into()),
        };
        assert!(matches!(
            src.install_into(&tmp.path().join("pc"), tmp.path(), &mut sink),
            Err(Error::HashMismatch { .. })
        ));
        let junk = tmp.path().join("junk.exe");
        std::fs::write(&junk, b"MZ nothing here").unwrap();
        assert!(matches!(
            extract_main_pak(&junk, &tmp.path().join("x")),
            Err(Error::NotFound { .. })
        ));
    }
}
