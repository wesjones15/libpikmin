# Pikmin 2's new enemies: survey

Written 2026-10-28. Facts are scanned from the decomp (`pikmin2-main/`, read-only) and the disc file list of `Pikmin 2 (USA).iso`; the
classification and the order are judgement. Nothing here is Nintendo data.

## How an enemy is built in Pikmin 2 (versus Pikmin 1)

- Pikmin 1's enemies are `Tai*`/`Yai*` strategies over an `.key` animation manager (what `libpikmin-teki` ports). Pikmin 2's are
  `Game::<Name>::Obj : EnemyBase` classes (`include/Game/Entities/<Name>.h`) with an `FSM` of `State` objects (`<Name>State.cpp`), a
  `Mgr` (`<Name>Mgr.cpp`, resources and the enemy's parm class), an `Animator` (`<Name>Animator.cpp`) and parms in code
  (`Parms`, defaults) overridden by the disc's `enemy/parm/enemyParms.szs`. The shared helpers are in `plugProjectYamashitaU/enemyBase.cpp`,
  `enemyFSM.cpp`, `enemyMgrBase.cpp` and `Game/EnemyFunc.h`.
- Files per enemy (disc): `enemy/data/<Name>/model.szs` (RARC with `enemy.bmd`), `anim.szs` (RARC of `.bca` clips named after the
  state, e.g. `wait1.bca`, `move1.bca`, `dead.bca`, `type1..5.bca`), and loose `.bti`/`.brk`/`.btk` (texture swaps and colour/texture
  animation). Variants reuse a model and swap a texture (Wealthy, Fart and Koganemushi use the Kogane model). The list of everything
  a stage loads is `enemy/enemyResList.txt`; shared parts are in `enemy/common/enemyCommon.szs`.
- Sounds: `PSSE_EN_*` ids (`include/SoundID.h`, the `PSM::*` enemy sound objects in `plugProjectHikinoU`); ids with the 0x800 bit play once, the
  others are held sounds re-requested each frame (see `libsms/crates/libsms-audio/tests/pikmin2_sounds.rs`). Several enemies are
  named differently in the id table (Kurage = `KIKU_*`, Armor = `YOROI_*`, BlackMan = `KAGE_*`, Houdai also `PONGASHI_*`, Tobi = `TOBINKO_*`).
- Effects: `efx::T*` classes (`include/efx/`), JPA2 particles in `user/Ebisawa/effect/game.jpc`; there is no JPA2 engine, so each enemy lists the effects it names and the host uses none or a stand-in.

## Already in Pikmin 1 / the plugin (skip)

| Pikmin 2 | Common name | Pikmin 1 kind (`libpikmin-teki`) |
|---|---|---|
| Pelplant | Pellet Posy | kind 23 |
| Kochappy | Dwarf Red Bulborb | kind 0 |
| Chappy | Red Bulborb | kind 1 |
| KumaKochappy | Dwarf Bulbear | kind 2 |
| KumaChappy | Spotty Bulbear | kind 3 |
| Frog / MaroFrog | Yellow Wollywog / Wollywog | kinds 5 / 4 |
| UjiA / UjiB | Female / Male Sheargrub | kinds 9 / 10 |
| Tobi | Shearwig | kind 11 |
| Tadpole | Wogpole | kind 12 |
| Mar | Puffy Blowhog | kind 14 |
| Miulin | Mamuta | kind 15 |
| Catfish | Water Dumple | kind 17 |
| Qurione | Honeywisp | kind 18 |
| PanModoki | Breadbug | kind 19 |
| Rock | Falling boulder | kind 20 |
| Hiba | Fire geyser | kind 22 |
| Sarai | Swooping Snitchbug | kind 8 |
| Tank | Fiery Blowhog | kind 7 |

The Pikmin 2 versions of these are separate state machines (and have updated parms); porting them is not part of this task unless a new enemy needs the base class (Demon needs `Sarai`).

Not enemies (skipped): Pom* (Candypop Buds: the core's), Item*/Pellet* (objects), Tanpopo, Clover, HikariKinoko, Ooinu, KareOoinu, Wakame, Tukushi, Watage, Hana, Daiodo*, Magaret, Nekojarashi, Chiyogami, Zenmai (plants and props from `plantsMgr`), Stone (a projectile), Nest/JigumoHouse/PanHouse (spawners), Fkabuto/FminiHoudai/Rkabuto (placed variants listed under their enemy).

## Priority order

Tier 1 = small and iconic (a handful of states, one port covers several enemies), 2 = small standalone, 3 = medium, 4 = bosses and very large (thousands of lines, multi-part bodies). Within a tier, the order listed.
The first enemy to port is the **Kogane family** (Iridescent Flint Beetle, Iridescent Glint Beetle, Doodlebug): one 5-state machine with three re-skinned subclasses.

| # | Tier | Enemy (decomp) | Common name | Class | AI lines | States |
|---|---|---|---|---|---|---|
| 1 | 1 | Kogane | Iridescent Flint Beetle | NEW | 1796 | 5 |
| 2 | 1 | Wealthy | Iridescent Glint Beetle | VAR of Kogane | 234 | - |
| 3 | 1 | Fart | Doodlebug | VAR of Kogane | 324 | - |
| 4 | 1 | Tank | Watery Blowhog (Wtank) / Fiery Blowhog (Tank, ported P1 kind 7) | VAR of Tank | 1484 | 7 |
| 5 | 2 | Baby | Bulborb Larva | NEW | 967 | 5 |
| 6 | 2 | TamagoMushi | Mitite | NEW | 1219 | 6 |
| 7 | 2 | Sokkuri | Skitter Leaf | NEW | 986 | 9 |
| 8 | 2 | GasHiba | Gas pipe | NEW | 604 | 3 |
| 9 | 2 | ElecHiba | Electrical wire | NEW | 987 | 4 |
| 10 | 2 | Bomb | Bomb-rock | NEW | 738 | 2 |
| 11 | 2 | Egg | Egg (Empress's) | NEW | 522 | 1 |
| 12 | 2 | ElecBug | Anode Beetle | NEW | 1052 | 10 |
| 13 | 2 | Demon | Bumbling Snitchbug | VAR of Sarai | 152 | - |
| 14 | 2 | Kabuto | Armored Cannon Beetle Larva / Decorated Cannon Beetle | NEW (+ Rkabuto, Fkabuto variants) | 1706 | 13 |
| 15 | 3 | ChappyBase | Bulborb family: Orange (BlueChappy), Hairy (YellowChappy), Dwarf Orange, Snow, Fiery Bulblax | VAR of Chappy | 3044 | 18 |
| 16 | 3 | FireChappy | Fiery Bulblax | NEW | 515 | - |
| 17 | 3 | Armor | Cloaking Burrow-nit | NEW | 1790 | 14 |
| 18 | 3 | Fuefuki | Antenna Beetle | NEW | 1262 | 9 |
| 19 | 3 | Hanachirashi | Withering Blowhog | NEW | 1655 | 13 |
| 20 | 3 | Kurage | Lesser Spotted Jellyfloat | NEW | 1470 | 11 |
| 21 | 3 | BombSarai | Careening Dirigibug | NEW | 1350 | 13 |
| 22 | 3 | OtakaraBase | Dweevils: Fiery, Caustic, Munge, Anode, Volatile | NEW | 2687 | 14 |
| 23 | 3 | Imomushi | Ravenous Whiskerpillar | NEW | 1429 | 14 |
| 24 | 3 | Jigumo | Hermit Crawmad | NEW | 2170 | 13 |
| 25 | 3 | ShijimiChou | Unmarked Spectralids | NEW | 1666 | 6 |
| 26 | 3 | MiniHoudai | Gatling Groink | NEW | 2746 | 11 |
| 27 | 3 | OoPanModoki | Giant Breadbug | NEW | 2402 | - |
| 28 | 3 | LeafChappy | Bulbmin | NEW | 235 | - |
| 29 | 4 | OniKurage | Greater Spotted Jellyfloat | NEW | 1807 | 12 |
| 30 | 4 | Queen | Empress Bulblax | NEW (boss) | 1671 | 7 |
| 31 | 4 | KingChappy | Emperor Bulblax | NEW (boss) | 2498 | 13 |
| 32 | 4 | Damagumo | Beady Long Legs | NEW (boss) | 1615 | 6 |
| 33 | 4 | BigFoot | Raging Long Legs | NEW (boss) | 1758 | 6 |
| 34 | 4 | Houdai | Man-at-Legs | NEW (boss) | 2819 | 7 |
| 35 | 4 | DangoMushi | Segmented Crawbster | NEW (boss) | 1904 | 9 |
| 36 | 4 | SnakeCrow | Burrowing Snagret | NEW (boss) | 1883 | 9 |
| 37 | 4 | SnakeWhole | Pileated Snagret | NEW (boss) | 2133 | 11 |
| 38 | 4 | UmiMushi | Ranging Bloyster / Toady Bloyster | NEW (boss) | 2279 | 10 |
| 39 | 4 | BigTreasure | Titan Dweevil | NEW (boss) | 5063 | 12 |
| 40 | 4 | BlackMan | Waterwraith (with Tyre rollers) | NEW (boss) | 3883 | 14 |
| 41 | 4 | Tyre | Waterwraith roller | part of BlackMan | 1104 | 4 |

'AI lines' counts the `.cpp` files of the enemy and its family (Mgr, State, Animator, Shadow, subclasses); 'States' are the `StateID` entries of the header (`-` where the enemy reuses a base class's).

## Bosses (both games)

Pikmin 1's bosses are not `Teki`: they are a separate system (`pikmin-main/src/plugPikiNishimura/`: `Boss.cpp`/`BossMgr.cpp` shared
base and manager, one `*.cpp` + `*Ai.cpp` per boss, ids in `include/Boss.h`: `BOSS_Spider`, `Snake`, `Slime`, `King`, `Kogane`, `Pom`,
`KingBack`, `Nucleus`, `CoreNucleus`, `BoxSnake`, `Mizu`, `Geyzer`; models `bosses/<name>/<name>.mod` + `parms.bin`). `libpikmin-teki`
and `TekiRoster` kinds 0-26 cover only the regular Teki, so none of these is in. Pikmin 2's bosses are `Entities` like the rest (the tier 4
rows above). The two games overlap: P1 Spider/Snake/King are the Beady Long Legs / Burrowing Snagret / Emperor Bulblax that Pikmin 2 also has (as
`Damagumo`, `SnakeCrow`, `KingChappy`), so port one version of each, not both. P1's Kogane is the Iridescent Flint Beetle (a "boss" object in
P1), the same creature as P2's `Koganemushi`: the Kogane family port serves both.

| Game | Boss | Decomp (AI lines, incl. parts) | Hull and hurtboxes | Ship Repair |
|---|---|---|---|---|
| P1 | Kogane (Iridescent Flint Beetle) | `Kogane.cpp` + `KoganeAi.cpp`, 961 | small, 1 hurtbox | Covered by the P2 Kogane port (stage 1). |
| P1 | Mizu (water geyser) | `Mizu*.cpp`, 475 | stationary hazard, no body | Hazard like Fire Geyser (kind 22); cheap. |
| P1 | Pom (Pikmin-swallowing flower) | `Pom*.cpp`, 911 | stationary, 1 hurtbox | A static hazard; needs Pikmin to swallow, odd for RoR2. |
| P1 | Spider (Beady Long Legs) | `Spider*.cpp` + `SpiderLeg.cpp`, 2205 | huge body, 4+ legs, hurtbox per leg | Possible: Golem-size hull, many hurtboxes, high cost. |
| P1 | Snake (Burrowing Snagret, BoxSnake) | `Snake*.cpp`, 2822 | long body of joints, burrows | Hard: segmented body. |
| P1 | King (Emperor Bulblax, KingBack) | `King*.cpp`, 2873 | very large, walks, 2 halves | Possible: a big Bulborb is the simplest boss body. |
| P1 | Slime (Plasm Wraith, Nucleus, CoreNucleus) | `Slime*`, `Nucleus*`, `CoreNucleus*`, ~2900 | soft multi-body with cores | Hard: deforming body, three kinds of part. |
| P2 | Queen (Empress Bulblax) | 1671 | large, walks, lays eggs/larvae | Possible; the eggs/larvae (tier 2) come first. |
| P2 | Damagumo (Beady Long Legs) | 1615 | large, 4 legs, leg system shared with BigFoot/Houdai | Same as the P1 Spider. |
| P2 | BigFoot (Raging Long Legs) | 1758 | huge, 2 legs | Large hull. |
| P2 | SnakeCrow (Burrowing Snagret) | 1883 | burrows | Same as the P1 Snake. |
| P2 | OniKurage (Greater Spotted Jellyfloat) | 1807 | floating, medium | Easiest P2 boss body; builds on Kurage (tier 3). |
| P2 | DangoMushi (Segmented Crawbster) | 1904 | long segmented body | Hard. |
| P2 | SnakeWhole (Pileated Snagret) | 2133 | very large, segmented | Hard. |
| P2 | UmiMushi (Ranging Bloyster) | 2279 | stationary/water, big | Awkward (water). |
| P2 | KingChappy (Emperor Bulblax) | 2498 | very large | Same as the P1 King. |
| P2 | Houdai (Man-at-Legs) | 2819 | huge, legs and a cannon | Large. |
| P2 | BlackMan + Tyre (Waterwraith) | 3883 | wraith with rolling parts | Hard. |
| P2 | BigTreasure (Titan Dweevil) | 5063 | huge, 4 arms, 12 phases | Hardest; last. |

Order for the bosses once the regular tiers are far enough along: (1) the Kogane family (not a real boss), (2) the geyser/flower hazards (Mizu, Pom),
(3) the Bulblax body (P2 `Queen` or the P1 `King`; the Empress needs the Larva/Egg first), (4) the Jellyfloat pair (`Kurage`, then `OniKurage`),
(5) the long-legged spiders, (6) the segmented and wraith bosses, (7) the Titan Dweevil. In Ship Repair a boss needs a Golem-sized hull, a hurtbox per
big part and a spawn cost near the existing large kinds (300-400 credits); none is small enough to jump the queue before the first regular enemy is wired.
No boss port was started in the first pass.

## Per enemy

### Kogane: Iridescent Flint Beetle

- Class: NEW, tier 1. Base of the Kogane family; 5 states (appear, disappear, move, wait, press). Loot bug: flees, buried in the ground, drops pellets/nectar when pressed or killed. `Koganemushi::Obj` is the Flint Beetle itself (a re-skin through the `karada` material).
- Header: `pikmin2-main/include/Game/Entities/Kogane.h`
- Decomp sources: `src/plugProjectNishimuraU/Fart.cpp`, `src/plugProjectNishimuraU/FartMgr.cpp`, `src/plugProjectNishimuraU/Kogane.cpp`, `src/plugProjectNishimuraU/KoganeAnimator.cpp`, `src/plugProjectNishimuraU/KoganeMgr.cpp`, `src/plugProjectNishimuraU/KoganeState.cpp`, `src/plugProjectNishimuraU/Koganemushi.cpp`, `src/plugProjectNishimuraU/KoganemushiMgr.cpp`, `src/plugProjectNishimuraU/Wealthy.cpp`, `src/plugProjectNishimuraU/WealthyMgr.cpp`
- Disc (`enemy/data/...`): `Fart/babakogane_s3tc.bti`, `Kogane/anim.szs`, `Kogane/kogane_s3tc.bti`, `Kogane/model.szs`, `Wealthy/oogane_s3tc.bti`
- AI size: 1796 lines, 5 states. States: `KOGANE_Appear`, `KOGANE_Disappear`, `KOGANE_Move`, `KOGANE_Wait`, `KOGANE_Press`
- Sounds: `PSSE_EN_ENEMY_LOOSE_ITEM`, `PSSE_EN_FART_BUZZ`, `PSSE_EN_FART_GAS`, `PSSE_EN_FART_HIT`, `PSSE_EN_KOGANE_HIT`, `PSSE_EN_OOGANE_HIT`, `PSSE_EN_OTAKARA_ATK_GAS`, `PSSE_EN_TAMAGOMUSHI_APPEAR`, `PSSE_EN_TAMAGOMUSHI_DIVE`. Id family: KOGANE_*, TAMAGOMUSHI_APPEAR/DIVE, ENEMY_LOOSE_ITEM.
- Effects (`efx::`): `TBabaFly_ver01`, `TBabaHe`, `TKoganeDive`, `TKoganeHit`, `TOoganeKira`.

### Wealthy: Iridescent Glint Beetle

- Class: VAR of Kogane, tier 1. `Wealthy::Obj : Kogane::Obj`; other texture (`oogane_s3tc.bti`) and parms, `TOoganeKira` sparkle, own drop table.
- Header: `pikmin2-main/include/Game/Entities/Wealthy.h`
- Decomp sources: `src/plugProjectNishimuraU/Wealthy.cpp`, `src/plugProjectNishimuraU/WealthyMgr.cpp`
- Disc (`enemy/data/...`): `Wealthy/oogane_s3tc.bti`
- AI size: 234 lines, 0 states.
- Sounds: `PSSE_EN_OOGANE_HIT`. Id family: OOGANE_HIT.
- Effects (`efx::`): `TOoganeKira`.

### Fart: Doodlebug

- Class: VAR of Kogane, tier 1. `Fart::Obj : Kogane::Obj`; texture `babakogane_s3tc.bti`; adds a poison gas cloud (`TBabaHe`).
- Header: `pikmin2-main/include/Game/Entities/Fart.h`
- Decomp sources: `src/plugProjectNishimuraU/Fart.cpp`, `src/plugProjectNishimuraU/FartMgr.cpp`
- Disc (`enemy/data/...`): `Fart/babakogane_s3tc.bti`
- AI size: 324 lines, 0 states.
- Sounds: `PSSE_EN_FART_BUZZ`, `PSSE_EN_FART_GAS`, `PSSE_EN_FART_HIT`, `PSSE_EN_OTAKARA_ATK_GAS`. Id family: FART_BUZZ/GAS/HIT, OTAKARA_ATK_GAS.
- Effects (`efx::`): `TBabaFly_ver01`, `TBabaHe`.

### Tank: Watery Blowhog (Wtank) / Fiery Blowhog (Tank, ported P1 kind 7)

- Class: VAR of Tank, tier 1. `Wtank.cpp`/`Ftank.cpp` are 150-line subclasses of `Tank::Obj` (water and fire shot). Fiery Blowhog is already kind 7 from the P1 strategy; the P2 `Tank::Obj` is a different state machine (7 states).
- Header: `pikmin2-main/include/Game/Entities/Tank.h`
- Decomp sources: `src/plugProjectNishimuraU/Ftank.cpp`, `src/plugProjectNishimuraU/FtankMgr.cpp`, `src/plugProjectNishimuraU/Tank.cpp`, `src/plugProjectNishimuraU/TankAnimator.cpp`, `src/plugProjectNishimuraU/TankMgr.cpp`, `src/plugProjectNishimuraU/TankState.cpp`, `src/plugProjectNishimuraU/Wtank.cpp`, `src/plugProjectNishimuraU/WtankMgr.cpp`
- Disc (`enemy/data/...`): `Tank/anim.szs`, `Tank/fire_butadokkuri_main_s3tc.bti`, `Tank/model.szs`
- AI size: 1484 lines, 7 states. States: `TANK_Dead`, `TANK_Wait`, `TANK_Move`, `TANK_MoveTurn`, `TANK_ChaseTurn`, `TANK_Attack`, `TANK_Flick`
- Sounds: `PSSE_EN_TANK_BREATH`, `PSSE_EN_TANK_FIRE`, `PSSE_EN_WATERTANK_BREATH`, `PSSE_EN_WATERTANK_SHOT`. Id family: TANK_*, WATERTANK_*, WATERSHOT.
- Effects (`efx::`): `TTankEffect`, `TWtankEffect`.

### Baby: Bulborb Larva

- Class: NEW, tier 2. 5 states (born, move, attack, press, dead). Hatches from Empress eggs; tiny, simple walker.
- Header: `pikmin2-main/include/Game/Entities/Baby.h`
- Decomp sources: `src/plugProjectNishimuraU/Baby.cpp`, `src/plugProjectNishimuraU/BabyAnimator.cpp`, `src/plugProjectNishimuraU/BabyMgr.cpp`, `src/plugProjectNishimuraU/BabyState.cpp`
- Disc (`enemy/data/...`): `Baby/anim.szs`, `Baby/model.szs`
- AI size: 967 lines, 5 states. States: `BABY_Dead`, `BABY_Press`, `BABY_Born`, `BABY_Move`, `BABY_Attack`
- Sounds: none named in the sources. Id family: BABY_PRESSED.
- Effects (`efx::`): `TBabyBecha`, `TBabyBorn`.

### TamagoMushi: Mitite

- Class: NEW, tier 2. 6 states; ground bug that hatches from eggs and eats Pikmin. Small.
- Header: `pikmin2-main/include/Game/Entities/TamagoMushi.h`
- Decomp sources: `src/plugProjectMorimuraU/tamagoMushi.cpp`, `src/plugProjectMorimuraU/tamagoMushiAnimator.cpp`, `src/plugProjectMorimuraU/tamagoMushiMgr.cpp`, `src/plugProjectMorimuraU/tamagoMushiState.cpp`
- Disc (`enemy/data/...`): `TamagoMushi/anim.szs`, `TamagoMushi/model.szs`
- AI size: 1219 lines, 6 states. States: `TAMAGOMUSHI_Walk`, `TAMAGOMUSHI_Turn`, `TAMAGOMUSHI_Appear`, `TAMAGOMUSHI_Hide`, `TAMAGOMUSHI_Dead`, `TAMAGOMUSHI_Wait`
- Sounds: `PSSE_EN_TAMAGOMUSHI_APPEAR`. Id family: TAMAGOMUSHI_*.
- Effects (`efx::`): `TEnemyBomb`, `TEnemyDead`, `TTamagoAp`, `TUjinkoHd`.

### Sokkuri: Skitter Leaf

- Class: NEW, tier 2. 9 states; skates on water, hides under leaves. Needs water height like the Wogpole.
- Header: `pikmin2-main/include/Game/Entities/Sokkuri.h`
- Decomp sources: `src/plugProjectNishimuraU/Sokkuri.cpp`, `src/plugProjectNishimuraU/SokkuriAnimator.cpp`, `src/plugProjectNishimuraU/SokkuriMgr.cpp`, `src/plugProjectNishimuraU/SokkuriState.cpp`
- Disc (`enemy/data/...`): `Sokkuri/anim.szs`, `Sokkuri/model.szs`
- AI size: 986 lines, 9 states. States: `SOKKURI_Dead`, `SOKKURI_Press`, `SOKKURI_Stay`, `SOKKURI_Appear`, `SOKKURI_Disappear`, `SOKKURI_Wait`, `SOKKURI_MoveGround`, `SOKKURI_MoveWater`, `SOKKURI_Flick`
- Sounds: none named in the sources. Id family: SOKKURI_*.
- Effects (`efx::`): `TJgmBubble`.

### GasHiba: Gas pipe

- Class: NEW, tier 2. 3 states (wait, attack, dead). Hazard: poison gas (`TGasuHiba`). Sibling of Fire Geyser (P1 kind 22).
- Header: `pikmin2-main/include/Game/Entities/GasHiba.h`
- Decomp sources: `src/plugProjectNishimuraU/GasHiba.cpp`, `src/plugProjectNishimuraU/GasHibaAnimator.cpp`, `src/plugProjectNishimuraU/GasHibaMgr.cpp`, `src/plugProjectNishimuraU/GasHibaState.cpp`
- Disc (`enemy/data/...`): `GasHiba/anim.szs`, `GasHiba/model.szs`
- AI size: 604 lines, 3 states. States: `GASHIBA_Dead`, `GASHIBA_Wait`, `GASHIBA_Attack`
- Sounds: `PSSE_EN_GAS_HIBA_VOMIT`, `PSSE_EN_HIBA_STOP`. Id family: GAS_HIBA_VOMIT, HIBA_STOP.
- Effects (`efx::`): `TEnemyBomb`, `TGasuHiba`.

### ElecHiba: Electrical wire

- Class: NEW, tier 2. 4 states (wait, sign, attack, dead). Hazard: electric arcs.
- Header: `pikmin2-main/include/Game/Entities/ElecHiba.h`
- Decomp sources: `src/plugProjectNishimuraU/ElecHiba.cpp`, `src/plugProjectNishimuraU/ElecHibaAnimator.cpp`, `src/plugProjectNishimuraU/ElecHibaMgr.cpp`, `src/plugProjectNishimuraU/ElecHibaState.cpp`
- Disc (`enemy/data/...`): `ElecHiba/anim.szs`, `ElecHiba/model.szs`
- AI size: 987 lines, 4 states. States: `ELECHIBA_Dead`, `ELECHIBA_Wait`, `ELECHIBA_Sign`, `ELECHIBA_Attack`
- Sounds: `PSSE_EN_ELEC_HIBA_CHARGE`, `PSSE_EN_ELEC_HIBA_SPARK`, `PSSE_EN_HIBA_STOP`. Id family: ELEC_HIBA_CHARGE/SPARK, HIBA_STOP.
- Effects (`efx::`): `TDenkiHibaMgr`, `TEnemyBomb`.

### Bomb: Bomb-rock

- Class: NEW, tier 2. 2 states; a thrown/rolling bomb that explodes. Item-like, usable as a hazard.
- Header: `pikmin2-main/include/Game/Entities/Bomb.h`
- Decomp sources: `src/plugProjectMorimuraU/bomb.cpp`, `src/plugProjectMorimuraU/bombAnimator.cpp`, `src/plugProjectMorimuraU/bombMgr.cpp`, `src/plugProjectMorimuraU/bombState.cpp`
- Disc (`enemy/data/...`): `Bomb/anim.szs`, `Bomb/model.szs`
- AI size: 738 lines, 2 states. States: `BOMB_Wait`, `BOMB_Bomb`
- Sounds: `PSSE_EN_BOMB_LOOP`. Id family: BOMB_*.
- Effects (`efx::`): `TBombrock`, `TBombrockABCD`, `TBombrockEFGH`, `TBombrockLight`.

### Egg: Egg (Empress's)

- Class: NEW, tier 2. 522 lines; laid by the Empress and hatches into Larva or Mitite. Companion to Baby.
- Header: `pikmin2-main/include/Game/Entities/Egg.h`
- Decomp sources: `src/plugProjectMorimuraU/egg.cpp`, `src/plugProjectMorimuraU/eggAnimator.cpp`, `src/plugProjectMorimuraU/eggMgr.cpp`, `src/plugProjectMorimuraU/eggState.cpp`
- Disc (`enemy/data/...`): `Egg/anim.szs`, `Egg/model.szs`
- AI size: 522 lines, 1 states. States: `EGG_Wait`
- Sounds: `PSSE_EN_EGG_BREAK`. Id family: EGG_*.
- Effects (`efx::`): `TEggdown`, `TEnemyBomb`.

### ElecBug: Anode Beetle

- Class: NEW, tier 2. 10 states; charges and discharges electricity, children chain. Electric hazard.
- Header: `pikmin2-main/include/Game/Entities/ElecBug.h`
- Decomp sources: `src/plugProjectNishimuraU/ElecBug.cpp`, `src/plugProjectNishimuraU/ElecBugAnimator.cpp`, `src/plugProjectNishimuraU/ElecBugMgr.cpp`, `src/plugProjectNishimuraU/ElecBugState.cpp`
- Disc (`enemy/data/...`): `ElecBug/anim.szs`, `ElecBug/model.szs`
- AI size: 1052 lines, 10 states. States: `ELECBUG_Dead`, `ELECBUG_Wait`, `ELECBUG_Turn`, `ELECBUG_Move`, `ELECBUG_Charge`, `ELECBUG_Discharge`, `ELECBUG_ChildCharge`, `ELECBUG_ChildDischarge`, `ELECBUG_Reverse`, `ELECBUG_Return`
- Sounds: none named in the sources. Id family: ELECBUG_*.
- Effects (`efx::`): `TDnkmsEffect`.

### Demon: Bumbling Snitchbug

- Class: VAR of Sarai, tier 2. 152 lines on top of `Sarai::Obj` (1142 lines, 11 states): the Snitchbug that carries the captain. Needs the P2 Sarai (the P1 `napkid` is a different strategy).
- Header: `pikmin2-main/include/Game/Entities/Demon.h`
- Decomp sources: `src/plugProjectNishimuraU/Demon.cpp`, `src/plugProjectNishimuraU/DemonMgr.cpp`
- Disc (`enemy/data/...`): `Demon/anim.szs`, `Demon/model.szs`
- AI size: 152 lines, 0 states.
- Sounds: none named in the sources. Id family: SARAI_*.
- Effects (`efx::`): none.

### Kabuto: Armored Cannon Beetle Larva / Decorated Cannon Beetle

- Class: NEW (+ Rkabuto, Fkabuto variants), tier 2. 13 states with fixed (`Fix*`) variants. Related to P1 Cannon Beetle (kind 6) in name only; different model/AI.
- Header: `pikmin2-main/include/Game/Entities/Kabuto.h`
- Decomp sources: `src/plugProjectNishimuraU/FixKabuto.cpp`, `src/plugProjectNishimuraU/FixKabutoMgr.cpp`, `src/plugProjectNishimuraU/GreenKabuto.cpp`, `src/plugProjectNishimuraU/GreenKabutoMgr.cpp`, `src/plugProjectNishimuraU/Kabuto.cpp`, `src/plugProjectNishimuraU/KabutoAnimator.cpp`, `src/plugProjectNishimuraU/KabutoMgr.cpp`, `src/plugProjectNishimuraU/KabutoState.cpp`, `src/plugProjectNishimuraU/RedKabuto.cpp`, `src/plugProjectNishimuraU/RedKabutoMgr.cpp`
- Disc (`enemy/data/...`): `Kabuto/anim.szs`, `Kabuto/babykabuto_green_s3tc.bti`, `Kabuto/bti.szs`, `Kabuto/model.szs`
- AI size: 1706 lines, 13 states. States: `KABUTO_Dead`, `KABUTO_Wait`, `KABUTO_Turn`, `KABUTO_Move`, `KABUTO_Flick`, `KABUTO_Attack`, `KABUTO_FixStay`, `KABUTO_FixAppear`, `KABUTO_FixHide`, `KABUTO_FixWait`, `KABUTO_FixTurn`, `KABUTO_FixAttack`, `KABUTO_FixFlick`
- Sounds: none named in the sources. Id family: KABUTO_*.
- Effects (`efx::`): `THebiAphd_kkabuto_appear`, `THebiAphd_kkabuto_dive`, `TKabutoAttack`, `TKkabutoRot`, `TKkabutoWait`.

### ChappyBase: Bulborb family: Orange (BlueChappy), Hairy (YellowChappy), Dwarf Orange, Snow, Fiery Bulblax

- Class: VAR of Chappy, tier 3. `ChappyBase` 518 + 8 states; Chappy/BlueChappy/YellowChappy are 80-240 line subclasses. Chappy/Kochappy are the P1 Bulborbs (kinds 1/0), but the P2 state machine is separate.
- Header: `pikmin2-main/include/Game/Entities/ChappyBase.h`
- Decomp sources: `src/plugProjectYamashitaU/BlueChappy.cpp`, `src/plugProjectYamashitaU/BlueChappyMgr.cpp`, `src/plugProjectYamashitaU/BlueKochappy.cpp`, `src/plugProjectYamashitaU/BlueKochappyMgr.cpp`, `src/plugProjectYamashitaU/ChappyBase.cpp`, `src/plugProjectYamashitaU/ChappyBaseMgr.cpp`, `src/plugProjectYamashitaU/KochappyBase.cpp`, `src/plugProjectYamashitaU/KochappyBaseMgr.cpp`, `src/plugProjectYamashitaU/YellowChappy.cpp`, `src/plugProjectYamashitaU/YellowChappyMgr.cpp`, `src/plugProjectYamashitaU/YellowKochappy.cpp`, `src/plugProjectYamashitaU/YellowKochappyMgr.cpp`, `src/plugProjectYamashitaU/chappy.cpp`, `src/plugProjectYamashitaU/chappyAnimator.cpp`, `src/plugProjectYamashitaU/chappyMgr.cpp`, `src/plugProjectYamashitaU/chappyState.cpp`, `src/plugProjectYamashitaU/kochappy.cpp`, `src/plugProjectYamashitaU/kochappyAnimator.cpp`, `src/plugProjectYamashitaU/kochappyMgr.cpp`, `src/plugProjectYamashitaU/kochappyState.cpp`
- Disc (`enemy/data/...`): `BlueChappy/moyou_565.3.bti`, `BlueChappy/swallow_565.3.bti`, `BlueKochappy/kochappy_body_s3tc.3.bti`, `Chappy/anim.szs`, `Chappy/model.szs`, `Chappy/moyou_565.1.bti`, `Chappy/swallow_565.1.bti`, `Kochappy/anim.szs`, `Kochappy/kochappy_body_s3tc.1.bti`, `Kochappy/model.szs`, `YellowChappy/moyou_565.2.bti`, `YellowChappy/swallow_565.2.bti`, `YellowKochappy/kochappy_body_s3tc.2.bti`
- AI size: 3044 lines, 18 states. States: `CHAPPY_Turn`, `CHAPPY_Dead`, `CHAPPY_Flick`, `CHAPPY_Walk`, `CHAPPY_Attack`, `CHAPPY_TurnToHome`, `CHAPPY_GoHome`, `CHAPPY_Sleep`
- Sounds: `PSSE_EN_KOCHAPPY_NOTICE`. Id family: CHAPPY_*, KOCHAPPY_*, FIRE_CHAPPY_*.
- Effects (`efx::`): `TChaseMtx`, `TEnemyDownSmoke`, `TEnemyDownWat`, `THanachoN`, `THanachoY`, `TKechappyOff`, `TKechappyTest`.

### FireChappy: Fiery Bulblax

- Class: NEW, tier 3. 515 lines on `ChappyBase`; fire breath.
- Header: `pikmin2-main/include/Game/Entities/FireChappy.h`
- Decomp sources: `src/plugProjectNishimuraU/FireChappy.cpp`, `src/plugProjectNishimuraU/FireChappyMgr.cpp`
- Disc (`enemy/data/...`): `FireChappy/anim.szs`, `FireChappy/model.szs`, `FireChappy/yakichappy.brk`, `FireChappy/yakichappy.btk`
- AI size: 515 lines, 0 states.
- Sounds: `PSSE_EN_FIRE_CHAPPY_FLAME`, `PSSE_EN_FIRE_CHAPPY_F_END`. Id family: FIRE_CHAPPY_FLAME.
- Effects (`efx::`): `THanachoY`, `TYakiBody`, `TYakiDeadsmoke`, `TYakiFlick`, `TYakiSteam`.

### Armor: Cloaking Burrow-nit

- Class: NEW, tier 3. 14 states; burrower with a shell, fakes out Pikmin, eats.
- Header: `pikmin2-main/include/Game/Entities/Armor.h`
- Decomp sources: `src/plugProjectNishimuraU/Armor.cpp`, `src/plugProjectNishimuraU/ArmorAnimator.cpp`, `src/plugProjectNishimuraU/ArmorMgr.cpp`, `src/plugProjectNishimuraU/ArmorState.cpp`
- Disc (`enemy/data/...`): `Armor/anim.szs`, `Armor/model.szs`
- AI size: 1790 lines, 14 states. States: `ARMOR_Dead`, `ARMOR_Stay`, `ARMOR_Appear`, `ARMOR_Dive`, `ARMOR_Move`, `ARMOR_MoveSide`, `ARMOR_MoveCentre`, `ARMOR_MoveTop`, `ARMOR_GoHome`, `ARMOR_Attack1`, `ARMOR_Attack2`, `ARMOR_Eat`, `ARMOR_Flick`, `ARMOR_Fail`
- Sounds: none named in the sources. Id family: YOROI_*.
- Effects (`efx::`): `TYoroiAp`, `TYoroiAttack`, `TYoroiAttackhit`, `TYoroiEat`, `TYoroiHd`.

### Fuefuki: Antenna Beetle

- Class: NEW, tier 3. 9 states; whistles and calls more enemies.
- Header: `pikmin2-main/include/Game/Entities/Fuefuki.h`
- Decomp sources: `src/plugProjectNishimuraU/Fuefuki.cpp`, `src/plugProjectNishimuraU/FuefukiAnimator.cpp`, `src/plugProjectNishimuraU/FuefukiMgr.cpp`, `src/plugProjectNishimuraU/FuefukiState.cpp`
- Disc (`enemy/data/...`): `Fuefuki/anim.szs`, `Fuefuki/model.szs`
- AI size: 1262 lines, 9 states. States: `FUEFUKI_Dead`, `FUEFUKI_Stay`, `FUEFUKI_Land`, `FUEFUKI_Jump`, `FUEFUKI_Wait`, `FUEFUKI_Turn`, `FUEFUKI_Walk`, `FUEFUKI_Whisle`, `FUEFUKI_Struggle`
- Sounds: `PSSE_EN_FUEFUKI_WHISTLE_ECHO`. Id family: FUEFUKI_*.
- Effects (`efx::`): `TCursor`, `TFuebugOnpa`.

### Hanachirashi: Withering Blowhog

- Class: NEW, tier 3. 13 states; flying poison-spreading balloon.
- Header: `pikmin2-main/include/Game/Entities/Hanachirashi.h`
- Decomp sources: `src/plugProjectNishimuraU/Hanachirashi.cpp`, `src/plugProjectNishimuraU/HanachirashiAnimator.cpp`, `src/plugProjectNishimuraU/HanachirashiMgr.cpp`, `src/plugProjectNishimuraU/HanachirashiState.cpp`
- Disc (`enemy/data/...`): `Hanachirashi/anim.szs`, `Hanachirashi/hanachirashi_model.brk`, `Hanachirashi/hanachirashi_model.btk`, `Hanachirashi/model.szs`
- AI size: 1655 lines, 13 states. States: `HANACHIRASHI_Dead`, `HANACHIRASHI_Wait`, `HANACHIRASHI_Move`, `HANACHIRASHI_Chase`, `HANACHIRASHI_ChaseInside`, `HANACHIRASHI_Attack`, `HANACHIRASHI_Fall`, `HANACHIRASHI_Land`, `HANACHIRASHI_Ground`, `HANACHIRASHI_TakeOff`, `HANACHIRASHI_FlyFlick`, `HANACHIRASHI_GroundFlick`, `HANACHIRASHI_Laugh`
- Sounds: none named in the sources. Id family: HANACHIRASHI_*.
- Effects (`efx::`): `TFusenDead`, `TFusenSui`, `TFusenhAir`, `TFusenhAirhit`.

### Kurage: Lesser Spotted Jellyfloat

- Class: NEW, tier 3. 11 states; floating jelly that lifts Pikmin.
- Header: `pikmin2-main/include/Game/Entities/Kurage.h`
- Decomp sources: `src/plugProjectNishimuraU/Kurage.cpp`, `src/plugProjectNishimuraU/KurageAnimator.cpp`, `src/plugProjectNishimuraU/KurageMgr.cpp`, `src/plugProjectNishimuraU/KurageState.cpp`
- Disc (`enemy/data/...`): `Kurage/anim.szs`, `Kurage/model.szs`
- AI size: 1470 lines, 11 states. States: `KURAGE_Dead`, `KURAGE_Wait`, `KURAGE_Move`, `KURAGE_Chase`, `KURAGE_Attack`, `KURAGE_Fall`, `KURAGE_Land`, `KURAGE_Ground`, `KURAGE_TakeOff`, `KURAGE_FlyFlick`, `KURAGE_GroundFlick`
- Sounds: `PSSE_EN_KURAGE_AIR`, `PSSE_EN_KURAGE_DAMAGE`, `PSSE_EN_KURAGE_GET_PIKI`, `PSSE_EN_KURAGE_HARETSU`, `PSSE_EN_KURAGE_SING`. Id family: KURAGE_* (KIKU_* in the id table).
- Effects (`efx::`): `TNewkurageBomb`, `TNewkurageDeadrun`, `TNewkurageEye`, `TNewkurageFlick`, `TNewkurageHire`, `TNewkurageKira`, `TNewkurageSui`.

### BombSarai: Careening Dirigibug

- Class: NEW, tier 3. 13 states; flying bomb carrier.
- Header: `pikmin2-main/include/Game/Entities/BombSarai.h`
- Decomp sources: `src/plugProjectNishimuraU/BombSarai.cpp`, `src/plugProjectNishimuraU/BombSaraiAnimator.cpp`, `src/plugProjectNishimuraU/BombSaraiMgr.cpp`, `src/plugProjectNishimuraU/BombSaraiState.cpp`
- Disc (`enemy/data/...`): `BombSarai/anim.szs`, `BombSarai/model.szs`
- AI size: 1350 lines, 13 states. States: `BOMBSARAI_Dead`, `BOMBSARAI_Damage`, `BOMBSARAI_Wait`, `BOMBSARAI_BombWait`, `BOMBSARAI_Move`, `BOMBSARAI_BombMove`, `BOMBSARAI_Supply`, `BOMBSARAI_Release`, `BOMBSARAI_Fall`, `BOMBSARAI_TakeOff1`, `BOMBSARAI_TakeOff2`, `BOMBSARAI_Flick`, `BOMBSARAI_BombFlick`
- Sounds: `PSSE_EN_BOMBSARAI_AIR`, `PSSE_EN_BOMBSARAI_DEAD`. Id family: BOMBSARAI_*.
- Effects (`efx::`): `TBsaraiDead`, `TBsaraiSupli`.

### OtakaraBase: Dweevils: Fiery, Caustic, Munge, Anode, Volatile

- Class: NEW, tier 3. `OtakaraBase` 1753 lines, 14 states, each Dweevil a ~190-line subclass (Fire/Water/Gas/ElecOtakara, BombOtakara). One port covers five enemies; they also carry a treasure model (p2-treasures overlap: coordinate).
- Header: `pikmin2-main/include/Game/Entities/OtakaraBase.h`
- Decomp sources: `src/plugProjectNishimuraU/BombOtakara.cpp`, `src/plugProjectNishimuraU/BombOtakaraMgr.cpp`, `src/plugProjectNishimuraU/ElecOtakara.cpp`, `src/plugProjectNishimuraU/ElecOtakaraMgr.cpp`, `src/plugProjectNishimuraU/FireOtakara.cpp`, `src/plugProjectNishimuraU/FireOtakaraMgr.cpp`, `src/plugProjectNishimuraU/GasOtakara.cpp`, `src/plugProjectNishimuraU/GasOtakaraMgr.cpp`, `src/plugProjectNishimuraU/OtakaraBase.cpp`, `src/plugProjectNishimuraU/OtakaraBaseAnimator.cpp`, `src/plugProjectNishimuraU/OtakaraBaseMgr.cpp`, `src/plugProjectNishimuraU/OtakaraBaseState.cpp`, `src/plugProjectNishimuraU/WaterOtakara.cpp`, `src/plugProjectNishimuraU/WaterOtakaraMgr.cpp`
- Disc (`enemy/data/...`): `BombOtakara/otakara_bomb_s3tc.bti`, `ElecOtakara/otakara_yellow_s3tc.bti`, `FireOtakara/anim.szs`, `FireOtakara/model.szs`, `FireOtakara/otakara_red_s3tc.bti`, `GasOtakara/otakara_purple_s3tc.bti`, `WaterOtakara/otakara_blue_s3tc.bti`
- AI size: 2687 lines, 14 states. States: `OTA_Dead`, `OTA_Flick`, `OTA_Wait`, `OTA_Move`, `OTA_Turn`, `OTA_Take`, `OTA_ItemWait`, `OTA_ItemMove`, `OTA_ItemTurn`, `OTA_ItemFlick`, `OTA_ItemDrop`, `OTA_BombWait`, `OTA_BombMove`, `OTA_BombTurn`
- Sounds: `PSSE_EN_OTAKARA_ATK_ELEC`, `PSSE_EN_OTAKARA_ATK_GAS`, `PSSE_EN_OTAKARA_ATK_WATER`, `PSSE_EN_OTAKARA_ATTACK_FIRE`, `PSSE_EN_OTAKARA_CHARGE`, `PSSE_EN_OTAKARA_DROP_ITEM`, `PSSE_EN_OTAKARA_STANDUP`. Id family: OTAKARA_*.
- Effects (`efx::`): `TOtaChargeelec`, `TOtaChargefire`, `TOtaChargegas`, `TOtaChargewat`, `TOtaElec`, `TOtaFire`, `TOtaGas`, `TOtaPartsoff`, `TOtaWat`.

### Imomushi: Ravenous Whiskerpillar

- Class: NEW, tier 3. 14 states; burrower.
- Header: `pikmin2-main/include/Game/Entities/Imomushi.h`
- Decomp sources: `src/plugProjectNishimuraU/Imomushi.cpp`, `src/plugProjectNishimuraU/ImomushiAnimator.cpp`, `src/plugProjectNishimuraU/ImomushiMgr.cpp`, `src/plugProjectNishimuraU/ImomushiState.cpp`
- Disc (`enemy/data/...`): `Imomushi/anim.szs`, `Imomushi/model.szs`
- AI size: 1429 lines, 14 states. States: `IMOMUSHI_Dead`, `IMOMUSHI_Wait`, `IMOMUSHI_FallDive`, `IMOMUSHI_FallMove`, `IMOMUSHI_Stay`, `IMOMUSHI_Appear`, `IMOMUSHI_Dive`, `IMOMUSHI_Move`, `IMOMUSHI_GoHome`, `IMOMUSHI_Attack`, `IMOMUSHI_Climb`, `IMOMUSHI_ZukanStay`, `IMOMUSHI_ZukanAppear`, `IMOMUSHI_ZukanMove`
- Sounds: none named in the sources. Id family: IMOMUSHI_*.
- Effects (`efx::`): `TImoEat`, `TImoSmoke`, `TUjinkoAp_Imo`, `TUjinkoHd_Imo`.

### Jigumo: Hermit Crawmad

- Class: NEW, tier 3. 13 states + Nest; shelled burrower.
- Header: `pikmin2-main/include/Game/Entities/Jigumo.h`
- Decomp sources: `src/plugProjectMorimuraU/jigumo.cpp`, `src/plugProjectMorimuraU/jigumoAnimator.cpp`, `src/plugProjectMorimuraU/jigumoMgr.cpp`, `src/plugProjectMorimuraU/jigumoState.cpp`
- Disc (`enemy/data/...`): `Jigumo/anim.szs`, `Jigumo/kochappy_body_s3tc.1.bti`, `Jigumo/model.szs`
- AI size: 2170 lines, 13 states. States: `JIGUMO_Wait`, `JIGUMO_Appear`, `JIGUMO_Hide`, `JIGUMO_Dead`, `JIGUMO_Attack`, `JIGUMO_Miss`, `JIGUMO_Return`, `JIGUMO_Carry`, `JIGUMO_Flick`, `JIGUMO_Eat`, `JIGUMO_Search`, `JIGUMO_SAttack`, `JIGUMO_SMiss`
- Sounds: none named in the sources. Id family: JIGUMO_*.
- Effects (`efx::`): `TEnemyDive`, `TImoSmoke`, `TJgmAttack`, `TJgmAttackW`, `TJgmBack`, `TJgmBackW`, `TJgmBubble`.

### ShijimiChou: Unmarked Spectralids

- Class: NEW, tier 3. 6 states; flock.
- Header: `pikmin2-main/include/Game/Entities/ShijimiChou.h`
- Decomp sources: `src/plugProjectMorimuraU/shijimiChou.cpp`, `src/plugProjectMorimuraU/shijimiChouAnimator.cpp`, `src/plugProjectMorimuraU/shijimiChouMgr.cpp`, `src/plugProjectMorimuraU/shijimiChouState.cpp`
- Disc (`enemy/data/...`): `ShijimiChou/anim.szs`, `ShijimiChou/model.szs`
- AI size: 1666 lines, 6 states. States: `SHIJIMICHOU_Wait`, `SHIJIMICHOU_Fly`, `SHIJIMICHOU_Fall`, `SHIJIMICHOU_Dead`, `SHIJIMICHOU_Leave`, `SHIJIMICHOU_Rest`
- Sounds: `PSSE_EN_BUTTERFLY_HIT`. Id family: (none in the id table).
- Effects (`efx::`): `TChouDown`, `TChouHit`, `TEnemyDead`.

### MiniHoudai: Gatling Groink

- Class: NEW, tier 3. 11 states; turret.
- Header: `pikmin2-main/include/Game/Entities/MiniHoudai.h`
- Decomp sources: `src/plugProjectNishimuraU/FixMiniHoudai.cpp`, `src/plugProjectNishimuraU/FixMiniHoudaiMgr.cpp`, `src/plugProjectNishimuraU/MiniHoudai.cpp`, `src/plugProjectNishimuraU/MiniHoudaiAnimator.cpp`, `src/plugProjectNishimuraU/MiniHoudaiMgr.cpp`, `src/plugProjectNishimuraU/MiniHoudaiShotGun.cpp`, `src/plugProjectNishimuraU/MiniHoudaiState.cpp`, `src/plugProjectNishimuraU/NormMiniHoudai.cpp`, `src/plugProjectNishimuraU/NormMiniHoudaiMgr.cpp`
- Disc (`enemy/data/...`): `MiniHoudai/anim.szs`, `MiniHoudai/model.szs`
- AI size: 2746 lines, 11 states. States: `MINIHOUDAI_Dead`, `MINIHOUDAI_Rebirth`, `MINIHOUDAI_Lost`, `MINIHOUDAI_Attack`, `MINIHOUDAI_Flick`, `MINIHOUDAI_Turn`, `MINIHOUDAI_TurnHome`, `MINIHOUDAI_TurnPath`, `MINIHOUDAI_Walk`, `MINIHOUDAI_WalkHome`, `MINIHOUDAI_WalkPath`
- Sounds: `PSSE_EN_BOMB_FLY`, `PSSE_EN_MINIHOU_BOMB`. Id family: MINIHOUDAI_*, MINI_*.
- Effects (`efx::`): `TChibiCharge`, `TChibiDeadLight`, `TChibiDeadMouth`, `TChibiDeadSe`, `TChibiHit`, `TChibiShell`, `TChibiShoot`, `TChibiSmokeL`, `TChibiSmokeS`, `THdamaHit3`.

### OoPanModoki: Giant Breadbug

- Class: NEW, tier 3. Lives in `panModoki*.cpp` with the small Breadbug (P1 kind 19), on `PanModokiBase`; the counts are for the shared files.
- Header: `pikmin2-main/include/Game/Entities/OoPanModoki.h`
- Decomp sources: `src/plugProjectMorimuraU/panModoki.cpp`, `src/plugProjectMorimuraU/panModokiAnimator.cpp`, `src/plugProjectMorimuraU/panModokiMgr.cpp`, `src/plugProjectMorimuraU/panModokiState.cpp`
- Disc (`enemy/data/...`): `PanModoki/anim.szs`, `PanModoki/model.szs`
- AI size: 2402 lines, 0 states.
- Sounds: `PSSE_EN_ENEMY_LOOSE_ITEM`, `PSSE_EN_OPAN_DOWN_NEW`, `PSSE_EN_OPAN_HIPPARARE`. Id family: OOPAN_*, OPAN_*.
- Effects (`efx::`): `TPanApp`, `TPanHide`, `TPanSmoke`.

### LeafChappy: Bulbmin

- Class: NEW, tier 3. 235 lines on `KumaChappy`; a Pikmin-like follower, not a hostile.
- Header: `pikmin2-main/include/Game/Entities/LeafChappy.h`
- Decomp sources: `src/plugProjectNishimuraU/LeafChappy.cpp`, `src/plugProjectNishimuraU/LeafChappyMgr.cpp`
- Disc (`enemy/data/...`): `LeafChappy/anim.szs`, `LeafChappy/model.szs`
- AI size: 235 lines, 0 states.
- Sounds: none named in the sources. Id family: (Bulbear's).
- Effects (`efx::`): none.

### OniKurage: Greater Spotted Jellyfloat

- Class: NEW, tier 4. 12 states; larger jellyfloat that transforms.
- Header: `pikmin2-main/include/Game/Entities/OniKurage.h`
- Decomp sources: `src/plugProjectNishimuraU/OniKurage.cpp`, `src/plugProjectNishimuraU/OniKurageAnimator.cpp`, `src/plugProjectNishimuraU/OniKurageMgr.cpp`, `src/plugProjectNishimuraU/OniKurageState.cpp`
- Disc (`enemy/data/...`): `OniKurage/anim.szs`, `OniKurage/model.szs`
- AI size: 1807 lines, 12 states. States: `ONIKURAGE_Dead`, `ONIKURAGE_Wait`, `ONIKURAGE_Move`, `ONIKURAGE_Chase`, `ONIKURAGE_Attack`, `ONIKURAGE_Fall`, `ONIKURAGE_Drop`, `ONIKURAGE_Land`, `ONIKURAGE_Ground`, `ONIKURAGE_TakeOff`, `ONIKURAGE_FlyFlick`, `ONIKURAGE_GroundFlick`
- Sounds: `PSSE_EN_KURAGE_GET_PIKI`, `PSSE_EN_ONIKURAGE_AIR`, `PSSE_EN_ONIKURAGE_BOWEL`, `PSSE_EN_ONIKURAGE_DAMAGE`, `PSSE_EN_ONIKURAGE_GET_ORIMA`, `PSSE_EN_ONIKURAGE_HARETSU`, `PSSE_EN_ONIKURAGE_VOMIT`. Id family: ONIKURAGE_*.
- Effects (`efx::`): `TKurageBomb`, `TKurageDeadrun`, `TKurageEye`, `TKurageFlick`, `TKurageGepu`, `TKurageHire`, `TKurageKira`, `TKurageSui`.

### Queen: Empress Bulblax

- Class: NEW (boss), tier 4. 7 states, 1671 lines.
- Header: `pikmin2-main/include/Game/Entities/Queen.h`
- Decomp sources: `src/plugProjectNishimuraU/Queen.cpp`, `src/plugProjectNishimuraU/QueenAnimator.cpp`, `src/plugProjectNishimuraU/QueenMgr.cpp`, `src/plugProjectNishimuraU/QueenShadow.cpp`, `src/plugProjectNishimuraU/QueenState.cpp`
- Disc (`enemy/data/...`): `Queen/anim.szs`, `Queen/model.szs`, `Queen/queenchappy_model.btk`
- AI size: 1671 lines, 7 states. States: `QUEEN_Dead`, `QUEEN_Sleep`, `QUEEN_Wait`, `QUEEN_Damage`, `QUEEN_Flick`, `QUEEN_Rolling`, `QUEEN_Born`
- Sounds: `PSSE_EN_QUEEN_CRUSH`. Id family: QUEEN_*.
- Effects (`efx::`): `TQueenCrashL`, `TQueenCrashR`, `TQueenCrashRock`, `TQueenDamage`, `TQueenDead`, `TQueenFlick`, `TQueenHanacho`, `TQueenLay`, `TQueenRoll`, `TQueenRollCL`, `TQueenRollCR`, `TQueenWakeup`.

### KingChappy: Emperor Bulblax

- Class: NEW (boss), tier 4. 13 states, 2498 lines.
- Header: `pikmin2-main/include/Game/Entities/KingChappy.h`
- Decomp sources: `src/plugProjectMorimuraU/kingChappy.cpp`, `src/plugProjectMorimuraU/kingChappyMgr.cpp`, `src/plugProjectMorimuraU/kingChappyState.cpp`
- Disc (`enemy/data/...`): `KingChappy/anim.szs`, `KingChappy/model.szs`
- AI size: 2498 lines, 13 states. States: `KINGCHAPPY_Walk`, `KINGCHAPPY_Attack`, `KINGCHAPPY_Dead`, `KINGCHAPPY_Flick`, `KINGCHAPPY_WarCry`, `KINGCHAPPY_Damage`, `KINGCHAPPY_Turn`, `KINGCHAPPY_Eat`, `KINGCHAPPY_Hide`, `KINGCHAPPY_HideWait`, `KINGCHAPPY_Appear`, `KINGCHAPPY_Caution`, `KINGCHAPPY_Swallow`
- Sounds: `PSSE_EN_KING_APPEAR`, `PSSE_EN_KING_WATER_APPEAR`. Id family: KING_*.
- Effects (`efx::`): `TEnemyHamonChasePos`, `TKchApSand`, `TKchApWat`, `TKchAttackYodare`, `TKchCryAB`, `TKchCryInd`, `TKchDamage`, `TKchDeadHana`, `TKchDeadYodare`, `TKchDiveSand`, `TKchDiveWat`, `TKchDownsmoke`, `TKchFlickSand`, `TKchSmokeHana`, `TKchYodare`.

### Damagumo: Beady Long Legs

- Class: NEW (boss), tier 4. 6 states, 1615 lines.
- Header: `pikmin2-main/include/Game/Entities/Damagumo.h`
- Decomp sources: `src/plugProjectNishimuraU/Damagumo.cpp`, `src/plugProjectNishimuraU/DamagumoAnimator.cpp`, `src/plugProjectNishimuraU/DamagumoMgr.cpp`, `src/plugProjectNishimuraU/DamagumoShadow.cpp`, `src/plugProjectNishimuraU/DamagumoState.cpp`
- Disc (`enemy/data/...`): `Damagumo/anim.szs`, `Damagumo/damagumo.btk`, `Damagumo/damagumo_model.brk`, `Damagumo/damagumo_model.btk`, `Damagumo/model.szs`
- AI size: 1615 lines, 6 states. States: `DAMAGUMO_Dead`, `DAMAGUMO_Stay`, `DAMAGUMO_Land`, `DAMAGUMO_Wait`, `DAMAGUMO_Flick`, `DAMAGUMO_Walk`
- Sounds: `PSSE_EN_DAMAGUMO_RAISE1`, `PSSE_EN_DAMAGUMO_RAISE2`, `PSSE_EN_DAMAGUMO_RAISE3`, `PSSE_EN_DAMAGUMO_SMOKE`, `PSSE_EN_SPIDER_WALK`. Id family: DAMAGUMO_*, SPIDER_*.
- Effects (`efx::`): `TDamaDeadBomb`, `TDamaDeadElecA`, `TDamaDeadElecB`, `TDamaDeadHahenA`, `TDamaDeadHahenB`, `TDamaDeadHahenC1`, `TDamaDeadHahenC2`, `TDamaFoot`, `TDamaFootw`, `TDamaHahen`, `TDamaSmoke`, `TDamaWalk`, `TDamaWalkw`.

### BigFoot: Raging Long Legs

- Class: NEW (boss), tier 4. 6 states.
- Header: `pikmin2-main/include/Game/Entities/BigFoot.h`
- Decomp sources: `src/plugProjectNishimuraU/BigFoot.cpp`, `src/plugProjectNishimuraU/BigFootAnimator.cpp`, `src/plugProjectNishimuraU/BigFootMgr.cpp`, `src/plugProjectNishimuraU/BigFootShadow.cpp`, `src/plugProjectNishimuraU/BigFootState.cpp`
- Disc (`enemy/data/...`): `BigFoot/anim.szs`, `BigFoot/model.szs`, `BigFoot/ooashi_model.brk`
- AI size: 1758 lines, 6 states. States: `BIGFOOT_Dead`, `BIGFOOT_Stay`, `BIGFOOT_Land`, `BIGFOOT_Wait`, `BIGFOOT_Flick`, `BIGFOOT_Walk`
- Sounds: `PSSE_EN_BIGFOOT_RAISE1`, `PSSE_EN_BIGFOOT_RAISE2`, `PSSE_EN_BIGFOOT_RAISE3`, `PSSE_EN_BIGFOOT_RAISE_FAST`, `PSSE_EN_BIGFOOT_WALK`, `PSSE_EN_DAMAGUMO_SMOKE`. Id family: BIGFOOT_*.
- Effects (`efx::`): `TDamaDeadBomb`, `TDamaDeadElecA`, `TDamaDeadElecB`, `TDamaFootw`, `TDamaSmoke`, `TDamaWalkw`, `TOdamaDeadHahenA`, `TOdamaDeadHahenB`, `TOdamaDeadHahenC1`, `TOdamaDeadHahenC2`, `TOdamaFoot`, `TOdamaFur1`, `TOdamaFur2`, `TOdamaHahen`, `TOdamaWalk`.

### Houdai: Man-at-Legs

- Class: NEW (boss), tier 4. 7 states, 2819 lines.
- Header: `pikmin2-main/include/Game/Entities/Houdai.h`
- Decomp sources: `src/plugProjectNishimuraU/Houdai.cpp`, `src/plugProjectNishimuraU/HoudaiAnimator.cpp`, `src/plugProjectNishimuraU/HoudaiMgr.cpp`, `src/plugProjectNishimuraU/HoudaiShadow.cpp`, `src/plugProjectNishimuraU/HoudaiShotGun.cpp`, `src/plugProjectNishimuraU/HoudaiState.cpp`
- Disc (`enemy/data/...`): `Houdai/anim.szs`, `Houdai/model.szs`
- AI size: 2819 lines, 7 states. States: `HOUDAI_Dead`, `HOUDAI_Stay`, `HOUDAI_Land`, `HOUDAI_Wait`, `HOUDAI_Flick`, `HOUDAI_Walk`, `HOUDAI_Shot`
- Sounds: `PSSE_EN_DAMAGUMO_SMOKE`, `PSSE_EN_HOUDAI_BAREFOOT`, `PSSE_EN_HOUDAI_BEAM`, `PSSE_EN_HOUDAI_IMPACT`, `PSSE_EN_HOUDAI_METALFOOT`, `PSSE_EN_HOUDAI_MOTOR`, `PSSE_EN_HOUDAI_RAISE_B1`, `PSSE_EN_HOUDAI_RAISE_B2`, `PSSE_EN_HOUDAI_RAISE_B3`, `PSSE_EN_HOUDAI_RAISE_M1`, `PSSE_EN_HOUDAI_RAISE_M2`, `PSSE_EN_HOUDAI_RAISE_M3`, `PSSE_EN_HOUDAI_SHOT`, `PSSE_EN_HOUDAI_STEAM`, `PSSE_EN_HOUDAI_WATER_IMPACT`. Id family: HOUDAI_*, PONGASHI_*.
- Effects (`efx::`): `TDamaDeadBomb`, `TDamaDeadElecA`, `TDamaSmoke`, `TDamaWalkw`, `TEnemyDownSmoke`, `TEnemyDownWat`, `THdamaDeadHahen1`, `THdamaDeadHahen2`, `THdamaDeadSteam`, `THdamaDeadSteamT`, `THdamaDeadbomb`, `THdamaHahen`, `THdamaHit1`, `THdamaHit2`, `THdamaHit2W`, `THdamaHit3`, `THdamaOnHahen1`, `THdamaOnHahen2`, `THdamaOnSmoke`, `THdamaOnSteam1`, `THdamaOnSteam2`, `THdamaOpen`, `THdamaShell`, `THdamaShoot`, `THdamaSight`, `THdamaSteam`, `THdamaSteamBd`, `THdamaSteamSt`, `TOootaWalk`.

### DangoMushi: Segmented Crawbster

- Class: NEW (boss), tier 4. 9 states, 1904 lines.
- Header: `pikmin2-main/include/Game/Entities/DangoMushi.h`
- Decomp sources: `src/plugProjectNishimuraU/DangoMushi.cpp`, `src/plugProjectNishimuraU/DangoMushiMgr.cpp`, `src/plugProjectNishimuraU/DangoMushiState.cpp`
- Disc (`enemy/data/...`): `DangoMushi/anim.szs`, `DangoMushi/dangomushi.brk`, `DangoMushi/model.szs`
- AI size: 1904 lines, 9 states. States: `DANGOMUSHI_Dead`, `DANGOMUSHI_Stay`, `DANGOMUSHI_Appear`, `DANGOMUSHI_Wait`, `DANGOMUSHI_Move`, `DANGOMUSHI_Attack`, `DANGOMUSHI_Turn`, `DANGOMUSHI_Recover`, `DANGOMUSHI_Flick`
- Sounds: `PSSE_EN_DANGO_ARM_GROUND`, `PSSE_EN_DANGO_FALL`, `PSSE_EN_DANGO_ROLL_GROUND`. Id family: DANGO_*.
- Effects (`efx::`): `TDangoAttack2`, `TDangoCrash`, `TDangoDamage`, `TDangoDead`, `TDangoDeadSmoke`, `TDangoFly`, `TDangoRun`, `TDangoTurn`, `TDangoWallBreak`.

### SnakeCrow: Burrowing Snagret

- Class: NEW (boss), tier 4. 9 states.
- Header: `pikmin2-main/include/Game/Entities/SnakeCrow.h`
- Decomp sources: `src/plugProjectNishimuraU/SnakeCrow.cpp`, `src/plugProjectNishimuraU/SnakeCrowAnimator.cpp`, `src/plugProjectNishimuraU/SnakeCrowMgr.cpp`, `src/plugProjectNishimuraU/SnakeCrowShadow.cpp`, `src/plugProjectNishimuraU/SnakeCrowState.cpp`
- Disc (`enemy/data/...`): `SnakeCrow/anim.szs`, `SnakeCrow/model.szs`
- AI size: 1883 lines, 9 states. States: `SNAKECROW_Dead`, `SNAKECROW_Stay`, `SNAKECROW_Appear1`, `SNAKECROW_Appear2`, `SNAKECROW_Disappear`, `SNAKECROW_Wait`, `SNAKECROW_Attack`, `SNAKECROW_Eat`, `SNAKECROW_Struggle`
- Sounds: `PSSE_EN_SNAKE_TURN`. Id family: SNAKE_*.
- Effects (`efx::`): `THebiAphd_appear1`, `THebiAphd_appear2_first`, `THebiAphd_appear2_late`, `THebiAphd_dive`, `THebiDead`, `THebiDeadHane_ver01`, `THebiRot`, `THebiWait`.

### SnakeWhole: Pileated Snagret

- Class: NEW (boss), tier 4. 11 states.
- Header: `pikmin2-main/include/Game/Entities/SnakeWhole.h`
- Decomp sources: `src/plugProjectNishimuraU/SnakeWhole.cpp`, `src/plugProjectNishimuraU/SnakeWholeAnimator.cpp`, `src/plugProjectNishimuraU/SnakeWholeMgr.cpp`, `src/plugProjectNishimuraU/SnakeWholeShadow.cpp`, `src/plugProjectNishimuraU/SnakeWholeState.cpp`
- Disc (`enemy/data/...`): `SnakeWhole/anim.szs`, `SnakeWhole/model.szs`
- AI size: 2133 lines, 11 states. States: `SNAKEWHOLE_Dead`, `SNAKEWHOLE_Stay`, `SNAKEWHOLE_Appear1`, `SNAKEWHOLE_Appear2`, `SNAKEWHOLE_Disappear`, `SNAKEWHOLE_Wait`, `SNAKEWHOLE_Walk`, `SNAKEWHOLE_Home`, `SNAKEWHOLE_Attack`, `SNAKEWHOLE_Eat`, `SNAKEWHOLE_Struggle`
- Sounds: none named in the sources. Id family: SNAKEWHOLE_*.
- Effects (`efx::`): `TCphebiDead`, `TCphebiDeadHane`, `THebiAphd_appear1`, `THebiAphd_appear2_first`, `THebiAphd_appear2_late`, `THebiAphd_dive`.

### UmiMushi: Ranging Bloyster / Toady Bloyster

- Class: NEW (boss), tier 4. 10 states.
- Header: `pikmin2-main/include/Game/Entities/UmiMushi.h`
- Decomp sources: `src/plugProjectMorimuraU/umiMushi.cpp`, `src/plugProjectMorimuraU/umiMushiAnimator.cpp`, `src/plugProjectMorimuraU/umiMushiMgr.cpp`, `src/plugProjectMorimuraU/umiMushiState.cpp`, `src/plugProjectNishimuraU/UmimushiShadow.cpp`
- Disc (`enemy/data/...`): `UmiMushi/anim.szs`, `UmiMushi/model.szs`, `UmiMushi/umimusi_model1.btk`
- AI size: 2279 lines, 10 states. States: `UMIMUSHI_Wait`, `UMIMUSHI_Walk`, `UMIMUSHI_Find`, `UMIMUSHI_Search`, `UMIMUSHI_Turn`, `UMIMUSHI_Flick`, `UMIMUSHI_Attack`, `UMIMUSHI_Eat`, `UMIMUSHI_Dead`, `UMIMUSHI_Lost`
- Sounds: `PSSE_EN_UMI_DEAD`, `PSSE_EN_UMI_DEAD_GND`, `PSSE_EN_UMI_SEARCH_LUGIE`, `PSSE_EN_UMI_SEARCH_ORIMER`, `PSSE_EN_UMI_ZURUZURU`. Id family: UMI_*.
- Effects (`efx::`): `TUmiAttack`, `TUmiDeadawa`, `TUmiDeadmelt`, `TUmiEat`, `TUmiEyeBlue`, `TUmiEyeRed`, `TUmiFlick`, `TUmiHamon`, `TUmiWeakBlue`, `TUmiWeakRed`.

### BigTreasure: Titan Dweevil

- Class: NEW (boss), tier 4. 12 states, 5063 lines.
- Header: `pikmin2-main/include/Game/Entities/BigTreasure.h`
- Decomp sources: `src/plugProjectNishimuraU/BigTreasure.cpp`, `src/plugProjectNishimuraU/BigTreasureAttack.cpp`, `src/plugProjectNishimuraU/BigTreasureMgr.cpp`, `src/plugProjectNishimuraU/BigTreasureShadow.cpp`, `src/plugProjectNishimuraU/BigTreasureState.cpp`
- Disc (`enemy/data/...`): `BigTreasure/anim.szs`, `BigTreasure/model.szs`
- AI size: 5063 lines, 12 states. States: `BIGTREASURE_Dead`, `BIGTREASURE_Stay`, `BIGTREASURE_Land`, `BIGTREASURE_Wait`, `BIGTREASURE_ItemWait`, `BIGTREASURE_Flick`, `BIGTREASURE_PreAttack`, `BIGTREASURE_Attack`, `BIGTREASURE_PutItem`, `BIGTREASURE_DropItem`, `BIGTREASURE_Walk`, `BIGTREASURE_ItemWalk`
- Sounds: `PSSE_EN_BIGTAKARA_DROP_ITEM`, `PSSE_EN_BIGTAKARA_EP_BOUND`, `PSSE_EN_BIGTAKARA_EP_BREAK`, `PSSE_EN_BIGTAKARA_E_SPARK`, `PSSE_EN_BIGTAKARA_E_SUSTAIN`, `PSSE_EN_BIGTAKARA_FIRE_BODY`, `PSSE_EN_BIGTAKARA_FIRE_ROOT`, `PSSE_EN_BIGTAKARA_FIRE_TAIL`, `PSSE_EN_BIGTAKARA_G_SPOUT`, `PSSE_EN_BIGTAKARA_G_SPRAY1`, `PSSE_EN_BIGTAKARA_G_SPRAY2`, `PSSE_EN_BIGTAKARA_G_SPRAY3`, `PSSE_EN_BIGTAKARA_G_SPRAY4`, `PSSE_EN_BIGTAKARA_RAISE`, `PSSE_EN_BIGTAKARA_SHELL`, `PSSE_EN_BIGTAKARA_WAIT2`, `PSSE_EN_BIGTAKARA_WALK`, `PSSE_EN_BIGTAKARA_W_GROUND`, `PSSE_EN_BIGTAKARA_W_SHOT`. Id family: BIGTAKARA_*.
- Effects (`efx::`): `TDamaDeadBomb`, `TDamaFootw`, `TDamaSmoke`, `TDamaWalkw`, `TOootaBombBody`, `TOootaBombLeg`, `TOootaChangeBody`, `TOootaChangeLeg`, `TOootaDeadAwa`, `TOootaDeadBody`, `TOootaDeadLeg`, `TOootaElec`, `TOootaElecAttack1`, `TOootaElecAttack2`, `TOootaElecLeg`, `TOootaElecparts`, `TOootaFire`, `TOootaFoot`, `TOootaGas`, `TOootaParticle`, `TOootaPartsoff`, `TOootaPdead`, `TOootaPhouden`, `TOootaStartBody`, `TOootaStartLeg`, `TOootaStartOta`, `TOootaStartSmoke`, `TOootaWalk`, `TOootaWbHit`, `TOootaWbShot`, `TOootaWbomb`.

### BlackMan: Waterwraith (with Tyre rollers)

- Class: NEW (boss), tier 4. 10 states + Tyre (4 states).
- Header: `pikmin2-main/include/Game/Entities/BlackMan.h`
- Decomp sources: `src/plugProjectMorimuraU/blackMan.cpp`, `src/plugProjectMorimuraU/blackManAnimator.cpp`, `src/plugProjectMorimuraU/blackManMgr.cpp`, `src/plugProjectMorimuraU/blackManState.cpp`, `src/plugProjectMorimuraU/tyre.cpp`, `src/plugProjectMorimuraU/tyreAnimator.cpp`, `src/plugProjectMorimuraU/tyreMgr.cpp`, `src/plugProjectMorimuraU/tyreState.cpp`, `src/plugProjectNishimuraU/TyreShadow.cpp`
- Disc (`enemy/data/...`): `BlackMan/anim.szs`, `BlackMan/kagebozu_model.btk`, `BlackMan/model.szs`, `Tyre/anim.szs`, `Tyre/model.szs`
- AI size: 3883 lines, 14 states. States: `WRAITH_Walk`, `WRAITH_Dead`, `WRAITH_Freeze`, `WRAITH_Bend`, `WRAITH_Escape`, `WRAITH_Fall`, `WRAITH_Flick`, `WRAITH_Recover`, `WRAITH_Tired`, `WRAITH_Count`
- Sounds: `PSSE_EN_KAGE_MELODYLOOP`, `PSSE_EN_KAGE_REJECT`, `PSSE_EN_KAGE_ROLLER`, `PSSE_EN_KAGE_ROLLER_LAND`, `PSSE_EN_KAGE_ZURUZURU`, `PSSE_EN_TIRE_FALL`. Id family: KAGE_*.
- Effects (`efx::`): `TEnemyHamonChasePos`, `TKageBend1`, `TKageDead1`, `TKageDead2`, `TKageFlick`, `TKageMove`, `TKageRecov`, `TKageRun`, `TKageTyredead`, `TKageTyresmoke`, `TKageTyreup`, `TKchApWat`.

### Tyre: Waterwraith roller

- Class: part of BlackMan, tier 4. Spawned by BlackMan.
- Header: `pikmin2-main/include/Game/Entities/Tyre.h`
- Decomp sources: `src/plugProjectMorimuraU/tyre.cpp`, `src/plugProjectMorimuraU/tyreAnimator.cpp`, `src/plugProjectMorimuraU/tyreMgr.cpp`, `src/plugProjectMorimuraU/tyreState.cpp`, `src/plugProjectNishimuraU/TyreShadow.cpp`
- Disc (`enemy/data/...`): `Tyre/anim.szs`, `Tyre/model.szs`
- AI size: 1104 lines, 4 states. States: `TYRE_Move`, `TYRE_Land`, `TYRE_Freeze`, `TYRE_Dead`
- Sounds: `PSSE_EN_KAGE_ROLLER_LAND`. Id family: KAGE_ROLLER*.
- Effects (`efx::`): `TEnemyHamonChasePos`, `TKageRecov`, `TKageTyredead`, `TKageTyresmoke`, `TKchApWat`.

## Status

Nothing ported yet (see the bottom of this file once stages land).

## Port status (2026-10-28)

Done through core, tests, draw path and Ship Repair roster wiring (staged by `ror2-olimar/build.ps1`; never played):
- **Kogane family** (kinds 27-29: Iridescent Flint Beetle, Iridescent Glint Beetle, Doodlebug): `libpikmin-teki/src/kogane.rs` + `p2.rs`
  (5 states, disc parms and clip keys, drops, Doodlebug gas), 7 tests in `tests/p2_kogane.rs`; enemy extractor
  `libsms::extract::p2_enemy` (own marker `p2enemy.ok`); body loader `libsms-present::enemy`; draw via `libpikmin-present::p2enemy`;
  sounds on the Pikmin 2 output (ids audible: `libsms-audio/tests/pikmin2_enemy_sounds.rs`); FFI `pik_p2_enemies_*`, no ABI change.
- Not verified: anything in game; the ffi/present suites pass except `stings::intro_is_audible...`, which depends on another
  session's uncommitted audio edits.
- Missing: JPA2 effects (none emitted), the variants' TEV konst-colour tweak, cave treasure drops, wall normal, stone/bitter state,
  Doodlebug panic (Pikmin in the gas die at once), spawn weights/costs untuned (2/1/2, 10/20/15).
- Not started: every other enemy in the table above, and all bosses.

### Update after the second pass (2026-10-28)

Ported through core, tests, draw path and Ship Repair roster wiring (kinds in `libpikmin-teki::p2`):
- 30 **Bulborb Larva** (`baby.rs`, 3 tests): hatch, chase the nearest Pikmin/player in sight (800), bite (2 damage, hits Pikmin at its mouth
  instead of the mouth-slot swallow), dies when hurt or pressed with a 20% nectar drop. Leaves no corpse (`EB_LeaveCarcass` off).
- 31 **Skitter Leaf** (`sokkuri.rs`, 3 tests): hidden until a player is in sight, ground/water skittering, waits, flick after
  `ip07`+1 hits, back under its leaf when home and alone, press/death. **Leaves the disc's corpse**: `carcass_config.txt` (radius,
  carriers, seeds, offset) is copied by the enemy extractor (marker v2) and loaded into `TekiInfo`; the dying clip is the corpse's pose;
  the host (`TekiBodies.Died`, `TekiCorpses`) makes the pellet and draws the model on it exactly as for Pikmin 1 corpses.

Corpse rule for the rest: `Kind::carcass` names the enemy's entry in `carcass_config.txt` (None where the decomp turns
`EB_LeaveCarcass` off: Kogane family, Baby, Mitite, GasHiba, Hanachirashi, Kurage), `Kind::dead` the clip it lies in. Enemies that do leave
one: Sokkuri, ElecBug, Tank/Wtank, Kabuto, Sarai/Demon, Fuefuki, and the bosses.

