//! Text drawn with the game's own image fonts.
//!
//! A [`PopText`] entity's `Transform` is the pen origin on the baseline of the
//! first line (left end or centre, per [`TextAlign`]). Its glyphs live in one
//! mesh per font layer, rebuilt only when the text changes, so moving or
//! rotating text costs nothing.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use rpvz_formats::ImageFont;

use crate::config::Config;
use crate::library::{Library, decode_font_page, font_page_path, image_size, to_bevy_image};

pub struct TextPlugin;

impl Plugin for TextPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FontCache>().add_systems(
            PostUpdate,
            build_text_meshes.before(TransformSystems::Propagate),
        );
    }
}

pub struct LoadedFont {
    pub font: ImageFont,
    /// Per layer: page texture (none in box mode), page size, colour.
    pages: Vec<(Option<Handle<Image>>, UVec2, Color)>,
}

impl LoadedFont {
    pub fn width(&self, text: &str) -> f32 {
        text.lines()
            .map(|l| self.font.text_width(l))
            .max()
            .unwrap_or(0) as f32
    }

    pub fn ascent(&self) -> f32 {
        self.font.ascent() as f32
    }

    pub fn line_height(&self) -> f32 {
        self.font.line_height().max(1) as f32
    }
}

#[derive(Resource, Default)]
pub struct FontCache {
    fonts: HashMap<String, Option<Arc<LoadedFont>>>,
}

impl FontCache {
    pub fn get(
        &mut self,
        id: &str,
        library: &Library,
        images: &mut Assets<Image>,
        load_pixels: bool,
    ) -> Option<Arc<LoadedFont>> {
        if let Some(f) = self.fonts.get(id) {
            return f.clone();
        }
        let loaded = load_font(id, library, images, load_pixels)
            .map_err(|e| warn!("font {id}: {e}"))
            .ok()
            .map(Arc::new);
        self.fonts.insert(id.to_owned(), loaded.clone());
        loaded
    }
}

fn load_font(
    id: &str,
    library: &Library,
    images: &mut Assets<Image>,
    load_pixels: bool,
) -> Result<LoadedFont, String> {
    let path = library.font_path(id).ok_or("not found")?;
    let font = ImageFont::parse(&std::fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut pages = Vec::new();
    for layer in &font.layers {
        let (page, alpha_only) = font_page_path(&path, &layer.image)
            .ok_or_else(|| format!("no page {}", layer.image))?;
        let [r, g, b, a] = layer.color_mult.map(|c| c.clamp(0, 255) as f32 / 255.0);
        let tint = Color::srgba(r, g, b, a);
        if load_pixels {
            let rgba = decode_font_page(&page, alpha_only)?;
            let size = UVec2::new(rgba.width(), rgba.height());
            pages.push((Some(images.add(to_bevy_image(rgba))), size, tint));
        } else {
            pages.push((None, image_size(&page)?, tint));
        }
    }
    Ok(LoadedFont { font, pages })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
}

#[derive(Component, Clone, Debug)]
#[require(Transform, Visibility)]
pub struct PopText {
    pub font: String,
    pub text: String,
    pub color: Color,
    pub align: TextAlign,
}

impl PopText {
    pub fn new(font: &str, text: impl Into<String>, color: Color, align: TextAlign) -> Self {
        Self {
            font: font.into(),
            text: text.into(),
            color,
            align,
        }
    }
}

#[derive(Component)]
struct GlyphMesh;

#[allow(clippy::too_many_arguments)]
fn build_text_meshes(
    mut commands: Commands,
    cfg: Res<Config>,
    library: Res<Library>,
    mut fonts: ResMut<FontCache>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    texts: Query<(Entity, &PopText, Option<&Children>), Changed<PopText>>,
    old: Query<(), With<GlyphMesh>>,
) {
    for (entity, text, children) in &texts {
        for child in children.into_iter().flatten() {
            if old.contains(*child) {
                commands.entity(*child).despawn();
            }
        }
        let Some(font) = fonts.get(&text.font, &library, &mut images, !cfg.boxes) else {
            continue;
        };
        for (i, layer) in font.font.layers.iter().enumerate() {
            let (texture, page, tint) = &font.pages[i];
            let mut positions = Vec::new();
            let mut uvs = Vec::new();
            let mut indices = Vec::new();
            for (line_no, line) in text.text.split('\n').enumerate() {
                let baseline = -(line_no as f32) * font.line_height();
                let shift = match text.align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => -(font.font.text_width(line) as f32) / 2.0,
                };
                for (c, pen) in font.font.layout(line) {
                    let Some(g) = layer.glyphs.get(&c) else {
                        continue;
                    };
                    let [rx, ry, rw, rh] = g.rect.map(|v| v as f32);
                    if rw <= 0.0 || rh <= 0.0 {
                        continue;
                    }
                    let left = shift + pen as f32 + g.offset[0] as f32;
                    let top = baseline + layer.ascent as f32 - g.offset[1] as f32;
                    let base = positions.len() as u32;
                    positions.extend([
                        [left, top, 0.0],
                        [left + rw, top, 0.0],
                        [left + rw, top - rh, 0.0],
                        [left, top - rh, 0.0],
                    ]);
                    let (pw, ph) = (page.x as f32, page.y as f32);
                    uvs.extend([
                        [rx / pw, ry / ph],
                        [(rx + rw) / pw, ry / ph],
                        [(rx + rw) / pw, (ry + rh) / ph],
                        [rx / pw, (ry + rh) / ph],
                    ]);
                    indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
                }
            }
            if positions.is_empty() {
                continue;
            }
            let normals = vec![[0.0f32, 0.0, 1.0]; positions.len()];
            let mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_indices(Indices::U32(indices));
            let color = multiply(text.color, *tint);
            let material = if cfg.boxes {
                ColorMaterial::from_color(color.with_alpha(0.5))
            } else {
                ColorMaterial {
                    color,
                    texture: texture.clone(),
                    ..default()
                }
            };
            commands.spawn((
                GlyphMesh,
                Mesh2d(meshes.add(mesh)),
                MeshMaterial2d(materials.add(material)),
                Transform::from_xyz(0.0, 0.0, i as f32 * 0.001),
                ChildOf(entity),
            ));
        }
    }
}

fn multiply(a: Color, b: Color) -> Color {
    let (a, b) = (a.to_srgba(), b.to_srgba());
    Color::srgba(
        a.red * b.red,
        a.green * b.green,
        a.blue * b.blue,
        a.alpha * b.alpha,
    )
}
