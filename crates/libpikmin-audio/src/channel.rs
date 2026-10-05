//! Logical channels: starting notes from instruments and percussion (oneshot.c), their
//! per-subframe envelope update and mixer levels (driverinterface.c
//! `CommonCallbackLogicalChannel`, `UpdateEffecterParam`), and the queue of channels waiting
//! for a DSP voice.

use std::sync::Arc;

use libpikmin_assets::sound::bank::{self, Inst, vmap_for};

use crate::calc::{DAC_RATE, c5base, sinf3};
use crate::engine::*;
use crate::osc::{Osc, osc_to_ofs, presets};

/// `polys_table` (oneshot.c): polyphony limits by the instrument flag's low nibble.
const POLYS: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32];

/// `calc_sw_table` (driverinterface.c): for each pan calculation type, how the (note, effect,
/// manager) values combine: 0 ignore, 1 add, 2 add weighted by the power distribution.
const CALC_SW: [u8; 81] = [
    0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 2, 0, 1, 0, 0, 2, 1, 0, 2, 2, 1, 0, 0, 1, 0,
    1, 1, 0, 2, 1, 1, 0, 1, 1, 1, 1, 1, 2, 1, 2, 0, 1, 2, 1, 1, 2, 2, 1, 0, 0, 2, 0, 1, 2, 0, 2, 2,
    1, 0, 2, 1, 1, 2, 1, 2, 2, 2, 0, 2, 2, 1, 2, 2, 2,
];

/// `DSPChannelCallbackEvent`
pub mod event {
    pub const NORMAL: u32 = 0;
    pub const COMPLETION: u32 = 2;
    pub const STATE_CHANGE: u32 = 3;
    pub const PRIORITY: u32 = 4;
}

/// `PanCalc`
fn pan_calc(m1: &[f32; 3], m2: &[f32; 3], kind: u8) -> f32 {
    let k = (kind as usize).min(26) * 3;
    let mut v = 0.0;
    for i in 0..3 {
        match CALC_SW[k + i] {
            1 => v += m1[i],
            2 => v += m1[i] * m2[i],
            _ => {}
        }
    }
    v
}

/// `Bank_SenseToOfs`
fn sense_to_ofs(s: &bank::Sense, input: u8) -> f32 {
    let x = input as f32;
    if s.threshold == 127 || s.threshold == 0 {
        return s.min + x * (s.max - s.min) / 127.0;
    }
    if input < s.threshold {
        return s.min + (1.0 - s.min) * (x / s.threshold as f32);
    }
    let a = (input - s.threshold) as f32;
    let b = (127 - s.threshold) as f32;
    1.0 + (s.max - 1.0) * (a / b)
}

/// Converts a bank oscillator, sharing one table when attack and release are the same data
/// (the bank points both at one table for LFOs; `Bank_OscToOfs` compares the pointers).
fn bank_osc(o: &bank::Osc) -> Osc {
    let mut osc = Osc::from_bank(o);
    if o.attack.is_some() && o.attack == o.release {
        osc.release = osc.attack.clone();
    }
    osc
}

impl Engine {
    // ------------------------------------------------------------ lists

    fn list_mut(&mut self, mgr: MgrId, kind: ListKind) -> &mut Vec<u16> {
        let m = self.mgr_mut(mgr);
        match kind {
            ListKind::Active => &mut m.active,
            ListKind::Releasing => &mut m.releasing,
            ListKind::Waiting => &mut m.waiting,
        }
    }

    /// `List_CutChannel`: false if it wasn't in a list.
    fn list_cut(&mut self, c: u16) -> bool {
        let Some(kind) = self.channels[c as usize].list.take() else {
            return false;
        };
        let mgr = self.channels[c as usize].mgr;
        let l = self.list_mut(mgr, kind);
        if let Some(i) = l.iter().position(|&x| x == c) {
            l.remove(i);
            true
        } else {
            false
        }
    }

    /// `List_AddChannelTail`
    fn list_add_tail(&mut self, mgr: MgrId, kind: ListKind, c: u16) {
        self.channels[c as usize].list = Some(kind);
        self.channels[c as usize].mgr = mgr;
        self.list_mut(mgr, kind).push(c);
    }

    /// Back to the global pool (`FixReleaseChannel`, or a manager's free list).
    fn free_channel(&mut self, c: u16) {
        let ch = &mut self.channels[c as usize];
        if !ch.in_use {
            return;
        }
        ch.in_use = false;
        ch.list = None;
        let mgr = ch.mgr;
        let m = self.mgr_mut(mgr);
        m.alloc_count = m.alloc_count.saturating_sub(1);
        self.free_channels.push(c);
    }

