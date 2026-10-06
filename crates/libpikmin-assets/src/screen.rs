//! 2D screens (`.blo`, Yamashita's P2D: `P2DScreen::makeHiearachyPanes`, P2DScreen.cpp) and the
//! game's bitmap font (`Font::setTexture`, graphics.cpp).
//!
//! A screen is a tree of panes, each a big-endian record: a u16 type, then for a pane a
//! visible byte, a spare byte, a four-character tag and its rectangle relative to the parent
//! (`P2DPane::P2DPane`). Type 1 opens the previous pane's children and 2 closes them.
//! Pictures (`P2DPicture`), windows (`P2DWindow`) and text boxes (`P2DTextBox`) add their own
//! fields. English screens live in `screen/eng_blo/`, their textures in `screen/eng_tex/`.
//!
//! Text is ASCII with escape tags: `ESC CC[rrggbbaa]` sets the colour, `ESC GC[...]` the
//! gradient colour, `ESC TM` pauses the typing; `ogMsgCtrlTagMgr::CheckCtrlTag` (ogSub.cpp)
//! lists the rest.

use crate::reader::Reader;
use crate::texture::Image;
use crate::Error;

/// `PANETYPE_*`
mod pane_type {
    pub const END: u16 = 0;
    pub const BEGIN_CHILDREN: u16 = 1;
    pub const END_CHILDREN: u16 = 2;
    pub const PANE: u16 = 0x10;
    pub const WINDOW: u16 = 0x11;
    pub const PICTURE: u16 = 0x12;
    pub const TEXT_BOX: u16 = 0x13;
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaneKind {
    Pane,
    /// `P2DWindow`: a frame drawn from four corner textures; `inner` is the content area.
    Window {
        inner: [i16; 4],
        corners: [Option<String>; 4],
        flag: u8,
        colors: [[u8; 4]; 4],
    },
    /// `P2DPicture`: one texture.
    Picture {
        texture: Option<String>,
        binding: u8,
        /// Bits 0-1 mirror, bit 2 tumble, bits 3-4 wrap.
        flags: u8,
    },
    /// `P2DTextBox`
    TextBox {
        font: Option<String>,
        color: [u8; 4],
        grad_color: [u8; 4],
        /// `P2DTextBoxHBinding` / `VBinding`.
        h_align: u8,
        v_align: u8,
        spacing: i16,
        leading: i16,
        font_width: i16,
        font_height: i16,
        text: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pane {
    pub kind: PaneKind,
    pub visible: bool,
    pub tag: String,
    /// x, y (relative to the parent), width, height.
    pub rect: [i16; 4],
    pub children: Vec<Pane>,
}

impl Pane {
    /// Depth-first search by tag (`P2DPane::search`).
    pub fn find(&self, tag: &str) -> Option<&Pane> {
        if self.tag == tag {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(tag))
    }

    pub fn find_mut(&mut self, tag: &str) -> Option<&mut Pane> {
        if self.tag == tag {
            return Some(self);
        }
        self.children.iter_mut().find_map(|c| c.find_mut(tag))
    }

    /// Every pane depth-first with its absolute position.
    pub fn walk<'a>(&'a self, x: i32, y: i32, out: &mut Vec<(i32, i32, &'a Pane)>) {
        let (px, py) = (x + self.rect[0] as i32, y + self.rect[1] as i32);
        out.push((px, py, self));
        for c in &self.children {
            c.walk(px, py, out);
        }
    }
}

/// `P2DStream::getResource`: a reference type byte, a length byte, then the name.
fn resource(r: &mut Reader) -> Result<Option<String>, Error> {
    let kind = r.u8()?;
    let len = r.u8()? as usize;
    if len == 0 {
        return Ok(None);
    }
    let s = r.take(len)?;
    if kind != 0 && kind != 2 {
        return Err(Error::Bad(format!("screen resource type {kind}")));
    }
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    Ok(Some(String::from_utf8_lossy(&s[..end]).into_owned()))
}

fn colour(r: &mut Reader) -> Result<[u8; 4], Error> {
    Ok([r.u8()?, r.u8()?, r.u8()?, r.u8()?])
}

/// `P2DStream::align`, relative to the start of the file.
fn align4(r: &mut Reader) {
    let p = r.pos();
    r.seek((p + 3) & !3);
}

fn read_pane(r: &mut Reader, ty: u16) -> Result<Pane, Error> {
    let visible = r.u8()? != 0;
    r.u8()?;
    let tag = String::from_utf8_lossy(r.take(4)?).trim_end_matches('\0').to_string();
    let rect = [r.i16()?, r.i16()?, r.i16()?, r.i16()?];
    let kind = match ty {
        pane_type::WINDOW => {
            let inner = [r.i16()?, r.i16()?, r.i16()?, r.i16()?];
            let corners = [resource(r)?, resource(r)?, resource(r)?, resource(r)?];
            resource(r)?; // TLUT
            let flag = r.u8()?;
            let colors = [colour(r)?, colour(r)?, colour(r)?, colour(r)?];
            align4(r);
            PaneKind::Window { inner, corners, flag, colors }
        }
        pane_type::PICTURE => {
            let texture = resource(r)?;
            resource(r)?; // TLUT
            let binding = r.u8()?;
            let flags = r.u8()?;
            align4(r);
            PaneKind::Picture { texture, binding, flags }
        }
        pane_type::TEXT_BOX => {
            let font = resource(r)?;
            let color = colour(r)?;
            let grad_color = colour(r)?;
            let h = r.u8()?;
            let v_align = r.u8()?;
            let (mut spacing, mut leading, mut font_width, mut font_height) = (0, 0, 0, 0);
            if h & 0x80 != 0 {
                spacing = r.i16()?;
                leading = r.i16()?;
                font_width = r.i16()?;
                font_height = r.i16()?;
            }
            let len = r.i16()?.max(0) as usize;
            let text = r.take(len)?.to_vec();
            let text = text[..text.iter().position(|&c| c == 0).unwrap_or(text.len())].to_vec();
            align4(r);
            PaneKind::TextBox {
                font,
                color,
                grad_color,
                h_align: h & 0x7F,
                v_align,
                spacing,
                leading,
                font_width,
                font_height,
                text,
            }
        }
        _ => PaneKind::Pane,
    };
    Ok(Pane { kind, visible, tag, rect, children: Vec::new() })
}

/// Reads panes into `parent` until the matching end (`makeHiearachyPanes`).
fn read_children(r: &mut Reader, parent: &mut Vec<Pane>) -> Result<bool, Error> {
    loop {
        let ty = r.u16()?;
        match ty {
            pane_type::END => return Ok(true),
            pane_type::BEGIN_CHILDREN => {
                r.u16()?;
                let last = parent
                    .last_mut()
                    .ok_or_else(|| Error::Bad("children before any pane".into()))?;
                if read_children(r, &mut last.children)? {
                    return Ok(true);
                }
            }
            pane_type::END_CHILDREN => {
                r.u16()?;
                return Ok(false);
            }
            pane_type::PANE | pane_type::WINDOW | pane_type::PICTURE | pane_type::TEXT_BOX => {
                parent.push(read_pane(r, ty)?);
            }
            other => return Err(Error::Bad(format!("pane type {other:#x} at {:#x}", r.pos() - 2))),
        }
    }
}

/// A parsed `.blo`. The root is the first pane (tag `ROOT`, the whole 640×480 screen).
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    pub root: Pane,
}

impl Screen {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        let mut top = Vec::new();
        read_children(&mut r, &mut top)?;
        let root = top.into_iter().next().ok_or_else(|| Error::Bad("empty screen".into()))?;
        Ok(Screen { root })
    }

    /// The screen's text boxes' strings with tags stripped, in order.
    pub fn plain_text(&self) -> Vec<String> {
        let mut all = Vec::new();
        self.root.walk(0, 0, &mut all);
        all.iter()
            .filter_map(|(_, _, p)| match &p.kind {
                PaneKind::TextBox { text, .. } => Some(strip_tags(text)),
                _ => None,
            })
            .collect()
    }
}

/// A run of text in one colour, from parsing the escape tags.
#[derive(Debug, Clone, PartialEq)]
pub enum TextPiece {
    Char(u8),
    Color([u8; 4]),
    GradColor([u8; 4]),
    /// `ESC TM`: a typing pause.
    Pause,
}

fn hex_colour(s: &[u8]) -> Option<[u8; 4]> {
    let s = std::str::from_utf8(s).ok()?;
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(if s.len() <= 6 { (v << 8 | 0xFF).to_be_bytes() } else { v.to_be_bytes() })
}

/// Splits text into characters and tags (`P2DPrint::doCtrlCode` / `CheckCtrlTag`). Unknown or
/// layout tags are dropped.
pub fn parse_text(text: &[u8]) -> Vec<TextPiece> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c != 0x1B {
            out.push(TextPiece::Char(c));
            i += 1;
            continue;
        }
        let tag = &text[i + 1..];
        if tag.starts_with(b"Z") {
            i += 2;
            continue;
        }
        if tag.starts_with(b"TM") {
            out.push(TextPiece::Pause);
            i += 3;
            continue;
        }
        let end = tag.iter().position(|&c| c == b']');
        let open = tag.iter().position(|&c| c == b'[');
        match (open, end) {
            (Some(o), Some(e)) if o < e => {
                let arg = &tag[o + 1..e];
                if tag.starts_with(b"CC") {
                    if let Some(col) = hex_colour(arg) {
                        out.push(TextPiece::Color(col));
                    }
                } else if tag.starts_with(b"GC")
                    && let Some(col) = hex_colour(arg)
                {
                    out.push(TextPiece::GradColor(col));
                }
                i += 1 + e + 1;
            }
            _ => i += 3,
        }
    }
    out
}

