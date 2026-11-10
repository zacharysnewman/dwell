//! Creatures (docs/adr/0073-creatures.md): animals as light server entities, not Rapier bodies.
//! They are spawned in groups near players by the data's rules (shared/creatures/creatures.json,
//! rolled once per chunk column and face from the world seed), wander (idle, then walk to a random
//! nearby point on walkable ground, stopping at water and cliffs), and are removed once no player
//! is within `despawn_m`. Everything is face-local through `FaceFrame`: face B mirrors face A.

use std::collections::{HashMap, HashSet};

use dwell_blocks::{StateId, VoxelShape, find_block, try_info};
use dwell_protocol::{CreatureNet, SIM_HZ};
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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnRule {
    pub creature: usize,
    pub on: &'static [&'static str],
    pub chance: f64,
    pub group: [u32; 2],
    /// Other creatures a group may hold in place of the rule's own (a herd's stag).
    pub mix: &'static [MixPart],
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
fn feet_y(f: FaceFrame, [x, y, z]: [i32; 3]) -> f64 {
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
}

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
        }
    }

    /// Rolls the spawn columns within `spawn_chunks` of each player (world feet) not yet rolled;
    /// a column whose cells aren't loaded waits. Deterministic: the roll, the cell and the group
    /// hash the world seed and the face-local column.
    pub fn spawn_near(&mut self, players: &[[f64; 3]], get: Get, seed: u64) {
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
                    if self.roll_column(get, seed, f, key, row) {
                        self.rolled.insert(key);
                    }
                }
            }
        }
    }

    /// One column's roll; false when its cells aren't loaded yet.
    fn roll_column(&mut self, get: Get, seed: u64, f: FaceFrame, key: [i32; 3], row: i32) -> bool {
        let col = [key[0], 0, key[2]];
        for (i, rule) in self.table.rules.iter().enumerate() {
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
                    rng: mix(seed, [x + dx, yy, z + dz], 99),
                };
                c.idle = c.idle_ticks(&def);
                self.list.push(c);
            }
            return true;
        }
        true
    }

    /// One tick: each creature idles or walks a step; a step onto ground it can't stand on (water,
    /// a cliff's edge, a wall, unloaded) ends the walk.
    pub fn step(&mut self, get: Get) {
        let dt = 1.0 / f64::from(SIM_HZ);
        for c in &mut self.list {
            let def = self.table.creatures[c.kind];
            let f = FaceFrame::at_height(c.position[1]);
            let local = f.point(c.position);
            if c.idle > 0 {
                c.idle -= 1;
                c.speed = 0.0;
                if c.idle == 0 {
                    let a = c.roll() * std::f64::consts::TAU;
                    let d = def.wander_m * (0.3 + 0.7 * c.roll());
                    c.target = Some([local[0] + d * a.sin(), local[2] + d * a.cos()]);
                }
                continue;
            }
            let Some([tx, tz]) = c.target else {
                c.idle = c.idle_ticks(&def).max(1);
                continue;
            };
            let (dx, dz) = (tx - local[0], tz - local[2]);
            let dist = (dx * dx + dz * dz).sqrt();
            let step = def.walk_speed * dt;
            if dist <= step {
                c.target = None;
                c.idle = c.idle_ticks(&def).max(1);
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
                    c.speed = def.walk_speed;
                }
                None => {
                    c.target = None;
                    c.idle = c.idle_ticks(&def).max(1);
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
            let keep = near(&c.position);
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
            let names: Vec<_> = cs.list.iter().map(|c| cs.table.creatures[c.kind].name).collect();
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
        assert!((0.25..0.55).contains(&share), "a stag in {share} of the herds");
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
}
