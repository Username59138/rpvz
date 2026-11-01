//! Downloading the free Android release from APKPure.
//!
//! The endpoint and headers are the ones APKPure's own app uses; see EFF's
//! `apkeep` (MIT) for the reference client. APKPure has been caught serving
//! modified apps, so the download is pinned to a known SHA-256 and nothing
//! from the APK is ever executed: only the game data is extracted.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;
use std::time::Duration;

use regex::bytes::Regex;

use crate::error::{Error, IoContext, Result};
use crate::manifest::Source;
use crate::progress::{Progress, ProgressFn, Stage};
use crate::source::{
    ANDROID_PACKAGE, ApkFileSource, AssetSource, KNOWN_ANDROID_DIGESTS, StageCtx, Staged,
};
use crate::util::{remove_if_exists, sha256_file};

const VERSIONS_URL: &str = "https://api.pureapk.com/m/v3/cms/app_version?hl=en-US&package_name=";

/// Pinned release: a single APK (no split configs), EA-signed.
pub const PINNED_VERSION: &str = "2.9.10";
pub const PINNED_SHA256: &str = "b15296d4609ee11f26f85463bf82f843cffcaa2b2afb1854971ba663c573bd72";
/// Rough size, for confirmation prompts.
pub const PINNED_SIZE: u64 = 99_768_432;

#[derive(Debug, Clone)]
pub struct ApkPureSource {
    pub package: String,
    pub version: String,
    pub sha256: String,
}

impl Default for ApkPureSource {
    fn default() -> Self {
        Self {
            package: ANDROID_PACKAGE.into(),
            version: PINNED_VERSION.into(),
            sha256: PINNED_SHA256.into(),
        }
    }
}

impl AssetSource for ApkPureSource {
    fn label(&self) -> String {
        format!("APKPure ({} {})", self.package, self.version)
    }

    fn stage(&self, ctx: &mut StageCtx<'_>) -> Result<Staged> {
        let apk = self.fetch(ctx.scratch, ctx.progress)?;
        let inner = ApkFileSource {
            path: apk,
            delete_after: true,
            known_digests: KNOWN_ANDROID_DIGESTS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };
        let mut staged = inner.stage(ctx)?;
        staged.source = Source::ApkPure {
            package: self.package.clone(),
            version: self.version.clone(),
            sha256: self.sha256.clone(),
        };
        Ok(staged)
    }
}

impl ApkPureSource {
    /// Downloads (or reuses) the pinned APK in `scratch` and returns its path.
    pub fn fetch(
        &self,
        scratch: &Path,
        progress: &mut ProgressFn<'_>,
    ) -> Result<std::path::PathBuf> {
        let apk = scratch.join(format!("{}-{}.apk", self.package, self.version));
        // A finished download from an earlier run that failed later is reused.
        if apk.is_file() && sha256_file(&apk)? == self.sha256 {
            return Ok(apk);
        }
        remove_if_exists(&apk)?;
        let part = apk.with_extension("apk.part");
        progress(Progress::Stage(Stage::Download));
        let agent = agent();
        let url = resolve_download_url(&agent, &self.package, &self.version)?;
        download(&agent, &url, &part, progress)?;
        verify_and_finish(
            &part,
            &apk,
            &self.sha256,
            &format!("{} {} from APKPure", self.package, self.version),
            progress,
        )?;
        Ok(apk)
    }
}

/// Checks `part` against `sha256` and renames it to `done`; deletes it on mismatch.
pub(crate) fn verify_and_finish(
    part: &Path,
    done: &Path,
    sha256: &str,
    what: &str,
    progress: &mut ProgressFn<'_>,
) -> Result<()> {
    progress(Progress::Stage(Stage::Verify));
    let actual = sha256_file(part)?;
    if actual != sha256 {
        remove_if_exists(part)?;
        return Err(Error::HashMismatch {
            what: what.to_owned(),
            expected: sha256.to_owned(),
            actual,
        });
    }
    std::fs::rename(part, done).at(part)
}