    /// `FixReleaseChannelAll`: a closing root track hands its playing channels to the global
    /// manager (they finish as they are) and drops the waiting ones.
    pub(crate) fn release_channels_to_global(&mut self, from: MgrId) {
        for kind in [ListKind::Active, ListKind::Releasing] {
            let list = std::mem::take(self.list_mut(from, kind));
            for c in list {
                self.channels[c as usize].list = None;
                self.list_add_front(MgrId::Global, kind, c);
            }
        }
        let waiting = std::mem::take(&mut self.mgr_mut(from).waiting);
        for c in waiting {
            self.cancel_wait(c);
            self.channels[c as usize].list = None;
            self.channels[c as usize].mgr = MgrId::Global;
            self.free_channel(c);
        }
        self.mgr_mut(from).alloc_count = 0;
    }

    /// `FixMoveChannelAll`: a closing child track hands its channels to its parent.
    pub(crate) fn move_channels(&mut self, from: MgrId, to: MgrId) {
        for kind in [ListKind::Active, ListKind::Releasing, ListKind::Waiting] {
            let list = std::mem::take(self.list_mut(from, kind));
            for c in list {
                self.channels[c as usize].list = None;
                self.list_add_front(to, kind, c);
            }
        }
        let n = std::mem::take(&mut self.mgr_mut(from).alloc_count);
        self.mgr_mut(to).alloc_count += n;
    }

    fn list_add_front(&mut self, mgr: MgrId, kind: ListKind, c: u16) {
        self.channels[c as usize].list = Some(kind);
        self.channels[c as usize].mgr = mgr;
        self.list_mut(mgr, kind).insert(0, c);
    }

    // ------------------------------------------------------------ starting notes

    /// `__Oneshot_GetLogicalChannel` + `Channel_Init`: a channel from the global pool for
    /// `mgr` (`FixAllocChannel(jcs, 1)`).
    fn get_logical_channel(
        &mut self,
        mgr: MgrId,
        wave: Option<(usize, usize, usize)>,
    ) -> Option<u16> {
        let c = self.free_channels.pop()?;
        self.mgr_mut(mgr).alloc_count += 1;
        let m = self.mgr(mgr).clone();
        let ch = &mut self.channels[c as usize];
        let id = ch.id.wrapping_add(1).max(1);
        *ch = Channel {
            id,
            in_use: true,
            mgr,
            bus: m.bus_connect,
            priority: m.channel_priority,
            release_time: m.release_time,
            pan_calc: m.pan_calc,
            wave,
            kind: 0,
            ..Channel::default()
        };
        self.update_pan_power(c, 1.0, 1.0, 1.0);
        Some(c)
    }

    /// The wave a velocity map names, through `WaveidToWavegroup` and `GetSoundHandle`.
    fn find_wave(&self, wave_id: u32, fallback_wsys: usize) -> Option<(usize, usize, usize)> {
        let virt = (wave_id >> 16) as u16;
        let wsys = if virt == 0xFFFF {
            fallback_wsys
        } else {
            self.data.bank.wsys_physical(virt)?
        };
        let ws = self.data.bank.wsys.get(wsys)?.as_ref()?;
        let scene = self.wave_scene.get(wsys).copied().unwrap_or(0);
        let (a, w) = ws.find_wave(scene, wave_id as u16)?;
        Some((wsys, a, w))
    }

    fn wave_info(&self, w: (usize, usize, usize)) -> bank::Wave {
        *self.data.bank.wsys[w.0].as_ref().unwrap().wave((w.1, w.2))
    }

    fn inst_osc(&mut self, bank: usize, program: u8, slot: usize, inst: &Inst) -> Option<Osc> {
        let o = inst.oscs[slot].as_ref()?;
        Some(
            self.inst_oscs
                .entry((bank, program, slot))
                .or_insert_with(|| bank_osc(o))
                .clone(),
        )
    }

    /// `__DoEffect`: a sensor or random effect at note-on.
    fn do_effect(&mut self, c: u16, id: u8, v: f32) {
        let ch = &mut self.channels[c as usize];
        match id {
            0 => ch.current_volume *= v,
            1 => ch.current_pitch *= v,
            2 => ch.pan[1][1] = v,
            3 => ch.pan[2][1] = v,
            4 => ch.pan[3][1] = v,
            _ => {}
        }
    }

    /// `DoEffectOsc`
    fn do_effect_osc(&mut self, c: u16, mode: u8, v: f32) {
        let ch = &mut self.channels[c as usize];
        match mode {
            0 => ch.volume_mod *= v,
            1 => ch.pitch_mod *= v,
            2 => ch.pan[1][1] = v,
            3 => ch.pan[2][1] = v,
            4 => ch.pan[3][1] = v,
            _ => {}
        }
    }

    fn reset_effect_mods(&mut self, c: u16) {
        let ch = &mut self.channels[c as usize];
        ch.pitch_mod = 1.0;
        ch.volume_mod = 1.0;
        ch.pan[1][1] = 0.5;
        ch.pan[2][1] = 0.0;
        ch.pan[3][1] = 0.0;
    }

    fn clamp_init_pan(&mut self, c: u16) {
        let ch = &mut self.channels[c as usize];
        for i in 1..3 {
            ch.pan[i][0] = ch.pan[i][0].clamp(0.0, 1.0);
        }
    }

