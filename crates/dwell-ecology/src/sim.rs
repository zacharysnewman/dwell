//! The far ecology's step (docs/ECOLOGY.md §2): every group of every species, over whole days.
//!
//! A step of `days` is cut into steps of at most `island.stepDays`; within one, each rate is
//! integrated in closed form over the step (regrowth, hunger and energy relax exponentially, growth
//! and loss multiply by `exp(rate · dt)`), so a big step is stable and close to many small ones.
//! Everything is deterministic from the seed: the only randomness is a hash of (seed, group, day
//! bucket), no state-carried generator, so a fast-forward of 100 days draws what 100 single days do.

use crate::data::{Data, Diet, SpeciesDef};
use crate::island::Island;

/// What a group is doing (its last step's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Resting,
    Grazing,
    Moving,
    Fleeing,
    Hunting,
    /// A hunter looking for prey it cannot sense.
    Roaming,
    /// A hungry hunter leaving for the island's biggest prey group, members leaving.
    Dispersing,
}

/// A herd or pack: one record for the whole group, its averages.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: u32,
    /// An index into `Data::species`.
    pub species: usize,
    /// Face-local metres on the face's island.
    pub x: f64,
    pub z: f64,
    /// Head (an average over the step: fractional between births and deaths).
    pub size: f64,
    /// 0 fed, 1 none.
    pub hunger: f64,
    /// 1 rested.
    pub energy: f64,
    pub activity: Activity,
}

/// One face's ecology: the groups and the cells' forage.
#[derive(Clone, Debug, PartialEq)]
pub struct Ecosystem {
    pub seed: u64,
    pub day: f64,
    /// Each island cell's forage now (deer-days).
    pub grazing: Vec<f64>,
    pub groups: Vec<Group>,
    pub next_id: u32,
}

/// How fast energy follows the activity (days).
const ENERGY_DAYS: f64 = 2.0;
/// The days a roaming hunter keeps one heading.
const ROAM_DAYS: f64 = 10.0;
/// The days of feed a grazer looks for in a cell.
const FEED_DAYS: f64 = 10.0;

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn hash(seed: u64, a: u64, b: u64) -> u64 {
    splitmix(splitmix(splitmix(seed) ^ a) ^ b)
}

/// [0, 1) from a hash.
fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// `value` relaxed toward `target` over `dt` with time constant `tau`.
fn relax(value: f64, target: f64, dt: f64, tau: f64) -> f64 {
    target + (value - target) * (-dt / tau).exp()
}

fn energy_target(a: Activity) -> f64 {
    match a {
        Activity::Resting | Activity::Grazing => 1.0,
        Activity::Moving | Activity::Hunting | Activity::Roaming => 0.6,
        Activity::Fleeing | Activity::Dispersing => 0.3,
    }
}

/// The step-start view of a group the others decide by.
#[derive(Clone, Copy)]
struct Seen {
    species: usize,
    x: f64,
    z: f64,
}

impl Ecosystem {
    /// A new ecology on `island`: each species' start groups on cells drawn from the seed (a
    /// grazer's on cells with forage), every cell's forage full.
    pub fn new(island: &Island, data: &Data, seed: u64) -> Self {
        let mut eco = Self {
            seed,
            day: 0.0,
            grazing: island.grazing_max.clone(),
            groups: Vec::new(),
            next_id: 0,
        };
        let land: Vec<usize> = (0..island.walkable.len())
            .filter(|&c| island.walkable[c])
            .collect();
        let pasture: Vec<usize> = land
            .iter()
            .copied()
            .filter(|&c| island.grazing_max[c] > 0.0)
            .collect();
        for (s, sp) in data.species.iter().enumerate() {
            let cells = match sp.diet {
                Diet::Graze { .. } if !pasture.is_empty() => &pasture,
                _ => &land,
            };
            if cells.is_empty() {
                continue;
            }
            for k in 0..sp.start_groups {
                let h = hash(seed, s as u64, k as u64);
                let (x, z) = island.centre(cells[(h % cells.len() as u64) as usize]);
                eco.push(s, x, z, sp.start_size, 0.2, 1.0);
            }
        }
        eco
    }

