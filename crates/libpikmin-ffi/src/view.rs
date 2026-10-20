//! Drawing and sound for hosts with their own engine: `libpikmin-present`'s [`View`] (the
//! disc's models, posed and skinned every frame) and [`Audio`] (the game's sounds for the
//! sim's events) behind a C ABI.
//!
//! Models are loaded once: a host builds a mesh per model from [`pik_view_model`],
//! [`pik_view_batches`] (one submesh each), [`pik_view_indices`] and [`pik_view_uvs`]. Each
//! frame [`pik_view_update`] poses the sim; [`pik_view_draws`] lists the instances, each with its
//! vertices in [`pik_view_positions`] / [`pik_view_normals`] (in the instance's local frame, sim
//! units) and per batch a colour in [`pik_view_colors`] and a texture in
//! [`pik_view_texture_ids`]. Textures are each material's TEV baked into one RGBA image
//! ([`pik_view_texture`]); new ones (a pellet's colour) can appear in any frame. Particles (the
//! whistle, dead Pikmin's ghosts) come as camera-facing quads in [`pik_view_particles`], each
//! with a texture from [`pik_view_particle_texture`].
//!
//! No music plays: the host keeps its own soundtrack.

use std::ffi::{CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

use glam::{Mat4, Vec4};
use libpikmin::Vec3;
use libpikmin::captain::WhistleState;
use libpikmin_assets::{Assets, Cull};
use libpikmin_present::audio::Creatures;
use libpikmin_present::{Audio, View};

use crate::{PikSim, PikVec3, path_arg};

/// One model's sizes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikModelInfo {
    pub vertex_count: u32,
    pub index_count: u32,
    pub batch_count: u32,
    /// Textures baked so far, every model's ([`pik_view_texture`] ids are below this).
    pub texture_count: u32,
    /// Bind-pose bounds.
    pub lo: PikVec3,
    pub hi: PikVec3,
}

/// One batch (a submesh with one material).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikBatch {
    pub first_index: u32,
    pub index_count: u32,
    pub first_vertex: u32,
    pub vertex_count: u32,
    /// Its texture with no per-draw colours ([`pik_view_texture`]), or -1. A draw's own is
    /// in [`pik_view_texture_ids`].
    pub texture: i32,
    /// 0 opaque, 1 alpha tested (cutout), 2 blended and unlit, 3 added by its alpha, unlit,
    /// no depth write (the Onions' light cones).
    pub blend: u8,
    /// Lit in the game: 1 its colour, 2 its alpha (opaque only where light falls; ambient
    /// alpha is 0). 0: unlit.
    pub lit: u8,
    /// 0 back, 1 front, 2 none (GX faces are front when clockwise on screen).
    pub cull: u8,
    /// 1: environment mapped: its texture coordinates change every frame
    /// ([`pik_view_frame_uvs`]).
    pub env: u8,
}

/// A texture's size and wrap modes (0 repeat, 1 clamp, 2 mirror).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikTexture {
    pub width: u32,
    pub height: u32,
    pub wrap_s: u8,
    pub wrap_t: u8,
    pub _pad: [u8; 2],
}

/// One posed instance: place an object of model `model` at `pos` rotated by `rot` (x, y, z,
/// w) and give it the vertices and colours at the offsets.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikDraw {
    /// Stable while the object lives (kind in the top byte).
    pub key: u64,
    pub model: u32,
    /// `libpikmin_present::view::kinds`: 0 captain, 1 Pikmin, 2 its leaf, 3 Onion, 4 pellet,
    /// 5 sprout, 6 its leaf, 7 cursor, 8 cursor marker, 9 ship, 10 an Onion's light cone.
    pub kind: u8,
    /// 1: flashing from a hit.
    pub flags: u8,
    pub _pad: [u8; 2],
    pub pos: PikVec3,
    pub rot: [f32; 4],
    pub first_vertex: u32,
    pub first_color: u32,
    /// 1 opaque .. 0 gone.
    pub alpha: f32,
    /// Which captain, Pikmin, Onion... (the low bits of `key`).
    pub id: u32,
}

/// One particle: a camera-facing quad (corners top-left, top-right, bottom-right, bottom-left,
/// texture (0,0) (1,0) (1,1) (0,1)), drawn as texture x `color`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikParticle {
    pub corners: [PikVec3; 4],
    /// RGBA 0..1. Additive particles have their alpha folded in already.
    pub color: [f32; 4],
    /// A [`pik_view_particle_texture`] id.
    pub texture: u32,
    /// 1: added to the screen (`colour * texture`, no depth writes); 0: alpha blended.
    pub additive: u8,
    pub _pad: [u8; 3],
}

