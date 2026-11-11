//! Loose items (docs/adr/0066-inventory-and-drops.md §5): what a broken block or a dead player
//! dropped, lying in the world until someone walks near or it despawns. Light server entities, not
//! Rapier bodies: a point that falls toward the midplane (face-local, through `FaceFrame`) until the
//! cell under it is solid, then rests on that cell's top, and falls again if that cell goes.

use dwell_protocol::inventory::{DESPAWN_SECONDS, MAX_DROPS, PICKUP_DELAY_SECONDS, Stack};
use dwell_protocol::{ItemDropNet, SIM_HZ};
use dwell_world::FaceFrame;
use dwell_world::{cell_of_pos, m_of_row};

/// Fall acceleration (m/s²) and the fastest fall (m/s: under a cell a tick at 60 Hz, so a loose item
/// never passes through a floor between two ticks).
const GRAVITY: f64 = 20.0;
const MAX_FALL: f64 = 20.0;
/// How far below its point a resting item checks for its support (m).
const SUPPORT_PROBE: f64 = 0.01;

#[derive(Clone, Debug, PartialEq)]
pub struct LooseItem {
    pub id: u32,
    pub stack: Stack,
    /// Its point (world metres): the bottom of its look.
    pub position: [f64; 3],
    /// Face-local fall speed (m/s, down positive); 0 at rest.
    pub fall: f64,
    pub resting: bool,
    /// Ticks since it was dropped.
    pub age: u32,
}

impl LooseItem {
    pub fn net(&self) -> ItemDropNet {
        ItemDropNet {
            id: self.id,
            item: self.stack.item.clone(),
            count: self.stack.count,
            position: self.position.map(|v| v as f32),
        }
    }
    /// Old enough to be picked up.
    pub fn takeable(&self) -> bool {
        f64::from(self.age) >= PICKUP_DELAY_SECONDS * f64::from(SIM_HZ)
    }
}

/// What a tick changed, for the clients: items that moved or appeared, and those gone.
#[derive(Debug, Default)]
pub struct Changes {
    pub moved: Vec<u32>,
    pub gone: Vec<u32>,
}

#[derive(Debug, Default)]
pub struct LooseItems {
    pub items: Vec<LooseItem>,
    next_id: u32,
    /// Items added or changed since the clients were last told (spawned, count changed).
    pub fresh: Vec<u32>,
    /// Items gone since the clients were last told.
    pub removed: Vec<u32>,
}

impl LooseItems {
    /// Drops `stack` at `position` (world metres); the oldest go when the world holds `MAX_DROPS`.
    pub fn drop(&mut self, stack: Stack, position: [f64; 3]) -> u32 {
        if stack.count == 0 {
            return 0;
        }
        while self.items.len() >= MAX_DROPS {
            let gone = self.items.remove(0);
            self.removed.push(gone.id);
        }
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        self.items.push(LooseItem {
            id,
            stack,
            position,
            fall: 0.0,
            resting: false,
            age: 0,
        });
        self.fresh.push(id);
        id
    }

    /// Restores saved items (ids renumbered).
    pub fn restore(&mut self, saved: Vec<LooseItem>) {
        for mut it in saved {
            self.next_id = self.next_id.wrapping_add(1).max(1);
            it.id = self.next_id;
            it.resting = false; // look for the ground again
            self.items.push(it);
        }
    }

    /// One tick: ages every item, removes those past `DESPAWN_SECONDS`, and moves the falling ones.
    /// `solid(cell)` is whether a cell holds something to rest on; `None` for a cell not loaded
    /// (an item there waits). Returns the ids that moved.
    pub fn step(&mut self, solid: &dyn Fn([i32; 3]) -> Option<bool>) -> Vec<u32> {
        let dt = 1.0 / f64::from(SIM_HZ);
        let despawn = (DESPAWN_SECONDS * f64::from(SIM_HZ)) as u32;
        let removed = &mut self.removed;
        self.items.retain(|it| {
            let keep = it.age < despawn;
            if !keep {
                removed.push(it.id);
            }
            keep
        });
        let mut moved = Vec::new();
        for it in &mut self.items {
            it.age += 1;
            let f = FaceFrame::at_height(it.position[1]);
            let mut local = f.point(it.position);
            if it.resting {
                let below = cell_of_pos(f.point([local[0], local[1] - SUPPORT_PROBE, local[2]]));
                if solid(below) != Some(false) {
                    continue;
                }
                it.resting = false;
            }
            let probe = cell_of_pos(f.point(local));
            if solid(probe).is_none() {
                continue; // not loaded: wait
            }
            it.fall = (it.fall + GRAVITY * dt).min(MAX_FALL);
            local[1] -= it.fall * dt;
            let cell = cell_of_pos(f.point(local));
            if solid(cell) == Some(true) {
                // On its top: the cell's face-local top.
                let (a, b) = (f.y(m_of_row(cell[1])), f.y(m_of_row(cell[1] + 1)));
                local[1] = a.max(b);
                it.fall = 0.0;
                it.resting = true;
            }
            it.position = f.point(local);
            moved.push(it.id);
        }
        moved
    }