    /// The channel's current oscillator definition in a slot.
    pub(crate) fn channel_osc(&self, c: u16, slot: usize) -> Option<Osc> {
        match self.channels[c as usize].oscs.get(slot)?.as_ref()? {
            OscRef::Own(o) => Some(o.clone()),
            OscRef::Track {
                track,
                index,
                generation,
                copy,
            } => {
                let tr = &self.tracks[*track];
                Some(if tr.generation == *generation {
                    tr.oscs[*index].clone()
                } else {
                    copy.clone()
                })
            }
        }
    }

    /// Runs one oscillator slot for a subframe; returns (mode, output, state after).
    fn run_osc(&mut self, c: u16, slot: usize) -> Option<(u8, f32, u8)> {
        let osc = self.channel_osc(c, slot)?;
        let buf = &mut self.channels[c as usize].bufs[slot];
        let v = osc_to_ofs(Some(&osc), buf);
        Some((osc.mode, v, buf.state))
    }

    /// `Effecter_Overwrite_1ShotD`
    pub(crate) fn effecter_overwrite(&mut self, c: u16, slot: usize, osc: OscRef) {
        if slot >= 4 {
            return;
        }
        let ch = &mut self.channels[c as usize];
        ch.bufs[slot].state = 1;
        ch.oscs[slot] = Some(osc);
        if let Some((mode, v, _)) = self.run_osc(c, slot) {
            self.do_effect_osc(c, mode, v);
        }
    }

    /// `EffecterInit`
    fn effecter_init(&mut self, c: u16, bank: usize, program: u8, inst: &Inst) {
        self.reset_effect_mods(c);
        for i in 0..2 {
            if let Some(s) = inst.sensors[i] {
                let trigger = match s.kind {
                    1 => self.channels[c as usize].velocity,
                    2 => self.channels[c as usize].note,
                    _ => 0,
                };
                self.do_effect(c, s.id, sense_to_ofs(&s, trigger));
            }
            if let Some(r) = inst.rands[i] {
                let v = self.rng.sf32() * r.range + r.value;
                self.do_effect(c, r.id, v);
            }
            match self.inst_osc(bank, program, i, inst) {
                Some(o) => {
                    let ch = &mut self.channels[c as usize];
                    ch.bufs[i].state = 1;
                    ch.oscs[i] = Some(OscRef::Own(o));
                    if let Some((mode, v, _)) = self.run_osc(c, i) {
                        self.do_effect_osc(c, mode, v);
                    }
                }
                None => self.channels[c as usize].oscs[i] = None,
            }
        }
        self.clamp_init_pan(c);
    }

    /// `Play_1shot`: an instrument note.
    pub(crate) fn play_1shot(
        &mut self,
        mgr: MgrId,
        bank: usize,
        program: u8,
        key: u8,
        vel: u8,
        duration: i32,
    ) -> Option<u16> {
        let data = self.data.clone();
        let inst = data.bank.banks.get(bank)?.as_ref()?.inst(program)?;
        let vmap = *vmap_for(&inst.key_region(key)?.vels, vel)?;
        let wave = self.find_wave(vmap.wave_id, bank)?;
        let w = self.wave_info(wave);
        let c = self.get_logical_channel(mgr, Some(wave))?;
        let pitch = c5base(key as i32 + 60 - w.key as i32);
        {
            let ch = &mut self.channels[c as usize];
            ch.velocity = vel;
            ch.note = key;
            ch.base_pitch = vmap.pitch * (w.sample_rate / DAC_RATE) * inst.pitch;
            ch.current_pitch = ch.base_pitch * pitch;
            ch.base_volume = vmap.volume * inst.volume;
            let v = vel as f32 / 127.0;
            ch.current_volume = v * v * ch.base_volume;
            ch.pan[1][0] = 0.5;
            ch.pan[2][0] = 0.0;
            ch.pan[3][0] = 0.0;
        }
        self.effecter_init(c, bank, program, inst);
        let mut flag = ((bank as u32) << 8 | program as u32) | inst.flag << 24;
        match inst.flag & 0xC0 {
            0xC0 => flag |= 0xFF_FFFF,
            0x80 => flag |= 0xFF,
            0x40 => {
                let k = inst
                    .keys
                    .iter()
                    .position(|k| key <= k.base_key)
                    .map_or(-1i32, |i| i as i32);
                flag |= (k as u32) << 16;
            }
            _ => {}
        }
        self.channels[c as usize].sound_id = 0;
        let mut pause = false;
        if !self.start_mono_poly_check(c, flag) {
            if inst.flag & 0x10 != 0 {
                self.free_channel(c);
                return None;
            }
            pause = true;
        }
        self.channels[c as usize].sound_id = flag;
        let r = self.play_start(mgr, c, duration);
        if pause && let Some(r) = r {
            self.wave_pause(r, true);
        }
        r
    }

