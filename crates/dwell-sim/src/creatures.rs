//! Creatures (docs/adr/0073-creatures.md): animals as light server entities, not Rapier bodies.
//! They are spawned in groups near players by the data's rules (shared/creatures/creatures.json,
//! rolled once per chunk column and face from the world seed), wander (idle, then walk to a random
//! nearby point on walkable ground, stopping at water and cliffs), and are removed once no player
//! is within `despawn_m`. Everything is face-local through `FaceFrame`: face B mirrors face A.
//!
//! NPCs (WORK_QUEUE item 38, CHARACTERS.md §11.10) are creatures with an `npc` block: they idle in a
//! pose picked by weight (stand, look around, sit), turn to face a player within `look_at_m`, step
//! aside when a player or another NPC comes within `bump_m`, and carry a palette variant seed the
//! client picks their colours from; their rules may keep groups apart (`spacing_m`) and to biomes.

use std::collections::{HashMap, HashSet};

use dwell_blocks::{StateId, VoxelShape, find_block, try_info};
use dwell_protocol::{CreatureNet, NPC_POSES, NpcNet, SIM_HZ};
use dwell_world::{CHUNK_SIZE, FaceFrame, cell_of_pos, m_of_row};

#[rustfmt::skip]
mod table {
    include!("creatures.gen.rs");
}
pub use table::{CREATURES, LIMITS, RULES};

/// The fixture data (shared/creatures/fixtures): a creature added by data alone, for tests.
#[rustfmt::skip]
pub mod fixture {
    include!("creatures_fixture.gen.rs");
}

/// The fixture NPC (shared/creatures/fixtures/npcs.json): an NPC kind added by data alone.
#[rustfmt::skip]
pub mod npc_fixture {
    include!("npcs_fixture.gen.rs");
}

/// An NPC's idle pose (`NpcNet::pose` on the wire, `NPC_POSES` its names).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pose {
    Stand = 0,
    Look = 1,
    Sit = 2,
}

impl Pose {
    pub fn name(self) -> &'static str {
        NPC_POSES[self as usize]
    }
}

/// Who an NPC treats as a foe (owner 2026-11-12: neutral by default): neutral starts no fight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    Neutral,
}

/// What an NPC does when attacked ([`Creatures::attacked`]): nothing, or turn on its attacker
/// (the swords' stances and blocks come with the sword mechanic, WORK_QUEUE item 35).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combat {
    None,
    FightBack,
}

/// One of an NPC's idles: the pose, its weight among them, how long (s, [min, max]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Idle {
    pub pose: Pose,
    pub weight: f64,
    pub seconds: [f64; 2],
}

/// A colour part of an NPC: its model's palette ramp `from` is drawn as one of `to`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColourSlot {
    pub part: &'static str,
    pub from: &'static str,
    pub to: &'static [&'static str],
}

/// An NPC's behaviour and looks (shared/creatures/creatures.json `npc`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcDef {
    pub look_at_m: f64,
    pub turn_deg_per_s: f64,
    pub bump_m: f64,
    pub sidestep_m: f64,
    /// The item in its hand (shared/items/items.json), drawn on its model's hand joint.
    pub held: Option<&'static str>,
    pub disposition: Disposition,
    pub combat: Combat,
    pub idles: &'static [Idle],
    pub colours: &'static [ColourSlot],
}

/// An NPC's colours from its `variant` seed: per part, (the model's ramp, the ramp drawn in its
/// place). Deterministic: the same seed always picks the same.
pub fn npc_colours(def: &NpcDef, variant: u32) -> Vec<(&'static str, &'static str)> {
    def.colours
        .iter()
        .enumerate()
        .map(|(j, c)| {
            let h = unit(mix(u64::from(variant), [j as i32, 0, 0], 0x636f_6c6f));
            let i = ((h * c.to.len() as f64) as usize).min(c.to.len() - 1);
            (c.from, c.to[i])
        })
        .collect()
}

/// The biome of a face-local column (`[x, z]`), by name; None: the world has no biome map (a
/// rule's `biomes` are then not checked).
pub type BiomeAt<'a> = &'a dyn Fn(FaceFrame, [i32; 2]) -> Option<&'static str>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub spawn_chunks: i32,
    pub despawn_m: f64,
    pub max: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CreatureDef {
    pub name: &'static str,
    pub model: &'static str,
    pub walk_speed: f64,
    pub wander_m: f64,
    pub idle_seconds: [f64; 2],
    pub rows: i32,
    pub climb_rows: i32,
    pub drop_rows: i32,
    /// An NPC's behaviour and looks; None: an animal.
    pub npc: Option<NpcDef>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnRule {
    pub creature: usize,
    pub on: &'static [&'static str],
    pub chance: f64,
    pub group: [u32; 2],
    /// Other creatures a group may hold in place of the rule's own (a herd's stag).
    pub mix: &'static [MixPart],
    /// No group within this many metres of another of its kind (0: no spacing).
    pub spacing_m: f64,
    /// The biomes it spawns in (shared/worldgen/biomes names); empty: any.
    pub biomes: &'static [&'static str],
}

/// A part of a rule's group mix (shared/creatures/creatures.json `mix`, ADR 0073): rolled once a
/// group with `chance`, it puts up to `max` of `creature` in the group in place of the rule's own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixPart {
    pub creature: usize,
    pub chance: f64,
    pub max: u32,
}

/// The kinds of a group of `n` under `rule`: the rule's creature, with each mix part that rolls
/// under its chance (`roll(part)`, 0..1) taking up to its `max` places from the group's end.
pub fn group_kinds(rule: &SpawnRule, n: u32, roll: impl Fn(usize) -> f64) -> Vec<usize> {
    let mut kinds = vec![rule.creature; n as usize];
    let mut end = kinds.len();
    for (j, part) in rule.mix.iter().enumerate() {
        if roll(j) < part.chance {
            for _ in 0..part.max.min(end as u32) {
                end -= 1;
                kinds[end] = part.creature;
            }
        }
    }
    kinds
}

