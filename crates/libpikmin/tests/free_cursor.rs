//! A host's free cursor (`CaptainInput::cursor_free`, a VR pointer's hit): the point is kept
//! as aimed, height and all, where the ordinary aimed cursor goes on the ground below it.

use libpikmin::{CaptainInput, CreatureFrame, Host, Params, Sim, Vec3};

struct Flat;

impl Host for Flat {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
    }
}

fn aimed(aim: Vec3, free: bool) -> Vec3 {
    let mut sim = Sim::new(Params::default(), 5);
    let c = sim.add_captain(Vec3::ZERO);
    let mut host = Flat;
    for _ in 0..3 {
        sim.set_input(c, CaptainInput { cursor: Some(aim), cursor_free: free, ..Default::default() });
        sim.step(&mut host);
    }
    sim.captain(c).cursor_world
}

#[test]
fn a_free_cursor_keeps_the_points_height() {
    let aim = Vec3::new(0.0, 60.0, 120.0);
    assert!(aimed(aim, true).distance(aim) < 0.01, "{:?}", aimed(aim, true));
}

#[test]
fn the_ordinary_aimed_cursor_still_goes_on_the_ground() {
    let at = aimed(Vec3::new(0.0, 60.0, 120.0), false);
    assert!(at.y < 2.0 && (at.z - 120.0).abs() < 0.01, "{at:?}");
}

#[test]
fn past_its_reach_a_free_cursor_goes_on_the_ground_at_the_edge() {
    let at = aimed(Vec3::new(0.0, 60.0, 100_000.0), true);
    assert!(at.y < 2.0 && at.z < 100_000.0, "{at:?}");
}
