//! C ABI over `libpikmin`, for hosts that aren't Rust (the Risk of Rain 2 plugin loads this as
//! `pikmin.dll` and calls in through P/Invoke).
//!
//! The shape:
//! 1. Find or build the assets folder: [`pik_assets_complete`], else [`pik_extract`] from the
//!    user's disc.
//! 2. [`pik_sim_new`] with that folder (tuning comes from the disc's parms).
//! 3. Add captains, Onions and Pikmin. Each host frame: [`pik_sim_set_input`] per captain,
//!    [`pik_sim_set_creatures`] with the host's enemies, then [`pik_sim_advance`] with a
//!    [`PikHost`] of world callbacks.
//! 4. Read back [`pik_sim_captain`], [`pik_sim_pikis`], [`pik_sim_pellets`], [`pik_sim_onions`],
//!    [`pik_sim_sprouts`] and drain [`pik_sim_events`].
//! 5. To draw with the disc's models and play its sounds: [`pik_view_new`], build meshes from
//!    [`pik_view_model`], [`pik_view_batches`], [`pik_view_indices`], [`pik_view_uvs`] and
//!    [`pik_view_texture`]; each frame [`pik_view_update`], then [`pik_view_draws`] with
//!    [`pik_view_positions`], [`pik_view_normals`] and [`pik_view_colors`].
//!
//! Units and axes are the sim's (the decomp's, roughly cm, Y up; a facing angle `a` points
//! along `(sin a, 0, cos a)`); hosts convert. Every struct is `#[repr(C)]` plain data, and
//! [`pik_struct_sizes`] reports their sizes so a host can refuse a mismatched build. Nothing
//! unwinds into the caller: a panic poisons that sim and later calls on it do nothing.

use std::collections::{HashMap, VecDeque};
use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use libpikmin::carry::PelletState;
use libpikmin::onion::{OnionAnim, SproutState};
use libpikmin::{
    CaptainId, CaptainInput, CaptainPose, CreatureFrame, CreatureId, CreaturePart, DamageSource,
    DeathCause, Event, Happa, Host, OnionId, Params, PelletDesc, PikiColor, PikiId, PikiState, Sim, Snapshot, Vec2,
    Vec3,
};
use libpikmin::host::CreatureView;

mod view;
pub mod teki_ai;
mod music;
pub use music::*;
pub use view::*;

