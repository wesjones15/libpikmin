//! A sim posed for a host that draws with its own engine: the disc's models (loaded once, for
//! the host to build meshes and textures from) and, every frame, a draw list of posed and
//! CPU-skinned instances. The host only uploads vertices; which animation plays, where, and in
//! what colour is decided here, the way the viewer decides it.
//!
//! Instances are in sim units and axes. Each one has a position and rotation; its vertices are
//! in that local frame (scale already applied), so a host places one object per instance.

use std::collections::{HashMap, HashSet};

use glam::{Mat3, Mat4, Quat, Vec3};
use libpikmin::{Event, PelletState, PikiState, Sim, SproutState};
use libpikmin_assets::pose::srt_matrix;
use libpikmin_assets::{AnimSet, Assets, Batch, Blend, Model, Skeleton, paths};

use crate::animator::{Animator, piki_look, piki_rgb};
use crate::effects::{Effects, Flat, WhistleFrame};
use crate::tev::{self, Inputs};
use libpikmin_assets::CollSphere;
use libpikmin_assets::particle::effects as fx_defs;

/// Models, in the order hosts see them.
pub mod models {
    pub const OLIMAR: u32 = 0;
    /// Blue, Red, Yellow (`PikiColor` order).
    pub const PIKMIN: [u32; 3] = [1, 2, 3];
    /// Leaf, bud, flower.
    pub const HAPPA: [u32; 3] = [4, 5, 6];
    pub const ONION: u32 = 7;
    /// Numbered pellets 1, 5, 10, 20.
    pub const PELLETS: [u32; 4] = [8, 9, 10, 11];
    pub const PIKIHEAD: u32 = 12;
    pub const CURSOR: u32 = 13;
    pub const MARKER: u32 = 14;
    pub const COUNT: usize = 15;
}

/// What a draw is, for hosts that treat some kinds differently.
pub mod kinds {
    pub const CAPTAIN: u8 = 0;
    pub const PIKI: u8 = 1;
    pub const HAPPA: u8 = 2;
    pub const ONION: u8 = 3;
    pub const PELLET: u8 = 4;
    pub const SPROUT: u8 = 5;
    pub const SPROUT_HAPPA: u8 = 6;
    pub const CURSOR: u8 = 7;
    pub const MARKER: u8 = 8;
}

/// Draw flags.
pub mod flags {
    /// Flashing from a hit: draw brighter.
    pub const FLASH: u8 = 1;
}

/// How a host draws a batch.
pub mod blend {
    pub const OPAQUE: u8 = 0;
    /// Alpha tested (a leaf's outline).
    pub const CUTOUT: u8 = 1;
    /// Blended and unlit (Olimar's helmet): a deferred host's lit shader can't blend.
    pub const TRANSLUCENT: u8 = 2;
}

/// Blended materials at least this opaque everywhere draw opaque (the pellets are 78%).
const OPAQUE_ENOUGH: f32 = 0.7;

/// Most material variants baked (pellet colours, ...) before new ones reuse the default.
const MAX_TEXTURES: usize = 256;

/// A loaded model.
pub struct ViewModel {
    pub model: Model,
    pub skel: Skeleton,
    pub batches: Vec<Batch>,
    pub vertex_count: usize,
    /// Bind-pose bounds.
    pub lo: Vec3,
    pub hi: Vec3,
    images: tev::Images,
    /// Per material: baked (it reads a texture over the mesh's coordinates).
    textured: Vec<bool>,
    /// Per batch: its baked texture with no per-draw inputs ([`View::textures`]), or -1.
    pub batch_texture: Vec<i32>,
    /// Per batch: [`blend`].
    pub batch_blend: Vec<u8>,
    /// Per batch: environment mapped ([`tev::Baked::env`]): its texture coordinates come from
    /// the view-space normals each frame ([`View::uvs`]), through this matrix.
    pub batch_env: Vec<Option<Option<[[f32; 3]; 2]>>>,
}

