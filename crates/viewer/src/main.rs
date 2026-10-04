//! Top-down 2D test host. World X → screen right, world Z → screen up.
//!
//! Controls: WASD/arrows move (hold Ctrl for a light tilt that only moves the cursor),
//! Space = throw (hold, release to throw), Shift / right mouse = whistle, X = dismiss,
//! Tab = switch captain, C = spawn a creature at the cursor, K = make creatures shake,
//! F1 = formation debug, R = reset, mouse wheel = zoom. Space next to a sprout plucks it.
//!
//! Dead creatures become corpses the Pikmin can carry to the Onions (red, yellow, blue).
//! Creatures fight back: they chase, bite (eating up to 3 Pikmin, hurting captains) and the big
//! one stomps. H revives (or heals) the active captain.

use std::path::PathBuf;

use libpikmin::captain::WhistleState;
use libpikmin::host::{CreatureFrame, CreatureId, CreaturePart};
use libpikmin::onion::OnionAnim;
use libpikmin::piki::AttackMode;
use libpikmin::{
    CaptainId, CaptainInput, Event, Happa, Host, Params, PelletDesc, PelletId, PelletState,
    PikiColor, PikiId, PikiState, Sim, SproutState, Vec2, Vec3,
};
use macroquad::prelude::*;

struct Dummy {
    id: CreatureId,
    pos: Vec3,
    yaw: f32,
    radius: f32,
    height: f32,
    hp: f32,
    max_hp: f32,
    wander: Vec3,
    shake_timer: f32,
    flash: f32,
    corpse: Option<PelletId>,
    bite_timer: f32,
    chew_timer: f32,
    stomp_timer: f32,
}

#[derive(Default)]
struct World {
    creatures: Vec<Dummy>,
    next_id: CreatureId,
    seed: u32,
}

