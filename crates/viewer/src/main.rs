//! Top-down 2D test host. World X → screen right, world Z → screen up.
//!
//! Controls: WASD/arrows move (hold Ctrl for a light tilt that only moves the cursor),
//! Space = throw (hold, release to throw), Shift / right mouse = whistle, X = dismiss,
//! Tab = switch captain, C = spawn a creature at the cursor, K = make creatures shake,
//! F1 = formation debug, R = reset, mouse wheel = zoom.

use std::path::PathBuf;

use libpikmin::captain::WhistleState;
use libpikmin::host::{CreatureFrame, CreatureId, CreaturePart};
use libpikmin::piki::AttackMode;
use libpikmin::{
    CaptainId, CaptainInput, Event, Host, Params, PikiColor, PikiId, PikiState, Sim, Vec2, Vec3,
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
        });
    }

    fn update(&mut self, dt: f32, sim: &mut Sim, force_shake: bool) {
        for i in 0..self.creatures.len() {
            let (r1, r2) = (self.rand(), self.rand());
            let c = &mut self.creatures[i];
            c.flash = (c.flash - dt).max(0.0);
            if c.hp <= 0.0 {
                continue;
            }
            // Slow wander so latched Pikmin visibly ride along.
            let to = c.wander - c.pos;
            if to.length() < 10.0 {
                c.wander = c.pos + Vec3::new((r1 - 0.5) * 300.0, 0.0, (r2 - 0.5) * 300.0);
            } else {
                let step = to.normalize() * 25.0 * dt;
                c.pos += step;
                c.yaw = step.x.atan2(step.z);
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
        for c in &self.creatures {
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

        for c in &g.world.creatures {
            let s = to_screen(c.pos);
            if c.hp <= 0.0 {
                draw_circle(s.x, s.y, c.radius * zoom, Color::from_rgba(90, 80, 70, 200));
                draw_text("corpse", s.x - 22.0, s.y + 4.0, 18.0, LIGHTGRAY);
                continue;
            }
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
        let mut order: Vec<_> = g.sim.pikis().iter().filter(|p| p.is_alive()).collect();
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
            let r = (p.size * zoom).max(2.0);
            draw_circle(s.x, s.y, r, piki_color(p.color));
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
            draw_circle(s.x, s.y, 9.0 * zoom, col);
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
            format!("hits {}  stolen {}  tick {}  {} fps  |  {}", g.hits, g.stolen, g.sim.tick(), get_fps(), g.params_source),
            "WASD move (Ctrl: aim only)  Space throw  Shift/RMB whistle  X dismiss  Tab captain  C creature  K shake  F1 slots  R reset".into(),
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
