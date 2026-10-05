//! Animation key timings that gameplay depends on.
//!
//! Many Pikmin actions are timed by animation key events rather than code constants:
//! `PaniAnimator::checkConstantKey` (plugPikiNakata/panianimator.cpp) turns an AnimInfo's
//! info keys into listener events (type 0 = loop start, 1 = loop end, 2..5 = `KEY_Action0..3`).
//! Motions play at `mMotionSpeed` = 30 frames/s for everything but locomotion
//! (`Piki::updateWalkAnimation`, piki.cpp), from anim key 0 to anim key 8 (clamped).
//!
//! The keys live in AnimMgr files on the disc (`AnimMgr::read`, `AnimInfo::doread`,
//! plugPikiColin/animMgr.cpp): `pikis/animMgr.bin` (Pikmin and captains share it) and
//! `objects/goal/anims.bin` (the Onion). Motion → AnimInfo index comes from
//! `PaniPikiAnimator::createMotionTable` (panipikianimator.cpp) and `GoalAI` (itemAI.cpp).

use crate::params::ParmError;

/// `PIKIANIM_Dead`, `Dead2`, `Dead3` (7, 8, 9) → 20, 42, 48: one is picked at random.
pub const ANIM_DEAD: [usize; 3] = [20, 42, 48];
/// `PIKIANIM_Damage` (10) → 21 "damage.dca": the captain is invulnerable while it plays.
pub const ANIM_DAMAGE: usize = 21;
/// `PIKIANIM_Punch` (25) → 94 "punch.dca": the captain's punch.
pub const ANIM_PUNCH: usize = 94;
/// `PIKIANIM_Nuku_Fast` (82) → 43 "nuku3.dca": chained plucks (`mFastPluckKeyTaps > 0`).
pub const ANIM_NUKU_FAST: usize = 43;
/// `PIKIANIM_Nuku` (5) → 28 "nuku.dca", captain plucking.
pub const ANIM_NUKU: usize = 28;
/// `PIKIANIM_Attack` (23) → 18 "attack1.dca".
pub const ANIM_ATTACK: usize = 18;
/// `PIKIANIM_PickLoop` (46) → 68 "pickloop.dca", lifting a pellet.
pub const ANIM_PICKLOOP: usize = 68;
/// `PIKIANIM_Kuttuku` (48) → 71 "kuttuku.dca".
pub const ANIM_KUTTUKU: usize = 71;
/// `PIKIANIM_Mizunomi` (47) → 67 "mizunomi.dca": drinking nectar (`PikiAbsorbState`).
pub const ANIM_MIZUNOMI: usize = 67;
/// `PIKIANIM_GrowUp1` / `GrowUp2` (17, 18) → 39, 40: blooming after nectar (`PikiGrowupState`).
pub const ANIM_GROW_UP: [usize; 2] = [39, 40];
/// Onion motion 0, "flowrot.dck": seeds are emitted at each loop end (`GoalAI`).
pub const ANIM_GOAL_FLOWROT: usize = 0;
/// Onion motion 2, "suction.dck": played when something is sucked in (`GOAL_Effect`).
pub const ANIM_GOAL_SUCTION: usize = 2;

/// Frame timings for one animation, as gameplay sees them. Missing keys are `f32::INFINITY`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimTiming {
    /// `KEY_Action0`
    pub action0: f32,
    /// `KEY_Action1`
    pub action1: f32,
    /// Loop start/end frames, if the animation loops.
    pub loop_range: Option<(f32, f32)>,
    /// Frame where a one-shot play finishes (`KEY_Finished`).
    pub end: f32,
}

