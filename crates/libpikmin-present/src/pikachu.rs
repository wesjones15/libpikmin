//! Olimar's Pikachu alt-skin: Melee's Pikachu (`PlPkNr.dat`) as the captain and, for each Pikmin in his
//! squad, an Ice Climber (Popo or Nana in another colour), all from the user's own Melee disc, shrunk to
//! fit and moving with the Pikmin's own motions.
//!
//! The Pikmin share Olimar's 12-joint skeleton (Pikmin 1 plays one motion table on both), so one retarget
//! serves the captain and the squad: each of Olimar's joints that has a Melee counterpart hands over its
//! world rotation change from the bind pose (host adaptation, like `sms::Retarget`), through the common
//! body parts the game uses to find a bone in any fighter: PlCo.dat's FighterPartsTable and
//! `ftParts_GetBoneIndex` (ft/ftparts.c; `Fighter_Part` in ft/forward.h). Olimar and the Pikmin have one
//! joint per limb, so the Melee shoulders, forearms and hands follow their upper arm and the shins and feet
//! their thigh. The lowest skinned vertex is put on the ground each frame.

use std::path::Path;

use glam::{EulerRot, Mat3, Mat4, Quat, Vec2, Vec3};
use libpikmin_assets::model::{Batch, BatchVertex, Cull, Skin};
use libssbm_assets::dat::Dat;
use libssbm_assets::fighter::Archive;
use libssbm_assets::model as hsd;

use crate::tev;

/// Skin id in the host's order (`pik_view_set_captain_skin`).
pub const SKIN_PIKACHU: u8 = 3;

/// (Olimar or Pikmin joint, common part) with the part ids of `Fighter_Part`: 4 HipN, 5 WaistN, 34 HeadN,
/// 37 and 19 the right and left upper arms, 11 and 6 the right and left thighs. Olimar's right is -x.
const PARTS: &[(usize, usize)] = &[(0, 4), (1, 5), (2, 34), (7, 37), (8, 19), (10, 11), (11, 6)];

/// Melee's 0.70 diffuse reads dark under RoR2's lighting; the Melee and Luigi hosts lift it by this much.
const BRIGHTNESS: f32 = 1.5;

/// One Melee body ready to draw, with the retarget from Olimar's rig.
pub struct HsdBody {
    pub model: hsd::Model,
    /// (Olimar joint, model joint).
    pairs: Vec<(usize, usize)>,
    src_bind: Vec<Mat3>,
    dst_bind: Vec<Mat3>,
    /// The model's index in the view's model table, and where its textures start.
    pub index: u32,
    /// Per drawn batch: its source batch, and its texture id in the view's list (or -1).
    pub shown: Vec<(usize, i32)>,
    /// Sim units per Melee unit.
    pub scale: f32,
    /// Bind height in Melee units.
    height: f32,
}

fn local_rot(j: &hsd::Joint) -> Mat3 {
    Mat3::from_quat(Quat::from_euler(EulerRot::ZYX, j.rotation.z, j.rotation.y, j.rotation.x))
}

impl HsdBody {
    /// Loads `costume` of the fighter in `data` (`PlPp.dat`...), mapping Olimar's joints through kind
    /// `kind`'s row of the common parts table. `src_bind` is Olimar's bind world rotation per joint.
    pub fn load(dir: &Path, costume: &str, data: Archive<'_>, kind: usize, src_bind: &[Mat3]) -> Result<Self, String> {
        let model = hsd::Model::load_costume(dir, costume, data, 0, None).map_err(|e| format!("{costume}: {e}"))?;
        let common = std::fs::read(dir.join("PlCo.dat")).map_err(|e| format!("PlCo.dat: {e}"))?;
        let d = Dat::parse(&common).map_err(|e| e.to_string())?;
        let tables = d.required_ptr(d.root("ftLoadCommonData").map_err(|e| e.to_string())? + 0x10).map_err(|e| e.to_string())?;
        let row = d.required_ptr(tables + kind * 4).map_err(|e| e.to_string())?;
        let map = d.required_ptr(row + 4).map_err(|e| e.to_string())?;
        let map = d.bytes(map, 53).map_err(|e| e.to_string())?;
        let mut pairs = Vec::new();
        for &(joint, part) in PARTS {
            let target = *map.get(part).ok_or("common part outside the table")? as usize;
            if target != 255 && target < model.joints.len() && joint < src_bind.len() {
                pairs.push((joint, target));
            }
        }
        if pairs.len() < 5 {
            return Err(format!("{costume}: only {} of Olimar's joints have a counterpart", pairs.len()));
        }
        let mut dst_bind: Vec<Mat3> = Vec::with_capacity(model.joints.len());
        for j in &model.joints {
            let r = local_rot(j);
            dst_bind.push(match j.parent {
                Some(p) => dst_bind[p] * r,
                None => r,
            });
        }
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for b in &model.batches {
            for v in &b.vertices {
                lo = lo.min(v.position.y);
                hi = hi.max(v.position.y);
            }
        }
        let height = (hi - lo).max(1e-3);
        Ok(HsdBody { model, pairs, src_bind: src_bind.to_vec(), dst_bind, index: 0, shown: Vec::new(), scale: 1.0, height })
    }

