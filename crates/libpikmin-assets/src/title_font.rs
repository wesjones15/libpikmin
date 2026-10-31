//! Area-name lettering from Pikmin 2's course-name screens, as one glyph per character.
//!
//! The Pikmin 1 area titles ("The Impact Site") are not a font on either disc. Pikmin 2 draws "Awakening Wood",
//! "Perplexing Pool" and "Wistful Wild" from one texture per letter (`al_s_*` small, `al_l_*` large;
//! `res_course_name01..03.szs`) and ships "Valley of Repose" and "Challenge Cave" as whole-string textures. Those
//! files are copied out by `libsms::extract::p2_treasure` into `p2/treasure/titlefont/`.
//!
//! The letter textures are placed one by one by the screens' layouts, so they carry no common baseline. This
//! module re-aligns them: small letters sit on a baseline, descenders hang from the x-height line. Letters the
//! disc lacks come from the nearest relative, in the order the user chose:
//! - `b`, `q`, `z` mirror `d`, `p`, `s`;
//! - `c` is a shrunken `C`, `m` a widened `n`, `v` and `h` come from "Challenge Cave", `y` from "Valley of Repose";
//! - capitals the disc has no letter for (everything but A C P R V W) are the lowercase letter at capital height.
//!
//! Every glyph is returned in a canvas of the same height ([`TitleFont::height`]), cropped to its width, so a host
//! only scales by height and advances by width.

use std::collections::HashMap;
use std::path::Path;

use crate::texture::{Image, decode_bti};

/// Where the letter files are, under the assets root.
pub const DIR: &str = "p2/treasure/titlefont";

/// Pixels of padding kept around each glyph's ink.
const PAD: i32 = 2;
/// Opacity at which a stroke counts for alignment (the glow around it does not).
const SOLID: u8 = 128;

#[derive(Clone, Copy, PartialEq)]
enum Class {
    /// Sits on the baseline: a c e m n o r s u v w x z.
    Small,
    /// Sits on the baseline, taller than the x-height (ascenders, dots and capitals).
    Tall,
    /// Hangs from the x-height line: g j p q y.
    Descender,
}

/// A letter before it is aligned: its image, class, and the x-height it was drawn at (capitals are scaled).
struct Raw {
    image: Image,
    class: Class,
    x_height: f32,
}

/// The built glyphs.
#[derive(Debug, Clone)]
pub struct TitleFont {
    glyphs: HashMap<char, Image>,
    /// Height of every glyph's canvas.
    pub height: u32,
    /// Rows from the top of a glyph to the baseline.
    pub baseline: u32,
}

type Rect = (i32, i32, i32, i32);

fn alpha(image: &Image, x: i32, y: i32) -> u8 {
    image.rgba[((y as usize) * image.width as usize + x as usize) * 4 + 3]
}

/// Bounding box (`x0, y0, x1, y1`, ends exclusive) of the pixels at least `threshold` opaque.
fn bounds(image: &Image, threshold: u8) -> Option<Rect> {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, -1, -1);
    for y in 0..image.height as i32 {
        for x in 0..image.width as i32 {
            if alpha(image, x, y) >= threshold {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    (x1 > 0).then_some((x0, y0, x1, y1))
}

fn flip_x(image: &Image) -> Image {
    let (w, h) = (image.width as usize, image.height as usize);
    let mut rgba = vec![0u8; image.rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let (from, to) = ((y * w + x) * 4, (y * w + (w - 1 - x)) * 4);
            rgba[to..to + 4].copy_from_slice(&image.rgba[from..from + 4]);
        }
    }
    Image { width: image.width, height: image.height, rgba }
}

/// Bilinear resize, weighting colour by alpha so a stroke's edge does not darken.
fn resample(image: &Image, width: u32, height: u32) -> Image {
    let (width, height) = (width.max(1), height.max(1));
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let (sw, sh) = (image.width as f32, image.height as f32);
    let at = |x: i32, y: i32| {
        let (x, y) = (x.clamp(0, image.width as i32 - 1) as usize, y.clamp(0, image.height as i32 - 1) as usize);
        let p = &image.rgba[(y * image.width as usize + x) * 4..][..4];
        let a = p[3] as f32;
        [p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, a]
    };
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let fx = (x as f32 + 0.5) * sw / width as f32 - 0.5;
            let fy = (y as f32 + 0.5) * sh / height as f32 - 0.5;
            let (x0, y0) = (fx.floor() as i32, fy.floor() as i32);
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            let mut sum = [0f32; 4];
            for (dx, dy, w) in [(0, 0, (1. - tx) * (1. - ty)), (1, 0, tx * (1. - ty)), (0, 1, (1. - tx) * ty), (1, 1, tx * ty)] {
                let p = at(x0 + dx, y0 + dy);
                for i in 0..4 {
                    sum[i] += p[i] * w;
                }
            }
            let out = &mut rgba[((y as u32 * width + x as u32) * 4) as usize..][..4];
            if sum[3] > 0.0 {
                for i in 0..3 {
                    out[i] = (sum[i] / sum[3]).round().clamp(0., 255.) as u8;
                }
            }
            out[3] = sum[3].round().clamp(0., 255.) as u8;
        }
    }
    Image { width, height, rgba }
}