impl AnimTiming {
    const fn new(action0: f32, action1: f32, loop_range: Option<(f32, f32)>, end: f32) -> Self {
        Self {
            action0,
            action1,
            loop_range,
            end,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimTimings {
    /// Frames per second for these motions (`mMotionSpeed`).
    pub fps: f32,
    /// Ground swing, `ActJumpAttack` state 4.
    pub attack: AnimTiming,
    /// Latched on, `ActJumpAttack` state 5.
    pub kuttuku: AnimTiming,
    /// Lifting a pellet: `KEY_Action0` sets `mIsLiftActionDone` (`ActTransport`).
    pub pickloop: AnimTiming,
    /// Captain plucking a sprout: `KEY_Action0` pulls it out (`NaviNukuState`).
    pub nuku: AnimTiming,
    /// Onion seed loop: one seed per loop end.
    pub goal_flowrot: AnimTiming,
    /// Onion suck-in reaction.
    pub goal_suction: AnimTiming,
    /// Lengths of the three death animations (`PikiDyingState`).
    pub dead: [f32; 3],
    /// Captain hit reaction (`Navi::finishDamage` at its end).
    pub damage: AnimTiming,
    /// Captain punch: hits from `KEY_Action0` to the end (`NaviAttackState`).
    pub punch: AnimTiming,
    /// Chained, faster plucking.
    pub nuku_fast: AnimTiming,
    /// Drinking nectar: drinks during the loop.
    pub mizunomi: AnimTiming,
    /// Blooming: `KEY_Action0` makes the flower. First bloom and later ones.
    pub grow_up1: AnimTiming,
    pub grow_up2: AnimTiming,
}

impl Default for AnimTimings {
    /// Values decoded from the GPIE01_01 disc. Used when the user's files aren't available;
    /// prefer the `apply_*` loaders.
    fn default() -> Self {
        const INF: f32 = f32::INFINITY;
        Self {
            fps: 30.0,
            attack: AnimTiming::new(3.0, 5.0, None, 19.0),
            kuttuku: AnimTiming::new(11.0, 17.0, Some((11.0, 30.0)), 34.0),
            pickloop: AnimTiming::new(10.0, 20.0, Some((13.0, 18.0)), 29.0),
            nuku: AnimTiming::new(23.0, 24.0, None, 39.0),
            goal_flowrot: AnimTiming::new(30.0, INF, Some((20.0, 40.0)), 58.0),
            goal_suction: AnimTiming::new(INF, INF, None, 48.0),
            dead: [104.0, 114.0, 129.0],
            damage: AnimTiming::new(INF, INF, None, 19.0),
            punch: AnimTiming::new(1.0, 4.0, None, 9.0),
            nuku_fast: AnimTiming::new(9.0, INF, None, 19.0),
            mizunomi: AnimTiming::new(INF, INF, Some((10.0, 30.0)), 39.0),
            grow_up1: AnimTiming::new(49.0, INF, None, 84.0),
            grow_up2: AnimTiming::new(32.0, INF, None, 69.0),
        }
    }
}

struct Info {
    anim_keys: Vec<i32>,
    /// (anim key index, key type)
    info_keys: Vec<(i32, u8)>,
}

impl Info {
    fn frame_of_type(&self, ty: u8) -> Option<f32> {
        let (idx, _) = self.info_keys.iter().find(|(_, t)| *t == ty)?;
        self.anim_keys.get(*idx as usize).map(|&f| f as f32)
    }

    fn timing(&self) -> Option<AnimTiming> {
        let end_idx = 8.min(self.anim_keys.len().checked_sub(1)?);
        Some(AnimTiming {
            action0: self.frame_of_type(2).unwrap_or(f32::INFINITY),
            action1: self.frame_of_type(3).unwrap_or(f32::INFINITY),
            loop_range: self.frame_of_type(0).zip(self.frame_of_type(1)),
            end: self.anim_keys[end_idx] as f32,
        })
    }
}

struct Reader<'a> {
    d: &'a [u8],
    o: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], ParmError> {
        let s = self.d.get(self.o..self.o + n).ok_or(ParmError::Truncated)?;
        self.o += n;
        Ok(s)
    }
    fn i32(&mut self) -> Result<i32, ParmError> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<(), ParmError> {
        let n = self.i32()?.max(0) as usize;
        self.take(n).map(|_| ())
    }
    /// Skips a Parameters block. `string_ids` are `Parm<String>`, which are length-prefixed
    /// regardless of the header's size byte.
    fn skip_params(&mut self, string_ids: &[&[u8; 3]]) -> Result<(), ParmError> {
        loop {
            let h = self.i32()?;
            if h == -1 {
                return Ok(());
            }
            let b = h.to_be_bytes();
            if string_ids.contains(&&[b[0], b[1], b[2]]) {
                self.string()?;
            } else {
                self.take(b[3] as usize)?;
            }
        }
    }
}

/// Reads every AnimInfo in an AnimMgr file.
fn parse_anim_mgr(data: &[u8]) -> Result<Vec<Info>, ParmError> {
    let mut r = Reader { d: data, o: 0 };
    let version = r.i32()?;
    let count = r.i32()?.max(0) as usize;
    r.skip_params(&[b"a01"])?;

    let mut infos = Vec::with_capacity(count);
    for _ in 0..count {
        r.string()?;
        r.skip_params(&[])?;
        let n = r.i32()?.max(0) as usize;
        let anim_keys = (0..n).map(|_| r.i32()).collect::<Result<Vec<_>, _>>()?;
        let mut info_keys = Vec::new();
        if version >= 1 {
            for _ in 0..r.i32()?.max(0) {
                let key = r.i32()?;
                let rest = r.take(4)?; // event type (i16), command id (u8), key type (u8)
                info_keys.push((key, rest[3]));
            }
        }
        for _ in 0..r.i32()?.max(0) {
            r.take(8)?;
        }
        infos.push(Info {
            anim_keys,
            info_keys,
        });
    }
    Ok(infos)
}

fn timing_at(infos: &[Info], idx: usize) -> Result<AnimTiming, ParmError> {
    infos
        .get(idx)
        .and_then(Info::timing)
        .ok_or(ParmError::Truncated)
}

impl AnimTimings {
    /// Applies `pikis/animMgr.bin` from the user's disc.
    pub fn apply_piki_anim_mgr(&mut self, data: &[u8]) -> Result<(), ParmError> {
        let infos = parse_anim_mgr(data)?;
        self.attack = timing_at(&infos, ANIM_ATTACK)?;
        self.kuttuku = timing_at(&infos, ANIM_KUTTUKU)?;
        self.pickloop = timing_at(&infos, ANIM_PICKLOOP)?;
        self.nuku = timing_at(&infos, ANIM_NUKU)?;
        self.damage = timing_at(&infos, ANIM_DAMAGE)?;
        self.punch = timing_at(&infos, ANIM_PUNCH)?;
        self.nuku_fast = timing_at(&infos, ANIM_NUKU_FAST)?;
        self.mizunomi = timing_at(&infos, ANIM_MIZUNOMI)?;
        self.grow_up1 = timing_at(&infos, ANIM_GROW_UP[0])?;
        self.grow_up2 = timing_at(&infos, ANIM_GROW_UP[1])?;
        for (slot, idx) in self.dead.iter_mut().zip(ANIM_DEAD) {
            *slot = timing_at(&infos, idx)?.end;
        }
        Ok(())
    }

