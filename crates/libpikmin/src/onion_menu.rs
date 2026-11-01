//! The Onion menu: calling stored Pikmin out and putting squad Pikmin away.
//!
//! - Opening: a captain within reach of the Onion's `cont` sphere presses the action button
//!   (`NaviWalkState::exec` → `NaviContainerState`, naviState.cpp). The game pauses while the
//!   window is up; hosts do the same by not advancing the sim.
//! - The window (`NaviContainerState::init`) shows the Onion's stock minus pending exits, the
//!   squad's Pikmin of that colour, and the field count (on the map plus pending exits)
//!   against `aiconst` p15 (100).
//! - Putting away (`NaviContainerState::enterPikis`): that many squad Pikmin of the colour
//!   switch to `EnterMode` (`ActEnter`, aiEnter.cpp): walk at half speed to one of the three
//!   legs, climb it at 15 units/s, shrink from 72% of the way, and are stored at 90%.
//! - Calling out (`GoalItem::exitPikis` / `exitPiki`, goalItem.cpp): one Pikmin every
//!   (0.2 + 0.1 rand) * 0.2 s, highest leaf stage first, appears at the top of a random leg and
//!   slides down it at 70 units/s (`ActExit`, aiExit.cpp), growing back from 72%. On the
//!   ground it joins the captain's squad (`TopAction::exec` with `mActionState` 0).
//! - Host-controlled hovering Onions can also call Pikmin out: they slide to the midpoint
//!   of a leg, then let go into the normal ballistic/falling state. Returning Pikmin
//!   jump to the midpoint and climb the upper half. The airborne loop's legs change through
//!   the loop, so these follow the drawn hip-knee-foot polyline at `Onion::hover_frame`.
//!   These are host extensions.

use glam::Vec3;

use crate::event::Event;
use crate::host::Host;
use crate::onion::OnionId;
use crate::piki::{Happa, PikiColor, PikiState};
use crate::sim::Sim;
use crate::{CaptainId, PikiId};

/// Leg ends measured on `objects/goal/goal.mod` in its bind pose: each leg mesh's lowest
/// vertex (the foot) and highest (where it meets the pod), relative to the Onion's position.
pub const LEG_FEET: [Vec3; 3] = [
    Vec3::new(2.3, 0.0, -115.2),
    Vec3::new(-100.9, 0.0, 55.7),
    Vec3::new(98.6, 0.0, 59.6),
];
pub const LEG_TOPS: [Vec3; 3] = [
    Vec3::new(0.7, 88.4, -19.5),
    Vec3::new(-18.4, 88.4, 9.3),
    Vec3::new(17.6, 88.3, 10.7),
];

/// The airborne loop's frame range in the take-off clip (`animator::onion_motion`).
pub const HOVER_FROM: f32 = 200.0;
pub const HOVER_TO: f32 = 260.0;
const HOVER_STEP: f32 = 5.0;

