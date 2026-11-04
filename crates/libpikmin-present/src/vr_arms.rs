//! VR arms for every captain body the view draws (libcommon's `vr_arms`): Olimar's own `nv3Model.mod`, Pikmin 2's
//! Louie and the two Piantas (J3D, through `libsms_present::vr_arms`) and Captain Pikachu (HSD, through
//! `libssbm_assets::vr_arms`'s bone rule). Each is laid out as [`crate::view::View`] pushes it, so a cut's vertex and
//! triangle numbers are the draw's.
//!
//! Olimar has no elbows: joints 7 and 8 (the view's `handR` / `handL` anchors) are each whole arm, one joint off the
//! body (joint 1), so both sides are [`Side::limb`]s.

use std::path::Path;

use glam::{Mat3, Mat4};
use libcommon::vr_arms::{Arms, Mesh, Side};
use libpikmin_assets::model::Skin;

use crate::view::ViewModel;

/// Olimar's arm joints (right, left): the view's `handR` and `handL` anchors.
pub const OLIMAR_ARMS: [usize; 2] = [7, 8];

/// Olimar's body in its bind pose, as [`crate::view::View`] pushes a MOD model: every batch's vertices in order,
/// its index list offset by its first vertex.
pub fn mod_mesh(vm: &ViewModel) -> Mesh {
    let pose = vm.skel.pose(Mat4::IDENTITY, None);
    let mut m = Mesh {
        parents: vm.model.joints.iter().map(|j| usize::try_from(j.parent).ok()).collect(),
        bind: pose.joints.iter().map(|b| b.to_cols_array()).collect(),
        ..Mesh::default()
    };
    for b in &vm.batches {
        let first = m.positions.len() as u32;
        for v in &b.vertices {
            m.positions.push(pose.skin(v).0.into());
            m.weights.push(match v.skin {
                Skin::Joint(j) => vec![(j, 1.0)],
                Skin::Envelope(e) => vm.model.envelopes.get(e as usize).map(|e| e.weights.clone()).unwrap_or_default(),
                Skin::Root => Vec::new(),
            });
        }
        m.triangles.extend(b.indices.chunks_exact(3).map(|t| [first + t[0], first + t[1], first + t[2]]));
    }
    m
}

/// An HSD body in its bind pose over the batches the view draws (`HsdBody::shown`, in that order).
pub fn hsd_mesh(body: &crate::pikachu::HsdBody) -> Mesh {
    let model = &body.model;
    let bind = model.bind_world();
    let mut m = Mesh {
        parents: model.joints.iter().map(|j| j.parent).collect(),
        bind: bind.iter().map(|b| b.to_cols_array()).collect(),
        ..Mesh::default()
    };
    for &(src, _) in &body.shown {
        let first = m.positions.len() as u32;
        for ((p, _), v) in model.skin(src, &bind).into_iter().zip(&model.batches[src].vertices) {
            m.positions.push(p.into());
            m.weights.push(v.weights.iter().map(|&(j, w)| (j as u16, w)).collect());
        }
        m.triangles.extend(model.batches[src].indices.chunks_exact(3).map(|t| [first + t[0], first + t[1], first + t[2]]));
    }
    m
}

/// Where the other captain bodies come from: each cache folder, or None to skip its bodies.
#[derive(Clone, Copy, Default)]
pub struct Sources<'a> {
    /// The Sunshine cache: the male and female Pianta.
    pub sms: Option<&'a Path>,
    /// The Pikmin 2 cache: Louie.
    pub p2: Option<&'a Path>,
    /// The Melee cache: Captain Pikachu.
    pub melee: Option<&'a Path>,
}

/// Every captain body's cut from `cache` (cut and cached first where it isn't there), with whether it was cut now:
/// `pik-olimar`, then `pik-louie`, `pik-pianta-m`, `pik-pianta-f` and `pik-pikachu` where their caches are given. A
/// body that won't load is an Err with its name.
pub fn bodies(olimar: &ViewModel, cache: &Path, src: Sources<'_>) -> Vec<Result<(Mesh, Arms, bool), String>> {
    use libsms_present::vr_arms as j3d;
    let mut out = Vec::new();
    let m = mod_mesh(olimar);
    let (arms, made) = Arms::load_or_cut(cache, "pik-olimar", &m, OLIMAR_ARMS.map(|j| Some(Side::limb(j))));
    out.push(Ok((m, arms, made)));
    if let Some(dir) = src.p2 {
        out.push(match libsms_present::body::Louie::load(dir) {
            Ok(l) => Ok(j3d::load_or_cut(cache, "pik-louie", &l.model.bmd, &l.model.batches, j3d::LOUIE)),
            Err(e) => Err(format!("pik-louie: {e}")),
        });
    }
    if let Some(dir) = src.sms {
        for (female, name) in [(false, "pik-pianta-m"), (true, "pik-pianta-f")] {
            out.push(match libsms_present::body::Pianta::load(dir, female, 0) {
                Ok(p) => Ok(j3d::load_or_cut(cache, name, &p.model.bmd, &p.model.batches, j3d::PIANTA)),
                Err(e) => Err(format!("{name}: {e}")),
            });
        }
    }
    if let Some(dir) = src.melee {
        out.push(pikachu(olimar, cache, dir).map_err(|e| format!("pik-pikachu: {e}")));
    }
    out
}

