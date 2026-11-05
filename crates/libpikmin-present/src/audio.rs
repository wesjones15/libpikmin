//! Sound: `libpikmin-audio` on a cpal output stream, and the sim's events mapped to the sounds
//! the game plays for them (call sites cited per event).
//!
//! The sound system runs on the audio thread behind a mutex; the game thread posts sounds and
//! the listener once per frame. The stream is opened and kept on a thread of its own, so the
//! host's thread (COM apartment, panics) doesn't matter, and it follows the default output
//! device (libcommon's `audio::out`): a headset's audio coming back doesn't leave it silent.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::collections::HashSet;

use cpal::traits::DeviceTrait;
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

/// What plays when a captain goes down.
pub const CAPTAIN_DOWN_SOUND: &str = "SYSSE_EVENING_ALERT";

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
    paused: Arc<AtomicBool>,
    /// The stream, kept on the default device (dropped with the Audio: the sound stops).
    _output: libcommon::audio::out::Output,
    whistling: bool,
    onion_spot: bool,
    external_death_sting: bool,
    bgm: usize,
    /// Pellets whose `SE_LIFT_TRY` loop is on (`Pellet::mIsPlayTrySound`).
    trying: Vec<libpikmin::PelletId>,
    /// Pikmin already heard entering their Onion leg this frame sequence.
    climbing: HashSet<libpikmin::PikiId>,
    /// The tick a jump pad's flying voice last played, so a squad leaving together is one cry.
    pad_voice: Option<u64>,
    /// The captain this machine's player is (`None`: no one in particular). Another captain's player
    /// sounds are not played in the listener's ears: the positional ones come from where he is.
    listener: Option<usize>,
    /// Captains wearing the Pianta body, whose own voice (`pianta::PiantaVoice`) replaces Olimar's
    /// throw and hurt cries.
    pub pianta_captains: HashSet<u32>,
    /// The output's rate and channels, for the log.
    pub rate: u32,
    pub channels: u16,
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
        let paused = Arc::new(AtomicBool::new(false));
        let (shared, g, p) = (sys.clone(), gain.clone(), paused.clone());
        let output = libcommon::audio::out::Output::start("pikmin-audio", move |device: &cpal::Device, lost| {
            open_stream(device, lost, shared.clone(), g.clone(), p.clone())
        })?;
        let (rate, channels) = (output.rate, output.channels);
        Ok(Audio {
            sys,
            gain,
            paused,
            _output: output,
            whistling: false,
            onion_spot: false,
            external_death_sting: false,
            bgm: 0,
            trying: Vec::new(),
            climbing: HashSet::new(),
            pad_voice: None,
            listener: None,
            pianta_captains: HashSet::new(),
            rate,
            channels,
        })
    }

    fn with<R>(&self, f: impl FnOnce(&mut SoundSystem) -> R) -> Option<R> {
        self.sys.lock().ok().map(|mut s| f(&mut s))
    }

    /// Holds every sound where it is (a paused game): silence out and the sound system standing
    /// still, so they carry on from the same place when unpaused.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    /// Output volume, 0..1 (the host's sound-effects volume).
    pub fn set_volume(&self, v: f32) {
        self.gain.store(v.clamp(0.0, 4.0).to_bits(), Ordering::Relaxed);
    }

    /// How loud the swarming trumpet plays against the disc's level (host tuning; 1 is the disc's).
    pub fn set_formation_gain(&self, gain: f32) {
        self.with(|s| s.set_formation_gain(gain));
    }

    pub fn play_bgm(&mut self, id: usize) {
        self.bgm = id;
        self.with(|s| s.play_bgm(0, id));
    }

    pub fn bgm(&self) -> usize {
        self.bgm
    }

    pub fn has_bgm(&self, id: usize) -> bool {
        self.with(|s| s.engine.data.seqs.seq(id).is_some()).unwrap_or(false)
    }

    pub fn stop_bgm(&mut self) {
        self.bgm = 0;
        self.with(|s| s.stop_bgm(0));
    }

    /// Independent host music stream: original enemy-proximity mix, not SE volume.
    /// `Jac_UpdateCamera` (pikiinter.c) drives mode flag 1 for nearby enemy events.
    pub fn music_frame(&self, nearby_enemy: bool, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
        if !paused {
            self.with(|s| { s.frame(); s.set_bgm_mode_flag(0, 1, nearby_enemy as u8); });
        }
    }

    /// Which captain this machine's player controls, so the player sounds of the others (their
    /// throws, calls and punches) are heard from where they are rather than as the listener's own.
    pub fn set_listener_captain(&mut self, captain: Option<usize>) {
        self.listener = captain;
    }

    /// A system (menu) sound by name.
    pub fn play(&self, name: &str) {
        self.with(|s| s.play(name, Vec3::ZERO));
    }

    /// A host-triggered sound at a world position (sim units).
    pub fn play_at(&self, name: &str, pos: Vec3) {
        self.with(|s| s.play(name, pos));
    }

    pub fn stop(&self, name: &str) {
        self.with(|s| s.stop(name));
    }

    /// Give a host the full vanilla bell as stereo 32 kHz PCM, so its death player can
    /// outlive this scene's audio stream. Ten seconds includes both phrases and the decay.
    pub fn take_death_sting(&mut self) -> Option<Vec<f32>> {
        let (data, tables) = self.with(|s| (s.engine.data.clone(), s.tables.clone()))?;
        let mut bell = SoundSystem::new(data, tables);
        if !bell.play(CAPTAIN_DOWN_SOUND, Vec3::ZERO) { return None; }
        let mut pcm = vec![0.0; 32000 * 10 * 2];
        bell.render(&mut pcm, 32000.0);
        self.external_death_sting = true;
        Some(pcm)
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
        // ActEnter::gotoLeg plays SE_CONTAINER_CLIMB once as each Pikmin starts climbing.
        let mut climbing_now = HashSet::new();
        let mut climb_sounds = Vec::new();
        for p in sim.pikis() {
            if let libpikmin::PikiState::Enter { onion, climbing: true, .. } = p.state {
                climbing_now.insert(p.id);
                if !self.climbing.contains(&p.id) {
                    climb_sounds.push((onion, p.pos));
                }
            }
        }
        self.climbing = climbing_now;
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
            for (onion, pos) in climb_sounds {
                s.play_from(onion_owner(onion), "SE_CONTAINER_CLIMB", pos);
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
        let external_death_sting = self.external_death_sting;
        // A host jump pad throws the squad the way a throw does: `PikiFlyingState::init`
        // (pikiState.cpp) plays `SE_PIKI_FLY` as each Pikmin goes airborne. Pikmin leave the
        // pad over several ticks, so they share one cry per few ticks.
        let listener = self.listener;
        let pianta = self.pianta_captains.clone();
        // True for the listener's own captain, and for everyone when no listener is set.
        let mine = move |c: libpikmin::CaptainId| listener.is_none_or(|l| l == c.0 as usize);
        let pad_voice = match *e {
            Event::PikiLaunched { .. } if self.pad_voice.is_none_or(|t| sim.tick().saturating_sub(t) >= 6) => {
                self.pad_voice = Some(sim.tick());
                true
            }
            _ => false,
        };
        self.with(|s| match *e {
            Event::PikiLaunched { piki, captain } => {
                if pad_voice {
                    s.play("SE_PIKI_FLY", if mine(captain) { Vec3::ZERO } else { piki_pos(piki) });
                }
            }
            Event::Recruited { piki, captain, .. } => {
                // `PikiLookAtState::init` (pikiState.cpp): the call and the Pikmin's answer.
                if mine(captain) {
                    s.play("SE_PIKI_CALLED", Vec3::ZERO);
                }
                s.play("SEF_PIKI_CALLED", piki_pos(piki));
            }
            Event::Thrown { piki, captain } => {
                // `NaviThrowState` (naviState.cpp) and `PikiFlyingState::init` (pikiState.cpp).
                if mine(captain) {
                    if !pianta.contains(&captain.0) {
                        s.play("SE_THROW", Vec3::ZERO);
                    }
                    s.play("SE_PIKI_FLY", Vec3::ZERO);
                } else {
                    s.play("SE_PIKI_FLY", piki_pos(piki));
                }
            }
            // `PikiFlyingState::exec` landing (pikiState.cpp).
            Event::Landed { piki } => {
                s.play("SEF_PIKI_LAND", piki_pos(piki));
            }
            // `PikiFlyingState::collideAI` (pikiState.cpp).
            Event::Latched { piki, .. } => {
                let other = sim.piki(piki).captain.is_some_and(|c| !mine(c));
                s.play("SE_PIKI_ATTACHENEMY", if other { piki_pos(piki) } else { Vec3::ZERO });
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
            // NaviNukuState::init and procAnimMsg(KEY_Action0), naviState.cpp.
            Event::PluckStarted { piki } => {
                let at = sim.piki(piki).captain.filter(|&c| mine(c)).map_or(piki_pos(piki), |_| Vec3::ZERO);
                s.play("SE_PIKI_PULLING", at);
            }
            Event::Plucked { captain, piki, .. } => {
                let at = if mine(captain) { Vec3::ZERO } else { piki_pos(piki) };
                for name in ["SE_PIKI_PULLED2", "SE_PIKI_PULLED"] { s.play(name, at); }
            }
            Event::PluckerPulled { plucked, .. } => {
                for name in ["SE_PIKI_PULLED2", "SE_PIKI_PULLED"] { s.play(name, piki_pos(plucked)); }
            }
            // `NaviBreakupState` (naviState.cpp).
            Event::Dismissed { captain } | Event::StrikeStarted { captain } => {
                if mine(captain) {
                    s.play("SE_BREAKUP", Vec3::ZERO);
                }
            }
            // `NaviAttackState` (naviState.cpp): punch, and the hit on the enemy.
            Event::Punched { captain, creature, .. } => {
                if mine(captain) {
                    s.play("SE_PLAYER_PUNCH", Vec3::ZERO);
                }
                s.play_from(creature_owner(creature), "SE_PIKI_ATTACK_HIT", teki_pos(creature));
            }
            // `Navi` damage handlers (navi.cpp).
            Event::CaptainHurt { captain, .. } => {
                if mine(captain) && !pianta.contains(&captain.0) {
                    s.play("SE_DAMAGED", Vec3::ZERO);
                }
            }
            // `NaviDeadState` (naviState.cpp) plays `SE_PLAYER_DOWN`; here the sundown bell
            // (`GameCoreSection::startSundownWarn`'s `SYSSE_EVENING_ALERT`) stands in for it.
            Event::CaptainDown { captain } => {
                if !external_death_sting && mine(captain) { s.play(CAPTAIN_DOWN_SOUND, Vec3::ZERO); }
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
            // FallWaterAI::CollideInit and PikiNormalState's AS_Drinking entry.
            Event::NectarSplash { pos } => { s.play("SE_KURIONE_WATER", pos); }
            Event::PikiDrinking { piki } => { s.play("SEF_PIKI_DRINK", piki_pos(piki)); }
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
                let sound = if sim.onions()[ship.0 as usize].ship.is_some() { "SE_UFO_PARTSIN" } else { "SE_CONTAINER_PELLETIN2" };
                s.play_from(onion_owner(ship), sound, pos);
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
            // Host extension (kill growth): the sprout's stage sounds, `PikiHeadAI::GrowupedExec`.
            Event::PikiGrew { piki, happa } => {
                let n = if happa == libpikmin::Happa::Flower { 4 } else { 2 };
                s.play(&format!("SEF_PIKI_GROW{n}"), piki_pos(piki));
            }
            _ => {}
        });
        self.trying = trying;
    }
}

/// A stream rendering `sys` on `device` (not yet playing); `lost` is set when it fails.
fn open_stream(device: &cpal::Device, lost: Arc<AtomicBool>, sys: Arc<Mutex<SoundSystem>>, gain: Arc<AtomicU32>,
    paused: Arc<AtomicBool>) -> Result<(cpal::Stream, u32, u16), String> {
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
                if paused.load(Ordering::Relaxed) { out.fill(0.0); return; }
                let frames = out.len() / ch;
                scratch.clear();
                scratch.resize(frames * 2, 0.0);
                let g = f32::from_bits(gain.load(Ordering::Relaxed));
                if let Ok(mut s) = sys.lock() {
                    s.render_with_gain(&mut scratch, rate as f32, g);
                }
                for (f, o) in scratch.as_chunks::<2>().0.iter().zip(out.chunks_exact_mut(ch)) {
                    for (c, v) in o.iter_mut().enumerate() {
                        *v = if ch == 1 { (f[0] + f[1]) * 0.5 } else if c < 2 { f[c] } else { 0.0 };
                    }
                }
            },
            move |e| {
                eprintln!("sound: stream error: {e}");
                lost.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| format!("output stream: {e}"))?;
    Ok((stream, rate, channels))
}
