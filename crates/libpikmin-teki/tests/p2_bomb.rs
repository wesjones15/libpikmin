//! Pikmin 2's Bomb-rock: it lies still until hit, burns the disc's 4.5 second fuse, then blows up the players and Pikmin
//! near it and is gone.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::bomb::{BOMB, WAIT};
use libpikmin_teki::p2::{self, BOMB_ROCK};
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
    p2::is_available(&a.root.join("p2"), BOMB_ROCK).then_some(a)
}

struct Rig {
    sim: Sim,
    tekis: Tekis,
    id: TekiId,
    navis: Vec<Navi>,
    outs: Vec<Out>,
}

impl Rig {
    fn new(a: &Assets) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        let id = tekis.spawn(BOMB_ROCK, 3, Vec3::ZERO, 0.0).expect("spawn");
        let navis = vec![
            Navi { id: 1, pos: Vec3::new(40.0, 0.0, 0.0), size: 8.0, alive: true },
            Navi { id: 2, pos: Vec3::new(400.0, 0.0, 0.0), size: 8.0, alive: true },
        ];
        Rig { sim, tekis, id, navis, outs: Vec::new() }
    }
    fn tick(&mut self) {
        let mut frame = CreatureFrame::default();
        self.tekis.set_ground(self.id, 0.0, false);
        self.tekis.frame(&mut frame);
        self.sim.step(&mut Frame(&frame));
        let out = self.tekis.update(&mut self.sim, &FlatHost, &self.navis, 1.0 / 30.0);
        self.outs.extend(out);
    }
    fn run(&mut self, seconds: f32) {
        for _ in 0..(seconds * 30.0) as u32 {
            self.tick();
        }
    }
}

#[test]
fn it_waits_until_hit_then_burns_its_fuse_and_blows_up_what_is_near() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a);
    assert_eq!(rig.tekis.info(BOMB_ROCK).unwrap().life, 4.5);
    let near = rig.sim.spawn_piki(PikiColor::Red, Vec3::new(-30.0, 0.0, 0.0)).unwrap();
    let far = rig.sim.spawn_piki(PikiColor::Red, Vec3::new(0.0, 0.0, 300.0)).unwrap();
    rig.run(10.0);
    assert_eq!(rig.tekis.get(rig.id).unwrap().state, WAIT, "unlit it lies still");
    assert!(rig.outs.is_empty());
    rig.tekis.damage(rig.id, 5.0, None);
    rig.run(1.0);
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "BOMB_START", .. })));
    rig.run(2.0);
    assert!(rig.tekis.get(rig.id).is_some(), "the fuse is still burning at 3 seconds");
    rig.run(3.0);
    assert!(rig.tekis.get(rig.id).is_none(), "gone after the blast");
    assert_eq!(rig.tekis.list.len(), 0);
    let hit: Vec<u32> = rig.outs.iter().filter_map(|o| if let Out::HitNavi { navi, .. } = o { Some(*navi) } else { None }).collect();
    assert_eq!(hit, vec![1], "the near player is hit, the far one is not");
    assert!(!rig.sim.piki(near).is_alive() && rig.sim.piki(far).is_alive());
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "PK_SE_BOMB", .. })));
    let _ = BOMB;
}