/// The cells of a spawn column tried for a rule's ground.
const TRIES: u64 = 12;

/// The data a world's creatures follow.
#[derive(Clone, Copy, Debug)]
pub struct Table {
    pub limits: Limits,
    pub creatures: &'static [CreatureDef],
    pub rules: &'static [SpawnRule],
}

impl Table {
    pub const SHIPPED: Table = Table {
        limits: LIMITS,
        creatures: &CREATURES,
        rules: &RULES,
    };
    pub const FIXTURE: Table = Table {
        limits: fixture::LIMITS,
        creatures: &fixture::CREATURES,
        rules: &fixture::RULES,
    };
    pub const NPC_FIXTURE: Table = Table {
        limits: npc_fixture::LIMITS,
        creatures: &npc_fixture::CREATURES,
        rules: &npc_fixture::RULES,
    };

    /// The NPC kind named `name`, if it is one.
    pub fn npc(&self, name: &str) -> Option<&'static NpcDef> {
        self.creatures
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.npc.as_ref())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Creature {
    pub id: u32,
    /// Its index in the table's creatures.
    pub kind: usize,
    /// Its feet (world metres).
    pub position: [f64; 3],
    /// Degrees; 0 faces +z (the same in both frames).
    pub yaw: f64,
    /// Its speed this tick (m/s): 0 idling.
    pub speed: f64,
    /// Where it walks to (face-local x, z), if walking.
    pub target: Option<[f64; 2]>,
    /// Ticks left idling.
    pub idle: u32,
    /// The spawn column it came from ([chunk x, face sign, chunk z]).
    pub home: [i32; 3],
    /// Driven by the near ecology (docs/ECOLOGY.md §8): it walks only to the targets it is given
    /// (no wandering of its own) and is never despawned here (it folds back into its herd).
    pub driven: bool,
    /// × its walking speed (a sprint, a flight; 1 walking).
    pub pace: f64,
    /// An NPC's idle pose (Stand while it walks).
    pub pose: Pose,
    /// An NPC's palette variant seed (its colours: [`npc_colours`]).
    pub variant: u32,
    /// An NPC walking aside from a bump.
    pub sidestepping: bool,
    /// An NPC's heading when its idle began (looking around sweeps about it) and that idle's ticks.
    look_base: f64,
    idle_len: u32,
    /// An NPC turned on its attacker (`Combat::FightBack`): where it is and ticks left.
    pub foe: Option<[f64; 3]>,
    foe_ticks: u32,
    rng: u64,
}

impl Creature {
    pub fn net(&self, table: &Table) -> CreatureNet {
        CreatureNet {
            id: self.id,
            model: table.creatures[self.kind].model.to_string(),
            position: self.position.map(|v| v as f32),
            yaw: self.yaw as f32,
            speed: self.speed as f32,
        }
    }

    /// Its NPC part on the wire; None for an animal.
    pub fn npc_net(&self, table: &Table) -> Option<NpcNet> {
        let def = &table.creatures[self.kind];
        def.npc.map(|_| NpcNet {
            id: self.id,
            kind: def.name.to_string(),
            pose: self.pose as u8,
            variant: self.variant,
        })
    }
}

/// A world cell's state; None: not loaded.
pub type Get<'a> = &'a dyn Fn([i32; 3]) -> Option<StateId>;

fn open(m: StateId) -> bool {
    try_info(m).is_some_and(|i| !i.solid && !i.liquid)
}

fn ground(m: StateId) -> bool {
    try_info(m).is_some_and(|i| i.solid && !i.liquid && i.shape == VoxelShape::Full)
}

/// The face-local row of the feet of a creature `rows` tall standing in the column at face-local
/// `(x, z)`, searched from `row + up` down to `row - down`: the first row whose ground is a solid
/// full block with `rows` open, dry cells over it. None: no such row (a cliff, water, unloaded:
/// a cell not loaded is neither ground nor open).
pub fn feet_row(
    get: Get,
    f: FaceFrame,
    [x, z]: [i32; 2],
    row: i32,
    up: i32,
    down: i32,
    rows: i32,
) -> Option<i32> {
    let at = |y: i32| get(f.cell([x, y, z]));
    for y in (row - down..=row + up).rev() {
        if !at(y - 1).is_some_and(ground) {
            continue;
        }
        if (0..rows).all(|k| at(y + k).is_some_and(open)) {
            return Some(y);
        }
    }
    None
}

/// The face-local height of the bottom of face-local row `y` (the top of the cell under it).
pub(crate) fn feet_y(f: FaceFrame, [x, y, z]: [i32; 3]) -> f64 {
    let c = f.cell([x, y - 1, z]);
    let (a, b) = (f.y(m_of_row(c[1])), f.y(m_of_row(c[1] + 1)));
    a.max(b)
}

/// splitmix64's finaliser over a seed, a cell and a salt.
fn mix(seed: u64, [x, y, z]: [i32; 3], salt: u64) -> u64 {
    let mut h = seed ^ 0x6372_6561_7475_7265; // "creature"
    for v in [x as u32 as u64, y as u32 as u64, z as u32 as u64, salt] {
        h = h.wrapping_add(v).wrapping_add(0x9e37_79b9_7f4a_7c15);
        h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^= h >> 31;
    }
    h
}

fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