impl ViewModel {
    fn load(assets: &Assets, path: &str) -> Result<Self, libpikmin_assets::Error> {
        let model = assets.model(path)?;
        let skel = Skeleton::new(&model);
        let batches = model.batches()?;
        let pose = skel.pose(Mat4::IDENTITY, None);
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for b in &batches {
            for v in &b.vertices {
                let p = pose.skin(v).0;
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        let vertex_count = batches.iter().map(|b| b.vertices.len()).sum();
        let images = tev::Images::new(&model);
        let textured = (0..model.materials.len()).map(|m| tev::textured(&model, m, &images)).collect();
        Ok(ViewModel {
            model,
            skel,
            batches,
            vertex_count,
            lo,
            hi,
            images,
            textured,
            batch_texture: Vec::new(),
            batch_blend: Vec::new(),
            batch_env: Vec::new(),
        })
    }

    /// Horizontal radius of the bind pose.
    pub fn radius(&self) -> f32 {
        [self.lo.x, self.lo.z, self.hi.x, self.hi.z]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()))
            .max(0.01)
    }

    /// How batch `b` is lit: 1 its colour, 2 its alpha (`LightingControlFlags::EnableAlpha0`:
    /// with the stages' ambient alpha of 0, Olimar's helmet is only opaque where light falls).
    pub fn batch_light(&self, b: usize) -> u8 {
        let m = &self.model.materials[self.batches[b].material];
        let alpha = m.flags & libpikmin_assets::model::mat_flags::PVW != 0 && m.lighting & 4 != 0;
        m.lit() as u8 | (alpha as u8) << 1
    }
}

/// Baked materials, shared by every model: each (model, material, inputs that change it)
/// once.
#[derive(Default)]
pub struct Textures {
    pub list: Vec<tev::Baked>,
    ids: HashMap<(u32, usize, [u8; 16]), i32>,
}

impl Textures {
    /// The texture for `material` of `vm` with `inputs`, baking it the first time; -1 if the
    /// material isn't textured.
    fn get(&mut self, model: u32, vm: &ViewModel, material: usize, inputs: Inputs) -> i32 {
        if !vm.textured[material] {
            return -1;
        }
        let k = (model, material, tev::key(&vm.model, material, inputs));
        if let Some(&id) = self.ids.get(&k) {
            return id;
        }
        if self.list.len() >= MAX_TEXTURES {
            return self.get(model, vm, material, Inputs::default());
        }
        let id = match tev::bake(&vm.model, material, inputs, &vm.images) {
            Some(b) => {
                self.list.push(b);
                self.list.len() as i32 - 1
            }
            None => -1,
        };
        self.ids.insert(k, id);
        id
    }
}

/// One posed instance.
#[derive(Debug, Clone, Copy)]
pub struct Draw {
    /// Stable while the object lives: kind in the top byte, then the object's id.
    pub key: u64,
    pub kind: u8,
    pub flags: u8,
    pub model: u32,
    pub pos: Vec3,
    pub rot: Quat,
    /// Its vertices in [`View::positions`] / [`View::normals`] (the model's vertex count).
    pub first_vertex: u32,
    /// Its batches' colours in [`View::colors`] and textures in [`View::texture_ids`] (one
    /// per batch).
    pub first_color: u32,
    /// 1 opaque .. 0 gone.
    pub alpha: f32,
}

fn key(kind: u8, id: u64) -> u64 {
    (kind as u64) << 56 | (id & ((1 << 56) - 1))
}

#[derive(Clone, Copy)]
struct Interp {
    prev: Vec3,
    cur: Vec3,
    tick: u64,
}

