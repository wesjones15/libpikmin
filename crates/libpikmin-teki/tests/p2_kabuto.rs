//! Pikmin 2's Armored Cannon Beetle Larva: the disc's numbers and corpse load, it wanders, turns on a player in sight, spits
//! a boulder along a line at him, and dies with the dying notice.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::info::{IWAGON, slot};
use libpikmin_teki::kabuto::{ATTACK, MOVE, TURN};
use libpikmin_teki::p2::{self, CANNON_LARVA};
use libpikmin_teki::{Navi, Out, TekiId, Tekis};

struct Frame<'a>(&'a CreatureFrame);

impl Host for Frame<'_> {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        Some(0.0)
    }
    fn creatures(&mut self, out: &mut CreatureFrame) {
        *out = self.0.clone();
    }
}

fn assets() -> Option<Assets> {
    let a = Assets { root: Assets::default_dir() };
    p2::is_available(&a.root.join("p2"), CANNON_LARVA).then_some(a)
}

struct Rig {
    sim: Sim,
    tekis: Tekis,
    id: TekiId,
    player: Navi,
    outs: Vec<Out>,
}

impl Rig {
    fn new(a: &Assets, player_at: Vec3) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        tekis.seed(4);
        let id = tekis.spawn(CANNON_LARVA, 3, Vec3::ZERO, 0.0).expect("spawn");
        Rig { sim, tekis, id, player: Navi { id: 5, pos: player_at, size: 8.0, alive: true }, outs: Vec::new() }
    }
    fn tick(&mut self) {
        let mut frame = CreatureFrame::default();
        self.tekis.set_ground(self.id, 0.0, false);
        self.tekis.frame(&mut frame);
        self.sim.step(&mut Frame(&frame));
        let out = self.tekis.update(&mut self.sim, &FlatHost, &[self.player], 1.0 / 30.0);
        self.outs.extend(out);
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

#[test]
fn it_loads_its_numbers_and_corpse() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let info = tekis.info(CANNON_LARVA).expect("loaded");
    assert_eq!((info.life, info.run_speed, info.visible_range), (850.0, 60.0, 350.0));
    assert_eq!(&info.clips[..6], &["dead", "move", "flick", "attack", "pivot", "wait"]);
    assert_eq!((info.corpse_size, info.corpse_min_carriers, info.corpse_max_carriers), (34.0, 7, 15));
    assert!(info.motions[slot::DEAD].end > 1.0);
}

#[test]
fn it_wanders_then_faces_a_player_and_spits_a_boulder_down_the_line() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 1500.0));
    assert!(rig.run_until(15.0, |r| r.state() == Some(MOVE)), "it wanders with nobody near");
    rig.player.pos = Vec3::new(120.0, 0.0, 120.0);
    assert!(rig.run_until(20.0, |r| r.state() == Some(TURN) || r.state() == Some(ATTACK)));
    assert!(rig.run_until(30.0, |r| r.outs.iter().any(|o| matches!(o, Out::Spawn { kind: IWAGON, .. }))), "a boulder leaves its mouth");
    let v = rig.outs.iter().find_map(|o| if let Out::Spawn { velocity, .. } = o { Some(*velocity) } else { None }).unwrap();
    assert!(v.length() > 100.0);
    let _ = ATTACK;
}

#[test]
fn it_dies_with_the_dying_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 1500.0));
    rig.run_until(2.0, |_| false);
    rig.tekis.damage(rig.id, 1.0e6, None);
    assert!(rig.run_until(8.0, |r| r.tekis.get(r.id).is_none()));
    let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
    let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
    assert!(dying.is_some() && died.is_some() && dying < died);
}