impl Creature {
    fn roll(&mut self) -> f64 {
        self.rng = mix(self.rng, [0; 3], 7);
        unit(self.rng)
    }
    fn idle_ticks(&mut self, def: &CreatureDef) -> u32 {
        let [lo, hi] = def.idle_seconds;
        ((lo + (hi - lo) * self.roll()) * f64::from(SIM_HZ)) as u32
    }

    /// Begins an idle: an animal's of `idle_seconds`, an NPC's a pose picked by weight for its
    /// seconds.
    fn start_idle(&mut self, def: &CreatureDef) {
        self.sidestepping = false;
        let Some(n) = def.npc else {
            self.idle = self.idle_ticks(def).max(1);
            return;
        };
        let total: f64 = n.idles.iter().map(|i| i.weight).sum();
        let mut r = self.roll() * total;
        let mut pick = n.idles[n.idles.len() - 1];
        for i in n.idles {
            if r < i.weight {
                pick = *i;
                break;
            }
            r -= i.weight;
        }
        let [lo, hi] = pick.seconds;
        self.pose = pick.pose;
        self.idle = (((lo + (hi - lo) * self.roll()) * f64::from(SIM_HZ)) as u32).max(1);
        self.idle_len = self.idle;
        self.look_base = self.yaw;
    }

    /// Turns its heading toward `to` (degrees) by at most `max` degrees.
    fn turn_toward(&mut self, to: f64, max: f64) {
        let d = (to - self.yaw + 540.0).rem_euclid(360.0) - 180.0;
        self.yaw += d.clamp(-max, max);
    }
}

/// How far (degrees) an NPC looking around turns either way, and the sweep's period (s).
const LOOK_SWEEP_DEG: f64 = 60.0;
const LOOK_PERIOD_S: f64 = 4.0;
/// How long an NPC that fights back stays turned on its attacker (s).
const FOE_SECONDS: f64 = 10.0;

/// The world's creatures.
#[derive(Debug)]
pub struct Creatures {
    pub table: Table,
    pub list: Vec<Creature>,
    next_id: u32,
    /// The spawn columns rolled while a player is near (kept until their creatures are gone).
    rolled: HashSet<[i32; 3]>,
    /// Creatures gone since the clients were last told.
    pub removed: Vec<u32>,
    /// Kinds the spawn rules leave out (the near ecology's: its herds and packs make them).
    pub reserved: Vec<usize>,
}

impl Default for Creatures {
    fn default() -> Self {
        Self::new(Table::SHIPPED)
    }
}

impl Creatures {
    pub fn new(table: Table) -> Self {
        Self {
            table,
            list: Vec::new(),
            next_id: 0,
            rolled: HashSet::new(),
            removed: Vec::new(),
            reserved: Vec::new(),
        }
    }

    /// The kind named `name` (`"dwell:wolf"`).
    pub fn kind_of(&self, name: &str) -> Option<usize> {
        self.table.creatures.iter().position(|c| c.name == name)
    }

    /// Adds a creature of `kind` driven by the near ecology, its feet at `position` (world), or
    /// None at the world's most creatures.
    pub fn add_driven(
        &mut self,
        kind: usize,
        position: [f64; 3],
        yaw: f64,
        rng: u64,
    ) -> Option<u32> {
        if self.list.len() >= self.table.limits.max || kind >= self.table.creatures.len() {
            return None;
        }
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let f = FaceFrame::at_height(position[1]);
        let local = f.point(position);
        let home = [
            (local[0].floor() as i32).div_euclid(CHUNK_SIZE),
            f.sign(),
            (local[2].floor() as i32).div_euclid(CHUNK_SIZE),
        ];
        self.list.push(Creature {
            id: self.next_id,
            kind,
            position,
            yaw,
            speed: 0.0,
            target: None,
            idle: 0,
            home,
            driven: true,
            pace: 1.0,
            pose: Pose::Stand,
            variant: 0,
            sidestepping: false,
            look_base: yaw,
            idle_len: 0,
            foe: None,
            foe_ticks: 0,
            rng,
        });
        Some(self.next_id)
    }

