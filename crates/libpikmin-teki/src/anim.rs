//! An enemy's animation playback: `PaniAnimator` (panianimator.cpp) over a `.key` motion. A motion plays
//! its anim key frames from the first to the last at its speed; a loop-start/loop-end key pair sends the
//! counter back each pass until the motion is told to finish; the action keys and the finish are events the
//! strategies read (`BTeki::animationKeyUpdated`).

use crate::info::MotionData;

/// Key events (`PaniAnimKeyEvent`), as bits of [`Animator::keys`].
pub mod key {
    pub const LOOP_START: u8 = 1 << 0;
    pub const LOOP_END: u8 = 1 << 1;
    pub const ACTION0: u8 = 1 << 2;
    pub const ACTION1: u8 = 1 << 3;
    pub const ACTION2: u8 = 1 << 4;
    pub const ACTION3: u8 = 1 << 5;
    pub const FINISHED: u8 = 1 << 6;
}

/// A sound or effect an animation event asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// `KEY_PlaySound`: index into the kind's sound table (`BTeki::playTableSound`).
    Sound(u8),
    /// `KEY_PlayEffect`: the kind's own effect id (`BTeki::createTekiEffect`).
    Effect(u8),
}

#[derive(Debug, Clone, Default)]
pub struct Animator {
    /// The motion playing (a `.key` index), or -1 before the first.
    pub motion: i32,
    /// The animation frame (`mAnimationCounter`).
    pub counter: f32,
    /// Next info key to pass; negative once the motion has finished (`isFinishing`).
    cur_key: i32,
    prev_key: i32,
    /// `finishMotion` was called: loops stop looping.
    finished: bool,
    start_frame: f32,
    end_frame: f32,
    /// Key events passed since the strategy last looked, as [`key`] bits.
    pub keys: u8,
}

impl Animator {
    pub fn new() -> Self {
        Animator { motion: -1, cur_key: -1, ..Default::default() }
    }

    /// `PaniAnimator::startMotion`: from the motion's first key to its last.
    pub fn start(&mut self, motion: usize, data: &[MotionData]) {
        let Some(m) = data.get(motion) else { return };
        self.motion = motion as i32;
        self.cur_key = 0;
        self.prev_key = 0;
        self.finished = false;
        self.start_frame = m.keys.first().copied().unwrap_or(0.0);
        self.end_frame = m.keys.last().copied().unwrap_or(0.0);
        self.counter = self.start_frame;
        self.keys = 0;
    }

    /// `PaniAnimator::finishMotion`: stop looping and play out to the end.
    pub fn finish(&mut self, data: &[MotionData]) {
        if self.motion < 0 {
            return;
        }
        self.finished = true;
        let max = data.get(self.motion as usize).map_or(0.0, |m| m.frames);
        if self.counter >= max {
            self.counter = max;
            self.finish_animation();
        }
    }

    fn finish_animation(&mut self) {
        self.cur_key = -1;
        self.keys |= key::FINISHED;
    }

    /// `PaniAnimator::isFinishing`: the motion has reached its end (`TaiMotionAction` is done).
    pub fn is_finishing(&self) -> bool {
        self.cur_key < 0
    }

    /// `PaniAnimator::isFinished`: told to finish.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// The frame of the key of type `ty` (0 loop start .. 5 action 3), or 0.
    pub fn key_value(&self, ty: u8, data: &[MotionData]) -> f32 {
        let Some(m) = usize::try_from(self.motion).ok().and_then(|i| data.get(i)) else { return 0.0 };
        m.info_keys.iter().find(|(_, t)| *t == ty).and_then(|&(k, _)| m.keys.get(k)).copied().unwrap_or(0.0)
    }

    /// `PaniAnimator::animate`: advance `speed` frames a second for `dt` seconds, collecting key events.
    pub fn animate(&mut self, speed: f32, dt: f32, data: &[MotionData], out: &mut Vec<Event>) {
        let Some(m) = usize::try_from(self.motion).ok().and_then(|i| data.get(i)) else { return };
        if self.cur_key < 0 || self.start_frame > self.end_frame {
            return;
        }
        let step = speed * dt;
        self.counter += step;
        // checkConstantKeys
        while (self.cur_key as usize) < m.info_keys.len() {
            let (k, ty) = m.info_keys[self.cur_key as usize];
            let at = m.keys.get(k).copied().unwrap_or(0.0);
            if self.counter < at {
                break;
            }
            if !self.finished {
                if ty == 0 {
                    self.prev_key = self.cur_key;
                } else if ty == 1 {
                    let first = m.info_keys.get(self.prev_key as usize).and_then(|&(k, _)| m.keys.get(k)).copied().unwrap_or(0.0);
                    self.counter -= at - first;
                    self.cur_key = self.prev_key;
                }
            }
            self.keys |= match ty {
                0 => key::LOOP_START,
                1 => key::LOOP_END,
                2 => key::ACTION0,
                3 => key::ACTION1,
                4 => key::ACTION2,
                5 => key::ACTION3,
                _ => 0,
            };
            self.cur_key += 1;
        }
        // checkEventKeys: sound and effect keys passed this step.
        let (from, to) = (self.counter - step, self.counter);
        for &(frame, ev, value) in &m.events {
            if from <= frame && frame < to {
                out.push(if ev == 0 { Event::Sound(value) } else { Event::Effect(value) });
            }
        }
        if self.counter >= self.end_frame {
            self.counter = self.end_frame;
            self.finish_animation();
        }
    }

    pub fn clear_keys(&mut self) {
        self.keys = 0;
    }

    pub fn has(&self, bit: u8) -> bool {
        self.keys & bit != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn looping() -> Vec<MotionData> {
        vec![MotionData {
            keys: vec![0.0, 10.0, 20.0, 30.0],
            info_keys: vec![(1, 0), (2, 1), (3, 2)],
            events: vec![(5.0, 0, 3)],
            speed: 30.0,
            dynamic_speed: false,
            frames: 31.0,
        }]
    }

    #[test]
    fn a_loop_runs_between_its_keys_until_told_to_finish() {
        let data = looping();
        let mut a = Animator::new();
        a.start(0, &data);
        let mut ev = Vec::new();
        for _ in 0..30 {
            a.animate(30.0, 1.0 / 30.0, &data, &mut ev);
        }
        // A second of play at 30 fps passed the loop end and came back: still inside 10..20.
        assert!(a.counter >= 10.0 && a.counter < 20.5, "{}", a.counter);
        assert!(!a.is_finishing());
        assert!(ev.contains(&Event::Sound(3)));
        a.finish(&data);
        for _ in 0..60 {
            a.animate(30.0, 1.0 / 30.0, &data, &mut ev);
        }
        assert!(a.is_finishing() && a.has(key::FINISHED) && a.counter == 30.0);
    }

    #[test]
    fn action_keys_are_seen_once() {
        let data = looping();
        let mut a = Animator::new();
        a.start(0, &data);
        let mut ev = Vec::new();
        let mut seen = 0;
        a.finish(&data);
        for _ in 0..60 {
            a.animate(30.0, 1.0 / 30.0, &data, &mut ev);
            if a.has(key::ACTION0) {
                seen += 1;
            }
            a.clear_keys();
        }
        assert_eq!(seen, 1);
    }
}
