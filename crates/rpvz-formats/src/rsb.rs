//! PopCap Resource Stream Bundle (`main.rsb`, magic `1bsr`, version 3), as
//! shipped in the Android release.
//!
//! An RSB holds:
//! - a list of all file paths and which packet (RSG) holds each,
//! - packets (`pgsr`), each with a general part (sounds, fonts) and an image
//!   part (`.PTX` textures), either of which may be zlib-compressed,
//! - a texture table (size, pitch, pixel format),
//! - a resource description: groups → per-resolution/locale subgroups →
//!   resources with ids, paths, atlas rectangles and properties.

use crate::error::{FormatError, Result};
use crate::read::{Reader, cstr_at, inflate, slice};
use crate::trie;

const MAGIC: &[u8; 4] = b"1bsr";
const PACKET_MAGIC: &[u8; 4] = b"pgsr";
const RSG_NAME_LEN: usize = 128;
const RSG_INFO_MIN: usize = RSG_NAME_LEN + 19 * 4;

/// Texture dimensions and pixel format from the RSB texture table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtxInfo {
    pub width: u32,
    pub height: u32,
    /// Bytes per row for linear formats.
    pub pitch: u32,
    /// Raw format id; see [`crate::ptx::PixelFormat`].
    pub format: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsbFile {
    /// Path as stored, upper-case with `\` separators.
    pub path: String,
    /// Index into [`Rsb::packets`].
    pub packet: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketInfo {
    pub name: String,
    pub offset: usize,
    pub length: usize,
    /// Index of the packet's first texture in [`Rsb::textures`].
    pub ptx_before: u32,
    pub ptx_count: u32,
}

#[derive(Debug)]
pub struct Rsb {
    data: Vec<u8>,
    pub files: Vec<RsbFile>,
    pub packets: Vec<PacketInfo>,
    pub textures: Vec<PtxInfo>,
    desc: DescRegions,
}

#[derive(Debug, Clone, Copy)]
struct DescRegions {
    groups: usize,
    resources: usize,
    strings: usize,
    end: usize,
}

impl Rsb {
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        let mut r = Reader::new(&data, "RSB header");
        let magic = r.bytes(4)?;
        if magic != MAGIC {
            return Err(FormatError::BadMagic {
                what: "RSB",
                found: magic.to_vec(),
            });
        }
        let version = r.u32()?;
        if version != 3 {
            return Err(FormatError::Unsupported(format!("RSB version {version}")));
        }
        // h[i] is the u32 at offset 0x08 + 4 * i.
        let mut h = [0usize; 25];
        for v in &mut h {
            *v = r.usize()?;
        }
        let data_offset = h[1];
        let (file_list_len, file_list_begin) = (h[2], h[3]);
        let (rsg_count, rsg_info_begin, rsg_info_size) = (h[8], h[9], h[10]);
        let (ptx_count, ptx_info_begin, ptx_info_size) = (h[19], h[20], h[21]);
        let desc = DescRegions {
            groups: h[22],
            resources: h[23],
            strings: h[24],
            end: data_offset.min(data.len()),
        };
        if rsg_info_size < RSG_INFO_MIN || ptx_info_size < 16 {
            return Err(FormatError::Unsupported(format!(
                "RSB entry sizes rsg={rsg_info_size} ptx={ptx_info_size}"
            )));
        }
        if !(desc.groups <= desc.resources
            && desc.resources <= desc.strings
            && desc.strings <= desc.end)
        {
            return Err(FormatError::Invalid("RSB description offsets".into()));
        }

        let files = trie::decode(&data, file_list_begin, file_list_len, |r| r.u32())?
            .into_iter()
            .map(|(path, packet)| RsbFile { path, packet })
            .collect::<Vec<_>>();

        let mut packets = Vec::with_capacity(rsg_count);
        for i in 0..rsg_count {
            let base = rsg_info_begin + i * rsg_info_size;
            let name = cstr_at(slice(&data, base, RSG_NAME_LEN, "RSG info")?, 0, "RSG name")?;
            let mut r = Reader::at(&data, base + RSG_NAME_LEN, "RSG info");
            let offset = r.usize()?;
            let length = r.usize()?;
            let mut rest = [0u32; 17];
            for v in &mut rest {
                *v = r.u32()?;
            }
            slice(&data, offset, length, "RSG packet")?;
            packets.push(PacketInfo {
                name,
                offset,
                length,
                ptx_count: rest[15],
                ptx_before: rest[16],
            });
        }

        let mut textures = Vec::with_capacity(ptx_count);
        for i in 0..ptx_count {
            let mut r = Reader::at(&data, ptx_info_begin + i * ptx_info_size, "PTX info");
            textures.push(PtxInfo {
                width: r.u32()?,
                height: r.u32()?,
                pitch: r.u32()?,
                format: r.u32()?,
            });
        }

        for f in &files {
            if f.packet as usize >= packets.len() {
                return Err(FormatError::Invalid(format!(
                    "{} points to packet {}",
                    f.path, f.packet
                )));
            }
        }
        Ok(Self {
            data,
            files,
            packets,
            textures,
            desc,
        })
    }

    pub fn packet_index(&self, name: &str) -> Option<usize> {
        self.packets
            .iter()
            .position(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// Parses packet `index` and inflates its parts.
    pub fn packet(&self, index: usize) -> Result<Packet> {
        let info = self
            .packets
            .get(index)
            .ok_or_else(|| FormatError::Invalid(format!("packet index {index}")))?;
        let raw = slice(&self.data, info.offset, info.length, "RSG packet")?;
        Packet::parse(raw, |i| {
            self.textures
                .get(info.ptx_before as usize + i as usize)
                .copied()
                .ok_or_else(|| {
                    FormatError::Invalid(format!("{}: texture {i} out of range", info.name))
                })
        })
    }

    /// Parses the resource description (groups, subgroups, resources).
    pub fn description(&self) -> Result<Vec<Group>> {
        parse_description(&self.data, self.desc)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// General data (sounds, fonts) in part 0.
    File,
    /// Texture in part 1.
    Texture { index: u32, info: PtxInfo },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketEntry {
    pub path: String,
    pub offset: usize,
    pub size: usize,
    pub kind: EntryKind,
}

/// One RSG packet with its parts inflated.
#[derive(Debug)]
pub struct Packet {
    pub entries: Vec<PacketEntry>,
    general: Vec<u8>,
    images: Vec<u8>,
}

impl Packet {
    fn parse(raw: &[u8], mut texture: impl FnMut(u32) -> Result<PtxInfo>) -> Result<Self> {
        let mut r = Reader::new(raw, "RSG header");
        let magic = r.bytes(4)?;
        if magic != PACKET_MAGIC {
            return Err(FormatError::BadMagic {
                what: "RSG",
                found: magic.to_vec(),
            });
        }
        let version = r.u32()?;
        if version != 3 {
            return Err(FormatError::Unsupported(format!("RSG version {version}")));
        }
        let mut r = Reader::at(raw, 0x10, "RSG header");
        let flags = r.u32()?;
        let _header_len = r.u32()?;
        let part0 = (r.usize()?, r.usize()?, r.usize()?);
        let _ = r.u32()?;
        let part1 = (r.usize()?, r.usize()?, r.usize()?);
        let mut r = Reader::at(raw, 0x48, "RSG header");
        let list_len = r.usize()?;
        let list_begin = r.usize()?;

        let general = read_part(raw, part0, flags & 2 != 0, "RSG general part")?;
        let images = read_part(raw, part1, flags & 1 != 0, "RSG image part")?;

        let raw_entries = trie::decode(raw, list_begin, list_len, |r| {
            let kind = r.u32()?;
            let offset = r.usize()?;
            let size = r.usize()?;
            if kind == 1 {
                let index = r.u32()?;
                r.bytes(8)?;
                let _width = r.u32()?;
                let _height = r.u32()?;
                Ok((offset, size, Some(index)))
            } else {
                Ok((offset, size, None))
            }
        })?;

        let mut entries = Vec::with_capacity(raw_entries.len());
        for (path, (offset, size, tex)) in raw_entries {
            let (kind, part) = match tex {
                Some(index) => (
                    EntryKind::Texture {
                        index,
                        info: texture(index)?,
                    },
                    &images,
                ),
                None => (EntryKind::File, &general),
            };
            slice(part, offset, size, "RSG entry")?;
            entries.push(PacketEntry {
                path,
                offset,
                size,
                kind,
            });
        }
        Ok(Self {
            entries,
            general,
            images,
        })
    }

    /// Bytes of one entry.
    pub fn data(&self, entry: &PacketEntry) -> Result<&[u8]> {
        let part = match entry.kind {
            EntryKind::File => &self.general,
            EntryKind::Texture { .. } => &self.images,
        };
        slice(part, entry.offset, entry.size, "RSG entry")
    }

    pub fn entry(&self, path: &str) -> Option<&PacketEntry> {
        self.entries
            .iter()
            .find(|e| e.path.eq_ignore_ascii_case(path))
    }
}

fn read_part(
    raw: &[u8],
    (offset, zlen, size): (usize, usize, usize),
    compressed: bool,
    what: &str,
) -> Result<Vec<u8>> {
    if size == 0 {
        return Ok(Vec::new());
    }
    if compressed && zlen != size {
        let z = slice(raw, offset, zlen, "RSG part")?;
        inflate(z, size, what)
    } else {
        Ok(slice(raw, offset, size, "RSG part")?.to_vec())
    }
}

// ---------------------------------------------------------------------------
// Resource description

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: String,
    pub subgroups: Vec<Subgroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subgroup {
    pub id: String,
    /// Art resolution (480 or 960), 0 for resolution-independent data.
    pub resolution: u32,
    /// Four-letter locale such as `ENUS`, `None` for shared data.
    pub locale: Option<String>,
    pub resources: Vec<Resource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Image,
    Sound,
    Font,
    Other(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub kind: ResourceKind,
    /// Full id, e.g. `IMAGE_FOO|480` or `IMAGE_FOO|480||454e5553`.
    pub id: String,
    pub path: String,
    pub image: Option<ImageInfo>,
    pub properties: Vec<(String, String)>,
}

impl Resource {
    /// Id without the `|resolution|locale` suffix.
    pub fn base_id(&self) -> &str {
        self.id.split('|').next().unwrap_or(&self.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    /// Stored as its own texture.
    Standalone,
    /// A texture other images are cut from.
    Atlas,
    /// A rectangle inside an atlas.
    Sprite,
    Other(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInfo {
    pub kind: ImageKind,
    pub flags: u16,
    /// Unverified: offsets applied when drawing.
    pub ax: i16,
    pub ay: i16,
    pub x: u16,
    pub y: u16,
    /// Sprite size, or the full size for atlases.
    pub width: u16,
    pub height: u16,
    pub rows: u16,
    pub cols: u16,
    /// For sprites: base id of the atlas they live in.
    pub parent: Option<String>,
}

fn parse_description(data: &[u8], d: DescRegions) -> Result<Vec<Group>> {
    let strings = slice(data, d.strings, d.end - d.strings, "description strings")?;
    let resources = slice(
        data,
        d.resources,
        d.strings - d.resources,
        "description resources",
    )?;
    let s = |off: usize| cstr_at(strings, off, "description string");

    let mut groups = Vec::new();
    let mut r = Reader::at(data, d.groups, "description groups");
    while r.pos() < d.resources {
        let id = s(r.usize()?)?;
        let sub_count = r.usize()?;
        let _sub_size = r.u32()?;
        let mut subgroups = Vec::with_capacity(sub_count.min(64));
        for _ in 0..sub_count {
            let resolution = r.u32()?;
            let locale = r.u32()?;
            let sub_id = s(r.usize()?)?;
            let res_count = r.usize()?;
            let mut list = Vec::with_capacity(res_count.min(4096));
            for _ in 0..res_count {
                list.push(parse_resource(resources, r.usize()?, &s)?);
            }
            subgroups.push(Subgroup {
                id: sub_id,
                resolution,
                locale: (locale != 0).then(|| crate::read::latin1(&locale.to_be_bytes())),
                resources: list,
            });
        }
        groups.push(Group { id, subgroups });
    }
    Ok(groups)
}

fn parse_resource(
    res: &[u8],
    offset: usize,
    s: &impl Fn(usize) -> Result<String>,
) -> Result<Resource> {
    let mut r = Reader::at(res, offset, "resource");
    let _type = r.u32()?;
    let kind = match r.u16()? {
        0 => ResourceKind::Image,
        1 => ResourceKind::Sound,
        2 => ResourceKind::Font,
        other => ResourceKind::Other(other),
    };
    let _header_len = r.u16()?;
    let props_at = r.usize()?;
    let image_at = r.usize()?;
    let id = s(r.usize()?)?;
    let path = s(r.usize()?)?;
    let prop_count = r.usize()?;

    let image = if image_at != 0 && kind == ResourceKind::Image {
        let mut r = Reader::at(res, image_at, "image info");
        let kind = match r.u16()? {
            0 => ImageKind::Standalone,
            2 => ImageKind::Atlas,
            4 => ImageKind::Sprite,
            other => ImageKind::Other(other),
        };
        let flags = r.u16()?;
        let ax = r.i16()?;
        let ay = r.i16()?;
        let (x, y, width, height) = (r.u16()?, r.u16()?, r.u16()?, r.u16()?);
        let (rows, cols) = (r.u16()?, r.u16()?);
        let parent_off = usize::from(r.u16()?) | usize::from(r.u16()?) << 16;
        let parent = if kind == ImageKind::Sprite {
            Some(s(parent_off)?)
        } else {
            None
        };
        Some(ImageInfo {
            kind,
            flags,
            ax,
            ay,
            x,
            y,
            width,
            height,
            rows,
            cols,
            parent,
        })
    } else {
        None
    };

    let mut properties = Vec::with_capacity(prop_count.min(64));
    let mut r = Reader::at(res, props_at, "resource properties");
    for _ in 0..prop_count {
        let key = s(r.usize()?)?;
        let _ = r.u32()?;
        let value = s(r.usize()?)?;
        properties.push((key, value));
    }
    Ok(Resource {
        kind,
        id,
        path,
        image,
        properties,
    })
}