    /// Removes creature `id` (the clients are told).
    pub fn remove(&mut self, id: u32) {
        if let Some(i) = self.list.iter().position(|c| c.id == id) {
            self.list.remove(i);
            self.removed.push(id);
        }
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Creature> {
        self.list.iter_mut().find(|c| c.id == id)
    }

    /// Rolls the spawn columns within `spawn_chunks` of each player (world feet) not yet rolled;
    /// a column whose cells aren't loaded waits. Deterministic: the roll, the cell and the group
    /// hash the world seed and the face-local column.
    pub fn spawn_near(&mut self, players: &[[f64; 3]], get: Get, seed: u64) {
        self.spawn_near_in(players, get, seed, &|_, _| None);
    }

    /// [`Creatures::spawn_near`] with the world's biomes (a rule's `biomes`).
    pub fn spawn_near_in(&mut self, players: &[[f64; 3]], get: Get, seed: u64, biome: BiomeAt) {
        let r = self.table.limits.spawn_chunks;
        for p in players {
            let f = FaceFrame::at_height(p[1]);
            let local = f.point(*p);
            let row = cell_of_pos(local)[1];
            let (pcx, pcz) = (
                (local[0].floor() as i32).div_euclid(CHUNK_SIZE),
                (local[2].floor() as i32).div_euclid(CHUNK_SIZE),
            );
            for cx in pcx - r..=pcx + r {
                for cz in pcz - r..=pcz + r {
                    let key = [cx, f.sign(), cz];
                    if self.rolled.contains(&key) {
                        continue;
                    }
                    if self.roll_column(get, seed, f, key, row, biome) {
                        self.rolled.insert(key);
                    }
                }
            }
        }
    }

    /// One column's roll; false when its cells aren't loaded yet.
    fn roll_column(
        &mut self,
        get: Get,
        seed: u64,
        f: FaceFrame,
        key: [i32; 3],
        row: i32,
        biome: BiomeAt,
    ) -> bool {
        let col = [key[0], 0, key[2]];
        for (i, rule) in self.table.rules.iter().enumerate() {
            if self.reserved.contains(&rule.creature) {
                continue;
            }
            let h = |salt: u64| unit(mix(seed, col, salt * 16 + i as u64));
            if h(0) >= rule.chance {
                continue;
            }
            let def = self.table.creatures[rule.creature];
            let on: Vec<_> = rule.on.iter().filter_map(|n| find_block(n)).collect();
            let on_ground = |c: [i32; 3]| {
                get(c).is_some_and(|m| on.iter().any(|b| b.states().contains(&usize::from(m))))
            };
            // Up to `TRIES` cells of the column, the first on the rule's ground: the surface near
            // the player's row (the search reads up to 48 rows either way).
            let mut found = None;
            for t in 0..TRIES {
                let x = key[0] * CHUNK_SIZE + (h(100 + 2 * t) * f64::from(CHUNK_SIZE)) as i32;
                let z = key[2] * CHUNK_SIZE + (h(101 + 2 * t) * f64::from(CHUNK_SIZE)) as i32;
                if get(f.cell([x, row, z])).is_none() {
                    return false; // not loaded: roll again later
                }
                if let Some(y) = feet_row(get, f, [x, z], row, 48, 48, def.rows)
                    && on_ground(f.cell([x, y - 1, z]))
                {
                    found = Some((x, y, z));
                    break;
                }
            }
            let Some((x, y, z)) = found else {
                continue;
            };
            if !rule.biomes.is_empty()
                && biome(f, [x, z]).is_some_and(|b| !rule.biomes.contains(&b))
            {
                continue;
            }
            // Spacing: no group within `spacing_m` of another of its kind on this face.
            if rule.spacing_m > 0.0
                && self.list.iter().any(|c| {
                    let l = f.point(c.position);
                    c.kind == rule.creature
                        && FaceFrame::at_height(c.position[1]).sign() == f.sign()
                        && (l[0] - f64::from(x)).hypot(l[2] - f64::from(z)) < rule.spacing_m
                })
            {
                continue;
            }
            let [lo, hi] = rule.group;
            let n = lo + ((h(3) * f64::from(hi - lo + 1)) as u32).min(hi - lo);
            let kinds = group_kinds(rule, n, |j| h(200 + j as u64));
            for k in 0..n {
                let kind = kinds[k as usize];
                let def = self.table.creatures[kind];
                if self.list.len() >= self.table.limits.max {
                    break;
                }
                let (dx, dz) = (
                    (h(10 + u64::from(k)) * 5.0) as i32 - 2,
                    (h(30 + u64::from(k)) * 5.0) as i32 - 2,
                );
                let Some(yy) = feet_row(get, f, [x + dx, z + dz], y, 2, 2, def.rows)
                    .filter(|&yy| on_ground(f.cell([x + dx, yy - 1, z + dz])))
                else {
                    continue;
                };
                self.next_id = self.next_id.wrapping_add(1).max(1);
                let at = [
                    f64::from(x + dx) + 0.5,
                    feet_y(f, [x + dx, yy, z + dz]),
                    f64::from(z + dz) + 0.5,
                ];
                let mut c = Creature {
                    id: self.next_id,
                    kind,
                    position: f.point(at),
                    yaw: h(50 + u64::from(k)) * 360.0,
                    speed: 0.0,
                    target: None,
                    idle: 0,
                    home: key,
                    driven: false,
                    pace: 1.0,
                    pose: Pose::Stand,
                    variant: mix(seed, [x + dx, yy, z + dz], 0x6e70_6373) as u32,
                    sidestepping: false,
                    look_base: 0.0,
                    idle_len: 0,
                    foe: None,
                    foe_ticks: 0,
                    rng: mix(seed, [x + dx, yy, z + dz], 99),
                };
                c.look_base = c.yaw;
                if def.npc.is_some() {
                    c.start_idle(&def);
                } else {
                    c.idle = c.idle_ticks(&def);
                }
                self.list.push(c);
            }
            return true;
        }
        true
    }

    /// One tick with no players near (no NPC looks at or steps aside from anyone).
    pub fn step(&mut self, get: Get) {
        self.step_near(get, &[]);
    }

    /// An NPC of `id` is attacked by something at `from` (world): with `Combat::FightBack` it
    /// stops and turns on it for `FOE_SECONDS`. True when it fights back.
    pub fn attacked(&mut self, id: u32, from: [f64; 3]) -> bool {
        let table = self.table;
        let Some(c) = self.get_mut(id) else {
            return false;
        };
        match table.creatures[c.kind].npc.map(|n| n.combat) {
            Some(Combat::FightBack) => {
                c.foe = Some(from);
                c.foe_ticks = (FOE_SECONDS * f64::from(SIM_HZ)) as u32;
                true
            }
            _ => false,
        }
    }

    /// One tick: each creature idles or walks a step; a step onto ground it can't stand on (water,
    /// a cliff's edge, a wall, unloaded) ends the walk. NPCs also look at the `players` (world
    /// feet) near them and step aside from a player or another NPC that bumps them.
    pub fn step_near(&mut self, get: Get, players: &[[f64; 3]]) {
        let dt = 1.0 / f64::from(SIM_HZ);
        let table = self.table;
        // Who can bump an NPC: the players (id 0) and the NPCs, as of the tick's start.
        let bumpers: Vec<(u32, [f64; 3])> = players
            .iter()
            .map(|p| (0, *p))
            .chain(
                self.list
                    .iter()
                    .filter(|c| table.creatures[c.kind].npc.is_some())
                    .map(|c| (c.id, c.position)),
            )
            .collect();
        for c in &mut self.list {
            let def = table.creatures[c.kind];
            let f = FaceFrame::at_height(c.position[1]);
            let local = f.point(c.position);
            if let (Some(n), false) = (def.npc, c.driven) {
                let near = |p: &[f64; 3]| {
                    (FaceFrame::at_height(p[1]).sign() == f.sign()).then(|| {
                        let l = f.point(*p);
                        let (dx, dz) = (l[0] - local[0], l[2] - local[2]);
                        (dx, dz, dx.hypot(dz))
                    })
                };
                let max_turn = n.turn_deg_per_s * dt;
                if c.foe_ticks > 0 {
                    c.foe_ticks -= 1;
                    c.target = None;
                    c.speed = 0.0;
                    c.pose = Pose::Stand;
                    if let Some((dx, dz, _)) = c.foe.as_ref().and_then(near) {
                        c.turn_toward(dx.atan2(dz).to_degrees(), max_turn);
                    }
                    if c.foe_ticks == 0 {
                        c.foe = None;
                        c.start_idle(&def);
                    }
                    continue;
                }
                let bump = bumpers
                    .iter()
                    .filter(|(id, _)| *id != c.id)
                    .filter_map(|(_, p)| near(p))
                    .filter(|&(_, _, d)| d < n.bump_m)
                    .min_by(|a, b| a.2.total_cmp(&b.2));
                if let (Some((dx, dz, d)), false) = (bump, c.sidestepping) {
                    // Away from the bumper; one right on top of it: to its side.
                    let (ax, az) = if d > 1e-6 {
                        (-dx / d, -dz / d)
                    } else {
                        let y = c.yaw.to_radians();
                        (y.cos(), -y.sin())
                    };
                    c.target = Some([local[0] + ax * n.sidestep_m, local[2] + az * n.sidestep_m]);
                    c.idle = 0;
                    c.pose = Pose::Stand;
                    c.sidestepping = true;
                }
                if c.idle > 0 {
                    // Idle: face the nearest player within `look_at_m`, else look around.
                    let watch = players
                        .iter()
                        .filter_map(near)
                        .filter(|&(_, _, d)| d <= n.look_at_m && d > 1e-6)
                        .min_by(|a, b| a.2.total_cmp(&b.2));
                    if let Some((dx, dz, _)) = watch {
                        c.turn_toward(dx.atan2(dz).to_degrees(), max_turn);
                    } else if c.pose == Pose::Look {
                        let t = f64::from(c.idle_len - c.idle) / f64::from(SIM_HZ);
                        let to = c.look_base
                            + LOOK_SWEEP_DEG * (t * std::f64::consts::TAU / LOOK_PERIOD_S).sin();
                        c.turn_toward(to, max_turn);
                    }
                }
            }
            if c.driven {
                c.idle = 0;
                if c.target.is_none() {
                    c.speed = 0.0;
                    continue;
                }
            }
            if c.idle > 0 {
                c.idle -= 1;
                c.speed = 0.0;
                if c.idle == 0 {
                    c.pose = Pose::Stand;
                    let a = c.roll() * std::f64::consts::TAU;
                    let d = def.wander_m * (0.3 + 0.7 * c.roll());
                    c.target = Some([local[0] + d * a.sin(), local[2] + d * a.cos()]);
                }
                continue;
            }
            let Some([tx, tz]) = c.target else {
                c.start_idle(&def);
                continue;
            };
            let (dx, dz) = (tx - local[0], tz - local[2]);
            let dist = (dx * dx + dz * dz).sqrt();
            let speed = def.walk_speed * c.pace;
            let step = speed * dt;
            if dist <= step {
                c.target = None;
                if !c.driven {
                    c.start_idle(&def);
                }
                c.speed = 0.0;
                continue;
            }
            let next = [local[0] + dx / dist * step, local[2] + dz / dist * step];
            let row = cell_of_pos(local)[1];
            let col = [next[0].floor() as i32, next[1].floor() as i32];
            match feet_row(get, f, col, row, def.climb_rows, def.drop_rows, def.rows) {
                Some(y) => {
                    let ny = feet_y(f, [col[0], y, col[1]]);
                    c.position = f.point([next[0], ny, next[1]]);
                    c.yaw = dx.atan2(dz).to_degrees();
                    c.speed = speed;
                }
                None => {
                    c.target = None;
                    if !c.driven {
                        c.start_idle(&def);
                    }
                    c.speed = 0.0;
                }
            }
        }
    }

    /// Removes the creatures with no player within `despawn_m`, and forgets the columns whose
    /// creatures are all gone and that no player is near (they roll again when one comes back).
    pub fn despawn_far(&mut self, players: &[[f64; 3]]) {
        let r2 = self.table.limits.despawn_m.powi(2);
        let near = |p: &[f64; 3]| {
            players
                .iter()
                .any(|q| (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>() <= r2)
        };
        let removed = &mut self.removed;
        self.list.retain(|c| {
            let keep = c.driven || near(&c.position);
            if !keep {
                removed.push(c.id);
            }
            keep
        });
        let mut live: HashMap<[i32; 3], ()> = HashMap::new();
        for c in &self.list {
            live.insert(c.home, ());
        }
        let span = f64::from(CHUNK_SIZE);
        self.rolled.retain(|k| {
            live.contains_key(k)
                || players.iter().any(|p| {
                    let f = FaceFrame::at_height(p[1]);
                    let l = f.point(*p);
                    f.sign() == k[1]
                        && ((f64::from(k[0]) + 0.5) * span - l[0])
                            .hypot((f64::from(k[2]) + 0.5) * span - l[2])
                            <= self.table.limits.despawn_m
                })
        });
    }

    pub fn get(&self, id: u32) -> Option<&Creature> {
        self.list.iter().find(|c| c.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::both_faces;

    /// A stone floor (face-local row 9 the ground, feet at row 10) over x, z in [-64, 64), with a
    /// strip of water at 20 <= x < 24 and a 6-row wall at x <= -6.
    fn world(f: FaceFrame) -> impl Fn([i32; 3]) -> Option<StateId> {
        let stone = find_block("dwell:stone").unwrap().first_state;
        let water = find_block("dwell:water").unwrap().first_state;
        move |c: [i32; 3]| {
            let [x, y, z] = f.cell(c);
            if !(-64..64).contains(&x) || !(-64..64).contains(&z) {
                return None;
            }
            Some(if y <= 9 || (x <= -6 && y <= 15) {
                stone
            } else if (20..24).contains(&x) && y <= 11 {
                water
            } else {
                0
            })
        }
    }

    fn spawn(f: FaceFrame) -> Creatures {
        let g = world(f);
        let mut cs = Creatures::new(Table::FIXTURE);
        let feet = f.point([0.5, m_of_row(10), 0.5]);
        cs.spawn_near(&[feet], &g, 42);
        cs
    }

    both_faces!(
        fn the_fixture_creature_spawns_by_data_on_its_ground(f: FaceFrame) {
            let cs = spawn(f);
            assert_eq!(cs.list.len(), 3, "the fixture's group of 3");
            // The mix by data: two walkers and their leader.
            let names: Vec<_> = cs
                .list
                .iter()
                .map(|c| cs.table.creatures[c.kind].name)
                .collect();
            assert_eq!(names, ["test:walker", "test:walker", "test:leader"]);
            for c in &cs.list {
                let l = f.point(c.position);
                assert!(
                    (l[1] - m_of_row(10)).abs() < 1e-9,
                    "feet on the floor: {l:?}"
                );
            }
            // Deterministic: the same seed spawns the same; the faces mirror each other.
            let a: Vec<_> = spawn(FaceFrame::A)
                .list
                .iter()
                .map(|c| c.position)
                .collect();
            let here: Vec<_> = cs
                .list
                .iter()
                .map(|c| FaceFrame::A.point(f.point(c.position)))
                .collect();
            assert_eq!(a, here);
        }
    );

    // Owner, 2026-11-10: "let's disable all the animals except wolves and deer for now". The rules
    // spawn only deer (does, the stag by the mix) and wolves, in packs of 2-4 rarer than the herds.
    #[test]
    fn only_deer_wolves_and_villagers_spawn_the_wolves_in_small_rarer_packs() {
        let t = Table::SHIPPED;
        let name = |k: usize| t.creatures[k].name;
        let mut spawned: Vec<_> = t
            .rules
            .iter()
            .flat_map(|r| std::iter::once(r.creature).chain(r.mix.iter().map(|m| m.creature)))
            .map(name)
            .collect();
        spawned.sort_unstable();
        // And the wandering NPCs (WORK_QUEUE item 38), the villagers.
        assert_eq!(
            spawned,
            ["dwell:deer", "dwell:doe", "dwell:villager", "dwell:wolf"]
        );
        let rule = |n: &str| t.rules.iter().find(|r| name(r.creature) == n).unwrap();
        let (wolf, doe) = (rule("dwell:wolf"), rule("dwell:doe"));
        assert_eq!(wolf.group, [2, 4]);
        assert!(wolf.chance < doe.chance);
        assert_eq!(t.creatures[wolf.creature].model, "wolf");
    }

    // Owner, 2026-11-10: herds "mostly does with sometimes one stag", as data: the game's deer rule
    // spawns does, a share of its groups (the mix's chance) with one stag, never two.
    #[test]
    fn a_herd_is_mostly_does_with_sometimes_one_stag() {
        let t = Table::SHIPPED;
        let name = |k: usize| t.creatures[k].name;
        let rule = t
            .rules
            .iter()
            .find(|r| name(r.creature) == "dwell:doe")
            .expect("a doe rule");
        let (mut with, mut groups) = (0usize, 0u32);
        for g in 0..400u64 {
            let roll = |j: usize| unit(mix(g, [0, 0, 0], 200 + j as u64));
            let kinds = group_kinds(rule, 4, roll);
            assert_eq!(kinds, group_kinds(rule, 4, roll), "deterministic");
            let stags = kinds.iter().filter(|&&k| name(k) == "dwell:deer").count();
            assert!(stags <= 1, "{stags} stags");
            assert!(kinds.iter().filter(|&&k| name(k) == "dwell:doe").count() >= 3);
            with += stags;
            groups += 1;
        }
        let share = with as f64 / f64::from(groups);
        assert!(
            (0.25..0.55).contains(&share),
            "a stag in {share} of the herds"
        );
    }

    both_faces!(
        fn wandering_stays_on_walkable_ground_out_of_water_and_off_the_wall(f: FaceFrame) {
            let g = world(f);
            let mut cs = spawn(f);
            let mut walked = 0.0;
            for _ in 0..60 * 120 {
                let before: Vec<_> = cs.list.iter().map(|c| c.position).collect();
                cs.step(&g);
                for (c, b) in cs.list.iter().zip(&before) {
                    let l = f.point(c.position);
                    walked += (0..3)
                        .map(|k| (c.position[k] - b[k]).powi(2))
                        .sum::<f64>()
                        .sqrt();
                    assert!(
                        (l[1] - m_of_row(10)).abs() < 1e-9,
                        "on the floor, not in the pool or on the wall: {l:?}"
                    );
                    assert!(
                        !(20.0..24.0).contains(&l[0]) && l[0] > -5.0,
                        "out of the water and off the wall: {l:?}"
                    );
                }
            }
            assert!(walked > 20.0, "they wander ({walked} m)");
        }
    );

    both_faces!(
        fn creatures_far_from_every_player_despawn(f: FaceFrame) {
            let mut cs = spawn(f);
            let near = f.point([0.5, m_of_row(10), 0.5]);
            cs.despawn_far(&[near]);
            assert_eq!(cs.list.len(), 3);
            cs.despawn_far(&[f.point([500.0, m_of_row(10), 0.5])]);
            assert!(cs.list.is_empty());
            assert_eq!(cs.removed.len(), 3);
            // Their column rolls again when a player comes back.
            let g = world(f);
            cs.spawn_near(&[near], &g, 42);
            assert_eq!(cs.list.len(), 3);
        }
    );

    // --- NPCs (WORK_QUEUE item 38) ---

    fn npcs(f: FaceFrame, at: [f64; 2]) -> Creatures {
        let g = world(f);
        let mut cs = Creatures::new(Table::NPC_FIXTURE);
        cs.spawn_near(&[f.point([at[0], m_of_row(10), at[1]])], &g, 7);
        cs
    }

    /// Face-local feet on walkable ground of `world`: the floor (out of the water, off the wall's
    /// side) or the wall's top (a 6-row drop off it: the fixture steps down 1).
    fn walkable(l: [f64; 3]) -> bool {
        let floor =
            (l[1] - m_of_row(10)).abs() < 1e-9 && !(20.0..24.0).contains(&l[0]) && l[0] > -5.0;
        let top = (l[1] - m_of_row(16)).abs() < 1e-9 && l[0] < -5.0;
        floor || top
    }

    /// One fixture NPC idling (standing) at face-local (x, z) on the floor, facing +z.
    fn one_npc(f: FaceFrame, [x, z]: [f64; 2]) -> Creatures {
        let mut cs = npcs(f, [0.5, 0.5]);
        cs.list.truncate(1);
        let c = &mut cs.list[0];
        c.position = f.point([x, m_of_row(10), z]);
        c.target = None;
        c.idle = 60 * 60;
        c.idle_len = c.idle;
        c.pose = Pose::Stand;
        c.yaw = 0.0;
        cs
    }

    both_faces!(
        fn an_npc_kind_spawns_by_data_near_a_player_kept_apart_and_none_far(f: FaceFrame) {
            let cs = npcs(f, [0.5, 0.5]);
            assert!(cs.list.len() >= 2, "{} NPCs", cs.list.len());
            let span = f64::from(CHUNK_SIZE) * 2.0;
            for (i, c) in cs.list.iter().enumerate() {
                assert_eq!(cs.table.creatures[c.kind].name, "test:person");
                assert!(cs.table.creatures[c.kind].npc.is_some());
                let l = f.point(c.position);
                assert!(walkable(l), "on the floor or the wall's top: {l:?}");
                assert!(
                    l[0].abs() <= span && l[2].abs() <= span,
                    "only near the player: {l:?}"
                );
                // The rule's spacing (20 m between groups; a member spawns up to 2 cells off).
                for d in &cs.list[i + 1..] {
                    let m = f.point(d.position);
                    assert!((l[0] - m[0]).hypot(l[2] - m[2]) > 20.0 - 3.0);
                }
            }
            // The same seed spawns the same, mirrored on face B.
            let a: Vec<_> = npcs(FaceFrame::A, [0.5, 0.5])
                .list
                .iter()
                .map(|c| (c.position, c.variant))
                .collect();
            let here: Vec<_> = cs
                .list
                .iter()
                .map(|c| (FaceFrame::A.point(f.point(c.position)), c.variant))
                .collect();
            assert_eq!(a, here);
            // Far from every player they go.
            let mut cs = cs;
            cs.despawn_far(&[f.point([500.0, m_of_row(10), 0.5])]);
            assert!(cs.list.is_empty());
        }
    );

    both_faces!(
        fn a_rule_s_biomes_limit_where_it_spawns(f: FaceFrame) {
            let rules: &'static [SpawnRule] = Box::leak(Box::new([SpawnRule {
                biomes: &["preset:steppe"],
                ..npc_fixture::RULES[0]
            }]));
            let table = Table {
                rules,
                ..Table::NPC_FIXTURE
            };
            let g = world(f);
            let feet = [f.point([0.5, m_of_row(10), 0.5])];
            let count = |biome: BiomeAt| {
                let mut cs = Creatures::new(table);
                cs.spawn_near_in(&feet, &g, 7, biome);
                cs.list.len()
            };
            assert_eq!(count(&|_, _| Some("preset:boreal")), 0, "another biome");
            assert!(count(&|_, _| Some("preset:steppe")) > 0, "its biome");
            assert!(count(&|_, _| None) > 0, "no biome map: unchecked");
        }
    );

