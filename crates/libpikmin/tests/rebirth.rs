use libpikmin::host::FlatHost;
use libpikmin::{PikiColor, Rebirth, Sim, Vec3};

fn kill_many(sim: &mut Sim, n: usize) {
    let ids: Vec<_> = (0..n)
        .map(|i| sim.spawn_piki(PikiColor::Red, Vec3::X * (i as f32 * 3.0)).unwrap())
        .collect();
    for id in ids {
        sim.kill_piki(id);
    }
    for _ in 0..120 {
        sim.step(&mut FlatHost);
    }
}

const RB: Rebirth = Rebirth { chance: 0.6, burst_chance: 0.8, burst_count: 5, window: 1.0 };

#[test]
fn off_by_default_leaves_no_sprouts_for_leaf_pikmin() {
    let mut sim = Sim::default();
    kill_many(&mut sim, 30);
    assert_eq!(sim.sprouts().count(), 0);
}

#[test]
fn mass_death_comes_back_mostly_as_sprouts() {
    let mut sim = Sim::default();
    sim.set_rebirth(Some(RB));
    kill_many(&mut sim, 60);
    let n = sim.sprouts().count();
    // ~80% of 60 once the mass window is reached; far above the 60% base and the decomp's 0%.
    assert!((36..=56).contains(&n), "sprouts = {n}");
}

#[test]
fn lone_deaths_use_the_base_chance() {
    let mut sim = Sim::default();
    sim.set_rebirth(Some(RB));
    for _ in 0..80 {
        kill_many(&mut sim, 1);
        for _ in 0..60 { sim.step(&mut FlatHost); }
        sim.reclaim_dead_pikis(std::iter::empty());
    }
    let n = sim.sprouts().count();
    assert!((34..=62).contains(&n), "sprouts = {n}");
}

#[test]
fn certain_and_impossible_chances_are_exact() {
    let mut sim = Sim::default();
    sim.set_rebirth(Some(Rebirth { chance: 1.0, burst_chance: 1.0, burst_count: 5, window: 1.0 }));
    kill_many(&mut sim, 20);
    assert_eq!(sim.sprouts().count(), 20);
    let mut sim = Sim::default();
    sim.set_rebirth(Some(Rebirth { chance: 0.0, burst_chance: 0.0, burst_count: 5, window: 1.0 }));
    kill_many(&mut sim, 20);
    assert_eq!(sim.sprouts().count(), 0);
}
