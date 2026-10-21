//! What the disc says about each enemy kind: its parameters, the animations the strategies play and
//! where their frames fall, and its corpse's config.
//!
//! The sim has no enemies of its own (a host's creatures are its own, see `Host`), so a host
//! that spawns a Dwarf or Spotty Bulborb decides what it does and tells the view which
//! animation to show: the instance's `motion` is a `.key` AnimMgr index, normally a slot of
//! [`TekiInfo::motions`]. Numbers come from `tekipara/<name>.bin` (`TekiParameters::read`,
//! tekiparameters.cpp), the animation keys from `tekikeys/<name>.key`, and which motion each
//! behaviour plays from the strategies (`TaiChappyStrategy`, taichappy.cpp, and
//! `TaiSwallowStrategy`, taiswallow.cpp), the same table the viewer's enemies use.

use glam::Vec3;
use libpikmin::pellet_config::{self, PelletConfig};
use libpikmin_assets::teki::{TekiParams, teki_paths, tpf, tpi};
use libpikmin_assets::{AnimMgr, AnimSet, Assets, Model, Skeleton};
use std::sync::Arc;

/// One enemy kind: its files (`TekiMgr::typeNames`), how many int parameters its file has (`TPI_COUNT`
/// plus the kind's own, e.g. `KINOKOPI_COUNT`), its corpse's config id (`TekiMgr::typeIds`), the first
/// `SE_*` of its sound table, and which `.key` motion each behaviour plays ([`slot`] order).
pub struct Kind {
    pub name: &'static str,
    pub ints: usize,
    pub corpse: &'static str,
    pub sound_base: &'static str,
    pub motions: [usize; slot::COUNT],
}

/// Wait1, WaitAct1, Move1, Attack, WaitAct2, Flick, Dead, Damage, Wait1, Type2: `TekiMotion` indexes the
/// `.key` file directly (`PaniTekiAnimator::createMotionTable`).
const STANDARD: [usize; slot::COUNT] = [2, 4, 6, 8, 5, 9, 0, 1, 2, 11];
/// The Spotty Bulborb sleeps on slot 9 and shakes off with Type1 (`TaiSwallowStrategy`).
const BULBORB: [usize; slot::COUNT] = [3, 4, 7, 8, 5, 10, 0, 1, 9, 3];
/// The Armored Cannon Beetle: its walk is slot 3, its turns the L/R turn motions, and Flick2 is its gulp.
const BEETLE: [usize; slot::COUNT] = [2, 10, 3, 8, 12, 9, 0, 1, 2, 2];

/// Dwarf Bulborb (`chappy`).
pub const DWARF: u8 = 0;
/// Spotty Bulborb (`swallow`).
pub const SPOTTY: u8 = 1;
pub const DWARF_ORANGE: u8 = 2;
pub const SPOTTY_ORANGE: u8 = 3;
pub const WOLLYWOG: u8 = 4;
pub const WOLLYWOG_YELLOW: u8 = 5;
pub const CANNON_BEETLE: u8 = 6;
pub const FIERY_BLOWHOG: u8 = 7;
pub const SNITCHBUG: u8 = 8;
pub const SHEARGRUB_F: u8 = 9;
pub const SHEARGRUB_M: u8 = 10;
pub const SHEARWIG: u8 = 11;
pub const WOGPOLE: u8 = 12;
pub const PUFFSTOOL: u8 = 13;
pub const PUFFY_BLOWHOG: u8 = 14;
pub const MAMUTA: u8 = 15;
pub const SMOKY_PROGG: u8 = 16;
pub const WATER_DUMPLE: u8 = 17;
pub const HONEYWISP: u8 = 18;
pub const BREADBUG: u8 = 19;
pub const IWAGON: u8 = 20;

