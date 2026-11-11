//! The server's side of inventories and loose items (docs/adr/0066-inventory-and-drops.md): what
//! breaking drops and placing costs, pickup, death drops, and telling clients that understand them.

use super::{Server, SessionId};
use dwell_blocks::{StateId, block_of, info};
use dwell_physics::DVec3;
use dwell_protocol::inventory::{self as inv, Inventory, PICKUP_RADIUS_M, Stack};
use dwell_protocol::modes::Ability;
use dwell_protocol::{
    Channel, InventoryAction, InventoryMessage, ItemDrops, MAX_ITEM_DROPS_PER_MESSAGE, Message,
    inventory_flags,
};
use dwell_world::{FaceFrame, M, chunk_of};

/// Ticks between sends of falling items' points (20 a second at 60 Hz).
const MOVED_EVERY: u32 = 3;

/// The item placing a state uses up: its block's `cost` (a ramp's base), else the block itself.
pub(super) fn cost_of(m: StateId) -> String {
    let name = block_of(m).name;
    inv::drops_of(name).map_or(name, |d| d.cost).to_owned()
}

/// What a region's changes place, by item: (item, count).
pub(super) fn region_costs(changes: &[([i32; 3], StateId)]) -> Vec<(String, u32)> {
    let mut need: Vec<(String, u32)> = Vec::new();
    for &(_, m) in changes {
        if m == M.air || info(m).liquid {
            continue;
        }
        let item = cost_of(m);
        match need.iter_mut().find(|(i, _)| *i == item) {
            Some((_, n)) => *n += 1,
            None => need.push((item, 1)),
        }
    }
    need
}

impl Server {
    /// Whether placing uses items up and breaking drops them: a mode without `unlimitedBlocks`.
    pub(super) fn uses_items(&self) -> bool {
        !self.settings.allows(Ability::UnlimitedBlocks)
    }

