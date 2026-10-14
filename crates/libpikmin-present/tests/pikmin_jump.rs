//! Olimar's jump can play the Pikmin's own jump motion instead of the geyser ride. One test in
//! its own file, since the setting is process-wide.

use libpikmin::host::FlatHost;
use libpikmin::{CaptainInput, Params, Sim, TICK_DT, Vec3};
use libpikmin_assets::{motion::Motion, Assets};
use libpikmin_present::animator::{captain_key, set_captain_pikmin_jump};
use libpikmin_present::View;

#[test]
fn olimar_jumps_like_a_pikmin_when_asked_and_with_the_geyser_ride_otherwise() {
    let Some(assets) = Assets::discover(None) else { return };
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::new(Params::default(), 35);
    let c = sim.add_captain(Vec3::ZERO);
    sim.step(&mut FlatHost);
    view.update(&sim, 1.0, TICK_DT);
    sim.set_input(c, CaptainInput { jump: true, ..Default::default() });
    for want in [(true, Motion::RollJmp), (false, Motion::OCarry)] {
        set_captain_pikmin_jump(want.0);
        for _ in 0..4 {
            sim.step(&mut FlatHost);
            view.update(&sim, 1.0, TICK_DT);
        }
        assert_eq!(view.animator.players[&captain_key(c)].anim, want.1.anim_index().unwrap(), "pikmin jump {}", want.0);
    }
}