    both_faces!(
        fn npcs_wander_on_walkable_ground_and_idle_in_their_poses(f: FaceFrame) {
            let g = world(f);
            let mut cs = npcs(f, [0.5, 0.5]);
            let (mut walked, mut poses) = (0.0, HashSet::new());
            for _ in 0..60 * 120 {
                let before: Vec<_> = cs.list.iter().map(|c| c.position).collect();
                cs.step_near(&g, &[]);
                for (c, b) in cs.list.iter().zip(&before) {
                    let l = f.point(c.position);
                    walked += (0..3)
                        .map(|k| (c.position[k] - b[k]).powi(2))
                        .sum::<f64>()
                        .sqrt();
                    assert!(walkable(l), "on the floor or the wall's top: {l:?}");
                    if c.idle > 0 {
                        poses.insert(c.pose);
                    }
                }
            }
            assert!(walked > 20.0, "they wander ({walked} m)");
            // The fixture's idles by data: looking around and sitting.
            assert_eq!(poses, HashSet::from([Pose::Look, Pose::Sit]));
        }
    );

    both_faces!(
        fn an_idle_npc_turns_to_look_at_a_near_player_and_not_a_far_one(f: FaceFrame) {
            let g = world(f);
            // A player 3 m to its +x (within the fixture's 5 m): it turns to face +x, 90 degrees.
            let mut cs = one_npc(f, [0.5, 0.5]);
            let near = f.point([3.5, m_of_row(10), 0.5]);
            for _ in 0..60 {
                cs.step_near(&g, &[near]);
            }
            let c = &cs.list[0];
            assert!((c.yaw - 90.0).abs() < 1.0, "faces the player: {}", c.yaw);
            assert_eq!(f.point(c.position)[0], 0.5, "turned, not moved");
            // One 10 m off: it keeps facing +z.
            let mut cs = one_npc(f, [0.5, 0.5]);
            for _ in 0..60 {
                cs.step_near(&g, &[f.point([10.5, m_of_row(10), 0.5])]);
            }
            assert!(cs.list[0].yaw.abs() < 1e-9, "{}", cs.list[0].yaw);
        }
    );

