//! The world's blueprints on the server (WORK_QUEUE item 56, docs/BLUEPRINTS.md): placed and
//! removed by players where the mode has the `blueprints` ability, sent to every client with the
//! `blueprints.world` capability, saved in the world's optional `blueprints` table, removed once
//! every cell holds its block; and their builders (`blueprints.json` `builder`), NPCs that walk to
//! each next cell and place it, foundation up, with unlimited blocks. A town's storehouse (B5,
//! docs/BLUEPRINTS.md §9) is built first; then the builders of the blueprints near it fetch what
//! each needs next from it, up to their job's `carryMax`, carry it to the site, place only what
//! they carried and walk back for more (items move only through `dwell_world::stock::transfer`).

use dwell_protocol::Message;
use dwell_protocol::blueprints::{
    Blueprint, BlueprintsMessage, SavedLoad, SavedStorehouse, SavedWork, list_from_bytes,
    list_to_bytes,
};
use dwell_protocol::capability_names::BLUEPRINTS_WORLD;
use dwell_world::blueprint::{Ghost, Rules};
use dwell_world::stock::{Holder, Load, Stock, transfer_list};

use super::*;

/// How often town sites near players are looked for (ticks).
const TOWNS_EVERY: u32 = SIM_HZ * 2;
/// How many rows above and below the height field's ground a blueprint's anchor looks for the
/// world's own ground (trees, players' edits).
const GROUND_SEARCH_ROWS: i32 = 12;

/// How often blueprints are checked for completion (ticks).
const CHECK_EVERY: u32 = SIM_HZ / 2;
/// A builder that hasn't come within reach of its cell for this long places it from where it is
/// (seconds): a cell its walk can't reach (a wall in the way, a cliff) doesn't stop the work.
const STUCK_S: f64 = 2.0;
/// A builder on a trip (B5) that moved less than this in a tick (m) didn't move.
const STILL_M: f64 = 0.005;
/// A trip leg that takes longer than this is stuck however it moves (seconds).
const TRIP_S: f64 = 60.0;

/// One builder at work.
#[derive(Clone, Debug)]
struct Builder {
    creature: u32,
    /// Ticks until it may place its next piece.
    wait: u32,
    /// Ticks it has been walking toward its cell without reaching it.
    walking: u32,
    /// What it carries (B5; always empty for a blueprint no storehouse serves).
    load: Load,
    /// Where it is going (B5): to the storehouse for a load, or to the site with one.
    trip: Trip,
    /// Where it stood last tick (world m): `walking` counts only the ticks it didn't move on a
    /// trip, so a long walk isn't taken for being stuck.
    last: [f64; 3],
    /// Ticks into this leg (a leg past `TRIP_S` counts as stuck).
    leg: u32,
}

/// A builder's leg of the work loop (B5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trip {
    ToStore,
    ToSite,
}

impl Builder {
    fn new(creature: u32, load: Load) -> Self {
        let trip = if load.is_empty() {
            Trip::ToStore
        } else {
            Trip::ToSite
        };
        Self {
            creature,
            wait: 0,
            walking: 0,
            load,
            trip,
            last: [f64::NAN; 3],
            leg: 0,
        }
    }

    /// Starts the leg `trip`.
    fn go(&mut self, trip: Trip) {
        self.trip = trip;
        self.walking = 0;
        self.leg = 0;
    }

