//! The leg pose solver of Pikmin 2's Long Legs (Beady Long Legs, Raging Long Legs, Man-at-Legs): the two-bone inverse
//! kinematics `IKSystemBase::makeMatrix` runs on each three-joint leg (`rhand1jnt`/`rhand2jnt`/`rhand3jnt` ..), and the
//! maths it calls (`NsMathExp::calcJointPos`, `calcLagrange`, nslibmath.cpp). Engine-free: it knows no model, only
//! points, lengths and the matrices it is handed.
//!
//! A leg is a hip (the *top* joint, which the body's animation carries), a knee (the *middle*) and a foot (the
//! *bottom*, which is where the leg is asked to stand). Given the hip, the foot target and the two segment lengths
//! measured when the legs were set up, [`solve_leg`] finds the knee (`calcJointPos`: the circle of knees that keep both
//! lengths, on the side the bend direction names) and returns the three joints' world matrices: the X axis of the
//! hip and knee runs along their segment, the foot points down and turns with the bend ratio (`makeBottomMatrix`).
//! While the body plays a blended animation (the flick) the joints keep the animation's own twist and only their
//! positions and X axes follow the solve (`mIsBlendMotionActive`).
//!
//! Conventions (the decomp's J3D ones): a matrix's columns 0, 1, 2 are the joint's X, Y, Z axes and column 3 its place.

use glam::{Mat4, Vec3, Vec4};

/// Where the first, second and third joints of a leg are in [`LegPose`] and the rig's tables.
pub const TOP: usize = 0;
pub const MIDDLE: usize = 1;
pub const BOTTOM: usize = 2;

/// `NsMathExp::calcLagrange`: the foot's path of a step, the quadratic through three control points at `t` = 0, 1, 2
/// (`controlPoints[0]` where it lifted from, `[1]` the peak, `[2]` where it lands).
pub fn calc_lagrange(p: &[Vec3; 3], t: f32) -> Vec3 {
    let (t1, t2) = (t - 1.0, t - 2.0);
    // The decomp's term order: (t-1)(P2/2 t) + ((t-2)(P0/2 (t-1)) - (t-2)(P1 t)).
    t1 * (p[2] * 0.5 * t) + (t2 * (p[0] * 0.5 * t1) - t2 * (p[1] * t))
}

/// `NsMathExp::calcJointPos`: the middle joint of a two-segment leg from its ends and segment lengths. `bend` says
/// which way the knee points (its part across the line from hip to foot is used); a leg out of reach (or one asked to
/// stand on its hip) is laid straight, the knee in proportion along the line.
pub fn calc_joint_pos(top: Vec3, bottom: Vec3, top_to_middle: f32, middle_to_bottom: f32, bend: Vec3) -> Vec3 {
    let line = bottom - top;
    let (a, b) = (top_to_middle * top_to_middle, middle_to_bottom * middle_to_bottom);
    let d = line.length_squared();
    if d >= 0.000001 {
        let along = 0.5 / d * (d + (a - b));
        let centre = top + line * along;
        let radius_sq = a - (centre - top).length_squared();
        if radius_sq > 0.0 {
            let plane_normal = bend.cross(line);
            let out = line.cross(plane_normal);
            let out_sq = out.length_squared();
            if out_sq != 0.0 {
                return centre + out * (radius_sq / out_sq).sqrt();
            }
        }
    }
    let ratio = top_to_middle / (top_to_middle + middle_to_bottom);
    top + line * ratio
}

/// One leg's solve: what `IKSystemBase::makeMatrix` reads.
#[derive(Debug, Clone, Copy)]
pub struct LegInput {
    /// The hip's place (`mLegJointMatrices[TOP]`'s translation).
    pub top: Vec3,
    /// Where the foot is to be (`mTargetPosition`).
    pub foot: Vec3,
    /// The segment lengths measured when the legs were set up (`mTopToMiddleDistance`, `mMiddleToBottomDistance`).
    pub top_to_middle: f32,
    pub middle_to_bottom: f32,
    /// The foot's tilt in radians about its Z axis (`mBendRatio`, see [`bend_ratio`]).
    pub bend: f32,
    /// The animation's own joints while it is blended with the solve (`mIsBlendMotionActive`), else `None`.
    pub blend: Option<Blend>,
}

/// The animated matrices a blended solve keeps the twist of (the hip's X and Y axes, the knee's Y axis).
#[derive(Debug, Clone, Copy)]
pub struct Blend {
    pub top_x: Vec3,
    pub top_y: Vec3,
    pub middle_y: Vec3,
}

/// A leg's three joints, world matrices.
#[derive(Debug, Clone, Copy)]
pub struct LegPose {
    pub top: Mat4,
    pub middle: Mat4,
    pub bottom: Mat4,
}

fn axes(x: Vec3, y: Vec3, z: Vec3, at: Vec3) -> Mat4 {
    Mat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), at.extend(1.0))
}

