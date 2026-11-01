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
            layers.push(Layer {
                ids: read_resources_xml(&pc),
                dir: pc,
            });
        }
        let goty = assets.join("goty");
        if goty.is_dir() {
            layers.push(Layer {
                ids: read_goty_index(&goty),
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
        Self {
            layers: vec![Layer {
                ids: read_resources_xml(dir),
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
/// `<Image id=".." path=".."/>` entries.
fn read_resources_xml(dir: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(bytes) = std::fs::read(dir.join("properties/resources.xml")) else {
        return out;
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
        } else if tag.starts_with("Image") {
            if let (Some(id), Some(path)) = (attr(tag, "id"), attr(tag, "path")) {
                let path = path.replace('\\', "/").to_ascii_lowercase();
                let full = if base.is_empty() {
                    path
                } else {
                    format!("{}/{path}", base.to_ascii_lowercase())
                };
                out.insert(format!("{prefix}{id}"), full);
            }
        }
    }
    out
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
    let is_jpg = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg"));
    if is_jpg && let Some(stem) = path.file_stem() {
        let mask = path.with_file_name(format!("{}_.png", stem.to_string_lossy()));
        if mask.is_file() {
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
    Ok(rgba)
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