    /// Takes the item `id` out of the world.
    pub fn take(&mut self, id: u32) -> Option<LooseItem> {
        let i = self.items.iter().position(|it| it.id == id)?;
        self.removed.push(id);
        Some(self.items.remove(i))
    }

    /// Sets an item's count (a part picked up); 0 takes it.
    pub fn set_count(&mut self, id: u32, count: u16) {
        if count == 0 {
            self.take(id);
        } else if let Some(it) = self.items.iter_mut().find(|it| it.id == id) {
            it.stack.count = count;
            self.fresh.push(id);
        }
    }

    pub fn get(&self, id: u32) -> Option<&LooseItem> {
        self.items.iter().find(|it| it.id == id)
    }

    /// The save record: each item's item name, count, point and age (resting is found again).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut w = dwell_protocol::bytes::ByteWriter::new(&mut out);
        w.u8(1);
        w.u32(self.items.len() as u32);
        for it in &self.items {
            w.str(&it.stack.item);
            w.u16(it.stack.count);
            for v in it.position {
                w.f64(v);
            }
            w.u32(it.age);
        }
        out
    }

    /// [`LooseItems::to_bytes`] read back; none if malformed.
    pub fn from_bytes(bytes: &[u8]) -> Option<Vec<LooseItem>> {
        let mut r = dwell_protocol::bytes::ByteReader::new(bytes);
        let version = r.u8();
        r.check(version == 1);
        let n = r.u32() as usize;
        r.check(n <= MAX_DROPS * 4);
        let mut out = Vec::new();
        for _ in 0..n {
            if !r.ok() {
                break;
            }
            let item = r.str(dwell_protocol::inventory::MAX_ITEM_NAME);
            let count = r.u16();
            let position = [r.f64(), r.f64(), r.f64()];
            let age = r.u32();
            r.check(position.iter().all(|v| v.is_finite()));
            if count > 0 && !item.is_empty() {
                out.push(LooseItem {
                    id: 0,
                    stack: Stack::new(&item, count),
                    position,
                    fall: 0.0,
                    resting: false,
                    age,
                });
            }
        }
        r.at_end().then_some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A floor: every cell at or below face-local row `top` solid.
    fn floor(f: FaceFrame, top: i32) -> impl Fn([i32; 3]) -> Option<bool> {
        move |c: [i32; 3]| Some(f.row(c[1]) <= top)
    }

    dwell_world::both_faces!(
        fn an_item_falls_and_rests_on_the_floor_top(f: FaceFrame) {
            let mut items = LooseItems::default();
            // Face-local: a floor whose top row is 10 (its top at 5.5 m), an item dropped at 9 m.
            let solid = floor(f, 10);
            let id = items.drop(Stack::new("dwell:dirt", 1), f.point([3.5, 9.0, 3.5]));
            for _ in 0..120 {
                items.step(&solid);
            }
            let it = items.get(id).unwrap();
            assert!(it.resting);
            let local = f.point(it.position);
            assert!((local[1] - 5.5).abs() < 1e-9, "{local:?}");
            assert_eq!((local[0], local[2]), (3.5, 3.5));
            // The floor goes: it falls to the next.
            let lower = floor(f, 4);
            for _ in 0..120 {
                items.step(&lower);
            }
            let local = f.point(items.get(id).unwrap().position);
            assert!((local[1] - 2.5).abs() < 1e-9, "{local:?}");
        }
    );

    #[test]
    fn items_despawn_after_their_time_and_the_oldest_go_at_the_cap() {
        // face-a-only: ageing and the cap don't depend on which way is up.
        let mut items = LooseItems::default();
        let solid = |_: [i32; 3]| Some(true);
        let first = items.drop(Stack::new("dwell:dirt", 1), [0.5, 10.0, 0.5]);
        let ticks = (DESPAWN_SECONDS * f64::from(SIM_HZ)) as u32;
        for _ in 0..ticks - 1 {
            items.step(&solid);
        }
        let second = items.drop(Stack::new("dwell:dirt", 1), [0.5, 10.0, 0.5]);
        items.step(&solid);
        assert!(items.get(first).is_some());
        items.step(&solid);
        assert!(items.get(first).is_none(), "gone after {DESPAWN_SECONDS} s");
        assert!(items.removed.contains(&first));
        assert!(items.get(second).is_some());
        for _ in 0..MAX_DROPS {
            items.drop(Stack::new("dwell:rock", 1), [0.5, 10.0, 0.5]);
        }
        assert_eq!(items.items.len(), MAX_DROPS);
        assert!(items.get(second).is_none(), "the oldest went first");
    }

    #[test]
    fn the_save_record_round_trips() {
        // face-a-only: a byte format.
        let mut items = LooseItems::default();
        items.drop(Stack::new("dwell:gem", 2), [1.0, -40.0, 2.0]);
        items.drop(Stack::new("mod:unknown", 5), [1.0, 40.0, 2.0]);
        let saved = LooseItems::from_bytes(&items.to_bytes()).unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[1].stack, Stack::new("mod:unknown", 5));
        assert_eq!(saved[0].position, [1.0, -40.0, 2.0]);
        assert!(LooseItems::from_bytes(&items.to_bytes()[..10]).is_none());
    }
}