/// `PSMTXConcat(m, rotZ(angle))`: turns the joint about its own Z axis.
fn rotate_local_z(m: Mat4, angle: f32) -> Mat4 {
    let (s, c) = angle.sin_cos();
    let (x, y) = (m.x_axis, m.y_axis);
    Mat4::from_cols(x * c + y * s, y * c - x * s, m.z_axis, m.w_axis)
}

/// `IKSystemBase::makeMatrix`: the pose of one leg standing on `input.foot`.
///
/// `animated` is the three joints as the body's animation left them (`mLegJointMatrices`): only their twist is read,
/// and only while `input.blend` is set; pass the identity otherwise.
pub fn solve_leg(input: &LegInput) -> LegPose {
    let top = input.top;
    let foot = input.foot;

    // `getMiddleDirection`: the animation's own X axis while blended, else up and away from the foot.
    let mut flat = foot - top;
    flat.y = 0.0;
    let flat = flat.normalize_or_zero();
    let middle_dir = match input.blend {
        Some(b) => b.top_x,
        None => flat + Vec3::new(0.0, 100.0, 0.0),
    };
    let knee = calc_joint_pos(top, foot, input.top_to_middle, input.middle_to_bottom, middle_dir);

    // `setTopJointRotation` / `setMiddleJointRotation`: X along the segment; Y and Z from the side the leg bends to
    // (or, blended, from the animation's Y).
    let side = Vec3::new(foot.z - top.z, 0.0, top.x - foot.x);
    let joint = |x: Vec3, old_y: Option<Vec3>| -> (Vec3, Vec3, Vec3) {
        let (y, z) = match old_y {
            Some(old) => {
                let z = x.cross(old);
                (z.cross(x), z)
            }
            None => {
                let y = side.cross(x);
                (y, x.cross(y))
            }
        };
        (x.normalize_or_zero(), y.normalize_or_zero(), z.normalize_or_zero())
    };
    let (tx, ty, tz) = joint(knee - top, input.blend.map(|b| b.top_y));
    let (mx, my, mz) = joint(foot - knee, input.blend.map(|b| b.middle_y));

    let top_m = axes(tx, ty, tz, top);
    let middle_m = axes(mx, my, mz, knee);

    // `makeBottomMatrix`: the foot faces along the ground from the hip, points down, and tilts by the bend.
    let bottom_m = if input.blend.is_some() {
        // Blended: the animation's own foot orientation stays; the caller keeps it and only the place moves.
        axes(Vec3::X, Vec3::Y, Vec3::Z, foot)
    } else {
        let mut x = foot - top;
        x.y = 0.0;
        let x = x.normalize_or_zero();
        rotate_local_z(axes(x, Vec3::NEG_Y, mz, foot), input.bend)
    };
    LegPose { top: top_m, middle: middle_m, bottom: bottom_m }
}

/// `IKSystemBase::makeBendRatio`: the foot's tilt while a leg is in the air: zero on the ground, rising to
/// `bend_factor` through the first part of the step and falling back as it comes down.
pub fn bend_ratio(on_ground: bool, move_ratio: f32, bend_factor: f32) -> f32 {
    if on_ground {
        return 0.0;
    }
    let ratio = 1.33 * ((1.0 - (1.0 - move_ratio).abs()) - 0.25);
    bend_factor * ratio.clamp(0.0, 1.0)
}