    both_faces!(
        fn a_bumped_npc_steps_aside_from_a_player_and_from_another_npc(f: FaceFrame) {
            let g = world(f);
            let mut cs = one_npc(f, [0.5, 0.5]);
            let player = f.point([0.8, m_of_row(10), 0.5]); // 0.3 m to its +x
            for _ in 0..60 * 3 {
                cs.step_near(&g, &[player]);
            }
            let l = f.point(cs.list[0].position);
            assert!(l[0] < 0.5 - 1.0, "stepped away from the player: {l:?}");
            assert!((l[0] - 0.8).hypot(l[2] - 0.5) > 0.8, "out of reach: {l:?}");
            // Two NPCs 0.4 m apart both step aside.
            let mut cs = npcs(f, [0.5, 0.5]);
            cs.list.truncate(2);
            for (c, x) in cs.list.iter_mut().zip([0.3, 0.7]) {
                c.position = f.point([x, m_of_row(10), 0.5]);
                c.target = None;
                c.idle = 60 * 60;
                c.pose = Pose::Sit;
            }
            for _ in 0..60 * 3 {
                cs.step_near(&g, &[]);
            }
            let (a, b) = (f.point(cs.list[0].position), f.point(cs.list[1].position));
            assert!((a[0] - b[0]).hypot(a[2] - b[2]) > 2.0, "apart: {a:?} {b:?}");
        }
    );

