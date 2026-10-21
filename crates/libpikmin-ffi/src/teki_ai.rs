//! Pikmin 1's enemies with their own AI (`libpikmin-teki`) for a host whose bodies carry them: the host
//! spawns an enemy for a body of its own, feeds in the damage the body takes and where it really is
//! each tick, and applies what the enemy does (`PikTekiOut`) and where it wants to be (`PikTekiState`).
//! Draw it with `pik_view_set_tekis`, from the state's motion and frame.

use std::ffi::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use glam::Vec3;
use libpikmin::host::Host;
use libpikmin_assets::Assets;
use libpikmin_teki::{Navi, Out, Target, Tekis, opt};

use crate::{PikSim, PikVec3, path_arg};

/// The enemies of one game.
pub struct PikTekis {
    tekis: Tekis,
    outs: Vec<Out>,
}

/// A player the enemies treat as a Navi.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikNavi {
    pub id: u32,
    pub pos: PikVec3,
    /// Body radius, sim units.
    pub size: f32,
    pub alive: u8,
    pub _pad: [u8; 3],
}

/// An enemy as the host moves and draws it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikTekiState {
    pub id: u32,
    pub kind: u32,
    pub pos: PikVec3,
    pub yaw: f32,
    /// The `.key` motion and frame it is in, for `pik_view_set_tekis`.
    pub motion: u32,
    pub frame: f32,
    /// Seconds of hit flash and of hit-reaction shake left.
    pub flash: f32,
    pub shake: f32,
    /// 0..1 how solid it is (the Honeywisp fades at the ends of its trips).
    pub fade: f32,
    pub state: i32,
    /// Bit 0 visible, 1 alive, 2 flying, 3 collidable, 4 holding a pellet.
    pub flags: u32,
    pub held_pellet: i32,
    pub hp: f32,
}

/// What an enemy did. `kind`: 1 effect (`a` the `EFF_*` id at `pos`), 2 named sound (`text`), 3 spawn (`a` the
/// kind at `pos`, facing `x`, `vel` its velocity), 4 hit a player (`a` the Navi id, `x` damage, `y` knock-back
/// power, `vel` where from), 5 finished (gone), 6 began dying, 7 shake (`x` strength), 8 pellets dropped
/// (`a` number, `b` count), 9 nectar (`b` count).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikTekiOut {
    pub kind: u32,
    pub teki: u32,
    pub a: u32,
    pub pos: PikVec3,
    pub x: f32,
    pub y: f32,
    pub vel: PikVec3,
    pub b: u32,
    pub text: [u8; 24],
}

/// No ground, no walls: the host's bodies decide where the enemies really are.
struct NoHost;

impl Host for NoHost {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        None
    }
}

/// Loads every enemy kind that has files and a ported strategy. Null if the assets are unreadable.
///
/// # Safety
/// `assets_dir` a nul-terminated UTF-8 path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_new(assets_dir: *const c_char, seed: u64) -> *mut PikTekis {
    // SAFETY: as documented.
    let Some(dir) = (unsafe { path_arg(assets_dir) }) else { return std::ptr::null_mut() };
    let made = catch_unwind(|| {
        let mut tekis = Tekis::load(&Assets { root: dir });
        tekis.seed(seed);
        PikTekis { tekis, outs: Vec::new() }
    });
    match made {
        Ok(t) => Box::into_raw(Box::new(t)),
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `t` from [`pik_tekis_new`], not used afterwards. Null is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_free(t: *mut PikTekis) {
    if !t.is_null() {
        // SAFETY: the caller hands back ownership.
        drop(unsafe { Box::from_raw(t) });
    }
}

/// `out[0]`, `out[1]`, `out[2]` the sizes of [`PikNavi`], [`PikTekiState`] and [`PikTekiOut`].
///
/// # Safety
/// `out` writable for three `u32`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_sizes(out: *mut u32) {
    if out.is_null() {
        return;
    }
    // SAFETY: as documented.
    unsafe {
        out.write(std::mem::size_of::<PikNavi>() as u32);
        out.add(1).write(std::mem::size_of::<PikTekiState>() as u32);
        out.add(2).write(std::mem::size_of::<PikTekiOut>() as u32);
    }
}

