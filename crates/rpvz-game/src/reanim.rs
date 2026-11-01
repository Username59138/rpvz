//! Playing compiled reanims: one entity per instance, one quad mesh per track.
//!
//! A track frame maps the image's pixel rect through
//! `[a c x; b d y]` with `a = cos(-kx)·sx`, `b = -sin(-kx)·sx`,
//! `c = sin(-ky)·sy`, `d = cos(-ky)·sy` (angles in degrees, game coordinates:
//! 800×600, y down). Equal `kx`/`ky` is a plain rotation; different ones skew,
//! which is why tracks are quads and not sprites.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use rpvz_formats::reanim::{Frame, ImageRef, Reanim};

use crate::config::Config;
use crate::library::{ImageCache, Library};

pub const SCREEN: Vec2 = Vec2::new(800.0, 600.0);

pub struct ReanimPlugin;

impl Plugin for ReanimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ImageCache>()
            .init_resource::<Clock>()
            .add_systems(PreUpdate, tick_clock.in_set(ClockSet))
            .add_systems(Update, advance_players.in_set(ReanimSet::Advance))
            .add_systems(PostUpdate, draw_tracks.before(TransformSystems::Propagate))
            .configure_sets(Update, ReanimSet::Advance);
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClockSet;

pub fn tick_clock_set() -> ClockSet {
    ClockSet
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReanimSet {
    /// Animation time moves on; game logic that reads frames runs after this.
    Advance,
}

/// Animation time step. Fixed at 1/60 s in automation runs, so screenshots
/// don't depend on how fast the software renderer is.
#[derive(Resource, Default)]
pub struct Clock {
    pub dt: f32,
    pub elapsed: f32,
}

fn tick_clock(time: Res<Time>, cfg: Res<Config>, mut clock: ResMut<Clock>) {
    clock.dt = if cfg.automation.active() {
        1.0 / 60.0
    } else {
        time.delta_secs()
    };
    clock.elapsed += clock.dt;
}

/// A parsed reanim with frames resolved and animation ranges found.
pub struct ReanimDef {
    pub reanim: Arc<Reanim>,
    frames: Vec<Vec<Frame>>,
    ranges: HashMap<String, (usize, usize)>,
    /// Tracks that draw something (have an image on some frame).
    visual: Vec<usize>,
    /// First frame where a track explicitly turns itself on (`f >= 0`), for
    /// tracks that key their visibility at all.
    first_shown: Vec<Option<usize>>,
}

impl ReanimDef {
    pub fn new(reanim: Arc<Reanim>) -> Self {
        let frames: Vec<Vec<Frame>> = reanim.tracks.iter().map(|t| t.resolved()).collect();
        let mut ranges = HashMap::new();
        let mut visual = Vec::new();
        for (i, track) in reanim.tracks.iter().enumerate() {
            if track.name.starts_with("anim_") {
                // An animation is the run of frames where its marker track is shown.
                let shown: Vec<usize> = frames[i]
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| f.f >= 0.0)
                    .map(|(n, _)| n)
                    .collect();
                if let (Some(&a), Some(&b)) = (shown.first(), shown.last()) {
                    ranges.insert(track.name.clone(), (a, b));
                }
            } else if track.frames.iter().any(|f| f.image.is_some()) {
                visual.push(i);
            }
        }
        let first_shown = reanim
            .tracks
            .iter()
            .map(|t| t.frames.iter().position(|f| f.f.is_some_and(|v| v >= 0.0)))
            .collect();
        Self {
            reanim,
            frames,
            ranges,
            visual,
            first_shown,
        }
    }

    pub fn frame_count(&self) -> usize {
        self.frames.first().map_or(0, Vec::len)
    }

    pub fn range(&self, anim: &str) -> Option<(usize, usize)> {
        self.ranges.get(anim).copied()
    }

    pub fn track_index(&self, name: &str) -> Option<usize> {
        self.reanim
            .tracks
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(name))
    }

    pub fn track_name(&self, index: usize) -> &str {
        &self.reanim.tracks[index].name
    }
}

/// Playback state of one animation range.
#[derive(Debug, Clone)]
pub struct Layer {
    pub range: (usize, usize),
    /// Frames since the start of `range`.
    pub time: f32,
    pub looping: bool,
    /// A non-looping range that reached its last frame and holds it.
    pub finished: bool,
    /// Tracks of a layer that hasn't started are hidden.
    pub started: bool,
    /// Played once the current non-looping range ends.
    pub next: Option<(String, bool)>,
}

