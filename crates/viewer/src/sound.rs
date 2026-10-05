//! The viewer's sound: `libpikmin-audio` on a cpal output stream, and the sim's events mapped
//! to the sounds the game plays for them (call sites cited per event).
//!
//! The sound system runs on the audio thread behind a mutex; the game thread posts sounds and
//! the listener once per frame.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use glam::{Mat4, Vec3};
use libpikmin::event::Event;
use libpikmin::{DeathCause, Sim};
use libpikmin_assets::Assets;
use libpikmin_assets::sound::SoundData;
use libpikmin_assets::sound::tables::JaudioTables;
use libpikmin_audio::SoundSystem;

use crate::teki::Tekis;

/// `tbl_stage_to_bgm` (piki_scene.c): Impact Site, Forest of Hope, Forest Navel, Distant
/// Spring, Final Trial.
pub const STAGE_BGM: [usize; 5] = [4, 5, 10, 9, 17];

pub struct Audio {
    sys: Arc<Mutex<SoundSystem>>,
    _stream: cpal::Stream,
    whistling: bool,
    bgm: usize,
}

/// Emitter ids for creatures: kind in the top byte.
fn piki_owner(id: libpikmin::PikiId) -> u64 {
    1 << 56 | id.0 as u64
}
fn teki_owner(id: libpikmin::host::CreatureId) -> u64 {
    2 << 56 | id
}
fn onion_owner(id: libpikmin::onion::OnionId) -> u64 {
    3 << 56 | id.0 as u64
}

