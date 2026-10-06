//! The game's side of JAudio: what `Jac_*` calls do with the sequencer.
//!
//! - Boot (`Jac_PlayInit`, `Jac_InitBgm`, verysimple.c / piki_bgm.c): sequences 0
//!   (pikise.jam, sound effects), 1 (sysevent.jam) and 13 (demobgm.jam) start in root slots 0-2.
//!   They register tracks the game talks to: 0x10009 system SE, 0x1000A Olimar SE, 0x20000 the
//!   parent of 16 positional event tracks, 0x10003 Pikmin chatter, 0x10007/0x10008 Olimar's
//!   formation and footsteps.
//! - Sounds are played by writing track ports, either directly or through 16-deep command
//!   queues that hand a value over once the sequence has read the last one (cmdqueue.c).
//! - Positional sounds are "events" (pikiinter.c): an emitter with a type and a position; its
//!   actions map through `ACTION_STATUS` to commands for one of the event tracks, whose volume
//!   and pan follow the emitter's distance and direction from the camera.
//! - Music (`Jac_PlayBgm`) plays in root slots 3 and 4 with per-track mixes (`bgm_mute_set`).

use std::collections::VecDeque;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use libpikmin_assets::sound::SoundData;
use libpikmin_assets::sound::tables::JaudioTables;

use crate::calc::{DAC_RATE, SUBFRAME};
use crate::engine::{Engine, outer};

/// `JacEventType`
pub mod event_type {
    pub const SYSTEM: i32 = -2;
    pub const PLAYER: i32 = -1;
    pub const BATTLE: i32 = 1;
    pub const PELLET: i32 = 2;
    pub const ONYON: i32 = 3;
    pub const BUILD: i32 = 4;
    pub const PIKI_WATER: i32 = 5;
    pub const PIKI: i32 = 6;
    pub const UFO: i32 = 7;
}

/// `Jac_UpdateCamera` rounds pi to 3.1416.
#[allow(clippy::approx_constant)]
const JAC_PI: f32 = 3.1416;

/// `JACORIMA_PIKISOUND`: Olimar sounds with this bit go straight to port 1.
const PIKISOUND: u32 = 0x8000;

#[derive(Debug, Clone)]
struct CmdQueue {
    /// Registered track id, resolved through `Jam_GetTrackHandle` each time.
    handle: QueueTrack,
    port: usize,
    q: VecDeque<u16>,
}

#[derive(Debug, Clone, Copy)]
enum QueueTrack {
    Registered(u32),
    Slot(usize),
}

#[derive(Debug, Clone, Copy, Default)]
struct StatusEntry {
    status: u32,
    group: u8,
    time: i32,
}

/// `SEvent_`
#[derive(Debug, Clone, Default)]
struct SEvent {
    event_type: i32,
    /// Camera-space offset in units of the SE cutoff distance.
    position: Vec3,
    entries: [StatusEntry; 16],
    track: Option<usize>,
    outer: Option<usize>,
    queue: usize,
    frame_timer: i32,
}

/// `BgmControl_`
#[derive(Debug, Clone, Default)]
struct Bgm {
    active: bool,
    needs_init: bool,
    mode_changed: bool,
    mode: u8,
    battle: u8,
    normal: u8,
    quiet: u8,
    fade_out: u8,
    song: usize,
    slot: usize,
    handle: Option<usize>,
    transition: u32,
    track_outer: [Option<usize>; 16],
    track_volumes: [f32; 16],
    game_volume: f32,
    crossfade: u32,
}

/// A sound context (`SeContext`, soundMgr.cpp): an emitter the game plays sounds through.
#[derive(Debug, Clone, Default)]
struct Context {
    /// The creature that owns it, for [`SoundSystem::play_from`]; `None` for one-off sounds.
    owner: Option<u64>,
    handle: Option<usize>,
    event_type: i32,
    position: Vec3,
    offset: Vec3,
}

pub struct SoundSystem {
    pub engine: Engine,
    pub tables: JaudioTables,
    queues: Vec<CmdQueue>,
    system_se: [usize; 2],
    orima_se: [usize; 2],
    /// `demo_q`: commands for the cutscene sound track (0x1000F port 2).
    demo_q: usize,
    /// `current_demo_no`
    demo: Option<u16>,
    events: Vec<SEvent>,
    events_ready: bool,
    current_time: i32,
    bgm: [Bgm; 3],
    bgm_calls: u32,
    fadeout_time: u32,
    game_bgm_volume: f32,
    game_bgm_level: u8,
    game_se_level: u8,
    volume_pending: bool,
    contexts: Vec<Context>,
    next_context: usize,
    /// `SeConstant` p00 (parms/sound.bin): the distance at which positional sounds fade out.
    pub se_cutoff: f32,
    listener: Vec3,
    view: Mat4,
    gaya_timer: u32,
    gaya_outer: Option<usize>,
    gaya_volume: f32,
    pikis: u32,
    flyready: bool,
    pulled: [u16; 3],
    out: VecDeque<f32>,
    resample_pos: f64,
    last: [f32; 2],
    next: [f32; 2],
}

