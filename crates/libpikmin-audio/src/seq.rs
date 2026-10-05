//! The BMS sequence interpreter: jammain_2.c (`Jam_SeqmainNote` and the `Cmd_*` table),
//! seqsetup.c (track setup, `Jaq_RootCallback`), noteon.c and jamosc.c.
//!
//! A sequence is a tree of tracks. Each tick a track runs bytecode until it waits: 0x00-0x7F
//! play notes, 0x80-0x8F/0xF9 stop voices or wait, 0x90 glides a timed parameter, 0xA0 does
//! register arithmetic, 0xB0 runs a command with register arguments, 0xC0+ are commands.
//! Ticks advance by `tempoFactor` per DSP subframe.

use std::sync::Arc;

use crate::calc::{DAC_RATE, pitch_to_cent};
use crate::engine::*;
use crate::osc::{Osc, OscBuf, Table, presets};

/// `Arglist` (jammain_2.c): (count, 2-bit types: 0 = u8, 1 = u16, 2 = u24, 3 = register).
const ARGLIST: [(u8, u16); 64] = [
    (0, 0),
    (2, 0x0008), // OpenTrack
    (2, 0x0008), // OpenTrackBros
    (1, 0x0002), // Call
    (0, 0),      // CallF
    (0, 0),      // Ret
    (1, 0),      // RetF
    (1, 0x0002), // Jmp
    (0, 0),      // JmpF
    (1, 0x0001), // LoopS
    (0, 0),      // LoopE
    (2, 0),      // ReadPort
    (2, 0x000C), // WritePort
    (1, 0),      // CheckPortImport
    (1, 0),      // CheckPortExport
    (1, 0x0003), // WaitReg
    (2, 0x0005), // ConnectName
    (2, 0x000C), // ParentWritePort
    (2, 0x000C), // ChildWritePort
    (2, 0x000F),
    (1, 0),      // SetLastNote
    (1, 0),      // TimeRelate
    (1, 0),      // SimpleOsc
    (2, 0x0008), // SimpleEnv
    (5, 0x0155), // SimpleADSR
    (1, 0),      // Transpose
    (1, 0),      // CloseTrack
    (1, 0),      // OutSwitch
    (1, 0x0001), // UpdateSync
    (2, 0x0004), // BusConnect
    (1, 0),      // PauseStatus
    (2, 0x0008), // SetInterrupt
    (1, 0),      // DisInterrupt
    (0, 0),      // ClrI
    (0, 0),      // SetI
    (0, 0),      // RetI
    (2, 0x0004), // IntTimer
    (0, 0),      // ConnectOpen
    (0, 0),      // ConnectClose
    (1, 0x0001), // SyncCPU
    (0, 0),      // FlushAll
    (0, 0),      // FlushRelease
    (1, 0x0002), // Wait3
    (5, 0),      // PanPowSet
    (4, 0x0055), // IIRSet
    (1, 0x0002), // FIRSet
    (1, 0x0002), // EXTSet
    (3, 0),      // PanSwSet
    (1, 0),      // OscRoute
    (1, 0),      // IIRCutOff
    (3, 0x0028), // OscFull
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (1, 0x0001), // CheckWave
    (0, 0),      // Printf
    (0, 0),      // Nop
    (1, 0x0001), // Tempo
    (1, 0x0001), // TimeBase
    (0, 0),      // Finish
];

/// `osc_table`: oscillator mode to the update flag it dirties.
const OSC_FLAGS: [u32; 5] = [
    outer::VOLUME,
    outer::PITCH,
    outer::PAN,
    outer::FXMIX,
    outer::DOLBY,
];

fn ext8(v: u16) -> u16 {
    if v & 0x80 != 0 { v | 0xFF00 } else { v }
}

/// `__PanCalc`
fn pan_calc(from: f32, to: f32, ratio: f32, kind: u8) -> f32 {
    match kind {
        0 => from,
        1 => to,
        _ => from * (1.0 - ratio) + to * ratio,
    }
}

/// Reads an envelope table out of sequence data.
fn table_at(data: &[u8], ofs: u32) -> Table {
    let mut t = Vec::new();
    let mut at = ofs as usize;
    while let Some(b) = data.get(at..at + 6) {
        let e = [
            i16::from_be_bytes([b[0], b[1]]),
            i16::from_be_bytes([b[2], b[3]]),
            i16::from_be_bytes([b[4], b[5]]),
        ];
        t.push(e);
        at += 6;
        if (0xD..=0xF).contains(&e[0]) || t.len() >= 64 {
            break;
        }
    }
    t.into()
}

/// What a command asks the interpreter loop to do next.
enum Flow {
    Continue,
    Wait,
    RetryInterrupt,
    Stop,
}

impl Engine {
    // ------------------------------------------------------------ reading

    fn rb(&mut self, t: usize) -> u8 {
        let tr = &mut self.tracks[t];
        let b = tr.data.get(tr.pc as usize).copied().unwrap_or(0);
        tr.pc = tr.pc.wrapping_add(1);
        b
    }

    fn rw(&mut self, t: usize) -> u16 {
        (self.rb(t) as u16) << 8 | self.rb(t) as u16
    }

    fn r24(&mut self, t: usize) -> u32 {
        (self.rb(t) as u32) << 16 | (self.rb(t) as u32) << 8 | self.rb(t) as u32
    }

    fn rb_at(&self, t: usize, ofs: u32) -> u8 {
        self.tracks[t].data.get(ofs as usize).copied().unwrap_or(0)
    }

    fn rw_at(&self, t: usize, ofs: u32) -> u16 {
        (self.rb_at(t, ofs) as u16) << 8 | self.rb_at(t, ofs.wrapping_add(1)) as u16
    }

    fn r24_at(&self, t: usize, ofs: u32) -> u32 {
        (self.rb_at(t, ofs) as u32) << 16
            | (self.rb_at(t, ofs + 1) as u32) << 8
            | self.rb_at(t, ofs + 2) as u32
    }

    fn r32_at(&self, t: usize, ofs: u32) -> u32 {
        (self.rw_at(t, ofs) as u32) << 16 | self.rw_at(t, ofs + 2) as u32
    }

    fn f32_at(&self, t: usize, ofs: u32) -> f32 {
        f32::from_bits(self.r32_at(t, ofs))
    }

    // ------------------------------------------------------------ registers

    /// `Jam_ReadRegDirect`
    pub fn read_reg(&mut self, t: usize, idx: u8) -> u16 {
        let raw: u16 = match idx {
            0x20 | 0x21 => self.tracks[t].regs[6],
            0x22 => {
                let lo = self.read_reg(t, 1);
                let hi = self.read_reg(t, 0);
                hi << 8 | lo
            }
            0x2C => {
                let mut r = 0u16;
                for i in (0..16).rev() {
                    r <<= 1;
                    if self.tracks[t].children[i].is_some_and(|c| self.tracks[c].state != 0) {
                        r |= 1;
                    }
                }
                r
            }
            0x2D => {
                let mut r = 0u16;
                for i in (0..8).rev() {
                    r <<= 1;
                    r |= self.check_note_stop(t, i) as u16;
                }
                r
            }
            0x30 => {
                let tr = &self.tracks[t];
                if tr.depth == 0 {
                    0
                } else {
                    tr.loop_counters[(tr.depth - 1) as usize % 8]
                }
            }
            i if (i as usize) < 32 => self.tracks[t].regs[i as usize],
            _ => 0,
        };
        match idx {
            0..=2 | 0x21 => raw & 0xFF,
            0x20 => ((raw as i16) >> 8) as u16,
            _ => raw,
        }
    }

    /// `Jam_WriteRegDirect`: also leaves the value in register 3.
    pub fn write_reg(&mut self, t: usize, mut idx: u8, mut value: u16) {
        let rv;
        match idx {
            0..=2 => {
                value &= 0xFF;
                rv = ext8(value);
            }
            32 | 33 => return,
            34 => {
                self.write_reg(t, 0, value >> 8);
                rv = value;
                value &= 0xFF;
                idx = 1;
            }
            _ => rv = value,
        }
        let tr = &mut self.tracks[t];
        if (idx as usize) < 32 {
            tr.regs[idx as usize] = value;
        }
        tr.regs[3] = rv;
    }

    fn ext_reg(&self, t: usize, k: usize) -> u32 {
        let r = &self.tracks[t].regs;
        (r[16 + 2 * k] as u32) << 16 | r[17 + 2 * k] as u32
    }

    fn set_ext_reg(&mut self, t: usize, k: usize, v: u32) {
        let r = &mut self.tracks[t].regs;
        r[16 + 2 * k] = (v >> 16) as u16;
        r[17 + 2 * k] = v as u16;
    }

