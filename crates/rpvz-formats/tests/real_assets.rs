//! Checks against installed game data. Skipped unless `RPVZ_TEST_ASSETS`
//! points at an Android install (the `assets/` dir written by rpvz-assets).

#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::PathBuf;

use rpvz_formats::rsb::{EntryKind, ImageKind, ResourceKind};
use rpvz_formats::{ImageIdTable, Reanim, RgbaImage, Rsb, ptx};

fn assets() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("RPVZ_TEST_ASSETS")?);
    dir.join("main.rsb").is_file().then_some(dir)
}

macro_rules! require_assets {
    () => {
        match assets() {
            Some(d) => d,
            None => {
                eprintln!("RPVZ_TEST_ASSETS not set; skipping");
                return;
            }
        }
    };
}

fn load_rsb(dir: &std::path::Path) -> Rsb {
    Rsb::parse(std::fs::read(dir.join("main.rsb")).unwrap()).unwrap()
}

#[test]
fn rsb_structure() {
    let dir = require_assets!();
    let rsb = load_rsb(&dir);
    assert_eq!(rsb.files.len(), 451);
    assert_eq!(rsb.packets.len(), 222);
    assert_eq!(rsb.textures.len(), 272);

    let mut kinds = HashMap::new();
    for i in 0..rsb.packets.len() {
        let p = rsb.packet(i).unwrap();
        for e in &p.entries {
            assert_eq!(p.data(e).unwrap().len(), e.size);
            *kinds
                .entry(matches!(e.kind, EntryKind::Texture { .. }))
                .or_insert(0) += 1;
        }
    }
    assert_eq!(kinds[&true], 272, "textures");
    assert_eq!(kinds[&false], 179, "sounds + fonts");

    let groups = rsb.description().unwrap();
    assert_eq!(groups.len(), 64);
    let all: Vec<_> = groups
        .iter()
        .flat_map(|g| &g.subgroups)
        .flat_map(|s| &s.resources)
        .collect();
    assert_eq!(
        all.iter().filter(|r| r.kind == ResourceKind::Sound).count(),
        163
    );
    assert_eq!(
        all.iter().filter(|r| r.kind == ResourceKind::Font).count(),
        16
    );
}

fn decode_all(rsb: &Rsb) -> HashMap<String, RgbaImage> {
    let mut out = HashMap::new();
    for i in 0..rsb.packets.len() {
        let p = rsb.packet(i).unwrap();
        for e in &p.entries {
            if let EntryKind::Texture { info, .. } = e.kind {
                let img = ptx::decode(&info, p.data(e).unwrap()).unwrap();
                out.insert(e.path.clone(), img);
            }
        }
    }
    out
}

/// Sprites exist at 480 and 960; a correct decoder makes the 960 version,
/// halved, match the 480 one.
#[test]
fn texture_decoding_matches_across_resolutions() {
    let dir = require_assets!();
    let rsb = load_rsb(&dir);
    let textures = decode_all(&rsb);
    assert_eq!(textures.len(), 272);

    let groups = rsb.description().unwrap();
    // (resolution, base id) -> (atlas texture path, x, y, w, h)
    let mut rects = HashMap::new();
    for sub in groups
        .iter()
        .flat_map(|g| &g.subgroups)
        .filter(|s| s.locale.is_none())
    {
        let atlases: HashMap<&str, &str> = sub
            .resources
            .iter()
            .filter(|r| r.image.as_ref().is_some_and(|i| i.kind == ImageKind::Atlas))
            .map(|r| (r.base_id(), r.path.as_str()))
            .collect();
        for r in &sub.resources {
            let Some(img) = &r.image else { continue };
            let Some(atlas) = img.parent.as_deref().and_then(|p| atlases.get(p)) else {
                continue;
            };
            let path = format!("{}.PTX", atlas.to_ascii_uppercase());
            rects.insert(
                (sub.resolution, r.base_id().to_owned()),
                (path, img.x, img.y, img.width, img.height),
            );
        }
    }

    let mut checked = 0;
    let mut good = 0;
    let mut ids: Vec<_> = rects
        .keys()
        .filter(|(res, _)| *res == 480)
        .map(|(_, id)| id.clone())
        .collect();
    ids.sort();
    for id in ids {
        let (Some(a), Some(b)) = (rects.get(&(480, id.clone())), rects.get(&(960, id.clone())))
        else {
            continue;
        };
        if a.3 < 16 || a.4 < 16 {
            continue;
        }
        let small = textures[&a.0]
            .crop(a.1.into(), a.2.into(), a.3.into(), a.4.into())
            .unwrap();
        let big = textures[&b.0]
            .crop(b.1.into(), b.2.into(), b.3.into(), b.4.into())
            .unwrap();
        // Compare alpha, which is never flat for sprites.
        let corr = correlation(&small, &big);
        checked += 1;
        if corr > 0.9 {
            good += 1;
        }
    }
    assert!(checked > 1000, "only {checked} sprite pairs");
    assert!(
        good as f64 / checked as f64 > 0.97,
        "{good}/{checked} sprite pairs match"
    );
}

fn correlation(small: &RgbaImage, big: &RgbaImage) -> f64 {
    let w = small.width.min(big.width / 2);
    let h = small.height.min(big.height / 2);
    let (mut sa, mut sb, mut saa, mut sbb, mut sab, mut n) = (0f64, 0f64, 0f64, 0f64, 0f64, 0f64);
    for y in 0..h {
        for x in 0..w {
            let a = f64::from(small.pixel(x, y)[3]);
            let b = (0..4)
                .map(|k| f64::from(big.pixel(2 * x + k % 2, 2 * y + k / 2)[3]))
                .sum::<f64>()
                / 4.0;
            sa += a;
            sb += b;
            saa += a * a;
            sbb += b * b;
            sab += a * b;
            n += 1.0;
        }
    }
    let cov = sab / n - sa / n * sb / n;
    let va = saa / n - (sa / n).powi(2);
    let vb = sbb / n - (sb / n).powi(2);
    if va < 1e-6 || vb < 1e-6 {
        // Fully opaque or empty: nothing to compare, count as a match.
        return 1.0;
    }
    cov / (va * vb).sqrt()
}