/// Bumped whenever a function or struct here changes.
pub const PIK_ABI_VERSION: u32 = 52;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_ship_part_competitive_delivery(sim: *mut PikSim, enabled: u8) {
    unsafe { with(sim, (), |s| s.sim.set_ship_part_competitive_delivery(enabled != 0)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_ship_part_owner(sim: *mut PikSim, ship: u32, captain: u32) {
    unsafe { with(sim, (), |s| s.sim.set_ship_part_owner(OnionId(ship), CaptainId(captain))) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_recruit_to_pellet(sim: *mut PikSim, pellet: u32, defeated: u64, range: f32) -> u32 {
    unsafe { with(sim, 0, |s| s.sim.recruit_to_pellet(libpikmin::carry::PelletId(pellet), Some(defeated), range) as u32) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_lock_target(sim: *mut PikSim, captain: u32, target: u64, enabled: u8) {
    unsafe { with(sim, (), |s| s.sim.set_lock_target(CaptainId(captain), (enabled != 0).then_some(target))) }
}

/// Locks the captain onto a pellet, corpse or ship part (its `PikPellet::id`); `enabled` 0 lets go. Call it
/// every tick, as `pik_sim_set_lock_target`: the lock lapses once the thing is delivered. An id from
/// `libpikmin::bulb::LOCK_BASE` (0x40000000) up is a Candypop Bud (its `PikBulb::id` plus that), so a host passes one
/// "thing" number for both; the other lock is let go.
///
/// # Safety
/// `sim` from `pik_sim_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_lock_pellet(sim: *mut PikSim, captain: u32, pellet: u32, enabled: u8) {
    use libpikmin::bulb::LOCK_BASE;
    unsafe {
        with(sim, (), |s| {
            let on = enabled != 0;
            let bulb = (on && pellet >= LOCK_BASE).then(|| libpikmin::BulbId(pellet - LOCK_BASE));
            let pellet = (on && pellet < LOCK_BASE).then_some(libpikmin::carry::PelletId(pellet));
            s.sim.set_lock_pellet(CaptainId(captain), pellet);
            s.sim.set_lock_bulb(CaptainId(captain), bulb);
        })
    }
}

/// How many Pikmin a plain Candypop Bud and the rainbow Pop take before they wither (Pikmin 2: 5 and 1; at most 200),
/// for the buds there are and later ones.
///
/// # Safety
/// `sim` from `pik_sim_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_bulb_slots(sim: *mut PikSim, normal: u32, queen: u32) {
    unsafe { with(sim, (), |s| s.sim.set_bulb_slots(normal, queen)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_camera_preset(zoom: u32, top: u8, out: *mut PikVec3) {
    if let Some(out) = unsafe { out.as_mut() } { *out = libpikmin::camera::preset(zoom, top != 0).into(); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_camera_pan(mode: u32, angles: *mut PikVec3, x: f32, y: f32, dt: f32, min_pitch: f32) {
    if let Some(a) = unsafe { angles.as_mut() } {
        let mode = if mode == 1 { libpikmin::camera::CameraMode::Orbit } else { libpikmin::camera::CameraMode::Vanilla };
        libpikmin::camera::pan(mode, &mut a.x, &mut a.y, Vec2::new(x, y), dt, min_pitch);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn pik_abi_version() -> u32 {
    PIK_ABI_VERSION
}

/// The fixed simulation rate in Hz.
#[unsafe(no_mangle)]
pub extern "C" fn pik_tick_hz() -> u32 {
    libpikmin::TICK_HZ
}

// ---------------------------------------------------------------------------------------------
// Plain data

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PikVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<Vec3> for PikVec3 {
    fn from(v: Vec3) -> Self {
        PikVec3 { x: v.x, y: v.y, z: v.z }
    }
}

impl From<PikVec3> for Vec3 {
    fn from(v: PikVec3) -> Self {
        Vec3::new(v.x, v.y, v.z)
    }
}

/// The world, as callbacks. Any may be null: no ground is a void everywhere, no walls nothing
/// to bump into, no damage means every hit is refused, no path a straight line. `user` is
/// passed back unchanged. Callbacks must not unwind (a C# host catches its own exceptions).
#[repr(C)]
pub struct PikHost {
    pub user: *mut c_void,
    /// Ground height straight down from `pos`: write it to `out` and return 1, or return 0
    /// over a void ([`Host::ground_height`]).
    pub ground_height: Option<unsafe extern "C" fn(*mut c_void, *const PikVec3, *mut f32) -> u8>,
    /// Push the sphere at `centre` with `radius` out of walls and write the new centre to
    /// `out` ([`Host::walls`]).
    pub walls: Option<unsafe extern "C" fn(*mut c_void, *const PikVec3, f32, *mut PikVec3)>,
    /// A hit on `creature` for `amount` ([`Host::damage`]). `part` is the part index or -1.
    /// `source` is [`PIK_SOURCE_PIKI`] or [`PIK_SOURCE_CAPTAIN`] and `source_id` that
    /// Pikmin's or captain's id. Return 1 if the hit landed.
    pub damage: Option<unsafe extern "C" fn(*mut c_void, u64, f32, i32, u32, u32) -> u8>,
    /// Waypoints from `from` to `to` (the last should be `to`), at most `max` into `out`.
    /// Return how many, or -1 if there's no way ([`Host::carry_path`]).
    pub carry_path:
        Option<unsafe extern "C" fn(*mut c_void, *const PikVec3, *const PikVec3, *mut PikVec3, u32) -> i32>,
}

pub const PIK_SOURCE_PIKI: u32 = 0;
pub const PIK_SOURCE_CAPTAIN: u32 = 1;
/// A captain's dismiss strike.
pub const PIK_SOURCE_STRIKE: u32 = 2;

/// One captain's controls ([`CaptainInput`]). Buttons are levels; the sim finds the edges.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikInput {
    /// Movement stick in world XZ, -1..1 per axis. Ignored with `use_pose`.
    pub stick_x: f32,
    pub stick_y: f32,
    /// Swarm (C-stick) direction in world XZ; zero is neutral.
    pub swarm_x: f32,
    pub swarm_y: f32,
    /// With `use_pose`, the host moves the captain and this is where he is.
    pub pose_pos: PikVec3,
    pub pose_vel: PikVec3,
    pub pose_face: f32,
    /// With `use_cursor`, the world point the player is aiming at.
    pub cursor: PikVec3,
    pub whistle: u8,
    pub throw: u8,
    pub dismiss: u8,
    pub use_pose: u8,
    pub use_cursor: u8,
    /// Next colour to throw (`CaptainInput::cycle`).
    pub cycle: u8,
    pub jump: u8,
    pub jump_blocked: u8,
}

impl PikInput {
    fn to_input(self) -> CaptainInput {
        CaptainInput {
            stick: Vec2::new(self.stick_x, self.stick_y),
            whistle: self.whistle != 0,
            throw: self.throw != 0,
            dismiss: self.dismiss != 0,
            swarm: Vec2::new(self.swarm_x, self.swarm_y),
            pose: (self.use_pose != 0).then(|| CaptainPose {
                pos: self.pose_pos.into(),
                vel: self.pose_vel.into(),
                face: self.pose_face,
            }),
            cursor: (self.use_cursor != 0).then(|| self.cursor.into()),
            cycle: self.cycle != 0,
            jump: self.jump != 0,
            jump_blocked: self.jump_blocked != 0,
        }
    }
}

/// A host creature this frame ([`CreatureView`]). Its parts are
/// `parts[parts_start .. parts_start + parts_len]` of the array passed alongside.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikCreature {
    pub id: u64,
    pub pos: PikVec3,
    pub yaw: f32,
    pub alive: u8,
    pub flying: u8,
    pub _pad: [u8; 2],
    pub parts_start: u32,
    pub parts_len: u32,
}

/// A collision sphere on a creature, in world space ([`CreaturePart`]). Keep the order of a
/// creature's parts stable between frames: latched Pikmin remember the index.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikPart {
    pub centre: PikVec3,
    pub radius: f32,
    pub stickable: u8,
    pub _pad: [u8; 3],
}

/// Something to carry ([`PelletDesc`]), e.g. a corpse.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikPelletDesc {
    /// The corpse's creature, with `has_creature`.
    pub creature: u64,
    pub pos: PikVec3,
    pub yaw: f32,
    pub radius: f32,
    pub height: f32,
    pub min_carriers: u32,
    pub max_carriers: u32,
    /// Seeds at an Onion of its colour (corpses always use this), and at any other.
    pub seeds_matching: i32,
    pub seeds_other: i32,
    /// A [`PikiColor`] index for numbered pellets, or -1.
    pub color: i32,
    pub has_creature: u8,
    pub _pad: [u8; 3],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikCaptain {
    pub pos: PikVec3,
    pub vel: PikVec3,
    pub face: f32,
    /// Where throws land and the whistle is centred.
    pub cursor_world: PikVec3,
    /// The whistle circle, 0 when not whistling.
    pub whistle_radius: f32,
    pub health: f32,
    /// The Pikmin in his hand, or -1.
    pub held_piki: i32,
    pub squad: u32,
    pub on_ground: u8,
    pub down: u8,
    /// Holding a Pikmin ready to throw.
    pub throwing: u8,
    pub plucking: u8,
}

/// What a Pikmin is doing, for animation ([`PikiState`] by variant).
pub mod piki_state {
    pub const IDLE: u8 = 0;
    pub const SQUAD: u8 = 1;
    pub const GO_HANG: u8 = 2;
    pub const HELD: u8 = 3;
    pub const FLYING: u8 = 4;
    pub const ATTACK: u8 = 5;
    pub const KNOCKED: u8 = 6;
    pub const CARRY: u8 = 7;
    pub const PRESSED: u8 = 8;
    pub const SWALLOWED: u8 = 9;
    pub const DYING: u8 = 10;
    pub const FADING: u8 = 11;
    pub const ABSORB: u8 = 12;
    pub const GROW_UP: u8 = 13;
    pub const ENTER: u8 = 14;
    pub const EXIT: u8 = 15;
    pub const DEAD: u8 = 16;
    /// The Onion's plucker, out pulling sprouts nobody plucked.
    pub const PLUCKER: u8 = 17;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikPiki {
    /// The creature it's attacking, latched to or inside, with `has_creature`.
    pub creature: u64,
    pub pos: PikVec3,
    pub vel: PikVec3,
    pub face: f32,
    pub size: f32,
    pub id: u32,
    pub generation: u32,
    /// Its captain, or -1.
    pub captain: i32,
    /// The pellet it carries, or -1.
    pub pellet: i32,
    /// Out of `piki_max_health` (100 by default).
    pub health: f32,
    /// A [`piki_state`] value.
    pub state: u8,
    /// [`PikiColor::index`].
    pub color: u8,
    /// 0 leaf, 1 bud, 2 flower.
    pub happa: u8,
    pub alive: u8,
    pub has_creature: u8,
    pub on_ground: u8,
    /// 1 while frozen ([`pik_sim_freeze_pikis_in_sphere`]).
    pub frozen: u8,
    pub _pad: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikPellet {
    pub creature: u64,
    pub pos: PikVec3,
    pub yaw: f32,
    pub radius: f32,
    /// Shrinks while it's sucked into the Onion.
    pub scale: f32,
    pub id: u32,
    /// The Onion it's heading for, or -1.
    pub onion: i32,
    pub carriers: u32,
    pub min_carriers: u32,
    pub max_carriers: u32,
    /// 0 on the ground, 1 carried, 2 being sucked in, 3 gone.
    pub state: u8,
    pub has_creature: u8,
    /// Ship parts never participate in a host's automatic Onion ground pickup.
    pub has_ship_part: u8,
    /// A numbered pellet's [`PikiColor`] index (0 blue, 1 red, 2 yellow), or 255.
    pub color: u8,
    pub _pad: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikOnion {
    pub pos: PikVec3,
    pub id: u32,
    /// Bit `PikiColor::index` set for each colour it takes.
    pub colors: u32,
    /// Pikmin inside, by colour index (all leaf stages).
    pub stored: [u32; 5],
    /// 0 idle, 1 suction, 2 seeds, 3 takeoff, 4 away, 5 landing, 6 hover, 7 dormant, 8 reviving.
    pub anim: u8,
    pub is_ship: u8,
    pub _pad: [u8; 2],
}

/// A Candypop Bud ([`pik_sim_bulbs`]), 36 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikBulb {
    pub pos: PikVec3,
    pub id: u32,
    /// The colour it spits now ([`PikiColor::index`]).
    pub colour: u32,
    /// 1 for the rainbow Candypop Pop.
    pub queen: u32,
    /// 0 opening, 1 open (a Pikmin went in), 2 closing, 3 spitting, 4 withering.
    pub state: u32,
    /// Seconds in that state.
    pub t: f32,
    /// Slots left.
    pub free: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikSprout {
    pub pos: PikVec3,
    pub id: u32,
    pub color: u8,
    pub happa: u8,
    /// 0 flying from the Onion, 1 buried, 2 seed showing, 3 pluckable, 4 wilted.
    pub state: u8,
    pub _pad: u8,
}

/// What the Onion window shows for one colour ([`libpikmin::onion_menu::OnionCounts`]).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikOnionCounts {
    /// Stored, not already on their way out.
    pub in_onion: u32,
    /// The captain's squad Pikmin of this colour.
    pub in_squad: u32,
    /// Pikmin and sprouts on the field plus pending exits, against `max_on_field`.
    pub on_field: u32,
    pub max_on_field: u32,
}

/// Event kinds ([`Event`] by variant). Field use per kind is listed on [`PikEvent`].
pub mod event_kind {
    pub const PIKI_ENTERED_ONION: u32 = 0;
    pub const PIKI_LEFT_ONION: u32 = 1;
    pub const ONION_LANDED: u32 = 2;
    pub const NECTAR_SPLASH: u32 = 3;
    pub const NECTAR_GONE: u32 = 4;
    pub const PIKI_DRINKING: u32 = 5;
    pub const BLOOMED: u32 = 6;
    pub const WHISTLE: u32 = 7;
    pub const RECRUITED: u32 = 8;
    pub const THROWN: u32 = 9;
    pub const LANDED: u32 = 10;
    pub const LATCHED: u32 = 11;
    pub const HIT: u32 = 12;
    pub const KNOCKED_OFF: u32 = 13;
    pub const PIKI_HURT: u32 = 14;
    pub const SHIP_PART_COLLECTED: u32 = 15;
    pub const CARRIER_GRABBED: u32 = 16;
    pub const DEFLOWERED: u32 = 17;
    pub const PIKI_GONE: u32 = 18;
    pub const PIKI_DIED: u32 = 19;
    pub const SWALLOWED: u32 = 20;
    pub const SPIT_OUT: u32 = 21;
    pub const CAPTAIN_HURT: u32 = 22;
    pub const CAPTAIN_DOWN: u32 = 23;
    pub const DISMISSED: u32 = 24;
    pub const PUNCHED: u32 = 25;
    pub const PELLET_LIFTED: u32 = 26;
    pub const PELLET_REROUTED: u32 = 27;
    pub const PELLET_DROPPED: u32 = 28;
    pub const PELLET_ARRIVED: u32 = 29;
    pub const PELLET_DELIVERED: u32 = 30;
    pub const SEED_LAUNCHED: u32 = 31;
    pub const SEED_STORED: u32 = 32;
    pub const SPROUT_PLANTED: u32 = 33;
    pub const SPROUT_GREW: u32 = 34;
    pub const SPROUT_WILTED: u32 = 35;
    pub const PLUCKED: u32 = 36;
    /// `b` the Onion, `a` its owner, `c` how many Pikmin it sent.
    pub const EMERGENCY_PIKMIN: u32 = 37;
    /// `a` the plucker, `b` its Onion.
    pub const PLUCKER_OUT: u32 = 38;
    /// `a` the plucked Pikmin, `c` the plucker, `b` the captain it joins or -1.
    pub const PLUCKER_PULLED: u32 = 39;
    /// `a` the captain, `c` 1 for a mid-air jump.
    pub const CAPTAIN_JUMPED: u32 = 40;
    /// `a` the captain, `pos` where it landed, `value` the drop in sim units.
    pub const CAPTAIN_STOMPED: u32 = 41;
    /// `a` the Pikmin, `b` its captain, `pos` where it landed.
    pub const PIKI_STOMPED: u32 = 42;
    /// `a` the captain, `value` the fall in sim units, `c` 1 for a stomp's landing.
    pub const CAPTAIN_LANDED: u32 = 43;
    /// `a` the Pikmin, `c` the stage it grew to (0 leaf, 1 bud, 2 flower) from kills.
    pub const PIKI_GREW: u32 = 44;
    /// `a` the captain began a dismiss strike.
    pub const STRIKE_STARTED: u32 = 45;
    /// `a` the Pikmin, `b` its captain: it took off to repeat his jump-pad throw.
    pub const PIKI_LAUNCHED: u32 = 46;
    /// `a` the Purple Pikmin, `pos` where it came down on something or the ground.
    pub const POUNDED: u32 = 47;
    /// `a` the Pikmin that pounded; the creature fields name the flying creature it brought down.
    pub const GROUNDED: u32 = 48;
    /// `a` the White Pikmin that was eaten; the creature fields name the creature that ate it.
    pub const POISONED_EATER: u32 = 49;
    /// `a` the Pikmin that went into a Candypop Bud, `b` the bud, `c` its colour, `pos` the bud.
    pub const BULB_SWALLOWED: u32 = 50;
    /// `a` the bud, `c` the colour it spat, `b` how many sprouts, `pos` where they leave.
    pub const BULB_SHOT: u32 = 51;
    /// `a` the heaviest Pikmin latched on, `b` the weight (a Purple counts five); the creature fields name the flier.
    pub const WEIGHED_DOWN: u32 = 52;
}

/// One [`Event`]. `a` is the Pikmin, pellet, sprout or captain it's about (the first id in the
/// variant), `b` the second id (onion, captain...), `c` a colour, leaf stage or count,
/// `creature` the creature with `has_creature`, `value` damage/health/radius, `pos` a place.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikEvent {
    pub creature: u64,
    pub pos: PikVec3,
    pub value: f32,
    pub kind: u32,
    pub a: u32,
    pub b: i32,
    pub c: i32,
    pub has_creature: u8,
    pub _pad: [u8; 7],
}

/// Sizes of every struct here, for the host to check against its own layouts.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikSizes {
    pub vec3: u32,
    pub host: u32,
    pub input: u32,
    pub creature: u32,
    pub part: u32,
    pub pellet_desc: u32,
    pub captain: u32,
    pub piki: u32,
    pub pellet: u32,
    pub onion: u32,
    pub sprout: u32,
    pub event: u32,
    pub onion_counts: u32,
    pub model_info: u32,
    pub batch: u32,
    pub texture: u32,
    pub draw: u32,
    pub view_frame: u32,
    pub particle: u32,
}

/// # Safety
/// `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_struct_sizes(out: *mut PikSizes) {
    use std::mem::size_of;
    let s = PikSizes {
        vec3: size_of::<PikVec3>() as u32,
        host: size_of::<PikHost>() as u32,
        input: size_of::<PikInput>() as u32,
        creature: size_of::<PikCreature>() as u32,
        part: size_of::<PikPart>() as u32,
        pellet_desc: size_of::<PikPelletDesc>() as u32,
        captain: size_of::<PikCaptain>() as u32,
        piki: size_of::<PikPiki>() as u32,
        pellet: size_of::<PikPellet>() as u32,
        onion: size_of::<PikOnion>() as u32,
        sprout: size_of::<PikSprout>() as u32,
        event: size_of::<PikEvent>() as u32,
        onion_counts: size_of::<PikOnionCounts>() as u32,
        model_info: size_of::<PikModelInfo>() as u32,
        batch: size_of::<PikBatch>() as u32,
        texture: size_of::<PikTexture>() as u32,
        draw: size_of::<PikDraw>() as u32,
        view_frame: size_of::<PikViewFrame>() as u32,
        particle: size_of::<PikParticle>() as u32,
    };
    // SAFETY: the caller promised a writable pointer.
    if let Some(o) = unsafe { out.as_mut() } {
        *o = s;
    }
}

// ---------------------------------------------------------------------------------------------
// Assets

/// Whether `dir` (UTF-8) holds a complete extraction.
///
/// # Safety
/// `dir` must be null or a nul-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_assets_complete(dir: *const c_char) -> u8 {
    // SAFETY: as documented.
    let Some(dir) = (unsafe { path_arg(dir) }) else {
        return 0;
    };
    catch_unwind(|| libpikmin_assets::extract::is_complete(&dir) as u8).unwrap_or(0)
}

/// One disc HUD digit (0..9), optionally its coloured shadow. RGBA8, rows top first,
/// like `pik_view_texture`. Null `rgba` queries the dimensions without copying pixels.
/// Returns 0 for a missing/bad asset, invalid digit or insufficient output space.
///
/// # Safety
/// `dir` null or nul-terminated UTF-8; `info` null or writable; `rgba` null or writable
/// for `len` bytes. This does not need a sim or a model view.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_number_texture(
    dir: *const c_char, digit: u32, shadow: u8, info: *mut PikTexture, rgba: *mut u8, len: u32,
) -> u8 {
    let Some(root) = (unsafe { path_arg(dir) }) else { return 0 };
    catch_unwind(|| {
        let assets = libpikmin_assets::Assets { root };
        let Ok(image) = libpikmin_assets::screen::number_texture(&assets, digit, shadow != 0) else { return 0 };
        // SAFETY: caller promises writable outputs of the documented lengths.
        unsafe {
            if let Some(info) = info.as_mut() {
                *info = PikTexture { width: image.width, height: image.height, wrap_s: 1, wrap_t: 1, _pad: [0; 2] };
            }
            if !rgba.is_null() {
                if (len as usize) < image.rgba.len() { return 0; }
                std::ptr::copy_nonoverlapping(image.rgba.as_ptr(), rgba, image.rgba.len());
            }
        }
        1
    }).unwrap_or(0)
}

/// A K, M, B or ! damage-number suffix from the disc's bigFont bitmap,
/// also used by the day/location screens. The output is a 64x64 RGBA glyph.
/// # Safety
/// Same writable-buffer contract as [`pik_number_texture`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_damage_suffix_texture(
    dir: *const c_char, glyph: u32, info: *mut PikTexture, rgba: *mut u8, len: u32,
) -> u8 {
    let Some(root) = (unsafe { path_arg(dir) }) else { return 0 };
    let c = match glyph { 0 => b'K', 1 => b'M', 2 => b'B', 3 => b'!', _ => return 0 };
    catch_unwind(|| {
        let assets = libpikmin_assets::Assets { root };
        let Ok(data) = assets.read(libpikmin_assets::screen::Font::PATH) else { return 0 };
        let Ok((image, _, _)) = libpikmin_assets::texture::decode_bti(&data) else { return 0 };
        let font = libpikmin_assets::screen::Font::new(image, 21, 42);
        let Some(g) = font.glyph(c) else { return 0 };
        if g.width <= 0 || g.height <= 0 { return 0; }
        let mut pixels = vec![0u8; 64 * 64 * 4];
        for y in 0..56usize {
            for x in 0..56usize {
                let sx = g.x as usize + x * g.width as usize / 56;
                let sy = g.y as usize + y * g.height as usize / 56;
                let from = (sy * font.image.width as usize + sx) * 4;
                let to = ((y + 4) * 64 + x + 4) * 4;
                pixels[to..to + 4].copy_from_slice(&font.image.rgba[from..from + 4]);
            }
        }
        unsafe {
            if let Some(info) = info.as_mut() {
                *info = PikTexture { width: 64, height: 64, wrap_s: 1, wrap_t: 1, _pad: [0; 2] };
            }
            if !rgba.is_null() {
                if (len as usize) < pixels.len() { return 0; }
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), rgba, pixels.len());
            }
        }
        1
    }).unwrap_or(0)
}

/// One printable ASCII glyph from Pikmin's bigFont.bti, used by the landing
/// banner and Ship Repair score screens. RGBA8, rows top first.
/// # Safety
/// Same writable-buffer contract as [`pik_number_texture`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_font_glyph_texture(
    dir: *const c_char, character: u32, info: *mut PikTexture, rgba: *mut u8, len: u32,
) -> u8 {
    let Some(root) = (unsafe { path_arg(dir) }) else { return 0 };
    if !(0x21..=0x7e).contains(&character) { return 0; }
    catch_unwind(|| {
        static FONTS: OnceLock<Mutex<HashMap<PathBuf, libpikmin_assets::screen::Font>>> = OnceLock::new();
        let Ok(mut fonts) = FONTS.get_or_init(|| Mutex::new(HashMap::new())).lock() else { return 0 };
        if !fonts.contains_key(&root) {
            let assets = libpikmin_assets::Assets { root: root.clone() };
            let Ok(data) = assets.read(libpikmin_assets::screen::Font::PATH) else { return 0 };
            let Ok((image, _, _)) = libpikmin_assets::texture::decode_bti(&data) else { return 0 };
            fonts.insert(root.clone(), libpikmin_assets::screen::Font::new(image, 21, 42));
        }
        let font = &fonts[&root];
        let Some(g) = font.glyph(character as u8) else { return 0 };
        if g.width <= 0 || g.height <= 0 { return 0; }
        let mut pixels = vec![0u8; 64 * 64 * 4];
        for y in 0..56usize {
            for x in 0..56usize {
                let sx = g.x as usize + x * g.width as usize / 56;
                let sy = g.y as usize + y * g.height as usize / 56;
                let from = (sy * font.image.width as usize + sx) * 4;
                let to = ((y + 4) * 64 + x + 4) * 4;
                pixels[to..to + 4].copy_from_slice(&font.image.rgba[from..from + 4]);
            }
        }
        unsafe {
            if let Some(info) = info.as_mut() {
                *info = PikTexture { width: 64, height: 64, wrap_s: 1, wrap_t: 1, _pad: [0; 2] };
            }
            if !rgba.is_null() {
                if (len as usize) < pixels.len() { return 0; }
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), rgba, pixels.len());
            }
        }
        1
    }).unwrap_or(0)
}

/// The original carry gauge's horizontal 0-9, dash glyph strip
/// (`LifeGaugeMgr::init`, `gauges.cpp`). RGBA8, rows top first. Null `rgba`
/// queries dimensions. The host selects one of its eleven equal UV cells.
///
/// # Safety
/// Same writable-buffer contract as [`pik_number_texture`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_carry_digit_texture(
    dir: *const c_char, info: *mut PikTexture, rgba: *mut u8, len: u32,
) -> u8 {
    let Some(root) = (unsafe { path_arg(dir) }) else { return 0 };
    catch_unwind(|| {
        let assets = libpikmin_assets::Assets { root };
        let Ok(data) = assets.read("intro/item_0_9.bti") else { return 0 };
        let Ok((image, _, _)) = libpikmin_assets::texture::decode_bti(&data) else { return 0 };
        if image.width == 0 || image.width % 11 != 0 { return 0; }
        // SAFETY: caller promises writable outputs of the documented lengths.
        unsafe {
            if let Some(info) = info.as_mut() {
                *info = PikTexture { width: image.width, height: image.height, wrap_s: 1, wrap_t: 1, _pad: [0; 2] };
            }
            if !rgba.is_null() {
                if (len as usize) < image.rgba.len() { return 0; }
                std::ptr::copy_nonoverlapping(image.rgba.as_ptr(), rgba, image.rgba.len());
            }
        }
        1
    }).unwrap_or(0)
}