/// Every kind a host can draw, indexed by the constants above.
pub const KIND_TABLE: [Kind; 21] = [
    Kind { name: "chappy", ints: 21, corpse: "tkch", sound_base: "SE_MINIC_WALK", motions: STANDARD },
    Kind { name: "swallow", ints: 20, corpse: "tksw", sound_base: "SE_CHAPPY_WALK", motions: BULBORB },
    Kind { name: "chappb", ints: 21, corpse: "tkcb", sound_base: "SE_MINIC_WALK", motions: STANDARD },
    Kind { name: "swallob", ints: 20, corpse: "tksb", sound_base: "SE_CHAPPY_WALK", motions: BULBORB },
    Kind { name: "frow", ints: 21, corpse: "tkfw", sound_base: "SE_FLOG_WALK", motions: STANDARD },
    Kind { name: "frog", ints: 21, corpse: "tkfr", sound_base: "SE_FLOG_WALK", motions: STANDARD },
    Kind { name: "beatle", ints: 20, corpse: "tkbe", sound_base: "SE_KABUTO_BREATH", motions: BEETLE },
    Kind { name: "tank", ints: 20, corpse: "tkta", sound_base: "SE_TANK_FIRE", motions: STANDARD },
    Kind { name: "napkid", ints: 21, corpse: "tkna", sound_base: "SE_SARAI_HOVER", motions: STANDARD },
    Kind { name: "kabekuiA", ints: 20, corpse: "tkka", sound_base: "SE_WALLEAT_WALK", motions: STANDARD },
    Kind { name: "kabekuiB", ints: 20, corpse: "tkkb", sound_base: "SE_WALLEAT_WALK", motions: STANDARD },
    Kind { name: "kabekuiC", ints: 20, corpse: "tkkc", sound_base: "SE_WALLEAT_WALK", motions: STANDARD },
    Kind { name: "otama", ints: 20, corpse: "tkot", sound_base: "SE_OTAMA_JUMP", motions: STANDARD },
    Kind { name: "kinoko", ints: 22, corpse: "tkki", sound_base: "SE_MUSH_WALK", motions: STANDARD },
    Kind { name: "mar", ints: 23, corpse: "tkma", sound_base: "SE_MAR_FLY", motions: STANDARD },
    Kind { name: "miurin", ints: 23, corpse: "tkmu", sound_base: "SE_MIURIN_WALK", motions: STANDARD },
    Kind { name: "dororo", ints: 21, corpse: "tkdo", sound_base: "SE_DORORO_WALK", motions: STANDARD },
    Kind { name: "namazu", ints: 21, corpse: "tknm", sound_base: "SE_NAMAZU_WALK", motions: STANDARD },
    Kind { name: "qurione", ints: 20, corpse: "tkqu", sound_base: "SE_KURIONE_HIT", motions: STANDARD },
    Kind { name: "collec", ints: 21, corpse: "tkco", sound_base: "SE_COLLEC_BREATH", motions: STANDARD },
    Kind { name: "iwagon", ints: 20, corpse: "tkiw", sound_base: "SE_ROCK_ROLL", motions: STANDARD },
];
pub const KINDS: usize = KIND_TABLE.len();

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
    KIND_TABLE.get(kind as usize).map(|k| k.name)
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