    /// Stand `units` tall.
    pub fn fit(&mut self, units: f32) {
        if units.is_finite() && units > 0.01 {
            self.scale = units / self.height;
        }
    }

    /// The model's joints for Olimar's (or a Pikmin's) pose `src`: mapped joints take the source's world
    /// rotation change, every other joint keeps its bind rotation under its parent.
    pub fn pose(&self, src: &[Mat4]) -> Vec<Mat4> {
        let n = self.model.joints.len();
        let mut mapped: Vec<Option<Mat3>> = vec![None; n];
        for &(s, m) in &self.pairs {
            let Some(w) = src.get(s) else { continue };
            let rot = Mat3::from_quat(w.to_scale_rotation_translation().1);
            mapped[m] = Some(rot * self.src_bind[s].transpose() * self.dst_bind[m]);
        }
        let mut world = vec![Mat3::IDENTITY; n];
        for i in 0..n {
            let parent = self.model.joints[i].parent;
            world[i] = match mapped[i] {
                Some(r) => r,
                None => match parent {
                    Some(p) => world[p] * local_rot(&self.model.joints[i]),
                    None => local_rot(&self.model.joints[i]),
                },
            };
        }
        let mut out = vec![Mat4::IDENTITY; n];
        for i in 0..n {
            let j = &self.model.joints[i];
            let parent = j.parent;
            let q = match parent {
                Some(p) => Quat::from_mat3(&(world[p].transpose() * world[i])),
                None => Quat::from_mat3(&world[i]),
            };
            let local = Mat4::from_scale_rotation_translation(j.scale, q, j.translation);
            out[i] = match parent {
                Some(p) => out[p] * local,
                None => local,
            };
        }
        out
    }

    /// Skins every drawn batch for `world` (from [`Self::pose`], `root` over it) into `positions` and
    /// `normals` (sim units), returning the lowest y written. It is `Model::skin` with each joint's
    /// matrices worked out once per batch instead of once per vertex, which a swarm of these needs.
    pub fn skin_into(&self, world: &[Mat4], root: Mat4, positions: &mut Vec<Vec3>, normals: &mut Vec<Vec3>) -> f32 {
        let scale = Mat4::from_scale(Vec3::splat(self.scale));
        let world: Vec<Mat4> = world.iter().map(|w| root * scale * *w).collect();
        let m = &self.model;
        let mut low = f32::INFINITY;
        let n = m.joints.len();
        for &(src, _) in &self.shown {
            let batch = &m.batches[src];
            // displayfunc.c::_HSD_mkEnvelopeModelNodeMtx, as `Model::skin`.
            let owner = batch.owner;
            let mut skeleton = Some(owner);
            while let Some(j) = skeleton {
                if m.joints[j].flags & 3 != 0 { break; }
                skeleton = m.joints[j].parent;
            }
            let right = if m.joints[owner].flags & 2 != 0 {
                Mat4::IDENTITY
            } else if let Some(j) = skeleton {
                if j == owner {
                    m.joints[j].envelope.unwrap_or(m.joints[j].inverse).inverse()
                } else if m.joints[j].flags & 2 != 0 {
                    world[j].inverse() * world[owner]
                } else {
                    (world[j] * m.joints[j].envelope.unwrap_or(m.joints[j].inverse)).inverse() * world[owner]
                }
            } else {
                Mat4::IDENTITY
            };
            let instance = batch.instance.map(|(i, t)| world[i] * world[t].inverse());
            let instance = instance.map(|mx| (mx, mx.inverse().transpose()));
            // Per joint: the matrix and its inverse transpose, plain and envelope.
            let mut plain: Vec<Option<(Mat4, Mat4)>> = vec![None; n];
            let mut env: Vec<Option<(Mat4, Mat4)>> = vec![None; n];
            for v in &batch.vertices {
                let (mut p, mut nrm) = (Vec3::ZERO, Vec3::ZERO);
                for &(j, w) in &v.weights {
                    let cache = if v.envelope { &mut env } else { &mut plain };
                    let (mx, it) = *cache[j].get_or_insert_with(|| {
                        let mx = if v.envelope {
                            world[j] * m.joints[j].envelope.unwrap_or(m.joints[j].inverse) * right
                        } else {
                            world[j] * m.joints[j].inverse
                        };
                        (mx, mx.inverse().transpose())
                    });
                    p += mx.transform_point3(v.position) * w;
                    nrm += it.transform_vector3(v.normal) * w;
                }
                if let Some((mx, it)) = instance {
                    p = mx.transform_point3(p);
                    nrm = it.transform_vector3(nrm);
                }
                low = low.min(p.y);
                positions.push(p);
                normals.push(nrm.normalize_or_zero());
            }
        }
        low
    }