impl Layer {
    fn new(range: (usize, usize), looping: bool) -> Self {
        Self {
            range,
            time: 0.0,
            looping,
            finished: false,
            started: true,
            next: None,
        }
    }

    fn position(&self) -> f32 {
        let len = (self.range.1 - self.range.0 + 1) as f32;
        if self.looping {
            self.time.rem_euclid(len)
        } else {
            self.time.min(len - 1.0)
        }
    }
}

/// One reanim instance. Several layers can play different ranges of the same
/// definition at once (the main menu animates its sign, grass and clouds on
/// their own); each track follows exactly one layer.
#[derive(Component)]
pub struct ReanimPlayer {
    pub def: Arc<ReanimDef>,
    pub layers: Vec<Layer>,
    track_layer: Vec<usize>,
    /// Track index → image id shown instead of the track's own.
    pub image_overrides: HashMap<usize, String>,
    /// Tracks hidden regardless of their frames.
    pub hidden: Vec<usize>,
}

impl ReanimPlayer {
    /// Plays the whole definition on one layer.
    pub fn new(def: Arc<ReanimDef>) -> Self {
        let range = (0, def.frame_count().saturating_sub(1));
        let tracks = def.reanim.tracks.len();
        Self {
            def,
            layers: vec![Layer::new(range, true)],
            track_layer: vec![0; tracks],
            image_overrides: HashMap::new(),
            hidden: Vec::new(),
        }
    }

    /// Switches layer 0 to a named animation (an `anim_*` track).
    pub fn play(&mut self, anim: &str, looping: bool) -> bool {
        self.play_on(0, anim, looping)
    }

    pub fn play_on(&mut self, layer: usize, anim: &str, looping: bool) -> bool {
        let Some(range) = self.def.range(anim) else {
            warn!("no animation {anim}");
            return false;
        };
        let l = &mut self.layers[layer];
        *l = Layer {
            next: l.next.take(),
            ..Layer::new(range, looping)
        };
        true
    }

    /// Adds a layer playing `anim` and moves onto it every track that first
    /// appears inside that range (the smallest matching range wins, so nested
    /// ranges keep their own tracks). Returns the layer index.
    pub fn add_layer(&mut self, anim: &str, looping: bool, started: bool) -> Option<usize> {
        let range = self.def.range(anim)?;
        let index = self.layers.len();
        self.layers.push(Layer {
            started,
            ..Layer::new(range, looping)
        });
        for (track, first) in self.def.first_shown.iter().enumerate() {
            let Some(first) = *first else { continue };
            if self.def.reanim.tracks[track].name.starts_with("anim_")
                || !(range.0..=range.1).contains(&first)
            {
                continue;
            }
            let current = &self.layers[self.track_layer[track]];
            let len = |r: (usize, usize)| r.1 - r.0;
            if self.track_layer[track] == 0 || len(range) < len(current.range) {
                self.track_layer[track] = index;
            }
        }
        Some(index)
    }

    pub fn layer_of(&self, track: usize) -> &Layer {
        &self.layers[self.track_layer[track]]
    }

    /// Whether a track's layer is running at all.
    pub fn track_started(&self, track: usize) -> bool {
        self.layer_of(track).started
    }

    /// The interpolated frame of a track right now.
    pub fn frame(&self, track: usize) -> Frame {
        let layer = self.layer_of(track);
        let frames = &self.def.frames[track];
        let (start, end) = layer.range;
        let pos = layer.position();
        let i0 = start + pos.floor() as usize;
        let mut i1 = i0 + 1;
        if i1 > end {
            i1 = if layer.looping { start } else { end };
        }
        let t = pos.fract();
        let last = frames.len() - 1;
        let (a, b) = (&frames[i0.min(last)], &frames[i1.min(last)]);
        let lerp = |x: f32, y: f32| x + (y - x) * t;
        Frame {
            x: lerp(a.x, b.x),
            y: lerp(a.y, b.y),
            kx: lerp(a.kx, b.kx),
            ky: lerp(a.ky, b.ky),
            sx: lerp(a.sx, b.sx),
            sy: lerp(a.sy, b.sy),
            a: lerp(a.a, b.a),
            ..a.clone()
        }
    }

    /// Image id a track shows on this frame, after overrides.
    pub fn image_id(&self, track: usize, frame: &Frame) -> Option<String> {
        if let Some(id) = self.image_overrides.get(&track) {
            return Some(id.clone());
        }
        match &frame.image {
            Some(ImageRef::Name(n)) => Some(n.clone()),
            _ => None,
        }
    }
}

