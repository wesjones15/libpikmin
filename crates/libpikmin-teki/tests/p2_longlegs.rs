//! Pikmin 2's Long Legs (Beady Long Legs, Raging Long Legs, Man-at-Legs): the disc's numbers and the model's rig load, each
//! waits unseen until someone is near, lands, walks on its four legs (feet stepping in turn, the body following them), crushes
//! what its feet come down on, shakes Pikmin off, and dies with the dying notice; the Man-at-Legs also shoots.

use glam::{Mat4, Vec3};
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::legs::{LEGS, SWINGING};
use libpikmin_teki::longlegs::{FLICK, LAND, STAY, WAIT, WALK};
use libpikmin_teki::p2::{self, BEADY_LONG_LEGS, MAN_AT_LEGS, RAGING_LONG_LEGS};
use libpikmin_teki::{Navi, Out, TekiId, Tekis, opt};

struct Frame<'a>(&'a CreatureFrame);

impl Host for Frame<'_> {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        Some(0.0)
    }
    fn creatures(&mut self, out: &mut CreatureFrame) {
        *out = self.0.clone();
    }
}

fn assets(kind: u8) -> Option<Assets> {
    let a = Assets { root: Assets::default_dir() };
    p2::is_available(&a.root.join("p2"), kind).then_some(a)
}

struct Rig {
    sim: Sim,
    tekis: Tekis,
    id: TekiId,
    player: Navi,
    outs: Vec<Out>,
    seen: std::collections::BTreeSet<i32>,
}

impl Rig {
    fn new(a: &Assets, kind: u8, player_at: Vec3) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        tekis.seed(10);
        let id = tekis.spawn(kind, 3, Vec3::ZERO, 0.0).expect("spawn");
        Rig { sim, tekis, id, player: Navi { id: 5, pos: player_at, size: 8.0, alive: true }, outs: Vec::new(), seen: Default::default() }
    }
    fn tick(&mut self) {
        let mut frame = CreatureFrame::default();
        self.tekis.set_ground(self.id, 0.0, false);
        self.tekis.frame(&mut frame);
        self.sim.step(&mut Frame(&frame));
        let out = self.tekis.update(&mut self.sim, &FlatHost, &[self.player], 1.0 / 30.0);
        self.outs.extend(out);
        if let Some(t) = self.tekis.get(self.id) {
            self.seen.insert(t.state);
        }
    }
    fn state(&self) -> Option<i32> {
        self.tekis.get(self.id).map(|t| t.state)
    }
    fn run_until(&mut self, seconds: f32, mut f: impl FnMut(&Rig) -> bool) -> bool {
        for _ in 0..(seconds * 30.0) as u32 {
            if f(self) {
                return true;
            }
            self.tick();
        }
        f(self)
    }
}

const KINDS: [u8; 3] = [BEADY_LONG_LEGS, RAGING_LONG_LEGS, MAN_AT_LEGS];

#[test]
fn they_load_the_discs_numbers_and_the_models_leg_rig() {
    for kind in KINDS {
        let Some(a) = assets(kind) else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
        let mut tekis = Tekis::load(&a);
        assert!(tekis.has_strategy(kind), "{kind}: the rig loaded, so there is a strategy");
        let info = tekis.info(kind).expect("loaded").clone();
        assert!(info.life >= 1300.0, "{kind}: {}", info.life);
        assert_eq!(&info.clips[..4], &["dead", "landing", "wait", "flick"]);
        let id = tekis.spawn(kind, 1, Vec3::ZERO, 0.0).unwrap();
        let legs = tekis.get(id).unwrap().legs.as_ref().expect("legs");
        // Four legs, hips high, feet low, segments of the length the model gives.
        for i in 0..LEGS {
            assert!(legs.rig.hips[i].y > 100.0 && legs.rig.feet[i].y < 20.0, "{kind} leg {i}: {:?} {:?}", legs.rig.hips[i], legs.rig.feet[i]);
            assert!(legs.rig.top_to_middle[i] > 50.0 && legs.rig.middle_to_bottom[i] > 50.0);
        }
        // Left and right: the rig is symmetric.
        assert!((legs.rig.feet[0].x + legs.rig.feet[1].x).abs() < 1.0);
        // Only the first two have foot spheres.
        assert_eq!(legs.rig.foot_radius > 0.0, kind != MAN_AT_LEGS, "{kind}");
        assert_eq!(legs.rig.gun.is_some(), kind == MAN_AT_LEGS);
    }
}

