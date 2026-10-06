//! The game's message window (`zen::ogScrMessageMgr`, ogMessage.cpp) showing its own screens
//! from the disc: the frame (`tu_base.blo`) and pages such as a ship part's description.
//!
//! Screens are drawn on the game's 640×480 canvas, scaled to fit the window. Pictures follow
//! `P2DPicture::drawFullSet`'s binding rules, text `P2DPrint::parse` with the grid font
//! (`P2DFont`: always `bigFont.bti`), escape tags for colour, size and position.
//!
//! Typing (`update`, `ogMsgCtrlTagMgr::CheckCtrlTag` in ogSub.cpp): one character every
//! 0.02964 s (5× with A held), `SYSSE_TYPEWRITER` per character, 0.25 s after the wait
//! characters (`wait_char.blo`'s `han` box) and `ESC TM`. B shows the whole page. Once a page
//! is out, A or B turns it; after the last one the window fades out with
//! `SYSSE_MESSAGE_CLOSE`. Fades take 0.25 s.

use std::collections::HashMap;

use libpikmin_assets::Assets;
use libpikmin_assets::screen::{Font, Pane, PaneKind, Screen};
use macroquad::miniquad::TextureWrap;
use macroquad::prelude::*;

const CHAR_TIME: f32 = 0.029639998;
const FADE: f32 = 0.25;

/// `cnvButtonIcon`: button letters and the colour each icon is drawn in (`a b c x y z l r`).
const BUTTONS: &[u8] = b"abcxyzlr";
const BUTTON_COLORS: [&str; 8] = ["00ff00", "ff0000", "ffff00", "808080", "808080", "4040ff", "808080", "808080"];
/// Glyph of the first button icon: kanji 0 of `kanji_convert_table` (`Font::charToIndex`).
const KANJI_BASE: usize = 0x126;
/// `ESC m`: 地 (kanji 8), the control stick.
const STICK_KANJI: usize = 8;

/// Loaded textures and the font.
pub struct Ui {
    assets_root: std::path::PathBuf,
    textures: HashMap<String, Option<Texture2D>>,
    font: Font,
    font_tex: Texture2D,
    /// `wait_char.blo`'s `han` box: characters followed by a 0.25 s pause.
    wait_chars: Vec<u8>,
    base: Screen,
}

fn load_bti(bytes: &[u8]) -> Option<(Texture2D, libpikmin_assets::Image, bool, bool)> {
    let (img, s, t) = libpikmin_assets::texture::decode_bti(bytes).ok()?;
    let tex = Texture2D::from_rgba8(img.width as u16, img.height as u16, &img.rgba);
    tex.set_filter(FilterMode::Linear);
    let wrap = |w: bool| if w { TextureWrap::Repeat } else { TextureWrap::Clamp };
    // SAFETY: main thread.
    unsafe { macroquad::window::get_internal_gl() }
        .quad_context
        .texture_set_wrap(tex.raw_miniquad_id(), wrap(s), wrap(t));
    Some((tex, img, s, t))
}

