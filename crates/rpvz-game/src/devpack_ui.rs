//! UI parts of the placeholder pack: dialog pieces, button caps, an edit box,
//! a bitmap font and the `resources.xml` that names them. All plain shapes of
//! our own; the font is rasterised from Bevy's bundled FiraMono (SIL OFL).

use std::path::Path;

use crate::devpack::Canvas;

const PIECES: [[&str; 3]; 3] = [
    ["topleft", "topmiddle", "topright"],
    ["centerleft", "centermiddle", "centerright"],
    ["bottomleft", "bottommiddle", "bottomright"],
];

/// Font ids the engine asks for, all served by the one placeholder font.
const FONT_IDS: &[&str] = &[
    "DWARVENTODCRAFT18",
    "DWARVENTODCRAFT24",
    "BRIANNETOD16",
    "HOUSEOFTERROR28",
];

pub fn write(dir: &Path) -> Result<(), String> {
    write_pieces(dir)?;
    write_font(dir)?;
    write_resources(dir)
}

fn copy_region(src: &Canvas, x0: u32, y0: u32, w: u32, h: u32) -> Canvas {
    let mut out = Canvas::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.set(x, y, src.get(x0 + x, y0 + y));
        }
    }
    out
}

fn write_pieces(dir: &Path) -> Result<(), String> {
    let images = dir.join("images");
    let fill = [52, 64, 92, 255];
    let border = [190, 200, 230, 255];

    // A 96×96 rounded panel cut into nine 32×32 pieces.
    let whole = Canvas::new(96, 96).panel(fill, border, 16.0);
    for (row, names) in PIECES.iter().enumerate() {
        for (col, name) in names.iter().enumerate() {
            copy_region(&whole, col as u32 * 32, row as u32 * 32, 32, 32)
                .save(&images.join(format!("dialog_{name}.png")))?;
        }
    }
    Canvas::new(180, 44)
        .panel([90, 120, 170, 255], border, 20.0)
        .save(&images.join("dialog_header.png"))?;

    for (prefix, f) in [("", [70, 90, 120, 255]), ("down_", [45, 60, 85, 255])] {
        let cap = Canvas::new(32, 40).panel(f, border, 14.0);
        copy_region(&cap, 0, 0, 16, 40).save(&images.join(format!("button_{prefix}left.png")))?;
        copy_region(&cap, 16, 0, 16, 40).save(&images.join(format!("button_{prefix}right.png")))?;
        let mut middle = Canvas::new(16, 40);
        for y in 0..40 {
            for x in 0..16 {
                middle.set(x, y, if !(4..36).contains(&y) { border } else { f });
            }
        }
        middle.save(&images.join(format!("button_{prefix}middle.png")))?;
    }
    Canvas::new(260, 34)
        .panel([20, 24, 34, 255], [120, 130, 160, 255], 6.0)
        .save(&images.join("editbox.png"))
}

fn write_resources(dir: &Path) -> Result<(), String> {
    let mut xml = String::from(
        "<ResourceManifest>\n<Resources id=\"Devpack\">\n<SetDefaults path=\"images\" idprefix=\"IMAGE_\" />\n",
    );
    for name in PIECES.iter().flatten() {
        xml += &format!(
            "<Image id=\"DIALOG_{}\" path=\"dialog_{name}\" />\n",
            name.to_ascii_uppercase()
        );
    }
    xml += "<Image id=\"DIALOG_HEADER\" path=\"dialog_header\" />\n";
    for side in ["LEFT", "MIDDLE", "RIGHT"] {
        let lower = side.to_ascii_lowercase();
        xml += &format!("<Image id=\"BUTTON_{side}\" path=\"button_{lower}\" />\n");
        xml += &format!("<Image id=\"BUTTON_DOWN_{side}\" path=\"button_down_{lower}\" />\n");
    }
    xml += "<Image id=\"EDITBOX\" path=\"editbox\" />\n";
    xml += "<SetDefaults path=\"data\" idprefix=\"FONT_\" />\n";
    for id in FONT_IDS {
        xml += &format!("<Font id=\"{id}\" path=\"devfont.txt\" />\n");
    }
    xml += "</Resources>\n</ResourceManifest>\n";
    let path = dir.join("properties/resources.xml");
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| e.to_string())?;
    std::fs::write(&path, xml).map_err(|e| format!("{}: {e}", path.display()))
}