/// The drawn leg while the Onion is in its airborne loop (the take-off clip's frames 200-260):
/// hip, knee and foot joints, relative to the Onion. The loop's legs bend at the knee and swing
/// through the loop, so a straight rope from the hip to the foot cuts through the bend and lets
/// Pikmin out beside the visible leg. `ActExit`/`ActEnter` follow the leg's rope; for this host
/// hover pose the rope is this two-segment polyline so they stay on the drawn leg.
///
/// Measured from the disc model by `libpikmin-assets --example onion_hover_legs` and
/// interpolated with `Onion::hover_frame`.
pub const HOVER_LEG_JOINTS: [[[Vec3; 3]; 3]; 13] = [
    // frame 200
    [[Vec3::new(-14.46, 86.50, -18.52), Vec3::new(-35.11, 69.18, -44.85), Vec3::new(-27.44, 32.24, -35.36)], [Vec3::new(-9.41, 85.42, 21.17), Vec3::new(-21.99, 65.79, 50.77), Vec3::new(-17.71, 29.83, 36.55)], [Vec3::new(22.94, 85.59, -4.30), Vec3::new(54.80, 66.21, -9.68), Vec3::new(40.22, 30.14, -9.05)]],
    // frame 205
    [[Vec3::new(-15.40, 87.20, -18.63), Vec3::new(-37.29, 70.88, -44.60), Vec3::new(-29.45, 33.61, -36.68)], [Vec3::new(-8.63, 84.76, 20.74), Vec3::new(-20.03, 64.22, 50.20), Vec3::new(-16.61, 28.74, 34.62)], [Vec3::new(22.58, 85.63, -6.11), Vec3::new(54.05, 66.30, -13.53), Vec3::new(39.29, 30.30, -13.47)]],
    // frame 210
    [[Vec3::new(-15.81, 87.68, -18.87), Vec3::new(-38.33, 72.03, -44.71), Vec3::new(-30.08, 34.58, -38.14)], [Vec3::new(-7.94, 84.11, 20.21), Vec3::new(-18.41, 62.64, 49.34), Vec3::new(-15.27, 27.65, 32.63)], [Vec3::new(22.52, 85.95, -7.45), Vec3::new(53.87, 67.03, -16.32), Vec3::new(39.29, 30.97, -16.93)]],
    // frame 215
    [[Vec3::new(-15.22, 87.57, -19.21), Vec3::new(-37.41, 71.71, -45.22), Vec3::new(-27.92, 34.43, -39.45)], [Vec3::new(-6.97, 83.47, 19.74), Vec3::new(-16.56, 61.10, 48.50), Vec3::new(-12.41, 26.57, 31.06)], [Vec3::new(23.21, 86.84, -8.07), Vec3::new(55.09, 69.12, -17.51), Vec3::new(41.67, 32.62, -18.76)]],
    // frame 220
    [[Vec3::new(-13.84, 87.02, -19.61), Vec3::new(-34.87, 70.33, -46.05), Vec3::new(-23.63, 33.51, -40.48)], [Vec3::new(-5.92, 82.97, 19.41), Vec3::new(-14.77, 59.89, 47.85), Vec3::new(-8.69, 25.77, 30.17)], [Vec3::new(24.43, 88.09, -7.95), Vec3::new(57.27, 72.05, -17.10), Vec3::new(45.69, 34.95, -18.85)]],
    // frame 225
    [[Vec3::new(-12.41, 86.50, -19.96), Vec3::new(-32.02, 69.02, -46.96), Vec3::new(-19.59, 32.64, -41.00)], [Vec3::new(-5.48, 82.79, 19.29), Vec3::new(-14.29, 59.42, 47.50), Vec3::new(-6.43, 25.54, 30.08)], [Vec3::new(25.44, 89.01, -7.19), Vec3::new(59.10, 74.19, -15.36), Vec3::new(48.97, 36.69, -17.37)]],
    // frame 230
    [[Vec3::new(-11.69, 86.29, -20.15), Vec3::new(-30.23, 68.54, -47.73), Vec3::new(-18.30, 32.18, -40.72)], [Vec3::new(-6.48, 83.10, 19.41), Vec3::new(-16.64, 60.19, 47.54), Vec3::new(-8.26, 26.01, 30.95)], [Vec3::new(25.59, 88.76, -5.79), Vec3::new(59.48, 73.64, -12.29), Vec3::new(49.20, 36.17, -14.21)]],
    // frame 235
    [[Vec3::new(-11.68, 85.91, -19.70), Vec3::new(-29.46, 67.72, -47.49), Vec3::new(-19.85, 31.22, -38.06)], [Vec3::new(-8.98, 84.45, 20.21), Vec3::new(-21.95, 63.51, 48.72), Vec3::new(-14.38, 28.17, 34.30)], [Vec3::new(24.77, 87.28, -3.22), Vec3::new(58.16, 70.21, -6.90), Vec3::new(46.09, 33.24, -7.68)]],
    // frame 240
    [[Vec3::new(-10.97, 83.72, -17.17), Vec3::new(-27.27, 62.32, -43.54), Vec3::new(-19.46, 27.23, -28.67)], [Vec3::new(-11.14, 87.59, 22.67), Vec3::new(-26.72, 70.93, 52.66), Vec3::new(-19.21, 33.84, 43.66)], [Vec3::new(24.25, 86.55, 1.64), Vec3::new(57.29, 68.47, 2.73), Vec3::new(44.20, 31.98, 5.97)]],
    // frame 245
    [[Vec3::new(-10.39, 82.04, -14.73), Vec3::new(-25.31, 57.91, -39.52), Vec3::new(-19.51, 24.74, -20.05)], [Vec3::new(-13.32, 90.67, 24.25), Vec3::new(-31.44, 78.01, 54.76), Vec3::new(-24.28, 39.98, 50.80)], [Vec3::new(23.45, 86.50, 6.18), Vec3::new(55.84, 68.11, 11.87), Vec3::new(41.60, 32.50, 18.43)]],
    // frame 250
    [[Vec3::new(-9.27, 83.34, -17.59), Vec3::new(-22.08, 61.30, -45.33), Vec3::new(-18.48, 26.73, -27.86)], [Vec3::new(-15.73, 89.24, 21.47), Vec3::new(-36.76, 74.76, 49.18), Vec3::new(-29.68, 36.99, 43.16)], [Vec3::new(22.51, 85.71, 6.62), Vec3::new(54.03, 66.39, 13.93), Vec3::new(38.85, 30.67, 16.56)]],
    // frame 255
    [[Vec3::new(-5.84, 86.50, -22.77), Vec3::new(-14.69, 69.19, -55.04), Vec3::new(-9.97, 32.28, -43.70)], [Vec3::new(-15.85, 84.78, 15.94), Vec3::new(-37.93, 64.29, 38.59), Vec3::new(-27.61, 28.73, 26.67)], [Vec3::new(23.62, 86.26, 4.32), Vec3::new(55.78, 67.80, 10.99), Vec3::new(43.13, 31.37, 5.84)]],
    // frame 260
    [[Vec3::new(-1.68, 84.12, -20.80), Vec3::new(-5.84, 63.25, -51.90), Vec3::new(0.51, 28.06, -36.56)], [Vec3::new(-15.44, 85.06, 16.77), Vec3::new(-38.08, 64.90, 39.14), Vec3::new(-23.90, 29.41, 31.83)], [Vec3::new(24.79, 88.87, 8.84), Vec3::new(57.42, 73.92, 20.32), Vec3::new(48.18, 36.23, 17.62)]],
];