/// Text with the escape tags removed.
pub fn strip_tags(text: &[u8]) -> String {
    parse_text(text)
        .into_iter()
        .filter_map(|p| match p {
            TextPiece::Char(c) => Some(c as char),
            _ => None,
        })
        .collect()
}

/// One glyph cell (`FontChar`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Glyph {
    /// Advance (`mCharSpacing`).
    pub advance: i32,
    /// From the pen to the glyph's left edge (`mLeftOffset`).
    pub left: i32,
    /// Source rectangle in the font texture.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The game's grid font (`bigFont.bti`, 21 × 42 cells, `P2DFont::loadFont`), with each cell's
/// extent and advance measured from its alpha the way `Font::setTexture` does: blank columns
/// trim the sides, and the bottom row marks the advance (a run of transparent pixels between
/// opaque ones).
#[derive(Debug, Clone)]
pub struct Font {
    pub image: Image,
    pub cell_width: i32,
    pub cell_height: i32,
    pub glyphs: Vec<Glyph>,
}

impl Font {
    pub const PATH: &'static str = "bigFont.bti";

    pub fn new(image: Image, columns: i32, rows: i32) -> Font {
        let cw = image.width as i32 / columns;
        let ch = image.height as i32 / rows;
        let alpha = |x: i32, y: i32| image.rgba[((y as usize) * image.width as usize + x as usize) * 4 + 3];
        let mut glyphs = Vec::with_capacity((columns * rows) as usize);
        for row in 0..rows {
            for col in 0..columns {
                let (ox, oy) = (col * cw, row * ch);
                let blank = |x: i32| (0..ch - 1).all(|y| alpha(ox + x, oy + y) == 0);
                let mut left = 0;
                for x in 0..cw {
                    if !blank(x) {
                        break;
                    }
                    left = x;
                }
                let mut right = 0;
                for x in (0..cw).rev() {
                    if !blank(x) {
                        break;
                    }
                    right = cw - x;
                }
                let (mut base, mut base_end) = (-1, cw);
                for x in 0..cw {
                    let a = alpha(ox + x, oy + ch - 1);
                    if base < 0 {
                        if a == 0 {
                            base = x;
                        }
                    } else if a != 0 {
                        base_end = x;
                        break;
                    }
                }
                glyphs.push(Glyph {
                    advance: base_end - base,
                    left: base - left,
                    x: ox + left,
                    y: oy,
                    width: cw - left - right,
                    height: ch - 1,
                });
            }
        }
        Font { image, cell_width: cw, cell_height: ch, glyphs }
    }

    /// `Font::charToIndex` for ASCII.
    pub fn glyph(&self, c: u8) -> Option<&Glyph> {
        self.glyphs.get((c as usize).checked_sub(0x20)?)
    }
}
