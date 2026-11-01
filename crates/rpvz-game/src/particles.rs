//! The original particle system (`TodParticle`): effects made of emitters,
//! each spawning particles whose every property follows a float track over
//! its lifetime, with a random pick inside each key's range. Simulated at
//! 100 ticks per second like the rest of the game; drawn as sprites.
//!
//! Not yet: cross-fading between emitters (`OnDuration`, `CrossFadeDuration`)
//! and additive blending (drawn normally).

use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::prelude::*;
use rpvz_formats::particle::{EmitterDef, Field, FloatTrack, ParticleDef, flags};

use crate::config::Config;
use crate::library::{ImageCache, Library};
use crate::reanim::{Clock, to_world};

pub struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            step_particles.after(crate::reanim::ReanimSet::Advance),
        )
        .add_systems(
            PostUpdate,
            draw_particles.before(TransformSystems::Propagate),
        );
    }
}

const TICK: f32 = 0.01;

/// A running effect. Spawn with [`spawn_particles`].
#[derive(Component)]
pub struct ParticleSystem {
    def: Arc<ParticleDef>,
    emitters: Vec<Emitter>,
    rng: u64,
    acc: f32,
    sprites: Vec<Entity>,
    pub dead: bool,
}

struct Emitter {
    index: usize,
    center: Vec2,
    spawn_accum: f32,
    spawned: i32,
    age: i32,
    duration: i32,
    time: f32,
    last_time: f32,
    track_interp: [f32; SYSTEM_TRACKS],
    field_interp: Vec<[f32; 2]>,
    particles: Vec<Particle>,
    dead: bool,
}

struct Particle {
    pos: Vec2,
    vel: Vec2,
    age: i32,
    duration: i32,
    time: f32,
    last_time: f32,
    interp: [f32; PARTICLE_TRACKS],
    field_interp: Vec<[f32; 2]>,
    spin: f32,
    spin_vel: f32,
    anim_time: f32,
    frame: i32,
    /// Offset the shake field applied last tick (undone before the next).
    shake: Vec2,
}

// Random interpolation slots, one per track that can use one.
const SYSTEM_TRACKS: usize = 10;
const SYS_SPAWN_RATE: usize = 0;
const SYS_MIN_ACTIVE: usize = 1;
const SYS_MAX_ACTIVE: usize = 2;
const SYS_MAX_LAUNCHED: usize = 3;
const SYS_PATH: usize = 4;
const SYS_RED: usize = 5;
const SYS_GREEN: usize = 6;
const SYS_BLUE: usize = 7;
const SYS_ALPHA: usize = 8;
const SYS_BRIGHTNESS: usize = 9;

const PARTICLE_TRACKS: usize = 16;
const P_RED: usize = 0;
const P_GREEN: usize = 1;
const P_BLUE: usize = 2;
const P_ALPHA: usize = 3;
const P_BRIGHTNESS: usize = 4;
const P_SPIN_ANGLE: usize = 5;
const P_SPIN_SPEED: usize = 6;
const P_SCALE: usize = 7;
const P_STRETCH: usize = 8;
const P_REFLECT: usize = 9;
const P_COLLISION_SPIN: usize = 10;
const P_CLIP_TOP: usize = 11;
const P_CLIP_BOTTOM: usize = 12;
const P_CLIP_LEFT: usize = 13;
const P_CLIP_RIGHT: usize = 14;
const P_ANIMATION_RATE: usize = 15;

/// `FloatTrackIsSet`: has keys and doesn't start constant (defaults are
/// single constant keys).
fn is_set(t: &FloatTrack) -> bool {
    t.nodes.first().is_some_and(|n| n.curve != 0)
}

