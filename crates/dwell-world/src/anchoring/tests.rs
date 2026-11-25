use super::*;
use crate::{
    CHUNK_SIZE, ChunkGenerator, FLAT_TOP_Y, FaceFrame, MIDPLANE_Y, both_faces, local_index,
};
use dwell_blocks::StateId;

/// The flat world's ground row (face-local): its top solid cells, under `FLAT_TOP_Y` (its first
/// row of air).
const G: i32 = FLAT_TOP_Y - 1;

fn planks() -> StateId {
    dwell_blocks::require("dwell:planks")
}

struct Fx {
    f: FaceFrame,
    world: VoxelWorld,
    a: Anchoring,
}

/// Stone from the midplane up to the flat world's ground on both faces (the flat generator makes
/// face A's only), within a small square round the origin.
fn both_faces_flat() -> ChunkGenerator {
    std::sync::Arc::new(|coord: ChunkCoord, chunk: &mut Chunk| {
        let stone = dwell_blocks::require("dwell:stone");
        for ly in 0..CHUNK_SIZE {
            let y = coord.y * CHUNK_SIZE + ly;
            // Face-local row (face B's rows mirror face A's).
            let row = FaceFrame::at_row(y).row(y);
            if !(MIDPLANE_Y..FLAT_TOP_Y).contains(&row) {
                continue;
            }
            for lz in 0..CHUNK_SIZE {
                for lx in 0..CHUNK_SIZE {
                    chunk.voxels_mut()[local_index(lx, ly, lz)] = stone;
                }
            }
        }
    })
}

impl Fx {
    fn new(f: FaceFrame) -> Self {
        Self {
            f,
            world: VoxelWorld::new(both_faces_flat(), None),
            a: Anchoring::new(),
        }
    }
    fn cell(&self, local: [i32; 3]) -> [i32; 3] {
        self.f.cell(local)
    }
    /// Writes face-local cells and tells the rule; the groups it cuts off, face-local.
    fn write(&mut self, cells: &[([i32; 3], StateId)]) -> Vec<Vec<[i32; 3]>> {
        let mut changed = Vec::new();
        for &(l, m) in cells {
            let c = self.cell(l);
            self.world.set_voxel(c[0], c[1], c[2], self.f.state(m));
            changed.push(c);
        }
        let groups = self.a.update(&mut self.world, &changed);
        let mut out: Vec<Vec<[i32; 3]>> = groups
            .into_iter()
            .map(|g| {
                let mut l: Vec<[i32; 3]> = g.into_iter().map(|c| self.local(c)).collect();
                l.sort_unstable();
                l
            })
            .collect();
        out.sort();
        out
    }
    fn place(&mut self, cells: &[[i32; 3]]) -> Vec<Vec<[i32; 3]>> {
        let p = planks();
        self.write(&cells.iter().map(|&c| (c, p)).collect::<Vec<_>>())
    }
    fn clear(&mut self, cells: &[[i32; 3]]) -> Vec<Vec<[i32; 3]>> {
        self.write(&cells.iter().map(|&c| (c, M.air)).collect::<Vec<_>>())
    }
    fn local(&self, c: [i32; 3]) -> [i32; 3] {
        // The face's cells mirror rows only: map back through the same frame.
        let back = self.f.cell(c);
        [c[0], back[1], c[2]]
    }
    /// Every kept distance equals a fresh search's from the anchors.
    fn check(&mut self, cells: &[[i32; 3]]) {
        let mut fresh = Anchoring::new();
        let world_cells: Vec<[i32; 3]> = cells.iter().map(|&l| self.cell(l)).collect();
        let mut cut = HashSet::default();
        for &c in &world_cells {
            if fresh.kind(&mut self.world, c) == Kind::Placed && !fresh.dist.contains_key(&c) {
                fresh.ensure(&mut self.world, c, &mut cut);
            }
        }
        for (&l, &c) in cells.iter().zip(&world_cells) {
            assert_eq!(
                self.a.distance(c),
                fresh.distance(c),
                "cell {l:?}: kept against fresh"
            );
        }
    }
}

/// A face-local column of cells from row `from` up `n`.
fn column(x: i32, z: i32, from: i32, n: i32) -> Vec<[i32; 3]> {
    (0..n).map(|k| [x, from + k, z]).collect()
}

both_faces!(
    fn a_pillar_cut_drops_what_was_above_the_cut_as_one(f: FaceFrame) {
        // A plank pillar 10 high on the ground: distances 1..10; cut at the fifth, the five above
        // fall as one group, the four below stay.
        let mut fx = Fx::new(f);
        let pillar = column(0, 0, G + 1, 10);
        let ground = fx.cell([0, G, 0]);
        assert_eq!(
            fx.a.kind(&mut fx.world, ground),
            Kind::Anchor,
            "the ground anchors"
        );
        assert!(
            fx.place(&pillar).is_empty(),
            "a pillar on the ground stands"
        );
        for (k, &c) in pillar.iter().enumerate() {
            assert_eq!(fx.a.distance(fx.cell(c)), Some(k as u32 + 1));
        }
        let groups = fx.clear(&[[0, G + 5, 0]]);
        assert_eq!(groups, vec![column(0, 0, G + 6, 5)]);
        fx.check(&column(0, 0, G + 1, 4));
    }
);