/// The airborne loop's hip, knee and foot at `frame` (clamped to
/// [`HOVER_FROM`]..[`HOVER_TO`]), relative to the Onion.
pub fn hover_leg_joints(leg: u8, frame: f32) -> [Vec3; 3] {
    let l = leg as usize % 3;
    let last = HOVER_LEG_JOINTS.len() - 1;
    let t = ((frame - HOVER_FROM) / HOVER_STEP).clamp(0.0, last as f32);
    let i = t.floor() as usize;
    let j = (i + 1).min(last);
    let u = t - i as f32;
    [0, 1, 2].map(|k| HOVER_LEG_JOINTS[i][l][k].lerp(HOVER_LEG_JOINTS[j][l][k], u))
}

/// A point `ratio` of the way down a leg given as its hip, knee and foot: 1 at the shoulder,
/// 0 at the foot, along the two segments. Shared by the sim's airborne loop and hosts that place
/// climbers on the Onion's actually posed leg.
pub fn leg_polyline_point(joints: [Vec3; 3], ratio: f32) -> Vec3 {
    let [hip, knee, foot] = joints;
    let (a, b) = (hip.distance(knee), knee.distance(foot));
    let len = a + b;
    if len <= 1e-6 {
        return hip;
    }
    let d = (1.0 - ratio.clamp(0.0, 1.0)) * len;
    if d <= a && a > 1e-6 {
        hip.lerp(knee, d / a)
    } else if b > 1e-6 {
        knee.lerp(foot, ((d - a) / b).clamp(0.0, 1.0))
    } else {
        knee
    }
}

/// A point `ratio` of the way down the drawn airborne leg: 1 at the shoulder, 0 at the foot,
/// along the hip-knee-foot polyline.
pub fn hover_leg_point(leg: u8, ratio: f32, frame: f32) -> Vec3 {
    leg_polyline_point(hover_leg_joints(leg, frame), ratio)
}

/// Arc length of the drawn airborne leg.
pub fn hover_leg_length(leg: u8, frame: f32) -> f32 {
    let [hip, knee, foot] = hover_leg_joints(leg, frame);
    hip.distance(knee) + knee.distance(foot)
}