    /// Counts a tick of walking from `at` (world feet): a tick it didn't move adds to the count, a
    /// tick it did clears it; a leg longer than `TRIP_S` is stuck however it moves.
    fn walked(&mut self, at: [f64; 3], stuck: u32) {
        let moved = (0..3)
            .map(|k| (at[k] - self.last[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        self.last = at;
        self.leg += 1;
        if moved.is_nan() || moved >= STILL_M {
            self.walking = 0;
        } else {
            self.walking += 1;
        }
        if self.leg >= (TRIP_S * f64::from(SIM_HZ)) as u32 {
            self.walking = self.walking.max(stuck);
        }
    }
}

/// A town's storehouse (B5): its box (world cells), the blueprint still building it (0: built,
/// open) and its stock.
#[derive(Clone, Debug)]
pub(crate) struct Storehouse {
    pub(crate) id: u32,
    pub(crate) face: FaceFrame,
    pub(crate) min: [i32; 3],
    pub(crate) max: [i32; 3],
    pub(crate) blueprint: u32,
    pub(crate) stock: Stock,
}

impl Storehouse {
    /// Its box's centre column (face-local m, x and z).
    fn centre(&self) -> [f64; 2] {
        let (lo, hi) = (self.face.cell(self.min), self.face.cell(self.max));
        [
            f64::from(lo[0] + hi[0] + 1) / 2.0,
            f64::from(lo[2] + hi[2] + 1) / 2.0,
        ]
    }

    /// Its box, face-local metres (x and z).
    fn bounds(&self) -> ([f64; 2], [f64; 2]) {
        let (lo, hi) = (self.face.cell(self.min), self.face.cell(self.max));
        (
            [f64::from(lo[0].min(hi[0])), f64::from(lo[0].max(hi[0]) + 1)],
            [f64::from(lo[2].min(hi[2])), f64::from(lo[2].max(hi[2]) + 1)],
        )
    }
}

/// The flat distance from (x, z) to the box `bx` × `bz` (0 inside).
fn to_box(bx: [f64; 2], bz: [f64; 2], p: [f64; 2]) -> f64 {
    let dx = (bx[0] - p[0]).max(p[0] - bx[1]).max(0.0);
    let dz = (bz[0] - p[1]).max(p[1] - bz[1]).max(0.0);
    dx.hypot(dz)
}

/// Where a builder stands to take from a storehouse: the point `out` m outside its box nearest
/// `p` (face-local x and z).
fn beside_box(bx: [f64; 2], bz: [f64; 2], out: f64, p: [f64; 2]) -> [f64; 2] {
    let c = [
        p[0].clamp(bx[0] - out, bx[1] + out),
        p[1].clamp(bz[0] - out, bz[1] + out),
    ];
    let inside =
        c[0] > bx[0] - out && c[0] < bx[1] + out && c[1] > bz[0] - out && c[1] < bz[1] + out;
    if !inside {
        return c;
    }
    // Inside the ring: out by the nearest side.
    [
        [bx[0] - out, c[1]],
        [bx[1] + out, c[1]],
        [c[0], bz[0] - out],
        [c[0], bz[1] + out],
    ]
    .into_iter()
    .min_by(|a, b| {
        let d = |q: &[f64; 2]| (q[0] - p[0]).hypot(q[1] - p[1]);
        d(a).total_cmp(&d(b))
    })
    .expect("four sides")
}

/// The blueprints, their ghosts and their builders.
pub(crate) struct Blueprints {
    pub(crate) rules: Rules,
    list: Vec<(Blueprint, Ghost)>,
    next_id: u32,
    /// A saved record this build couldn't read (a newer one's): kept, not written over.
    unread: bool,
    dirty: bool,
    builders: HashMap<u32, Vec<Builder>>,
    /// The town sites founded (face, grid cell): their blueprints placed once (B4).
    founded: std::collections::BTreeSet<(u8, i32, i32)>,
    /// The ground town sites need (none: a world without a height field has no towns).
    pub(crate) town_ground: Option<dwell_worldgen::TownGround>,
    /// The towns' storehouses (B5).
    pub(crate) storehouses: Vec<Storehouse>,
    next_storehouse: u32,
    /// The loads of builders not standing now (nobody near, or just loaded), by blueprint: a
    /// builder that comes back takes its load up again.
    kept: HashMap<u32, Vec<Load>>,
}

impl Default for Blueprints {
    fn default() -> Self {
        Self {
            rules: dwell_world::blueprint::SHIPPED,
            list: Vec::new(),
            next_id: 1,
            unread: false,
            dirty: false,
            builders: HashMap::default(),
            founded: Default::default(),
            town_ground: None,
            storehouses: Vec::new(),
            next_storehouse: 1,
            kept: HashMap::default(),
        }
    }
}

impl Blueprints {
    /// The saved record (none: a world from before blueprints).
    pub(crate) fn restore(&mut self, bytes: Option<Vec<u8>>) {
        let Some(bytes) = bytes else {
            return;
        };
        match list_from_bytes(&bytes) {
            Some((next, list, towns, work)) => {
                self.next_id = next.max(1);
                self.founded = towns.into_iter().collect();
                self.list = list
                    .into_iter()
                    .map(|b| (b.clone(), Ghost::of(&b)))
                    .collect();
                self.restore_work(work);
            }
            None => self.unread = true,
        }
    }

    /// The record to save, when it changed (and was read).
    pub(crate) fn record_to_save(&mut self) -> Option<Vec<u8>> {
        if self.unread || !self.dirty {
            return None;
        }
        self.dirty = false;
        let list: Vec<Blueprint> = self.list.iter().map(|(b, _)| b.clone()).collect();
        let towns: Vec<(u8, i32, i32)> = self.founded.iter().copied().collect();
        Some(list_to_bytes(self.next_id, &list, &towns, &self.work()))
    }

    /// The saved work (format 3): the storehouses and every builder's load (standing or kept).
    fn work(&self) -> SavedWork {
        let names = |items: &[(dwell_blocks::StateId, u32)]| -> Vec<(String, u32)> {
            items
                .iter()
                .map(|&(s, n)| (dwell_blocks::info(s).name.to_string(), n))
                .collect()
        };
        let storehouses = self
            .storehouses
            .iter()
            .map(|s| SavedStorehouse {
                id: s.id,
                face: dwell_world::blueprint::face_byte(s.face),
                min: s.min,
                max: s.max,
                blueprint: s.blueprint,
                counted: match &s.stock {
                    Stock::Unlimited => None,
                    Stock::Counted(items) => Some(names(items)),
                },
            })
            .collect();
        let mut ids: Vec<u32> = self
            .builders
            .keys()
            .chain(self.kept.keys())
            .copied()
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let mut loads = Vec::new();
        for id in ids {
            let standing = self
                .builders
                .get(&id)
                .into_iter()
                .flatten()
                .map(|w| &w.load);
            for l in standing.chain(self.kept.get(&id).into_iter().flatten()) {
                if !l.is_empty() {
                    loads.push(SavedLoad {
                        blueprint: id,
                        items: names(&l.items),
                    });
                }
            }
        }
        SavedWork {
            next_storehouse: self.next_storehouse,
            storehouses,
            loads,
        }
    }

    /// The saved work back: the storehouses, and the loads kept for their builders' return.
    fn restore_work(&mut self, work: SavedWork) {
        let cap = self.rules.builder_job().carry_max;
        let states = |items: Vec<(String, u32)>| -> Vec<(dwell_blocks::StateId, u32)> {
            items
                .into_iter()
                .filter_map(|(n, c)| Some((dwell_blocks::parse_state(&n).ok()?, c)))
                .collect()
        };
        self.next_storehouse = work.next_storehouse.max(1);
        self.storehouses = work
            .storehouses
            .into_iter()
            .map(|s| Storehouse {
                id: s.id,
                face: FaceFrame::of(if s.face == 1 {
                    dwell_world::Face::B
                } else {
                    dwell_world::Face::A
                }),
                min: s.min,
                max: s.max,
                blueprint: s.blueprint,
                stock: s
                    .counted
                    .map_or(Stock::Unlimited, |c| Stock::Counted(states(c))),
            })
            .collect();
        for l in work.loads {
            let mut load = Load::new(cap);
            for (state, n) in states(l.items) {
                load.take(state, n);
            }
            self.kept.entry(l.blueprint).or_default().push(load);
        }
    }

    /// The storehouse serving blueprint `id` (index into `storehouses`): the nearest on its face
    /// within `range_m` of it, not the one it builds. None: B3's unlimited builders.
    fn store_for(&self, id: u32, g: &Ghost) -> Option<usize> {
        let (lo, hi) = (g.face.cell(g.min), g.face.cell(g.max));
        let c = [
            f64::from(lo[0] + hi[0] + 1) / 2.0,
            f64::from(lo[2] + hi[2] + 1) / 2.0,
        ];
        self.storehouses
            .iter()
            .enumerate()
            .filter(|(_, s)| s.face.is_b() == g.face.is_b() && s.blueprint != id)
            .map(|(i, s)| {
                let m = s.centre();
                (i, (m[0] - c[0]).hypot(m[1] - c[1]))
            })
            .filter(|&(_, d)| d <= self.rules.storehouse.range_m)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Whether blueprint `id` waits for its storehouse to be built first.
    fn waits_for_store(&self, id: u32, g: &Ghost) -> bool {
        self.store_for(id, g)
            .is_some_and(|i| self.storehouses[i].blueprint != 0)
    }
}

/// Whether the segment `a`→`b` (x, z) passes through the box `bx` × `bz` grown by a body's
/// width, leaving out its first steps (a builder standing against the box's side may walk away
/// from it). Before B5 the box was shrunk instead, so a builder standing within its margin walked
/// straight into the wall.
fn crosses(bx: [f64; 2], bz: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    let m = 0.35;
    (1..=40).any(|k| {
        let t = f64::from(k) / 40.0;
        let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        (p[0] - a[0]).hypot(p[1] - a[1]) > 0.45
            && p[0] > bx[0] - m
            && p[0] < bx[1] + m
            && p[1] > bz[0] - m
            && p[1] < bz[1] + m
    })
}

/// The next point on the way from `at` to `to` round the box: `to` when the way is clear, else the
/// corner of the ring `out` m round the box that is clear from `at` and gives the shortest way on,
/// by one corner that sees `to` or, from the far side, by two. (Before B5 the second leg wasn't
/// counted, so a builder at one corner bound for the far side picked the corner it stood at and
/// the next one in turn, and stood still between them.)
fn round_box(bx: [f64; 2], bz: [f64; 2], out: f64, at: [f64; 2], to: [f64; 2]) -> [f64; 2] {
    if !crosses(bx, bz, at, to) {
        return to;
    }
    let d = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]);
    let corners = [
        [bx[0] - out, bz[0] - out],
        [bx[1] + out, bz[0] - out],
        [bx[0] - out, bz[1] + out],
        [bx[1] + out, bz[1] + out],
    ];
    // The way on from corner `c`: straight to `to`, else by another corner that sees it.
    let on = |c: [f64; 2]| -> f64 {
        if !crosses(bx, bz, c, to) {
            return d(c, to);
        }
        corners
            .iter()
            .filter(|&&c2| d(c, c2) > 0.3 && !crosses(bx, bz, c, c2) && !crosses(bx, bz, c2, to))
            .map(|&c2| d(c, c2) + d(c2, to))
            .fold(f64::INFINITY, f64::min)
    };
    corners
        .into_iter()
        .filter(|&c| !crosses(bx, bz, at, c) && d(at, c) > 0.3)
        .min_by(|&a, &b| (d(at, a) + on(a)).total_cmp(&(d(at, b) + on(b))))
        .unwrap_or(to)
}

impl Server {
    /// Sends the world's blueprints once to a client that said `blueprints.world`.
    pub(super) fn send_blueprints(&mut self, id: SessionId) {
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        if s.blueprints_sent || !s.joined() || !s.capabilities.has(BLUEPRINTS_WORLD) {
            return;
        }
        s.blueprints_sent = true;
        let all: Vec<Blueprint> = self
            .blueprints
            .list
            .iter()
            .map(|(b, _)| b.clone())
            .collect();
        let m = Message::Blueprints(BlueprintsMessage::All(all));
        self.send_reliable(id, &m, Channel::World);
    }

    /// Tells every client with the capability.
    fn broadcast_blueprints(&mut self, m: BlueprintsMessage) {
        let m = Message::Blueprints(m);
        let ids: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.blueprints_sent)
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            self.send_reliable(id, &m, Channel::World);
        }
    }