    /// `Jam_ReadReg32`
    fn read_reg32(&mut self, t: usize, idx: u8) -> u32 {
        match idx {
            40..=43 => self.ext_reg(t, idx as usize - 40),
            35 => (self.read_reg(t, 4) as u32) << 16 | self.read_reg(t, 5) as u32,
            _ => self.read_reg(t, idx) as u32,
        }
    }

    /// `__ExchangeRegisterValue`: registers below 64, ports above.
    fn exchange(&mut self, t: usize, reg: u8) -> u32 {
        if reg < 64 {
            self.read_reg32(t, reg)
        } else {
            self.tracks[t].ports[(reg - 64) as usize % 16].value as u32
        }
    }

    fn write_reg_xy(&mut self, t: usize, v: u32) {
        self.write_reg(t, 4, (v >> 16) as u16);
        self.write_reg(t, 5, v as u16);
    }

    /// `LoadTbl`
    fn load_table(&self, t: usize, ofs: u32, idx: u32, kind: u32) -> u32 {
        match kind {
            4 => self.rb_at(t, ofs.wrapping_add(idx)) as u32,
            5 => self.rw_at(t, ofs.wrapping_add(idx * 2)) as u32,
            6 => self.r24_at(t, ofs.wrapping_add(idx * 3)),
            7 => self.r32_at(t, ofs.wrapping_add(idx * 4)),
            8 => self.r32_at(t, ofs.wrapping_add(idx)),
            _ => 0,
        }
    }

    /// `Jam_WriteRegParam` (opcodes 0xA0-0xAF).
    fn write_reg_param(&mut self, t: usize, mut control: u8) {
        let mut src = control & 0xC;
        let mut op = control & 3;
        let mut table_kind = 0u32;
        let mut r27: u32 = 0;
        if control & 0xF == 0xB {
            src = 0;
            op = 0xB;
        }
        if control & 0xF == 0xA {
            control = self.rb(t);
            src = control & 0xC;
            op = 0xA;
            table_kind = (control >> 4) as u32 + 4;
        }
        if control & 0xF == 0x9 {
            control = self.rb(t);
            src = control & 0xC;
            op = control & 0xF0;
            if src == 8 {
                src = 0x10;
            }
        }
        let mut reg = self.rb(t);
        if op == 0xA {
            let r = self.rb(t);
            r27 = self.read_reg32(t, r);
        }
        let mut new: i16 = match src {
            0 => {
                let r = self.rb(t);
                self.read_reg(t, r) as i16
            }
            4 => self.rb(t) as i16,
            8 => ((self.rb(t) as u16) << 8) as i16,
            12 => self.rw(t) as i16,
            _ => -1,
        };
        let old = self.read_reg(t, reg) as i16;
        match op {
            0x00 => {}
            0x01 => {
                if src == 4 {
                    new = ext8(new as u16) as i16;
                }
                new = old.wrapping_add(new);
            }
            0x02 => {
                let v = (old as i32).wrapping_mul(new as i32) as u32;
                self.write_reg_xy(t, v);
                return;
            }
            0x03 => {
                self.tracks[t].regs[3] = old.wrapping_sub(new) as u16;
                return;
            }
            0x0B => new = old.wrapping_sub(new),
            0x10 => {
                if src == 4 {
                    new = ext8(new as u16) as i16;
                }
                let o = old as u16;
                new = if new < 0 {
                    o.checked_shr(new.unsigned_abs() as u32).unwrap_or(0) as i16
                } else {
                    o.checked_shl(new as u32).unwrap_or(0) as i16
                };
            }
            0x20 => {
                if src == 4 {
                    new = ext8(new as u16) as i16;
                }
                new = if new < 0 {
                    old.checked_shr(new.unsigned_abs() as u32)
                        .unwrap_or(if old < 0 { -1 } else { 0 })
                } else {
                    old.checked_shl(new as u32).unwrap_or(0)
                };
            }
            0x30 => new &= old,
            0x40 => new |= old,
            0x50 => new ^= old,
            0x60 => new = old.wrapping_neg(),
            0x90 => {
                // `unaff_r27` is a u32, so the modulo is unsigned.
                let r = self.rng.s32() as u32;
                let m = new as u16 as u32;
                new = if m == 0 { 0 } else { (r % m) as i16 };
            }
            0x0A => {
                r27 = self.load_table(t, r27, new as u16 as u32, table_kind);
                new = r27 as u16 as i16;
            }
            _ => {}
        }
        let mut result = new as u16;
        match reg {
            0..=2 => {
                new = (new as u16 & 0xFF) as i16;
                result = ext8(new as u16);
            }
            0x21 => {
                reg = 6;
                new = ((self.tracks[t].regs[6] & 0xFF00) | (new as u16 & 0xFF)) as i16;
                result = new as u16;
            }
            0x20 => {
                reg = 6;
                new = ((self.tracks[t].regs[6] & 0x00FF) | ((new as u16) << 8)) as i16;
                result = new as u16;
            }
            0x2E => {
                reg = 0xD;
                new = ((self.tracks[t].regs[13] & 0xFF00) | (new as u16 & 0xFF)) as i16;
                result = new as u16;
            }
            0x2F => {
                reg = 0xD;
                new = ((self.tracks[t].regs[13] & 0x00FF) | ((new as u16) << 8)) as i16;
                result = new as u16;
            }
            0x22 => {
                self.write_reg(t, 0, (new as u16) >> 8);
                new = (new as u16 & 0xFF) as i16;
                result = new as u16;
                reg = 1;
            }
            0x28..=0x2B => {
                self.set_ext_reg(t, reg as usize - 0x28, r27);
                return;
            }
            _ => {}
        }
        let tr = &mut self.tracks[t];
        if (reg as usize) < 32 {
            tr.regs[reg as usize] = new as u16;
        }
        tr.regs[3] = result;
        match reg {
            6 => self.osc_clear_overwrite(t),
            7 => self.tracks[t].update_flags |= outer::PITCH,
            0xD => {
                let tr = &mut self.tracks[t];
                tr.jcs.channel_priority = tr.regs[13] as u32 | 0x10000;
                tr.jcs.release_time = 0;
            }
            _ => {}
        }
    }

    /// `Jam_WriteTimeParam` (opcodes 0x90-0x9F).
    fn write_time_param(&mut self, t: usize, control: u8) {
        let index = self.rb(t) as usize;
        let target: i16 = match control & 0xC {
            0 => {
                let r = self.rb(t) as usize;
                self.tracks[t].regs.get(r).copied().unwrap_or(0) as i16
            }
            4 => self.rb(t) as i16,
            8 => ((self.rb(t) as u16) << 8) as i16,
            _ => self.rw(t) as i16,
        };
        let duration: i32 = match control & 3 {
            0 => -1,
            1 => {
                let r = self.rb(t) as usize;
                self.tracks[t].regs.get(r).copied().unwrap_or(0) as i32
            }
            2 => self.rb(t) as i32,
            _ => self.rw(t) as i32,
        };
        let Some(m) = self.tracks[t].timed.get_mut(index) else {
            return;
        };
        m.target = target as f32 / 32768.0;
        if duration >= 0 {
            m.duration = duration as f32;
        }
        if m.duration <= 0.0 {
            m.current = m.target;
            m.step = 0.0;
            m.duration = 1.0;
        } else {
            m.step = (m.target - m.current) / m.duration;
        }
    }

    /// `__ConditionCheck`
    fn condition(&mut self, t: usize, code: u8) -> bool {
        let r = self.read_reg(t, 3);
        match code & 0xF {
            0 => true,
            1 => r == 0,
            2 => r != 0,
            3 => r == 1,
            4 => r >= 0x8000,
            5 => r < 0x8000,
            _ => false,
        }
    }

    /// `Jam_SEQtimeToDSPtime`: ticks (scaled by the gate percentage) to subframes.
    fn seq_time_to_dsp(&self, t: usize, ticks: u32, gate: u8) -> i32 {
        let tr = &self.tracks[t];
        let mut d = ticks as f32 * gate as f32 / 100.0;
        if tr.time_relation == 1 {
            d /= tr.tempo_factor;
        } else {
            d = d * 120.0 / tr.time_base as f32;
        }
        d as i32
    }

    // ------------------------------------------------------------ ports and interrupts

    /// `Jam_WritePortAppDirect`: the game writing a port (ports 0 and 1 raise interrupts).
    pub fn write_port(&mut self, t: usize, port: usize, value: u16) {
        let tr = &mut self.tracks[t];
        let Some(p) = tr.ports.get_mut(port) else {
            return;
        };
        p.value = value;
        p.import = true;
        match port {
            0 => self.set_interrupt(t, 3),
            1 => self.set_interrupt(t, 4),
            _ => {}
        }
    }

