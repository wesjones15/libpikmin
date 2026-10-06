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
    /// Pellets whose `SE_LIFT_TRY` loop is on (`Pellet::mIsPlayTrySound`).
    trying: Vec<libpikmin::PelletId>,
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
fn pellet_owner(id: libpikmin::PelletId) -> u64 {
    4 << 56 | id.0 as u64
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
            trying: Vec::new(),
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
    pub fn frame(
        &mut self,
        view: Mat4,
        listener: Vec3,
        sim: &Sim,
        tekis: &mut Tekis,
        whistling: bool,
        sprout_sounds: &[(Option<libpikmin::onion::OnionId>, u8, Vec3)],
    ) {
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
        // `ActTransport::cleanup`: the last carrier letting go stops the try loop.
        let (keep, stop): (Vec<_>, Vec<_>) = std::mem::take(&mut self.trying).into_iter().partition(|&p| {
            let pel = sim.pellet(p);
            !pel.carriers.is_empty() && matches!(pel.state, libpikmin::PelletState::Idle)
        });
        self.trying = keep;
        let pellets: Vec<(u64, Vec3)> = sim
            .pellets()
            .iter()
            .filter(|p| !matches!(p.state, libpikmin::PelletState::Gone))
            .map(|p| (pellet_owner(p.id), p.pos))
            .collect();
        let onions: Vec<Vec3> = sim.onions().iter().map(|o| o.pos).collect();
        self.with(|s| {
            s.set_listener(view, listener);
            s.set_piki_count(squad as u32);
            for (owner, pos) in teki_pos.into_iter().chain(pellets) {
                s.move_emitter(owner, pos);
            }
            for p in stop {
                s.stop_from(pellet_owner(p), "SE_LIFT_TRY");
            }
            // `PikiHeadItem::playSound`: key value v is `SE` 0xCC + v on the parent Onion, else
            // `SEF` 0x108 + v (only for v > 0).
            for &(onion, v, pos) in sprout_sounds {
                match onion {
                    Some(o) => {
                        if let Some(name) = s.tables.sound_by_id(0xCC + v as i32).map(|e| e.name.clone()) {
                            s.play_from(onion_owner(o), &name, onions.get(o.0 as usize).copied().unwrap_or(pos));
                        }
                    }
                    None if v > 0 => {
                        if let Some(name) = s.tables.sound_by_id(0x108 + v as i32).map(|e| e.name.clone()) {
                            s.play(&name, pos);
                        }
                    }
                    None => {}
                }
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
        let mut trying = std::mem::take(&mut self.trying);
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
            // `InteractPress` (interactBattle.cpp) when crushed.
            Event::PikiDied { piki, cause } => {
                if matches!(cause, DeathCause::Crushed) {
                    s.play("SE_PIKI_PRESSED", piki_pos(piki));
                }
            }
            // `Piki::doKill` (pikidoKill.cpp): `playSoundDirect(1, SE_PIKI_DEAD)` as the ghost
            // rises.
            Event::PikiGone { piki, pos } => {
                s.play("SE_PIKI_DEAD", pos);
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
            // `ActTransport::gotoLiftPos` (aiTransport.cpp): `SEF_PIKI_HANG` as each carrier takes
            // hold, and the pellet's `SE_LIFT_TRY` loop while it isn't up yet
            // (`Pellet::setTrySound`).
            Event::CarrierGrabbed { piki, pellet } => {
                s.play("SEF_PIKI_HANG", piki_pos(piki));
                let pel = sim.pellet(pellet);
                if matches!(pel.state, libpikmin::PelletState::Idle) && !trying.contains(&pellet) {
                    s.play_from(pellet_owner(pellet), "SE_LIFT_TRY", pel.pos);
                    trying.push(pellet);
                }
            }
            // `Pellet::startPick` stops the try loop; the stick leader starts `SE_LIFT_MOVE`
            // (`ActTransport::doLift`).
            Event::PelletLifted { pellet, .. } => {
                trying.retain(|&p| p != pellet);
                s.stop_from(pellet_owner(pellet), "SE_LIFT_TRY");
                s.play_from(pellet_owner(pellet), "SE_LIFT_MOVE", sim.pellet(pellet).pos);
            }
            // `Pellet::finishPick`: both loops stop.
            Event::PelletDropped { pellet } => {
                s.stop_from(pellet_owner(pellet), "SE_LIFT_MOVE");
                s.stop_from(pellet_owner(pellet), "SE_LIFT_TRY");
            }
            // `PelletGoalState` (pelletState.cpp): on the Onion's emitter.
            Event::PelletArrived { onion, pellet } => {
                // `ActTransport` at the goal: `stopEventSound(pel, SE_LIFT_MOVE)`.
                s.stop_from(pellet_owner(pellet), "SE_LIFT_MOVE");
                let pos = sim.onions()[onion.0 as usize].pos;
                s.play_from(onion_owner(onion), "SE_CONTAINER_PELLETIN", pos);
            }
            // `GoalItem` seed shot (goalItem.cpp).
            Event::PelletDelivered { onion, .. } => {
                let pos = sim.onions()[onion.0 as usize].pos;
                s.play_from(onion_owner(onion), "SE_CONTAINER_PELLETIN2", pos);
            }
            // `UfoItem::suckMe` (ufoItem.cpp).
            Event::ShipPartCollected { ship, .. } => {
                let pos = sim.onions()[ship.0 as usize].pos;
                s.play_from(onion_owner(ship), "SE_UFO_PARTSIN", pos);
            }
            // `GoalAI::EmitPiki` (itemAI.cpp): each seed shot.
            Event::SeedLaunched { onion, .. } => {
                let pos = sim.onions()[onion.0 as usize].pos;
                s.play_from(onion_owner(onion), "SE_CONTAINER_PIKIBORN", pos);
            }
            // `PikiHeadAI::BounceSound` (itemAI.cpp): on the parent Onion's emitter.
            Event::SproutPlanted { sprout } => {
                if let Some(sp) = sim.sprouts().find(|x| x.id == sprout) {
                    match sp.onion {
                        Some(o) => s.play_from(onion_owner(o), "SE_PIKI_GROW0", sim.onions()[o.0 as usize].pos),
                        None => s.play("SEF_PIKI_GROW0", sp.pos),
                    };
                }
            }
            // `PikiHeadAI::GrowupedExec` (itemAI.cpp): leaf -> bud -> flower, on the parent Onion's
            // emitter (`SE_PIKI_GROW*`) or the sprout's own (`SEF_PIKI_GROW*`).
            Event::SproutGrew { sprout, happa } => {
                if let Some(sp) = sim.sprouts().find(|x| x.id == sprout) {
                    let n = match happa as u8 {
                        1 => 2,
                        2 => 4,
                        _ => 3,
                    };
                    match sp.onion {
                        Some(o) => s.play_from(onion_owner(o), &format!("SE_PIKI_GROW{n}"), sim.onions()[o.0 as usize].pos),
                        None => s.play(&format!("SEF_PIKI_GROW{n}"), sp.pos),
                    };
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
        self.trying = trying;
    }
}
