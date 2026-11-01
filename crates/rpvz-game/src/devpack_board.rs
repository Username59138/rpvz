//! Placeholder art for the lawn: plain shapes under the image ids and reanim
//! track names the board expects. Nothing here resembles the game's art; it
//! only has the same sizes and animation ranges, so layout and logic can be
//! checked without game files.

use std::path::Path;

use rpvz_formats::reanim::{ImageRef, RawFrame, Reanim, Track, Variant, write_compiled};

use crate::devpack::Canvas;

/// `resources.xml` entries (id without `IMAGE_`, path under `images/`).
pub const IMAGES: &[(&str, &str)] = &[
    ("BACKGROUND1UNSODDED", "background1unsodded"),
    ("SOD1ROW", "sod1row"),
    ("SEEDBANK", "seedbank"),
    ("SEEDS", "seeds"),
    ("PROJECTILEPEA", "projectilepea"),
];

/// Where a track is on one frame; `None` hides it.
#[derive(Clone, Copy)]
struct Pose {
    x: f32,
    y: f32,
    /// Degrees, clockwise on screen.
    rot: f32,
    scale: f32,
    alpha: f32,
}

fn at(x: f32, y: f32) -> Pose {
    Pose {
        x,
        y,
        rot: 0.0,
        scale: 1.0,
        alpha: 1.0,
    }
}

impl Pose {
    fn rot(self, rot: f32) -> Self {
        Self { rot, ..self }
    }

    fn scale(self, scale: f32) -> Self {
        Self { scale, ..self }
    }

    fn alpha(self, alpha: f32) -> Self {
        Self { alpha, ..self }
    }
}

/// An `anim_*` marker visible on `range`.
fn marker(name: &str, frames: usize, range: std::ops::Range<usize>) -> Track {
    Track {
        name: name.into(),
        frames: (0..frames)
            .map(|i| RawFrame {
                f: Some(if range.contains(&i) { 0.0 } else { -1.0 }),
                ..RawFrame::default()
            })
            .collect(),
    }
}

/// An image track posed by `pose(frame)`.
fn part(name: &str, image: &str, frames: usize, pose: impl Fn(usize) -> Option<Pose>) -> Track {
    Track {
        name: name.into(),
        frames: (0..frames)
            .map(|i| match pose(i) {
                Some(p) => RawFrame {
                    x: Some(p.x),
                    y: Some(p.y),
                    // Reanim skew is counter-clockwise in a y-down space.
                    kx: Some(p.rot),
                    ky: Some(p.rot),
                    sx: Some(p.scale),
                    sy: Some(p.scale),
                    f: Some(0.0),
                    a: Some(p.alpha),
                    image: (i == 0).then(|| ImageRef::Name(format!("IMAGE_REANIM_{image}"))),
                    ..RawFrame::default()
                },
                None => RawFrame {
                    f: Some(-1.0),
                    image: (i == 0).then(|| ImageRef::Name(format!("IMAGE_REANIM_{image}"))),
                    ..RawFrame::default()
                },
            })
            .collect(),
    }
}

fn save_reanim(dir: &Path, name: &str, fps: f32, tracks: Vec<Track>) -> Result<(), String> {
    let path = dir.join(format!("compiled/reanim/{name}.reanim.compiled"));
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| e.to_string())?;
    let bytes = write_compiled(&Reanim { fps, tracks }, Variant::Pc).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Phase of frame `i` in a loop of `len` frames, 0..2π.
fn phase(i: usize, len: usize) -> f32 {
    (i % len) as f32 / len as f32 * std::f32::consts::TAU
}

pub fn write(dir: &Path) -> Result<(), String> {
    write_images(dir)?;
    write_plant(dir)?;
    write_zombie(dir)?;
    write_sun(dir)?;
    write_mower(dir)
}