/// `TodCurveEvaluate`.
fn curve(t: f32, start: f32, end: f32, kind: u32) -> f32 {
    let quad = |t: f32| t * t;
    let inv_quad = |t: f32| 2.0 * t - t * t;
    let s = |t: f32| 3.0 * t * t - 2.0 * t * t * t;
    let inv_quad_s = |t: f32| {
        if t <= 0.5 {
            inv_quad(t * 2.0) * 0.5
        } else {
            quad((t - 0.5) * 2.0) * 0.5 + 0.5
        }
    };
    let bounce = |t: f32| 1.0 - (2.0 * t - 1.0).abs();
    let w = match kind {
        0 => 0.0,
        1 => t,
        2 => quad(t),
        3 => inv_quad(t),
        4 => s(s(t)),
        5 => s(t),
        6 => inv_quad_s(inv_quad_s(t)),
        7 => inv_quad_s(t),
        9 => bounce(t),
        10 => quad(bounce(t)),
        11 => inv_quad(bounce(t)),
        12 => (TAU * t).sin(),
        13 => (TAU * s(t)).sin(),
        _ => t,
    };
    (end - start) * w + start
}

/// `FloatTrackEvaluate`, with the loader's default for tracks the file
/// leaves empty.
fn eval(track: &FloatTrack, time: f32, interp: f32, default: f32) -> f32 {
    let nodes = &track.nodes;
    let Some(first) = nodes.first() else {
        return default;
    };
    if time < first.time {
        return curve(interp, first.low, first.high, first.distribution);
    }
    for w in nodes.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if time <= b.time {
            let f = (time - a.time) / (b.time - a.time);
            let left = curve(interp, a.low, a.high, a.distribution);
            let right = curve(interp, b.low, b.high, b.distribution);
            return curve(f, left, right, a.curve);
        }
    }
    let last = &nodes[nodes.len() - 1];
    curve(interp, last.low, last.high, last.distribution)
}

/// `FloatTrackEvaluateFromLastTime`: zero before the first update.
fn eval_last(track: &FloatTrack, time: f32, interp: f32, default: f32) -> f32 {
    if time < 0.0 {
        0.0
    } else {
        eval(track, time, interp, default)
    }
}

fn constant_zero(t: &FloatTrack) -> bool {
    t.nodes.is_empty() || (t.nodes.len() == 1 && t.nodes[0].low == 0.0 && t.nodes[0].high == 0.0)
}

impl ParticleSystem {
    fn rand(&mut self) -> f32 {
        rand01(&mut self.rng)
    }

    fn new(def: Arc<ParticleDef>, pos: Vec2, seed: u64) -> Self {
        let mut s = Self {
            def: def.clone(),
            emitters: Vec::new(),
            rng: seed | 1,
            acc: 0.0,
            sprites: Vec::new(),
            dead: false,
        };
        for (index, e) in def.emitters.iter().enumerate() {
            if is_set(&e.cross_fade_duration) {
                // Only reached by cross-fades, which aren't supported yet.
                continue;
            }
            let duration = if is_set(&e.system_duration) {
                let r = s.rand();
                eval(&e.system_duration, 0.0, r, 0.0)
            } else {
                eval(&e.particle_duration, 0.0, 1.0, 100.0)
            };
            let field_interp = (0..e.system_fields.len())
                .map(|_| [s.rand(), s.rand()])
                .collect();
            let mut track_interp = [0.0; SYSTEM_TRACKS];
            for v in &mut track_interp {
                *v = s.rand();
            }
            s.emitters.push(Emitter {
                index,
                center: pos,
                spawn_accum: 0.0,
                spawned: 0,
                age: -1,
                duration: (duration as i32).max(1),
                time: -1.0,
                last_time: -1.0,
                track_interp,
                field_interp,
                particles: Vec::new(),
                dead: false,
            });
            let last = s.emitters.len() - 1;
            s.update_emitter(last);
        }
        s
    }

    /// One tick.
    fn update(&mut self) {
        let mut alive = false;
        for i in 0..self.emitters.len() {
            self.update_emitter(i);
            alive |= !self.emitters[i].dead;
        }
        if !alive {
            self.dead = true;
        }
    }