    /// The batches for the host's model table (vertices in drawn-batch order, so posed ones line up).
    pub fn table_batches(&self) -> Vec<Batch> {
        self.shown.iter().map(|&(src, _)| {
            let b = &self.model.batches[src];
            Batch {
                material: 0,
                cull: Cull::None,
                vertices: b.vertices.iter().map(|v| BatchVertex {
                    position: Vec3::ZERO,
                    normal: Vec3::Y,
                    uv: Vec2::new(v.uv.x, v.uv.y),
                    color: [255; 4],
                    skin: Skin::Root,
                }).collect(),
                indices: b.indices.clone(),
            }
        }).collect()
    }

    /// Bind-pose bounds in sim units.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for &(src, _) in &self.shown {
            for v in &self.model.batches[src].vertices {
                lo = lo.min(v.position * self.scale);
                hi = hi.max(v.position * self.scale);
            }
        }
        if lo.x > hi.x { (Vec3::ZERO, Vec3::ZERO) } else { (lo, hi) }
    }

    /// Picks the batches to draw (not empty, not a hidden alternative part: `ftParts_8007487C`'s choices) and
    /// bakes their images into `textures` from the list's end.
    pub fn register(&mut self, index: u32, textures: &mut Vec<tev::Baked>) {
        self.index = index;
        self.shown.clear();
        for (src, b) in self.model.batches.iter().enumerate() {
            if b.indices.is_empty() || !self.model.normal_visible.get(b.dobj).copied().unwrap_or(true) {
                continue;
            }
            let id = match &b.texture {
                Some(t) => {
                    textures.push(tev::Baked { width: t.width as u32, height: t.height as u32, wrap: [t.wrap_s, t.wrap_t], rgba: t.rgba.clone(), env: None });
                    textures.len() as i32 - 1
                }
                None => -1,
            };
            self.shown.push((src, id));
        }
    }

    /// The colour of a drawn batch (the material's, lifted).
    pub fn color(&self, shown: usize) -> [f32; 4] {
        let c = self.model.batches[self.shown[shown].0].color;
        [c[0] * BRIGHTNESS, c[1] * BRIGHTNESS, c[2] * BRIGHTNESS, c[3]]
    }

    /// Blend class and lit bits for the model table.
    pub fn blend_lit(&self) -> (Vec<u8>, Vec<u8>) {
        let b = self.shown.iter().map(|&(s, _)| self.model.batches[s].blend).collect();
        let l = self.shown.iter().map(|&(s, _)| self.model.batches[s].lit).collect();
        (b, l)
    }
}

/// Pikachu as the captain and an Ice Climber for each Pikmin colour (Blue, Red, Yellow).
pub struct PikachuSkin {
    pub captain: HsdBody,
    pub climbers: [HsdBody; 3],
    olimar_height: f32,
    captain_ratio: f32,
    climber_ratio: f32,
}