/// Where a reanim instance sits, in game coordinates.
#[derive(Component, Default, Clone, Copy)]
pub struct ReanimOrigin(pub Vec2);

#[derive(Component)]
pub struct TrackVisual {
    pub track: usize,
    mesh: Handle<Mesh>,
    material: Handle<ColorMaterial>,
}

#[derive(Component)]
pub struct TrackLabel;

/// Corners of an image of `size` placed by `frame`, in game coordinates:
/// top-left, top-right, bottom-right, bottom-left.
pub fn quad(frame: &Frame, size: Vec2, origin: Vec2) -> [Vec2; 4] {
    let (a, b, c, d) = matrix(frame);
    let map = |u: f32, v: f32| origin + Vec2::new(a * u + c * v + frame.x, b * u + d * v + frame.y);
    [
        map(0.0, 0.0),
        map(size.x, 0.0),
        map(size.x, size.y),
        map(0.0, size.y),
    ]
}

fn matrix(f: &Frame) -> (f32, f32, f32, f32) {
    let kx = (-f.kx).to_radians();
    let ky = (-f.ky).to_radians();
    (
        kx.cos() * f.sx,
        -kx.sin() * f.sx,
        ky.sin() * f.sy,
        ky.cos() * f.sy,
    )
}

/// Whether `point` (game coordinates) lies on the image placed by `frame`.
pub fn hit(frame: &Frame, size: Vec2, origin: Vec2, point: Vec2) -> bool {
    let (a, b, c, d) = matrix(frame);
    let det = a * d - b * c;
    if det.abs() < 1e-6 {
        return false;
    }
    let p = point - origin - Vec2::new(frame.x, frame.y);
    let u = (d * p.x - c * p.y) / det;
    let v = (-b * p.x + a * p.y) / det;
    (0.0..=size.x).contains(&u) && (0.0..=size.y).contains(&v)
}

/// Game coordinates (y down, origin top-left) → world (y up, origin centre).
pub fn to_world(p: Vec2) -> Vec2 {
    Vec2::new(p.x - SCREEN.x / 2.0, SCREEN.y / 2.0 - p.y)
}

pub fn to_game(world: Vec2) -> Vec2 {
    Vec2::new(world.x + SCREEN.x / 2.0, SCREEN.y / 2.0 - world.y)
}

/// Spawns a reanim instance with one child per visual track.
pub fn spawn_reanim(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    player: ReanimPlayer,
    origin: Vec2,
    z: f32,
    labels: bool,
) -> Entity {
    let def = player.def.clone();
    let root = commands
        .spawn((
            player,
            ReanimOrigin(origin),
            Transform::from_xyz(0.0, 0.0, z),
            Visibility::default(),
        ))
        .id();
    for (layer, &track) in def.visual.iter().enumerate() {
        let mesh = meshes.add(quad_mesh());
        let material = materials.add(ColorMaterial::from_color(Color::NONE));
        let child = commands
            .spawn((
                TrackVisual {
                    track,
                    mesh: mesh.clone(),
                    material: material.clone(),
                },
                Mesh2d(mesh),
                MeshMaterial2d(material),
                Transform::from_xyz(0.0, 0.0, layer as f32 * 0.01),
                Visibility::Hidden,
                ChildOf(root),
            ))
            .id();
        if labels {
            commands.spawn((
                TrackLabel,
                Text2d::new(def.track_name(track).to_owned()),
                TextFont::from_font_size(9.0),
                TextColor(Color::WHITE),
                bevy::sprite::Anchor::TOP_LEFT,
                Transform::from_xyz(0.0, 0.0, 50.0),
                ChildOf(child),
            ));
        }
    }
    root
}

fn quad_mesh() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, 0.0, 0.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 0.0, 1.0]; 4])
    .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
}

