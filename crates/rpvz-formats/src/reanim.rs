//! Compiled reanimation files (`*.reanim.compiled`), PC and Android variants.
//!
//! Container: `u32 0xDEADFED4`, `u32 uncompressed_size`, zlib data.
//! Inside is the 32-bit in-memory layout of the definition:
//!
//! ```text
//! 0x00 u32 ×2   (pointers, ignored)
//! 0x08 u32      track count
//! 0x0C f32      fps
//! 0x10 u32      (atlas pointer, ignored)
//! 0x14 u32      track struct size: 12 on PC, 16 on Android
//! 0x18          tracks × { pointers..., u32 frame_count } (count is the last field)
//! then per track:
//!   u32 name_len, name
//!   u32 transform size = 44
//!   frames × { f32 x, y, kx, ky, sx, sy, f, a; u32 ×3 (pointers) }
//!   frames × image, then u32 font_len + font, u32 text_len + text
//!     image is a length-prefixed name on PC (`IMAGE_REANIM_...`)
//!     and an i32 number on Android (-1 = none)
//! ```
//!
//! A float of `-10000.0` means "same as the previous frame". Android image
//! numbers are translated with [`crate::image_ids`].

use crate::error::{FormatError, Result};
use crate::read::{Reader, inflate, latin1};

pub const MAGIC: u32 = 0xDEAD_FED4;
const PLACEHOLDER: f32 = -10000.0;
const TRANSFORM_STRUCT: u32 = 44;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// PC release: 12-byte track headers, images by name.
    Pc,
    /// Android `compiled32`: 16-byte track headers, images by number.
    Android,
}

impl Variant {
    fn track_struct(self) -> u32 {
        match self {
            Variant::Pc => 12,
            Variant::Android => 16,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageRef {
    /// Android image number, see [`crate::image_ids`].
    Id(u32),
    /// Resource id such as `IMAGE_REANIM_PEASHOOTER_HEAD`.
    Name(String),
}

impl ImageRef {
    pub fn name(&self) -> Option<&str> {
        match self {
            ImageRef::Name(n) => Some(n),
            ImageRef::Id(_) => None,
        }
    }
}

/// One keyframe as stored: `None` means inherit from the previous frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawFrame {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub kx: Option<f32>,
    pub ky: Option<f32>,
    pub sx: Option<f32>,
    pub sy: Option<f32>,
    /// Visibility: 0 shown, -1 hidden.
    pub f: Option<f32>,
    pub a: Option<f32>,
    pub image: Option<ImageRef>,
    pub font: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub name: String,
    pub frames: Vec<RawFrame>,
}

/// A frame with every field filled in.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    /// Skew angles in degrees.
    pub kx: f32,
    pub ky: f32,
    pub sx: f32,
    pub sy: f32,
    pub f: f32,
    pub a: f32,
    pub image: Option<ImageRef>,
    pub font: Option<String>,
    pub text: Option<String>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            kx: 0.0,
            ky: 0.0,
            sx: 1.0,
            sy: 1.0,
            f: 0.0,
            a: 1.0,
            image: None,
            font: None,
            text: None,
        }
    }
}