/// Whether the Onion is drawn in its airborne loop rather than a flight transition or a landed
/// pose. `Hover` alone is not enough: while a host holds it low to fill, `flight_motion` picks a
/// take-off/landing frame, whose legs are still the landed ones.
pub(crate) fn flying_pose(o: &crate::onion::Onion) -> bool {
    o.anim == crate::onion::OnionAnim::Hover && o.flight_motion.is_none()
}
/// Horizontal reach of the `cont` sphere plus the captain, from the Onion's centre. The
/// sphere lives in the Onion's collision data, which libpikmin doesn't load; this is a fit
/// to "standing under the Onion".
pub const CONTAINER_REACH: f32 = 70.0;
/// `ActEnter::climb`
pub const CLIMB_SPEED: f32 = 15.0;
/// Host extension: the jump from the ground to a flying leg's midpoint before climbing. The
/// negative `Enter` ratio counts it down.
pub const ENTER_JUMP_SECONDS: f32 = 0.65;
/// `ActExit::exec`
pub const SLIDE_SPEED: f32 = 70.0;
/// Rope ratio where Pikmin start shrinking into / growing out of the Onion.
pub const SHRINK_FROM: f32 = 0.72;
/// `ActEnter::climb`: stored past this ratio.
pub const ENTER_AT: f32 = 0.9;

/// What the Onion window shows (`containerWindow->start` arguments).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnionCounts {
    pub color: PikiColor,
    /// Stored Pikmin of `color` not already on their way out.
    pub in_onion: u32,
    /// The captain's squad Pikmin of `color` that could be put away.
    pub in_squad: u32,
    /// Pikmin and sprouts on the field plus pending exits.
    pub on_field: u32,
    pub max_on_field: u32,
}

impl OnionCounts {
    /// How many could be called out right now.
    pub fn can_call_out(&self) -> u32 {
        self.in_onion.min(self.max_on_field.saturating_sub(self.on_field))
    }
}

/// Where a Pikmin on `leg`'s rope sits: 0 at the foot, 1 at the shoulder. `hover` is the
/// airborne loop's frame (`Onion::hover_frame`) when the Onion is drawn in that pose; `None`
/// follows the landed, bind-pose leg.
pub(crate) fn leg_point(onion: Vec3, leg: u8, ratio: f32, hover: Option<f32>) -> Vec3 {
    onion + match hover {
        Some(frame) => hover_leg_point(leg, ratio, frame),
        None => LEG_FEET[leg as usize % 3].lerp(LEG_TOPS[leg as usize % 3], ratio.clamp(0.0, 1.0)),
    }
}

pub(crate) fn leg_top(onion: Vec3, leg: u8, hover: Option<f32>) -> Vec3 {
    onion + match hover {
        Some(frame) => hover_leg_joints(leg, frame)[0],
        None => LEG_TOPS[leg as usize % 3],
    }
}

pub(crate) fn leg_length(leg: u8, hover: Option<f32>) -> f32 {
    match hover {
        Some(frame) => hover_leg_length(leg, frame),
        None => (LEG_TOPS[leg as usize % 3] - LEG_FEET[leg as usize % 3]).length(),
    }
}

impl Sim {
    /// The Onion a captain can open the menu of, if any.
    pub fn onion_in_reach(&self, captain: CaptainId) -> Option<OnionId> {
        let c = self.captains.get(captain.0 as usize)?;
        if c.down || c.pluck.is_some() {
            return None;
        }
        self.onions
            .iter()
            .filter(|o| o.ship.is_none() && o.grounded() && (o.pos - c.pos).with_y(0.0).length() < CONTAINER_REACH)
            .min_by(|a, b| {
                let da = (a.pos - c.pos).length();
                let db = (b.pos - c.pos).length();
                da.total_cmp(&db)
            })
            .map(|o| o.id)
    }

    fn squad_of_color(&self, captain: CaptainId, color: PikiColor) -> Vec<PikiId> {
        self.pikis
            .iter()
            .filter(|p| {
                p.color == color && p.captain == Some(captain) && p.state.in_squad()
            })
            .map(|p| p.id)
            .collect()
    }