impl World {
    fn rand(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 10_000) as f32 / 10_000.0
    }

    fn spawn(&mut self, pos: Vec3, radius: f32, hp: f32) {
        self.next_id += 1;
        self.creatures.push(Dummy {
            id: self.next_id,
            pos,
            yaw: 0.0,
            radius,
            height: radius * 0.8,
            hp,
            max_hp: hp,
            wander: pos,
            shake_timer: 3.0,
            flash: 0.0,
            corpse: None,
            bite_timer: 2.0,
            chew_timer: 0.0,
            stomp_timer: 5.0,
        });
    }

    fn update(&mut self, dt: f32, sim: &mut Sim, force_shake: bool) {
        for i in 0..self.creatures.len() {
            let (r1, r2) = (self.rand(), self.rand());
            let c = &mut self.creatures[i];
            c.flash = (c.flash - dt).max(0.0);
            if c.hp <= 0.0 {
                // Dead: becomes a corpse the Pikmin can carry. Carry weights and seeds are this
                // host's choice (Pikmin 1's own per-enemy table lives in the disc's pelMgr.bin).
                if c.corpse.is_none() {
                    let min = (c.radius / 6.0).round().max(1.0) as u32;
                    c.corpse = Some(sim.spawn_pellet(PelletDesc {
                        pos: c.pos,
                        yaw: c.yaw,
                        radius: c.radius * 0.8,
                        height: c.height,
                        min_carriers: min,
                        max_carriers: min * 2,
                        seeds_matching: min as i32 + 1,
                        seeds_other: min as i32 + 1,
                        color: None,
                        creature: Some(c.id),
                    }));
                }
                continue;
            }
            // A simple enemy AI (this host's, not Pikmin's): chase the nearest Pikmin or
            // captain on the ground, bite, chew. The sim only gets told what happened.
            c.bite_timer -= dt;
            c.stomp_timer -= dt;
            let id = c.id;
            if c.chew_timer > 0.0 {
                c.chew_timer -= dt;
                if c.chew_timer <= 0.0 {
                    sim.chew(id);
                }
            } else {
                let mut target = None;
                let mut best = 200.0f32;
                for cap in sim.captains().iter().filter(|k| !k.down) {
                    let d = (cap.pos - c.pos).with_y(0.0).length();
                    if d < best {
                        best = d;
                        target = Some(cap.pos);
                    }
                }
                for pk in sim.pikis() {
                    let free = pk.is_alive()
                        && pk.on_ground
                        && pk.state.latched_to().is_none()
                        && !matches!(
                            pk.state,
                            PikiState::Swallowed { .. } | PikiState::Pressed { .. }
                        );
                    let d = (pk.pos - c.pos).with_y(0.0).length();
                    if free && d < best {
                        best = d;
                        target = Some(pk.pos);
                    }
                }
                let fwd = Vec3::new(c.yaw.sin(), 0.0, c.yaw.cos());
                let mouth = c.pos + fwd * (c.radius + 6.0) + Vec3::new(0.0, 6.0, 0.0);
                match target {
                    Some(t) if best < c.radius + 24.0 && c.bite_timer <= 0.0 => {
                        let eaten = sim.swallow_pikis(id, 0, mouth, 20.0, 3);
                        let caps: Vec<_> = sim
                            .captains()
                            .iter()
                            .filter(|k| !k.down && (k.pos - mouth).with_y(0.0).length() < 26.0)
                            .map(|k| k.id)
                            .collect();
                        for k in caps {
                            sim.damage_captain(k, 10.0); // InteractSwallow::actNavi takes 10
                        }
                        c.bite_timer = 2.5;
                        if eaten > 0 {
                            c.chew_timer = 1.0;
                        }
                        let _ = t;
                    }
                    Some(t) => {
                        let to = (t - c.pos).with_y(0.0);
                        if to.length() > c.radius + 10.0 {
                            let step = to.normalize() * 40.0 * dt;
                            c.pos += step;
                        }
                        c.yaw = to.x.atan2(to.z);
                    }
                    None => {
                        let to = c.wander - c.pos;
                        if to.length() < 10.0 {
                            c.wander =
                                c.pos + Vec3::new((r1 - 0.5) * 300.0, 0.0, (r2 - 0.5) * 300.0);
                        } else {
                            let step = to.normalize() * 25.0 * dt;
                            c.pos += step;
                            c.yaw = step.x.atan2(step.z);
                        }
                    }
                }
                // The big one stomps when Pikmin crowd it.
                if c.radius >= 35.0 && c.stomp_timer <= 0.0 && best < c.radius + 30.0 {
                    sim.crush_pikis_in_sphere(c.pos, c.radius + 30.0, 20.0);
                    sim.shake_off(id);
                    c.stomp_timer = 6.0;
                }
            }
            // Shake when enough Pikmin hang on, like most Pikmin 1 enemies.
            c.shake_timer -= dt;
            let latched = sim
                .pikis()
                .iter()
                .filter(|p| p.state.latched_to() == Some(c.id))
                .count();
            if force_shake || (latched >= 4 && c.shake_timer <= 0.0) {
                sim.shake_off(c.id);
                c.shake_timer = 4.0;
            }
        }
    }
}

impl Host for World {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
        for c in self.creatures.iter().filter(|c| c.hp > 0.0) {
            let body = CreaturePart {
                centre: c.pos + Vec3::new(0.0, c.height, 0.0),
                radius: c.radius,
                stickable: true,
            };
            out.push(c.id, c.pos, c.yaw, c.hp > 0.0, false, &[body]);
        }
    }

    fn damage(
        &mut self,
        creature: CreatureId,
        amount: f32,
        _part: Option<u32>,
        _from: PikiId,
    ) -> bool {
        let Some(c) = self.creatures.iter_mut().find(|c| c.id == creature) else {
            return false;
        };
        if c.hp <= 0.0 {
            return false;
        }
        c.hp -= amount;
        c.flash = 0.08;
        true
    }
}

fn load_params() -> (Params, String) {
    let mut params = Params::default();
    let candidates = [
        std::env::var_os("PIKMIN_EXTRACTED").map(PathBuf::from),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../extracted")),
    ];
    for dir in candidates.into_iter().flatten() {
        if let Ok(applied) = params.load_extracted(&dir)
            && !applied.is_empty()
        {
            return (params, format!("disc data ({} files)", applied.len()));
        }
    }
    (
        params,
        "code defaults (no extracted disc data; set PIKMIN_EXTRACTED)".into(),
    )
}

struct Game {
    sim: Sim,
    world: World,
    captains: Vec<CaptainId>,
    active: usize,
    hits: u32,
    stolen: u32,
    params_source: String,
}

