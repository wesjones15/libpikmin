//! Pikmin 1's particle effects (Yamashita's `zen::particleGenerator`, particleGenerator.cpp):
//! the `.pcr` generator files in `effects/pcr/` and the generator that runs them.
//!
//! A `.pcr` is a 4-byte version, the offset of its emitter block (`pmSetDDF`) at 4, a billboard
//! flag at 6, then either the colour animation at 8 (billboards) or a length scale, pivot and
//! the colour animation at 16 (particles oriented along their velocity). Everything is
//! big-endian. Particles live in "frames": the game steps generators by `30 * frame time`
//! (`particleManager::update`), so velocities are per 1/30 s.
//!
//! The generator is renderer-agnostic: [`Generator::quads`] gives each particle's corners,
//! texture coordinates and TEV colours; how to combine them is [`Pcr::color_mode`]
//! (`DGXGraphics::setBlendMode`) and how to blend is [`Pcr::blend_factor`] / [`Pcr::z_mode`].

use std::f32::consts::TAU;
use std::sync::Arc;

use glam::{Mat3, Quat, Vec3};

use crate::Error;
use crate::reader::{Reader, ReaderExt};

/// An `EffectMgr` entry: a generator and its texture (and child-particle texture).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectDef {
    pub pcr: &'static str,
    pub tex: &'static str,
    pub child_tex: Option<&'static str>,
}

/// Entries of `EffectMgr`'s resource table (effectMgr.cpp) the hosts use.
pub mod effects {
    use super::EffectDef;

    /// EffectMgr ids 50/51; PikiNukareState::procAnimMsg(KEY_Action0),
    /// plugPikiKando/pikiState.cpp; resource table in plugPikiYamashita/effectMgr.cpp.
    pub const PLUCK_DIRT: [EffectDef; 2] = [
        one("effects/pcr/sd_rakk1.pcr", "effects/tex/hokori4.bti"),
        one("effects/pcr/sd_rakk2.pcr", "effects/tex/iwa_1_32.bti"),
    ];

    /// `EFF_Navi_Whistle1..3`: the whistle's beam from the antenna to the cursor
    /// (`NaviWhistle::emit`, NaviWhistle.h).
    pub const NAVI_WHISTLE: [EffectDef; 3] = [
        EffectDef { pcr: "effects/pcr/nav_fue1.pcr", tex: "effects/tex/ps_ball.bti", child_tex: None },
        EffectDef { pcr: "effects/pcr/nav_fue2.pcr", tex: "effects/tex/fuchibal.bti", child_tex: None },
        EffectDef {
            pcr: "effects/pcr/nav_fue3.pcr",
            tex: "effects/tex/ps_glow.bti",
            child_tex: Some("effects/tex/ps_ball.bti"),
        },
    ];
    /// `EFF_Piki_DeadSoul`: the ghost that rises where a Pikmin died (`Piki::doKill`).
    pub const PIKI_DEAD_SOUL: EffectDef = EffectDef {
        pcr: "effects/pcr/pk_dead.pcr",
        tex: "effects/tex/deadpiki.bti",
        child_tex: Some("effects/tex/deadpiki.bti"),
    };
    /// `EFF_Navi_WhistleCursor`: one of the 24 sparkles around the whistle circle
    /// (`NaviFue::emit`).
    pub const WHISTLE_CIRCLE: EffectDef =
        EffectDef { pcr: "effects/pcr/g_curs00.pcr", tex: "effects/tex/g_cursor.bti", child_tex: None };

    const fn one(pcr: &'static str, tex: &'static str) -> EffectDef {
        EffectDef { pcr, tex, child_tex: None }
    }
    /// `EFF_Rocket_NaviRecover`: the star burst when Olimar is healed at the ship
    /// (`NaviUfoState::exec`).
    pub const NAVI_RECOVER: EffectDef = one("effects/pcr/ore_rec.pcr", "effects/tex/star8_i.bti");
    /// `EFF_Rocket_NaviNormalRings`: the rings on the ship's goal spot (`UfoItem::setSpotActive`).
    pub const NAVI_RINGS: EffectDef = one("effects/pcr/ore_nml.pcr", "effects/tex/fuchibal.bti");
    /// `EFF_Rocket_NaviSparkle`: the glowing sparkles at the ship's goal spot (`UfoItem`).
    pub const NAVI_SPARKLE: EffectDef = one("effects/pcr/ore_kira.pcr", "effects/tex/ps_glow.bti");
    /// `EFF_Rocket_C`: sparkles over the ship when it powers up (`UfoItem::startLevelFlag`).
    pub const ROCKET_C: EffectDef = one("effects/pcr/rkt_c.pcr", "effects/tex/star4_i.bti");
    /// `EFF_Rocket_Complete1/2`: the same at the last level.
    pub const ROCKET_COMPLETE: [EffectDef; 2] = [
        one("effects/pcr/r_cpl01.pcr", "effects/tex/ov_cloud.bti"),
        one("effects/pcr/r_cpl02.pcr", "effects/tex/star4_i.bti"),
    ];
    /// `EFF_Rocket_Gep`: a puff where a part goes in (`UfoItem::finishSuck`).
    pub const ROCKET_GEP: EffectDef = one("effects/pcr/rct_gep.pcr", "effects/tex/hokori4.bti");
    /// `EFF_UfoPart_ASN01/02`: sparkles as a part fits itself on (its Appear motion's keys).
    pub const UFO_PART_ASSIGN: [EffectDef; 2] = [
        one("effects/pcr/asn01.pcr", "effects/tex/star4_i.bti"),
        one("effects/pcr/asn02.pcr", "effects/tex/star4_i.bti"),
    ];
    /// `EFF_Rocket_Nke1/2`: debris and dust as the broken ship shudders (`kiso_t`'s keys).
    pub const ROCKET_NKE: [EffectDef; 2] = [
        one("effects/pcr/rct_nke1.pcr", "effects/tex/iwa_1_32.bti"),
        one("effects/pcr/rct_nke2.pcr", "effects/tex/hokori4.bti"),
    ];
    /// `EFF_Piki_GrowUp1`: sparkles on a sprout two seconds before it grows a stage
    /// (`PikiHeadAI::WaitExec`).
    pub const PIKI_GROW_UP1: EffectDef = one("effects/pcr/growup00.pcr", "effects/tex/growup00.bti");
    /// `EFF_Piki_GrowUp2`: the burst as it does (`PikiHeadAI::GrowupedExec`).
    pub const PIKI_GROW_UP2: EffectDef = one("effects/pcr/growup01.pcr", "effects/tex/growup01.bti");
    /// `EFF_Piki_DeflowerPetals` and `EFF_Piki_DeflowerSpores`: a Pikmin getting up from a
    /// knockdown a stage lower (`PikiFlickState`).
    pub const PIKI_DEFLOWER: [EffectDef; 2] = [
        one("effects/pcr/chiru.pcr", "effects/tex/hanabira.bti"),
        one("effects/pcr/chiru_s.pcr", "effects/tex/ice_smok.bti"),
    ];
    /// `EFF_CloudOfDust_1`
    pub const CLOUD_OF_DUST_1: EffectDef = one("effects/pcr/kch_kem1.pcr", "effects/tex/hokori4.bti");

