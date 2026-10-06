//! Playing one motion with its key events, like `PaniAnimator` (plugPikiNakata/panianimator.cpp).
//!
//! A motion plays from its first anim key to its last (`startMotion` →
//! `startAnim(ANIMSTATE_OneShot, id, 0, 8)`, the end index clamped to the last key). Passing a
//! loop-end info key jumps back by the loop's length (`checkConstantKey`); reaching the last key
//! holds it and reports [`KeyEvent::Finished`] once (`finishAnimation`). Frame events fire when
//! a step crosses their frame (`checkEventKeys`).

use crate::anim::AnimInfo;

/// `PaniAnimKeyEvent` types as a listener sees them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    LoopStart,
    LoopEnd,
    /// `KEY_Action0..3` (info key types 2..5).
    Action(u8),
    Finished,
    /// `KEY_PlaySound` with the key's value.
    Sound(u8),
    /// `KEY_PlayEffect` with the key's value.
    Effect(u8),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaniPlayer {
    /// AnimMgr index playing (motion tables for ships and pellets map motion `i` to entry `i`).
    pub motion: usize,
    /// `mAnimationCounter`
    pub frame: f32,
    /// `mCurrentKeyIndex`: the next info key to check; `None` once finished.
    next_info: Option<usize>,
    /// `mPreviousKeyIndex`: the last loop-start info key passed.
    loop_start: usize,
    end_key: usize,
}

impl PaniPlayer {
    /// `PaniAnimator::startMotion`
    pub fn start(info: &AnimInfo, motion: usize) -> Self {
        PaniPlayer {
            motion,
            frame: info.keys.first().copied().unwrap_or(0) as f32,
            next_info: Some(0),
            loop_start: 0,
            end_key: info.keys.len().saturating_sub(1).min(8),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.next_info.is_none()
    }

    /// `UfoAnimator::setMotionLastFrame`: one frame before the last key.
    pub fn set_last_frame(&mut self, info: &AnimInfo) {
        self.frame = info.keys.last().copied().unwrap_or(0) as f32 - 1.0;
    }

    /// `PaniAnimator::animate` by `step` frames (the speed times the frame time).
    pub fn animate(&mut self, info: &AnimInfo, step: f32, events: &mut Vec<KeyEvent>) {
        let Some(_) = self.next_info else { return };
        let key = |i: usize| info.keys.get(i).copied().unwrap_or(0) as f32;
        let (start, end) = (key(0), key(self.end_key));
        if start > end {
            return;
        }
        self.frame += step;
        self.check_constant_keys(info, events);
        let (from, to) = (self.frame - step, self.frame);
        for e in &info.event_keys {
            let f = e.frame as f32;
            if from <= f && f < to {
                events.push(match e.event {
                    0 => KeyEvent::Sound(e.value),
                    1 => KeyEvent::Effect(e.value),
                    _ => KeyEvent::Finished,
                });
            }
        }
        if self.frame >= end {
            self.frame = end;
            self.next_info = None;
            events.push(KeyEvent::Finished);
        }
    }

    /// `checkConstantKeys` / `checkConstantKey`
    fn check_constant_keys(&mut self, info: &AnimInfo, events: &mut Vec<KeyEvent>) {
        let value = |i: usize| {
            let k = info.info_keys.get(i).map_or(0, |&(k, _)| k.max(0) as usize);
            info.keys.get(k).copied().unwrap_or(0) as f32
        };
        let Some(mut i) = self.next_info else { return };
        while i < info.info_keys.len() {
            if self.frame < value(i) {
                break;
            }
            let ty = info.info_keys[i].1;
            match ty {
                0 => {
                    self.loop_start = i;
                    events.push(KeyEvent::LoopStart);
                }
                1 => {
                    self.frame -= value(i) - value(self.loop_start);
                    i = self.loop_start;
                    events.push(KeyEvent::LoopEnd);
                }
                2..=5 => events.push(KeyEvent::Action(ty - 2)),
                _ => {}
            }
            i += 1;
        }
        self.next_info = Some(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::EventKey;

    fn info(keys: &[i32], info_keys: &[(i32, u8)], events: &[(i32, i16, u8)]) -> AnimInfo {
        AnimInfo {
            name: String::new(),
            flags: 0,
            speed: 30.0,
            keys: keys.to_vec(),
            info_keys: info_keys.to_vec(),
            event_keys: events.iter().map(|&(frame, event, value)| EventKey { frame, event, value }).collect(),
        }
    }

    fn run(p: &mut PaniPlayer, i: &AnimInfo, steps: usize) -> Vec<KeyEvent> {
        let mut ev = Vec::new();
        for _ in 0..steps {
            p.animate(i, 1.0, &mut ev);
        }
        ev
    }

    #[test]
    fn one_shot_holds_the_last_key_and_finishes_once() {
        let i = info(&[0, 14, 94], &[(1, 2)], &[(30, 1, 2)]);
        let mut p = PaniPlayer::start(&i, 0);
        let ev = run(&mut p, &i, 14);
        assert_eq!(ev, vec![KeyEvent::Action(0)]);
        let ev = run(&mut p, &i, 100);
        assert_eq!(ev, vec![KeyEvent::Effect(2), KeyEvent::Finished]);
        assert_eq!(p.frame, 94.0);
        assert!(p.is_finished());
    }

    #[test]
    fn loops_between_the_loop_keys() {
        let i = info(&[0, 10, 30, 39], &[(1, 0), (2, 1)], &[]);
        let mut p = PaniPlayer::start(&i, 0);
        let ev = run(&mut p, &i, 30);
        assert_eq!(ev, vec![KeyEvent::LoopStart, KeyEvent::LoopEnd]);
        assert_eq!(p.frame, 10.0);
        // Jumping back skips past the loop start, so only the end fires from now on.
        let ev = run(&mut p, &i, 20);
        assert_eq!(ev, vec![KeyEvent::LoopEnd]);
        assert_eq!(p.frame, 10.0);
        assert!(!p.is_finished());
    }
}
