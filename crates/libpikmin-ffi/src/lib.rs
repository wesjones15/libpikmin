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

use std::collections::VecDeque;
use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use libpikmin::carry::PelletState;
use libpikmin::onion::{OnionAnim, SproutState};
use libpikmin::{
    CaptainId, CaptainInput, CaptainPose, CreatureFrame, CreatureId, CreaturePart, DamageSource,
    DeathCause, Event, Happa, Host, OnionId, Params, PelletDesc, PikiColor, PikiId, PikiState, Sim, Vec2,
    Vec3,
};
use libpikmin::host::CreatureView;

mod view;
pub use view::*;

/// Bumped whenever a function or struct here changes.
pub const PIK_ABI_VERSION: u32 = 12;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_recruit_to_pellet(sim: *mut PikSim, pellet: u32, defeated: u64, range: f32) -> u32 {
    unsafe { with(sim, 0, |s| s.sim.recruit_to_pellet(libpikmin::carry::PelletId(pellet), Some(defeated), range) as u32) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_lock_target(sim: *mut PikSim, captain: u32, target: u64, enabled: u8) {
    unsafe { with(sim, (), |s| s.sim.set_lock_target(CaptainId(captain), (enabled != 0).then_some(target))) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_camera_preset(zoom: u32, top: u8, out: *mut PikVec3) {
    if let Some(out) = unsafe { out.as_mut() } { *out = libpikmin::camera::preset(zoom, top != 0).into(); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_camera_pan(mode: u32, angles: *mut PikVec3, x: f32, y: f32, dt: f32) {
    if let Some(a) = unsafe { angles.as_mut() } {
        let mode = if mode == 1 { libpikmin::camera::CameraMode::Orbit } else { libpikmin::camera::CameraMode::Vanilla };
        libpikmin::camera::pan(mode, &mut a.x, &mut a.y, Vec2::new(x, y), dt);
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
    pub _pad: [u8; 2],
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
    pub _pad: [u8; 6],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikOnion {
    pub pos: PikVec3,
    pub id: u32,
    /// Bit `PikiColor::index` set for each colour it takes.
    pub colors: u32,
    /// Pikmin inside, by colour index (all leaf stages).
    pub stored: [u32; 3],
    /// 0 idle, 1 suction, 2 emitting seeds, 3 taking off, 4 away, 5 landing.
    pub anim: u8,
    pub is_ship: u8,
    pub _pad: [u8; 2],
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
    /// The creatures the host gave last, copied into the sim each tick.
    frame: CreatureFrame,
    /// Events for [`pik_sim_events`].
    pending: VecDeque<Event>,
    /// Events for [`pik_view_update`] (animations, sounds).
    view_log: Vec<Event>,
    params_source: CString,
    poisoned: bool,
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
        let source = match dir {
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
        (Sim::new(params, seed), source)
    });
    match made {
        Ok((sim, source)) => Box::into_raw(Box::new(PikSim {
            sim,
            frame: CreatureFrame::default(),
            pending: VecDeque::new(),
            view_log: Vec::new(),
            params_source: CString::new(source).unwrap_or_default(),
            poisoned: false,
        })),
        Err(_) => std::ptr::null_mut(),
    }
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
    let colors: Vec<PikiColor> = (0..3).filter(|i| colors & (1 << i) != 0).map(PikiColor::from_index).collect();
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

/// Updates a host-controlled Onion's pose and airborne state.
/// # Safety
/// `sim` from [`pik_sim_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_sim_set_onion_pose(sim: *mut PikSim, onion: u32, pos: *const PikVec3, flying: u8) {
    let Some(pos) = (unsafe { pos.as_ref() }).copied() else { return };
    unsafe { with(sim, (), |s| s.sim.set_onion_pose(OnionId(onion), pos.into(), flying != 0)) }
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
            let mut h = FfiHost { host, frame: &s.frame, path: Vec::new() };
            let ticks = s.sim.advance(&mut h, dt.clamp(0.0, 0.25));
            let events: Vec<Event> = s.sim.drain_events().collect();
            // Nobody reading either queue mustn't grow it forever: keep the newest.
            if s.view_log.len() + events.len() <= EVENT_BACKLOG {
                s.view_log.extend_from_slice(&events);
            }
            s.pending.extend(events);
            while s.pending.len() > EVENT_BACKLOG {
                s.pending.pop_front();
            }
            ticks
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
                    captain: p.captain.map_or(-1, |c| c.0 as i32),
                    pellet: pellet.map_or(-1, |id| id as i32),
                    health: p.health,
                    state: state_code(&p.state),
                    color: p.color.index() as u8,
                    happa: happa_code(p.happa),
                    alive: p.is_alive() as u8,
                    has_creature: creature.is_some() as u8,
                    on_ground: p.on_ground as u8,
                    _pad: [0; 2],
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
                    _pad: [0; 6],
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
                    anim: match o.anim {
                        OnionAnim::Idle => 0,
                        OnionAnim::Hover => 6,
                        OnionAnim::Suction { .. } => 1,
                        OnionAnim::Emitting { .. } => 2,
                        OnionAnim::TakeOff { .. } => 3,
                        OnionAnim::Away => 4,
                        OnionAnim::Landing { .. } => 5,
                    },
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
    (i < 3).then(|| PikiColor::from_index(i as usize))
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
        Event::PikiStomped { piki, captain, pos } => {
            (v.kind, v.a, v.b, v.pos) = (PIKI_STOMPED, piki.0, captain.0 as i32, pos.into())
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