impl SoundSystem {
    /// Boots the sound system: loads the preloaded sequences and starts the sound effect ones.
    pub fn new(data: Arc<SoundData>, tables: JaudioTables) -> Self {
        let mut engine = Engine::new(data.clone());
        // `Jac_InitBgm`: sequences 0, 1, 13 start in root slots 0, 1, 2.
        for (seq, slot) in [(0usize, 0usize), (1, 1), (13, 2)] {
            if let Some(bytes) = data.seqs.seq(seq)
                && let Some(h) = engine.set_seq_data(slot, Arc::from(bytes))
            {
                engine.start_seq(h);
            }
        }
        let mut s = SoundSystem {
            engine,
            tables,
            queues: Vec::new(),
            system_se: [0; 2],
            orima_se: [0; 2],
            demo_q: 0,
            demo: None,
            events: vec![SEvent::default(); 16],
            events_ready: false,
            current_time: 0,
            bgm: Default::default(),
            bgm_calls: 0,
            fadeout_time: 30,
            game_bgm_volume: 1.0,
            game_bgm_level: 8,
            game_se_level: 8,
            volume_pending: true,
            contexts: vec![Context::default(); 16],
            next_context: 0,
            se_cutoff: 700.0,
            listener: Vec3::ZERO,
            view: Mat4::IDENTITY,
            gaya_timer: 0,
            gaya_outer: None,
            gaya_volume: 0.0,
            pikis: 0,
            flyready: false,
            pulled: [4, 5, 6],
            out: VecDeque::new(),
            resample_pos: 0.0,
            last: [0.0; 2],
            next: [0.0; 2],
        };
        s.system_se = [
            s.add_queue(QueueTrack::Registered(0x10009), 0),
            s.add_queue(QueueTrack::Registered(0x10009), 1),
        ];
        s.orima_se = [
            s.add_queue(QueueTrack::Registered(0x1000A), 0),
            s.add_queue(QueueTrack::Registered(0x1000A), 2),
        ];
        s.demo_q = s.add_queue(QueueTrack::Registered(0x1000F), 2);
        for i in 0..16 {
            let q = s.add_queue(QueueTrack::Slot(usize::MAX), 0);
            s.events[i].queue = q;
        }
        // Let the boot sequences register their tracks.
        for _ in 0..40 {
            s.step_subframe(None);
        }
        s
    }

    /// Reads `SeConstant` from `parms/sound.bin` (p00 cutoff distance).
    pub fn set_parms(&mut self, sound_bin: &[u8]) {
        let mut at = 0;
        while at + 6 <= sound_bin.len() {
            let id = &sound_bin[at..at + 3];
            let size = sound_bin[at + 3] as usize;
            if &sound_bin[at..at + 4] == b"\xff\xff\xff\xff" || at + 4 + size > sound_bin.len() {
                break;
            }
            if id == b"p00" && size == 4 {
                self.se_cutoff = f32::from_be_bytes(sound_bin[at + 4..at + 8].try_into().unwrap());
            }
            at += 4 + size;
        }
    }

    fn add_queue(&mut self, handle: QueueTrack, port: usize) -> usize {
        self.queues.push(CmdQueue {
            handle,
            port,
            q: VecDeque::new(),
        });
        self.queues.len() - 1
    }

    fn queue_track(&self, q: usize) -> Option<usize> {
        match self.queues[q].handle {
            QueueTrack::Registered(id) => self.engine.track_handle(id),
            QueueTrack::Slot(t) => {
                (t != usize::MAX && self.engine.tracks[t].state != 0).then_some(t)
            }
        }
    }

    /// `Jal_SendCmdQueue_Noblock`
    fn send(&mut self, q: usize, v: u16) {
        if self.queues[q].q.len() < 16 {
            self.queues[q].q.push_back(v);
        }
    }

    /// `Jal_SendCmdQueue_Force`
    fn send_force(&mut self, q: usize, v: u16) {
        if self.queues[q].q.len() >= 16 {
            self.queues[q].q.pop_front();
        }
        self.queues[q].q.push_back(v);
    }

    /// `Jal_FrameWork`: hand each queue's next value to its port once the sequence read the
    /// last one.
    fn queue_framework(&mut self) {
        for q in 0..self.queues.len() {
            let Some(t) = self.queue_track(q) else {
                continue;
            };
            let port = self.queues[q].port;
            if self.engine.port_free(t, port)
                && let Some(v) = self.queues[q].q.pop_front()
            {
                self.engine.write_port(t, port, v);
            }
        }
    }

    // ------------------------------------------------------------ the audio clock