/// The 8-connected strokes at least [`SOLID`] opaque, left to right.
fn components(image: &Image) -> Vec<Rect> {
    let (w, h) = (image.width as i32, image.height as i32);
    let mut seen = vec![false; (w * h) as usize];
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if seen[(y * w + x) as usize] || alpha(image, x, y) < SOLID {
                continue;
            }
            let (mut x0, mut y0, mut x1, mut y1, mut count) = (x, y, x + 1, y + 1, 0);
            let mut stack = vec![(x, y)];
            seen[(y * w + x) as usize] = true;
            while let Some((cx, cy)) = stack.pop() {
                count += 1;
                (x0, y0, x1, y1) = (x0.min(cx), y0.min(cy), x1.max(cx + 1), y1.max(cy + 1));
                for (nx, ny) in (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (cx + dx, cy + dy))) {
                    if nx >= 0 && ny >= 0 && nx < w && ny < h && !seen[(ny * w + nx) as usize] && alpha(image, nx, ny) >= SOLID {
                        seen[(ny * w + nx) as usize] = true;
                        stack.push((nx, ny));
                    }
                }
            }
            if count > 6 {
                out.push((x0, y0, x1, y1));
            }
        }
    }
    out.sort();
    out
}

/// Splits a whole-string texture into `groups` letters: strokes are merged across their smallest gaps until
/// that many are left (a letter's own pieces, like an `h`'s stem and arch, touch closer than letters do).
/// `None` when the texture has fewer strokes than letters.
fn split(image: &Image, groups: usize) -> Option<Vec<Rect>> {
    let mut parts = components(image);
    if parts.len() < groups {
        return None;
    }
    while parts.len() > groups {
        let gap = |i: usize| parts[i + 1].0 - parts[i].2;
        let i = (0..parts.len() - 1).min_by_key(|&i| gap(i))?;
        let next = parts.remove(i + 1);
        let p = &mut parts[i];
        *p = (p.0.min(next.0), p.1.min(next.1), p.2.max(next.2), p.3.max(next.3));
    }
    Some(parts)
}

fn crop(image: &Image, (x0, y0, x1, y1): Rect) -> Image {
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        let from = ((y0 as usize + y) * image.width as usize + x0 as usize) * 4;
        rgba[y * w * 4..(y + 1) * w * 4].copy_from_slice(&image.rgba[from..from + w * 4]);
    }
    Image { width: w as u32, height: h as u32, rgba }
}

fn class_of(c: char) -> Class {
    match c {
        'g' | 'j' | 'p' | 'q' | 'y' => Class::Descender,
        'a' | 'c' | 'e' | 'm' | 'n' | 'o' | 'r' | 's' | 'u' | 'v' | 'w' | 'x' | 'z' => Class::Small,
        _ => Class::Tall,
    }
}

fn load_image(dir: &Path, name: &str) -> Option<Image> {
    decode_bti(&std::fs::read(dir.join(name)).ok()?).ok().map(|(image, _, _)| image)
}