    const fn jet(fb: &'static str, fa: &'static str) -> [EffectDef; 2] {
        [one(fb, "effects/tex/kaen_2.bti"), one(fa, "effects/tex/kaen_2.bti")]
    }
    /// `EFF_Rocket_NJ*FB` / `NJ*FA`: the engines' flames (ufoItem.cpp `effects[8][4]`, the two
    /// columns `setJetEffect` uses without smoke). Rows 0-3 are for ship levels 2-5; rows
    /// 4-7 are one per engine below level 2.
    pub const ROCKET_JETS: [[EffectDef; 2]; 8] = [
        jet("effects/pcr/r_nj2fb.pcr", "effects/pcr/r_nj2fa.pcr"),
        jet("effects/pcr/r_nj2fb.pcr", "effects/pcr/r_nj2fa.pcr"),
        jet("effects/pcr/r_nj3fb.pcr", "effects/pcr/r_nj3fa.pcr"),
        jet("effects/pcr/r_nj3fb.pcr", "effects/pcr/r_nj3fa.pcr"),
        jet("effects/pcr/r_nj1fb.pcr", "effects/pcr/r_nj1fa.pcr"),
        jet("effects/pcr/r_nj1fb2.pcr", "effects/pcr/r_nj1fa2.pcr"),
        jet("effects/pcr/r_nj1fb3.pcr", "effects/pcr/r_nj1fa3.pcr"),
        jet("effects/pcr/r_nj1fb4.pcr", "effects/pcr/r_nj1fa4.pcr"),
    ];
    /// `EFF_Rocket_JetG01-03`: the blast on the ground as the ship lifts off.
    pub const ROCKET_JET_G: [EffectDef; 3] = [
        one("effects/pcr/jetg01.pcr", "effects/tex/ov_cloud.bti"),
        one("effects/pcr/jetg02.pcr", "effects/tex/ov_cloud.bti"),
        one("effects/pcr/jetg03.pcr", "effects/tex/ov_cloud.bti"),
    ];
    /// `EFF_Rocket_Fkm1`
    pub const ROCKET_FKM1: EffectDef = one("effects/pcr/rct_fkm1.pcr", "effects/tex/hokori4.bti");
    /// `EFF_Rocket_Bstg`, `Bst1db`, `Bst1da`: the finished ship's ground blast.
    pub const ROCKET_BOOST_GROUND: [EffectDef; 3] = [
        one("effects/pcr/bstg.pcr", "effects/tex/ov_cloud.bti"),
        one("effects/pcr/r_bst1db.pcr", "effects/tex/hokori4.bti"),
        one("effects/pcr/r_bst1da.pcr", "effects/tex/ov_cloud.bti"),
    ];
    /// `EFF_Rocket_Bst1fb`, `Bst1fa`, `Bst1cb`: the finished ship's booster flame at 'gcen'.
    pub const ROCKET_BOOSTER: [EffectDef; 3] = [
        one("effects/pcr/r_bst1fb.pcr", "effects/tex/kaen_2.bti"),
        one("effects/pcr/r_bst1fa.pcr", "effects/tex/kaen_2.bti"),
        one("effects/pcr/r_bst1cb.pcr", "effects/tex/kaen_2.bti"),
    ];
    /// `EFF_Rocket_PCA1/2`: a side thruster puff (`UfoItem::setPca1Effect`).
    pub const ROCKET_PCA: [EffectDef; 2] = [
        one("effects/pcr/pca1.pcr", "effects/tex/p_cloud05.bti"),
        one("effects/pcr/pca2.pcr", "effects/tex/p_hsbs3.bti"),
    ];
    /// `EFF_Rocket_Bm1o` / `Bm2o`: the ship's light beam from its hatch to the goal spot
    /// (`UfoItem::startConeEffect`, `WhistleTemplate` in uteffect.h; its particles ride the line like the
    /// whistle's).
    pub const ROCKET_BEAM: [EffectDef; 2] = [
        one("effects/pcr/rct_bm1o.pcr", "effects/tex/ring_i4.bti"),
        one("effects/pcr/rct_bm2o.pcr", "effects/tex/ps_ball.bti"),
    ];
    /// `EFF_Rocket_Land`: dust as the ship is back (`UfoItem::startAI`).
    pub const ROCKET_LAND: EffectDef = one("effects/pcr/rkt_land.pcr", "effects/tex/p_cloud05.bti");
    /// `EFF_Kafun_NG`: sparkles trailing from a flying Onion (`GoalItem::setFlowEffect`).
    pub const KAFUN_NG: EffectDef = one("effects/pcr/kafun_ng.pcr", "effects/tex/star4_i.bti");
    /// `EFF_Rocket_TakeS`: dust as an Onion lifts off.
    pub const ROCKET_TAKE_S: EffectDef = one("effects/pcr/take_s.pcr", "effects/tex/hokori4.bti");
    /// Wrecked Dolphin trouble emitters from `UfoItem::setTroubleEffect`.
    pub const ROCKET_TROUBLE: [EffectDef; 4] = [
        one("effects/pcr/rkt_mk_b.pcr", "effects/tex/hokori4.bti"),
        one("effects/pcr/rkt_mk_s.pcr", "effects/tex/hokori4.bti"),
        one("effects/pcr/rkt_hiba.pcr", "effects/tex/ps_ball.bti"),
        one("effects/pcr/rkt_biri.pcr", "effects/tex/biriri.bti"),
    ];
    /// `EFF_Rocket_LandS`: dust as an Onion touches down.
    pub const ROCKET_LAND_S: EffectDef = one("effects/pcr/land_s.pcr", "effects/tex/hokori4.bti");
    /// `EFF_Rocket_SmokeD`: smoke at an Onion's feet as it lands.
    pub const ROCKET_SMOKE_D: EffectDef = one("effects/pcr/kemuri_d.pcr", "effects/tex/kaen_ok.bti");
    /// `EFF_Onyon_Suck`: light drawn up into an Onion as a pellet arrives (`GoalEffect`,
    /// cast by `PelletGoalState::init`).
    pub const ONYON_SUCK: EffectDef = one("effects/pcr/goal_sui.pcr", "effects/tex/ps_glow.bti");
    /// GoalItem::playEffect motion 12: sparkle at the top and a smoke puff by a leg.
    pub const ONYON_SPARKLES: EffectDef = one("effects/pcr/ony_kira.pcr", "effects/tex/star4_i.bti");
    pub const ONYON_PUFF: EffectDef = one("effects/pcr/ony_paf.pcr", "effects/tex/hokori4.bti");
    /// GoalItem::playEffect, startup motions 3..5.
    pub const ONYON_BOOT_FLOW: [EffectDef; 2] = [
        one("effects/pcr/kafun_b.pcr", "effects/tex/star4_i.bti"),
        one("effects/pcr/kafun_bs.pcr", "effects/tex/hokori4.bti"),
    ];
    pub const ONYON_BOOT_DUST: [EffectDef; 2] = [
        one("effects/pcr/ony_st1.pcr", "effects/tex/hokori4.bti"),
        one("effects/pcr/ony_st2.pcr", "effects/tex/hokori4.bti"),
    ];
    pub const ONYON_BOOT_WATER: [EffectDef; 3] = [
        one("effects/pcr/ony_shib.pcr", "effects/tex/mizutama.bti"),
        one("effects/pcr/ony_ham1.pcr", "effects/tex/hamon.bti"),
        one("effects/pcr/ony_ham2.pcr", "effects/tex/hamon.bti"),
    ];
    pub const ONYON_BOOT_BUBBLES: EffectDef = one("effects/pcr/ony_shbs.pcr", "effects/tex/mizutama.bti");
    pub const SMOKE_RING_M: EffectDef = one("effects/pcr/kemuri_m.pcr", "effects/tex/kaen_ok.bti");
    /// `EFF_Onyon_BeaconRing{Blue,Red,Yellow}`, by `PikiColor`: rings under an Onion while a
    /// captain stands at it (`GoalItem::setSpotActive`).
    pub const ONYON_BEACON_RING: [EffectDef; 3] = [
        one("effects/pcr/ony_ac_b.pcr", "effects/tex/ring_i4.bti"),
        one("effects/pcr/ony_ac_r.pcr", "effects/tex/ring_i4.bti"),
        one("effects/pcr/ony_ac_y.pcr", "effects/tex/ring_i4.bti"),
    ];
    /// `EFF_Onyon_HaloRing{Blue,Red,Yellow}`: their glow, alongside.
    pub const ONYON_HALO_RING: [EffectDef; 3] = [
        one("effects/pcr/ony_a_br.pcr", "effects/tex/fuchibal.bti"),
        one("effects/pcr/ony_a_rr.pcr", "effects/tex/fuchibal.bti"),
        one("effects/pcr/ony_a_yr.pcr", "effects/tex/fuchibal.bti"),
    ];
}

