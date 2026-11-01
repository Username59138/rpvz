//! Compiled particle definitions (`compiled/particles/*.xml.compiled`, PC).
//!
//! Same container as compiled reanims: `u32 0xDEADFED4`, `u32 size`, zlib.
//! Inside: a schema hash, then the definition struct as raw 32-bit memory,
//! then — in the order of the definition's *field list*, which differs from
//! the struct's member order — the data behind its pointers:
//! strings and image names as `u32 len` + bytes, float tracks as
//! `u32 count` + 20-byte nodes, arrays as `u32 element size` + the raw
//! elements followed by each element's own pointer data. Array counts come
//! from the raw struct, so the struct offsets of arrays matter.

use crate::error::{FormatError, Result};
use crate::read::{Reader, inflate, latin1};

const MAGIC: u32 = 0xDEAD_FED4;
/// `sizeof(TodEmitterDefinition)` on 32-bit PC.
const EMITTER_SIZE: usize = 356;
/// `sizeof(ParticleField)`.
const FIELD_SIZE: usize = 20;
/// Struct offsets of the emitter's two field arrays' counts.
const PARTICLE_FIELDS_COUNT: usize = 216;
const SYSTEM_FIELDS_COUNT: usize = 224;

/// A particle effect: one or more emitters.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParticleDef {
    pub emitters: Vec<EmitterDef>,
}

/// How a value in a track is interpolated or distributed (`TodCurves`).
pub type Curve = u32;

/// One key of a [`FloatTrack`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackNode {
    /// Share of the lifetime, 0..1.
    pub time: f32,
    pub low: f32,
    pub high: f32,
    /// Easing towards the next node.
    pub curve: Curve,
    /// Distribution of the random pick between `low` and `high`.
    pub distribution: Curve,
}

/// A value over a lifetime, each key a random range (`FloatParameterTrack`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FloatTrack {
    pub nodes: Vec<TrackNode>,
}

impl FloatTrack {
    pub fn is_set(&self) -> bool {
        !self.nodes.is_empty()
    }
}

/// A force acting on particles (`ParticleField`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Field {
    /// `ParticleFieldType`: 1 friction, 2 acceleration, 3 attractor,
    /// 4 max velocity, 5 velocity, 6 position, 7 system position,
    /// 8 ground constraint, 9 shake, 10 circle, 11 away.
    pub kind: u32,
    pub x: FloatTrack,
    pub y: FloatTrack,
}

/// Flags of [`EmitterDef::flags`] (bit numbers).
pub mod flags {
    pub const RANDOM_LAUNCH_SPIN: u32 = 0;
    pub const ALIGN_LAUNCH_SPIN: u32 = 1;
    pub const ALIGN_TO_PIXELS: u32 = 2;
    pub const SYSTEM_LOOPS: u32 = 3;
    pub const PARTICLE_LOOPS: u32 = 4;
    pub const PARTICLES_DONT_FOLLOW: u32 = 5;
    pub const RANDOM_START_TIME: u32 = 6;
    pub const DIE_IF_OVERLOADED: u32 = 7;
    pub const ADDITIVE: u32 = 8;
    pub const FULLSCREEN: u32 = 9;
    pub const SOFTWARE_ONLY: u32 = 10;
    pub const HARDWARE_ONLY: u32 = 11;
}