/// The letters of a whole-string texture, cut out and rescaled to the per-letter textures' proportions by
/// matching the string's `a` to the per-letter `a`. `groups` names each piece; only `want` are returned.
fn cut_string(
    image: &Image, groups: &[&str], reference_a: Rect, a_group: usize, want: &[(usize, char)],
) -> Vec<(char, Image)> {
    let Some(rects) = split(image, groups.len()) else { return Vec::new() };
    let a = rects[a_group];
    let sx = (reference_a.2 - reference_a.0) as f32 / (a.2 - a.0) as f32;
    let sy = (reference_a.3 - reference_a.1) as f32 / (a.3 - a.1) as f32;
    want.iter()
        .map(|&(i, c)| {
            // Stop at the middle of the gap to a neighbour so its ink stays out.
            let (r, w) = (rects[i], image.width as i32);
            let left = if i > 0 { (r.0 - 2).max((rects[i - 1].2 + r.0 + 1) / 2) } else { (r.0 - 2).max(0) };
            let right = if i + 1 < rects.len() { (r.2 + 2).min((r.2 + rects[i + 1].0) / 2) } else { (r.2 + 2).min(w) };
            let piece = crop(image, (left.min(r.0), 0, right.max(r.2), image.height as i32));
            let scaled = resample(&piece, (piece.width as f32 * sx).round() as u32, (piece.height as f32 * sy).round() as u32);
            (c, scaled)
        })
        .collect()
}