/// `PTCLFLAG_*` (zen/particle.h).
pub mod flags {
    pub const SHAPE_SPHERE: u32 = 1 << 0;
    pub const SHAPE_BOX: u32 = 1 << 1;
    pub const VEL_OMNI: u32 = 1 << 2;
    pub const VEL_DIRECTIONAL: u32 = 1 << 3;
    pub const CLAMP_VELOCITY: u32 = 1 << 4;
    pub const RATE_MANUAL: u32 = 1 << 6;
    pub const RATE_LINEAR: u32 = 1 << 7;
    pub const RADIUS_MANUAL: u32 = 1 << 8;
    pub const RADIUS_LINEAR: u32 = 1 << 9;
    pub const VEL_MANUAL: u32 = 1 << 10;
    pub const VEL_LINEAR: u32 = 1 << 11;
    pub const DISABLE_EMISSION: u32 = 1 << 12;
    pub const CHILDREN: u32 = 1 << 13;
    pub const CHILD_INHERIT_COLOR: u32 = 1 << 14;
    pub const GRAVITY: u32 = 1 << 16;
    pub const AIR: u32 = 1 << 17;
    pub const VORTEX: u32 = 1 << 18;
    pub const DAMPED_NEWTON: u32 = 1 << 19;
    pub const NEWTON: u32 = 1 << 20;
    pub const SOLID_TEX: u32 = 1 << 21;
    pub const JITTER: u32 = 1 << 22;
    pub const LINE: u32 = 1 << 23;
}

/// A value that is constant or keyed over the generator's frame (`pmIntpManual` /
/// `pmIntpLinear`).
#[derive(Debug, Clone, PartialEq)]
pub enum Keyed {
    Const(f32),
    /// Holds each value until the next threshold.
    Manual(Vec<f32>, Vec<f32>),
    Linear(Vec<f32>, Vec<f32>),
}

impl Keyed {
    fn read(r: &mut Reader, flags: u32, manual: u32, linear: u32) -> Result<Self, Error> {
        if flags & (manual | linear) == 0 {
            return Ok(Keyed::Const(r.f32()?));
        }
        let n = r.u8()? as usize;
        r.skip(3)?;
        let t = (0..n).map(|_| r.f32()).collect::<Result<Vec<_>, _>>()?;
        let v = (0..n).map(|_| r.f32()).collect::<Result<Vec<_>, _>>()?;
        Ok(if flags & manual != 0 { Keyed::Manual(t, v) } else { Keyed::Linear(t, v) })
    }

    /// The value at `frame` of `max_frame`.
    fn at(&self, frame: f32, max_frame: f32) -> f32 {
        match self {
            Keyed::Const(v) => *v,
            Keyed::Manual(t, v) => {
                let mut i = 1;
                while i < t.len() && frame > max_frame * t[i] {
                    i += 1;
                }
                v.get(i - 1).copied().unwrap_or(0.0)
            }
            Keyed::Linear(t, v) => {
                let ratio = frame / max_frame.max(1.0);
                let mut i = 1;
                while i + 1 < t.len() && ratio > t[i] {
                    i += 1;
                }
                if i >= t.len() {
                    return v.last().copied().unwrap_or(0.0);
                }
                let span = t[i] - t[i - 1];
                if span == 0.0 {
                    return v[i];
                }
                (v[i] * (ratio - t[i - 1]) + v[i - 1] * (t[i] - ratio)) / span
            }
        }
    }
}

/// `bBoardColourAnimData`: primary and env colours keyed over a particle's life.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorAnim {
    /// How the TEV combines texture and colours (`DGXGraphics::setBlendMode`'s last argument).
    pub blend_mode: u8,
    pub duration: u8,
    /// Non-zero: the keys span the particle's life and hold at the end; zero: they loop over
    /// `duration` frames.
    pub flags: u8,
    pub thresholds: Vec<f32>,
    pub prim: Vec<[u8; 4]>,
    pub env: Vec<[u8; 4]>,
}

/// Up to nine bytes the emitter reads after the shorts (`pmSetDDF`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fields {
    pub gravity: Vec3,
    pub air: Vec3,
    pub vortex_axis: Vec3,
    pub vortex_rotation: f32,
    pub vortex_strength: f32,
    pub vortex_falloff: f32,
    pub vortex_divisor: f32,
    pub damped_newton: Vec3,
    pub damped_newton_strength: f32,
    pub newton: Vec3,
    pub newton_strength: f32,
    pub jitter: f32,
    pub line_axis: Vec3,
    pub line_axial: f32,
    pub line_radial: f32,
}

/// Child particles: small fading copies a particle leaves behind.
#[derive(Debug, Clone, PartialEq)]
pub struct Children {
    pub scale: f32,
    pub alpha: f32,
    pub pos_jitter: f32,
    pub color: [u8; 4],
    pub life: u8,
    pub interval: u8,
}

