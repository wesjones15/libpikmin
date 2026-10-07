//! A material's flat colour, for hosts whose shaders can't run TEV (Risk of Rain 2's
//! HGStandard takes one texture times one colour).
//!
//! The material's TEV stages run once on the CPU with its texture read as white and lighting
//! left to the host, so what comes out is the colour the texture gets multiplied by: the
//! colour registers (an Onion's colour lives in an animated register), the konst colours and
//! the material colour all fold in. GX semantics as in `DGXGraphics::setMaterial`
//! (sysDolphin/dgxGraphics.cpp): each stage computes `(d ± lerp(a, b, c) + bias) * scale` and
//! writes it to PREV or a register. Materials without TEV stages are texture x material colour.

use libpikmin_assets::model::{TevCombiner, TevInfo, mat_flags};
use libpikmin_assets::{Material, Model};

type C4 = [f32; 4];

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

/// GX konst selection (`GXTevKColorSel` / `GXTevKAlphaSel`): 0..7 are fractions 8/8..1/8,
/// 12..15 a konst colour, then its R, G, B, A replicated.
fn konst(sel: u8, k: &[[u8; 4]; 4]) -> C4 {
    if sel < 8 {
        let v = 1.0 - sel as f32 / 8.0;
        return [v; 4];
    }
    let idx = (sel as usize).wrapping_sub(12) % 4;
    let c = k[idx].map(|v| v as f32 / 255.0);
    match sel {
        12..=15 => c,
        16..=19 => [c[0]; 4],
        20..=23 => [c[1]; 4],
        24..=27 => [c[2]; 4],
        _ => [c[3]; 4],
    }
}

struct Regs {
    prev: C4,
    r: [C4; 3],
}

/// `GXTevColorArg`.
fn color_in(a: u8, g: &Regs, tex: C4, ras: C4, k: C4) -> [f32; 3] {
    let rgb = |c: C4| [c[0], c[1], c[2]];
    let rep = |v: f32| [v; 3];
    match a {
        0 => rgb(g.prev),
        1 => rep(g.prev[3]),
        2 => rgb(g.r[0]),
        3 => rep(g.r[0][3]),
        4 => rgb(g.r[1]),
        5 => rep(g.r[1][3]),
        6 => rgb(g.r[2]),
        7 => rep(g.r[2][3]),
        8 => rgb(tex),
        9 => rep(tex[3]),
        10 => rgb(ras),
        11 => rep(ras[3]),
        12 => rep(1.0),
        13 => rep(0.5),
        14 => rgb(k),
        _ => rep(0.0),
    }
}

/// `GXTevAlphaArg`.
fn alpha_in(a: u8, g: &Regs, tex: C4, ras: C4, k: C4) -> f32 {
    match a {
        0 => g.prev[3],
        1 => g.r[0][3],
        2 => g.r[1][3],
        3 => g.r[2][3],
        4 => tex[3],
        5 => ras[3],
        6 => k[3],
        _ => 0.0,
    }
}

/// The register a stage writes (`GX_TEVPREV`, `GX_TEVREG0..2`).
fn slot(g: &mut Regs, out: u8) -> &mut C4 {
    match out {
        0 => &mut g.prev,
        n => &mut g.r[(n as usize - 1).min(2)],
    }
}

fn combine(c: &TevCombiner, a: f32, b: f32, cc: f32, d: f32) -> f32 {
    let (a, b, cc) = (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0), cc.clamp(0.0, 1.0));
    let l = a + (b - a) * cc;
    let bias = match c.bias {
        1 => 0.5,
        2 => -0.5,
        _ => 0.0,
    };
    let scale = match c.scale {
        1 => 2.0,
        2 => 4.0,
        3 => 0.5,
        _ => 1.0,
    };
    let v = (d + if c.op == 1 { -l } else { l } + bias) * scale;
    if c.clamp { v.clamp(0.0, 1.0) } else { v.clamp(-4.0, 4.0) }
}

/// The colour `material` multiplies its texture by, RGBA 0..1.
pub fn flat_color(model: &Model, mi: usize, inputs: Inputs) -> C4 {
    let m: &Material = &model.materials[mi];
    let mat = inputs
        .material
        .unwrap_or_else(|| m.color_at(inputs.frame))
        .map(|v| v as f32 / 255.0);
    let pvw = m.flags & mat_flags::PVW != 0;
    let tev: Option<&TevInfo> = m.tev_info.and_then(|t| model.tev_infos.get(t)).filter(|_| pvw);
    let Some(tev) = tev.filter(|t| !t.stages.is_empty()) else {
        return mat;
    };
    let mut regs = tev.regs.each_ref().map(|r| r.color_at(inputs.frame).map(|v| v / 255.0));
    if let Some(c) = inputs.reg0 {
        for ch in 0..3 {
            regs[0][ch] = c[ch] as f32 / 255.0;
        }
    }
    let mut g = Regs { prev: [0.0; 4], r: regs };
    let white = [1.0; 4];
    for s in &tev.stages {
        // Channel 0 is the lit material colour (lighting is the host's job); channel 1 is the
        // specular term, which has no flat value.
        let ras = match s.channel {
            0 | 4 => mat,
            1 | 5 => [0.0, 0.0, 0.0, 1.0],
            _ => [0.0; 4],
        };
        let kc = konst(s.kcolor_sel, &tev.konst);
        let ka = konst(s.kalpha_sel, &tev.konst);
        let c = &s.color;
        let (ca, cb, ccc, cd) = (
            color_in(c.a, &g, white, ras, kc),
            color_in(c.b, &g, white, ras, kc),
            color_in(c.c, &g, white, ras, kc),
            color_in(c.d, &g, white, ras, kc),
        );
        let rgb: [f32; 3] = std::array::from_fn(|i| combine(c, ca[i], cb[i], ccc[i], cd[i]));
        let al = &s.alpha;
        let a = combine(
            al,
            alpha_in(al.a, &g, white, ras, ka),
            alpha_in(al.b, &g, white, ras, ka),
            alpha_in(al.c, &g, white, ras, ka),
            alpha_in(al.d, &g, white, ras, ka),
        );
        slot(&mut g, c.out)[..3].copy_from_slice(&rgb);
        slot(&mut g, al.out)[3] = a;
    }
    g.prev.map(|v| v.clamp(0.0, 1.0))
}

/// The texture (`TexAttr` index) a material's colour comes from: its first stage that reads a
/// texture map, or the plain material's texture.
pub fn texture_of(model: &Model, mi: usize) -> Option<usize> {
    let m = &model.materials[mi];
    if m.flags & mat_flags::PVW == 0 {
        return m.tex_attr;
    }
    let tev = m.tev_info.and_then(|t| model.tev_infos.get(t));
    let map = tev
        .and_then(|t| t.stages.iter().find(|s| s.tex_map != 0xFF).map(|s| s.tex_map as usize))
        .unwrap_or(0);
    m.tex_datas.get(map).map(|d| d.tex_attr).or(m.tex_attr)
}