fn write_images(dir: &Path) -> Result<(), String> {
    let img = |name: &str| dir.join(format!("images/{name}.png"));
    // Backdrop: a dark strip for the house side, a flat brown yard with a
    // faint grid where the lawn is.
    let mut bg = Canvas::new(1400, 600).backdrop([70, 96, 140, 255], [60, 74, 52, 255]);
    for y in 80..580 {
        for x in 0..1400 {
            if x < 180 {
                bg.set(x, y, [58, 50, 64, 255]);
            } else if (260..980).contains(&x) {
                let cell = ((x - 260) / 80 + (y - 80) / 100) % 2;
                let c = if cell == 0 {
                    [112, 88, 60, 255]
                } else {
                    [104, 82, 56, 255]
                };
                bg.set(x, y, c);
            }
        }
    }
    bg.save(&img("background1unsodded"))?;
    let mut sod = Canvas::new(771, 127);
    for y in 0..127 {
        for x in 0..771 {
            let edge = !(6..=120).contains(&y) || !(6..=764).contains(&x);
            let check = (x / 80 + y / 64) % 2 == 0;
            let c = match (edge, check) {
                (true, _) => [56, 110, 52, 255],
                (false, true) => [84, 156, 70, 255],
                (false, false) => [76, 146, 64, 255],
            };
            sod.set(x, y, c);
        }
    }
    sod.save(&img("sod1row"))?;
    let mut bank = Canvas::new(446, 87).panel([128, 92, 60, 255], [76, 52, 34, 255], 8.0);
    // Sun counter slot.
    for y in 66..84 {
        for x in 8..62 {
            bank.set(x, y, [236, 226, 196, 255]);
        }
    }
    bank.save(&img("seedbank"))?;
    let mut seeds = Canvas::new(450, 70);
    for cel in 0..9u32 {
        let panel = Canvas::new(50, 70).panel([214, 222, 190, 255], [120, 132, 96, 255], 5.0);
        for y in 0..70 {
            for x in 0..50 {
                seeds.set(cel * 50 + x, y, panel.get(x, y));
            }
        }
    }
    seeds.save(&img("seeds"))?;
    Canvas::new(28, 28)
        .ellipse([110, 200, 80, 255])
        .save(&img("projectilepea"))
}

/// A green blob on a stalk: 25-frame idle, the head bobbing.
fn write_plant(dir: &Path) -> Result<(), String> {
    let img = |name: &str| dir.join(format!("reanim/plant_{name}.png"));
    Canvas::new(12, 40)
        .panel([60, 140, 60, 255], [40, 100, 40, 255], 3.0)
        .save(&img("stalk"))?;
    Canvas::new(46, 40)
        .ellipse([120, 200, 90, 255])
        .save(&img("head"))?;
    Canvas::new(16, 14)
        .panel([90, 170, 70, 255], [50, 110, 40, 255], 3.0)
        .save(&img("mouth"))?;
    const N: usize = 25;
    let bob = |i: usize| 2.0 * phase(i, N).sin();
    let tracks = vec![
        marker("anim_full_idle", N, 0..N),
        part("stalk_bottom", "PLANT_STALK", N, |_| Some(at(30.0, 46.0))),
        part("anim_face", "PLANT_HEAD", N, move |i| {
            Some(at(14.0 + bob(i), 12.0))
        }),
        part("idle_mouth", "PLANT_MOUTH", N, move |i| {
            Some(at(56.0 + bob(i), 24.0))
        }),
    ];
    save_reanim(dir, crate::board::PEASHOOTER, 12.0, tracks)
}