    /// Applies `objects/goal/anims.bin` (the Onion) from the user's disc.
    pub fn apply_goal_anims(&mut self, data: &[u8]) -> Result<(), ParmError> {
        let infos = parse_anim_mgr(data)?;
        self.goal_flowrot = timing_at(&infos, ANIM_GOAL_FLOWROT)?;
        self.goal_suction = timing_at(&infos, ANIM_GOAL_SUCTION)?;
        Ok(())
    }
}

/// Nectar timings from `objects/water/anims.bin` and `objects/fl_water/anims.bin`.
pub fn nectar_timings(water: &[u8], fall: &[u8]) -> Result<crate::nectar::NectarTimings, ParmError> {
    let w = parse_anim_mgr(water)?;
    let f = parse_anim_mgr(fall)?;
    let end = |infos: &[Info], i: usize| -> Result<f32, ParmError> {
        infos
            .get(i)
            .and_then(|x| x.anim_keys.last())
            .map(|&k| k as f32)
            .ok_or(ParmError::Truncated)
    };
    let fall0 = f.first().and_then(Info::timing).ok_or(ParmError::Truncated)?;
    Ok(crate::nectar::NectarTimings {
        leave_end: end(&f, 1)?,
        fall_loop: fall0.loop_range.unwrap_or((19.0, 23.0)),
        fall_emit: fall0.action0,
        fall_end: end(&f, 0)?,
        wait_end: end(&w, 0)?,
        touch_end: end(&w, 1)?,
        appear_end: end(&w, 2)?,
        drain_end: end(&w, 3)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_disc_matches_documented_defaults_if_present() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let (Ok(piki), Ok(goal)) = (
            std::fs::read(root.join("pikis/animMgr.bin")),
            std::fs::read(root.join("objects/goal/anims.bin")),
        ) else {
            eprintln!(
                "skipping: assets/ not extracted yet (run the viewer once) under {}",
                root.display()
            );
            return;
        };
        let mut t = AnimTimings::default();
        t.apply_piki_anim_mgr(&piki).unwrap();
        t.apply_goal_anims(&goal).unwrap();
        assert_eq!(t, AnimTimings::default());
        if let (Ok(w), Ok(f)) = (
            std::fs::read(root.join("objects/water/anims.bin")),
            std::fs::read(root.join("objects/fl_water/anims.bin")),
        ) {
            assert_eq!(nectar_timings(&w, &f).unwrap(), crate::nectar::NectarTimings::default());
        }
    }
}
