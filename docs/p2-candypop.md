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

Enemy projectiles that carry a hurt box and health of their own (`PikminWorld.Shots.cs`) are creatures too: a hook on
`ProjectileController.Start` queues the ones that have a hurt box (no scene search; if the hook cannot be made the scene is searched
once a second instead), and a Pikmin latched to one destroys it. Which RoR2 projectiles are like that was not checked.

## Room for big squads, lock-on, sounds and effects (2026-10-29)

- **Slots:** Pikmin 2's 5 (1 for the Pop) is the core's default; the host sets `Candypop.SlotsPerBulb` (25) and `SlotsPerPop` (5,
  which spits 25 sprouts) through `pik_sim_set_bulb_slots`. Each Pikmin in is still one sprout out; the bud's own colour gives
  its slot back. `Bulb::slots` is in the snapshot (v10).
- **Lock-on:** `Captain::lock_bulb` (`Sim::set_lock_bulb`): the cursor stands in front of the mouth like a pellet's and the
  throw is guided to it. The host's lock points list open buds with room beside the pellets (RT, RB cycles); one lock "thing"
  id covers both (`bulb::LOCK_BASE` plus the bud's id, so `pik_sim_set_lock_pellet`, the input packet and the network need no
  new field). The lock lapses when the bud is full, closing or gone.
- **Sounds** (on the Pikmin 2 output, with the treasures'; `libpikmin-present::bulb::moments`, found from the bud's state between
  frames, so a client hears them too): the Pom has no sound of its own in the decomp (its sounds come from its animation's
  sequence), so these are Pikmin 2's Onion and sprout sounds that mean the same, each verified audible in
  `libsms-audio/tests/pikmin2_bud_sounds.rs`: opening `0x281B`, a Pikmin goes in `0x281D`, closing `0x281A`, the seeds out
  `0x2816`, withering `0x281C`. A bud met for the first time (a stage's worth at once, or a client joining) is not announced.
- **Effects** (stand-ins, no JPA2 emitter, like Louie's light and the treasures' sparkles: one additive glow from each resource's
  texture and colours, tinted with the bud's colour): a puff when a Pikmin goes in (`PID_OnyonPuffKira` 0x103), smoke as it shuts
  (`PID_EnemyDownSmoke` 0x053), the seeds' burst (`PID_OnyonLay_1` 0x101, `createShotEffect`) and the wither (`PID_PonDead` 0x271,
  `createPomDeadEffect`); two twinkles circle the mouth of an open bud that has room, so it can be found.

## Performance

- **Core:** nothing runs per bud but a timer; a thrown Pikmin tests each bud once a tick (a few hundred comparisons at worst); the
  weighing runs every half second, over the fliers that have a need, scanning the Pikmin once each.
- **Native calls:** the host sends a flier's need when it is new, changed or back after a gap (not every tick for every flier), reads
  the buds back once a tick (a few dozen bytes), and finds projectiles by hook, not by searching the scene.
- **View:** a bud is 973 vertices; one is skinned only when a captain is within `bulb::DRAW_DISTANCE` (3000 sim units, 120 m), and
  its twinkles, bursts and sounds follow. Five models for the five colours are built once at load (about 5000 vertices and five
  small bakes); the colour copies of the model are written once into the ignored cache. A bud that is standing open is posed on
  the same last frame every tick; that is the avoidable cost left (a cached skin would save about 1000 vertices a bud a frame).
- **Network:** a bud is about 45 bytes in each snapshot, only while buds exist; events are a few bytes.
- **Not measured in the game.** The plugin's `OlimarPerformance` meter (Gather, Draw) will show it if there is a cost.

## Verified and not

Tested: the bud's rules and timings, the room for more Pikmin, the lock-on (cursor, throw, lapse), the snapshot round trip, the
weights and thresholds, the ABI, the baked colours differing, the model animating, the moments of a bud, the effects drawing in
the bud's colour, the draw distance, the borrowed sounds being audible, the C# boundary. Never seen or played: how the buds look
and stand on real stages, whether a thrown Pikmin finds the mouth, the placement on each map, the sprouts' flight from a bud, the
Purple bud's near-black petals, how the borrowed sounds sit with the rest, how the stand-in effects look, which enemies the hull
rule calls small or large, a projectile ever being found, the weight hold feeling right, the lock-on picking a bud in the
camera's view.