/// A boxy walker made of rectangles: walk 0..24, eat 24..40, death 40..64.
fn write_zombie(dir: &Path) -> Result<(), String> {
    let img = |name: &str| dir.join(format!("reanim/zed_{name}.png"));
    let grey = [120, 128, 140, 255];
    let dark = [70, 76, 86, 255];
    Canvas::new(34, 52)
        .panel([96, 100, 120, 255], dark, 4.0)
        .save(&img("body"))?;
    Canvas::new(36, 34)
        .ellipse([160, 172, 150, 255])
        .save(&img("head"))?;
    Canvas::new(12, 26)
        .panel(grey, dark, 3.0)
        .save(&img("arm"))?;
    Canvas::new(12, 22)
        .panel(grey, dark, 3.0)
        .save(&img("arm_stump"))?;
    Canvas::new(12, 12)
        .ellipse([160, 172, 150, 255])
        .save(&img("hand"))?;
    Canvas::new(14, 34)
        .panel([80, 70, 60, 255], [50, 44, 38, 255], 3.0)
        .save(&img("leg"))?;
    Canvas::new(26, 26)
        .panel([220, 130, 50, 255], [150, 80, 30, 255], 4.0)
        .save(&img("hat"))?;
    Canvas::new(22, 10)
        .panel([140, 150, 130, 255], dark, 3.0)
        .save(&img("jaw"))?;
    Canvas::new(18, 8)
        .panel([60, 56, 52, 255], [40, 36, 34, 255], 3.0)
        .save(&img("hair"))?;
    Canvas::new(8, 6)
        .panel([200, 90, 100, 255], [140, 60, 70, 255], 2.0)
        .save(&img("tongue"))?;

    const WALK: usize = 24;
    const EAT: usize = 16;
    const DEATH: usize = 24;
    const N: usize = WALK + EAT + DEATH;
    let walk = 0..WALK;
    let eat = WALK..WALK + EAT;
    let death = WALK + EAT..N;
    // Death: the whole figure tips backwards and fades.
    let fall = move |i: usize| {
        if i >= WALK + EAT {
            ((i - WALK - EAT) as f32 / (DEATH - 1) as f32).min(1.0)
        } else {
            0.0
        }
    };
    // Every part pivots around the feet when falling.
    let pose = move |i: usize, x: f32, y: f32| {
        let t = fall(i);
        let a = (-80.0 * t).to_radians();
        let (px, py) = (40.0, 120.0);
        let (dx, dy) = (x - px, y - py);
        let p = at(
            px + dx * a.cos() - dy * a.sin(),
            py + dx * a.sin() + dy * a.cos(),
        );
        p.rot(-80.0 * t).alpha(1.0 - 0.6 * t)
    };
    let sway = move |i: usize| {
        if i < WALK {
            3.0 * phase(i, WALK).sin()
        } else {
            0.0
        }
    };
    let chew = move |i: usize| {
        if (WALK..WALK + EAT).contains(&i) {
            6.0 * phase(i - WALK, EAT / 2).sin().abs()
        } else {
            0.0
        }
    };
    let tracks = vec![
        marker("anim_walk", N, walk),
        marker("anim_eat", N, eat),
        marker("anim_death", N, death),
        part("Zombie_innerleg_upper", "ZED_LEG", N, move |i| {
            Some(pose(i, 36.0 + sway(i), 88.0))
        }),
        part("Zombie_outerleg_upper", "ZED_LEG", N, move |i| {
            Some(pose(i, 50.0 - sway(i), 88.0))
        }),
        part("Zombie_body", "ZED_BODY", N, move |i| {
            Some(pose(i, 34.0, 40.0))
        }),
        part("anim_head1", "ZED_HEAD", N, move |i| {
            Some(pose(i, 26.0 - chew(i) * 0.5, 8.0))
        }),
        part("anim_head2", "ZED_JAW", N, move |i| {
            Some(pose(i, 28.0 - chew(i) * 0.5, 36.0 + chew(i) * 0.5))
        }),
        part("anim_hair", "ZED_HAIR", N, move |i| {
            Some(pose(i, 34.0 - chew(i) * 0.5, 4.0))
        }),
        part("anim_tongue", "ZED_TONGUE", N, move |i| {
            Some(pose(i, 30.0 - chew(i) * 0.5, 42.0))
        }),
        part("anim_cone", "ZED_HAT", N, move |i| {
            Some(pose(i, 32.0, -12.0))
        }),
        part("Zombie_outerarm_upper", "ZED_ARM", N, move |i| {
            Some(pose(i, 30.0 - chew(i), 46.0))
        }),
        part("Zombie_outerarm_lower", "ZED_ARM", N, move |i| {
            Some(pose(i, 24.0 - chew(i), 66.0))
        }),
        part("Zombie_outerarm_hand", "ZED_HAND", N, move |i| {
            Some(pose(i, 22.0 - chew(i), 88.0))
        }),
    ];
    // The stump image the board swaps in when the arm falls off.
    let stump = dir.join("reanim/zombie_outerarm_upper2.png");
    std::fs::copy(img("arm_stump"), &stump).map_err(|e| format!("{}: {e}", stump.display()))?;
    save_reanim(dir, "zombie", 12.0, tracks)
}

/// Two discs around the origin, the outer one pulsing.
fn write_sun(dir: &Path) -> Result<(), String> {
    let img = |name: &str| dir.join(format!("reanim/{name}.png"));
    Canvas::new(80, 80)
        .ellipse([255, 230, 120, 110])
        .save(&img("sun3"))?;
    Canvas::new(40, 40)
        .ellipse([255, 214, 60, 255])
        .save(&img("sun1"))?;
    const N: usize = 13;
    let pulse = |i: usize| 1.0 + 0.08 * phase(i, N).sin();
    let tracks = vec![
        part("Sun3", "SUN3", N, move |i| {
            let s = pulse(i);
            Some(at(-40.0 * s, -40.0 * s).scale(s))
        }),
        part("Sun1", "SUN1", N, |_| Some(at(-20.0, -20.0))),
    ];
    save_reanim(dir, "sun", 12.0, tracks)
}

/// A box on two wheels.
fn write_mower(dir: &Path) -> Result<(), String> {
    let img = |name: &str| dir.join(format!("reanim/lawnmower_{name}.png"));
    Canvas::new(58, 34)
        .panel([200, 70, 60, 255], [120, 40, 34, 255], 6.0)
        .save(&img("body"))?;
    Canvas::new(18, 18)
        .ellipse([40, 40, 44, 255])
        .save(&img("wheel1"))?;
    const N: usize = 17;
    let shake = |i: usize| phase(i, 4).sin();
    let tracks = vec![
        marker("anim_normal", N, 0..N),
        part("LawnMower_body", "LAWNMOWER_BODY", N, move |i| {
            Some(at(4.0, 8.0 + shake(i)))
        }),
        part("Lawnmower_frontwheel1", "LAWNMOWER_WHEEL1", N, |_| {
            Some(at(8.0, 38.0))
        }),
        part("Lawnmower_frontwheel2", "LAWNMOWER_WHEEL1", N, |_| {
            Some(at(44.0, 38.0))
        }),
    ];
    save_reanim(dir, "lawnmower", 20.0, tracks)
}