/// The frame's camera and listening for [`pik_view_update`].
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikViewFrame {
    /// Seconds since the last update (0 while paused).
    pub dt: f32,
    /// Camera position and facing, sim units and axes.
    pub eye: PikVec3,
    pub forward: PikVec3,
    pub up: PikVec3,
    /// The captain whose whistle and own sounds are heard, or -1.
    pub listener: i32,
    /// Sound volume 0..1.
    pub volume: f32,
    /// Ticks behind the newest sim state to draw, 0..1. Hosts stepping in FixedUpdate
    /// pass `StepSmoother.LagTicks(render_alpha, 1)`; zero draws the latest state.
    pub lag: f32,
}

/// An opaque view handle.
pub struct PikView {
    view: View,
    audio: Option<Audio>,
    status: CString,
    particles: Vec<PikParticle>,
    last_tick: Option<u64>,
    poisoned: bool,
}

/// Loads the disc's models (and with `sound` non-zero, opens the default output device for
/// the game's sounds) from `assets_dir`. Null if the models can't be loaded. Free with
/// [`pik_view_free`].
///
/// # Safety
/// `assets_dir` a nul-terminated UTF-8 path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_new(assets_dir: *const c_char, sound: u8) -> *mut PikView {
    // SAFETY: as documented.
    let Some(dir) = (unsafe { path_arg(assets_dir) }) else {
        return std::ptr::null_mut();
    };
    let made = catch_unwind(|| {
        let assets = Assets { root: dir };
        let view = View::load(&assets).ok()?;
        let (audio, sound_status) = if sound != 0 {
            match Audio::start(&assets) {
                Ok(a) => {
                    let s = format!("sound {} Hz, {} channels", a.rate, a.channels);
                    (Some(a), s)
                }
                Err(e) => (None, format!("no sound ({e})")),
            }
        } else {
            (None, "sound off".to_string())
        };
        let status = format!("{} models; {sound_status}", view.models.len());
        Some(PikView {
            view,
            audio,
            status: CString::new(status).unwrap_or_default(),
            particles: Vec::new(),
            last_tick: None,
            poisoned: false,
        })
    });
    match made {
        Ok(Some(v)) => Box::into_raw(Box::new(v)),
        _ => std::ptr::null_mut(),
    }
}

/// # Safety
/// `view` from [`pik_view_new`] and not used afterwards. Null is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_free(view: *mut PikView) {
    if !view.is_null() {
        // SAFETY: the caller hands back ownership.
        let v = unsafe { Box::from_raw(view) };
        // Stopping the stream joins its thread; never let a panic out.
        let _ = catch_unwind(AssertUnwindSafe(move || drop(v)));
    }
}

/// What loaded ("15 models; sound 48000 Hz, 2 channels" or why there's no sound). Valid until
/// the view is freed.
///
/// # Safety
/// `view` from [`pik_view_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_status(view: *const PikView) -> *const c_char {
    // SAFETY: as documented.
    match unsafe { view.as_ref() } {
        Some(v) => v.status.as_ptr(),
        None => std::ptr::null(),
    }
}

/// Whether to draw each captain's cursor and marker (`show` nonzero; the default). Off for a
/// captain shown on his own, like on a character select screen.
///
/// # Safety
/// `view` from [`pik_view_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_show_cursors(view: *mut PikView, show: u8) {
    // SAFETY: as documented.
    unsafe { with_view(view, (), |v| v.view.show_cursors = show != 0) }
}

/// Host UI and Onion-healing sounds. 0 open, 1 count changed, 2 count limit,
/// 3 close, 4 Olimar healed, 5 one Pikmin ghost, 6/7 recall whistle start/stop.
/// `pos` is in sim units.
/// # Safety
/// `view` is a live view handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_play_sound(view: *mut PikView, sound: u8, pos: PikVec3) {
    let name = match sound {
        0 => "SYSSE_CMENU_ON",
        1 => "SYSSE_CMENU_SELECT",
        2 => "SYSSE_CMENU_ERROR",
        3 => "SYSSE_CMENU_OFF",
        4 => "SE_PLAYER_TOUCHHONEY",
        5 => "SE_PIKI_DEAD",
        6 => "SE_GATHER",
        7 => "SE_GATHER",
        _ => return,
    };
    // Menu cues in drawContainer.cpp; honey heals Olimar in itemAI.cpp; ghost
    // calls follow Piki::doKill in pikidoKill.cpp.
    unsafe { with_view(view, (), |v| {
        if let Some(audio) = &v.audio {
            if sound == 7 { audio.stop(name); }
            else { audio.play_at(name, pos.into()); }
        }
    }) }
}

/// Holds this view's sounds still while the host's game is paused (nonzero) and lets them go on
/// from the same place when it isn't. Call it every frame; a view without sound ignores it.
/// # Safety
/// `view` is a live view handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_set_paused(view: *mut PikView, paused: u8) {
    if let Some(v) = unsafe { view.as_mut() } {
        if let Some(audio) = &v.audio { audio.set_paused(paused != 0); }
    }
}