    pub(super) fn random(&mut self) -> f64 {
        let mut b = [0u8; 8];
        self.entropy.fill(&mut b);
        (u64::from_le_bytes(b) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Drops what breaking `was` gives (its block's `drops`; a block the data doesn't list, a mod's,
    /// drops itself) at `at` (world metres).
    pub(super) fn drop_block(&mut self, was: StateId, at: [f64; 3]) {
        if was == M.air || info(was).liquid {
            return;
        }
        let name = block_of(was).name;
        let stacks = match inv::drops_of(name) {
            // Rolled with the server's entropy.
            Some(d) => inv::roll(d.drops, &mut || self.random()),
            None => vec![Stack::new(name, 1)],
        };
        for s in stacks {
            self.loose.drop(s, at);
        }
    }

    /// A broken leaf of a recorded species tree drops its species' sapling at the species'
    /// `sapling.dropChance` (ADR 0079 S4), rolled with the server's entropy, besides the leaf's own
    /// drops. `cell` is the broken cell (world), `at` where the drop falls from.
    pub(super) fn drop_sapling(&mut self, cell: [i32; 3], at: [f64; 3]) {
        let Some(r) = dwell_worldgen::growth::tree_of_leaf(&self.growth, cell).cloned() else {
            return;
        };
        if let Some(item) = dwell_worldgen::growth::sapling_drop(&r, self.random()) {
            self.loose.drop(Stack::new(item, 1), at);
        }
    }

    /// What hit `k` of the `n` that break block `was` flings (docs/BLOCK_DAMAGE.md): its share of
    /// the block's certain drops (half of them spread over the hits before the last).
    pub(super) fn drop_hit(&mut self, was: StateId, at: [f64; 3], n: u32, k: u32) {
        for (item, total) in certain_drops_of(was) {
            let c = dwell_protocol::damage::hit_share(total, n, k);
            if c > 0 {
                self.loose.drop(Stack::new(item, c as u16), at);
            }
        }
    }

    /// What block `was`, broken by the last of `n` hits, drops: its drops less what the hits gave
    /// ([`drop_hit`](Self::drop_hit)), so the hits and the destroy total the block's drops.
    pub(super) fn drop_destroyed(&mut self, was: StateId, at: [f64; 3], n: u32) {
        if was == M.air || info(was).liquid {
            return;
        }
        let name = block_of(was).name;
        let mut stacks = match inv::drops_of(name) {
            Some(d) => inv::roll(d.drops, &mut || self.random()),
            None => vec![Stack::new(name, 1)],
        };
        for (item, total) in certain_drops_of(was) {
            let mut given = (total - dwell_protocol::damage::destroy_share(total, n)) as u16;
            for st in stacks.iter_mut().filter(|st| st.item == item) {
                let off = given.min(st.count);
                st.count -= off;
                given -= off;
            }
        }
        for s in stacks.into_iter().filter(|s| s.count > 0) {
            self.loose.drop(s, at);
        }
    }

    /// A player's saved inventory, at join.
    pub(super) fn load_inventory(&mut self, id: SessionId, key: &[u8; 32]) {
        let saved = self
            .config
            .store
            .as_ref()
            .and_then(|st| st.db().load_inventory(key));
        // A player new to this world starts with the data's starting items (an axe) in a mode
        // that uses items (adventure); creative's palette needs none.
        let fresh = if self.uses_items() {
            Inventory::starting()
        } else {
            Inventory::default()
        };
        let s = self.sessions.get_mut(&id).expect("session");
        match saved {
            Some(bytes) => match Inventory::from_bytes(&bytes) {
                Some(i) => s.inventory = i,
                None => s.inventory_unread = true,
            },
            None => s.inventory = fresh,
        }
    }

    /// Everything a dying player holds falls where it died (in a mode that uses items); the
    /// wallet is kept.
    pub(super) fn drop_inventory(&mut self, id: SessionId, at: [f64; 3]) {
        if !self.uses_items() {
            return;
        }
        let s = self.sessions.get_mut(&id).expect("session");
        let stacks = s.inventory.take_all();
        if stacks.is_empty() {
            return;
        }
        s.inventory_dirty = true;
        // Spread a little around the death point, face-local, so they don't sit in one spot.
        let f = FaceFrame::at_height(at[1]);
        let local = f.point(at);
        for (i, st) in stacks.into_iter().enumerate() {
            let a = i as f64 * 2.399_963; // the golden angle
            let r = 0.15 * (i as f64).sqrt();
            let p = f.point([
                local[0] + r * a.cos(),
                local[1] + 0.25,
                local[2] + r * a.sin(),
            ]);
            self.loose.drop(st, p);
        }
    }

    pub(super) fn inventory_action(&mut self, id: SessionId, a: InventoryAction) {
        let s = self.sessions.get_mut(&id).expect("session");
        match a {
            InventoryAction::Hello => {
                s.inventory_hello = true;
                s.inventory_dirty = true;
                let all: Vec<_> = self.loose.items.iter().map(|it| it.net()).collect();
                for part in all.chunks(MAX_ITEM_DROPS_PER_MESSAGE) {
                    let m = Message::ItemDrops(ItemDrops {
                        items: part.to_vec(),
                        gone: Vec::new(),
                    });
                    self.send_reliable(id, &m, Channel::World);
                }
            }
            InventoryAction::Move { from, to, count } => {
                // Changed or not, the client is sent the truth (its drag may have guessed wrong).
                s.inventory
                    .move_items(usize::from(from), usize::from(to), count);
                s.inventory_dirty = true;
            }
            InventoryAction::Select { slot } => {
                if usize::from(slot) < inv::HOTBAR_SLOTS {
                    s.inventory.held = slot;
                }
            }
            InventoryAction::GroundHello => self.ground_hello(id),
            InventoryAction::Pick { cell } => {
                self.pick_ground(id, cell);
            }
            InventoryAction::Combine { recipe } => {
                // Made or not, the client is sent the truth.
                if let Some(r) = inv::recipe_in(inv::RECIPES, recipe) {
                    s.inventory.combine(r);
                }
                s.inventory_dirty = true;
            }
        }
    }

    /// One tick of the loose items: they fall, the living pick up the ones near them, the old go;
    /// then clients are told what changed.
    pub(super) fn step_items(&mut self) {
        if self.loose.items.is_empty()
            && self.loose.removed.is_empty()
            && !self.sessions.values().any(|s| s.inventory_dirty)
        {
            return;
        }
        let world = &self.world;
        let solid = |c: [i32; 3]| {
            world.find(chunk_of(c[0], c[1], c[2]))?;
            Some(info(world.get_voxel_if_loaded(c)).solid)
        };
        let moved = self.loose.step(&solid);
        self.loose_moved.extend(moved);
        self.pick_up();
        self.send_items();
    }

    /// Each takeable item goes to the nearest living player within reach that has room for some.
    fn pick_up(&mut self) {
        let radius = f64::from(self.config.player_config.body.radius);
        let bodies: Vec<(SessionId, DVec3, f64)> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.joined())
            .filter_map(|(&id, s)| s.handle.map(|h| (id, h)))
            .map(|(id, h)| {
                let half = (f64::from(self.players.half_height(h)) - radius).max(0.0);
                (id, self.players.position(&self.physics, h), half)
            })
            .collect();
        if bodies.is_empty() {
            return;
        }
        let near: Vec<(u32, Vec<(f64, SessionId)>)> = self
            .loose
            .items
            .iter()
            .filter(|it| it.takeable())
            .map(|it| {
                let q = DVec3::from(it.position);
                let mut by: Vec<(f64, SessionId)> = bodies
                    .iter()
                    .filter_map(|&(id, c, half)| {
                        let y = (q.y - c.y).clamp(-half, half);
                        let d = (q - (c + DVec3::new(0.0, y, 0.0))).length();
                        (d <= PICKUP_RADIUS_M).then_some((d, id))
                    })
                    .collect();
                by.sort_by(|a, b| a.0.total_cmp(&b.0));
                (it.id, by)
            })
            .filter(|(_, by)| !by.is_empty())
            .collect();
        for (item, by) in near {
            for (_, id) in by {
                let Some(it) = self.loose.get(item) else {
                    break;
                };
                let stack = it.stack.clone();
                let count = stack.count;
                let s = self.sessions.get_mut(&id).expect("session");
                // Wear and all (a dead player's worn tool).
                let left = s.inventory.add_stack(&stack);
                if left == count {
                    continue;
                }
                s.inventory_dirty = true;
                self.loose.set_count(item, left);
                if left == 0 {
                    break;
                }
            }
        }
    }

    fn send_items(&mut self) {
        let mut fresh = std::mem::take(&mut self.loose.fresh);
        let gone = std::mem::take(&mut self.loose.removed);
        if self.tick.is_multiple_of(MOVED_EVERY) {
            fresh.extend(self.loose_moved.drain());
        }
        fresh.sort_unstable();
        fresh.dedup();
        let items: Vec<_> = fresh
            .iter()
            .filter(|&&id| !gone.contains(&id))
            .filter_map(|&id| self.loose.get(id).map(|it| it.net()))
            .collect();
        for &id in &gone {
            self.loose_moved.remove(&id);
        }
        let hello: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.joined() && s.inventory_hello)
            .map(|(&id, _)| id)
            .collect();
        if !items.is_empty() || !gone.is_empty() {
            let mut msgs = Vec::new();
            for part in items.chunks(MAX_ITEM_DROPS_PER_MESSAGE) {
                msgs.push(ItemDrops {
                    items: part.to_vec(),
                    gone: Vec::new(),
                });
            }
            for part in gone.chunks(MAX_ITEM_DROPS_PER_MESSAGE) {
                msgs.push(ItemDrops {
                    items: Vec::new(),
                    gone: part.to_vec(),
                });
            }
            for m in msgs {
                let m = Message::ItemDrops(m);
                for &id in &hello {
                    self.send_reliable(id, &m, Channel::World);
                }
            }
        }
        // Every server of this build keeps ground items (ADR 0071).
        let flags = inventory_flags::GROUND
            | inventory_flags::CREATURES
            | inventory_flags::DAMAGE
            | inventory_flags::COMBINE
            | inventory_flags::CAUSES
            | inventory_flags::ECOLOGY
            | if self.uses_items() {
                inventory_flags::USES_ITEMS
            } else {
                0
            };
        for id in hello {
            let s = self.sessions.get_mut(&id).expect("session");
            if !s.inventory_dirty {
                continue;
            }
            s.inventory_dirty = false;
            let m = Message::Inventory(InventoryMessage {
                flags,
                inventory: s.inventory.clone(),
            });
            self.send_reliable(id, &m, Channel::Control);
        }
    }

    // --- for tests and tools ---

    /// A player's inventory.
    pub fn inventory_of(&self, player_id: u16) -> Option<&Inventory> {
        self.sessions
            .values()
            .find(|s| s.joined() && s.player_id == player_id)
            .map(|s| &s.inventory)
    }

    /// Gives a player items (tests, a future host command): what didn't fit.
    pub fn give(&mut self, player_id: u16, item: &str, count: u16) -> u16 {
        let Some(s) = self
            .sessions
            .values_mut()
            .find(|s| s.joined() && s.player_id == player_id)
        else {
            return count;
        };
        s.inventory_dirty = true;
        s.inventory.add(item, count)
    }

    /// The loose items lying in the world.
    pub fn loose_items(&self) -> &[crate::loose_items::LooseItem] {
        &self.loose.items
    }

    /// Drops a stack at a point (tests, tools).
    pub fn drop_item(&mut self, stack: Stack, at: [f64; 3]) -> u32 {
        self.loose.drop(stack, at)
    }
}

/// A block state's certain drops (item, count; docs/BLOCK_DAMAGE.md): a block without `drops`
/// drops itself, one.
fn certain_drops_of(was: StateId) -> Vec<(&'static str, u32)> {
    if was == M.air || info(was).liquid {
        return Vec::new();
    }
    let name = block_of(was).name;
    match inv::drops_of(name) {
        Some(d) => dwell_protocol::damage::certain_drops(d.drops),
        None => vec![(name, 1)],
    }
}
