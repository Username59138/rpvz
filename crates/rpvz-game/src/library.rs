//! Finds game files by resource id across the installed layers and loads
//! images the way the original does (JPG colour + `name_.png` alpha).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rpvz_formats::Reanim;

/// One directory laid out like the PC data (`reanim/`, `images/`,
/// `compiled/reanim/`, optional `properties/resources.xml`).
#[derive(Debug, Clone)]
struct Layer {
    dir: PathBuf,
    /// Ids from `resources.xml` or `resources.json`, mapped to a path without
    /// extension (relative to `dir`).
    ids: HashMap<String, String>,
    /// Font ids (`FONT_...`) mapped to descriptor paths relative to `dir`.
    fonts: HashMap<String, String>,
}

#[derive(Resource, Debug, Clone)]
pub struct Library {
    /// Highest priority first.
    layers: Vec<Layer>,
}

impl Library {
    /// Layers for an installed assets dir: `pc/` before `goty/`.
    pub fn from_install(assets: &Path) -> Result<Self, String> {
        let mut layers = Vec::new();
        let pc = assets.join("pc");
        if pc.is_dir() {
            let (ids, fonts) = read_resources_xml(&pc);
            layers.push(Layer {
                ids,
                fonts,
                dir: pc,
            });
        }
        let goty = assets.join("goty");
        if goty.is_dir() {
            layers.push(Layer {
                ids: read_goty_index(&goty),
                fonts: HashMap::new(),
                dir: goty,
            });
        }
        if layers.is_empty() {
            return Err(format!(
                "no PC data in {} (run `rpvz-assets install`)",
                assets.display()
            ));
        }
        Ok(Self { layers })
    }

    /// A single PC-style directory, e.g. a placeholder pack.
    pub fn from_dir(dir: &Path) -> Self {
        let (ids, fonts) = read_resources_xml(dir);
        Self {
            layers: vec![Layer {
                ids,
                fonts,
                dir: dir.to_owned(),
            }],
        }
    }