#[test]
fn the_solver_puts_the_knees_where_the_model_has_them_when_the_feet_stand_where_the_model_does() {
    use libpikmin_teki::ik::{LegInput, solve_leg};
    for kind in KINDS {
        let Some(a) = assets(kind) else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
        let k = p2::kind(kind).unwrap();
        let base = a.root.join("p2/enemy/data").join(k.data);
        let bmd = std::fs::read(base.join("model/enemy.bmd")).unwrap();
        let sk = libsms::Skeleton::from_bmd(&bmd).unwrap();
        let clip = libsms::Clip::parse_bca(&std::fs::read(base.join("anim/landing.bca")).unwrap()).unwrap();
        let pose: Vec<Mat4> = sk.pose(&clip, (clip.frames - 1.0).max(0.0));
        let rig = libpikmin_teki::legs::LegRig::from_pose(&sk.names, &pose).unwrap();
        for i in 0..LEGS {
            let [h, m, f] = rig.joints[i];
            let knee = solve_leg(&LegInput {
                top: pose[h].w_axis.truncate(),
                foot: pose[f].w_axis.truncate(),
                top_to_middle: rig.top_to_middle[i],
                middle_to_bottom: rig.middle_to_bottom[i],
                bend: 0.0,
                blend: None,
            })
            .middle
            .w_axis
            .truncate();
            let model_knee = pose[m].w_axis.truncate();
            assert!((knee - model_knee).length() < 3.0, "{kind} leg {i}: solved {knee} model {model_knee}");
        }
    }
}

#[test]
fn damagumo_waits_unseen_until_someone_is_near_then_lands_and_stands() {
    let Some(a) = assets(BEADY_LONG_LEGS) else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, BEADY_LONG_LEGS, Vec3::new(0.0, 0.0, 2000.0));
    for _ in 0..90 {
        rig.tick();
    }
    let t = rig.tekis.get(rig.id).unwrap();
    assert_eq!(t.state, STAY);
    assert!(!t.option(opt::VISIBLE) && !t.option(opt::ATARI), "unseen and untouchable");
    rig.player.pos = Vec3::new(0.0, 0.0, 300.0);
    assert!(rig.run_until(3.0, |r| r.state() == Some(LAND)));
    let t = rig.tekis.get(rig.id).unwrap();
    assert!(t.option(opt::VISIBLE) && t.option(opt::ATARI));
    assert!(rig.run_until(10.0, |r| r.state() == Some(WAIT)), "{:?}", rig.seen);
    assert!(rig.tekis.get(rig.id).unwrap().legs.as_ref().unwrap().active, "its legs stand on their own feet");
}

fn stand_and_walk(kind: u8) {
    let Some(a) = assets(kind) else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, kind, Vec3::new(0.0, 0.0, 300.0));
    assert!(rig.run_until(40.0, |r| r.state() == Some(WALK)), "{kind}: {:?}", rig.seen);
    // The player stands off ahead: it walks after him, its feet lifting in turn.
    rig.player.pos = Vec3::new(0.0, 0.0, 330.0);
    let start = rig.tekis.get(rig.id).unwrap().pos;
    let mut lifts = Vec::new();
    let mut last = [0i32; LEGS];
    for _ in 0..(20.0 * 30.0) as u32 {
        rig.tick();
        let Some(t) = rig.tekis.get(rig.id) else { break };
        if let Some(l) = t.legs.as_ref() {
            for i in 0..LEGS {
                if l.states[i] == SWINGING && last[i] != SWINGING {
                    lifts.push(i);
                }
                last[i] = l.states[i];
            }
        }
    }
    assert!(lifts.len() >= 4, "{kind}: {lifts:?} {:?}", rig.seen);
    // Each leg lifts after the one before it, round after round.
    for w in lifts.windows(2) {
        assert_eq!(w[1], (w[0] + 1) % LEGS, "{kind}: {lifts:?}");
    }
    let t = rig.tekis.get(rig.id).unwrap();
    assert!(t.pos.distance(start) > 20.0, "{kind} walked from {start} to {}", t.pos);
    // Its feet stay near its middle and on the ground.
    let l = t.legs.as_ref().unwrap();
    for i in 0..LEGS {
        assert!(l.legs[i].foot.with_y(0.0).distance(t.pos.with_y(0.0)) < 400.0, "{kind} leg {i}");
        assert!(l.legs[i].foot.y >= -0.01);
    }
}

#[test]
fn damagumo_stands_up_and_walks_on_its_four_legs() {
    stand_and_walk(BEADY_LONG_LEGS);
}

#[test]
fn bigfoot_stands_up_and_walks_on_its_four_legs() {
    stand_and_walk(RAGING_LONG_LEGS);
}

#[test]
fn houdai_stands_up_and_walks_on_its_four_legs() {
    stand_and_walk(MAN_AT_LEGS);
}