/// One of the original lower-HUD icons from `screen/eng_tex/`: 0 Olimar,
/// 1 health backing, 2 Pikmin backing, 3 no Pikmin, 4-6 Blue/Red/Yellow leaf,
/// 7 Dolphin icon, 8 X button, 9 A button, 10 star, 11 menu backing,
/// 12 Pikmin multiplication sign, 13-18 bud/flower throw portraits,
/// 19 the circular backing, 20 the low-life pulse, and 21 the original
/// w08_160.bti rounded number window (screen/eng_tex).
/// RGBA8, rows top first. Null `rgba` queries dimensions.
/// # Safety
/// Same writable-buffer contract as [`pik_number_texture`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_hud_icon_texture(
    dir: *const c_char, icon: u32, info: *mut PikTexture, rgba: *mut u8, len: u32,
) -> u8 {
    let Some(root) = (unsafe { path_arg(dir) }) else { return 0 };
    let name = match icon {
        0 => "navi01.bti", 1 => "n_64.bti", 2 => "i_base64.bti",
        3 => "piki0.bti", 4 => "bp_l64.bti", 5 => "rp_l64.bti", 6 => "yp_l64.bti",
        7 => "i_rocket.bti", 8 => "x_btn.bti", 9 => "a_btn.bti",
        10 => "star2.bti", 11 => "p_bg.bti",
        12 => "x_64.bti",
        13 => "bp_b64.bti", 14 => "rp_b64.bti", 15 => "yp_b64.bti",
        16 => "bp_f64.bti", 17 => "rp_f64.bti", 18 => "yp_f64.bti",
        19 => "0p.bti", 20 => "pinch64.bti",
        21 => "w08_160.bti",
        _ => return 0,
    };
    catch_unwind(|| {
        let assets = libpikmin_assets::Assets { root };
        let path = format!("screen/eng_tex/{name}");
        let Ok(data) = assets.read(&path) else { return 0 };
        let Ok((image, _, _)) = libpikmin_assets::texture::decode_bti(&data) else { return 0 };
        // SAFETY: caller promises writable outputs of the documented lengths.
        unsafe {
            if let Some(info) = info.as_mut() {
                *info = PikTexture { width: image.width, height: image.height, wrap_s: 1, wrap_t: 1, _pad: [0; 2] };
            }
            if !rgba.is_null() {
                if (len as usize) < image.rgba.len() { return 0; }
                std::ptr::copy_nonoverlapping(image.rgba.as_ptr(), rgba, image.rgba.len());
            }
        }
        1
    }).unwrap_or(0)
}

/// Looks for a Pikmin disc image (.iso/.gcm/.ciso) in each of the `;`-separated folders in
/// `dirs` and writes the first one's path (UTF-8, nul-terminated) to `out`. Returns 0 if none
/// was found or it didn't fit in `out_len` bytes.
///
/// # Safety
/// `dirs` a nul-terminated string; `out` writable for `out_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_find_disc(dirs: *const c_char, out: *mut u8, out_len: u32) -> u8 {
    if dirs.is_null() {
        return 0;
    }
    // SAFETY: as documented.
    let dirs = unsafe { CStr::from_ptr(dirs) }.to_string_lossy().into_owned();
    let found = catch_unwind(|| {
        let dirs: Vec<PathBuf> = dirs.split(';').filter(|s| !s.is_empty()).map(PathBuf::from).collect();
        libpikmin_assets::extract::find_disc(&dirs)
    });
    match found {
        // SAFETY: as documented.
        Ok(Some(p)) => unsafe { write_str(&p.to_string_lossy(), out, out_len) },
        _ => 0,
    }
}

/// Like [`pik_find_disc`] for a Pikmin 2 image (`GPV*`), for Louie.
///
/// # Safety
/// `dirs` a nul-terminated string; `out` writable for `out_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_p2_find_disc(dirs: *const c_char, out: *mut u8, out_len: u32) -> u8 {
    if dirs.is_null() {
        return 0;
    }
    // SAFETY: as documented.
    let dirs = unsafe { CStr::from_ptr(dirs) }.to_string_lossy().into_owned();
    let found = catch_unwind(|| {
        let dirs: Vec<PathBuf> = dirs.split(';').filter(|s| !s.is_empty()).map(PathBuf::from).collect();
        libsms::extract::p2::find_disc(&dirs)
    });
    match found {
        // SAFETY: as documented.
        Ok(Some(p)) => unsafe { write_str(&p.to_string_lossy(), out, out_len) },
        _ => 0,
    }
}

/// Like [`pik_find_disc`] for the user's Melee image (GALE01 revision 2), for the Pikachu skin.
///
/// # Safety
/// `dirs` a nul-terminated string; `out` writable for `out_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_ssbm_find_disc(dirs: *const c_char, out: *mut u8, out_len: u32) -> u8 {
    if dirs.is_null() {
        return 0;
    }
    // SAFETY: as documented.
    let dirs = unsafe { CStr::from_ptr(dirs) }.to_string_lossy().into_owned();
    let found = catch_unwind(|| {
        for dir in dirs.split(';').filter(|s| !s.is_empty()) {
            let Ok(rd) = std::fs::read_dir(dir) else { continue };
            let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            entries.sort();
            for p in entries {
                let ext = p.extension().and_then(|e| e.to_str()).map(str::to_lowercase);
                if matches!(ext.as_deref(), Some("iso" | "gcm" | "ciso")) && libssbm_assets::disc::Disc::open(&p).is_ok() {
                    return Some(p);
                }
            }
        }
        None
    });
    match found {
        // SAFETY: as documented.
        Ok(Some(p)) => unsafe { write_str(&p.to_string_lossy(), out, out_len) },
        _ => 0,
    }
}

/// First-run extraction: builds the assets folder `out_dir` from the user's disc image at
/// `disc`. Does nothing if `out_dir` is already complete. Returns 1 on success; either way a
/// short message (the error, or what was done) goes to `msg`.
///
/// # Safety
/// `disc` and `out_dir` nul-terminated strings; `msg` null or writable for `msg_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_extract(
    disc: *const c_char,
    out_dir: *const c_char,
    msg: *mut u8,
    msg_len: u32,
) -> u8 {
    // SAFETY: as documented.
    let (Some(disc), Some(out)) = (unsafe { path_arg(disc) }, unsafe { path_arg(out_dir) }) else {
        return 0;
    };
    let result = catch_unwind(|| {
        libpikmin_assets::extract::extract_assets(&disc, &out, &mut |_| {})
    });
    let (ok, text) = match result {
        Ok(Ok(r)) => (1, format!("{}: {} files into {}", r.game_id, r.files, r.out.display())),
        Ok(Err(e)) => (0, e.to_string()),
        Err(_) => (0, "the extractor panicked".to_string()),
    };
    // SAFETY: as documented.
    unsafe { write_str(&text, msg, msg_len) };
    ok
}

/// Cache the intro in `dir/ror2_audio` from extracted banks, without an audio device.
/// Existing complete files are reused. Returns 1 on success, with a UTF-8 diagnostic.
///
/// # Safety
/// `dir` null or nul-terminated UTF-8; `msg` null or writable for `msg_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_render_stings(dir: *const c_char, msg: *mut u8, msg_len: u32) -> u8 {
    let result = catch_unwind(|| -> Result<(), String> {
        let dir = unsafe { path_arg(dir) }.ok_or("missing asset directory")?;
        libpikmin_present::stings::render_stings(&dir)
    });
    let (ok, text) = match result {
        Ok(Ok(())) => (1, "Impact Site intro cached".into()),
        Ok(Err(e)) => (0, e),
        Err(_) => (0, "intro rendering panicked".into()),
    };
    unsafe { write_str(&text, msg, msg_len) };
    ok
}

/// # Safety
/// `p` null or a nul-terminated string.
unsafe fn path_arg(p: *const c_char) -> Option<PathBuf> {
    if p.is_null() {
        return None;
    }
    // SAFETY: the caller promised a nul-terminated string.
    Some(PathBuf::from(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()))
}

/// Writes `s` nul-terminated into `out` (truncated to fit). Returns 1 if it all fit.
///
/// # Safety
/// `out` null or writable for `len` bytes.
unsafe fn write_str(s: &str, out: *mut u8, len: u32) -> u8 {
    if out.is_null() || len == 0 {
        return 0;
    }
    let n = s.len().min(len as usize - 1);
    // SAFETY: `n + 1 <= len` bytes, which the caller promised are writable.
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), out, n);
        *out.add(n) = 0;
    }
    (n == s.len()) as u8
}

// ---------------------------------------------------------------------------------------------
// The sim

/// Most events kept for a reader that hasn't caught up.
const EVENT_BACKLOG: usize = 16384;

/// An opaque sim handle.
pub struct PikSim {
    sim: Sim,
    part_specs: Vec<Option<PelletDesc>>,
    /// Pikmin 2's treasures from the Pikmin 2 cache beside the assets (`<assets>/p2/treasure`), by their index in
    /// `otakara_config.txt`; a bad config is `None` so the indices stay put.
    treasure_specs: Vec<Option<TreasureSpec>>,
    /// The creatures the host gave last, copied into the sim each tick.
    frame: CreatureFrame,
    /// Events for [`pik_sim_events`].
    pending: VecDeque<Event>,
    /// Events for [`pik_view_update`] (animations, sounds).
    view_log: Vec<Event>,
    /// Events for the next [`pik_sim_snapshot`] (what clients' replicas need to play).
    net_log: Vec<Event>,
    networked: bool,
    /// Set on a replica ([`pik_replica_new`]): the sim a client lays the server's snapshots over.
    replica: Option<Replica>,
    params_source: CString,
    poisoned: bool,
}

/// A replica's snapshots, played back on a steady clock a little behind the server.
///
/// Snapshots arrive unevenly (the network bunches them, and a big squad's are split into fragments),
/// so blending toward each one as it lands stops and starts: the swarm moves in steps. Instead they
/// wait in `queue`, and `play` (in server ticks) runs at the server's rate, `delay` ticks behind the
/// newest the server is estimated to have sent, so there is nearly always a snapshot on either side
/// of it to blend between. The delay is the usual gap between snapshots plus room for the jitter
/// seen so far; `play` speeds up or slows down a little to keep to it, and jumps when far off.
struct Replica {
    /// Snapshots `play` hasn't reached yet, oldest first.
    queue: std::collections::VecDeque<Snapshot>,
    /// The pair `play` is between, the later one applied to the sim.
    from: Option<Snapshot>,
    to: Option<Snapshot>,
    /// Playback position and this machine's clock, both in server ticks.
    play: f64,
    clock: f64,
    /// The server's tick minus `clock`, from the least-delayed arrivals; how far arrivals stray
    /// from it; and the usual ticks between snapshots.
    offset: f64,
    jitter: f64,
    gap: f64,
    started: bool,
}

impl Replica {
    fn new() -> Self {
        Self {
            queue: std::collections::VecDeque::new(),
            from: None,
            to: None,
            play: 0.0,
            clock: 0.0,
            offset: 0.0,
            jitter: 2.0,
            gap: 3.0,
            started: false,
        }
    }

    /// The newest snapshot held.
    fn newest_tick(&self) -> Option<u64> {
        self.queue.back().or(self.to.as_ref()).map(|s| s.tick)
    }

    /// Ticks behind the server's newest that playback aims for.
    fn delay(&self) -> f64 {
        (self.gap + 2.0 * self.jitter + 1.0).clamp(2.0, 24.0)
    }

    /// A snapshot arrived: update the clock estimates and queue it.
    fn push(&mut self, snap: Snapshot) {
        let sample = snap.tick as f64 - self.clock;
        if !self.started {
            self.started = true;
            self.offset = sample;
            self.play = snap.tick as f64 - self.delay();
        } else {
            if let Some(last) = self.newest_tick() {
                self.gap += ((snap.tick.saturating_sub(last) as f64).clamp(1.0, 30.0) - self.gap) * 0.1;
            }
            // An early arrival is the truer offset (less delay); a late one only drifts it down slowly.
            let d = sample - self.offset;
            self.offset += if d > 0.0 { d * 0.5 } else { d * 0.02 };
            // Quick to widen the margin, slow to narrow it again.
            let spread = d.abs().min(30.0);
            self.jitter += (spread - self.jitter) * if spread > self.jitter { 0.3 } else { 0.03 };
        }
        self.queue.push_back(snap);
        while self.queue.len() > 64 {
            self.queue.pop_front();
        }
    }

    /// `dt` seconds on: moves `play` toward where it should be.
    fn step_clock(&mut self, dt: f32) {
        let ticks = dt.clamp(0.0, 0.25) as f64 * libpikmin::TICK_HZ as f64;
        self.clock += ticks;
        // Past the newest with nothing queued: the margin was too small; widen it, which slows
        // playback until it has a snapshot ahead again.
        if self.queue.is_empty() && self.to.as_ref().is_some_and(|t| self.play >= t.tick as f64) {
            self.jitter = (self.jitter + ticks * 0.5).min(11.0);
        }
        let target = self.clock + self.offset - self.delay();
        let err = target - self.play;
        if err.abs() > 30.0 {
            // Far behind (a stall, a burst after one): catch up at once. Never back: the view's
            // clock only goes forward.
            self.play = self.play.max(target);
        } else {
            self.play += ticks * (1.0 + err * 0.05).clamp(0.85, 1.15);
        }
    }

    /// The next queued snapshot is due: `play` has reached the one shown, or nothing is shown yet.
    fn due(&self) -> bool {
        !self.queue.is_empty() && self.to.as_ref().is_none_or(|t| t.tick as f64 <= self.play)
    }

    /// How far `play` is from `from` to `to`.
    fn fraction(&self) -> f32 {
        match (&self.from, &self.to) {
            (Some(a), Some(b)) if b.tick > a.tick => ((self.play - a.tick as f64) / (b.tick - a.tick) as f64).clamp(0.0, 1.0) as f32,
            _ => 1.0,
        }
    }
}

struct FfiHost<'a> {
    host: &'a PikHost,
    frame: &'a CreatureFrame,
    path: Vec<PikVec3>,
}