impl Audio {
    /// Starts the sound system and an output stream. `None` if there's no sound data or no
    /// output device.
    pub fn start(assets: &Assets) -> Option<Audio> {
        let data = match SoundData::load(assets) {
            Ok(d) => Arc::new(d),
            Err(e) => {
                eprintln!("sound: {e}");
                return None;
            }
        };
        let tables = match JaudioTables::load(assets) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("sound: {e} (re-run extraction to get the sound tables)");
                return None;
            }
        };
        let mut sys = SoundSystem::new(data, tables);
        if let Ok(b) = assets.read("parms/sound.bin") {
            sys.set_parms(&b);
        }
        let sys = Arc::new(Mutex::new(sys));
        let device = cpal::default_host().default_output_device()?;
        let config = device.default_output_config().ok()?;
        let channels = config.channels() as usize;
        let rate = config.sample_rate().0 as f32;
        let shared = sys.clone();
        let mut scratch = Vec::new();
        let stream = device
            .build_output_stream(
                &config.into(),
                move |out: &mut [f32], _| {
                    let frames = out.len() / channels;
                    scratch.resize(frames * 2, 0.0);
                    if let Ok(mut s) = shared.lock() {
                        s.render(&mut scratch, rate);
                    }
                    for (f, o) in scratch.as_chunks::<2>().0.iter().zip(out.chunks_exact_mut(channels)) {
                        for (c, v) in o.iter_mut().enumerate() {
                            *v = f[c.min(1)];
                        }
                    }
                },
                |e| eprintln!("sound: stream error: {e}"),
                None,
            )
            .ok()?;
        stream.play().ok()?;
        println!("sound: {} Hz, {channels} channels", rate);
        Some(Audio {
            sys,
            _stream: stream,
            whistling: false,
            bgm: 0,
        })
    }

    fn with<R>(&self, f: impl FnOnce(&mut SoundSystem) -> R) -> Option<R> {
        self.sys.lock().ok().map(|mut s| f(&mut s))
    }

    pub fn play_bgm(&mut self, id: usize) {
        self.bgm = id;
        self.with(|s| s.play_bgm(0, id));
    }

    pub fn bgm(&self) -> usize {
        self.bgm
    }

    pub fn stop_bgm(&mut self) {
        self.bgm = 0;
        self.with(|s| s.stop_bgm(0));
    }

    /// A system (menu) sound by name.
    pub fn play(&self, name: &str) {
        self.with(|s| s.play(name, Vec3::ZERO));
    }

    /// Once per game frame: the camera, emitter positions, enemy animation sounds, and
    /// whether the active captain is whistling.
    pub fn frame(&mut self, view: Mat4, listener: Vec3, sim: &Sim, tekis: &mut Tekis, whistling: bool) {
        let squad = sim.pikis().iter().filter(|p| p.captain.is_some()).count();
        let whistle_started = whistling && !self.whistling;
        let whistle_stopped = self.whistling && !whistling;
        self.whistling = whistling;
        let teki_sounds: Vec<(u64, &'static str, u8, Vec3)> = tekis
            .list
            .iter_mut()
            .flat_map(|t| {
                let (id, base, pos) = (teki_owner(t.id), t.kind.sound_base(), t.pos);
                std::mem::take(&mut t.sounds).into_iter().map(move |i| (id, base, i, pos))
            })
            .collect();
        let teki_pos: Vec<(u64, Vec3)> = tekis.list.iter().map(|t| (teki_owner(t.id), t.pos)).collect();
        self.with(|s| {
            s.set_listener(view, listener);
            s.set_piki_count(squad as u32);
            for (owner, pos) in teki_pos {
                s.move_emitter(owner, pos);
            }
            for (owner, base, i, pos) in teki_sounds {
                // `BTeki::playTableSound`: the table's sounds are consecutive `SE_*` ids.
                if let Some(id) = s.tables.sound(base).map(|e| e.se_id + i as i32)
                    && let Some(name) = s.tables.sound_by_id(id).map(|e| e.name.clone())
                {
                    s.play_from(owner, &name, pos);
                }
            }
            // `NaviWhistleState::init` (naviState.cpp): `playPlayerSe(SE_GATHER)`.
            if whistle_started {
                s.play("SE_GATHER", Vec3::ZERO);
            }
            if whistle_stopped {
                // `NaviWhistleState` cleanup: `stopPlayerSe(SE_GATHER)` (naviState.cpp).
                s.stop("SE_GATHER");
            }
            s.frame();
        });
    }

    /// The sounds the game plays for a sim event.
    pub fn on_event(&mut self, e: &Event, sim: &Sim, tekis: &Tekis) {
        let piki_pos = |id: libpikmin::PikiId| sim.piki(id).pos;
        let teki_pos = |id: libpikmin::host::CreatureId| tekis.list.iter().find(|t| t.id == id).map_or(Vec3::ZERO, |t| t.pos);
        self.with(|s| match *e {
            Event::Recruited { piki, .. } => {
                // `PikiLookAtState::init` (pikiState.cpp): the call and the Pikmin's answer.
                s.play("SE_PIKI_CALLED", Vec3::ZERO);
                s.play("SEF_PIKI_CALLED", piki_pos(piki));
            }
            Event::Thrown { .. } => {
                // `NaviThrowState` (naviState.cpp) and `PikiFlyingState::init` (pikiState.cpp).
                s.play("SE_THROW", Vec3::ZERO);
                s.play("SE_PIKI_FLY", Vec3::ZERO);
            }
            // `PikiFlyingState::exec` landing (pikiState.cpp).
            Event::Landed { piki } => {
                s.play("SEF_PIKI_LAND", piki_pos(piki));
            }
            // `PikiFlyingState::collideAI` (pikiState.cpp).
            Event::Latched { .. } => {
                s.play("SE_PIKI_ATTACHENEMY", Vec3::ZERO);
            }
            // `ActAttack` (aiAttack.cpp): the attack voice on the target's emitter.
            Event::Hit { creature, .. } => {
                s.play_from(teki_owner(creature), "SE_PIKI_ATTACK_VOICE", teki_pos(creature));
            }
            // `InteractAttack` and friends (interactBattle.cpp).
            Event::PikiHurt { piki, .. } => {
                s.play_from(piki_owner(piki), "SE_PIKI_DAMAGED", piki_pos(piki));
            }
            Event::Swallowed { piki, .. } => {
                s.play_from(piki_owner(piki), "SE_PIKI_EATEN", piki_pos(piki));
            }
            Event::PikiDied { piki, cause } => {
                // `InteractPress` (interactBattle.cpp) for crushing; `pikidoKill.cpp` otherwise.
                let name = if matches!(cause, DeathCause::Crushed) { "SE_PIKI_PRESSED" } else { "SE_PIKI_DEAD" };
                s.play(name, piki_pos(piki));
                s.release_emitter(piki_owner(piki));
            }
            // `NaviPluckState` (naviState.cpp): `SE_PIKI_PULLED` once out.
            Event::Plucked { .. } => {
                s.play("SE_PIKI_PULLED", Vec3::ZERO);
            }
            // `NaviBreakupState` (naviState.cpp).
            Event::Dismissed { .. } => {
                s.play("SE_BREAKUP", Vec3::ZERO);
            }
            // `NaviAttackState` (naviState.cpp): punch, and the hit on the enemy.
            Event::Punched { creature, .. } => {
                s.play("SE_PLAYER_PUNCH", Vec3::ZERO);
                s.play_from(teki_owner(creature), "SE_PIKI_ATTACK_HIT", teki_pos(creature));
            }
            // `Navi` damage handlers (navi.cpp).
            Event::CaptainHurt { .. } => {
                s.play("SE_DAMAGED", Vec3::ZERO);
            }
            // `NaviDeadState` (naviState.cpp).
            Event::CaptainDown { .. } => {
                s.play("SE_PLAYER_DOWN", Vec3::ZERO);
            }
            // `ActTransport` lifting (aiTransport.cpp).
            Event::PelletLifted { pellet, .. } => {
                s.play("SEF_PIKI_HANG", sim.pellet(pellet).pos);
            }
            // `PelletGoalState` (pelletState.cpp): on the Onion's emitter.
            Event::PelletArrived { onion, .. } => {
                let pos = sim.onions()[onion.0 as usize].pos;
                s.play_from(onion_owner(onion), "SE_CONTAINER_PELLETIN", pos);
            }
            // `GoalItem` seed shot (goalItem.cpp).
            Event::PelletDelivered { onion, .. } => {
                let pos = sim.onions()[onion.0 as usize].pos;
                s.play_from(onion_owner(onion), "SE_CONTAINER_PELLETIN2", pos);
            }
            Event::SeedLaunched { onion, .. } => {
                let pos = sim.onions()[onion.0 as usize].pos;
                s.play_from(onion_owner(onion), "SE_CONTAINER_HANABI", pos);
            }
            // `PikiHeadAI::BounceSound` (itemAI.cpp): on the parent Onion's emitter.
            Event::SproutPlanted { sprout } => {
                if let Some(sp) = sim.sprouts().find(|x| x.id == sprout) {
                    let pos = sp.pos;
                    match sim.onions().iter().min_by(|a, b| (a.pos - pos).length().total_cmp(&(b.pos - pos).length())) {
                        Some(o) => s.play_from(onion_owner(o.id), "SE_PIKI_GROW0", pos),
                        None => s.play("SEF_PIKI_GROW0", pos),
                    };
                }
            }
            // `PikiHeadAI::GrowupedExec` (itemAI.cpp): leaf -> bud -> flower.
            Event::SproutGrew { sprout, happa } => {
                if let Some(sp) = sim.sprouts().find(|x| x.id == sprout) {
                    let name = match happa as u8 {
                        1 => "SEF_PIKI_GROW2",
                        2 => "SEF_PIKI_GROW4",
                        _ => "SEF_PIKI_GROW3",
                    };
                    s.play(name, sp.pos);
                }
            }
            // `GoalItem::enterGoal` / `exitPiki` (goalItem.cpp).
            Event::PikiEnteredOnion { onion, .. } => {
                let pos = sim.onions()[onion.0 as usize].pos;
                s.play_from(onion_owner(onion), "SE_PIKI_GOHOME", pos);
            }
            Event::PikiLeftOnion { onion, .. } => {
                let pos = sim.onions()[onion.0 as usize].pos;
                s.play_from(onion_owner(onion), "SE_PIKI_OUTHOME", pos);
            }
            // `PikiAbsorbState` / `PikiGrowupState` (pikiState.cpp).
            Event::PikiDrinking { piki } => {
                s.play("SEF_PIKI_DRINK", piki_pos(piki));
            }
            Event::Bloomed { piki } => {
                s.play("SEF_PIKI_GROW4", piki_pos(piki));
            }
            _ => {}
        });
    }
}