/// What a body gives the solver for the leg joints it overrides: world matrices of the joints before the solve.
/// [`apply_leg`] writes the solved joints over them.
pub fn apply_leg(joints: &mut [Mat4], rig: [usize; 3], pose: &LegPose, keep_bottom_rotation: bool) {
    joints[rig[TOP]] = pose.top;
    joints[rig[MIDDLE]] = pose.middle;
    if keep_bottom_rotation {
        // The blended foot keeps the animation's axes and takes only the new place.
        joints[rig[BOTTOM]].w_axis = Vec4::from((pose.bottom.w_axis.truncate(), 1.0));
    } else {
        joints[rig[BOTTOM]] = pose.bottom;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leg(foot: Vec3) -> LegInput {
        LegInput { top: Vec3::new(0.0, 180.0, 0.0), foot, top_to_middle: 160.0, middle_to_bottom: 120.0, bend: 0.0, blend: None }
    }

    #[test]
    fn the_foot_lands_where_it_was_asked_and_both_segments_keep_their_length() {
        for foot in [Vec3::new(75.0, 0.0, 75.0), Vec3::new(-100.0, 6.0, 40.0), Vec3::new(0.0, 0.0, 150.0), Vec3::new(30.0, 40.0, -90.0)] {
            let i = leg(foot);
            let p = solve_leg(&i);
            let (top, knee, bottom) = (p.top.w_axis.truncate(), p.middle.w_axis.truncate(), p.bottom.w_axis.truncate());
            assert!((bottom - foot).length() < 1e-4, "the foot is on its target: {bottom} vs {foot}");
            assert!(((knee - top).length() - 160.0).abs() < 1e-2, "hip to knee {} for {foot}", (knee - top).length());
            assert!(((bottom - knee).length() - 120.0).abs() < 1e-2, "knee to foot {} for {foot}", (bottom - knee).length());
        }
    }

    #[test]
    fn the_knee_bends_up_and_out_and_each_x_axis_runs_along_its_segment() {
        let i = leg(Vec3::new(75.0, 0.0, 75.0));
        let p = solve_leg(&i);
        let knee = p.middle.w_axis.truncate();
        // Out beyond the line from hip to foot and above its middle.
        assert!(knee.x > 75.0 * 0.5 && knee.z > 75.0 * 0.5 && knee.y > 90.0, "{knee}");
        let hip_x = p.top.x_axis.truncate();
        assert!((hip_x - (knee - i.top).normalize()).length() < 1e-4);
        let knee_x = p.middle.x_axis.truncate();
        assert!((knee_x - (i.foot - knee).normalize()).length() < 1e-4);
        // Orthonormal joints (no stretching).
        for m in [p.top, p.middle, p.bottom] {
            let (x, y, z) = (m.x_axis.truncate(), m.y_axis.truncate(), m.z_axis.truncate());
            assert!((x.length() - 1.0).abs() < 1e-4 && (y.length() - 1.0).abs() < 1e-4 && (z.length() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn a_foot_out_of_reach_is_laid_straight_toward_it() {
        let i = leg(Vec3::new(0.0, 0.0, 600.0));
        let p = solve_leg(&i);
        let (top, knee) = (p.top.w_axis.truncate(), p.middle.w_axis.truncate());
        let on_line = top + (i.foot - top) * (160.0 / 280.0);
        assert!((knee - on_line).length() < 1e-3, "{knee} vs {on_line}");
        // The target is where the foot goes even when the legs cannot reach it: the leg is as long as asked.
        assert!((p.bottom.w_axis.truncate() - i.foot).length() < 1e-4);
    }

    #[test]
    fn the_foot_points_down_and_tilts_with_the_bend() {
        let flat = solve_leg(&leg(Vec3::new(75.0, 0.0, 75.0)));
        assert!((flat.bottom.y_axis.truncate() - Vec3::NEG_Y).length() < 1e-5);
        let mut i = leg(Vec3::new(75.0, 0.0, 75.0));
        i.bend = 0.5;
        let tilted = solve_leg(&i);
        let angle = flat.bottom.y_axis.truncate().angle_between(tilted.bottom.y_axis.truncate());
        assert!((angle - 0.5).abs() < 1e-4, "{angle}");
    }

    #[test]
    fn blending_keeps_the_animations_twist_but_the_legs_still_reach_their_feet() {
        let mut i = leg(Vec3::new(75.0, 0.0, 75.0));
        i.blend = Some(Blend { top_x: Vec3::new(1.0, 0.0, 1.0), top_y: Vec3::Y, middle_y: Vec3::Y });
        let p = solve_leg(&i);
        let (top, knee, bottom) = (p.top.w_axis.truncate(), p.middle.w_axis.truncate(), p.bottom.w_axis.truncate());
        assert!((bottom - i.foot).length() < 1e-4);
        assert!(((knee - top).length() - 160.0).abs() < 1e-2 && ((bottom - knee).length() - 120.0).abs() < 1e-2);
        // The joints are right-handed and orthonormal whatever the animation's Y.
        for m in [p.top, p.middle] {
            let (x, y, z) = (m.x_axis.truncate(), m.y_axis.truncate(), m.z_axis.truncate());
            assert!(x.dot(y).abs() < 1e-4 && x.dot(z).abs() < 1e-4 && y.dot(z).abs() < 1e-4);
        }
    }

    #[test]
    fn the_steps_path_runs_through_its_three_points() {
        let pts = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 120.0, 0.0), Vec3::new(80.0, 0.0, 0.0)];
        assert!((calc_lagrange(&pts, 0.0) - pts[0]).length() < 1e-4);
        assert!((calc_lagrange(&pts, 1.0) - pts[1]).length() < 1e-4);
        assert!((calc_lagrange(&pts, 2.0) - pts[2]).length() < 1e-4);
    }

    #[test]
    fn the_bend_rises_through_the_step_and_is_zero_on_the_ground() {
        assert_eq!(bend_ratio(true, 1.0, 0.67), 0.0);
        assert_eq!(bend_ratio(false, 0.0, 0.67), 0.0);
        assert!((bend_ratio(false, 1.0, 0.67) - 0.67 * 0.9975).abs() < 1e-5, "peak is 1.33 * 0.75 of the factor");
        assert!(bend_ratio(false, 0.5, 0.67) > 0.0 && bend_ratio(false, 0.5, 0.67) < 0.67);
    }
}
