//! The only way the sim sees the world. Everything is in decomp units; hosts convert.

use glam::Vec3;

use crate::{CaptainId, PikiId};

/// Who landed a hit on a creature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSource {
    Piki(PikiId),
    /// A captain's punch (`NaviAttackState`).
    Captain(CaptainId),
    /// Host extension: a captain's dismiss strike ([`crate::Sim::set_dismiss_strike`]).
    Strike(CaptainId),
}

/// Host-chosen stable handle for a creature (enemy, animal, boss...).
pub type CreatureId = u64;

/// A collision sphere on a creature, in world space this tick. Pikmin latch onto these.
/// Part order must stay stable for a creature between ticks: latched Pikmin remember the index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CreaturePart {
    pub centre: Vec3,
    pub radius: f32,
    /// Pikmin can latch on (decomp `CollPart::isStickable`). Non-stickable parts still block.
    pub stickable: bool,
}

/// What the sim needs to know about one creature this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CreatureView {
    pub id: CreatureId,
    /// Feet/origin position.
    pub pos: Vec3,
    /// Facing, decomp convention (`(sin yaw, 0, cos yaw)` is forward).
    pub yaw: f32,
    pub alive: bool,
    /// Flying creatures can be latched by thrown Pikmin, but not targeted from the ground
    /// (`ActAttack::exec` drops targets that start flying).
    pub flying: bool,
    /// Slice of [`CreatureFrame::parts`]. Must be non-empty for the creature to be attackable.
    pub parts_start: u32,
    pub parts_len: u32,
}

impl CreatureView {
    /// Global indices of this creature's parts in [`CreatureFrame::parts`].
    pub fn parts(&self) -> std::ops::Range<u32> {
        self.parts_start..self.parts_start + self.parts_len
    }
}

/// Snapshot of creatures, refilled by the host every tick. Reused to avoid allocation.
#[derive(Debug, Default, Clone)]
pub struct CreatureFrame {
    pub creatures: Vec<CreatureView>,
    pub parts: Vec<CreaturePart>,
}

impl CreatureFrame {
    pub fn clear(&mut self) {
        self.creatures.clear();
        self.parts.clear();
    }

    /// Convenience for hosts: push a creature and its parts.
    pub fn push(
        &mut self,
        id: CreatureId,
        pos: Vec3,
        yaw: f32,
        alive: bool,
        flying: bool,
        parts: &[CreaturePart],
    ) {
        let start = self.parts.len() as u32;
        self.parts.extend_from_slice(parts);
        self.creatures.push(CreatureView {
            id,
            pos,
            yaw,
            alive,
            flying,
            parts_start: start,
            parts_len: parts.len() as u32,
        });
    }

    pub fn get(&self, id: CreatureId) -> Option<&CreatureView> {
        self.creatures.iter().find(|c| c.id == id)
    }

    pub fn parts_of(&self, c: &CreatureView) -> &[CreaturePart] {
        &self.parts[c.parts().start as usize..c.parts().end as usize]
    }

    /// Bounding centre and radius over all parts (decomp `getCentre`/`getCentreSize` stand-in).
    pub fn bounds(&self, c: &CreatureView) -> (Vec3, f32) {
        let parts = self.parts_of(c);
        if parts.is_empty() {
            return (c.pos, 0.0);
        }
        let centre = parts.iter().map(|p| p.centre).sum::<Vec3>() / parts.len() as f32;
        let radius = parts
            .iter()
            .map(|p| (p.centre - centre).length() + p.radius)
            .fold(0.0, f32::max);
        (centre, radius)
    }
}

pub trait Host {
    /// The highest ground at or below `pos` (straight down), or `None` over a void. A host
    /// with real 3D geometry (raycasts) can't see a floor the probe starts under, so callers
    /// probe from above where they expect the ground: a body's feet plus its step height.
    fn ground_height(&self, pos: Vec3) -> Option<f32>;

    /// Push a sphere of `radius` centred at `centre` out of walls and return the corrected
    /// centre. Floors are [`Host::ground_height`]'s job, so only push sideways. Stands in for
    /// the map half of `MapMgr::traceMove` (mapMgr.cpp), which sweeps the same sphere, sitting
    /// `radius` above the creature's feet, through the map's collision triangles. Default: no
    /// walls.
    fn walls(&self, centre: Vec3, _radius: f32) -> Vec3 {
        centre
    }

    /// Fill `out` with every creature Pikmin may interact with. Called once per tick.
    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
    }

    /// A Pikmin hits a creature (decomp `InteractAttack`). Return false if the hit was
    /// refused (invulnerable, already dead...).
    fn damage(
        &mut self,
        _creature: CreatureId,
        _amount: f32,
        _part: Option<u32>,
        _from: DamageSource,
    ) -> bool {
        false
    }

    /// Waypoints for carrying something from `from` to an Onion at `to` (the last point should
    /// be `to`). Pikmin 1 uses hand-placed route graphs (`RouteMgr`); hosts use their own
    /// navmesh. Return false if there's no way, and the carriers put it down. Default: straight line.
    fn carry_path(&mut self, _from: Vec3, to: Vec3, out: &mut Vec<Vec3>) -> bool {
        out.clear();
        out.push(to);
        true
    }
}

/// Infinite flat floor at y = 0 and no creatures. Useful for tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct FlatHost;

impl Host for FlatHost {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }
}
