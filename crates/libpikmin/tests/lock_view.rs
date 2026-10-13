//! Where the locked-on cursor is drawn: in front of the enemy, whatever its shape or size.

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
fn the_cursor_floats_on_the_surface_facing_the_captain() {
    let aim = Vec3::new(200.0, 30.0, 0.0);
    let (sim, _, c) = locked(vec![part(aim, 20.0, true)]);
    let view = sim.captain(c).lock_view.expect("locked on");
    // Between the captain and the aimed centre, just outside the sphere.
    assert!(view.pos.x < aim.x - 20.0 && view.pos.x > aim.x - 30.0, "{view:?}");
    assert!(view.pos.distance(aim) > 20.0);
    assert!(view.eye.x.abs() < 1.0 && view.eye.y > 0.0);
    // The throw still lands on the part's centre.
    assert!(sim.captain(c).cursor_world.distance(aim) < 0.01);
}

#[test]
fn a_bigger_enemy_gets_a_bigger_cursor_up_to_a_limit() {
    let small = locked(vec![part(Vec3::new(120.0, 20.0, 0.0), 12.0, true)]).0;
    let big = locked(vec![part(Vec3::new(400.0, 100.0, 0.0), 120.0, true)]).0;
    let huge = locked(vec![part(Vec3::new(2000.0, 100.0, 0.0), 1500.0, true)]).0;
    let scale = |s: &Sim| s.captain(CaptainId(0)).lock_view.unwrap().scale;
    assert_eq!(scale(&small), 1.0);
    assert!(scale(&big) > 2.0 && scale(&big) < scale(&huge), "{} {}", scale(&big), scale(&huge));
    assert!(scale(&huge) <= 6.0);
}

#[test]
fn it_is_pushed_out_of_a_part_that_covers_the_aimed_one() {
    // The aimed part is buried in a larger body between it and the captain.
    let body = part(Vec3::new(150.0, 40.0, 0.0), 80.0, false);
    let aimed = part(Vec3::new(190.0, 40.0, 0.0), 30.0, true);
    let (sim, _, c) = locked(vec![body, aimed]);
    let view = sim.captain(c).lock_view.unwrap();
    assert!(view.pos.distance(body.centre) >= body.radius, "{view:?} is inside the body");
    assert!(view.pos.distance(aimed.centre) >= aimed.radius, "{view:?} is inside the aimed part");
    assert!(view.pos.x < body.centre.x, "{view:?} is not in front");
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
