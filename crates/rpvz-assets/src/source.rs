//! Asset sources: things that can produce a set of game files.

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::{Path, PathBuf};

use crate::error::{Error, IoContext, Result};
use crate::manifest::{FileEntry, Layout, Source, content_digest};
use crate::pak::PakReader;
use crate::paths::{sanitize_relative, to_slash};
use crate::progress::{Progress, ProgressFn, Stage};
use crate::steam::SteamInstall;
use crate::util::{remove_if_exists, sha256_file, write_hashed};

/// Android package of the free PvZ release.
pub const ANDROID_PACKAGE: &str = "com.ea.game.pvzfree_row";
/// Name of the expansion file bundled inside the APK.
pub const ANDROID_OBB: &str = "main.0.com.ea.game.pvzfree_row.obb";

/// Content digests (see [`content_digest`]) of known-good Android game data.
/// The OBB contents are identical from 2.9.10 through 3.18.0.
pub const KNOWN_ANDROID_DIGESTS: &[&str] =
    &["a4166ca044e1809c35427f6b094e04eff6bf35fc8ef1d1ff5440d40d3321a8e0"];

/// Working directories handed to a source while it stages files.
pub struct StageCtx<'a> {
    /// Empty directory that must end up holding exactly the game files.
    pub staging: &'a Path,
    /// Directory for temporary and resumable files. Persists across runs.
    pub scratch: &'a Path,
    pub progress: &'a mut ProgressFn<'a>,
}

/// Result of a successful [`AssetSource::stage`].
#[derive(Debug)]
pub struct Staged {
    pub layout: Layout,
    pub source: Source,
    pub files: Vec<FileEntry>,
}

pub trait AssetSource {
    /// Human-readable description for logs and UI.
    fn label(&self) -> String;

    /// Writes the game files into `ctx.staging`.
    fn stage(&self, ctx: &mut StageCtx<'_>) -> Result<Staged>;
}

/// Extracts `main.pak` (plus the loose `properties/` folder) from a Steam install.
pub struct SteamSource {
    pub install: SteamInstall,
}

impl AssetSource for SteamSource {
    fn label(&self) -> String {
        format!("Steam ({})", self.install.dir.display())
    }

    fn stage(&self, ctx: &mut StageCtx<'_>) -> Result<Staged> {
        (ctx.progress)(Progress::Stage(Stage::Extract));
        let files = unpack_pc(
            &self.install.pak,
            Some(&self.install.dir.join("properties")),
            &ctx.staging.join(PC_DIR),
            ctx.progress,
        )?;
        Ok(Staged {
            layout: Layout::Pc,
            source: Source::Steam {
                install_dir: self.install.dir.clone(),
            },
            files: prefixed(PC_DIR, files),
        })
    }
}

/// Install subdirectory holding PC data (`main.pak` contents).
pub const PC_DIR: &str = "pc";
/// Install subdirectory holding GOTY additions converted from Android data.
pub const GOTY_DIR: &str = "goty";

/// Prepends `dir/` to every entry path.
pub(crate) fn prefixed(dir: &str, files: Vec<FileEntry>) -> Vec<FileEntry> {
    files
        .into_iter()
        .map(|f| FileEntry {
            path: format!("{dir}/{}", f.path),
            ..f
        })
        .collect()
}

/// Unpacks a PC `main.pak` into `dest`, plus loose files from `loose_root`
/// (which override packed ones, as in the game). Paths are lower-cased: the
/// game was written for case-insensitive Windows file systems.
pub(crate) fn unpack_pc(
    pak_path: &Path,
    loose_root: Option<&Path>,
    dest: &Path,
    progress: &mut ProgressFn<'_>,
) -> Result<Vec<FileEntry>> {
    let mut pak = PakReader::new(BufReader::new(File::open(pak_path).at(pak_path)?))?;
    let entries = pak.entries().to_vec();
    let loose = match loose_root {
        Some(root) => list_files(root)?,
        None => Vec::new(),
    };
    let total = entries.len() + loose.len();

    let mut files: Vec<FileEntry> = Vec::with_capacity(total);
    for (i, entry) in entries.iter().enumerate() {
        let rel = sanitize_relative(&entry.name.to_ascii_lowercase())?;
        let out = dest.join(&rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).at(parent)?;
        }
        let file = File::create(&out).at(&out)?;
        let mut writer = crate::util::HashingWriter::new(std::io::BufWriter::new(file));
        pak.copy_entry(entry, &mut writer)?;
        let (inner, size, sha256) = writer.finish();
        inner.into_inner().map_err(|e| e.into_error()).at(&out)?;
        files.push(FileEntry {
            path: to_slash(&rel),
            size,
            sha256,
        });
        progress(Progress::Files { done: i + 1, total });
    }

