//! Materials for hosts whose shaders can't run TEV (Risk of Rain 2's HGStandard takes one
//! texture times one colour).
//!
//! A material's TEV stages run on the CPU with lighting left to the host. [`bake`] runs them
//! once per texel over the mesh's texture coordinates, folding every texture map, colour
//! register, konst colour and the material colour into one RGBA image: a leaf's green body and
//! its veins, a pellet's colour, pattern and number. Maps a stage reads through an environment
//! (normal-based) texgen, like the pellets' shine, can't be baked over the mesh's coordinates
//! and count as their average colour there. A material whose only maps are environment maps
//! (Olimar's helmet) bakes over the environment map's own space instead ([`Baked::env`]), and
//! the host gives that batch sphere-mapped coordinates each frame. [`flat_color`] is the same
//! with no texture, for untextured materials.
//!
//! GX semantics as in `DGXGraphics::setMaterial` (sysDolphin/dgxGraphics.cpp): each stage
//! computes `(d ± lerp(a, b, c) + bias) * scale` and writes it to PREV or a register.
//! Materials without TEV stages are texture x material colour.

use libpikmin_assets::model::{TevCombiner, TevInfo, TevStage, mat_flags};
use libpikmin_assets::{Image, Model};

use libcommon::tev::{C4, Combine, Regs, alpha_in, color_in, combine_alpha, combine_color, konst};
use libcommon::texture::{Wrap, sample_rgba8_bilinear};

/// Per-draw overrides of a material's inputs.
#[derive(Debug, Clone, Copy, Default)]
pub struct Inputs {
    /// Replaces the material colour (`ViewPiki` tints material 0 with the Pikmin's colour).
    pub material: Option<[u8; 4]>,
    /// Replaces colour register 0's RGB (a pellet's colour).
    pub reg0: Option<[u8; 4]>,
    /// Frame for material and register colour animations.
    pub frame: Option<f32>,
}

/// A model's texture images, decoded once.
pub struct Images {
    decoded: Vec<Option<Image>>,
    average: Vec<C4>,
}

impl Images {
    pub fn new(model: &Model) -> Self {
        let decoded: Vec<Option<Image>> = model.textures.iter().map(|t| t.decode().ok()).collect();
        let average = decoded
            .iter()
            .map(|img| {
                let Some(img) = img else { return [1.0; 4] };
                let n = (img.rgba.len() / 4).max(1) as f32;
                let mut s = [0.0f32; 4];
                for px in img.rgba.as_chunks::<4>().0 {
                    for c in 0..4 {
                        s[c] += px[c] as f32;
                    }
                }
                s.map(|v| v / n / 255.0)
            })
            .collect();
        Images { decoded, average }
    }

    fn image(&self, model: &Model, attr: usize) -> Option<(&Image, [u8; 2])> {
        let a = model.tex_attrs.get(attr)?;
        if a.image & 0x8000 != 0 {
            return None;
        }
        let img = self.decoded.get(a.image as usize)?.as_ref()?;
        Some((img, [wrap(a.tiling), wrap(a.tiling >> 8)]))
    }

    fn average(&self, model: &Model, attr: usize) -> Option<C4> {
        let a = model.tex_attrs.get(attr)?;
        self.average.get(a.image as usize).copied()
    }
}

/// `TexAttr` tiling bits to 0 repeat, 1 clamp, 2 mirror.
pub fn wrap(bits: u16) -> u8 {
    if bits & 1 != 0 {
        1
    } else if bits & 2 != 0 {
        2
    } else {
        0
    }
}

/// A baked material: RGBA8, rows top first, sampled with the mesh's texture coordinates.
#[derive(Debug, Clone)]
pub struct Baked {
    pub width: u32,
    pub height: u32,
    /// 0 repeat, 1 clamp, 2 mirror.
    pub wrap: [u8; 2],
    pub rgba: Vec<u8>,
    /// Baked over an environment map: sample it at the view-space normal's `(x, -y)` through
    /// this matrix (`GX_TG_NRM` with a texture matrix; `None` is identity), not the mesh's
    /// coordinates.
    pub env: Option<Option<[[f32; 3]; 2]>>,
}

/// An environment-mapped texture coordinate: view-space normal `n` through `mtx`, as GX's
/// normal texgen (the viewer's shader does the same).
pub fn env_uv(n: glam::Vec3, mtx: Option<[[f32; 3]; 2]>) -> [f32; 2] {
    let (x, y) = (n.x, -n.y);
    match mtx {
        Some(m) => [m[0][0] * x + m[0][1] * y + m[0][2], m[1][0] * x + m[1][1] * y + m[1][2]],
        None => [x, y],
    }
}

impl Baked {
    /// The lowest alpha anywhere, 0..1.
    pub fn min_alpha(&self) -> f32 {
        self.rgba.as_chunks::<4>().0.iter().map(|p| p[3]).min().unwrap_or(255) as f32 / 255.0
    }
}