    /// One DSP subframe (`UpdateDSP`), mixed into `out` if given.
    fn step_subframe(&mut self, out: Option<&mut [f32; SUBFRAME * 2]>) {
        self.engine.update_sequences();
        self.engine.update_voices();
        // `PlayerCallback`: the BGM controller every 8th call, then the command queues.
        self.bgm_calls += 1;
        if self.bgm_calls & 7 == 0 {
            self.bgm_framework();
        }
        self.queue_framework();
        for m in std::mem::take(&mut self.engine.syncs) {
            self.track_receive(m.track_id, m.message, m.port2);
        }
        if !self.events_ready {
            self.init_event_system();
        }
        if self.volume_pending {
            self.game_volume(self.game_bgm_level, self.game_se_level);
        }
        if let Some(out) = out {
            self.engine.render_subframe(out);
        }
        self.engine.subframes += 1;
    }

    /// Renders interleaved stereo at `rate` Hz into `out`, running the sound system as needed.
    pub fn render(&mut self, out: &mut [f32], rate: f32) {
        let step = DAC_RATE as f64 / rate as f64;
        for frame in out.as_chunks_mut::<2>().0 {
            while self.resample_pos >= 1.0 || self.out.len() < 2 {
                if self.out.len() < 2 {
                    let mut buf = [0f32; SUBFRAME * 2];
                    self.step_subframe(Some(&mut buf));
                    self.out.extend(buf);
                }
                if self.resample_pos >= 1.0 {
                    self.last = self.next;
                    self.next = [self.out.pop_front().unwrap(), self.out.pop_front().unwrap()];
                    self.resample_pos -= 1.0;
                }
            }
            let f = self.resample_pos as f32;
            for c in 0..2 {
                frame[c] = (self.last[c] + (self.next[c] - self.last[c]) * f).clamp(-1.0, 1.0);
            }
            self.resample_pos += step;
        }
    }

    /// Renders `seconds` at the DSP's own rate (for tests and tools).
    pub fn render_native(&mut self, seconds: f32) -> Vec<f32> {
        let n = (seconds * DAC_RATE / SUBFRAME as f32) as usize;
        let mut v = Vec::with_capacity(n * SUBFRAME * 2);
        for _ in 0..n {
            let mut buf = [0f32; SUBFRAME * 2];
            self.step_subframe(Some(&mut buf));
            v.extend(buf);
        }
        v
    }

    // ------------------------------------------------------------ verysimple.c / piki_player.c

    /// `Jac_PlaySystemSe`
    pub fn play_system_se(&mut self, id: i32) {
        match id {
            // JACSYS_ContainerOK
            16 => return self.play_orima_se(14),
            // JACSYS_MenuOn / Pause
            6 | 3 => {
                if let Some(t) = self.engine.track_handle(0x20000) {
                    self.engine.pause_track(t, true);
                }
                if let Some(t) = self.engine.track_handle(0x1000A) {
                    self.engine.pause_track(t, true);
                }
            }
            // JACSYS_MenuOff / Unpause
            7 | 15 => {
                if let Some(t) = self.engine.track_handle(0x20000) {
                    self.engine.unpause_track(t, true);
                }
                if let Some(t) = self.engine.track_handle(0x1000A) {
                    self.engine.unpause_track(t, true);
                }
            }
            _ => {}
        }
        self.send(self.system_se[0], id as u16);
    }

    /// `Jac_StopSystemSe`
    pub fn stop_system_se(&mut self, id: i32) {
        if id == 16 {
            return self.stop_orima_se(14);
        }
        self.send(self.system_se[1], id as u16);
    }

    /// `Jac_PlayOrimaSe`
    pub fn play_orima_se(&mut self, id: u32) {
        let Some(t) = self.engine.track_handle(0x1000A) else {
            return;
        };
        if id & PIKISOUND != 0 {
            if !self.flyready || id != 0x8004 {
                self.engine.write_port(t, 1, (id & !PIKISOUND) as u16);
                match id {
                    0x8002 => self.flyready = false,
                    0x8004 => self.flyready = true,
                    _ => {}
                }
            }
            return;
        }
        self.send(self.orima_se[0], id as u16);
        if id == 4 {
            // JACORIMA_PikiPulled2: a Pikmin voice that avoids repeating itself.
            let r = (self.engine.rng.ulimit(4) & 3) as u16;
            let mut variant = if r != 3 { r + 0x800D } else { 0x800B };
            let [o1, o2, o3] = &mut self.pulled;
            if *o3 == *o2 && *o2 == *o1 {
                variant = if *o1 != r { r + 0x8010 } else { 0x8014 };
                *o1 = 5;
            } else {
                *o3 = *o2;
                *o2 = *o1;
                *o1 = r;
            }
            self.engine.write_port(t, 1, variant & !(PIKISOUND as u16));
        }
        if id == 12 {
            // JACORIMA_PlayerDown
            self.fade_out_bgm(0, 100);
            self.fade_out_bgm(1, 100);
        }
    }

    /// `Jac_StopOrimaSe`
    pub fn stop_orima_se(&mut self, id: u32) {
        if id & PIKISOUND == 0 {
            self.send(self.orima_se[1], id as u16);
        }
    }

