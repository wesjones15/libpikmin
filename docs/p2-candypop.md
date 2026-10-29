# Candypop Buds and weighing fliers down

Added 2026-10-28. Built, tested headless and staged; **never played in RoR2**.

## Candypop Buds (`libpikmin::bulb`)

Spec: Pikmin 2's `Game::Pom` (`plugProjectNishimuraU/Pom.cpp`, `PomState.cpp`, parms in `Pom.h`).

- A bud stands open. A thrown Pikmin that comes down on its mouth is swallowed into one of its slots (5; the rainbow
  Candypop Pop has 1). It waits 30 s after the last Pikmin (`mRemainOpenTime`), or shuts at once when full, then spits one
  leaf sprout of its colour per Pikmin swallowed (the Pop spits 5 per Pikmin), thrown up at (110, 750, 110) and taking root
  (`Obj::shotPikmin`). A Pikmin of the bud's own colour gives its slot back; any other uses it up, and a bud with no slots
  left withers after it spits. The Pop turns Blue, Red, Yellow every 1.25 s while it waits.
- Phase lengths are the disc's clips (`enemy/data/Pom/anim`: `type1` open, key 2 at frame 25 lets it swallow; `type2` close,
  30 frames; `type3` spit, 40 frames, key 2 at frame 20 throws the seeds; `dead` 40 frames); a test reads them from the cache.
- Host adaptation: "presses the `slot` part" is a thrown Pikmin coming down within 18 units of the bud's foot and under 46.
- `Sim::add_bulb(colour, queen, pos)`, `bulbs()`, `remove_bulb`; events `BulbSwallowed`, `BulbShot`; in the snapshot (v9), so
  clients draw them. FFI: `pik_sim_add_bulb` / `remove_bulb` / `bulbs` (`PikBulb`, 36 bytes), `pik_view_load_bulbs`.
- Drawing (`libpikmin-present::bulb`): the Pom model; the petal material `hanabira1_v` has TEV colour 0 set to the bud's
  colour at run time in the game (Blue 50,50,255, Red 255,20,20, Yellow 255,255,20, Purple 28,0,52, White 200,255,220). Here
  one model per colour is baked from a copy of `enemy.bmd` with that colour table entry rewritten (`enemy_c<n>.bmd`, written
  once beside the model in the ignored cache). Needs the Pikmin 2 enemy cache (`pik_p2_enemies_extract`).

## Host (ror2-olimar, `PikminWorld.Bulbs.cs`)

- Every skin's Onion now takes all five colours (was Louie's only); every skin starts with Red, Yellow and Blue only (Louie
  keeps his Purple and White starts). Without Pikmin 2's files only the three.
- Each stage the server scatters buds over ground-graph nodes at least `Candypop.BulbSpacingMetres` from the player and each
  other: `Candypop.BulbsShipRepair` (default 14) in Ship Repair, `Candypop.BulbsOther` (default 3) elsewhere. A stage with six
  or more has each colour once, in a shuffled order; the rest follow Pikmin 2's odds (Purple and White 15% together) and one
  pick in ten is a Pop.

## Weighing fliers down (`libpikmin::weigh`)

A host rule applied evenly (Pikmin 2's decomp has only the Purple pound, `Event::Grounded`, which still works):
the Pikmin latched to a flying creature weigh it down, a Purple counting as five.

| flier | Pikmin | Purple |
|---|---|---|
| small | 5 | 1 |
| large | 15 | 3 |
| boss | 25 | 5 |
| shot that can be shot down | 1 | 1 |

`pik_sim_set_ground_need(creature, need)` per flier (the host classifies: a boss or champion is `boss`, a Human-hull flier
`small`, other hulls `large`; `pik_weigh_need(class)` gives the numbers). The sim checks every half second and raises
`WeighedDown`; the host holds the enemy to the ground for `Pikmin.WeighGroundSeconds` (1.5 s) after each (the same stun and
pull as the pound). `Pikmin.WeighDownFliers` turns it off.

Enemy projectiles that carry a hurt box and health of their own (`PikminWorld.Shots.cs`) are creatures too: found every
quarter second, a Pikmin latched to one destroys it. Which RoR2 projectiles are like that was not checked.

## Verified and not

Tested: the bud's rules and timings, the snapshot round trip, the weights and thresholds, the ABI, the baked colours differing,
the model animating, the C# boundary. Never seen or played: how the buds look and stand on real stages, whether a thrown
Pikmin finds the mouth, the placement on each map, the sprouts' flight from a bud, the Purple bud's near-black petals,
which enemies the hull rule calls small or large, a projectile ever being found, the weight hold feeling right, any sound
or effect for a bud (none made).