    /// `Jam_ReadPortAppDirect`
    pub fn read_port(&mut self, t: usize, port: usize) -> u16 {
        let p = &mut self.tracks[t].ports[port % 16];
        p.export = false;
        p.value
    }

    /// `Jam_CheckPortAppDirect` with condition 1: is the port free to write (the sequence has
    /// read the last value)?
    pub fn port_free(&self, t: usize, port: usize) -> bool {
        !self.tracks[t].ports[port % 16].import
    }

    /// `Jam_SetInterrupt`
    pub fn set_interrupt(&mut self, t: usize, n: u8) {
        let tr = &mut self.tracks[t];
        if tr.int_enable & (1 << n) != 0 {
            tr.int_pending |= 1 << n;
        }
    }

    /// `Jam_TryInterrupt`
    fn try_interrupt(&mut self, t: usize) -> bool {
        let tr = &mut self.tracks[t];
        if tr.int_active != 0 {
            return false;
        }
        for i in 0..8 {
            let m = 1u8 << i;
            if tr.int_enable & m != 0 && tr.int_pending & m != 0 {
                tr.saved_pc = tr.pc;
                tr.pc = tr.int_addr[i];
                tr.int_active = m;
                tr.saved_wait = tr.wait;
                tr.wait = 0;
                tr.int_pending ^= m;
                return true;
            }
        }
        false
    }

    // ------------------------------------------------------------ registry and outer params

    /// `Jam_RegistTrack`
    fn regist_track(&mut self, t: usize, id: u32) {
        if self.registry.iter().any(|(i, _)| *i == id) {
            return;
        }
        if let Some(slot) = self.registry.iter_mut().find(|r| r.1.is_none()) {
            *slot = (id, Some(t));
        } else if self.registry.len() < 32 {
            self.registry.push((id, Some(t)));
        } else {
            return;
        }
        self.tracks[t].registered = true;
    }

    /// `Jam_UnRegistTrack`
    fn unregist_track(&mut self, t: usize) {
        if !self.tracks[t].registered {
            return;
        }
        for r in self.registry.iter_mut() {
            if r.1 == Some(t) {
                r.1 = None;
            }
        }
        self.tracks[t].registered = false;
        while self.registry.last().is_some_and(|r| r.1.is_none()) {
            self.registry.pop();
        }
    }

    /// `Jam_SetExtParamD`
    pub fn set_outer_param(&mut self, o: usize, value: f32, flag: u32) {
        let p = &mut self.outers[o];
        match flag {
            outer::VOLUME => p.volume = value,
            outer::PITCH => p.pitch = value,
            outer::FXMIX => p.fxmix = value,
            outer::DOLBY => p.dolby = value,
            outer::PAN => p.pan = value,
            outer::TEMPO => p.tempo = value,
            _ => return,
        }
        p.update_flags |= flag as u16;
    }

    /// `Jam_OnExtSwitchD`
    pub fn outer_on(&mut self, o: usize, flags: u16) {
        self.outers[o].flags |= flags;
        self.outers[o].update_flags |= flags;
    }

    /// `Jam_OffExtSwitchD`
    pub fn outer_off(&mut self, o: usize, flags: u16) {
        let p = &mut self.outers[o];
        p.flags &= !flags;
        p.update_flags |= flags;
    }

    /// A fresh `OuterParam_` the game owns (`Jam_InitExtBuffer`).
    pub fn new_outer(&mut self) -> usize {
        self.outers.push(OuterParam::default());
        self.outers.len() - 1
    }

    // ------------------------------------------------------------ track parameters

    /// `Jam_UpdateTrackAll`
    pub(crate) fn update_track_all(&mut self, t: usize) {
        let tr = &self.tracks[t];
        if tr.flags == 4 {
            return;
        }
        let distance = tr.regs[11] as f32 / 32767.0;
        let mut volume = if tr.muted > 0 {
            0.0
        } else {
            tr.timed[tp::VOLUME].current
        };
        let mut pitch = pitch_to_cent(tr.timed[tp::PITCH].current, tr.regs[7] as f32);
        let mut pan = tr.timed[tp::PAN].current;
        let mut fxmix = tr.timed[tp::FXMIX].current;
        let mut dolby = tr.timed[tp::DOLBY].current;
        if let Some(o) = tr.outer.map(|o| self.outers[o]) {
            let f = o.flags as u32;
            if f & outer::VOLUME != 0 {
                volume *= o.volume;
            }
            if f & outer::PITCH != 0 {
                pitch *= o.pitch;
            }
            if f & outer::FXMIX != 0 {
                fxmix = pan_calc(fxmix, o.fxmix, distance, tr.pan_calc[1]);
            }
            if f & outer::DOLBY != 0 {
                dolby = pan_calc(dolby, o.dolby, distance, tr.pan_calc[2]);
            }
            if f & outer::PAN != 0 {
                pan = pan_calc(pan, o.pan, distance, tr.pan_calc[0]);
            }
        }
        let parent = tr
            .parent
            .filter(|_| tr.flags & 1 == 0)
            .map(|p| self.tracks[p].jcs.clone());
        let parent_distance = tr.regs[12] as f32 / 32767.0;
        let ppc = tr.parent_pan_calc;
        let base_priority = tr.regs[13];
        let tr = &mut self.tracks[t];
        tr.jcs.channel_priority = base_priority as u32 | 0x10000;
        tr.jcs.release_time = 0;
        match parent {
            None => {
                tr.jcs.volume = volume;
                tr.jcs.pitch = pitch;
                tr.jcs.pan = pan;
                tr.jcs.fxmix = fxmix;
                tr.jcs.dolby = dolby;
            }
            Some(p) => {
                tr.jcs.volume = p.volume * volume;
                tr.jcs.pitch = p.pitch * pitch;
                tr.jcs.pan = pan_calc(pan, p.pan, parent_distance, ppc[0]);
                tr.jcs.fxmix = pan_calc(fxmix, p.fxmix, parent_distance, ppc[1]);
                tr.jcs.dolby = pan_calc(dolby, p.dolby, parent_distance, ppc[2]);
            }
        }
    }

    /// `Jam_UpdateTrack`: recomputes the parameters `flags` names into the track's channel
    /// manager, combining the timed parameters, the outer parameters, track oscillators routed
    /// to the track (route 14) and the parent's values.
    pub(crate) fn update_track(&mut self, t: usize, flags: u32) {
        if self.tracks[t].flags == 4 {
            return;
        }
        if flags & outer::TEMPO != 0 && self.tracks[t].parent.is_none() {
            self.update_tempo(t);
        }
        let tr = &self.tracks[t];
        let distance = tr.regs[11] as f32 / 32767.0;
        let o = tr.outer.map(|o| self.outers[o]);
        let of = o.map_or(0, |o| o.flags as u32);
        let mut volume = 0.0;
        let mut pitch = 1.0;
        let mut pan = 0.5;
        let mut fxmix = 0.0;
        let mut dolby = 0.0;
        if flags & outer::VOLUME != 0 {
            volume = if tr.muted != 0 {
                0.0
            } else {
                tr.timed[tp::VOLUME].current
            };
            if of & outer::VOLUME != 0 {
                volume *= o.unwrap().volume;
            }
            if tr.paused && tr.pause_status & 1 != 0 {
                volume *= tr.timed[tp::PAUSE_VOLUME].current;
            }
        }
        if flags & outer::PITCH != 0 {
            pitch = pitch_to_cent(tr.timed[tp::PITCH].current, tr.regs[7] as f32);
            if of & outer::PITCH != 0 {
                pitch *= o.unwrap().pitch;
            }
        }
        if flags & outer::PAN != 0 {
            pan = tr.timed[tp::PAN].current;
            if of & outer::PAN != 0 {
                pan = pan_calc(pan, o.unwrap().pan, distance, tr.pan_calc[0]);
            }
        }
        if flags & outer::FXMIX != 0 {
            fxmix = tr.timed[tp::FXMIX].current;
            if of & outer::FXMIX != 0 {
                fxmix = pan_calc(fxmix, o.unwrap().fxmix, distance, tr.pan_calc[1]);
            }
        }
        if flags & outer::DOLBY != 0 {
            dolby = tr.timed[tp::DOLBY].current;
            if of & outer::DOLBY != 0 {
                dolby = pan_calc(dolby, o.unwrap().dolby, distance, tr.pan_calc[2]);
            }
        }
        for i in 0..2 {
            if self.tracks[t].osc_route[i] != 0x0E {
                continue;
            }
            let tr = &mut self.tracks[t];
            let osc = tr.oscs[i].clone();
            let ofs = crate::osc::osc_to_ofs(Some(&osc), &mut tr.osc_bufs[i]);
            match osc.mode {
                0 => volume *= ofs,
                1 => pitch *= ofs,
                2 => pan *= ofs,
                3 => fxmix *= ofs,
                4 => dolby *= ofs,
                _ => {}
            }
        }
        let tr = &self.tracks[t];
        let parent = tr
            .parent
            .filter(|_| tr.flags & 1 == 0)
            .map(|p| self.tracks[p].jcs.clone());
        let parent_distance = tr.regs[12] as f32 / 32767.0;
        let ppc = tr.parent_pan_calc;
        let j = &mut self.tracks[t].jcs;
        let set = |flag: u32| flags & flag != 0;
        match parent {
            None => {
                if set(outer::VOLUME) {
                    j.volume = volume;
                }
                if set(outer::PITCH) {
                    j.pitch = pitch;
                }
                if set(outer::PAN) {
                    j.pan = pan;
                }
                if set(outer::FXMIX) {
                    j.fxmix = fxmix;
                }
                if set(outer::DOLBY) {
                    j.dolby = dolby;
                }
            }
            Some(p) => {
                if set(outer::VOLUME) {
                    j.volume = p.volume * volume;
                }
                if set(outer::PITCH) {
                    j.pitch = p.pitch * pitch;
                }
                if set(outer::PAN) {
                    j.pan = pan_calc(pan, p.pan, parent_distance, ppc[0]);
                }
                if set(outer::FXMIX) {
                    j.fxmix = pan_calc(fxmix, p.fxmix, parent_distance, ppc[1]);
                }
                if set(outer::DOLBY) {
                    j.dolby = pan_calc(dolby, p.dolby, parent_distance, ppc[2]);
                }
            }
        }
    }