/// One `.pcr` generator definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcr {
    /// Camera-facing quads (`drawPtclBillboard`); otherwise quads stretched along each
    /// particle's velocity or position (`drawPtclOriented`).
    pub billboard: bool,
    pub length_scale: f32,
    pub pivot_y: f32,
    /// Oriented only: 0 Y, 1 X, 2 Z, 3 XY, 4 XZ, 5 YZ, 6 XYZ (`mRotAxisCallBack`).
    pub rot_axis: u8,
    /// Oriented only: true = along the position from the emitter, false = along the velocity.
    pub orient_by_position: bool,
    /// Oriented only: a second quad crossing the first.
    pub double_sided: bool,
    pub anim: ColorAnim,
    pub flags: u32,
    pub emit_offset: Vec3,
    pub emit_dir: Vec3,
    pub box_size: Vec3,
    pub spread: f32,
    pub rate: Keyed,
    pub rate_jitter: f32,
    pub radius_scale: f32,
    pub radius: Keyed,
    pub init_vel: Keyed,
    pub vel_jitter: f32,
    pub drag: f32,
    pub drag_jitter: f32,
    pub max_vel: f32,
    pub scale_thresholds: [f32; 2],
    pub min_scales: [f32; 2],
    pub size: f32,
    pub size_jitter: f32,
    pub alpha_thresholds: [f32; 2],
    pub alpha_jitter: f32,
    pub life_jitter: f32,
    /// Angles are 16-bit (0x10000 = a full turn).
    pub rot_speed: i16,
    pub rot_speed_jitter: i16,
    pub rot_angle: i16,
    pub life: i16,
    pub max_frame: i16,
    pub free_motion_time: u8,
    /// 0 = emit forever.
    pub max_passes: u8,
    /// GX blend factors: source in the low nibble, destination in the high one.
    pub blend_factor: u8,
    /// GX Z mode: bit 3 test, bits 0-2 compare function, bit 4 write.
    pub z_mode: u8,
    pub fields: Fields,
    pub children: Option<Children>,
}

impl Pcr {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        r.seek(4);
        let ddf = r.u16()? as usize;
        let billboard = r.u8()? != 0;
        let (length_scale, pivot_y, anim_at) = if billboard {
            (0.0, 0.0, 8)
        } else {
            r.seek(8);
            (r.f32()?, r.f32()?, 16)
        };
        r.seek(anim_at);
        let blend_mode = r.u8()?;
        let duration = r.u8()?;
        let mut anim_flags = r.u8()?;
        let n = r.u8()? as usize;
        let thresholds = (0..n).map(|_| r.f32()).collect::<Result<Vec<_>, _>>()?;
        let colors = |r: &mut Reader| {
            (0..n)
                .map(|_| Ok([r.u8()?, r.u8()?, r.u8()?, r.u8()?]))
                .collect::<Result<Vec<_>, Error>>()
        };
        let prim = colors(&mut r)?;
        // Some files' env colours run into the emitter block; the game reads them anyway.
        let env = colors(&mut r)?;
        let (rot_axis, orient_by_position, double_sided) = if billboard {
            (0, false, false)
        } else {
            let f = anim_flags;
            anim_flags &= 1;
            ((f >> 1) & 7, f & 0x20 != 0, f & 0x40 != 0)
        };

        r.seek(ddf);
        let flags = r.u32()?;
        let emit_offset = r.vec3()?;
        let emit_dir = r.vec3()?;
        let box_size = r.vec3()?;
        let spread = r.f32()?;
        let rate = Keyed::read(&mut r, flags, flags::RATE_MANUAL, flags::RATE_LINEAR)?;
        let rate_jitter = r.f32()?;
        let radius_scale = r.f32()?;
        let radius = Keyed::read(&mut r, flags, flags::RADIUS_MANUAL, flags::RADIUS_LINEAR)?;
        let init_vel = Keyed::read(&mut r, flags, flags::VEL_MANUAL, flags::VEL_LINEAR)?;
        let vel_jitter = r.f32()?;
        let drag = r.f32()?;
        let drag_jitter = r.f32()?;
        let max_vel = r.f32()?;
        let scale_thresholds = [r.f32()?, r.f32()?];
        let min_scales = [r.f32()?, r.f32()?];
        let size = r.f32()?;
        let size_jitter = r.f32()?;
        let alpha_thresholds = [r.f32()?, r.f32()?];
        let alpha_jitter = r.f32()?;
        let life_jitter = r.f32()?;
        let rot_speed = r.i16()?;
        let rot_speed_jitter = r.i16()?;
        let rot_angle = r.i16()?;
        let life = r.i16()?;
        let max_frame = r.i16()?;
        let free_motion_time = r.u8()?;
        r.skip(1)?;
        let max_passes = r.u8()?;
        let blend_factor = r.u8()?;
        let z_mode = r.u8()?;
        r.skip(1)?;

        let mut f = Fields {
            vortex_axis: Vec3::Y,
            vortex_divisor: 1.0,
            line_axis: Vec3::Y,
            ..Default::default()
        };
        if flags & flags::GRAVITY != 0 {
            f.gravity = r.vec3()?;
        }
        if flags & flags::AIR != 0 {
            f.air = r.vec3()?;
        }
        if flags & flags::VORTEX != 0 {
            f.vortex_axis = r.vec3()?;
            f.vortex_rotation = r.f32()?;
            f.vortex_strength = r.f32()?;
            f.vortex_falloff = r.f32()?;
            f.vortex_divisor = r.f32()?;
        }
        if flags & flags::DAMPED_NEWTON != 0 {
            f.damped_newton = r.vec3()?;
            f.damped_newton_strength = r.f32()?;
        }
        if flags & flags::NEWTON != 0 {
            f.newton = r.vec3()?;
            f.newton_strength = r.f32()?;
        }
        if flags & flags::SOLID_TEX != 0 {
            // Force multiplier and grid; the field's lookup tables live in the executable, so
            // this force isn't applied.
            r.skip(12 + 4)?;
        }
        if flags & flags::JITTER != 0 {
            f.jitter = r.f32()?;
        }
        if flags & flags::LINE != 0 {
            f.line_axis = r.vec3()?;
            f.line_axial = r.f32()?;
            f.line_radial = r.f32()?;
        }
        let children = if flags & flags::CHILDREN != 0 {
            let scale = r.f32()?;
            let alpha = r.f32()?;
            let pos_jitter = r.f32()?;
            let color = [r.u8()?, r.u8()?, r.u8()?, r.u8()?];
            let life = r.u8()?;
            let interval = r.u8()?;
            Some(Children { scale, alpha, pos_jitter, color, life, interval })
        } else {
            None
        };
        // `pmSetDDF` normalises the field axes (a zero one gets a random axis at use).
        f.vortex_axis = f.vortex_axis.normalize_or_zero();
        f.line_axis = f.line_axis.normalize_or_zero();