    if let Some(root) = loose_root {
        for (i, rel) in loose.iter().enumerate() {
            let src = root.join(rel);
            let rel = Path::new("properties")
                .join(sanitize_relative(&to_slash(rel).to_ascii_lowercase())?);
            let (size, sha256) = write_hashed(File::open(&src).at(&src)?, &dest.join(&rel))?;
            let path = to_slash(&rel);
            files.retain(|f| f.path != path);
            files.push(FileEntry { path, size, sha256 });
            progress(Progress::Files {
                done: entries.len() + i + 1,
                total,
            });
        }
    }
    Ok(files)
}

/// Relative paths of all regular files under `root` (empty if it doesn't exist).
fn list_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).at(&dir)? {
            let entry = entry.at(&dir)?;
            let path = entry.path();
            let kind = entry.file_type().at(&path)?;
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file()
                && let Ok(rel) = path.strip_prefix(root)
            {
                out.push(rel.to_path_buf());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Extracts the game data from an Android `.apk` or `.xapk` on disk.
pub struct ApkFileSource {
    pub path: PathBuf,
    /// Delete the input file once it has been extracted successfully.
    pub delete_after: bool,
    /// Accepted content digests. Empty means "accept anything".
    pub known_digests: Vec<String>,
}

impl ApkFileSource {
    /// Source for a user-supplied file, checked against the known Android data.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            delete_after: false,
            known_digests: KNOWN_ANDROID_DIGESTS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

impl AssetSource for ApkFileSource {
    fn label(&self) -> String {
        format!("file ({})", self.path.display())
    }

    fn stage(&self, ctx: &mut StageCtx<'_>) -> Result<Staged> {
        (ctx.progress)(Progress::Stage(Stage::Verify));
        let input_sha = sha256_file(&self.path)?;

        (ctx.progress)(Progress::Stage(Stage::Extract));
        let obb = ctx.scratch.join("main.obb.tmp");
        let nested = ctx.scratch.join("base.apk.tmp");
        let result = (|| {
            find_obb(&self.path, &obb, &nested, 1)?;
            extract_zip(&obb, ctx.staging, ctx.progress)
        })();
        remove_if_exists(&obb)?;
        remove_if_exists(&nested)?;
        let files = result?;

        let digest = content_digest(&files);
        if !self.known_digests.is_empty() && !self.known_digests.contains(&digest) {
            return Err(Error::HashMismatch {
                what: "Android game data (unknown or modified APK)".into(),
                expected: self.known_digests.join(" or "),
                actual: digest,
            });
        }

        if self.delete_after {
            remove_if_exists(&self.path)?;
        }
        Ok(Staged {
            layout: Layout::Android,
            source: Source::File {
                path: self.path.clone(),
                sha256: input_sha,
            },
            files,
        })
    }
}

/// Copies the OBB out of `archive` into `obb_out`. Handles a plain APK
/// (`assets/<obb>`), an XAPK with the OBB under `Android/obb/`, and an XAPK
/// wrapping the base APK (recursing at most `depth` times).
fn find_obb(archive: &Path, obb_out: &Path, nested_tmp: &Path, depth: u32) -> Result<()> {
    let mut zip =
        zip::ZipArchive::new(BufReader::new(File::open(archive).at(archive)?)).at(archive)?;
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();

    let obb_name = names.iter().find(|n| {
        let lower = n.to_ascii_lowercase();
        lower == format!("assets/{ANDROID_OBB}")
            || (lower.starts_with("android/obb/")
                && lower.ends_with(&format!(".{ANDROID_PACKAGE}.obb")))
    });
    if let Some(name) = obb_name {
        let entry = zip.by_name(name).at(archive)?;
        write_hashed(entry, obb_out)?;
        return Ok(());
    }

    let base_apk = names
        .iter()
        .find(|n| *n == &format!("{ANDROID_PACKAGE}.apk"))
        .or_else(|| {
            names
                .iter()
                .find(|n| n.ends_with(".apk") && !n.contains("config."))
        });
    if let (Some(name), true) = (base_apk, depth > 0) {
        let entry = zip.by_name(name).at(archive)?;
        write_hashed(entry, nested_tmp)?;
        drop(zip);
        // Read the nested APK from a second temp name so the recursion can
        // reuse `nested_tmp` safely.
        let inner = nested_tmp.with_extension("inner");
        std::fs::rename(nested_tmp, &inner).at(nested_tmp)?;
        let r = find_obb(&inner, obb_out, nested_tmp, depth - 1);
        remove_if_exists(&inner)?;
        return r;
    }

    Err(Error::NotFound {
        what: "PvZ game data",
        detail: format!("{} contains no {ANDROID_OBB}", archive.display()),
    })
}

/// Extracts every file of a zip into `dest`, hashing as it goes.
fn extract_zip(path: &Path, dest: &Path, progress: &mut ProgressFn<'_>) -> Result<Vec<FileEntry>> {
    extract_zip_reader(
        BufReader::new(File::open(path).at(path)?),
        path,
        dest,
        progress,
    )
}

fn extract_zip_reader(
    reader: impl Read + Seek,
    path: &Path,
    dest: &Path,
    progress: &mut ProgressFn<'_>,
) -> Result<Vec<FileEntry>> {
    let mut zip = zip::ZipArchive::new(reader).at(path)?;
    let total = zip.len();
    let mut files = Vec::with_capacity(total);
    for i in 0..total {
        let entry = zip.by_index(i).at(path)?;
        if entry.is_dir() {
            continue;
        }
        let rel = sanitize_relative(entry.name())?;
        let (size, sha256) = write_hashed(entry, &dest.join(&rel))?;
        files.push(FileEntry {
            path: to_slash(&rel),
            size,
            sha256,
        });
        progress(Progress::Files { done: i + 1, total });
    }
    Ok(files)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    pub fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in files {
            if name.ends_with('/') {
                zip.add_directory(*name, SimpleFileOptions::default())
                    .unwrap();
            } else {
                zip.start_file(*name, SimpleFileOptions::default()).unwrap();
                zip.write_all(data).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    /// Fake APK holding a fake OBB, laid out like the real one.
    pub fn make_fake_apk(dir: &Path) -> PathBuf {
        let obb = dir.join("fake.obb");
        make_zip(
            &obb,
            &[
                ("compiled32/", b""),
                ("compiled32/reanim/a.reanim.compiled", b"reanim"),
                ("main.rsb", b"1bsr"),
            ],
        );
        let obb_bytes = std::fs::read(&obb).unwrap();
        let apk = dir.join("fake.apk");
        make_zip(
            &apk,
            &[
                ("classes.dex", b"dex"),
                (&format!("assets/{ANDROID_OBB}"), &obb_bytes),
            ],
        );
        apk
    }

    fn run(source: &dyn AssetSource, staging: &Path, scratch: &Path) -> Result<Staged> {
        let mut sink = |_: Progress| {};
        let mut ctx = StageCtx {
            staging,
            scratch,
            progress: &mut sink,
        };
        source.stage(&mut ctx)
    }

    #[test]
    fn extracts_apk_and_xapk() {
        let tmp = tempfile::tempdir().unwrap();
        let apk = make_fake_apk(tmp.path());
        let xapk = tmp.path().join("fake.xapk");
        make_zip(
            &xapk,
            &[
                ("manifest.json", b"{}"),
                ("config.arm64_v8a.apk", b"not this one"),
                (
                    &format!("{ANDROID_PACKAGE}.apk"),
                    &std::fs::read(&apk).unwrap(),
                ),
            ],
        );

        let mut digests = Vec::new();
        for input in [&apk, &xapk] {
            let staging = tempfile::tempdir().unwrap();
            let scratch = tempfile::tempdir().unwrap();
            let mut src = ApkFileSource::new(input.clone());
            src.known_digests.clear();
            let staged = run(&src, staging.path(), scratch.path()).unwrap();
            assert_eq!(staged.layout, Layout::Android);
            assert_eq!(staged.files.len(), 2);
            assert_eq!(
                std::fs::read(staging.path().join("compiled32/reanim/a.reanim.compiled")).unwrap(),
                b"reanim"
            );
            assert_eq!(
                std::fs::read_dir(scratch.path()).unwrap().count(),
                0,
                "scratch cleaned"
            );
            digests.push(content_digest(&staged.files));
        }
        assert_eq!(digests[0], digests[1], "same data, same digest");
    }

    #[test]
    fn rejects_unknown_content_and_keeps_input() {
        let tmp = tempfile::tempdir().unwrap();
        let apk = make_fake_apk(tmp.path());
        let staging = tempfile::tempdir().unwrap();
        let src = ApkFileSource {
            path: apk.clone(),
            delete_after: true,
            known_digests: vec!["0000".into()],
        };
        let err = run(&src, staging.path(), tmp.path()).unwrap_err();
        assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
        assert!(apk.exists(), "input must survive a failed install");
    }

    #[test]
    fn rejects_apk_without_obb() {
        let tmp = tempfile::tempdir().unwrap();
        let apk = tmp.path().join("other.apk");
        make_zip(&apk, &[("classes.dex", b"dex")]);
        let staging = tempfile::tempdir().unwrap();
        let err = run(&ApkFileSource::new(apk), staging.path(), tmp.path()).unwrap_err();
        assert!(matches!(err, Error::NotFound { .. }), "{err}");
    }

    #[test]
    fn rejects_path_traversal_in_obb() {
        let tmp = tempfile::tempdir().unwrap();
        let obb = tmp.path().join("evil.obb");
        make_zip(&obb, &[("../escape.txt", b"x")]);
        let staging = tmp.path().join("staging");
        std::fs::create_dir(&staging).unwrap();
        let mut sink = |_: Progress| {};
        let err = extract_zip(&obb, &staging, &mut sink).unwrap_err();
        assert!(matches!(err, Error::UnsafePath(_)), "{err}");
        assert!(!tmp.path().join("escape.txt").exists());
    }

    #[test]
    fn steam_source_extracts_pak_and_loose_files() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("game");
        std::fs::create_dir_all(game.join("properties")).unwrap();
        let pak = game.join("main.pak");
        let mut buf = Vec::new();
        crate::pak::write_pak(
            &[
                ("reanim\\Peashooter.reanim", b"<reanim/>"),
                ("properties\\default.xml", b"packed"),
            ],
            &mut buf,
        )
        .unwrap();
        std::fs::write(&pak, buf).unwrap();
        std::fs::write(game.join("properties/default.xml"), b"loose").unwrap();

        let staging = tempfile::tempdir().unwrap();
        let src = SteamSource {
            install: SteamInstall { dir: game, pak },
        };
        let staged = run(&src, staging.path(), tmp.path()).unwrap();
        assert_eq!(staged.layout, Layout::Pc);
        assert_eq!(staged.files.len(), 2);
        assert_eq!(
            std::fs::read(staging.path().join("pc/properties/default.xml")).unwrap(),
            b"loose"
        );
        assert!(
            staging.path().join("pc/reanim/peashooter.reanim").is_file(),
            "lower-cased under pc/"
        );
    }
}