    /// `Jam_UpdateTempo`
    pub(crate) fn update_tempo(&mut self, t: usize) {
        match self.tracks[t].parent {
            None => {
                let tr = &self.tracks[t];
                let mut f = tr.time_base as f32 * tr.tempo as f32 / (DAC_RATE * 60.0 / 80.0);
                if let Some(o) = tr.outer.map(|o| self.outers[o])
                    && o.flags as u32 & outer::TEMPO != 0
                {
                    f *= o.tempo;
                }
                self.tracks[t].tempo_factor = f;
            }
            Some(p) => {
                self.tracks[t].tempo_factor = self.tracks[p].tempo_factor;
                self.tracks[t].time_base = self.tracks[p].time_base;
            }
        }
        for i in 0..16 {
            if let Some(c) = self.tracks[t].children[i]
                && self.tracks[c].state != 0
            {
                self.update_tempo(c);
            }
        }
    }

    /// `SeqUpdate`
    pub(crate) fn seq_update(&mut self, t: usize, flags: u32) {
        let mut f = flags | self.tracks[t].update_flags;
        if let Some(o) = self.tracks[t].outer {
            f |= self.outers[o].update_flags as u32;
            self.outers[o].update_flags = 0;
        }
        self.tracks[t].update_flags = 0;
        if f != 0 {
            self.update_track(t, f);
        }
        for i in 0..16 {
            if let Some(c) = self.tracks[t].children[i]
                && self.tracks[c].state != 0
            {
                self.seq_update(c, f);
            }
        }
    }

    /// `Jam_MuteTrack`
    pub fn mute_track(&mut self, t: usize, mute: u8) {
        if let Some(p) = self.tracks[t].parent {
            let mask = 1u16 << (self.tracks[t].track_id & 0xF);
            self.tracks[t].muted = self.tracks[p].muted | mute;
            if mute == 0 {
                self.tracks[p].child_mute &= !mask;
            } else {
                self.tracks[p].child_mute |= mask;
            }
        } else {
            self.tracks[t].muted = mute;
        }
        self.tracks[t].update_flags |= outer::VOLUME;
        if self.tracks[t].muted != 0 && self.tracks[t].pause_status & 0x20 != 0 {
            for i in 0..8 {
                self.note_off_r(t, i, 10);
            }
        }
    }

    /// `Jam_PauseTrack`
    pub fn pause_track(&mut self, t: usize, recursive: bool) {
        self.tracks[t].paused = true;
        let ps = self.tracks[t].pause_status;
        if ps & 1 != 0 {
            self.tracks[t].update_flags |= outer::VOLUME;
        }
        if ps & 4 != 0 {
            for i in 0..8 {
                self.note_off_r(t, i, 10);
            }
        }
        if ps & 8 != 0 {
            for i in 0..8 {
                if let Some(c) = self.tracks[t].channels[i]
                    && self.tracks[t].sound_ids[i] == self.channels[c as usize].id
                {
                    self.channels[c as usize].pause = true;
                }
            }
        }
        self.set_interrupt(t, 0);
        if recursive {
            for i in 0..16 {
                if let Some(c) = self.tracks[t].children[i]
                    && self.tracks[c].state != 0
                {
                    self.pause_track(c, true);
                }
            }
        }
    }

    /// `Jam_UnPauseTrack`
    pub fn unpause_track(&mut self, t: usize, recursive: bool) {
        self.tracks[t].paused = false;
        self.tracks[t].update_flags |= outer::VOLUME;
        for i in 0..8 {
            if let Some(c) = self.tracks[t].channels[i]
                && self.tracks[t].sound_ids[i] == self.channels[c as usize].id
            {
                self.channels[c as usize].pause = false;
            }
        }
        self.set_interrupt(t, 1);
        if recursive {
            for i in 0..16 {
                if let Some(c) = self.tracks[t].children[i]
                    && self.tracks[c].state != 0
                {
                    self.unpause_track(c, true);
                }
            }
        }
    }

    // ------------------------------------------------------------ oscillators (jamosc.c)

    /// `Osc_Clear_Overwrite`
    fn osc_clear_overwrite(&mut self, t: usize) {
        self.tracks[t].osc_route = [15, 15];
    }

    /// `Osc_Update_Param`
    fn osc_update_param(&mut self, t: usize, id: usize, v: f32) {
        let o = &mut self.tracks[t].oscs;
        match id {
            6 => o[0].width = v,
            7 => o[0].rate = v,
            8 => o[0].vertex = v,
            9 => o[1].width = v,
            10 => o[1].rate = v,
            11 => o[1].vertex = v,
            _ => {}
        }
    }

    // ------------------------------------------------------------ track setup (seqsetup.c)

    /// `Init_Track`
    fn init_track(&mut self, t: usize, source: u32, parent: Option<usize>) {
        let p = parent.map(|p| self.tracks[p].clone());
        let tr = &mut self.tracks[t];
        match &p {
            None => {
                tr.pc = 0;
                tr.tempo = 120;
                tr.time_base = 48;
                tr.time_relation = 1;
                tr.paused = false;
                tr.pause_status = 10;
                tr.child_mute = 0;
                tr.muted = 0;
            }
            Some(p) => {
                tr.data = p.data.clone();
                tr.pc = source;
                tr.tempo = p.tempo;
                tr.needs_tempo_sync = false;
                tr.tempo_factor = p.tempo_factor;
                tr.time_base = p.time_base;
                tr.time_relation = p.time_relation;
                tr.paused = p.paused;
                tr.pause_status = p.pause_status;
                tr.child_mute = 0;
            }
        }
        tr.depth = 0;
        tr.note_duration = 0;
        tr.wait = 0;
        tr.state = 1;
        tr.parent = parent;
        tr.int_enable = 0;
        tr.int_active = 0;
        tr.int_pending = 0;
        tr.timer = 0;
        for m in tr.timed.iter_mut() {
            *m = Move {
                current: 1.0,
                target: 1.0,
                duration: 0.0,
                step: 0.0,
            };
        }
        for i in [
            tp::PITCH,
            tp::FXMIX,
            tp::DOLBY,
            tp::PAN_DELAY,
            tp::DIST_FILTER,
            13,
            14,
            15,
        ] {
            tr.timed[i].current = 0.0;
            tr.timed[i].target = 0.0;
        }
        tr.timed[tp::PAN].current = 0.5;
        tr.timed[tp::PAN].target = 0.5;
        tr.timed[tp::PAUSE_VOLUME].current = 0.5;
        tr.timed[tp::PAUSE_VOLUME].target = 0.5;
        tr.regs = [0; 32];
        if let Some(p) = p.as_ref().filter(|_| tr.flags & 2 == 0) {
            tr.regs[8..13].copy_from_slice(&p.regs[8..13]);
            tr.regs[6] = p.regs[6];
            tr.regs[7] = p.regs[7];
            tr.regs[13] = p.regs[13];
            tr.pan_calc = p.pan_calc;
            tr.parent_pan_calc = p.parent_pan_calc;
            tr.jcs.pan_calc = p.jcs.pan_calc;
        } else {
            tr.regs[8..13].copy_from_slice(&[0, 1, 1, 0x7FFF, 0x4000]);
            tr.pan_calc = [2; 3];
            tr.parent_pan_calc = [2; 3];
            tr.jcs.pan_calc = [26; 3];
            tr.regs[6] = 0xF0;
            tr.regs[7] = 0x0C;
            tr.regs[13] = 0x40;
        }
        tr.child_outer = [None; 16];
        tr.children = [None; 16];
        tr.active_notes = [0xFF; 8];
        tr.channels = [None; 8];
        tr.sound_ids = [0; 8];
        tr.note_flags = 0;
        tr.last_note = 0;
        tr.was_note_played = 0;
        tr.gate_mode = false;
        // `Osc_Init_Env`
        tr.oscs[0] = presets::envelope();
        tr.osc_route = [15, 15];
        tr.transpose = 0;
        tr.final_transpose = 0;
        tr.tick = -1;
        for p in tr.ports.iter_mut() {
            p.import = false;
            p.export = false;
        }
        tr.registered = false;
        tr.generation = tr.generation.wrapping_add(1);
    }

