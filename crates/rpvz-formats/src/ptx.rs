//! PTX textures from RSB packets, decoded to RGBA8.
//!
//! All formats seen in the Android release are 16 bits per pixel, little-endian.
//! The "tiled" variants store 32×32 pixel tiles row by row, each tile row-major,
//! with the image padded up to a multiple of 32 in both directions.

use crate::error::{FormatError, Result};
use crate::rsb::PtxInfo;

const TILE: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// id 1
    Rgba4444,
    /// id 2
    Rgb565,
    /// id 3
    Rgba5551,
    /// id 21
    Rgba4444Tiled,
    /// id 22
    Rgb565Tiled,
    /// id 23
    Rgba5551Tiled,
}

impl PixelFormat {
    pub fn from_id(id: u32) -> Option<Self> {
        Some(match id {
            1 => Self::Rgba4444,
            2 => Self::Rgb565,
            3 => Self::Rgba5551,
            21 => Self::Rgba4444Tiled,
            22 => Self::Rgb565Tiled,
            23 => Self::Rgba5551Tiled,
            _ => return None,
        })
    }

    pub fn is_tiled(self) -> bool {
        matches!(
            self,
            Self::Rgba4444Tiled | Self::Rgb565Tiled | Self::Rgba5551Tiled
        )
    }

    fn to_rgba(self, v: u16) -> [u8; 4] {
        let v = u32::from(v);
        let x4 = |s: u32| (((v >> s) & 0xF) * 17) as u8;
        let x5 = |s: u32| {
            let c = (v >> s) & 0x1F;
            ((c << 3) | (c >> 2)) as u8
        };
        let x6 = |s: u32| {
            let c = (v >> s) & 0x3F;
            ((c << 2) | (c >> 4)) as u8
        };
        match self {
            Self::Rgba4444 | Self::Rgba4444Tiled => [x4(12), x4(8), x4(4), x4(0)],
            Self::Rgb565 | Self::Rgb565Tiled => [x5(11), x6(5), x5(0), 255],
            Self::Rgba5551 | Self::Rgba5551Tiled => {
                [x5(11), x5(6), x5(1), if v & 1 != 0 { 255 } else { 0 }]
            }
        }
    }
}

/// Straight (non-premultiplied) RGBA8, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    /// Copies out a sub-rectangle (e.g. a sprite from an atlas).
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Result<RgbaImage> {
        if x.checked_add(width).is_none_or(|r| r > self.width)
            || y.checked_add(height).is_none_or(|b| b > self.height)
        {
            return Err(FormatError::Invalid(format!(
                "crop {x},{y} {width}x{height} outside {}x{}",
                self.width, self.height
            )));
        }
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for row in y..y + height {
            let start = (row as usize * self.width as usize + x as usize) * 4;
            pixels.extend_from_slice(&self.pixels[start..start + width as usize * 4]);
        }
        Ok(RgbaImage {
            width,
            height,
            pixels,
        })
    }
}

pub fn decode(info: &PtxInfo, data: &[u8]) -> Result<RgbaImage> {
    let format = PixelFormat::from_id(info.format)
        .ok_or_else(|| FormatError::Unsupported(format!("PTX pixel format {}", info.format)))?;
    let (w, h) = (info.width as usize, info.height as usize);
    let mut pixels = vec![0u8; w * h * 4];
    let read = |i: usize| u16::from_le_bytes([data[2 * i], data[2 * i + 1]]);

    if format.is_tiled() {
        let tiles_x = w.div_ceil(TILE);
        let tiles_y = h.div_ceil(TILE);
        need(data, tiles_x * tiles_y * TILE * TILE * 2)?;
        for y in 0..h {
            let (ty, py) = (y / TILE, y % TILE);
            for x in 0..w {
                let (tx, px) = (x / TILE, x % TILE);
                let src = ((ty * tiles_x + tx) * TILE + py) * TILE + px;
                let dst = (y * w + x) * 4;
                pixels[dst..dst + 4].copy_from_slice(&format.to_rgba(read(src)));
            }
        }
    } else {
        let pitch = info.pitch as usize;
        if pitch < w * 2 {
            return Err(FormatError::Invalid(format!(
                "PTX pitch {pitch} < width {w} * 2"
            )));
        }
        if h > 0 {
            need(data, pitch * (h - 1) + w * 2)?;
        }
        for y in 0..h {
            for x in 0..w {
                let dst = (y * w + x) * 4;
                pixels[dst..dst + 4].copy_from_slice(&format.to_rgba(read(y * pitch / 2 + x)));
            }
        }
    }
    Ok(RgbaImage {
        width: info.width,
        height: info.height,
        pixels,
    })
}