    /// `Play_1shot_Perc`
    pub(crate) fn play_1shot_perc(
        &mut self,
        mgr: MgrId,
        bank: usize,
        program: u8,
        key: u8,
        vel: u8,
        duration: i32,
    ) -> Option<u16> {
        let data = self.data.clone();
        let perc = data.bank.banks.get(bank)?.as_ref()?.perc(program)?;
        let pk = perc.keys.get(key as usize)?.as_ref()?;
        let vmap = *vmap_for(&pk.vels, vel)?;
        let wave = self.find_wave(vmap.wave_id, bank)?;
        let w = self.wave_info(wave);
        let c = self.get_logical_channel(mgr, Some(wave))?;
        let release = perc.release[key as usize & 0x7F];
        {
            let ch = &mut self.channels[c as usize];
            ch.velocity = vel;
            ch.note = key;
            ch.base_pitch = (w.sample_rate / DAC_RATE) * vmap.pitch * pk.pitch;
            ch.current_pitch = ch.base_pitch;
            ch.base_volume = vmap.volume * pk.volume;
            let v = vel as f32 / 127.0;
            ch.current_volume = v * v * ch.base_volume;
            ch.pan[1][0] = perc.pan[key as usize & 0x7F] as f32 / 127.0;
            ch.pan[2][0] = 0.0;
            ch.pan[3][0] = 0.0;
        }
        // `EffecterInit_Perc`
        self.reset_effect_mods(c);
        for r in pk.rands.iter().flatten() {
            let v = self.rng.sf32() * r.range + r.value;
            self.do_effect(c, r.id, v);
        }
        {
            let ch = &mut self.channels[c as usize];
            ch.oscs = [Some(OscRef::Own(presets::perc_env())), None, None, None];
            ch.bufs[0].state = 1;
        }
        self.run_osc(c, 0);
        self.channels[c as usize].bufs[0].release_param = release;
        self.clamp_init_pan(c);
        self.channels[c as usize].sound_id = 0;
        self.play_start(mgr, c, duration)
    }

    /// `Play_1shot_Osc`: a DSP-generated waveform (programs 0xF0+).
    pub(crate) fn play_1shot_osc(
        &mut self,
        mgr: MgrId,
        program: u8,
        key: u8,
        vel: u8,
        duration: i32,
    ) -> Option<u16> {
        let c = self.get_logical_channel(mgr, None)?;
        {
            let ch = &mut self.channels[c as usize];
            ch.osc_type = (program - 0xF0) as u32;
            ch.kind = 2;
            ch.velocity = vel;
            ch.note = key;
            ch.base_pitch = 16736.016 / DAC_RATE;
            ch.current_pitch = ch.base_pitch * c5base(key as i32);
            ch.base_volume = 1.0;
            let v = vel as f32 / 127.0;
            ch.current_volume = v * v;
            ch.pan[1][0] = 0.5;
            ch.pan[2][0] = 0.0;
            ch.pan[3][0] = 0.0;
        }
        // `EffecterInit_Osc`
        self.reset_effect_mods(c);
        {
            let ch = &mut self.channels[c as usize];
            ch.oscs = [Some(OscRef::Own(presets::osc_env())), None, None, None];
            ch.bufs[0].state = 1;
        }
        self.run_osc(c, 0);
        self.channels[c as usize].sound_id = 0;
        self.play_start(mgr, c, duration)
    }

    /// `__Oneshot_Play_Start`
    fn play_start(&mut self, mgr: MgrId, c: u16, duration: i32) -> Option<u16> {
        let note_id = if duration == 0 { -1 } else { duration };
        {
            let ch = &mut self.channels[c as usize];
            ch.note_id = note_id;
            ch.last_note_played = note_id;
            ch.has_updater = true;
        }
        let voice = self.alloc_voice(c);
        self.channels[c as usize].voice = voice;
        let play = if voice.is_none() {
            self.check_logical_channel(c)
        } else {
            self.play_logical_channel(c)
        };
        if voice.is_none() && play {
            if self.add_wait(c) {
                self.list_add_tail(mgr, ListKind::Waiting, c);
                return Some(c);
            }
            self.free_channel(c);
            return None;
        }
        if !play {
            if let Some(v) = voice {
                self.dealloc_voice(v, c);
            }
            self.channels[c as usize].voice = None;
            self.free_channel(c);
            return None;
        }
        self.list_add_tail(mgr, ListKind::Active, c);
        Some(c)
    }

    /// `__Oneshot_WavePause`
    fn wave_pause(&mut self, c: u16, pause: bool) {
        let ch = &mut self.channels[c as usize];
        ch.pause = pause;
        ch.to_flush = 1;
    }

    fn channels_of(&self, mgr: MgrId, kind: ListKind) -> Vec<u16> {
        let m = self.mgr(mgr);
        match kind {
            ListKind::Active => m.active.clone(),
            ListKind::Releasing => m.releasing.clone(),
            ListKind::Waiting => m.waiting.clone(),
        }
    }