/// One `.key` motion as the animator plays it (`AnimInfo`, panianimator.cpp): the anim key frames a
/// play runs between, the loop/action keys inside it, the sound and effect events, and its speed.
#[derive(Debug, Clone, Default)]
pub struct MotionData {
    /// `AnimInfo::keys`: the anim key frames (a motion starts at the first and ends at the last).
    pub keys: Vec<f32>,
    /// (anim key index, key type): 0 loop start, 1 loop end, 2..5 `KEY_Action0..3`.
    pub info_keys: Vec<(usize, u8)>,
    /// (frame, event 0 sound / 1 effect, value).
    pub events: Vec<(f32, i16, u8)>,
    /// Frames a second (`spd`), and `ANIMFLAG_UseDynamicSpeed` (its speed follows the creature's).
    pub speed: f32,
    pub dynamic_speed: bool,
    /// Frames in the animation itself.
    pub frames: f32,
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
    /// `TPI_CarcassSize`: 0 large, 1 medium, 2 small, 3 none (picks the death and soul effects).
    pub carcass_size: i32,
    /// Where the body lies in the last frame of the death motion, in the local frame, scaled
    /// (`BTeki::dieSoon` puts the pellet on the body, not at the feet).
    pub corpse_offset: Vec3,
    /// Bind-pose bounds, scaled.
    pub lo: Vec3,
    pub hi: Vec3,
    pub motions: [TekiMotion; slot::COUNT],
    /// Every `.key` motion, by AnimMgr index.
    pub motion_data: Vec<MotionData>,
    /// The model and animations, for where its parts are in a motion (`CollInfo::getSphere`).
    pub shape: Arc<Model>,
    pub skeleton: Arc<Skeleton>,
    pub anims: Arc<AnimSet>,
    /// The kind's `TPF_*` / `TPI_*` parameters by index, for the strategies' own.
    pub floats: Vec<f32>,
    pub ints: Vec<i32>,
}

impl TekiInfo {
    pub fn load(assets: &Assets, kind: u8, model_index: u32, pellets: &[PelletConfig]) -> Result<Self, String> {
        let name = name(kind).ok_or("unknown teki kind")?;
        let [model_path, anm, key, para] = teki_paths(name);
        let e = |e: libpikmin_assets::Error| e.to_string();
        let p = TekiParams::parse_any(&assets.read(&para).map_err(e)?, KIND_TABLE[kind as usize].ints).map_err(e)?;
        let mgr = AnimMgr::parse(&assets.read(&key).map_err(e)?).map_err(e)?;
        let motions = KIND_TABLE[kind as usize].motions.map(|index| {
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
        let motion_data = mgr
            .infos
            .iter()
            .enumerate()
            .map(|(i, m)| MotionData {
                keys: m.keys.iter().map(|&k| k as f32).collect(),
                info_keys: m.info_keys.iter().filter_map(|&(k, t)| Some((usize::try_from(k).ok()?, t))).collect(),
                events: m.event_keys.iter().map(|e| (e.frame as f32, e.event, e.value)).collect(),
                speed: if m.speed > 0.0 { m.speed } else { 30.0 },
                dynamic_speed: m.flags & 2 != 0,
                frames: set.get(i).map_or(0.0, |(_, a)| a.frames as f32),
            })
            .collect();
        let corpse = pellet_config::find(pellets, KIND_TABLE[kind as usize].corpse);
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
            carcass_size: p.i(tpi::CARCASS_SIZE),
            corpse_offset,
            lo: lo * scale,
            hi: hi * scale,
            motions,
            motion_data,
            skeleton: Arc::new(skel),
            shape: Arc::new(model),
            anims: Arc::new(set),
            floats: p.floats.clone(),
            ints: p.ints.clone(),
        })
    }
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
        for kind in 0..KINDS as u8 {
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

#[cfg(test)]
mod survey {
    use super::*;
    #[test]
    #[ignore]
    fn print_every_kind() {
        let a = Assets { root: Assets::default_dir() };
        let pellets = libpikmin::pellet_config::parse_pellet_configs(&a.read("parms/pelMgr.bin").unwrap()).unwrap();
        for k in 0..KINDS as u8 {
            match TekiInfo::load(&a, k, 0, &pellets) {
                Ok(t) => println!("{:9} life={:6.0} scale={:.2} run={:4.0} turn={:.1} vis={:4.0}/{:3.0} atk={:4.0} carc={} corpse={}-{} H={:.0}",
                    name(k).unwrap(), t.life, t.scale, t.run_speed, t.turn_speed, t.visible_range, t.visible_angle, t.attack_power, t.carcass_size,
                    t.corpse_min_carriers, t.corpse_max_carriers, t.hi.y - t.lo.y),
                Err(e) => println!("{} FAILED {e}", name(k).unwrap()),
            }
        }
    }
}