    /// `Init_1shot` with mode 0: channels are borrowed from the global pool per note.
    fn init_jcs(&mut self, t: usize) {
        self.release_channels_to_global(MgrId::Track(t));
        let pan_calc = self.tracks[t].jcs.pan_calc;
        self.tracks[t].jcs = Mgr::default();
        self.tracks[t].jcs.pan_calc = pan_calc;
    }

    /// `Jaq_SetSeqData`: loads a sequence into root slot `slot` (file_seq.c's `rootseq[slot]`)
    /// and returns its handle.
    pub fn set_seq_data(&mut self, slot: usize, data: Arc<[u8]>) -> Option<usize> {
        let t = slot;
        let handle = self.roots.iter().position(Option::is_none)?;
        self.roots[handle] = Some(t);
        let tr = &mut self.tracks[t];
        tr.allocated = false;
        tr.data = data;
        tr.track_id = handle as u32;
        tr.flags = 3;
        self.init_track(t, 0, None);
        self.outers[handle] = OuterParam::default();
        self.tracks[t].outer = Some(handle);
        self.init_jcs(t);
        self.tracks[t].tempo_acc = 0.0;
        self.tracks[t].tempo_factor = 1.0;
        self.update_track_all(t);
        self.tracks[t].state = 2;
        Some(handle)
    }

    /// `Jaq_StartSeq`
    pub fn start_seq(&mut self, handle: usize) -> bool {
        let Some(t) = self.roots.get(handle).copied().flatten() else {
            return false;
        };
        if self.tracks[t].state != 2 {
            return false;
        }
        self.tracks[t].state = 1;
        true
    }

    /// `Jaq_StopSeq`
    pub fn stop_seq(&mut self, handle: usize) -> bool {
        let Some(t) = self.roots.get(handle).copied().flatten() else {
            return false;
        };
        match self.tracks[t].state {
            0 => {}
            2 => {
                self.tracks[t].state = 0;
                self.roots[handle] = None;
            }
            _ => self.tracks[t].state = 3,
        }
        true
    }

    pub fn root_track(&self, handle: usize) -> Option<usize> {
        self.roots.get(handle).copied().flatten()
    }

    /// `__StopSeq`
    fn stop_seq_now(&mut self, t: usize) {
        self.seq_update(t, 0);
        self.close_track(t);
        if let Some(h) = self.roots.iter().position(|r| *r == Some(t)) {
            self.roots[h] = None;
        }
    }

    /// `Jaq_OpenTrack`
    fn open_track(&mut self, t: usize, flags: u32, source: u32) {
        let mut index = (flags & 0xF) as u8;
        let mut mode = ((flags & 0xC0) >> 6) as u8;
        if flags & 0x20 != 0 {
            mode = 4;
            index = self.read_reg(t, index) as u8;
        }
        if index >= 16 {
            return;
        }
        if let Some(old) = self.tracks[t].children[index as usize] {
            self.close_track(old);
        }
        let Some(c) = self.free_tracks.pop_front() else {
            return;
        };
        self.tracks[c].state = 2;
        self.tracks[c].allocated = true;
        self.tracks[t].children[index as usize] = Some(c);
        let pid = self.tracks[t].track_id;
        self.tracks[c].track_id = ((pid << 4 | index as u32) & 0x0FFF_FFFF)
            | ((pid & 0xF000_0000).wrapping_add(0x1000_0000));
        self.tracks[c].connection_id = 0;
        self.tracks[c].flags = mode;
        self.init_track(c, source, Some(t));
        let pm = self.tracks[t].muted;
        let pcm = self.tracks[t].child_mute;
        self.tracks[c].muted = pm | ((pcm >> index) & 1) as u8;
        self.tracks[c].outer = self.tracks[t].child_outer[index as usize];
        self.init_jcs(c);
        self.update_track_all(c);
    }

    /// `__AllNoteOff`
    fn all_note_off(&mut self, t: usize) {
        let root = self.tracks[t].parent.is_none();
        for i in 0..8 {
            if root {
                self.note_off_r(t, i, 10);
            } else {
                self.note_off(t, i);
            }
            self.tracks[t].active_notes[i] = 0xFF;
            self.tracks[t].channels[i] = None;
        }
    }

    /// `Jaq_CloseTrack`
    pub(crate) fn close_track(&mut self, t: usize) {
        if self.tracks[t].state == 0 {
            return;
        }
        self.all_note_off(t);
        self.tracks[t].state = 0;
        if self.tracks[t].allocated {
            self.free_tracks.push_back(t);
            self.tracks[t].allocated = false;
        }
        for i in 0..16 {
            if let Some(c) = self.tracks[t].children[i].take() {
                self.close_track(c);
            }
        }
        self.tracks[t].outer = None;
        self.tracks[t].child_outer = [None; 16];
        self.tracks[t].muted = 0;
        self.tracks[t].child_mute = 0;
        match self.tracks[t].parent {
            Some(p) => self.move_channels(MgrId::Track(t), MgrId::Track(p)),
            None => self.release_channels_to_global(MgrId::Track(t)),
        }
        self.unregist_track(t);
        self.tracks[t].generation = self.tracks[t].generation.wrapping_add(1);
    }

    /// `Jaq_RootCallback`: once per subframe.
    pub(crate) fn root_callback(&mut self, t: usize) {
        if self.tracks[t].state == 3 {
            self.stop_seq_now(t);
            return;
        }
        self.tracks[t].tempo_acc += self.tracks[t].tempo_factor;
        let mut guard = 0;
        while self.tracks[t].tempo_acc >= 1.0 && guard < 64 {
            guard += 1;
            self.tracks[t].tempo_acc -= 1.0;
            if self.seqmain_note(t, 0) == -1 {
                self.stop_seq_now(t);
                return;
            }
        }
        self.seq_update(t, 0);
    }

    // ------------------------------------------------------------ notes (noteon.c)

    /// `NoteON`
    fn note_on(&mut self, t: usize, voice: usize, key: u8, vel: u8, duration: i32) -> i32 {
        if self.tracks[t].muted != 0 && self.tracks[t].pause_status & 0x40 != 0 {
            return -1;
        }
        if self.tracks[t].channels[voice].is_some() {
            self.note_off(t, voice);
        }
        let mgr = if self.tracks[t].flags == 4 {
            match self.tracks[t].parent {
                Some(p) => MgrId::Track(p),
                None => return -1,
            }
        } else {
            MgrId::Track(t)
        };
        let reg = self.read_reg(t, 6);
        let Some(bank) = self.data.bank.bank_physical(reg >> 8) else {
            return -1;
        };
        let program = reg as u8;
        let chan = if program >= 0xF0 {
            self.play_1shot_osc(mgr, program, key, vel, duration)
        } else if program >= 0xE4 {
            self.play_1shot_perc(mgr, bank, program, key, vel, duration)
        } else {
            self.play_1shot(mgr, bank, program, key, vel, duration)
        };
        self.tracks[t].channels[voice] = chan;
        let Some(c) = chan else {
            return -1;
        };
        self.tracks[t].sound_ids[voice] = self.channels[c as usize].id;
        let a = self.tracks[t].regs;
        self.update_pan_power(c, a[8] as f32, a[9] as f32, a[10] as f32);
        for i in 0..2 {
            let route = self.tracks[t].osc_route[i];
            if route == 15 || route == 14 {
                continue;
            }
            let mut slot = route as usize;
            if route >= 8 {
                slot -= 8;
                if let Some(o) = self.channel_osc(c, slot) {
                    self.tracks[t].oscs[i] = o;
                }
            } else if route >= 4 {
                slot -= 4;
                if let Some(o) = self.channel_osc(c, slot) {
                    let rel = self.tracks[t].oscs[i].release.clone();
                    self.tracks[t].oscs[i] = o;
                    self.tracks[t].oscs[i].release = rel;
                }
            }
            if slot < 4 {
                let copy = self.tracks[t].oscs[i].clone();
                let generation = self.tracks[t].generation;
                self.effecter_overwrite(
                    c,
                    slot,
                    OscRef::Track {
                        track: t,
                        index: i,
                        generation,
                        copy,
                    },
                );
            }
        }
        self.update_track(t, 3);
        self.reset_initial_volume(c);
        0
    }