    /// `__Oneshot_StartMonoPolyCheck`: limits how many notes of one instrument play at once.
    fn start_mono_poly_check(&mut self, c: u16, id: u32) -> bool {
        let flag = (id >> 24) as u8;
        let poly = POLYS[(flag & 0xF) as usize];
        if poly == 0 {
            return true;
        }
        let mgr = self.channels[c as usize].mgr;
        let mut count = 0u8;
        for x in self.channels_of(mgr, ListKind::Active) {
            if self.channels[x as usize].sound_id != id {
                continue;
            }
            if flag & 0x20 != 0 {
                count += 1;
            } else {
                let ch = &mut self.channels[x as usize];
                ch.poly += 1;
                if ch.poly == poly {
                    if flag & 0x10 != 0 {
                        self.force_stop_logical_channel(x);
                    } else {
                        self.wave_pause(x, true);
                    }
                }
            }
        }
        for x in self.channels_of(mgr, ListKind::Releasing) {
            if self.channels[x as usize].sound_id != id {
                continue;
            }
            if flag & 0x20 != 0 {
                count += 1;
            } else {
                let ch = &mut self.channels[x as usize];
                ch.poly += 1;
                if ch.poly == poly {
                    self.force_stop_logical_channel(x);
                }
            }
        }
        if flag & 0x20 != 0 {
            self.channels[c as usize].poly = count;
            return count < poly;
        }
        self.channels[c as usize].poly = 0;
        true
    }

    /// `__Oneshot_StopMonoPolyCheck`
    fn stop_mono_poly_check(&mut self, c: u16, id: u32) {
        let poly = POLYS[((id >> 24) & 0xF) as usize];
        if id == 0 || poly == 0 {
            return;
        }
        let mine = self.channels[c as usize].poly;
        let mgr = self.channels[c as usize].mgr;
        let shared = (id >> 24) & 0x20 != 0;
        for x in self.channels_of(mgr, ListKind::Active) {
            let ch = &mut self.channels[x as usize];
            if ch.sound_id != id || ch.poly <= mine {
                continue;
            }
            ch.poly -= 1;
            let resume = if shared {
                ch.poly == poly - 1
            } else {
                ch.poly < poly
            };
            if resume {
                self.wave_pause(x, false);
            }
        }
    }

    /// `UpdatePanPower_1Shot`
    pub(crate) fn update_pan_power(&mut self, c: u16, l: f32, r: f32, center: f32) {
        let sum = l + r + center;
        if sum == 0.0 {
            return;
        }
        self.channels[c as usize].pan[0] = [l / sum, r / sum, center / sum];
    }

    // ------------------------------------------------------------ stopping and gating

    /// `Jesus1Shot_Update`: status 0 = release, 1 = wave finished, 2 = envelope finished,
    /// 6 = cancelled while waiting for a voice.
    pub(crate) fn jesus_update(&mut self, c: u16, status: u32) {
        if status == 0 {
            let mut releasing = false;
            for i in 0..2 {
                let ch = &mut self.channels[c as usize];
                if ch.oscs[i].is_some() && ch.bufs[i].state != 6 && ch.bufs[i].state != 7 {
                    ch.bufs[i].state = 4;
                    releasing = true;
                }
            }
            if releasing && self.list_cut(c) {
                let mgr = self.channels[c as usize].mgr;
                self.list_add_tail(mgr, ListKind::Releasing, c);
                if let Some(v) = self.channels[c as usize].voice {
                    let p = (self.channels[c as usize].priority >> 8) as u8;
                    self.voices[v as usize].prio = if p == 0 { 1 } else { p };
                }
            }
            self.channels[c as usize].note_id = -1;
            return;
        }
        if self.list_cut(c) {
            let id = std::mem::take(&mut self.channels[c as usize].sound_id);
            self.stop_mono_poly_check(c, id);
            self.free_channel(c);
        }
        if status != 6 {
            self.stop_logical_channel(c);
        } else {
            self.del_wait(c);
        }
        let ch = &mut self.channels[c as usize];
        ch.note = 0xFF;
        ch.note_id = -1;
        ch.has_updater = false;
    }

    /// `Stop_1Shot`
    pub(crate) fn stop_1shot(&mut self, c: u16) {
        let status = if self.channels[c as usize].voice.is_none() {
            6
        } else {
            0
        };
        self.jesus_update(c, status);
    }

    /// `Stop_1Shot_R`: release over `release` instead of the envelope's own release.
    pub(crate) fn stop_1shot_r(&mut self, c: u16, release: u16) {
        if self.channels[c as usize].voice.is_none() {
            self.jesus_update(c, 6);
        } else {
            self.channels[c as usize].bufs[0].release_param = release;
            self.jesus_update(c, 0);
        }
    }

    /// `AllStop_1Shot`
    pub(crate) fn all_stop_1shot(&mut self, mgr: MgrId) {
        for c in self.channels_of(mgr, ListKind::Active) {
            self.stop_1shot(c);
        }
    }

