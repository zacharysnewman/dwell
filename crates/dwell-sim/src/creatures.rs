//! Creatures (docs/adr/0073-creatures.md): animals as light server entities, not Rapier bodies.
//! They are spawned in groups near players by the data's rules (shared/creatures/creatures.json,
//! rolled once per chunk column and face from the world seed), wander (idle, then walk to a random
//! nearby point on walkable ground, stopping at water and cliffs), and are removed once no player
//! is within `despawn_m`. Everything is face-local through `FaceFrame`: face B mirrors face A.
//!
//! NPCs (WORK_QUEUE item 38, CHARACTERS.md §11.11) are creatures with an `npc` block: they idle in a
//! pose picked by weight (stand, look around, sit), turn to face a player within `look_at_m`, step
//! aside when a player or another NPC comes within `bump_m`, and carry a palette variant seed the
//! client picks their colours from; their rules may keep groups apart (`spacing_m`) and to biomes.

use std::collections::{HashMap, HashSet};

use dwell_blocks::{StateId, VoxelShape, find_block, try_info};
use dwell_protocol::chopping::{Clash, WeaponDef};
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

/// The stance an NPC fighting with a weapon picks (`fight.stance`): the one that blocks its foe's
/// swing, or either at random (picked anew each wind-up).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FightStance {
    Counter,
    Random,
}

/// How an NPC whose `fightBack` triggered fights with the weapon it holds (shared/creatures
/// `npc.fight`; WORK_QUEUE items 35 and 38, round 2): its stance rule, whether it raises its block
/// while its foe winds up, whether it strikes back while its foe recovers (within
/// `strike_within_m`), the wait between its own swings (s), and how long without being struck it
/// gives up (s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FightDef {
    pub stance: FightStance,
    pub raise_on_windup: bool,
    pub strike_back: bool,
    pub strike_within_m: f64,
    pub cooldown_s: f64,
    pub give_up_s: f64,
}

/// Where a foe's weapon swing is, as an NPC reads it ([`Creatures::step_fights`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwingPhase {
    /// Winding up or on its arc, before its contact: the blade is coming.
    Coming,
    /// After its contact, recovering: an opening.
    Recovering,
}

/// What an NPC reads of the player it fights: where it stands (world feet), its weapon swing (the
/// phase, its plane, its weapon), if one is under way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoeView {
    pub position: [f64; 3],
    pub swing: Option<(SwingPhase, bool, &'static str)>,
}

/// An NPC's own swing landing this tick ([`Creatures::step_fights`]): the NPC, the player it
/// swings at, its weapon and plane, and its eye (world) the strike comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcSwing {
    pub npc: u32,
    pub player: u16,
    pub weapon: &'static str,
    pub vertical: bool,
    pub eye: [f64; 3],
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
    pub fight: FightDef,
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
    /// The space creatures steer to keep between their bodies (BUGS.md #80), at up to `push_mps`.
    pub gap_m: f64,
    pub push_mps: f64,
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
    /// Its health to a weapon (WORK_QUEUE item 35); 0: it can't be hurt.
    pub health: f64,
    /// The radius round its body a swing's arc must come within to hit it (m).
    pub hit_radius_m: f64,
    /// Its body's radius on the ground (m; BUGS.md #80, #81): kept out of walls and other bodies.
    pub body_radius_m: f64,
    /// What it drops when killed: (item, min, max).
    pub drops: &'static [(&'static str, u32, u32)],
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
    /// Its health (its kind's `health` when whole; WORK_QUEUE item 35).
    pub health: f64,
    /// Where (world) the last hit came from, until its mind takes it ([`Creature::take_hurt`]).
    pub hurt_from: Option<[f64; 3]>,
    /// The knockback still to slide (face-local x, z metres).
    pub knock: [f64; 2],
    /// Its sword, if it holds one (the defender API: NPCs, WORK_QUEUE items 35 and 38).
    pub guard: Option<Guard>,
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
    /// The player it fights (its fight rules read that player's swing), if a player struck it.
    pub foe_player: Option<u16>,
    /// Ticks until it may start its own swing, and its own swing's ticks to its contact.
    fight_wait: u32,
    own_swing: Option<u32>,
    rng: u64,
}

/// A defender's sword (WORK_QUEUE item 35; owner, 2026-11-12: the Wii Resort model): the weapon it
/// holds (a `weapon` tool's item name), its stance (vertical or level, the two swing kinds),
/// whether it holds its block, and how long its own swing still runs (attacking drops the block).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guard {
    pub weapon: &'static str,
    pub vertical: bool,
    pub raised: bool,
    /// Ticks left in its own swing.
    pub attacking: u32,
}

impl Guard {
    /// A sword held in a stance, not raised.
    pub fn holding(weapon: &'static str, vertical: bool) -> Self {
        Self {
            weapon,
            vertical,
            raised: false,
            attacking: 0,
        }
    }
    /// Whether its blade guards now: raised and not mid-swing (attacking drops the block).
    pub fn blocking(&self) -> bool {
        self.raised && self.attacking == 0
    }
    /// Starts its own swing for `ticks`: the block drops until it ends.
    pub fn attack(&mut self, ticks: u32) {
        self.attacking = self.attacking.max(ticks);
    }
}