    /// A client's place or removal: only from a joined client with the capability that may edit,
    /// where the mode has the `blueprints` ability.
    pub(super) fn blueprints_message(&mut self, id: SessionId, m: BlueprintsMessage) {
        let Some(s) = self.sessions.get(&id) else {
            return;
        };
        if !s.joined()
            || !s.capabilities.has(BLUEPRINTS_WORLD)
            || !self.may_edit(s)
            || !self.settings.allows(Ability::Blueprints)
        {
            return;
        }
        match m {
            BlueprintsMessage::Place(b) => {
                self.place_blueprint(b);
            }
            BlueprintsMessage::Remove(bid) => self.remove_blueprint(bid),
            _ => {}
        }
    }

    /// Places a blueprint (a player's, or the world's own, e.g. a town's): its id, or None past
    /// the world's most, with nothing to build, or outside the world.
    pub fn place_blueprint(&mut self, mut b: Blueprint) -> Option<u32> {
        if self.blueprints.list.len() >= self.blueprints.rules.per_world {
            return None;
        }
        let ghost = Ghost::of(&b);
        if ghost.cells.is_empty() {
            return None;
        }
        let size: [i32; 3] = std::array::from_fn(|i| ghost.max[i] - ghost.min[i] + 1);
        if !dwell_world::region::box_in_world_of(&self.bounds(), ghost.min, size) {
            return None;
        }
        b.id = self.blueprints.next_id;
        self.blueprints.next_id = self.blueprints.next_id.wrapping_add(1).max(1);
        let mut ghost = ghost;
        ghost.id = b.id;
        self.blueprints.list.push((b.clone(), ghost));
        self.blueprints.dirty = true;
        self.broadcast_blueprints(BlueprintsMessage::Added(b.clone()));
        Some(b.id)
    }