    /// `NoteOFF_R`
    pub(crate) fn note_off_r(&mut self, t: usize, voice: usize, release: u16) -> bool {
        let Some(c) = self.tracks[t].channels[voice] else {
            return false;
        };
        if self.channels[c as usize].id == self.tracks[t].sound_ids[voice] {
            if release == 0 {
                self.stop_1shot(c);
            } else {
                self.stop_1shot_r(c, release);
            }
        }
        self.tracks[t].channels[voice] = None;
        self.tracks[t].sound_ids[voice] = 0;
        true
    }

    fn note_off(&mut self, t: usize, voice: usize) -> bool {
        self.note_off_r(t, voice, 0)
    }

    /// `GateON`: a tied note changes the playing note's key.
    fn gate_on(&mut self, t: usize, voice: usize, key: u8, vel: u8, duration: i32) -> i32 {
        match self.tracks[t].channels[voice] {
            Some(c) => {
                self.gate_1shot(c, key, vel, duration);
                0
            }
            None => -1,
        }
    }

    /// `CheckNoteStop`
    fn check_note_stop(&mut self, t: usize, voice: usize) -> bool {
        let Some(c) = self.tracks[t].channels[voice] else {
            return true;
        };
        if self.channels[c as usize].id != self.tracks[t].sound_ids[voice] {
            self.tracks[t].channels[voice] = None;
            self.tracks[t].sound_ids[voice] = 0;
            return true;
        }
        self.channels[c as usize].note == 0xFF
    }

    // ------------------------------------------------------------ commands

    /// `Cmd_Process`
    fn cmd_process(&mut self, t: usize, cmd: u8, mask: u16) -> Flow {
        let idx = cmd.wrapping_sub(0xC0) as usize;
        let Some(&(count, types)) = ARGLIST.get(idx) else {
            return Flow::Continue;
        };
        let mut types = types | mask;
        // `SEQ_ARG` is global: arguments a command doesn't take keep their old values.
        for i in 0..count as usize {
            self.seq_args[i] = match types & 3 {
                0 => self.rb(t) as u32,
                1 => self.rw(t) as u32,
                2 => self.r24(t),
                _ => {
                    let r = self.rb(t);
                    self.exchange(t, r)
                }
            };
            types >>= 2;
        }
        let args = self.seq_args;
        self.command(t, cmd, idx, &args)
    }

    /// `RegCmd_Process` (opcodes 0xB0-0xBF): a command whose arguments may come from registers.
    fn reg_cmd_process(&mut self, t: usize, from_reg: bool, count: u32) -> Flow {
        let mut cmd = self.rb(t);
        if from_reg {
            cmd = self.exchange(t, cmd) as u8;
        }
        let mut mask = 0u16;
        if !from_reg || count != 0 {
            let mut bits = self.rb(t);
            let mut pair = 0b11u16;
            for _ in 0..count + 1 {
                if bits & 0x80 != 0 {
                    mask |= pair;
                }
                bits <<= 1;
                pair <<= 2;
            }
        }
        self.cmd_process(t, cmd, mask)
    }

