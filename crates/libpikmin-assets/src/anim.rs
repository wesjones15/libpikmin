//! Skeletal animations: `.dca` (one sample per frame) and `.dck` (Hermite keys), the `.anm`
//! bundles that pack them, and `AnimMgr` files that name and time them.
//!
//! Formats: `AnimDca::read`, `AnimDck::read`, `DataChunk::read` (Animator.h),
//! `StdSystem::loadBundle` (sysCommon/stdSystem.cpp), `AnimMgr::read` / `AnimInfo::doread`
//! (plugPikiColin/animMgr.cpp). Sampling: `AnimData::extractSRT` (DCA) and
//! `AnimDck::extractSRT` with the file-static `extract` (shapeBase.cpp).

use glam::Vec3;

use crate::Error;
use crate::reader::{Reader, ReaderExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimKind {
    Dca,
    Dck,
}

/// One channel (`AnimParam`): `count` samples or keys at `offset` in its data block.
#[derive(Debug, Clone, Copy, Default)]
pub struct Channel {
    pub count: u32,
    pub offset: u32,
    /// DCK only: 0 = keys are (time, value, tangent), else (time, value, in, out).
    pub flags: u32,
}

#[derive(Debug, Clone)]
pub struct JointTrack {
    pub parent: i32,
    pub scale: [Channel; 3],
    pub rotation: [Channel; 3],
    pub translation: [Channel; 3],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Srt {
    pub scale: Vec3,
    pub rotation: Vec3,
    pub translation: Vec3,
}

#[derive(Debug, Clone)]
pub struct Anim {
    pub name: String,
    pub kind: AnimKind,
    pub frames: u32,
    scale: Vec<f32>,
    rotation: Vec<f32>,
    translation: Vec<f32>,
    pub joints: Vec<JointTrack>,
}

fn data_chunk(r: &mut Reader) -> Result<Vec<f32>, Error> {
    let n = r.count()?;
    (0..n).map(|_| Ok(r.f32()?)).collect()
}

impl Anim {
    pub fn parse(name: &str, kind: AnimKind, data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        let joint_count = r.count()?;
        let frames = r.u32()?;
        let scale = data_chunk(&mut r)?;
        let rotation = data_chunk(&mut r)?;
        let translation = data_chunk(&mut r)?;
        let channel = |r: &mut Reader| -> Result<Channel, Error> {
            Ok(Channel {
                count: r.u32()?,
                offset: r.u32()?,
                flags: if kind == AnimKind::Dck { r.u32()? } else { 0 },
            })
        };
        let mut joints = Vec::with_capacity(joint_count);
        for _ in 0..joint_count {
            let _group = r.i32()?;
            let parent = r.i32()?;
            let three = |r: &mut Reader| -> Result<[Channel; 3], Error> {
                Ok([channel(r)?, channel(r)?, channel(r)?])
            };
            let scale = three(&mut r)?;
            let rotation = three(&mut r)?;
            let translation = three(&mut r)?;
            joints.push(JointTrack {
                parent,
                scale,
                rotation,
                translation,
            });
        }
        Ok(Anim {
            name: name.to_string(),
            kind,
            frames,
            scale,
            rotation,
            translation,
            joints,
        })
    }

    /// The joint's local transform at `frame`, or `None` if the animation has no track for it.
    pub fn sample(&self, joint: usize, frame: f32) -> Option<Srt> {
        let t = self.joints.get(joint)?;
        let get = |block: &[f32], chans: &[Channel; 3], empty: f32| {
            Vec3::from_array(std::array::from_fn(|i| self.channel(block, chans[i], frame, empty)))
        };
        Some(Srt {
            scale: get(&self.scale, &t.scale, 1.0),
            rotation: get(&self.rotation, &t.rotation, 0.0),
            // `AnimDck::extractSRT` really defaults an empty translation channel to 1.0.
            translation: get(&self.translation, &t.translation, 1.0),
        })
    }

    fn channel(&self, block: &[f32], c: Channel, frame: f32, empty: f32) -> f32 {
        let at = |i: usize| block.get(i).copied().unwrap_or(0.0);
        match (self.kind, c.count) {
            (_, 0) => empty,
            (AnimKind::Dca, n) => {
                let i = (frame.max(0.0) as u32).min(n - 1);
                at((c.offset + i) as usize)
            }
            (AnimKind::Dck, 1) => at(c.offset as usize),
            (AnimKind::Dck, n) => {
                let stride = if c.flags == 0 { 3 } else { 4 };
                let base = c.offset as usize;
                // Find the key pair around `frame`; past the ends the last value holds.
                let pair = (0..n as usize - 1).find(|&k| {
                    let o = base + k * stride;
                    at(o) <= frame && at(o + stride) >= frame
                });
                let Some(k) = pair else {
                    return at(base + stride * (n as usize - 1) + 1);
                };
                let (s, e) = (base + k * stride, base + (k + 1) * stride);
                let (t0, v0, out0) = (at(s), at(s + 1), at(s + if stride == 3 { 2 } else { 3 }));
                let (t1, v1, in1) = (at(e), at(e + 1), at(e + 2));
                hermite(frame, t0, v0, out0, t1, v1, in1)
            }
        }
    }
}

/// `extract` in shapeBase.cpp: cubic Hermite with tangents in units per second at 30 fps.
fn hermite(frame: f32, t0: f32, v0: f32, m0: f32, t1: f32, v1: f32, m1: f32) -> f32 {
    libcommon::anim::hermite(frame, t0, v0, m0 / 30.0, t1, v1, m1 / 30.0)
}

/// Reads every animation in an `.anm` bundle, keyed by the path stored in the bundle.
/// Textures and binary entries are skipped.
pub fn parse_bundle(data: &[u8]) -> Result<Vec<Anim>, Error> {
    let mut r = Reader::new(data);
    let n = r.count()?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let kind = r.u32()?;
        let size = r.u32()? as usize;
        let path = r.string()?;
        let body = r.take(size)?;
        match kind {
            2 => out.push(Anim::parse(&path, AnimKind::Dca, body)?),
            3 => out.push(Anim::parse(&path, AnimKind::Dck, body)?),
            _ => {}
        }
    }
    Ok(out)
}

/// One `AnimInfo` from an AnimMgr file.
#[derive(Debug, Clone, Default)]
pub struct AnimInfo {
    /// File name relative to [`AnimMgr::base_path`].
    pub name: String,
    pub flags: i32,
    /// Playback rate in frames per second (`spd`, default 30).
    pub speed: f32,
    /// Anim key frames; play ranges run between two of these.
    pub keys: Vec<i32>,
    /// (anim key index, key type): 0/1 = loop start/end, 2..5 = `KEY_Action0..3`.
    pub info_keys: Vec<(i32, u8)>,
    /// Frame events (`AnimInfo::mEventKeys`, animMgr.cpp).
    pub event_keys: Vec<EventKey>,
}

/// An event at an animation frame. `PaniAnimator::checkEventKeys` (panianimator.cpp) fires
/// it when playback passes `frame`: `event` 0 (`ANIMEVENT_Notify`) plays sound `value` from
/// the creature's sound table (`KEY_PlaySound`), 1 (`ANIMEVENT_Action`) an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventKey {
    pub frame: i32,
    pub event: i16,
    pub value: u8,
}