    /// Removes a blueprint and its builders (a player's removal): a storehouse it was building
    /// is given up (the town's blueprints then build as B3's).
    pub fn remove_blueprint(&mut self, id: u32) {
        self.blueprints.storehouses.retain(|s| s.blueprint != id);
        self.drop_blueprint(id, false);
    }

    /// A blueprint every cell of which holds its block: gone, a storehouse it built opened, its
    /// builders on to the next blueprint their storehouse serves (B5), else gone too.
    fn complete_blueprint(&mut self, id: u32) {
        for s in &mut self.blueprints.storehouses {
            if s.blueprint == id {
                s.blueprint = 0;
            }
        }
        self.drop_blueprint(id, true);
    }

    fn drop_blueprint(&mut self, id: u32, completed: bool) {
        let Some(i) = self.blueprints.list.iter().position(|(b, _)| b.id == id) else {
            return;
        };
        let (_, ghost) = self.blueprints.list.remove(i);
        self.blueprints.dirty = true;
        self.blueprints.kept.remove(&id);
        let store = self.blueprints.store_for(id, &ghost);
        for mut w in self.blueprints.builders.remove(&id).unwrap_or_default() {
            // On to the next blueprint near (its storehouse's, or the one this built).
            let next = completed
                .then(|| self.next_blueprint_for(&ghost, store, id))
                .flatten();
            match next {
                Some(to) => {
                    w.go(if w.load.is_empty() {
                        Trip::ToStore
                    } else {
                        Trip::ToSite
                    });
                    w.wait = 0;
                    self.blueprints.builders.entry(to).or_default().push(w);
                }
                None => {
                    // What it carried goes back to the storehouse.
                    if let Some(si) = store {
                        w.load
                            .empty_into(&mut self.blueprints.storehouses[si].stock);
                    }
                    self.creatures.remove(w.creature);
                }
            }
        }
        self.broadcast_blueprints(BlueprintsMessage::Removed(id));
    }