/// A one-layer font in the PopCap descriptor format.
fn write_font(dir: &Path) -> Result<(), String> {
    use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
    let font = FontRef::try_from_slice(bevy::text::DEFAULT_FONT_DATA).map_err(|e| e.to_string())?;
    let scaled = font.as_scaled(PxScale::from(20.0));
    let ascent = scaled.ascent().ceil() as i32;
    let (page_w, page_h) = (512u32, 256u32);
    let mut page = Canvas::new(page_w, page_h);
    let (mut cx, mut cy, mut row_h) = (1u32, 1u32, 0u32);
    let (mut chars, mut widths, mut rects, mut offsets) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for c in (32u8..=126).map(char::from) {
        let mut glyph = scaled.scaled_glyph(c);
        glyph.position = point(0.0, ascent as f32);
        let advance = scaled.h_advance(scaled.glyph_id(c)).round() as i32;
        // The descriptor quotes characters with '; a quote itself is written '''.
        chars.push(if c == '\'' {
            "'''".to_owned()
        } else {
            format!("'{c}'")
        });
        widths.push(advance.to_string());
        match font.outline_glyph(glyph) {
            Some(outline) => {
                let b = outline.px_bounds();
                let (w, h) = (b.width().ceil() as u32, b.height().ceil() as u32);
                if cx + w + 1 > page_w {
                    cx = 1;
                    cy += row_h + 1;
                    row_h = 0;
                }
                outline.draw(|x, y, coverage| {
                    let a = (coverage.clamp(0.0, 1.0) * 255.0) as u8;
                    page.set(cx + x, cy + y, [255, 255, 255, a]);
                });
                rects.push(format!("({cx}, {cy}, {w}, {h})"));
                offsets.push(format!(
                    "({}, {})",
                    b.min.x.floor() as i32,
                    b.min.y.floor() as i32
                ));
                cx += w + 1;
                row_h = row_h.max(h);
            }
            None => {
                rects.push("(0, 0, 0, 0)".into());
                offsets.push("(0, 0)".into());
            }
        }
    }
    let descriptor = format!(
        "Define CharList ({});\nDefine WidthList ({});\nDefine RectList ({});\nDefine OffsetList ({});\n\
         CreateLayer Main;\nLayerSetImage Main 'devfont';\nLayerSetAscent Main {ascent};\n\
         LayerSetCharWidths Main CharList WidthList;\nLayerSetImageMap Main CharList RectList;\n\
         LayerSetCharOffsets Main CharList OffsetList;\nLayerSetPointSize Main 20;\nSetDefaultPointSize 20;\n",
        chars.join(", "),
        widths.join(", "),
        rects.join(", "),
        offsets.join(", ")
    );
    let data = dir.join("data");
    page.save(&data.join("devfont.png"))?;
    std::fs::write(data.join("devfont.txt"), descriptor).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_font_parses_and_measures() {
        let dir = std::env::temp_dir().join(format!("rpvz-devfont-{}", std::process::id()));
        write_font(&dir).unwrap();
        let font =
            rpvz_formats::ImageFont::parse(&std::fs::read(dir.join("data/devfont.txt")).unwrap())
                .unwrap();
        let main = font.main().unwrap();
        assert_eq!(main.glyphs.len(), 95, "printable ASCII");
        assert!(font.text_width("Hello") > font.text_width("Hi"));
        assert!(main.glyphs[&'\''].width > 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
