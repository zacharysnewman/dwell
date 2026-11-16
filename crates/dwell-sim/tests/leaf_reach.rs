//! BUGS.md #193 (owner, 2026-11-16: "Sometimes leaves hang in the air after cutting down a tree
//! ... Leaves should only be attached to trees or fall to the ground"): a felled trunk takes the
//! riders within `riderReach` (shared/physics/felling.json) of its carriers, through riders; a
//! leaf farther than that stays where it was. Every species' tree, grown full, must have each of
//! its leaves within that reach of its wood (through leaves touching at a face, an edge or a corner),
//! so the data can't grow a crown the fell leaves behind.

use std::collections::{HashMap, VecDeque};

use dwell_blocks::SupportRole;
use dwell_worldgen::growth::{PLANTED, Record, cells_at};
use dwell_worldgen::species::SPECIES;

#[test]
fn every_species_leaves_are_within_the_felling_rider_reach_of_its_wood() {
    // face-a-only: a tree's cells by face-local face A; reach is by face neighbours, the same on B.
    let reach = dwell_sim::felling::FELLING.rider_reach as i32;
    for (si, s) in SPECIES.iter().enumerate() {
        for seed in 0..40u32 {
            let r = Record {
                anchor: [0, 200, 0],
                face: dwell_world::Face::A,
                species: si as u16,
                seed: seed.wrapping_mul(2_654_435_761),
                stage: (s.stages - 1) as u8,
                since: 0.0,
                turns: 0,
                planter: PLANTED,
                variant: 0,
                stopped: false,
            };
            let cells: HashMap<[i32; 3], u16> = cells_at(&r, r.stage).into_iter().collect();
            let role = |m: u16| dwell_blocks::info(m).support_role;
            let mut dist: HashMap<[i32; 3], i32> = HashMap::new();
            let mut q = VecDeque::new();
            for (c, m) in &cells {
                if role(*m) == SupportRole::Carrier {
                    dist.insert(*c, 0);
                    q.push_back(*c);
                }
            }
            while let Some(c) = q.pop_front() {
                let d = dist[&c];
                // A leaf touches through a face, an edge or a corner (dwell-world support's
                // `AROUND`).
                for i in 0..27 {
                    let o = [i % 3 - 1, (i / 3) % 3 - 1, i / 9 - 1];
                    if o == [0, 0, 0] {
                        continue;
                    }
                    let n = [c[0] + o[0], c[1] + o[1], c[2] + o[2]];
                    if cells
                        .get(&n)
                        .is_some_and(|m| role(*m) == SupportRole::Rider)
                        && !dist.contains_key(&n)
                    {
                        dist.insert(n, d + 1);
                        q.push_back(n);
                    }
                }
            }
            for (c, m) in &cells {
                // A leaf touching nothing of its tree (a palm's frond across a gap) is not
                // reached: the fell's sweep of the tree's box takes it (dwell-world support).
                if role(*m) != SupportRole::Rider || !dist.contains_key(c) {
                    continue;
                }
                let d = dist.get(c).copied();
                assert!(
                    d.is_some_and(|d| d <= reach),
                    "{} seed {seed}: a leaf at {c:?} is {d:?} cells from its wood through leaves \
                     (riderReach {reach})",
                    s.name
                );
            }
        }
    }
}