    /// `Jac_Orima_Walk`: a footstep of type `kind`.
    pub fn orima_walk(&mut self, kind: u16) {
        self.gaya_timer = 0;
        if let Some(t) = self.engine.track_handle(0x10008) {
            self.engine.write_port(t, 0, kind);
        }
    }

    /// A cutscene's sound without the cutscene: `Jac_StartDemo` tells the demo track which
    /// scene it is (port 0), then `Jac_DemoSound` hands it command `cmd` (the scene's timed
    /// events, pikidemo.c). Only the track's own sounds; scene music and fades aren't done.
    pub fn demo_sound(&mut self, demo: u16, cmd: u16) {
        if self.demo != Some(demo) {
            let Some(t) = self.engine.track_handle(0x1000F) else { return };
            self.engine.write_port(t, 0, demo);
            self.demo = Some(demo);
        }
        self.send(self.demo_q, cmd);
    }

    /// `__Jac_FinishDemo`: the demo track goes quiet.
    pub fn end_demo(&mut self) {
        if self.demo.take().is_some()
            && let Some(t) = self.engine.track_handle(0x1000F)
        {
            self.engine.write_port(t, 0, 0);
        }
    }

    /// `Jac_Piki_Number`: how many Pikmin are around, for the chatter loop.
    pub fn set_piki_count(&mut self, n: u32) {
        self.pikis = match n {
            100.. => 29,
            50.. => (n - 50) / 10 + 25,
            25.. => (n - 25) / 5 + 20,
            15.. => (n - 15) / 2 + 15,
            _ => n,
        };
    }

    /// `Jac_UpdatePikiGaya`: the squad's chatter fades in after Olimar stands still a while.
    fn update_gaya(&mut self) {
        let Some(t) = self.engine.track_handle(0x10003) else {
            return;
        };
        let o = match self.gaya_outer {
            Some(o) => o,
            None => {
                let o = self.engine.new_outer();
                self.engine.tracks[t].outer = Some(o);
                self.engine.outer_on(o, outer::VOLUME as u16);
                self.gaya_outer = Some(o);
                o
            }
        };
        if self.gaya_timer < 150 || self.pikis < 1 {
            self.engine
                .set_outer_param(o, self.gaya_volume, outer::VOLUME);
            self.gaya_volume = (self.gaya_volume - 0.05).max(0.0);
            if self.pikis == 0 {
                self.engine.write_port(t, 0, 0);
            }
        } else if self.gaya_timer >= 240 {
            self.engine.write_port(t, 0, self.pikis as u16);
            self.gaya_volume = (self.gaya_volume + 0.05).min(1.0);
            self.engine
                .set_outer_param(o, self.gaya_volume, outer::VOLUME);
        }
        self.gaya_timer += 1;
    }

    /// `Jac_GameVolume`: the options screen's levels (0-10, default 8).
    pub fn game_volume(&mut self, bgm: u8, se: u8) {
        self.game_bgm_level = bgm.min(10);
        self.game_se_level = se.min(10);
        let (Some(se_t), Some(ev_t)) = (
            self.engine.track_handle(0x10000),
            self.engine.track_handle(0x20000),
        ) else {
            self.volume_pending = true;
            return;
        };
        self.volume_pending = false;
        self.game_bgm_volume = self.tables.game_bgm_vol[self.game_bgm_level as usize];
        let se_vol = self.tables.game_se_vol[self.game_se_level as usize];
        if let Some(t) = self.engine.track_handle(0x30000) {
            let demo = self.tables.game_demo_vol[self.game_bgm_level as usize];
            self.engine.write_port(t, 2, demo);
        }
        self.engine.write_port(se_t, 0, se_vol);
        self.engine.write_port(ev_t, 0, se_vol);
        let slot = 0;
        self.bgm[slot].game_volume = self.game_bgm_volume;
        self.bgm[slot].crossfade = 10;
    }

    /// `TrackReceive` (verysimple.c): `SyncCPU` messages from the event tracks.
    fn track_receive(&mut self, track_id: u32, msg: u16, port2: u16) {
        if msg & 0x8000 == 0x8000 {
            let child = ((track_id & 0xF0) >> 4) as usize;
            let slot = (track_id & 0xF) as usize;
            if (track_id & 0xF00) >> 8 == 1 {
                self.mml_stop_event_action(child, slot, msg & 0xFFF);
            }
        }
        if msg & 0x9000 == 0x9000 {
            self.mml_stop_event_all((track_id & 0xF) as usize, port2);
        }
    }

    // ------------------------------------------------------------ pikiinter.c

    /// `Jac_InitEventSystem`
    fn init_event_system(&mut self) {
        let Some(parent) = self.engine.track_handle(0x20000) else {
            return;
        };
        for i in 0..16 {
            let Some(c) = self.engine.tracks[parent].children[i] else {
                continue;
            };
            let o = self.engine.new_outer();
            self.engine.tracks[c].outer = Some(o);
            self.engine.outer_on(o, 9);
            self.events[i].track = Some(c);
            self.events[i].outer = Some(o);
            self.events[i].event_type = 0;
            self.queues[self.events[i].queue].handle = QueueTrack::Slot(c);
        }
        self.events_ready = true;
    }

