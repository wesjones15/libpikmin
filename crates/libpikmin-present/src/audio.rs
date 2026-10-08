//! Sound: `libpikmin-audio` on a cpal output stream, and the sim's events mapped to the sounds
//! the game plays for them (call sites cited per event).
//!
//! The sound system runs on the audio thread behind a mutex; the game thread posts sounds and
//! the listener once per frame. The stream is opened and kept on a thread of its own, so the
//! host's thread (COM apartment, panics) doesn't matter.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use glam::{Mat4, Vec3};
use libpikmin::event::Event;
use libpikmin::host::CreatureId;
use libpikmin::{DeathCause, Sim};
use libpikmin_assets::Assets;
use libpikmin_assets::sound::SoundData;
use libpikmin_assets::sound::tables::JaudioTables;
use libpikmin_audio::SoundSystem;

/// `tbl_stage_to_bgm` (piki_scene.c): Impact Site, Forest of Hope, Forest Navel, Distant
/// Spring, Final Trial.
pub const STAGE_BGM: [usize; 5] = [4, 5, 10, 9, 17];

/// Emitter ids: kind in the top byte.
fn piki_owner(id: libpikmin::PikiId) -> u64 {
    1 << 56 | id.0 as u64
}
/// The emitter of a host creature (enemy).
pub fn creature_owner(id: CreatureId) -> u64 {
    2 << 56 | id
}
fn onion_owner(id: libpikmin::onion::OnionId) -> u64 {
    3 << 56 | id.0 as u64
}
fn pellet_owner(id: libpikmin::PelletId) -> u64 {
    4 << 56 | id.0 as u64
}

/// Sounds the 3D scene's animations asked for this frame.
#[derive(Default, Clone, Copy)]
pub struct SceneSounds<'a> {
    pub swarm: glam::Vec2,
    /// Sprout animation keys: (parent Onion, value, position).
    pub sprouts: &'a [(Option<libpikmin::onion::OnionId>, u8, Vec3)],
    /// The ship's: (ship, sound, start or stop).
    pub ship: &'a [(libpikmin::onion::OnionId, &'static str, bool)],
    /// The listener's captain is at an Onion (`GoalItem::setSpotActive`, from
    /// `NaviWalkState::exec`): `SYSSE_CONTAINER_OK` plays while he is.
    pub onion_spot: bool,
}

/// Host creatures this frame, for positional sounds.
#[derive(Default, Clone, Copy)]
pub struct Creatures<'a> {
    /// Where each creature is.
    pub positions: &'a [(CreatureId, Vec3)],
    /// Sounds from the creatures' own sound tables: (creature, table, index). The table's
    /// sounds are consecutive `SE_*` ids (`BTeki::playTableSound`).
    pub table_sounds: &'a [(CreatureId, &'static str, u8)],
}

impl Creatures<'_> {
    fn pos(&self, id: CreatureId) -> Vec3 {
        self.positions.iter().find(|(c, _)| *c == id).map_or(Vec3::ZERO, |(_, p)| *p)
    }
}

