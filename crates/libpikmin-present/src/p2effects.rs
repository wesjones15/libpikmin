//! Exact Pikmin 2 JPA2 emitters on the common Sunshine JParticle runtime.
//! Identities: ParticleID.h and efx/TGasuHiba.h, TKechappy.h, TYaki.h,
//! THanacho.h, THdama.h, TDama.h. Enemy activation is supplied by the caller.
use crate::effects::{Flat, FlatQuad};
use glam::{Mat4, Vec3};
use libsms_assets::jpa2::Jpc;
use libsms_present::particles::Particles;
use std::{collections::HashMap, path::Path};
pub const IDS: &[u16] = &[
    0x74, 0x75, 0x2b4, 0x2b5, 0x283, 0x284, 0x285, 0x294, 0x1e6, 0x1e7, 0x1e8, 0x1e9, 0x241, 0x242,
    0x286, 0x8b, 0x8c, 0x9b, 0x9f, 0x9c, 0x9d, 0x9e, 0x247, 0x8d, 0x8e, 0x8f, 0x90, 0x91, 0x92,
    0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0x27, 0x23, 0x24, 0x25, 0x28, 0x29, 0x2a, 0x2b,
    0x2c, 0xf1, 0xf2, 0xf3, 0xf5, 0xf6,
];
pub struct P2Effects {
    pub particles: Particles,
    textures: HashMap<u32, u32>,
    pub missing: Vec<u16>,
    previous: HashMap<u32, (u8, u32)>,
    external: HashMap<u32, (u16, Mat4, f32, u64)>,
    tick: u64,
    hair: HashMap<u32, (f32, u64)>,
    alphas: Vec<(u32, f32)>,
}
impl P2Effects {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(dir.join("effects/game.jpc")).map_err(|e| e.to_string())?;
        let jpc = Jpc::parse(&bytes).map_err(|e| e.to_string())?;
        let mut particles = Particles::default();
        let mut missing = Vec::new();
        for &id in IDS {
            match jpc.particle(id) {
                Ok(p) => particles.add(&format!("p2_{id:x}"), p),
                Err(_) => missing.push(id),
            }
        }
        Ok(Self {
            particles,
            textures: HashMap::new(),
            missing,
            previous: HashMap::new(),
            external: HashMap::new(),
            tick: 0,
            hair: HashMap::new(),
            alphas: Vec::new(),
        })
    }
    pub fn enemy(
        &mut self,
        t: &crate::teki::TekiInstance,
        p: &crate::p2enemy::P2Teki,
        joints: &[Mat4],
        at: Vec3,
        yaw: f32,
        scale: f32,
    ) {
        use libpikmin_teki::{p2, presentation::*};
        let f = t.presentation;
        if f & VALID == 0 {
            return;
        }
        let root = Mat4::from_translation(at)
            * Mat4::from_rotation_y(yaw)
            * Mat4::from_scale(Vec3::splat(scale));
        let joint = |name: &str| {
            p.body
                .model
                .bmd
                .joints
                .iter()
                .position(|j| j.name == name)
                .and_then(|i| joints.get(i))
                .map(|j| root * *j)
                .unwrap_or(root)
        };
        let body = joint("body");
        let head = joint("head");
        let before = self.previous.insert(t.id, (f, t.motion));
        let ids: &[u16] = if f & GAS != 0 { &[0x2b4, 0x2b5] } else { &[] };
        for &id in ids {
            self.request(t.id * 1024 + id as u32, id, root, scale);
        }
        if f & FIRE != 0 {
            for id in 0x1e6..=0x1e9 {
                self.request(t.id * 1024 + id as u32, id, body, scale);
            }
        }
        if t.kind == p2::HAIRY_BULBORB {
            let h = self.hair.entry(t.id).or_insert((0.0, self.tick));
            let delta = self.tick.saturating_sub(h.1) as f32;
            h.1 = self.tick;
            h.0 = (h.0 + delta * if f & HAIR != 0 { 10.0 } else { -50.0 }).clamp(0.0, 255.0);
            let alpha = h.0 / 255.0;
            if alpha > 0.0 {
                for id in 0x283..=0x285 {
                    let key = t.id * 1024 + id as u32;
                    self.request(key, id, body, scale);
                    self.alphas.push((key, alpha));
                }
            }
        }
        if f & SLEEP != 0 {
            let id = if t.kind == p2::FIERY_BULBLAX {
                0x8c
            } else {
                0x8b
            };
            self.request(t.id * 1024 + id as u32, id, head, scale);
        }
        // Damagumo/BigFoot::startPinchJointEffect: three distinct legs, knee or foot.
        if t.kind == p2::RAGING_LONG_LEGS && f & DEAD == 0 {
            self.request(t.id * 1024 + 0xf2, 0xf2, joint("kosi"), scale);
            if let Some(rig) = &p.legs {
                for i in 0..4 {
                    if let Some(m) = joints.get(rig.joints[i][1]) {
                        self.request(t.id * 1024 + 0x310 + i as u32, 0xf3, root * *m, scale);
                    }
                }
            }
        }
        if f & PINCH != 0 {
            if let Some(rig) = &p.legs {
                for i in 0..if t.kind == p2::MAN_AT_LEGS { 2 } else { 3 } {
                    let (leg, part) = if t.kind == p2::MAN_AT_LEGS {
                        (0, i)
                    } else {
                        ((i + (t.id as usize % 4)) % 4, (t.id as usize >> i) & 1)
                    };
                    if let Some(m) = joints.get(rig.joints[leg][part]) {
                        self.request(t.id * 1024 + 0x300 + i as u32, 0x27, Mat4::from_translation((root * *m).w_axis.truncate()), scale);
                    }
                }
            }
        }
        if let Some((old, motion)) = before {
            if old & FIRE != 0 && f & FIRE == 0 {
                self.emit(if f & DEAD != 0 { 0x241 } else { 0x286 }, body, scale);
            }
            if old & HAIR != 0 && f & HAIR == 0 {
                self.emit(0x294, body, scale);
            }
            if motion != t.motion
                && t.kind == p2::FIERY_BULBLAX
                && p.clip(t.motion).is_some_and(|c| c.contains("flick"))
            {
                self.emit(0x242, body, scale);
            }
        }
    }
    pub fn request(&mut self, key: u32, id: u16, mtx: Mat4, scale: f32) {
        self.particles
            .request_scaled(key, &format!("p2_{id:x}"), mtx, scale);
    }
    pub fn emit(&mut self, id: u16, mtx: Mat4, scale: f32) {
        self.particles.emit(&format!("p2_{id:x}"), mtx, scale);
    }
    pub fn follow(&mut self, key: u32, id: u16, pos: Vec3, dir: Vec3, scale: f32, on: bool) {
        if !on {
            self.external.remove(&key);
            return;
        }
        let rot = glam::Quat::from_rotation_arc(Vec3::Y, dir.normalize_or(Vec3::Y));
        self.external.insert(
            key,
            (
                id,
                Mat4::from_translation(pos) * Mat4::from_quat(rot),
                scale,
                self.tick,
            ),
        );
    }
    pub fn advance(&mut self, tick: u64) {
        self.tick = tick;
        self.external
            .retain(|_, (_, _, _, born)| tick.saturating_sub(*born) < 8);
        for (&key, &(id, mtx, scale, _)) in &self.external {
            self.particles
                .request_scaled(key | 0x80000000, &format!("p2_{id:x}"), mtx, scale);
        }
        self.particles.advance(tick * 4);
        for (slot, alpha) in self.alphas.drain(..) {
            self.particles.set_global_alpha(slot, alpha);
        }
    }
    pub fn draw(&mut self, flat: &mut Flat, right: Vec3, up: Vec3) {
        self.particles.build(right, up);
        for b in &self.particles.batches {
            let texture = *self.textures.entry(b.texture).or_insert_with(|| {
                let t = &self.particles.textures[b.texture as usize];
                flat.add_texture(
                    libpikmin_assets::Image {
                        width: t.width,
                        height: t.height,
                        rgba: t.rgba.clone(),
                    },
                    t.wrap_modes().map(|w| w == 0),
                )
            });
            let t = &self.particles.textures[b.texture as usize];
            if flat.textures[texture as usize].version!=t.version {
                flat.textures[texture as usize].image.rgba.clone_from(&t.rgba);
            }
            flat.textures[texture as usize].sampler = Some(t.wrap_modes());
            flat.textures[texture as usize].version = t.version;
            for q in self.particles.vertices[b.first_vertex as usize..][..b.vertex_count as usize]
                .chunks_exact(4)
            {
                flat.quads.push(FlatQuad {
                    corners: std::array::from_fn(|i| q[i].pos),
                    uvs: std::array::from_fn(|i| [q[i].u, q[i].v]),
                    color: if b.blend == 0 {
                        let c = q[0].color;
                        [c[0] * c[3], c[1] * c[3], c[2] * c[3], 1.0]
                    } else {
                        q[0].color
                    },
                    texture,
                    additive: b.blend == 0,
                    refraction: b.blend == 2,
                });
            }
        }
    }
}