    /// `Jac_CreateEvent`: `pos` is the camera-space offset (see [`Self::set_listener`]).
    fn create_event(&mut self, event_type: i32, pos: Vec3) -> Option<usize> {
        if event_type == 0 || !self.events_ready {
            return None;
        }
        let i = self.events.iter().position(|e| e.event_type == 0)?;
        let e = &mut self.events[i];
        e.event_type = event_type;
        e.position = Vec3::new(pos.x, pos.z, pos.y);
        e.entries = Default::default();
        e.frame_timer = 100;
        if event_type == 7 {
            self.play_event_action(i, 4);
        }
        Some(i)
    }

    /// `Jac_UpdateEventPosition`
    fn update_event_position(&mut self, i: usize, pos: Vec3) {
        if self.events[i].event_type != 0 {
            self.events[i].position = pos;
            self.events[i].frame_timer = 100;
        }
    }

    /// `Jac_PlayEventAction`
    fn play_event_action(&mut self, i: usize, action: i32) -> bool {
        let ev = &self.events[i];
        if ev.event_type == 0 {
            return false;
        }
        let Some(&off) = self.tables.event_offset.get(ev.event_type as usize) else {
            return false;
        };
        let status = (action + off) as usize;
        let Some(&st) = self.tables.action_status.get(status) else {
            return false;
        };
        let prio_of = |s: u32| {
            self.tables
                .action_status
                .get(s as usize)
                .map_or(0, |a| a.prio)
        };
        let used = ev.entries.iter().take_while(|e| e.status != 0).count();
        let mut target = used;
        if st.flags & 0x10 != 0 {
            let max = match st.flags & 0xF {
                0 => 16,
                n => n as usize,
            };
            let mut count = 0;
            let mut oldest_time = 0;
            let mut oldest = 0;
            let mut prio = st.prio;
            for (k, e) in ev.entries.iter().enumerate() {
                if e.status == 0 {
                    continue;
                }
                if e.group == st.group {
                    count += 1;
                }
                let p = prio_of(e.status);
                if prio > p {
                    oldest_time = self.current_time - e.time;
                    oldest = k;
                    prio = p;
                }
                if prio == p && self.current_time - e.time > oldest_time {
                    oldest_time = self.current_time - e.time;
                    oldest = k;
                }
            }
            if count >= max || target == 16 {
                if st.flags & 0x20 != 0 {
                    return false;
                }
                target = oldest;
            }
        } else {
            let mut first_free = 16;
            let mut k = 0;
            while k < 16 {
                let e = ev.entries[k];
                if e.status == 0 {
                    first_free = k;
                    k += 1;
                    continue;
                }
                if e.group != st.group {
                    k += 1;
                    continue;
                }
                if st.flags & 0x20 != 0 {
                    if st.prio > prio_of(e.status) {
                        target = k;
                    } else {
                        return false;
                    }
                } else {
                    target = k;
                }
                break;
            }
            if k == 16 {
                target = first_free;
            }
        }
        if target == 16 {
            let mut lowest = st.prio + 1;
            for (k, e) in ev.entries.iter().enumerate() {
                let p = prio_of(e.status);
                if p < lowest {
                    lowest = p;
                    target = k;
                }
            }
            if target == 16 {
                return false;
            }
        }
        let q = self.events[i].queue;
        self.send_force(q, ((target as u16) << 12) | st.cmd);
        let t = self.current_time;
        self.events[i].entries[target] = StatusEntry {
            status: status as u32,
            group: st.group,
            time: t,
        };
        true
    }

    /// `Jac_StopEventAction`
    fn stop_event_action(&mut self, i: usize, action: i32) {
        let ev = &self.events[i];
        if ev.event_type == 0 {
            return;
        }
        let off = self.tables.event_offset[ev.event_type as usize % 8];
        let status = (action + off) as u32;
        for k in 0..16 {
            if self.events[i].entries[k].status == status {
                let q = self.events[i].queue;
                self.send_force(q, (k as u16) << 12);
                self.events[i].entries[k].status = 0;
            }
        }
    }

    /// `MML_StopEventAction`: the sequence says a sound finished.
    fn mml_stop_event_action(&mut self, i: usize, slot: usize, cmd: u16) {
        let Some(ev) = self.events.get_mut(i) else {
            return;
        };
        if ev.event_type == 0 || slot >= 16 {
            return;
        }
        let s = ev.entries[slot].status as usize;
        if self.tables.action_status.get(s).map(|a| a.cmd) == Some(cmd) {
            ev.entries[slot].status = 0;
        }
    }