    pub fn onion_counts(&self, onion: OnionId, captain: CaptainId, color: PikiColor) -> OnionCounts {
        let o = &self.onions[onion.0 as usize];
        let pending = o.exits.iter().filter(|e| e.0 == color).count() as u32;
        OnionCounts {
            color,
            in_onion: o.stored_of(color).saturating_sub(pending),
            in_squad: self.squad_of_color(captain, color).len() as u32,
            on_field: self.field_count() as u32,
            max_on_field: self.params.ai.max_pikis_on_field.max(0) as u32,
        }
    }

    /// Sends up to `n` of the captain's squad Pikmin of `color` into the Onion. Returns how
    /// many were sent.
    pub fn onion_put_away(&mut self, captain: CaptainId, onion: OnionId, color: PikiColor, n: u32) -> u32 {
        if !self.onions[onion.0 as usize].accepts(color) || !self.onions[onion.0 as usize].takes_deliveries() {
            return 0;
        }
        let ids: Vec<_> = self.squad_of_color(captain, color).into_iter().take(n as usize).collect();
        for &id in &ids {
            self.leave_current(id);
            // `ActEnter::findLeg`: a random leg.
            let leg = (self.rng.f32() * 3.0) as u8 % 3;
            let p = &mut self.pikis[id.0 as usize];
            p.jump = None;
            p.jump_queue.clear();
            p.state = PikiState::Enter {
                onion,
                leg,
                climbing: false,
                ratio: 0.0,
            };
        }
        ids.len() as u32
    }

    /// Host extension: the Pikmin a captain left behind (he went down, or his owner died) that stand about
    /// join `to`'s squad instead. Returns how many.
    pub fn adopt_orphans(&mut self, from: CaptainId, to: CaptainId) -> u32 {
        if from == to || to.0 as usize >= self.captains.len() || self.captains[to.0 as usize].down {
            return 0;
        }
        let ids = self.orphans_of(from, None);
        for &id in &ids {
            self.leave_current(id);
            self.join_squad(id, to);
        }
        ids.len() as u32
    }

    /// Host extension: a dead captain's living Pikmin abandon their jobs and walk home.
    /// Returners already on a leg keep their progress; dying/swallowed Pikmin cannot be saved.
    pub fn send_orphans_home(&mut self, from: CaptainId, onion: OnionId) -> u32 {
        let Some(o) = self.onions.get(onion.0 as usize) else { return 0 };
        if o.ship.is_some() || !o.grounded() {
            return 0;
        }
        let ids = self.orphans_of(from, Some(onion));
        for &id in &ids {
            self.leave_current(id);
            let leg = (self.rng.f32() * 3.0) as u8 % 3;
            self.pikis[id.0 as usize].jump = None;
            self.pikis[id.0 as usize].jump_queue.clear();
            self.pikis[id.0 as usize].state = PikiState::Enter { onion, leg, climbing: false, ratio: 0.0 };
        }
        ids.len() as u32
    }

    /// Host recovery after a death-recall timeout or stage unload. Prevent unreachable
    /// returners being lost to host terrain; preserve their colour and leaf stage.
    pub fn finish_orphan_recall(&mut self, captain: CaptainId, onion: OnionId) {
        let Some(o) = self.onions.get(onion.0 as usize) else { return };
        if o.ship.is_some() { return; }
        let ids: Vec<_> = self.pikis.iter().filter(|p| p.is_alive() && p.captain == Some(captain)
            && o.accepts(p.color) && !matches!(p.state, PikiState::Dying { .. } | PikiState::Swallowed { .. }))
            .map(|p| p.id).collect();
        for id in ids {
            self.leave_current(id);
            let p = &mut self.pikis[id.0 as usize];
            self.onions[onion.0 as usize].stored[p.color.index()][p.happa as usize] += 1;
            p.state = PikiState::Dead;
            p.jump = None;
            self.events.push(Event::PikiEnteredOnion { piki: id, onion });
        }
    }