        Ok(Pcr {
            billboard,
            length_scale,
            pivot_y,
            rot_axis,
            orient_by_position,
            double_sided,
            anim: ColorAnim { blend_mode, duration, flags: anim_flags, thresholds, prim, env },
            flags,
            emit_offset,
            emit_dir,
            box_size,
            spread,
            rate,
            rate_jitter,
            radius_scale,
            radius,
            init_vel,
            vel_jitter,
            drag,
            drag_jitter,
            max_vel,
            scale_thresholds,
            min_scales,
            size,
            size_jitter,
            alpha_thresholds,
            alpha_jitter,
            life_jitter,
            rot_speed,
            rot_speed_jitter,
            rot_angle,
            life,
            max_frame,
            free_motion_time,
            max_passes,
            blend_factor,
            z_mode,
            fields: f,
            children,
        })
    }

    /// `DGXGraphics::setBlendMode`'s colour combiner, with C0 = primary and C1 = env:
    /// 0 `lerp(prim, 1, tex)`, 1 `prim * tex`, 2 `lerp(env, prim, tex)`, 3 `env + prim * tex`,
    /// 4 `prim`. Alpha is always `tex.a * prim.a`.
    pub fn color_mode(&self) -> u8 {
        self.anim.blend_mode
    }

    /// `mScaleRate1/2`, `mAlphaRate1/2` (end of `pmSetDDF`).
    fn rates(&self) -> [f32; 4] {
        let [s1, s2] = self.scale_thresholds;
        let [a1, a2] = self.alpha_thresholds;
        [
            if s1 == 0.0 { 1.0 } else { (1.0 - self.min_scales[0]) / s1 },
            if s2 == 1.0 { 1.0 } else { (1.0 - self.min_scales[1]) / (1.0 - s2) },
            if a1 == 0.0 { 1.0 } else { 1.0 / a1 },
            if a2 == 1.0 { 1.0 } else { 1.0 / (1.0 - a2) },
        ]
    }
}

/// `particleMdl`
#[derive(Debug, Clone, PartialEq)]
pub struct Particle {
    /// Offset from `global` (the emitter's position when it was born).
    pub local: Vec3,
    pub global: Vec3,
    pub vel: Vec3,
    pub size: f32,
    pub scale: f32,
    pub alpha: f32,
    /// Host extension: the [`Generator::max_age`] fade, 1 at full and 0 gone. It multiplies
    /// the drawn alpha only; `life` keeps driving the authored scale/colour curves, so a
    /// clipping cloud fades in place instead of re-scoring `age / life` and inflating.
    pub fade: f32,
    /// Host extension: frames left before this particle starts its `fade`. The cap gives each
    /// particle a different delay, so a cloud dissolves puff by puff instead of dimming as
    /// one sheet and vanishing on a single frame.
    pub fade_delay: f32,
    pub rot_speed: f32,
    /// 16-bit angle units.
    pub rot_angle: f32,
    pub age: i32,
    age_timer: f32,
    pub life: i32,
    pub prim: [u8; 4],
    pub env: [u8; 4],
    pub normal: Vec3,
    anim_progress: f32,
    anim_frame: usize,
    anim_duration: f32,
}

/// `particleChildMdl`
#[derive(Debug, Clone, PartialEq)]
pub struct Child {
    pub pos: Vec3,
    pub size: f32,
    pub color: [u8; 4],
    life: u8,
    age: u8,
    timer: f32,
    fade: u8,
}

/// A particle to draw: four corners (top-left, top-right, bottom-right, bottom-left, texture
/// (0,0) (1,0) (1,1) (0,1)) and its TEV colours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub corners: [Vec3; 4],
    pub prim: [u8; 4],
    pub env: [u8; 4],
    /// Children draw with the child texture (if any) and `prim` as the colour.
    pub child: bool,
}

/// Frames (1/30 s each) the [`Generator::max_age`] fade takes.
pub const FADE_FRAMES: f32 = 20.0;

/// Frames a particle may hold at full alpha after the cap before its own fade starts. Spread
/// across the cloud, this makes the dust dissolve over a second instead of dimming and cutting.
pub const STAGGER_FRAMES: f32 = 10.0;

/// `particleGenerator`: emits and moves particles from one `.pcr`.
#[derive(Debug, Clone)]
pub struct Generator {    pub pcr: Arc<Pcr>,
    /// Where particles are born (`getGPos`); hosts move it to follow something.
    pub emit_pos: Vec3,
    /// Host extension: multiplies every particle's drawn alpha (a landing cloud toned down
    /// without changing the shared effect table).
    pub alpha_scale: f32,
    /// Host extension: stop emitting `max_age` frames (30 = 1 s) after the spawn and fade the
    /// particles out over [`FADE_FRAMES`] in place, so a landing cloud clears instead of
    /// shrouding the scene. The fade touches alpha only: shortening `life` here would re-score
    /// the authored scale curve (`age / life`) and balloon the puffs for the last few frames.
    pub max_age: Option<f32>,
    age: f32,
    /// `max_age` was reached; the remaining particles are fading out.
    fading: bool,
    /// Host-controlled density for attached emitters (for example, ground dust as an Onion rises).
    pub emission_scale: f32,
    /// Added to every new particle's velocity (`mEmitVelocity`).
    pub emit_vel: Vec3,
    /// New particles' orientation normal (`mOrientedNormal`).
    pub oriented_normal: Vec3,
    /// `setOrientedConstZAxis`: keep each particle's normal fixed when drawing oriented.
    pub fixed_normal: bool,
    /// `mEmitDir`, from the file until `setEmitDir` changes it.
    pub emit_dir: Vec3,
    /// From this generator frame on, no gravity (`setGravityField(0)` in a callback, like
    /// `GoalEffect::invoke` at frame 50).
    pub gravity_off_at: Option<f32>,
    no_gravity: bool,
    pub particles: Vec<Particle>,
    pub children: Vec<Child>,
    partial: f32,
    pass_timer: f32,
    frame: f32,
    pass: u8,
    /// `finish`: no more emission; the generator dies when its particles have.
    finished: bool,
    /// Host extension: run the authored pass curve again and again instead of stopping after
    /// `max_passes` (the Onion's one-pass lift cloud while it stays aloft). Keeps `pcr` shared
    /// with the effect table, which is what drawing looks the generator up by.
    pub loop_forever: bool,
    rng: u32,
}

impl Generator {
    pub fn new(pcr: Arc<Pcr>, pos: Vec3, seed: u32) -> Self {
        Generator {
            emit_dir: pcr.emit_dir,
            pcr,
            max_age: None,
            age: 0.0,
            fading: false,
            emit_pos: pos,
            alpha_scale: 1.0,
            emission_scale: 1.0,
            emit_vel: Vec3::ZERO,
            oriented_normal: Vec3::Y,
            fixed_normal: false,
            gravity_off_at: None,
            no_gravity: false,
            particles: Vec::new(),
            children: Vec::new(),
            partial: 0.0,
            pass_timer: 0.0,
            frame: 0.0,
            pass: 0,
            finished: false,
            loop_forever: false,
            rng: seed | 1,
        }
    }