/// Budget distant Pikmin skinning (sim units); zero disables it. Requires draw flag 4 support.
/// # Safety
/// `view` is a live view handle, `eye` is readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_set_pose_budget(view: *mut PikView, eye: *const PikVec3, far: f32) {
    if let (Some(v), Some(eye)) = (unsafe { view.as_mut() }, unsafe { eye.as_ref() }) {
        v.view.pose_budget = (far.is_finite() && far > 0.0).then_some((Vec3::from(*eye), far));
    }
}

/// Have views made after this draw the Dolphin's parts as the sim fits them (`animator`'s sibling,
/// `view::set_repaired_ship`). Call before [`pik_view_new`]: the parts' models are loaded then.
#[unsafe(no_mangle)]
pub extern "C" fn pik_view_set_repaired_ship(on: u8) {
    libpikmin_present::view::set_repaired_ship(on != 0);
}

/// Victory posing is host-owned and leaves the simulation unchanged.
/// # Safety
/// `view` is a live view handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_set_celebrating(view: *mut PikView, on: u8) {
    if let Some(v) = unsafe { view.as_mut() } { v.view.celebrating = on != 0; }
}

/// # Safety
/// `view` is a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_set_ship_repair_mode(view: *mut PikView, on: u8) {
    if let Some(v) = unsafe { view.as_mut() } { v.view.repair_mode = on != 0; }
}

/// The host's terrain height below an Onion, in simulation units.
/// # Safety
/// `view` is a live view handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_set_onion_ground(view: *mut PikView, onion: u32, height: f32) {
    if let Some(v) = unsafe { view.as_mut() } {
        v.view.onion_ground.insert(onion, height);
    }
}

/// # Safety
/// `view` is a live view handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_repairs_pending(view: *const PikView) -> u32 {
    unsafe { view.as_ref() }.map_or(0, |v| v.view.pending_repairs() as u32)
}

/// The complete-ending farewell music: pikidemo.c DEMOID_GoodEndingWave's
/// stream 11, piki_scene.c filelist[11] = compend0.stx. Assets remain local.
/// Returns required/written stereo float samples, zero if unavailable.
/// # Safety
/// `assets_dir` is NUL terminated; `rate` writable; non-null `out` writable for `cap` floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_victory_pcm(assets_dir: *const c_char, out: *mut f32, cap: u32, rate: *mut u32) -> u32 {
    let result = std::panic::catch_unwind(|| {
        let dir = unsafe { crate::path_arg(assets_dir) }?;
        let assets = libpikmin_assets::Assets::discover(Some(&dir))?;
        let data = assets.read("SndData/compend0.stx").ok()?;
        let header = libpikmin_assets::sound::stream::StreamHeader::parse(&data).ok()?;
        let hz = header.sample_rate as u32;
        if !(8000..=96000).contains(&hz) || header.samples == 0 || header.samples > hz * 120 { return None; }
        let decoded = libpikmin_assets::sound::Stream::decode(&data).ok()?;
        Some((hz, decoded.stereo))
    });
    let Ok(Some((hz, samples))) = result else { return 0 };
    if let Some(rate) = unsafe { rate.as_mut() } { *rate = hz; } else { return 0; }
    if out.is_null() { return samples.len() as u32; }
    if cap < samples.len() as u32 { return 0; }
    let output = unsafe { std::slice::from_raw_parts_mut(out, samples.len()) };
    for (i, (&sample, value)) in samples.iter().zip(output).enumerate() {
        let fade = ((samples.len() - i) as f32 / (hz as f32 * 0.4)).min(1.0);
        *value = sample as f32 / 32768.0 * fade;
    }
    samples.len() as u32
}

/// Captain `captain` appears in a beam: `progress` 0 (a thin column) to 1 (himself, which ends it).
///
/// # Safety
/// `view` from [`pik_view_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_set_materialize(view: *mut PikView, captain: u32, progress: f32) {
    // SAFETY: as documented.
    unsafe { with_view(view, (), |v| v.view.set_materialize(captain, progress)) }
}

/// Dolphin healing particles and one ghost per sacrificed Pikmin at a captain.
/// # Safety
/// `view` from `pik_view_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_set_healing(view: *mut PikView, captain: u32, progress: f32, ghosts: u32) {
    unsafe { with_view(view, (), |v| v.view.set_healing(captain, progress, ghosts)) }
}

/// Olimar jumps with the Pikmin's own jump motion (`animator::set_captain_pikmin_jump`).
#[unsafe(no_mangle)]
pub extern "C" fn pik_view_set_captain_pikmin_jump(on: u8) {
    libpikmin_present::animator::set_captain_pikmin_jump(on != 0);
}