impl Creature {
    /// Where its last hit came from (world), once: its mind's to react to.
    pub fn take_hurt(&mut self) -> Option<[f64; 3]> {
        self.hurt_from.take()
    }

    /// Its health as a share of its kind's whole (1 for a kind that can't be hurt).
    pub fn health_share(&self, table: &Table) -> f64 {
        let whole = table.creatures[self.kind].health;
        if whole > 0.0 {
            (self.health / whole).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

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
    /// Its NPC part; with `swings` (the client's `npcs.swings`) its pose is its strike's while it
    /// swings its sword (`NPC_STRIKE_POSES`), else its idle pose.
    pub fn npc_net(&self, table: &Table, swings: bool) -> Option<NpcNet> {
        let def = &table.creatures[self.kind];
        let striking = self
            .guard
            .filter(|g| swings && self.own_swing.is_some() && g.attacking > 0)
            .map(|g| (NPC_POSES.len() + usize::from(g.vertical)) as u8);
        def.npc.map(|_| NpcNet {
            id: self.id,
            kind: def.name.to_string(),
            pose: striking.unwrap_or(self.pose as u8),
            variant: self.variant,
        })
    }

    /// Starts its own swing now (tests and the dev tools: an NPC mid-strike).
    pub fn strike_now(&mut self, ticks: u32) {
        if let Some(g) = &mut self.guard {
            g.raised = false;
            g.attack(ticks);
            self.own_swing = Some(ticks / 2);
        }
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

fn solid(m: StateId) -> bool {
    try_info(m).is_some_and(|i| i.solid)
}

/// Whether a body of `def`'s radius with its feet in face-local row `y` at face-local `(x, z)`
/// keeps out of the solid cells round its column (BUGS.md #81): each other column its disc
/// reaches has nothing solid from `climb_rows` over its feet up to its height (a step it could
/// climb is no wall; a cell not loaded is one). Cheap: the few columns its disc reaches, a row or
/// two each.
fn body_clear(get: Get, f: FaceFrame, def: &CreatureDef, [x, z]: [f64; 2], y: i32) -> bool {
    let r = def.body_radius_m;
    let (cx, cz) = (x.floor() as i32, z.floor() as i32);
    let rows = def.climb_rows..def.rows.max(def.climb_rows + 1);
    for gx in (x - r).floor() as i32..=(x + r).floor() as i32 {
        for gz in (z - r).floor() as i32..=(z + r).floor() as i32 {
            if (gx, gz) == (cx, cz) {
                continue;
            }
            // The disc reaches the column only if its nearest point is within the radius.
            let nx = x.clamp(f64::from(gx), f64::from(gx + 1));
            let nz = z.clamp(f64::from(gz), f64::from(gz + 1));
            if (nx - x).hypot(nz - z) >= r - 1e-9 {
                continue;
            }
            for k in rows.clone() {
                if get(f.cell([gx, y + k, gz])).is_none_or(solid) {
                    return false;
                }
            }
        }
    }
    true
}

/// Where a creature at face-local `local` (its feet in row `row`) may go toward face-local `next`
/// this tick: there, if it can stand there with its body clear; else sliding along one axis (a
/// wall walked into slanting); None: blocked. A body already not clear (spawned or pushed against
/// a wall) moves by its centre alone, so it can walk out.
fn step_to(
    get: Get,
    f: FaceFrame,
    def: &CreatureDef,
    local: [f64; 3],
    next: [f64; 2],
) -> Option<[f64; 3]> {
    let row = cell_of_pos(local)[1];
    let stand = |p: [f64; 2]| {
        let col = [p[0].floor() as i32, p[1].floor() as i32];
        let y = feet_row(get, f, col, row, def.climb_rows, def.drop_rows, def.rows)?;
        Some((y, [p[0], feet_y(f, [col[0], y, col[1]]), p[1]]))
    };
    if !body_clear(get, f, def, [local[0], local[2]], row) {
        return stand(next).map(|(_, p)| p);
    }
    [next, [next[0], local[2]], [local[0], next[1]]]
        .into_iter()
        .filter(|p| (p[0] - local[0]).abs() + (p[1] - local[2]).abs() > 1e-12)
        .find_map(|p| {
            let (y, at) = stand(p)?;
            body_clear(get, f, def, p, y).then_some(at)
        })
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
/// An NPC's eye above its feet, as a share of its height (`rows`): where its strikes come from.
const NPC_EYE_SHARE: f64 = 0.9;
/// Passes of [`Creatures::separate`] a tick: a crowd's overlaps settle in a few.
const SEPARATE_PASSES: usize = 4;

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
            health: self.table.creatures[kind].health,
            hurt_from: None,
            knock: [0.0; 2],
            guard: guard_of(&self.table.creatures[kind]),
            pose: Pose::Stand,
            variant: 0,
            sidestepping: false,
            look_base: yaw,
            idle_len: 0,
            foe: None,
            foe_ticks: 0,
            foe_player: None,
            fight_wait: 0,
            own_swing: None,
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
                    health: def.health,
                    hurt_from: None,
                    knock: [0.0; 2],
                    guard: guard_of(&def),
                    pose: Pose::Stand,
                    variant: mix(seed, [x + dx, yy, z + dz], 0x6e70_6373) as u32,
                    sidestepping: false,
                    look_base: 0.0,
                    idle_len: 0,
                    foe: None,
                    foe_ticks: 0,
                    foe_player: None,
                    fight_wait: 0,
                    own_swing: None,
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
    /// stops and turns on it for its `fight.giveUpS` (each new strike starts it again). True when
    /// it fights back.
    pub fn attacked(&mut self, id: u32, from: [f64; 3]) -> bool {
        let table = self.table;
        let Some(c) = self.get_mut(id) else {
            return false;
        };
        match table.creatures[c.kind].npc.map(|n| n.combat) {
            Some(Combat::FightBack) => {
                let give_up = table.creatures[c.kind]
                    .npc
                    .map_or(0.0, |n| n.fight.give_up_s);
                c.foe = Some(from);
                c.foe_ticks = ((give_up * f64::from(SIM_HZ)) as u32).max(1);
                true
            }
            _ => false,
        }
    }

    /// The player who struck NPC `id` (after [`Self::strike`]): the one its fight rules watch.
    pub fn set_foe_player(&mut self, id: u32, player: u16) {
        if let Some(c) = self.get_mut(id)
            && c.foe_ticks > 0
        {
            c.foe_player = Some(player);
        }
    }

    /// The NPCs' fights, a tick (after [`Self::step_near`]; WORK_QUEUE items 35 and 38, round 2):
    /// each NPC turned on a player (`foe_player`) and holding a weapon reads that player
    /// (`foes`: None, gone) by its `fight` rules: while the player's blade is coming it raises its
    /// block in its stance (`counter`: the one that blocks that swing; `random`), lowering it
    /// otherwise; while the player recovers, within `strike_within_m`, its wait over, it swings
    /// back (attacking drops its block); and it gives up `giveUpS` after it was last struck. Its
    /// own swings landing this tick are returned for the server to resolve (an NPC's strike never
    /// hurts a player). Face-local through `FaceFrame`.
    pub fn step_fights(&mut self, foes: &dyn Fn(u16) -> Option<FoeView>) -> Vec<NpcSwing> {
        let table = self.table;
        let mut out = Vec::new();
        for c in &mut self.list {
            let Some(n) = table.creatures[c.kind].npc else {
                continue;
            };
            let fight = n.fight;
            c.fight_wait = c.fight_wait.saturating_sub(1);
            let Some(mut g) = c.guard else {
                continue;
            };
            let Some(t) = dwell_protocol::chopping::tool(g.weapon) else {
                continue;
            };
            let Some(w) = t.weapon else {
                continue;
            };
            let view = c.foe_player.filter(|_| c.foe_ticks > 0).and_then(foes);
            let Some(view) = view else {
                // Given up (or its foe gone): its block lowered, its swing dropped.
                c.foe_player = None;
                c.own_swing = None;
                if g.raised {
                    g.raised = false;
                    c.guard = Some(g);
                }
                continue;
            };
            c.foe = Some(view.position);
            let f = FaceFrame::at_height(c.position[1]);
            let (me, them) = (f.point(c.position), f.point(view.position));
            let dist = (them[0] - me[0]).hypot(them[2] - me[2]);
            // Its own swing on its way to its contact.
            if let Some(left) = c.own_swing {
                if left <= 1 {
                    c.own_swing = None;
                    let up = f64::from(table.creatures[c.kind].rows) * dwell_world::CELL_H;
                    let eye = f.point([me[0], me[1] + up * NPC_EYE_SHARE, me[2]]);
                    out.push(NpcSwing {
                        npc: c.id,
                        player: c.foe_player.expect("a foe"),
                        weapon: g.weapon,
                        vertical: g.vertical,
                        eye,
                    });
                } else {
                    c.own_swing = Some(left - 1);
                }
                c.guard = Some(g);
                continue;
            }
            match view.swing {
                Some((SwingPhase::Coming, strike_vertical, weapon)) if fight.raise_on_windup => {
                    if !g.raised {
                        g.vertical = match fight.stance {
                            FightStance::Counter => {
                                let theirs = dwell_protocol::chopping::tool(weapon)
                                    .and_then(|t| t.weapon)
                                    .map_or(w.guard, |x| x.guard);
                                w.guard.counter(strike_vertical, &theirs)
                            }
                            FightStance::Random => {
                                c.rng = mix(c.rng, [0; 3], 7);
                                unit(c.rng) < 0.5
                            }
                        };
                        g.raised = true;
                    }
                }
                Some((SwingPhase::Recovering, ..))
                    if fight.strike_back && c.fight_wait == 0 && dist <= fight.strike_within_m =>
                {
                    // An opening: it swings back, its block dropped for the swing.
                    let hz = f64::from(SIM_HZ);
                    let contact = ((t.swing.centre_s() * hz).ceil() as u32).max(1);
                    g.raised = false;
                    g.attack((t.swing_seconds * hz).ceil() as u32);
                    c.own_swing = Some(contact);
                    c.fight_wait = ((fight.cooldown_s.max(t.swing_seconds)) * hz).ceil() as u32;
                }
                Some((SwingPhase::Coming, ..)) => {}
                _ => g.raised = false,
            }
            c.guard = Some(g);
        }
        out
    }

    /// An NPC's swing was blocked: it may swing again only after `recoil_s`.
    pub fn recoil(&mut self, id: u32, recoil_s: f64) {
        if let Some(c) = self.get_mut(id) {
            let ticks = (recoil_s * f64::from(SIM_HZ)).ceil() as u32;
            c.fight_wait = c.fight_wait.max(ticks);
        }
    }

    /// The fighters' guards as others see them: each creature holding a weapon, its stance and
    /// whether its block is raised.
    pub fn guards(&self) -> Vec<(u32, bool, bool)> {
        self.list
            .iter()
            .filter_map(|c| c.guard.map(|g| (c.id, g.vertical, g.blocking())))
            .collect()
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
            if let Some(g) = &mut c.guard {
                g.attacking = g.attacking.saturating_sub(1);
            }
            // A hit's knockback slides it first, over ground it could walk (a wall, water or a
            // cliff's edge stops it), walking nothing meanwhile.
            let left = c.knock[0].hypot(c.knock[1]);
            if left > 1e-9 {
                let step = (KNOCK_SPEED_MPS * dt).min(left);
                let next = [
                    local[0] + c.knock[0] / left * step,
                    local[2] + c.knock[1] / left * step,
                ];
                match step_to(get, f, &def, local, next) {
                    Some(at) => {
                        c.position = f.point(at);
                        let k = (left - step) / left;
                        c.knock = [c.knock[0] * k, c.knock[1] * k];
                    }
                    None => c.knock = [0.0; 2],
                }
                c.speed = 0.0;
                continue;
            }
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
            match step_to(get, f, &def, local, next) {
                Some(at) => {
                    c.position = f.point(at);
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
        self.separate(get);
    }

    /// Keeps creatures' bodies apart (BUGS.md #80): two closer than their radii plus the data's
    /// `gap_m` steer apart at up to `push_mps`, and two overlapping are pushed straight out (a few
    /// passes, so a crowd settles), each move kept to ground it can stand on with its body clear.
    fn separate(&mut self, get: Get) {
        let table = self.table;
        let push = table.limits.push_mps / f64::from(SIM_HZ);
        let gap = table.limits.gap_m;
        for pass in 0..SEPARATE_PASSES {
            let n = self.list.len();
            let mut moved = vec![[0.0f64; 2]; n];
            let mut any = false;
            for i in 0..n {
                for j in i + 1..n {
                    let (a, b) = (&self.list[i], &self.list[j]);
                    let f = FaceFrame::at_height(a.position[1]);
                    if FaceFrame::at_height(b.position[1]) != f {
                        continue;
                    }
                    let (da, db) = (&table.creatures[a.kind], &table.creatures[b.kind]);
                    let (p, q) = (f.point(a.position), f.point(b.position));
                    // One on a ledge over the other: no contact.
                    if (p[1] - q[1]).abs() >= f64::from(da.rows.max(db.rows)) * dwell_world::CELL_H
                    {
                        continue;
                    }
                    let touch = da.body_radius_m + db.body_radius_m;
                    let (dx, dz) = (q[0] - p[0], q[2] - p[2]);
                    let d = dx.hypot(dz);
                    let hard = (touch - d) / 2.0;
                    let each = if pass == 0 {
                        hard.max(((touch + gap - d) / 2.0).min(push))
                    } else {
                        hard
                    };
                    if each <= 1e-9 {
                        continue;
                    }
                    // Right on top of each other: apart along a heading their ids give.
                    let (ux, uz) = if d > 1e-9 {
                        (dx / d, dz / d)
                    } else {
                        let a = unit(mix(u64::from(a.id), [b.id as i32, 0, 0], 0x5e9a))
                            * std::f64::consts::TAU;
                        (a.sin(), a.cos())
                    };
                    // A hair past touching, so the next pass finds them apart.
                    let each = each + if hard > 0.0 { 1e-6 } else { 0.0 };
                    moved[i] = [moved[i][0] - ux * each, moved[i][1] - uz * each];
                    moved[j] = [moved[j][0] + ux * each, moved[j][1] + uz * each];
                    any = true;
                }
            }
            if !any {
                return;
            }
            for (c, m) in self.list.iter_mut().zip(&moved) {
                if m[0] == 0.0 && m[1] == 0.0 {
                    continue;
                }
                let def = table.creatures[c.kind];
                let f = FaceFrame::at_height(c.position[1]);
                let local = f.point(c.position);
                if let Some(at) = step_to(get, f, &def, local, [local[0] + m[0], local[2] + m[1]]) {
                    c.position = f.point(at);
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

    /// The creatures a swing's head crosses (WORK_QUEUE item 35): `arc` its steps in the order the
    /// head sweeps them, each a direction (world, unit) from `eye` and how far the head reaches on
    /// it (the weapon's reach, or less where a solid block stops it). A creature is crossed when a
    /// step's segment comes within its kind's `hitRadiusM` of its body (a vertical line from its
    /// feet up its `rows`), on the eye's face; only kinds with health. At most `max`, each once,
    /// in the order the head meets them: (creature id, step).
    pub fn crossed(&self, eye: [f64; 3], arc: &[([f64; 3], f64)], max: usize) -> Vec<(u32, u32)> {
        let f = FaceFrame::at_height(eye[1]);
        let e = f.point(eye);
        let mut out: Vec<(u32, u32)> = Vec::new();
        for (step, &(dir, reach)) in arc.iter().enumerate() {
            let d = f.vector(dir);
            let mut met: Vec<(f64, u32)> = Vec::new();
            for c in &self.list {
                let def = &self.table.creatures[c.kind];
                if def.health <= 0.0 || out.iter().any(|o| o.0 == c.id) {
                    continue;
                }
                if FaceFrame::at_height(c.position[1]).sign() != f.sign() {
                    continue;
                }
                let feet = f.point(c.position);
                let top = feet[1] + f64::from(def.rows) * dwell_world::CELL_H;
                if let Some(t) = segment_meets_post(e, d, reach, feet, top, def.hit_radius_m) {
                    met.push((t, c.id));
                }
            }
            met.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, id) in met {
                if out.len() < max {
                    out.push((id, step as u32));
                }
            }
            if out.len() >= max {
                break;
            }
        }
        out
    }

    /// A weapon's strike on creature `id` from a striker at `from` (world eye): if it holds a raised
    /// sword facing the striker (within the striker's weapon's `frontDeg` of its facing), the
    /// blades clash by angle ([`GuardDef::clash`]): blocked, nothing lands; a glance or a connect
    /// lands its share of the damage ([`Self::hurt`]). None: no such creature or one that can't be
    /// hurt.
    pub fn strike(
        &mut self,
        id: u32,
        weapon: &WeaponDef,
        vertical: bool,
        from: [f64; 3],
    ) -> Option<Strike> {
        let table = self.table;
        let c = self.get(id)?;
        if table.creatures[c.kind].health <= 0.0 {
            return None;
        }
        let clash = c.guard.filter(Guard::blocking).and_then(|g| {
            let theirs = dwell_protocol::chopping::tool(g.weapon)?.weapon?;
            let f = FaceFrame::at_height(c.position[1]);
            let (me, them) = (f.point(c.position), f.point(from));
            let to = (them[2] - me[2]).atan2(them[0] - me[0]);
            let facing = c.yaw.to_radians().cos().atan2(c.yaw.to_radians().sin());
            let off = (to - facing + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            (off.abs().to_degrees() <= weapon.guard.front_deg / 2.0)
                .then(|| weapon.guard.clash(vertical, &theirs.guard, g.vertical))
        });
        // An NPC turns on whoever struck it (its `fightBack`), blocked or not.
        self.attacked(id, from);
        let share = clash.map_or(1.0, |k| weapon.guard.damage_share(k));
        let hurt = if share > 0.0 {
            self.hurt(id, weapon.damage * share, weapon.knockback_m * share, from)
        } else {
            None
        };
        Some(Strike { clash, hurt })
    }

    /// Sets creature `id`'s sword (None: it holds none). False: no such creature.
    pub fn set_guard(&mut self, id: u32, guard: Option<Guard>) -> bool {
        self.get_mut(id).map(|c| c.guard = guard).is_some()
    }

    /// A hit on creature `id` (a weapon's): `damage` from its health, a `knockback_m` slide away
    /// from `from` (world; the striker) along the ground, and `from` kept for its mind to react
    /// to. Returns whether it died (its health gone): it is then removed (the clients are told).
    /// None: no such creature, or one that can't be hurt.
    pub fn hurt(&mut self, id: u32, damage: f64, knockback_m: f64, from: [f64; 3]) -> Option<Hurt> {
        let table = self.table;
        let c = self.get_mut(id)?;
        let def = &table.creatures[c.kind];
        if def.health <= 0.0 {
            return None;
        }
        c.health -= damage;
        let f = FaceFrame::at_height(c.position[1]);
        let (me, them) = (f.point(c.position), f.point(from));
        let (dx, dz) = (me[0] - them[0], me[2] - them[2]);
        let d = dx.hypot(dz);
        let (ax, az) = if d > 1e-6 {
            (dx / d, dz / d)
        } else {
            (0.0, 1.0)
        };
        c.knock = [ax * knockback_m, az * knockback_m];
        c.hurt_from = Some(from);
        let hurt = Hurt {
            kind: c.kind,
            position: c.position,
            killed: c.health <= 1e-9,
        };
        if hurt.killed {
            self.remove(id);
        }
        Some(hurt)
    }
}

/// What a strike did (`Creatures::strike`): the clash with a raised blade, if it met one, and the
/// hurt that landed (None: blocked).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strike {
    pub clash: Option<Clash>,
    pub hurt: Option<Hurt>,
}

/// What a hit did (`Creatures::hurt`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hurt {
    /// The kind hit (its `drops` when killed).
    pub kind: usize,
    /// Where it stood (world feet).
    pub position: [f64; 3],
    pub killed: bool,
}

/// The sword a creature of `def` holds from its spawn: an NPC whose `held` is a weapon, in its level
/// stance and lowered (the defender API; WORK_QUEUE items 35 and 38).
pub fn guard_of(def: &CreatureDef) -> Option<Guard> {
    let held = def.npc?.held?;
    dwell_protocol::chopping::tool(held)
        .filter(|t| t.is_weapon())
        .map(|t| Guard::holding(t.name, false))
}

/// How fast a knockback slides a creature (m/s).
pub const KNOCK_SPEED_MPS: f64 = 6.0;

/// Where along the segment from `e` along unit `d` for `reach` metres (face-local) it first comes
/// within `r` of the vertical post at `feet`'s x, z from `feet`'s y to `top`: its distance from
/// `e`, or None. Sampled finely along the segment (a swing's step is short).
fn segment_meets_post(
    e: [f64; 3],
    d: [f64; 3],
    reach: f64,
    feet: [f64; 3],
    top: f64,
    r: f64,
) -> Option<f64> {
    const N: u32 = 24;
    (0..=N).find_map(|i| {
        let t = reach * f64::from(i) / f64::from(N);
        let p = [e[0] + d[0] * t, e[1] + d[1] * t, e[2] + d[2] * t];
        let dy = if p[1] < feet[1] {
            feet[1] - p[1]
        } else if p[1] > top {
            p[1] - top
        } else {
            0.0
        };
        let h = (p[0] - feet[0]).hypot(p[2] - feet[2]);
        (h * h + dy * dy <= r * r).then_some(t)
    })
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
        fn a_creature_s_body_keeps_out_of_a_wall_it_walks_into(f: FaceFrame) {
            // BUGS.md #81: a wolf walked partly inside a cliff wall: only the cell under its
            // centre was checked, so its body went up to its radius into the stone. The wall's
            // face is at x = -5 (cells x <= -6, rows up to 15).
            let g = world(f);
            let mut cs = Creatures::new(Table::SHIPPED);
            let wolf = put(&mut cs, f, "dwell:wolf", -2.0, 0.5, 270.0);
            let doe = put(&mut cs, f, "dwell:doe", -2.0, 6.5, 270.0);
            // Straight at the wall, and slanting into it (a slide along it is fine).
            cs.get_mut(wolf).unwrap().target = Some([-12.0, 0.5]);
            cs.get_mut(doe).unwrap().target = Some([-12.0, 2.5]);
            let mut least = f64::MAX;
            for _ in 0..60 * 8 {
                cs.step(&g);
                for c in &cs.list {
                    let r = cs.table.creatures[c.kind].body_radius_m;
                    least = least.min(f.point(c.position)[0] - r + 5.0);
                }
            }
            assert!(least >= -1e-9, "a body {:.3} m inside the wall", -least);
            // It still walks right up to it.
            let x = f.point(cs.get(wolf).unwrap().position)[0];
            assert!(x < -4.0, "it walked up to the wall: x {x}");
        }
    );

    /// A creature of `name` (shipped table) at face-local (x, floor, z), facing yaw.
    fn put(cs: &mut Creatures, f: FaceFrame, name: &str, x: f64, z: f64, yaw: f64) -> u32 {
        let k = cs.kind_of(name).unwrap();
        cs.add_driven(k, f.point([x, m_of_row(10), z]), yaw, 1)
            .unwrap()
    }

    /// An eye 1.6 m over the floor at face-local (0.5, 0.5), and a level arc toward face-local +x
    /// of the sword's reach (no blocks in the way).
    fn eye_and_arc(f: FaceFrame) -> ([f64; 3], Vec<([f64; 3], f64)>) {
        let t = dwell_protocol::chopping::tool("dwell:sword").unwrap();
        let arc = dwell_world::swing::Arc::of(t);
        let eye = f.point([0.5, m_of_row(10) + 1.6, 0.5]);
        let dirs = dwell_world::swing::arc_dirs(f, f.vector([1.0, 0.0, 0.0]), false, &arc);
        (eye, dirs.into_iter().map(|d| (d, arc.reach_m)).collect())
    }

    both_faces!(
        fn a_swing_crosses_the_deer_in_its_arc_and_not_one_outside_it(f: FaceFrame) {
            let mut cs = Creatures::new(Table::SHIPPED);
            let near = put(&mut cs, f, "dwell:doe", 2.0, 0.5, 0.0);
            let behind = put(&mut cs, f, "dwell:doe", -2.5, 0.5, 0.0);
            let far = put(&mut cs, f, "dwell:doe", 6.0, 0.5, 0.0);
            let (eye, arc) = eye_and_arc(f);
            let hit: Vec<u32> = cs.crossed(eye, &arc, 4).iter().map(|h| h.0).collect();
            assert_eq!(
                hit,
                [near],
                "only the deer in the arc ({behind}, {far} not)"
            );
            // Short of it (a block stopping the head at 0.5 m): nothing.
            let short: Vec<_> = arc.iter().map(|&(d, _)| (d, 0.5)).collect();
            assert!(cs.crossed(eye, &short, 4).is_empty());
        }
    );

    both_faces!(
        fn a_strike_hurts_knocks_back_and_kills_dropping_its_data_drops(f: FaceFrame) {
            let g = world(f);
            let mut cs = Creatures::new(Table::SHIPPED);
            let doe = put(&mut cs, f, "dwell:doe", 2.0, 0.5, 0.0);
            let w = dwell_protocol::chopping::tool("dwell:sword")
                .unwrap()
                .weapon
                .unwrap();
            let whole = cs.table.creatures[cs.get(doe).unwrap().kind].health;
            let eye = f.point([0.5, m_of_row(10) + 1.6, 0.5]);
            let s = cs.strike(doe, &w, false, eye).unwrap();
            assert_eq!(s.clash, None);
            assert!(!s.hurt.unwrap().killed);
            assert!((cs.get(doe).unwrap().health - (whole - w.damage)).abs() < 1e-9);
            assert!(
                cs.get_mut(doe).unwrap().take_hurt().is_some(),
                "its mind is told"
            );
            // The knockback slides it away from the striker along the ground.
            for _ in 0..60 {
                cs.step(&g);
            }
            let l = f.point(cs.get(doe).unwrap().position);
            assert!(
                (l[0] - (2.0 + w.knockback_m)).abs() < 0.05,
                "pushed +x: {l:?}"
            );
            assert!((l[1] - m_of_row(10)).abs() < 1e-9);
            // Struck until its health is gone: killed, removed, its kind's drops by data.
            let mut last = None;
            while cs.get(doe).is_some() {
                last = cs.strike(doe, &w, false, eye).unwrap().hurt;
            }
            let hurt = last.unwrap();
            assert!(hurt.killed);
            assert_eq!(
                cs.table.creatures[hurt.kind].drops,
                &[("dwell:raw_meat", 1, 2)]
            );
            assert!(cs.removed.contains(&doe));
        }
    );

    both_faces!(
        fn a_fixture_creature_by_data_alone_takes_a_fixture_weapons_damage(f: FaceFrame) {
            let mut cs = Creatures::new(Table::FIXTURE);
            let k = cs.kind_of("test:walker").unwrap();
            let id = cs
                .add_driven(k, f.point([2.0, m_of_row(10), 0.5]), 0.0, 1)
                .unwrap();
            let club = dwell_protocol::chopping::fixture::TOOLS[0].weapon.unwrap();
            let eye = f.point([0.5, m_of_row(10) + 1.6, 0.5]);
            assert!(
                !cs.strike(id, &club, false, eye)
                    .unwrap()
                    .hurt
                    .unwrap()
                    .killed
            );
            assert!((cs.get(id).unwrap().health - 2.0).abs() < 1e-9, "5 - 3");
            let h = cs.strike(id, &club, false, eye).unwrap().hurt.unwrap();
            assert!(h.killed);
            assert_eq!(cs.table.creatures[h.kind].drops, &[("dwell:stick", 2, 2)]);
            // The leader has no health: never crossed, never struck.
            let leader = cs.kind_of("test:leader").unwrap();
            let l = cs
                .add_driven(leader, f.point([2.0, m_of_row(10), 0.5]), 0.0, 2)
                .unwrap();
            assert!(cs.strike(l, &club, false, eye).is_none());
        }
    );

    // Owner, 2026-11-12: the Wii Resort model. A defender (an NPC's API) holding its sword raised
    // facing the striker blocks a strike across its blade, not one along it; attacking drops the
    // block; facing away it guards nothing.
    both_faces!(
        fn a_raised_blade_blocks_across_not_along_and_attacking_drops_it(f: FaceFrame) {
            let w = dwell_protocol::chopping::tool("dwell:sword")
                .unwrap()
                .weapon
                .unwrap();
            let eye = f.point([0.5, m_of_row(10) + 1.6, 0.5]);
            let health = |cs: &Creatures, id| cs.get(id).unwrap().health;
            // Facing the striker (toward face-local -x: yaw -90, its facing (sin, cos)).
            let mut cs = Creatures::new(Table::SHIPPED);
            let d = put(&mut cs, f, "dwell:wolf", 2.0, 0.5, -90.0);
            let whole = health(&cs, d);
            let mut guard = Guard::holding("dwell:sword", true);
            guard.raised = true;
            cs.set_guard(d, Some(guard));
            // A level slash across the upright blade: blocked, nothing lands.
            let s = cs.strike(d, &w, false, eye).unwrap();
            assert_eq!(s.clash, Some(Clash::Blocked));
            assert!(s.hurt.is_none() && health(&cs, d) == whole);
            assert_eq!(cs.get(d).unwrap().knock, [0.0; 2], "no knockback");
            // A vertical slash along it: connects, the whole damage.
            let s = cs.strike(d, &w, true, eye).unwrap();
            assert_eq!(s.clash, Some(Clash::Connected));
            assert!((health(&cs, d) - (whole - w.damage)).abs() < 1e-9);
            // Attacking drops the block: a level slash lands while it swings, and is blocked again
            // once its swing is over.
            let g = world(f);
            cs.get_mut(d).unwrap().guard.as_mut().unwrap().attack(3);
            assert_eq!(cs.strike(d, &w, false, eye).unwrap().clash, None);
            for _ in 0..3 {
                cs.step(&g);
            }
            cs.get_mut(d).unwrap().knock = [0.0; 2];
            assert_eq!(
                cs.strike(d, &w, false, eye).unwrap().clash,
                Some(Clash::Blocked)
            );
            // Turned away from the striker its blade guards nothing.
            cs.get_mut(d).unwrap().yaw = 90.0;
            assert_eq!(cs.strike(d, &w, false, eye).unwrap().clash, None);
        }
    );

    // Owner, 2026-11-12: "Wandering NPCs will get swords." The villager holds its data's sword as
    // its guard from its spawn (lowered: it connects), turns on its striker, and blocks once raised.
    both_faces!(
        fn a_villager_holds_its_sword_as_its_guard_and_turns_on_its_striker(f: FaceFrame) {
            let w = dwell_protocol::chopping::tool("dwell:sword")
                .unwrap()
                .weapon
                .unwrap();
            let mut cs = Creatures::new(Table::SHIPPED);
            let v = put(&mut cs, f, "dwell:villager", 2.0, 0.5, -90.0);
            let g = cs.get(v).unwrap().guard.expect("its data's sword");
            assert_eq!(g.weapon, "dwell:sword");
            assert!(!g.blocking(), "lowered");
            let eye = f.point([0.5, m_of_row(10) + 1.6, 0.5]);
            let s = cs.strike(v, &w, false, eye).unwrap();
            assert!(s.clash.is_none() && s.hurt.is_some());
            assert!(cs.get(v).unwrap().foe.is_some(), "fights back");
            cs.get_mut(v).unwrap().guard.as_mut().unwrap().raised = true;
            let s = cs.strike(v, &w, true, eye).unwrap();
            assert_eq!(
                s.clash,
                Some(Clash::Blocked),
                "vertical across its level blade"
            );
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
        // The game's villager: the Peasant, five parts (the hat its own: owner 2026-11-12), sent with its kind and variant.
        let t = Table::SHIPPED;
        let v = t.npc("dwell:villager").expect("the villager");
        let parts: Vec<_> = v.colours.iter().map(|c| c.part).collect();
        assert_eq!(parts, ["shirt", "trousers", "skin", "hair", "hat"]);
        let kind = t
            .creatures
            .iter()
            .position(|c| c.name == "dwell:villager")
            .unwrap();
        assert_eq!(t.creatures[kind].model, "peasant");
        let cs = npcs(FaceFrame::A, [0.5, 0.5]);
        let net = cs.list[0].npc_net(&cs.table, false).expect("an NPC part");
        assert_eq!(
            (net.kind.as_str(), net.variant),
            ("test:person", cs.list[0].variant)
        );
        assert!(
            spawn(FaceFrame::A).list[0]
                .npc_net(&Table::FIXTURE, false)
                .is_none()
        );
    }
    // WORK_QUEUE item 35: an NPC mid-swing is sent in its strike's pose to a client with
    // `npcs.swings`, and in its idle pose to one without (older clients reject poses past 2).
    #[test]
    fn an_npc_mid_swing_is_sent_striking_only_to_a_capable_client() {
        let mut cs = npcs(FaceFrame::A, [0.5, 0.5]);
        let c = &mut cs.list[0];
        if c.guard.is_none() {
            c.guard = Some(Guard::holding("dwell:sword", false));
        }
        let idle = c.npc_net(&cs.table, true).unwrap().pose;
        assert!(usize::from(idle) < NPC_POSES.len());
        let c = &mut cs.list[0];
        c.strike_now(30);
        assert_eq!(c.npc_net(&cs.table, true).unwrap().pose, 3, "level strike");
        assert_eq!(
            c.npc_net(&cs.table, false).unwrap().pose,
            idle,
            "older client"
        );
        c.guard.as_mut().unwrap().vertical = true;
        assert_eq!(
            c.npc_net(&cs.table, true).unwrap().pose,
            4,
            "vertical strike"
        );
    }
}