fn advance_players(clock: Res<Clock>, mut players: Query<&mut ReanimPlayer>) {
    for mut p in &mut players {
        let fps = p.def.reanim.fps.max(1.0);
        let def = p.def.clone();
        for layer in &mut p.layers {
            if layer.finished || !layer.started {
                continue;
            }
            layer.time += clock.dt * fps;
            let len = (layer.range.1 - layer.range.0 + 1) as f32;
            if !layer.looping && layer.time >= len - 1.0 {
                layer.time = len - 1.0;
                match layer
                    .next
                    .take()
                    .and_then(|(anim, looping)| Some((def.range(&anim)?, looping)))
                {
                    Some((range, looping)) => {
                        *layer = Layer {
                            started: true,
                            ..Layer::new(range, looping)
                        }
                    }
                    None => layer.finished = true,
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_tracks(
    cfg: Res<Config>,
    library: Res<Library>,
    mut cache: ResMut<ImageCache>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    players: Query<(&ReanimPlayer, &ReanimOrigin, &Children)>,
    mut tracks: Query<(&TrackVisual, &mut Visibility, Option<&Children>)>,
    mut labels: Query<&mut Transform, With<TrackLabel>>,
    mut gizmos: Gizmos,
) {
    for (player, origin, children) in &players {
        for child in children.iter() {
            let Ok((visual, mut vis, label_children)) = tracks.get_mut(child) else {
                continue;
            };
            let frame = player.frame(visual.track);
            let image = player.image_id(visual.track, &frame);
            let shown = frame.f >= 0.0
                && player.track_started(visual.track)
                && !player.hidden.contains(&visual.track);
            let cached = image
                .as_deref()
                .and_then(|id| cache.get(id, &library, &mut images, !cfg.boxes));
            let Some(cached) = cached.filter(|_| shown) else {
                *vis = Visibility::Hidden;
                continue;
            };
            *vis = Visibility::Inherited;

            let corners = quad(&frame, cached.size.as_vec2(), origin.0);
            if let Some(mut mesh) = meshes.get_mut(&visual.mesh) {
                let positions: Vec<[f32; 3]> = corners
                    .iter()
                    .map(|c| {
                        let w = to_world(*c);
                        [w.x, w.y, 0.0]
                    })
                    .collect();
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            }
            if let Some(mut mat) = materials.get_mut(&visual.material) {
                if cfg.boxes {
                    mat.texture = None;
                    mat.color =
                        track_color(visual.track).with_alpha(0.25 * frame.a.clamp(0.0, 1.0));
                } else {
                    mat.texture = cached.handle.clone();
                    mat.color = Color::WHITE.with_alpha(frame.a.clamp(0.0, 1.0));
                }
            }
            if cfg.boxes {
                let pts: Vec<Vec2> = corners
                    .iter()
                    .chain([&corners[0]])
                    .map(|c| to_world(*c))
                    .collect();
                gizmos.linestrip_2d(pts, track_color(visual.track));
            }
            if let Some(kids) = label_children {
                for k in kids.iter() {
                    if let Ok(mut t) = labels.get_mut(k) {
                        let w = to_world(corners[0]);
                        t.translation = Vec3::new(w.x + 2.0, w.y - 1.0, 50.0);
                    }
                }
            }
        }
    }
}

/// Stable, distinct colour per track index for box mode.
fn track_color(i: usize) -> Color {
    Color::hsl((i as f32 * 47.0) % 360.0, 0.75, 0.6)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: f32, y: f32, kx: f32, ky: f32, sx: f32, sy: f32) -> Frame {
        Frame {
            x,
            y,
            kx,
            ky,
            sx,
            sy,
            ..Frame::default()
        }
    }

    #[test]
    fn identity_and_translation() {
        let q = quad(
            &frame(10.0, 20.0, 0.0, 0.0, 1.0, 1.0),
            Vec2::new(4.0, 2.0),
            Vec2::ZERO,
        );
        assert_eq!(
            q,
            [
                Vec2::new(10.0, 20.0),
                Vec2::new(14.0, 20.0),
                Vec2::new(14.0, 22.0),
                Vec2::new(10.0, 22.0)
            ]
        );
    }

    #[test]
    fn rotation_by_90_turns_x_axis_down() {
        // Screen y points down, so +90° maps the image's x axis onto +y.
        let q = quad(
            &frame(0.0, 0.0, 90.0, 90.0, 1.0, 1.0),
            Vec2::new(10.0, 0.0),
            Vec2::ZERO,
        );
        assert!((q[1] - Vec2::new(0.0, 10.0)).length() < 1e-4, "{:?}", q[1]);
    }

    #[test]
    fn hit_matches_quad() {
        let f = frame(100.0, 50.0, 30.0, 10.0, 2.0, 0.5);
        let size = Vec2::new(40.0, 20.0);
        let q = quad(&f, size, Vec2::ZERO);
        let centre = (q[0] + q[2]) / 2.0;
        assert!(hit(&f, size, Vec2::ZERO, centre));
        assert!(!hit(&f, size, Vec2::ZERO, q[0] - (centre - q[0])));
    }

    #[test]
    fn world_roundtrip() {
        let p = Vec2::new(123.0, 456.0);
        assert_eq!(to_game(to_world(p)), p);
        assert_eq!(to_world(Vec2::ZERO), Vec2::new(-400.0, 300.0));
    }
}