    /// `MML_StopEventAll`
    fn mml_stop_event_all(&mut self, i: usize, active: u16) {
        let Some(ev) = self.events.get_mut(i) else {
            return;
        };
        if ev.event_type == 0 {
            return;
        }
        for k in 0..16 {
            if active & (1 << k) == 0 {
                ev.entries[k].status = 0;
            }
        }
    }

    /// `Jac_DestroyEvent`
    fn destroy_event(&mut self, i: usize) {
        if self.events[i].event_type == 0 {
            return;
        }
        for e in self.events[i].entries.iter_mut() {
            e.status = 0;
        }
        let q = self.events[i].queue;
        self.send_force(q, 0xFFFF);
        self.events[i].event_type = 0;
    }

    /// `Jac_UpdateCamera`: each event's volume and pan from its offset.
    fn update_camera(&mut self) {
        self.current_time += 1;
        for i in 0..16 {
            let e = &self.events[i];
            if e.event_type == 0 {
                continue;
            }
            let scale = self.tables.dist_scale[e.event_type as usize % 8];
            let volume = 1.0 - (e.position.length() * scale).min(1.0);
            let mut angle = e.position.x.atan2(e.position.y) + std::f32::consts::PI;
            if angle >= JAC_PI {
                angle -= 2.0 * JAC_PI;
            }
            let mut pan = 0.5 - angle / JAC_PI;
            if pan > 1.0 {
                pan = 2.0 - pan;
            }
            if pan < 0.0 {
                pan = -pan;
            }
            // `SendToStack` -> `__SetVolandPan`
            if let Some(o) = e.outer {
                self.engine.set_outer_param(o, volume, outer::VOLUME);
                self.engine.set_outer_param(o, pan, outer::PAN);
            }
        }
        self.update_gaya();
    }

    /// `Jac_EventFrameCheck`: events nobody updates for 100 frames are silenced.
    fn event_frame_check(&mut self) {
        for i in 0..16 {
            let e = &mut self.events[i];
            if e.event_type == 0 || e.frame_timer == 0 {
                continue;
            }
            e.frame_timer -= 1;
            if e.frame_timer != 0 {
                continue;
            }
            // (The original clears `statusEntries[i]` here, indexing by the event.)
            if let Some(x) = e.entries.get_mut(i) {
                x.status = 0;
            }
            let q = e.queue;
            self.send_force(q, 0xFFFF);
        }
    }

    // ------------------------------------------------------------ soundMgr.cpp (SeSystem)

    /// The camera for positional sounds: `view` is the world-to-camera matrix, `listener` the
    /// point sounds are measured from (`SeSystem::update`'s listener position).
    pub fn set_listener(&mut self, view: Mat4, listener: Vec3) {
        self.view = view;
        self.listener = listener;
    }

    /// `SeSystem::calcCameraPos`
    fn camera_offset(&self, pos: Vec3) -> Vec3 {
        (self.view.transform_point3(pos) - self.view.transform_point3(self.listener))
            / self.se_cutoff
    }

    /// Once per game frame (30 Hz): `SeSystem::update` and `Jac_Gsync`.
    pub fn frame(&mut self) {
        for i in 0..self.contexts.len() {
            if let Some(h) = self.contexts[i].handle {
                let off = self.camera_offset(self.contexts[i].position);
                self.contexts[i].offset = off;
                self.update_event_position(h, off);
            }
        }
        self.update_camera();
        self.event_frame_check();
    }

    /// Plays an `SE_*` sound by name (`seSystem->playSysSe` / `playPlayerSe` /
    /// `playSoundDirect`, by the sound's event type). `pos` is where positional sounds come
    /// from.
    pub fn play(&mut self, name: &str, pos: Vec3) -> bool {
        let Some(s) = self.tables.sound(name).cloned() else {
            return false;
        };
        match s.event {
            event_type::SYSTEM => self.play_system_se(s.jac_id),
            event_type::PLAYER => self.play_orima_se(s.jac_id as u32),
            ev => self.play_sound_direct(ev, s.jac_id, pos),
        }
        true
    }

    /// Stops a sound started with [`Self::play`].
    pub fn stop(&mut self, name: &str) {
        let Some(s) = self.tables.sound(name).cloned() else {
            return;
        };
        match s.event {
            event_type::SYSTEM => self.stop_system_se(s.jac_id),
            event_type::PLAYER => self.stop_orima_se(s.jac_id as u32),
            ev => {
                for i in 0..self.contexts.len() {
                    if self.contexts[i].event_type == ev
                        && let Some(h) = self.contexts[i].handle
                    {
                        self.stop_event_action(h, s.jac_id);
                    }
                }
            }
        }
    }