/// One emitter (`TodEmitterDefinition`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EmitterDef {
    /// Image resource id, e.g. `IMAGE_REANIM_ZOMBIE_HEAD`.
    pub image: Option<String>,
    pub image_col: i32,
    pub image_row: i32,
    pub image_frames: i32,
    pub animated: i32,
    /// Bit set of [`flags`].
    pub flags: u32,
    /// 0 circle, 1 box, 2 box path, 3 circle path, 4 circle even spacing.
    pub emitter_type: i32,
    pub name: String,
    pub on_duration: String,
    pub system_duration: FloatTrack,
    pub cross_fade_duration: FloatTrack,
    pub spawn_rate: FloatTrack,
    pub spawn_min_active: FloatTrack,
    pub spawn_max_active: FloatTrack,
    pub spawn_max_launched: FloatTrack,
    pub emitter_radius: FloatTrack,
    pub emitter_offset_x: FloatTrack,
    pub emitter_offset_y: FloatTrack,
    pub emitter_box_x: FloatTrack,
    pub emitter_box_y: FloatTrack,
    pub emitter_path: FloatTrack,
    pub emitter_skew_x: FloatTrack,
    pub emitter_skew_y: FloatTrack,
    pub particle_duration: FloatTrack,
    pub system_red: FloatTrack,
    pub system_green: FloatTrack,
    pub system_blue: FloatTrack,
    pub system_alpha: FloatTrack,
    pub system_brightness: FloatTrack,
    pub launch_speed: FloatTrack,
    pub launch_angle: FloatTrack,
    pub particle_fields: Vec<Field>,
    pub system_fields: Vec<Field>,
    pub particle_red: FloatTrack,
    pub particle_green: FloatTrack,
    pub particle_blue: FloatTrack,
    pub particle_alpha: FloatTrack,
    pub particle_brightness: FloatTrack,
    pub particle_spin_angle: FloatTrack,
    pub particle_spin_speed: FloatTrack,
    pub particle_scale: FloatTrack,
    pub particle_stretch: FloatTrack,
    pub collision_reflect: FloatTrack,
    pub collision_spin: FloatTrack,
    pub clip_top: FloatTrack,
    pub clip_bottom: FloatTrack,
    pub clip_left: FloatTrack,
    pub clip_right: FloatTrack,
    pub animation_rate: FloatTrack,
}

impl EmitterDef {
    pub fn has_flag(&self, bit: u32) -> bool {
        self.flags & (1 << bit) != 0
    }
}

impl ParticleDef {
    /// Parses a compiled particle file.
    pub fn parse_compiled(file: &[u8]) -> Result<Self> {
        let mut r = Reader::new(file, "particle container");
        let magic = r.u32()?;
        if magic != MAGIC {
            return Err(FormatError::BadMagic {
                what: "compiled particles",
                found: magic.to_le_bytes().to_vec(),
            });
        }
        let size = r.usize()?;
        let data = inflate(&file[8..], size, "compiled particles")?;
        let mut r = Reader::new(&data, "particle definition");
        let _schema_hash = r.u32()?;
        // TodParticleDefinition { TodEmitterDefinition* mEmitterDefs; int mEmitterDefCount; }
        let _ptr = r.u32()?;
        let count = r.usize()?;
        let emitters = read_array(&mut r, count, EMITTER_SIZE, read_emitter)?;
        if !r.is_at_end() {
            return Err(FormatError::Invalid(format!(
                "particle definition: {} bytes left over",
                data.len() - r.pos()
            )));
        }
        Ok(Self { emitters })
    }
}

/// An array: element size, raw elements, then each element's pointer data.
fn read_array<T>(
    r: &mut Reader,
    count: usize,
    expected_size: usize,
    read: impl Fn(&[u8], &mut Reader) -> Result<T>,
) -> Result<Vec<T>> {
    let size = r.usize()?;
    if size != expected_size {
        return Err(FormatError::Unsupported(format!(
            "particle array element size {size}, expected {expected_size}"
        )));
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let raw = r
        .bytes(size.checked_mul(count).ok_or_else(|| {
            FormatError::Invalid(format!("particle array of {count} × {size}"))
        })?)?;
    raw.chunks_exact(size).map(|e| read(e, r)).collect()
}

fn raw_i32(raw: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        raw[offset],
        raw[offset + 1],
        raw[offset + 2],
        raw[offset + 3],
    ])
}

