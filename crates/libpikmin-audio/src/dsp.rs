//! DSP voices: allocation and priority stealing (dspdriver.c), and the mixer the DSP microcode
//! runs, which isn't in the decomp. The mixer here resamples each voice by `pitch / 4096`
//! with linear interpolation and sums it into the buses the channel's `busRouting` names (1 =
//! left, 2 = right; 3-6 are effect sends, not emulated), ramping levels across each subframe,
//! then scales by `DSP_MIXERLEVEL / 4096`.

use crate::calc::SUBFRAME;
use crate::channel::event;
use crate::engine::*;

impl Engine {
    /// `AllocDSPchannel(0, ...)`: the first free voice.
    pub(crate) fn alloc_voice(&mut self, owner: u16) -> Option<u8> {
        let i = self.voices.iter().position(|v| v.alloc == 0)?;
        let v = &mut self.voices[i];
        *v = Voice {
            alloc: 1,
            owner: Some(owner),
            prio: 1,
            ..Voice::default()
        };
        Some(i as u8)
    }

    /// `DeAllocDSPchannel`
    pub(crate) fn dealloc_voice(&mut self, i: u8, owner: u16) {
        let v = &mut self.voices[i as usize];
        if v.owner != Some(owner) {
            return;
        }
        v.alloc = 0;
        v.prio = 0;
        v.has_callback = false;
        v.owner = None;
    }

    /// `ForceStopDSPchannel`: the DSP fades the voice out over the next subframe.
    pub(crate) fn force_stop_voice(&mut self, i: u8) -> bool {
        let v = &mut self.voices[i as usize];
        if v.alloc == 4 || !v.enabled {
            return false;
        }
        v.end_requested = true;
        v.alloc = 4;
        true
    }

    /// `GetLowerDSPchannel`: a free voice, else the lowest-priority one (oldest on ties).
    fn lower_voice(&mut self) -> usize {
        let mut max = 255u8;
        let mut id = 0;
        let mut age = 0u32;
        for i in 0..VOICES {
            let v = &self.voices[i];
            if v.alloc == 4 {
                continue;
            }
            if v.alloc == 0 {
                self.voices[i].prio = 0;
                return i;
            }
            if v.has_callback
                && v.prio <= max
                && (max != v.prio || (age != 0 && (v.age >= age || v.age == 0)))
            {
                age = v.age;
                id = i;
                max = v.prio;
            }
        }
        id
    }

    /// `BreakLowerDSPchannel`: stop a voice of at most `prio` so a new note can have it.
    pub(crate) fn break_lower(&mut self, prio: u8) -> bool {
        let i = self.lower_voice();
        if self.voices[i].prio > prio || self.voices[i].alloc == 0 {
            return false;
        }
        if self.voices[i].has_callback {
            self.voices[i].callback_timer = self.common_callback(i as u8, event::STATE_CHANGE);
            self.force_stop_voice(i as u8);
            self.voices[i].alloc = 4;
        }
        self.force_stop_voice(i as u8);
        true
    }

    /// `UpdateDSPchannelAll`
    pub fn update_voices(&mut self) {
        for i in 0..VOICES {
            if self.voices[i].alloc == 0 {
                continue;
            }
            if self.voices[i].done {
                if self.voices[i].has_callback {
                    self.voices[i].callback_timer =
                        self.common_callback(i as u8, event::COMPLETION);
                }
                self.voices[i].done = false;
                self.voices[i].enabled = false;
                if self.voices[i].alloc == 0 {
                    continue;
                }
            }
            if !self.voices[i].end_requested {
                self.voices[i].age += 1;
                if self.voices[i].age == self.voices[i].release_time as u32
                    && self.voices[i].has_callback
                {
                    self.voices[i].callback_timer = self.common_callback(i as u8, event::PRIORITY);
                }
            }
            if self.voices[i].has_callback {
                let v = &mut self.voices[i];
                if v.callback_timer != 0 {
                    v.callback_timer -= 1;
                }
                if v.callback_timer == 0 {
                    let t = self.common_callback(i as u8, event::NORMAL);
                    self.voices[i].callback_timer = t;
                    if t == 0 {
                        self.voices[i].done = false;
                        self.voices[i].enabled = false;
                        self.entry_wait_channel(true);
                    }
                }
            }
        }
        self.entry_check_wait();
    }

    /// Mixes one subframe into `out` (interleaved stereo, 80 frames).
    pub fn render_subframe(&mut self, out: &mut [f32; SUBFRAME * 2]) {
        out.fill(0.0);
        let gain = self.dsp_level;
        for v in self.voices.iter_mut() {
            if !v.enabled || v.done {
                continue;
            }
            let fade_out = v.end_requested;
            let step = v.pitch as f64 / 4096.0;
            let mut levels = [0f32; 2];
            let mut targets = [0f32; 2];
            for slot in 0..6 {
                if let b @ 1..=2 = v.bus[slot] {
                    levels[b as usize - 1] += v.level[slot];
                    targets[b as usize - 1] += if fade_out { 0.0 } else { v.target[slot] };
                }
            }
            let mut finished = false;
            for n in 0..SUBFRAME {
                let s = if v.paused {
                    0.0
                } else {
                    match &v.source {
                        VoiceSource::Wave {
                            pcm,
                            looping,
                            loop_start,
                            end,
                        } => {
                            let end = (*end as f64).min(pcm.len() as f64);
                            if v.pos >= end {
                                if *looping && (*loop_start as f64) < end {
                                    v.pos = *loop_start as f64
                                        + (v.pos - end) % (end - *loop_start as f64);
                                } else {
                                    finished = true;
                                    break;
                                }
                            }
                            let i = v.pos as usize;
                            let f = (v.pos - i as f64) as f32;
                            let a = pcm[i] as f32;
                            let next = if i + 1 < end as usize {
                                pcm[i + 1]
                            } else if *looping {
                                pcm.get(*loop_start as usize).copied().unwrap_or(0)
                            } else {
                                0
                            } as f32;
                            v.pos += step;
                            a + (next - a) * f
                        }
                        VoiceSource::Osc(kind) => {
                            // 32 steps per cycle at key 60 = C5 (`Play_1shot_Osc` base pitch).
                            let ph = v.osc_phase.fract() as f32;
                            v.osc_phase += step / 32.0;
                            let s = match kind {
                                0 => (ph * std::f32::consts::TAU).sin(),
                                1 => {
                                    if ph < 0.5 {
                                        1.0
                                    } else {
                                        -1.0
                                    }
                                }
                                2 => 2.0 * ph - 1.0,
                                _ => 1.0 - 4.0 * (ph - 0.5).abs(),
                            };
                            s * 16384.0
                        }
                        VoiceSource::None => 0.0,
                    }
                };
                let k = n as f32 / SUBFRAME as f32;
                let l = levels[0] + (targets[0] - levels[0]) * k;
                let r = levels[1] + (targets[1] - levels[1]) * k;
                out[n * 2] += s * l * gain / 32768.0;
                out[n * 2 + 1] += s * r * gain / 32768.0;
            }
            for slot in 0..6 {
                v.level[slot] = if fade_out { 0.0 } else { v.target[slot] };
            }
            if finished || fade_out {
                v.done = true;
            }
        }
    }
}