    fn update_emitter(&mut self, i: usize) {
        let def = self.def.clone();
        let mut rng = self.rng;
        let e = &mut self.emitters[i];
        let d = &def.emitters[e.index];
        if e.dead {
            return;
        }
        e.age += 1;
        let mut die = false;
        if e.age >= e.duration {
            if d.has_flag(flags::SYSTEM_LOOPS) {
                e.age = 0;
            } else {
                e.age = e.duration - 1;
                die = true;
            }
        }
        e.time = e.age as f32 / (e.duration - 1).max(1) as f32;
        for (n, field) in d.system_fields.iter().enumerate() {
            // Only `SystemPosition` acts on the emitter itself.
            if field.kind == 7 {
                let [ix, iy] = e.field_interp[n];
                let x = eval(&field.x, e.time, ix, 0.0) - eval_last(&field.x, e.last_time, ix, 0.0);
                let y = eval(&field.y, e.time, iy, 0.0) - eval_last(&field.y, e.last_time, iy, 0.0);
                e.center += Vec2::new(x, y);
            }
        }
        let center = e.center;
        e.particles
            .retain_mut(|p| update_particle(d, center, p, &mut rng));
        spawn(d, e, &mut rng);
        if die {
            e.particles.clear();
            e.dead = true;
            self.rng = rng;
            return;
        }
        e.last_time = e.time;
        self.rng = rng;
    }
}

fn rand01(rng: &mut u64) -> f32 {
    // xorshift64*
    let mut x = *rng;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *rng = x;
    ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
}

fn spawn(d: &EmitterDef, e: &mut Emitter, rng: &mut u64) {
    let sys =
        |t: &FloatTrack, slot: usize, default: f32| eval(t, e.time, e.track_interp[slot], default);
    e.spawn_accum += sys(&d.spawn_rate, SYS_SPAWN_RATE, 0.0) * 0.01;
    let mut count = e.spawn_accum as i32;
    e.spawn_accum -= count as f32;
    let active = e.particles.len() as i32;
    let min_active = sys(&d.spawn_min_active, SYS_MIN_ACTIVE, -1.0) as i32;
    if min_active >= 0 && count < min_active - active {
        count = min_active - active;
    }
    let max_active = sys(&d.spawn_max_active, SYS_MAX_ACTIVE, -1.0) as i32;
    if max_active >= 0 && count > max_active - active {
        count = max_active - active;
    }
    if is_set(&d.spawn_max_launched) {
        let max = sys(&d.spawn_max_launched, SYS_MAX_LAUNCHED, -1.0) as i32;
        count = count.min(max - e.spawned);
    }
    for n in 0..count.max(0) {
        let p = spawn_particle(d, e, n, count, rng);
        e.particles.insert(0, p);
        e.spawned += 1;
    }
}