impl Ui {
    pub fn load(assets: &Assets) -> Option<Ui> {
        let (font_tex, img, ..) = load_bti(&assets.read(Font::PATH).ok()?)?;
        let font = Font::new(img, 21, 42);
        let base = Screen::parse(&assets.read(libpikmin_assets::ship::MESSAGE_BASE_SCREEN).ok()?).ok()?;
        let wait_chars = assets
            .read("screen/eng_blo/wait_char.blo")
            .ok()
            .and_then(|d| Screen::parse(&d).ok())
            .and_then(|s| match &s.root.find("han")?.kind {
                PaneKind::TextBox { text, .. } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| b".!?".to_vec());
        Some(Ui {
            assets_root: assets.root.clone(),
            textures: HashMap::new(),
            font,
            font_tex,
            wait_chars,
            base,
        })
    }

    pub fn screen(&self, path: &str) -> Option<Screen> {
        Screen::parse(&std::fs::read(self.assets_root.join(path)).ok()?).ok()
    }

    fn texture(&mut self, name: &str) -> Option<Texture2D> {
        let root = self.assets_root.clone();
        self.textures
            .entry(name.to_string())
            .or_insert_with(|| {
                let bytes = std::fs::read(root.join(libpikmin_assets::ship::SCREEN_TEXTURES).join(name)).ok()?;
                load_bti(&bytes).map(|t| t.0)
            })
            .clone()
    }
}

/// `cnvButtonIcon`: `ESC <button>` becomes a large coloured icon glyph; `ESC m` the stick.
/// Icons are two-byte codes here: 0xFF then the glyph index - `KANJI_BASE`.
fn expand_buttons(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == 0x1B
            && let Some(&c) = text.get(i + 1)
        {
            if let Some(k) = BUTTONS.iter().position(|&b| b == c) {
                out.extend_from_slice(format!("\x1bFX[32]\x1bFY[28]\x1bCC[{}]", BUTTON_COLORS[k]).as_bytes());
                out.extend_from_slice(&[0xFF, k as u8]);
                out.extend_from_slice(b"\x1bCC[b4ffff]\x1bFX[24]\x1bFY[24]");
                i += 2;
                continue;
            }
            if c == b'm' {
                out.extend_from_slice(&[0xFF, STICK_KANJI as u8]);
                i += 2;
                continue;
            }
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

/// `cnvSpecialNumber`: `ESC d<n>` becomes `numbers[n]` (`ESC Z<n>` zero-pads).
fn fill_numbers(text: &[u8], numbers: &[i32; 10]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut pad = 0usize;
    let mut i = 0;
    while i < text.len() {
        if text[i] == 0x1B {
            match text.get(i + 1) {
                Some(b'd') => {
                    if let Some(d) = text.get(i + 2).filter(|c| c.is_ascii_digit()) {
                        let n = numbers[(d - b'0') as usize];
                        out.extend_from_slice(format!("{n:0pad$}").as_bytes());
                        i += 3;
                    } else {
                        i += 2;
                    }
                    continue;
                }
                Some(b'Z') => {
                    if let Some(d) = text.get(i + 2).filter(|c| c.is_ascii_digit()) {
                        let w = (d - b'0') as usize;
                        pad = if w > 1 { w } else { 0 };
                        i += 3;
                    } else {
                        i += 2;
                    }
                    continue;
                }
                _ => {}
            }
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

/// One step of typing from byte `i`: (next index, wait seconds, play the type sound)
/// (`CheckCtrlTag`).
fn type_step(text: &[u8], i: usize, wait_chars: &[u8]) -> (usize, f32, bool) {
    let c = text[i];
    if c == 0x1B {
        let tag = &text[i + 1..];
        if tag.starts_with(b"Z") || tag.starts_with(b"d") {
            return (i + 3.min(text.len() - i), 0.0, false);
        }
        if tag.starts_with(b"TM") {
            return (i + 3, 0.25, false);
        }
        let n = match tag.iter().position(|&b| b == b']') {
            Some(e) if tag.len() >= 2 => e + 2,
            _ => 3,
        };
        return ((i + n).min(text.len()), 0.0, false);
    }
    if c >= 0x80 {
        return ((i + 2).min(text.len()), 0.0, true);
    }
    let wait = if wait_chars.contains(&c) { 0.25 } else { 0.0 };
    (i + 1, wait, true)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    FadingIn(f32),
    Typing,
    Shown,
    FadingOut(f32),
}

/// What [`Message::update`] wants played.
pub enum Sound {
    Typewriter,
    Close,
}

/// An open message window going through its pages.
pub struct Message {
    pages: Vec<Screen>,
    page: usize,
    state: State,
    /// Typing: (text box index, byte offset) and the time until the next character.
    pane: usize,
    offset: usize,
    timer: f32,
    numbers: [i32; 10],
    time: f32,
}

/// Text boxes of a page that are typed out, in pane order (`pk*` panes are Pikmin
/// animation scripts, not text).
fn page_texts(screen: &Screen) -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    screen.root.walk(0, 0, &mut all);
    all.iter()
        .filter(|(_, _, p)| !p.tag.starts_with("pk"))
        .filter_map(|(_, _, p)| match &p.kind {
            PaneKind::TextBox { text, .. } => Some(expand_buttons(text)),
            _ => None,
        })
        .collect()
}

impl Message {
    /// `numbers` fill `ESC d<n>` (`setSpecialNumber`): 1 day, 2 parts, 3 parts left,
    /// 4 days left, 5 parts to the next upgrade.
    pub fn new(pages: Vec<Screen>, numbers: [i32; 10]) -> Message {
        Message {
            pages,
            page: 0,
            state: State::FadingIn(0.0),
            pane: 0,
            offset: 0,
            timer: 0.0,
            numbers,
            time: 0.0,
        }
    }

    pub fn is_done(&self) -> bool {
        self.pages.is_empty() || matches!(self.state, State::FadingOut(t) if t >= FADE)
    }

    /// Advances typing and page turns. `a`/`b` are presses, `a_held` the fast-forward.
    pub fn update(&mut self, ui: &Ui, dt: f32, a: bool, b: bool, a_held: bool, sounds: &mut Vec<Sound>) {
        self.time += dt;
        let Some(page) = self.pages.get(self.page) else { return };
        let texts = page_texts(page);
        match self.state {
            State::FadingIn(t) => {
                self.state = if t + dt >= FADE { State::Typing } else { State::FadingIn(t + dt) };
            }
            State::FadingOut(t) => self.state = State::FadingOut(t + dt),
            State::Typing => {
                if b {
                    self.pane = texts.len();
                    self.state = State::Shown;
                    return;
                }
                self.timer -= dt * if a_held { 5.0 } else { 1.0 };
                let mut steps = 0;
                while self.timer <= 0.0 && steps < 64 {
                    steps += 1;
                    let Some(text) = texts.get(self.pane) else {
                        self.state = State::Shown;
                        break;
                    };
                    if self.offset >= text.len() {
                        self.pane += 1;
                        self.offset = 0;
                        continue;
                    }
                    let (next, wait, sound) = type_step(text, self.offset, &ui.wait_chars);
                    self.offset = next;
                    if sound {
                        sounds.push(Sound::Typewriter);
                        self.timer += CHAR_TIME + wait;
                    } else {
                        self.timer += wait;
                    }
                }
            }
            State::Shown => {
                if a || b {
                    if self.page + 1 < self.pages.len() {
                        self.page += 1;
                        self.pane = 0;
                        self.offset = 0;
                        self.timer = 0.0;
                        self.state = State::Typing;
                    } else {
                        sounds.push(Sound::Close);
                        self.state = State::FadingOut(0.0);
                    }
                }
            }
        }
    }

    fn alpha(&self) -> f32 {
        match self.state {
            State::FadingIn(t) => (t / FADE).clamp(0.0, 1.0),
            State::FadingOut(t) => (1.0 - t / FADE).clamp(0.0, 1.0),
            _ => 1.0,
        }
    }

    pub fn draw(&self, ui: &mut Ui) {
        let Some(page) = self.pages.get(self.page).cloned() else { return };
        let alpha = self.alpha();
        let s = (screen_width() / 640.0).min(screen_height() / 480.0);
        let ox = (screen_width() - 640.0 * s) / 2.0;
        let oy = (screen_height() - 480.0 * s) / 2.0;
        let blink = |period: f32| ((self.time / period * std::f32::consts::TAU).sin() * 0.5 + 0.5).max(0.15);
        let shown = matches!(self.state, State::Shown);
        let base = ui.base.clone();
        let mut panes = Vec::new();
        base.root.walk(0, 0, &mut panes);
        let mut cursor = None;
        // Page text first so the cursor can sit after it; then draw the frame under it.
        let mut page_panes = Vec::new();
        page.root.walk(0, 0, &mut page_panes);
        for (x, y, p) in &panes {
            let a = match p.tag.as_str() {
                // `mButtonPromptPane`: blinks once the page is out.
                "a_bt" if !shown => continue,
                "a_bt" => alpha * blink(1.0),
                // Drawn at the text's end below.
                "curs" | "capu" => continue,
                t if t.starts_with("pk") => continue,
                _ => alpha,
            };
            draw_pane(ui, p, *x, *y, s, ox, oy, a, None, &self.numbers);
        }
        let mut text_index = 0;
        for (x, y, p) in &page_panes {
            if p.tag.starts_with("pk") || !p.visible {
                continue;
            }
            let reveal = match p.kind {
                PaneKind::TextBox { .. } => {
                    let r = if shown || text_index < self.pane {
                        usize::MAX
                    } else if text_index == self.pane {
                        self.offset
                    } else {
                        0
                    };
                    text_index += 1;
                    Some(r)
                }
                _ => None,
            };
            if let Some(end) = draw_pane(ui, p, *x, *y, s, ox, oy, alpha, reveal, &self.numbers)
                && reveal.is_some_and(|r| r > 0)
            {
                cursor = Some(end);
            }
        }
        // `setCursorXY`: the blinking cursor at the end of the text being typed.
        if !shown
            && let (Some((cx, cy)), Some(curs)) = (cursor, base.root.find("curs"))
        {
            let mut c = curs.clone();
            c.rect[0] = cx as i16;
            c.rect[1] = (cy - c.rect[3] as f32) as i16;
            draw_pane(ui, &c, 0, 0, s, ox, oy, alpha * blink(0.5), None, &self.numbers);
        }
    }
}

/// Draws one pane (not its children) at absolute canvas position (x, y). For text boxes,
/// `reveal` limits how many bytes of the (button-expanded) text show; returns where the pen
/// ended.
#[allow(clippy::too_many_arguments)]
fn draw_pane(
    ui: &mut Ui,
    p: &Pane,
    x: i32,
    y: i32,
    s: f32,
    ox: f32,
    oy: f32,
    alpha: f32,
    reveal: Option<usize>,
    numbers: &[i32; 10],
) -> Option<(f32, f32)> {
    if !p.visible {
        return None;
    }
    let (w, h) = (p.rect[2] as f32, p.rect[3] as f32);
    let to = |cx: f32, cy: f32| vec2(ox + cx * s, oy + cy * s);
    match &p.kind {
        PaneKind::Picture { texture: Some(name), binding, flags } => {
            let tex = ui.texture(name)?;
            let (tw, th) = (tex.width(), tex.height());
            let wrap = (flags >> 3) & 3;
            let (mut px, mut py, mut pw, mut ph) = (x as f32, y as f32, w, h);
            // `drawFullSet`, without tumble: clamped textures keep their size, anchored by
            // the bound edges (8 left, 4 right, 2 top, 1 bottom).
            if wrap == 0 {
                if binding & 8 != 0 {
                    if binding & 4 == 0 && tw < pw {
                        pw = tw;
                    }
                } else if tw < pw {
                    px = if binding & 4 != 0 { px + pw - tw } else { px + (pw - tw) / 2.0 };
                    pw = tw;
                }
                if binding & 2 != 0 {
                    if binding & 1 == 0 && th < ph {
                        ph = th;
                    }
                } else if th < ph {
                    py = if binding & 1 != 0 { py + ph - th } else { py + (ph - th) / 2.0 };
                    ph = th;
                }
            }
            let (l, r, t, b) = (binding & 8 != 0, binding & 4 != 0, binding & 2 != 0, binding & 1 != 0);
            let (mut u0, mut u1) = if l {
                (0.0, if r { 1.0 } else { pw / tw })
            } else if r {
                (1.0 - pw / tw, 1.0)
            } else {
                (0.5 - pw / tw / 2.0, 0.5 + pw / tw / 2.0)
            };
            let (mut v0, mut v1) = if t {
                (0.0, if b { 1.0 } else { ph / th })
            } else if b {
                (1.0 - ph / th, 1.0)
            } else {
                (0.5 - ph / th / 2.0, 0.5 + ph / th / 2.0)
            };
            if flags & 2 != 0 {
                std::mem::swap(&mut u0, &mut u1);
            }
            if flags & 1 != 0 {
                std::mem::swap(&mut v0, &mut v1);
            }
            let p0 = to(px, py);
            let p1 = to(px + pw, py + ph);
            let c = [255, 255, 255, (alpha * 255.0) as u8];
            let v = |p: Vec2, u: f32, v: f32| Vertex {
                position: vec3(p.x, p.y, 0.0),
                uv: vec2(u, v),
                color: c,
                normal: Vec4::ZERO,
            };
            draw_mesh(&Mesh {
                vertices: vec![v(p0, u0, v0), v(vec2(p1.x, p0.y), u1, v0), v(p1, u1, v1), v(vec2(p0.x, p1.y), u0, v1)],
                indices: vec![0, 1, 2, 0, 2, 3],
                texture: Some(tex),
            });
            None
        }
        PaneKind::TextBox { color, grad_color, h_align, v_align, spacing, leading, font_width, font_height, text, .. } => {
            let full = expand_buttons(text);
            let shown = &full[..reveal.unwrap_or(usize::MAX).min(full.len())];
            let shown = fill_numbers(shown, numbers);
            let whole = fill_numbers(&full, numbers);
            let style = TextStyle {
                top: *color,
                bottom: *grad_color,
                spacing: *spacing as f32,
                leading: *leading as f32,
                width: *font_width as f32,
                height: *font_height as f32,
            };
            // Alignment from the whole text so lines don't move while typing.
            let lines = layout(ui, &whole, &style, w, None);
            let block_h = lines.height + style.height;
            let y0 = match v_align {
                1 => y as f32 + h - block_h,
                2 => y as f32 + (h - block_h) / 2.0,
                _ => y as f32,
            };
            let offsets: Vec<f32> = lines
                .widths
                .iter()
                .map(|lw| match h_align {
                    1 => w - lw,
                    2 => (w - lw) / 2.0,
                    _ => 0.0,
                })
                .collect();
            let mut verts = Vec::new();
            let end = layout_draw(ui, &shown, &style, w, x as f32, y0 + style.height, &offsets, alpha, &mut verts);
            for chunk in verts.chunks(4 * 1000) {
                let mut indices = Vec::new();
                for q in 0..chunk.len() / 4 {
                    let b = (q * 4) as u16;
                    indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
                }
                let mapped: Vec<Vertex> = chunk
                    .iter()
                    .map(|v| Vertex {
                        position: vec3(ox + v.position.x * s, oy + v.position.y * s, 0.0),
                        ..*v
                    })
                    .collect();
                draw_mesh(&Mesh { vertices: mapped, indices, texture: Some(ui.font_tex.clone()) });
            }
            Some(end)
        }
        _ => None,
    }
}

struct TextStyle {
    top: [u8; 4],
    bottom: [u8; 4],
    spacing: f32,
    leading: f32,
    width: f32,
    height: f32,
}

struct Lines {
    widths: Vec<f32>,
    height: f32,
}

/// The glyph index for a byte or icon pair (`Font::charToIndex`).
fn glyph_index(text: &[u8], i: usize) -> (usize, usize) {
    if text[i] == 0xFF {
        return (KANJI_BASE + *text.get(i + 1).unwrap_or(&0) as usize, 2);
    }
    if text[i] >= 0x80 {
        // Other two-byte characters aren't in the English screens; show '@'.
        return (b'@' as usize - 0x20, 2);
    }
    ((text[i] as usize).saturating_sub(0x20), 1)
}

fn layout(ui: &Ui, text: &[u8], st: &TextStyle, max_w: f32, _: Option<()>) -> Lines {
    let mut lines = Lines { widths: Vec::new(), height: 0.0 };
    walk_text(ui, text, st, max_w, 0.0, 0.0, &[], |_, _, _, _, _| {}, &mut lines);
    lines
}

#[allow(clippy::too_many_arguments)]
fn layout_draw(
    ui: &Ui,
    text: &[u8],
    st: &TextStyle,
    max_w: f32,
    x: f32,
    y: f32,
    offsets: &[f32],
    alpha: f32,
    out: &mut Vec<Vertex>,
) -> (f32, f32) {
    let mut lines = Lines { widths: Vec::new(), height: 0.0 };
    let (tw, th) = (ui.font.image.width as f32, ui.font.image.height as f32);
    let cell_w = ui.font.cell_width as f32;
    let cell_h = ui.font.cell_height as f32;
    let font = &ui.font;
    walk_text(
        ui,
        text,
        st,
        max_w,
        x,
        y,
        offsets,
        |gi, px, py, (cw, ch), (top, bottom)| {
            let Some(g) = font.glyphs.get(gi) else { return };
            // `P2DFont::drawChar`: the pen is on the baseline at the cell's bottom.
            let (xs, ys) = (cw / cell_w, ch / cell_h);
            let x0 = px - g.left as f32 * xs;
            let x1 = px + (g.width - g.left) as f32 * xs;
            let (y0, y1) = (py - cell_h * ys, py);
            let (u0, u1) = (g.x as f32 / tw, (g.x + g.width) as f32 / tw);
            let (v0, v1) = (g.y as f32 / th, (g.y + g.height - 1) as f32 / th);
            let col = |c: [u8; 4]| [c[0], c[1], c[2], (c[3] as f32 * alpha) as u8];
            let vtx = |x: f32, y: f32, u: f32, v: f32, c: [u8; 4]| Vertex {
                position: vec3(x, y, 0.0),
                uv: vec2(u, v),
                color: col(c),
                normal: Vec4::ZERO,
            };
            out.extend([
                vtx(x0, y0, u0, v0, top),
                vtx(x1, y0, u1, v0, top),
                vtx(x1, y1, u1, v1, bottom),
                vtx(x0, y1, u0, v1, bottom),
            ]);
        },
        &mut lines,
    )
}

/// `P2DPrint::parse`: walks characters and tags, wrapping at `max_w`, calling `glyph` for
/// each drawn character with its pen position, size and colours. Returns the pen's end.
#[allow(clippy::too_many_arguments)]
fn walk_text(
    ui: &Ui,
    text: &[u8],
    st: &TextStyle,
    max_w: f32,
    x: f32,
    y: f32,
    offsets: &[f32],
    mut glyph: impl FnMut(usize, f32, f32, (f32, f32), ([u8; 4], [u8; 4])),
    lines: &mut Lines,
) -> (f32, f32) {
    let (mut cx, mut cy) = (x, y);
    let (mut cw, mut ch) = (st.width, st.height);
    let (mut top, mut bottom) = (st.top, st.bottom);
    let (spacing, mut leading) = (st.spacing, st.leading);
    let mut line = 0usize;
    let mut line_w = 0.0f32;
    let line_x = |line: usize| offsets.get(line).copied().unwrap_or(0.0);
    let num = |t: &[u8]| -> Option<(f32, usize)> {
        if t.first() != Some(&b'[') {
            return None;
        }
        let e = t.iter().position(|&c| c == b']')?;
        Some((std::str::from_utf8(&t[1..e]).ok()?.parse().ok()?, e + 1))
    };
    let col = |t: &[u8]| -> Option<([u8; 4], usize)> {
        if t.first() != Some(&b'[') {
            return None;
        }
        let e = t.iter().position(|&c| c == b']')?;
        let s = std::str::from_utf8(&t[1..e]).ok()?;
        let v = u32::from_str_radix(s, 16).ok()?;
        let v = match s.len() {
            8 => v,
            6 => v << 8 | 0xFF,
            _ => return None,
        };
        Some((v.to_be_bytes(), e + 1))
    };
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == 0x1B {
            let tag = &text[i + 1..];
            let code = tag.get(..2).unwrap_or(b"");
            let args = tag.get(2..).unwrap_or(b"");
            let mut used = 3;
            match code {
                b"CU" | b"CD" | b"CL" | b"CR" => {
                    let (n, k) = num(args).unwrap_or((1.0, 0));
                    used += k;
                    match code {
                        b"CU" => cy -= n,
                        b"CD" => cy += n,
                        b"CL" => cx -= n,
                        _ => cx += n,
                    }
                }
                b"LU" => cy -= leading,
                b"LD" => cy += leading,
                b"FX" => {
                    let (n, k) = num(args).unwrap_or((st.width, 0));
                    used += k;
                    cw = n;
                }
                b"FY" => {
                    let (n, k) = num(args).unwrap_or((st.height, 0));
                    used += k;
                    ch = n;
                }
                b"SV" => {
                    let (n, k) = num(args).unwrap_or((st.leading, 0));
                    used += k;
                    leading = n;
                }
                b"CC" | b"CA" | b"GC" | b"GA" => {
                    if let Some((v, k)) = col(args) {
                        used += k;
                        let keep = |c: [u8; 4], old: [u8; 4]| if code[1] == b'C' { [c[0], c[1], c[2], st.top[3]] } else { [c[0], c[1], c[2], old[3]] };
                        if code[0] == b'C' {
                            top = keep(v, top);
                        } else {
                            bottom = keep(v, bottom);
                        }
                    }
                }
                _ => {
                    // Tags this renderer doesn't lay out: skip any [argument].
                    if let Some(e) = args.first().filter(|&&b| b == b'[').and(args.iter().position(|&c| c == b']')) {
                        used += e + 1;
                    }
                    if code.first() == Some(&b'Z') {
                        used = 2;
                    }
                }
            }
            i += used.min(text.len() - i);
            continue;
        }
        if c == b'\n' {
            lines.widths.push(line_w);
            line += 1;
            line_w = 0.0;
            cx = x;
            cy += leading;
            i += 1;
            continue;
        }
        if c < 0x20 {
            i += 1;
            continue;
        }
        let (gi, n) = glyph_index(text, i);
        let adv = ui.font.glyphs.get(gi).map_or(0.0, |g| g.advance as f32 * cw / ui.font.cell_width as f32 + 1.0);
        if cx + adv - x > max_w && cx > x {
            lines.widths.push(line_w);
            line += 1;
            line_w = 0.0;
            cx = x;
            cy += leading;
            continue;
        }
        glyph(gi, cx + line_x(line), cy, (cw, ch), (top, bottom));
        cx += adv;
        line_w = line_w.max(cx - x);
        cx += spacing;
        lines.height = lines.height.max(cy - y);
        i += n;
    }
    lines.widths.push(line_w);
    (cx + line_x(line), cy)
}
