# libpikmin behavior spec

Written from the projectPiki Pikmin 1 decomp (GPIE01_01, 100% matching) as a behavioral
spec, in our own words. Citations are `file: function` under `pikmin-main/src/`
(Kando's gameplay code is in `plugPikiKando/`). Units are decomp units (roughly cm).

Status: **implemented** = in `crates/libpikmin` with tests; **planned** = not yet.

## Host extension: jumping (implemented)

This mod adds voluntary jumps rather than reproducing a Pikmin 1 control. A rising edge
of `CaptainInput.jump` launches a grounded, active captain; `jump_blocked` suppresses it.
Gravity is read from the disc as usual, with a 40-unit jump height. Standing squad members
hop in place. Moving squad members queue the captain's takeoff positions and launch there
using his takeoff direction and horizontal speed. Up to 16 points are retained for 15 seconds;
leaving the squad clears them. Held, thrown, latched, knocked-down and busy Pikmin keep
their existing state behavior. Locomotion jumps preserve the current job and resume it on landing.

Navigation checks for low obstructions or short gaps, an unobstructed raised body, and a
reachable landing before jumping. Pikmin may jump while approaching a carrying slot,
but never during lifting or carrying. These are local probes, not a general path planner.

Host jump pads supply full launch velocity and gravity through `Sim::launch_captain`.
Following squad members queue that exact takeoff position and repeat its ballistic arc,
including vertical pads; other captains' squads and busy Pikmin stay on their own jobs.
Ordinary knockback does not queue a squad launch. Ground contact restores normal walking.
`set_captain_hold` holds voluntary movement for roots and actions as well for stuns;
external launches and gravity still run. These are host extensions, not Pikmin 1 behavior.

Presentation uses the extracted shared animation bundle: `OCARRY` (motion 12, AnimInfo 45)
is started by `NaviGeyzerState::init` in `plugPikiKando/naviState.cpp`; Pikmin use `ROLLJMP`
(motion 34, AnimInfo 61), started by `PikiFlyingState::init` in `plugPikiKando/pikiState.cpp`.
Both are already part of first-run extraction. The RoR2 Pikmin scheme binds B to jump,
LB + B to whistle and suppresses jumping for the entire LB hold. Menu closing consumes B.
The viewer uses J for this extension. ABI 9 uses the two former input-padding bytes for
`jump` and `jump_blocked`; Rust and C# must be rebuilt together.

## 0. Tuning values: code defaults vs the disc

The `Parm<T>` defaults in the headers are *not* what ships. At load, `Parameters::read`
(plugPikiColin/parameters.cpp) overrides them from `dataDir/parms/*.bin`:

- Format: big-endian entries, `header = (3-char id << 8) | size`, then the value. A header of
  `-1` ends a block. Prop files are a `CreatureProp` block followed by the object's block.
- A value goes to the **first** parameter with that id in declaration order. Unknown ids are skipped by size.
  (NaviMgr.h declares `p48` twice; the file's `p48` lands on `mCursorMoveDelayTime`.)

libpikmin reads these from the user's disc (`Params::load_p1_dir` / `apply_*_bin`) and falls
back to code defaults. Values that matter so far (GPIE01_01 disc vs code default):

| Param | Disc | Code | Used for |
|---|---|---|---|
| Navi p04/p56 move/run speed | 160 / 160 | 170 / 210 | captain speed (always run speed, see §2) |
| Navi s03 accel | 0.1 | 0.2 | captain velocity easing |
| Navi p01 whistle max radius | 90 | 100 | §3 |
| Navi p02 whistle expand time | 0.45 s | 0.2 s | §3 |
| Navi p03 whistle hold time | 1.0 s | 0.15 s | §3 |
| Navi p10/p11 throw distance max/min | 140 / 140 | 600 / 150 | unused by the actual throw (§5) |
| Navi p24/p25 throw height max/min | 75 / 75 | 100 / 80 | §5 |
| Navi p54 yellow throw height | 115 | 200 | §5 |
| Navi p37 pluck/grab range | 15 | 80 | §5 |
| Navi p21 body radius | 8 | 15 | collision |
| Navi p44 cursor-move stick threshold | 0.65 | 0.75 | §2 |
| Navi p45/p46 cursor min/max radius | 0 / 100 | 50 / 300 | §2 |
| Navi p47 cursor speed | 300 | 200 | §2 |
| Navi p48 cursor move delay | 0.85 s | 2.0 s | §2 |
| Piki p00/p01 walk/run speed | 10 / 120 | 84 / 162.5 | §4 |
| Piki s02/s03 faceDirAdjust/accel | 0.8 / 0.14 | 0.25 / 0.1 | piki motion |
| Piki p27/p34 size min/max | 3 / 5 | 6 / 10 | body radius |
| Piki p41 idle attack search range | 70 | 100 | §6 |
| Piki p43/p74 flick knockdown max/min | 3 / 1 s | 0.7 / 0.7 s | §7 |
| cunit p00/p01/p02 offset/length/slot | 17.5 / 90 / 7 | 30 / 150 / 12 | §4 |
| aiconst p00 gravity | 560 | 550 | everything airborne |
| aiconst p01 auto attack | 0 | 1 | §6 |
| aiconst p08 return after attack | 0 | 1 | §6 |
| aiconst p10 attack once | 0 (until death) | 1 | §6 |
| aiconst p15 field limit | 100 | 100 | spawn cap |

Attack power (red 15, blue/yellow 10) matches between disc and code.

## 1. Time step — implemented

In-game sections run at 30 Hz: `NewPikiGameSection` calls `gsys->setFrameClamp(2)`
(plugPikiColin/newPikiGame.cpp), and `System::updateSysClock` (sysDolphin/system.cpp) clamps
the frame delta to 1/30 s. We tick exactly 1/30 s and accumulate host time (max 4 ticks per frame).

## 2. Captain (Navi) movement and cursor — implemented

`Navi::makeVelocity` (navi.cpp):
- Stick axes are clamped to ±1. Target velocity = stick × `mRunSpeed`, because
  `CPlate::canNaviRunFast` always returns true.
- Velocity eases toward the target: `vel += (target − vel) × dt / accel` (`Creature::moveVelocity`, creature.cpp).
  Facing turns toward velocity by `faceDirAdjust` per tick.
- **Cursor** is an XZ offset from the captain. It moves at `mCursorMoveSpeed` along the
  *normalized* stick direction. Past `mCursorMaxRadius` the radial component is removed, so it
  slides around the circle. It doesn't recentre when the stick is released.
- **Light tilt aims**: if the stick magnitude is above the neutral threshold but
  ≤ `mCursorMoveStickThreshold` (or the stick has been neutral for `mCursorMoveDelayTime`), the
  captain stops and turns 20%/tick toward the cursor instead of walking.
- The cursor's world position is snapped to the ground + 1 (`Navi::refresh`).

## 3. Whistle — implemented (with our multi-captain rule)

`NaviGatherState::exec` (naviState.cpp), `Navi::callPikis` (navi.cpp):
- Pressing starts the circle at `mWhistleMinRadius`, growing linearly to `mWhistleMaxRadius`
  over `mWhistleExpandTime`. **Nothing is called while it grows.**
- If held to full size, it calls at max radius every tick for `mWhistleHoldTime`, then ends.
  Releasing ends it.
- Releasing early calls once at the current radius.
- The circle is centred on the **cursor**, not the captain. Distance is XZ only.
- Pikmin that are idle, attacking (latched ones let go) or knocked down join the squad.
- **Ours, not the decomp's:** Pikmin 1 only recruits Pikmin whose `mNavi` is this captain or
  null. libpikmin supports N captains, and a whistle also takes members of *other* captains'
  squads (`Event::Recruited { stolen_from }`). Pikmin in flight or in someone's hand can't be taken.
- Not yet: the `LookAt` reaction delay, whistling buried sprouts, waking drowning or burning Pikmin.

## 4. Formation (CPlate) and following (ActCrowd) — implemented

`CPlate` (cPlate.cpp):
- Inner radius = 2.1·slot·√(n/π), and base = 1.9·slot. Plate half-width `size` = the larger
  of the two (C-stick spread not implemented). Half-length = 4·n·slot²/(size·π), capped at
  `mLengthLimit`, after which the width grows instead.
- Slots are laid in rows from the back of the ellipse (z = −length) forward, row spacing 2·slot.
  Each row has 2k+1 slots, where k = ⌊half-chord / 2·slot⌋, filled alternately left→right and right→left.
- `setPos`: plate centre = captain + dir(angle) × (length + offset). Offset is `mStartOffset`
  unless the squad is moving faster than 5.
- New members take the last slot. Leaving shifts later members down.

`Navi::makeCStick` (C-stick neutral branch):
- The plate faces captain-facing + π while the captain is moving or holding a throw. Otherwise it keeps its last direction.
- When the nearest squad member is beyond `mPikiFormationChangeRange`, slots are re-sorted by
  distance to the captain (`CPlate::rearrangeSlot`) once per approach.

`ActCrowd::exec` (aiCrowd.cpp):
- Within 7 of the slot (or within 15 while settling), a Pikmin stops and faces the captain. Between 7 and 15 it brakes to arrive. Beyond that it runs at `getSpeed(1)`.
- Pikmin on the far side of the captain from the plate add a sideways component (sign by slot
  parity) to arc around him.
- Slip/break: beyond `mFormationSlipRange` a Pikmin moves to the last slot and starts a lost
  timer. Past `mMaxLostChildTime`, or beyond `mFormationBreakRange`, it leaves the squad.
- `Piki::getSpeed(r)` = walk + (max − walk)·r, where max depends on leaf/bud/flower if aiconst p14 is set.
- Not yet: trips, boredom animations, huddling when idle, C-stick formation steering.

## 5. Throwing — implemented

`NaviThrowWaitState`, `NaviThrowState` (naviState.cpp), `Navi::throwPiki` (navi.cpp):
- Pressing throw picks the nearest squad member in the normal state within 80. Pikmin in front
  of the captain count as 10 farther.
  - Within `mPluckGrabRange` it's taken into the hand.
  - Otherwise it walks over (`GoHang`), with a 3 s timeout.
    Host lock-on extension: while an enemy lock has a live stickable target, the called
    Pikmin closes the distance to the hand in one simulation tick, with immediate
    acceleration and normal wall collision. This gives the fast hand supply of C-stick-up throwing;
    press/release controls and thrown flight time stay the same. Losing the lock restores
    normal pickup speed.
  - If nobody is in range, the original punches (not implemented).
- Releasing throws the held Pikmin **at the cursor**. Distance parameters p10/p11 are computed
  but never used.
- Trajectory from captain + (0, 10, 0):
  - half = flightTime/2
  - height = yellow ? `mYellowThrowHeight` : lerp(min, max height, holdTime / maxHoldTime)
  - vy = g·half/2 + height/half
  - vxz = dist / (2·half), toward the cursor
  - the captain's velocity is added
- Thrown Pikmin leave the squad.
- Not yet: the throw animation's release-frame delay (we throw on the release tick), flower
  gliding (`PikiFlyingState::exec`), bomb rocks, auto-pick of the next Pikmin.

## 6. Landing and attacking — implemented

`PikiFlyingState::procCollideMsg` / `procBounceMsg` (pikiState.cpp), `Piki::actOnSituaton` and
`graspSituation` (piki.cpp), `ActAttack` and `ActJumpAttack` (aiAttack.cpp):
- **Thrown into a creature:** touching a stickable part **latches** the Pikmin onto that part's
  surface (`startStickObject`), and it starts attacking (`ActJumpAttack` state 5). Non-stickable
  parts deflect it.
- **Landing on the ground:** the Pikmin goes free, then looks for the nearest living,
  non-flying creature within `mIdleAttackSearchRange` + that creature's size. If one is found, it attacks.
- **Approach (state 0):** run at the nearest part. When facing it (< π/10) and within
  part radius + own size + 10 in 3D, it swings (state 4). When within reach horizontally, there's
  a 30%-per-tick chance of switching to jump-prep (state 2).
- **Jump prep (2):** back off or close in at half speed to the [size, size+6] band. Once facing,
  it jumps with velocity (100·dir.xz, 200). Touching the target in the air latches it. Landing returns to state 0.
- **Swing (4):** plays "ATTACK". Between `KEY_Action0` and `KEY_Action1` the hit connects if
  within 10 of the part surface and facing within π/4; otherwise it retries next tick. When the
  animation finishes, the Pikmin goes back to state 0, which swings again if still in reach.
- **Latched (5):** rides the part (offset kept in the creature's yaw frame) and plays the
  looping "KUTTUKU" cling animation, hitting once per loop.
- Each hit is `InteractAttack` → `Host::damage(creature, power, part)`. Power: red 15,
  blue 10, yellow 10 (`Piki::getAttackPower`).
- **aiconst p10** (`_C4`): 1 = after one *swing* the attack ends, 0 = keep swinging until the
  target dies. Retail is 0. **It never affects latched Pikmin:** the cling animation loops, so
  `KEY_Finished` never fires and state 5 never returns. Latched Pikmin hit until shaken off or the
  target dies, whatever p10 says.
- **aiconst p08:** return to the squad after attacking if within `mPostWorkJoinPartyRange`. Retail is 0, so they stay free.
- **aiconst p01:** idle Pikmin look for targets on their own. Retail is 0.
- Target dead, gone, or (for non-latched) flying: latched Pikmin fall off. All go free.

### Attack timing (animation keys)

Hit timing is driven by animation key events, not code constants. `PaniAnimator::checkConstantKey`
(plugPikiNakata/panianimator.cpp) maps an AnimInfo's info keys to events:
- type 0 = loop start, type 1 = loop end (jump back to loop start unless finishing);
- types 2–5 = `KEY_Action0`–`KEY_Action3`.

A motion plays from anim key 0 to key 8 (clamped) at `mMotionSpeed` = 30 frames/s, the speed for
every non-locomotion motion (`Piki::updateWalkAnimation`). That's one frame per tick.

The keys are in `pikis/animMgr.bin` (`AnimMgr::read`, `AnimInfo::doread`, plugPikiColin/animMgr.cpp).
The motion → AnimInfo mapping is in `PaniPikiAnimator::createMotionTable`. libpikmin parses the file
from the user's disc (`Params::apply_anim_mgr_bin`). The GPIE01_01 values, also used as fallback defaults:

| Motion | AnimInfo | Anim keys | Info keys | Result |
|---|---|---|---|---|
| ATTACK (23) | 18 `attack1.dca` | 0, 3, 5, 19 | Action0 @3, Action1 @5 | hit window frames 3–5, swing lasts 19 frames (0.63 s) |
| KUTTUKU (48) | 71 `kuttuku.dca` | 0, 11, 17, 30, 34 | loop 11→30, Action0 @11, Action1 @17 | first hit 11 frames after latching (0.37 s), then every 19 frames (0.63 s) |

- `getNearestCollPart(pos, '*t**')` is approximated as "nearest stickable part".
- Not yet: climbing platform parts (`doClimb`), bosses' special parts, punching.

## 7. Shake-off — implemented (host-triggered)

Enemies flick Pikmin off with their own AI (teki strategies). The host calls `Sim::shake_off(creature)`.
Every Pikmin latched to it is thrown outward (120 horizontal, 150 up) and lies down for a random
time between `mMinFlickKnockDownTime` and `mMaxFlickKnockDownTime` (`PikiFlickState`), then goes
free. A whistle wakes them immediately.

## 8. Collision — simplified

The original resolves creature-vs-creature collision with inverse masses (Navi p22, Piki p28).
We push grounded Pikmin apart on XZ, and push them and captains out of creature parts and
captains. Pikmin walking to the hand ignore the captain's body.

## 9. Corpses and pellets — implemented

A dead enemy becomes a pellet: `BTeki::dieSoon` → `becomePellet(typeID)` at its `'carc'`
collision sphere (plugPikiNakata/tekibteki.cpp). From then on it's a normal carryable with a
`PelletConfig` (pelletMgr.cpp): min/max carriers (p01/p02) and Onion seeds (p06 matching, p07
otherwise; negative = fireworks).
- Hosts call `Sim::spawn_pellet(PelletDesc)` when a creature dies, choosing min/max carriers and
  seeds (Pikmin 1's own table is in `pelMgr.bin`). They then drive their corpse object from
  `Pellet::{pos, yaw, scale}` until `Event::PelletDelivered`.
- Carry slots: `max_carriers` evenly spaced around the bottom radius, angle `TAU/max·i`
  (`Pellet::getSlotLocalPos`). Corpses have no colour, so they always give `seeds_matching`
  (the retail corpse configs have equal p06/p07 anyway).

## 10. Carrying (ActTransport) — implemented

aiTransport.cpp:
- **Starting.** A thrown Pikmin that touches a pellet with a free slot starts carrying it
  (`PikiFlyingState::procCollideMsg`). So does a landed one, if a pellet is the nearest job
  within `mIdleWorkSearchRange` of its edge (`graspSituation` compares enemies and pellets by distance).
- **Go.** Head for a random free slot (`getRandomFreeSlotIndex`). After 60 ticks within 100 of
  it, switch to the nearest free slot, or give up if that's the same one. Within 6, take the slot
  if it's free, else the nearest free one.
- **Lift.** Hold the slot playing PICKLOOP. Its `KEY_Action0` (frame 10) marks this carrier
  lifted (`mIsLiftActionDone`). When lifted carriers ≥ min, the stick leader decides the goal and
  the pellet starts moving (`doLift`).
- **Goal (`decideGoal`).** The majority colour among carriers (ties at random) picks the Onion.
  With several Onions accepting that colour we take the nearest. If none accepts it, we take the
  nearest Onion and use its colour. Pikmin 1 decides only at lift time. We also re-decide (and
  re-path) whenever carriers join or leave and the goal no longer takes a majority colour, the
  way Pikmin 2 keeps live colour counts (`Pellet::onSlotStickStart`/`End`, `getPelletGoal`).
- **Speed.** Carriers count 1 + 0.5·leaf stage, so leaf 1, bud 1.5, flower 2 (`getCarriers`).
  - max = 0.7·`mMaxLeafMoveSpeed`, min = 0.5·max
  - speed = min + (carriers + 1 − minCarry) / maxCarry · (max − min)
  - Retail values give 42–84.
- **Too few carriers.** If carriers fall below the minimum while moving, the pellet is put down.
  The original's carriers then go free and re-grab through the free AI; ours keep hold and struggle
  (Lift), which looks the same.
- **Paths.** The original follows the map's route graph (`RouteMgr`, `crMove`). We ask
  `Host::carry_path` for waypoints (default: a straight line). No path = put down.
- **Arrival.** Within 10 (XZ) of the Onion's goal point (`GoalItem::getGoalPos` = its position),
  carriers let go and the pellet enters `PelletGoalState`.

## 11. Onion — implemented

goalItem.cpp, pelletState.cpp, itemAI.cpp `GoalAI`:
- **Suck-in.** The pellet waits 1.5 s, then moves to the suck point (Onion + 74 up) with speed
  accelerating at 720/s², shrinking to 25% (`PelletGoalState::exec`). Then `GoalItem::suckMe` adds
  the seed count (fireworks if negative).
- **Seed timing** comes from the Onion's animations (`objects/goal/anims.bin`). "suction"
  (48 frames) plays, then "flowrot" loops 20→40, and **one seed is launched at each loop end**:
  the first 40 frames after the suction, then every 20 frames (0.67 s).
- **Seed flight.** From 110 above the Onion at (290·sin r, 800, 290·cos r), with r random.
  Gravity applies plus air drag with R = 10 − 0.5·rand (`PikiHeadItem::startAI`):
  `vy −= dt·R·vy` and `vxz −= dt·R·0.2·vxz` (`Creature::moveVelocity`, creatureMove.cpp). Seeds
  rise about 85, drift about 145 out and float down at about 56/s.
- **Field limit.** If Pikmin + sprouts ≥ aiconst p15, the seed stays inside the Onion
  (`mHeldPikis`) instead. Getting stored Pikmin back out (the Onion menu) is not implemented yet.
- **Colours.** An Onion accepts a *set* of colours: one per Onion like Pikmin 1, or several for
  a shared Onion. Seeds take the delivery's colour.

## 12. Sprouts — implemented

`PikiHeadAI` (itemAI.cpp). Each timer adds a random 0–2 s.
- Landing buries the seed for `mSeedUpTime`, then it shows as a seed for `mPluckWaitTime`.
- It then becomes pluckable (`canPullout`: states Wait/Unk7/Growuped) and grows leaf → bud →
  flower every `mGrowUpTime`.
- A flower wilts after `mWiltTime` and reburies as a leaf after `mRebirthSeedTime`.
- Retail: 0.1 s, 5 s, 120 s, 180 s and 60 s respectively.

## 13. Plucking — implemented

`Navi::procActionButton`, `NaviNukuAdjustState`, `NaviNukuState`:
- **Reach.** The action button plucks instead of throwing when a pluckable sprout is within reach
  and within 25 vertically. Reach is `mActionRadius` (retail 25) if within 220 of any Onion, else
  `mPluckDistanceOutsideOnyon` (15; not in the retail file).
- **Adjust.** The captain steps to 6 units short of the sprout. He turns 20% of the angle per
  tick (at least π/20) and moves at 100/s, slowing to 0.5 per tick when close. This ends when
  facing within π/10 and closer than 1. It gives up if he moves less than 4 units in 0.4 s
  (`mOdoMeter.start(0.4, 4.0)`).
- **Pull.** "NUKU" plays; the Pikmin comes out at `KEY_Action0` (frame 23) with the sprout's
  colour and leaf stage, and joins the squad. The captain is locked until frame 39.
- **Chaining** (`NaviNukuState::exec` / `procAnimMsg`, `mFastPluckKeyTaps`).
  - During a pull, once the button has been seen up (`keyUp` is a level, reset at each pull's
    start), pressing or holding it asks for another.
  - When the pull finishes, the captain looks for the next sprout within
    `mContinuousPluckDistance` (retail 200) and pulls it with "NUKU fast" (AnimInfo 43: out at
    frame 9, done at 19).
  - In Pikmin 1 that means mashing.
  - **Extra:** `Params::extras.hold_to_chain_pluck` (on by default) also chains while the button
    is simply held. Turn it off for Pikmin 1 rules.
  - The original's squad auto-pluck (`PikiAutoNukiState` via the whistle) is off on the retail
    disc (aiconst p13 = 0) and not implemented.

## 14. Pikmin dying — implemented (host-triggered)

Enemies are host AI, so the host calls the interaction. These follow interactBattle.cpp and
pikiState.cpp:
- **Attack** (`InteractAttack::actPiki`, `Sim::attack_piki` / `attack_pikis_in_sphere`).
  - Subtracts health, which starts at `mPikiMaxHealth` (retail 30, code 100).
  - Pikmin that are dying, dead or knocked down can't be hit.
  - At ≤ 0 health the Pikmin enters `PikiDyingState`: one of three random death animations
    (DEAD/DEAD2/DEAD3 → AnimInfo 20/42/48, 104/114/129 frames).
  - Then comes `PikiDeadState`, a 1.5 s shrink, and then it's gone.
- **Crush** (`InteractPress::actPiki`, `PikiPressedState`, `Sim::crush_pikis_in_sphere`).
  - Subtracts health, then the Pikmin is flattened with a 1.5 s invincible stun.
  - The death check (`deathTimer > mPressDeathFactor − mNormalPikiPressFactor && health ≤ 0`)
    passes on the first tick, so a crush that empties health kills at once.
  - Otherwise the Pikmin stays flat for the stun plus `mPressDeathFactor` (retail 1.5 + 2.5 s),
    then gets up.
- **Flick** (`InteractFlick::actPiki`, `Sim::flick_pikis_in_sphere`).
  - Subtracts health and knocks the Pikmin away like a shake-off.
  - A Pikmin at ≤ 0 health dies when it gets up.
  - Knocked-down Pikmin with health left can be whistled awake.
  - Phases, each ending with its motion (`PikiFlickState`): JHIT flying back, JKOKE landing
    (dead here if health is gone, straight to the shrink), lying for the knock-down time (or
    until whistled), GETUP. Getting up costs a bud or flower a stage 25% of the time.
- **Swallow** (`InteractSwallow::actPiki`, `PikiSwallowedState`, `Sim::swallow_pikis`).
  - Up to `max` Pikmin near the mouth stick to the given creature part and ride along.
  - `Sim::chew` kills them; the enemy's AI decides when, through `InteractKill` in the original.
  - `Sim::spit_out` frees them, and so does the creature dying.
- **Kill** (`InteractKill`, `Sim::kill_piki`): no animation, straight to the fade.
- **Seed left behind** (`Piki::doKill`): a dying *flower* that wasn't eaten leaves a seed when
  `rand ≥ mPikiLeaveSeedChance`, so 20% at the retail 0.8. The original stores it for the next
  day (`BPikiInfMgr`); we bury it on the spot.
- **Leaving.** Dying, crushed and swallowed Pikmin leave their squad, carry slot or latch first.
- **Field limit.** Dying and fading Pikmin still count toward the field limit until gone
  (`Piki::on_field`).

## 15. Captain damage — implemented

navi.cpp (`InteractAttack/Press/Swallow/Bomb/Flick::actNavi`, `Navi::finishDamage`) and
naviState.cpp (`NaviDeadState`):
- **Damage** (`Sim::damage_captain`). A hit subtracts health (`NaviParams::health`, 100).
  - The captain is then invulnerable while "DAMAGE" plays (`isDamaged()`): AnimInfo 21,
    19 frames.
  - When it ends (`Navi::finishDamage`), a captain at ≤ 1 health goes down.
  - Swallow attacks on captains always take 10 (`InteractSwallow::actNavi`); hosts pass that
    as the damage.
- **Down** (`NaviDeadState::init` → `Navi::releasePikis`). The squad is released and the
  captain stops responding.
  - Pikmin 1 ends the day here. We emit `Event::CaptainDown` and let the host decide
    (`Sim::revive_captain`).
  - `Sim::down_captain` does this directly, for hosts whose player character has its own health.
- **Healing.** The ship refills health completely (`NaviUfoState::cleanup`): `Sim::heal_captain`.
- **Not yet:** knockback and flattening of the captain himself (`NaviFlickState`, `NaviPressedState`).

## 16. Captain punch — implemented

`NaviThrowWaitState::exec` → `NaviAttackState`:
- **Trigger.** The throw button punches when no squad member is within 80 to throw.
- **Walking.** The captain keeps walking (`makeVelocity`) while "PUNCH" (AnimInfo 94) plays.
  - We assume 30 fps; `NaviAttackState` doesn't set the motion speed.
- **Hit.** From `KEY_Action0` (frame 1) to the end (frame 9) it hits every living, non-flying
  creature that is both:
  - within its size + the captain's size + `mPunchRadius` (retail 10), and
  - within 135° of facing.
  - Damage is `mPunchDamage` (retail 10, code 1), sent to `Host::damage` with
    `DamageSource::Captain`. Once something takes the hit, the punch is done.
- **Whistle.** A whistle pressed mid-punch starts when the punch ends (`mGatherRequested`).

## 17. C-stick swarming — implemented

`Navi::makeCStick` (C-stick branch), `ActCrowd::exec`, `Piki::collisionCallback`:
- **Input.** `CaptainInput::swarm` is the C-stick in world XZ; the dead zone is 0.05.
- **Direction.** If the stick points within 120° of the squad's current direction, the plate
  turns 40% of the way toward it each tick; otherwise it snaps to the stick.
- **Shape.** Strength s = (|stick| − 0.05)/0.95 maps to 1 at s ≥ 0.9, else 0.6·s/0.9. That's
  the plate's `refresh` interp, so a harder push makes it slimmer and longer. The plate sits
  `length + offset` from the captain in that direction.
- **No arcing.** While swarming, squad Pikmin don't arc around the captain
  (`newVer && !check`).
- **Bumping jobs.** A squad member that has reached its slot at least once (`STATE_Formed`)
  takes a job when it touches something:
  - **Enemy:** it attacks (aiconst p05 "宮本接触バトル", on for retail). This works whether or
    not the C-stick is pushed, so walking a settled squad into an enemy starts a fight.
  - **Pellet or corpse with a free slot:** it starts carrying, *only while the C-stick is
    pushed* (`distCheck` = `mCStick.length() ≥ 0.1`).
  - Gates, boulders, grass and bridges are the same idea; those objects aren't implemented yet.
- **Charge.** There's no charge in Pikmin 1 or 2; it arrived in Pikmin 3. Swarming fills that role.

## 18. Nectar — implemented

`MizuItem` with `FallWaterAI` / `WaterAI` (mizuItem.cpp, itemAI.cpp); `PikiNormalState::
procCollideMsg`, `PikiAbsorbState`, `PikiGrowupState` (pikiState.cpp). Frames are 30 fps and come
from `objects/fl_water/anims.bin` and `objects/water/anims.bin` (retail values in parentheses).

- **Drop.** Spawned in the air (a Honeywisp drops one when hit). It plays "leave" (8), then loops
  "fall" (19–23) under gravity. When it touches the ground it jumps to frame 29
  (`finishMotion(29)`), and the puddle appears at `KEY_Action0` (30). The drop disappears at
  the end (37).
- **Puddle.** Collision radius 15 (`Creature::getSize`). The states are:
  - "swing" while forming (42 frames).
  - Then a "wait" loop (40).
  - A captain stepping in plays "touch" (50).
  - The first drink message while waiting or touched starts "dead" (draining, 50 frames). From
    then on the puddle isn't alive and is removed at the end.
  - Drinking during "swing" doesn't drain it, because no arrow leaves that state.
- **Drinking.** A Pikmin in its normal state that isn't a flower and isn't carrying starts
  drinking when it touches a live puddle. "Normal state" here means idle, in a squad, or running
  at an enemy. Starting to drink:
  - takes it out of the squad (free mode);
  - turns it to face the puddle;
  - plays MIZUNOMI (anim 67).
  - Collisions skip non-alive creatures (`creatureCollision.cpp`), so a draining puddle gets no
    new drinkers.
- **Drinking loop.** From the loop start (10) it drinks every tick and counts as having drunk,
  even after the puddle starts draining. At each loop end (30) it loops again while the puddle
  is alive, otherwise it plays out to the end (39).
- **Blooming.** If it drank, it plays GROWUP1 (anim 39, first bloom) or GROWUP2 (anim 40,
  later). It becomes a flower at `KEY_Action0` (49 / 32). When the motion ends (84 / 69) it is
  idle and free. If it never reached the loop, it just goes back to normal.
- **Whistle.** `Navi::callPikis` skips Pikmin that are drinking or blooming.
- **What this means in play.** Everyone who touches the puddle while it is still forming will
  drink, so nectar landing inside a packed squad blooms most of it.

## 19. Onion menu — implemented

`NaviWalkState::exec`, `NaviContainerState` (naviState.cpp); `ActEnter` (aiEnter.cpp),
`ActExit` (aiExit.cpp); `GoalItem::exitPikis` / `exitPiki` (goalItem.cpp).

RoR2 host extensions: its one-page menu combines all colours, with bulk release/return
shortcuts. Stored Pikmin can be sacrificed for health: leaves first at 5% max health,
buds at 10%, flowers at 20%, stopping at the health deficit or one remaining unreserved
stored Pikmin. Exits reserve their highest stages before sacrifices are selected; squad
Pikmin are never consumed. The host plays the Dolphin's recovery effects with one soul
effect per sacrifice, without moving Olimar to the Dolphin.

Hosts can request a world-space jump height after converting it to sim units. The
fixed-step takeoff compensates gravity integration so the sampled apex meets that height;
squad and navigation jumps inherit their captain's takeoff speed, including extra jumps.

- **Opening.** A captain standing within reach of the Onion's `cont` sphere opens the window by
  pressing the action button. The walk state checks this before the button does anything
  else. The game pauses while the window is open.
- **The window** shows three numbers:
  - Pikmin of that colour in the Onion, minus those already waiting to come out.
  - The squad's Pikmin of that colour.
  - The field count (Pikmin and sprouts on the map plus pending exits) against `aiconst`
    p15 (100).
- **Putting away.** That many squad Pikmin of the colour switch to `EnterMode`:
  - Each walks at half speed to a random leg's foot.
  - It climbs at 15 units/s, shrinks from 72% of the way up, and is stored at 90%.
  - Pikmin are stored by leaf stage.
- **Calling out.** The count is limited by the Onion's stock and the field limit. Pikmin come
  out one every (0.2 + 0.1·rand)·0.2 s, the highest leaf stage first:
  - Each appears at the top of a random leg and slides down it at 70 units/s, growing back
    from 72%.
  - At the bottom it joins its captain's squad (`mActionState` 0 → `joinParty`).
  - Host extension: a host-controlled hovering Onion can also call Pikmin out. Each plays
    the first half of the same leg slide, detaches at ratio 0.5, and falls under the normal
    Pikmin flight physics from that position. Grounded call-outs still use the whole leg;
    putting Pikmin away still requires landing. Stock and pending exits retain the field cap.
- **Safety.** The whistle and enemy attacks leave Pikmin in either mode alone.
- **Geometry.** Legs are measured on `goal.mod`: feet about 115 out at 180° and ±60°, tops at
  height 88. The `cont` reach is 70 from the Onion's centre. The real sphere is in the
  Onion's collision data, which isn't loaded.

## Planned

- Pikmin 2 types (Purple, White, Bulbmin), Candypop Buds, hazards.
- Onion menu (calling stored Pikmin out, putting them away), sunset.
- Honeywisps and Spotty/Dwarf Bulborb AI in the core (they are viewer-side for now).
- Free-mode AI (`ActFree`: wandering and the periodic work search).