    /// The blueprint a builder of the completed one (`done`, its storehouse `store`) moves on to:
    /// the nearest on its face served by the same storehouse (or by the storehouse `done` built)
    /// whose storehouse is open, with room for a builder.
    fn next_blueprint_for(&self, done: &Ghost, store: Option<usize>, id: u32) -> Option<u32> {
        let built = self.blueprints.storehouses.iter().position(|s| {
            s.blueprint == 0
                && s.face.is_b() == done.face.is_b()
                && s.min == done.min
                && s.max == done.max
        });
        let mine = store.or(built)?;
        let per = self.blueprints.rules.builder.per_blueprint as usize;
        let c = |g: &Ghost| -> [f64; 2] {
            [
                f64::from(g.min[0] + g.max[0]) / 2.0,
                f64::from(g.min[2] + g.max[2]) / 2.0,
            ]
        };
        let here = c(done);
        self.blueprints
            .list
            .iter()
            .filter(|(b, g)| {
                b.id != id
                    && self.blueprints.store_for(b.id, g) == Some(mine)
                    && !self.blueprints.waits_for_store(b.id, g)
                    && self.blueprints.builders.get(&b.id).map_or(0, Vec::len) < per
            })
            .min_by(|(_, a), (_, b)| {
                let d = |g: &Ghost| {
                    let p = c(g);
                    (p[0] - here[0]).hypot(p[1] - here[1])
                };
                d(a).total_cmp(&d(b))
            })
            .map(|(b, _)| b.id)
    }

    /// Each tick: completed blueprints go; builders come to blueprints with a player near, go from
    /// those with none, and build.
    pub(super) fn step_blueprints(&mut self) {
        if self.tick.is_multiple_of(TOWNS_EVERY) {
            self.found_towns();
        }
        if self.blueprints.list.is_empty() {
            return;
        }
        if self.tick.is_multiple_of(CHECK_EVERY) {
            let done: Vec<u32> = {
                let world = &self.world;
                let get = |c: [i32; 3]| {
                    world.find(chunk_of(c[0], c[1], c[2]))?;
                    Some(world.get_voxel_if_loaded(c))
                };
                self.blueprints
                    .list
                    .iter()
                    .filter(|(_, g)| g.complete(&get) == Some(true))
                    .map(|(b, _)| b.id)
                    .collect()
            };
            for id in done {
                self.complete_blueprint(id);
            }
        }
        self.staff_builders();
        self.work_builders();
    }

    /// Builders for blueprints with a living player within `near_m` (same face); none for those
    /// with nobody near (they come back when someone does).
    fn staff_builders(&mut self) {
        let rules = self.blueprints.rules.builder;
        let Some(kind) = self.creatures.kind_of(rules.creature) else {
            return;
        };
        let players = self.living_points();
        let near = |g: &Ghost, slack: f64| {
            let c: [f64; 3] = std::array::from_fn(|i| f64::from(g.min[i] + g.max[i]) / 2.0);
            let centre = [c[0] + 0.5, dwell_world::m_of_row(c[1] as i32), c[2] + 0.5];
            players.iter().any(|p| {
                FaceFrame::at_height(p[1]).is_b() == g.face.is_b()
                    && (p[0] - centre[0]).hypot(p[2] - centre[2]) <= rules.near_m * slack
            })
        };
        let ids: Vec<(u32, bool, bool, bool)> = self
            .blueprints
            .list
            .iter()
            .map(|(b, g)| {
                let waits = self.blueprints.waits_for_store(b.id, g);
                (b.id, near(g, 1.0), near(g, 1.25), waits)
            })
            .collect();
        let cap = self.blueprints.rules.builder_job().carry_max;
        for (id, close, within, waits) in ids {
            let have = self.blueprints.builders.get(&id).map_or(0, Vec::len);
            if !within && have > 0 {
                for w in self.blueprints.builders.remove(&id).unwrap_or_default() {
                    self.creatures.remove(w.creature);
                    // Its load kept for when it comes back.
                    if !w.load.is_empty() {
                        self.blueprints.kept.entry(id).or_default().push(w.load);
                    }
                }
                continue;
            }
            // A town's blueprints wait for its storehouse to be built first (B5).
            if !close || waits || have >= rules.per_blueprint as usize {
                continue;
            }
            let Some(at) = self.builder_spot(id) else {
                continue;
            };
            let rng = self.config.world_seed ^ u64::from(id).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            if let Some(c) = self.creatures.add_driven(kind, at, 0.0, rng) {
                if let Some(cr) = self.creatures.get_mut(c) {
                    cr.variant = (rng >> 32) as u32 ^ have as u32;
                }
                let load = self
                    .blueprints
                    .kept
                    .get_mut(&id)
                    .and_then(Vec::pop)
                    .unwrap_or_else(|| Load::new(cap));
                self.blueprints
                    .builders
                    .entry(id)
                    .or_default()
                    .push(Builder::new(c, load));
            }
        }
    }

