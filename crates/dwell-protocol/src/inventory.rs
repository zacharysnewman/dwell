//! Inventories, items and block drops (docs/adr/0066-inventory-and-drops.md), as data
//! (`shared/inventory/inventory.json` and every block's `drops`, generated into `inventory.gen.rs` by
//! `shared/inventory/gen.mjs`). An item is a namespaced name: every block is an item of its own name,
//! and the data lists the items that are not blocks. Currency items (a `value`) go to the wallet, a
//! total apart from the slots, capped by the wallet's capacity. The server keeps a player's
//! [`Inventory`]; the client shows the copy the server sends.

use crate::bytes::{ByteReader, ByteWriter};

#[rustfmt::skip]
mod table {
    include!("inventory.gen.rs");
}

pub use table::{
    BACKPACK_SLOTS, DESPAWN_SECONDS, DROPS, HOTBAR_SLOTS, ITEMS, MAX_DROPS, PICKUP_DELAY_SECONDS,
    PICKUP_RADIUS_M, STACK_SIZE, STARTING_ITEMS, STARTING_WALLET, WALLETS,
};

/// Every slot: the hotbar's first, then the backpack's.
pub const SLOTS: usize = HOTBAR_SLOTS + BACKPACK_SLOTS;
/// The longest item name on the wire or in a save.
pub const MAX_ITEM_NAME: usize = 64;

/// An item that is not a block: its label and simple look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDef {
    pub name: &'static str,
    pub label: &'static str,
    pub color: [u8; 3],
    /// Metres across, as a loose item.
    pub size: f64,
    pub stack: u16,
    /// A currency item's worth in the wallet; 0 for an ordinary item.
    pub value: u16,
    /// Its one shaped look (`look`), drawn the same in the hand, on the hotbar, loose and lying on
    /// the ground (a ground item's look is its item's); none: a small block in `color`.
    pub look: Option<ItemLook>,
}

/// An item look's shape: a fixed vocabulary, sized by the data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookShape {
    /// A box, long along its x: a twig.
    Rod,
    /// A rounded lump (a squat, tapered hexagonal prism): a stone.
    Lump,
    /// A small heap of tiny lumps (several, in the look's footprint, the middle one on top):
    /// seeds.
    Cluster,
}

/// An item's shaped look (`inventory.json` `look`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemLook {
    pub shape: LookShape,
    /// Metres: x (its length), y (up), z.
    pub size: [f64; 3],
    /// The block whose side tile it is drawn with.
    pub texture: &'static str,
    /// sRGB tint on the texture.
    pub color: [u8; 3],
}

/// A wallet: how much currency it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalletDef {
    pub name: &'static str,
    pub label: &'static str,
    pub capacity: u16,
}

/// One outcome of a drop rule: `min..=max` of `item`, picked by `weight` among the rule's choices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropChoice {
    pub item: &'static str,
    pub min: u16,
    pub max: u16,
    pub weight: f64,
}

/// A drop rule: at `chance`, one of `choices` (by weight).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropRule {
    pub chance: f64,
    pub choices: &'static [DropChoice],
}

/// What a block drops, and the item placing it uses up (`cost`: a ramp's base).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockDrops {
    pub block: &'static str,
    pub cost: &'static str,
    pub drops: &'static [DropRule],
}

/// The non-block item of that name.
pub fn item(name: &str) -> Option<&'static ItemDef> {
    ITEMS.iter().find(|i| i.name == name)
}

/// The most of `name` one slot holds.
pub fn stack_of(name: &str) -> u16 {
    item(name).map_or(STACK_SIZE, |i| i.stack)
}

/// A currency item's worth (0: not currency).
pub fn value_of(name: &str) -> u16 {
    item(name).map_or(0, |i| i.value)
}

/// The wallet of that name.
pub fn wallet(name: &str) -> Option<&'static WalletDef> {
    WALLETS.iter().find(|w| w.name == name)
}

/// A block's drops and cost, by its namespaced name.
pub fn drops_of(block: &str) -> Option<&'static BlockDrops> {
    drops_in(DROPS, block)
}

/// [`drops_of`] over a table (the data's, or a test's fixture).
pub fn drops_in<'a>(table: &'a [BlockDrops], block: &str) -> Option<&'a BlockDrops> {
    table.iter().find(|d| d.block == block)
}

