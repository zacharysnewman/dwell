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
use dwell_ecology::{
    Activity, Data, Diet, Ecosystem, Group, HurtReact, IdlePose, Island, NearDef, PlayerReact,
    pick_idle,
};
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
    /// Seconds left reacting to a hit (its species' `near.hurt`), and where (face-local x, z) the
    /// striker stood.
    pub hurt_s: f64,
    pub hurt_from: [f64; 2],
    /// Seconds left alarmed by a player (its group's flight, `near.players`), and where (face-local
    /// x, z) the player stood.
    pub alarm_s: f64,
    pub alarm_from: [f64; 2],
    /// Its pose (an idle's, eating's or sleep's; Stand moving) and the seconds its idle has left.
    pub pose: IdlePose,
    pub idle_s: f64,
    /// Seconds left before it may mate again (a parent's or a young's rest, BUGS.md #79).
    pub rest_s: f64,
    rng: u64,
}

impl Member {
    fn new(creature: u32, hunger: f64, energy: f64, x: f64, z: f64, rng: u64) -> Self {
        Self {
            creature,
            hunger,
            energy,
            mind: Mind::LookingForCompany,
            x,
            z,
            sleep_s: 0.0,
            hurt_s: 0.0,
            hurt_from: [0.0; 2],
            rest_s: 0.0,
            alarm_s: 0.0,
            alarm_from: [0.0; 2],
            pose: IdlePose::Stand,
            idle_s: 0.0,
            rng,
        }
    }
}

/// How a player moves, as animals notice it (WORK_QUEUE item 39): crouching, and its horizontal
/// speed (m/s).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stance {
    pub crouching: bool,
    pub speed: f64,
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
    /// Seconds left before its next young may be born (its species' `mate.groupEveryS`).
    pub birth_s: f64,
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
    /// The living players' feet (world): what the animals see, flee and notice.
    pub players: &'a [[f64; 3]],
    /// Where groups come near and stay (world): the players' feet and the theater cameras'
    /// viewpoints (docs/THEATER_MODE.md §7), which bring the world near but are never seen.
    pub near: &'a [[f64; 3]],
    /// Their stances, by the same index (a player without one stands still).
    pub stances: &'a [Stance],
    pub get: Get<'a>,
    /// Whether it is night on each face.
    pub night: [bool; 2],
    pub seed: u64,
    /// The animals see no player (the dev tools' `animalsIgnorePlayers`, for filming their idles
    /// up close): no fleeing, avoiding or noticing; the players still bring groups near.
    pub ignore_players: bool,
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