impl Track {
    /// Frames with inherited values resolved, starting from [`Frame::default`].
    pub fn resolved(&self) -> Vec<Frame> {
        let mut cur = Frame::default();
        self.frames
            .iter()
            .map(|raw| {
                macro_rules! take {
                    ($($field:ident),*) => {$(
                        if let Some(v) = raw.$field {
                            cur.$field = v;
                        }
                    )*};
                }
                take!(x, y, kx, ky, sx, sy, f, a);
                if raw.image.is_some() {
                    cur.image = raw.image.clone();
                }
                if raw.font.is_some() {
                    cur.font = raw.font.clone();
                }
                if raw.text.is_some() {
                    cur.text = raw.text.clone();
                }
                cur.clone()
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reanim {
    pub fps: f32,
    pub tracks: Vec<Track>,
}

impl Reanim {
    pub fn track(&self, name: &str) -> Option<&Track> {
        self.tracks
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }

    /// Parses a compiled file of either variant.
    pub fn parse_compiled(file: &[u8]) -> Result<Self> {
        Ok(Self::parse_compiled_with_variant(file)?.0)
    }

    /// Like [`Self::parse_compiled`], also reporting which variant it was.
    pub fn parse_compiled_with_variant(file: &[u8]) -> Result<(Self, Variant)> {
        let mut r = Reader::new(file, "reanim container");
        let magic = r.u32()?;
        if magic != MAGIC {
            return Err(FormatError::BadMagic {
                what: "compiled reanim",
                found: magic.to_le_bytes().to_vec(),
            });
        }
        let size = r.usize()?;
        let data = inflate(&file[8..], size, "compiled reanim")?;
        let variant = match Reader::at(&data, 0x14, "reanim header").u32()? {
            12 => Variant::Pc,
            16 => Variant::Android,
            other => {
                return Err(FormatError::Unsupported(format!(
                    "reanim track struct size {other}"
                )));
            }
        };
        Ok((Self::parse_definition(&data, variant)?, variant))
    }

    fn parse_definition(data: &[u8], variant: Variant) -> Result<Self> {
        let mut r = Reader::at(data, 8, "reanim header");
        let track_count = r.usize()?;
        let fps = r.f32()?;
        let _atlas = r.u32()?;
        let _track_struct = r.u32()?;
        let skip = variant.track_struct() as usize - 4;
        let mut counts = Vec::with_capacity(track_count.min(1024));
        for _ in 0..track_count {
            r.bytes(skip)?;
            counts.push(r.usize()?);
        }

        let mut tracks = Vec::with_capacity(counts.len());
        for count in counts {
            let name_len = r.usize()?;
            let name = latin1(r.bytes(name_len)?);
            let transform_struct = r.u32()?;
            if transform_struct != TRANSFORM_STRUCT {
                return Err(FormatError::Unsupported(format!(
                    "reanim transform struct size {transform_struct}"
                )));
            }
            if count > data.len() / TRANSFORM_STRUCT as usize {
                return Err(FormatError::Invalid(format!(
                    "track {name}: {count} frames"
                )));
            }
            let mut frames = Vec::with_capacity(count);
            for _ in 0..count {
                let mut v = [0f32; 8];
                for slot in &mut v {
                    *slot = r.f32()?;
                }
                r.bytes(12)?;
                let opt = |x: f32| (x != PLACEHOLDER).then_some(x);
                frames.push(RawFrame {
                    x: opt(v[0]),
                    y: opt(v[1]),
                    kx: opt(v[2]),
                    ky: opt(v[3]),
                    sx: opt(v[4]),
                    sy: opt(v[5]),
                    f: opt(v[6]),
                    a: opt(v[7]),
                    ..RawFrame::default()
                });
            }
            for frame in &mut frames {
                frame.image = match variant {
                    Variant::Android => u32::try_from(r.i32()?).ok().map(ImageRef::Id),
                    Variant::Pc => {
                        let len = r.usize()?;
                        let name = latin1(r.bytes(len)?);
                        (!name.is_empty()).then_some(ImageRef::Name(name))
                    }
                };
                let font_len = r.usize()?;
                let font = latin1(r.bytes(font_len)?);
                let text_len = r.usize()?;
                let text = latin1(r.bytes(text_len)?);
                frame.font = (!font.is_empty()).then_some(font);
                frame.text = (!text.is_empty()).then_some(text);
            }
            tracks.push(Track { name, frames });
        }
        if !r.is_at_end() {
            return Err(FormatError::Invalid(format!(
                "compiled reanim: {} trailing bytes",
                data.len() - r.pos()
            )));
        }
        Ok(Self { fps, tracks })
    }

    /// Multiplies every stored x/y by `factor` (e.g. Android → PC units).
    pub fn scale_positions(&mut self, factor: f32) {
        for frame in self.tracks.iter_mut().flat_map(|t| &mut t.frames) {
            if let Some(x) = &mut frame.x {
                *x *= factor;
            }
            if let Some(y) = &mut frame.y {
                *y *= factor;
            }
        }
    }

    /// Replaces numeric image refs with names. Returns the first id `lookup`
    /// could not resolve, leaving the reanim partly converted.
    pub fn name_images(&mut self, lookup: impl Fn(u32) -> Option<String>) -> Result<(), u32> {
        for frame in self.tracks.iter_mut().flat_map(|t| &mut t.frames) {
            if let Some(ImageRef::Id(id)) = frame.image {
                frame.image = Some(ImageRef::Name(lookup(id).ok_or(id)?));
            }
        }
        Ok(())
    }

    /// Every image name used by any frame.
    pub fn image_names(&self) -> std::collections::BTreeSet<&str> {
        self.tracks
            .iter()
            .flat_map(|t| &t.frames)
            .filter_map(|f| f.image.as_ref()?.name())
            .collect()
    }
}

/// Builds a compiled file. Every image must already be a [`ImageRef::Name`]
/// for [`Variant::Pc`] and a [`ImageRef::Id`] for [`Variant::Android`].
pub fn write_compiled(reanim: &Reanim, variant: Variant) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut d = Vec::new();
    let put = |d: &mut Vec<u8>, v: u32| d.extend_from_slice(&v.to_le_bytes());
    let put_str = |d: &mut Vec<u8>, s: &str| {
        d.extend_from_slice(&(s.len() as u32).to_le_bytes());
        d.extend(s.chars().map(|c| c as u8));
    };
    put(&mut d, 0);
    put(&mut d, 0);
    put(&mut d, reanim.tracks.len() as u32);
    d.extend_from_slice(&reanim.fps.to_le_bytes());
    put(&mut d, 0);
    put(&mut d, variant.track_struct());
    for t in &reanim.tracks {
        for _ in 0..(variant.track_struct() / 4 - 1) {
            put(&mut d, 0);
        }
        put(&mut d, t.frames.len() as u32);
    }
    for t in &reanim.tracks {
        put_str(&mut d, &t.name);
        put(&mut d, TRANSFORM_STRUCT);
        for f in &t.frames {
            for v in [f.x, f.y, f.kx, f.ky, f.sx, f.sy, f.f, f.a] {
                d.extend_from_slice(&v.unwrap_or(PLACEHOLDER).to_le_bytes());
            }
            for _ in 0..3 {
                put(&mut d, 0);
            }
        }
        for f in &t.frames {
            match (variant, &f.image) {
                (Variant::Pc, None) => put_str(&mut d, ""),
                (Variant::Pc, Some(ImageRef::Name(n))) => put_str(&mut d, n),
                (Variant::Android, None) => d.extend_from_slice(&(-1i32).to_le_bytes()),
                (Variant::Android, Some(ImageRef::Id(id))) => put(&mut d, *id),
                (_, Some(other)) => {
                    return Err(FormatError::Invalid(format!(
                        "track {}: image {other:?} not valid for {variant:?}",
                        t.name
                    )));
                }
            }
            put_str(&mut d, f.font.as_deref().unwrap_or(""));
            put_str(&mut d, f.text.as_deref().unwrap_or(""));
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&(d.len() as u32).to_le_bytes());
    let mut z = flate2::write::ZlibEncoder::new(out, flate2::Compression::default());
    z.write_all(&d)
        .and_then(|_| z.finish())
        .map_err(|e| FormatError::Zlib(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(image: ImageRef) -> Reanim {
        Reanim {
            fps: 12.0,
            tracks: vec![
                Track {
                    name: "anim_idle".into(),
                    frames: vec![
                        RawFrame {
                            f: Some(0.0),
                            ..RawFrame::default()
                        },
                        RawFrame::default(),
                    ],
                },
                Track {
                    name: "head".into(),
                    frames: vec![
                        RawFrame {
                            x: Some(10.0),
                            y: Some(-3.5),
                            image: Some(image),
                            ..RawFrame::default()
                        },
                        RawFrame {
                            x: Some(11.0),
                            sx: Some(0.5),
                            text: Some("hi".into()),
                            ..RawFrame::default()
                        },
                    ],
                },
            ],
        }
    }

    #[test]
    fn roundtrip_both_variants() {
        for (variant, image) in [
            (Variant::Android, ImageRef::Id(532)),
            (Variant::Pc, ImageRef::Name("IMAGE_REANIM_TEST_HEAD".into())),
        ] {
            let r = sample(image);
            let file = write_compiled(&r, variant).unwrap();
            let (back, detected) = Reanim::parse_compiled_with_variant(&file).unwrap();
            assert_eq!(detected, variant);
            assert_eq!(back, r);
        }
    }

    #[test]
    fn writer_rejects_wrong_image_kind() {
        let r = sample(ImageRef::Id(1));
        assert!(write_compiled(&r, Variant::Pc).is_err());
    }

    #[test]
    fn convert_android_to_pc() {
        let mut r = sample(ImageRef::Id(532));
        r.scale_positions(2.0);
        assert_eq!(r.tracks[1].frames[0].x, Some(20.0));
        assert_eq!(r.tracks[1].frames[0].y, Some(-7.0));
        assert_eq!(
            r.tracks[0].frames[0].x, None,
            "placeholders stay placeholders"
        );
        assert_eq!(r.name_images(|_| None), Err(532));
        r.name_images(|id| Some(format!("IMAGE_{id}"))).unwrap();
        assert_eq!(
            r.image_names().into_iter().collect::<Vec<_>>(),
            vec!["IMAGE_532"]
        );
        let file = write_compiled(&r, Variant::Pc).unwrap();
        assert_eq!(Reanim::parse_compiled(&file).unwrap(), r);
    }

    #[test]
    fn resolve_inherits_previous_values() {
        let r = sample(ImageRef::Id(532));
        let frames = r.track("HEAD").unwrap().resolved();
        assert_eq!((frames[0].x, frames[0].y, frames[0].sx), (10.0, -3.5, 1.0));
        assert_eq!((frames[1].x, frames[1].y, frames[1].sx), (11.0, -3.5, 0.5));
        assert_eq!(
            frames[1].image,
            Some(ImageRef::Id(532)),
            "image carries over"
        );
        assert_eq!(frames[1].text.as_deref(), Some("hi"));
    }

    #[test]
    fn rejects_bad_input() {
        let mut file = write_compiled(&sample(ImageRef::Id(1)), Variant::Android).unwrap();
        assert!(Reanim::parse_compiled(&file[..20]).is_err());
        file[0] ^= 1;
        assert!(matches!(
            Reanim::parse_compiled(&file),
            Err(FormatError::BadMagic { .. })
        ));
    }
}