impl Host for FfiHost<'_> {
    fn ground_height(&self, pos: Vec3) -> Option<f32> {
        let f = self.host.ground_height?;
        let mut y = 0.0;
        // SAFETY: the host promised a valid callback; both pointers live for the call.
        let hit = unsafe { f(self.host.user, &PikVec3::from(pos), &mut y) };
        (hit != 0).then_some(y)
    }

    fn walls(&self, centre: Vec3, radius: f32) -> Vec3 {
        let Some(f) = self.host.walls else {
            return centre;
        };
        let mut out = PikVec3::from(centre);
        // SAFETY: as above.
        unsafe { f(self.host.user, &PikVec3::from(centre), radius, &mut out) };
        let out = Vec3::from(out);
        if out.is_finite() { out } else { centre }
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.creatures.clone_from(&self.frame.creatures);
        out.parts.clone_from(&self.frame.parts);
    }

    fn damage(&mut self, creature: CreatureId, amount: f32, part: Option<u32>, from: DamageSource) -> bool {
        let Some(f) = self.host.damage else {
            return false;
        };
        let (kind, id) = match from {
            DamageSource::Piki(p) => (PIK_SOURCE_PIKI, p.0),
            DamageSource::Captain(c) => (PIK_SOURCE_CAPTAIN, c.0),
            DamageSource::Strike(c) => (PIK_SOURCE_STRIKE, c.0),
        };
        let part = part.map_or(-1, |p| p as i32);
        // SAFETY: as above.
        unsafe { f(self.host.user, creature, amount, part, kind, id) != 0 }
    }

    fn carry_path(&mut self, from: Vec3, to: Vec3, out: &mut Vec<Vec3>) -> bool {
        out.clear();
        let Some(f) = self.host.carry_path else {
            out.push(to);
            return true;
        };
        const MAX: usize = 256;
        self.path.resize(MAX, PikVec3::default());
        // SAFETY: as above; `path` has room for MAX points.
        let n = unsafe {
            f(self.host.user, &PikVec3::from(from), &PikVec3::from(to), self.path.as_mut_ptr(), MAX as u32)
        };
        if n < 0 {
            return false;
        }
        out.extend(self.path[..(n as usize).min(MAX)].iter().map(|&p| Vec3::from(p)));
        if out.is_empty() {
            out.push(to);
        }
        true
    }
}

/// Creates a sim with the disc's tuning from the assets folder `assets_dir` (UTF-8; null for
/// code defaults) and RNG `seed`. If the folder can't be read it still starts, on code
/// defaults; [`pik_sim_params_source`] says which. Free with [`pik_sim_free`].
///
/// # Safety
/// `assets_dir` null or a nul-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_new(assets_dir: *const c_char, seed: u64) -> *mut PikSim {
    // SAFETY: as documented.
    let dir = unsafe { path_arg(assets_dir) };
    let made = catch_unwind(|| {
        let mut params = Params::default();
        let source = match dir.as_ref() {
            None => "code defaults".to_string(),
            Some(dir) => match params.load_extracted(Path::new(&dir)) {
                Ok(applied) if !applied.is_empty() => format!("disc ({} files)", applied.len()),
                Ok(_) => format!("code defaults (no parms in {})", dir.display()),
                Err(e) => {
                    params = Params::default();
                    format!("code defaults ({e})")
                }
            },
        };
        // Purple and White's tuning, when the Pikmin 2 files were extracted beside the Pikmin 1 ones (Louie's skin).
        if let Some(dir) = dir.as_ref() {
            params.load_pikmin2_dir(&dir.join("p2").join("parms"));
        }
        let part_specs = load_part_specs(&params, dir.as_deref());
        let treasure_specs = load_treasure_specs(dir.as_deref());
        (Sim::new(params, seed), source, part_specs, treasure_specs)
    });
    match made {
        Ok((sim, source, part_specs, treasure_specs)) => Box::into_raw(Box::new(PikSim {
            sim,
            part_specs,
            treasure_specs,
            frame: CreatureFrame::default(),
            pending: VecDeque::new(),
            view_log: Vec::new(),
            net_log: Vec::new(),
            networked: true,
            replica: None,
            params_source: CString::new(source).unwrap_or_default(),
            poisoned: false,
        })),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Overrides the disc's field population limit for hosts with a larger swarm budget.
/// `i32::MAX` removes the original game's 100-Pikmin gameplay cap.
///
/// # Safety
/// `sim` is a live handle from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_field_limit(sim: *mut PikSim, limit: u32) {
    unsafe { with(sim, (), |s| s.sim.params_mut().ai.max_pikis_on_field = limit.min(i32::MAX as u32) as i32) }
}

// PelletConfig/PelletShapeObject (pelletMgr.cpp): dimensions and weights come from
// the user's disc; absent or invalid required assets disable spawning that part.
fn load_part_specs(params: &Params, dir: Option<&Path>) -> Vec<Option<PelletDesc>> {
    let Some(assets) = libpikmin_assets::Assets::discover(dir) else { return Vec::new() };
    let infos = assets.read(libpikmin_assets::ship::PEL_ANIM).ok()
        .and_then(|b| libpikmin_assets::ship::parse_pel_anim(&b).ok()).unwrap_or_default();
    libpikmin_assets::ship::PART_IDS.iter().enumerate().map(|(part, id)| {
        let cfg = params.pellets.iter().find(|c| c.kind == 3 && (c.model_id == *id || c.pellet_id == *id))?;
        let info = infos.iter().find(|i| i.id == *id)?;
        let model = assets.model(&info.model()).ok()?;
        let radius = model.coll.iter().find(|c| c.id == "cent").map(|c| c.radius * cfg.scale)?;
        if !radius.is_finite() || radius <= 0.0 || cfg.min_carriers <= 0 || cfg.max_carriers < cfg.min_carriers { return None; }
        Some(PelletDesc { pos: Vec3::ZERO, yaw: 0.0, radius, height: cfg.carry_info_height.max(1.0),
            min_carriers: cfg.min_carriers as u32, max_carriers: cfg.max_carriers as u32,
            seeds_matching: 0, seeds_other: 0, color: None, creature: None, ship_part: Some(part as u32) })
    }).collect()
}

/// A Pikmin 2 treasure as a Ship Repair objective: its pellet (a ship-part pellet from
/// [`libpikmin::TREASURE_PART_BASE`] up), its in-game name and its worth in Pokos.
struct TreasureSpec {
    desc: PelletDesc,
    name: String,
    value: u32,
}

// `PelletConfig` (pelletConfig.cpp) of `otakara_config.txt`: `radius` the footprint, `height`, `min`/`max` the
// carriers (`Pellet::getPelletConfigMin/Max`), `money` the Pokos; the name is from `pikmin2.bmg` (`TItemZukan`).
// The cache is what `libsms::extract::p2_treasure` writes; without it there are no treasures.
fn load_treasure_specs(dir: Option<&Path>) -> Vec<Option<TreasureSpec>> {
    let Some(dir) = dir else { return Vec::new() };
    let root = dir.join("p2").join("treasure");
    let Ok(table) = libsms_assets::treasure::Table::load(&root) else { return Vec::new() };
    table.treasures.iter().enumerate().map(|(index, c)| {
        if c.min == 0 || c.max < c.min || !c.radius.is_finite() || c.radius <= 0.0 { return None; }
        let name = table.display_name(c)?;
        Some(TreasureSpec {
            desc: PelletDesc { pos: Vec3::ZERO, yaw: 0.0, radius: c.radius, height: c.height.max(1.0),
                min_carriers: c.min, max_carriers: c.max.min(128),
                seeds_matching: 0, seeds_other: 0, color: None, creature: None,
                ship_part: Some(libpikmin::TREASURE_PART_BASE + index as u32) },
            name, value: c.money,
        })
    }).collect()
}

/// Whether the Pikmin 2 cache folder `dir` (the one [`pik_p2_extract`] fills) holds the treasure files too.
/// # Safety
/// `dir` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_p2_treasures_complete(dir: *const c_char) -> u8 {
    // SAFETY: as documented.
    let Some(dir) = (unsafe { path_arg(dir) }) else { return 0 };
    libsms::extract::p2_treasure::is_complete(&dir) as u8
}

/// Extracts Pikmin 2's treasures (models, configs, names, jingles) from the user's disc image `iso` into the Pikmin 2
/// cache folder `out`, beside Louie's files; 0 when done (or already complete), else 1.
/// # Safety
/// Both arguments are NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_p2_extract_treasures(iso: *const c_char, out: *const c_char) -> i32 {
    // SAFETY: as documented.
    let (Some(iso), Some(out)) = (unsafe { (path_arg(iso), path_arg(out)) }) else { return 1 };
    match catch_unwind(|| libsms::extract::p2_treasure::extract(&iso, &out, &mut |_| {})) {
        Ok(Ok(_)) => 0,
        _ => 1,
    }
}

/// How many treasures the Pikmin 2 cache holds (188 with it, 0 without).
/// # Safety
/// `sim` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_treasure_count(sim: *mut PikSim) -> u32 {
    unsafe { with(sim, 0, |s| s.treasure_specs.len() as u32) }
}

/// Treasure `index`'s carry requirement (Pikmin needed to lift it), or 0 when it is not usable.
/// # Safety
/// `sim` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_treasure_weight(sim: *mut PikSim, index: u32) -> u32 {
    unsafe { with(sim, 0, |s| s.treasure_specs.get(index as usize).and_then(|t| t.as_ref()).map_or(0, |t| t.desc.min_carriers)) }
}

/// Treasure `index`'s worth in Pokos (what the Onion pays in Pikmin 2), or 0 when it is not usable.
/// # Safety
/// `sim` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_treasure_value(sim: *mut PikSim, index: u32) -> u32 {
    unsafe { with(sim, 0, |s| s.treasure_specs.get(index as usize).and_then(|t| t.as_ref()).map_or(0, |t| t.value)) }
}

/// Copies treasure `index`'s English name, NUL terminated; returns the bytes written excluding the NUL.
/// # Safety
/// `out` is writable for `cap` bytes; `sim` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_treasure_name(sim: *mut PikSim, index: u32, out: *mut u8, cap: u32) -> u32 {
    if out.is_null() || cap == 0 { return 0; }
    unsafe { with(sim, 0, |s| {
        let Some(t) = s.treasure_specs.get(index as usize).and_then(|t| t.as_ref()) else { return 0 };
        let n = t.name.len().min(cap as usize - 1);
        std::ptr::copy_nonoverlapping(t.name.as_ptr(), out, n);
        *out.add(n) = 0;
        n as u32
    }) }
}

/// Spawns treasure `index` as a pellet a ship receives like a part; its part id in the receipt is
/// `TREASURE_PART_BASE + index` (1000 + index). Returns the pellet id or -1.
/// # Safety
/// `sim` is live; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_spawn_treasure(sim: *mut PikSim, index: u32, pos: *const PikVec3, yaw: f32) -> i32 {
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else { return -1 };
    if !Vec3::from(pos).is_finite() || !yaw.is_finite() { return -1; }
    unsafe { with(sim, -1, |s| {
        let Some(mut d) = s.treasure_specs.get(index as usize).and_then(|t| t.as_ref()).map(|t| t.desc) else { return -1 };
        d.pos = pos.into(); d.yaw = yaw;
        s.sim.spawn_pellet(d).0 as i32
    }) }
}

/// Host-selected receipt policy; the default continues to deliver to ships.
/// # Safety
/// `sim` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_ship_part_onion_delivery(sim: *mut PikSim, enabled: u8) {
    unsafe { with(sim, (), |s| s.sim.set_ship_part_onion_delivery(enabled != 0)) }
}

/// Ship delivery with an authoritative host receipt before fitting.
/// # Safety
/// `sim` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_ship_part_host_delivery(sim: *mut PikSim, enabled: u8) {
    unsafe { with(sim, (), |s| s.sim.set_ship_part_host_delivery(enabled != 0)) }
}

/// The part's disc carry requirement, or zero when its assets/config are invalid.
/// # Safety
/// `sim` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_ship_part_weight(sim: *mut PikSim, part: u32) -> u32 {
    unsafe { with(sim, 0, |s| s.part_specs.get(part as usize).and_then(|d| *d).map_or(0, |d| d.min_carriers)) }
}

/// Copies a UTF-8 part name, NUL terminated. Returns bytes written excluding NUL.
/// # Safety
/// `out` is writable for `cap` bytes; `sim` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_ship_part_name(sim: *mut PikSim, part: u32, out: *mut u8, cap: u32) -> u32 {
    if out.is_null() || cap == 0 { return 0; }
    unsafe { with(sim, 0, |s| {
        let Some(id) = libpikmin_assets::ship::PART_IDS.get(part as usize) else { return 0 };
        // The part's name in the game (`UfoPartID`), not its config's file name.
        let name = libpikmin_assets::ship::PART_NAMES.get(part as usize).copied().or_else(|| {
            s.sim.params().pellets.iter().find(|c| c.kind == 3 && (c.model_id == *id || c.pellet_id == *id)).map(|c| c.name.as_str())
        });
        let Some(name) = name else { return 0 };
        let n = name.len().min(cap as usize - 1);
        std::ptr::copy_nonoverlapping(name.as_ptr(), out, n);
        *out.add(n) = 0;
        n as u32
    }) }
}

/// Spawns an actual ship-part pellet using its extracted dimensions and weight.
/// # Safety
/// `sim` is live; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_spawn_ship_part(sim: *mut PikSim, part: u32, pos: *const PikVec3, yaw: f32) -> i32 {
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else { return -1 };
    if !Vec3::from(pos).is_finite() || !yaw.is_finite() { return -1; }
    unsafe { with(sim, -1, |s| {
        let Some(mut d) = s.part_specs.get(part as usize).and_then(|d| *d) else { return -1 };
        d.pos = pos.into(); d.yaw = yaw;
        s.sim.spawn_pellet(d).0 as i32
    }) }
}

/// A host character (Mario) takes the pellet in his hands, or moves the one he holds.
/// # Safety
/// `sim` is live; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_hold_pellet(sim: *mut PikSim, pellet: u32, pos: *const PikVec3, yaw: f32) -> u8 {
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else { return 0 };
    unsafe { with(sim, 0, |s| s.sim.hold_pellet(libpikmin::PelletId(pellet), pos.into(), yaw) as u8) }
}

/// Puts a held pellet down at `pos`.
/// # Safety
/// `sim` is live; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_release_pellet(sim: *mut PikSim, pellet: u32, pos: *const PikVec3, yaw: f32) -> u8 {
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else { return 0 };
    unsafe { with(sim, 0, |s| s.sim.release_pellet(libpikmin::PelletId(pellet), pos.into(), yaw) as u8) }
}

/// Puts a held or loose ship part into `ship`'s receiver by hand; returns whether the ship took it.
/// # Safety
/// `sim` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_hand_ship_part(sim: *mut PikSim, ship: u32, pellet: u32) -> u8 {
    unsafe { with(sim, 0, |s| s.sim.hand_ship_part(libpikmin::OnionId(ship), libpikmin::PelletId(pellet)) as u8) }
}

/// Marks `ship` as a captain-less host character's (Mario's): parts go in by hand, no Pikmin deliver.
/// # Safety
/// `sim` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_ship_hand_owned(sim: *mut PikSim, ship: u32) {
    unsafe { with(sim, (), |s| s.sim.set_ship_hand_owned(libpikmin::OnionId(ship))) }
}

/// # Safety
/// `sim` is live; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_recover_pellet(sim: *mut PikSim, pellet: u32, pos: *const PikVec3) -> u8 {
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else { return 0 };
    unsafe { with(sim, 0, |s| s.sim.recover_pellet(libpikmin::PelletId(pellet), pos.into()) as u8) }
}