fn tev(model: &Model, mi: usize) -> Option<&TevInfo> {
    let m = &model.materials[mi];
    m.tev_info
        .and_then(|t| model.tev_infos.get(t))
        .filter(|t| m.flags & mat_flags::PVW != 0 && !t.stages.is_empty())
}

/// The material colour and colour registers with `inputs` applied, 0..255.
fn resolve(model: &Model, mi: usize, inputs: Inputs) -> ([u8; 4], [[u8; 4]; 3]) {
    let m = &model.materials[mi];
    let mat = inputs.material.unwrap_or_else(|| m.color_at(inputs.frame));
    let mut regs = [[255u8; 4]; 3];
    if let Some(t) = tev(model, mi) {
        regs = t.regs.each_ref().map(|r| r.color_at(inputs.frame).map(|v| v.round().clamp(0.0, 255.0) as u8));
        if let Some(c) = inputs.reg0 {
            regs[0][..3].copy_from_slice(&c[..3]);
        }
    }
    (mat, regs)
}

/// What decides how `inputs` change a material: two inputs with the same key look the same.
pub fn key(model: &Model, mi: usize, inputs: Inputs) -> [u8; 16] {
    let (mat, regs) = resolve(model, mi, inputs);
    let mut k = [0u8; 16];
    k[..4].copy_from_slice(&mat);
    for (i, r) in regs.iter().enumerate() {
        k[4 + 4 * i..8 + 4 * i].copy_from_slice(r);
    }
    k
}

/// Where a stage's texture read comes from.
#[derive(Clone, Copy)]
enum Read {
    /// No texture: white.
    None,
    /// `TexAttr` `attr` over the mesh's coordinates, through a texture matrix.
    Uv { attr: usize, mtx: Option<[[f32; 3]; 2]> },
    /// An environment map (`GX_TG_NRM`) through a texture matrix.
    Env { attr: usize, mtx: Option<[[f32; 3]; 2]> },
    /// Some other generated coordinate, as the map's average colour.
    Average(C4),
}

/// Each stage's read at rest (one for a material without TEV stages).
fn reads(model: &Model, mi: usize, images: &Images) -> Vec<Read> {
    reads_at(model, mi, images, 0.0)
}

fn reads_at(model: &Model, mi: usize, images: &Images, seconds: f32) -> Vec<Read> {
    let m = &model.materials[mi];
    let Some(t) = tev(model, mi) else {
        return vec![m.tex_attr.map_or(Read::None, |attr| Read::Uv { attr, mtx: None })];
    };
    // `setMatMatrices`: the n-th matrix texgen uses the n-th texture data's matrix.
    let mut gens = Vec::new();
    let mut j = 0;
    for g in &m.tex_gens {
        let mtx = if g.kind == 1 {
            let mtx = m.tex_datas.get(j).filter(|_| g.matrix != 10).and_then(|d| d.matrix(seconds * 30.0 * d.speed));
            j += 1;
            mtx
        } else {
            None
        };
        gens.push((g.src, g.kind, mtx));
    }
    t.stages
        .iter()
        .map(|s: &TevStage| {
            let Some(attr) = m.tex_datas.get(s.tex_map as usize).map(|d| d.tex_attr) else {
                return Read::None;
            };
            match gens.get(s.tex_coord as usize) {
                // GX_TG_TEX0..7: the mesh has one set of coordinates.
                Some(&(src, 1, mtx)) if (4..=11).contains(&src) => Read::Uv { attr, mtx },
                Some(&(1, 1, mtx)) => Read::Env { attr, mtx },
                Some(_) => images.average(model, attr).map_or(Read::None, Read::Average),
                None => Read::None,
            }
        })
        .collect()
}

/// Runs the material with `tex(stage)` for each stage's texture read.
fn eval(model: &Model, mi: usize, mat: [u8; 4], regs: [[u8; 4]; 3], tex: &dyn Fn(usize) -> C4) -> C4 {
    let mat = mat.map(|v| v as f32 / 255.0);
    let Some(t) = tev(model, mi) else {
        let c = tex(0);
        return std::array::from_fn(|i| c[i] * mat[i]);
    };
    let mut g = Regs { prev: [0.0; 4], r: regs.map(|r| r.map(|v| v as f32 / 255.0)) };
    let konsts = t.konst.map(|k| k.map(|v| v as f32 / 255.0));
    let combine = |c: &libpikmin_assets::model::TevCombiner| Combine {
        op: c.op,
        bias: c.bias,
        scale: c.scale,
        clamp: c.clamp,
    };
    for (i, s) in t.stages.iter().enumerate() {
        // Channel 0 is the lit material colour (lighting is the host's job); channel 1 is the
        // specular term, which has no flat value.
        let ras = match s.channel {
            0 | 4 => mat,
            1 | 5 => [0.0, 0.0, 0.0, 1.0],
            _ => [0.0; 4],
        };
        let tx = tex(i);
        let kc = konst(s.kcolor_sel, &konsts);
        let ka = konst(s.kalpha_sel, &konsts);
        let c = &s.color;
        let rgb = combine_color(
            &combine(c),
            color_in(c.a, &g, tx, ras, kc),
            color_in(c.b, &g, tx, ras, kc),
            color_in(c.c, &g, tx, ras, kc),
            color_in(c.d, &g, tx, ras, kc),
        );
        let al = &s.alpha;
        let a = combine_alpha(
            &combine(al),
            alpha_in(al.a, &g, tx, ras, ka),
            alpha_in(al.b, &g, tx, ras, ka),
            alpha_in(al.c, &g, tx, ras, ka),
            alpha_in(al.d, &g, tx, ras, ka),
        );
        g.slot(c.out)[..3].copy_from_slice(&rgb);
        g.slot(al.out)[3] = a;
    }
    g.prev.map(|v| v.clamp(0.0, 1.0))
}

