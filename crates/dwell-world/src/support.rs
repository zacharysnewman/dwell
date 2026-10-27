//! Support: which blocks lose their hold when something is removed (docs/FELLING.md §2).
//!
//! Each block has a [`SupportRole`] (block data `support`): `Ground` anchors whatever touches it,
//! a `Carrier` (a log) stands while a path of carriers reaches a ground block it touches, and a
//! `Rider` (leaves) holds nothing up, staying while the carrier it belongs to stands. After a
//! removal the cells round it are searched ([`detached_pieces`]): each component of carriers no
//! longer reaching the ground, with the riders that belong to it, is one piece to fall; riders
//! that belong to no carrier fall on their own. The search is bounded: a system of carriers and
//! riders larger than `max_cells` is left standing (and judged again by a later change).

use std::collections::{HashMap, VecDeque};

use dwell_blocks::{StateId, SupportRole, info};

use crate::VoxelWorld;

/// The six face neighbours' offsets (cells).
const FACES: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

/// The search's bounds (docs/FELLING.md §5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupportRules {
    /// How far (cells, through riders) a rider belongs to the carrier nearest it.
    pub rider_reach: u32,
    /// The most carriers and riders one search looks at: a larger system stands.
    pub max_cells: usize,
}

impl Default for SupportRules {
    fn default() -> Self {
        Self {
            rider_reach: 6,
            max_cells: 16_384,
        }
    }
}

/// A piece that lost its support: its cells (world cells), carriers first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Piece {
    pub cells: Vec<[i32; 3]>,
    /// How many of `cells` (the first ones) are carriers; 0 for riders falling on their own.
    pub carriers: usize,
}

/// The role a cell's block plays (air and liquids are none).
fn role(m: StateId) -> Option<SupportRole> {
    let s = info(m);
    (s.solid && !s.liquid).then_some(s.support_role)
}

/// Whether `m` anchors a carrier touching it: a solid ground block (not air, a liquid or a plant).
fn anchors(m: StateId) -> bool {
    let s = info(m);
    s.solid && !s.liquid && !s.passable && s.support_role == SupportRole::Ground
}

fn add(a: [i32; 3], d: [i32; 3]) -> [i32; 3] {
    [a[0] + d[0], a[1] + d[1], a[2] + d[2]]
}

/// The pieces left without support once the cells `changed` have been cleared (or changed): the
/// carriers and riders round them are searched (each system once), and every component of carriers
/// with no path to the ground, with its riders, is returned, then the riders that belong to no
/// carrier. Deterministic: the same world and cells give the same pieces in the same order.
pub fn detached_pieces(
    world: &mut VoxelWorld,
    changed: &[[i32; 3]],
    rules: SupportRules,
) -> Vec<Piece> {
    let mut seen: HashMap<[i32; 3], SupportRole> = HashMap::new();
    let mut pieces = Vec::new();
    for &c in changed {
        for d in std::iter::once([0, 0, 0]).chain(FACES) {
            let start = add(c, d);
            if seen.contains_key(&start) {
                continue;
            }
            let Some(r) = role(world.get_voxel(start[0], start[1], start[2])) else {
                continue;
            };
            if r == SupportRole::Ground {
                continue;
            }
            if let Some(found) = system(world, start, rules, &mut seen) {
                pieces.extend(found);
            }
        }
    }
    pieces
}