pub struct View {
    pub models: Vec<ViewModel>,
    piki_anims: AnimSet,
    onion_anims: AnimSet,
    pikihead_anims: AnimSet,
    pub animator: Animator,
    pub draws: Vec<Draw>,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Per batch of every draw: what multiplies its texture (white for baked materials, whose
    /// colour is in the texture), alpha included.
    pub colors: Vec<[f32; 4]>,
    /// Per batch of every draw: its texture in [`View::textures`], or -1.
    pub texture_ids: Vec<i32>,
    /// Every instance's texture coordinates this frame, alongside [`View::positions`]: the
    /// model's own, except environment-mapped batches' ([`ViewModel::batch_env`]).
    pub uvs: Vec<[f32; 2]>,
    /// The camera's right, up and backward axes (GX view space), for environment maps.
    camera: [Vec3; 3],
    pub textures: Textures,
    /// The whistle and dead Pikmin's ghosts (none if the effect files didn't load).
    pub effects: Option<Effects>,
    /// [`View::effects`] as quads, after [`View::particles`].
    pub particles: Flat,
    /// Olimar's antenna sphere, where the whistle's beam starts (`Navi::demoDraw`).
    ante: Option<CollSphere>,
    whistles: Vec<WhistleFrame>,
    interp: HashMap<u64, Interp>,
    time: f32,
}

impl View {
    pub fn load(assets: &Assets) -> Result<Self, libpikmin_assets::Error> {
        let mut paths_in_order: Vec<&str> = vec![paths::OLIMAR];
        paths_in_order.extend(paths::PIKMIN);
        paths_in_order.extend(paths::HAPPA);
        paths_in_order.push(paths::ONION);
        paths_in_order.extend(paths::PELLETS);
        paths_in_order.extend([paths::PIKIHEAD, paths::CURSOR, paths::CURSOR_MARKER]);
        debug_assert_eq!(paths_in_order.len(), models::COUNT);
        let mut models = paths_in_order
            .iter()
            .map(|p| ViewModel::load(assets, p))
            .collect::<Result<Vec<_>, _>>()?;
        // Each material baked as it is with no per-draw inputs, which also says how opaque it is.
        let mut textures = Textures::default();
        for (mi, vm) in models.iter_mut().enumerate() {
            let mut ids = Vec::new();
            let mut blends = Vec::new();
            let mut envs = Vec::new();
            for b in &vm.batches {
                let id = textures.get(mi as u32, vm, b.material, Inputs::default());
                let floor = match usize::try_from(id) {
                    Ok(i) => textures.list[i].min_alpha(),
                    Err(_) => tev::flat_color(&vm.model, b.material, Inputs::default(), &vm.images)[3],
                };
                blends.push(match vm.model.materials[b.material].blend() {
                    Blend::Opaque => blend::OPAQUE,
                    Blend::AlphaTest => blend::CUTOUT,
                    Blend::AlphaBlend | Blend::Inverse if floor >= OPAQUE_ENOUGH => blend::OPAQUE,
                    Blend::AlphaBlend | Blend::Inverse => blend::TRANSLUCENT,
                });
                envs.push(usize::try_from(id).ok().and_then(|i| textures.list[i].env));
                ids.push(id);
            }
            vm.batch_texture = ids;
            vm.batch_blend = blends;
            vm.batch_env = envs;
        }
        let ante = models[models::OLIMAR as usize].model.coll.iter().find(|c| c.id == "ante").cloned();
        Ok(View {
            models,
            piki_anims: AnimSet::load(assets, paths::PIKI_ANIM_MGR, paths::PIKI_BUNDLE)?,
            onion_anims: AnimSet::load(assets, paths::ONION_ANIM_MGR, paths::ONION_BUNDLE)?,
            pikihead_anims: AnimSet::load(assets, paths::PIKIHEAD_ANIM_MGR, paths::PIKIHEAD_BUNDLE)?,
            animator: Animator::default(),
            draws: Vec::new(),
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            texture_ids: Vec::new(),
            uvs: Vec::new(),
            camera: [Vec3::X, Vec3::Y, Vec3::Z],
            textures,
            effects: Effects::load(assets),
            particles: Flat::default(),
            ante,
            whistles: Vec::new(),
            interp: HashMap::new(),
            time: 0.0,
        })
    }

    pub fn on_event(&mut self, e: &Event) {
        self.animator.on_event(e);
        // `Piki::doKill`: the ghost rises from where it died.
        if let Event::PikiGone { pos, .. } = e
            && let Some(fx) = self.effects.as_mut()
        {
            fx.spawn(fx_defs::PIKI_DEAD_SOUL, *pos);
        }
    }

