//! The Fiery Blowhog: `TAItankStrategy` (TAItank.cpp). It patrols a loop around its nest; seeing something it
//! stops, turns to face it, backs off if it is too near, breathes fire along the stream from its mouth, and
//! shakes off what climbs on it. Too far from home it walks back.

use glam::Vec3;

use crate::bulborb::*;
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use crate::yai::*;

// TAItankFloatParams
const STEP_BACK_DISTANCE: usize = 50;
const FIRE_ANIM_SPEED: usize = 51;
const STEP_BACK_SPEED: usize = 52;
const PATROL_TIMEOUT: usize = 53;

/// `TAIAinitTank`: its collision circle is wide.
fn init(next: i32) -> Act {
    Action::new(next).on_start(|t, _| t.coll_radius = 48.0).on_act(|_, _| true).build()
}

/// `TaiState` ids are `TAItankStateID::Unk*`.
pub fn tank(_info: &TekiInfo) -> Strategy {
    let patrol_points = vec![Vec3::new(0.0, 0.0, 0.5), Vec3::new(0.5, 0.0, -0.5), Vec3::new(-0.5, 0.0, -0.5)];
    let dead_check = dead(0);
    let damage = yai_damage(true);
    let stop = stop_move();
    let patrol = patrol(MOVE1, WAITACT1, WAITACT1, patrol_points, PATROL_TIMEOUT);
    let visible_piki1 = visible_piki(3);
    let visible_navi1 = visible_navi(3);
    let flick_check = flick_check(9, -1);
    let eff = eff_cloud();

    let mut s = Strategy::new(12, 1);
    s.set(1, vec![init(2)]);
    s.set(0, vec![yai_dying(DEAD), stop.clone()]);
    s.set(2, vec![dead_check.clone(), damage.clone(), patrol, visible_navi1, visible_piki1, flick_check.clone(), eff.clone()]);
    s.set(3, vec![dead_check.clone(), damage.clone(), flick_check.clone(), wait(5, WAIT1, 0.5), eff.clone()]);
    s.set(4, vec![dead_check.clone(), damage.clone(), wait(2, WAITACT2, 0.0)]);
    s.set(5, vec![
        dead_check.clone(), damage.clone(), flick_check.clone(), attackable_target(6, false), turn_focus(7, WAITACT1, WAITACT1, true),
        unvisible_target(4), eff.clone(),
    ]);
    s.set(6, vec![
        dead_check.clone(), damage.clone(), outside_danger_territory(11), flick_check.clone(), step_back(7, MOVE2, STEP_BACK_SPEED),
        outside_range(7, STEP_BACK_DISTANCE), unvisible_target(4), eff.clone(),
    ]);
    s.set(7, vec![eff.clone(), dead_check.clone(), damage.clone(), fire_breath(8, ATTACK, FIRE_ANIM_SPEED), stop.clone()]);
    s.set(8, vec![visible_piki(5), visible_navi(5), no_reaction(2)]);
    s.set(9, vec![dead_check.clone(), damage.clone(), yai_flicking(7, FLICK, true), eff.clone()]);
    s.set(10, vec![dead_check.clone(), damage.clone(), outside_danger_territory(11), random_walk(11, TYPE1), eff.clone()]);
    s.set(11, vec![dead_check, damage, going_home_face(2, TYPE1), eff]);
    s
}
