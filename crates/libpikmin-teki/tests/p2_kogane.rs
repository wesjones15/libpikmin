//! Pikmin 2's Kogane family (Iridescent Flint Beetle, Iridescent Glint Beetle, Doodlebug): the disc's numbers load,
//! and the beetle hides, appears, scuttles, flips when hit, drops what the decomp says and leaves.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::kogane::{APPEAR, DISAPPEAR, MOVE, PRESS, WAIT};
use libpikmin_teki::p2::{self, DOODLEBUG, FLINT_BEETLE, GLINT_BEETLE};
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

fn assets() -> Option<Assets> {
    let a = Assets { root: Assets::default_dir() };
    p2::is_available(&a.root.join("p2"), FLINT_BEETLE).then_some(a)
}

struct Rig {
    sim: Sim,
    tekis: Tekis,
    id: TekiId,
    player: Navi,
    outs: Vec<Out>,
}

impl Rig {
    fn new(a: &Assets, kind: u8, player_at: Vec3) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        tekis.seed(11);
        let id = tekis.spawn(kind, 3, Vec3::ZERO, 0.0).expect("spawn");
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
    fn run_until(&mut self, max_seconds: f32, mut f: impl FnMut(&Rig) -> bool) -> bool {
        for _ in 0..(max_seconds * 30.0) as u32 {
            if f(self) {
                return true;
            }
            self.tick();
        }
        f(self)
    }
    /// Hits it when it is able to be flipped and runs until it has flipped and recovered (or left).
    fn flip(&mut self) -> bool {
        if !self.run_until(10.0, |r| matches!(r.state(), Some(MOVE | WAIT))) {
            return false;
        }
        self.tekis.damage(self.id, 10.0, None);
        self.tick();
        if self.state() != Some(PRESS) {
            return false;
        }
        self.run_until(5.0, |r| r.state() != Some(PRESS))
    }
}

#[test]
fn the_family_loads_its_numbers_from_the_disc() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    for (kind, scale) in [(FLINT_BEETLE, 0.8), (GLINT_BEETLE, 0.7), (DOODLEBUG, 0.7)] {
        let info = tekis.info(kind).unwrap_or_else(|| panic!("kind {kind} did not load"));
        assert!((info.scale - scale).abs() < 1e-4, "kind {kind} scale {}", info.scale);
        assert_eq!(info.motion_data.len(), 3, "move, wait, damage");
        assert!(info.motion_data.iter().all(|m| m.frames > 1.0));
        assert!(info.run_speed > 100.0 && info.visible_range > 0.0);
        assert!(tekis.has_strategy(kind));
    }
    // The decomp's `ProperParms` values on the disc differ from the code defaults (Flint: appear 20..30, not 15..30).
    let flint = tekis.info(FLINT_BEETLE).unwrap();
    assert_eq!((flint.floats[p2::fp::own::MIN_APPEAR], flint.floats[p2::fp::own::MAX_APPEAR]), (20.0, 30.0));
    // The Damage clip carries the keys the flip reads: the special sound, the item, the end of the lock.
    let damage = &flint.motion_data[2];
    let types: Vec<u8> = damage.info_keys.iter().map(|&(_, t)| t).collect();
    assert_eq!(types, vec![2, 3, 4]);
    assert_eq!((damage.keys[1], damage.keys[2], damage.keys[3]), (5.0, 7.0, 29.0));
}

#[test]
fn it_stays_hidden_until_someone_comes_close() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, FLINT_BEETLE, Vec3::new(0.0, 0.0, 400.0));
    for _ in 0..90 {
        rig.tick();
    }
    let t = rig.tekis.get(rig.id).unwrap();
    assert_eq!(t.state, APPEAR);
    assert!(!t.option(opt::VISIBLE) && !t.option(opt::ATARI));
    // A player within the sight radius (50) brings it up.
    rig.player.pos = Vec3::new(0.0, 0.0, 30.0);
    assert!(rig.run_until(2.0, |r| r.state() == Some(MOVE)));
    let t = rig.tekis.get(rig.id).unwrap();
    assert!(t.option(opt::VISIBLE) && t.option(opt::ATARI));
    // It grows to full size within a fraction of a second and the popping-up sound plays.
    rig.run_until(1.0, |_| false);
    assert!((rig.tekis.get(rig.id).unwrap().fade - 1.0).abs() < 1e-3);
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "TAMAGOMUSHI_APPEAR", .. })));
}