    /// The camera's world axes (`forward` is where it looks), for environment maps and
    /// particles. Call before [`View::update`].
    pub fn set_camera(&mut self, right: Vec3, up: Vec3, forward: Vec3) {
        self.camera = [right, up, -forward];
    }

    /// This frame's particles as quads facing a camera whose world axes are `right` and `up`
    /// (into [`View::particles`]).
    pub fn particles(&mut self, right: Vec3, up: Vec3) {
        match self.effects.as_mut() {
            Some(fx) => self.particles.update(fx, right, up),
            None => self.particles.quads.clear(),
        }
    }

    /// Where an object is between the last two ticks (`alpha` 0..1 into the next tick).
    fn place(&mut self, k: u64, at: Vec3, tick: u64, alpha: f32) -> Vec3 {
        let e = self.interp.entry(k).or_insert(Interp { prev: at, cur: at, tick });
        if e.tick != tick {
            // A jump (an Onion's Pikmin coming out, a respawn) isn't smoothed.
            e.prev = if (at - e.cur).length() > 200.0 { at } else { e.cur };
            e.cur = at;
            e.tick = tick;
        }
        e.prev.lerp(e.cur, alpha)
    }

    /// Skins `model` posed by `pose_root`/`anim`, appends a draw, and returns the joints.
    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        k: u64,
        kind: u8,
        model: u32,
        pos: Vec3,
        rot: Quat,
        pose: libpikmin_assets::Pose,
        inputs: &dyn Fn(usize) -> Inputs,
        alpha: f32,
        flags: u8,
    ) {
        let vm = &self.models[model as usize];
        let first_vertex = self.positions.len() as u32;
        let first_color = self.colors.len() as u32;
        for (bi, b) in vm.batches.iter().enumerate() {
            let env = vm.batch_env.get(bi).copied().flatten();
            for v in &b.vertices {
                let (p, n) = pose.skin(v);
                let n = n.normalize_or_zero();
                self.positions.push(p);
                self.normals.push(n);
                self.uvs.push(match env {
                    Some(mtx) => {
                        let w = rot * n;
                        let [r, u, back] = self.camera;
                        tev::env_uv(Vec3::new(w.dot(r), w.dot(u), w.dot(back)), mtx)
                    }
                    None => [v.uv.x, v.uv.y],
                });
            }
            let inp = inputs(b.material);
            let id = self.textures.get(model, vm, b.material, inp);
            let mut c = if id >= 0 { [1.0; 4] } else { tev::flat_color(&vm.model, b.material, inp, &vm.images) };
            c[3] *= alpha;
            self.colors.push(c);
            self.texture_ids.push(id);
        }
        self.draws.push(Draw { key: k, kind, flags, model, pos, rot, first_vertex, first_color, alpha });
    }