/// How loud the swarming trumpet plays, times the disc's level (default 1; 0 to 4).
///
/// # Safety
/// `view` from [`pik_view_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_set_trumpet_gain(view: *mut PikView, gain: f32) {
    // SAFETY: as documented.
    unsafe {
        with_view(view, (), |v| {
            if let Some(a) = v.audio.as_ref() {
                a.set_formation_gain(gain);
            }
        })
    }
}

/// Transfer death-bell playback to the host. Writes 640000 stereo f32 samples at 32000 Hz
/// and suppresses the view's duplicate CaptainDown sound. Returns 0 on failure/short buffer.
/// The PCM comes from the user's disc; it must not be distributed with the mod.
///
/// # Safety
/// `view` from [`pik_view_new`]; `out` writable for `max` f32 samples.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_take_death_sting(view: *mut PikView, out: *mut f32, max: u32) -> u32 {
    if out.is_null() || max < 640000 { return 0; }
    unsafe {
        with_view(view, 0, |v| {
            let Some(pcm) = v.audio.as_mut().and_then(Audio::take_death_sting) else { return 0; };
            std::ptr::copy_nonoverlapping(pcm.as_ptr(), out, pcm.len());
            pcm.len() as u32
        })
    }
}

/// Runs `f` on a live view, poisoning it if `f` panics.
///
/// # Safety
/// `view` null or from [`pik_view_new`], not aliased for the call.
unsafe fn with_view<R>(view: *mut PikView, fallback: R, f: impl FnOnce(&mut PikView) -> R) -> R {
    // SAFETY: as documented.
    let Some(v) = (unsafe { view.as_mut() }) else {
        return fallback;
    };
    if v.poisoned {
        return fallback;
    }
    match catch_unwind(AssertUnwindSafe(|| f(v))) {
        Ok(r) => r,
        Err(_) => {
            v.poisoned = true;
            fallback
        }
    }
}

/// Number of models (`libpikmin_present::view::models`: 0 Olimar, 1-3 Blue/Red/Yellow
/// Pikmin, 4-6 leaf/bud/flower, 7 Onion, 8-11 pellets 1/5/10/20, 12 sprout, 13 cursor,
/// 14 cursor marker).
///
/// # Safety
/// `view` from [`pik_view_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_model_count(view: *mut PikView) -> u32 {
    // SAFETY: as documented.
    unsafe { with_view(view, 0, |v| v.view.models.len() as u32) }
}

/// # Safety
/// `view` from [`pik_view_new`]; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_model(view: *mut PikView, model: u32, out: *mut PikModelInfo) -> u8 {
    // SAFETY: as documented.
    unsafe {
        with_view(view, 0, |v| {
            let (Some(m), Some(out)) = (v.view.models.get(model as usize), out.as_mut()) else {
                return 0;
            };
            *out = PikModelInfo {
                vertex_count: m.vertex_count as u32,
                index_count: m.batches.iter().map(|b| b.indices.len() as u32).sum(),
                batch_count: m.batches.len() as u32,
                texture_count: v.view.textures.list.len() as u32,
                lo: m.lo.into(),
                hi: m.hi.into(),
            };
            1
        })
    }
}

/// Writes up to `max` of the model's batches; returns how many.
///
/// # Safety
/// `view` from [`pik_view_new`]; `out` writable for `max`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_batches(view: *mut PikView, model: u32, out: *mut PikBatch, max: u32) -> u32 {
    // SAFETY: as documented.
    unsafe {
        with_view(view, 0, |v| {
            let Some(m) = v.view.models.get(model as usize) else { return 0 };
            let (mut index, mut vertex) = (0u32, 0u32);
            let mut n = 0;
            for (i, b) in m.batches.iter().enumerate() {
                if n >= max as usize || out.is_null() {
                    break;
                }
                let cull = match b.cull {
                    Cull::Back => 0,
                    Cull::Front => 1,
                    Cull::None => 2,
                };
                out.add(n).write(PikBatch {
                    first_index: index,
                    index_count: b.indices.len() as u32,
                    first_vertex: vertex,
                    vertex_count: b.vertices.len() as u32,
                    texture: m.batch_texture[i],
                    blend: m.batch_blend[i],
                    lit: m.batch_light(i),
                    cull,
                    env: m.batch_env.get(i).is_some_and(|e| e.is_some()) as u8,
                });
                index += b.indices.len() as u32;
                vertex += b.vertices.len() as u32;
                n += 1;
            }
            n as u32
        })
    }
}

