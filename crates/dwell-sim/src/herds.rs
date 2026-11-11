//! The near ecology (docs/ECOLOGY.md §8, WORK_QUEUE item 11 slice 3): a herd or pack record a
//! player comes within `near.spawnM` of becomes its individuals — the species' `near.creature`
//! (ADR 0073's creatures, driven: they walk only where they are sent) around the record's place,
//! with its hunger and energy — and the record leaves the far step. Once no player on its face is
//! within `near.foldM` of them (the hysteresis), they fold back into the record: its size the
//! survivors, its hunger and energy their averages, its place their centroid.
//!
//! Near players the individuals follow the owner's mockup (docs/proposals/ECOSYSTEM_MOCKUP_NOTES.md)
//! as data (each species' `near`): every `near.thinkS` each one, in priority order, flees what its
//! species flees, sleeps (when tired, or at night by chance), eats when hungry (a grazer the island
//! cell's forage, and sometimes the real block its data names — the only world change, near players
//! only; a hunter chases the nearest prey at a sprint and kills it within reach, the kill feeding its
//! packmates), mates when fed and rested (a young up to the group's `cap`), else keeps to its group or
//! wanders. Hunger near players never kills (starving is the far step's, over days); kills do.
//!
//! Everything is face-local through `FaceFrame` (each group is on one face and sees only its face's
//! players and animals), and deterministic: the only randomness is each member's own hash chain,
//! seeded from (world seed, record id, member).

use crate::creatures::{Creatures, Get, feet_row, feet_y, group_kinds};
use dwell_ecology::{Activity, Data, Diet, Ecosystem, Group, Island, NearDef};
use dwell_protocol::SIM_HZ;
use dwell_world::{Face, FaceFrame, cell_of_pos, m_of_row};

/// The faces, by index (an ecology's `ecos[k]` and `islands[k]` are `FACES[k]`'s).
pub const FACES: [Face; 2] = [Face::A, Face::B];

/// The rows searched up and down for an individual's feet at spawn.
const SPAWN_ROWS: i32 = 48;
/// The rows searched up and down for a next column's feet when measuring the ground's slope.
const SLOPE_ROWS: i32 = 8;

/// What an individual is doing (the mockup's states).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mind {
    LookingForFood,
    Eating,
    Sleeping,
    Fleeing,
    Chasing,
    Mating,
    LookingForCompany,
    Wandering,
}

impl Mind {
    pub fn name(self) -> &'static str {
        match self {
            Mind::LookingForFood => "looking for food",
            Mind::Eating => "eating",
            Mind::Sleeping => "sleeping",
            Mind::Fleeing => "fleeing",
            Mind::Chasing => "chasing",
            Mind::Mating => "mating",
            Mind::LookingForCompany => "looking for company",
            Mind::Wandering => "wandering",
        }
    }
}

/// One individual: its creature and its needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    /// Its creature's id ([`Creatures`]).
    pub creature: u32,
    /// 0 fed, 1 none.
    pub hunger: f64,
    /// 1 rested.
    pub energy: f64,
    pub mind: Mind,
    /// Face-local x, z at the last decision.
    pub x: f64,
    pub z: f64,
    /// Seconds left asleep.
    sleep_s: f64,
    rng: u64,
}

/// A herd or pack near players: its record (out of the far step) and its individuals.
#[derive(Clone, Debug, PartialEq)]
pub struct NearGroup {
    /// An index into [`FACES`].
    pub face: usize,
    pub record: Group,
    pub members: Vec<Member>,
    /// Members killed and young born since it came near.
    pub deaths: u32,
    pub births: u32,
    /// The head in its far summary as last written ([`Ecosystem::near`]); a far hunter's kill
    /// lowers the summary, and the loss is read back as deaths.
    summary_size: f64,
    /// A far kill's share of a member not yet taken.
    far_loss: f64,
}

impl NearGroup {
    /// Its record as it would fold back now (None: none left).
    pub fn folded(&self) -> Option<Group> {
        let n = self.members.len();
        if n == 0 {
            return None;
        }
        let k = n as f64;
        let avg = |v: fn(&Member) -> f64| self.members.iter().map(v).sum::<f64>() / k;
        Some(Group {
            size: k,
            hunger: avg(|m| m.hunger).clamp(0.0, 1.0),
            energy: avg(|m| m.energy).clamp(0.0, 1.0),
            x: avg(|m| m.x),
            z: avg(|m| m.z),
            activity: Activity::Resting,
            ..self.record.clone()
        })
    }

    /// Its summary for the far step: its record as it would fold back, doing what most of its
    /// members do.
    fn summary(&self) -> Option<Group> {
        let mut g = self.folded()?;
        let count = |m: Mind| self.members.iter().filter(|o| o.mind == m).count();
        g.activity = [
            (Mind::Fleeing, Activity::Fleeing),
            (Mind::Chasing, Activity::Hunting),
            (Mind::Eating, Activity::Grazing),
            (Mind::Sleeping, Activity::Resting),
        ]
        .into_iter()
        .filter(|&(m, _)| count(m) * 2 > self.members.len())
        .map(|(_, a)| a)
        .next()
        .unwrap_or(Activity::Moving);
        Some(g)
    }
}

/// What the near ecology sees of the world each tick.
pub struct World<'a> {
    pub data: &'a Data,
    /// Both faces' islands and ecosystems, by [`FACES`] index.
    pub islands: &'a [Island],
    pub ecos: &'a mut [Ecosystem],
    /// The living players' feet (world).
    pub players: &'a [[f64; 3]],
    pub get: Get<'a>,
    /// Whether it is night on each face.
    pub night: [bool; 2],
    pub seed: u64,
}