    /// `zen::Rand`: uniform in [0, max).
    fn rand(&mut self, max: f32) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32 * max
    }

    /// `zen::RandShift`: uniform in [-max, max).
    fn rand_shift(&mut self, max: f32) -> f32 {
        self.rand(2.0 * max) - max
    }

    /// `pmGetArbitUnitVec` (its two angles aren't uniform over the sphere, like the game's).
    fn arbit_unit(&mut self) -> Vec3 {
        let (a, b) = (self.rand(TAU), self.rand(TAU));
        Vec3::new(a.sin() * b.cos(), a.sin() * b.sin(), a.cos())
    }

    /// `particleGenerator::finish`
    pub fn finish(&mut self) {
        self.finished = true;
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// `particleGenerator::update`: one step of `step` frames (1.0 = 1/30 s). `each` runs on
    /// every new and updated particle (the game's per-particle callback). Returns false once
    /// it has finished and every particle is gone.
    pub fn update(&mut self, step: f32, each: &mut dyn FnMut(&mut Particle)) -> bool {
        if let Some(max) = self.max_age {
            self.age += step;
            if self.age >= max && !self.fading {
                self.finished = true;
                self.fading = true;
                self.children.clear();
                // Stagger the fade so the cloud dissolves rather than dimming as one sheet and
                // disappearing on a single frame.
                let delays: Vec<f32> = (0..self.particles.len()).map(|_| self.rand(STAGGER_FRAMES)).collect();
                for (p, delay) in self.particles.iter_mut().zip(delays) { p.fade_delay = delay; }
            }
        }
        if !self.finished {
            self.emit(each);
        } else if self.particles.is_empty() && self.children.is_empty() {
            return false;
        }
        if self.gravity_off_at.is_some_and(|f| self.frame >= f) {
            self.no_gravity = true;
        }
        self.update_particles(step, each);
        self.pass_timer += step;
        self.frame = self.pass_timer.floor();
        if self.frame >= self.pcr.max_frame as f32 {
            self.pass_timer = 0.0;
            self.frame = 0.0;
            self.pass = self.pass.saturating_add(1);
            if !self.loop_forever && self.pcr.max_passes != 0 && self.pass >= self.pcr.max_passes {
                self.pass = self.pcr.max_passes;
                self.finish();
            }
        }
        true
    }

    /// `SetPtclsLife`
    fn emit(&mut self, each: &mut dyn FnMut(&mut Particle)) {
        let pcr = self.pcr.clone();
        if pcr.flags & flags::DISABLE_EMISSION != 0 {
            return;
        }
        let a = pcr.rate.at(self.frame, pcr.max_frame as f32);
        self.partial += (a + self.rand_shift(a * pcr.rate_jitter)).max(0.0) * self.emission_scale;
        let n = self.partial.max(0.0) as usize;
        self.partial -= n as f32;
        for _ in 0..n {
            let life = (pcr.life as f32 * (1.0 - self.rand(pcr.life_jitter))) as i32;
            let mut p = self.spawn(&pcr, life);
            each(&mut p);
            self.particles.push(p);
        }
    }

    /// `PtclsGen`: a new particle's position and velocity from the emitter shape.
    fn spawn(&mut self, pcr: &Pcr, life: i32) -> Particle {
        let dir = self.emit_dir.try_normalize().unwrap_or_else(|| self.arbit_unit());
        let init_vel = pcr.init_vel.at(self.frame, pcr.max_frame as f32);
        let b = pcr.radius.at(self.frame, pcr.max_frame as f32);
        let c = pcr.radius_scale * b;
        let mut life = life;
        let shape = pcr.flags & (flags::SHAPE_SPHERE | flags::SHAPE_BOX);
        let veldir = pcr.flags & (flags::VEL_OMNI | flags::VEL_DIRECTIONAL);
        let jitter_vel = |g: &mut Self| init_vel + g.rand_shift(init_vel * pcr.vel_jitter);
        let (pos, vel) = if shape == flags::SHAPE_SPHERE {
            if veldir == flags::VEL_OMNI {
                let v = self.arbit_unit();
                let r = c + self.rand(b - c);
                (pcr.emit_offset + v * r, v * jitter_vel(self))
            } else if veldir == flags::VEL_DIRECTIONAL {
                let t = self.arbit_unit().cross(dir) * self.rand(pcr.spread);
                let v = (dir + t).normalize_or_zero();
                (pcr.emit_offset, v * jitter_vel(self))
            } else {
                // A ring around the emit direction.
                let v = self.arbit_unit().cross(dir);
                let v = match v.try_normalize() {
                    Some(v) => v,
                    None => {
                        life = 1;
                        Vec3::ZERO
                    }
                };
                let r = c + self.rand(b - c);
                (pcr.emit_offset + v * r, v * jitter_vel(self))
            }
        } else if shape == flags::SHAPE_BOX {
            let p = Vec3::new(
                self.rand_shift(1.0) * pcr.box_size.x,
                self.rand_shift(1.0) * pcr.box_size.y,
                self.rand_shift(1.0) * pcr.box_size.z,
            ) + pcr.emit_offset;
            (p, dir * jitter_vel(self))
        } else {
            // A cylinder around the emit direction (box x = radius, y = half height).
            let v = self.arbit_unit().cross(dir);
            let v = match v.try_normalize() {
                Some(v) => v,
                None => {
                    life = 1;
                    Vec3::ZERO
                }
            };
            let h = self.rand_shift(1.0) * pcr.box_size.y;
            let side = dir * h + v * pcr.box_size.x;
            let s = 1.0 + self.rand(pcr.radius_scale - 1.0);
            let vel = if veldir == flags::VEL_DIRECTIONAL { dir } else { v } * jitter_vel(self);
            (side * s + pcr.emit_offset, vel)
        };
        let size = ((pcr.size_jitter - 2.0 * pcr.size_jitter * self.rand_shift(1.0)) * pcr.size + pcr.size).max(0.0);
        let rot_speed = pcr.rot_speed as f32 + pcr.rot_speed_jitter as f32 * self.rand_shift(1.0);
        let rot_angle = pcr.rot_angle as f32 * self.rand_shift(1.0);
        // `bBoardColourAnim::init`
        let anim_duration = if pcr.anim.flags != 0 { life as f32 } else { pcr.anim.duration as f32 };
        Particle {
            local: pos,
            global: self.emit_pos,
            vel: vel + self.emit_vel,
            size,
            scale: 1.0,
            alpha: 1.0,
            fade: 1.0,
            fade_delay: 0.0,
            rot_speed,
            rot_angle,
            age: 0,
            age_timer: 0.0,
            life: life.max(1),
            prim: pcr.anim.prim.first().copied().unwrap_or([255; 4]),
            env: pcr.anim.env.first().copied().unwrap_or([255; 4]),
            normal: self.oriented_normal,
            anim_progress: 0.0,
            anim_frame: 0,
            anim_duration,
        }
    }

    /// `pmCalcAccel` (the solid-texture field is skipped; its tables are in the executable).
    fn accel(&mut self, p: &mut Particle) -> Vec3 {
        let pcr = self.pcr.clone();
        let f = &pcr.fields;
        let mut a = Vec3::ZERO;
        if pcr.flags & flags::GRAVITY != 0 && !self.no_gravity {
            a += f.gravity;
        }
        if pcr.flags & flags::VORTEX != 0 {
            let axis = f.vortex_axis;
            let radial = p.local - axis * axis.dot(p.local);
            let d2 = radial.length_squared();
            let k = if f.vortex_divisor == 0.0 {
                1.0
            } else {
                (f.vortex_falloff * f.vortex_divisor - d2) / (f.vortex_divisor + d2) + 1.0
            };
            let push = radial * f.vortex_strength * k;
            p.local = Quat::from_axis_angle(axis, -f.vortex_rotation * k) * p.local + push;
        }
        if pcr.flags & flags::DAMPED_NEWTON != 0 {
            a += (f.damped_newton - p.local) * f.damped_newton_strength - p.vel;
        }
        if pcr.flags & flags::NEWTON != 0 {
            a += (f.newton - p.local) * f.newton_strength;
        }
        if pcr.flags & flags::JITTER != 0 {
            a += Vec3::new(self.rand_shift(f.jitter), self.rand_shift(f.jitter), self.rand_shift(f.jitter));
        }
        if pcr.flags & flags::LINE != 0 {
            p.vel = Vec3::ZERO;
            let along = f.line_axis * f.line_axis.dot(p.local);
            a += f.line_axis * f.line_axial + (along - p.local) * f.line_radial;
        }
        if pcr.flags & flags::AIR != 0 {
            p.local += f.air;
        }
        a
    }

    /// `UpdatePtclsStatus`
    fn update_particles(&mut self, step: f32, each: &mut dyn FnMut(&mut Particle)) {
        let pcr = self.pcr.clone();
        let rates = pcr.rates();
        let drag = pcr.drag - self.rand(pcr.drag_jitter);
        let mut list = std::mem::take(&mut self.particles);
        if self.fading {
            for p in &mut list {
                if p.fade_delay > 0.0 {
                    p.fade_delay -= step;
                } else {
                    p.fade = (p.fade - step / FADE_FRAMES).max(0.0);
                }
            }
            list.retain(|p| p.fade_delay > 0.0 || p.fade > 0.0);
        }
        list.retain(|p| p.age + 1 < p.life);
        for p in list.iter_mut() {
            let a = if p.age >= pcr.free_motion_time as i32 { self.accel(p) } else { Vec3::ZERO };
            p.vel = p.vel * drag + a;
            if pcr.flags & flags::CLAMP_VELOCITY != 0 && p.vel.length_squared() > pcr.max_vel * pcr.max_vel {
                p.vel = p.vel.normalize() * pcr.max_vel;
            }
            p.local += p.vel;
            let t = p.age as f32 / p.life as f32;
            let [s1, s2] = pcr.scale_thresholds;
            p.scale = if t < s1 {
                t * rates[0] + pcr.min_scales[0]
            } else if t < s2 {
                1.0
            } else {
                (1.0 - t) * rates[1] + pcr.min_scales[1]
            };
            let [a1, a2] = pcr.alpha_thresholds;
            p.alpha = if t < a1 {
                t * rates[2]
            } else if t < a2 {
                1.0
            } else {
                (1.0 - t) * rates[3]
            };
            p.alpha *= 1.0 - self.rand(pcr.alpha_jitter);
            p.alpha *= p.fade;
            p.rot_angle += p.rot_speed;
            p.age_timer += step;
            p.age = round_off(p.age_timer);
            if let Some(c) = pcr.children.as_ref().filter(|c| c.interval > 0)
                && (p.age + 1) % c.interval as i32 == 0
            {
                let pos = p.global
                    + p.local
                    + Vec3::new(self.rand_shift(c.pos_jitter), self.rand_shift(c.pos_jitter), self.rand_shift(c.pos_jitter));
                let mut color = if pcr.flags & flags::CHILD_INHERIT_COLOR != 0 {
                    p.prim
                } else {
                    [c.color[0], c.color[1], c.color[2], 255]
                };
                color[3] = (color[3] as f32 * c.alpha) as u8;
                let fade = round_off(color[3] as f32 / (c.life as f32 / step)).clamp(0, 255) as u8;
                self.children.push(Child {
                    pos,
                    size: p.size * p.scale * c.scale,
                    color,
                    life: c.life,
                    age: 0,
                    timer: 0.0,
                    fade,
                });
            }
            animate_color(&pcr.anim, p, step);
            each(p);
        }
        self.particles = list;
        // `updatePtclChildren`
        self.children.retain_mut(|c| {
            if c.age as u32 + 1 >= c.life as u32 {
                return false;
            }
            c.color[3] = c.color[3].saturating_sub(c.fade);
            c.timer += step;
            c.age = round_off(c.timer).clamp(0, 255) as u8;
            true
        });
    }

    /// Quads for every particle (children first, as `draw` does). `right` and `up` are the
    /// camera's axes in world space. Oriented particles whose direction vanishes are killed,
    /// like `drawPtclOriented` does.
    pub fn quads(&mut self, right: Vec3, up: Vec3) -> Vec<Quad> {
        let pcr = self.pcr.clone();
        let mut out = Vec::with_capacity(self.particles.len() + self.children.len());
        for c in &self.children {
            // `drawParticle`: an axis-aligned billboard of half-size 25 * size.
            let r = 25.0 * c.size;
            let (x, y) = (right * r, up * r);
            out.push(Quad {
                corners: [c.pos - x + y, c.pos + x + y, c.pos + x - y, c.pos - x - y],
                prim: c.color,
                env: c.color,
                child: true,
            });
        }
        let fixed = self.fixed_normal;
        for p in self.particles.iter_mut() {
            let prim = [p.prim[0], p.prim[1], p.prim[2], round_off(p.prim[3] as f32 * p.alpha).clamp(0, 255) as u8];
            let at = p.local + p.global;
            if pcr.billboard {
                // `drawRotParticle`: corners at the quad's half-diagonal 25 * size, turned by
                // the particle's angle.
                let r = p.size * p.scale * 25.0;
                let corner = |turn: f32| {
                    let a = (-p.rot_angle + turn) / 65536.0 * TAU;
                    at + right * (r * a.sin()) + up * (r * a.cos())
                };
                out.push(Quad {
                    corners: [corner(-8192.0), corner(8192.0), corner(24576.0), corner(40960.0)],
                    prim,
                    env: p.env,
                    child: false,
                });
                continue;
            }
            let dir = if pcr.orient_by_position { p.local } else { p.vel };
            let (Some(along), true) = (dir.try_normalize(), dir.length_squared() != 0.0) else {
                p.age = p.life;
                p.age_timer = p.life as f32;
                continue;
            };
            let Some(side) = along.cross(p.normal).try_normalize() else {
                p.age = p.life;
                p.age_timer = p.life as f32;
                continue;
            };
            let (along, normal) = if fixed {
                (p.normal.cross(side).normalize_or_zero(), p.normal)
            } else {
                (along, side.cross(along).normalize_or_zero())
            };
            if !fixed {
                p.normal = normal;
            }
            let a = p.size * p.scale;
            let basis = Mat3::from_cols(side * a, along * a * pcr.length_scale, normal * a);
            let (s, c) = (p.rot_angle / 65536.0 * TAU).sin_cos();
            let rot = rot_axis(pcr.rot_axis, s, c);
            let pivot = Vec3::new(0.0, 25.0 * pcr.pivot_y, 0.0);
            let m = basis * rot;
            let place = |v: Vec3| at + m * v + basis * pivot;
            // `lpsPos`: a quad in XY, and for double-sided ones a second in YZ.
            let quads: &[[Vec3; 4]] = if pcr.double_sided {
                &[XY_QUAD, YZ_QUAD]
            } else {
                &[XY_QUAD]
            };
            for q in quads {
                out.push(Quad {
                    corners: q.map(place),
                    prim,
                    env: p.env,
                    child: false,
                });
            }
        }
        out
    }
}