/// The model's triangle indices, batch after batch, into the model's whole vertex list (so
/// batch `b`'s are `first_index .. first_index + index_count`). Returns how many were written.
///
/// # Safety
/// `view` from [`pik_view_new`]; `out` writable for `max`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_indices(view: *mut PikView, model: u32, out: *mut u32, max: u32) -> u32 {
    // SAFETY: as documented.
    unsafe {
        with_view(view, 0, |v| {
            let Some(m) = v.view.models.get(model as usize) else { return 0 };
            if out.is_null() {
                return 0;
            }
            let (mut n, mut base) = (0usize, 0u32);
            for b in &m.batches {
                for &i in &b.indices {
                    if n >= max as usize {
                        return n as u32;
                    }
                    out.add(n).write(base + i);
                    n += 1;
                }
                base += b.vertices.len() as u32;
            }
            n as u32
        })
    }
}

/// The model's texture coordinates, one per vertex in vertex order. Returns how many.
///
/// # Safety
/// `view` from [`pik_view_new`]; `out` writable for `max` pairs of floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_uvs(view: *mut PikView, model: u32, out: *mut [f32; 2], max: u32) -> u32 {
    // SAFETY: as documented.
    unsafe {
        with_view(view, 0, |v| {
            let Some(m) = v.view.models.get(model as usize) else { return 0 };
            if out.is_null() {
                return 0;
            }
            let mut n = 0usize;
            for vtx in m.batches.iter().flat_map(|b| &b.vertices) {
                if n >= max as usize {
                    break;
                }
                out.add(n).write([vtx.uv.x, vtx.uv.y]);
                n += 1;
            }
            n as u32
        })
    }
}

/// Baked texture `id`: its size and wrap modes in `info`, and if `rgba` isn't null, its pixels
/// (RGBA8, rows top first, `width * height * 4` bytes; `len` is the room). The colour is all
/// there: a host multiplies it by its lighting and the draw's colour (white, with alpha).
/// Returns 0 for an unknown id or too little room.
///
/// # Safety
/// `view` from [`pik_view_new`]; `info` null or writable; `rgba` null or writable for `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_texture(view: *mut PikView, id: u32, info: *mut PikTexture, rgba: *mut u8, len: u32) -> u8 {
    // SAFETY: as documented.
    unsafe {
        with_view(view, 0, |v| {
            let Some(t) = v.view.textures.list.get(id as usize) else { return 0 };
            if let Some(info) = info.as_mut() {
                *info = PikTexture {
                    width: t.width,
                    height: t.height,
                    wrap_s: t.wrap[0],
                    wrap_t: t.wrap[1],
                    _pad: [0; 2],
                };
            }
            if rgba.is_null() {
                return 1;
            }
            if t.rgba.len() > len as usize {
                return 0;
            }
            std::ptr::copy_nonoverlapping(t.rgba.as_ptr(), rgba, t.rgba.len());
            1
        })
    }
}

/// # Safety
/// `view` must be a live view handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_texture_version(view: *mut PikView, id: u32) -> u32 {
    unsafe { with_view(view, 0, |v| v.view.textures.version(id)) }
}

/// World-to-camera for the sound system: camera looking down -Z with +X to its right.
/// Hosts pass Unity-style axes (left-handed: right = up x forward), which is what the sim's
/// numbers are in a host that copies its coordinates straight across.
fn listener_view(eye: Vec3, forward: Vec3, up: Vec3) -> Mat4 {
    let f = forward.normalize_or(Vec3::Z);
    let r = up.cross(f).normalize_or(Vec3::X);
    let u = f.cross(r);
    let rot = Mat4::from_cols(r.extend(0.0), u.extend(0.0), (-f).extend(0.0), Vec4::W).transpose();
    rot * Mat4::from_translation(-eye)
}

