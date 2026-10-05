//! The state JAudio keeps in globals, as one struct: sequence tracks (`seqp_`), channel
//! managers (`jcs_`), logical channels (`jc_`) and DSP voices (`dspch_` + `DSPchannel_`).
//! The behaviour lives in `seq.rs` (jammain_2.c, seqsetup.c, noteon.c, jamosc.c),
//! `channel.rs` (oneshot.c, driverinterface.c) and `dsp.rs` (dspdriver.c and the DSP's mixer).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use libpikmin_assets::sound::SoundData;

use crate::calc::Random;
use crate::osc::{Osc, OscBuf, presets};

/// `SEQ_SIZE` (seqsetup.c) pool tracks, after the 16 `rootseq` tracks of file_seq.c.
pub const ROOT_TRACKS: usize = 16;
pub const POOL_TRACKS: usize = 256;
/// `CHANNEL_SIZE` (driverinterface.c).
pub const CHANNELS: usize = 256;
/// `DSPCH_LENGTH` (dspdriver.c).
pub const VOICES: usize = 64;

/// `MoveParam_`: a parameter gliding to a target.
#[derive(Debug, Clone, Copy, Default)]
pub struct Move {
    pub current: f32,
    pub target: f32,
    pub duration: f32,
    pub step: f32,
}

/// Indices into `TimedParam_::move`.
pub mod tp {
    pub const VOLUME: usize = 0;
    pub const PITCH: usize = 1;
    pub const FXMIX: usize = 2;
    pub const PAN: usize = 3;
    pub const DOLBY: usize = 4;
    pub const DIST_FILTER: usize = 5;
    pub const IIR0: usize = 12;
    pub const PAUSE_VOLUME: usize = 16;
    pub const PAN_DELAY: usize = 17;
}

/// `OuterParamFlag`
pub mod outer {
    pub const VOLUME: u32 = 1;
    pub const PITCH: u32 = 2;
    pub const FXMIX: u32 = 4;
    pub const PAN: u32 = 8;
    pub const DOLBY: u32 = 0x10;
    pub const DIST_FILT: u32 = 0x20;
    pub const TEMPO: u32 = 0x40;
    pub const MASTER_LEVEL: u32 = 1 << 17;
}

