//! Pikmin 2's gas pipe: it vents at once on the disc's numbers, kills Pikmin in the cloud but not White ones or far ones,
//! never hurts a player, and is stopped for good when its health is gone.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::gashiba::{ATTACK, DEAD};
use libpikmin_teki::p2::{self, GAS_PIPE};
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
    p2::is_available(&a.root.join("p2"), GAS_PIPE).then_some(a)
}

struct Rig {
    sim: Sim,
    tekis: Tekis,
    id: TekiId,
    outs: Vec<Out>,
}

impl Rig {
    fn new(a: &Assets) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        let id = tekis.spawn(GAS_PIPE, 3, Vec3::ZERO, 0.0).expect("spawn");
        Rig { sim, tekis, id, outs: Vec::new() }
    }
    fn tick(&mut self) {
        let mut frame = CreatureFrame::default();
        self.tekis.set_ground(self.id, 0.0, false);
        self.tekis.frame(&mut frame);
        self.sim.step(&mut Frame(&frame));
        let player = Navi { id: 5, pos: Vec3::new(10.0, 0.0, 10.0), size: 8.0, alive: true };
        let out = self.tekis.update(&mut self.sim, &FlatHost, &[player], 1.0 / 30.0);
        self.outs.extend(out);
    }
    fn run(&mut self, seconds: f32) {
        for _ in 0..(seconds * 30.0) as u32 {
            self.tick();
        }
    }
}

#[test]
fn it_vents_gas_that_kills_pikmin_but_not_white_ones_or_players() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a);
    let info = rig.tekis.info(GAS_PIPE).unwrap();
    assert_eq!(info.life, 150.0);
    assert_eq!(info.floats[p2::fp::ATTACK_RADIUS], 60.0);
    let red = rig.sim.spawn_piki(PikiColor::Red, Vec3::new(30.0, 0.0, 0.0)).unwrap();
    let white = rig.sim.spawn_piki(PikiColor::White, Vec3::new(-30.0, 0.0, 0.0)).unwrap();
    let far = rig.sim.spawn_piki(PikiColor::Blue, Vec3::new(0.0, 0.0, 200.0)).unwrap();
    rig.run(0.2);
    assert_eq!(rig.tekis.get(rig.id).unwrap().state, ATTACK, "the disc's wait time is zero: it vents at once");
    rig.run(2.0);
    assert!(!rig.sim.piki(red).is_alive());
    assert!(rig.sim.piki(white).is_alive() && rig.sim.piki(far).is_alive());
    assert!(!rig.outs.iter().any(|o| matches!(o, Out::HitNavi { .. })));
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "GAS_HIBA_VOMIT", .. })));
}

#[test]
fn it_stops_for_good_when_wrecked() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a);
    rig.run(1.0);
    rig.tekis.damage(rig.id, 1.0e6, None);
    rig.run(3.0);
    assert_eq!(rig.tekis.get(rig.id).unwrap().state, DEAD);
    let victim = rig.sim.spawn_piki(PikiColor::Red, Vec3::new(20.0, 0.0, 0.0)).unwrap();
    rig.run(2.0);
    assert!(rig.sim.piki(victim).is_alive(), "no more gas");
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "HIBA_STOP", .. })));
}
