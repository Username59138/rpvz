//! GOTY additions taken from the Android release and converted to PC units.
//!
//! The PC demo is version 1.0. Content added later (Disco Zombie and his
//! backup dancers, achievements, holiday decorations, a few signs and props)
//! exists in the Android data, which is a rescaled copy of the PC art:
//! reanim coordinates are PC × 8/15 and the 960 sprites are PC × 16/15. This
//! module selects what the PC data lacks and converts it back:
//!
//! - reanims: positions × 15/8, image numbers → names, written in the PC
//!   compiled format;
//! - images: cut from the 960 atlases, scaled × 15/16, written as PNG;
//! - `resources.json`: id → file for everything in this layer.
//!
//! Everything runs on the user's machine from files they downloaded; nothing
//! of the game ships with RPvZ.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use regex::Regex;
use rpvz_formats::reanim::write_compiled;
use rpvz_formats::rsb::{EntryKind, ImageKind, Resource, ResourceKind};
use rpvz_formats::{ImageIdTable, ImageRef, Reanim, RgbaImage, Rsb, Variant, ptx};
use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::manifest::FileEntry;
use crate::progress::{Progress, ProgressFn, Stage};
use crate::util::sha256_file;

/// Android `compiled32` reanim coordinates → PC.
pub const COORD_SCALE: f32 = 15.0 / 8.0;
/// Android 960 sprite size → PC.
pub const SPRITE_SCALE: f64 = 15.0 / 16.0;

pub const INDEX_FILE: &str = "resources.json";

/// `goty/resources.json`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GotyIndex {
    /// Image id → path relative to the GOTY dir.
    pub images: BTreeMap<String, String>,
    /// Reanim names (file stems) provided by this layer.
    pub reanims: Vec<String>,
    /// Images the GOTY reanims use that neither layer provides (mobile-only
    /// highlight images); the engine skips them.
    pub unresolved: Vec<String>,
}

