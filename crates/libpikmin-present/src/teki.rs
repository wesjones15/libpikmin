//! Enemies ("teki") for hosts that bring their own AI: what the disc says about a kind (its
//! parameters, the animations the strategies play and where their frames fall) and the
//! instances a host asks [`crate::View`] to draw.
//!
//! The sim has no enemies of its own (a host's creatures are its own, see `Host`), so a host
//! that spawns a Dwarf or Spotty Bulborb decides what it does and tells the view which
//! animation to show: [`TekiInstance::motion`] is a `.key` AnimMgr index, normally a slot of
//! [`TekiInfo::motions`]. Numbers come from `tekipara/<name>.bin` (`TekiParameters::read`,
//! tekiparameters.cpp), the animation keys from `tekikeys/<name>.key`, and which motion each
//! behaviour plays from the strategies (`TaiChappyStrategy`, taichappy.cpp, and
//! `TaiSwallowStrategy`, taiswallow.cpp), the same table the viewer's enemies use.

use glam::Vec3;
use libpikmin::pellet_config::{self, PelletConfig};
use libpikmin_assets::teki::{TekiParams, teki_paths, tpf, tpi};
use libpikmin_assets::{AnimMgr, AnimSet, Assets, Skeleton};

/// Dwarf Bulborb (`chappy`).
pub const DWARF: u8 = 0;
/// Spotty Bulborb (`swallow`).
pub const SPOTTY: u8 = 1;
pub const KINDS: usize = 2;

/// What a behaviour plays, in the order of [`TekiInfo::motions`].
pub mod slot {
    pub const WAIT: usize = 0;
    pub const TURN: usize = 1;
    pub const CHASE: usize = 2;
    pub const ATTACK: usize = 3;
    pub const GULP: usize = 4;
    pub const FLICK: usize = 5;
    pub const DEAD: usize = 6;
    pub const HURT: usize = 7;
    pub const SLEEP: usize = 8;
    pub const CRY: usize = 9;
    pub const COUNT: usize = 10;
}

/// `TekiMgr::typeNames`.
pub fn name(kind: u8) -> Option<&'static str> {
    match kind {
        DWARF => Some("chappy"),
        SPOTTY => Some("swallow"),
        _ => None,
    }
}

/// The int parameters a kind has (`TPI_COUNT` plus its own).
fn int_count(kind: u8) -> usize {
    if kind == DWARF { 21 } else { 20 }
}

/// The corpse's pellet config id (`TekiMgr::getTypeId`).
fn corpse_id(kind: u8) -> &'static str {
    if kind == DWARF { "tkch" } else { "tksw" }
}

/// AnimMgr entries the strategies use (indices into `tekikeys/<name>.key`), by [`slot`].
fn motion_indices(kind: u8) -> [usize; slot::COUNT] {
    match kind {
        // Wait1, WaitAct1, Move1, Attack, WaitAct2, Flick, Dead, Damage, Wait1, Type2 (cry).
        DWARF => [2, 4, 6, 8, 5, 9, 0, 1, 2, 11],
        // Sleeping plays slot 9 (`TaiMotionAction(.., Flick)` in the sleep state), shaking
        // off is Type1 (`TaiSwallowFlickingAction`).
        _ => [3, 4, 7, 8, 5, 10, 0, 1, 9, 3],
    }
}

/// Frame keys of one motion (`AnimInfo::keys` / `info_keys`), in animation frames (30 a second).
#[derive(Debug, Clone, Copy, Default)]
pub struct TekiMotion {
    pub index: u32,
    pub start: f32,
    pub end: f32,
    /// `KEY_LoopStart` / `KEY_LoopEnd`, or `end` for both when the motion has none.
    pub loop_start: f32,
    pub loop_end: f32,
    /// `KEY_Action0` / `KEY_Action1`, or `end` when absent: the frames where a bite lands and
    /// where the chewing ends.
    pub action0: f32,
    pub action1: f32,
}