/// The colour of `material` with every texture over the mesh's coordinates read as white (what
/// a baked texture gets multiplied by is white; this is for untextured materials), RGBA 0..1.
pub fn flat_color(model: &Model, mi: usize, inputs: Inputs, images: &Images) -> C4 {
    let rd = reads(model, mi, images);
    let (mat, regs) = resolve(model, mi, inputs);
    eval(model, mi, mat, regs, &|i| match rd.get(i) {
        Some(Read::Average(c)) => *c,
        Some(Read::Env { attr, .. }) => images.average(model, *attr).unwrap_or([1.0; 4]),
        _ => [1.0; 4],
    })
}

/// Whether `material` reads a texture over the mesh's or an environment map's coordinates (so
/// has a [`bake`]).
pub fn textured(model: &Model, mi: usize, images: &Images) -> bool {
    reads(model, mi, images).iter().any(|r| match r {
        Read::Uv { attr, .. } | Read::Env { attr, .. } => images.image(model, *attr).is_some(),
        _ => false,
    })
}

/// `material` with `inputs`, baked over the mesh's texture coordinates at the largest of its
/// textures' sizes, or failing that over its environment maps' own space ([`Baked::env`]).
/// `None` for an untextured material.
pub fn bake(model: &Model, mi: usize, inputs: Inputs, images: &Images) -> Option<Baked> {
    bake_at(model, mi, inputs, images, 0.0)
}

/// PVWTextureData::animate: each layer advances at its own disc-authored speed.
pub fn bake_at(model: &Model, mi: usize, inputs: Inputs, images: &Images, seconds: f32) -> Option<Baked> {
    let rd = reads_at(model, mi, images, seconds);
    let uv_mapped = rd.iter().any(|r| matches!(r, Read::Uv { attr, .. } if images.image(model, *attr).is_some()));
    let mut size = (0u32, 0u32);
    let mut out_wrap = None;
    let mut env = None;
    for r in &rd {
        let (attr, env_mtx) = match *r {
            Read::Uv { attr, .. } => (attr, None),
            Read::Env { attr, mtx } if !uv_mapped => (attr, Some(mtx)),
            _ => continue,
        };
        if let Some((img, w)) = images.image(model, attr) {
            size = (size.0.max(img.width), size.1.max(img.height));
            out_wrap.get_or_insert(w);
            if env.is_none() {
                env = env_mtx;
            }
        }
    }
    let wrap = out_wrap?;
    let (w, h) = size;
    if w == 0 || h == 0 {
        return None;
    }
    let (mat, regs) = resolve(model, mi, inputs);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            let c = eval(model, mi, mat, regs, &|i| match rd.get(i) {
                Some(Read::Uv { attr, mtx }) => match images.image(model, *attr) {
                    Some((img, wr)) => {
                        let (su, sv) = match mtx {
                            Some(m) => (m[0][0] * u + m[0][1] * v + m[0][2], m[1][0] * u + m[1][1] * v + m[1][2]),
                            None => (u, v),
                        };
                        sample(img, wr, su, sv)
                    }
                    None => [1.0; 4],
                },
                // Over the mesh's coordinates an environment map is its average; baked over its
                // own space, it's read where the host's coordinates will point.
                Some(Read::Env { attr, .. }) if uv_mapped => images.average(model, *attr).unwrap_or([1.0; 4]),
                Some(Read::Env { attr, .. }) => match images.image(model, *attr) {
                    Some((img, wr)) => sample(img, wr, u, v),
                    None => [1.0; 4],
                },
                Some(Read::Average(c)) => *c,
                _ => [1.0; 4],
            });
            rgba.extend(c.map(|v| (v * 255.0).round() as u8));
        }
    }
    Some(Baked { width: w, height: h, wrap, rgba, env })
}

/// GX bilinear filtering at `uv`, wrapping as the texture says (host codes: 0 repeat, 1 clamp,
/// 2 mirror). White for an empty image.
fn sample(img: &Image, wrap: [u8; 2], u: f32, v: f32) -> C4 {
    sample_rgba8_bilinear(img.width, img.height, &img.rgba, wrap.map(Wrap::from_host), u, v)
        .unwrap_or([1.0; 4])
}