/// The herds and packs near players.
#[derive(Debug, Default)]
pub struct Near {
    pub groups: Vec<NearGroup>,
    ticks: u64,
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// [0, 1) from a hash.
fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// The feet row of an individual `rows` tall in face `k`'s face-local column `col` (searched
/// around `row`) if it is the ecology's ground there: its island cell walkable land, a dry full
/// block under open cells, and no steeper than the data's `near.maxSlopeDeg` to any next column
/// with ground. `Err`: not loaded (wait).
fn ecology_ground(
    w: &World,
    k: usize,
    f: FaceFrame,
    col: [i32; 2],
    row: i32,
    rows: i32,
) -> Result<Option<i32>, ()> {
    if (w.get)(f.cell([col[0], row, col[1]])).is_none() {
        return Err(());
    }
    let (cx, cz) = (f64::from(col[0]) + 0.5, f64::from(col[1]) + 0.5);
    if !w.islands.get(k).is_some_and(|i| i.is_walkable(cx, cz)) {
        return Ok(None);
    }
    let Some(y) = feet_row(w.get, f, col, row, SPAWN_ROWS, SPAWN_ROWS, rows) else {
        return Ok(None);
    };
    let most = w.data.near.max_slope_deg.to_radians().tan();
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let next = [col[0] + dx, col[1] + dz];
        if let Some(ny) = feet_row(w.get, f, next, y, SLOPE_ROWS, SLOPE_ROWS, rows)
            && (m_of_row(ny) - m_of_row(y)).abs() > most + 1e-9
        {
            return Ok(None);
        }
    }
    Ok(Some(y))
}

/// Up to `n` of the nearest loaded columns within the data's `near.searchM` of face-local
/// `centre` that are the ecology's ground ([`ecology_ground`]), nearest first, as face-local
/// feet points at their columns' centres.
fn nearest_grounds(
    w: &World,
    k: usize,
    f: FaceFrame,
    centre: (f64, f64),
    row: i32,
    rows: i32,
    n: usize,
) -> Vec<[f64; 3]> {
    let r = w.data.near.search_m;
    let (ci, cj) = (centre.0.floor() as i32, centre.1.floor() as i32);
    let span = r.ceil() as i32;
    let mut cols: Vec<(f64, [i32; 2])> = Vec::new();
    for dj in -span..=span {
        for di in -span..=span {
            let c = [ci + di, cj + dj];
            let d = (f64::from(c[0]) + 0.5 - centre.0).hypot(f64::from(c[1]) + 0.5 - centre.1);
            if d <= r {
                cols.push((d, c));
            }
        }
    }
    // Nearest first (ties in the grid's order: deterministic).
    cols.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    for (_, c) in cols {
        if out.len() >= n {
            break;
        }
        if let Ok(Some(y)) = ecology_ground(w, k, f, c, row, rows) {
            out.push([
                f64::from(c[0]) + 0.5,
                feet_y(f, [c[0], y, c[1]]),
                f64::from(c[1]) + 0.5,
            ]);
        }
    }
    out
}

fn roll(rng: &mut u64) -> f64 {
    *rng = splitmix(*rng);
    (*rng >> 11) as f64 / (1u64 << 53) as f64
}

fn face_index(f: FaceFrame) -> usize {
    usize::from(f.is_b())
}

/// The players on face `k`, face-local.
fn players_on(players: &[[f64; 3]], k: usize) -> Vec<[f64; 3]> {
    players
        .iter()
        .map(|&p| (FaceFrame::at_height(p[1]), p))
        .filter(|(f, _)| face_index(*f) == k)
        .map(|(f, p)| f.point(p))
        .collect()
}