/// The system of carriers and riders connected to `start` (face neighbours), and the pieces of it
/// that lost their support; none when it is larger than `rules.max_cells` (left standing).
fn system(
    world: &mut VoxelWorld,
    start: [i32; 3],
    rules: SupportRules,
    seen: &mut HashMap<[i32; 3], SupportRole>,
) -> Option<Vec<Piece>> {
    // 1. The whole connected system, in breadth-first order, and its carriers touching ground.
    let mut cells: Vec<[i32; 3]> = Vec::new();
    let mut roles: HashMap<[i32; 3], SupportRole> = HashMap::new();
    let mut grounded: Vec<[i32; 3]> = Vec::new();
    let mut queue = VecDeque::from([start]);
    let r0 = role(world.get_voxel(start[0], start[1], start[2]))?;
    roles.insert(start, r0);
    let mut too_big = false;
    while let Some(c) = queue.pop_front() {
        cells.push(c);
        if cells.len() > rules.max_cells {
            too_big = true;
            break;
        }
        let mine = roles[&c];
        let mut touches_ground = false;
        for d in FACES {
            let n = add(c, d);
            if roles.contains_key(&n) {
                continue;
            }
            let m = world.get_voxel(n[0], n[1], n[2]);
            match role(m) {
                Some(SupportRole::Ground) => touches_ground |= anchors(m),
                Some(r) => {
                    roles.insert(n, r);
                    queue.push_back(n);
                }
                None => {}
            }
        }
        if mine == SupportRole::Carrier && touches_ground {
            grounded.push(c);
        }
    }
    for (&c, &r) in &roles {
        seen.insert(c, r);
    }
    if too_big {
        return None;
    }
    // 2. Carrier components (through carriers only), anchored if one of them touches ground.
    let mut component: HashMap<[i32; 3], usize> = HashMap::new();
    let mut members: Vec<Vec<[i32; 3]>> = Vec::new();
    let mut anchored: Vec<bool> = Vec::new();
    for &c in &cells {
        if roles[&c] != SupportRole::Carrier || component.contains_key(&c) {
            continue;
        }
        let id = members.len();
        let mut list = Vec::new();
        let mut q = VecDeque::from([c]);
        component.insert(c, id);
        while let Some(p) = q.pop_front() {
            list.push(p);
            for d in FACES {
                let n = add(p, d);
                if roles.get(&n) == Some(&SupportRole::Carrier) && !component.contains_key(&n) {
                    component.insert(n, id);
                    q.push_back(n);
                }
            }
        }
        list.sort_unstable();
        members.push(list);
        anchored.push(false);
    }
    for g in &grounded {
        anchored[component[g]] = true;
    }
    // 3. Each rider belongs to the carrier component nearest it through riders (anchored ones
    //    first on a tie, then the lowest component), within the reach.
    let mut owner: HashMap<[i32; 3], (u32, usize)> = HashMap::new();
    let mut frontier: VecDeque<([i32; 3], u32, usize)> = VecDeque::new();
    let mut order: Vec<usize> = (0..members.len()).collect();
    order.sort_by_key(|&i| (!anchored[i], i));
    for &i in &order {
        for &c in &members[i] {
            frontier.push_back((c, 0, i));
        }
    }
    while let Some((c, dist, id)) = frontier.pop_front() {
        if dist >= rules.rider_reach {
            continue;
        }
        for d in FACES {
            let n = add(c, d);
            if roles.get(&n) != Some(&SupportRole::Rider) || owner.contains_key(&n) {
                continue;
            }
            owner.insert(n, (dist + 1, id));
            frontier.push_back((n, dist + 1, id));
        }
    }
    // 4. The pieces: unanchored components with their riders, then the loose riders' components.
    let mut riders_of: Vec<Vec<[i32; 3]>> = vec![Vec::new(); members.len()];
    let mut loose: Vec<[i32; 3]> = Vec::new();
    for &c in &cells {
        if roles[&c] != SupportRole::Rider {
            continue;
        }
        match owner.get(&c) {
            Some(&(_, id)) => riders_of[id].push(c),
            None => loose.push(c),
        }
    }
    let mut out = Vec::new();
    for (i, list) in members.into_iter().enumerate() {
        if anchored[i] {
            continue;
        }
        let mut riders = std::mem::take(&mut riders_of[i]);
        riders.sort_unstable();
        let carriers = list.len();
        let mut cells = list;
        cells.extend(riders);
        out.push(Piece { cells, carriers });
    }
    loose.sort_unstable();
    let mut taken: HashMap<[i32; 3], ()> = HashMap::new();
    for &c in &loose {
        if taken.contains_key(&c) {
            continue;
        }
        let mut cells = Vec::new();
        let mut q = VecDeque::from([c]);
        taken.insert(c, ());
        while let Some(p) = q.pop_front() {
            cells.push(p);
            for d in FACES {
                let n = add(p, d);
                if loose.binary_search(&n).is_ok() && !taken.contains_key(&n) {
                    taken.insert(n, ());
                    q.push_back(n);
                }
            }
        }
        cells.sort_unstable();
        out.push(Piece { cells, carriers: 0 });
    }
    Some(out)
}