/// Poses `sim` for this frame and plays the sounds for its events since the last update.
///
/// # Safety
/// `view` from [`pik_view_new`]; `sim` from `pik_sim_new`; `frame` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_update(view: *mut PikView, sim: *mut PikSim, frame: *const PikViewFrame) {
    // SAFETY: as documented.
    let (Some(s), Some(f)) = (unsafe { sim.as_mut() }, unsafe { frame.as_ref() }) else {
        return;
    };
    if s.poisoned {
        return;
    }
    let f = *f;
    // SAFETY: as documented.
    unsafe {
        with_view(view, (), |v| {
            if let Some(a) = v.audio.as_mut() {
                a.set_listener_captain(usize::try_from(f.listener).ok());
            }
            let mut events = std::mem::take(&mut s.view_log);
            let positions: Vec<(libpikmin::CreatureId, Vec3)> =
                s.frame.creatures.iter().map(|c| (c.id, c.pos)).collect();
            let creatures = Creatures { positions: &positions, table_sounds: &[] };
            for e in &events {
                v.view.on_event(e);
                if let Some(a) = v.audio.as_mut() {
                    a.on_event(e, &s.sim, creatures);
                }
            }
            let dt = f.dt.clamp(0.0, 0.25);
            // The camera, for environment maps and billboards: the same axes the listener uses.
            let fwd = Vec3::from(f.forward).normalize_or(Vec3::Z);
            let right = Vec3::from(f.up).cross(fwd).normalize_or(Vec3::X);
            let up = fwd.cross(right);
            v.view.set_camera(right, up, fwd);
            let alpha = if f.lag.is_finite() { 1.0 - f.lag.clamp(0.0, 1.0) } else { 1.0 };
            v.view.update(&s.sim, alpha, dt);
            v.view.particles(right, up);
            v.particles.clear();
            v.particles.extend(v.view.particles.quads.iter().map(|q| PikParticle {
                corners: q.corners.map(PikVec3::from),
                color: q.color,
                texture: q.texture,
                additive: q.additive as u8,
                _pad: [0; 3],
            }));
            let tick = s.sim.tick();
            if let Some(a) = v.audio.as_mut() {
                a.set_volume(f.volume);
                for command in std::mem::take(&mut v.view.demo_sounds) { a.demo(command); }
                // `SeSystem::update` runs once per game frame (30 Hz): once per tick here.
                if v.last_tick != Some(tick) {
                    let me = usize::try_from(f.listener).ok().and_then(|i| s.sim.captains().get(i));
                    let at = me.map_or(Vec3::from(f.eye), |c| c.pos);
                    let whistling = me.is_some_and(|c| matches!(c.whistle, WhistleState::Calling { .. }));
                    let cam = listener_view(f.eye.into(), f.forward.into(), f.up.into());
                    let sprouts = std::mem::take(&mut v.view.animator.sprout_sounds);
                    let ship = std::mem::take(&mut v.view.ship_sounds);
                    let swarm = me.filter(|c| c.swarming).map_or(glam::Vec2::ZERO, |c| c.swarm_input());
                    let onion_spot = me.is_some_and(|c| s.sim.onion_in_reach(c.id).is_some());
                    let scene = libpikmin_present::audio::SceneSounds { sprouts: &sprouts, ship: &ship, swarm, onion_spot };
                    a.frame(cam, at, &s.sim, creatures, whistling, scene);
                }
            } else {
                v.view.animator.sprout_sounds.clear();
                v.view.ship_sounds.clear();
                v.view.demo_sounds.clear();
            }
            v.last_tick = Some(tick);
            events.clear();
            s.view_log = events;
        })
    }
}

/// Writes up to `max` of this frame's instances; returns how many.
///
/// # Safety
/// `view` from [`pik_view_new`]; `out` writable for `max`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_draws(view: *mut PikView, out: *mut PikDraw, max: u32) -> u32 {
    // SAFETY: as documented.
    unsafe {
        with_view(view, 0, |v| {
            if out.is_null() {
                return v.view.draws.len() as u32;
            }
            let n = v.view.draws.len().min(max as usize);
            for (i, d) in v.view.draws.iter().take(n).enumerate() {
                out.add(i).write(PikDraw {
                    key: d.key,
                    model: d.model,
                    kind: d.kind,
                    flags: d.flags,
                    _pad: [0; 2],
                    pos: d.pos.into(),
                    rot: d.rot.to_array(),
                    first_vertex: d.first_vertex,
                    first_color: d.first_color,
                    alpha: d.alpha,
                    id: (d.key & 0xFFFF_FFFF) as u32,
                });
            }
            n as u32
        })
    }
}

/// This frame's skinned positions for every instance; `count` gets how many. Valid until the
/// next [`pik_view_update`] or [`pik_view_free`].
///
/// # Safety
/// `view` from [`pik_view_new`]; `count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_positions(view: *mut PikView, count: *mut u32) -> *const PikVec3 {
    // SAFETY: as documented. `PikVec3` and glam's `Vec3` are both three packed f32s.
    unsafe {
        with_view(view, std::ptr::null(), |v| {
            if let Some(c) = count.as_mut() {
                *c = v.view.positions.len() as u32;
            }
            v.view.positions.as_ptr().cast()
        })
    }
}

/// Like [`pik_view_positions`], the normals.
///
/// # Safety
/// As [`pik_view_positions`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_normals(view: *mut PikView, count: *mut u32) -> *const PikVec3 {
    // SAFETY: as above.
    unsafe {
        with_view(view, std::ptr::null(), |v| {
            if let Some(c) = count.as_mut() {
                *c = v.view.normals.len() as u32;
            }
            v.view.normals.as_ptr().cast()
        })
    }
}