/// The players on face `k`, face-local, with their stances.
fn players_seen_on(players: &[[f64; 3]], stances: &[Stance], k: usize) -> Vec<([f64; 3], Stance)> {
    players
        .iter()
        .enumerate()
        .map(|(i, &p)| (FaceFrame::at_height(p[1]), p, i))
        .filter(|(f, _, _)| face_index(*f) == k)
        .map(|(f, p, i)| (f.point(p), stances.get(i).copied().unwrap_or_default()))
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
    /// Resting from a young: not a partner.
    resting: bool,
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
        self.refresh(w, creatures);
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

    /// Every member's place from its creature; a member whose creature is gone (killed by a
    /// weapon, WORK_QUEUE item 35) is a death; a hit since the last decision starts its reaction.
    fn refresh(&mut self, w: &World, creatures: &mut Creatures) {
        for g in &mut self.groups {
            let f = FaceFrame::of(FACES[g.face]);
            let before = g.members.len();
            g.members.retain(|m| creatures.get(m.creature).is_some());
            g.deaths += (before - g.members.len()) as u32;
            let hurt = w.data.species[g.record.species].near.and_then(|n| n.hurt);
            for m in &mut g.members {
                if let Some(c) = creatures.get_mut(m.creature) {
                    let l = f.point(c.position);
                    (m.x, m.z) = (l[0], l[2]);
                    if let Some(from) = c.take_hurt()
                        && let Some(h) = hurt
                    {
                        let p = f.point(from);
                        m.hurt_s = h.seconds;
                        m.hurt_from = [p[0], p[2]];
                        m.sleep_s = 0.0;
                    }
                    // An angry one (its creature's `attack`) closes on where its striker is now,
                    // not where the blow came from, so its bite can reach (WORK_QUEUE item 35).
                    if m.hurt_s > 0.0
                        && let Some(to) = c.chase_to
                    {
                        let p = f.point(to);
                        m.hurt_from = [p[0], p[2]];
                    }
                }
            }
        }
        self.groups.retain(|g| !g.members.is_empty());
    }

    fn fold_far(&mut self, w: &mut World, creatures: &mut Creatures) {
        let fold = w.data.near.fold_m;
        let mut i = 0;
        while i < self.groups.len() {
            let g = &self.groups[i];
            let on = players_on(w.near, g.face);
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
            let on = players_on(w.near, k);
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
                        members.push(Member::new(id, r.hunger, r.energy, at[0], at[2], rng));
                    }
                    Some(members)
                })();
                match made {
                    Some(members) => {
                        w.ecos[k].groups.remove(i);
                        // No young the moment it comes near (BUGS.md #79): its first after the
                        // species' `mate.groupEveryS`.
                        let birth_s = w.data.species[r.species]
                            .near
                            .map_or(0.0, |n| n.mate.group_every_s);
                        self.groups.push(NearGroup {
                            face: k,
                            record: r,
                            summary_size: members.len() as f64,
                            members,
                            deaths: 0,
                            births: 0,
                            birth_s,
                            far_loss: 0.0,
                        });
                    }
                    None => i += 1,
                }
            }
        }
    }

    fn think(&mut self, w: &mut World, creatures: &mut Creatures, dt: f64) -> Vec<[i32; 3]> {
        let seen_players: Vec<Vec<([f64; 3], Stance)>> = (0..FACES.len())
            .map(|k| {
                if w.ignore_players {
                    Vec::new()
                } else {
                    players_seen_on(w.players, w.stances, k)
                }
            })
            .collect();
        // A herd that flees players flees as one (WORK_QUEUE item 39): a member that notices one
        // alarms its whole group, from that player, for the data's `calmS`.
        for g in &mut self.groups {
            let Some(p) = w.data.species[g.record.species]
                .near
                .and_then(|n| n.players)
                .filter(|p| p.react == PlayerReact::Flee)
            else {
                continue;
            };
            let noticed = g.members.iter().find_map(|m| {
                seen_players[g.face]
                    .iter()
                    .map(|(q, st)| ((q[0] - m.x).hypot(q[2] - m.z), q, st))
                    .filter(|(d, _, st)| *d <= p.notice_at(st.crouching, st.speed, m.hunger))
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, q, _)| [q[0], q[2]])
            });
            if let Some(from) = noticed {
                for m in &mut g.members {
                    m.alarm_s = p.calm_s.max(dt);
                    m.alarm_from = from;
                    m.sleep_s = 0.0;
                }
            }
        }
        // Each face's far prey records (a pack near players tracks a herd still a record).
        let records: Vec<Vec<(usize, f64, f64)>> = w
            .ecos
            .iter()
            .map(|e| e.groups.iter().map(|g| (g.species, g.x, g.z)).collect())
            .collect();
        for g in &mut self.groups {
            g.birth_s = (g.birth_s - dt).max(0.0);
        }
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
                    resting: m.rest_s > 0.0,
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
            let ready = size < cap && self.groups[gi].birth_s <= 0.0;
            let go = {
                let m = &mut self.groups[gi].members[mi];
                m.rest_s = (m.rest_s - dt).max(0.0);
                m.hunger = (m.hunger + def.hunger_per_s * dt).min(1.0);
                if m.mind != Mind::Sleeping {
                    m.energy = (m.energy - def.energy_per_s * dt).max(0.0);
                }
                let health = creatures
                    .get(m.creature)
                    .map_or(1.0, |c| c.health_share(&creatures.table));
                decide(
                    DecideIn {
                        players: &seen_players[me.face],
                        records: &records[me.face],
                        def: &def,
                        health,
                        diet: sp.diet,
                        flee_from: sp.flee.map(|fl| fl.from).unwrap_or(&[]),
                        me,
                        night,
                        dt,
                        room: ready && m.rest_s <= 0.0,
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
                    // Paced (BUGS.md #79): the group's next young after `groupEveryS`, each
                    // parent's next mating after `restS`.
                    g.birth_s = def.mate.group_every_s;
                    let p = &mut g.members[partner.member];
                    p.hunger = (p.hunger + def.mate.cost).min(1.0);
                    p.rest_s = def.mate.rest_s;
                    let m = &mut g.members[mi];
                    m.hunger = (m.hunger + def.mate.cost).min(1.0);
                    m.rest_s = def.mate.rest_s;
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
                        if c.target.is_some() {
                            m.pose = IdlePose::Stand;
                        } else if m.idle_s > 0.0 {
                            m.idle_s -= dt;
                        } else if def.idles.is_empty() || roll(&mut m.rng) < def.walk_chance {
                            // Walk (with no idles, as before: a walk at the data's chance).
                            if def.idles.is_empty() && roll(&mut m.rng) >= def.walk_chance {
                                m.pose = IdlePose::Stand;
                            } else {
                                let a = roll(&mut m.rng) * std::f64::consts::TAU;
                                let d = def.wander_m * (0.3 + 0.7 * roll(&mut m.rng));
                                c.target = Some([me.x + d * a.sin(), me.z + d * a.cos()]);
                                m.pose = IdlePose::Stand;
                            }
                        } else if let Some(i) = pick_idle(def.idles, roll(&mut m.rng)) {
                            // An idle by the data's weights (graze, look up, sit, …).
                            let [lo, hi] = i.seconds;
                            m.pose = i.pose;
                            m.idle_s = lo + (hi - lo) * roll(&mut m.rng);
                        }
                    }
                }
            }
            // Its pose for what it does: eating's and sleep's by the data, Stand on the move.
            match m.mind {
                Mind::Eating => m.pose = def.graze.map_or(IdlePose::Graze, |g| g.pose),
                Mind::Sleeping => m.pose = def.sleep.pose,
                Mind::Wandering | Mind::LookingForFood => {}
                _ => {
                    m.pose = IdlePose::Stand;
                    m.idle_s = 0.0;
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
            let rest_s = w.data.species[self.groups[gi].record.species]
                .near
                .map_or(0.0, |n| n.mate.rest_s);
            let g = &mut self.groups[gi];
            g.births += 1;
            let mut young = Member::new(id, 0.5, 1.0, at[0], at[1], rng);
            young.rest_s = rest_s;
            g.members.push(young);
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
    /// The players on its face (face-local feet) with their stances.
    players: &'a [([f64; 3], Stance)],
    /// Its face's far records (species, x, z).
    records: &'a [(usize, f64, f64)],
    def: &'a NearDef,
    /// Its health as a share of its whole.
    health: f64,
    diet: Diet,
    flee_from: &'a [usize],
    me: Seen,
    night: bool,
    dt: f64,
    /// Whether it may have a young now: room in the group, the group's pace and its own rest
    /// allowing (BUGS.md #79).
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
    // 0. Hurt (a weapon's hit, WORK_QUEUE item 35): flee from the striker, or fight (close on it)
    //    unless too hurt, by the species' `near.hurt`.
    if let Some(h) = def.hurt
        && m.hurt_s > 0.0
    {
        m.hurt_s -= i.dt;
        m.sleep_s = 0.0;
        let [hx, hz] = m.hurt_from;
        let (dx, dz) = (me.x - hx, me.z - hz);
        let d = dx.hypot(dz);
        let fight = h.react == HurtReact::Fight && i.health >= h.flee_below_health;
        if fight {
            m.mind = Mind::Chasing;
            return Act::Go(Go::To([hx, hz], h.pace));
        }
        m.mind = Mind::Fleeing;
        let (ax, az) = if d > 1e-6 {
            (dx / d, dz / d)
        } else {
            (1.0, 0.0)
        };
        return Act::Go(Go::To([me.x + ax * h.run_m, me.z + az * h.run_m], h.pace));
    }
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
    // 1b. Players (WORK_QUEUE item 39): a group alarmed by one runs from it (the pre-pass in
    //     `think` alarms the whole group); an avoider walks off to keep its notice distance.
    if let Some(p) = def.players {
        if p.react == PlayerReact::Flee && m.alarm_s > 0.0 {
            m.alarm_s -= i.dt;
            m.mind = Mind::Fleeing;
            m.sleep_s = 0.0;
            let [px, pz] = m.alarm_from;
            let (dx, dz) = (me.x - px, me.z - pz);
            let d = dx.hypot(dz);
            let (ax, az) = if d > 1e-6 {
                (dx / d, dz / d)
            } else {
                (1.0, 0.0)
            };
            return Act::Go(Go::To([me.x + ax * p.run_m, me.z + az * p.run_m], p.pace));
        }
        if p.react == PlayerReact::Avoid
            && let Some((q, d, r)) = i
                .players
                .iter()
                .map(|(q, st)| {
                    let d = (q[0] - me.x).hypot(q[2] - me.z);
                    (q, d, p.notice_at(st.crouching, st.speed, m.hunger))
                })
                .filter(|(_, d, r)| d <= r)
                .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            m.mind = Mind::Fleeing;
            m.sleep_s = 0.0;
            let (ax, az) = if d > 1e-6 {
                ((me.x - q[0]) / d, (me.z - q[2]) / d)
            } else {
                (1.0, 0.0)
            };
            let run = r - d + 2.0;
            return Act::Go(Go::To([me.x + ax * run, me.z + az * run], p.pace));
        }
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
            // Out of sense: track the nearest prey within `trackM`, an individual or a herd's far
            // record (WORK_QUEUE item 39), to bring the hunt in range.
            near => {
                let record = i
                    .records
                    .iter()
                    .filter(|r| species.contains(&r.0))
                    .map(|r| ((r.1, r.2), (r.1 - me.x).hypot(r.2 - me.z)))
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                let target = near
                    .map(|(o, d)| ((o.x, o.z), d))
                    .into_iter()
                    .chain(record)
                    .filter(|t| t.1 <= h.track_m)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                m.mind = Mind::LookingForFood;
                match target {
                    Some(((x, z), _)) => Act::Go(Go::To([x, z], 1.0)),
                    None => Act::Go(Go::Wander),
                }
            }
        };
    }
    // 5. Mate: fed and rested, a fed groupmate near, room in the group.
    let mt = def.mate;
    if i.room && m.hunger <= mt.below_hunger && m.energy >= mt.above_energy {
        let partner = nearest(&|o| {
            o.group == me.group
                && !o.resting
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

    /// `d` with no species noticing players: the older scenes are about the animals among
    /// themselves, a player standing by only to bring them near.
    fn unnoticing(d: Data) -> Data {
        let species: Vec<_> = d
            .species
            .iter()
            .map(|s| {
                let mut s = *s;
                if let Some(mut n) = s.near {
                    n.players = None;
                    s.near = Some(n);
                }
                s
            })
            .collect();
        Data {
            species: Box::leak(species.into_boxed_slice()),
            ..d
        }
    }

    struct Scene {
        f: FaceFrame,
        data: Data,
        /// The players' stances (none: standing still).
        stances: Vec<Stance>,
        ignore_players: bool,
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
        /// Groups `(species, x, z, size, hunger)` on face `f`'s ecosystem (the other face's none),
        /// none noticing the players ([`unnoticing`]).
        fn new(f: FaceFrame, groups: &[(&str, f64, f64, f64, f64)], seed: u64) -> Self {
            Self::with_data(f, unnoticing(DATA), groups, seed)
        }

        /// The same with the species of `data`, as they are.
        fn with_data(
            f: FaceFrame,
            data: Data,
            groups: &[(&str, f64, f64, f64, f64)],
            seed: u64,
        ) -> Self {
            let isl = Island::from_ground(&data, 4, -300.0, |_, _| Ground::Land {
                height_m: 10.0,
                slope_deg: 1.0,
            });
            let mut ecos: Vec<Ecosystem> = (0..2)
                .map(|_| {
                    let mut e = Ecosystem::new(&isl, &data, seed);
                    e.groups.clear();
                    e
                })
                .collect();
            let k = face_index(f);
            for (id, &(name, x, z, size, hunger)) in groups.iter().enumerate() {
                ecos[k].groups.push(Group {
                    id: id as u32,
                    species: data.find(name).unwrap(),
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
                data,
                stances: Vec::new(),
                ignore_players: false,
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
                    near: &players,
                    stances: &self.stances,
                    get: &g,
                    night: [self.night; 2],
                    seed: self.seed,
                    ignore_players: self.ignore_players,
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

    /// The mind and the distance from face-local `(x, z)` of the one creature after a hit from
    /// there and `secs` more (WORK_QUEUE item 35).
    fn after_a_hit(s: &mut Scene, from: (f64, f64), health: f64, secs: f64) -> (Mind, f64, f64) {
        let f = s.f;
        let c = &mut s.creatures.list[0];
        let before = f.point(c.position);
        c.health = health;
        c.hurt_from = Some(f.point([from.0, m_of_row(10), from.1]));
        s.run(&[(-30.0, 0.0)], secs, |_| {});
        let mind = s.near.groups[0].members[0].mind;
        let now = f.point(s.creatures.list[0].position);
        let d = |p: [f64; 3]| (p[0] - from.0).hypot(p[2] - from.1);
        (mind, d(before), d(now))
    }

    both_faces!(
        fn a_hurt_deer_flees_and_a_hurt_wolf_fights_unless_badly_hurt(f: FaceFrame) {
            // A lone, fed doe and a lone, fed wolf; a hit from 2 m to their -x.
            let mut s = Scene::new(f, &[("dwell:deer", 0.0, 0.0, 1.0, 0.0)], 9);
            s.run(&[(-30.0, 0.0)], 0.6, |_| {});
            let at = f.point(s.creatures.list[0].position);
            let (mind, d0, d1) = after_a_hit(&mut s, (at[0] - 2.0, at[2]), 1.0, 2.0);
            assert_eq!(mind, Mind::Fleeing, "the deer's data: flee");
            assert!(d1 > d0 + 2.0, "ran off: {d0} → {d1}");
            let mut s = Scene::new(f, &[("dwell:wolf", 0.0, 0.0, 1.0, 0.0)], 9);
            s.run(&[(-30.0, 0.0)], 0.6, |_| {});
            let at = f.point(s.creatures.list[0].position);
            let whole = s.creatures.table.creatures[s.creatures.list[0].kind].health;
            let (mind, d0, d1) = after_a_hit(&mut s, (at[0] - 4.0, at[2]), whole, 1.0);
            assert_eq!(mind, Mind::Chasing, "the wolf's data: fight");
            assert!(d1 < d0 - 1.0, "closed on the striker: {d0} → {d1}");
            // Under its fleeBelowHealth it flees instead.
            let (mind, ..) = after_a_hit(&mut s, (at[0] - 4.0, at[2]), 0.1 * whole, 1.0);
            assert_eq!(mind, Mind::Fleeing);
        }
    );

    both_faces!(
        fn a_member_killed_leaves_its_group_as_a_death(f: FaceFrame) {
            let mut s = Scene::new(f, &[("dwell:deer", 0.0, 0.0, 3.0, 0.6)], 4);
            s.run(&[(-20.0, 0.0)], 0.6, |_| {});
            let id = s.creatures.list[0].id;
            s.creatures.remove(id);
            s.run(&[(-20.0, 0.0)], 0.6, |_| {});
            let g = &s.near.groups[0];
            assert_eq!((g.members.len(), g.deaths), (2, 1));
        }
    );

    /// The shipped ecology with the deer's births paced every `every_s` (a group) and `rest_s` (a
    /// parent): a fixture by data alone.
    fn paced(every_s: f64, rest_s: f64) -> Data {
        let k = species("dwell:deer");
        let mut list = DATA.species.to_vec();
        let mut near = list[k].near.unwrap();
        near.mate.group_every_s = every_s;
        near.mate.rest_s = rest_s;
        list[k].near = Some(near);
        Data {
            species: Box::leak(list.into_boxed_slice()),
            ..DATA
        }
    }

    both_faces!(
        fn births_follow_the_data_s_pace(f: FaceFrame) {
            // BUGS.md #79's pace is data: the deer's `groupEveryS` spaces a group's young.
            let births = |every: f64| {
                let mut s = Scene::new(f, &[("dwell:deer", 0.0, 0.0, 4.0, 0.0)], 5);
                s.data = paced(every, 0.0);
                s.run(&[(0.0, 0.0)], 125.0, |_| {});
                s.near.groups[0].births
            };
            let (fast, slow) = (births(10.0), births(60.0));
            assert!(fast > slow, "faster pace, more young: {fast} vs {slow}");
            assert!(slow <= 2, "at most one every 60 s: {slow} in 125 s");
        }
    );

    both_faces!(
        fn a_fed_pair_has_young_up_to_the_cap(f: FaceFrame) {
            let mut s = Scene::new(f, &[("dwell:deer", 0.0, 0.0, 2.0, 0.0)], 5);
            // Births paced by data alone: a young at most every 20 s, parents resting 30 s.
            s.data = paced(20.0, 30.0);
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
        fn a_fed_herd_near_a_player_has_at_most_one_young_a_minute(f: FaceFrame) {
            // BUGS.md #79: a fed, rested herd of 6 near a player grew to 12-13 within a minute
            // (a young each half-second try at the deer's 0.5 chance). It comes near once (its
            // record out of the far step, one creature per member) and its births are paced.
            let mut s = Scene::new(f, &[("dwell:deer", 0.0, 0.0, 6.0, 0.0)], 3);
            let k = s.k();
            s.run(&[(0.0, 0.0)], 60.0, |_| {});
            assert!(
                s.ecos[k].groups.is_empty(),
                "the record is out of the far step"
            );
            assert_eq!(s.near.groups.len(), 1, "spawned once");
            let g = &s.near.groups[0];
            assert_eq!(
                s.creatures.list.len(),
                g.members.len(),
                "one creature a member"
            );
            assert!(g.births <= 1, "{} young in a minute", g.births);
            assert!(
                g.members.len() <= 7,
                "{} head after a minute",
                g.members.len()
            );
        }
    );

    /// The least gap (m) between two creatures' bodies (their centres' distance less both radii)
    /// on face `f`, over every pair.
    fn least_gap(s: &Scene) -> f64 {
        let cs = &s.creatures;
        let mut least = f64::MAX;
        for (i, a) in cs.list.iter().enumerate() {
            for b in &cs.list[i + 1..] {
                let (p, q) = (s.f.point(a.position), s.f.point(b.position));
                let body = |k: usize| cs.table.creatures[k].body_radius_m;
                let r = body(a.kind) + body(b.kind);
                least = least.min((p[0] - q[0]).hypot(p[2] - q[2]) - r);
            }
        }
        least
    }

    both_faces!(
        fn herd_members_never_stand_inside_each_other(f: FaceFrame) {
            // BUGS.md #80: two does stood in the same place for seconds. A herd keeps its
            // members' bodies apart (they spawn 1-4 m round the record, walk to each other for
            // company and mating, and young are born by a parent).
            let mut s = Scene::new(f, &[("dwell:deer", 0.0, 0.0, 8.0, 0.0)], 21);
            s.run(&[(0.0, 0.0)], 1.0, |_| {});
            let mut least = f64::MAX;
            for _ in 0..240 {
                s.run(&[(0.0, 0.0)], 0.25, |_| {});
                least = least.min(least_gap(&s));
            }
            assert!(least >= -1e-6, "two bodies overlapped by {:.3} m", -least);
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
            // Somewhat tired (not under `belowEnergy`): they don't nap by day, but a night's nap
            // lasts past its least.
            let k = s.k();
            s.ecos[k].groups[0].energy = 0.45;
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

    /// The members of `species`' near groups.
    fn members<'a>(s: &'a Scene, name: &str) -> Vec<&'a Member> {
        let sp = s.data.find(name).unwrap();
        s.near
            .groups
            .iter()
            .filter(|g| g.record.species == sp)
            .flat_map(|g| &g.members)
            .collect()
    }

    /// The face-local distance from `(x, z)` to the nearest creature of the near groups of `name`.
    fn nearest_to(s: &Scene, name: &str, x: f64, z: f64) -> f64 {
        members(s, name)
            .iter()
            .filter_map(|m| s.creatures.get(m.creature))
            .map(|c| {
                let l = s.f.point(c.position);
                (l[0] - x).hypot(l[2] - z)
            })
            .fold(f64::INFINITY, f64::min)
    }

    const WALKING: Stance = Stance {
        crouching: false,
        speed: 3.0,
    };
    const CROUCHING: Stance = Stance {
        crouching: true,
        speed: 1.5,
    };

    both_faces!(
        fn a_herd_flees_a_player_within_its_notice_distance_and_calms(f: FaceFrame) {
            // WORK_QUEUE item 39 (owner, 2026-11-12: "deer flee from players"): a walking player
            // 16 m from a fed herd (inside `noticeM` 22): every deer runs, the whole herd, away.
            let mut s = Scene::with_data(f, DATA, &[("dwell:deer", -14.0, 0.0, 3.0, 0.1)], 5);
            s.stances = vec![WALKING];
            s.run(&[(-30.0, 0.0)], 0.1, |_| {});
            let start = nearest_to(&s, "dwell:deer", -30.0, 0.0);
            assert!(start < 22.0, "in range: {start}");
            s.run(&[(-30.0, 0.0)], 1.0, |_| {});
            let minds: Vec<_> = members(&s, "dwell:deer").iter().map(|m| m.mind).collect();
            assert!(
                minds.iter().all(|&m| m == Mind::Fleeing),
                "the herd flees as one: {minds:?}"
            );
            s.run(&[(-30.0, 0.0)], 4.0, |_| {});
            let gone = nearest_to(&s, "dwell:deer", -30.0, 0.0);
            assert!(gone > start + 10.0, "ran off: {start} → {gone}");
            // Out of notice for `calmS` (8 s), it calms.
            s.run(&[(-30.0, 0.0)], 12.0, |_| {});
            assert!(
                members(&s, "dwell:deer")
                    .iter()
                    .all(|m| m.mind != Mind::Fleeing && m.alarm_s <= 0.0),
                "calmed"
            );
        }
    );

    both_faces!(
        fn animals_told_to_ignore_players_keep_their_idles_beside_one(f: FaceFrame) {
            // The dev tools' `animalsIgnorePlayers` (for filming the idles): a walking player 16 m
            // from the herd, inside its notice distance, and no deer flees.
            let mut s = Scene::with_data(f, DATA, &[("dwell:deer", -14.0, 0.0, 3.0, 0.1)], 5);
            s.stances = vec![WALKING];
            s.ignore_players = true;
            s.run(&[(-30.0, 0.0)], 3.0, |_| {});
            let minds: Vec<_> = members(&s, "dwell:deer").iter().map(|m| m.mind).collect();
            assert!(!minds.is_empty(), "the herd came near");
            assert!(
                minds.iter().all(|&m| m != Mind::Fleeing),
                "none flees: {minds:?}"
            );
        }
    );

    both_faces!(
        fn a_crouching_player_gets_closer_before_the_deer_notice(f: FaceFrame) {
            // 14 m off: past `crouchM` (8), inside `noticeM` (22). Crouching, the deer stay;
            // walking at the same place, they run.
            for (stance, flees) in [(CROUCHING, false), (WALKING, true)] {
                let mut s = Scene::with_data(f, DATA, &[("dwell:deer", 0.0, 0.0, 1.0, 0.1)], 9);
                s.stances = vec![stance];
                s.run(&[(-14.0, 0.0)], 3.0, |_| {});
                let fled = members(&s, "dwell:deer")
                    .iter()
                    .any(|m| m.mind == Mind::Fleeing || m.alarm_s > 0.0);
                assert_eq!(fled, flees, "{stance:?}");
            }
            let p = DATA.species[species("dwell:deer")]
                .near
                .unwrap()
                .players
                .unwrap();
            assert!(p.notice_at(true, 1.0, 0.0) < p.notice_at(false, 3.0, 0.0));
            assert!(
                p.notice_at(false, 0.0, 0.0) < p.notice_at(false, 3.0, 0.0),
                "still"
            );
            assert!(
                p.notice_at(false, 8.0, 0.0) > p.notice_at(false, 3.0, 0.0),
                "running"
            );
        }
    );

    both_faces!(
        fn wolves_keep_their_distance_from_players_unless_hungry(f: FaceFrame) {
            // A fed wolf 12 m from a standing player (inside `stillM` 18) walks off; a hungry one
            // (over `boldAboveHunger`) only keeps `boldM` (8 m) and doesn't.
            for (hunger, avoids) in [(0.1, true), (0.9, false)] {
                let mut s = Scene::with_data(f, DATA, &[("dwell:wolf", 12.0, 0.0, 1.0, hunger)], 4);
                s.run(&[(0.0, 0.0)], 0.6, |_| {});
                let start = nearest_to(&s, "dwell:wolf", 0.0, 0.0);
                let mut kept = false;
                s.run(&[(0.0, 0.0)], 3.0, |n| {
                    kept |= n
                        .groups
                        .iter()
                        .flat_map(|g| &g.members)
                        .any(|m| m.mind == Mind::Fleeing);
                });
                assert_eq!(kept, avoids, "hunger {hunger}");
                if avoids {
                    let end = nearest_to(&s, "dwell:wolf", 0.0, 0.0);
                    assert!(
                        end >= 18.0 - 1.0 && end > start,
                        "kept its distance: {start} → {end}"
                    );
                }
            }
        }
    );

    both_faces!(
        fn a_hungry_pack_tracks_a_herd_out_of_sense_and_the_hunt_starts(f: FaceFrame) {
            // 80 m apart (past `hunt.senseM` 28, inside `trackM` 160): the pack walks to the herd
            // and, once in sense, chases and kills (WORK_QUEUE item 39).
            let mut s = Scene::new(
                f,
                &[
                    ("dwell:deer", 40.0, 0.0, 3.0, 0.1),
                    ("dwell:wolf", -40.0, 0.0, 2.0, 0.9),
                ],
                12,
            );
            let mut chased = false;
            s.run(&[(0.0, 30.0)], 90.0, |n| {
                chased |= n
                    .groups
                    .iter()
                    .flat_map(|g| &g.members)
                    .any(|m| m.mind == Mind::Chasing);
            });
            assert!(chased, "the pack closed in and chased");
            let deaths = s
                .near
                .groups
                .iter()
                .find(|g| g.record.species == species("dwell:deer"))
                .map_or(3, |g| g.deaths);
            assert!(deaths >= 1, "a kill");
        }
    );

    #[test]
    fn idle_picks_follow_the_data_weights() {
        // Every species' idles: over an even spread of rolls, each pose's share is its weight's.
        for sp in DATA.species {
            let idles = sp.near.unwrap().idles;
            assert!(!idles.is_empty(), "{} idles", sp.name);
            let total: f64 = idles.iter().map(|i| i.weight).sum();
            let n = 10_000;
            let mut count = std::collections::HashMap::new();
            for k in 0..n {
                let i = pick_idle(idles, (k as f64 + 0.5) / n as f64).unwrap();
                *count.entry(i.pose).or_insert(0usize) += 1;
            }
            for i in idles {
                let share = count[&i.pose] as f64 / n as f64;
                assert!(
                    (share - i.weight / total).abs() < 0.01,
                    "{}: {:?} {share}",
                    sp.name,
                    i.pose
                );
            }
        }
        assert!(pick_idle(&[], 0.5).is_none());
    }

    both_faces!(
        fn deer_graze_head_down_and_wolves_sit_and_lie_by_their_idles(f: FaceFrame) {
            // Fed and rested (no food to find, no mate: `mate` needs a fed pair under the cap,
            // and one each is alone), they idle in their data's poses only, all of them in time.
            let mut s = Scene::new(
                f,
                &[
                    ("dwell:deer", 20.0, 0.0, 1.0, 0.0),
                    ("dwell:wolf", -20.0, 0.0, 1.0, 0.0),
                ],
                21,
            );
            let mut poses: std::collections::HashMap<&str, std::collections::HashSet<IdlePose>> =
                Default::default();
            for _ in 0..120 {
                s.run(&[(0.0, 40.0)], 1.0, |_| {});
                for name in ["dwell:deer", "dwell:wolf"] {
                    for m in members(&s, name) {
                        poses.entry(name).or_default().insert(m.pose);
                    }
                }
            }
            for name in ["dwell:deer", "dwell:wolf"] {
                let def = DATA.species[species(name)].near.unwrap();
                let allowed: std::collections::HashSet<_> = def
                    .idles
                    .iter()
                    .map(|i| i.pose)
                    .chain([IdlePose::Stand, def.sleep.pose])
                    .chain(def.graze.map(|g| g.pose))
                    .collect();
                let seen = &poses[name];
                assert!(seen.is_subset(&allowed), "{name}: {seen:?}");
            }
            assert!(
                poses["dwell:deer"].contains(&IdlePose::Graze),
                "{:?}",
                poses["dwell:deer"]
            );
            let wolf = &poses["dwell:wolf"];
            assert!(
                wolf.contains(&IdlePose::Sit) && wolf.contains(&IdlePose::Lie),
                "{wolf:?}"
            );
        }
    );

    both_faces!(
        fn a_fixture_species_flees_players_and_idles_by_data_alone(f: FaceFrame) {
            // The fixture's fox (shared/ecology/fixtures/ecology.json): `players` flee at
            // `noticeM` 15, `crouchM` 3; idles sit and look up, walking never.
            let data = dwell_ecology::fixture::DATA;
            for (stance, at, flees) in [(WALKING, 10.0, true), (CROUCHING, 10.0, false)] {
                let mut s = Scene::with_data(f, data, &[("test:fox", 0.0, 0.0, 2.0, 0.0)], 2);
                s.stances = vec![stance];
                let mut fled = false;
                let mut poses = std::collections::HashSet::new();
                s.run(&[(-at, 0.0)], 6.0, |n| {
                    for m in n.groups.iter().flat_map(|g| &g.members) {
                        fled |= m.mind == Mind::Fleeing;
                        poses.insert(m.pose);
                    }
                });
                assert_eq!(fled, flees, "{stance:?}");
                if !flees {
                    assert!(
                        poses.is_subset(&[IdlePose::Stand, IdlePose::Sit, IdlePose::LookUp].into()),
                        "{poses:?}"
                    );
                    assert!(poses.contains(&IdlePose::Sit), "{poses:?}");
                }
            }
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
