//! Lakes: the depressions left after erosion, found by flooding (`flood.rs`). A depression big
//! and deep enough (`lake_min_area`, `lake_min_depth`) is a lake: its cells under one flat water
//! surface at its spill level, draining through one outlet. A smaller one is filled to its spill
//! level, or breached: a channel cut at its floor's height from its lowest cell along its drainage
//! path until the ground falls below it (`small_pits`; deeper than `max_breach_depth`, it is filled).

use std::collections::VecDeque;

use super::flood::{Flood, Grid, NONE, flood, shore};
use super::{HydrologyParams, Lake, SmallPits};

/// The depressions of a flood: connected cells under water (level > height), each under one flat
/// level. Labelled in index order.
pub struct Depressions {
    /// Each cell's depression, or `NONE`.
    pub of: Vec<u32>,
    /// Each depression's cells, in breadth-first order from its lowest-index cell.
    pub cells: Vec<Vec<u32>>,
}

pub fn depressions(grid: Grid, h: &[f64], f: &Flood, sea: &[bool]) -> Depressions {
    let len = grid.len();
    let wet = |i: usize| !sea[i] && f.level[i] > h[i];
    let mut of = vec![NONE; len];
    let mut cells = Vec::new();
    let mut queue = VecDeque::new();
    for s in 0..len {
        if of[s] != NONE || !wet(s) {
            continue;
        }
        let id = cells.len() as u32;
        let mut members = Vec::new();
        of[s] = id;
        queue.push_back(s);
        while let Some(c) = queue.pop_front() {
            members.push(c as u32);
            for (j, _) in grid.neighbours(c) {
                if of[j] == NONE && wet(j) {
                    of[j] = id;
                    queue.push_back(j);
                }
            }
        }
        cells.push(members);
    }
    Depressions { of, cells }
}

/// Fills or breaches the depressions too small to be lakes, until none is left (at most a few
/// rounds: a breach can expose a smaller depression downstream, which the next round handles; the
/// last round fills).
pub fn remove_small_pits(grid: Grid, cell: f64, h: &mut [f64], sea: &[bool], p: &HydrologyParams) {
    const ROUNDS: usize = 6;
    let shore = shore(grid, sea);
    for round in 0..ROUNDS {
        let f = flood(grid, h, sea, &shore);
        let d = depressions(grid, h, &f, sea);
        let mut any = false;
        for (id, members) in d.cells.iter().enumerate() {
            let level = f.level[members[0] as usize];
            let lowest = *members
                .iter()
                .min_by(|&&a, &&b| h[a as usize].total_cmp(&h[b as usize]).then(a.cmp(&b)))
                .expect("a depression has cells") as usize;
            let depth = level - h[lowest];
            let area = members.len() as f64 * cell * cell;
            if area >= p.lake_min_area && depth >= p.lake_min_depth {
                continue;
            }
            any = true;
            let breach = p.small_pits == SmallPits::Breach
                && depth <= p.max_breach_depth
                && round + 1 < ROUNDS;
            if breach {
                // Cut a channel at the floor's height along the drainage path, out of the
                // depression and down until the ground is lower, stopping at any other depression
                // (whose water it then joins).
                let floor = h[lowest];
                let mut c = f.parent[lowest] as usize;
                while c != NONE as usize && !sea[c] && h[c] >= floor {
                    if d.of[c] != NONE && d.of[c] != id as u32 {
                        break;
                    }
                    h[c] = floor;
                    c = f.parent[c] as usize;
                }
            } else {
                for &m in members {
                    h[m as usize] = level;
                }
            }
        }
        if !any {
            return;
        }
    }
}

/// The lakes of the final surface and the receivers: each dry land cell drains to its flood parent
/// (its lowest neighbour on the way to the sea), and a lake's cells drain across it, breadth first,
/// to the lake's first-flooded cell and from there to its outlet, so each lake has exactly one.
pub struct Drainage {
    pub lakes: Vec<Lake>,
    /// Each cell's lake, or `NONE`.
    pub lake_of: Vec<u32>,
    pub receiver: Vec<u32>,
    /// The water surface (m above sea level): a dry cell's height, a lake's level, 0 at sea.
    pub surface: Vec<f64>,
}

pub fn drainage(grid: Grid, cell: f64, h: &[f64], sea: &[bool]) -> Drainage {
    let f = flood(grid, h, sea, &shore(grid, sea));
    let d = depressions(grid, h, &f, sea);
    let mut receiver = f.parent.clone();
    let mut surface: Vec<f64> = f.level.clone();
    // The first-flooded cell of each lake.
    let mut entry = vec![NONE; d.cells.len()];
    for &c in &f.order {
        let id = d.of[c as usize];
        if id != NONE && entry[id as usize] == NONE {
            entry[id as usize] = c;
        }
    }
    let mut lakes = Vec::with_capacity(d.cells.len());
    let mut seen = vec![false; grid.len()];
    let mut queue = VecDeque::new();
    for (id, members) in d.cells.iter().enumerate() {
        let e = entry[id] as usize;
        let level = f.level[e];
        let outlet = f.parent[e];
        // Breadth first across the lake from its entry.
        seen[e] = true;
        queue.push_back(e);
        while let Some(c) = queue.pop_front() {
            for (j, _) in grid.neighbours(c) {
                if !seen[j] && d.of[j] == id as u32 {
                    seen[j] = true;
                    receiver[j] = c as u32;
                    queue.push_back(j);
                }
            }
        }
        let (mut depth, mut volume) = (0.0f64, 0.0f64);
        for &m in members {
            let m = m as usize;
            depth = depth.max(level - h[m]);
            volume += (level - h[m]) * cell * cell;
            surface[m] = level;
        }
        lakes.push(Lake {
            level,
            outlet,
            entry: e as u32,
            cells: members.len() as u32,
            area: members.len() as f64 * cell * cell,
            max_depth: depth,
            volume,
        });
    }
    for (i, s) in sea.iter().enumerate() {
        if *s {
            receiver[i] = NONE;
            surface[i] = 0.0;
        }
    }
    Drainage {
        lakes,
        lake_of: d.of,
        receiver,
        surface,
    }
}

/// The land cells in an order where every receiver comes before its donors (Braun & Willett's
/// stack): a depth-first walk up the donors from each cell that drains to the sea.
pub fn stack(grid: Grid, receiver: &[u32], sea: &[bool]) -> Vec<u32> {
    let len = grid.len();
    let mut count = vec![0u32; len + 1];
    for (i, &r) in receiver.iter().enumerate() {
        if !sea[i] && r != NONE {
            count[r as usize + 1] += 1;
        }
    }
    for i in 0..len {
        count[i + 1] += count[i];
    }
    let mut fill = count.clone();
    let mut donors = vec![0u32; count[len] as usize];
    for (i, &r) in receiver.iter().enumerate() {
        if !sea[i] && r != NONE {
            donors[fill[r as usize] as usize] = i as u32;
            fill[r as usize] += 1;
        }
    }
    let mut out = Vec::with_capacity(len);
    let mut todo = Vec::new();
    for i in 0..len {
        if sea[i] || !sea[receiver[i] as usize] {
            continue;
        }
        todo.push(i as u32);
        while let Some(c) = todo.pop() {
            out.push(c);
            let c = c as usize;
            todo.extend_from_slice(&donors[count[c] as usize..count[c + 1] as usize]);
        }
    }
    out
}