    /// File for an image id, trying each layer in order.
    pub fn image_path(&self, id: &str) -> Option<PathBuf> {
        for layer in &self.layers {
            let mut stems = Vec::new();
            if let Some(p) = layer.ids.get(id) {
                stems.push(p.clone());
            }
            if let Some(name) = id.strip_prefix("IMAGE_REANIM_") {
                let name = name.to_ascii_lowercase();
                stems.push(format!("reanim/{name}"));
                stems.push(format!("images/{name}"));
            }
            for stem in stems {
                for ext in ["png", "jpg", "gif"] {
                    let p = layer.dir.join(format!("{stem}.{ext}"));
                    if p.is_file() {
                        return Some(p);
                    }
                }
                let p = layer.dir.join(&stem);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        None
    }

    /// A file by relative path (lower-case), first layer that has it.
    pub fn file(&self, rel: &str) -> Option<PathBuf> {
        self.layers
            .iter()
            .map(|l| l.dir.join(rel))
            .find(|p| p.is_file())
    }

    /// Descriptor file of a font id such as `FONT_DWARVENTODCRAFT18`.
    pub fn font_path(&self, id: &str) -> Option<PathBuf> {
        self.layers
            .iter()
            .find_map(|l| l.fonts.get(id).map(|p| l.dir.join(p)))
            .filter(|p| p.is_file())
    }

    pub fn reanim_path(&self, name: &str) -> Option<PathBuf> {
        let file = format!(
            "compiled/reanim/{}.reanim.compiled",
            name.to_ascii_lowercase()
        );
        self.layers
            .iter()
            .map(|l| l.dir.join(&file))
            .find(|p| p.is_file())
    }

    pub fn load_reanim(&self, name: &str) -> Result<Arc<Reanim>, String> {
        let path = self
            .reanim_path(name)
            .ok_or_else(|| format!("reanim {name} not found"))?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Reanim::parse_compiled(&bytes)
            .map(Arc::new)
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// `resources.xml`: `<SetDefaults path=".." idprefix=".."/>` then
/// `<Image id=".." path=".."/>` and `<Font .../>` entries. Returns image and
/// font maps, both with lower-case paths.
fn read_resources_xml(dir: &Path) -> (HashMap<String, String>, HashMap<String, String>) {
    let (mut images, mut fonts) = (HashMap::new(), HashMap::new());
    let Ok(bytes) = std::fs::read(dir.join("properties/resources.xml")) else {
        return (images, fonts);
    };
    let text = String::from_utf8_lossy(&bytes);
    let attr = |tag: &str, name: &str| -> Option<String> {
        let key = format!("{name}=\"");
        let start = tag.find(&key)? + key.len();
        let end = tag[start..].find('"')? + start;
        Some(tag[start..end].to_owned())
    };
    let (mut prefix, mut base) = (String::new(), String::new());
    for tag in text.split('<').skip(1) {
        if tag.starts_with("SetDefaults") {
            prefix = attr(tag, "idprefix").unwrap_or_default();
            base = attr(tag, "path").unwrap_or_default();
            continue;
        }
        let is_image = tag.starts_with("Image");
        if !is_image && !tag.starts_with("Font") {
            continue;
        }
        if let (Some(id), Some(path)) = (attr(tag, "id"), attr(tag, "path")) {
            let path = path.replace('\\', "/").to_ascii_lowercase();
            let full = if base.is_empty() {
                path
            } else {
                format!("{}/{path}", base.to_ascii_lowercase())
            };
            let map = if is_image { &mut images } else { &mut fonts };
            map.insert(format!("{prefix}{id}"), full);
        }
    }
    (images, fonts)
}

fn read_goty_index(dir: &Path) -> HashMap<String, String> {
    rpvz_assets::goty::load_index(dir)
        .map(|idx| {
            idx.images
                .into_iter()
                .map(|(id, path)| {
                    let stem = path
                        .rsplit_once('.')
                        .map_or(path.clone(), |(s, _)| s.to_owned());
                    (id, stem)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Decodes an image file. JPGs get their alpha from a `name_.png` next to them.
pub fn decode_image(path: &Path) -> Result<image::RgbaImage, String> {
    let mut rgba = image::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .to_rgba8();
    // JPG (and some GIF) images keep their alpha in a `name_.png|gif` beside them.
    let has_alpha_companion = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("gif"));
    if has_alpha_companion && let Some(stem) = path.file_stem() {
        let stem = stem.to_string_lossy();
        let mask = ["png", "gif"]
            .iter()
            .map(|ext| path.with_file_name(format!("{stem}_.{ext}")))
            .find(|p| p.is_file());
        if let Some(mask) = mask {
            let alpha = image::open(&mask)
                .map_err(|e| format!("{}: {e}", mask.display()))?
                .to_luma8();
            if alpha.dimensions() == rgba.dimensions() {
                for (px, a) in rgba.pixels_mut().zip(alpha.pixels()) {
                    px.0[3] = a.0[0];
                }
            }
        }
    }
    bleed_alpha(&mut rgba, 4);
    Ok(rgba)
}

/// Gives fully transparent pixels the average colour of their visible
/// neighbours, spreading `passes` pixels outwards. Alpha is untouched.
///
/// The original art often stores white or black in transparent pixels;
/// bilinear filtering then blends that colour into the edges, which shows as
/// a light or dark outline when an image is scaled or placed off-grid.
pub fn bleed_alpha(img: &mut image::RgbaImage, passes: u32) {
    let (w, h) = img.dimensions();
    let mut known: Vec<bool> = img.pixels().map(|p| p.0[3] > 0).collect();
    for _ in 0..passes {
        let mut updates = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if known[(y * w + x) as usize] {
                    continue;
                }
                let (mut sum, mut n) = ([0u32; 3], 0u32);
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        if (dx, dy) == (0, 0)
                            || nx < 0
                            || ny < 0
                            || nx >= w as i32
                            || ny >= h as i32
                        {
                            continue;
                        }
                        if known[(ny as u32 * w + nx as u32) as usize] {
                            let p = img.get_pixel(nx as u32, ny as u32).0;
                            for c in 0..3 {
                                sum[c] += u32::from(p[c]);
                            }
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    updates.push((x, y, sum.map(|s| (s / n) as u8)));
                }
            }
        }
        if updates.is_empty() {
            break;
        }
        for (x, y, rgb) in updates {
            let p = img.get_pixel_mut(x, y);
            p.0[..3].copy_from_slice(&rgb);
            known[(y * w + x) as usize] = true;
        }
    }
}

/// A font page `stem` from the directory of `descriptor`: `stem.png|gif|jpg`
/// in colour (with the usual `_` alpha companion for JPG), or only an alpha
/// map `_stem.png|gif`, drawn white.
pub fn font_page_path(descriptor: &Path, stem: &str) -> Option<(PathBuf, bool)> {
    let dir = descriptor.parent()?;
    let stem = stem.to_ascii_lowercase();
    for ext in ["png", "gif", "jpg"] {
        let p = dir.join(format!("{stem}.{ext}"));
        if p.is_file() {
            return Some((p, false));
        }
    }
    for ext in ["png", "gif"] {
        let p = dir.join(format!("_{stem}.{ext}"));
        if p.is_file() {
            return Some((p, true));
        }
    }
    None
}

/// Decodes a font page; alpha-only pages become white with that alpha.
pub fn decode_font_page(path: &Path, alpha_only: bool) -> Result<image::RgbaImage, String> {
    if !alpha_only {
        return decode_image(path);
    }
    let luma = image::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .to_luma8();
    let (w, h) = luma.dimensions();
    Ok(image::RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([255, 255, 255, luma.get_pixel(x, y).0[0]])
    }))
}

/// Image size without decoding pixels.
pub fn image_size(path: &Path) -> Result<UVec2, String> {
    let (w, h) = image::image_dimensions(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(UVec2::new(w, h))
}

pub fn to_bevy_image(rgba: image::RgbaImage) -> Image {
    let (w, h) = rgba.dimensions();
    Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Loaded images by id, plus sizes (known even in box mode, where pixels are
/// never loaded).
#[derive(Resource, Default)]
pub struct ImageCache {
    entries: HashMap<String, Option<CachedImage>>,
}

#[derive(Clone)]
pub struct CachedImage {
    pub size: UVec2,
    /// `None` in box mode.
    pub handle: Option<Handle<Image>>,
}

impl ImageCache {
    pub fn get(
        &mut self,
        id: &str,
        library: &Library,
        images: &mut Assets<Image>,
        load_pixels: bool,
    ) -> Option<CachedImage> {
        if let Some(e) = self.entries.get(id) {
            return e.clone();
        }
        let entry = (|| {
            let path = library.image_path(id)?;
            let result = if load_pixels {
                decode_image(&path).map(|rgba| {
                    let size = UVec2::new(rgba.width(), rgba.height());
                    CachedImage {
                        size,
                        handle: Some(images.add(to_bevy_image(rgba))),
                    }
                })
            } else {
                image_size(&path).map(|size| CachedImage { size, handle: None })
            };
            result.map_err(|e| warn!("{e}")).ok()
        })();
        if entry.is_none() {
            warn!("image {id} not found");
        }
        self.entries.insert(id.to_owned(), entry.clone());
        entry
    }

    pub fn missing(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bleed_replaces_hidden_white_at_edges() {
        // 5x5: brown opaque centre, fully transparent white around it.
        let mut img = image::RgbaImage::from_pixel(5, 5, image::Rgba([255, 255, 255, 0]));
        img.put_pixel(2, 2, image::Rgba([100, 80, 60, 255]));
        bleed_alpha(&mut img, 4);
        for (x, y, p) in img.enumerate_pixels() {
            assert_eq!(&p.0[..3], &[100, 80, 60], "pixel {x},{y}");
            assert_eq!(
                p.0[3],
                if (x, y) == (2, 2) { 255 } else { 0 },
                "alpha kept at {x},{y}"
            );
        }
    }

    #[test]
    fn bleed_leaves_opaque_and_empty_images_alone() {
        let mut empty = image::RgbaImage::from_pixel(3, 3, image::Rgba([9, 9, 9, 0]));
        bleed_alpha(&mut empty, 4);
        assert!(empty.pixels().all(|p| p.0 == [9, 9, 9, 0]));
        let mut solid = image::RgbaImage::from_pixel(3, 3, image::Rgba([1, 2, 3, 200]));
        bleed_alpha(&mut solid, 4);
        assert!(solid.pixels().all(|p| p.0 == [1, 2, 3, 200]));
    }

    /// With real PC data: no transparent edge pixel of the name sign keeps the
    /// near-white colour that caused the outline.
    #[test]
    fn real_woodsign_edges_take_wood_colour() {
        let Some(dir) = std::env::var_os("RPVZ_TEST_PC_ASSETS").map(PathBuf::from) else {
            return;
        };
        let img = decode_image(&dir.join("reanim/selectorscreen_woodsign1.png")).unwrap();
        let (w, h) = img.dimensions();
        let opaque = |x: i64, y: i64| {
            x >= 0
                && y >= 0
                && x < i64::from(w)
                && y < i64::from(h)
                && img.get_pixel(x as u32, y as u32).0[3] > 0
        };
        let mut edge = 0;
        for (x, y, p) in img.enumerate_pixels() {
            let (x, y) = (i64::from(x), i64::from(y));
            if p.0[3] == 0
                && (opaque(x - 1, y) || opaque(x + 1, y) || opaque(x, y - 1) || opaque(x, y + 1))
            {
                edge += 1;
                let brightness = p.0[..3].iter().map(|&c| u32::from(c)).sum::<u32>() / 3;
                assert!(
                    brightness < 180,
                    "edge pixel {x},{y} still light: {:?}",
                    p.0
                );
            }
        }
        assert!(
            edge > 100,
            "expected an outline of transparent edge pixels, found {edge}"
        );
    }

    #[test]
    fn resources_xml_ids() {
        let tmp = std::env::temp_dir().join(format!("rpvz-lib-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("properties")).unwrap();
        std::fs::write(
            tmp.join("properties/resources.xml"),
            r#"<SetDefaults path="images" idprefix="IMAGE_" /><Image id="SEEDBANK" path="SeedBank" />"#,
        )
        .unwrap();
        std::fs::create_dir_all(tmp.join("images")).unwrap();
        std::fs::write(tmp.join("images/seedbank.png"), b"").unwrap();
        std::fs::create_dir_all(tmp.join("reanim")).unwrap();
        std::fs::write(tmp.join("reanim/foo_bar.jpg"), b"").unwrap();
        let lib = Library::from_dir(&tmp);
        assert_eq!(
            lib.image_path("IMAGE_SEEDBANK"),
            Some(tmp.join("images/seedbank.png"))
        );
        assert_eq!(
            lib.image_path("IMAGE_REANIM_FOO_BAR"),
            Some(tmp.join("reanim/foo_bar.jpg"))
        );
        assert_eq!(lib.image_path("IMAGE_NOPE"), None);
        std::fs::remove_dir_all(tmp).unwrap();
    }
}