    fn push(&mut self, species: usize, x: f64, z: f64, size: f64, hunger: f64, energy: f64) {
        self.groups.push(Group {
            id: self.next_id,
            species,
            x,
            z,
            size,
            hunger,
            energy,
            activity: Activity::Resting,
        });
        self.next_id += 1;
    }

    /// The head of `species` on the island.
    pub fn population(&self, species: usize) -> f64 {
        self.groups
            .iter()
            .filter(|g| g.species == species)
            .map(|g| g.size)
            .sum()
    }

    /// The groups of `species` on the island.
    pub fn group_count(&self, species: usize) -> usize {
        self.groups.iter().filter(|g| g.species == species).count()
    }

    /// A hash of the whole state (determinism checks).
    pub fn digest(&self) -> u64 {
        let mut h = splitmix(self.seed ^ self.day.to_bits());
        for g in &self.groups {
            for v in [g.x, g.z, g.size, g.hunger, g.energy] {
                h = splitmix(h ^ v.to_bits());
            }
            h = splitmix(h ^ u64::from(g.id) ^ ((g.species as u64) << 32));
        }
        for v in &self.grazing {
            h = splitmix(h ^ v.to_bits());
        }
        h
    }

    /// Advances the ecology `days`, in steps of at most `island.stepDays`.
    pub fn step(&mut self, island: &Island, data: &Data, days: f64) {
        let mut left = days;
        while left > 1e-9 {
            let dt = left.min(data.island.step_days);
            self.step_once(island, data, dt);
            left -= dt;
        }
    }

    fn step_once(&mut self, island: &Island, data: &Data, dt: f64) {
        let regrow = (-data.regrow_per_day * dt).exp();
        for (g, &max) in self.grazing.iter_mut().zip(&island.grazing_max) {
            *g = max - (max - *g) * regrow;
        }
        let species = data.species.len();
        let totals: Vec<f64> = (0..species).map(|s| self.population(s)).collect();
        let capacity: Vec<f64> = (0..species)
            .map(|s| island.carrying_capacity(data, s))
            .collect();
        let seen: Vec<Seen> = self
            .groups
            .iter()
            .map(|g| Seen {
                species: g.species,
                x: g.x,
                z: g.z,
            })
            .collect();
        for i in 0..self.groups.len() {
            let sp = &data.species[self.groups[i].species];
            let fed = match sp.diet {
                Diet::Graze { per_head_per_day } => {
                    self.graze(island, sp, &seen, i, per_head_per_day, dt)
                }
                Diet::Prey { .. } => self.hunt(island, sp, i, dt),
            };
            // The island's bound (owner, 2026-11-11: no runaway populations): breeding slows to
            // nothing as the species' total nears its carrying capacity; a hunter's is also its
            // prey's head now over the prey it needs a head.
            let s = self.groups[i].species;
            let mut room = capacity[s];
            if let Diet::Prey {
                species: prey,
                prey_per_head,
                ..
            } = sp.diet
            {
                room = room.min(prey.iter().map(|&p| totals[p]).sum::<f64>() / prey_per_head);
            }
            let crowd = if room > 0.0 {
                (1.0 - totals[s] / room).max(0.0)
            } else {
                0.0
            };
            let g = &mut self.groups[i];
            g.hunger = relax(g.hunger, 1.0 - fed, dt, sp.hunger_days).clamp(0.0, 1.0);
            g.energy = relax(g.energy, energy_target(g.activity), dt, ENERGY_DAYS);
            let well_fed = if sp.breed_below_hunger > 0.0 {
                (1.0 - g.hunger / sp.breed_below_hunger).max(0.0)
            } else {
                0.0
            };
            let mut rate = sp.breed_per_day * well_fed * crowd;
            if g.hunger > sp.starve_above_hunger && sp.starve_above_hunger < 1.0 {
                rate -= sp.starve_per_day * (g.hunger - sp.starve_above_hunger)
                    / (1.0 - sp.starve_above_hunger);
            }
            if g.activity == Activity::Dispersing
                && let Some(d) = sp.disperse
            {
                rate -= d.leave_per_day;
            }
            g.size *= (rate * dt).exp();
        }
        // No floor (owner, 2026-11-11: "systemically resistant to being wiped out, not immune
        // from it"): a group under its `dieBelow` is gone.
        self.groups
            .retain(|g| g.size >= data.species[g.species].group.die_below);
        self.merge(data);
        self.split(island, data);
        self.day += dt;
    }

