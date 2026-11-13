//! The world's blueprints on the server (WORK_QUEUE item 55, docs/BLUEPRINTS.md): placed and
//! removed by players where the mode has the `blueprints` ability, sent to every client with the
//! `blueprints.world` capability, saved in the world's optional `blueprints` table, removed once
//! every cell holds its block; and their builders (`blueprints.json` `builder`), NPCs that walk to
//! each next cell and place it, foundation up, with unlimited blocks.

use dwell_protocol::Message;
use dwell_protocol::blueprints::{Blueprint, BlueprintsMessage, list_from_bytes, list_to_bytes};
use dwell_protocol::capability_names::BLUEPRINTS_WORLD;
use dwell_world::blueprint::{Ghost, Rules};

use super::*;

/// How often blueprints are checked for completion (ticks).
const CHECK_EVERY: u32 = SIM_HZ / 2;
/// A builder that hasn't come within reach of its cell for this long places it from where it is
/// (seconds): a cell its walk can't reach (a wall in the way, a cliff) doesn't stop the work.
const STUCK_S: f64 = 2.0;

/// One builder at work.
#[derive(Clone, Debug)]
struct Builder {
    creature: u32,
    /// Ticks until it may place its next piece.
    wait: u32,
    /// Ticks it has been walking toward its cell without reaching it.
    walking: u32,
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
            Some((next, list)) => {
                self.next_id = next.max(1);
                self.list = list
                    .into_iter()
                    .map(|b| (b.clone(), Ghost::of(&b)))
                    .collect();
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
        Some(list_to_bytes(self.next_id, &list))
    }
}

/// Whether the segment `a`→`b` (x, z) passes through the box `bx` × `bz` (shrunk by a little).
fn crosses(bx: [f64; 2], bz: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    let m = 0.2;
    (0..=20).any(|k| {
        let t = f64::from(k) / 20.0;
        let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        p[0] > bx[0] + m && p[0] < bx[1] - m && p[1] > bz[0] + m && p[1] < bz[1] - m
    })
}

/// The next point on the way from `at` to `to` round the box: `to` when the way is clear, else the
/// corner of the ring `out` m round the box that is clear from `at` and nearest the whole way.
fn round_box(bx: [f64; 2], bz: [f64; 2], out: f64, at: [f64; 2], to: [f64; 2]) -> [f64; 2] {
    if !crosses(bx, bz, at, to) {
        return to;
    }
    let d = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]);
    [
        [bx[0] - out, bz[0] - out],
        [bx[1] + out, bz[0] - out],
        [bx[0] - out, bz[1] + out],
        [bx[1] + out, bz[1] + out],
    ]
    .into_iter()
    .filter(|&c| !crosses(bx, bz, at, c) && d(at, c) > 0.3)
    .min_by(|&a, &b| (d(at, a) + d(a, to)).total_cmp(&(d(at, b) + d(b, to))))
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

    /// Removes a blueprint and its builders (removed, or completed).
    pub fn remove_blueprint(&mut self, id: u32) {
        let Some(i) = self.blueprints.list.iter().position(|(b, _)| b.id == id) else {
            return;
        };
        self.blueprints.list.remove(i);
        self.blueprints.dirty = true;
        for w in self.blueprints.builders.remove(&id).unwrap_or_default() {
            self.creatures.remove(w.creature);
        }
        self.broadcast_blueprints(BlueprintsMessage::Removed(id));
    }

    /// Each tick: completed blueprints go; builders come to blueprints with a player near, go from
    /// those with none, and build.
    pub(super) fn step_blueprints(&mut self) {
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
                self.remove_blueprint(id);
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
        let ids: Vec<(u32, bool, bool)> = self
            .blueprints
            .list
            .iter()
            .map(|(b, g)| (b.id, near(g, 1.0), near(g, 1.25)))
            .collect();
        for (id, close, within) in ids {
            let have = self.blueprints.builders.get(&id).map_or(0, Vec::len);
            if !within && have > 0 {
                for w in self.blueprints.builders.remove(&id).unwrap_or_default() {
                    self.creatures.remove(w.creature);
                }
                continue;
            }
            if !close || have >= rules.per_blueprint as usize {
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
                self.blueprints
                    .builders
                    .entry(id)
                    .or_default()
                    .push(Builder {
                        creature: c,
                        wait: 0,
                        walking: 0,
                    });
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
    /// `STUCK_S` without getting there), places it, one piece every `pace_s`.
    fn work_builders(&mut self) {
        let rules = self.blueprints.rules.builder;
        let pace = (rules.pace_s * f64::from(SIM_HZ)).round().max(1.0) as u32;
        let stuck = (STUCK_S * f64::from(SIM_HZ)) as u32;
        let capsules = self.player_capsules();
        let mut placed: Vec<([i32; 3], dwell_blocks::StateId)> = Vec::new();
        let ids: Vec<u32> = self.blueprints.builders.keys().copied().collect();
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
            let spot = self.builder_spot(id);
            let g = g.clone();
            let workers = self.blueprints.builders.get_mut(&id).expect("builders");
            for w in workers.iter_mut() {
                w.wait = w.wait.saturating_sub(1);
                let Some(cr) = self.creatures.get_mut(w.creature) else {
                    continue;
                };
                let next = {
                    let world = &self.world;
                    let get = |c: [i32; 3]| {
                        if placed.iter().any(|p| p.0 == c) {
                            return None;
                        }
                        world.find(chunk_of(c[0], c[1], c[2]))?;
                        Some(world.get_voxel_if_loaded(c))
                    };
                    g.builder_next_near(&get, Some(f.point(cr.position)))
                };
                let Some(cell) = next else {
                    cr.target = None;
                    continue;
                };
                let feet = f.point(cr.position);
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
                        }
                    }
                    continue;
                }
                w.walking += 1;
                // Walled in (or standing where it must build) too long: back to its spot outside.
                if w.walking >= stuck && (own || inside) {
                    if let Some(at) = spot {
                        cr.position = at;
                    }
                    w.walking = 0;
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
                // way clear of the box.
                let to = round_box(bx, bz, out, [feet[0], feet[2]], to);
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

    // --- for tests and tools ---

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

    /// Replaces the blueprints' rules (tests: a faster pace).
    pub fn set_blueprint_rules(&mut self, rules: Rules) {
        self.blueprints.rules = rules;
    }
}