/// Popo and Nana's costume for each Pikmin colour: Blue is Popo's blue, Red his red, Yellow Nana's yellow
/// (`ftpopo.c`, `ftnana.c` costume lists).
const CLIMBERS: [(&str, &str, &str, usize); 3] = [
    ("PlPpNr.dat", "PlPp.dat", "ftDataPopo", 10),
    ("PlPpRe.dat", "PlPp.dat", "ftDataPopo", 10),
    ("PlNnYe.dat", "PlNn.dat", "ftDataNana", 11),
];

impl PikachuSkin {
    pub fn load(dir: &Path, src_bind: &[Mat3], olimar_height: f32) -> Result<Self, String> {
        let captain = HsdBody::load(dir, "PlPkNr.dat", Archive { file: "PlPk.dat", root: "ftDataPikachu" }, 12, src_bind)?;
        let mut load = |i: usize| -> Result<HsdBody, String> {
            let (costume, data, root, kind) = CLIMBERS[i];
            HsdBody::load(dir, costume, Archive { file: data, root }, kind, src_bind)
        };
        let climbers = [load(0)?, load(1)?, load(2)?];
        let mut skin = PikachuSkin { captain, climbers, olimar_height, captain_ratio: 1.0, climber_ratio: 0.42 };
        skin.refit();
        Ok(skin)
    }

    fn refit(&mut self) {
        self.captain.fit(self.olimar_height * self.captain_ratio);
        for c in &mut self.climbers {
            c.fit(self.olimar_height * self.climber_ratio);
        }
    }

    /// Pikachu stands `captain` times as tall as Olimar, an Ice Climber `climber` times.
    pub fn set_ratios(&mut self, captain: f32, climber: f32) {
        if captain.is_finite() && captain > 0.1 { self.captain_ratio = captain; }
        if climber.is_finite() && climber > 0.05 { self.climber_ratio = climber; }
        self.refit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libpikmin_assets::{Assets, Skeleton, paths};

    fn melee() -> Option<std::path::PathBuf> {
        let dir = std::env::var_os("SSBM_ASSETS").map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libssbm/assets"));
        ["PlCo.dat", "PlPk.dat", "PlPkNr.dat", "PlPp.dat", "PlPpNr.dat", "PlPpRe.dat", "PlNn.dat", "PlNnYe.dat"]
            .iter().all(|f| dir.join(f).is_file()).then_some(dir)
    }

    /// Olimar's bind world rotations, from the disc.
    fn olimar_bind(a: &Assets) -> Vec<Mat3> {
        let m = a.model(paths::OLIMAR).unwrap();
        Skeleton::new(&m).pose(Mat4::IDENTITY, None).joints.iter()
            .map(|j| Mat3::from_quat(j.to_scale_rotation_translation().1)).collect()
    }

    #[test]
    fn pikachu_and_the_climbers_follow_olimars_pose() {
        let Some(dir) = melee() else { eprintln!("SKIP: no Melee cache with the Ice Climbers"); return };
        let Some(a) = Assets::discover(None) else { eprintln!("SKIP: no Pikmin assets"); return };
        let src_bind = olimar_bind(&a);
        let mut skin = PikachuSkin::load(&dir, &src_bind, 25.1).expect("load");
        let m = a.model(paths::OLIMAR).unwrap();
        let sk = Skeleton::new(&m);
        let bind = sk.pose(Mat4::IDENTITY, None);
        for i in 0..4 {
            let (body, want) = if i == 0 { (&mut skin.captain, 25.1f32) } else { (&mut skin.climbers[i - 1], 25.1 * 0.42) };
            let mut tex = Vec::new();
            body.register(0, &mut tex);
            assert!(!body.shown.is_empty());
            let (mut p, mut n) = (Vec::new(), Vec::new());
            let low = body.skin_into(&body.pose(&bind.joints), Mat4::IDENTITY, &mut p, &mut n);
            assert!(low.is_finite());
            let high = p.iter().fold(f32::MIN, |h, v| h.max(v.y)) - low;
            assert!((high - want).abs() < want * 0.2, "stands {high}, wanted about {want}");
            assert!(p.iter().all(|v| v.is_finite()) && n.iter().all(|v| v.is_finite()));
        }
    }
}