    /// Plays a sound from a creature's own emitter (`Creature::playEventSound` on its
    /// `SeContext`): its sounds share one event that follows the creature. `owner` is any id the
    /// host keeps stable for that creature.
    pub fn play_from(&mut self, owner: u64, name: &str, pos: Vec3) -> bool {
        let Some(s) = self.tables.sound(name).cloned() else {
            return false;
        };
        if s.event < 1 {
            return self.play(name, pos);
        }
        let i = match self.contexts.iter().position(|c| c.owner == Some(owner)) {
            Some(i) => i,
            None => {
                let i = self.next_context;
                self.next_context = (i + 1) % self.contexts.len();
                if let Some(h) = self.contexts[i].handle.take() {
                    self.destroy_event(h);
                }
                self.contexts[i] = Context {
                    owner: Some(owner),
                    event_type: s.event,
                    ..Context::default()
                };
                i
            }
        };
        self.contexts[i].position = pos;
        if self.contexts[i].handle.is_none() {
            let off = self.camera_offset(pos);
            self.contexts[i].offset = off;
            self.contexts[i].handle = self.create_event(self.contexts[i].event_type, off);
        }
        if let Some(h) = self.contexts[i].handle {
            self.play_event_action(h, s.jac_id);
        }
        true
    }

    /// Moves a creature's emitter.
    pub fn move_emitter(&mut self, owner: u64, pos: Vec3) {
        if let Some(c) = self.contexts.iter_mut().find(|c| c.owner == Some(owner)) {
            c.position = pos;
        }
    }

    /// Stops a sound on a creature's emitter (`Creature::stopEventSound`).
    pub fn stop_from(&mut self, owner: u64, name: &str) {
        let Some(s) = self.tables.sound(name).cloned() else {
            return;
        };
        if let Some(h) = self.contexts.iter().find(|c| c.owner == Some(owner)).and_then(|c| c.handle) {
            self.stop_event_action(h, s.jac_id);
        }
    }

    /// The creature is gone (`SeContext::releaseEvent`).
    pub fn release_emitter(&mut self, owner: u64) {
        if let Some(i) = self.contexts.iter().position(|c| c.owner == Some(owner)) {
            if let Some(h) = self.contexts[i].handle.take() {
                self.destroy_event(h);
            }
            self.contexts[i].owner = None;
        }
    }

    /// `SeSystem::playSoundDirect`: reuse a nearby emitter of the same type, else a new one.
    fn play_sound_direct(&mut self, event_type: i32, jac_id: i32, pos: Vec3) {
        let mut best = None;
        let mut min = 200.0;
        for (i, c) in self.contexts.iter().enumerate() {
            if c.handle.is_some() && c.owner.is_none() && c.event_type == event_type {
                let d = (c.position - pos).length();
                if d <= min {
                    min = d;
                    best = Some(i);
                }
            }
        }
        let i = match best {
            Some(i) => i,
            None => {
                let i = self.next_context;
                self.next_context = (i + 1) % self.contexts.len();
                if let Some(h) = self.contexts[i].handle.take() {
                    self.destroy_event(h);
                }
                self.contexts[i] = Context {
                    owner: None,
                    handle: None,
                    event_type,
                    position: pos,
                    offset: Vec3::ZERO,
                };
                i
            }
        };
        if self.contexts[i].handle.is_none() {
            let off = self.camera_offset(pos);
            self.contexts[i].offset = off;
            self.contexts[i].handle = self.create_event(event_type, off);
        }
        if let Some(h) = self.contexts[i].handle {
            self.play_event_action(h, jac_id);
        }
    }

    // ------------------------------------------------------------ piki_bgm.c

    /// `Jac_PlayBgm`: `slot` 0 is the main music, 1 the boss music.
    pub fn play_bgm(&mut self, slot: usize, bgm_id: usize) {
        let slot = slot.min(1);
        if self.bgm[slot].active {
            self.stop_bgm(slot);
        }
        let id = bgm_id.max(2);
        let Some(bytes) = self.engine.data.seqs.seq(id).map(Arc::<[u8]>::from) else {
            return;
        };
        let b = &mut self.bgm[slot];
        *b = Bgm {
            active: true,
            needs_init: true,
            song: id - 2,
            slot: slot + 3,
            ..Bgm::default()
        };
        for flag in [2, 1, 4, 8] {
            self.set_bgm_mode_flag(slot, flag, 0);
        }
        let handle = self.engine.set_seq_data(slot + 3, bytes);
        self.bgm[slot].handle = handle;
        self.bgm_framework();
        let t = slot + 3;
        // `Jaq_SetBankNumber`: the song's own instrument bank.
        let lo = self.engine.tracks[t].regs[6] & 0xFF;
        self.engine.tracks[t].regs[6] = (id as u16) << 8 | lo;
        self.bgm[slot].crossfade = 0;
        if slot == 0 {
            self.bgm[slot].game_volume = self.game_bgm_volume;
            self.engine.mute_track(t, 0);
        } else {
            self.bgm[slot].game_volume = 0.0;
            self.engine.mute_track(t, 1);
        }
        if let Some(o) = self.engine.tracks[t].outer {
            self.engine
                .set_outer_param(o, self.bgm[slot].game_volume, outer::VOLUME);
            self.engine.outer_on(o, outer::VOLUME as u16);
            self.engine.outer_off(o, outer::TEMPO as u16);
        }
        self.engine.tracks[t].pause_status = 74;
        if let Some(h) = handle {
            self.engine.start_seq(h);
        }
    }

