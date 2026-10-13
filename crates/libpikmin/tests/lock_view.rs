//! Where the locked-on cursor is drawn: in front of the middle of the enemy's body, whatever
//! its shape or size, at the same size as ever.

use libpikmin::{CaptainId, CreatureFrame, CreaturePart, Host, Params, Sim, Vec3};

struct Enemy {
    alive: bool,
    parts: Vec<CreaturePart>,
}

impl Host for Enemy {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
        out.push(7, Vec3::ZERO, 0.0, self.alive, false, &self.parts);
    }
}

fn part(centre: Vec3, radius: f32, stickable: bool) -> CreaturePart {
    CreaturePart { centre, radius, stickable }
}

fn locked(parts: Vec<CreaturePart>) -> (Sim, Enemy, CaptainId) {
    let mut sim = Sim::new(Params::default(), 5);
    let c = sim.add_captain(Vec3::ZERO);
    let mut enemy = Enemy { alive: true, parts };
    sim.step(&mut enemy);
    sim.set_lock_target(c, Some(7));
    sim.step(&mut enemy);
    (sim, enemy, c)
}

#[test]
fn a_single_sphere_gets_the_cursor_on_its_near_side() {
    let aim = Vec3::new(200.0, 30.0, 0.0);
    let (sim, _, c) = locked(vec![part(aim, 20.0, true)]);
    let view = sim.captain(c).lock_view.expect("locked on");
    assert!(view.pos.x < aim.x - 20.0 && view.pos.x > aim.x - 30.0, "{view:?}");
    assert!(view.eye.x.abs() < 1.0 && view.eye.y > 0.0);
    // The throw still lands on the part's centre.
    assert!(sim.captain(c).cursor_world.distance(aim) < 0.01);
}

#[test]
fn on_a_big_body_it_is_centred_not_at_the_part_the_throw_aims_at() {
    // A rock body with a stickable lump low on its right side (the first stickable part).
    let body = part(Vec3::new(400.0, 150.0, 0.0), 150.0, false);
    let lump = part(Vec3::new(430.0, 20.0, 110.0), 30.0, true);
    let (sim, _, c) = locked(vec![body, lump]);
    let view = sim.captain(c).lock_view.unwrap();
    // On the line from his eye to the box around everything, in front of the body.
    let (lo, hi) = (Vec3::new(250.0, -10.0, -150.0), Vec3::new(550.0, 300.0, 150.0));
    let middle = (lo + hi) * 0.5;
    let toward = (middle - view.eye).normalize();
    let along = (view.pos - view.eye).normalize();
    assert!(toward.dot(along) > 0.999, "{view:?} is off the line to the middle");
    assert!(view.pos.distance(body.centre) >= body.radius, "{view:?} is inside the body");
    assert!(view.pos.x < body.centre.x, "{view:?} is not in front");
    // Well away from the lump the throw aims at.
    assert!(view.pos.distance(lump.centre) > 150.0);
    assert!(sim.captain(c).cursor_world.distance(lump.centre) < 0.01);
}

#[test]
fn it_never_sits_inside_any_part() {
    let a = part(Vec3::new(150.0, 40.0, 0.0), 80.0, false);
    let b = part(Vec3::new(190.0, 40.0, 0.0), 30.0, true);
    let (sim, _, c) = locked(vec![a, b]);
    let view = sim.captain(c).lock_view.unwrap();
    for p in [a, b] {
        assert!(view.pos.distance(p.centre) >= p.radius - 0.01, "{view:?} is inside {p:?}");
    }
}

#[test]
fn a_ring_of_parts_with_nothing_in_the_middle_gets_the_cursor_at_its_near_side() {
    let ring: Vec<_> = (0..6)
        .map(|k| {
            let a = k as f32 * std::f32::consts::TAU / 6.0;
            part(Vec3::new(600.0 + 200.0 * a.cos(), 100.0, 200.0 * a.sin()), 40.0, k == 0)
        })
        .collect();
    let (sim, _, c) = locked(ring.clone());
    let view = sim.captain(c).lock_view.unwrap();
    // In front of the first part the line to the middle enters, not buried in the ring.
    assert!(view.pos.x < 400.0 && view.pos.x > 300.0, "{view:?}");
    for p in &ring {
        assert!(view.pos.distance(p.centre) >= p.radius - 0.01);
    }
}

#[test]
fn no_lock_or_a_dead_target_has_no_view() {
    let (mut sim, mut enemy, c) = locked(vec![part(Vec3::new(100.0, 20.0, 0.0), 15.0, true)]);
    assert!(sim.captain(c).lock_view.is_some());
    enemy.alive = false;
    sim.step(&mut enemy);
    assert!(sim.captain(c).lock_view.is_none());
    enemy.alive = true;
    sim.step(&mut enemy);
    assert!(sim.captain(c).lock_view.is_some());
    sim.set_lock_target(c, None);
    sim.step(&mut enemy);
    assert!(sim.captain(c).lock_view.is_none());
}
