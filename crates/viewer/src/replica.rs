//! `PIKMIN_REPLICA=1`: draws a replica of the sim instead of the sim itself, to see what another
//! machine in a networked host would see. Every three ticks the real sim is snapshotted (and the
//! snapshot sent through `Snapshot::encode` and `decode`, as it would be over a network), the
//! replica takes it, and positions blend between the last two snapshots as time passes. The
//! events (sounds, effects) play when a snapshot arrives, not when they happened.

use libpikmin::{Event, Params, Sim, Snapshot};

pub struct Replica {
    pub sim: Sim,
    prev: Option<Snapshot>,
    cur: Option<Snapshot>,
    pending: Vec<Event>,
    last_sent: u64,
    since: f32,
    interval: f32,
    /// Bytes of the last snapshot, for the on-screen note.
    pub last_bytes: usize,
}

impl Replica {
    pub fn new(params: Params) -> Self {
        Replica {
            sim: Sim::new(params, 0),
            prev: None,
            cur: None,
            pending: Vec::new(),
            last_sent: 0,
            since: 0.0,
            interval: 0.1,
            last_bytes: 0,
        }
    }

    /// One frame: `events` are what the real sim just raised. Returns the events to play now, which are
    /// those of a snapshot that has just "arrived".
    pub fn step(&mut self, real: &Sim, events: Vec<Event>, dt: f32) -> Vec<Event> {
        self.pending.extend(events);
        self.since += dt;
        let mut play = Vec::new();
        if self.cur.is_none() || real.tick() >= self.last_sent + 3 {
            self.last_sent = real.tick();
            let bytes = Snapshot::of(real, &self.pending).encode();
            self.last_bytes = bytes.len();
            self.pending.clear();
            if let Some(snap) = Snapshot::decode(&bytes) {
                play = snap.events.clone();
                if self.cur.is_some() {
                    self.interval += (self.since.clamp(0.02, 0.5) - self.interval) * 0.2;
                }
                self.since = 0.0;
                self.prev = self.cur.take();
                let from = self.prev.clone().unwrap_or_else(|| snap.clone());
                self.sim.apply_snapshot_blend(&from, &snap, 0.0);
                self.cur = Some(snap);
            }
        }
        if let Some(cur) = self.cur.as_ref() {
            let from = self.prev.as_ref().unwrap_or(cur);
            self.sim.apply_snapshot_blend(from, cur, (self.since / self.interval).min(1.0));
            self.sim.set_tick(cur.tick + (self.since * libpikmin::TICK_HZ as f32) as u64);
        }
        play
    }
}