impl AnimInfo {
    fn key_of_type(&self, ty: u8) -> Option<f32> {
        let (k, _) = self.info_keys.iter().find(|(_, t)| *t == ty)?;
        self.keys.get(*k as usize).map(|&f| f as f32)
    }

    /// Loop range from the info keys, if any.
    pub fn loop_range(&self) -> Option<(f32, f32)> {
        self.key_of_type(0).zip(self.key_of_type(1))
    }

    /// Frame of `KEY_Action0`.
    pub fn action0(&self) -> Option<f32> {
        self.key_of_type(2)
    }
}

#[derive(Debug, Clone)]
pub struct AnimMgr {
    pub base_path: String,
    pub infos: Vec<AnimInfo>,
}

/// Reads a `Parameters` block (plugPikiColin/parameters.cpp), handing each entry to `f`.
/// `Parm<String>` entries are length-prefixed whatever their header says.
pub(crate) fn read_params(
    r: &mut Reader,
    string_ids: &[&[u8; 3]],
    mut f: impl FnMut(&[u8; 3], ParmValue),
) -> Result<(), Error> {
    loop {
        let h = r.i32()?;
        if h == -1 {
            return Ok(());
        }
        let b = h.to_be_bytes();
        let id = [b[0], b[1], b[2]];
        if string_ids.contains(&&id) {
            f(&id, ParmValue::Str(r.string()?));
        } else {
            let raw = r.take(b[3] as usize)?;
            let word = (raw.len() == 4).then(|| u32::from_be_bytes(raw.try_into().unwrap()));
            f(&id, ParmValue::Word(word));
        }
    }
}

pub(crate) enum ParmValue {
    Str(String),
    Word(Option<u32>),
}

impl AnimMgr {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        let version = r.i32()?;
        let count = r.count()?;
        let mut base_path = String::new();
        read_params(&mut r, &[b"a01"], |id, v| {
            if let (b"a01", ParmValue::Str(s)) = (id, v) {
                base_path = s;
            }
        })?;
        let mut infos = Vec::with_capacity(count);
        for _ in 0..count {
            let name = r.string()?;
            let (mut flags, mut speed) = (2, 30.0);
            read_params(&mut r, &[], |id, v| match (id, v) {
                (b"p00", ParmValue::Word(Some(w))) => flags = w as i32,
                (b"spd", ParmValue::Word(Some(w))) => speed = f32::from_bits(w),
                _ => {}
            })?;
            let n = r.count()?;
            let keys = (0..n).map(|_| r.i32()).collect::<Result<_, _>>()?;
            let mut info_keys = Vec::new();
            if version >= 1 {
                for _ in 0..r.count()? {
                    let frame = r.i32()?;
                    let rest = r.take(4)?;
                    info_keys.push((frame, rest[3]));
                }
            }
            let mut event_keys = Vec::new();
            for _ in 0..r.count()? {
                let frame = r.i32()?;
                let mut event = r.i16()?;
                let _cmd = r.u8()?;
                let value = r.u8()?;
                // `AnimInfo::read`: unknown event types become `ANIMEVENT_Notify`.
                if event >= 3 {
                    event = 0;
                }
                event_keys.push(EventKey { frame, event, value });
            }
            infos.push(AnimInfo {
                name,
                flags,
                speed,
                keys,
                info_keys,
                event_keys,
            });
        }
        Ok(AnimMgr { base_path, infos })
    }
}