/// `OuterParam_`: parameters the game (or an `EXTSet` buffer) layers over a track.
#[derive(Debug, Clone, Copy, Default)]
pub struct OuterParam {
    pub flags: u16,
    pub update_flags: u16,
    pub volume: f32,
    pub fxmix: f32,
    pub dolby: f32,
    pub pitch: f32,
    pub pan: f32,
    pub tempo: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Port {
    pub import: bool,
    pub export: bool,
    pub value: u16,
}

/// A channel manager (`jcs_`): one per track, plus the global one.
#[derive(Debug, Clone)]
pub struct Mgr {
    pub alloc_count: u32,
    pub active: Vec<u16>,
    pub releasing: Vec<u16>,
    pub waiting: Vec<u16>,
    pub volume: f32,
    pub pitch: f32,
    pub pan: f32,
    pub fxmix: f32,
    pub dolby: f32,
    pub bus_connect: [u16; 6],
    pub master_levels: [u8; 6],
    pub channel_priority: u32,
    pub release_time: u16,
    pub pan_calc: [u8; 3],
}

impl Default for Mgr {
    /// `InitJcs`
    fn default() -> Self {
        Mgr {
            alloc_count: 0,
            active: Vec::new(),
            releasing: Vec::new(),
            waiting: Vec::new(),
            volume: 1.0,
            pitch: 1.0,
            pan: 0.5,
            fxmix: 0.0,
            dolby: 0.0,
            bus_connect: [0x150, 0x210, 0x352, 0x412, 0, 0],
            master_levels: [0; 6],
            channel_priority: 0x20103,
            release_time: 600,
            pan_calc: [26, 1, 1],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MgrId {
    Global,
    Track(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Active,
    Releasing,
    Waiting,
}

/// A sequence track (`seqp_`).
#[derive(Debug, Clone)]
pub struct Track {
    /// `trackState`: 0 free, 1 playing, 2 ready, 3 stop requested.
    pub state: u8,
    pub allocated: bool,
    /// Bumped whenever the slot is reused, so channels holding its oscillators notice.
    pub generation: u32,
    pub data: Arc<[u8]>,
    pub pc: u32,
    pub call_stack: [u32; 8],
    pub loop_counters: [u16; 8],
    pub depth: u32,
    pub flags: u8,
    pub parent: Option<usize>,
    pub children: [Option<usize>; 16],
    pub connection_id: u32,
    pub track_id: u32,
    pub wait: i32,
    pub was_note_played: u32,
    pub active_notes: [u8; 8],
    pub channels: [Option<u16>; 8],
    pub sound_ids: [u16; 8],
    pub gate_time: u8,
    pub last_velocity: u8,
    pub note_duration: u32,
    pub note_flags: u8,
    pub last_note: u8,
    pub gate_mode: bool,
    pub jcs: Mgr,
    pub timed: [Move; 18],
    /// `URegisterParam_::reg`: 3 = result, 6 = bank/program, 7 = pitch scale, 8-12 = pan power
    /// arguments, 13 = priority, 16-23 = the four 32-bit extended registers.
    pub regs: [u16; 32],
    pub outer: Option<usize>,
    pub child_outer: [Option<usize>; 16],
    pub ports: [Port; 16],
    pub tempo_acc: f32,
    pub tempo_factor: f32,
    pub time_base: u16,
    pub tempo: u16,
    pub time_relation: u8,
    pub oscs: [Osc; 2],
    pub osc_route: [u8; 2],
    pub osc_bufs: [OscBuf; 2],
    pub transpose: i8,
    pub final_transpose: u8,
    pub tick: i32,
    pub paused: bool,
    pub pause_status: u8,
    pub muted: u8,
    pub child_mute: u16,
    pub int_active: u8,
    pub int_pending: u8,
    pub int_enable: u8,
    pub timer_count: u8,
    pub int_addr: [u32; 8],
    pub saved_pc: u32,
    pub saved_wait: i32,
    pub timer: u32,
    pub max_time: u32,
    pub update_flags: u32,
    pub pan_calc: [u8; 3],
    pub parent_pan_calc: [u8; 3],
    pub registered: bool,
    pub needs_tempo_sync: bool,
}

impl Default for Track {
    fn default() -> Self {
        Track {
            state: 0,
            allocated: false,
            generation: 0,
            data: Arc::from(Vec::new()),
            pc: 0,
            call_stack: [0; 8],
            loop_counters: [0; 8],
            depth: 0,
            flags: 0,
            parent: None,
            children: [None; 16],
            connection_id: 0,
            track_id: 0,
            wait: 0,
            was_note_played: 0,
            active_notes: [0xFF; 8],
            channels: [None; 8],
            sound_ids: [0; 8],
            gate_time: 0,
            last_velocity: 0,
            note_duration: 0,
            note_flags: 0,
            last_note: 0,
            gate_mode: false,
            jcs: Mgr::default(),
            timed: [Move::default(); 18],
            regs: [0; 32],
            outer: None,
            child_outer: [None; 16],
            ports: [Port::default(); 16],
            tempo_acc: 0.0,
            tempo_factor: 1.0,
            time_base: 48,
            tempo: 120,
            time_relation: 1,
            oscs: [presets::envelope(), presets::envelope()],
            osc_route: [15; 2],
            osc_bufs: [OscBuf::default(); 2],
            transpose: 0,
            final_transpose: 0,
            tick: 0,
            paused: false,
            pause_status: 0,
            muted: 0,
            child_mute: 0,
            int_active: 0,
            int_pending: 0,
            int_enable: 0,
            timer_count: 0,
            int_addr: [0; 8],
            saved_pc: 0,
            saved_wait: 0,
            timer: 0,
            max_time: 0,
            update_flags: 0,
            pan_calc: [2; 3],
            parent_pan_calc: [2; 3],
            registered: false,
            needs_tempo_sync: false,
        }
    }
}

/// Where a channel's oscillator comes from: the bank (fixed), or a track's oscillator slot,
/// which the track can keep changing while the note plays (`Effecter_Overwrite_1ShotD` stores
/// a pointer to `seqp_::oscillators`).
#[derive(Debug, Clone)]
pub enum OscRef {
    Own(Osc),
    Track {
        track: usize,
        index: usize,
        generation: u32,
        copy: Osc,
    },
}

/// A logical channel (`jc_`).
#[derive(Debug, Clone)]
pub struct Channel {
    pub velocity: u8,
    /// 0xFF once stopped.
    pub note: u8,
    pub pause: bool,
    pub to_flush: u8,
    pub mgr: MgrId,
    pub list: Option<ListKind>,
    /// `logicalChanType`: 0 wave, 2 oscillator.
    pub kind: u8,
    /// (physical wsys, archive, wave)
    pub wave: Option<(usize, usize, usize)>,
    pub osc_type: u32,
    pub voice: Option<u8>,
    /// `updateCallback` is `Jesus1Shot_Update`.
    pub has_updater: bool,
    pub sweeping: bool,
    pub note_id: i32,
    pub last_note_played: i32,
    pub oscs: [Option<OscRef>; 4],
    pub bufs: [OscBuf; 4],
    pub base_pitch: f32,
    pub base_volume: f32,
    pub current_pitch: f32,
    pub current_volume: f32,
    /// 0 power distribution, 1 pan, 2 fx send, 3 dolby; each (note, effect, manager).
    pub pan: [[f32; 3]; 4],
    pub pitch_mod: f32,
    pub volume_mod: f32,
    pub target_pitch: f32,
    pub final_pitch: u16,
    pub sweep_steps: u16,
    pub last_mgr: Option<MgrId>,
    pub mgr_pitch: f32,
    pub mgr_volume: f32,
    pub bus: [u16; 6],
    pub levels: [u16; 6],
    pub priority: u32,
    pub release_time: u16,
    pub id: u16,
    pub sound_id: u32,
    pub poly: u8,
    pub pan_calc: [u8; 3],
    pub in_use: bool,
}

impl Default for Channel {
    fn default() -> Self {
        Channel {
            velocity: 0,
            note: 0xFF,
            pause: false,
            to_flush: 0,
            mgr: MgrId::Global,
            list: None,
            kind: 0,
            wave: None,
            osc_type: 0,
            voice: None,
            has_updater: false,
            sweeping: false,
            note_id: 0,
            last_note_played: 0,
            oscs: [None, None, None, None],
            bufs: [OscBuf::default(); 4],
            base_pitch: 1.0,
            base_volume: 1.0,
            current_pitch: 1.0,
            current_volume: 1.0,
            pan: [[0.0; 3]; 4],
            pitch_mod: 1.0,
            volume_mod: 1.0,
            target_pitch: 0.0,
            final_pitch: 0,
            sweep_steps: 0,
            last_mgr: None,
            mgr_pitch: 1.0,
            mgr_volume: 1.0,
            bus: [0x150, 0x210, 0x352, 0x412, 0, 0],
            levels: [0; 6],
            priority: 0x10101,
            release_time: 600,
            id: 0,
            sound_id: 0,
            poly: 0,
            pan_calc: [26, 1, 1],
            in_use: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub enum VoiceSource {
    #[default]
    None,
    Wave {
        pcm: Arc<Vec<i16>>,
        looping: bool,
        loop_start: u32,
        end: u32,
    },
    Osc(u32),
}

/// A DSP voice (`dspch_` and the DSP's `DSPchannel_`).
#[derive(Debug, Clone, Default)]
pub struct Voice {
    /// `DSPChannelAllocState`: 0 free, 1 mono, 4 stopping.
    pub alloc: u8,
    pub owner: Option<u16>,
    /// `logicalChanCb` is `CommonCallbackLogicalChannel`.
    pub has_callback: bool,
    pub callback_timer: u16,
    pub prio: u8,
    pub release_time: u16,
    // DSP side
    pub enabled: bool,
    pub done: bool,
    pub end_requested: bool,
    pub age: u32,
    pub paused: bool,
    pub source: VoiceSource,
    pub pos: f64,
    pub pitch: u16,
    pub level: [f32; 6],
    pub target: [f32; 6],
    pub bus: [u8; 6],
    pub osc_phase: f64,
}

/// A message the sequence sent with `SyncCPU` (handled by the game layer's `TrackReceive`),
/// with port 2 read at the time for 0x9xxx messages.
#[derive(Debug, Clone, Copy)]
pub struct SyncMessage {
    pub track_id: u32,
    pub message: u16,
    pub port2: u16,
}

pub struct Engine {
    pub data: Arc<SoundData>,
    pub rng: Random,
    pub tracks: Vec<Track>,
    pub free_tracks: VecDeque<usize>,
    /// seqsetup.c `rootseq[]`: root handles to track slots.
    pub roots: [Option<usize>; 16],
    /// `TRACK_LIST`: (connection id, track).
    pub registry: Vec<(u32, Option<usize>)>,
    pub outers: Vec<OuterParam>,
    pub global: Mgr,
    pub channels: Vec<Channel>,
    pub free_channels: Vec<u16>,
    pub voices: Vec<Voice>,
    /// `waitp` ring: channels waiting for a voice.
    pub waiting: VecDeque<Option<u16>>,
    /// `mCurrentSceneIndex` per physical wsys.
    pub wave_scene: Vec<usize>,
    pub syncs: Vec<SyncMessage>,
    /// `SEQ_ARG`
    pub seq_args: [u32; 8],
    /// Instrument oscillators converted once: (bank, program, slot).
    pub inst_oscs: HashMap<(usize, u8, usize), Osc>,
    /// `MAX_MIXERLEVEL` after `Jac_SetMixerLevel(1.1, 2.0)` (verysimple.c).
    pub max_mixer_level: f32,
    /// `DSP_MIXERLEVEL / 4096`.
    pub dsp_level: f32,
    pub subframes: u64,
}

impl Engine {
    pub fn new(data: Arc<SoundData>) -> Self {
        let mut tracks = vec![Track::default(); ROOT_TRACKS + POOL_TRACKS];
        for t in tracks.iter_mut() {
            t.state = 0;
        }
        let wsys = data.bank.wsys.len();
        let mut e = Engine {
            data,
            rng: Random::default(),
            tracks,
            free_tracks: (ROOT_TRACKS..ROOT_TRACKS + POOL_TRACKS).collect(),
            roots: [None; 16],
            registry: Vec::new(),
            // ROOT_OUTER[16]
            outers: vec![OuterParam::default(); 16],
            global: Mgr::default(),
            channels: vec![Channel::default(); CHANNELS],
            free_channels: (0..CHANNELS as u16).rev().collect(),
            voices: vec![Voice::default(); VOICES],
            waiting: VecDeque::new(),
            wave_scene: vec![0; wsys],
            syncs: Vec::new(),
            seq_args: [0; 8],
            inst_oscs: HashMap::new(),
            max_mixer_level: 1.1 * 16384.0,
            dsp_level: 2.0,
            subframes: 0,
        };
        for c in e.channels.iter_mut() {
            c.id = 0;
        }
        e
    }

    pub fn mgr(&self, id: MgrId) -> &Mgr {
        match id {
            MgrId::Global => &self.global,
            MgrId::Track(t) => &self.tracks[t].jcs,
        }
    }

    pub fn mgr_mut(&mut self, id: MgrId) -> &mut Mgr {
        match id {
            MgrId::Global => &mut self.global,
            MgrId::Track(t) => &mut self.tracks[t].jcs,
        }
    }

    /// `Jam_GetTrackHandle`
    pub fn track_handle(&self, id: u32) -> Option<usize> {
        self.registry
            .iter()
            .find(|(i, t)| *i == id && t.is_some())
            .and_then(|r| r.1)
    }

    /// One DSP subframe: `UpdateDSP` (dspbuf.c) minus the game-side player callbacks, which
    /// the caller runs between `update_sequences` and `update_voices`.
    pub fn update_sequences(&mut self) {
        // `DspPlayerCallback`: every root sequence registered by `Jaq_StartSeq`.
        for h in 0..16 {
            if let Some(t) = self.roots[h]
                && (self.tracks[t].state == 1 || self.tracks[t].state == 3)
            {
                self.root_callback(t);
            }
        }
    }
}
