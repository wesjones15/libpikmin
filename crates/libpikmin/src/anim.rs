//! Animation key timings that gameplay depends on.
//!
//! Several Pikmin actions are timed by animation key events rather than code constants:
//! `PaniAnimator::checkConstantKey` (plugPikiNakata/panianimator.cpp) turns an AnimInfo's
//! info keys into listener events (type 0 = loop start, 1 = loop end, 2..5 = `KEY_Action0..3`).
//! Motions play at `mMotionSpeed` = 30 frames/s for everything but locomotion
//! (`Piki::updateWalkAnimation`, piki.cpp), from anim key 0 to anim key 8 (clamped).
//!
//! The keys live in `pikis/animMgr.bin` on the disc (`AnimMgr::read`, `AnimInfo::doread`,
//! plugPikiColin/animMgr.cpp). Motion → AnimInfo index comes from
//! `PaniPikiAnimator::createMotionTable` (panipikianimator.cpp).

use crate::params::ParmError;

/// `PIKIANIM_Attack` → AnimInfo 18 ("attack1.dca").
pub const ANIM_ATTACK: usize = 18;
/// `PIKIANIM_Kuttuku` → AnimInfo 71 ("kuttuku.dca").
pub const ANIM_KUTTUKU: usize = 71;

/// Frame timings for one animation, as gameplay sees them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimTiming {
    /// `KEY_Action0`: hit window opens.
    pub action0: f32,
    /// `KEY_Action1`: hit window closes.
    pub action1: f32,
    /// Loop start/end frames, if the animation loops.
    pub loop_range: Option<(f32, f32)>,
    /// Frame where a one-shot play finishes (`KEY_Finished`).
    pub end: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimTimings {
    /// Frames per second for these motions (`mMotionSpeed`).
    pub fps: f32,
    /// Ground swing, `ActJumpAttack` state 4.
    pub attack: AnimTiming,
    /// Latched on, `ActJumpAttack` state 5.
    pub kuttuku: AnimTiming,
}

impl Default for AnimTimings {
    /// Values decoded from the GPIE01_01 disc's `pikis/animMgr.bin`. Used when the user's
    /// file isn't available; prefer [`AnimTimings::from_anim_mgr_bin`].
    fn default() -> Self {
        Self {
            fps: 30.0,
            attack: AnimTiming {
                action0: 3.0,
                action1: 5.0,
                loop_range: None,
                end: 19.0,
            },
            kuttuku: AnimTiming {
                action0: 11.0,
                action1: 17.0,
                loop_range: Some((11.0, 30.0)),
                end: 34.0,
            },
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
            action0: self.frame_of_type(2)?,
            action1: self.frame_of_type(3).unwrap_or(f32::MAX),
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

impl AnimTimings {
    /// Parses `pikis/animMgr.bin` from the user's disc.
    pub fn from_anim_mgr_bin(data: &[u8]) -> Result<Self, ParmError> {
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
                    let frame = r.i32()?;
                    let rest = r.take(4)?; // event type (i16), command id (u8), key type (u8)
                    info_keys.push((frame, rest[3]));
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

        let get = |i: usize| {
            infos
                .get(i)
                .and_then(Info::timing)
                .ok_or(ParmError::Truncated)
        };
        Ok(Self {
            fps: 30.0,
            attack: get(ANIM_ATTACK)?,
            kuttuku: get(ANIM_KUTTUKU)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_disc_matches_documented_defaults_if_present() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/pikis/animMgr.bin");
        let Ok(data) = std::fs::read(&path) else {
            eprintln!("skipping: {} not found", path.display());
            return;
        };
        assert_eq!(
            AnimTimings::from_anim_mgr_bin(&data).unwrap(),
            AnimTimings::default()
        );
    }
}