/// Converts GOTY content from an extracted Android OBB (`android`) into `out`,
/// using the unpacked PC data in `pc` to decide what is missing.
pub fn convert(
    android: &Path,
    pc: &Path,
    out: &Path,
    progress: &mut ProgressFn<'_>,
) -> Result<Vec<FileEntry>> {
    progress(Progress::Stage(Stage::Extract));
    let pc_ids = pc_resource_ids(pc)?;
    let pc_reanims = stems(&pc.join("compiled/reanim"), ".reanim.compiled")?;

    let rsb_path = android.join("main.rsb");
    let rsb = Rsb::parse(std::fs::read(&rsb_path).at(&rsb_path)?)?;
    let groups = rsb.description()?;
    let table = ImageIdTable::from_description(&groups);

    // All Android reanims: what they reference, and which are new.
    let reanim_dir = android.join("compiled32/reanim");
    let mut android_reanims = BTreeMap::new();
    let mut referenced = HashSet::new();
    for stem in stems(&reanim_dir, ".reanim.compiled")? {
        let path = reanim_dir.join(format!("{stem}.reanim.compiled"));
        let r = Reanim::parse_compiled(&std::fs::read(&path).at(&path)?)?;
        for f in r.tracks.iter().flat_map(|t| &t.frames) {
            if let Some(ImageRef::Id(id)) = f.image
                && let Some(name) = table.name(id)
            {
                referenced.insert(name.to_owned());
            }
        }
        android_reanims.insert(stem, r);
    }
    let new_reanims: Vec<String> = android_reanims
        .keys()
        .filter(|s| !pc_reanims.contains(*s))
        .cloned()
        .collect();

    // Every non-atlas Android image at 960 (shared or English), with its texture.
    let mut all: BTreeMap<String, (&Resource, Option<String>)> = BTreeMap::new();
    // Images the PC data lacks that GOTY features need.
    let mut wanted_ids = BTreeSet::new();
    for group in &groups {
        for sub in &group.subgroups {
            if !matches!(sub.resolution, 0 | 960)
                || sub.locale.as_deref().is_some_and(|l| l != "ENUS")
            {
                continue;
            }
            let atlases: HashMap<&str, &str> = sub
                .resources
                .iter()
                .filter(|r| r.image.as_ref().is_some_and(|i| i.kind == ImageKind::Atlas))
                .map(|r| (r.base_id(), r.path.as_str()))
                .collect();
            for res in &sub.resources {
                let Some(img) = &res.image else { continue };
                let id = res.base_id();
                if res.kind != ResourceKind::Image || img.kind == ImageKind::Atlas {
                    continue;
                }
                let texture = match img.kind {
                    ImageKind::Sprite => img
                        .parent
                        .as_deref()
                        .and_then(|p| atlases.get(p))
                        .map(|p| (*p).to_owned()),
                    _ => Some(res.path.clone()),
                };
                // Prefer shared resources over the English-only copy.
                if sub.locale.is_none() || !all.contains_key(id) {
                    all.insert(id.to_owned(), (res, texture));
                }
                let mobile_only = id.contains("GLOW") || id.starts_with("IMAGE_CACHED_");
                let wanted = referenced.contains(id)
                    || group.id.contains("Achievement")
                    || id.contains("STOCKING")
                    || id.starts_with("IMAGE_PRESENT_");
                if wanted && !mobile_only && !pc_ids.contains(id) {
                    wanted_ids.insert(id.to_owned());
                }
            }
        }
    }

    // New reanims were laid out against the Android crops of their images,
    // so they get their own converted copy of every image they use, under a
    // private id, even if PC has an image of the same name.
    let mut private: HashMap<String, String> = HashMap::new();
    for stem in &new_reanims {
        for f in android_reanims[stem].tracks.iter().flat_map(|t| &t.frames) {
            if let Some(ImageRef::Id(id)) = f.image
                && let Some(name) = table.name(id)
                && !wanted_ids.contains(name)
                && all.contains_key(name)
            {
                private.insert(
                    name.to_owned(),
                    format!("IMAGE_GOTY_{}", name.trim_start_matches("IMAGE_")),
                );
            }
        }
    }
    let mut picks: BTreeMap<String, (&Resource, Option<String>)> = BTreeMap::new();
    for id in &wanted_ids {
        picks.insert(id.clone(), all[id].clone());
    }
    for (name, private_id) in &private {
        picks.insert(private_id.clone(), all[name].clone());
    }

    // Where each texture lives.
    let mut texture_packet = HashMap::new();
    for f in &rsb.files {
        texture_packet.insert(f.path.to_ascii_uppercase(), f.packet as usize);
    }

    let total = picks.len() + new_reanims.len();
    let mut done = 0;
    let mut files = Vec::new();
    let mut index = GotyIndex::default();
    let mut textures: HashMap<String, RgbaImage> = HashMap::new();

    for (id, (res, texture)) in &picks {
        let Some(texture) = texture else { continue };
        let tex_path = format!("{}.PTX", texture.to_ascii_uppercase());
        if !textures.contains_key(&tex_path) {
            let packet_index = *texture_packet
                .get(&tex_path)
                .ok_or_else(|| Error::NotFound {
                    what: "texture",
                    detail: tex_path.clone(),
                })?;
            let packet = rsb.packet(packet_index)?;
            let entry = packet.entry(&tex_path).ok_or_else(|| Error::NotFound {
                what: "texture",
                detail: tex_path.clone(),
            })?;
            let EntryKind::Texture { info, .. } = entry.kind else {
                return Err(Error::Image(format!("{tex_path} is not a texture")));
            };
            textures.insert(tex_path.clone(), ptx::decode(&info, packet.data(entry)?)?);
        }
        let tex = &textures[&tex_path];
        let img = res.image.as_ref().filter(|i| i.kind == ImageKind::Sprite);
        let sprite = match img {
            Some(i) => tex.crop(i.x.into(), i.y.into(), i.width.into(), i.height.into())?,
            None => tex.clone(),
        };
        let rel = format!("images/{}.png", id.to_ascii_lowercase());
        let dest = out.join(&rel);
        write_png(&scale(&sprite, SPRITE_SCALE)?, &dest)?;
        files.push(entry_for(&dest, &rel)?);
        index.images.insert(id.clone(), rel);
        done += 1;
        progress(Progress::Files { done, total });
    }

    let mut unresolved = BTreeSet::new();
    for stem in &new_reanims {
        let mut r = android_reanims[stem].clone();
        r.scale_positions(COORD_SCALE);
        r.name_images(|id| {
            let name = table
                .name(id)
                .map_or_else(|| format!("IMAGE_UNKNOWN_{id}"), str::to_owned);
            Some(private.get(&name).cloned().unwrap_or(name))
        })
        .map_err(|id| Error::NotFound {
            what: "reanim image",
            detail: format!("{stem}: id {id}"),
        })?;
        for name in r.image_names() {
            if !pc_ids.contains(name) && !index.images.contains_key(name) {
                unresolved.insert(name.to_owned());
            }
        }
        let rel = format!("compiled/reanim/{stem}.reanim.compiled");
        let dest = out.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).at(parent)?;
        }
        std::fs::write(&dest, write_compiled(&r, Variant::Pc)?).at(&dest)?;
        files.push(entry_for(&dest, &rel)?);
        index.reanims.push(stem.clone());
        done += 1;
        progress(Progress::Files { done, total });
    }
    index.unresolved = unresolved.into_iter().collect();

    let dest = out.join(INDEX_FILE);
    let json = serde_json::to_string_pretty(&index).map_err(|e| Error::Image(e.to_string()))?;
    std::fs::write(&dest, json).at(&dest)?;
    files.push(entry_for(&dest, INDEX_FILE)?);
    Ok(files)
}