    /// Poses everything in `sim` for this frame. `alpha` is how far into the next tick the
    /// clock is ([`Sim::alpha`]), `dt` the frame time.
    pub fn update(&mut self, sim: &Sim, alpha: f32, dt: f32) {
        self.draws.clear();
        self.positions.clear();
        self.normals.clear();
        self.colors.clear();
        self.texture_ids.clear();
        self.uvs.clear();
        self.whistles.clear();
        self.time += dt;
        self.animator.begin_frame(dt);
        let tick = sim.tick();
        let params = sim.params().clone();
        let mut live = HashSet::new();
        let no_inputs = |_: usize| Inputs::default();

        // Onions (`GoalItem`): their colour is a register animation at frame colour * 2
        // (`GoalItem::refresh`). Ships aren't drawn here.
        for (i, o) in sim.onions().iter().enumerate() {
            if o.ship.is_some() {
                continue;
            }
            let Some((_, anim, _)) = self.animator.onion(&self.onion_anims, i, o, dt) else {
                continue;
            };
            let k = key(kinds::ONION, i as u64);
            live.insert(crate::animator::onion_key(i));
            let pose = self.models[models::ONION as usize].skel.pose(Mat4::IDENTITY, anim);
            let frame = o.colors[0].index() as f32 * 2.0;
            let inputs = move |_: usize| Inputs { frame: Some(frame), ..Default::default() };
            self.push(k, kinds::ONION, models::ONION, o.pos, Quat::IDENTITY, pose, &inputs, 1.0, 0);
        }

        // Pellets and corpses: the numbered pellets, sized to the thing being carried
        // (corpses too: a host's enemies have no Pikmin model).
        for pel in sim.pellets() {
            if pel.state == PelletState::Gone {
                continue;
            }
            let idx = match pel.desc.min_carriers {
                20.. => 3,
                10.. => 2,
                5.. => 1,
                _ => 0,
            };
            let model = models::PELLETS[idx];
            let vm = &self.models[model as usize];
            let s = pel.desc.radius / vm.radius() * pel.scale;
            let lift = -vm.lo.y * s;
            let k = key(kinds::PELLET, pel.id.0 as u64);
            let pos = self.place(k, pel.pos + Vec3::Y * lift, tick, alpha);
            let pose = self.models[model as usize].skel.pose(srt_matrix(Vec3::splat(s), Vec3::ZERO, Vec3::ZERO), None);
            let tint = pel.desc.color.map(piki_rgb).unwrap_or([230, 230, 230, 255]);
            let inputs = move |_: usize| Inputs { reg0: Some(tint), ..Default::default() };
            self.push(k, kinds::PELLET, model, pos, Quat::from_rotation_y(pel.yaw), pose, &inputs, 1.0, 0);
        }

        // Sprouts and flying seeds, with the leaf, bud or flower on joint 3.
        let mut live_sprouts = Vec::new();
        for sp in sim.sprouts() {
            live_sprouts.push(sp.id);
            live.insert(crate::animator::sprout_key(sp.id));
            let k = key(kinds::SPROUT, sp.id.0 as u64);
            let pos = self.place(k, sp.pos, tick, alpha);
            let anim = self.animator.sprout(&self.pikihead_anims, sp, dt);
            let rot = if matches!(sp.state, SproutState::Flying) && sp.vel.length() > 0.0 {
                // Flying seeds point their Y axis against their velocity.
                let y = -sp.vel.normalize();
                let x = y.cross(Vec3::Z).normalize_or(Vec3::X);
                let z = x.cross(y).normalize();
                Quat::from_mat3(&Mat3::from_cols(x, y, z))
            } else {
                Quat::IDENTITY
            };
            let pose = self.models[models::PIKIHEAD as usize].skel.pose(Mat4::IDENTITY, anim);
            let head = pose.joints.get(3).copied();
            let tint = piki_rgb(sp.color);
            let inputs = move |m: usize| Inputs { material: (m == 0).then_some(tint), ..Default::default() };
            self.push(k, kinds::SPROUT, models::PIKIHEAD, pos, rot, pose, &inputs, 1.0, 0);
            if let Some(j) = head {
                let model = models::HAPPA[sp.happa as usize];
                let pose = self.models[model as usize].skel.pose(j, None);
                let k = key(kinds::SPROUT_HAPPA, sp.id.0 as u64);
                self.push(k, kinds::SPROUT_HAPPA, model, pos, rot, pose, &no_inputs, 1.0, 0);
            }
        }
        self.animator.retain_sprouts(&live_sprouts);

        // Pikmin, with the leaf, bud or flower on joint 6 (`ViewPiki::demoDraw`).
        for p in sim.pikis() {
            let Some(look) = piki_look(&params, p) else {
                continue;
            };
            live.insert(crate::animator::piki_key(p.id));
            let k = key(kinds::PIKI, p.id.0 as u64);
            let pos = self.place(k, p.pos, tick, alpha);
            let anim = self.animator.piki(&self.piki_anims, p, &look, dt);
            let rot = Quat::from_rotation_y(p.face);
            let model = models::PIKMIN[p.color.index()];
            let pose = self.models[model as usize].skel.pose(srt_matrix(look.scale, Vec3::ZERO, Vec3::ZERO), anim);
            let head = pose.joints.get(6).copied();
            let tint = piki_rgb(p.color);
            let inputs = move |m: usize| Inputs { material: (m == 0).then_some(tint), ..Default::default() };
            self.push(k, kinds::PIKI, model, pos, rot, pose, &inputs, look.fade, 0);
            if let Some(j) = head
                && !matches!(p.state, PikiState::Pressed { .. })
            {
                let model = models::HAPPA[p.happa as usize];
                let pose = self.models[model as usize].skel.pose(j, None);
                let k = key(kinds::HAPPA, p.id.0 as u64);
                self.push(k, kinds::HAPPA, model, pos, rot, pose, &no_inputs, look.fade, 0);
            }
        }

        // Captains, and each one's cursor with the marker under it (`Navi::refresh`).
        for c in sim.captains() {
            live.insert(crate::animator::captain_key(c.id));
            let k = key(kinds::CAPTAIN, c.id.0 as u64);
            let pos = self.place(k, c.pos, tick, alpha);
            let anim = self.animator.captain(&self.piki_anims, &params, c, dt);
            let pose = self.models[models::OLIMAR as usize].skel.pose(Mat4::IDENTITY, anim);
            let flash = c.damage_frames > 0.0 && (c.damage_frames as i32 / 2) % 2 == 0;
            let f = if flash { flags::FLASH } else { 0 };
            // The whistle's beam starts at the antenna (`Navi::demoDraw`'s 'ante' sphere).
            let rot = Quat::from_rotation_y(c.face);
            let antenna = self
                .ante
                .as_ref()
                .and_then(|a| Some(pos + rot * pose.joints.get(a.joint)?.transform_point3(a.centre)))
                .unwrap_or(pos + Vec3::Y * 30.0);
            self.whistles.push(WhistleFrame {
                captain: c.id,
                state: c.whistle,
                antenna,
                cursor: c.cursor_world,
                captain_pos: pos,
                radius: c.whistle_radius(&params),
            });
            self.push(k, kinds::CAPTAIN, models::OLIMAR, pos, rot, pose, &no_inputs, 1.0, f);

            // On the ground 1 above, Z pointing away from the captain.
            let kc = key(kinds::CURSOR, c.id.0 as u64);
            let at = self.place(kc, c.cursor_world.with_y(c.cursor_world.y + 1.0), tick, alpha);
            let z = (at - pos).with_y(0.0).normalize_or(Vec3::Z);
            let x = Vec3::Y.cross(z).normalize_or(Vec3::X);
            let rot = Quat::from_mat3(&Mat3::from_cols(x, Vec3::Y, z));
            let time = self.time;
            let inputs = move |_: usize| Inputs { frame: Some(time * 30.0), ..Default::default() };
            let pose = self.models[models::CURSOR as usize].skel.pose(Mat4::IDENTITY, None);
            self.push(kc, kinds::CURSOR, models::CURSOR, at, rot, pose, &inputs, 1.0, 0);
            // The marker takes the next Pikmin to throw's colour (`findNextThrowPiki`: the
            // nearest squad Pikmin within 200), else white.
            let next = c.held_piki().map(|id| sim.piki(id).color).or_else(|| {
                sim.pikis()
                    .iter()
                    .filter(|p| p.captain == Some(c.id) && matches!(p.state, PikiState::Squad { .. }))
                    .map(|p| (p, (p.pos - c.pos).length()))
                    .filter(|(_, d)| *d < 200.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(p, _)| p.color)
            });
            let tint = next.map(piki_rgb).unwrap_or([255; 4]);
            let inputs = move |m: usize| Inputs { material: (m == 0).then_some(tint), ..Default::default() };
            let pose = self.models[models::MARKER as usize].skel.pose(Mat4::IDENTITY, None);
            let km = key(kinds::MARKER, c.id.0 as u64);
            self.push(km, kinds::MARKER, models::MARKER, at, rot, pose, &inputs, 1.0, 0);
        }

        if let Some(fx) = self.effects.as_mut() {
            fx.update(&self.whistles, params.navi.whistle_hold_time, dt);
        }

        self.animator.retain_players(&live);
        let drawn: HashSet<u64> = self.draws.iter().map(|d| d.key).collect();
        self.interp.retain(|k, _| drawn.contains(k));
    }
}