    /// `Jac_StopBgm`
    pub fn stop_bgm(&mut self, slot: usize) {
        if self.bgm[slot].active {
            if let Some(h) = self.bgm[slot].handle.take() {
                self.engine.stop_seq(h);
                // `Jaf_StopSeq` waits until the sequence is gone.
                for _ in 0..4 {
                    self.engine.update_sequences();
                }
            }
            self.bgm[slot].active = false;
        }
    }

    /// `Jac_FadeOutBgm`
    pub fn fade_out_bgm(&mut self, slot: usize, frames: u32) {
        self.fadeout_time = frames;
        self.set_bgm_mode_flag(slot, 8, 1);
    }

    /// `Jac_SetBgmModeFlag`: 1 normal mix, 2 battle mix, 4 quiet (menus), 8 fade out.
    pub fn set_bgm_mode_flag(&mut self, slot: usize, flag: u8, on: u8) {
        let b = &mut self.bgm[slot];
        match flag {
            2 => b.battle = on,
            1 => b.normal = on,
            4 => b.quiet = on,
            8 => b.fade_out = on,
            _ => {}
        }
        let mode = b.normal + (b.battle << 1) + (b.quiet << 2) + (b.fade_out << 3);
        if mode != b.mode && b.active {
            b.mode = mode;
            b.mode_changed = true;
        }
    }

    /// `Jac_BgmFrameWork`
    fn bgm_framework(&mut self) {
        for i in 0..3 {
            if !self.bgm[i].active {
                continue;
            }
            if self.bgm[i].needs_init {
                self.change_bgm_track_vol(i);
                continue;
            }
            if self.bgm[i].mode_changed {
                self.change_bgm_track_vol(i);
                self.bgm[i].mode_changed = false;
            }
            if self.bgm[i].transition != 0 {
                self.move_bgm_track_vol(i);
            }
            if self.bgm[i].crossfade != 0 {
                let t = self.bgm[i].slot;
                let Some(o) = self.engine.tracks[t].outer else {
                    self.bgm[i].crossfade = 0;
                    continue;
                };
                let a = self.bgm[i].crossfade as f32;
                let cur = self.engine.outers[o].volume;
                self.engine.set_outer_param(
                    o,
                    cur + (self.bgm[i].game_volume - cur) / a,
                    outer::VOLUME,
                );
                self.bgm[i].crossfade -= 1;
            }
        }
    }

    /// `Jac_ChangeBgmTrackVol`: which of the song's 16 tracks play in the current mix.
    fn change_bgm_track_vol(&mut self, i: usize) {
        let song = self.bgm[i].song.min(19);
        let mode = self.bgm[i].mode;
        let mute = self.tables.bgm_mute[song][(mode & 3) as usize];
        let mut vol = self.tables.bgm_volume[song][((mode >> 2) & 1) as usize];
        if mode & 8 != 0 {
            vol = 0.0;
        }
        for k in 0..16 {
            self.bgm[i].track_volumes[k] = if (mute >> k) & 1 != 0 { vol } else { 0.0 };
        }
        let t = self.bgm[i].slot;
        if self.bgm[i].needs_init {
            for k in 0..16 {
                let o = self.engine.new_outer();
                self.bgm[i].track_outer[k] = Some(o);
                // `Jam_AssignExtBufferP`: children opened later pick this up.
                self.engine.tracks[t].child_outer[k] = Some(o);
                if let Some(c) = self.engine.tracks[t].children[k] {
                    self.engine.tracks[c].outer = Some(o);
                }
                let v = self.bgm[i].track_volumes[k];
                self.engine.set_outer_param(o, v, outer::VOLUME);
                self.engine.outer_on(o, outer::VOLUME as u16);
            }
            self.bgm[i].needs_init = false;
        } else if mode & 8 != 0 {
            self.bgm[i].transition = self.fadeout_time;
        } else {
            self.bgm[i].transition = 60;
        }
    }

    /// `Jac_MoveBgmTrackVol`
    fn move_bgm_track_vol(&mut self, i: usize) {
        let a = self.bgm[i].transition as f32;
        self.bgm[i].transition -= 1;
        for k in 0..16 {
            if let Some(o) = self.bgm[i].track_outer[k] {
                let cur = self.engine.outers[o].volume;
                let v = cur + (self.bgm[i].track_volumes[k] - cur) / a;
                self.engine.set_outer_param(o, v, outer::VOLUME);
            }
        }
    }

    /// Whether a sequence track is registered (for hosts waiting on boot).
    pub fn has_track(&self, id: u32) -> bool {
        self.engine.track_handle(id).is_some()
    }

    /// Voices currently sounding.
    pub fn active_voices(&self) -> usize {
        self.engine
            .voices
            .iter()
            .filter(|v| v.enabled && !v.done)
            .count()
    }
}