    /// `FlushRelease_1Shot`: cut releasing notes short.
    pub(crate) fn flush_release_1shot(&mut self, mgr: MgrId) {
        for c in self.channels_of(mgr, ListKind::Releasing) {
            let ch = &mut self.channels[c as usize];
            for j in 0..2 {
                if ch.oscs[j].is_some() && ch.bufs[j].state != 6 && ch.bufs[j].state != 7 {
                    ch.bufs[j].state = 6;
                }
            }
        }
    }

    fn key_pitch(&self, c: u16, key: u8) -> f32 {
        let ch = &self.channels[c as usize];
        let k = match (ch.kind, ch.wave) {
            (0, Some(w)) => key as i32 + 60 - self.wave_info(w).key as i32,
            _ => key as i32,
        };
        c5base(k)
    }

    /// `Gate_1Shot`
    pub(crate) fn gate_1shot(&mut self, c: u16, key: u8, vel: u8, note_id: i32) {
        if self.channels[c as usize].note_id != -1 {
            return;
        }
        let p = self.key_pitch(c, key);
        let ch = &mut self.channels[c as usize];
        ch.note_id = note_id;
        ch.last_note_played = note_id;
        ch.velocity = vel;
        ch.note = key;
        ch.current_pitch = ch.base_pitch * p;
        let v = vel as f32 / 127.0;
        ch.current_volume = v * v * ch.base_volume;
    }

    /// `SetKeyTarget_1Shot` / `SetPitchTarget_1Shot`: a portamento.
    pub(crate) fn set_key_target(&mut self, c: u16, key: u8, steps: u32) {
        let p = self.key_pitch(c, key);
        let ch = &mut self.channels[c as usize];
        let pitch = ch.base_pitch * p;
        if steps == 0 {
            ch.current_pitch = pitch;
            ch.sweeping = false;
            return;
        }
        ch.target_pitch = pitch;
        ch.sweep_steps = steps.min(u16::MAX as u32) as u16;
        ch.sweeping = true;
    }

    /// `Extra_Update`
    fn extra_update(&mut self, c: u16) {
        let ch = &mut self.channels[c as usize];
        if ch.sweep_steps != 0 {
            ch.current_pitch += (ch.target_pitch - ch.current_pitch) / ch.sweep_steps as f32;
            ch.sweep_steps -= 1;
            if ch.sweep_steps == 0 {
                ch.sweeping = false;
            }
        }
    }

    /// `One_CheckInstWave`: 0 if the instrument note would find a wave. The bank byte is used
    /// as a physical index, as the original does.
    pub(crate) fn check_inst_wave(&self, reg6: u16, arg: u16) -> u32 {
        let (bank, program, key, vel) = (
            (reg6 >> 8) as usize,
            reg6 as u8,
            (arg >> 8) as u8,
            arg as u8,
        );
        let Some(inst) = self
            .data
            .bank
            .banks
            .get(bank)
            .and_then(Option::as_ref)
            .and_then(|b| b.inst(program))
        else {
            return 1;
        };
        let Some(vmap) = inst.key_region(key).and_then(|k| vmap_for(&k.vels, vel)) else {
            return 2;
        };
        if self.find_wave(vmap.wave_id, bank).is_none() {
            return 4;
        }
        0
    }

    // ------------------------------------------------------------ per-subframe update

    /// `UpdateEffecterParam`: mixer levels and pitch from everything layered on the note.
    fn update_effecter_param(&mut self, c: u16) {
        let mgr = self.channels[c as usize].mgr;
        if self.channels[c as usize].last_mgr == Some(mgr) {
            let m = self.mgr(mgr);
            let (p, v, pan, fx, dolby, pc) =
                (m.pitch, m.volume, m.pan, m.fxmix, m.dolby, m.pan_calc);
            let ch = &mut self.channels[c as usize];
            ch.mgr_pitch = p;
            ch.mgr_volume = v;
            ch.pan[1][2] = pan;
            ch.pan[2][2] = fx;
            ch.pan[3][2] = dolby;
            ch.pan_calc = pc;
        }
        let max = self.max_mixer_level;
        let ch = &mut self.channels[c as usize];
        let pan = if ch.pan_calc[0] == 0 {
            0.5
        } else {
            pan_calc(&ch.pan[1], &ch.pan[0], ch.pan_calc[0])
        }
        .clamp(0.0, 1.0);
        let fx = pan_calc(&ch.pan[2], &ch.pan[0], ch.pan_calc[1]).clamp(0.0, 1.0);
        let dolby = pan_calc(&ch.pan[3], &ch.pan[0], ch.pan_calc[2]).clamp(0.0, 1.0);
        let volume = ch.current_volume * ch.volume_mod * ch.mgr_volume;
        ch.final_pitch =
            (ch.current_pitch * ch.pitch_mod * ch.mgr_pitch * 4096.0).clamp(0.0, 65535.0) as u16;
        let mut angle = 0.0;
        for i in 0..6 {
            let cfg = ch.bus[i];
            if cfg >> 8 == 0 {
                ch.levels[i] = 0;
                continue;
            }
            let mut v = volume;
            for nib in [(cfg >> 4) & 0xF, cfg & 0xF] {
                if nib == 0 {
                    continue;
                }
                angle = match nib {
                    1 => pan,
                    2 => fx,
                    3 => dolby,
                    5 => 1.0 - pan,
                    6 => 1.0 - fx,
                    7 => 1.0 - dolby,
                    _ => angle,
                };
                v *= sinf3(angle);
            }
            ch.levels[i] = (v.clamp(0.0, 1.0) * max) as u16;
        }
    }

