//! The server's side of creatures (docs/adr/0073-creatures.md): each tick they spawn near the
//! living players, wander and despawn far from all of them; clients that said `CreaturesHello`
//! are told every `SEND_EVERY` ticks (all of them: at most `limits.max`, 64, ~2 KB). NPCs
//! (WORK_QUEUE item 38) are creatures too; a client with the `creatures.npcs` capability also gets
//! their part (kind, pose, palette variant) at the end of each message.

use super::{Server, SessionId};
use dwell_protocol::CreaturePoseNet;
use dwell_protocol::capability_names::{CREATURES_NPCS, CREATURES_POSES};
use dwell_protocol::{
    Channel, CreatureNet, CreaturesMessage, MAX_CREATURES_PER_MESSAGE, Message, NpcNet,
};
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
        let npcs = s.capabilities.has(CREATURES_NPCS);
        let poses = npcs && s.capabilities.has(CREATURES_POSES);
        let table = self.creatures.table;
        let all: Vec<CreatureNet> = self.creatures.list.iter().map(|c| c.net(&table)).collect();
        for part in all.chunks(MAX_CREATURES_PER_MESSAGE) {
            let m = CreaturesMessage {
                creatures: part.to_vec(),
                gone: Vec::new(),
                npcs: None,
                poses: None,
            };
            let m = Message::Creatures(self.with_parts(m, npcs, poses));
            self.send_reliable(id, &m, Channel::World);
        }
    }

    /// The animals' idle poses near players (WORK_QUEUE item 39): each herd member's creature id
    /// and its pose's index in `CREATURE_POSES`, stand left out.
    pub fn creature_poses(&self) -> Vec<CreaturePoseNet> {
        let mut out = Vec::new();
        for g in &self.ecology.near.groups {
            for m in &g.members {
                let pose = dwell_ecology::IdlePose::ALL
                    .iter()
                    .position(|&p| p == m.pose)
                    .unwrap_or(0);
                if pose != 0 {
                    out.push(CreaturePoseNet {
                        id: m.creature,
                        pose: pose as u8,
                    });
                }
            }
        }
        out
    }

    /// `m` with its parts for a client's capabilities: the NPCs' (`npcs`, `creatures.npcs`) and
    /// the animals' poses (`poses`, `creatures.poses`); as it is for a client with neither.
    fn with_parts(&self, m: CreaturesMessage, npcs: bool, poses: bool) -> CreaturesMessage {
        let mut m = self.with_npcs(m, npcs);
        if poses {
            let ids: std::collections::HashSet<u32> = m.creatures.iter().map(|c| c.id).collect();
            m.poses = Some(
                self.creature_poses()
                    .into_iter()
                    .filter(|p| ids.contains(&p.id))
                    .collect(),
            );
        }
        m
    }

    /// `m` with its creatures' NPC part, for a client with `creatures.npcs` (`npcs`); as it is
    /// for one without.
    fn with_npcs(&self, mut m: CreaturesMessage, npcs: bool) -> CreaturesMessage {
        if npcs {
            let table = self.creatures.table;
            let part: Vec<NpcNet> = m
                .creatures
                .iter()
                .filter_map(|c| self.creatures.get(c.id)?.npc_net(&table))
                .collect();
            m.npcs = Some(part);
        }
        m
    }

    pub(super) fn living_points(&self) -> Vec<[f64; 3]> {
        self.sessions
            .values()
            .filter(|s| s.joined())
            .filter_map(|s| s.handle)
            .map(|h| self.players.position(&self.physics, h).to_array())
            .collect()
    }

    /// The living players' stances, in [`Self::living_points`]' order (WORK_QUEUE item 39: animals
    /// notice a crouching player closer, a running one farther).
    pub(super) fn living_stances(&self) -> Vec<crate::herds::Stance> {
        self.sessions
            .values()
            .filter(|s| s.joined())
            .filter_map(|s| s.handle)
            .map(|h| {
                let v = self.players.velocity(&self.physics, h);
                crate::herds::Stance {
                    crouching: self.players.controller(h).crouching,
                    speed: f64::from(v.x.hypot(v.z)),
                }
            })
            .collect()
    }

    pub(super) fn step_creatures(&mut self) {
        let players = self.living_points();
        // The near ecology's herds and packs (docs/ECOLOGY.md §8) before the creatures move.
        self.step_herds();
        if players.is_empty() && self.creatures.list.is_empty() {
            return;
        }
        let before = self.creatures.list.len();
        if self.tick.is_multiple_of(SPAWN_EVERY) {
            // The rules' biomes from the world's terrain, once its maps are in (none: unchecked).
            let terrain = self.grass_terrain();
            let world = &self.world;
            let get = |c: [i32; 3]| {
                world.find(chunk_of(c[0], c[1], c[2]))?;
                Some(world.get_voxel_if_loaded(c))
            };
            let biome = |f: dwell_world::FaceFrame, [x, z]: [i32; 2]| {
                let t = terrain.as_ref()?.local(f.face);
                let i = t.biome_mix(i64::from(x), i64::from(z)).chosen;
                t.biomes().get(usize::from(i)).map(|b| b.name)
            };
            self.creatures
                .spawn_near_in(&players, &get, self.config.world_seed, &biome);
        }
        let world = &self.world;
        let get = |c: [i32; 3]| {
            world.find(chunk_of(c[0], c[1], c[2]))?;
            Some(world.get_voxel_if_loaded(c))
        };
        self.creatures.step_near(&get, &players);
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
        let hello: Vec<(SessionId, bool, bool)> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.joined() && s.creatures_hello)
            .map(|(&id, s)| {
                let npcs = s.capabilities.has(CREATURES_NPCS);
                (id, npcs, npcs && s.capabilities.has(CREATURES_POSES))
            })
            .collect();
        let mut msgs = Vec::new();
        for part in moving.chunks(MAX_CREATURES_PER_MESSAGE) {
            msgs.push(CreaturesMessage {
                creatures: part.to_vec(),
                gone: Vec::new(),
                npcs: None,
                poses: None,
            });
        }
        for part in gone.chunks(MAX_CREATURES_PER_MESSAGE) {
            msgs.push(CreaturesMessage {
                creatures: Vec::new(),
                gone: part.to_vec(),
                npcs: None,
                poses: None,
            });
        }
        for m in msgs {
            // Built once each way: with both parts, the NPCs' only, and as older clients read it.
            let all = Message::Creatures(self.with_parts(m.clone(), true, true));
            let with = Message::Creatures(self.with_npcs(m.clone(), true));
            let without = Message::Creatures(m);
            for &(id, npcs, poses) in &hello {
                let msg = match (npcs, poses) {
                    (_, true) => &all,
                    (true, false) => &with,
                    _ => &without,
                };
                self.send_reliable(id, msg, Channel::World);
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
