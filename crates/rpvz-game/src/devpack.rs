//! A placeholder pack with original, deliberately plain art: flat panels and
//! shapes in the PC data layout, using the track and image names the engine
//! expects. It lets the engine run (and be screenshotted in CI) without any
//! game files.

use std::path::Path;

use rpvz_formats::reanim::{ImageRef, RawFrame, Reanim, Track, Variant, write_compiled};

type Rgba = [u8; 4];

pub(crate) struct Canvas {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Canvas {
    pub(crate) fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            px: vec![0; (w * h * 4) as usize],
        }
    }

    pub(crate) fn set(&mut self, x: u32, y: u32, c: Rgba) {
        if x < self.w && y < self.h {
            let i = ((y * self.w + x) * 4) as usize;
            self.px[i..i + 4].copy_from_slice(&c);
        }
    }

    pub(crate) fn get(&self, x: u32, y: u32) -> Rgba {
        if x < self.w && y < self.h {
            let i = ((y * self.w + x) * 4) as usize;
            [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
        } else {
            [0; 4]
        }
    }

    /// Rounded rectangle with a darker border.
    pub(crate) fn panel(mut self, fill: Rgba, border: Rgba, radius: f32) -> Self {
        let (w, h) = (self.w as f32, self.h as f32);
        for y in 0..self.h {
            for x in 0..self.w {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let dx = (radius - fx).max(fx - (w - radius)).max(0.0);
                let dy = (radius - fy).max(fy - (h - radius)).max(0.0);
                let d = (dx * dx + dy * dy).sqrt();
                if d <= radius {
                    let edge =
                        fx < 4.0 || fy < 4.0 || fx > w - 4.0 || fy > h - 4.0 || d > radius - 4.0;
                    self.set(x, y, if edge { border } else { fill });
                }
            }
        }
        self
    }

    pub(crate) fn ellipse(mut self, fill: Rgba) -> Self {
        let (rx, ry) = (self.w as f32 / 2.0, self.h as f32 / 2.0);
        for y in 0..self.h {
            for x in 0..self.w {
                let (dx, dy) = ((x as f32 + 0.5 - rx) / rx, (y as f32 + 0.5 - ry) / ry);
                if dx * dx + dy * dy <= 1.0 {
                    self.set(x, y, fill);
                }
            }
        }
        self
    }

    /// Vertical gradient with faint horizontal bands.
    pub(crate) fn backdrop(mut self, top: Rgba, bottom: Rgba) -> Self {
        for y in 0..self.h {
            let t = y as f32 / self.h as f32;
            let band = if (y / 40) % 2 == 0 { 0.0 } else { 6.0 };
            let c = [0, 1, 2].map(|i| {
                (f32::from(top[i]) * (1.0 - t) + f32::from(bottom[i]) * t + band).min(255.0) as u8
            });
            for x in 0..self.w {
                self.set(x, y, [c[0], c[1], c[2], 255]);
            }
        }
        self
    }

    pub(crate) fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        image::save_buffer(
            path,
            &self.px,
            self.w,
            self.h,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Selector-screen frames: 0..30 open, 30..90 idle.
const OPEN: usize = 30;
const IDLE: usize = 60;

fn marker(name: &str, range: std::ops::Range<usize>) -> Track {
    Track {
        name: name.into(),
        frames: (0..OPEN + IDLE)
            .map(|i| RawFrame {
                f: Some(if range.contains(&i) { 0.0 } else { -1.0 }),
                ..RawFrame::default()
            })
            .collect(),
    }
}

/// A track whose position is `pos(frame)`, rotation `rot(frame)` degrees and
/// skew `skew(frame)` (added to `ky` only).
fn moving(
    name: &str,
    image: &str,
    pos: impl Fn(usize) -> (f32, f32),
    rot: impl Fn(usize) -> f32,
    skew: impl Fn(usize) -> f32,
) -> Track {
    Track {
        name: name.into(),
        frames: (0..OPEN + IDLE)
            .map(|i| {
                let (x, y) = pos(i);
                RawFrame {
                    x: Some(x),
                    y: Some(y),
                    kx: Some(rot(i)),
                    ky: Some(rot(i) + skew(i)),
                    sx: Some(1.0),
                    sy: Some(1.0),
                    f: Some(0.0),
                    a: Some(1.0),
                    image: (i == 0).then(|| ImageRef::Name(image.into())),
                    ..RawFrame::default()
                }
            })
            .collect(),
    }
}

fn ease_in(i: usize, delay: usize) -> f32 {
    let t = (i.saturating_sub(delay) as f32 / (OPEN - delay) as f32).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn write(dir: &Path) -> Result<(), String> {
    let img = |name: &str| dir.join(format!("reanim/selectorscreen_{name}.png"));
    Canvas::new(800, 600)
        .backdrop([24, 34, 64, 255], [38, 92, 70, 255])
        .save(&img("bg"))?;
    Canvas::new(180, 70)
        .ellipse([220, 226, 240, 200])
        .save(&img("cloud1"))?;
    Canvas::new(150, 120)
        .panel([150, 110, 70, 255], [90, 60, 35, 255], 10.0)
        .save(&img("woodsign1"))?;
    Canvas::new(120, 34)
        .panel([170, 130, 85, 255], [90, 60, 35, 255], 8.0)
        .save(&img("woodsign2"))?;
    crate::devpack_ui::write(dir)?;
    crate::devpack_board::write(dir)?;

    let buttons: [(&str, &str, &str, Rgba); 4] = [
        (
            "StartAdventure",
            "startadventure_button1",
            "startadventure_highlight",
            [70, 150, 220, 255],
        ),
        (
            "Survival",
            "survival_button",
            "survival_highlight",
            [220, 140, 60, 255],
        ),
        (
            "Challenges",
            "challenges_button",
            "challenges_highlight",
            [160, 90, 200, 255],
        ),
        (
            "ZenGarden",
            "vasebreaker_button",
            "vasebreaker_highlight",
            [70, 180, 110, 255],
        ),
    ];
    let mut tracks = vec![
        marker("anim_open", 0..OPEN),
        marker("anim_idle", OPEN..OPEN + IDLE),
    ];
    tracks.push(moving(
        "SelectorScreen_BG",
        "IMAGE_REANIM_SELECTORSCREEN_BG",
        |_| (0.0, 0.0),
        |_| 0.0,
        |_| 0.0,
    ));
    tracks.push(moving(
        "Cloud1",
        "IMAGE_REANIM_SELECTORSCREEN_CLOUD1",
        |i| (40.0 + i as f32 * 2.0, 60.0),
        |_| 0.0,
        |_| 0.0,
    ));
    // A sign that swings in with skew, to exercise kx != ky.
    tracks.push(moving(
        "woodsign1",
        "IMAGE_REANIM_SELECTORSCREEN_WOODSIGN1",
        |i| (60.0, -140.0 + 230.0 * ease_in(i, 0)),
        |i| {
            if i < OPEN {
                0.0
            } else {
                3.0 * ((i - OPEN) as f32 * 0.21).sin()
            }
        },
        |i| {
            if i < OPEN {
                0.0
            } else {
                6.0 * ((i - OPEN) as f32 * 0.21).sin()
            }
        },
    ));
    // A small plank under the sign: clicking it renames the player.
    tracks.push(moving(
        "woodsign2",
        "IMAGE_REANIM_SELECTORSCREEN_WOODSIGN2",
        |i| (75.0, -20.0 + 230.0 * ease_in(i, 0)),
        |_| 0.0,
        |_| 0.0,
    ));
    for (n, (track, file, highlight, color)) in buttons.iter().enumerate() {
        let border = color.map(|c| c / 2);
        let light = color.map(|c| c.saturating_add(60));
        Canvas::new(300, 80)
            .panel(*color, [border[0], border[1], border[2], 255], 18.0)
            .save(&img(file))?;
        Canvas::new(300, 80)
            .panel(
                [light[0], light[1], light[2], 255],
                [255, 255, 255, 255],
                18.0,
            )
            .save(&img(highlight))?;
        let y = 90.0 + n as f32 * 100.0;
        let delay = n * 4;
        tracks.push(moving(
            &format!("SelectorScreen_{track}_button"),
            &format!("IMAGE_REANIM_SELECTORSCREEN_{}", file.to_ascii_uppercase()),
            move |i| {
                let bob = if i >= OPEN {
                    2.0 * ((i - OPEN) as f32 * 0.3 + n as f32).sin()
                } else {
                    0.0
                };
                (430.0 + 420.0 * (1.0 - ease_in(i, delay)), y + bob)
            },
            move |i| -8.0 * (1.0 - ease_in(i, delay)),
            |_| 0.0,
        ));
    }

    let reanim = Reanim { fps: 20.0, tracks };
    let path = dir.join("compiled/reanim/selectorscreen.reanim.compiled");
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| e.to_string())?;
    let bytes = write_compiled(&reanim, Variant::Pc).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}