    /// `__UpdateJcToDSP` + flush; `init` sets the levels without a ramp (`__UpdateJcToDSPInit`).
    fn update_jc_to_dsp(&mut self, c: u16, init: bool) {
        let Some(v) = self.channels[c as usize].voice else {
            return;
        };
        let ch = &self.channels[c as usize];
        let (levels, pitch, pause) = (ch.levels, ch.final_pitch, ch.pause);
        let voice = &mut self.voices[v as usize];
        for i in 0..6 {
            voice.target[i] = levels[i] as f32 / 32768.0;
            if init {
                voice.level[i] = voice.target[i];
            }
        }
        voice.pitch = pitch;
        voice.paused = pause;
    }

    /// `CommonCallbackLogicalChannel`: the voice's callback. Returns the subframes until the
    /// next call (0 = stopped or "now").
    pub(crate) fn common_callback(&mut self, v: u8, mut ev: u32) -> u16 {
        let Some(c) = self.voices[v as usize].owner else {
            let voice = &mut self.voices[v as usize];
            voice.has_callback = false;
            voice.prio = 0;
            self.kill_broken(v);
            return 0;
        };
        if self.channels[c as usize].voice != Some(v) {
            match self.channels[c as usize].voice {
                Some(other) if self.voices[other as usize].owner == Some(c) => self.kill_broken(v),
                _ => {
                    self.stop_logical_channel(c);
                    if self.list_cut(c) {
                        self.free_channel(c);
                    }
                }
            }
            let voice = &mut self.voices[v as usize];
            voice.owner = None;
            voice.prio = 0;
            voice.has_callback = false;
            return 0;
        }
        if ev == event::COMPLETION {
            if self.channels[c as usize].has_updater {
                self.jesus_update(c, 1);
            } else {
                self.stop_logical_channel(c);
                if self.list_cut(c) {
                    self.free_channel(c);
                }
            }
            return 0;
        }
        if ev == event::PRIORITY {
            let p = (self.channels[c as usize].priority >> 16) as u8;
            if p < self.voices[v as usize].prio {
                self.voices[v as usize].prio = p;
            }
            return 0;
        }
        if ev == event::STATE_CHANGE {
            self.channels[c as usize].bufs[0].state = 6;
            if !self.list_cut(c) {
                return 1;
            }
            let mgr = self.channels[c as usize].mgr;
            self.list_add_front(mgr, ListKind::Waiting, c);
            ev = event::NORMAL;
        }
        if ev == event::NORMAL {
            self.reset_effect_mods(c);
            let mut active = 0;
            for i in 0..4 {
                let Some((mode, val, state)) = self.run_osc(c, i) else {
                    continue;
                };
                self.do_effect_osc(c, mode, val);
                if state == 0 {
                    if !self.channels[c as usize].has_updater {
                        if !self.stop_logical_channel(c) {
                            self.voices[v as usize].enabled = false;
                        }
                        if self.list_cut(c) {
                            self.free_channel(c);
                        }
                    } else {
                        self.jesus_update(c, 2);
                    }
                    return 0;
                }
                active += 1;
            }
            if active != 0 {
                self.update_effecter_param(c);
                self.channels[c as usize].to_flush = 1;
            }
            if self.channels[c as usize].sweeping {
                self.extra_update(c);
            }
            if !self.channels[c as usize].has_updater {
                return 1;
            }
            let ch = &mut self.channels[c as usize];
            if ch.last_note_played > 0 {
                ch.last_note_played -= 1;
            }
        }
        if self.channels[c as usize].last_note_played == 0 {
            self.jesus_update(c, 0);
            let ch = &mut self.channels[c as usize];
            ch.last_note_played = ch.note_id;
        }
        if self.channels[c as usize].to_flush != 0 {
            self.update_jc_to_dsp(c, false);
            self.channels[c as usize].to_flush = 0;
        }
        1
    }

    /// `KillBrokenLogicalChannels`
    fn kill_broken(&mut self, v: u8) {
        for c in 0..CHANNELS as u16 {
            if self.channels[c as usize].voice == Some(v) {
                self.stop_logical_channel(c);
                if self.list_cut(c) {
                    self.free_channel(c);
                }
            }
        }
    }

    /// `StopLogicalChannel`
    pub(crate) fn stop_logical_channel(&mut self, c: u16) -> bool {
        let Some(v) = self.channels[c as usize].voice.take() else {
            return false;
        };
        let voice = &mut self.voices[v as usize];
        voice.has_callback = false;
        voice.callback_timer = 0;
        voice.enabled = false;
        self.dealloc_voice(v, c);
        true
    }