    /// Pikmin that were `captain`'s and are not doing anything else: standing about or still following
    /// (of a colour `onion` accepts, when one is given).
    fn orphans_of(&self, captain: CaptainId, onion: Option<OnionId>) -> Vec<PikiId> {
        self.pikis
            .iter()
            .filter(|p| p.is_alive() && p.captain == Some(captain) &&
                (onion.is_some() || matches!(p.state, PikiState::Idle | PikiState::Squad { .. })) &&
                !matches!(p.state, PikiState::Enter { .. } | PikiState::Exit { .. } | PikiState::Dying { .. } | PikiState::Swallowed { .. }))
            .filter(|p| onion.is_none_or(|o| self.onions[o.0 as usize].accepts(p.color)))
            .map(|p| p.id)
            .collect()
    }

    /// Host extension: how long a dormant Onion still has to wait before it revives (it does not restart a
    /// revival already under way). 0 wakes it.
    pub fn set_onion_dormancy_seconds(&mut self, onion: OnionId, seconds: f32) {
        if let Some(d) = self.onions.get_mut(onion.0 as usize).and_then(|o| o.dormancy.as_mut())
            && !d.reviving
            && seconds.is_finite()
        {
            d.remaining = seconds.max(0.0);
        }
    }

    /// Asks the Onion to let up to `n` Pikmin of `color` out to join `captain`. Limited by the
    /// stock and the field limit. Returns how many will come out.
    pub fn onion_call_out(&mut self, captain: CaptainId, onion: OnionId, color: PikiColor, n: u32) -> u32 {
        if !self.onions[onion.0 as usize].accepts(color) || !self.onions[onion.0 as usize].takes_deliveries() { return 0; }
        let k = self.onion_counts(onion, captain, color).can_call_out().min(n);
        let o = &mut self.onions[onion.0 as usize];
        for _ in 0..k {
            o.exits.push_back((color, captain));
        }
        k
    }

    /// Host extension (Onion Ride's sneeze): lets up to `n` Pikmin of `color` out of the Onion at once, in
    /// the same step, `n / 3` down each of its three legs (`GoalItem` dispenses one at a time; this is
    /// not that). They slide out as `ActExit` does and join `captain`. Highest leaf stage first, limited
    /// by the stock and the field limit. Returns how many came out.
    pub fn onion_burst_out(&mut self, captain: CaptainId, onion: OnionId, color: PikiColor, n: u32) -> u32 {
        let oi = onion.0 as usize;
        if !self.onions[oi].accepts(color) || !self.onions[oi].takes_deliveries() { return 0; }
        let mut out = 0;
        for k in 0..n {
            let o = &mut self.onions[oi];
            let Some(happa) = [Happa::Flower, Happa::Bud, Happa::Leaf]
                .into_iter()
                .find(|&h| o.stored[color.index()][h as usize] > 0)
            else { break };
            let (id_on, opos, hover) = (o.id, o.pos, flying_pose(o).then_some(o.hover_frame));
            // Legs in turn, so each takes a third.
            let leg = (k % 3) as u8;
            let Some(id) = self.spawn_piki(color, leg_point(opos, leg, 1.0, hover)) else { break };
            self.onions[oi].stored[color.index()][happa as usize] -= 1;
            let p = &mut self.pikis[id.0 as usize];
            p.happa = happa;
            p.captain = Some(captain);
            p.on_ground = false;
            p.state = PikiState::Exit { onion: id_on, leg, ratio: 1.0 };
            self.events.push(Event::PikiLeftOnion { piki: id, onion: id_on });
            out += 1;
        }
        out
    }

    /// Host extension (Onion Ride ends): Pikmin of `captain` still on their way into `onion` give up and rejoin his
    /// squad where they are, instead of finishing the climb. Returns how many.
    pub fn onion_cancel_entries(&mut self, captain: CaptainId, onion: OnionId) -> u32 {
        let ids: Vec<_> = self.pikis.iter()
            .filter(|p| p.captain == Some(captain) && matches!(p.state, PikiState::Enter { onion: on, .. } if on == onion))
            .map(|p| p.id).collect();
        for &id in &ids {
            self.leave_current(id);
            let p = &mut self.pikis[id.0 as usize];
            p.jump = None;
            p.jump_queue.clear();
            p.on_ground = false;
            self.join_squad(id, captain);
        }
        ids.len() as u32
    }