const XY_QUAD: [Vec3; 4] = [
    Vec3::new(-25.0, 25.0, 0.0),
    Vec3::new(25.0, 25.0, 0.0),
    Vec3::new(25.0, -25.0, 0.0),
    Vec3::new(-25.0, -25.0, 0.0),
];
const YZ_QUAD: [Vec3; 4] = [
    Vec3::new(0.0, 25.0, -25.0),
    Vec3::new(0.0, 25.0, 25.0),
    Vec3::new(0.0, -25.0, 25.0),
    Vec3::new(0.0, -25.0, -25.0),
];

/// `RotAxisX` … `RotAxisXYZ`, as row-major GX matrices turned into glam's column form.
fn rot_axis(kind: u8, s: f32, c: f32) -> Mat3 {
    let rows: [[f32; 3]; 3] = match kind {
        1 => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
        2 => [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]],
        3 => [[c, s * s, -s * c], [0.0, c, s], [s, -s * c, c * c]],
        4 => [[c, -s * c, -s * s], [s, c * c, s * c], [0.0, -s, c]],
        5 => [[c * c, -s, -s * c], [s * c, c, -s * s], [s, 0.0, c]],
        6 => [
            [c * c, -c * s, -s],
            [(1.0 - s) * (c * s), c * c + s * s * s, -c * s],
            [s * (s + c * c), (1.0 - s) * (c * s), c * c],
        ],
        _ => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
    };
    Mat3::from_cols_array_2d(&rows).transpose()
}