    fn command(&mut self, t: usize, cmd: u8, idx: usize, a: &[u32; 8]) -> Flow {
        match idx {
            1 => self.open_track(t, a[0], a[1]),
            2 => match self.tracks[t].parent {
                Some(p) => self.open_track(p, a[0], a[1]),
                None => return Flow::Continue,
            },
            3 => {
                let tr = &mut self.tracks[t];
                tr.call_stack[tr.depth as usize % 8] = tr.pc;
                tr.depth += 1;
                tr.pc = a[0];
            }
            4 | 8 => {
                // `Cmd_CallF` / `Cmd_JmpF`
                let flags = self.rb(t);
                let target = if flags & 0x80 != 0 {
                    let r = self.rb(t);
                    let mut target = self.read_reg(t, r) as u32;
                    if flags & 0x40 != 0 {
                        let base = if flags & 0x20 != 0 {
                            let r = self.rb(t);
                            self.read_reg(t, r) as u32
                        } else {
                            self.r24(t)
                        };
                        target = self.r24_at(t, base.wrapping_add(target * 3));
                    }
                    target
                } else {
                    self.r24(t)
                };
                if self.condition(t, flags & 0xF) {
                    let tr = &mut self.tracks[t];
                    if cmd == 0xC4 {
                        tr.call_stack[tr.depth as usize % 8] = tr.pc;
                        tr.depth += 1;
                    }
                    tr.pc = target;
                }
            }
            5 => {
                let tr = &mut self.tracks[t];
                tr.depth = tr.depth.wrapping_sub(1);
                tr.pc = tr.call_stack[tr.depth as usize % 8];
            }
            6 => {
                if self.condition(t, a[0] as u8 & 0xF) {
                    let tr = &mut self.tracks[t];
                    tr.depth = tr.depth.wrapping_sub(1);
                    tr.pc = tr.call_stack[tr.depth as usize % 8];
                }
            }
            7 => self.tracks[t].pc = a[1],
            9 => {
                let tr = &mut self.tracks[t];
                let d = tr.depth as usize % 8;
                tr.call_stack[d] = tr.pc;
                tr.loop_counters[d] = a[0] as u16;
                tr.depth += 1;
            }
            10 => {
                let tr = &mut self.tracks[t];
                if tr.depth == 0 {
                    return Flow::Continue;
                }
                let d = (tr.depth - 1) as usize % 8;
                let mut n = tr.loop_counters[d];
                n = n.saturating_sub(1);
                if n == 0 {
                    tr.depth -= 1;
                } else {
                    tr.loop_counters[d] = n;
                    tr.pc = tr.call_stack[d];
                }
            }
            11 => {
                let p = a[0] as usize % 16;
                let v = self.tracks[t].ports[p].value;
                self.tracks[t].ports[p].import = false;
                self.write_reg(t, a[1] as u8, v);
            }
            12 => {
                let p = &mut self.tracks[t].ports[a[0] as usize % 16];
                p.value = a[1] as u16;
                p.export = true;
            }
            13 => {
                let v = self.tracks[t].ports[a[0] as usize % 16].import as u16;
                self.write_reg(t, 3, v);
            }
            14 => {
                let v = self.tracks[t].ports[a[0] as usize % 16].export as u16;
                self.write_reg(t, 3, v);
            }
            15 | 42 => {
                self.tracks[t].wait = a[0] as i32;
                if a[0] != 0 {
                    return Flow::Wait;
                }
            }
            16 => self.tracks[t].connection_id = a[0] << 16 | a[1],
            17 => {
                if let Some(p) = self.tracks[t].parent {
                    self.write_port(p, a[0] as usize & 0xF, a[1] as u16);
                }
            }
            18 => {
                if let Some(c) = self.tracks[t].children[(a[0] >> 4) as usize % 16] {
                    self.write_port(c, a[0] as usize & 0xF, a[1] as u16);
                }
            }
            20 => {
                let tr = &mut self.tracks[t];
                tr.last_note = (a[0] as u8).wrapping_add(tr.final_transpose);
            }
            21 => self.tracks[t].time_relation = a[0] as u8,
            22 => match a[0] {
                // `Osc_Setup_Simple`
                0 => self.tracks[t].oscs[1] = presets::vibrato(),
                1 => self.tracks[t].oscs[0] = presets::tremolo(),
                2 => self.tracks[t].oscs[1] = presets::tremolo(),
                _ => {}
            },
            23 => {
                // `Osc_Setup_SimpleEnv`
                let table = table_at(&self.tracks[t].data, a[1]);
                match a[0] {
                    0 => {
                        self.tracks[t].oscs[0] = presets::envelope();
                        self.tracks[t].oscs[0].attack = Some(table);
                    }
                    1 => self.tracks[t].oscs[0].release = Some(table),
                    _ => {}
                }
            }
            24 => {
                let mut v = [0i16; 5];
                for (i, x) in v.iter_mut().enumerate() {
                    *x = a[i] as i16;
                }
                self.tracks[t].oscs[0] = presets::adsr(v);
            }
            25 => {
                let parent = self.tracks[t].parent.map(|p| self.tracks[p].transpose);
                let tr = &mut self.tracks[t];
                tr.transpose = a[0] as i8;
                tr.final_transpose = match parent {
                    Some(p) => (p as u8).wrapping_add(tr.transpose as u8),
                    None => tr.transpose as u8,
                };
            }
            26 => {
                let i = a[0] as usize;
                if i >= 16 {
                    return Flow::Continue;
                }
                if let Some(c) = self.tracks[t].children[i].take() {
                    self.close_track(c);
                }
            }
            27 => {
                if let Some(o) = self.tracks[t].outer {
                    self.outers[o].flags = a[0] as u16;
                    self.outers[o].update_flags = 0xFFFF;
                }
            }
            28 => self.update_track(t, a[0]),
            29 => {
                if (a[0] as usize) < 6 {
                    self.tracks[t].jcs.bus_connect[a[0] as usize] = a[1] as u16;
                }
            }
            30 => self.tracks[t].pause_status = a[0] as u8,
            31 => {
                let tr = &mut self.tracks[t];
                tr.int_enable |= 1u8.wrapping_shl(a[0]);
                tr.int_addr[a[0] as usize % 8] = a[1];
            }
            32 => self.tracks[t].int_enable &= !1u8.wrapping_shl(a[0]),
            33 => self.tracks[t].int_active = 0,
            34 => self.tracks[t].int_active = 1,
            35 => {
                let tr = &mut self.tracks[t];
                tr.wait = tr.saved_wait;
                tr.int_active = 0;
                tr.pc = tr.saved_pc;
                return Flow::RetryInterrupt;
            }
            36 => {
                let tr = &mut self.tracks[t];
                tr.timer_count = a[0] as u8;
                tr.timer = a[1];
                tr.max_time = a[1];
            }
            37 => {
                let id = self.tracks[t].connection_id;
                self.regist_track(t, id);
            }
            38 => self.unregist_track(t),
            39 => {
                // `Cmd_SyncCPU`: `TrackReceive` (verysimple.c) only has side effects on the
                // game's event table and always answers 0.
                let message = a[0] as u16;
                let port2 = if message & 0x9000 == 0x9000 {
                    self.read_port(t, 2)
                } else {
                    0
                };
                self.syncs.push(SyncMessage {
                    track_id: self.tracks[t].track_id,
                    message,
                    port2,
                });
                self.write_reg(t, 3, 0);
            }
            40 => {
                self.all_stop_1shot(MgrId::Track(t));
                self.flush_release_1shot(MgrId::Track(t));
            }
            41 => self.flush_release_1shot(MgrId::Track(t)),
            43 => {
                let tr = &mut self.tracks[t];
                for i in 0..3 {
                    tr.regs[8 + i] = a[i] as u16;
                }
                for i in 3..5 {
                    tr.regs[8 + i] = (a[i] as f32 * 327.67) as u16;
                }
            }
            44 | 49 => {} // IIR filters: not emulated
            45 => {}      // FIR filter: not emulated
            46 => {
                // `Cmd_EXTSet`: an outer parameter buffer living in the sequence data.
                let ofs = a[0];
                let o = OuterParam {
                    flags: 0,
                    update_flags: 0,
                    volume: self.f32_at(t, ofs + 0xC),
                    fxmix: self.f32_at(t, ofs + 0x10),
                    dolby: self.f32_at(t, ofs + 0x14),
                    pitch: self.f32_at(t, ofs + 0x18),
                    pan: self.f32_at(t, ofs + 0x1C),
                    tempo: self.f32_at(t, ofs + 0x20),
                };
                self.outers.push(o);
                self.tracks[t].outer = Some(self.outers.len() - 1);
            }
            47 => {
                const CALC: [u8; 7] = [0, 0, 0, 1, 1, 2, 2];
                const PARENT: [u8; 7] = [0, 1, 2, 0, 2, 0, 2];
                let tr = &mut self.tracks[t];
                for i in 0..3 {
                    let k = (a[i] >> 5) as usize;
                    tr.pan_calc[i] = CALC.get(k).copied().unwrap_or(0);
                    tr.parent_pan_calc[i] = PARENT.get(k).copied().unwrap_or(0);
                    tr.jcs.pan_calc[i] = (a[i] & 0x1F) as u8;
                    tr.update_flags |= outer::PAN;
                }
            }
            48 => {
                let route = (a[0] & 0xF) as u8;
                let i = ((a[0] >> 4) & 1) as usize;
                self.tracks[t].osc_route[i] = route;
                if route == 14 {
                    self.tracks[t].osc_bufs[i].state = 1;
                }
            }
            50 => {
                // `Osc_Setup_Full`
                let flag = a[0];
                let i = ((flag >> 4) & 1) as usize;
                if flag & 0x80 != 0 {
                    self.tracks[t].oscs[i] = presets::envelope();
                    self.tracks[t].oscs[i].mode = (flag & 0xF) as u8;
                    if flag & 0xF == 1 {
                        self.tracks[t].oscs[i].vertex = 1.0;
                    }
                }
                if flag & 0x40 != 0 {
                    let table = table_at(&self.tracks[t].data, a[1]);
                    self.tracks[t].oscs[i].attack = Some(table);
                }
                if flag & 0x20 != 0 {
                    let table = table_at(&self.tracks[t].data, a[2]);
                    self.tracks[t].oscs[i].release = Some(table);
                }
            }
            58 => {
                // `Cmd_CheckWave`
                let reg6 = self.read_reg(t, 6);
                let has = self.check_inst_wave(reg6, a[0] as u16);
                self.write_reg(t, 3, has as u16);
            }
            59 => self.skip_printf(t),
            61 => {
                self.tracks[t].tempo = a[0] as u16;
                if self.tracks[t].parent.is_none() {
                    self.update_tempo(t);
                } else {
                    self.tracks[t].needs_tempo_sync = true;
                }
            }
            62 => {
                self.tracks[t].time_base = a[0] as u16;
                if self.tracks[t].parent.is_none() {
                    self.update_tempo(t);
                }
            }
            63 => {
                // `Cmd_Finish`
                let mut flags = 0;
                for i in 0..18 {
                    let m = &mut self.tracks[t].timed[i];
                    if m.duration > 0.0 {
                        m.current += m.step;
                        m.duration -= 1.0;
                        let v = m.current;
                        if i <= 5 || i >= 11 {
                            flags |= 1 << i;
                        } else {
                            self.osc_update_param(t, i, v);
                        }
                    }
                }
                self.seq_update(t, flags);
                return Flow::Stop;
            }
            _ => {}
        }
        Flow::Continue
    }

    /// `Cmd_Printf`: consumes the format string and its register arguments.
    fn skip_printf(&mut self, t: usize) {
        let mut args = 0;
        let mut n = 0;
        loop {
            n += 1;
            if n > 0x80 {
                break;
            }
            let c = self.rb(t);
            if c == 0 {
                break;
            }
            if c == b'\\' {
                if self.rb(t) == 0 {
                    break;
                }
                continue;
            }
            if c == b'%' {
                if self.rb(t) == 0 {
                    break;
                }
                args += 1;
            }
        }
        for _ in 0..args {
            self.rb(t);
        }
    }

    // ------------------------------------------------------------ the main loop