    /// Puts Pikmin straight into an Onion's stock (for hosts restoring a save, for example).
    pub fn store_in_onion(&mut self, onion: OnionId, color: PikiColor, happa: Happa, n: u32) {
        if let Some(o) = self.onions.get_mut(onion.0 as usize) {
            o.stored[color.index()][happa as usize] += n;
        }
    }

    /// Host extension: sacrifice stored Pikmin to heal a captain, leaves before buds before
    /// flowers (5%, 10%, 20% max health). Preserve one available Pikmin and every queued exit.
    /// Returns the number consumed and the health fraction restored, capped at the deficit.
    pub fn sacrifice_onion_stock(&mut self, onion: OnionId, missing: f32) -> (u32, f32) {
        if !missing.is_finite() || missing <= 0.0 { return (0, 0.0); }
        let Some(o) = self.onions.get_mut(onion.0 as usize) else { return (0, 0.0) };
        if o.ship.is_some() || !o.takes_deliveries() { return (0, 0.0); }
        let mut available = o.stored;
        for (colour, stock) in available.iter_mut().enumerate() {
            // Exits take flowers first, then buds and leaves. These are already promised.
            let mut pending = o.exits.iter().filter(|e| e.0.index() == colour).count() as u32;
            for stage in (0..3).rev() {
                let reserved = stock[stage].min(pending);
                stock[stage] -= reserved;
                pending -= reserved;
            }
        }
        let budget = available.iter().flatten().sum::<u32>().saturating_sub(1);
        let (mut killed, mut healing) = (0, 0.0f32);
        let missing = missing.min(1.0);
        for (stage, fraction) in [0.05, 0.1, 0.2].into_iter().enumerate() {
            for (colour, stock) in available.iter().enumerate() {
                for _ in 0..stock[stage] {
                    if killed == budget || healing + 1e-6 >= missing { return (killed, healing.min(missing)); }
                    o.stored[colour][stage] -= 1;
                    killed += 1;
                    healing += fraction;
                }
            }
        }
        (killed, healing.min(missing))
    }

    /// `GoalItem` dispensing: one Pikmin out every 0.04-0.06 s.
    pub(crate) fn update_onion_exits(&mut self, dt: f32) {
        for oi in 0..self.onions.len() {
            if self.onions[oi].exits.is_empty() || !self.onions[oi].takes_deliveries() {
                continue;
            }
            self.onions[oi].exit_timer -= dt;
            if self.onions[oi].exit_timer > 0.0 {
                continue;
            }
            self.onions[oi].exit_timer = (self.rng.f32() * 0.1 + 0.2) * 0.2;
            let Some((color, captain)) = self.onions[oi].exits.pop_front() else {
                continue;
            };
            let o = &mut self.onions[oi];
            // Highest leaf stage first.
            let Some(happa) = [Happa::Flower, Happa::Bud, Happa::Leaf]
                .into_iter()
                .find(|&h| o.stored[color.index()][h as usize] > 0)
            else {
                continue;
            };
            o.stored[color.index()][happa as usize] -= 1;
            let (onion, opos, hover) = (o.id, o.pos, flying_pose(o).then_some(o.hover_frame));
            let leg = (self.rng.f32() * 3.0) as u8 % 3;
            let Some(id) = self.spawn_piki(color, leg_point(opos, leg, 1.0, hover)) else {
                // Field full after all: keep it inside.
                self.onions[oi].stored[color.index()][happa as usize] += 1;
                continue;
            };
            let p = &mut self.pikis[id.0 as usize];
            p.happa = happa;
            p.captain = Some(captain);
            p.on_ground = false;
            p.state = PikiState::Exit {
                onion,
                leg,
                ratio: 1.0,
            };
            self.events.push(Event::PikiLeftOnion { piki: id, onion });
        }
    }