    /// A grazer picks the cell in its sense with the most feed and the least danger, walks toward
    /// it and eats from the cell it ends in; returns the share of its need it ate.
    fn graze(
        &mut self,
        island: &Island,
        sp: &SpeciesDef,
        seen: &[Seen],
        i: usize,
        per_head: f64,
        dt: f64,
    ) -> f64 {
        let g = &self.groups[i];
        let at = (g.x, g.z);
        let here = island.cell_at(g.x, g.z);
        let need = (g.size * per_head * FEED_DAYS).max(1e-9);
        let (flee_from, flee_r): (&[usize], f64) =
            sp.flee.map_or((&[], 0.0), |f| (f.from, f.radius_m));
        let threats: Vec<(f64, f64)> = seen
            .iter()
            .filter(|o| flee_from.contains(&o.species))
            .map(|o| (o.x, o.z))
            .filter(|&p| dist(p, at) <= flee_r + sp.sense_radius_m)
            .collect();
        let danger = |p: (f64, f64)| {
            threats
                .iter()
                .map(|&t| ((flee_r - dist(t, p)) / flee_r.max(1e-9)).max(0.0))
                .fold(0.0, f64::max)
        };
        let fleeing = danger(at) > 0.0;
        let r = (sp.sense_radius_m / island.cell).ceil() as i64;
        let (ci, cj) = (
            ((g.x - island.x0) / island.cell + 0.5).floor() as i64,
            ((g.z - island.x0) / island.cell + 0.5).floor() as i64,
        );
        // Every walkable cell in its sense, scored: feed, danger, distance.
        let mut cells: Vec<(f64, usize, f64)> = Vec::new();
        for dj in -r..=r {
            for di in -r..=r {
                let (a, b) = (ci + di, cj + dj);
                if a < 0 || b < 0 || a >= island.n as i64 || b >= island.n as i64 {
                    continue;
                }
                let c = b as usize * island.n + a as usize;
                if !island.walkable[c] {
                    continue;
                }
                let centre = island.centre(c);
                let d = dist(at, centre);
                if d > sp.sense_radius_m + island.cell * 0.5 {
                    continue;
                }
                let food = (self.grazing[c] / need).min(1.0);
                let stay = if Some(c) == here { 0.05 } else { 0.0 };
                // A refuge takes the danger away (a threatened herd makes for the crags).
                let threat = 2.0 * danger(centre) * (1.0 - island.refuge[c]);
                let score = food - threat - 0.02 * d / island.cell + stay;
                cells.push((score, c, d));
            }
        }
        // Best first (ties: the grid's order, so deterministic).
        cells.sort_by(|a, b| b.0.total_cmp(&a.0));
        let target = match cells.first() {
            Some(&(_, c, _)) if Some(c) != here => island.centre(c),
            _ => at,
        };
        let activity = if fleeing {
            Activity::Fleeing
        } else if target == at {
            Activity::Grazing
        } else {
            Activity::Moving
        };
        let budget = sp.speed_m_per_day * dt * (0.5 + 0.5 * g.energy);
        let (x, z) = walk(island, at, target, budget);
        let demand = g.size * per_head * dt;
        let g = &mut self.groups[i];
        g.x = x;
        g.z = z;
        g.activity = activity;
        // It eats where it ends, then, over a long step, from the other cells it could reach in
        // its sense, best first: a herd grazing five days roams as five single days would.
        let end = island.cell_at(x, z);
        let mut eaten = 0.0;
        let order = end.into_iter().chain(
            cells
                .iter()
                .filter(|e| e.2 <= budget && Some(e.1) != end)
                .map(|e| e.1),
        );
        for c in order {
            if eaten >= demand {
                break;
            }
            let take = self.grazing[c].min(demand - eaten);
            self.grazing[c] -= take;
            eaten += take;
        }
        if demand > 0.0 { eaten / demand } else { 1.0 }
    }