/// # Safety
/// `sim` from [`pik_sim_new`] and not used afterwards. Null is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_free(sim: *mut PikSim) {
    if !sim.is_null() {
        // SAFETY: the caller hands back ownership.
        drop(unsafe { Box::from_raw(sim) });
    }
}

/// Where the tuning came from ("disc (N files)" or "code defaults (why)"). Valid until the sim
/// is freed.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_params_source(sim: *const PikSim) -> *const c_char {
    // SAFETY: as documented.
    match unsafe { sim.as_ref() } {
        Some(s) => s.params_source.as_ptr(),
        None => std::ptr::null(),
    }
}

/// Whether a panic has stopped this sim.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_poisoned(sim: *const PikSim) -> u8 {
    // SAFETY: as documented.
    unsafe { sim.as_ref() }.is_none_or(|s| s.poisoned) as u8
}

/// Runs `f` on a live sim, poisoning it if `f` panics. `fallback` when the sim is null or poisoned.
///
/// # Safety
/// `sim` null or from [`pik_sim_new`], not aliased for the call.
unsafe fn with<R>(sim: *mut PikSim, fallback: R, f: impl FnOnce(&mut PikSim) -> R) -> R {
    // SAFETY: as documented.
    let Some(s) = (unsafe { sim.as_mut() }) else {
        return fallback;
    };
    if s.poisoned {
        return fallback;
    }
    match catch_unwind(AssertUnwindSafe(|| f(s))) {
        Ok(r) => r,
        Err(_) => {
            s.poisoned = true;
            fallback
        }
    }
}

/// Adds a captain standing at `pos`. Returns his id (0, 1, ...).
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_add_captain(sim: *mut PikSim, pos: *const PikVec3) -> i32 {
    // SAFETY: as documented.
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else {
        return -1;
    };
    // SAFETY: as documented.
    unsafe { with(sim, -1, |s| s.sim.add_captain(pos.into()).0 as i32) }
}

/// # Safety
/// `sim` from [`pik_sim_new`]; `input` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_input(sim: *mut PikSim, captain: u32, input: *const PikInput) {
    // SAFETY: as documented.
    let Some(input) = (unsafe { input.as_ref() }).copied() else {
        return;
    };
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            if (captain as usize) < s.sim.captains().len() {
                s.sim.set_input(CaptainId(captain), input.to_input());
            }
        })
    }
}

/// The colour `captain` throws next ([`PikiColor::index`]) once the player has picked one
/// with `PikInput::cycle`; -1 while he throws the nearest Pikmin.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_captain_throw_color(sim: *mut PikSim, captain: u32) -> i32 {
    // SAFETY: as documented.
    unsafe {
        with(sim, -1, |s| {
            s.sim
                .captains()
                .get(captain as usize)
                .and_then(|c| c.throw_color)
                .map_or(-1, |c| c.index() as i32)
        })
    }
}

/// `on` (non-zero): separated Pikmin go back to their captain by themselves; dismissed ones
/// still wait (see `Sim::set_auto_rejoin`).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_auto_rejoin(sim: *mut PikSim, on: u8) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_auto_rejoin(on != 0)) }
}

/// Pikmin latched for less than `seconds` hold on through [`pik_sim_shake_off`] (see
/// `Sim::set_shake_protection`). 0 protects none, as in Pikmin 1.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_shake_protection(sim: *mut PikSim, seconds: f32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_shake_protection(seconds)) }
}

/// Scales how far `captain`'s cursor reaches, and so how far he throws, by `scale`.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_captain_reach(sim: *mut PikSim, captain: u32, scale: f32) {
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            if (captain as usize) < s.sim.captains().len() {
                s.sim.set_captain_reach(CaptainId(captain), scale);
            }
        })
    }
}

/// Scales `captain`'s run speed by `scale` (the host's move speed stat over its base).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_captain_speed(sim: *mut PikSim, captain: u32, scale: f32) {
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            if (captain as usize) < s.sim.captains().len() {
                s.sim.set_captain_speed(CaptainId(captain), scale);
            }
        })
    }
}

/// Host control hold: 0 free, 1 movement only, 2 movement and actions.
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_captain_hold(sim: *mut PikSim, captain: u32, hold: u32) {
    unsafe { with(sim, (), |s| s.sim.set_captain_hold(CaptainId(captain), hold != 0, hold >= 2)) }
}

/// Launch a captain with a host velocity, in sim units per second.
/// `gravity` is host gravity in sim units/s² (0 uses disc tuning); `squad` queues the
/// same takeoff for squad members when nonzero (pads, not knockback).
/// # Safety
/// `sim` from [`pik_sim_new`]; `velocity` readable (null is ignored).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_launch_captain(sim: *mut PikSim, captain: u32, velocity: *const PikVec3, gravity: f32, squad: u8) {
    let Some(v) = (unsafe { velocity.as_ref() }) else { return; };
    unsafe { with(sim, (), |s| s.sim.launch_captain(CaptainId(captain), (*v).into(), gravity, squad != 0)) }
}

/// Spawns a Pikmin of colour `color` ([`PikiColor::index`]) at `pos`, straight into
/// `captain`'s squad unless that's -1. Returns its id, or -1 at the field limit.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_spawn_piki(sim: *mut PikSim, color: u32, pos: *const PikVec3, captain: i32) -> i32 {
    // SAFETY: as documented.
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else {
        return -1;
    };
    let Some(color) = color_arg(color) else {
        return -1;
    };
    // SAFETY: as documented.
    unsafe {
        with(sim, -1, |s| {
            let id = if captain >= 0 && (captain as usize) < s.sim.captains().len() {
                s.sim.spawn_piki_in_squad(color, pos.into(), CaptainId(captain as u32))
            } else {
                s.sim.spawn_piki(color, pos.into())
            };
            id.map_or(-1, |p| p.0 as i32)
        })
    }
}

/// Restores a Pikmin's leaf stage without changing its task.
/// # Safety
/// `sim` from `pik_sim_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_piki_happa(sim: *mut PikSim, piki: u32, happa: u32) {
    let Some(happa) = happa_arg(happa) else { return };
    unsafe { with(sim, (), |s| {
        if (piki as usize) < s.sim.pikis().len() { s.sim.piki_mut(PikiId(piki)).happa = happa; }
    }) }
}

/// Stored count for one colour and leaf stage; preserves maturity across host stage transfers.
/// # Safety
/// `sim` from `pik_sim_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_onion_stock(sim: *mut PikSim, onion: u32, color: u32, happa: u32) -> u32 {
    if color_arg(color).is_none() || happa_arg(happa).is_none() { return 0; }
    unsafe { with(sim, 0, |s| s.sim.onions().get(onion as usize).map_or(0, |o| o.stored[color as usize][happa as usize])) }
}

/// Settles living death-recall stragglers in their Onion with their leaf stages intact.
/// # Safety
/// `sim` from `pik_sim_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_finish_orphan_recall(sim: *mut PikSim, captain: u32, onion: u32) {
    unsafe { with(sim, (), |s| {
        if captain_ok(&s.sim, captain) { s.sim.finish_orphan_recall(CaptainId(captain), OnionId(onion)); }
    }) }
}

/// Places an Onion at `pos` taking the colours in the bit mask `colors` (bit
/// [`PikiColor::index`]). Returns its id.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_add_onion(sim: *mut PikSim, pos: *const PikVec3, colors: u32) -> i32 {
    // SAFETY: as documented.
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else {
        return -1;
    };
    let colors: Vec<PikiColor> = (0..5).filter(|i| colors & (1 << i) != 0).filter_map(PikiColor::from_id).collect();
    if colors.is_empty() {
        return -1;
    }
    // SAFETY: as documented.
    unsafe { with(sim, -1, |s| s.sim.add_onion(pos.into(), &colors).0 as i32) }
}

/// Adds the S.S. Dolphin, independently of ship-repair gameplay.
/// # Safety
/// `sim` from [`pik_sim_new`]; `pos` readable. No ship-parts objective is enabled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_add_ship(sim: *mut PikSim, pos: *const PikVec3, yaw: f32) -> i32 {
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else { return -1 };
    let pos: Vec3 = pos.into();
    if !pos.is_finite() || !yaw.is_finite() { return -1; }
    unsafe { with(sim, -1, |s| s.sim.add_ship(pos,
        pos + Vec3::new(yaw.sin(), 0.0, yaw.cos()) * 100.0,
        pos + Vec3::Y * 74.0).0 as i32) }
}

/// Fits ship part `part` (0..30) to the Dolphin `ship` ([`Sim::give_ship_part`]); with all 30 it is the
/// repaired ship, which the view draws with them when [`pik_view_set_repaired_ship`] was on at load.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_give_ship_part(sim: *mut PikSim, ship: u32, part: u32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.give_ship_part(OnionId(ship), part)) }
}

/// Updates a host-controlled Onion's pose and airborne state.
/// # Safety
/// `sim` from [`pik_sim_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_onion_pose(sim: *mut PikSim, onion: u32, pos: *const PikVec3, flying: u8) {
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else { return };
    unsafe { with(sim, (), |s| s.sim.set_onion_pose(OnionId(onion), pos.into(), flying != 0)) }
}

/// Disables an Onion, retaining its stock, until its timed disc startup completes.
/// # Safety
/// `sim` from pik_sim_new; `ground` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_make_onion_dormant(sim: *mut PikSim, onion: u32, seconds: f32, ground: *const PikVec3) {
    let Some(ground) = (unsafe { ground.as_ref() }).copied() else { return };
    unsafe { with(sim, (), |s| s.sim.make_onion_dormant(OnionId(onion), seconds, ground.into())) }
}

/// Loose ground pickup; validates destination, proximity and pellet state.
/// # Safety
/// `sim` from pik_sim_new.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_collect_ground_pellet(sim: *mut PikSim, onion: u32, pellet: u32) -> u8 {
    unsafe { with(sim, 0, |s| s.sim.collect_ground_pellet(OnionId(onion), libpikmin::PelletId(pellet)) as u8) }
}

/// Selects a host-timed disc flight transition without changing gameplay availability.
/// # Safety
/// `sim` from [`pik_sim_new`]. `landing` selects landing rather than takeoff.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_onion_flight_motion(sim: *mut PikSim, onion: u32, landing: u8, progress: f32) {
    unsafe { with(sim, (), |s| s.sim.set_onion_flight_motion(OnionId(onion), landing != 0, progress)) }
}

/// Selects a cosmetic colour without changing storage/accepted types.
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_onion_color(sim: *mut PikSim, onion: u32, color: u32) {
    let Some(color) = color_arg(color) else { return };
    unsafe { with(sim, (), |s| s.sim.set_onion_color(OnionId(onion), color)) }
}

/// Puts something carryable down. Returns its id.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `desc` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_spawn_pellet(sim: *mut PikSim, desc: *const PikPelletDesc) -> i32 {
    // SAFETY: as documented.
    let Some(d) = (unsafe { desc.as_ref() }).copied() else {
        return -1;
    };
    let desc = PelletDesc {
        pos: d.pos.into(),
        yaw: d.yaw,
        radius: d.radius,
        height: d.height,
        min_carriers: d.min_carriers.max(1),
        max_carriers: d.max_carriers.max(d.min_carriers).max(1),
        seeds_matching: d.seeds_matching,
        seeds_other: d.seeds_other,
        color: u32::try_from(d.color).ok().and_then(color_arg),
        creature: (d.has_creature != 0).then_some(d.creature),
        ship_part: None,
    };
    // SAFETY: as documented.
    unsafe { with(sim, -1, |s| s.sim.spawn_pellet(desc).0 as i32) }
}

/// Puts down a numbered pellet: the largest of 1, 5, 10 and 20 not above `number`, in
/// [`PikiColor`] index `color`, with the disc's carriers, seeds and size
/// ([`PelletDesc::numbered`]). Returns its id, or -1.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_spawn_numbered_pellet(
    sim: *mut PikSim,
    number: u32,
    color: u32,
    pos: *const PikVec3,
    yaw: f32,
) -> i32 {
    // SAFETY: as documented.
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else {
        return -1;
    };
    let Some(color) = color_arg(color) else {
        return -1;
    };
    // SAFETY: as documented.
    unsafe {
        with(sim, -1, |s| {
            let desc = PelletDesc::numbered(s.sim.params(), number, color, pos.into(), yaw);
            s.sim.spawn_pellet(desc).0 as i32
        })
    }
}

/// Plants a sprout of colour `color` at `pos`, ready to grow and be plucked.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_plant_sprout(sim: *mut PikSim, color: u32, pos: *const PikVec3) -> i32 {
    // SAFETY: as documented.
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else {
        return -1;
    };
    let Some(color) = color_arg(color) else {
        return -1;
    };
    // SAFETY: as documented.
    unsafe { with(sim, -1, |s| s.sim.plant_sprout(color, pos.into()).0 as i32) }
}

/// The host's creatures for the coming ticks: `n` creatures and the `m` parts they index into.
/// Copied; the arrays can be reused right after. Call before [`pik_sim_advance`] each frame.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `creatures` readable for `n` and `parts` for `m` (or null if 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_creatures(
    sim: *mut PikSim,
    creatures: *const PikCreature,
    n: u32,
    parts: *const PikPart,
    m: u32,
) {
    // SAFETY: as documented.
    let cs = unsafe { slice(creatures, n) };
    // SAFETY: as documented.
    let ps = unsafe { slice(parts, m) };
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            let f = &mut s.frame;
            f.clear();
            f.parts.extend(ps.iter().map(|p| CreaturePart {
                centre: p.centre.into(),
                radius: p.radius,
                stickable: p.stickable != 0,
            }));
            for c in cs {
                // Parts out of range would index past the array inside the sim.
                let end = c.parts_start as u64 + c.parts_len as u64;
                let len = if end <= ps.len() as u64 { c.parts_len } else { 0 };
                f.creatures.push(CreatureView {
                    id: c.id,
                    pos: c.pos.into(),
                    yaw: c.yaw,
                    alive: c.alive != 0,
                    flying: c.flying != 0,
                    parts_start: c.parts_start,
                    parts_len: len,
                });
            }
        })
    }
}

/// Feeds `dt` seconds of host time into the fixed 30 Hz clock and runs the ticks that are due,
/// calling `host` for the world. Returns how many ticks ran.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `host` readable, with callbacks safe to call as documented.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_advance(sim: *mut PikSim, host: *const PikHost, dt: f32) -> u32 {
    // SAFETY: as documented.
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            s.sim.reclaim_dead_pikis(s.pending.iter().chain(&s.view_log).chain(&s.net_log).flat_map(Event::piki_ids));
            let mut h = FfiHost { host, frame: &s.frame, path: Vec::new() };
            let ticks = s.sim.advance(&mut h, dt.clamp(0.0, 0.25));
            let events: Vec<Event> = s.sim.drain_events().collect();
            // Nobody reading either queue mustn't grow it forever: keep the newest.
            if s.view_log.len() + events.len() <= EVENT_BACKLOG {
                s.view_log.extend_from_slice(&events);
            }
            if s.networked && s.net_log.len() + events.len() <= EVENT_BACKLOG {
                s.net_log.extend_from_slice(&events);
            }
            s.pending.extend(events);
            while s.pending.len() > EVENT_BACKLOG {
                s.pending.pop_front();
            }
            ticks
        })
    }
}