    /// `Jam_SeqmainNote`: one tick of a track and its children. -1 closes the track.
    pub(crate) fn seqmain_note(&mut self, t: usize, muted: u8) -> i32 {
        let mut update_flags = 0u32;
        if let Some(p) = self.tracks[t].parent
            && self.tracks[t].needs_tempo_sync
        {
            let ratio = (self.tracks[t].tempo as f32 / self.tracks[p].tempo.max(1) as f32).min(1.0);
            self.tracks[t].tempo_acc += ratio;
            if self.tracks[t].tempo_acc < 1.0 {
                return 0;
            }
            self.tracks[t].tempo_acc -= 1.0;
        }
        if let Some(p) = self.tracks[t].parent {
            self.tracks[t].muted = muted;
            let pt = self.tracks[p].final_transpose;
            let tr = &mut self.tracks[t];
            tr.final_transpose = (tr.transpose as u8).wrapping_add(pt);
        } else {
            let tr = &mut self.tracks[t];
            tr.final_transpose = tr.transpose as u8;
        }
        self.set_interrupt(t, 7);
        {
            let tr = &mut self.tracks[t];
            if tr.timer != 0 {
                tr.timer -= 1;
                if tr.timer == 0 {
                    if tr.int_enable & (1 << 6) != 0 {
                        tr.int_pending |= 1 << 6;
                    }
                    if tr.timer_count != 0 {
                        tr.timer_count -= 1;
                        if tr.timer_count != 0 {
                            tr.timer = tr.max_time;
                        }
                    } else {
                        tr.timer = tr.max_time;
                    }
                }
            }
        }
        'interrupt: loop {
            self.try_interrupt(t);
            if self.tracks[t].paused && self.tracks[t].pause_status & 2 != 0 {
                return self.process_children(t, update_flags);
            }
            self.tracks[t].tick = self.tracks[t].tick.wrapping_add(1);
            let mut run = true;
            if self.tracks[t].wait == -1 {
                if self.check_note_stop(t, 0) {
                    self.tracks[t].wait = 0;
                } else {
                    run = false;
                }
            }
            if run && self.tracks[t].wait > 0 {
                self.tracks[t].wait -= 1;
                if self.tracks[t].wait != 0 {
                    run = false;
                } else {
                    let tr = &mut self.tracks[t];
                    if tr.note_duration != u32::MAX && tr.note_flags == 0 {
                        for i in 0..tr.was_note_played as usize {
                            tr.active_notes[i] = 0xFF;
                            tr.channels[i] = None;
                        }
                    }
                }
            }
            if run {
                let mut steps = 0u32;
                loop {
                    steps += 1;
                    if steps > 10_000 {
                        // A runaway loop with no waits; treat it as finished.
                        return -1;
                    }
                    let op = self.rb(t);
                    if op & 0x80 == 0 {
                        if self.note_event(t, op) {
                            break;
                        }
                    } else if op & 0xF0 == 0x80 || op == 0xF9 {
                        match self.note_off_or_wait(t, op) {
                            Some(true) => break,
                            _ => continue,
                        }
                    } else {
                        let flow = match op & 0xF0 {
                            0x90 => {
                                self.write_time_param(t, op & 0xF);
                                Flow::Continue
                            }
                            0xA0 => {
                                self.write_reg_param(t, op & 0xF);
                                Flow::Continue
                            }
                            0xB0 => self.reg_cmd_process(t, op & 8 != 0, (op & 7) as u32),
                            _ => self.cmd_process(t, op, 0),
                        };
                        match flow {
                            Flow::Continue => continue,
                            Flow::Wait => break,
                            Flow::RetryInterrupt => continue 'interrupt,
                            Flow::Stop => return -1,
                        }
                    }
                }
            }
            break;
        }
        // timed:
        for i in 0..18 {
            let m = &mut self.tracks[t].timed[i];
            if m.duration > 0.0 {
                m.current += m.step;
                m.duration -= 1.0;
                let v = m.current;
                if i <= 5 || i >= 11 {
                    update_flags |= 1 << i;
                } else {
                    self.osc_update_param(t, i, v);
                }
            }
        }
        for i in 0..2 {
            if self.tracks[t].osc_route[i] == 0x0E {
                let mode = self.tracks[t].oscs[i].mode as usize;
                update_flags |= OSC_FLAGS.get(mode).copied().unwrap_or(0);
            }
        }
        self.process_children(t, update_flags)
    }

    /// `PROCESS_CHILD_TRACKS`
    fn process_children(&mut self, t: usize, update_flags: u32) -> i32 {
        self.tracks[t].update_flags |= update_flags;
        for i in 0..16 {
            let Some(c) = self.tracks[t].children[i] else {
                continue;
            };
            if self.tracks[c].state == 0 {
                continue;
            }
            let m = self.tracks[t].muted | ((self.tracks[t].child_mute >> i) & 1) as u8;
            if self.seqmain_note(c, m) == -1 {
                self.close_track(c);
                self.tracks[t].children[i] = None;
            }
        }
        0
    }

    /// A note opcode (0x00-0x7F). Returns true when the track should stop for this tick.
    fn note_event(&mut self, t: usize, op: u8) -> bool {
        let ft = self.tracks[t].final_transpose;
        let mut key = op.wrapping_add(ft);
        let flags = self.rb(t);
        if flags & 0x80 != 0 {
            key = (self.exchange(t, key) as u8).wrapping_add(ft);
        }
        let mut sweep_target = 0u8;
        if (flags >> 5) & 2 != 0 {
            sweep_target = key;
            key = self.tracks[t].last_note;
        }
        let mut vel = self.rb(t);
        if vel >= 0x80 {
            vel = self.exchange(t, vel - 0x80) as u8;
        }
        let low = flags & 7;
        let (voice, gate, ticks): (usize, u8, u32);
        if low == 0 {
            voice = 0;
            let mut g = self.rb(t);
            if g >= 0x80 {
                g = self.exchange(t, g - 0x80) as u8;
            }
            gate = g;
            let n = (flags >> 3) & 3;
            let mut d = 0u32;
            for _ in 0..n {
                d = d << 8 | self.rb(t) as u32;
            }
            if n == 1 && d >= 0x80 {
                d = self.exchange(t, (d - 0x80) as u8);
            }
            ticks = d;
        } else {
            let mut v = low as u32;
            if (flags >> 3) & 3 != 0 {
                v = self.exchange(t, (v - 1) as u8);
                if v >= 8 {
                    return true;
                }
            }
            voice = v as usize;
            ticks = u32::MAX;
            gate = 100;
        }
        let note_flags = (flags >> 5) & 3;
        self.tracks[t].note_flags = note_flags;
        let gate_mode = self.tracks[t].gate_mode;
        let paused_notes = self.tracks[t].paused && self.tracks[t].pause_status & 0x10 != 0;
        let mut duration: i32 = if ticks == u32::MAX {
            -1
        } else {
            self.seq_time_to_dsp(t, ticks, gate)
        };
        if note_flags & 1 != 0 {
            duration = -1;
        }
        let played = if paused_notes {
            -1
        } else if gate_mode {
            self.gate_on(t, voice, key, vel, duration)
        } else {
            self.note_on(t, voice, key, vel, duration)
        };
        let tr = &mut self.tracks[t];
        if played != -1 {
            tr.active_notes[voice] = key;
        }
        tr.was_note_played = (played != -1) as u32;
        tr.gate_time = gate;
        tr.last_velocity = vel;
        tr.note_duration = ticks;
        tr.gate_mode = note_flags & 1 != 0;
        if note_flags & 2 != 0 {
            let mut steps = duration;
            if steps == -1 {
                steps = self.seq_time_to_dsp(t, ticks, self.tracks[t].gate_time);
            }
            if let Some(c) = self.tracks[t].channels[0] {
                let ft = self.tracks[t].final_transpose;
                self.set_key_target(c, sweep_target.wrapping_add(ft), steps.max(0) as u32);
            }
            key = sweep_target;
        }
        let tr = &mut self.tracks[t];
        tr.last_note = key;
        if ticks != u32::MAX {
            tr.wait = ticks as i32;
            if ticks == 0 {
                tr.wait = -1;
            }
            return true;
        }
        false
    }

    /// 0x80-0x8F and 0xF9: stop a voice, or wait. `Some(true)` = wait (stop for this tick).
    fn note_off_or_wait(&mut self, t: usize, mut op: u8) -> Option<bool> {
        let mut wait_bytes = 1;
        let mut release = 0u16;
        if op == 0xF9 {
            let b = self.rb(t);
            let v = self.exchange(t, b & 7) as u8;
            if v > 7 || v == 0 {
                if b & 0x80 != 0 {
                    self.rb(t);
                }
                return None;
            }
            op = v + 0x80;
            if b & 0x80 != 0 {
                op += 8;
            }
        }
        let mut voice = op & 0xF;
        if voice == 8 {
            wait_bytes = 2;
            voice -= 8;
        }
        if voice > 8 {
            voice -= 8;
            let mut r = self.rb(t) as u16;
            if r > 100 {
                r = (r - 98) * 20;
            }
            release = r;
        }
        if voice == 0 {
            let mut w = 0i32;
            for _ in 0..wait_bytes {
                w = w << 8 | self.rb(t) as i32;
            }
            self.tracks[t].wait = w;
            return Some(w != 0);
        }
        if release == 0 {
            self.note_off(t, voice as usize);
        } else {
            self.note_off_r(t, voice as usize, release);
        }
        None
    }
}

#[allow(dead_code)]
fn _assert_types(_: OscBuf, _: Osc) {}