fn new_game() -> Game {
    let (params, params_source) = load_params();
    let mut sim = Sim::new(params, 0x5eed);
    let olimar = sim.add_captain(Vec3::ZERO);
    let louie = sim.add_captain(Vec3::new(-250.0, 0.0, 150.0));
    let colors = [PikiColor::Red, PikiColor::Yellow, PikiColor::Blue];
    for i in 0..60 {
        let a = i as f32 * 2.4;
        let pos = Vec3::new(
            a.sin() * (20.0 + i as f32),
            0.0,
            a.cos() * (20.0 + i as f32) - 40.0,
        );
        sim.spawn_piki_in_squad(colors[i % 3], pos, olimar);
    }
    for i in 0..20 {
        let a = i as f32 * 2.4;
        let pos = Vec3::new(-250.0 + a.sin() * 30.0, 0.0, 110.0 + a.cos() * 30.0);
        sim.spawn_piki_in_squad(PikiColor::Blue, pos, louie);
    }
    for i in 0..20 {
        sim.spawn_piki(
            PikiColor::Red,
            Vec3::new(
                220.0 + (i % 5) as f32 * 14.0,
                0.0,
                -200.0 + (i / 5) as f32 * 14.0,
            ),
        );
    }
    let mut world = World {
        seed: 0x1234_5678,
        ..Default::default()
    };
    world.spawn(Vec3::new(200.0, 0.0, 150.0), 26.0, 750.0);
    world.spawn(Vec3::new(-100.0, 0.0, 300.0), 18.0, 300.0);
    world.spawn(Vec3::new(350.0, 0.0, -50.0), 40.0, 2000.0);
    sim.add_onion(Vec3::new(-220.0, 0.0, -260.0), &[PikiColor::Red]);
    sim.add_onion(Vec3::new(0.0, 0.0, -340.0), &[PikiColor::Yellow]);
    sim.add_onion(Vec3::new(220.0, 0.0, -260.0), &[PikiColor::Blue]);
    Game {
        sim,
        world,
        captains: vec![olimar, louie],
        active: 0,
        hits: 0,
        stolen: 0,
        params_source,
    }
}

fn piki_color(c: PikiColor) -> Color {
    match c {
        PikiColor::Red => Color::from_rgba(225, 45, 45, 255),
        PikiColor::Yellow => Color::from_rgba(245, 215, 40, 255),
        PikiColor::Blue => Color::from_rgba(55, 100, 235, 255),
    }
}