    /// `CheckLogicalChannel`
    fn check_logical_channel(&self, c: u16) -> bool {
        let ch = &self.channels[c as usize];
        !(ch.wave.is_none() && ch.kind == 0)
    }

    /// `PlayLogicalChannel`: starts the voice.
    fn play_logical_channel(&mut self, c: u16) -> bool {
        let Some(v) = self.channels[c as usize].voice else {
            return false;
        };
        if !self.check_logical_channel(c) {
            return false;
        }
        let source = match (
            self.channels[c as usize].kind,
            self.channels[c as usize].wave,
        ) {
            (0, Some(w)) => {
                let info = self.wave_info(w);
                match self.data.pcm(w.0, w.1, w.2) {
                    Ok(pcm) => VoiceSource::Wave {
                        pcm,
                        looping: info.looping,
                        loop_start: info.loop_start,
                        end: info.end,
                    },
                    Err(_) => VoiceSource::Wave {
                        pcm: Arc::new(Vec::new()),
                        looping: false,
                        loop_start: 0,
                        end: 0,
                    },
                }
            }
            _ => VoiceSource::Osc(self.channels[c as usize].osc_type),
        };
        let bus = self.channels[c as usize].bus;
        {
            let voice = &mut self.voices[v as usize];
            voice.has_callback = true;
            voice.callback_timer = 1;
            voice.source = source;
            for i in 0..6 {
                voice.bus[i] = (bus[i] >> 8) as u8;
            }
        }
        let mgr = self.channels[c as usize].mgr;
        self.channels[c as usize].last_mgr = Some(mgr);
        self.update_effecter_param(c);
        self.update_jc_to_dsp(c, true);
        let ch = &self.channels[c as usize];
        let (prio, release) = (ch.priority as u8, ch.release_time);
        let voice = &mut self.voices[v as usize];
        voice.prio = prio;
        voice.release_time = release;
        // `DSP_PlayStart`
        voice.enabled = true;
        voice.done = false;
        voice.end_requested = false;
        voice.pos = 0.0;
        voice.osc_phase = 0.0;
        true
    }

    /// `ResetInitialVolume`
    pub(crate) fn reset_initial_volume(&mut self, c: u16) -> bool {
        if self.channels[c as usize].voice.is_none() || !self.check_logical_channel(c) {
            return false;
        }
        self.update_effecter_param(c);
        self.update_jc_to_dsp(c, true);
        true
    }

    // ------------------------------------------------------------ waiting for a voice

    /// `Add_WaitDSPChannel`: steal a lower-priority voice and queue for it.
    fn add_wait(&mut self, c: u16) -> bool {
        if self.waiting.len() >= 32 {
            return false;
        }
        if !self.break_lower(self.channels[c as usize].priority as u8) {
            return false;
        }
        self.waiting.push_back(Some(c));
        true
    }

    /// `Del_WaitDSPChannel`
    fn del_wait(&mut self, c: u16) {
        if let Some(s) = self.waiting.iter_mut().find(|s| **s == Some(c)) {
            *s = None;
        }
    }

    /// `Cancel_WaitDSPChannel`
    fn cancel_wait(&mut self, c: u16) {
        for s in self.waiting.iter_mut() {
            if *s == Some(c) {
                *s = None;
            }
        }
    }

    /// `__Entry_WaitChannel`: give freed voices to waiting channels (`one` = at most one).
    pub(crate) fn entry_wait_channel(&mut self, one: bool) {
        while let Some(&front) = self.waiting.front() {
            let Some(c) = front else {
                self.waiting.pop_front();
                continue;
            };
            let Some(v) = self.alloc_voice(c) else {
                break;
            };
            self.channels[c as usize].voice = Some(v);
            self.play_logical_channel(c);
            if self.list_cut(c) {
                let mgr = self.channels[c as usize].mgr;
                self.list_add_tail(mgr, ListKind::Active, c);
            }
            self.waiting.pop_front();
            if one {
                break;
            }
        }
    }

    /// `EntryCheck_WaitDSPChannel`: waiting notes still count down their duration.
    pub(crate) fn entry_check_wait(&mut self) {
        self.entry_wait_channel(false);
        for i in 0..self.waiting.len() {
            let Some(c) = self.waiting[i] else {
                continue;
            };
            let ch = &mut self.channels[c as usize];
            if ch.note_id > 0 {
                ch.note_id -= 1;
            }
            if ch.note_id == 0 {
                self.waiting[i] = None;
                self.jesus_update(c, 6);
            }
        }
    }

    /// `ForceStopLogicalChannel`
    fn force_stop_logical_channel(&mut self, c: u16) -> bool {
        match self.channels[c as usize].voice {
            Some(v) => {
                self.force_stop_voice(v);
                true
            }
            None => false,
        }
    }
}