fn nearest_player(players: &[[f64; 3]], x: f64, z: f64) -> Option<([f64; 3], f64)> {
    players
        .iter()
        .map(|p| (*p, (p[0] - x).hypot(p[2] - z)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// One member as the others see it at a decision's start.
#[derive(Clone, Copy)]
struct Seen {
    group: usize,
    member: usize,
    species: usize,
    face: usize,
    x: f64,
    z: f64,
    mind: Mind,
    hunger: f64,
}

/// A decision's outcome for one member's creature.
enum Go {
    Stop,
    To([f64; 2], f64),
    Wander,
}

impl Near {
    /// One tick: every `near.thinkS`, groups far from every player fold back, records near one
    /// become individuals, and every individual decides. Returns the cells whose block was eaten
    /// (the server writes them, if the block is still there).
    pub fn tick(&mut self, w: &mut World, creatures: &mut Creatures) -> Vec<[i32; 3]> {
        let every = ((w.data.near.think_s * f64::from(SIM_HZ)).round() as u64).max(1);
        let now = self.ticks;
        self.ticks += 1;
        if !now.is_multiple_of(every) {
            return Vec::new();
        }
        self.refresh(creatures);
        self.far_losses(w, creatures);
        self.fold_far(w, creatures);
        self.spawn_near(w, creatures);
        let eaten = self.think(w, creatures, every as f64 / f64::from(SIM_HZ));
        self.summarise(w);
        eaten
    }

    /// A far hunter's kills on a near group's summary since the last decision become deaths among
    /// its members (the last first), a member for each whole head.
    fn far_losses(&mut self, w: &World, creatures: &mut Creatures) {
        for g in &mut self.groups {
            let Some(r) = w
                .ecos
                .get(g.face)
                .and_then(|e| e.near.iter().find(|r| r.id == g.record.id))
            else {
                continue;
            };
            g.far_loss += (g.summary_size - r.size).max(0.0);
            while g.far_loss >= 1.0 - 1e-9 {
                g.far_loss -= 1.0;
                let Some(m) = g.members.pop() else {
                    break;
                };
                creatures.remove(m.creature);
                g.deaths += 1;
            }
        }
        self.groups.retain(|g| !g.members.is_empty());
    }

    /// Writes each face's summaries of the groups near players ([`Ecosystem::near`]), so the far
    /// step still sees them.
    fn summarise(&mut self, w: &mut World) {
        for (k, eco) in w.ecos.iter_mut().enumerate() {
            eco.near.clear();
            for g in self.groups.iter_mut().filter(|g| g.face == k) {
                if let Some(r) = g.summary() {
                    g.summary_size = r.size;
                    eco.near.push(r);
                }
            }
        }
    }

    /// Every member's place from its creature.
    fn refresh(&mut self, creatures: &Creatures) {
        for g in &mut self.groups {
            let f = FaceFrame::of(FACES[g.face]);
            for m in &mut g.members {
                if let Some(c) = creatures.get(m.creature) {
                    let l = f.point(c.position);
                    (m.x, m.z) = (l[0], l[2]);
                }
            }
        }
    }

    fn fold_far(&mut self, w: &mut World, creatures: &mut Creatures) {
        let fold = w.data.near.fold_m;
        let mut i = 0;
        while i < self.groups.len() {
            let g = &self.groups[i];
            let on = players_on(w.players, g.face);
            let near = g
                .members
                .iter()
                .any(|m| nearest_player(&on, m.x, m.z).is_some_and(|(_, d)| d <= fold));
            if near {
                i += 1;
                continue;
            }
            let g = self.groups.remove(i);
            for m in &g.members {
                creatures.remove(m.creature);
            }
            if let Some(r) = g.folded() {
                let eco = &mut w.ecos[g.face];
                let at = eco.groups.partition_point(|o| o.id < r.id);
                eco.groups.insert(at, r);
            }
        }
    }

    fn spawn_near(&mut self, w: &mut World, creatures: &mut Creatures) {
        let spawn = w.data.near.spawn_m;
        for (k, &face) in FACES.iter().enumerate().take(w.ecos.len()) {
            let on = players_on(w.players, k);
            if on.is_empty() {
                continue;
            }
            let f = FaceFrame::of(face);
            let mut i = 0;
            while i < w.ecos[k].groups.len() {
                let r = w.ecos[k].groups[i].clone();
                let made = (|| {
                    let def = w.data.species[r.species].near?;
                    let kind = creatures.kind_of(def.creature)?;
                    let (p, d) = nearest_player(&on, r.x, r.z)?;
                    if d > spawn {
                        return None;
                    }
                    let n = r.size.round().max(1.0) as usize;
                    if creatures.list.len() + n > creatures.table.limits.max {
                        return None; // wait for room: the whole group or none
                    }
                    // The herd's mix (does, a stag, …): the creature rule's for its creature, by
                    // data (shared/creatures/creatures.json `mix`), rolled from the record.
                    let kinds = match creatures.table.rules.iter().find(|q| q.creature == kind) {
                        Some(rule) => group_kinds(rule, n as u32, |j| {
                            unit(splitmix(
                                w.seed ^ splitmix(u64::from(r.id) ^ 0x6d69_7800 ^ j as u64),
                            ))
                        }),
                        None => vec![kind; n],
                    };
                    let rows = kinds
                        .iter()
                        .map(|&q| creatures.table.creatures[q].rows)
                        .max()
                        .unwrap_or(1);
                    let row = cell_of_pos(p)[1];
                    let mut places = Vec::with_capacity(n);
                    let mut fallback: Option<Vec<[f64; 3]>> = None;
                    for m in 0..n {
                        let mut rng = splitmix(w.seed ^ splitmix(u64::from(r.id) << 16 | m as u64));
                        let mut at = None;
                        for _ in 0..8 {
                            let a = roll(&mut rng) * std::f64::consts::TAU;
                            let rad = 1.0 + 3.0 * roll(&mut rng);
                            let (x, z) = (r.x + rad * a.sin(), r.z + rad * a.cos());
                            let col = [x.floor() as i32, z.floor() as i32];
                            // Not loaded: wait.
                            if let Some(y) = ecology_ground(w, k, f, col, row, rows).ok()? {
                                at = Some([x, feet_y(f, [col[0], y, col[1]]), z]);
                                break;
                            }
                        }
                        // None of its tries is the ecology's ground (water, too steep): the
                        // nearest columns that are, one each, within the data's `searchM`.
                        if at.is_none() {
                            let list = fallback.get_or_insert_with(|| {
                                nearest_grounds(w, k, f, (r.x, r.z), row, rows, n)
                            });
                            if !list.is_empty() {
                                at = Some(list.remove(0));
                            }
                        }
                        // Keep the head: a member with no ground of its own stands by the last.
                        let at = at.or_else(|| places.last().map(|(p, _)| *p))?;
                        places.push((at, rng));
                    }
                    let mut members = Vec::with_capacity(n);
                    for ((at, mut rng), &kind) in places.into_iter().zip(&kinds) {
                        let yaw = roll(&mut rng) * 360.0;
                        let id = creatures.add_driven(kind, f.point(at), yaw, rng)?;
                        members.push(Member {
                            creature: id,
                            hunger: r.hunger,
                            energy: r.energy,
                            mind: Mind::LookingForCompany,
                            x: at[0],
                            z: at[2],
                            sleep_s: 0.0,
                            rng,
                        });
                    }
                    Some(members)
                })();
                match made {
                    Some(members) => {
                        w.ecos[k].groups.remove(i);
                        self.groups.push(NearGroup {
                            face: k,
                            record: r,
                            summary_size: members.len() as f64,
                            members,
                            deaths: 0,
                            births: 0,
                            far_loss: 0.0,
                        });
                    }
                    None => i += 1,
                }
            }
        }
    }

    fn think(&mut self, w: &mut World, creatures: &mut Creatures, dt: f64) -> Vec<[i32; 3]> {
        let mut seen = Vec::new();
        for (gi, g) in self.groups.iter().enumerate() {
            for (mi, m) in g.members.iter().enumerate() {
                seen.push(Seen {
                    group: gi,
                    member: mi,
                    species: g.record.species,
                    face: g.face,
                    x: m.x,
                    z: m.z,
                    mind: m.mind,
                    hunger: m.hunger,
                });
            }
        }
        let mut dead: Vec<(usize, usize)> = Vec::new();
        let mut young: Vec<(usize, [f64; 2], usize, u64, u32)> = Vec::new();
        let mut eaten = Vec::new();
        let is_dead = |dead: &[(usize, usize)], gi: usize, mi: usize| dead.contains(&(gi, mi));
        for s in 0..seen.len() {
            let me = seen[s];
            let (gi, mi) = (me.group, me.member);
            if is_dead(&dead, gi, mi) {
                continue;
            }
            let sp = &w.data.species[me.species];
            let Some(def) = sp.near else {
                continue;
            };
            let f = FaceFrame::of(FACES[me.face]);
            let cap = sp.group.cap as usize;
            let nearest = |dead: &[(usize, usize)], pick: &dyn Fn(&Seen) -> bool| {
                seen.iter()
                    .filter(|o| o.face == me.face && !(o.group == gi && o.member == mi))
                    .filter(|o| !is_dead(dead, o.group, o.member) && pick(o))
                    .map(|o| (*o, (o.x - me.x).hypot(o.z - me.z)))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
            };
            let night = w.night[me.face];
            let pending = young.iter().filter(|y| y.0 == gi).count();
            let size =
                self.groups[gi].members.len() + pending - dead.iter().filter(|d| d.0 == gi).count();
            let go = {
                let m = &mut self.groups[gi].members[mi];
                m.hunger = (m.hunger + def.hunger_per_s * dt).min(1.0);
                if m.mind != Mind::Sleeping {
                    m.energy = (m.energy - def.energy_per_s * dt).max(0.0);
                }
                decide(
                    DecideIn {
                        def: &def,
                        diet: sp.diet,
                        flee_from: sp.flee.map(|fl| fl.from).unwrap_or(&[]),
                        me,
                        night,
                        dt,
                        room: size < cap,
                    },
                    m,
                    &|pick| nearest(&dead, pick),
                )
            };
            // The decision's effects on the world, the others and the creature.
            let go = match go {
                Act::Go(g) => g,
                Act::Graze => {
                    let g = def.graze.expect("a grazer");
                    let eco = &mut w.ecos[me.face];
                    let m = &mut self.groups[gi].members[mi];
                    match w.islands[me.face].cell_at(me.x, me.z) {
                        Some(c) if f64::from(eco.grazing[c]) >= g.forage_per_s * dt => {
                            eco.grazing[c] =
                                (f64::from(eco.grazing[c]) - g.forage_per_s * dt) as f32;
                            m.hunger = (m.hunger - g.eat_per_s * dt).max(0.0);
                            m.mind = Mind::Eating;
                            if g.block.is_some() && roll(&mut m.rng) < g.block_chance_per_s * dt {
                                let id = m.creature;
                                eaten.extend(eat_block(w.get, f, me, g.block, creatures, id));
                            }
                            Go::Stop
                        }
                        _ => {
                            m.mind = Mind::LookingForFood;
                            Go::Wander
                        }
                    }
                }
                Act::Kill(prey) => {
                    dead.push((prey.group, prey.member));
                    let h = def.hunt.expect("a hunter");
                    let feeds = h.feeds_pack;
                    let g = &mut self.groups[gi];
                    for (k, o) in g.members.iter_mut().enumerate() {
                        if k == mi {
                            o.hunger = 0.0;
                            o.mind = Mind::Sleeping;
                            o.sleep_s = def.sleep.min_s;
                        } else if (o.x - me.x).hypot(o.z - me.z) <= h.feeds_pack_m {
                            o.hunger *= 1.0 - feeds;
                        }
                    }
                    Go::Stop
                }
                Act::Young(partner) => {
                    let g = &mut self.groups[gi];
                    g.members[partner.member].hunger =
                        (g.members[partner.member].hunger + def.mate.cost).min(1.0);
                    let m = &mut g.members[mi];
                    m.hunger = (m.hunger + def.mate.cost).min(1.0);
                    let rng = splitmix(m.rng ^ 0x0079_6f75_6e67);
                    // The young are the species' own creature (the herd's mix is its start's).
                    let kind = creatures
                        .kind_of(def.creature)
                        .or_else(|| creatures.get(m.creature).map(|c| c.kind))
                        .unwrap_or(0);
                    young.push((gi, [me.x + 0.6, me.z + 0.6], kind, rng, m.creature));
                    Go::Stop
                }
            };
            let m = &mut self.groups[gi].members[mi];
            if let Some(c) = creatures.get_mut(m.creature) {
                match go {
                    Go::Stop => {
                        c.target = None;
                        c.pace = 1.0;
                    }
                    Go::To(t, pace) => {
                        c.target = Some(t);
                        c.pace = pace;
                    }
                    Go::Wander => {
                        c.pace = 1.0;
                        if c.target.is_none() && roll(&mut m.rng) < 0.3 {
                            let a = roll(&mut m.rng) * std::f64::consts::TAU;
                            let d = def.wander_m * (0.3 + 0.7 * roll(&mut m.rng));
                            c.target = Some([me.x + d * a.sin(), me.z + d * a.cos()]);
                        }
                    }
                }
            }
        }
        // The young, by their parents (on the parent's ground).
        for (gi, at, kind, mut rng, parent) in young {
            let face = self.groups[gi].face;
            let f = FaceFrame::of(FACES[face]);
            let Some(feet) = creatures.get(parent).map(|c| f.point(c.position)[1]) else {
                continue;
            };
            let yaw = roll(&mut rng) * 360.0;
            let Some(id) = creatures.add_driven(kind, f.point([at[0], feet, at[1]]), yaw, rng)
            else {
                continue;
            };
            let g = &mut self.groups[gi];
            g.births += 1;
            g.members.push(Member {
                creature: id,
                hunger: 0.5,
                energy: 1.0,
                mind: Mind::LookingForCompany,
                x: at[0],
                z: at[1],
                sleep_s: 0.0,
                rng,
            });
        }
        // The killed, last to first so the indices hold.
        dead.sort_unstable();
        for &(gi, mi) in dead.iter().rev() {
            let g = &mut self.groups[gi];
            let m = g.members.remove(mi);
            g.deaths += 1;
            creatures.remove(m.creature);
        }
        // A group with none left died out near players: its record is gone.
        self.groups.retain(|g| !g.members.is_empty());
        eaten
    }

    /// The head near players of `species`, by face index.
    pub fn head(&self, face: usize, species: usize) -> usize {
        self.groups
            .iter()
            .filter(|g| g.face == face && g.record.species == species)
            .map(|g| g.members.len())
            .sum()
    }

    /// A hash of the whole near state with the creatures' places (determinism checks).
    pub fn digest(&self, creatures: &Creatures) -> u64 {
        let mut h = splitmix(self.ticks);
        for g in &self.groups {
            h = splitmix(h ^ u64::from(g.record.id) ^ (u64::from(g.deaths) << 32));
            for m in &g.members {
                h = splitmix(h ^ m.hunger.to_bits() ^ m.energy.to_bits().rotate_left(17));
                if let Some(c) = creatures.get(m.creature) {
                    for v in c.position {
                        h = splitmix(h ^ v.to_bits());
                    }
                }
            }
        }
        h
    }
}

/// The block a grazer eats by its feet (its own column, then the 8 around it), if one stands
/// there.
fn eat_block(
    get: Get,
    f: FaceFrame,
    me: Seen,
    block: Option<&'static str>,
    creatures: &Creatures,
    id: u32,
) -> Option<[i32; 3]> {
    let b = dwell_blocks::find_block(block?)?;
    let feet = creatures.get(id).map(|c| f.point(c.position))?;
    let row = cell_of_pos(feet)[1];
    let (x, z) = (me.x.floor() as i32, me.z.floor() as i32);
    for (dx, dz) in [
        (0, 0),
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (-1, 1),
        (1, -1),
        (-1, -1),
    ] {
        let c = f.cell([x + dx, row, z + dz]);
        if get(c).is_some_and(|m| b.states().contains(&usize::from(m))) {
            return Some(c);
        }
    }
    None
}

struct DecideIn<'a> {
    def: &'a NearDef,
    diet: Diet,
    flee_from: &'a [usize],
    me: Seen,
    night: bool,
    dt: f64,
    /// Whether the group has room for a young.
    room: bool,
}

enum Act {
    Go(Go),
    /// Eat the island cell's forage here (or look for some).
    Graze,
    Kill(Seen),
    Young(Seen),
}

/// The nearest other living member on the decider's face a filter accepts, with its distance.
type Nearest<'a> = &'a dyn Fn(&dyn Fn(&Seen) -> bool) -> Option<(Seen, f64)>;

/// One member's decision, in the mockup's priority order. `nearest(pick)`: the nearest other
/// living member on its face that `pick` accepts, with its distance.
fn decide(i: DecideIn, m: &mut Member, nearest: Nearest) -> Act {
    let (def, me) = (i.def, i.me);
    // 1. Flee what its species flees, within the radius.
    if let Some(fl) = def.flee
        && let Some((o, d)) = nearest(&|o| i.flee_from.contains(&o.species))
        && d <= fl.radius_m
    {
        m.mind = Mind::Fleeing;
        m.sleep_s = 0.0;
        let (ax, az) = if d > 1e-6 {
            ((me.x - o.x) / d, (me.z - o.z) / d)
        } else {
            (1.0, 0.0)
        };
        let run = fl.radius_m;
        return Act::Go(Go::To([me.x + ax * run, me.z + az * run], fl.pace));
    }
    // 2. Asleep: rest until rested (or too hungry) and the nap's least is over.
    let sl = def.sleep;
    if m.mind == Mind::Sleeping {
        m.energy = (m.energy + sl.gain_per_s * i.dt).min(1.0);
        m.sleep_s -= i.dt;
        if m.sleep_s > 0.0 || (m.energy < sl.wake_above_energy && m.hunger <= sl.wake_above_hunger)
        {
            return Act::Go(Go::Stop);
        }
        m.mind = Mind::LookingForCompany;
    }
    // 3. Fall asleep: tired, or at night by chance while not too hungry.
    if m.energy < sl.below_energy
        || (i.night
            && m.hunger < sl.night_below_hunger
            && roll(&mut m.rng) < sl.night_chance_per_s * i.dt)
    {
        m.mind = Mind::Sleeping;
        m.sleep_s = sl.min_s;
        return Act::Go(Go::Stop);
    }
    // 4. Food.
    if let Some(g) = def.graze {
        let hungry =
            m.hunger > g.above_hunger || (m.mind == Mind::Eating && m.hunger > g.full_below);
        if hungry {
            return Act::Graze;
        }
    }
    if let (Some(h), Diet::Prey { species, .. }) = (def.hunt, i.diet)
        && m.hunger > h.above_hunger
    {
        return match nearest(&|o| species.contains(&o.species)) {
            Some((o, d)) if d <= h.reach_m => Act::Kill(o),
            Some((o, d)) if d <= h.sense_m => {
                m.mind = Mind::Chasing;
                Act::Go(Go::To([o.x, o.z], h.sprint))
            }
            _ => {
                m.mind = Mind::LookingForFood;
                Act::Go(Go::Wander)
            }
        };
    }
    // 5. Mate: fed and rested, a fed groupmate near, room in the group.
    let mt = def.mate;
    if i.room && m.hunger <= mt.below_hunger && m.energy >= mt.above_energy {
        let partner = nearest(&|o| {
            o.group == me.group
                && o.hunger <= mt.below_hunger
                && !matches!(o.mind, Mind::Sleeping | Mind::Fleeing)
        });
        if let Some((o, d)) = partner.filter(|p| p.1 <= mt.radius_m) {
            m.mind = Mind::Mating;
            if d > mt.reach_m {
                return Act::Go(Go::To([o.x, o.z], 1.0));
            }
            if roll(&mut m.rng) < mt.chance {
                return Act::Young(o);
            }
            m.energy = (m.energy - mt.tired).max(0.0);
            return Act::Go(Go::Stop);
        }
    }
    // 6. Company: back toward the nearest groupmate when it is farther than the data's, else
    //    wander.
    if let Some((o, d)) = nearest(&|o| o.group == me.group)
        && d > def.company_m
    {
        m.mind = Mind::LookingForCompany;
        return Act::Go(Go::To([o.x, o.z], 1.0));
    }
    m.mind = Mind::Wandering;
    Act::Go(Go::Wander)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creatures::Table;
    use dwell_blocks::{StateId, find_block};
    use dwell_ecology::{DATA, Ground};
    use dwell_world::{both_faces, m_of_row};

    /// The top row of a floor at face-local column (x, z).
    type Floor = fn(i32, i32) -> i32;

    /// The stone floor: row 9 everywhere (feet at row 10).
    fn flat(_: i32, _: i32) -> i32 {
        9
    }

    /// Stairs rising west (x < 0) 2 rows (1 m) a column, steeper than the data's 40°; flat east.
    fn stairs(x: i32, _: i32) -> i32 {
        9 + 2 * (-x).max(0)
    }

    /// A stone floor (face-local row 9, feet at row 10) over x, z in [-64, 64), tall grass on it.
    fn world(f: FaceFrame) -> impl Fn([i32; 3]) -> Option<StateId> {
        world_on(f, flat)
    }

    /// A stone floor whose top row is `floor`'s, over x, z in [-64, 64), tall grass on it.
    fn world_on(f: FaceFrame, floor: Floor) -> impl Fn([i32; 3]) -> Option<StateId> {
        let stone = find_block("dwell:stone").unwrap().first_state;
        let grass = find_block("dwell:tall_grass").unwrap().first_state;
        move |c: [i32; 3]| {
            let [x, y, z] = f.cell(c);
            if !(-64..64).contains(&x) || !(-64..64).contains(&z) {
                return None;
            }
            let top = floor(x, z);
            Some(if y <= top {
                stone
            } else if y == top + 1 && top == 9 {
                grass
            } else {
                0
            })
        }
    }

    fn species(name: &str) -> usize {
        DATA.find(name).unwrap()
    }

    struct Scene {
        f: FaceFrame,
        data: Data,
        near: Near,
        creatures: Creatures,
        islands: Vec<Island>,
        ecos: Vec<Ecosystem>,
        seed: u64,
        eaten: Vec<[i32; 3]>,
        night: bool,
        floor: Floor,
    }

    impl Scene {
        /// Groups `(species, x, z, size, hunger)` on face `f`'s ecosystem (the other face's none).
        fn new(f: FaceFrame, groups: &[(&str, f64, f64, f64, f64)], seed: u64) -> Self {
            let isl = Island::from_ground(&DATA, 4, -300.0, |_, _| Ground::Land {
                height_m: 10.0,
                slope_deg: 1.0,
            });
            let mut ecos: Vec<Ecosystem> = (0..2)
                .map(|_| {
                    let mut e = Ecosystem::new(&isl, &DATA, seed);
                    e.groups.clear();
                    e
                })
                .collect();
            let k = face_index(f);
            for (id, &(name, x, z, size, hunger)) in groups.iter().enumerate() {
                ecos[k].groups.push(Group {
                    id: id as u32,
                    species: species(name),
                    x,
                    z,
                    size,
                    hunger,
                    energy: 1.0,
                    activity: Activity::Resting,
                });
            }
            Self {
                f,
                data: DATA,
                near: Near::default(),
                creatures: Creatures::new(Table::SHIPPED),
                islands: vec![isl.clone(), isl],
                ecos,
                seed,
                eaten: Vec::new(),
                night: false,
                floor: flat,
            }
        }

        fn player(&self, x: f64, z: f64) -> [f64; 3] {
            self.f.point([x, m_of_row(10), z])
        }

        /// Runs `secs` with the players at face-local `(x, z)`s; `each` sees every decision.
        fn run(&mut self, at: &[(f64, f64)], secs: f64, mut each: impl FnMut(&Near)) {
            let g = world_on(self.f, self.floor);
            let players: Vec<_> = at.iter().map(|&(x, z)| self.player(x, z)).collect();
            for t in 0..(secs * f64::from(SIM_HZ)) as u64 {
                let mut w = World {
                    data: &self.data,
                    islands: &self.islands,
                    ecos: &mut self.ecos,
                    players: &players,
                    get: &g,
                    night: [self.night; 2],
                    seed: self.seed,
                };
                self.eaten
                    .extend(self.near.tick(&mut w, &mut self.creatures));
                self.creatures.step(&g);
                if t % 30 == 0 {
                    each(&self.near);
                }
            }
        }

        fn k(&self) -> usize {
            face_index(self.f)
        }
    }

    both_faces!(
        fn a_herd_becomes_its_deer_near_a_player_and_folds_back_keeping_its_head(f: FaceFrame) {
            let mut s = Scene::new(f, &[("dwell:deer", 5.0, 5.0, 5.0, 0.6)], 7);
            s.run(&[(0.0, 0.0)], 1.0, |_| {});
            assert_eq!(s.near.groups.len(), 1, "the herd came near");
            assert!(
                s.ecos[s.k()].groups.is_empty(),
                "its record left the far step"
            );
            assert_eq!(s.creatures.list.len(), 5, "five deer");
            for c in &s.creatures.list {
                let l = f.point(c.position);
                assert!(
                    c.driven && (l[1] - m_of_row(10)).abs() < 1e-9,
                    "on the floor: {l:?}"
                );
                // Does, sometimes with a stag (the creature rule's mix).
                let name = s.creatures.table.creatures[c.kind].name;
                assert!(["dwell:doe", "dwell:deer"].contains(&name), "{name}");
            }
            let m = &s.near.groups[0].members[0];
            assert!(
                (m.hunger - 0.6).abs() < 0.1,
                "the record's hunger: {}",
                m.hunger
            );
            s.run(&[(0.0, 0.0)], 30.0, |_| {});
            // Still near inside the hysteresis (past spawnM, inside foldM).
            s.run(&[(80.0, 0.0)], 2.0, |_| {});
            assert_eq!(s.near.groups.len(), 1, "inside foldM it stays near");
            let g = s.near.groups[0].clone();
            let head = 5 + g.births as usize - g.deaths as usize;
            assert_eq!(g.members.len(), head);
            s.run(&[(500.0, 0.0)], 1.0, |_| {});
            assert!(
                s.near.groups.is_empty() && s.creatures.list.is_empty(),
                "folded back"
            );
            let r = &s.ecos[s.k()].groups;
            assert_eq!(r.len(), 1);
            assert_eq!(r[0].size, head as f64, "size = the survivors");
            assert_eq!(r[0].id, 0, "the same record");
            assert!(
                r[0].x.hypot(r[0].z - 0.0) < 60.0,
                "at their centroid: {:?}",
                (r[0].x, r[0].z)
            );
            assert!(r[0].hunger < 0.6, "they ate: {}", r[0].hunger);
        }
    );

    both_faces!(
        fn a_hungry_wolf_chases_a_deer_at_a_sprint_and_kills_it_feeding_the_pack(f: FaceFrame) {
            let mut s = Scene::new(
                f,
                &[
                    ("dwell:deer", 10.0, 0.0, 3.0, 0.1),
                    ("dwell:wolf", -6.0, 0.0, 2.0, 0.9),
                ],
                11,
            );
            let mut chased = false;
            s.run(&[(0.0, 0.0)], 60.0, |n| {
                chased |= n
                    .groups
                    .iter()
                    .flat_map(|g| &g.members)
                    .any(|m| m.mind == Mind::Chasing);
            });
            assert!(chased, "a wolf chased");
            let deer = s
                .near
                .groups
                .iter()
                .find(|g| g.record.species == species("dwell:deer"));
            let deaths = deer.map_or(3, |g| g.deaths);
            assert!(deaths >= 1, "a deer was killed");
            let wolves = s
                .near
                .groups
                .iter()
                .find(|g| g.record.species == species("dwell:wolf"))
                .unwrap();
            assert!(
                wolves.members.iter().all(|m| m.hunger < 0.9),
                "the kill fed the pack: {:?}",
                wolves.members.iter().map(|m| m.hunger).collect::<Vec<_>>()
            );
        }
    );

    both_faces!(
        fn a_deer_flees_a_wolf_within_its_radius(f: FaceFrame) {
            // A fed wolf (not hunting) 8 m off: the deer still keep away.
            let mut s = Scene::new(
                f,
                &[
                    ("dwell:deer", 0.0, 0.0, 1.0, 0.1),
                    ("dwell:wolf", 8.0, 0.0, 1.0, 0.0),
                ],
                3,
            );
            s.run(&[(0.0, 0.0)], 0.6, |_| {});
            let gap = |s: &Scene| {
                let p: Vec<_> = s
                    .creatures
                    .list
                    .iter()
                    .map(|c| f.point(c.position))
                    .collect();
                (p[0][0] - p[1][0]).hypot(p[0][2] - p[1][2])
            };
            let start = gap(&s);
            let deer = &s
                .near
                .groups
                .iter()
                .find(|g| g.record.species == species("dwell:deer"))
                .unwrap();
            assert_eq!(deer.members[0].mind, Mind::Fleeing);
            s.run(&[(0.0, 0.0)], 4.0, |_| {});
            assert!(gap(&s) > start + 4.0, "ran off: {start} → {}", gap(&s));
        }
    );

    both_faces!(
        fn a_fed_pair_has_young_up_to_the_cap(f: FaceFrame) {
            let mut s = Scene::new(f, &[("dwell:deer", 0.0, 0.0, 2.0, 0.0)], 5);
            let cap = DATA.species[species("dwell:deer")].group.cap as usize;
            let mut most = 0;
            s.run(&[(0.0, 0.0)], 900.0, |n| {
                most = most.max(n.groups.iter().map(|g| g.members.len()).sum::<usize>());
            });
            let g = &s.near.groups[0];
            assert!(g.births >= 1, "young were born");
            assert_eq!(g.members.len(), cap, "up to the cap");
            assert!(most <= cap, "never past it: {most}");
            assert_eq!(s.creatures.list.len(), cap);
        }
    );

    both_faces!(
        fn deer_graze_the_cell_and_sometimes_eat_the_real_tall_grass(f: FaceFrame) {
            let mut s = Scene::new(f, &[("dwell:deer", 0.0, 0.0, 4.0, 0.6)], 9);
            let cell = s.islands[0].cell_at(0.0, 0.0).unwrap();
            let before = s.ecos[s.k()].grazing[cell];
            s.run(&[(0.0, 0.0)], 120.0, |_| {});
            assert!(
                s.ecos[s.k()].grazing[cell] < before,
                "the grazing value dropped"
            );
            assert!(!s.eaten.is_empty(), "a tall grass block was eaten");
            let g = world(f);
            let tall = find_block("dwell:tall_grass").unwrap();
            for &c in &s.eaten {
                assert!(tall.states().contains(&usize::from(g(c).unwrap())));
                assert_eq!(f.cell(c)[1], 10, "by their feet");
            }
            // Data-gated: with no block named, the world is never touched.
            let mut sp = DATA.species.to_vec();
            let k = species("dwell:deer");
            let mut n = sp[k].near.unwrap();
            n.graze = Some(dwell_ecology::NearGraze {
                block: None,
                ..n.graze.unwrap()
            });
            sp[k].near = Some(n);
            let mut s = Scene::new(f, &[("dwell:deer", 0.0, 0.0, 4.0, 0.6)], 9);
            s.data = Data {
                species: Box::leak(sp.into_boxed_slice()),
                ..DATA
            };
            s.run(&[(0.0, 0.0)], 120.0, |_| {});
            assert!(s.eaten.is_empty());
        }
    );

    both_faces!(
        fn the_near_ecology_is_deterministic_given_a_seed(f: FaceFrame) {
            let run = |seed| {
                let mut s = Scene::new(
                    f,
                    &[
                        ("dwell:deer", 10.0, 0.0, 4.0, 0.4),
                        ("dwell:wolf", -8.0, 3.0, 3.0, 0.8),
                    ],
                    seed,
                );
                s.run(&[(0.0, 0.0)], 40.0, |_| {});
                // Face-local: the same on both faces.
                let local: Vec<_> = s
                    .creatures
                    .list
                    .iter()
                    .map(|c| f.point(c.position).map(|v| v.to_bits()))
                    .collect();
                (s.near.digest(&s.creatures), local)
            };
            assert_eq!(run(1), run(1));
            assert_ne!(run(1).1, run(2).1, "another seed, another run");
        }
    );

    both_faces!(
        fn at_night_fed_animals_fall_asleep(f: FaceFrame) {
            let mut s = Scene::new(f, &[("dwell:deer", 0.0, 0.0, 3.0, 0.1)], 4);
            s.run(&[(0.0, 0.0)], 30.0, |_| {});
            let mut slept = 0;
            s.night = true;
            s.run(&[(0.0, 0.0)], 60.0, |n| {
                slept = slept.max(
                    n.groups[0]
                        .members
                        .iter()
                        .filter(|m| m.mind == Mind::Sleeping)
                        .count(),
                );
            });
            assert!(slept >= 2, "most of the herd slept at night ({slept})");
        }
    );

    /// The creature names of each near group, by group.
    fn kinds(s: &Scene) -> Vec<Vec<&'static str>> {
        s.near
            .groups
            .iter()
            .map(|g| {
                g.members
                    .iter()
                    .map(|m| {
                        let c = s.creatures.get(m.creature).unwrap();
                        s.creatures.table.creatures[c.kind].name
                    })
                    .collect()
            })
            .collect()
    }

    both_faces!(
        fn a_herd_comes_near_as_the_creature_rules_mix_does_and_a_stag(f: FaceFrame) {
            // Twelve herds of four: the doe rule's mix gives some of them one stag, never two.
            let herds: Vec<_> = (0..12)
                .map(|k| ("dwell:deer", -44.0 + 8.0 * k as f64, 20.0, 4.0, 0.6))
                .collect();
            let mut s = Scene::new(f, &herds, 7);
            s.run(&[(0.0, 0.0)], 0.6, |_| {});
            let all = kinds(&s);
            assert_eq!(all.len(), 12, "every herd came near");
            let stags: Vec<usize> = all
                .iter()
                .map(|g| g.iter().filter(|&&n| n == "dwell:deer").count())
                .collect();
            assert!(stags.contains(&1), "some herds have a stag");
            assert!(stags.iter().all(|&n| n <= 1), "never two: {stags:?}");
            assert!(
                all.iter()
                    .flatten()
                    .all(|&n| ["dwell:doe", "dwell:deer"].contains(&n)),
                "{all:?}"
            );
            // By data alone: a mix of two stags always, and the herds come near with two each.
            let mut rules = Table::SHIPPED.rules.to_vec();
            let doe = s.creatures.kind_of("dwell:doe").unwrap();
            let stag = s.creatures.kind_of("dwell:deer").unwrap();
            let r = rules.iter_mut().find(|r| r.creature == doe).unwrap();
            r.mix = Box::leak(Box::new([crate::creatures::MixPart {
                creature: stag,
                chance: 1.0,
                max: 2,
            }]));
            let table = Table {
                rules: Box::leak(rules.into_boxed_slice()),
                ..Table::SHIPPED
            };
            let mut s = Scene::new(f, &herds[..3], 7);
            s.creatures = Creatures::new(table);
            s.run(&[(0.0, 0.0)], 0.6, |_| {});
            for g in kinds(&s) {
                assert_eq!(g.iter().filter(|&&n| n == "dwell:deer").count(), 2, "{g:?}");
                assert_eq!(g.len(), 4, "the head kept: {g:?}");
            }
        }
    );

    both_faces!(
        fn individuals_spawn_only_on_the_ecologys_ground_else_the_nearest(f: FaceFrame) {
            // Too steep: stairs rising 1 m a column west of x 0 (over the data's 40°); a herd
            // on them stands on the nearest flat columns instead (x ≥ 1: x 0's west step is steep).
            let mut s = Scene::new(f, &[("dwell:deer", -6.0, 0.0, 4.0, 0.6)], 3);
            s.floor = stairs;
            s.run(&[(0.0, 0.0)], 0.6, |_| {});
            assert_eq!(
                s.creatures.list.len(),
                4,
                "the herd came near, its head kept"
            );
            for c in &s.creatures.list {
                let l = f.point(c.position);
                assert!(l[0] >= 1.0, "on the flat, not the stairs: {l:?}");
                assert!((l[1] - m_of_row(10)).abs() < 1e-9, "on the floor: {l:?}");
                assert!(
                    (l[0] + 6.0).hypot(l[2]) <= DATA.near.search_m + 1.0,
                    "within searchM: {l:?}"
                );
            }
            // Water: the island's cells west of x 0 are sea; a herd there stands on the land.
            let mut s = Scene::new(f, &[("dwell:deer", -5.0, 0.0, 4.0, 0.6)], 3);
            let isl = Island::from_ground(&DATA, 4, -300.0, |x, _| {
                if x < 0.0 {
                    Ground::Water
                } else {
                    Ground::Land {
                        height_m: 10.0,
                        slope_deg: 1.0,
                    }
                }
            });
            s.islands = vec![isl.clone(), isl];
            s.run(&[(0.0, 0.0)], 0.6, |_| {});
            assert_eq!(s.creatures.list.len(), 4);
            for c in &s.creatures.list {
                let l = f.point(c.position);
                assert!(l[0] >= 0.0, "on the island's land: {l:?}");
            }
            // No ground within searchM at all: the herd waits as a record.
            let mut s = Scene::new(f, &[("dwell:deer", -40.0, 0.0, 4.0, 0.6)], 3);
            s.floor = stairs;
            s.run(&[(-40.0, 0.0)], 0.6, |_| {});
            assert!(s.near.groups.is_empty() && s.creatures.list.is_empty());
            assert_eq!(s.ecos[s.k()].groups.len(), 1, "still a record");
        }
    );

    both_faces!(
        fn the_far_step_sees_a_near_herd_and_its_kills_become_deaths(f: FaceFrame) {
            let mut s = Scene::new(f, &[("dwell:deer", 5.0, 5.0, 5.0, 0.6)], 7);
            s.run(&[(0.0, 0.0)], 0.6, |_| {});
            let k = s.k();
            assert!(s.ecos[k].groups.is_empty(), "out of the far step");
            let r = s.ecos[k].near.clone();
            assert_eq!(r.len(), 1, "its summary is there: {r:?}");
            assert_eq!((r[0].id, r[0].size), (0, 5.0));
            assert!(s.ecos[1 - k].near.is_empty(), "on its own face only");
            // A far pack's kills on the summary: 1.5 head, then 0.6 more.
            s.ecos[k].near[0].size -= 1.5;
            s.run(&[(0.0, 0.0)], 0.5, |_| {});
            assert_eq!(s.near.groups[0].deaths, 1);
            assert_eq!(s.creatures.list.len(), 4);
            assert_eq!(s.ecos[k].near[0].size, 4.0, "rewritten from the members");
            s.ecos[k].near[0].size -= 0.6;
            s.run(&[(0.0, 0.0)], 0.5, |_| {});
            assert_eq!(s.near.groups[0].deaths, 2, "the shares add up");
            assert_eq!(s.creatures.list.len(), 3);
            // The far step itself: a hungry far pack closes on the near herd's summary.
            let mut eco = s.ecos[k].clone();
            eco.groups.push(Group {
                id: 9,
                species: species("dwell:wolf"),
                x: 300.0,
                z: 0.0,
                size: 4.0,
                hunger: 0.5,
                energy: 1.0,
                activity: Activity::Resting,
            });
            dwell_ecology::step(&s.islands[k], &DATA, &mut eco, 0.05);
            assert!(
                eco.groups[0].x < 250.0,
                "the pack moved: {}",
                eco.groups[0].x
            );
            // Folded back, the summary goes and the record returns.
            s.run(&[(500.0, 0.0)], 0.5, |_| {});
            assert!(s.ecos[k].near.is_empty());
            assert_eq!(s.ecos[k].groups[0].size, 3.0);
        }
    );

    #[test]
    fn every_species_names_a_creature_and_a_block_the_build_has() {
        let t = Table::SHIPPED;
        for s in DATA.species {
            let n = s.near.expect("the build's species come near");
            assert!(
                t.creatures.iter().any(|c| c.name == n.creature),
                "{}",
                n.creature
            );
            if let Some(b) = n.graze.and_then(|g| g.block) {
                assert!(find_block(b).is_some(), "{b}");
            }
        }
    }
}
