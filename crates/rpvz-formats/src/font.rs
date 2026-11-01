//! PopCap image fonts (`data/*.txt` descriptors with glyph pages).
//!
//! A descriptor is a small statement language:
//!
//! ```text
//! Define CharList ('A', 'B', ...);
//! Define RectList ((0, 0, 12, 18), ...);
//! CreateLayer Main;
//! LayerSetImage Main 'BrianneTod16';
//! LayerSetCharWidths Main CharList WidthList;
//! LayerSetImageMap Main CharList RectList;
//! LayerSetCharOffsets Main CharList OffsetList;
//! LayerSetKerningPairs Main KerningPairs KerningValues;
//! LayerSetAscent Main 19;
//! ```
//!
//! Arguments are either names of `Define`d values or inline values. Text is
//! Latin-1. Layers are drawn in order (e.g. an outline under the main layer).

use std::collections::HashMap;

use crate::error::{FormatError, Result};

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Int(i64),
    Str(String),
    Ident(String),
    List(Vec<Value>),
}

impl Value {
    fn int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }

    fn list(&self) -> Option<&[Value]> {
        match self {
            Value::List(v) => Some(v),
            _ => None,
        }
    }

    /// A one-character string as a char.
    fn ch(&self) -> Option<char> {
        match self {
            Value::Str(s) if s.chars().count() == 1 => s.chars().next(),
            _ => None,
        }
    }

    fn ints<const N: usize>(&self) -> Option<[i32; N]> {
        let l = self.list()?;
        if l.len() != N {
            return None;
        }
        let mut out = [0; N];
        for (o, v) in out.iter_mut().zip(l) {
            *o = i32::try_from(v.int()?).ok()?;
        }
        Some(out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Glyph {
    /// Source rectangle on the layer image: x, y, w, h.
    pub rect: [i32; 4],
    /// Where the rectangle is drawn relative to the pen (y relative to the
    /// top of the line).
    pub offset: [i32; 2],
    /// Pen advance.
    pub width: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FontLayer {
    pub name: String,
    /// Image file stem (looked up next to the descriptor).
    pub image: String,
    pub ascent: i32,
    pub ascent_padding: i32,
    pub line_spacing_offset: i32,
    pub point_size: i32,
    /// RGBA multiplier, 0..=255 each.
    pub color_mult: [i32; 4],
    pub glyphs: HashMap<char, Glyph>,
    pub kerning: HashMap<(char, char), i32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImageFont {
    pub layers: Vec<FontLayer>,
    pub default_point_size: i32,
}

impl ImageFont {
    pub fn layer(&self, name: &str) -> Option<&FontLayer> {
        self.layers
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }

    /// Main layer: the one named `Main`, else the first.
    pub fn main(&self) -> Option<&FontLayer> {
        self.layer("Main").or_else(|| self.layers.first())
    }

    pub fn ascent(&self) -> i32 {
        self.main().map_or(0, |l| l.ascent)
    }

    pub fn line_height(&self) -> i32 {
        self.main()
            .map_or(0, |l| l.ascent + l.ascent_padding + l.line_spacing_offset)
            + self.main().map_or(0, |l| l.point_size / 4)
    }

    /// Width of a single line of text in pixels, using the main layer.
    pub fn text_width(&self, text: &str) -> i32 {
        let Some(main) = self.main() else { return 0 };
        let mut width = 0;
        let mut prev = None;
        for c in text.chars() {
            if let Some(g) = main.glyphs.get(&c) {
                if let Some(p) = prev {
                    width += main.kerning.get(&(p, c)).copied().unwrap_or(0);
                }
                width += g.width;
            }
            prev = Some(c);
        }
        width
    }

    /// Pen position of each character of one line, using the main layer's
    /// advances and kerning (all layers share the pen).
    pub fn layout(&self, text: &str) -> Vec<(char, i32)> {
        let Some(main) = self.main() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(text.len());
        let mut x = 0;
        let mut prev = None;
        for c in text.chars() {
            if let Some(p) = prev {
                x += main.kerning.get(&(p, c)).copied().unwrap_or(0);
            }
            out.push((c, x));
            x += main.glyphs.get(&c).map_or(0, |g| g.width);
            prev = Some(c);
        }
        out
    }

    pub fn parse(text: &[u8]) -> Result<Self> {
        let text: String = text.iter().map(|&b| b as char).collect();
        let statements = parse_statements(&tokenize(&text)?)?;
        let mut defines: HashMap<String, Value> = HashMap::new();
        let mut font = ImageFont::default();

        // Resolves an argument: a defined name, or an inline value.
        let resolve = |defines: &HashMap<String, Value>, v: &Value| -> Value {
            match v {
                Value::Ident(name) => defines.get(name).cloned().unwrap_or_else(|| v.clone()),
                other => other.clone(),
            }
        };

        for stmt in statements {
            let Some(Value::Ident(cmd)) = stmt.first() else {
                return Err(bad("statement without a command"));
            };
            let args = &stmt[1..];
            let arg = |i: usize| {
                args.get(i)
                    .ok_or_else(|| bad(&format!("{cmd}: missing argument {i}")))
            };
            if cmd == "Define" {
                let Value::Ident(name) = arg(0)? else {
                    return Err(bad("Define needs a name"));
                };
                let value = resolve(&defines, arg(1)?);
                defines.insert(name.clone(), value);
                continue;
            }
            if cmd == "SetDefaultPointSize" {
                font.default_point_size = arg(0)?.int().unwrap_or(0) as i32;
                continue;
            }
            if cmd == "CreateLayer" {
                let Value::Ident(name) = arg(0)? else {
                    return Err(bad("CreateLayer needs a name"));
                };
                font.layers.push(FontLayer {
                    name: name.clone(),
                    color_mult: [255; 4],
                    ..FontLayer::default()
                });
                continue;
            }
            let Some(rest) = cmd.strip_prefix("LayerSet") else {
                // Unknown top-level commands are ignored, as the game does.
                continue;
            };
            let Value::Ident(layer_name) = arg(0)? else {
                return Err(bad(&format!("{cmd} needs a layer")));
            };
            let layer = font
                .layers
                .iter_mut()
                .find(|l| &l.name == layer_name)
                .ok_or_else(|| bad(&format!("{cmd}: no layer {layer_name}")))?;
            let int_arg = |i: usize| -> Result<i32> {
                resolve(&defines, arg(i)?)
                    .int()
                    .map(|v| v as i32)
                    .ok_or_else(|| bad(&format!("{cmd}: argument {i} is not a number")))
            };
            match rest {
                "Image" => {
                    layer.image = match resolve(&defines, arg(1)?) {
                        Value::Str(s) => s,
                        _ => return Err(bad("LayerSetImage needs a file name")),
                    }
                }
                "Ascent" => layer.ascent = int_arg(1)?,
                "AscentPadding" => layer.ascent_padding = int_arg(1)?,
                "LineSpacingOffset" => layer.line_spacing_offset = int_arg(1)?,
                "PointSize" => layer.point_size = int_arg(1)?,
                "ColorMult" => {
                    let v = resolve(&defines, arg(1)?);
                    layer.color_mult = match v.int() {
                        // A packed 0xAARRGGBB; alpha 0 means opaque, as in the
                        // framework's Color(int).
                        Some(packed) => {
                            let p = packed as u32;
                            let a = (p >> 24) & 0xFF;
                            [
                                ((p >> 16) & 0xFF) as i32,
                                ((p >> 8) & 0xFF) as i32,
                                (p & 0xFF) as i32,
                                if a == 0 { 255 } else { a as i32 },
                            ]
                        }
                        None => v.ints::<4>().ok_or_else(|| {
                            bad("LayerSetColorMult needs (r, g, b, a) or a packed colour")
                        })?,
                    };
                }
                "CharWidths" | "ImageMap" | "CharOffsets" => {
                    let chars = char_list(&resolve(&defines, arg(1)?))?;
                    let values = resolve(&defines, arg(2)?);
                    let values = values
                        .list()
                        .ok_or_else(|| bad(&format!("{cmd}: values are not a list")))?;
                    if values.len() != chars.len() {
                        return Err(bad(&format!(
                            "{cmd}: {} chars but {} values",
                            chars.len(),
                            values.len()
                        )));
                    }
                    for (c, v) in chars.into_iter().zip(values) {
                        let g = layer.glyphs.entry(c).or_default();
                        match rest {
                            "CharWidths" => {
                                g.width =
                                    v.int().ok_or_else(|| bad("width is not a number"))? as i32
                            }
                            "ImageMap" => {
                                g.rect = v
                                    .ints::<4>()
                                    .ok_or_else(|| bad("rect is not (x, y, w, h)"))?
                            }
                            _ => {
                                g.offset =
                                    v.ints::<2>().ok_or_else(|| bad("offset is not (x, y)"))?
                            }
                        }
                    }
                }
                "KerningPairs" => {
                    let pairs = resolve(&defines, arg(1)?);
                    let values = resolve(&defines, arg(2)?);
                    let (Some(pairs), Some(values)) = (pairs.list(), values.list()) else {
                        return Err(bad("kerning needs two lists"));
                    };
                    for (p, v) in pairs.iter().zip(values) {
                        let (Value::Str(s), Some(v)) = (p, v.int()) else {
                            continue;
                        };
                        let mut cs = s.chars();
                        if let (Some(a), Some(b)) = (cs.next(), cs.next()) {
                            layer.kerning.insert((a, b), v as i32);
                        }
                    }
                }
                // Draw modes, offsets and the like don't affect layout yet.
                _ => {}
            }
        }
        if font.layers.is_empty() {
            return Err(bad("font has no layers"));
        }
        Ok(font)
    }
}

fn char_list(v: &Value) -> Result<Vec<char>> {
    v.list()
        .ok_or_else(|| bad("char list is not a list"))?
        .iter()
        .map(|c| {
            c.ch()
                .ok_or_else(|| bad("char list entry is not one character"))
        })
        .collect()
}

fn bad(msg: &str) -> FormatError {
    FormatError::Invalid(format!("font: {msg}"))
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Int(i64),
    Str(String),
    Ident(String),
    Open,
    Close,
    Comma,
    Semi,
}

fn tokenize(text: &str) -> Result<Vec<Token>> {
    let mut out = Vec::new();
    let mut it = text.chars().peekable();
    while let Some(&c) = it.peek() {
        match c {
            c if c.is_whitespace() => {
                it.next();
            }
            '/' => {
                it.next();
                if it.peek() == Some(&'/') {
                    for c in it.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                } else {
                    return Err(bad("stray '/'"));
                }
            }
            '(' => {
                it.next();
                out.push(Token::Open);
            }
            ')' => {
                it.next();
                out.push(Token::Close);
            }
            ',' => {
                it.next();
                out.push(Token::Comma);
            }
            ';' => {
                it.next();
                out.push(Token::Semi);
            }
            '\'' | '"' => {
                let quote = c;
                it.next();
                let mut s = String::new();
                loop {
                    match it.next() {
                        None => return Err(bad("unterminated string")),
                        // A quote directly after the opening one is the
                        // character itself, as in '''.
                        Some(q) if q == quote && !s.is_empty() => break,
                        Some(q) if q == quote && it.peek() != Some(&quote) => break,
                        Some(ch) => s.push(ch),
                    }
                }
                out.push(Token::Str(s));
            }
            c if c == '-' || c.is_ascii_digit() => {
                let mut s = String::new();
                s.push(c);
                it.next();
                while let Some(&d) = it.peek() {
                    if d.is_ascii_digit() {
                        s.push(d);
                        it.next();
                    } else {
                        break;
                    }
                }
                out.push(Token::Int(
                    s.parse().map_err(|_| bad(&format!("bad number {s}")))?,
                ));
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut s = String::new();
                while let Some(&d) = it.peek() {
                    if d.is_alphanumeric() || d == '_' {
                        s.push(d);
                        it.next();
                    } else {
                        break;
                    }
                }
                out.push(Token::Ident(s));
            }
            other => return Err(bad(&format!("unexpected character {other:?}"))),
        }
    }
    Ok(out)
}

fn parse_statements(tokens: &[Token]) -> Result<Vec<Vec<Value>>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < tokens.len() {
        let mut stmt = Vec::new();
        while pos < tokens.len() && tokens[pos] != Token::Semi {
            stmt.push(parse_value(tokens, &mut pos)?);
        }
        pos += 1; // ';'
        if !stmt.is_empty() {
            out.push(stmt);
        }
    }
    Ok(out)
}

fn parse_value(tokens: &[Token], pos: &mut usize) -> Result<Value> {
    let t = tokens.get(*pos).ok_or_else(|| bad("unexpected end"))?;
    *pos += 1;
    Ok(match t {
        Token::Int(v) => Value::Int(*v),
        Token::Str(s) => Value::Str(s.clone()),
        Token::Ident(s) => Value::Ident(s.clone()),
        Token::Open => {
            let mut items = Vec::new();
            loop {
                match tokens.get(*pos) {
                    Some(Token::Close) => {
                        *pos += 1;
                        break;
                    }
                    Some(Token::Comma) => *pos += 1,
                    Some(_) => items.push(parse_value(tokens, pos)?),
                    None => return Err(bad("missing ')'")),
                }
            }
            Value::List(items)
        }
        other => return Err(bad(&format!("unexpected {other:?}"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
Define CharList ('A', 'V', ' ', ''', '"');
Define WidthList (10, 9, 4, 3, 5);
Define RectList ((0, 0, 11, 14), (11, 0, 10, 14), (0, 0, 0, 0), (21, 0, 3, 5), (24, 0, 5, 5));
Define OffsetList ((0, 2), (-1, 2), (0, 0), (0, 1), (0, 1));
Define KerningPairs ("AV", "VA");
Define KerningValues (-2, -1);
CreateLayer Main;
LayerSetImage Main 'TestFont';
LayerSetAscent Main 12;
LayerSetCharWidths Main CharList WidthList;
LayerSetCharWidths Main (' ') (6);   // inline lists override
LayerSetImageMap Main CharList RectList;
LayerSetCharOffsets Main CharList OffsetList;
LayerSetKerningPairs Main KerningPairs KerningValues;
LayerSetColorMult Main (255, 128, 0, 255);
SetDefaultPointSize 14;
"#;

    #[test]
    fn parses_layers_glyphs_and_kerning() {
        let f = ImageFont::parse(SAMPLE.as_bytes()).unwrap();
        let main = f.main().unwrap();
        assert_eq!(main.image, "TestFont");
        assert_eq!(main.ascent, 12);
        assert_eq!(main.color_mult, [255, 128, 0, 255]);
        assert_eq!(main.glyphs[&'V'].offset, [-1, 2]);
        assert_eq!(main.glyphs[&'A'].rect, [0, 0, 11, 14]);
        assert_eq!(main.glyphs[&' '].width, 6, "later statement wins");
        assert_eq!(main.glyphs[&'\''].width, 3);
        assert_eq!(main.glyphs[&'"'].width, 5);
        assert_eq!(main.kerning[&('A', 'V')], -2);
        assert_eq!(f.default_point_size, 14);
    }

    #[test]
    fn width_and_layout_use_kerning() {
        let f = ImageFont::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(f.text_width("AV"), 10 - 2 + 9);
        assert_eq!(f.layout("AVA"), vec![('A', 0), ('V', 8), ('A', 16)]);
        assert_eq!(f.text_width("A?"), 10, "unknown chars take no space");
    }

    #[test]
    fn packed_colour_mult() {
        let src = "Define C ('a'); Define W (1); CreateLayer Outline; LayerSetCharWidths Outline C W; \
                   LayerSetColorMult Outline 0;";
        let f = ImageFont::parse(src.as_bytes()).unwrap();
        assert_eq!(f.layers[0].color_mult, [0, 0, 0, 255], "0 is opaque black");
        let src = src.replace("Outline 0;", "Outline 2164195328;"); // 0x80FF0000
        let f = ImageFont::parse(src.as_bytes()).unwrap();
        assert_eq!(f.layers[0].color_mult, [255, 0, 0, 128]);
    }

    #[test]
    fn rejects_mismatched_lists() {
        let bad =
            "Define C ('a', 'b'); Define W (1); CreateLayer Main; LayerSetCharWidths Main C W;";
        assert!(ImageFont::parse(bad.as_bytes()).is_err());
        assert!(ImageFont::parse(b"Define C ('a');").is_err(), "no layers");
    }
}