    /// `ActEnter` and `ActExit`.
    pub(crate) fn piki_onion_travel(&mut self, pi: usize, host: &mut dyn Host, dt: f32) {
        match self.pikis[pi].state {
            PikiState::Enter {
                onion,
                leg,
                climbing,
                ratio,
            } => {
                let o = &self.onions[onion.0 as usize];
                let opos = o.pos;
                // The airborne loop's legs swing out and fold through the loop; `hover` carries
                // its frame so entry and exit follow the same rope the view draws.
                let hover = flying_pose(o).then_some(o.hover_frame);
                if !o.takes_deliveries() {
                    self.pikis[pi].state = PikiState::Idle;
                    return;
                }
                if !climbing {
                    // Host extension: JUMP (21) to the middle of a flying leg,
                    // then ActEnter::gotoLeg/climb's HNoboru (62) up its top half.
                    // Negative ratio is the remaining jump time; snapshots already carry it.
                    let foot = leg_point(opos, leg, if hover.is_some() { 0.5 } else { 0.0 }, hover);
                    if ratio < 0.0 {
                        let p = &mut self.pikis[pi];
                        let remaining = -ratio;
                        let step = dt.min(remaining);
                        p.pos += (foot - p.pos) * (step / remaining);
                        p.vel = Vec3::ZERO;
                        p.on_ground = false;
                        p.state = if remaining <= dt {
                            PikiState::Enter { onion, leg, climbing: true, ratio: if hover.is_some() { 0.5 } else { 0.0 } }
                        } else {
                            PikiState::Enter { onion, leg, climbing: false, ratio: ratio + step }
                        };
                        return;
                    }
                    let to = (foot - self.pikis[pi].pos).with_y(0.0);
                    if to.length() < 8.0 {
                        let p = &mut self.pikis[pi];
                        p.vel = Vec3::ZERO;
                        p.on_ground = false;
                        p.state = PikiState::Enter {
                            onion,
                            leg,
                            climbing: hover.is_none(),
                            ratio: if hover.is_some() { -ENTER_JUMP_SECONDS } else { 0.0 },
                        };
                        return;
                    }
                    let speed = self.piki_speed(pi, 0.5);
                    self.pikis[pi].target_vel = to.normalize() * speed;
                    self.physics(pi, host, dt);
                    return;
                }
                let r = ratio + CLIMB_SPEED * dt / leg_length(leg, hover);
                let p = &mut self.pikis[pi];
                p.pos = leg_point(opos, leg, r, hover);
                p.vel = Vec3::ZERO;
                p.on_ground = false;
                let d = leg_top(opos, leg, hover) - p.pos;
                p.face = d.x.atan2(d.z);
                if r > ENTER_AT {
                    // `GoalItem::enterGoal`
                    let (color, happa, id) = (p.color, p.happa, p.id);
                    p.state = PikiState::Dead;
                    self.onions[onion.0 as usize].stored[color.index()][happa as usize] += 1;
                    self.events.push(Event::PikiEnteredOnion { piki: id, onion });
                } else {
                    p.state = PikiState::Enter {
                        onion,
                        leg,
                        climbing: true,
                        ratio: r,
                    };
                }
            }
            PikiState::Exit { onion, leg, ratio } => {
                let o = &self.onions[onion.0 as usize];
                let opos = o.pos;
                let hover = flying_pose(o).then_some(o.hover_frame);
                let r = ratio - SLIDE_SPEED * dt / leg_length(leg, hover);
                let p = &mut self.pikis[pi];
                if hover.is_some() && r <= 0.5 {
                    // Host flight: play the first half of ActExit, then fall from that
                    // exact point rather than following the moving leg to its foot.
                    p.pos = leg_point(opos, leg, 0.5, hover);
                    p.vel = Vec3::NEG_Y * SLIDE_SPEED;
                    p.target_vel = p.vel;
                    p.on_ground = false;
                    p.state = PikiState::Flying { ground_frames: 0 };
                    return;
                }
                p.pos = leg_point(opos, leg, r, hover);
                p.vel = Vec3::ZERO;
                let d = p.pos - leg_top(opos, leg, hover);
                p.face = d.x.atan2(d.z);
                if r > 0.0 {
                    p.state = PikiState::Exit {
                        onion,
                        leg,
                        ratio: r,
                    };
                    return;
                }
                p.on_ground = true;
                let id = p.id;
                match p.captain.filter(|c| !self.captains[c.0 as usize].down) {
                    Some(c) => self.join_squad(id, c),
                    None => self.pikis[pi].state = PikiState::Idle,
                }
            }
            _ => {}
        }
    }
}