#[test]
fn all_reanims_parse_and_resolve_images() {
    let dir = require_assets!();
    let rsb = load_rsb(&dir);
    let table = ImageIdTable::from_description(&rsb.description().unwrap());

    let mut files: Vec<_> = std::fs::read_dir(dir.join("compiled32/reanim"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    assert_eq!(files.len(), 119);

    let mut used = std::collections::BTreeSet::new();
    for path in &files {
        let r = Reanim::parse_compiled(&std::fs::read(path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(r.fps > 0.0);
        for t in &r.tracks {
            for f in &t.frames {
                if let Some(rpvz_formats::reanim::ImageRef::Id(id)) = f.image {
                    used.insert(id);
                }
            }
        }
    }
    assert_eq!(used.len(), 1180);
    for id in &used {
        let name = table
            .name(*id)
            .unwrap_or_else(|| panic!("image id {id} unresolved"));
        assert!(name.starts_with("IMAGE_"), "{id} -> {name}");
    }

    // Spot checks from track names.
    let pea = Reanim::parse_compiled(
        &std::fs::read(dir.join("compiled32/reanim/peashooter.reanim.compiled")).unwrap(),
    )
    .unwrap();
    let image_of = |track: &str| {
        let t = pea.track(track).unwrap();
        let id = t
            .frames
            .iter()
            .find_map(|f| match f.image {
                Some(rpvz_formats::reanim::ImageRef::Id(i)) => Some(i),
                _ => None,
            })
            .unwrap();
        table.name(id).unwrap().to_owned()
    };
    assert_eq!(image_of("backleaf"), "IMAGE_REANIM_PEASHOOTER_BACKLEAF");
    assert_eq!(image_of("stalk_top"), "IMAGE_REANIM_PEASHOOTER_STALK_TOP");
    assert_eq!(image_of("idle_mouth"), "IMAGE_REANIM_PEASHOOTER_MOUTH");
}

/// PC data (unpacked `main.pak`, lower-case paths) from `RPVZ_TEST_PC_ASSETS`.
#[test]
fn pc_reanims_parse_and_images_exist() {
    let Some(dir) = std::env::var_os("RPVZ_TEST_PC_ASSETS").map(PathBuf::from) else {
        eprintln!("RPVZ_TEST_PC_ASSETS not set; skipping");
        return;
    };
    // The game looks for IMAGE_REANIM_X as reanim/X, then images/X.
    let mut pngs = std::collections::HashSet::new();
    let entries = std::fs::read_dir(dir.join("reanim"))
        .unwrap()
        .chain(std::fs::read_dir(dir.join("images")).unwrap());
    for e in entries {
        let name = e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .to_ascii_uppercase();
        if let Some(stem) = name
            .strip_suffix(".PNG")
            .or_else(|| name.strip_suffix(".JPG"))
        {
            pngs.insert(format!("IMAGE_REANIM_{stem}"));
        }
    }
    let mut count = 0;
    let mut missing = std::collections::BTreeSet::new();
    for e in std::fs::read_dir(dir.join("compiled/reanim")).unwrap() {
        let path = e.unwrap().path();
        let (r, variant) = Reanim::parse_compiled_with_variant(&std::fs::read(&path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(variant, rpvz_formats::Variant::Pc);
        for name in r.image_names() {
            if name.starts_with("IMAGE_REANIM_") && !pngs.contains(name) {
                missing.insert(name.to_owned());
            }
        }
        count += 1;
    }
    assert_eq!(count, 143);
    assert!(
        missing.is_empty(),
        "reanims reference missing images: {missing:?}"
    );
}

/// Every PC font descriptor parses and its glyphs fit on their pages.
#[test]
fn pc_fonts_parse_and_fit_their_images() {
    let Some(dir) = std::env::var_os("RPVZ_TEST_PC_ASSETS").map(PathBuf::from) else {
        eprintln!("RPVZ_TEST_PC_ASSETS not set; skipping");
        return;
    };
    let data = dir.join("data");
    let mut count = 0;
    for e in std::fs::read_dir(&data).unwrap() {
        let path = e.unwrap().path();
        if path.extension().is_none_or(|x| x != "txt") {
            continue;
        }
        let font = rpvz_formats::ImageFont::parse(&std::fs::read(&path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for layer in &font.layers {
            assert!(
                !layer.glyphs.is_empty(),
                "{}: layer {} has no glyphs",
                path.display(),
                layer.name
            );
            let stem = layer.image.to_ascii_lowercase();
            let page = ["png", "gif", "jpg"]
                .iter()
                .flat_map(|ext| [format!("{stem}.{ext}"), format!("_{stem}.{ext}")])
                .map(|f| data.join(f))
                .find(|p| p.is_file())
                .unwrap_or_else(|| panic!("{}: no image for {}", path.display(), layer.image));
            let (w, h) = image::image_dimensions(&page).unwrap();
            for (c, g) in &layer.glyphs {
                let [x, y, gw, gh] = g.rect;
                assert!(
                    x >= 0 && y >= 0 && x + gw <= w as i32 && y + gh <= h as i32,
                    "{}: {c:?} rect {:?} outside {w}x{h}",
                    path.display(),
                    g.rect
                );
            }
        }
        assert!(font.text_width("Hello") > 0, "{}", path.display());
        count += 1;
    }
    assert_eq!(count, 20);
}