/// Whether kind `kind` has files and a ported strategy.
///
/// # Safety
/// `t` from [`pik_tekis_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_has_strategy(t: *const PikTekis, kind: u32) -> u8 {
    // SAFETY: as documented.
    unsafe { t.as_ref() }.is_some_and(|t| u8::try_from(kind).is_ok_and(|k| t.tekis.has_strategy(k))) as u8
}

/// A new enemy of `kind` for the host's body `creature` (the id the sim's creatures use). Its id, or -1.
///
/// # Safety
/// `t` from [`pik_tekis_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_spawn(t: *mut PikTekis, kind: u32, creature: u64, pos: *const PikVec3, yaw: f32) -> i32 {
    // SAFETY: as documented.
    let (Some(t), Some(pos)) = (unsafe { t.as_mut() }, unsafe { pos.as_ref() }) else { return -1 };
    let Ok(kind) = u8::try_from(kind) else { return -1 };
    t.tekis.spawn(kind, creature, Vec3::from(*pos), yaw).map_or(-1, |id| id as i32)
}

/// # Safety
/// `t` from [`pik_tekis_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_remove(t: *mut PikTekis, id: u32) {
    // SAFETY: as documented.
    if let Some(t) = unsafe { t.as_mut() } {
        t.tekis.remove(id);
    }
}

/// A hit, in the disc's life units. `attacker_navi` the player who did it, or -1.
///
/// # Safety
/// `t` from [`pik_tekis_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_damage(t: *mut PikTekis, id: u32, amount: f32, attacker_navi: i32) {
    // SAFETY: as documented.
    if let Some(t) = unsafe { t.as_mut() } {
        let who = u32::try_from(attacker_navi).ok().map(Target::Navi);
        t.tekis.damage(id, amount, who);
    }
}

/// The host's body is here (it was blocked or pushed): the enemy follows.
///
/// # Safety
/// `t` from [`pik_tekis_new`]; `pos` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_set_position(t: *mut PikTekis, id: u32, pos: *const PikVec3) {
    // SAFETY: as documented.
    if let (Some(t), Some(pos)) = (unsafe { t.as_mut() }, unsafe { pos.as_ref() }) {
        t.tekis.set_position(id, Vec3::from(*pos));
    }
}

/// Where an enemy that roams goes (a Honeywisp's far point) and how far.
///
/// # Safety
/// `t` from [`pik_tekis_new`]; `target` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_set_territory(t: *mut PikTekis, id: u32, territory: f32, target: *const PikVec3) {
    // SAFETY: as documented.
    if let (Some(t), Some(target)) = (unsafe { t.as_mut() }, unsafe { target.as_ref() })
        && let Some(e) = t.tekis.get_mut(id)
    {
        e.territory = territory;
        e.target_pos = Vec3::from(*target);
    }
}

/// Squashed by something.
///
/// # Safety
/// `t` from [`pik_tekis_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_press(t: *mut PikTekis, id: u32) {
    // SAFETY: as documented.
    if let Some(t) = unsafe { t.as_mut() } {
        t.tekis.press(id);
    }
}

/// Runs `ticks` 30 Hz ticks of every enemy against `sim` and the players. Returns how many things they did
/// ([`pik_tekis_outs`]).
///
/// # Safety
/// `t` from [`pik_tekis_new`]; `sim` from `pik_sim_new`; `navis` readable for `count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_update(t: *mut PikTekis, sim: *mut PikSim, navis: *const PikNavi, count: u32, ticks: u32) -> u32 {
    // SAFETY: as documented.
    let (Some(t), Some(s)) = (unsafe { t.as_mut() }, unsafe { sim.as_mut() }) else { return 0 };
    if s.poisoned {
        return 0;
    }
    // SAFETY: as documented.
    let list: Vec<Navi> = if navis.is_null() || count == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(navis, count as usize) }
            .iter()
            .map(|n| Navi { id: n.id, pos: Vec3::from(n.pos), size: n.size, alive: n.alive != 0 })
            .collect()
    };
    let done = catch_unwind(AssertUnwindSafe(|| {
        t.outs.clear();
        for _ in 0..ticks.min(8) {
            let out = t.tekis.update(&mut s.sim, &NoHost, &list, 1.0 / 30.0);
            t.outs.extend(out);
        }
    }));
    if done.is_err() {
        return 0;
    }
    t.outs.len() as u32
}

