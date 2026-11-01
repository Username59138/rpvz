//! The default source without Steam: the PC demo (full 1.0 data, original PC
//! look) plus GOTY content converted from the Android release.

use std::path::PathBuf;

use crate::apkpure::ApkPureSource;
use crate::demo::DemoSource;
use crate::error::Result;
use crate::goty;
use crate::manifest::{Layout, Source};
use crate::source::{
    ApkFileSource, AssetSource, GOTY_DIR, KNOWN_ANDROID_DIGESTS, PC_DIR, StageCtx, Staged, prefixed,
};
use crate::util::{remove_if_exists, sha256_file};

#[derive(Debug, Clone, Default)]
pub struct PcGotySource {
    pub demo: DemoSource,
    pub apk: ApkPureSource,
    /// Use this `.apk`/`.xapk` instead of downloading from APKPure.
    pub apk_file: Option<PathBuf>,
}

impl PcGotySource {
    /// Approximate download size in bytes when nothing is supplied locally.
    pub fn download_size(&self) -> u64 {
        let demo = if self.demo.file.is_none() {
            crate::demo::DEMO_SIZE
        } else {
            0
        };
        let apk = if self.apk_file.is_none() {
            crate::apkpure::PINNED_SIZE
        } else {
            0
        };
        demo + apk
    }
}

impl AssetSource for PcGotySource {
    fn label(&self) -> String {
        let apk = match &self.apk_file {
            Some(p) => format!("file {}", p.display()),
            None => self.apk.label(),
        };
        format!("{} + GOTY from {apk}", self.demo.label())
    }

    fn stage(&self, ctx: &mut StageCtx<'_>) -> Result<Staged> {
        let pc = ctx.staging.join(PC_DIR);
        let (pc_files, demo_sha256) = self.demo.install_into(&pc, ctx.scratch, ctx.progress)?;

        let (apk_path, owned) = match &self.apk_file {
            Some(p) => (p.clone(), false),
            None => (self.apk.fetch(ctx.scratch, ctx.progress)?, true),
        };
        let apk_sha256 = sha256_file(&apk_path)?;

        let android = ctx.scratch.join("android-tmp");
        remove_if_exists(&android)?;
        std::fs::create_dir_all(&android).map_err(|source| crate::Error::Io {
            path: android.clone(),
            source,
        })?;
        let result = (|| {
            let apk = ApkFileSource {
                path: apk_path.clone(),
                delete_after: false,
                known_digests: KNOWN_ANDROID_DIGESTS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            };
            let mut sub = StageCtx {
                staging: &android,
                scratch: ctx.scratch,
                progress: &mut *ctx.progress,
            };
            apk.stage(&mut sub)?;
            goty::convert(&android, &pc, &ctx.staging.join(GOTY_DIR), ctx.progress)
        })();
        remove_if_exists(&android)?;
        let goty_files = result?;
        if owned {
            remove_if_exists(&apk_path)?;
        }

        let mut files = prefixed(PC_DIR, pc_files);
        files.extend(prefixed(GOTY_DIR, goty_files));
        Ok(Staged {
            layout: Layout::PcGoty,
            source: Source::DemoGoty {
                demo_sha256,
                apk_version: match &self.apk_file {
                    Some(_) => "local file".into(),
                    None => self.apk.version.clone(),
                },
                apk_sha256,
            },
            files,
        })
    }
}