    /// A hunter goes for the nearest prey group it senses (or, starving, the island's biggest) and
    /// kills within reach: at most `killsPerHeadPerDay` a head, times the Holling type III share of
    /// the prey head it can find in its sense outside refuges ([`Data::catch_share`]), never more
    /// than the target's head outside its refuge. Returns the share of its need it ate.
    fn hunt(&mut self, island: &Island, sp: &SpeciesDef, i: usize, dt: f64) -> f64 {
        let Diet::Prey {
            species: prey,
            kills_per_head_per_day,
            need_per_head_per_day,
            reach_m,
            ..
        } = sp.diet
        else {
            return 0.0;
        };
        let g = &self.groups[i];
        let at = (g.x, g.z);
        let huntable = |o: &Group| prey.contains(&o.species) && o.size > 0.0;
        // The head of a prey group a hunter can take: what is outside its cell's refuge.
        let exposed = |o: &Group| o.size * (1.0 - island.refuge_at(o.x, o.z));
        let dispersing = sp.disperse.filter(|d| g.hunger > d.above_hunger);
        let mut factor = 1.0;
        let (target, activity) = if let Some(d) = dispersing {
            factor = d.speed_factor;
            let biggest = self
                .groups
                .iter()
                .filter(|o| huntable(o))
                .max_by(|a, b| a.size.total_cmp(&b.size));
            (biggest.map_or(at, |o| (o.x, o.z)), Activity::Dispersing)
        } else {
            let nearest = self
                .groups
                .iter()
                .filter(|o| huntable(o))
                .map(|o| (dist(at, (o.x, o.z)), (o.x, o.z)))
                .filter(|&(d, _)| d <= sp.sense_radius_m)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            match nearest {
                Some((d, _)) if d <= reach_m * 0.5 => (at, Activity::Hunting),
                Some((_, p)) => (p, Activity::Hunting),
                None if g.hunger < 0.3 => (at, Activity::Resting),
                None => {
                    let h = hash(self.seed, u64::from(g.id), (self.day / ROAM_DAYS) as u64);
                    let a = unit(h) * std::f64::consts::TAU;
                    let far = sp.speed_m_per_day * dt;
                    (
                        (at.0 + a.cos() * far, at.1 + a.sin() * far),
                        Activity::Roaming,
                    )
                }
            }
        };
        let budget = sp.speed_m_per_day * dt * (0.5 + 0.5 * g.energy) * factor;
        let (x, z) = walk(island, at, target, budget);
        let size = g.size;
        let g = &mut self.groups[i];
        g.x = x;
        g.z = z;
        g.activity = activity;
        let demand = size * need_per_head_per_day * dt;
        let near = self
            .groups
            .iter()
            .enumerate()
            .filter(|(_, o)| huntable(o))
            .map(|(j, o)| (dist((x, z), (o.x, o.z)), j))
            .filter(|&(d, _)| d <= reach_m)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, j)) = near else {
            return 0.0;
        };
        let found: f64 = self
            .groups
            .iter()
            .filter(|o| huntable(o) && dist((x, z), (o.x, o.z)) <= sp.sense_radius_m.max(reach_m))
            .map(exposed)
            .sum();
        let kill = demand
            .min(size * kills_per_head_per_day * Data::catch_share(sp.diet, found) * dt)
            .min(exposed(&self.groups[j]))
            .max(0.0);
        self.groups[j].size -= kill;
        if kill > 0.0 && dispersing.is_none() {
            self.groups[i].activity = Activity::Hunting;
        }
        if demand > 0.0 { kill / demand } else { 1.0 }
    }

    /// Small groups join a group of their kind nearby, if together they fit the cap.
    #[allow(clippy::needless_range_loop)] // pairs of `self.groups` and their `gone` marks
    fn merge(&mut self, data: &Data) {
        let n = self.groups.len();
        let mut gone = vec![false; n];
        for i in 0..n {
            if gone[i] {
                continue;
            }
            for j in i + 1..n {
                if gone[j] || self.groups[j].species != self.groups[i].species {
                    continue;
                }
                let rules = data.species[self.groups[i].species].group;
                let (a, b) = (&self.groups[i], &self.groups[j]);
                if dist((a.x, a.z), (b.x, b.z)) > rules.merge_radius_m
                    || (a.size >= rules.merge_below && b.size >= rules.merge_below)
                    || a.size + b.size > rules.cap
                {
                    continue;
                }
                let (sa, sb) = (a.size, b.size);
                let (hb, eb) = (b.hunger, b.energy);
                let a = &mut self.groups[i];
                a.hunger = (a.hunger * sa + hb * sb) / (sa + sb);
                a.energy = (a.energy * sa + eb * sb) / (sa + sb);
                a.size = sa + sb;
                gone[j] = true;
            }
        }
        let mut k = 0;
        self.groups.retain(|_| {
            k += 1;
            !gone[k - 1]
        });
    }

    /// A group above its cap splits in two (each half lives: cap ≥ 2 × `dieBelow`); at the island's
    /// most groups it stops at the cap instead.
    fn split(&mut self, island: &Island, data: &Data) {
        let mut i = 0;
        while i < self.groups.len() {
            let s = self.groups[i].species;
            let rules = data.species[s].group;
            while self.groups[i].size > rules.cap {
                if self.group_count(s) >= rules.max_groups {
                    self.groups[i].size = rules.cap;
                    break;
                }
                let g = &self.groups[i];
                let half = g.size / 2.0;
                let a = unit(hash(self.seed, u64::from(g.id), u64::from(self.next_id)))
                    * std::f64::consts::TAU;
                let p = (
                    g.x + a.cos() * island.cell * 0.5,
                    g.z + a.sin() * island.cell * 0.5,
                );
                let (x, z) = if island.is_walkable(p.0, p.1) {
                    p
                } else {
                    (g.x, g.z)
                };
                let (hunger, energy) = (g.hunger, g.energy);
                self.groups[i].size = half;
                self.push(s, x, z, half, hunger, energy);
            }
            i += 1;
        }
    }
}

/// Walks from `from` toward `to` up to `budget` m, stopping at the last walkable point on the way
/// (water stops it).
fn walk(island: &Island, from: (f64, f64), to: (f64, f64), budget: f64) -> (f64, f64) {
    let d = dist(from, to);
    if d < 1e-9 || budget <= 0.0 {
        return from;
    }
    let travel = budget.min(d);
    let steps = (travel / (island.cell * 0.5)).ceil().max(1.0) as usize;
    let (ux, uz) = ((to.0 - from.0) / d, (to.1 - from.1) / d);
    let mut best = from;
    for k in 1..=steps {
        let t = travel * k as f64 / steps as f64;
        let p = (from.0 + ux * t, from.1 + uz * t);
        if !island.is_walkable(p.0, p.1) {
            break;
        }
        best = p;
    }
    best
}

/// Advances `eco` on `island` by `days` (the far ecology's one entry point).
pub fn step(island: &Island, data: &Data, eco: &mut Ecosystem, days: f64) {
    eco.step(island, data, days);
}