    /// Where a builder stands up: the column just outside the blueprint's box (its face-local
    /// lower corner, one cell out), on the ground near its anchor's row. None while unloaded.
    fn builder_spot(&self, id: u32) -> Option<[f64; 3]> {
        let (b, g) = self.blueprints.list.iter().find(|(b, _)| b.id == id)?;
        let f = g.face;
        let world = &self.world;
        let get = |c: [i32; 3]| {
            world.find(chunk_of(c[0], c[1], c[2]))?;
            Some(world.get_voxel_if_loaded(c))
        };
        let rows = self
            .creatures
            .kind_of(self.blueprints.rules.builder.creature)
            .map_or(4, |k| self.creatures.table.creatures[k].rows);
        let (lo, hi) = (f.cell(g.min), f.cell(g.max));
        let (x0, z0) = (lo[0].min(hi[0]) - 1, lo[2].min(hi[2]) - 1);
        for (x, z) in [(x0, z0), (x0, b.at[2]), (b.at[0], z0)] {
            if let Some(y) = crate::creatures::feet_row(&get, f, [x, z], b.at[1], 16, 16, rows) {
                let feet = dwell_world::m_of_row(y);
                return Some(f.point([f64::from(x) + 0.5, feet, f64::from(z) + 0.5]));
            }
        }
        None
    }