fn window_conf() -> Conf {
    Conf {
        window_title: "libpikmin viewer".into(),
        window_width: 1280,
        window_height: 800,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let mut g = new_game();
    let mut zoom = 1.4f32;
    let mut show_plate = false;

    loop {
        let dt = get_frame_time();
        if is_key_pressed(KeyCode::R) {
            g = new_game();
        }
        if is_key_pressed(KeyCode::Tab) {
            g.active = (g.active + 1) % g.captains.len();
        }
        if is_key_pressed(KeyCode::F1) {
            show_plate = !show_plate;
        }
        zoom = (zoom * 1.1f32.powf(mouse_wheel().1.signum())).clamp(0.3, 5.0);

        // Input for the active captain; the other one stands still.
        let mut stick = Vec2::ZERO;
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
            stick.y += 1.0;
        }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
            stick.y -= 1.0;
        }
        if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
            stick.x += 1.0;
        }
        if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
            stick.x -= 1.0;
        }
        if is_key_down(KeyCode::LeftControl) && stick != Vec2::ZERO {
            stick = stick.normalize() * 0.5;
        }
        for (i, &cid) in g.captains.iter().enumerate() {
            let input = if i == g.active {
                CaptainInput {
                    stick,
                    whistle: is_key_down(KeyCode::LeftShift)
                        || is_mouse_button_down(MouseButton::Right),
                    throw: is_key_down(KeyCode::Space),
                    dismiss: is_key_down(KeyCode::X),
                    pose: None,
                }
            } else {
                CaptainInput::default()
            };
            g.sim.set_input(cid, input);
        }

        let active = g.captains[g.active];
        if is_key_pressed(KeyCode::C) {
            let at = g.sim.captain(active).cursor_world;
            g.world.spawn(Vec3::new(at.x, 0.0, at.z), 24.0, 600.0);
        }
        if is_key_pressed(KeyCode::H) {
            let c = g.sim.captain(active);
            if c.down {
                let pos = c.pos;
                g.sim.revive_captain(active, pos);
            } else {
                g.sim.heal_captain(active, None);
            }
        }
        g.world.update(dt, &mut g.sim, is_key_pressed(KeyCode::K));
        g.sim.advance(&mut g.world, dt);
        for e in g.sim.drain_events() {
            match e {
                Event::Hit { .. } => g.hits += 1,
                Event::Recruited {
                    stolen_from: Some(_),
                    ..
                } => g.stolen += 1,
                _ => {}
            }
        }

        // ---- draw ----
        clear_background(Color::from_rgba(58, 86, 50, 255));
        let cam = g.sim.captain(active).pos;
        let (cx, cy) = (screen_width() / 2.0, screen_height() / 2.0);
        let to_screen = |p: Vec3| vec2(cx + (p.x - cam.x) * zoom, cy - (p.z - cam.z) * zoom);
        // Height shows as a lift on screen, with a shadow left on the ground.
        let lifted = |p: Vec3| to_screen(p) - vec2(0.0, p.y * zoom * 0.6);

        draw_grid(&to_screen, zoom, cam);

        for o in g.sim.onions() {
            let s = to_screen(o.pos);
            let col = piki_color(o.colors[0]);
            draw_circle(s.x, s.y, 34.0 * zoom, Color::new(col.r, col.g, col.b, 0.35));
            draw_circle_lines(s.x, s.y, 34.0 * zoom, 3.0, col);
            let state = match o.anim {
                OnionAnim::Idle => "",
                OnionAnim::Suction { .. } => "slurp",
                OnionAnim::Emitting { .. } => "seeds!",
            };
            let stored: u32 = o.stored.iter().sum();
            draw_text(
                format!("Onion {state}"),
                s.x - 30.0,
                s.y - 38.0 * zoom,
                18.0,
                WHITE,
            );
            if stored > 0 {
                draw_text(
                    format!("{stored} inside"),
                    s.x - 26.0,
                    s.y + 6.0,
                    16.0,
                    WHITE,
                );
            }
        }

        for pel in g.sim.pellets() {
            if pel.state == PelletState::Gone {
                continue;
            }
            let r = pel.desc.radius * pel.scale * zoom;
            let shadow = to_screen(Vec3::new(pel.pos.x, 0.0, pel.pos.z));
            draw_circle(shadow.x, shadow.y, r, Color::from_rgba(0, 0, 0, 60));
            let s = lifted(pel.pos + Vec3::new(0.0, pel.desc.height * 0.5, 0.0));
            draw_circle(s.x, s.y, r, Color::from_rgba(140, 90, 110, 255));
            draw_circle_lines(s.x, s.y, r, 2.0, BLACK);
            if !matches!(pel.state, PelletState::Sucking { .. }) {
                let label = format!("{}/{}", pel.carriers.len(), pel.desc.min_carriers);
                draw_text(&label, s.x - 10.0, s.y - r - 6.0, 18.0, WHITE);
            }
        }

        for sp in g.sim.sprouts() {
            let col = piki_color(sp.color);
            match sp.state {
                SproutState::Flying => {
                    let sh = to_screen(Vec3::new(sp.pos.x, 0.0, sp.pos.z));
                    draw_circle(sh.x, sh.y, 2.0 * zoom, Color::from_rgba(0, 0, 0, 70));
                    let s = lifted(sp.pos);
                    draw_circle(s.x, s.y, 3.0 * zoom, col);
                }
                SproutState::Buried { .. } => {}
                SproutState::Tane { .. } | SproutState::Wilted { .. } => {
                    let s = to_screen(sp.pos);
                    draw_circle(s.x, s.y, 1.5 * zoom, Color::from_rgba(110, 80, 40, 255));
                }
                SproutState::Ready { .. } => {
                    let s = to_screen(sp.pos);
                    let top = vec2(s.x, s.y - 7.0 * zoom);
                    draw_line(
                        s.x,
                        s.y,
                        top.x,
                        top.y,
                        1.5,
                        Color::from_rgba(120, 220, 120, 255),
                    );
                    let (tr, tc) = match sp.happa {
                        Happa::Leaf => (2.0, Color::from_rgba(90, 200, 90, 255)),
                        Happa::Bud => (2.0, WHITE),
                        Happa::Flower => (3.0, Color::from_rgba(255, 230, 250, 255)),
                    };
                    draw_circle(top.x, top.y, tr * zoom, tc);
                    draw_circle(s.x, s.y, 1.5 * zoom, col);
                }
            }
        }

        for c in g.world.creatures.iter().filter(|c| c.hp > 0.0) {
            let s = to_screen(c.pos);
            draw_circle(s.x, s.y, c.radius * zoom, Color::from_rgba(0, 0, 0, 60));
            let body = lifted(c.pos + Vec3::new(0.0, c.height, 0.0));
            let col = if c.flash > 0.0 {
                WHITE
            } else {
                Color::from_rgba(200, 120, 150, 255)
            };
            draw_circle(body.x, body.y, c.radius * zoom, col);
            let eye = body + vec2(c.yaw.sin(), -c.yaw.cos()) * c.radius * zoom * 0.6;
            draw_circle(eye.x, eye.y, 3.0 * zoom, BLACK);
            let w = c.radius * 2.0 * zoom;
            let frac = (c.hp / c.max_hp).clamp(0.0, 1.0);
            draw_rectangle(
                body.x - w / 2.0,
                body.y - c.radius * zoom - 10.0,
                w,
                5.0,
                DARKGRAY,
            );
            draw_rectangle(
                body.x - w / 2.0,
                body.y - c.radius * zoom - 10.0,
                w * frac,
                5.0,
                GREEN,
            );
        }

        // Pikmin: ground ones first, then airborne/latched on top.
        let mut order: Vec<_> = g.sim.pikis().iter().filter(|p| p.on_field()).collect();
        order.sort_by(|a, b| a.pos.y.total_cmp(&b.pos.y));
        for p in &order {
            let shadow = to_screen(Vec3::new(p.pos.x, 0.0, p.pos.z));
            if p.pos.y > 1.0 {
                draw_circle(
                    shadow.x,
                    shadow.y,
                    p.size * zoom * 0.8,
                    Color::from_rgba(0, 0, 0, 70),
                );
            }
            let s = lifted(p.pos);
            let mut r = (p.size * zoom).max(2.0);
            let mut col = piki_color(p.color);
            match p.state {
                PikiState::Dying { .. } => {
                    col = Color::new(col.r * 0.5, col.g * 0.5, col.b * 0.5, 1.0)
                }
                PikiState::Fading { timer } => {
                    r *= (timer / 1.5).clamp(0.0, 1.0);
                    col.a = 0.6;
                }
                PikiState::Swallowed { .. } => {
                    r *= 0.7;
                    col.a = 0.45;
                }
                PikiState::Pressed { .. } => {
                    draw_ellipse(s.x, s.y, r * 2.0, r * 0.5, 0.0, col);
                    continue;
                }
                _ => {}
            }
            if r < 0.5 {
                continue;
            }
            draw_circle(s.x, s.y, r, col);
            let outline = match p.state {
                PikiState::Attack(a) if matches!(a.mode, AttackMode::Latched { .. }) => Some(WHITE),
                PikiState::Attack(_) => Some(ORANGE),
                PikiState::Knocked { .. } => Some(GRAY),
                PikiState::Idle => Some(Color::from_rgba(20, 20, 20, 180)),
                _ => None,
            };
            if let Some(o) = outline {
                draw_circle_lines(s.x, s.y, r + 1.0, 1.5, o);
            }
            // Leaf
            let tip = s + vec2(p.face.sin(), -p.face.cos()) * r * 1.3;
            draw_line(
                s.x,
                s.y,
                tip.x,
                tip.y,
                1.0,
                Color::from_rgba(120, 220, 120, 255),
            );
        }

        let params = g.sim.params().clone();
        for (i, c) in g.sim.captains().iter().enumerate() {
            let s = lifted(c.pos);
            let col = if i == 0 {
                Color::from_rgba(250, 120, 30, 255)
            } else {
                Color::from_rgba(120, 200, 255, 255)
            };
            let body = if c.down {
                GRAY
            } else if c.damage_frames > 0.0 {
                WHITE
            } else {
                col
            };
            draw_circle(s.x, s.y, 9.0 * zoom, body);
            let hp = (c.health / params.navi.health).clamp(0.0, 1.0);
            draw_rectangle(s.x - 15.0, s.y + 11.0 * zoom, 30.0, 4.0, DARKGRAY);
            draw_rectangle(
                s.x - 15.0,
                s.y + 11.0 * zoom,
                30.0 * hp,
                4.0,
                if hp > 0.25 { GREEN } else { RED },
            );
            if c.down {
                draw_text("DOWN (H)", s.x - 30.0, s.y + 30.0 * zoom, 18.0, RED);
            }
            let tip = s + vec2(c.face.sin(), -c.face.cos()) * 14.0 * zoom;
            draw_line(s.x, s.y, tip.x, tip.y, 2.0, BLACK);
            draw_text(
                if i == 0 { "Olimar" } else { "Louie" },
                s.x - 20.0,
                s.y - 14.0 * zoom,
                16.0,
                WHITE,
            );

            let cur = to_screen(c.cursor_world);
            draw_circle_lines(cur.x, cur.y, 8.0 * zoom, 2.0, col);
            let wr = c.whistle_radius(&params);
            if wr > 0.0 {
                let calling = matches!(c.whistle, WhistleState::Calling { .. });
                let wc = if calling {
                    Color::from_rgba(255, 240, 120, 200)
                } else {
                    Color::from_rgba(255, 240, 120, 90)
                };
                draw_circle_lines(cur.x, cur.y, wr * zoom, 3.0, wc);
            }
            if show_plate {
                for slot in 0..c.plate.len() {
                    let sp = to_screen(c.plate.slot_world(slot));
                    draw_circle_lines(sp.x, sp.y, 2.0, 1.0, col);
                }
                let pc = to_screen(c.plate.centre);
                draw_circle(pc.x, pc.y, 3.0, col);
            }
        }

        // HUD
        let count =
            |f: &dyn Fn(&PikiState) -> bool| g.sim.pikis().iter().filter(|p| f(&p.state)).count();
        let lines = [
            format!(
                "{}  squad {} | Louie squad {} | idle {} | attacking {} | latched {} | knocked {}",
                if g.active == 0 { "Olimar" } else { "Louie" },
                g.sim.captain(g.captains[0]).plate.len(),
                g.sim.captain(g.captains[1]).plate.len(),
                count(&|s| matches!(s, PikiState::Idle)),
                count(&|s| matches!(s, PikiState::Attack(_))),
                count(&|s| s.latched_to().is_some()),
                count(&|s| matches!(s, PikiState::Knocked { .. })),
            ),
            format!(
                "carrying {} | sprouts {} (pluckable {}) | Pikmin {}",
                count(&|s| matches!(s, PikiState::Carry(_))),
                g.sim.sprouts().count(),
                g.sim.sprouts().filter(|s| s.pluckable()).count(),
                g.sim.pikis().iter().filter(|p| p.is_alive()).count(),
            ),
            format!("hits {}  stolen {}  tick {}  {} fps  |  {}", g.hits, g.stolen, g.sim.tick(), get_fps(), g.params_source),
            "WASD move (Ctrl: aim only)  Space throw/pluck  Shift/RMB whistle  H revive/heal  X dismiss  Tab captain  C creature  K shake  F1 slots  R reset".into(),
        ];
        for (i, l) in lines.iter().enumerate() {
            draw_text(l, 12.0, 22.0 + i as f32 * 20.0, 20.0, WHITE);
        }

        next_frame().await;
    }
}

fn draw_grid(to_screen: &dyn Fn(Vec3) -> macroquad::math::Vec2, zoom: f32, cam: Vec3) {
    let c = Color::from_rgba(255, 255, 255, 18);
    let step = 100.0;
    let half_w = screen_width() / zoom / 2.0 + step;
    let half_h = screen_height() / zoom / 2.0 + step;
    let mut x = ((cam.x - half_w) / step).floor() * step;
    while x < cam.x + half_w {
        let a = to_screen(Vec3::new(x, 0.0, cam.z - half_h));
        let b = to_screen(Vec3::new(x, 0.0, cam.z + half_h));
        draw_line(a.x, a.y, b.x, b.y, 1.0, c);
        x += step;
    }
    let mut z = ((cam.z - half_h) / step).floor() * step;
    while z < cam.z + half_h {
        let a = to_screen(Vec3::new(cam.x - half_w, 0.0, z));
        let b = to_screen(Vec3::new(cam.x + half_w, 0.0, z));
        draw_line(a.x, a.y, b.x, b.y, 1.0, c);
        z += step;
    }
}
