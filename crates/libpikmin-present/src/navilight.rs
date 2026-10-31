//! Louie's navi light: a stand-in for Pikmin 2's `TNaviEffect` light (`efx::TOrimaLight`, efxPikmin.cpp), the one
//! particle effect that differs between him and Olimar (`setNaviType` gives Louie `PID_OrimaLight_Loozy_1/_2`,
//! 0xDE and 0xE1; Olimar's are 0x123 and 0x126; ParticleID.h).
//!
//! The real thing is a JPA2 emitter (a star shape with a trail and a base). There is no JPA2 emitter engine here,
//! so this reads only the resources' base shapes (`libsms_assets::jpa2`): their textures, colours and blend, and
//! draws each as one camera-facing glow over the captain's head, breathing slowly. Colour follows the particle's
//! TEV the way `Flat` splits it: `alpha * (env * (1 - texture) + prm * texture)`, added to the screen.

use std::path::Path;

use glam::Vec3;
use libpikmin_assets::Image;
use libsms_assets::jpa2::Jpc;

use crate::effects::{Flat, FlatQuad};

/// `PID_OrimaLight_Loozy_1` (the star) and `_2` (the trail / base).
pub const LOUIE_STAR: u16 = 0xDE;
pub const LOUIE_BASE: u16 = 0xE1;

/// One glow: its texture split for additive layers, and where those layers live in [`Flat::textures`].
pub(crate) struct Glow {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    prm: [f32; 3],
    env: [f32; 3],
    /// Half the size of the glow as a multiple of the captain's height.
    scale: f32,
    /// `Flat::textures` ids of `rgb * a` and `(1 - rgb) * a`, once registered.
    ids: Option<[u32; 2]>,
}

pub struct NaviLight {
    base: Glow,
    star: Glow,
}

pub(crate) fn glow(jpc: &Jpc, id: u16, scale: f32) -> Option<Glow> {
    let (shape, tex) = jpc.look(id)?;
    let rgb = |c: [u8; 4]| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0];
    Some(Glow {
        width: tex.image.width,
        height: tex.image.height,
        rgba: tex.image.rgba.clone(),
        prm: rgb(shape.prm),
        env: rgb(shape.env),
        scale,
        ids: None,
    })
}

impl Glow {
    /// `rgb * a` and `(1 - rgb) * a` as opaque-alpha textures, added to `flat` the first time.
    fn textures(&mut self, flat: &mut Flat) -> [u32; 2] {
        if let Some(ids) = self.ids {
            return ids;
        }
        let derive = |inverse: bool| {
            let mut rgba = self.rgba.clone();
            for px in rgba.as_chunks_mut::<4>().0 {
                let a = px[3] as u32;
                for c in &mut px[..3] {
                    let v = if inverse { 255 - *c as u32 } else { *c as u32 };
                    *c = (v * a / 255) as u8;
                }
                px[3] = 255;
            }
            Image { width: self.width, height: self.height, rgba }
        };
        let ids = [flat.add_texture(derive(false), [false, false]), flat.add_texture(derive(true), [false, false])];
        self.ids = Some(ids);
        ids
    }

    pub(crate) fn push(&mut self, flat: &mut Flat, centre: Vec3, half: f32, strength: f32, right: Vec3, up: Vec3) {
        self.push_tinted(flat, centre, half, strength, [1.0; 3], right, up);
    }

    /// As [`Glow::push`], with its colours multiplied by `tint` (a Candypop Bud's effects take the bud's colour).
    pub(crate) fn push_tinted(&mut self, flat: &mut Flat, centre: Vec3, half: f32, strength: f32, tint: [f32; 3], right: Vec3, up: Vec3) {
        let [rgb_a, inv_a] = self.textures(flat);
        let (r, u) = (right * half, up * half);
        let corners = [centre - r + u, centre + r + u, centre + r - u, centre - r - u];
        for (texture, c) in [(inv_a, self.env), (rgb_a, self.prm)] {
            flat.quads.push(FlatQuad {
                corners,
                color: [c[0] * strength * tint[0], c[1] * strength * tint[1], c[2] * strength * tint[2], 1.0],
                texture,
                additive: true,
                uvs: crate::effects::QUAD_UVS,
                refraction: false,
            });
        }
    }
}

impl NaviLight {
    /// Reads Louie's two light resources from the Pikmin 2 cache `dir` (`effects/game.jpc`, `libsms::extract::p2`).
    pub fn load(dir: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(dir.join("effects/game.jpc")).map_err(|e| format!("effects/game.jpc: {e}"))?;
        let jpc = Jpc::parse(&bytes).map_err(|e| format!("effects/game.jpc: {e}"))?;
        let need = |id: u16, scale: f32| glow(&jpc, id, scale).ok_or_else(|| format!("no light resource {id:#x}"));
        // The base is the bigger, softer one (0.6 against the star's 0.45 in the files).
        Ok(NaviLight { base: need(LOUIE_BASE, 0.62)?, star: need(LOUIE_STAR, 0.46)? })
    }

    /// Draws the light over a captain `height` tall whose feet are at `feet`, `time` seconds into the run.
    pub fn draw(&mut self, flat: &mut Flat, feet: Vec3, height: f32, time: f32, right: Vec3, up: Vec3) {
        // Above his head, breathing about once every 1.4 s.
        let breath = 0.5 + 0.5 * (time * std::f32::consts::TAU / 1.4).sin();
        let centre = feet + Vec3::Y * height * 1.12;
        let base = self.base.scale * height * (0.9 + 0.2 * breath);
        let star = self.star.scale * height * (0.95 + 0.1 * (1.0 - breath));
        self.base.push(flat, centre, base, 0.55 + 0.25 * breath, right, up);
        self.star.push(flat, centre, star, 0.8 + 0.2 * (1.0 - breath), right, up);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_draws_four_additive_layers() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libsms/assets/p2");
        if !dir.join("effects/game.jpc").is_file() {
            eprintln!("SKIP: no Pikmin 2 effects");
            return;
        }
        let mut light = NaviLight::load(&dir).expect("load");
        let mut flat = Flat::default();
        light.draw(&mut flat, Vec3::ZERO, 25.0, 0.3, Vec3::X, Vec3::Y);
        assert_eq!(flat.quads.len(), 4);
        assert_eq!(flat.textures.len(), 4);
        assert!(flat.quads.iter().all(|q| q.additive && q.color.iter().all(|c| c.is_finite())));
        // Drawn again, it reuses its textures.
        light.draw(&mut flat, Vec3::ZERO, 25.0, 0.4, Vec3::X, Vec3::Y);
        assert_eq!(flat.textures.len(), 4);
        // Centred above the head.
        let centre = flat.quads[0].corners.iter().map(|c| c.y).sum::<f32>() / 4.0;
        assert!(centre > 25.0, "centred at {centre}");
    }
}