fn pikachu(olimar: &ViewModel, cache: &Path, dir: &Path) -> Result<(Mesh, Arms, bool), String> {
    let src_bind: Vec<Mat3> = olimar.skel.pose(Mat4::IDENTITY, None).joints.iter()
        .map(|j| Mat3::from_quat(j.to_scale_rotation_translation().1)).collect();
    let height = crate::louie::olimar_height(&olimar.model);
    let mut skin = crate::pikachu::PikachuSkin::load(dir, &src_bind, height)?;
    skin.captain.register(0, &mut Vec::new());
    let bones = libssbm_assets::held::common_bones(dir, &libssbm_assets::fighter::PIKACHU).map_err(|e| e.to_string())?;
    let m = hsd_mesh(&skin.captain);
    let (arms, made) = Arms::load_or_cut(cache, "pik-pikachu", &m, libssbm_assets::vr_arms::sides(&bones));
    Ok((m, arms, made))
}

/// First run: cuts every captain body that has no current cut yet into `cache/vr_arms/`. One note per body cut now
/// or failed; none when every one was already there.
pub fn bake(olimar: &ViewModel, cache: &Path, src: Sources<'_>) -> Vec<String> {
    bodies(olimar, cache, src).into_iter().filter_map(|b| match b {
        Ok((_, arms, true)) => {
            let tris: Vec<usize> = arms.arms.iter().map(|a| a.triangles.len()).collect();
            Some(format!("{}: {} arms ({tris:?} triangles)", arms.name, arms.arms.len()))
        }
        Ok(_) => None,
        Err(e) => Some(e),
    }).collect()
}

/// The low bits of a VR arm draw's key under [`crate::view::kinds::VR_ARM`]: + 0 right, + 1 left (as libsms's
/// `VR_ARM_KEY`, so hosts share ror2-common's `VrCharacter.ArmSide`).
pub const VR_ARM_KEY: u64 = 0x10;

/// A body's cut as the view draws it: the arms, and each side's vertex map over the body's draw.
pub(crate) struct Cut {
    pub(crate) arms: Arms,
    pub(crate) maps: Vec<(u8, Vec<u32>)>,
}

impl Cut {
    pub(crate) fn new(mesh: &Mesh, arms: Arms) -> Cut {
        let maps = arms.arms.iter().map(|a| (a.side, a.vertex_map(&mesh.triangles, mesh.positions.len()))).collect();
        Cut { arms, maps }
    }
}

/// Which captain body a cut is for, as the view holds it.
pub(crate) enum Body<'a> {
    Olimar(&'a ViewModel),
    Louie(&'a libsms_present::view::Model),
    Pianta(&'a libsms_present::view::Model, bool),
    Pikachu(&'a crate::pikachu::HsdBody),
}

/// A body's cut from `cache` for drawing: read, or cut now where it can be (Pikachu's needs the Melee cache's bones,
/// so his is only read; the first-run bake writes it).
pub(crate) fn cut(cache: &Path, body: Body<'_>) -> Option<Cut> {
    use libsms_present::vr_arms as j3d;
    let (mesh, arms) = match body {
        Body::Olimar(vm) => {
            let m = mod_mesh(vm);
            let (arms, _) = Arms::load_or_cut(cache, "pik-olimar", &m, OLIMAR_ARMS.map(|j| Some(Side::limb(j))));
            (m, arms)
        }
        Body::Louie(m) => {
            let (mesh, arms, _) = j3d::load_or_cut(cache, "pik-louie", &m.bmd, &m.batches, j3d::LOUIE);
            (mesh, arms)
        }
        Body::Pianta(m, female) => {
            let name = if female { "pik-pianta-f" } else { "pik-pianta-m" };
            let (mesh, arms, _) = j3d::load_or_cut(cache, name, &m.bmd, &m.batches, j3d::PIANTA);
            (mesh, arms)
        }
        Body::Pikachu(b) => {
            let m = hsd_mesh(b);
            let arms = Arms::load(cache, "pik-pikachu")?;
            (m, arms)
        }
    };
    (!arms.arms.is_empty() && arms.fits(mesh.positions.len(), mesh.triangles.len())).then(|| Cut::new(&mesh, arms))
}