/// Everything a host needs about a kind, from the disc. Lengths are in sim units, angles in
/// degrees, speeds in units a second.
#[derive(Debug, Clone)]
pub struct TekiInfo {
    pub kind: u8,
    /// The kind's model in [`crate::View::models`].
    pub model: u32,
    pub life: f32,
    /// The model's scale (`TPF_Scale`); drawn vertices already have it.
    pub scale: f32,
    pub weight: f32,
    pub run_speed: f32,
    /// Radians a second.
    pub turn_speed: f32,
    pub visible_range: f32,
    pub visible_angle: f32,
    pub attackable_range: f32,
    pub attackable_angle: f32,
    pub attack_range: f32,
    pub attack_power: f32,
    pub collision_radius: f32,
    /// `TPF_CorpseSize` / `TPF_CorpseHeight`: the corpse pellet's radius and height.
    pub corpse_size: f32,
    pub corpse_height: f32,
    /// The corpse pellet's config (`pelMgr.bin`).
    pub corpse_min_carriers: u32,
    pub corpse_max_carriers: u32,
    pub corpse_seeds_matching: i32,
    pub corpse_seeds_other: i32,
    /// `TPI_SwallowCount`: how many Pikmin one bite takes.
    pub swallow_count: u32,
    /// Where the body lies in the last frame of the death motion, in the local frame, scaled
    /// (`BTeki::dieSoon` puts the pellet on the body, not at the feet).
    pub corpse_offset: Vec3,
    /// Bind-pose bounds, scaled.
    pub lo: Vec3,
    pub hi: Vec3,
    pub motions: [TekiMotion; slot::COUNT],
}