pub(crate) fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    ureq::Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .user_agent(concat!("rpvz-assets/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn api_get(agent: &ureq::Agent, url: &str) -> Result<ureq::http::Response<ureq::Body>> {
    agent
        .get(url)
        .header("x-cv", "3172501")
        .header("x-sv", "29")
        .header("x-abis", "arm64-v8a,armeabi-v7a,armeabi,x86,x86_64")
        .header("x-gp", "1")
        .call()
        .map_err(|e| Error::Http(e.to_string()))
}

/// Asks APKPure for the direct download link of `package` at `version`.
pub fn resolve_download_url(agent: &ureq::Agent, package: &str, version: &str) -> Result<String> {
    let mut resp = api_get(agent, &format!("{VERSIONS_URL}{package}"))?;
    if resp.status() != 200 {
        return Err(Error::Http(format!(
            "version list returned HTTP {}",
            resp.status()
        )));
    }
    let body = resp
        .body_mut()
        .read_to_vec()
        .map_err(|e| Error::Http(e.to_string()))?;
    find_download_url(&body, version).ok_or_else(|| Error::NotFound {
        what: "APKPure download",
        detail: format!("{package} {version}"),
    })
}

/// The response is a binary blob listing versions, each followed by an
/// `APKJ`/`XAPKJ` marker and a URL. Pull out the first URL after `version`.
fn find_download_url(body: &[u8], version: &str) -> Option<String> {
    let pattern = format!(
        r"(?s-u)(?:^|[^0-9.]){}:.+?X?APKJ..(https?://[-a-zA-Z0-9@:%._+~#=]{{1,256}}\.[a-zA-Z0-9()]{{1,6}}\b[-a-zA-Z0-9()@:%_+.~#?&/=]*)",
        regex::escape(version)
    );
    let re = Regex::new(&pattern).ok()?;
    let caps = re.captures(body)?;
    String::from_utf8(caps.get(1)?.as_bytes().to_vec()).ok()
}

/// Downloads `url` into `part`, resuming if `part` already holds a prefix.
pub(crate) fn download(
    agent: &ureq::Agent,
    url: &str,
    part: &Path,
    progress: &mut ProgressFn<'_>,
) -> Result<()> {
    if let Some(parent) = part.parent() {
        std::fs::create_dir_all(parent).at(parent)?;
    }
    let have = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    let mut req = agent.get(url);
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| Error::Http(e.to_string()))?;

    let (mut done, append) = match resp.status().as_u16() {
        200 => (0, false),
        206 => (have, true),
        // Our prefix is already the whole file.
        416 if have > 0 => return Ok(()),
        code => return Err(Error::Http(format!("download returned HTTP {code}"))),
    };
    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map(|len| len + done);

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(part)
        .at(part)?;
    let mut out = BufWriter::new(file);
    let mut body = resp.into_body().into_reader();
    let mut buf = vec![0; 256 * 1024];
    progress(Progress::Bytes { done, total });
    loop {
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Error::Http(format!("download interrupted: {e}"))),
        };
        out.write_all(&buf[..n]).at(part)?;
        done += n as u64;
        progress(Progress::Bytes { done, total });
    }
    let file: File = out.into_inner().map_err(|e| e.into_error()).at(part)?;
    file.sync_all().at(part)?;

    if let Some(total) = total
        && done != total
    {
        return Err(Error::Http(format!(
            "download ended at {done} of {total} bytes"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_url_for_requested_version() {
        let mut body = Vec::new();
        body.extend_from_slice(b"\x12\x063.18.0:(aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)\x05XAPKJ\x01\x02https://d.example.com/b/XAPK/new?x=1\x00");
        body.extend_from_slice(b"\x12\x062.9.10:(bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb)\x04APKJ\x03\x04https://d.example.com/b/APK/old?versionCode=2910\x00");
        body.extend_from_slice(
            b"\x12\x0612.9.10:(cccc)\x04APKJ\x03\x04https://d.example.com/wrong\x00",
        );

        assert_eq!(
            find_download_url(&body, "2.9.10").as_deref(),
            Some("https://d.example.com/b/APK/old?versionCode=2910")
        );
        assert_eq!(
            find_download_url(&body, "3.18.0").as_deref(),
            Some("https://d.example.com/b/XAPK/new?x=1")
        );
        assert_eq!(find_download_url(&body, "1.0.0"), None);
    }
}
