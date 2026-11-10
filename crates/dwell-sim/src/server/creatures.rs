//! The server's side of creatures (docs/adr/0073-creatures.md): each tick they spawn near the
//! living players, wander and despawn far from all of them; clients that said `CreaturesHello`
//! are told every `SEND_EVERY` ticks (all of them: at most `limits.max`, 64, ~2 KB).

use super::{Server, SessionId};
use dwell_protocol::{Channel, CreatureNet, CreaturesMessage, MAX_CREATURES_PER_MESSAGE, Message};
use dwell_world::chunk_of;

/// Ticks between sends of the creatures' points (10 a second at 60 Hz).
const SEND_EVERY: u32 = 6;
/// Ticks between spawn rolls (a column rolls once while players are near).
const SPAWN_EVERY: u32 = 30;

impl Server {
    pub(super) fn creatures_hello(&mut self, id: SessionId) {
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        s.creatures_hello = true;
        let table = self.creatures.table;
        let all: Vec<CreatureNet> = self.creatures.list.iter().map(|c| c.net(&table)).collect();
        for part in all.chunks(MAX_CREATURES_PER_MESSAGE) {
            let m = Message::Creatures(CreaturesMessage {
                creatures: part.to_vec(),
                gone: Vec::new(),
            });
            self.send_reliable(id, &m, Channel::World);
        }
    }

    fn living_points(&self) -> Vec<[f64; 3]> {
        self.sessions
            .values()
            .filter(|s| s.joined())
            .filter_map(|s| s.handle)
            .map(|h| self.players.position(&self.physics, h).to_array())
            .collect()
    }

    pub(super) fn step_creatures(&mut self) {
        let players = self.living_points();
        if players.is_empty() && self.creatures.list.is_empty() {
            return;
        }
        let world = &self.world;
        let get = |c: [i32; 3]| {
            world.find(chunk_of(c[0], c[1], c[2]))?;
            Some(world.get_voxel_if_loaded(c))
        };
        let before = self.creatures.list.len();
        if self.tick.is_multiple_of(SPAWN_EVERY) {
            self.creatures
                .spawn_near(&players, &get, self.config.world_seed);
        }
        self.creatures.step(&get);
        self.creatures.despawn_far(&players);
        let gone = std::mem::take(&mut self.creatures.removed);
        let spawned = self.creatures.list.len() > before;
        let table = self.creatures.table;
        let moving: Vec<CreatureNet> = if spawned || self.tick.is_multiple_of(SEND_EVERY) {
            self.creatures.list.iter().map(|c| c.net(&table)).collect()
        } else {
            Vec::new()
        };
        if moving.is_empty() && gone.is_empty() {
            return;
        }
        let hello: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.joined() && s.creatures_hello)
            .map(|(&id, _)| id)
            .collect();
        let mut msgs = Vec::new();
        for part in moving.chunks(MAX_CREATURES_PER_MESSAGE) {
            msgs.push(CreaturesMessage {
                creatures: part.to_vec(),
                gone: Vec::new(),
            });
        }
        for part in gone.chunks(MAX_CREATURES_PER_MESSAGE) {
            msgs.push(CreaturesMessage {
                creatures: Vec::new(),
                gone: part.to_vec(),
            });
        }
        for m in msgs {
            let m = Message::Creatures(m);
            for &id in &hello {
                self.send_reliable(id, &m, Channel::World);
            }
        }
    }

    // --- for tests and tools ---

    /// Replaces the creatures' data (tests: the fixture table) and removes those there are.
    pub fn set_creature_table(&mut self, table: crate::creatures::Table) {
        let gone: Vec<u32> = self.creatures.list.iter().map(|c| c.id).collect();
        self.creatures = crate::creatures::Creatures::new(table);
        self.creatures.removed = gone;
    }

    /// The creatures in the world.
    pub fn creatures(&self) -> &[crate::creatures::Creature] {
        &self.creatures.list
    }
}