/// This frame's batch colours (RGBA 0..1, one per batch of every instance: what its texture
/// is multiplied by, or its whole colour if it has none); `count` gets how many. Valid as [`pik_view_positions`].
///
/// # Safety
/// As [`pik_view_positions`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_colors(view: *mut PikView, count: *mut u32) -> *const [f32; 4] {
    // SAFETY: as above.
    unsafe {
        with_view(view, std::ptr::null(), |v| {
            if let Some(c) = count.as_mut() {
                *c = v.view.colors.len() as u32;
            }
            v.view.colors.as_ptr()
        })
    }
}

/// This frame's batch textures (one per batch of every instance, alongside
/// [`pik_view_colors`]): a [`pik_view_texture`] id or -1. Valid as [`pik_view_positions`].
///
/// # Safety
/// As [`pik_view_positions`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_texture_ids(view: *mut PikView, count: *mut u32) -> *const i32 {
    // SAFETY: as above.
    unsafe {
        with_view(view, std::ptr::null(), |v| {
            if let Some(c) = count.as_mut() {
                *c = v.view.texture_ids.len() as u32;
            }
            v.view.texture_ids.as_ptr()
        })
    }
}

/// This frame's texture coordinates for every instance, alongside [`pik_view_positions`]:
/// the model's own, except environment-mapped batches' ([`PikBatch::env`]), which follow the
/// camera. Valid as [`pik_view_positions`].
///
/// # Safety
/// As [`pik_view_positions`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_frame_uvs(view: *mut PikView, count: *mut u32) -> *const [f32; 2] {
    // SAFETY: as above.
    unsafe {
        with_view(view, std::ptr::null(), |v| {
            if let Some(c) = count.as_mut() {
                *c = v.view.uvs.len() as u32;
            }
            v.view.uvs.as_ptr()
        })
    }
}

/// This frame's particles; `count` gets how many. Valid as [`pik_view_positions`].
///
/// # Safety
/// As [`pik_view_positions`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_particles(view: *mut PikView, count: *mut u32) -> *const PikParticle {
    // SAFETY: as above.
    unsafe {
        with_view(view, std::ptr::null(), |v| {
            if let Some(c) = count.as_mut() {
                *c = v.particles.len() as u32;
            }
            v.particles.as_ptr()
        })
    }
}

/// Particle texture `id`, as [`pik_view_texture`] (RGBA8, rows top first; wrap 0 repeat, 1
/// clamp). New ids can appear in any frame.
///
/// # Safety
/// As [`pik_view_texture`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_particle_texture(
    view: *mut PikView,
    id: u32,
    info: *mut PikTexture,
    rgba: *mut u8,
    len: u32,
) -> u8 {
    // SAFETY: as documented.
    unsafe {
        with_view(view, 0, |v| {
            let Some(t) = v.view.particles.textures.get(id as usize) else { return 0 };
            if let Some(info) = info.as_mut() {
                *info = PikTexture {
                    width: t.image.width,
                    height: t.image.height,
                    wrap_s: (!t.wrap[0]) as u8,
                    wrap_t: (!t.wrap[1]) as u8,
                    _pad: [0; 2],
                };
            }
            if rgba.is_null() {
                return 1;
            }
            if t.image.rgba.len() > len as usize {
                return 0;
            }
            std::ptr::copy_nonoverlapping(t.image.rgba.as_ptr(), rgba, t.image.rgba.len());
            1
        })
    }
}

const _: () = assert!(std::mem::size_of::<PikVec3>() == std::mem::size_of::<Vec3>());

// ------------------------------------------------------------------------------------------
// Enemies (teki) that a host's own AI moves: the view only draws them.

/// Frame keys of one of an enemy's motions, in animation frames (30 a second).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikTekiMotion {
    /// The `.key` AnimMgr index to give [`PikTekiInstance::motion`].
    pub index: u32,
    pub start: f32,
    pub end: f32,
    pub loop_start: f32,
    pub loop_end: f32,
    /// Where a bite lands and where the chewing ends (`end` when the motion has none).
    pub action0: f32,
    pub action1: f32,
}

/// What the disc says about an enemy kind (`libpikmin_present::teki::TekiInfo`), in sim units.
/// `motions` follow `teki::slot`: wait, turn, chase, attack, gulp, flick, dead, hurt, sleep, cry.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikTekiInfo {
    pub kind: u32,
    /// The kind's model, as in [`pik_view_model`] and [`PikDraw::model`].
    pub model: u32,
    pub life: f32,
    pub scale: f32,
    pub weight: f32,
    pub run_speed: f32,
    /// Radians a second.
    pub turn_speed: f32,
    pub visible_range: f32,
    pub visible_angle: f32,
    pub attackable_range: f32,
    pub attackable_angle: f32,
    pub attack_range: f32,
    pub attack_power: f32,
    pub collision_radius: f32,
    pub corpse_size: f32,
    pub corpse_height: f32,
    pub corpse_min_carriers: u32,
    pub corpse_max_carriers: u32,
    pub corpse_seeds_matching: i32,
    pub corpse_seeds_other: i32,
    pub swallow_count: u32,
    pub corpse_offset: PikVec3,
    pub lo: PikVec3,
    pub hi: PikVec3,
    pub motions: [PikTekiMotion; 10],
}