fn read_string(r: &mut Reader) -> Result<String> {
    let len = r.usize()?;
    if len > 100_000 {
        return Err(FormatError::Invalid(format!(
            "particle string of {len} bytes"
        )));
    }
    Ok(latin1(r.bytes(len)?))
}

fn read_track(r: &mut Reader) -> Result<FloatTrack> {
    let count = r.usize()?;
    if count > 10_000 {
        return Err(FormatError::Invalid(format!(
            "float track of {count} nodes"
        )));
    }
    let nodes = (0..count)
        .map(|_| {
            Ok(TrackNode {
                time: r.f32()?,
                low: r.f32()?,
                high: r.f32()?,
                curve: r.u32()?,
                distribution: r.u32()?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(FloatTrack { nodes })
}

fn read_field(raw: &[u8], r: &mut Reader) -> Result<Field> {
    Ok(Field {
        kind: raw_i32(raw, 0) as u32,
        x: read_track(r)?,
        y: read_track(r)?,
    })
}

fn read_emitter(raw: &[u8], r: &mut Reader) -> Result<EmitterDef> {
    // Pointer data follows the field list order (gEmitterDefFields).
    let image = read_string(r)?;
    let name = read_string(r)?;
    let system_duration = read_track(r)?;
    let on_duration = read_string(r)?;
    let mut e = EmitterDef {
        image: (!image.is_empty()).then_some(image),
        image_col: raw_i32(raw, 4),
        image_row: raw_i32(raw, 8),
        image_frames: raw_i32(raw, 12),
        animated: raw_i32(raw, 16),
        flags: raw_i32(raw, 20) as u32,
        emitter_type: raw_i32(raw, 24),
        name,
        on_duration,
        system_duration,
        ..EmitterDef::default()
    };
    e.cross_fade_duration = read_track(r)?;
    e.spawn_rate = read_track(r)?;
    e.spawn_min_active = read_track(r)?;
    e.spawn_max_active = read_track(r)?;
    e.spawn_max_launched = read_track(r)?;
    e.emitter_radius = read_track(r)?;
    e.emitter_offset_x = read_track(r)?;
    e.emitter_offset_y = read_track(r)?;
    e.emitter_box_x = read_track(r)?;
    e.emitter_box_y = read_track(r)?;
    e.emitter_path = read_track(r)?;
    e.emitter_skew_x = read_track(r)?;
    e.emitter_skew_y = read_track(r)?;
    e.particle_duration = read_track(r)?;
    e.system_red = read_track(r)?;
    e.system_green = read_track(r)?;
    e.system_blue = read_track(r)?;
    e.system_alpha = read_track(r)?;
    e.system_brightness = read_track(r)?;
    e.launch_speed = read_track(r)?;
    e.launch_angle = read_track(r)?;
    let pf = raw_i32(raw, PARTICLE_FIELDS_COUNT).max(0) as usize;
    e.particle_fields = read_array(r, pf, FIELD_SIZE, read_field)?;
    let sf = raw_i32(raw, SYSTEM_FIELDS_COUNT).max(0) as usize;
    e.system_fields = read_array(r, sf, FIELD_SIZE, read_field)?;
    e.particle_red = read_track(r)?;
    e.particle_green = read_track(r)?;
    e.particle_blue = read_track(r)?;
    e.particle_alpha = read_track(r)?;
    e.particle_brightness = read_track(r)?;
    e.particle_spin_angle = read_track(r)?;
    e.particle_spin_speed = read_track(r)?;
    e.particle_scale = read_track(r)?;
    e.particle_stretch = read_track(r)?;
    e.collision_reflect = read_track(r)?;
    e.collision_spin = read_track(r)?;
    e.clip_top = read_track(r)?;
    e.clip_bottom = read_track(r)?;
    e.clip_left = read_track(r)?;
    e.clip_right = read_track(r)?;
    e.animation_rate = read_track(r)?;
    Ok(e)
}
