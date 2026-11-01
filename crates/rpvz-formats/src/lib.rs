//! Parsers for the Plants vs. Zombies data formats RPvZ reads.
//!
//! Android release (installed by `rpvz-assets`):
//! - [`rsb`]: `main.rsb` bundles: file lists, packets, texture table, resource
//!   descriptions with atlas rectangles.
//! - [`ptx`]: textures from those packets, decoded to RGBA8.
//! - [`reanim`]: `compiled32/reanim/*.reanim.compiled` skeletal animations.
//! - [`image_ids`]: maps reanim image numbers to image resource ids.
//!
//! No Bevy here: everything is plain data, so it is easy to test.

pub mod error;
pub mod font;
pub mod image_ids;
pub mod particle;
pub mod ptx;
mod read;
pub mod reanim;
pub mod rsb;
mod trie;

pub use error::{FormatError, Result};
pub use font::ImageFont;
pub use image_ids::ImageIdTable;
pub use particle::ParticleDef;
pub use ptx::{PixelFormat, RgbaImage};
pub use reanim::{Frame, ImageRef, Reanim, Track, Variant};
pub use rsb::{Group, Packet, PtxInfo, Resource, Rsb};