/// Rolls `rules` with `random` (uniform in [0, 1)): the items dropped, same items merged, in rule
/// order.
pub fn roll(rules: &[DropRule], random: &mut impl FnMut() -> f64) -> Vec<Stack> {
    let mut out: Vec<Stack> = Vec::new();
    for r in rules {
        if r.choices.is_empty() || random() >= r.chance {
            continue;
        }
        let total: f64 = r.choices.iter().map(|c| c.weight).sum();
        let mut at = random() * total;
        let c = r
            .choices
            .iter()
            .find(|c| {
                at -= c.weight;
                at < 0.0
            })
            .unwrap_or(&r.choices[r.choices.len() - 1]);
        let span = f64::from(c.max - c.min + 1);
        let count = c.min + ((random() * span) as u16).min(c.max - c.min);
        if count == 0 {
            continue;
        }
        match out.iter_mut().find(|s| s.item == c.item) {
            Some(s) => s.count += count,
            None => out.push(Stack::new(c.item, count)),
        }
    }
    out
}

/// Some of one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stack {
    pub item: String,
    pub count: u16,
}

impl Stack {
    pub fn new(item: &str, count: u16) -> Self {
        Self {
            item: item.to_owned(),
            count,
        }
    }
}

/// A player's items: `SLOTS` slots (the hotbar's first), the held hotbar slot, and the wallet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inventory {
    pub slots: Vec<Option<Stack>>,
    /// The held hotbar slot (`InventoryAction::Select`): placing takes from it first.
    pub held: u8,
    /// The wallet's kind (`WALLETS`) and the currency it holds.
    pub wallet: String,
    pub money: u16,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: vec![None; SLOTS],
            held: 0,
            wallet: STARTING_WALLET.to_owned(),
            money: 0,
        }
    }
}

impl Inventory {
    /// A new player's inventory: the data's starting items (`STARTING_ITEMS`; owner, 2026-11-08:
    /// "Adventure starts with an axe") in the first slots, the starting wallet empty.
    pub fn starting() -> Self {
        let mut inv = Self::default();
        for &(item, count) in STARTING_ITEMS {
            inv.add(item, count);
        }
        inv
    }

    /// The wallet's capacity (an unknown wallet: the starting one's).
    pub fn capacity(&self) -> u16 {
        wallet(&self.wallet)
            .or_else(|| wallet(STARTING_WALLET))
            .map_or(0, |w| w.capacity)
    }

    /// Adds `count` of `item`: currency to the wallet (whole items, up to its capacity), anything
    /// else onto stacks of it first, then empty slots, the hotbar first. Returns what didn't fit.
    pub fn add(&mut self, item: &str, count: u16) -> u16 {
        let value = value_of(item);
        if value > 0 {
            let room = (self.capacity().saturating_sub(self.money)) / value;
            let n = room.min(count);
            self.money += n * value;
            return count - n;
        }
        let max = stack_of(item);
        let mut left = count;
        for s in self.slots.iter_mut().flatten() {
            if left == 0 {
                break;
            }
            if s.item == item && s.count < max {
                let n = (max - s.count).min(left);
                s.count += n;
                left -= n;
            }
        }
        for slot in self.slots.iter_mut() {
            if left == 0 {
                break;
            }
            if slot.is_none() {
                let n = max.min(left);
                *slot = Some(Stack::new(item, n));
                left -= n;
            }
        }
        left
    }