    /// Each builder walks toward its blueprint's next cell and, within reach (or after walking
    /// `STUCK_S` without getting there), places it, one piece every `pace_s`. A blueprint a
    /// storehouse serves (B5) is built by trips: at the storehouse a builder puts back what it
    /// still carries and takes the next load (the next cells in build order, at most `carryMax`),
    /// at the site it places only what it carries, and when empty (or when nothing it carries can
    /// go in) it walks back.
    fn work_builders(&mut self) {
        let rules = self.blueprints.rules.builder;
        let job = *self.blueprints.rules.builder_job();
        let store_reach = self.blueprints.rules.storehouse.reach_m;
        let pace = (rules.pace_s * f64::from(SIM_HZ)).round().max(1.0) as u32;
        let stuck = (STUCK_S * f64::from(SIM_HZ)) as u32;
        let wait_ticks = (job.wait_s * f64::from(SIM_HZ)).round().max(1.0) as u32;
        let capsules = self.player_capsules();
        let mut placed: Vec<([i32; 3], dwell_blocks::StateId)> = Vec::new();
        let mut ids: Vec<u32> = self.blueprints.builders.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let Some(i) = self.blueprints.list.iter().position(|(b, _)| b.id == id) else {
                continue;
            };
            let g = &self.blueprints.list[i].1;
            let f = g.face;
            // The box, face-local metres (x and z).
            let (lo, hi) = (f.cell(g.min), f.cell(g.max));
            let bx = [f64::from(lo[0].min(hi[0])), f64::from(lo[0].max(hi[0]) + 1)];
            let bz = [f64::from(lo[2].min(hi[2])), f64::from(lo[2].max(hi[2]) + 1)];
            let store = self
                .blueprints
                .store_for(id, g)
                .filter(|&si| self.blueprints.storehouses[si].blueprint == 0)
                .map(|si| (si, self.blueprints.storehouses[si].bounds()));
            let spot = self.builder_spot(id);
            let g = g.clone();
            let bp = &mut self.blueprints;
            let workers = bp.builders.get_mut(&id).expect("builders");
            for w in workers.iter_mut() {
                w.wait = w.wait.saturating_sub(1);
                let Some(cr) = self.creatures.get_mut(w.creature) else {
                    continue;
                };
                let world = &self.world;
                let get = |c: [i32; 3]| {
                    if placed.iter().any(|p| p.0 == c) {
                        return None;
                    }
                    world.find(chunk_of(c[0], c[1], c[2]))?;
                    Some(world.get_voxel_if_loaded(c))
                };
                let feet = f.point(cr.position);
                // To the storehouse for a load (B5).
                if let Some((si, (sbx, sbz))) = store
                    && w.trip == Trip::ToStore
                {
                    let p = [feet[0], feet[2]];
                    let d = to_box(sbx, sbz, p);
                    w.walked(cr.position, stuck);
                    if d <= store_reach || w.walking >= stuck {
                        cr.target = None;
                        if w.wait > 0 {
                            continue;
                        }
                        let stock = &mut bp.storehouses[si].stock;
                        w.load.cap = job.carry_max;
                        w.load.empty_into(stock);
                        let want = g.next_load(&get, None, job.carry_max, job.mixed_loads);
                        transfer_list(stock, &mut w.load, &want);
                        bp.dirty = true;
                        if w.load.is_empty() {
                            // Nothing it can put now, or the storehouse lacks it: it waits.
                            w.wait = wait_ticks;
                        } else {
                            w.go(Trip::ToSite);
                        }
                        continue;
                    }
                    let to = beside_box(sbx, sbz, 1.0, p);
                    let to = round_box(bx, bz, 1.5, p, round_box(sbx, sbz, 1.0, p, to));
                    if cr
                        .target
                        .is_none_or(|t| (t[0] - to[0]).hypot(t[1] - to[1]) > 0.5)
                    {
                        cr.target = Some(to);
                    }
                    continue;
                }
                let trips = store.is_some();
                let next = if trips {
                    g.builder_next_of(&get, Some(feet), &|s| w.load.has(s))
                } else {
                    g.builder_next_near(&get, Some(feet))
                };
                let Some(cell) = next else {
                    cr.target = None;
                    if trips {
                        // Nothing it carries can go in: back for what can.
                        w.go(Trip::ToStore);
                    }
                    continue;
                };
                let centre = f.point(dwell_world::centre_of_cell(cell.world));
                let (dx, dz) = (centre[0] - feet[0], centre[2] - feet[2]);
                let flat = dx.hypot(dz);
                let reach = flat <= rules.reach_m && (centre[1] - feet[1]).abs() <= rules.reach_m;
                // Never into its own body's column (it would wall itself in).
                let own = flat < 0.75 && centre[1] >= feet[1] - 0.25;
                let inside =
                    feet[0] > bx[0] && feet[0] < bx[1] && feet[2] > bz[0] && feet[2] < bz[1];
                if !own && (reach || w.walking >= stuck) {
                    cr.target = None;
                    if w.wait == 0 {
                        let into_player =
                            dwell_world::edit::overlaps_player(cell.state, cell.world, &capsules);
                        if !into_player {
                            placed.push((cell.world, cell.state));
                            w.wait = pace;
                            w.walking = 0;
                            if trips {
                                w.load.give(cell.state, 1);
                                bp.dirty = true;
                                if w.load.is_empty() {
                                    w.go(Trip::ToStore);
                                }
                            }
                        }
                    }
                    continue;
                }
                if trips {
                    w.walked(cr.position, stuck);
                } else {
                    w.walking += 1;
                }
                // Walled in (or standing where it must build) too long: back to its spot outside.
                if w.walking >= stuck && (own || inside) {
                    if let Some(at) = spot {
                        cr.position = at;
                    }
                    w.walking = 0;
                    w.leg = 0;
                    cr.target = None;
                    continue;
                }
                // To the side of the box nearest the cell, 1.5 m outside it, level with the cell:
                // builders work from outside, so they never build themselves in.
                let out = 1.5;
                let sides = [
                    [bx[0] - out, centre[2]],
                    [bx[1] + out, centre[2]],
                    [centre[0], bz[0] - out],
                    [centre[0], bz[1] + out],
                ];
                let to = sides
                    .into_iter()
                    .min_by(|a, b| {
                        let d = |p: &[f64; 2]| (p[0] - centre[0]).hypot(p[1] - centre[2]);
                        let e = |p: &[f64; 2]| (p[0] - feet[0]).hypot(p[1] - feet[2]);
                        (d(a) + 0.25 * e(a)).total_cmp(&(d(b) + 0.25 * e(b)))
                    })
                    .expect("four sides");
                // Round the box, not through it: by the corner of the ring 1.5 m out that keeps the
                // way clear of the box (and round its storehouse).
                let at = [feet[0], feet[2]];
                let to = round_box(bx, bz, out, at, to);
                let to = match store {
                    Some((_, (sbx, sbz))) => round_box(sbx, sbz, 1.0, at, to),
                    None => to,
                };
                if cr
                    .target
                    .is_none_or(|t| (t[0] - to[0]).hypot(t[1] - to[1]) > 0.5)
                {
                    cr.target = Some(to);
                }
            }
        }
        if placed.is_empty() {
            return;
        }
        let mut changes = TickChanges::default();
        for (cell, state) in placed {
            changes.set_and_unsupport(&mut self.world, cell, state);
        }
        self.broadcast_changes(changes, VoxelModificationReason::Edit);
    }

    /// Town sites (B4, docs/BLUEPRINTS.md §6): each site within `found_m` of a living player
    /// (its face's) not yet founded is founded once its blueprints' ground is loaded: each of its
    /// plan's blueprints placed with its anchor on the ground under it, then the site recorded.
    fn found_towns(&mut self) {
        let Some(ground) = self.blueprints.town_ground.clone() else {
            return;
        };
        let rules = self.blueprints.rules.towns;
        let seed = self.config.world_seed;
        let mut sites = Vec::new();
        for p in self.living_points() {
            let f = FaceFrame::at_height(p[1]);
            let l = f.point(p);
            let g = |x: i32, z: i32| ground(f.face, x, z);
            for t in rules.sites_near(seed, f, l[0], l[2], rules.found_m, &g) {
                let key = (
                    dwell_world::blueprint::face_byte(t.face),
                    t.cell[0],
                    t.cell[1],
                );
                if !self.blueprints.founded.contains(&key) && !sites.contains(&(key, t)) {
                    sites.push((key, t));
                }
            }
        }
        for (key, t) in sites {
            let f = t.face;
            let mut placed = Vec::new();
            let mut ready = true;
            for (name, [x, z], turn, role) in rules.layout_roles(&t) {
                let Ok(s) = dwell_world::structure::load(name) else {
                    continue;
                };
                let Some(h) = ground(f.face, x, z) else {
                    continue; // that blueprint's column is water: left out
                };
                let row = dwell_world::rows_for_m(h);
                let feet = {
                    let world = &self.world;
                    let get = |c: [i32; 3]| {
                        world.find(chunk_of(c[0], c[1], c[2]))?;
                        Some(world.get_voxel_if_loaded(c))
                    };
                    if get(f.cell([x, row, z])).is_none() {
                        ready = false;
                        break;
                    }
                    crate::creatures::feet_row(
                        &get,
                        f,
                        [x, z],
                        row,
                        GROUND_SEARCH_ROWS,
                        GROUND_SEARCH_ROWS,
                        1,
                    )
                };
                // No ground within the search where the height field has it (not generated yet,
                // or dug away): the site waits.
                let Some(y) = feet else {
                    ready = false;
                    break;
                };
                if let Some(b) = dwell_world::blueprint::record_of(&s, f, [x, y, z], turn) {
                    placed.push((b, role));
                }
            }
            if !ready {
                continue; // its ground isn't loaded yet: next time
            }
            for (b, role) in placed {
                let Some(id) = self.place_blueprint(b) else {
                    continue;
                };
                if role == dwell_world::blueprint::Role::Storehouse {
                    self.add_storehouse(id);
                }
            }
            self.blueprints.founded.insert(key);
            self.blueprints.dirty = true;
        }
    }

    /// Makes blueprint `id` a storehouse (B5): built first, its stock the data's mode, then the
    /// hand-off point of the blueprints near it. Its id.
    pub fn add_storehouse(&mut self, id: u32) -> Option<u32> {
        let g = &self.blueprints.list.iter().find(|(b, _)| b.id == id)?.1;
        let sid = self.blueprints.next_storehouse;
        self.blueprints.next_storehouse = sid.wrapping_add(1).max(1);
        let s = Storehouse {
            id: sid,
            face: g.face,
            min: g.min,
            max: g.max,
            blueprint: id,
            stock: Stock::of(self.blueprints.rules.storehouse.stock),
        };
        self.blueprints.storehouses.push(s);
        self.blueprints.dirty = true;
        Some(sid)
    }

    // --- for tests and tools ---

    /// The towns founded (face, grid cell).
    pub fn towns_founded(&self) -> Vec<(u8, i32, i32)> {
        self.blueprints.founded.iter().copied().collect()
    }

    /// Replaces the ground town sites need (tests: a flat one; none: no towns).
    pub fn set_town_ground(&mut self, g: Option<dwell_worldgen::TownGround>) {
        self.blueprints.town_ground = g;
    }

    /// The world's blueprints.
    pub fn blueprint_list(&self) -> Vec<Blueprint> {
        self.blueprints
            .list
            .iter()
            .map(|(b, _)| b.clone())
            .collect()
    }

    /// The creatures building blueprint `id`.
    pub fn builders_of(&self, id: u32) -> Vec<u32> {
        self.blueprints
            .builders
            .get(&id)
            .map_or(Vec::new(), |w| w.iter().map(|w| w.creature).collect())
    }

    /// The storehouses: each one's id, its box (world cells) and whether it is built (open).
    pub fn storehouses(&self) -> Vec<(u32, [i32; 3], [i32; 3], bool)> {
        self.blueprints
            .storehouses
            .iter()
            .map(|s| (s.id, s.min, s.max, s.blueprint == 0))
            .collect()
    }

    /// What builder creature `creature` carries (block, count), in load order; None: not a
    /// builder.
    pub fn builder_load(&self, creature: u32) -> Option<Vec<(dwell_blocks::StateId, u32)>> {
        self.blueprints
            .builders
            .values()
            .flatten()
            .find(|w| w.creature == creature)
            .map(|w| w.load.items.clone())
    }

    /// Each standing builder's creature and load (the `npcs.carry` part).
    pub(crate) fn builder_loads(&self) -> HashMap<u32, Vec<(dwell_blocks::StateId, u32)>> {
        self.blueprints
            .builders
            .values()
            .flatten()
            .filter(|w| !w.load.is_empty())
            .map(|w| (w.creature, w.load.items.clone()))
            .collect()
    }

    /// Replaces the blueprints' rules (tests: a faster pace).
    pub fn set_blueprint_rules(&mut self, rules: Rules) {
        self.blueprints.rules = rules;
    }
}