/// Whether this sim has network snapshot consumers. Disable for solo/local prediction so
/// undeliverable network events cannot pin retired entity slots.
/// # Safety
/// `sim` from `pik_sim_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_networked(sim: *mut PikSim, enabled: u8) {
    unsafe { with(sim, (), |s| {
        s.networked = enabled != 0;
        if !s.networked { s.net_log.clear(); }
    }) }
}

/// Encodes the sim's entities, and the events since the last call, into `out` (at most `cap`
/// bytes) for [`pik_replica_apply`] on another machine; returns the length, or 0 if it didn't fit
/// (nothing is consumed then). The server calls this a few times a second.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `out` writable for `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_snapshot(sim: *mut PikSim, out: *mut u8, cap: u32) -> u32 {
    if out.is_null() {
        return 0;
    }
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            let bytes = Snapshot::of(&s.sim, &s.net_log).encode_replica();
            if bytes.len() > cap as usize {
                return 0;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
            s.net_log.clear();
            bytes.len() as u32
        })
    }
}

/// Creates a replica: a sim that never steps, only holds what the server's snapshots say, so the
/// view can draw and sound it as it would the real one. Tuning loads as in [`pik_sim_new`] (it
/// should match the server's). Free with [`pik_sim_free`].
///
/// # Safety
/// `assets_dir` null or a nul-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_replica_new(assets_dir: *const c_char) -> *mut PikSim {
    // SAFETY: as documented.
    let sim = unsafe { pik_sim_new(assets_dir, 0) };
    // SAFETY: just made, or null.
    if let Some(s) = unsafe { sim.as_mut() } {
        s.replica = Some(Replica::new());
    }
    sim
}

/// A snapshot from the server's [`pik_sim_snapshot`] arrived. Returns whether it was accepted
/// (this is a replica, and the bytes are a snapshot of this build). It is queued and shown when
/// the replica's playback clock reaches it ([`pik_replica_update`]); the first is shown at once.
/// Its events play when it is shown.
///
/// # Safety
/// `sim` from [`pik_replica_new`]; `bytes` readable for `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_replica_apply(sim: *mut PikSim, bytes: *const u8, len: u32) -> u8 {
    if bytes.is_null() {
        return 0;
    }
    // SAFETY: as documented.
    let data = unsafe { std::slice::from_raw_parts(bytes, len as usize) };
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            let Some(r) = s.replica.as_mut() else { return 0 };
            let Some(snap) = Snapshot::decode(data) else { return 0 };
            // A newer one only: a late packet from before the newest on hand is dropped.
            if r.newest_tick().is_some_and(|t| snap.tick <= t) {
                return 0;
            }
            r.push(snap);
            if s.replica.as_ref().is_some_and(|r| r.to.is_none()) {
                replica_show_due(s);
            }
            1
        })
    }
}

/// Shows every queued snapshot the replica's playback has reached: each is applied to the sim
/// in turn, with its events queued for the view.
fn replica_show_due(s: &mut PikSim) {
    loop {
        let Some(r) = s.replica.as_mut() else { return };
        if !r.due() {
            return;
        }
        let Some(mut next) = r.queue.pop_front() else { return };
        // Several may be shown before a render. Don't resolve an older occupant's deferred
        // spatial cue through a recycled slot. Ghosts carry their own position.
        if let Some(previous) = &r.to {
            s.view_log.retain(|event| matches!(event, Event::PikiGone { .. }) || event.piki_ids().all(|id| {
                previous.pikis.get(id.0 as usize).zip(next.pikis.get(id.0 as usize))
                    .is_some_and(|(a, b)| a.generation == b.generation)
            }));
        }
        let events = std::mem::take(&mut next.events);
        if s.view_log.len() + events.len() <= EVENT_BACKLOG {
            s.view_log.extend(events);
        }
        let clock = s.sim.tick();
        s.sim.apply_snapshot(&next);
        s.sim.set_tick(clock.max(next.tick.min(r.play.max(0.0) as u64)));
        // Where the creatures were, for the sounds of what they do.
        s.frame.clear();
        for &(id, pos, yaw) in &next.creatures {
            s.frame.creatures.push(CreatureView { id, pos, yaw, alive: true, flying: false, parts_start: 0, parts_len: 0 });
        }
        r.from = r.to.replace(next);
    }
}

/// Moves a replica `dt` seconds on: its playback clock runs at the server's tick rate a little
/// behind the newest snapshot, shows each snapshot it reaches, and blends positions between the
/// two either side of it; the tick follows the clock so the view's per-tick work (sounds,
/// animation) runs at its normal rate. Call once a frame before [`pik_view_update`].
///
/// # Safety
/// `sim` from [`pik_replica_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_replica_update(sim: *mut PikSim, dt: f32) {
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            let Some(r) = s.replica.as_mut() else { return };
            if r.to.is_none() && r.queue.is_empty() {
                return;
            }
            r.step_clock(dt);
            replica_show_due(s);
            let Some(r) = s.replica.as_ref() else { return };
            let Some(to) = r.to.as_ref() else { return };
            let from = r.from.as_ref().unwrap_or(to);
            let t = r.fraction();
            s.sim.blend_snapshot_motion(from, to, t);
            let play = r.play.max(0.0) as u64;
            s.sim.set_tick(s.sim.tick().max(play));
        })
    }
}

/// On a replica, shows `captain` (the server's id for this player's captain) moving as captain
/// `from_captain` of `mover` does, so a client's own Olimar responds at once instead of a round
/// trip late. `pos` (3 floats) and `face` (angle) override the mover's, if `pos` is non-null: pass
/// the smoothed body. Call after [`pik_replica_update`] each frame, before the view.
///
/// # Safety
/// `replica` from [`pik_replica_new`]; `mover` from [`pik_sim_new`]; `pos` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_replica_overlay_captain(
    replica: *mut PikSim,
    captain: u32,
    mover: *mut PikSim,
    from_captain: u32,
    pos: *const PikVec3,
    face: f32,
) {
    // SAFETY: as documented.
    let Some(m) = (unsafe { mover.as_ref() }) else { return };
    if m.poisoned {
        return;
    }
    let Some(from) = m.sim.captains().get(from_captain as usize).cloned() else { return };
    // SAFETY: as documented.
    let pos = unsafe { pos.as_ref() }.map(|p| Vec3::from(*p));
    // SAFETY: as documented.
    unsafe {
        with(replica, (), |s| {
            if s.replica.is_some() {
                s.sim.overlay_captain_motion(CaptainId(captain), &from, pos, pos.map(|_| face));
            }
        })
    }
}

/// How far into the next tick the clock is (0..1), for drawing between ticks.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_alpha(sim: *mut PikSim) -> f32 {
    // SAFETY: as documented.
    unsafe { with(sim, 0.0, |s| s.sim.alpha()) }
}

/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_tick(sim: *mut PikSim) -> u64 {
    // SAFETY: as documented.
    unsafe { with(sim, 0, |s| s.sim.tick()) }
}

/// A creature shook its Pikmin off (its shake attack landed).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_shake_off(sim: *mut PikSim, creature: u64) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.shake_off(creature)) }
}

/// An attack hits every Pikmin inside the sphere for `damage`. Returns how many it hit.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `centre` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_attack_pikis_in_sphere(
    sim: *mut PikSim,
    centre: *const PikVec3,
    radius: f32,
    damage: f32,
) -> u32 {
    // SAFETY: as documented.
    let Some(c) = (unsafe { centre.as_ref() }).copied() else {
        return 0;
    };
    // SAFETY: as documented.
    unsafe { with(sim, 0, |s| s.sim.attack_pikis_in_sphere(c.into(), radius, damage)) }
}

/// A freezing blast: every Pikmin inside the sphere stands frozen for `seconds`, letting go of
/// what it held ([`Sim::freeze_pikis_in_sphere`]). Returns how many froze.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `centre` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_freeze_pikis_in_sphere(
    sim: *mut PikSim,
    centre: *const PikVec3,
    radius: f32,
    seconds: f32,
) -> u32 {
    // SAFETY: as documented.
    let Some(c) = (unsafe { centre.as_ref() }).copied() else {
        return 0;
    };
    // SAFETY: as documented.
    unsafe { with(sim, 0, |s| s.sim.freeze_pikis_in_sphere(c.into(), radius, seconds)) }
}

/// Kills a Pikmin outright (a pit, a hazard the host decides on).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_kill_piki(sim: *mut PikSim, piki: u32) {
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            if (piki as usize) < s.sim.pikis().len() {
                s.sim.kill_piki(PikiId(piki));
            }
        })
    }
}

// ---------------------------------------------------------------------------------------------
// The Onion menu (`NaviContainerState`, naviState.cpp; see `libpikmin::onion_menu`). The
// original pauses the game while the window is up; a host that can't pause just doesn't.

/// The Onion `captain` is standing under, or -1.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_onion_in_reach(sim: *mut PikSim, captain: u32) -> i32 {
    // SAFETY: as documented.
    unsafe { with(sim, -1, |s| s.sim.onion_in_reach(CaptainId(captain)).map_or(-1, |o| o.0 as i32)) }
}

/// Fills `out` with what the Onion window would show for `color`. Returns 0 for a bad id.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_onion_counts(
    sim: *mut PikSim,
    onion: u32,
    captain: u32,
    color: u32,
    out: *mut PikOnionCounts,
) -> u8 {
    // SAFETY: as documented.
    let Some(out) = (unsafe { out.as_mut() }) else {
        return 0;
    };
    let Some(color) = color_arg(color) else {
        return 0;
    };
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            if !onion_ok(&s.sim, onion) {
                return 0;
            }
            let c = s.sim.onion_counts(OnionId(onion), CaptainId(captain), color);
            *out = PikOnionCounts {
                in_onion: c.in_onion,
                in_squad: c.in_squad,
                on_field: c.on_field,
                max_on_field: c.max_on_field,
            };
            1
        })
    }
}

/// Sends up to `n` of `captain`'s squad Pikmin of `color` into the Onion
/// (`NaviContainerState::enterPikis`). Returns how many went.
///
/// # Safety
/// The Pikmin captain `from` left behind join captain `to`'s squad; returns how many.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_adopt_orphans(sim: *mut PikSim, from: u32, to: u32) -> u32 {
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            let n = s.sim.captains().len() as u32;
            if from >= n || to >= n {
                return 0;
            }
            s.sim.adopt_orphans(CaptainId(from), CaptainId(to))
        })
    }
}

/// The Pikmin captain `from` left behind go into the landed Onion `onion`; returns how many.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_send_orphans_home(sim: *mut PikSim, from: u32, onion: u32) -> u32 {
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            if !onion_ok(&s.sim, onion) || from as usize >= s.sim.captains().len() {
                return 0;
            }
            s.sim.send_orphans_home(CaptainId(from), OnionId(onion))
        })
    }
}

/// How many seconds a dormant Onion still waits before reviving (0 wakes it).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_onion_dormancy_seconds(sim: *mut PikSim, onion: u32, seconds: f32) {
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            if onion_ok(&s.sim, onion) {
                s.sim.set_onion_dormancy_seconds(OnionId(onion), seconds);
            }
        })
    }
}

/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_onion_put_away(sim: *mut PikSim, captain: u32, onion: u32, color: u32, n: u32) -> u32 {
    let Some(color) = color_arg(color) else {
        return 0;
    };
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            if !onion_ok(&s.sim, onion) || captain as usize >= s.sim.captains().len() {
                return 0;
            }
            s.sim.onion_put_away(CaptainId(captain), OnionId(onion), color, n)
        })
    }
}

/// Calls up to `n` Pikmin of `color` out of the Onion to join `captain` (`GoalItem::exitPikis`),
/// within the stock and the field limit. Returns how many will come.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_onion_call_out(sim: *mut PikSim, captain: u32, onion: u32, color: u32, n: u32) -> u32 {
    let Some(color) = color_arg(color) else {
        return 0;
    };
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            if !onion_ok(&s.sim, onion) || captain as usize >= s.sim.captains().len() {
                return 0;
            }
            s.sim.onion_call_out(CaptainId(captain), OnionId(onion), color, n)
        })
    }
}

/// Sacrifice unreserved Onion stock, keeping one inside. Leaf/bud/flower restore 5/10/20%.
/// Returns the health fraction and writes the number consumed for presentation.
/// # Safety
/// `sim` from `pik_sim_new`; `killed` null or writable for one u32.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_sacrifice_onion(sim: *mut PikSim, onion: u32, missing: f32, killed: *mut u32) -> f32 {
    let (count, fraction) = unsafe { with(sim, (0, 0.0), |s| s.sim.sacrifice_onion_stock(OnionId(onion), missing)) };
    if !killed.is_null() { unsafe { *killed = count; } }
    fraction
}

/// Puts `n` Pikmin of `color` at leaf stage `happa` (0 leaf, 1 bud, 2 flower) straight into an
/// Onion's stock, for a host carrying Pikmin over from one level to the next.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_store_in_onion(sim: *mut PikSim, onion: u32, color: u32, happa: u32, n: u32) {
    let (Some(color), Some(happa)) = (color_arg(color), happa_arg(happa)) else {
        return;
    };
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            if onion_ok(&s.sim, onion) {
                s.sim.store_in_onion(OnionId(onion), color, happa, n);
            }
        })
    }
}

/// A real Onion (the ship has no menu).
fn onion_ok(sim: &Sim, onion: u32) -> bool {
    sim.onions().get(onion as usize).is_some_and(|o| o.ship.is_none())
}

/// # Safety
/// `sim` from [`pik_sim_new`]; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_captain(sim: *mut PikSim, captain: u32, out: *mut PikCaptain) -> u8 {
    // SAFETY: as documented.
    let Some(out) = (unsafe { out.as_mut() }) else {
        return 0;
    };
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            let Some(c) = s.sim.captains().get(captain as usize) else {
                return 0;
            };
            let id = CaptainId(captain);
            let squad = s.sim.pikis().iter().filter(|p| p.captain == Some(id) && p.state.in_squad()).count();
            *out = PikCaptain {
                pos: c.pos.into(),
                vel: c.vel.into(),
                face: c.face,
                cursor_world: c.cursor_world.into(),
                whistle_radius: c.whistle_radius(s.sim.params()),
                health: c.health,
                held_piki: c.held_piki().map_or(-1, |p| p.0 as i32),
                squad: squad as u32,
                on_ground: c.on_ground as u8,
                down: c.down as u8,
                throwing: matches!(c.throw, libpikmin::captain::ThrowState::Wait { .. }) as u8,
                plucking: c.pluck.is_some() as u8,
            };
            1
        })
    }
}