#[test]
fn a_foot_coming_down_crushes_a_player_under_it() {
    let Some(a) = assets(BEADY_LONG_LEGS) else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, BEADY_LONG_LEGS, Vec3::new(0.0, 0.0, 300.0));
    assert!(rig.run_until(40.0, |r| r.state() == Some(WALK)));
    // Wait for a foot to be coming down, then put the player right under it.
    let mut hit = false;
    for _ in 0..(40.0 * 30.0) as u32 {
        rig.tick();
        let t = rig.tekis.get(rig.id).unwrap();
        let l = t.legs.as_ref().unwrap();
        if let Some(i) = (0..LEGS).find(|&i| l.stomping(i) && !l.stomped[i]) {
            rig.player.pos = l.legs[i].foot.with_y(0.0);
        }
        if rig.outs.iter().any(|o| matches!(o, Out::HitNavi { .. })) {
            hit = true;
            break;
        }
    }
    assert!(hit, "{:?}", rig.seen);
}

#[test]
fn it_shakes_pikmin_off_after_enough_hits_and_goes_back_to_walking() {
    let Some(a) = assets(BEADY_LONG_LEGS) else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, BEADY_LONG_LEGS, Vec3::new(0.0, 0.0, 300.0));
    assert!(rig.run_until(40.0, |r| r.state() == Some(WAIT) || r.state() == Some(WALK)));
    for _ in 0..40 {
        rig.tekis.damage(rig.id, 1.0, None);
    }
    assert!(rig.run_until(15.0, |r| r.state() == Some(FLICK)), "{:?}", rig.seen);
    assert!(rig.run_until(15.0, |r| r.state() == Some(WALK) || r.state() == Some(WAIT)), "back to its business: {:?}", rig.seen);
}

#[test]
fn raging_long_legs_rages_after_being_shaken() {
    let Some(a) = assets(RAGING_LONG_LEGS) else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, RAGING_LONG_LEGS, Vec3::new(0.0, 0.0, 300.0));
    assert!(rig.run_until(40.0, |r| r.state() == Some(WAIT) || r.state() == Some(WALK)));
    for _ in 0..200 {
        rig.tekis.damage(rig.id, 1.0, None);
    }
    assert!(rig.run_until(20.0, |r| r.state() == Some(FLICK)), "{:?}", rig.seen);
    assert!(rig.run_until(20.0, |r| r.state() == Some(WALK)));
    assert!(rig.tekis.get(rig.id).unwrap().flags[0], "enraged for the next walk");
    assert!(rig.run_until(40.0, |r| r.state() == Some(WAIT)), "{:?}", rig.seen);
    assert!(!rig.tekis.get(rig.id).unwrap().flags[0], "calm again after it");
}

#[test]
fn they_die_with_the_dying_notice() {
    for kind in KINDS {
        let Some(a) = assets(kind) else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
        let mut rig = Rig::new(&a, kind, Vec3::new(0.0, 0.0, 300.0));
        assert!(rig.run_until(40.0, |r| r.state() == Some(WAIT) || r.state() == Some(WALK)));
        rig.tekis.damage(rig.id, 1.0e6, None);
        assert!(rig.run_until(40.0, |r| r.tekis.get(r.id).is_none()), "{kind}: {:?}", rig.seen);
        let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
        let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
        assert!(dying.is_some() && died.is_some() && dying < died, "{kind}");
    }
}

#[test]
fn man_at_legs_opens_his_gun_aims_and_fires_shells() {
    let Some(a) = assets(MAN_AT_LEGS) else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, MAN_AT_LEGS, Vec3::new(0.0, 0.0, 300.0));
    assert!(rig.run_until(40.0, |r| r.state() == Some(WAIT) || r.state() == Some(WALK)));
    // Shaken, he goes straight to the gun.
    for _ in 0..40 {
        rig.tekis.damage(rig.id, 1.0, None);
    }
    assert!(rig.run_until(20.0, |r| r.state() == Some(libpikmin_teki::houdai::SHOT)), "{:?}", rig.seen);
    rig.player.pos = Vec3::new(0.0, 0.0, 350.0);
    let mut shells = 0;
    for _ in 0..(25.0 * 30.0) as u32 {
        rig.tick();
        let Some(t) = rig.tekis.get(rig.id) else { break };
        shells = shells.max(t.legs.as_ref().map_or(0, |l| l.gun.shells.len()));
        if t.state != libpikmin_teki::houdai::SHOT && shells > 0 {
            break;
        }
    }
    assert!(shells > 0, "he fired: {:?}", rig.seen);
    let sounds = rig.outs.iter().filter(|o| matches!(o, Out::Sound { name: "HOUDAI_SHOT", .. })).count();
    assert!(sounds >= 3, "a burst: {sounds}");
    assert!(rig.run_until(30.0, |r| r.state() == Some(WALK)), "he folds the gun away and walks: {:?}", rig.seen);
}