fn spawn_particle(d: &EmitterDef, e: &Emitter, index: i32, total: i32, rng: &mut u64) -> Particle {
    let mut r = || rand01(rng);
    let field_interp: Vec<[f32; 2]> = (0..d.particle_fields.len()).map(|_| [r(), r()]).collect();
    let mut interp = [0.0; PARTICLE_TRACKS];
    for v in &mut interp {
        *v = r();
    }
    let t = e.time;
    let duration_interp = r();
    let speed_interp = r();
    let offset_x_interp = r();
    let offset_y_interp = r();
    let duration = (eval(&d.particle_duration, t, duration_interp, 100.0) as i32).max(1);
    let age = if d.has_flag(flags::RANDOM_START_TIME) {
        (r() * duration as f32) as i32
    } else {
        0
    };
    let speed = eval(&d.launch_speed, t, speed_interp, 0.0) * 0.01;
    let angle_interp = r();
    let angle = match d.emitter_type {
        3 => {
            eval(&d.emitter_path, t, e.track_interp[SYS_PATH], 0.0) * TAU
                + eval(&d.launch_angle, t, angle_interp, 0.0).to_radians()
        }
        4 => {
            TAU * index as f32 / total.max(1) as f32
                + eval(&d.launch_angle, t, angle_interp, 0.0).to_radians()
        }
        _ if constant_zero(&d.launch_angle) => r() * TAU,
        _ => eval(&d.launch_angle, t, angle_interp, 0.0).to_radians(),
    };
    let (px, py) = match d.emitter_type {
        // Circle shapes: angle 0 points down.
        0 | 3 | 4 => {
            let radius = eval(&d.emitter_radius, t, r(), 0.0);
            (angle.sin() * radius, angle.cos() * radius)
        }
        2 => {
            let pos = eval(&d.emitter_path, t, e.track_interp[SYS_PATH], 0.0);
            let (x0, x1) = (
                eval(&d.emitter_box_x, t, 0.0, 0.0),
                eval(&d.emitter_box_x, t, 1.0, 0.0),
            );
            let (y0, y1) = (
                eval(&d.emitter_box_y, t, 0.0, 0.0),
                eval(&d.emitter_box_y, t, 1.0, 0.0),
            );
            let (dx, dy) = (x1 - x0, y1 - y0);
            let p = pos * 2.0 * (dx + dy);
            if p < dy {
                (x0, y0 + p)
            } else if p < dy + dx {
                (x0 + (p - dy), y1)
            } else if p < 2.0 * dy + dx {
                (x1, y1 - (p - dy - dx))
            } else {
                (x1 - (p - 2.0 * dy - dx), y0)
            }
        }
        // Box.
        _ => {
            let (ix, iy) = (r(), r());
            (
                eval(&d.emitter_box_x, t, ix, 0.0),
                eval(&d.emitter_box_y, t, iy, 0.0),
            )
        }
    };
    let (skx, sky) = (r(), r());
    let skew_x = eval(&d.emitter_skew_x, t, skx, 0.0);
    let skew_y = eval(&d.emitter_skew_y, t, sky, 0.0);
    let mut pos = e.center + Vec2::new(px + py * skew_x, py + px * skew_y);
    pos.x += eval(&d.emitter_offset_x, t, offset_x_interp, 0.0);
    pos.y += eval(&d.emitter_offset_y, t, offset_y_interp, 0.0);
    let frame = if d.animated != 0 || is_set(&d.animation_rate) {
        0
    } else {
        (r() * d.image_frames.max(1) as f32) as i32
    };
    let spin = if d.has_flag(flags::RANDOM_LAUNCH_SPIN) {
        r() * TAU
    } else if d.has_flag(flags::ALIGN_LAUNCH_SPIN) {
        angle
    } else {
        0.0
    };
    let mut p = Particle {
        pos,
        vel: Vec2::new(angle.sin() * speed, angle.cos() * speed),
        age,
        duration,
        time: -1.0,
        last_time: -1.0,
        interp,
        field_interp,
        spin,
        spin_vel: 0.0,
        anim_time: 0.0,
        frame,
        shake: Vec2::ZERO,
    };
    update_particle(d, e.center, &mut p, rng);
    p
}

/// `UpdateParticle`; false once the particle is done.
fn update_particle(d: &EmitterDef, center: Vec2, p: &mut Particle, rng: &mut u64) -> bool {
    if p.age >= p.duration {
        if d.has_flag(flags::PARTICLE_LOOPS) {
            p.age = 0;
        } else {
            return false;
        }
    }
    p.time = p.age as f32 / (p.duration - 1).max(1) as f32;
    for (n, field) in d.particle_fields.iter().enumerate() {
        update_field(d, center, p, field, n, rng);
    }
    p.pos += p.vel;
    let spin_speed = eval(&d.particle_spin_speed, p.time, p.interp[P_SPIN_SPEED], 0.0) * 0.01;
    let spin_angle = eval(&d.particle_spin_angle, p.time, p.interp[P_SPIN_ANGLE], 0.0);
    let last_angle = eval_last(
        &d.particle_spin_angle,
        p.last_time,
        p.interp[P_SPIN_ANGLE],
        0.0,
    );
    p.spin += (spin_speed + spin_angle - last_angle).to_radians() + p.spin_vel;
    if is_set(&d.animation_rate) {
        let rate = eval(&d.animation_rate, p.time, p.interp[P_ANIMATION_RATE], 0.0) * 0.01;
        p.anim_time = (p.anim_time + rate).rem_euclid(1.0);
    }
    p.age += 1;
    p.last_time = p.time;
    true
}