/// Copies up to `max` Pikmin (by id, the dead included) into `out`. Returns how many there are
/// in all, so a host can grow its buffer.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `out` writable for `max` (or null if 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_pikis(sim: *mut PikSim, out: *mut PikPiki, max: u32) -> u32 {
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            let all = s.sim.pikis();
            for (i, p) in all.iter().take(max as usize).enumerate() {
                let (creature, pellet) = match p.state {
                    PikiState::Attack(a) => (Some(a.target), None),
                    PikiState::Swallowed { creature, .. } => (Some(creature), None),
                    PikiState::Carry(c) => (None, Some(c.pellet.0)),
                    _ => (None, None),
                };
                let v = PikPiki {
                    creature: creature.unwrap_or(0),
                    pos: p.pos.into(),
                    vel: p.vel.into(),
                    face: p.face,
                    size: p.size,
                    id: p.id.0,
                    generation: p.generation,
                    captain: p.captain.map_or(-1, |c| c.0 as i32),
                    pellet: pellet.map_or(-1, |id| id as i32),
                    health: p.health,
                    state: state_code(&p.state),
                    color: p.color.index() as u8,
                    happa: happa_code(p.happa),
                    alive: p.is_alive() as u8,
                    has_creature: creature.is_some() as u8,
                    on_ground: p.on_ground as u8,
                    frozen: (p.frozen > 0.0) as u8,
                    _pad: 0,
                };
                // SAFETY: `i < max`, which the caller promised is writable.
                out.add(i).write(v);
            }
            all.len() as u32
        })
    }
}

/// Copies up to `max` pellets into `out`. Returns how many there are in all.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `out` writable for `max` (or null if 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_pellets(sim: *mut PikSim, out: *mut PikPellet, max: u32) -> u32 {
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            let all = s.sim.pellets();
            for (i, p) in all.iter().take(max as usize).enumerate() {
                let (state, onion) = match &p.state {
                    PelletState::Idle => (0, -1),
                    PelletState::Carried { onion, .. } => (1, onion.0 as i32),
                    PelletState::Sucking { onion, .. } => (2, onion.0 as i32),
                    PelletState::Gone => (3, -1),
                    PelletState::Held => (4, -1),
                };
                let v = PikPellet {
                    creature: p.desc.creature.unwrap_or(0),
                    pos: p.pos.into(),
                    yaw: p.yaw,
                    radius: p.desc.radius,
                    scale: p.scale,
                    id: p.id.0,
                    onion,
                    carriers: p.carriers.len() as u32,
                    min_carriers: p.desc.min_carriers,
                    max_carriers: p.desc.max_carriers,
                    state,
                    has_creature: p.desc.creature.is_some() as u8,
                    has_ship_part: p.desc.ship_part.is_some() as u8,
                    color: p.desc.color.map_or(255, |c| c as u8),
                    _pad: [0; 4],
                };
                // SAFETY: `i < max`, which the caller promised is writable.
                out.add(i).write(v);
            }
            all.len() as u32
        })
    }
}

/// Copies up to `max` Onions into `out`. Returns how many there are in all.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `out` writable for `max` (or null if 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_onions(sim: *mut PikSim, out: *mut PikOnion, max: u32) -> u32 {
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            let all = s.sim.onions();
            for (i, o) in all.iter().take(max as usize).enumerate() {
                let v = PikOnion {
                    pos: o.pos.into(),
                    id: o.id.0,
                    colors: o.colors.iter().fold(0, |m, c| m | 1 << c.index()),
                    stored: o.stored.map(|by_happa| by_happa.iter().sum()),
                    anim: if let Some(d) = o.dormancy { if d.reviving { 8 } else { 7 } } else { match o.anim {
                        OnionAnim::Idle => 0,
                        OnionAnim::Hover => 6,
                        OnionAnim::Suction { .. } => 1,
                        OnionAnim::Emitting { .. } => 2,
                        OnionAnim::TakeOff { .. } => 3,
                        OnionAnim::Away => 4,
                        OnionAnim::Landing { .. } => 5,
                    } },
                    is_ship: o.ship.is_some() as u8,
                    _pad: [0; 2],
                };
                // SAFETY: `i < max`, which the caller promised is writable.
                out.add(i).write(v);
            }
            all.len() as u32
        })
    }
}

/// Returns a receiver's actual carry destination, including the Dolphin's offset.
/// # Safety
/// `sim` is live and `out` is writable for one vector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_onion_goal(sim: *mut PikSim, id: u32, out: *mut PikVec3) -> u32 {
    if out.is_null() { return 0; }
    unsafe { with(sim, 0, |s| {
        let Some(o) = s.sim.onions().iter().find(|o| o.id.0 == id) else { return 0; };
        out.write(o.goal_pos().into());
        1
    }) }
}

/// Plants a Candypop Bud ([`libpikmin::bulb`]) of colour `colour` at `pos`; `queen` makes it the rainbow Pop. Returns its
/// id, or -1.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_add_bulb(sim: *mut PikSim, colour: u32, queen: u32, pos: *const PikVec3) -> i32 {
    // SAFETY: as documented.
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else { return -1 };
    let Some(colour) = color_arg(colour) else { return -1 };
    // SAFETY: as documented.
    unsafe { with(sim, -1, |s| s.sim.add_bulb(colour, queen != 0, pos.into()).0 as i32) }
}

/// Takes the Candypop Bud `id` away.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_remove_bulb(sim: *mut PikSim, id: u32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.remove_bulb(libpikmin::BulbId(id))) }
}

/// Copies up to `max` Candypop Buds into `out`. Returns how many there are in all.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `out` writable for `max` (or null if 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_bulbs(sim: *mut PikSim, out: *mut PikBulb, max: u32) -> u32 {
    use libpikmin::BulbState;
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            let bulbs = s.sim.bulbs();
            for (i, b) in bulbs.iter().take(max as usize).enumerate() {
                let (state, t) = match b.state {
                    BulbState::Open { t } => (if b.used > 0 || t >= libpikmin::bulb::OPEN_TIME { 1 } else { 0 }, t),
                    BulbState::Swing { t } => (1, t),
                    BulbState::Close { t } => (2, t),
                    BulbState::Shot { t, .. } => (3, t),
                    BulbState::Dead { t } => (4, t),
                };
                let v = PikBulb {
                    pos: b.pos.into(),
                    id: b.id.0,
                    colour: b.colour.index() as u32,
                    queen: b.queen as u32,
                    state,
                    t,
                    free: b.slots().saturating_sub(b.used),
                };
                // SAFETY: `i < max`, which the caller promised is writable.
                out.add(i).write(v);
            }
            bulbs.len() as u32
        })
    }
}

/// How much weight of Pikmin brings the flying creature `creature` down ([`libpikmin::weigh`]: 5 small, 15 large,
/// 25 boss, 1 for a projectile; a Purple counts five); 0 takes the rule away. The sim then raises `WEIGHED_DOWN`.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_ground_need(sim: *mut PikSim, creature: u64, need: u32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_ground_need(creature, need)) }
}

/// The weight of Pikmin that brings a flying creature of a class down ([`libpikmin::weigh`]): 0 a small flier (5), 1 a large
/// one (15), 2 a boss (25), 3 a projectile that can be shot (1). A Purple counts five. 0 for any other class.
#[unsafe(no_mangle)]
pub extern "C" fn pik_weigh_need(kind: u32) -> u32 {
    use libpikmin::weigh;
    match kind {
        0 => weigh::SMALL,
        1 => weigh::LARGE,
        2 => weigh::BOSS,
        3 => weigh::PROJECTILE,
        _ => 0,
    }
}

/// Copies up to `max` sprouts in the ground or in the air into `out`. Returns how many there
/// are in all.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `out` writable for `max` (or null if 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_sprouts(sim: *mut PikSim, out: *mut PikSprout, max: u32) -> u32 {
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            let mut n = 0u32;
            for sp in s.sim.sprouts() {
                if n < max {
                    let v = PikSprout {
                        pos: sp.pos.into(),
                        id: sp.id.0,
                        color: sp.color.index() as u8,
                        happa: happa_code(sp.happa),
                        state: match sp.state {
                            SproutState::Flying => 0,
                            SproutState::Buried { .. } => 1,
                            SproutState::Tane { .. } => 2,
                            SproutState::Ready { .. } => 3,
                            SproutState::Wilted { .. } => 4,
                        },
                        _pad: 0,
                    };
                    // SAFETY: `n < max`, which the caller promised is writable.
                    out.add(n as usize).write(v);
                }
                n += 1;
            }
            n
        })
    }
}

/// Moves up to `max` of the events since the last call into `out`, oldest first, and returns
/// how many. Call until it returns less than `max`.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `out` writable for `max`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_events(sim: *mut PikSim, out: *mut PikEvent, max: u32) -> u32 {
    if out.is_null() {
        return 0;
    }
    // SAFETY: as documented.
    unsafe {
        with(sim, 0, |s| {
            let mut n = 0;
            while n < max as usize
                && let Some(e) = s.pending.pop_front()
            {
                // SAFETY: `n < max`, which the caller promised is writable.
                out.add(n).write(event(e));
                n += 1;
            }
            n as u32
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Conversions

/// # Safety
/// `p` readable for `n` items, or `n` is 0.
unsafe fn slice<'a, T>(p: *const T, n: u32) -> &'a [T] {
    if p.is_null() || n == 0 {
        &[]
    } else {
        // SAFETY: the caller promised `n` readable items.
        unsafe { std::slice::from_raw_parts(p, n as usize) }
    }
}

fn color_arg(i: u32) -> Option<PikiColor> {
    PikiColor::from_id(i as usize)
}

fn happa_arg(i: u32) -> Option<Happa> {
    match i {
        0 => Some(Happa::Leaf),
        1 => Some(Happa::Bud),
        2 => Some(Happa::Flower),
        _ => None,
    }
}

fn happa_code(h: Happa) -> u8 {
    match h {
        Happa::Leaf => 0,
        Happa::Bud => 1,
        Happa::Flower => 2,
    }
}

fn state_code(s: &PikiState) -> u8 {
    use piki_state::*;
    match s {
        PikiState::Idle => IDLE,
        PikiState::Squad { .. } => SQUAD,
        PikiState::GoHang { .. } => GO_HANG,
        PikiState::Held { .. } => HELD,
        PikiState::Flying { .. } => FLYING,
        PikiState::Attack(_) => ATTACK,
        PikiState::Knocked { .. } => KNOCKED,
        PikiState::Carry(_) => CARRY,
        PikiState::Pressed { .. } => PRESSED,
        PikiState::Swallowed { .. } => SWALLOWED,
        PikiState::Dying { .. } => DYING,
        PikiState::Fading { .. } => FADING,
        PikiState::Absorb { .. } => ABSORB,
        PikiState::GrowUp { .. } => GROW_UP,
        PikiState::Enter { .. } => ENTER,
        PikiState::Exit { .. } => EXIT,
        PikiState::Plucker(_) => PLUCKER,
        PikiState::Dead => DEAD,
    }
}

fn event(e: Event) -> PikEvent {
    use event_kind::*;
    let mut v = PikEvent { b: -1, c: -1, ..Default::default() };
    let creature = |v: &mut PikEvent, c: CreatureId| {
        v.creature = c;
        v.has_creature = 1;
    };
    match e {
        Event::PikiEnteredOnion { piki, onion } => (v.kind, v.a, v.b) = (PIKI_ENTERED_ONION, piki.0, onion.0 as i32),
        Event::PikiLeftOnion { piki, onion } => (v.kind, v.a, v.b) = (PIKI_LEFT_ONION, piki.0, onion.0 as i32),
        Event::OnionLanded { onion } => (v.kind, v.a) = (ONION_LANDED, onion.0),
        Event::NectarSplash { pos } => (v.kind, v.pos) = (NECTAR_SPLASH, pos.into()),
        Event::NectarGone => v.kind = NECTAR_GONE,
        Event::PikiDrinking { piki } => (v.kind, v.a) = (PIKI_DRINKING, piki.0),
        Event::Bloomed { piki } => (v.kind, v.a) = (BLOOMED, piki.0),
        Event::Whistle { captain, centre, radius } => {
            (v.kind, v.a, v.pos, v.value) = (WHISTLE, captain.0, centre.into(), radius)
        }
        Event::Recruited { piki, captain, stolen_from } => {
            (v.kind, v.a, v.b) = (RECRUITED, piki.0, captain.0 as i32);
            v.c = stolen_from.map_or(-1, |c| c.0 as i32);
        }
        Event::Thrown { piki, captain } => (v.kind, v.a, v.b) = (THROWN, piki.0, captain.0 as i32),
        Event::Landed { piki } => (v.kind, v.a) = (LANDED, piki.0),
        Event::Latched { piki, creature: c } => {
            (v.kind, v.a) = (LATCHED, piki.0);
            creature(&mut v, c);
        }
        Event::Hit { piki, creature: c, damage, latched } => {
            (v.kind, v.a, v.value, v.c) = (HIT, piki.0, damage, latched as i32);
            creature(&mut v, c);
        }
        Event::KnockedOff { piki, creature: c } => {
            (v.kind, v.a) = (KNOCKED_OFF, piki.0);
            if let Some(c) = c {
                creature(&mut v, c);
            }
        }
        Event::PikiHurt { piki, damage } => (v.kind, v.a, v.value) = (PIKI_HURT, piki.0, damage),
        Event::ShipPartCollected { pellet, ship, part, total } => {
            (v.kind, v.a, v.b, v.c, v.value) = (SHIP_PART_COLLECTED, pellet.0, ship.0 as i32, part as i32, total as f32)
        }
        Event::CarrierGrabbed { piki, pellet } => (v.kind, v.a, v.b) = (CARRIER_GRABBED, piki.0, pellet.0 as i32),
        Event::Deflowered { piki } => (v.kind, v.a) = (DEFLOWERED, piki.0),
        Event::PikiGone { piki, pos } => (v.kind, v.a, v.pos) = (PIKI_GONE, piki.0, pos.into()),
        Event::PikiDied { piki, cause } => {
            let cause = match cause {
                DeathCause::Attacked => 0,
                DeathCause::Crushed => 1,
                DeathCause::Eaten => 2,
                DeathCause::Killed => 3,
            };
            (v.kind, v.a, v.c) = (PIKI_DIED, piki.0, cause)
        }
        Event::Swallowed { piki, creature: c } => {
            (v.kind, v.a) = (SWALLOWED, piki.0);
            creature(&mut v, c);
        }
        Event::SpitOut { piki } => (v.kind, v.a) = (SPIT_OUT, piki.0),
        Event::CaptainHurt { captain, damage, health } => {
            (v.kind, v.a, v.value, v.pos.x) = (CAPTAIN_HURT, captain.0, damage, health)
        }
        Event::CaptainDown { captain } => (v.kind, v.a) = (CAPTAIN_DOWN, captain.0),
        Event::Dismissed { captain } => (v.kind, v.a) = (DISMISSED, captain.0),
        Event::Punched { captain, creature: c, damage } => {
            (v.kind, v.a, v.value) = (PUNCHED, captain.0, damage);
            creature(&mut v, c);
        }
        Event::PelletLifted { pellet, onion } => (v.kind, v.a, v.b) = (PELLET_LIFTED, pellet.0, onion.0 as i32),
        Event::PelletRerouted { pellet, onion } => (v.kind, v.a, v.b) = (PELLET_REROUTED, pellet.0, onion.0 as i32),
        Event::PelletDropped { pellet } => (v.kind, v.a) = (PELLET_DROPPED, pellet.0),
        Event::PelletArrived { pellet, onion } => (v.kind, v.a, v.b) = (PELLET_ARRIVED, pellet.0, onion.0 as i32),
        Event::PelletDelivered { pellet, onion, color, seeds } => {
            (v.kind, v.a, v.b, v.c, v.value) =
                (PELLET_DELIVERED, pellet.0, onion.0 as i32, color.index() as i32, seeds as f32)
        }
        Event::SeedLaunched { onion, sprout, color } => {
            (v.kind, v.a, v.b, v.c) = (SEED_LAUNCHED, sprout.0, onion.0 as i32, color.index() as i32)
        }
        Event::SeedStored { onion, color } => (v.kind, v.b, v.c) = (SEED_STORED, onion.0 as i32, color.index() as i32),
        Event::SproutPlanted { sprout } => (v.kind, v.a) = (SPROUT_PLANTED, sprout.0),
        Event::SproutGrew { sprout, happa } => (v.kind, v.a, v.c) = (SPROUT_GREW, sprout.0, happa_code(happa) as i32),
        Event::SproutWilted { sprout } => (v.kind, v.a) = (SPROUT_WILTED, sprout.0),
        Event::Plucked { captain, piki, sprout } => {
            (v.kind, v.a, v.b, v.c) = (PLUCKED, piki.0, captain.0 as i32, sprout.0 as i32)
        }
        Event::EmergencyPikmin { onion, captain, count } => {
            (v.kind, v.a, v.b, v.c) = (EMERGENCY_PIKMIN, captain.0, onion.0 as i32, count as i32)
        }
        Event::PluckerOut { piki, onion } => (v.kind, v.a, v.b) = (PLUCKER_OUT, piki.0, onion.0 as i32),
        Event::PluckerPulled { plucker, plucked, captain } => {
            (v.kind, v.a, v.b, v.c) = (PLUCKER_PULLED, plucked.0, captain.map_or(-1, |c| c.0 as i32), plucker.0 as i32)
        }
        Event::CaptainJumped { captain, air } => (v.kind, v.a, v.c) = (CAPTAIN_JUMPED, captain.0, air as i32),
        Event::CaptainStomped { captain, pos, drop } => {
            (v.kind, v.a, v.pos, v.value) = (CAPTAIN_STOMPED, captain.0, pos.into(), drop)
        }
        Event::CaptainLanded { captain, drop, stomp } => {
            (v.kind, v.a, v.value, v.c) = (CAPTAIN_LANDED, captain.0, drop, stomp as i32)
        }
        Event::StrikeStarted { captain } => (v.kind, v.a) = (STRIKE_STARTED, captain.0),
        Event::PikiGrew { piki, happa } => (v.kind, v.a, v.c) = (PIKI_GREW, piki.0, happa_code(happa) as i32),
        Event::PikiStomped { piki, captain, pos } => {
            (v.kind, v.a, v.b, v.pos) = (PIKI_STOMPED, piki.0, captain.0 as i32, pos.into())
        }
        Event::PikiLaunched { piki, captain } => (v.kind, v.a, v.b) = (PIKI_LAUNCHED, piki.0, captain.0 as i32),
        Event::Pounded { piki, pos } => (v.kind, v.a, v.pos) = (POUNDED, piki.0, pos.into()),
        Event::Grounded { creature: c, piki } => {
            (v.kind, v.a) = (GROUNDED, piki.0);
            creature(&mut v, c);
        }
        Event::PoisonedEater { piki, creature: c } => {
            (v.kind, v.a) = (POISONED_EATER, piki.0);
            creature(&mut v, c);
        }
        Event::BulbSwallowed { piki, bulb, colour, pos } => {
            (v.kind, v.a, v.b, v.c, v.pos) = (BULB_SWALLOWED, piki.0, bulb.0 as i32, colour.index() as i32, pos.into())
        }
        Event::BulbShot { bulb, colour, count, pos } => {
            (v.kind, v.a, v.b, v.c, v.pos) = (BULB_SHOT, bulb.0, count as i32, colour.index() as i32, pos.into())
        }
        Event::WeighedDown { creature: c, piki, weight } => {
            (v.kind, v.a, v.b) = (WEIGHED_DOWN, piki.0, weight as i32);
            creature(&mut v, c);
        }
    }
    v
}

// ---- host item hooks (libpikmin's items.rs, plucker.rs, jump.rs) ---------------------------

fn captain_ok(sim: &Sim, captain: u32) -> bool {
    (captain as usize) < sim.captains().len()
}

/// [`Sim::set_captain_attack_rate`]: his punches and plucks and his Pikmin's attacks.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_captain_attack_rate(sim: *mut PikSim, captain: u32, rate: f32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| if captain_ok(&s.sim, captain) { s.sim.set_captain_attack_rate(CaptainId(captain), rate) }) }
}

/// [`Sim::set_squad_speed`].
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_squad_speed(sim: *mut PikSim, captain: u32, scale: f32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| if captain_ok(&s.sim, captain) { s.sim.set_squad_speed(CaptainId(captain), scale) }) }
}