both_faces!(
    fn a_wall_broken_low_stays_up_and_walks_only_round_the_break(f: FaceFrame) {
        // A 7 × 6 plank wall: breaking a block at its foot drops nothing (its neighbours hold
        // what was above it), and the distances match a fresh search.
        let mut fx = Fx::new(f);
        let wall: Vec<[i32; 3]> = (0..7).flat_map(|x| column(x, 0, G + 1, 6)).collect();
        fx.place(&wall);
        assert!(fx.clear(&[[3, G + 1, 0]]).is_empty());
        let left: Vec<[i32; 3]> = wall
            .iter()
            .copied()
            .filter(|&c| c != [3, G + 1, 0])
            .collect();
        fx.check(&left);
        // Over the break: held round it by its neighbours, 2 each from the ground.
        assert_eq!(
            fx.a.distance(fx.cell([3, G + 2, 0])),
            Some(3),
            "round the break"
        );
    }
);

both_faces!(
    fn a_bridge_stands_on_either_pier_and_falls_whole_when_both_are_cut(f: FaceFrame) {
        let mut fx = Fx::new(f);
        let mut build = column(0, 0, G + 1, 3);
        build.extend(column(6, 0, G + 1, 3));
        build.extend((1..6).map(|x| [x, G + 3, 0]));
        fx.place(&build);
        assert!(
            fx.clear(&[[0, G + 1, 0]]).is_empty(),
            "the other pier holds it"
        );
        let mut left: Vec<[i32; 3]> = build
            .iter()
            .copied()
            .filter(|&c| c != [0, G + 1, 0])
            .collect();
        fx.check(&left);
        let groups = fx.clear(&[[6, G + 1, 0]]);
        left.retain(|&c| c != [6, G + 1, 0]);
        left.sort_unstable();
        assert_eq!(groups, vec![left], "everything left, one body");
    }
);

both_faces!(
    fn digging_the_ground_out_from_under_a_block_drops_it(f: FaceFrame) {
        let mut fx = Fx::new(f);
        fx.place(&[[0, G + 1, 0]]);
        assert_eq!(fx.a.distance(fx.cell([0, G + 1, 0])), Some(1));
        let groups = fx.clear(&[[0, G, 0]]);
        assert_eq!(groups, vec![vec![[0, G + 1, 0]]]);
    }
);

both_faces!(
    fn blocks_with_no_distance_get_one_when_a_change_reaches_them(f: FaceFrame) {
        // Written straight into the world (a save loaded, blocks from before the rule): no
        // distances; cutting the pillar works them out and drops what was above the cut.
        let mut fx = Fx::new(f);
        let p = f.state(planks());
        for l in column(0, 0, G + 1, 8) {
            let c = fx.cell(l);
            fx.world.set_voxel(c[0], c[1], c[2], p);
        }
        assert!(fx.a.is_empty());
        let groups = fx.clear(&[[0, G + 4, 0]]);
        assert_eq!(groups, vec![column(0, 0, G + 5, 4)]);
        fx.check(&column(0, 0, G + 1, 3));
    }
);

both_faces!(
    fn a_block_against_a_log_is_held_by_it(f: FaceFrame) {
        // A log has its own rule (a tree's carrier): it anchors what is placed against it.
        let mut fx = Fx::new(f);
        let log = dwell_blocks::require("dwell:log");
        fx.write(&[([0, G + 5, 0], log)]);
        assert!(fx.place(&[[1, G + 5, 0]]).is_empty());
        assert_eq!(fx.a.distance(fx.cell([1, G + 5, 0])), Some(1));
    }
);

both_faces!(
    fn an_arm_hung_off_a_pillar_falls_with_it_as_one_body(f: FaceFrame) {
        // An L: a pillar and an arm out from its top, hanging over nothing. Cutting the pillar's
        // foot drops pillar and arm together.
        let mut fx = Fx::new(f);
        let mut build = column(0, 0, G + 1, 5);
        build.extend((1..5).map(|x| [x, G + 5, 0]));
        assert!(fx.place(&build).is_empty());
        assert_eq!(fx.a.distance(fx.cell([4, G + 5, 0])), Some(9));
        let groups = fx.clear(&[[0, G + 1, 0]]);
        let mut rest: Vec<[i32; 3]> = build.into_iter().filter(|&c| c != [0, G + 1, 0]).collect();
        rest.sort_unstable();
        assert_eq!(groups, vec![rest]);
    }
);

both_faces!(
    fn a_new_block_gives_its_neighbours_a_shorter_way_down(f: FaceFrame) {
        // The arm's tip is 9 from the ground round the pillar; a new pillar under the tip makes
        // the arm's far end near the ground again.
        let mut fx = Fx::new(f);
        let mut build = column(0, 0, G + 1, 5);
        build.extend((1..5).map(|x| [x, G + 5, 0]));
        fx.place(&build);
        let props = column(4, 0, G + 1, 4);
        assert!(fx.place(&props).is_empty());
        assert_eq!(fx.a.distance(fx.cell([4, G + 5, 0])), Some(5));
        let mut all = build.clone();
        all.extend(props);
        fx.check(&all);
    }
);

both_faces!(
    fn a_break_in_a_big_wall_rechecks_only_the_blocks_round_it(f: FaceFrame) {
        // Owner (2026-11-25): the distance marks keep a break in a big supported build local. A
        // 40 × 30 wall: a block broken at its foot re-checks only the column over it, held by its
        // neighbours (the old search would walk the wall to find the ground).
        let mut fx = Fx::new(f);
        let wall: Vec<[i32; 3]> = (0..40).flat_map(|x| column(x, 0, G + 1, 30)).collect();
        fx.place(&wall);
        assert_eq!(fx.a.len(), wall.len());
        assert!(fx.clear(&[[20, G + 1, 0]]).is_empty());
        assert!(fx.a.walked() <= 30, "re-checked {} blocks", fx.a.walked());
        // High in it, nothing depends on it: none.
        assert!(fx.clear(&[[5, G + 30, 0]]).is_empty());
        assert_eq!(fx.a.walked(), 0);
    }
);