impl TitleFont {
    /// Builds the glyphs from the extracted letter files under `dir` (`<assets>/`[`DIR`]). `None` when the
    /// per-letter textures are missing (no Pikmin 2 disc was extracted).
    pub fn load(dir: &Path) -> Option<TitleFont> {
        let mut raws: HashMap<char, Raw> = HashMap::new();
        let mut x_heights = Vec::new();
        for c in 'a'..='z' {
            if let Some(image) = load_image(dir, &format!("al_s_{c}.bti")) {
                if class_of(c) == Class::Small {
                    x_heights.push(bounds(&image, SOLID).map(|b| (b.3 - b.1) as f32)?);
                }
                raws.insert(c, Raw { image, class: class_of(c), x_height: 0.0 });
            }
            if let Some(image) = load_image(dir, &format!("al_l_{c}.bti")) {
                raws.insert(c.to_ascii_uppercase(), Raw { image, class: Class::Tall, x_height: 0.0 });
            }
        }
        if x_heights.len() < 3 {
            return None;
        }
        x_heights.sort_by(|a, b| a.total_cmp(b));
        let x_height = x_heights[x_heights.len() / 2];
        for r in raws.values_mut() {
            r.x_height = x_height;
        }
        let a_box = bounds(&raws.get(&'a')?.image, SOLID)?;

        // Whole-string names: y and the capitals V, R from "Valley of Repose"; C, h and v from "Challenge Cave".
        let valley = ["V", "a", "l", "l", "e", "y", "o", "f", "R", "e", "p", "o", "s", "e"];
        let cave = ["C", "h", "a", "l", "l", "e", "n", "ge", "C", "a", "v", "e"];
        let mut cut = Vec::new();
        if let Some(image) = load_image(dir, "valley_of_repose.bti") {
            cut.extend(cut_string(&image, &valley, a_box, 1, &[(0, 'V'), (5, 'y'), (8, 'R')]));
        }
        if let Some(image) = load_image(dir, "challenge_cave.bti") {
            cut.extend(cut_string(&image, &cave, a_box, 2, &[(0, 'C'), (1, 'h'), (10, 'v')]));
        }
        for (c, image) in cut {
            raws.entry(c).or_insert(Raw { image, class: class_of(c), x_height });
        }

        // The mirrored and derived letters.
        let mirrored = [('b', 'd'), ('q', 'p'), ('z', 's')];
        for (to, from) in mirrored {
            if let Some(r) = raws.get(&from).filter(|_| !raws.contains_key(&to)) {
                let image = flip_x(&r.image);
                raws.insert(to, Raw { image, class: class_of(to), x_height });
            }
        }
        // No `j` on the disc: the `i` stands in.
        if !raws.contains_key(&'j') {
            if let Some(i) = raws.get(&'i') {
                let image = i.image.clone();
                raws.insert('j', Raw { image, class: Class::Tall, x_height });
            }
        }
        if !raws.contains_key(&'c') {
            if let Some(big) = raws.get(&'C') {
                let b = bounds(&big.image, SOLID)?;
                let k = x_height / (b.3 - b.1) as f32;
                let image = resample(&big.image, (big.image.width as f32 * k).round() as u32, (big.image.height as f32 * k).round() as u32);
                raws.insert('c', Raw { image, class: Class::Small, x_height });
            }
        }
        if !raws.contains_key(&'m') {
            if let Some(n) = raws.get(&'n') {
                let image = resample(&n.image, (n.image.width as f32 * 1.5).round() as u32, n.image.height);
                raws.insert('m', Raw { image, class: Class::Small, x_height });
            }
        }
        // Capitals the disc lacks: the lowercase letter at capital height.
        let cap_height = raws.get(&'A').and_then(|r| bounds(&r.image, SOLID)).map_or(x_height * 1.4, |b| (b.3 - b.1) as f32);
        let k = cap_height / x_height;
        for c in 'a'..='z' {
            let upper = c.to_ascii_uppercase();
            if raws.contains_key(&upper) {
                continue;
            }
            // `I` is a plain stroke: the `l`, not the dotted `i`.
            let Some(r) = raws.get(&if c == 'i' { 'l' } else { c }) else { continue };
            // Descenders scale by the x-height (their tail then hangs below); the rest to the capital height.
            let k = match (r.class, bounds(&r.image, SOLID)) {
                (Class::Descender, _) | (_, None) => k,
                (_, Some(b)) => cap_height / (b.3 - b.1) as f32,
            };
            let image = resample(&r.image, (r.image.width as f32 * k).round() as u32, (r.image.height as f32 * k).round() as u32);
            let class = if r.class == Class::Small { Class::Tall } else { r.class };
            let x_height = x_height * if r.class == Class::Descender { k } else { 1.0 };
            raws.insert(upper, Raw { image, class, x_height });
        }

        // Align: find each glyph's baseline row and ink box, then size one canvas for all of them.
        struct Placed { c: char, image: Image, ink: Rect, baseline: i32 }
        let mut placed = Vec::new();
        for (&c, r) in &raws {
            let (Some(solid), Some(ink)) = (bounds(&r.image, SOLID), bounds(&r.image, 8)) else { continue };
            let baseline = if r.class == Class::Descender { solid.1 + r.x_height.round() as i32 } else { solid.3 };
            placed.push(Placed { c, image: crop(&r.image, ink), ink, baseline });
        }
        let up = placed.iter().map(|p| p.baseline - p.ink.1).max()? + PAD;
        let down = placed.iter().map(|p| p.ink.3 - p.baseline).max()?.max(0) + PAD;
        let height = (up + down) as u32;
        let mut glyphs = HashMap::new();
        for p in placed {
            let width = p.image.width as i32 + 2 * PAD;
            let mut rgba = vec![0u8; (width * height as i32 * 4) as usize];
            let top = up - (p.baseline - p.ink.1);
            for y in 0..p.image.height as i32 {
                let (dy, from) = (top + y, y as usize * p.image.width as usize * 4);
                if dy < 0 || dy >= height as i32 {
                    continue;
                }
                let to = ((dy * width + PAD) * 4) as usize;
                rgba[to..to + p.image.width as usize * 4].copy_from_slice(&p.image.rgba[from..from + p.image.width as usize * 4]);
            }
            glyphs.insert(p.c, Image { width: width as u32, height, rgba });
        }
        Some(TitleFont { glyphs, height, baseline: up as u32 })
    }

    /// The glyph for `c`, if the disc and the substitutions above cover it.
    pub fn glyph(&self, c: char) -> Option<&Image> {
        self.glyphs.get(&c)
    }
}