fn update_field(
    d: &EmitterDef,
    center: Vec2,
    p: &mut Particle,
    f: &Field,
    n: usize,
    rng: &mut u64,
) {
    let [ix, iy] = p.field_interp[n];
    let x = eval(&f.x, p.time, ix, 0.0);
    let y = eval(&f.y, p.time, iy, 0.0);
    match f.kind {
        // Friction.
        1 => {
            p.vel.x *= 1.0 - x;
            p.vel.y *= 1.0 - y;
        }
        // Acceleration.
        2 => p.vel += Vec2::new(x, y) * 0.01,
        // Attractor.
        3 => p.vel += (Vec2::new(x, y) - (p.pos - center)) * 0.01,
        // Max velocity.
        4 => {
            p.vel.x = p.vel.x.clamp(-x, x);
            p.vel.y = p.vel.y.clamp(-y, y);
        }
        // Velocity.
        5 => p.pos += Vec2::new(x, y) * 0.01,
        // Position.
        6 => {
            let lx = eval_last(&f.x, p.last_time, ix, 0.0);
            let ly = eval_last(&f.y, p.last_time, iy, 0.0);
            p.pos += Vec2::new(x - lx, y - ly);
        }
        // Ground: bounce off `center.y + y`.
        8 => {
            if p.pos.y > center.y + y {
                p.pos.y = center.y + y;
                let reflect = eval(&d.collision_reflect, p.time, p.interp[P_REFLECT], 0.0);
                let spin =
                    eval(&d.collision_spin, p.time, p.interp[P_COLLISION_SPIN], 0.0) / 1000.0;
                p.spin_vel = p.vel.y * spin;
                p.vel.x *= reflect;
                p.vel.y *= -reflect;
            }
        }
        // Shake: a fresh random offset each tick, the previous one undone.
        9 => {
            let r = Vec2::new(rand01(rng) * 2.0 - 1.0, rand01(rng) * 2.0 - 1.0);
            let offset = Vec2::new(x, y) * r;
            p.pos += offset - p.shake;
            p.shake = offset;
        }
        // Circle and away from the centre.
        10 | 11 => {
            let to = p.pos - center;
            let radius = to.length();
            let dir = if f.kind == 10 { to.perp() } else { to };
            p.pos += dir.normalize_or_zero() * 0.01 * (x + radius * y);
        }
        _ => {}
    }
}

/// Starts an effect at `pos` (game coordinates), drawn at world depth `z`.
pub fn spawn_particles(
    commands: &mut Commands,
    def: Arc<ParticleDef>,
    pos: Vec2,
    z: f32,
    seed: u64,
) -> Entity {
    commands
        .spawn((
            ParticleSystem::new(def, pos, seed),
            Transform::from_xyz(0.0, 0.0, z),
            Visibility::default(),
        ))
        .id()
}

fn step_particles(
    mut commands: Commands,
    clock: Res<Clock>,
    mut systems: Query<(Entity, &mut ParticleSystem)>,
) {
    for (entity, mut s) in &mut systems {
        s.acc += clock.dt;
        let mut steps = 0;
        while s.acc >= TICK && steps < 20 {
            s.acc -= TICK;
            s.update();
            steps += 1;
        }
        trace!(
            "particles: {} emitters, {} particles",
            s.emitters.len(),
            s.emitters.iter().map(|e| e.particles.len()).sum::<usize>()
        );
        if s.dead {
            commands.entity(entity).despawn();
        }
    }
}

/// One drawn particle.
#[derive(Component)]
struct ParticleSprite;