    /// How many of `item` it holds in its slots.
    pub fn count_of(&self, item: &str) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.item == item)
            .map(|s| u32::from(s.count))
            .sum()
    }

    /// Takes one `item`: from the held slot if it holds it, otherwise the first slot that does.
    pub fn take_one(&mut self, item: &str) -> bool {
        let held = usize::from(self.held);
        let at = (self.slots.get(held).and_then(Option::as_ref))
            .filter(|s| s.item == item)
            .map(|_| held)
            .or_else(|| {
                self.slots
                    .iter()
                    .position(|s| s.as_ref().is_some_and(|s| s.item == item))
            });
        let Some(at) = at else {
            return false;
        };
        let slot = &mut self.slots[at];
        let s = slot.as_mut().expect("a stack");
        s.count -= 1;
        if s.count == 0 {
            *slot = None;
        }
        true
    }

    /// Moves `count` (0: all) from slot `from` to slot `to`: onto an empty slot or the same item it
    /// merges (as much as fits), onto another item the whole stacks swap. False if it changes
    /// nothing or names no slot.
    pub fn move_items(&mut self, from: usize, to: usize, count: u16) -> bool {
        if from == to || from >= self.slots.len() || to >= self.slots.len() {
            return false;
        }
        let Some(src) = self.slots[from].clone() else {
            return false;
        };
        let n = if count == 0 {
            src.count
        } else {
            count.min(src.count)
        };
        match self.slots[to].clone() {
            None => {
                self.slots[to] = Some(Stack::new(&src.item, n));
            }
            Some(dst) if dst.item == src.item => {
                let room = stack_of(&src.item).saturating_sub(dst.count);
                let n = n.min(room);
                if n == 0 {
                    return false;
                }
                self.slots[to].as_mut().expect("a stack").count += n;
                return self.take_from(from, n);
            }
            Some(dst) => {
                if n < src.count {
                    return false; // a part onto another item: nowhere for it
                }
                self.slots[to] = Some(src);
                self.slots[from] = Some(dst);
                return true;
            }
        }
        self.take_from(from, n)
    }

    fn take_from(&mut self, at: usize, n: u16) -> bool {
        let slot = &mut self.slots[at];
        let s = slot.as_mut().expect("a stack");
        s.count -= n;
        if s.count == 0 {
            *slot = None;
        }
        true
    }

    /// Empties every slot (death), returning what they held. The wallet is kept (ADR 0066 Q7).
    pub fn take_all(&mut self) -> Vec<Stack> {
        self.slots.iter_mut().filter_map(Option::take).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none) && self.money == 0
    }

    /// The record (saved and sent): version 1, held slot, wallet name and money, then each
    /// non-empty slot's index, item name and count.
    pub fn write(&self, w: &mut ByteWriter) {
        w.u8(1);
        w.u8(self.held);
        w.str(crate::bytes::clamp_str(&self.wallet, MAX_ITEM_NAME));
        w.u16(self.money);
        let full: Vec<(usize, &Stack)> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
            .collect();
        w.u8(full.len() as u8);
        for (i, s) in full {
            w.u8(i as u8);
            w.str(crate::bytes::clamp_str(&s.item, MAX_ITEM_NAME));
            w.u16(s.count);
        }
    }

    /// [`Inventory::write`] read back; `None` if malformed. Item names this build doesn't know are
    /// kept (a newer build's items); slots past this build's count are left out.
    pub fn read(r: &mut ByteReader) -> Option<Inventory> {
        let version = r.u8();
        r.check(version == 1);
        let mut inv = Inventory {
            held: r.u8(),
            wallet: r.str(MAX_ITEM_NAME),
            money: r.u16(),
            ..Inventory::default()
        };
        let n = r.u8();
        for _ in 0..n {
            let (i, item, count) = (usize::from(r.u8()), r.str(MAX_ITEM_NAME), r.u16());
            if r.ok() && i < SLOTS && count > 0 && !item.is_empty() {
                inv.slots[i] = Some(Stack { item, count });
            }
        }
        if usize::from(inv.held) >= HOTBAR_SLOTS {
            inv.held = 0;
        }
        r.ok().then_some(inv)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut ByteWriter::new(&mut out));
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Inventory> {
        let mut r = ByteReader::new(bytes);
        let inv = Inventory::read(&mut r)?;
        r.at_end().then_some(inv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_has_the_owners_numbers_and_rules() {
        assert_eq!((HOTBAR_SLOTS, BACKPACK_SLOTS, STACK_SIZE), (9, 27, 64));
        assert_eq!(DESPAWN_SECONDS, 300.0);
        assert_eq!(wallet(STARTING_WALLET).unwrap().capacity, 99);
        // Grass drops dirt and (sometimes) rocks; dirt drops dirt; leaves drop sticks.
        let items = |b: &str| -> Vec<&str> {
            drops_of(b)
                .unwrap()
                .drops
                .iter()
                .flat_map(|r| r.choices.iter().map(|c| c.item))
                .collect()
        };
        assert_eq!(items("dwell:grass"), ["dwell:dirt", "dwell:rock"]);
        assert_eq!(items("dwell:dirt"), ["dwell:dirt"]);
        assert_eq!(items("dwell:leaves"), ["dwell:stick"]);
        assert_eq!(items("dwell:stone"), ["dwell:stone"]);
        assert_eq!(
            items("dwell:tall_grass"),
            ["dwell:grass_seed", "dwell:gem", "dwell:gem_large"]
        );
        // A ramp drops what its base drops and placing it costs its base.
        assert_eq!(items("dwell:grass_ramp"), items("dwell:grass"));
        assert_eq!(drops_of("dwell:stone_ramp").unwrap().cost, "dwell:stone");
        assert!(drops_of("dwell:water").unwrap().drops.is_empty());
        assert!(drops_of("dwell:bedrock").unwrap().drops.is_empty());
        assert!(drops_of("dwell:torch_upper").unwrap().drops.is_empty());
    }

    /// A small deterministic generator (xorshift) for the rolls.
    fn rng(seed: u64) -> impl FnMut() -> f64 {
        let mut x = seed;
        move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    #[test]
    fn tall_grass_drops_seeds_half_the_time_and_gems_an_eighth_mostly_small() {
        let rules = drops_of("dwell:tall_grass").unwrap().drops;
        let mut random = rng(0x5eed);
        let n = 40_000;
        let (mut seeds, mut gems, mut large) = (0, 0, 0);
        for _ in 0..n {
            for s in roll(rules, &mut random) {
                match s.item.as_str() {
                    "dwell:grass_seed" => seeds += s.count,
                    "dwell:gem" => gems += s.count,
                    "dwell:gem_large" => large += s.count,
                    other => panic!("{other}"),
                }
            }
        }
        let frac = |k: u16| f64::from(k) / f64::from(n);
        assert!((frac(seeds) - 0.5).abs() < 0.01, "seeds {}", frac(seeds));
        assert!(
            (frac(gems + large) - 0.125).abs() < 0.006,
            "gems {}",
            frac(gems + large)
        );
        let share = f64::from(large) / f64::from(gems + large);
        assert!((share - 0.05).abs() < 0.015, "large share {share}");
    }

    #[test]
    fn a_fixture_drop_table_works_by_data_alone() {
        static FIXTURE: &[BlockDrops] = &[BlockDrops {
            block: "test:ore",
            cost: "test:ore",
            drops: &[DropRule {
                chance: 1.0,
                choices: &[DropChoice {
                    item: "dwell:rock",
                    min: 2,
                    max: 4,
                    weight: 1.0,
                }],
            }],
        }];
        let rules = drops_in(FIXTURE, "test:ore").unwrap().drops;
        let mut random = rng(9);
        for _ in 0..200 {
            let got = roll(rules, &mut random);
            assert_eq!(got.len(), 1);
            assert!((2..=4).contains(&got[0].count), "{got:?}");
        }
    }

    #[test]
    fn adding_fills_stacks_then_empty_slots_and_leaves_what_does_not_fit() {
        let mut inv = Inventory::default();
        assert_eq!(inv.add("dwell:dirt", 70), 0);
        assert_eq!(inv.slots[0], Some(Stack::new("dwell:dirt", 64)));
        assert_eq!(inv.slots[1], Some(Stack::new("dwell:dirt", 6)));
        assert_eq!(inv.add("dwell:stone", 1), 0);
        assert_eq!(inv.slots[2], Some(Stack::new("dwell:stone", 1)));
        assert_eq!(inv.add("dwell:dirt", 10), 0);
        assert_eq!(inv.slots[1], Some(Stack::new("dwell:dirt", 16)));
        // Full: every slot a full stack of something else.
        let mut full = Inventory::default();
        for s in full.slots.iter_mut() {
            *s = Some(Stack::new("dwell:sand", 64));
        }
        assert_eq!(full.add("dwell:dirt", 5), 5);
        full.slots[30] = Some(Stack::new("dwell:dirt", 62));
        assert_eq!(full.add("dwell:dirt", 5), 3);
    }

    #[test]
    fn the_wallet_is_apart_from_the_slots_and_capped() {
        let mut inv = Inventory::default();
        assert_eq!(inv.add("dwell:gem", 3), 0);
        assert_eq!(inv.add("dwell:gem_large", 2), 0);
        assert_eq!(inv.money, 13);
        assert!(inv.slots.iter().all(Option::is_none), "no slot used");
        // Up to 99: whole gems only, the rest left.
        assert_eq!(inv.add("dwell:gem_large", 20), 3); // 17 fit: 13 + 85 = 98
        assert_eq!(inv.money, 98);
        assert_eq!(inv.add("dwell:gem_large", 1), 1);
        assert_eq!(inv.add("dwell:gem", 4), 3);
        assert_eq!(inv.money, 99);
        // A full inventory still takes currency.
        let mut full = Inventory::default();
        for s in full.slots.iter_mut() {
            *s = Some(Stack::new("dwell:sand", 64));
        }
        assert_eq!(full.add("dwell:gem", 1), 0);
        assert_eq!(full.take_all().len(), SLOTS);
        assert_eq!(full.money, 1, "death keeps the wallet");
    }

    #[test]
    fn taking_prefers_the_held_slot_and_moves_merge_or_swap() {
        let mut inv = Inventory::default();
        inv.slots[0] = Some(Stack::new("dwell:dirt", 2));
        inv.slots[4] = Some(Stack::new("dwell:dirt", 1));
        inv.held = 4;
        assert!(inv.take_one("dwell:dirt"));
        assert_eq!(inv.slots[4], None);
        assert!(inv.take_one("dwell:dirt"));
        assert_eq!(inv.slots[0], Some(Stack::new("dwell:dirt", 1)));
        assert!(!inv.take_one("dwell:stone"));
        inv.slots[1] = Some(Stack::new("dwell:stone", 5));
        assert!(inv.move_items(1, 20, 2)); // a part onto an empty slot
        assert_eq!(inv.slots[20], Some(Stack::new("dwell:stone", 2)));
        assert!(inv.move_items(20, 1, 0)); // merged back
        assert_eq!(inv.slots[1], Some(Stack::new("dwell:stone", 5)));
        assert!(inv.move_items(0, 1, 0)); // swapped
        assert_eq!(inv.slots[0], Some(Stack::new("dwell:stone", 5)));
        assert_eq!(inv.slots[1], Some(Stack::new("dwell:dirt", 1)));
        assert!(!inv.move_items(0, 99, 0));
    }

    #[test]
    fn the_record_round_trips_and_keeps_unknown_items() {
        let mut inv = Inventory::default();
        inv.add("dwell:dirt", 70);
        inv.slots[35] = Some(Stack::new("mod:future_thing", 3));
        inv.held = 2;
        inv.money = 42;
        let b = inv.to_bytes();
        assert_eq!(Inventory::from_bytes(&b), Some(inv));
        assert_eq!(Inventory::from_bytes(&b[..b.len() - 1]), None);
    }

    #[test]
    fn the_new_messages_round_trip_and_reject_bad_ones() {
        use crate::{InventoryAction, InventoryMessage, ItemDropNet, ItemDrops, Message};
        let mut inventory = Inventory::default();
        inventory.add("dwell:rock", 3);
        let all = [
            Message::InventoryAction(InventoryAction::Hello),
            Message::InventoryAction(InventoryAction::Move {
                from: 1,
                to: 30,
                count: 0,
            }),
            Message::InventoryAction(InventoryAction::Select { slot: 4 }),
            Message::Inventory(InventoryMessage {
                flags: crate::inventory_flags::USES_ITEMS,
                inventory,
            }),
            Message::ItemDrops(ItemDrops {
                items: vec![ItemDropNet {
                    id: 7,
                    item: "dwell:dirt".into(),
                    count: 2,
                    position: [1.5, 64.25, -3.0],
                }],
                gone: vec![3, 4],
            }),
            // Ground items (ADR 0071).
            Message::InventoryAction(InventoryAction::GroundHello),
            Message::InventoryAction(InventoryAction::Pick {
                cell: [-3, 520, 17],
            }),
            Message::GroundGone(vec![[1, 2, 3], [-4, 600, 9]]),
        ];
        for m in all {
            let b = m.encode();
            assert_eq!(Message::decode(&b), Some(m.clone()));
            assert_eq!(Message::decode(&b[..b.len() - 1]), None, "{m:?}");
        }
        let mut bad = Message::InventoryAction(InventoryAction::Hello).encode();
        *bad.last_mut().unwrap() = 9;
        assert_eq!(Message::decode(&bad), None);
    }
    #[test]
    fn a_new_player_starts_with_the_datas_items_an_axe() {
        // Owner (2026-11-08): "Adventure starts with an axe."
        let inv = Inventory::starting();
        assert_eq!(inv.count_of("dwell:stone_axe"), 1);
        assert_eq!(
            inv.slots[0].as_ref().map(|s| s.item.as_str()),
            Some("dwell:stone_axe")
        );
        assert_eq!(inv.money, 0);
    }
}
