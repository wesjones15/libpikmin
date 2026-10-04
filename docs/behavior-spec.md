# libpikmin behavior spec

Written from the projectPiki Pikmin 1 decomp (GPIE01_01, 100% matching) as a behavioral
spec, in our own words. Citations are `file: function` under `pikmin-main/src/`
(Kando's gameplay code is in `plugPikiKando/`). Units are decomp units (roughly cm).

Status: **implemented** = in `crates/libpikmin` with tests; **planned** = not yet.

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
- **Swing (4):** after the swing time, a hit lands if within 10 of the part surface and facing within π/4.
- **Latched (5):** rides the part (offset kept in the creature's yaw frame) and hits once per
  cling-animation cycle.
- Each hit is `InteractAttack` → `Host::damage(creature, power, part)`. Power: red 15,
  blue 10, yellow 10 (`Piki::getAttackPower`).
- **aiconst p10** (`_C4`): 1 = stop after one attack (latched Pikmin drop off), 0 = keep going
  until the target dies. Retail is 0.
- **aiconst p08:** return to the squad after attacking if within `mPostWorkJoinPartyRange`. Retail is 0, so they stay free.
- **aiconst p01:** idle Pikmin look for targets on their own. Retail is 0.
- Target dead, gone, or (for non-latched) flying: latched Pikmin fall off. All go free.
- **APPROXIMATIONS:** hit timing comes from animation key events (`KEY_Action0` in the
  "Kuttuku"/"Attack" motions), whose frames live in `pikis/animMgr.bin` and the animation bundle.
  Until we decode those, latched hits are every 0.5 s and swings every 0.6 s
  (`LATCH_HIT_INTERVAL`, `SWING_INTERVAL` in piki.rs).
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

## Planned

- Carry: `ActTransport` (aiTransport.cpp), `Pellet`/`PelletConfig` (pelletMgr.cpp). Speed:
  max = 0.7·runSpeed, min = 0.5·max, speed = min + (carriers + 1 − minCarry)/maxCarry·(max − min).
  Below the minimum carrier count the object is put down. Paths come from `Host`.
- Corpses: `BTeki::dieSoon` → `becomePellet` (plugPikiNakata/tekibteki.cpp).
- Onion: `GoalItem` (goalItem.cpp), seeds/sprouts/pluck. The Onion will hold a **set of colors**:
  one Onion per color (Pikmin 1 style) or a single shared Onion are both just configuration.
- Pikmin 2 types (Purple, White, Bulbmin), Candypop Buds, hazards.
- Pikmin death (eaten, crushed), captain damage.