fn need(data: &[u8], n: usize) -> Result<()> {
    if data.len() < n {
        return Err(FormatError::Truncated {
            what: "PTX data",
            offset: 0,
            needed: n,
            len: data.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(w: u32, h: u32, format: u32) -> PtxInfo {
        PtxInfo {
            width: w,
            height: h,
            pitch: w * 2,
            format,
        }
    }

    #[test]
    fn channel_expansion() {
        assert_eq!(PixelFormat::Rgba4444.to_rgba(0xF08F), [255, 0, 136, 255]);
        assert_eq!(PixelFormat::Rgb565.to_rgba(0xF800), [255, 0, 0, 255]);
        assert_eq!(PixelFormat::Rgb565.to_rgba(0x07E0), [0, 255, 0, 255]);
        assert_eq!(PixelFormat::Rgba5551.to_rgba(0x003F), [0, 0, 255, 255]);
        assert_eq!(PixelFormat::Rgba5551.to_rgba(0x003E), [0, 0, 255, 0]);
    }

    #[test]
    fn linear_respects_pitch() {
        // 2x2 image with 4 bytes of row padding.
        let mut data = Vec::new();
        for row in [[0xF00Fu16, 0x0F0F], [0x00FF, 0xFFFF]] {
            for v in row {
                data.extend_from_slice(&v.to_le_bytes());
            }
            data.extend_from_slice(&[0xAA; 4]);
        }
        let img = decode(
            &PtxInfo {
                width: 2,
                height: 2,
                pitch: 8,
                format: 1,
            },
            &data,
        )
        .unwrap();
        assert_eq!(img.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(img.pixel(1, 0), [0, 255, 0, 255]);
        assert_eq!(img.pixel(0, 1), [0, 0, 255, 255]);
        assert_eq!(img.pixel(1, 1), [255, 255, 255, 255]);
    }

    #[test]
    fn tiled_layout() {
        // 40x33 image: 2x2 tiles. Encode each pixel's coordinates into the value
        // via a tile-ordered buffer and check they come out in place.
        let (w, h) = (40usize, 33usize);
        let mut data = vec![0u8; 2 * 2 * TILE * TILE * 2];
        let code = |x: usize, y: usize| (((x & 0xF) << 12) | ((y & 0xF) << 8) | 0x00F) as u16;
        for y in 0..h {
            for x in 0..w {
                let src = (((y / TILE) * 2 + x / TILE) * TILE + y % TILE) * TILE + x % TILE;
                data[2 * src..2 * src + 2].copy_from_slice(&code(x, y).to_le_bytes());
            }
        }
        let img = decode(&info(w as u32, h as u32, 21), &data).unwrap();
        for (x, y) in [(0, 0), (31, 0), (32, 0), (39, 31), (5, 32), (39, 32)] {
            let expect = PixelFormat::Rgba4444.to_rgba(code(x, y));
            assert_eq!(img.pixel(x as u32, y as u32), expect, "pixel {x},{y}");
        }
    }

    #[test]
    fn rejects_short_data_and_unknown_format() {
        assert!(decode(&info(32, 32, 21), &[0; 100]).is_err());
        assert!(decode(&info(2, 2, 1), &[0; 7]).is_err());
        assert!(matches!(
            decode(&info(1, 1, 99), &[0; 2]),
            Err(FormatError::Unsupported(_))
        ));
    }

    #[test]
    fn crop_bounds() {
        let img = decode(&info(2, 2, 1), &[0xFF; 8]).unwrap();
        assert_eq!(img.crop(1, 1, 1, 1).unwrap().pixels.len(), 4);
        assert!(img.crop(1, 1, 2, 1).is_err());
    }
}
