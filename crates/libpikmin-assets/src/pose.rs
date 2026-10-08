//! Posing a model: joint matrices from an animation, envelope matrices for weighted
//! vertices, and CPU skinning.
//!
//! `BaseShape::updateAnim` builds each joint's matrix as parent * SRT (root parent = the
//! model's world matrix). `calcWeightedMatrices` sums `weight * joint * inverse_bind` per
//! envelope, where the bind pose comes from `calcBasePose` with an identity root.

use glam::{Mat4, Vec3, Vec4};

use crate::anim::{Anim, Srt};
use crate::model::{BatchVertex, Model, Skin};

/// `Matrix4f::makeSRT` (matMath.cpp): T * Rz * Ry * Rx * S.
pub fn srt_matrix(s: Vec3, r: Vec3, t: Vec3) -> Mat4 {
    let (sx, cx) = r.x.sin_cos();
    let (sy, cy) = r.y.sin_cos();
    let (sz, cz) = r.z.sin_cos();
    // Columns of the rotation (row/col naming follows mMtx[row][col] in the decomp).
    let c0 = Vec4::new(cy * cz, cy * sz, -sy, 0.0) * s.x;
    let c1 = Vec4::new(sx * sy * cz - cx * sz, sx * sy * sz + cx * cz, sx * cy, 0.0) * s.y;
    let c2 = Vec4::new(cx * cz * sy + sx * sz, cx * sz * sy - sx * cz, cx * cy, 0.0) * s.z;
    Mat4::from_cols(c0, c1, c2, t.extend(1.0))
}

/// Joint data needed to pose a model, precomputed once per model.
#[derive(Debug, Clone)]
pub struct Skeleton {
    pub parents: Vec<i32>,
    pub bind: Vec<Srt>,
    pub inverse_bind: Vec<Mat4>,
    pub envelopes: Vec<Vec<(u16, f32)>>,
}

impl Skeleton {
    pub fn new(model: &Model) -> Self {
        let parents: Vec<i32> = model.joints.iter().map(|j| j.parent).collect();
        let bind: Vec<Srt> = model
            .joints
            .iter()
            .map(|j| Srt {
                scale: j.scale,
                rotation: j.rotation,
                translation: j.translation,
            })
            .collect();
        let world = Self::world(&parents, Mat4::IDENTITY, |i| bind[i]);
        Skeleton {
            parents,
            bind,
            inverse_bind: world.iter().map(Mat4::inverse).collect(),
            envelopes: model.envelopes.iter().map(|e| e.weights.clone()).collect(),
        }
    }

    /// Which override drives each joint: `BaseShape::overrideAnim(joint, ctx)` points a
    /// joint and everything under it at a context, applied in the order given, so later ones
    /// win. Joint `i` gets the index into `joints` of the last override covering it (0 when
    /// none does).
    pub fn override_map(&self, joints: &[usize]) -> Vec<usize> {
        let mut map = vec![0; self.parents.len()];
        for (ctx, &root) in joints.iter().enumerate() {
            for j in self.subtree(root) {
                map[j] = ctx;
            }
        }
        map
    }

    /// `root` and every joint under it.
    pub fn subtree(&self, root: usize) -> Vec<usize> {
        let mut inside = vec![false; self.parents.len()];
        let mut out = Vec::new();
        for (i, &p) in self.parents.iter().enumerate() {
            let under = i == root || (p >= 0 && (p as usize) < i && inside[p as usize]);
            if under && i >= root {
                inside[i] = true;
                out.push(i);
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.parents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parents.is_empty()
    }

    fn world(parents: &[i32], root: Mat4, local: impl Fn(usize) -> Srt) -> Vec<Mat4> {
        let mut out: Vec<Mat4> = Vec::with_capacity(parents.len());
        for (i, &p) in parents.iter().enumerate() {
            let l = local(i);
            let parent = if p >= 0 && (p as usize) < i {
                out[p as usize]
            } else {
                root
            };
            out.push(parent * srt_matrix(l.scale, l.rotation, l.translation));
        }
        out
    }

    /// World matrices for every joint. Joints the animation has no track for keep their
    /// bind pose.
    pub fn pose(&self, root: Mat4, anim: Option<(&Anim, f32)>) -> Pose {
        self.pose_by(root, |_| anim)
    }

    /// Blend two samples of the same clip for a seamless extracted flight loop.
    pub fn pose_crossfade(&self, anim: &Anim, frame: f32, next: f32, weight: f32) -> Pose {
        let weight = weight.clamp(0.0, 1.0);
        let mut pose = self.pose(Mat4::IDENTITY, Some((anim, frame)));
        let other = self.pose(Mat4::IDENTITY, Some((anim, next)));
        for (a, b) in pose.joints.iter_mut().zip(&other.joints) {
            let (sa, ra, ta) = a.to_scale_rotation_translation();
            let (sb, rb, tb) = b.to_scale_rotation_translation();
            *a = Mat4::from_scale_rotation_translation(sa.lerp(sb, weight), ra.slerp(rb, weight), ta.lerp(tb, weight));
        }
        for (matrix, weights) in pose.envelopes.iter_mut().zip(&self.envelopes) {
            *matrix = Mat4::ZERO;
            for &(joint, weight) in weights {
                if let (Some(jm), Some(ib)) = (pose.joints.get(joint as usize), self.inverse_bind.get(joint as usize)) {
                    *matrix += *jm * *ib * weight;
                }
            }
        }
        pose
    }

    /// Like [`pose`](Self::pose) with each joint's animation and frame chosen by `anim`
    /// (`BaseShape::updateAnim` reads joint `i` from `mAnimOverrides[i]`).
    pub fn pose_by<'a>(&self, root: Mat4, anim: impl Fn(usize) -> Option<(&'a Anim, f32)>) -> Pose {
        let joints = Self::world(&self.parents, root, |i| {
            anim(i).and_then(|(a, f)| a.sample(i, f)).unwrap_or(self.bind[i])
        });
        let envelopes = self
            .envelopes
            .iter()
            .map(|weights| {
                let mut m = Mat4::ZERO;
                for &(j, w) in weights {
                    let j = j as usize;
                    if let (Some(jm), Some(ib)) = (joints.get(j), self.inverse_bind.get(j)) {
                        m += (*jm * *ib) * w;
                    }
                }
                m
            })
            .collect();
        Pose {
            root,
            joints,
            envelopes,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Pose {
    pub root: Mat4,
    pub joints: Vec<Mat4>,
    pub envelopes: Vec<Mat4>,
}

impl Pose {
    pub fn matrix(&self, skin: Skin) -> Mat4 {
        match skin {
            Skin::Joint(j) => self.joints.get(j as usize).copied().unwrap_or(self.root),
            Skin::Envelope(e) => self.envelopes.get(e as usize).copied().unwrap_or(self.root),
            Skin::Root => self.root,
        }
    }

    /// World position and (unnormalised) normal of a batch vertex.
    pub fn skin(&self, v: &BatchVertex) -> (Vec3, Vec3) {
        let m = self.matrix(v.skin);
        (m.transform_point3(v.position), m.transform_vector3(v.normal))
    }
}