#[test]
fn it_scuttles_about_and_moves_at_the_disc_speed() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, FLINT_BEETLE, Vec3::new(0.0, 0.0, 30.0));
    rig.run_until(3.0, |r| r.state() == Some(MOVE));
    let mut top_speed = 0.0f32;
    let mut states = std::collections::BTreeSet::new();
    let mut last = rig.tekis.get(rig.id).unwrap().pos;
    for _ in 0..(12 * 30) {
        rig.tick();
        let t = rig.tekis.get(rig.id).unwrap();
        states.insert(t.state);
        top_speed = top_speed.max((t.pos - last).with_y(0.0).length() * 30.0);
        last = t.pos;
    }
    assert!(states.contains(&MOVE) && states.contains(&WAIT), "it should alternate: {states:?}");
    assert!((top_speed - 200.0).abs() < 1.0, "walks at fp06 = 200, got {top_speed}");
}

#[test]
fn the_flint_beetle_drops_a_pellet_then_nectar_and_leaves() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, FLINT_BEETLE, Vec3::new(0.0, 0.0, 30.0));
    let pellets = |r: &Rig| r.outs.iter().filter(|o| matches!(o, Out::SpawnPellets { .. })).count();
    let nectar = |r: &Rig| r.outs.iter().filter_map(|o| if let Out::SpawnNectar { count, .. } = o { Some(*count) } else { None }).collect::<Vec<_>>();

    assert!(rig.flip());
    assert_eq!((pellets(&rig), nectar(&rig)), (1, vec![]), "first flip: a number-1 pellet");
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "KOGANE_HIT", .. })));
    assert!(rig.flip());
    assert_eq!(nectar(&rig), vec![2], "second flip: two nectar");
    assert!(rig.flip() || rig.state() == Some(DISAPPEAR) || rig.tekis.get(rig.id).is_none());
    assert_eq!(nectar(&rig), vec![2, 3], "third flip: three nectar");
    // Having given all, it dives away and is gone.
    assert!(rig.run_until(10.0, |r| r.tekis.get(r.id).is_none()), "it should leave after the third flip");
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Died { .. })));
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "TAMAGOMUSHI_DIVE", .. })));
}

#[test]
fn the_glint_beetle_gives_more() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, GLINT_BEETLE, Vec3::new(0.0, 0.0, 30.0));
    assert!(rig.flip());
    let pellets: Vec<_> = rig.outs.iter().filter_map(|o| if let Out::SpawnPellets { number, count, .. } = o { Some((*number, *count)) } else { None }).collect();
    // Three number-5 pellets (`PELLET_NUMBER_FIVE` x 3).
    assert_eq!(pellets, vec![(1, 1), (1, 1), (1, 1)]);
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "OOGANE_HIT", .. })));
}

#[test]
fn an_idle_beetle_dives_away_on_its_own_after_its_time() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, FLINT_BEETLE, Vec3::new(0.0, 0.0, 30.0));
    // `mAppearTimer` starts 0..10 s in and it goes at 30: gone within 35 s, not before 20.
    assert!(!rig.run_until(19.0, |r| r.tekis.get(r.id).is_none()));
    assert!(rig.run_until(16.0, |r| r.tekis.get(r.id).is_none()));
}

#[test]
fn the_doodlebug_gasses_pikmin_but_not_white_ones() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, DOODLEBUG, Vec3::new(0.0, 0.0, 30.0));
    // `Fart::createFartEffect`: the cloud forms `scale * maxAttackRange` (0.7 * 50) behind it as it starts to scuttle,
    // facing +Z; `mAttackRadius` is 20.
    let red = rig.sim.spawn_piki(PikiColor::Red, Vec3::new(0.0, 0.0, -35.0)).unwrap();
    let white = rig.sim.spawn_piki(PikiColor::White, Vec3::new(4.0, 0.0, -35.0)).unwrap();
    let far = rig.sim.spawn_piki(PikiColor::Red, Vec3::new(0.0, 0.0, -300.0)).unwrap();
    assert!(rig.run_until(3.0, |r| r.state() == Some(MOVE)));
    rig.tick();
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "FART_GAS", .. })));
    rig.run_until(1.0, |_| false);
    assert!(!rig.sim.piki(red).is_alive(), "a Red Pikmin in the gas dies");
    assert!(rig.sim.piki(white).is_alive(), "a White Pikmin is immune to gas");
    assert!(rig.sim.piki(far).is_alive(), "a Pikmin out of the cloud is fine");
    // It buzzes while it is out, and the player is not hurt by the gas (`InteractGas::actNavi` is false).
    assert!(rig.outs.iter().any(|o| matches!(o, Out::Sound { name: "FART_BUZZ", .. })));
    assert!(!rig.outs.iter().any(|o| matches!(o, Out::HitNavi { .. })));
}