/// Every resource id the PC data provides: `resources.xml` entries, images the
/// reanim system finds by file name, and names used inside compiled files.
pub fn pc_resource_ids(pc: &Path) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    let xml_path = pc.join("properties/resources.xml");
    let xml = std::fs::read(&xml_path).at(&xml_path)?;
    let xml = String::from_utf8_lossy(&xml);
    let re =
        Regex::new(r#"<SetDefaults[^>]*idprefix="([^"]*)"|<(?:Image|Sound|Font)\s+id="([^"]+)""#)
            .map_err(|e| Error::Image(e.to_string()))?;
    let mut prefix = String::new();
    for c in re.captures_iter(&xml) {
        if let Some(p) = c.get(1) {
            prefix = p.as_str().to_owned();
        } else if let Some(id) = c.get(2) {
            ids.insert(format!("{prefix}{}", id.as_str()));
        }
    }
    for dir in ["reanim", "images"] {
        for stem in stems(&pc.join(dir), "")? {
            if let Some((name, _ext)) = stem.rsplit_once('.') {
                ids.insert(format!("IMAGE_REANIM_{}", name.to_ascii_uppercase()));
            }
        }
    }
    let names =
        regex::bytes::Regex::new(r"IMAGE_[A-Z0-9_]+").map_err(|e| Error::Image(e.to_string()))?;
    for sub in ["compiled/reanim", "compiled/particles"] {
        let dir = pc.join(sub);
        for stem in stems(&dir, "")? {
            let path = dir.join(&stem);
            let data = std::fs::read(&path).at(&path)?;
            if data.len() < 8 {
                continue;
            }
            let mut plain = Vec::new();
            use std::io::Read;
            if flate2::read::ZlibDecoder::new(&data[8..])
                .read_to_end(&mut plain)
                .is_ok()
            {
                for m in names.find_iter(&plain) {
                    ids.insert(String::from_utf8_lossy(m.as_bytes()).into_owned());
                }
            }
        }
    }
    Ok(ids)
}

/// File names in `dir` ending with `suffix`, with the suffix removed.
fn stems(dir: &Path, suffix: &str) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for e in std::fs::read_dir(dir).at(dir)? {
        let e = e.at(dir)?;
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(suffix)
            && e.file_type().at(e.path())?.is_file()
        {
            out.insert(stem.to_owned());
        }
    }
    Ok(out)
}