fn draw_particles(
    mut commands: Commands,
    cfg: Res<Config>,
    library: Res<Library>,
    mut cache: ResMut<ImageCache>,
    mut images: ResMut<Assets<Image>>,
    mut systems: Query<(Entity, &mut ParticleSystem)>,
    mut sprites: Query<(&mut Sprite, &mut Transform, &mut Visibility), With<ParticleSprite>>,
) {
    for (entity, mut system) in &mut systems {
        let system = &mut *system;
        let mut used = 0;
        for e in &system.emitters {
            let d = &system.def.emitters[e.index];
            let Some(id) = d.image.as_deref() else {
                continue;
            };
            let Some(cached) = cache.get(id, &library, &mut images, !cfg.boxes) else {
                continue;
            };
            let (cols, rows) = library.image_grid(id);
            let cel = cached.size.as_vec2() / Vec2::new(cols as f32, rows as f32);
            let sys = |t: &FloatTrack, slot: usize, default: f32| {
                eval(t, e.time, e.track_interp[slot], default)
            };
            for p in &e.particles {
                let part = |t: &FloatTrack, slot: usize, default: f32| {
                    eval(t, p.time, p.interp[slot], default)
                };
                let brightness = part(&d.particle_brightness, P_BRIGHTNESS, 1.0)
                    * sys(&d.system_brightness, SYS_BRIGHTNESS, 1.0);
                let channel = |pt: &FloatTrack, ps: usize, st: &FloatTrack, ss: usize| {
                    (part(pt, ps, 1.0) * sys(st, ss, 1.0) * 255.0 * brightness)
                        .round()
                        .clamp(0.0, 255.0)
                        / 255.0
                };
                let color = Color::srgba(
                    channel(&d.particle_red, P_RED, &d.system_red, SYS_RED),
                    channel(&d.particle_green, P_GREEN, &d.system_green, SYS_GREEN),
                    channel(&d.particle_blue, P_BLUE, &d.system_blue, SYS_BLUE),
                    channel(&d.particle_alpha, P_ALPHA, &d.system_alpha, SYS_ALPHA),
                );
                if color.alpha() <= 0.0 {
                    continue;
                }
                let frames = d.image_frames.max(1);
                let mut frame = if is_set(&d.animation_rate) {
                    ((p.anim_time * frames as f32) as i32).clamp(0, frames - 1)
                } else if d.animated != 0 {
                    ((p.time * frames as f32) as i32).clamp(0, frames - 1)
                } else {
                    p.frame
                };
                frame = (frame + d.image_col).min(cols as i32 - 1);
                let row = d.image_row.min(rows as i32 - 1);
                let clip = [
                    part(&d.clip_top, P_CLIP_TOP, 0.0),
                    part(&d.clip_bottom, P_CLIP_BOTTOM, 0.0),
                    part(&d.clip_left, P_CLIP_LEFT, 0.0),
                    part(&d.clip_right, P_CLIP_RIGHT, 0.0),
                ];
                let min = Vec2::new(frame as f32 * cel.x, row as f32 * cel.y)
                    + Vec2::new(clip[2] * cel.x, clip[0] * cel.y).round();
                let size = cel
                    - (Vec2::new(cel.x * (clip[2] + clip[3]), cel.y * (clip[0] + clip[1]))).round();
                let mut pos = p.pos + Vec2::new(clip[2] * cel.x, clip[0] * cel.y);
                if d.has_flag(flags::ALIGN_TO_PIXELS) {
                    pos = pos.round();
                }
                let scale = part(&d.particle_scale, P_SCALE, 1.0);
                let stretch = part(&d.particle_stretch, P_STRETCH, 1.0);
                let w = to_world(pos);
                let transform = Transform {
                    translation: Vec3::new(w.x, w.y, used as f32 * 0.001),
                    // y-down `[cos sin; -sin cos]` turns the other way on screen.
                    rotation: Quat::from_rotation_z(p.spin),
                    scale: Vec3::new(scale, scale * stretch, 1.0),
                };
                let sprite = match cached.handle.clone() {
                    Some(image) => Sprite {
                        image,
                        rect: Some(Rect::from_corners(min, min + size)),
                        color,
                        ..default()
                    },
                    None => {
                        Sprite::from_color(Color::srgba(1.0, 0.6, 0.2, 0.5 * color.alpha()), size)
                    }
                };
                if let Some(&child) = system.sprites.get(used) {
                    if let Ok((mut s, mut t, mut v)) = sprites.get_mut(child) {
                        *s = sprite;
                        *t = transform;
                        *v = Visibility::Inherited;
                    }
                } else {
                    let child = commands
                        .spawn((ParticleSprite, sprite, transform, ChildOf(entity)))
                        .id();
                    system.sprites.push(child);
                }
                used += 1;
            }
        }
        for &child in &system.sprites[used.min(system.sprites.len())..] {
            if let Ok((_, _, mut v)) = sprites.get_mut(child) {
                *v = Visibility::Hidden;
            }
        }
    }
}