/// [`Sim::set_captain_motion`]: his jumps' takeoff speed and his acceleration as multiples of
/// Pikmin's (the host's jump power and acceleration over their bases).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_captain_motion(sim: *mut PikSim, captain: u32, jump: f32, accel: f32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| if captain_ok(&s.sim, captain) { s.sim.set_captain_motion(CaptainId(captain), jump, accel) }) }
}

/// Match a host's jump height; squad jumps inherit the same takeoff speed.
/// # Safety
/// `sim` from `pik_sim_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_captain_jump_height(sim: *mut PikSim, captain: u32, height: f32) {
    unsafe { with(sim, (), |s| s.sim.set_captain_jump_height(CaptainId(captain), height)) }
}

/// [`Sim::set_captain_air_jumps`] and [`Sim::set_captain_stomp`].
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_captain_jumps(sim: *mut PikSim, captain: u32, air_jumps: u32, stomp: u8) {
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            if captain_ok(&s.sim, captain) {
                s.sim.set_captain_air_jumps(CaptainId(captain), air_jumps);
                s.sim.set_captain_stomp(CaptainId(captain), stomp != 0);
            }
        })
    }
}

/// [`Sim::boost_captain_jump`] (Wax Quail).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_boost_captain_jump(sim: *mut PikSim, captain: u32, factor: f32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| if captain_ok(&s.sim, captain) { s.sim.boost_captain_jump(CaptainId(captain), factor) }) }
}

/// [`Sim::set_sprout_stages`]: seconds per planted sprout's stage and its flower's time
/// (0: the disc's).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_sprout_stages(sim: *mut PikSim, stage: f32, flower: f32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_sprout_stages(stage, flower)) }
}

/// [`Sim::set_kill_growth`]: credited kills per growth stage (0: off).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_kill_growth(sim: *mut PikSim, kills: u32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_kill_growth(kills)) }
}

/// [`Sim::credit_kill`]: creature `creature` (the host's handle) died. Returns how many
/// Pikmin grew.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_credit_kill(sim: *mut PikSim, creature: u64) -> u32 {
    // SAFETY: as documented.
    unsafe { with(sim, 0, |s| s.sim.credit_kill(creature)) }
}

/// [`Sim::attackers_by_colour`]: how many living Pikmin of each colour (blue, red, yellow, written to
/// `out[0..5]`, then purple and white) attacked `creature`. Returns their total. Ask before [`pik_sim_credit_kill`].
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `out` is null or has room for 5 values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_attackers(sim: *mut PikSim, creature: u64, out: *mut u32) -> u32 {
    // SAFETY: as documented.
    let by_colour = unsafe { with(sim, [0; 5], |s| s.sim.attackers_by_colour(creature)) };
    if !out.is_null() {
        // SAFETY: the caller promised room for 5.
        unsafe { std::ptr::copy_nonoverlapping(by_colour.as_ptr(), out, 5) };
    }
    by_colour.iter().sum()
}

/// [`Sim::set_dismiss_strike`]: the dismiss button strikes (0 or less for `damage_scale`: dismisses).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_dismiss_strike(sim: *mut PikSim, damage_scale: f32, reach: f32, cooldown: f32, charge: f32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_dismiss_strike(damage_scale, reach, cooldown, charge)) }
}

/// [`Sim::set_punch_damage`]: the punch's damage (and so the strike's).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_punch_damage(sim: *mut PikSim, damage: f32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_punch_damage(damage)) }
}

/// [`Sim::set_cursor_homing`]: throws without a lock-on are steered onto the cursor point.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_cursor_homing(sim: *mut PikSim, on: u8) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_cursor_homing(on != 0)) }
}

/// [`Sim::set_squad_pace`]: Pikmin speed by stage as a multiple of their captain's top speed.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_squad_pace(sim: *mut PikSim, leaf: f32, bud: f32, flower: f32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_squad_pace(leaf, bud, flower)) }
}

/// Cooldown items: [`Sim::set_growth_rate`] and [`Sim::set_rejoin_delay`].
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_growth(sim: *mut PikSim, growth_rate: f32, rejoin_delay: f32) {
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            s.sim.set_growth_rate(growth_rate);
            s.sim.set_rejoin_delay(rejoin_delay);
        })
    }
}

/// [`Sim::give_onion_seeds`]: `seeds` more of `color` for the Onion to send out (Backup
/// Magazine's extra sprouts per delivery).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_give_onion_seeds(sim: *mut PikSim, onion: u32, color: u32, seeds: u32) {
    let Some(color) = color_arg(color) else { return };
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            if onion_ok(&s.sim, onion) {
                s.sim.give_onion_seeds(OnionId(onion), color, seeds.min(i32::MAX as u32));
            }
        })
    }
}

/// [`Sim::set_auto_carry_range`], in sim units (0 off).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_auto_carry(sim: *mut PikSim, range: f32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_auto_carry_range(range)) }
}

/// [`Sim::set_auto_pluck`]: seconds, or below 0 for off.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_auto_pluck(sim: *mut PikSim, seconds: f32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_auto_pluck((seconds >= 0.0).then_some(seconds))) }
}

/// Trigger the landed Onion's plucker for ready sprouts in reach.
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_trigger_plucker(sim: *mut PikSim, onion: u32) -> u8 {
    unsafe { with(sim, 0, |s| s.sim.trigger_plucker(OnionId(onion)) as u8) }
}

/// Recall a captain's carriers from pellets whose team is below minimum.
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_recall_stuck_carriers(sim: *mut PikSim, captain: u32) -> u32 {
    unsafe { with(sim, 0, |s| s.sim.recall_stuck_carriers(CaptainId(captain)) as u32) }
}

/// [`Sim::set_emergency_recovery`] (`count` 0 off).
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_emergency(sim: *mut PikSim, seconds: f32, count: u32) {
    // SAFETY: as documented.
    unsafe { with(sim, (), |s| s.sim.set_emergency_recovery(seconds, count)) }
}

/// [`Sim::set_onion_owner`]; `captain` below 0 for none.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_onion_owner(sim: *mut PikSim, onion: u32, captain: i32) {
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            if onion_ok(&s.sim, onion) {
                let owner = (captain >= 0 && captain_ok(&s.sim, captain as u32)).then_some(CaptainId(captain as u32));
                s.sim.set_onion_owner(OnionId(onion), owner);
            }
        })
    }
}

/// [`Sim::heal_squad`]. Returns the health restored, summed.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_heal_squad(sim: *mut PikSim, captain: u32, amount: f32) -> f32 {
    // SAFETY: as documented.
    unsafe { with(sim, 0.0, |s| if captain_ok(&s.sim, captain) { s.sim.heal_squad(CaptainId(captain), amount) } else { 0.0 }) }
}

/// [`Sim::heal_pikis_in_sphere`]. Returns the health restored, summed.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `centre` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_heal_pikis_in_sphere(sim: *mut PikSim, centre: *const PikVec3, radius: f32, amount: f32) -> f32 {
    // SAFETY: as documented.
    let Some(c) = (unsafe { centre.as_ref() }).copied() else {
        return 0.0;
    };
    unsafe { with(sim, 0.0, |s| s.sim.heal_pikis_in_sphere(c.into(), radius, amount)) }
}

/// [`Sim::teleport_squad`].
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_teleport_squad(sim: *mut PikSim, captain: u32, pos: *const PikVec3) {
    // SAFETY: as documented.
    let Some(p) = (unsafe { pos.as_ref() }).copied() else {
        return;
    };
    unsafe { with(sim, (), |s| if captain_ok(&s.sim, captain) { s.sim.teleport_squad(CaptainId(captain), p.into()) }) }
}

/// [`Sim::captain_piki_count`].
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_captain_piki_count(sim: *mut PikSim, captain: u32) -> u32 {
    // SAFETY: as documented.
    unsafe { with(sim, 0, |s| if captain_ok(&s.sim, captain) { s.sim.captain_piki_count(CaptainId(captain)) } else { 0 }) }
}

/// [`Sim::spawn_squad`]: `count` Pikmin straight into his squad around `pos`.
///
/// # Safety
/// `sim` from [`pik_sim_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_spawn_squad(sim: *mut PikSim, captain: u32, pos: *const PikVec3, count: u32) -> u32 {
    // SAFETY: as documented.
    let Some(p) = (unsafe { pos.as_ref() }).copied() else {
        return 0;
    };
    unsafe { with(sim, 0, |s| if captain_ok(&s.sim, captain) { s.sim.spawn_squad(CaptainId(captain), p.into(), count) } else { 0 }) }
}

/// [`Sim::remove_captain_pikis`].
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_remove_captain_pikis(sim: *mut PikSim, captain: u32) -> u32 {
    // SAFETY: as documented.
    unsafe { with(sim, 0, |s| if captain_ok(&s.sim, captain) { s.sim.remove_captain_pikis(CaptainId(captain)) } else { 0 }) }
}

/// [`Sim::down_captain`]: he's out (his squad goes idle), as when his host body dies.
///
/// # Safety
/// `sim` from [`pik_sim_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_down_captain(sim: *mut PikSim, captain: u32, down: u8) {
    // SAFETY: as documented.
    unsafe {
        with(sim, (), |s| {
            if !captain_ok(&s.sim, captain) {
                return;
            }
            let id = CaptainId(captain);
            if down != 0 {
                s.sim.down_captain(id);
            } else if s.sim.captain(id).down {
                let pos = s.sim.captain(id).pos;
                s.sim.revive_captain(id, pos);
            }
        })
    }
}

#[cfg(test)]
mod tests;
