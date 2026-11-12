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
    PICKUP_RADIUS_M, RECIPES, STACK_SIZE, STARTING_ITEMS, STARTING_WALLET, WALLETS,
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
    /// An angular flake (an uneven pyramid on a five-sided base, its tip off-centre): a rock shard
    /// knocked off by a pickaxe's hit (BLOCK_DAMAGE.md §6.2), the piece the carved facet left.
    Shard,
    /// A prop's own mesh (`look: { model }`, a prop of shared/models/props.json; ADR 0070 §8):
    /// its triangles and palette colours, untextured.
    Model,
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
    /// The prop a [`LookShape::Model`] look draws (`dwell_models::prop`); empty for the others.
    pub model: &'static str,
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

/// A recipe (combining, owner 2026-11-11): its `inputs` (item, count) make its `output`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recipe {
    pub name: &'static str,
    pub inputs: &'static [(&'static str, u16)],
    pub output: (&'static str, u16),
}

/// A recipe's id on the wire (`InventoryAction::Combine`): its name's FNV-1a hash, the same in
/// every build that has the recipe, whatever order the data lists them in.
pub fn recipe_id(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// The recipe of that id in `table` (the data's, or a test's fixture).
pub fn recipe_in(table: &[Recipe], id: u32) -> Option<&Recipe> {
    table.iter().find(|r| recipe_id(r.name) == id)
}

/// The request to make recipe `name` once, if the server (its `Inventory`'s `flags`) takes it:
/// `None` for an older server without `inventory_flags::COMBINE` (it would read the action as
/// malformed) or a recipe this build doesn't have.
pub fn combine_request(server_flags: u8, name: &str) -> Option<crate::messages::InventoryAction> {
    (server_flags & crate::messages::inventory_flags::COMBINE != 0
        && RECIPES.iter().any(|r| r.name == name))
    .then(|| crate::messages::InventoryAction::Combine {
        recipe: recipe_id(name),
    })
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
    /// The hits a wearing tool has used (`chopping::durability`; 0: unworn).
    pub wear: u16,
}

impl Stack {
    pub fn new(item: &str, count: u16) -> Self {
        Self {
            item: item.to_owned(),
            count,
            wear: 0,
        }
    }
}

/// Wear slot `index`'s base in the record: a slot's wear is written as an entry at `WEAR_AT + slot`
/// (no item, the wear as its count) after the stacks. A build without wear skips it, as it skips
/// any entry past its slot count, so the record stays version 1 and readable on the whole line.
pub const WEAR_AT: usize = 128;

/// What a tool's hit did to it (`Inventory::wear_held`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wear {
    /// The held item doesn't wear (or nothing is held).
    None,
    /// It wore by one hit; this many hits are left.
    Worn(u16),
    /// That was its last hit: it broke and is gone from its slot.
    Broke,
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
        self.add_worn(item, count, 0)
    }

    /// Adds a stack with its wear kept (a loose tool picked up): [`Inventory::add`]'s rule, merging
    /// only onto stacks with the same wear. Returns what didn't fit.
    pub fn add_stack(&mut self, stack: &Stack) -> u16 {
        self.add_worn(&stack.item, stack.count, stack.wear)
    }

    fn add_worn(&mut self, item: &str, count: u16, wear: u16) -> u16 {
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
            // Stacking never merges different wear (a worn tool onto a fresh one).
            if s.item == item && s.wear == wear && s.count < max {
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
                *slot = Some(Stack {
                    wear,
                    ..Stack::new(item, n)
                });
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

    /// How many times `recipe` can be made from what it holds (each input's count over its need).
    pub fn can_make(&self, recipe: &Recipe) -> u32 {
        recipe
            .inputs
            .iter()
            .map(|&(item, n)| self.count_of(item) / u32::from(n.max(1)))
            .min()
            .unwrap_or(0)
    }

    /// Makes `recipe` once: takes its inputs (from anywhere, the backpack's last slots first so the
    /// hotbar keeps its stacks) and adds its output. False, changing nothing, if an input is short or
    /// the output has no room (counting the slots the inputs free).
    pub fn combine(&mut self, recipe: &Recipe) -> bool {
        if self.can_make(recipe) == 0 {
            return false;
        }
        let before = self.clone();
        for &(item, n) in recipe.inputs {
            let mut left = n;
            for slot in self.slots.iter_mut().rev() {
                if left == 0 {
                    break;
                }
                if let Some(s) = slot.as_mut().filter(|s| s.item == item) {
                    let k = s.count.min(left);
                    s.count -= k;
                    left -= k;
                    if s.count == 0 {
                        *slot = None;
                    }
                }
            }
        }
        let (item, count) = recipe.output;
        if self.add(item, count) > 0 {
            *self = before;
            return false;
        }
        true
    }

    /// One hit with the held item (owner, 2026-11-11: tools wear): an item `durability` (by name)
    /// gives hits wears by one, and breaks (its slot emptied) on its last.
    pub fn wear_held(&mut self, durability: impl Fn(&str) -> Option<u16>) -> Wear {
        let Some(slot) = self.slots.get_mut(usize::from(self.held)) else {
            return Wear::None;
        };
        let Some(s) = slot.as_mut() else {
            return Wear::None;
        };
        let Some(hits) = durability(&s.item) else {
            return Wear::None;
        };
        s.wear = s.wear.saturating_add(1);
        if s.wear >= hits {
            *slot = None;
            return Wear::Broke;
        }
        Wear::Worn(hits - s.wear)
    }

    /// Whether going from `self` to `next` broke a tool from wear (the client's break sound): an
    /// item `durability` gives hits had one hit left in a slot and `next` holds fewer of it.
    pub fn tool_broke(&self, next: &Inventory, durability: impl Fn(&str) -> Option<u16>) -> bool {
        self.slots.iter().flatten().any(|s| {
            durability(&s.item).is_some_and(|hits| s.wear + 1 >= hits)
                && next.count_of(&s.item) < self.count_of(&s.item)
        })
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
            None if n == src.count => {
                // The whole stack, wear and all.
                self.slots[to] = Some(src);
                self.slots[from] = None;
                return true;
            }
            None => {
                self.slots[to] = Some(Stack {
                    count: n,
                    ..src.clone()
                });
            }
            Some(dst) if dst.item == src.item && dst.wear == src.wear => {
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
        let worn: Vec<(usize, u16)> = full
            .iter()
            .filter(|(_, s)| s.wear > 0)
            .map(|&(i, s)| (i, s.wear))
            .collect();
        w.u8((full.len() + worn.len()) as u8);
        for &(i, s) in &full {
            w.u8(i as u8);
            w.str(crate::bytes::clamp_str(&s.item, MAX_ITEM_NAME));
            w.u16(s.count);
        }
        // Wear after the stacks, at `WEAR_AT + slot` with no item: an older build skips them.
        for (i, wear) in worn {
            w.u8((WEAR_AT + i) as u8);
            w.str("");
            w.u16(wear);
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
                inv.slots[i] = Some(Stack {
                    item,
                    count,
                    wear: 0,
                });
            } else if r.ok()
                && (WEAR_AT..WEAR_AT + SLOTS).contains(&i)
                && item.is_empty()
                && let Some(s) = inv.slots[i - WEAR_AT].as_mut()
            {
                s.wear = count;
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

    // --- Pieces, combining and wear (owner, 2026-11-11) ---

    /// The certain drops of a block, as `damage` splits them (item, least count).
    fn certain(block: &str) -> Vec<(&'static str, u16)> {
        drops_of(block)
            .unwrap()
            .drops
            .iter()
            .filter(|r| r.chance >= 1.0 && r.choices.len() == 1)
            .map(|r| (r.choices[0].item, r.choices[0].min))
            .collect()
    }

    #[test]
    fn a_half_metre_cell_breaks_into_eight_pieces_of_its_material() {
        for (block, piece) in [
            ("dwell:stone", "dwell:rock_shard"),
            ("dwell:stone_ramp", "dwell:rock_shard"),
            ("dwell:dirt", "dwell:dirt_clump"),
            ("dwell:dirt_ramp", "dwell:dirt_clump"),
            ("dwell:grass", "dwell:dirt_clump"),
            ("dwell:grass_ramp", "dwell:dirt_clump"),
            ("dwell:snowy_grass", "dwell:dirt_clump"),
            ("dwell:snowy_grass_ramp", "dwell:dirt_clump"),
            ("dwell:log", "dwell:wood"),
        ] {
            assert_eq!(certain(block), vec![(piece, 8)], "{block}");
            // Half over the hits, half on the destroy, totalling the 8 (stone pickaxe: 5 hits).
            let hits: u32 = (1..5).map(|k| crate::damage::hit_share(8, 5, k)).sum();
            assert_eq!((hits, crate::damage::destroy_share(8, 5)), (4, 4));
        }
        // "We still want rocks to be found in grass and dirt at random"; sticks from leaves.
        for block in ["dwell:grass", "dwell:dirt", "dwell:snowy_grass"] {
            let rules = drops_of(block).unwrap().drops;
            assert!(
                rules
                    .iter()
                    .any(|r| r.chance < 1.0 && r.choices[0].item == "dwell:rock"),
                "{block}"
            );
        }
        let leaves = drops_of("dwell:leaves").unwrap().drops;
        assert_eq!(leaves[0].choices[0].item, "dwell:stick");
        // Tall grass gives a grass fiber every time (the stone tools' last input).
        assert!(certain("dwell:tall_grass").contains(&("dwell:grass_fiber", 1)));
        assert_eq!(item("dwell:wood").unwrap().label, "Wood chunk");
    }

    static FIXTURE: &[Recipe] = &[Recipe {
        name: "test:pebble_pile",
        inputs: &[("dwell:rock", 3), ("dwell:stick", 1)],
        output: ("dwell:stone", 2),
    }];

    #[test]
    fn a_fixture_recipe_by_data_alone_takes_its_inputs_and_adds_its_output() {
        let r = recipe_in(FIXTURE, recipe_id("test:pebble_pile")).expect("found by its id");
        let mut inv = Inventory::default();
        inv.add("dwell:rock", 5);
        inv.add("dwell:stick", 1);
        assert_eq!(inv.can_make(r), 1);
        assert!(inv.combine(r));
        assert_eq!(inv.count_of("dwell:rock"), 2);
        assert_eq!(inv.count_of("dwell:stick"), 0);
        assert_eq!(inv.count_of("dwell:stone"), 2);
        // Refused without its inputs, changing nothing.
        let before = inv.clone();
        assert!(!inv.combine(r));
        assert_eq!(inv, before);
        assert!(recipe_in(FIXTURE, 1).is_none());
    }

    #[test]
    fn the_stone_tools_take_two_sticks_two_shards_and_two_fibers() {
        for tool in ["dwell:stone_axe", "dwell:stone_pickaxe"] {
            let r = RECIPES.iter().find(|r| r.name == tool).expect(tool);
            assert_eq!(
                r.inputs,
                &[
                    ("dwell:stick", 2),
                    ("dwell:rock_shard", 2),
                    ("dwell:grass_fiber", 2)
                ]
            );
            assert_eq!(r.output, (tool, 1));
            let mut inv = Inventory::default();
            inv.add("dwell:stick", 2);
            inv.add("dwell:rock_shard", 2);
            assert!(!inv.combine(r), "{tool}: refused without the fibers");
            inv.add("dwell:grass_fiber", 2);
            assert!(inv.combine(r));
            assert_eq!(inv.count_of(tool), 1);
            assert_eq!(inv.slots.iter().flatten().count(), 1, "the inputs used up");
        }
        for (name, piece, block) in [
            ("dwell:stone_slab", "dwell:rock_shard", "dwell:stone"),
            ("dwell:dirt_slab", "dwell:dirt_clump", "dwell:dirt"),
            ("dwell:log_slab", "dwell:wood", "dwell:log"),
        ] {
            let r = RECIPES.iter().find(|r| r.name == name).expect(name);
            assert_eq!((r.inputs, r.output), (&[(piece, 8)][..], (block, 1)));
        }
    }

    #[test]
    fn a_combine_request_goes_only_to_a_server_that_combines() {
        use crate::messages::{InventoryAction, inventory_flags};
        let older = inventory_flags::GROUND | inventory_flags::DAMAGE | inventory_flags::USES_ITEMS;
        assert_eq!(combine_request(older, "dwell:stone_axe"), None);
        assert_eq!(
            combine_request(older | inventory_flags::COMBINE, "dwell:stone_axe"),
            Some(InventoryAction::Combine {
                recipe: recipe_id("dwell:stone_axe")
            })
        );
        assert_eq!(combine_request(inventory_flags::COMBINE, "test:none"), None);
    }

    #[test]
    fn the_stone_axe_wears_by_one_a_hit_and_breaks_after_its_32nd() {
        let hits = |item: &str| crate::chopping::durability(item);
        assert_eq!(hits("dwell:stone_axe"), Some(32));
        // Owner, 2026-11-12: metal tools "have double durability".
        assert_eq!(hits("dwell:metal_axe"), Some(64), "double the stone axe's");
        let mut inv = Inventory::starting();
        assert_eq!(inv.slots[0].as_ref().unwrap().item, "dwell:stone_axe");
        for n in 1..32 {
            assert_eq!(inv.wear_held(hits), Wear::Worn(32 - n));
        }
        let before = inv.clone();
        assert_eq!(inv.wear_held(hits), Wear::Broke);
        assert!(inv.slots[0].is_none(), "gone");
        assert!(before.tool_broke(&inv, hits), "the client's break sound");
        assert_eq!(inv.wear_held(hits), Wear::None, "nothing held");
        // An item that doesn't wear.
        inv.add("dwell:rock", 1);
        assert_eq!(inv.wear_held(hits), Wear::None);
    }

    #[test]
    fn the_stone_pickaxe_and_the_wooden_shovel_wear_like_the_stone_axe() {
        // Owner, 2026-11-11: "stone pick wears too like axe, same with shovel"; 2026-11-12: metal
        // tools "have double durability".
        let hits = |item: &str| crate::chopping::durability(item);
        for item in ["dwell:stone_pickaxe", "dwell:wooden_shovel"] {
            assert_eq!(hits(item), Some(32), "{item}");
            let mut inv = Inventory::starting();
            inv.slots[0] = None;
            inv.add(item, 1);
            assert_eq!(inv.slots[0].as_ref().unwrap().item, item);
            for n in 1..32 {
                assert_eq!(inv.wear_held(hits), Wear::Worn(32 - n), "{item}");
            }
            assert_eq!(inv.wear_held(hits), Wear::Broke, "{item}");
            assert!(inv.slots[0].is_none(), "{item}: gone");
        }
        for item in [
            "dwell:metal_axe",
            "dwell:metal_pickaxe",
            "dwell:metal_shovel",
        ] {
            assert_eq!(hits(item), Some(64), "{item}: double");
        }
    }

    #[test]
    fn wear_round_trips_and_an_older_build_reads_the_record_without_it() {
        let mut inv = Inventory::starting();
        inv.add("dwell:rock", 3);
        inv.wear_held(crate::chopping::durability);
        inv.wear_held(crate::chopping::durability);
        // Moving the axe keeps its wear.
        assert!(inv.move_items(0, 10, 0));
        assert_eq!(inv.slots[10].as_ref().unwrap().wear, 2);
        let bytes = inv.to_bytes();
        assert_eq!(Inventory::from_bytes(&bytes), Some(inv.clone()));
        // An older build's reader (before wear): it skips entries past its slot count, so it reads
        // the same stacks unworn and the record is still version 1.
        let mut r = ByteReader::new(&bytes);
        assert_eq!(r.u8(), 1);
        let (_, _, _) = (r.u8(), r.str(MAX_ITEM_NAME), r.u16());
        let n = r.u8();
        let mut stacks = 0;
        for _ in 0..n {
            let (i, item, count) = (usize::from(r.u8()), r.str(MAX_ITEM_NAME), r.u16());
            if i < SLOTS && count > 0 && !item.is_empty() {
                stacks += 1;
            }
        }
        assert!(r.ok() && r.at_end());
        assert_eq!(stacks, 2, "the axe and the rocks, the wear entry skipped");
    }

    #[test]
    fn a_stack_added_keeps_its_wear_and_never_merges_with_different_wear() {
        let worn = |item: &str, count, wear| Stack {
            wear,
            ..Stack::new(item, count)
        };
        let mut inv = Inventory::default();
        // A worn axe picked up keeps its hits.
        assert_eq!(inv.add_stack(&worn("dwell:stone_axe", 1, 11)), 0);
        assert_eq!(inv.slots[0], Some(worn("dwell:stone_axe", 1, 11)));
        // A stackable item (to see the rule, not its data): different wear goes to its own slot,
        // the same wear merges, and an unworn add goes to the unworn stack.
        inv.add_stack(&worn("dwell:rock", 2, 3));
        inv.add("dwell:rock", 4);
        inv.add_stack(&worn("dwell:rock", 1, 3));
        assert_eq!(inv.slots[1], Some(worn("dwell:rock", 3, 3)));
        assert_eq!(inv.slots[2], Some(Stack::new("dwell:rock", 4)));
        // A move onto different wear swaps rather than merging; a part moved keeps its wear.
        assert!(inv.move_items(2, 1, 0));
        assert_eq!(inv.slots[1], Some(Stack::new("dwell:rock", 4)));
        assert_eq!(inv.slots[2], Some(worn("dwell:rock", 3, 3)));
        assert!(
            !inv.move_items(2, 1, 1),
            "a part onto different wear: nowhere"
        );
        assert!(inv.move_items(2, 5, 1));
        assert_eq!(inv.slots[5], Some(worn("dwell:rock", 1, 3)));
    }

    /// Owner, 2026-11-10: the hotbar went from 9 slots to 6 and the backpack from 27 to 30, so
    /// every slot keeps its index (36 in all). A record saved with 9 hotbar slots reads back with
    /// old slots 7–9 (indices 6–8) as the backpack's first three: nothing lost, the record as it
    /// was; a held slot past the new hotbar is the first.
    #[test]
    fn a_nine_slot_hotbar_save_loads_with_slots_7_to_9_in_the_backpack() {
        assert_eq!(
            SLOTS, 36,
            "the slot indices of a saved record stay where they were"
        );
        // The old build's record: version 1, held 8, the pouch, 3 gems, four stacks.
        let mut old = Vec::new();
        let mut w = ByteWriter::new(&mut old);
        w.u8(1);
        w.u8(8);
        w.str("dwell:pouch");
        w.u16(3);
        w.u8(4);
        for (i, item, n) in [
            (0u8, "dwell:stone_axe", 1u16),
            (6, "dwell:dirt", 12),
            (7, "dwell:stick", 5),
            (8, "dwell:rock", 64),
        ] {
            w.u8(i);
            w.str(item);
            w.u16(n);
        }
        let inv = Inventory::from_bytes(&old).unwrap();
        assert_eq!(inv.held, 0);
        assert_eq!(inv.money, 3);
        let at = |i: usize| inv.slots[i].as_ref().map(|s| (s.item.as_str(), s.count));
        assert_eq!(at(0), Some(("dwell:stone_axe", 1)));
        for (i, want) in [
            (6, ("dwell:dirt", 12)),
            (7, ("dwell:stick", 5)),
            (8, ("dwell:rock", 64)),
        ] {
            assert!(i >= HOTBAR_SLOTS, "slot {i} is the backpack's now");
            assert_eq!(at(i), Some(want));
        }
    }

    #[test]
    fn the_data_has_the_owners_numbers_and_rules() {
        assert_eq!((HOTBAR_SLOTS, BACKPACK_SLOTS, STACK_SIZE), (6, 30, 64));
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
        // Owner, 2026-11-11: a cell breaks into 8 pieces of its material; rocks still at random.
        assert_eq!(items("dwell:grass"), ["dwell:dirt_clump", "dwell:rock"]);
        assert_eq!(items("dwell:dirt"), ["dwell:dirt_clump", "dwell:rock"]);
        assert_eq!(items("dwell:leaves"), ["dwell:stick"]);
        assert_eq!(items("dwell:stone"), ["dwell:rock_shard"]);
        assert_eq!(
            items("dwell:tall_grass"),
            [
                "dwell:grass_fiber",
                "dwell:grass_seed",
                "dwell:gem",
                "dwell:gem_large"
            ]
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
        let (mut seeds, mut gems, mut large, mut fibers) = (0, 0, 0, 0);
        for _ in 0..n {
            for s in roll(rules, &mut random) {
                match s.item.as_str() {
                    "dwell:grass_seed" => seeds += s.count,
                    "dwell:gem" => gems += s.count,
                    "dwell:gem_large" => large += s.count,
                    "dwell:grass_fiber" => fibers += s.count,
                    other => panic!("{other}"),
                }
            }
        }
        assert_eq!(fibers, n, "a grass fiber every time");
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