/// One enemy to draw at the next [`pik_view_update`].
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PikTekiInstance {
    /// Stable while it lives: the draw's `id`.
    pub id: u32,
    pub kind: u32,
    pub pos: PikVec3,
    /// Radians about +Y; it faces `(sin, 0, cos)`.
    pub yaw: f32,
    pub motion: u32,
    pub frame: f32,
    /// Seconds of hit flash left.
    pub flash: f32,
    /// 1 at the disc's size.
    pub scale: f32,
    /// 1 opaque .. 0 gone.
    pub alpha: f32,
}

/// Have views made after this load the enemy models (`view::set_teki_models`). Call before
/// [`pik_view_new`].
#[unsafe(no_mangle)]
pub extern "C" fn pik_view_set_teki_models(on: u8) {
    libpikmin_present::view::set_teki_models(on != 0);
}

/// `out[0]` the size of [`PikTekiInfo`], `out[1]` of [`PikTekiInstance`], for hosts to check
/// their mirrors.
///
/// # Safety
/// `out` writable for two `u32`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_teki_sizes(out: *mut u32) {
    if out.is_null() {
        return;
    }
    // SAFETY: as documented.
    unsafe {
        out.write(std::mem::size_of::<PikTekiInfo>() as u32);
        out.add(1).write(std::mem::size_of::<PikTekiInstance>() as u32);
    }
}

/// Fills `out` for enemy kind `kind` (0 Dwarf Bulborb, 1 Spotty Bulborb); 0 if the kind's files
/// weren't loaded (no enemy models, or not on the disc).
///
/// # Safety
/// `view` from [`pik_view_new`]; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_teki_info(view: *mut PikView, kind: u32, out: *mut PikTekiInfo) -> u8 {
    if out.is_null() {
        return 0;
    }
    // SAFETY: as documented.
    unsafe {
        with_view(view, 0, |v| {
            let Some(t) = u8::try_from(kind).ok().and_then(|k| v.view.teki(k)) else { return 0 };
            let i = &t.info;
            let mut m = [PikTekiMotion::default(); 10];
            for (o, s) in m.iter_mut().zip(&i.motions) {
                *o = PikTekiMotion {
                    index: s.index,
                    start: s.start,
                    end: s.end,
                    loop_start: s.loop_start,
                    loop_end: s.loop_end,
                    action0: s.action0,
                    action1: s.action1,
                };
            }
            out.write(PikTekiInfo {
                kind,
                model: i.model,
                life: i.life,
                scale: i.scale,
                weight: i.weight,
                run_speed: i.run_speed,
                turn_speed: i.turn_speed,
                visible_range: i.visible_range,
                visible_angle: i.visible_angle,
                attackable_range: i.attackable_range,
                attackable_angle: i.attackable_angle,
                attack_range: i.attack_range,
                attack_power: i.attack_power,
                collision_radius: i.collision_radius,
                corpse_size: i.corpse_size,
                corpse_height: i.corpse_height,
                corpse_min_carriers: i.corpse_min_carriers,
                corpse_max_carriers: i.corpse_max_carriers,
                corpse_seeds_matching: i.corpse_seeds_matching,
                corpse_seeds_other: i.corpse_seeds_other,
                swallow_count: i.swallow_count,
                corpse_offset: i.corpse_offset.into(),
                lo: i.lo.into(),
                hi: i.hi.into(),
                motions: m,
            });
            1
        })
    }
}

/// The enemies to draw at the next [`pik_view_update`] (replacing any given since the last):
/// their draws have kind 12 and `id` = [`PikTekiInstance::id`].
///
/// # Safety
/// `view` from [`pik_view_new`]; `list` readable for `count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_view_set_tekis(view: *mut PikView, list: *const PikTekiInstance, count: u32) {
    // SAFETY: as documented.
    unsafe {
        with_view(view, (), |v| {
            let src = if list.is_null() || count == 0 { &[][..] } else { std::slice::from_raw_parts(list, count as usize) };
            let instances: Vec<_> = src
                .iter()
                .filter_map(|t| {
                    Some(libpikmin_present::teki::TekiInstance {
                        id: t.id,
                        kind: u8::try_from(t.kind).ok()?,
                        pos: t.pos.into(),
                        yaw: t.yaw,
                        motion: t.motion,
                        frame: t.frame,
                        flash: t.flash,
                        scale: t.scale,
                        alpha: t.alpha,
                    })
                })
                .collect();
            v.view.set_tekis(&instances);
        })
    }
}