/// Resizes with premultiplied alpha so transparent edges don't darken.
pub fn scale(img: &RgbaImage, factor: f64) -> Result<RgbaImage> {
    let w = ((f64::from(img.width) * factor).round() as u32).max(1);
    let h = ((f64::from(img.height) * factor).round() as u32).max(1);
    let mut pre = img.pixels.clone();
    for px in pre.chunks_exact_mut(4) {
        let a = u16::from(px[3]);
        for c in &mut px[..3] {
            *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
    }
    let buf = image::RgbaImage::from_raw(img.width, img.height, pre)
        .ok_or_else(|| Error::Image("pixel buffer size mismatch".into()))?;
    let mut out =
        image::imageops::resize(&buf, w, h, image::imageops::FilterType::Lanczos3).into_raw();
    for px in out.chunks_exact_mut(4) {
        let a = u16::from(px[3]);
        for c in &mut px[..3] {
            // Fully transparent pixels keep their (zero) colour.
            if let Some(v) = (u16::from(*c) * 255 + a / 2).checked_div(a) {
                *c = v.min(255) as u8;
            }
        }
    }
    Ok(RgbaImage {
        width: w,
        height: h,
        pixels: out,
    })
}

fn write_png(img: &RgbaImage, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).at(parent)?;
    }
    image::save_buffer_with_format(
        dest,
        &img.pixels,
        img.width,
        img.height,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| Error::Image(format!("{}: {e}", dest.display())))
}

fn entry_for(path: &Path, rel: &str) -> Result<FileEntry> {
    let size = std::fs::metadata(path).at(path)?.len();
    Ok(FileEntry {
        path: rel.to_owned(),
        size,
        sha256: sha256_file(path)?,
    })
}

/// The GOTY dir's index, if present.
pub fn load_index(goty: &Path) -> Result<GotyIndex> {
    let path: PathBuf = goty.join(INDEX_FILE);
    let text = std::fs::read_to_string(&path).at(&path)?;
    serde_json::from_str(&text).map_err(|e| Error::Manifest {
        path,
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_keeps_transparent_edges_clean() {
        // Opaque red next to fully transparent black: no dark fringe.
        let mut pixels = Vec::new();
        for x in 0..16 {
            pixels.extend_from_slice(if x < 8 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 0, 0]
            });
        }
        let img = RgbaImage {
            width: 16,
            height: 1,
            pixels,
        };
        let out = scale(&img, 15.0 / 16.0).unwrap();
        assert_eq!((out.width, out.height), (15, 1));
        for px in out.pixels.chunks_exact(4).filter(|p| p[3] > 16) {
            assert!(
                px[0] > 200 && px[1] < 40 && px[2] < 40,
                "fringe pixel {px:?}"
            );
        }
    }

    #[test]
    fn pc_ids_from_resources_and_files() {
        let tmp = tempfile::tempdir().unwrap();
        let pc = tmp.path();
        std::fs::create_dir_all(pc.join("properties")).unwrap();
        std::fs::create_dir_all(pc.join("reanim")).unwrap();
        std::fs::write(
            pc.join("properties/resources.xml"),
            r#"<SetDefaults path="images" idprefix="IMAGE_" /><Image id="SEEDBANK" path="SeedBank" /><SetDefaults path="sounds" idprefix="SOUND_" /><Sound id="BUTTONCLICK" path="buttonclick" />"#,
        )
        .unwrap();
        std::fs::write(pc.join("reanim/peashooter_head.png"), b"").unwrap();
        let ids = pc_resource_ids(pc).unwrap();
        for id in [
            "IMAGE_SEEDBANK",
            "SOUND_BUTTONCLICK",
            "IMAGE_REANIM_PEASHOOTER_HEAD",
        ] {
            assert!(ids.contains(id), "{id} in {ids:?}");
        }
    }
}