Verification: all `libpikmin-teki` tests, the FFI draw/run test and `build.ps1` pass; the staged plugin is current. Never played.

**Not done (in survey order):** Wtank (+ P2 Tank, 7 states, leaves a corpse), Mitite, GasHiba, ElecHiba, Bomb-rock, Egg, ElecBug, Demon (+ P2 Sarai),
Kabuto, then tiers 3 and 4 and every boss. The disc files for all of them (bosses included) are already in the extracted cache
(`assets/p2/enemy/data/<Name>/`), so each remaining enemy needs only its `Kind` row, strategy module, tests and roster entry. Bosses
additionally need hull and hurtbox work in `TekiBodies`/`TekiBody` and are not started.

### Update after the third pass (2026-10-28)

Ported through core, tests, draw path and Ship Repair wiring (all staged by `build.ps1`, never played):
- 32 **Mitite** (`mitite.rs`, 4 tests): hidden until found, the leader calls ten fellows (`Out::Spawn`; the host marks them with
  `count[0] = 1` so they do not call more), wander in walks and turns, dive away after 8-10 s, a hit kills them with a nectar drop and the disc's corpse.
- 33 **Gas pipe** (`gashiba.rs`, 2 tests): vents at once (the disc's wait time is zero), the cloud kills non-White Pikmin, not players; a fixture
  Ship Repair places each stage after the first (`GasPipesPerStage`), unkillable like the Fire Geyser.
- 34 **Bomb-rock** (`bomb.rs`, 1 test): lies still until hit, burns the 4.5 s fuse, blows up players and Pikmin in 90 units. Damage to other
  enemies and chain reactions are left out. Unkillable: it goes off instead.
- 35 **Watery Blowhog** (`wtank.rs`, 3 tests): patrol, chase turn, a growing water stream (the Fiery Blowhog's flame cylinder with a damage of its
  own; the disc's bubble damage is zero), flick, death with the disc's corpse (7-15 carriers, 8 seeds). On the Fiery Blowhog's model with the water texture.

Judged impractical for now (reason):
- **ElecHiba, ElecBug (Anode Beetle)**: both work as pairs (an electrode with a child; bugs that find a partner and arc between them).
  The enemy framework has no way to link two enemies' states; it needs a partner link in `Tekis` first.
- **Demon / Sarai (Snitchbugs)**: they carry a captain off through the air (`InteractSarai`, mouth slots); a RoR2 player cannot be carried
  by the sim. P1's Snitchbug (kind 8) already covers the creature.
- **Egg**: only meaningful with the Empress (it hatches Larvae and Mitites); comes with the Queen if she is ported.

Still to do, in survey order: Kabuto (and Rkabuto/Fkabuto), Fuefuki, then tier 3 (Bulborb family, Fiery Bulblax, Armor, Hanachirashi, Kurage,
BombSarai, Dweevils, Imomushi, Jigumo, ShijimiChou, MiniHoudai, OoPanModoki, LeafChappy) and every boss. Of the bosses, the survey marks
Queen/KingChappy (a big Bulborb body), OniKurage (after Kurage) and the Long Legs (Damagumo, BigFoot) as the practical ones for Ship Repair.

### Update after the fourth pass (2026-10-28)

- 36 **Armored Cannon Beetle Larva** (`kabuto.rs`, 3 tests; ported through core, tests, draw path, roster): wander (`Wait`/`Turn`/`Move`), turn on whatever it
  sees, spit Pikmin 1's rolling boulder (kind 20) along a narrow lane ahead, flick, death with the disc's corpse (7-15 carriers, radius 34).
  The fixed turret variants (`Fix*`, the Decorated and fixed Cannon Beetles) are placed fixtures and not ported.
  `isAttackableTarget`'s lane (15 wide, 15 to sight radius long) is from the decomp; the stone's speed is a guess (250), unverified.
- **Fuefuki (Antenna Beetle)** read but not ported: its `Whisle` state summons other enemies from the stage's table, which Ship Repair
  does not have; it would be a call for a random ported kind (`Out::Spawn`), which is a design choice for the user.
- **Bulborb family** (`ChappyBase`, 8 states plus ~1200 lines of Chappy/Sleep/Walk/Attack code, six variants) is the next big win and the
  next thing to do; **bosses** are untouched.

Counts: Kogane family (3), Larva, Skitter Leaf, Mitite, Gas pipe, Bomb-rock, Watery Blowhog, Cannon Larva = 10 Pikmin 2 kinds (27-36)
ported and wired of the survey's ~40 new enemies; ElecHiba, ElecBug and Demon judged impractical (above); the rest and all bosses remain.

### Update after the fifth pass (2026-10-28)

Ported through core, tests, draw path and Ship Repair roster (never played):
- 37-40 **Orange, Hairy, Dwarf Orange and Snow Bulborbs** (`bulborb2.rs`, 4 tests with the Fiery one): the `ChappyBase` machine (sleep at home, wake, turn, walk, bite, flick,
  turn home, go home, die with the disc's corpse), on the Red Bulborbs' model with their own textures (`EnemyBody::load_swapped`: textures 0 and 1) and numbers. The Orange
  one wakes when anyone is within its wake radius, the others when hit. The dwarfs run the same machine (the decomp's `KochappyBase` has a `Wait` for the sleep, a `Press`
  squash, a short absent-minded pause: approximated; a pressed dwarf plays its press clip).
- 41 **Fiery Bulblax**: that machine plus the burning body (`FireChappy::collisionCallback`: the body burns Pikmin and players that touch it until water puts it out).
- 42 **Empress Bulblax** (boss, `queen.rs`, 4 tests): sleeps and lays larvae (kind 30, a fifth of the disc's 25-50 limit), wakes when hurt, shakes off a mob, rolls across her
  territory crushing and crashing at its ends, dies with the disc's corpse (20-30 carriers, 30 seeds). Roster: always drops a ship part, cost 900, on the Golem body.
- Bug fixed on the way: only the Kogane family's parm file has an `mScale`; every other Pikmin 2 kind had loaded at scale 0.01 (drawn at 1% size, with tiny bounds).
  Kinds 30-36 are drawn at the model's size now.

Still to do: the Emperor Bulblax (`KingChappy`, 13 states, 2498 lines: the next practical boss), Kurage then OniKurage, Fuefuki (needs a design choice, above), Armor, Hanachirashi, BombSarai, the
Dweevils, Imomushi, Jigumo, ShijimiChou, MiniHoudai, the Long Legs, and the bosses the survey marks hard. Nothing in this file has been played.
