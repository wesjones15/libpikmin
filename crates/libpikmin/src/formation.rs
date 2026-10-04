//! A captain's squad formation, after `CPlate` (plugPikiKando/cPlate.cpp).
//!
//! Slots fill an elliptical "plate" behind the captain. Its area grows with the squad size,
//! and its length is capped at `length_limit`, after which it widens instead.

use std::f32::consts::PI;

use glam::Vec3;

use crate::PikiId;
use crate::math::{dir_of, rotate_y};
use crate::params::PlateParams;

#[derive(Debug, Clone, Default)]
pub struct Plate {
    /// Squad members in slot order. Index = slot.
    pub members: Vec<PikiId>,
    /// Slot offsets from `centre`, parallel to `members`.
    pub slot_offsets: Vec<Vec3>,
    /// Ellipse half-width (`mPlateSize`).
    pub size: f32,
    /// Ellipse half-length (`mPlateLength`).
    pub length: f32,
    /// `mInnerRadius`
    pub inner_radius: f32,
    /// Facing of the plate from the captain (`mDirectionAngle`). Usually captain facing + π.
    pub direction: f32,
    /// World centre of the plate (`mPlateCenter`).
    pub centre: Vec3,
    /// `mPlateOffset`: point between the captain and the plate.
    pub offset: Vec3,
    velocity: Vec3,
}

impl Plate {
    pub fn slot_world(&self, slot: usize) -> Vec3 {
        self.centre + self.slot_offsets.get(slot).copied().unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// `CPlate::getSlot`: new members go to the end.
    pub fn add(&mut self, piki: PikiId) {
        if !self.members.contains(&piki) {
            self.members.push(piki);
        }
    }

    /// `CPlate::releaseSlot`: later members shift down one slot.
    pub fn remove(&mut self, piki: PikiId) -> bool {
        if let Some(i) = self.members.iter().position(|&p| p == piki) {
            self.members.remove(i);
            true
        } else {
            false
        }
    }

    pub fn slot_of(&self, piki: PikiId) -> Option<usize> {
        self.members.iter().position(|&p| p == piki)
    }

    /// `CPlate::setPos`: place the plate `direction` from the captain at `position`.
    pub fn set_pos(&mut self, parms: &PlateParams, position: Vec3, direction: f32, velocity: Vec3) {
        // A moving squad tucks in behind the captain; a still one keeps a little gap.
        let offset = if self.velocity.x.hypot(self.velocity.z) > 5.0 {
            0.0
        } else {
            parms.start_offset
        };
        self.direction = direction;
        self.centre = position + dir_of(direction) * (self.length + offset);
        self.velocity = velocity;
        self.offset = position + dir_of(direction) * self.inner_radius;
    }

    /// `CPlate::refresh`: resize the ellipse for the current squad size and recompute slots.
    /// `interp` 0 = rounder plate, 1 = slimmer (driven by the C-stick in the original).
    pub fn refresh(&mut self, parms: &PlateParams, interp: f32) {
        let n = self.members.len() as f32;
        let pos_size = parms.max_pos_size;
        self.inner_radius = 2.1 * pos_size * (n / PI).sqrt();

        let base = 1.9 * pos_size;
        let (max_r, min_r) = if base > self.inner_radius {
            (base, self.inner_radius)
        } else {
            (self.inner_radius, base)
        };
        self.size = max_r - (max_r - min_r) * interp;
        self.length = if self.size > 0.0 {
            4.0 * n * pos_size * pos_size / (self.size * PI)
        } else {
            0.0
        };
        if self.length > parms.length_limit {
            self.length = parms.length_limit;
            self.size = 4.0 * n * pos_size * pos_size / (self.length * PI);
        }
        self.refresh_slots(parms);
    }

    /// `CPlate::refreshSlot`: rows across the ellipse, back to front, alternating direction.
    fn refresh_slots(&mut self, parms: &PlateParams) {
        let total = self.members.len();
        self.slot_offsets.clear();
        let step = 2.0 * parms.max_pos_size;
        let mut z = -self.length;
        let mut flip = 1.0f32;
        while self.slot_offsets.len() < total {
            let half_chord = if self.length > 0.0 {
                (self.length * self.length - z * z).max(0.0).sqrt() * self.size / self.length
            } else {
                0.0
            };
            let per_side = ((half_chord / step) as i32).max(0);
            let mut x = flip * step * per_side as f32;
            for _ in 0..(2 * per_side + 1) {
                if self.slot_offsets.len() >= total {
                    break;
                }
                self.slot_offsets
                    .push(rotate_y(Vec3::new(x, 0.0, z), self.direction));
                x -= step * flip;
            }
            z += step;
            flip = -flip;
        }
    }

    /// `CPlate::rearrangeSlot`: bubble members so those nearest `position` get the earliest slots.
    pub fn rearrange(&mut self, position: Vec3, pos_of: impl Fn(PikiId) -> Vec3) {
        let n = self.members.len();
        for i in (1..n).rev() {
            for j in (1..=i).rev() {
                let dj = (position - pos_of(self.members[j])).length();
                let dp = (position - pos_of(self.members[j - 1])).length();
                if dj < dp {
                    self.members.swap(j, j - 1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plate(n: u32) -> Plate {
        let mut p = Plate::default();
        for i in 0..n {
            p.add(PikiId(i));
        }
        p
    }

    #[test]
    fn every_member_gets_a_distinct_slot() {
        let parms = PlateParams::default();
        for n in [1, 5, 20, 100] {
            let mut p = plate(n);
            p.refresh(&parms, 0.0);
            p.set_pos(&parms, Vec3::ZERO, 0.0, Vec3::ZERO);
            assert_eq!(p.slot_offsets.len(), n as usize);
            for a in 0..n as usize {
                for b in (a + 1)..n as usize {
                    let d = (p.slot_offsets[a] - p.slot_offsets[b]).length();
                    assert!(d > 1.0, "slots {a} and {b} overlap ({d}) for n={n}");
                }
            }
        }
    }

    #[test]
    fn plate_sits_behind_when_direction_is_backwards() {
        let parms = PlateParams::default();
        let mut p = plate(10);
        p.refresh(&parms, 0.0);
        // Captain faces +Z, so the plate goes towards -Z.
        p.set_pos(&parms, Vec3::ZERO, std::f32::consts::PI, Vec3::ZERO);
        for s in 0..10 {
            assert!(p.slot_world(s).z < 0.0, "slot {s} at {:?}", p.slot_world(s));
        }
    }

    #[test]
    fn length_is_capped() {
        let parms = PlateParams::default();
        let mut p = plate(100);
        p.refresh(&parms, 1.0);
        assert!(p.length <= parms.length_limit + 1e-3);
    }
}