/// `zen::RoundOff`
fn round_off(v: f32) -> i32 {
    if v >= 0.0 { (v + 0.5) as i32 } else { (v - 0.5) as i32 }
}

/// `bBoardColourAnim::update`
fn animate_color(anim: &ColorAnim, p: &mut Particle, step: f32) {
    let n = anim.thresholds.len().min(anim.prim.len()).min(anim.env.len());
    if n == 0 {
        return;
    }
    if n == 1 {
        p.prim = anim.prim[0];
        p.env = anim.env[0];
        return;
    }
    let i = p.anim_frame.min(n - 2);
    let span = p.anim_duration - 1.0;
    let (t0, t1) = (span * anim.thresholds[i], span * anim.thresholds[i + 1]);
    let t = if t1 > t0 { ((p.anim_progress - t0) / (t1 - t0)).min(1.0) } else { 1.0 };
    let lerp = |a: [u8; 4], b: [u8; 4]| std::array::from_fn(|k| (a[k] as f32 * (1.0 - t) + b[k] as f32 * t) as u8);
    p.prim = lerp(anim.prim[i], anim.prim[i + 1]);
    p.env = lerp(anim.env[i], anim.env[i + 1]);
    if p.anim_progress >= t1 {
        p.anim_frame = i + 1;
        if p.anim_frame >= n - 1 {
            p.anim_frame = if anim.flags != 0 { n - 1 } else { 0 };
        }
    }
    p.anim_progress += step;
    if p.anim_progress >= p.anim_duration {
        if anim.flags != 0 {
            p.anim_progress = p.anim_duration;
            p.anim_frame = n - 1;
        } else {
            p.anim_progress = 0.0;
            p.anim_frame = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A billboard `.pcr` built by hand: one particle a frame, rising 2 per frame for 8 frames.
    fn sample() -> Vec<u8> {
        let mut d = b"1-00".to_vec();
        d.extend_from_slice(&0x24u16.to_be_bytes());
        d.extend_from_slice(&[1, 0]);
        // Colour anim: mode 2, 80 frames, held, 2 keys.
        d.extend_from_slice(&[2, 80, 1, 2]);
        for t in [0.0f32, 1.0] {
            d.extend_from_slice(&t.to_be_bytes());
        }
        d.extend_from_slice(&[255, 0, 0, 255, 0, 0, 255, 255]);
        d.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(d.len(), 0x24);
        let f = |d: &mut Vec<u8>, v: f32| d.extend_from_slice(&v.to_be_bytes());
        d.extend_from_slice(&(flags::SHAPE_SPHERE | flags::VEL_DIRECTIONAL).to_be_bytes());
        for v in [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0] {
            f(&mut d, v); // offset, dir, box
        }
        for v in [0.0, 1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.9, 0.0, 0.0] {
            f(&mut d, v); // spread, rate .. life jitter
        }
        for s in [0i16, 0, 0, 8, 80] {
            d.extend_from_slice(&s.to_be_bytes());
        }
        d.extend_from_slice(&[0, 0, 0, 0x14, 0x0B, 0]);
        d
    }

    #[test]
    fn parses_and_runs_a_billboard_generator() {
        let pcr = Arc::new(Pcr::parse(&sample()).unwrap());
        assert!(pcr.billboard);
        assert_eq!(pcr.life, 8);
        assert_eq!(pcr.blend_factor, 0x14);
        assert_eq!(pcr.init_vel, Keyed::Const(2.0));
        let mut g = Generator::new(pcr, Vec3::new(10.0, 0.0, 0.0), 1);
        for _ in 0..20 {
            assert!(g.update(1.0, &mut |_| {}));
        }
        // One a frame, each living 8 frames.
        assert!((6..=8).contains(&g.particles.len()), "{}", g.particles.len());
        let oldest = g.particles.iter().max_by_key(|p| p.age).unwrap();
        assert!((oldest.local.y - 2.0 * oldest.age as f32).abs() < 1e-3, "{oldest:?}");
        assert_eq!(oldest.global, Vec3::new(10.0, 0.0, 0.0));
        let quads = g.quads(Vec3::X, Vec3::Y);
        assert_eq!(quads.len(), g.particles.len());
        g.finish();
        let mut steps = 0;
        while g.update(1.0, &mut |_| {}) {
            steps += 1;
            assert!(steps < 20);
        }
    }

    /// The host's `max_age` used to shorten `life`; `age / life` then jumped and the authored
    /// scale curve inflated every puff for the fade. The fade must dim alpha, not resize.
    #[test]
    fn the_max_age_fade_keeps_the_authored_scale_and_fades_alpha() {
        let mut pcr = Pcr::parse(&sample()).unwrap();
        pcr.scale_thresholds = [0.0, 0.0];
        pcr.min_scales = [1.0, 2.4023438];
        pcr.life = 100;
        pcr.rate = Keyed::Const(1.0);
        let mut g = Generator::new(Arc::new(pcr), Vec3::ZERO, 1);
        g.max_age = Some(10.0);
        for _ in 0..10 {
            assert!(g.update(1.0, &mut |_| {}));
        }
        let scale_before = g.particles.iter().map(|p| p.scale).fold(0.0, f32::max);
        
        assert!(scale_before > 1.0, "the authored curve is growing: {scale_before}");
        g.update(1.0, &mut |_| {});
        let scale_after = g.particles.iter().map(|p| p.scale).fold(0.0, f32::max);
        
        assert!((scale_after - scale_before).abs() < 0.02,
            "the fade must not inflate: {scale_before} -> {scale_after}");
        assert!(g.particles.iter().any(|p| p.fade < 1.0), "the stagger starts some fades");
        let mut steps = 0;
        while g.update(1.0, &mut |_| {}) {
            steps += 1;
            assert!(steps < (FADE_FRAMES + STAGGER_FRAMES) as i32 + 2, "the fade must finish");
        }
    }
}