impl TekiInfo {
    pub fn load(assets: &Assets, kind: u8, model_index: u32, pellets: &[PelletConfig]) -> Result<Self, String> {
        let name = name(kind).ok_or("unknown teki kind")?;
        let [model_path, anm, key, para] = teki_paths(name);
        let e = |e: libpikmin_assets::Error| e.to_string();
        let p = TekiParams::parse_any(&assets.read(&para).map_err(e)?, int_count(kind)).map_err(e)?;
        let mgr = AnimMgr::parse(&assets.read(&key).map_err(e)?).map_err(e)?;
        let motions = motion_indices(kind).map(|index| {
            let Some(i) = mgr.infos.get(index) else { return TekiMotion { index: index as u32, ..Default::default() } };
            let key_at = |n: i32| usize::try_from(n).ok().and_then(|n| i.keys.get(n)).map(|&f| f as f32);
            let of = |ty: u8| i.info_keys.iter().find(|(_, t)| *t == ty).and_then(|&(n, _)| key_at(n));
            let end = i.keys.last().map_or(0.0, |&f| f as f32);
            let (loop_start, loop_end) = of(0).zip(of(1)).unwrap_or((end, end));
            TekiMotion {
                index: index as u32,
                start: i.keys.first().map_or(0.0, |&f| f as f32),
                end,
                loop_start,
                loop_end,
                action0: of(2).unwrap_or(end),
                action1: of(3).unwrap_or(end),
            }
        });
        let scale = p.f(tpf::SCALE);
        let model = assets.model(&model_path).map_err(e)?;
        let set = AnimSet::load(assets, &key, &anm).map_err(e)?;
        let skel = Skeleton::new(&model);
        let bounds = |pose: &libpikmin_assets::Pose| {
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for b in model.batches().into_iter().flatten() {
                for v in &b.vertices {
                    let p = pose.skin(v).0;
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
            (lo, hi)
        };
        let (lo, hi) = bounds(&skel.pose(glam::Mat4::IDENTITY, None));
        let dead = motions[slot::DEAD];
        let corpse_offset = set
            .get(dead.index as usize)
            .map(|(_, anim)| {
                let end = dead.end.min(anim.frames.saturating_sub(1) as f32);
                let (lo, hi) = bounds(&skel.pose(glam::Mat4::IDENTITY, Some((anim, end))));
                ((lo + hi) * 0.5).with_y(0.0) * scale
            })
            .unwrap_or_default();
        let corpse = pellet_config::find(pellets, corpse_id(kind));
        Ok(TekiInfo {
            kind,
            model: model_index,
            life: p.f(tpf::LIFE),
            scale,
            weight: p.f(tpf::WEIGHT),
            run_speed: p.f(tpf::RUN_VELOCITY),
            turn_speed: p.f(tpf::TURN_VELOCITY),
            visible_range: p.f(tpf::VISIBLE_RANGE),
            visible_angle: p.f(tpf::VISIBLE_ANGLE),
            attackable_range: p.f(tpf::ATTACKABLE_RANGE),
            attackable_angle: p.f(tpf::ATTACKABLE_ANGLE),
            attack_range: p.f(tpf::ATTACK_RANGE),
            attack_power: p.f(tpf::ATTACK_POWER),
            collision_radius: p.f(tpf::COLLISION_RADIUS),
            corpse_size: p.f(tpf::CORPSE_SIZE) * scale,
            corpse_height: p.f(tpf::CORPSE_HEIGHT),
            corpse_min_carriers: corpse.map_or(3, |c| c.min_carriers.max(1) as u32),
            corpse_max_carriers: corpse.map_or(6, |c| c.max_carriers.max(1) as u32),
            corpse_seeds_matching: corpse.map_or(4, |c| c.seeds_matching),
            corpse_seeds_other: corpse.map_or(4, |c| c.seeds_other),
            swallow_count: p.i(tpi::SWALLOW_COUNT).max(1) as u32,
            corpse_offset,
            lo: lo * scale,
            hi: hi * scale,
            motions,
        })
    }
}

/// One enemy for the view to draw this frame. The host owns where it is and what it does.
#[derive(Debug, Clone, Copy)]
pub struct TekiInstance {
    /// Stable while the enemy lives; the draw's key and id.
    pub id: u32,
    pub kind: u8,
    pub pos: Vec3,
    /// Radians about +Y (the direction it faces is `(sin, 0, cos)`).
    pub yaw: f32,
    /// A `.key` AnimMgr index, usually [`TekiInfo::motions`]`[slot].index`.
    pub motion: u32,
    /// Animation frame (30 a second).
    pub frame: f32,
    /// Seconds of hit flash left.
    pub flash: f32,
    /// 1 at the disc's size; below 1 shrinks a corpse into the Onion.
    pub scale: f32,
    /// 1 opaque .. 0 gone.
    pub alpha: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> Option<Assets> {
        let assets = Assets { root: Assets::default_dir() };
        assets.root.join("tekis/swallow/swallow.mod").exists().then_some(assets)
    }

    fn pellets(a: &Assets) -> Vec<PelletConfig> {
        libpikmin::pellet_config::parse_pellet_configs(&a.read("parms/pelMgr.bin").unwrap()).unwrap()
    }

    #[test]
    fn both_bulborbs_load_from_the_disc() {
        let Some(a) = assets() else { eprintln!("SKIP: Pikmin assets absent"); return };
        let pellets = pellets(&a);
        for kind in [DWARF, SPOTTY] {
            let t = TekiInfo::load(&a, kind, 0, &pellets).unwrap();
            assert!(t.life > 0.0 && t.scale > 0.0, "{kind}: {t:?}");
            assert!(t.run_speed > 0.0 && t.turn_speed > 0.0);
            assert!(t.corpse_min_carriers >= 1 && t.corpse_max_carriers >= t.corpse_min_carriers);
            assert!(t.hi.y > t.lo.y);
            for m in &t.motions {
                assert!(m.end > m.start, "motion {} has no frames", m.index);
                assert!(m.action0 <= m.end && m.loop_start <= m.loop_end);
            }
        }
    }

    #[test]
    fn the_spotty_is_the_bigger_enemy() {
        let Some(a) = assets() else { eprintln!("SKIP: Pikmin assets absent"); return };
        let pellets = pellets(&a);
        let (dwarf, spotty) = (
            TekiInfo::load(&a, DWARF, 0, &pellets).unwrap(),
            TekiInfo::load(&a, SPOTTY, 0, &pellets).unwrap(),
        );
        assert!(spotty.life > dwarf.life);
        assert!(spotty.corpse_min_carriers > dwarf.corpse_min_carriers);
        assert!(spotty.hi.y - spotty.lo.y > dwarf.hi.y - dwarf.lo.y);
    }
}