/// What the last update's enemies did, up to `max`.
///
/// # Safety
/// `t` from [`pik_tekis_new`]; `out` writable for `max`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_outs(t: *const PikTekis, out: *mut PikTekiOut, max: u32) -> u32 {
    // SAFETY: as documented.
    let Some(t) = (unsafe { t.as_ref() }) else { return 0 };
    if out.is_null() {
        return t.outs.len() as u32;
    }
    let n = t.outs.len().min(max as usize);
    for (i, o) in t.outs.iter().take(n).enumerate() {
        let mut r = PikTekiOut::default();
        match o {
            Out::Effect { id, pos } => (r.kind, r.a, r.pos) = (1, *id as u32, (*pos).into()),
            Out::Sound { name, pos } => {
                r.kind = 2;
                r.pos = (*pos).into();
                let b = name.as_bytes();
                let n = b.len().min(r.text.len() - 1);
                r.text[..n].copy_from_slice(&b[..n]);
            }
            Out::Spawn { kind, pos, yaw, parent, velocity } => {
                (r.kind, r.a, r.pos, r.x, r.teki, r.vel) = (3, *kind as u32, (*pos).into(), *yaw, *parent, (*velocity).into());
            }
            Out::HitNavi { teki, navi, damage, from, knock } => {
                (r.kind, r.teki, r.a, r.x, r.y, r.vel) = (4, *teki, *navi, *damage, *knock, (*from).into());
            }
            Out::Died { teki } => (r.kind, r.teki) = (5, *teki),
            Out::Dying { teki } => (r.kind, r.teki) = (6, *teki),
            Out::Shake { pos, strength } => (r.kind, r.pos, r.x) = (7, (*pos).into(), *strength),
            Out::SpawnPellets { pos, number, color, count } => (r.kind, r.pos, r.a, r.y, r.b) = (8, (*pos).into(), *number, *color as f32, *count),
            Out::SpawnNectar { pos, count } => (r.kind, r.pos, r.b) = (9, (*pos).into(), *count),
            Out::TableSound { .. } | Out::Message { .. } => continue,
        }
        // SAFETY: `i < max`.
        unsafe { out.add(i).write(r) };
    }
    n as u32
}

/// An enemy's state; 0 if there is none.
///
/// # Safety
/// `t` from [`pik_tekis_new`]; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_tekis_state(t: *const PikTekis, id: u32, out: *mut PikTekiState) -> u8 {
    // SAFETY: as documented.
    let (Some(t), false) = (unsafe { t.as_ref() }, out.is_null()) else { return 0 };
    let Some(e) = t.tekis.get(id) else { return 0 };
    let mut flags = 0;
    flags |= e.option(opt::VISIBLE) as u32;
    flags |= (e.alive() as u32) << 1;
    flags |= (e.flying as u32) << 2;
    flags |= (e.option(opt::ATARI) as u32) << 3;
    flags |= (e.held.is_some() as u32) << 4;
    // SAFETY: as documented.
    unsafe {
        out.write(PikTekiState {
            id,
            kind: e.kind as u32,
            pos: e.pos.into(),
            yaw: e.yaw,
            motion: e.anim.motion.max(0) as u32,
            frame: e.anim.counter,
            flash: e.flash,
            shake: e.shake,
            fade: e.fade,
            state: e.state,
            flags,
            held_pellet: e.held.map_or(-1, |p| p.0 as i32),
            hp: e.hp,
        })
    };
    1
}