pub struct Audio {
    sys: Arc<Mutex<SoundSystem>>,
    /// Output gain, as f32 bits.
    gain: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    whistling: bool,
    onion_spot: bool,
    bgm: usize,
    /// Pellets whose `SE_LIFT_TRY` loop is on (`Pellet::mIsPlayTrySound`).
    trying: Vec<libpikmin::PelletId>,
    /// The output's rate and channels, for the log.
    pub rate: u32,
    pub channels: u16,
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

impl Audio {
    /// Starts the sound system and an output stream on the default device. `Err` says why
    /// there's no sound (no sound data, no device).
    pub fn start(assets: &Assets) -> Result<Audio, String> {
        let data = Arc::new(SoundData::load(assets).map_err(|e| format!("sound data: {e}"))?);
        let tables =
            JaudioTables::load(assets).map_err(|e| format!("{e} (re-run extraction to get the sound tables)"))?;
        let mut sys = SoundSystem::new(data, tables);
        if let Ok(b) = assets.read("parms/sound.bin") {
            sys.set_parms(&b);
        }
        let sys = Arc::new(Mutex::new(sys));
        let gain = Arc::new(AtomicU32::new(1f32.to_bits()));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(u32, u16), String>>();
        let (shared, g, st) = (sys.clone(), gain.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("pikmin-audio".into())
            .spawn(move || {
                let stream = open_stream(shared, g);
                match stream {
                    Ok((stream, rate, channels)) => {
                        let _ = ready_tx.send(Ok((rate, channels)));
                        while !st.load(Ordering::Relaxed) {
                            std::thread::park();
                        }
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| format!("audio thread: {e}"))?;
        let (rate, channels) = match ready_rx.recv() {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("audio thread died".into()),
        };
        Ok(Audio {
            sys,
            gain,
            stop,
            thread: Some(thread),
            whistling: false,
            onion_spot: false,
            bgm: 0,
            trying: Vec::new(),
            rate,
            channels,
        })
    }

    fn with<R>(&self, f: impl FnOnce(&mut SoundSystem) -> R) -> Option<R> {
        self.sys.lock().ok().map(|mut s| f(&mut s))
    }

    /// Output volume, 0..1 (the host's sound-effects volume).
    pub fn set_volume(&self, v: f32) {
        self.gain.store(v.clamp(0.0, 4.0).to_bits(), Ordering::Relaxed);
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

    /// A command for the cutscene sound track (`Jac_DemoSound`), or `None` to end it.
    pub fn demo(&self, cmd: Option<(u16, u16)>) {
        self.with(|s| match cmd {
            Some((demo, cmd)) => s.demo_sound(demo, cmd),
            None => s.end_demo(),
        });
    }

    /// Once per game frame: the camera (`view` is the world-to-camera matrix, camera looking
    /// down -Z with +X to its right), emitter positions, creature sounds, and whether the
    /// listening captain is whistling.
    pub fn frame(
        &mut self,
        view: Mat4,
        listener: Vec3,
        sim: &Sim,
        creatures: Creatures,
        whistling: bool,
        scene: SceneSounds,
    ) {
        let SceneSounds { sprouts: sprout_sounds, ship: ship_sounds, swarm, onion_spot } = scene;
        let squad = sim.pikis().iter().filter(|p| p.captain.is_some()).count();
        let spot_changed = onion_spot != self.onion_spot;
        self.onion_spot = onion_spot;
        let whistle_started = whistling && !self.whistling;
        let whistle_stopped = self.whistling && !whistling;
        self.whistling = whistling;
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
            // navi.cpp Navi::update: silent with an empty squad (`mPlateMgr->mTotalSlotCount`).
            let swarm = if squad > 0 { swarm } else { glam::Vec2::ZERO };
            s.formation(swarm.x, swarm.y);
            // `GoalItem::setSpotActive`: `playSysSe` / `stopSysSe(SYSSE_CONTAINER_OK)`.
            if spot_changed {
                if onion_spot { s.play_system_se(16) } else { s.stop_system_se(16) }
            }
            for (owner, pos) in creatures
                .positions
                .iter()
                .map(|&(c, p)| (creature_owner(c), p))
                .chain(pellets)
            {
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
            // The ship's animation sounds (`UfoItem` and `PlayerState::UfoParts`).
            for &(ship, name, play) in ship_sounds {
                if play {
                    s.play_from(onion_owner(ship), name, onions.get(ship.0 as usize).copied().unwrap_or_default());
                } else {
                    s.stop_from(onion_owner(ship), name);
                }
            }
            for &(c, base, i) in creatures.table_sounds {
                // `BTeki::playTableSound`: the table's sounds are consecutive `SE_*` ids.
                if let Some(id) = s.tables.sound(base).map(|e| e.se_id + i as i32)
                    && let Some(name) = s.tables.sound_by_id(id).map(|e| e.name.clone())
                {
                    s.play_from(creature_owner(c), &name, creatures.pos(c));
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
    pub fn on_event(&mut self, e: &Event, sim: &Sim, creatures: Creatures) {
        let piki_pos = |id: libpikmin::PikiId| sim.piki(id).pos;
        let teki_pos = |id: CreatureId| creatures.pos(id);
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
            // `InteractFlick::actPiki` (interactBattle.cpp): `SE_PIKI_DAMAGED` on the flicking
            // creature's emitter as the Pikmin flies off.
            Event::KnockedOff { piki, creature } => {
                let owner = creature.map_or(piki_owner(piki), creature_owner);
                s.play_from(owner, "SE_PIKI_DAMAGED", piki_pos(piki));
            }
            // `ActAttack` (aiAttack.cpp): the attack voice on the target's emitter.
            Event::Hit { creature, .. } => {
                s.play_from(creature_owner(creature), "SE_PIKI_ATTACK_VOICE", teki_pos(creature));
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
                s.play_from(creature_owner(creature), "SE_PIKI_ATTACK_HIT", teki_pos(creature));
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

/// Opens the default output device and starts rendering `sys` into it.
fn open_stream(sys: Arc<Mutex<SoundSystem>>, gain: Arc<AtomicU32>) -> Result<(cpal::Stream, u32, u16), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no output device")?;
    let config = device.default_output_config().map_err(|e| format!("output config: {e}"))?;
    if config.sample_format() != cpal::SampleFormat::F32 {
        return Err(format!("output format {:?} isn't f32", config.sample_format()));
    }
    let channels = config.channels();
    let rate = config.sample_rate().0;
    let mut scratch = Vec::new();
    let ch = channels as usize;
    let stream = device
        .build_output_stream(
            &config.into(),
            move |out: &mut [f32], _| {
                let frames = out.len() / ch;
                scratch.clear();
                scratch.resize(frames * 2, 0.0);
                if let Ok(mut s) = sys.lock() {
                    s.render(&mut scratch, rate as f32);
                }
                let g = f32::from_bits(gain.load(Ordering::Relaxed));
                for (f, o) in scratch.as_chunks::<2>().0.iter().zip(out.chunks_exact_mut(ch)) {
                    for (c, v) in o.iter_mut().enumerate() {
                        *v = f[c.min(1)] * g;
                    }
                }
            },
            |e| eprintln!("sound: stream error: {e}"),
            None,
        )
        .map_err(|e| format!("output stream: {e}"))?;
    stream.play().map_err(|e| format!("play: {e}"))?;
    Ok((stream, rate, channels))
}