    both_faces!(
        fn an_npc_that_fights_back_stops_and_turns_on_its_attacker(f: FaceFrame) {
            let g = world(f);
            let mut cs = one_npc(f, [0.5, 0.5]);
            let id = cs.list[0].id;
            let from = f.point([0.5, m_of_row(10), -2.5]); // behind it (-z)
            assert!(cs.attacked(id, from), "the fixture's combat is fightBack");
            for _ in 0..90 {
                cs.step_near(&g, &[]);
            }
            let c = &cs.list[0];
            assert!((c.yaw.abs() - 180.0).abs() < 1.0, "faces it: {}", c.yaw);
            assert_eq!(c.speed, 0.0);
            assert_eq!(c.foe, Some(from));
            // An animal has no combat hook.
            let mut animals = spawn(f);
            let id = animals.list[0].id;
            assert!(!animals.attacked(id, from));
        }
    );

    #[test]
    fn an_npc_s_colours_are_deterministic_per_seed_and_from_its_data() {
        let def = Table::NPC_FIXTURE
            .npc("test:person")
            .expect("the fixture NPC");
        assert_eq!(def.held, Some("dwell:stone_axe"));
        assert_eq!(def.disposition, Disposition::Neutral);
        let mut seen: Vec<HashSet<&str>> = vec![HashSet::new(); def.colours.len()];
        for v in 0..400u32 {
            let c = npc_colours(def, v.wrapping_mul(0x9e37_79b9));
            assert_eq!(
                c,
                npc_colours(def, v.wrapping_mul(0x9e37_79b9)),
                "deterministic"
            );
            for ((from, to), (slot, seen)) in c.iter().zip(def.colours.iter().zip(&mut seen)) {
                assert_eq!(*from, slot.from);
                assert!(slot.to.contains(to));
                seen.insert(*to);
            }
        }
        for (slot, seen) in def.colours.iter().zip(&seen) {
            assert_eq!(
                seen.len(),
                slot.to.len(),
                "every option of {} turns up",
                slot.part
            );
        }
        // The game's villager: the Peasant, four parts, sent with its kind and variant.
        let t = Table::SHIPPED;
        let v = t.npc("dwell:villager").expect("the villager");
        let parts: Vec<_> = v.colours.iter().map(|c| c.part).collect();
        assert_eq!(parts, ["shirt", "trousers", "skin", "hair"]);
        let kind = t
            .creatures
            .iter()
            .position(|c| c.name == "dwell:villager")
            .unwrap();
        assert_eq!(t.creatures[kind].model, "peasant");
        let cs = npcs(FaceFrame::A, [0.5, 0.5]);
        let net = cs.list[0].npc_net(&cs.table).expect("an NPC part");
        assert_eq!(
            (net.kind.as_str(), net.variant),
            ("test:person", cs.list[0].variant)
        );
        assert!(
            spawn(FaceFrame::A).list[0]
                .npc_net(&Table::FIXTURE)
                .is_none()
        );
    }
}
