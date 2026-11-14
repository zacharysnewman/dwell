//! Trees on slopes stand on a levelled pad the trunk's width (WORK_QUEUE item 44; owner,
//! 2026-11-12: "For now let's just do level pad, can just be the the tree width, 1m."): a
//! placement with `pad: "trunk"` fills the ground under each trunk column (the structure's lowest
//! cells) up to the trunk's base with the column's own ground, ramps there made full cells. The
//! ground outside the trunk's columns, and on the flat, is what it was; chunks agree in any order;
//! the LOD's tree levels fill under the trunk too.

use std::collections::HashMap;

use dwell_blocks::StateId;
use dwell_world::*;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::placements::{PLACEMENTS, Placement, Tree};
use dwell_worldgen::terrain::{BIOMES, Biome};
use dwell_worldgen::*;

const S: i32 = CHUNK_SIZE;
const SEED: u64 = 7;

fn generator(f: FaceFrame) -> TerrainGenerator {
    TerrainGenerator::new(face_seed(SEED, f.face))
}

/// The build's placements with no pads: the ground as it was before item 44.
fn unpadded(f: FaceFrame) -> TerrainGenerator {
    let rules: Vec<Placement> = PLACEMENTS
        .iter()
        .map(|p| Placement {
            pad_trunk: false,
            ..*p
        })
        .collect();
    // The rules must outlive the generator's planters: leak them (a test).
    let rules: &'static [Placement] = Box::leak(rules.into_boxed_slice());
    generator(f).with_placements(rules)
}

/// A fixture placement of the proto rock with `pad_trunk`, by data alone: its nine lowest cells
/// stand in for a thick trunk, on slopes up to 44°, every 12 m.
fn wide_base(f: FaceFrame) -> TerrainGenerator {
    let rule = Placement {
        name: "test_wide_base",
        structures: &[("dwell:proto_rock", 1)],
        on: &[
            "dwell:grass",
            "dwell:snowy_grass",
            "dwell:dirt",
            "dwell:stone",
        ],
        footprint_on: false,
        pad_trunk: true,
        grid: 12,
        density: 1.0,
        grove_wavelength: 0,
        grove_coverage: 1.0,
        grove_outside: 1.0,
        min_height: 0,
        max_height: 3000,
        rise_full: 0,
        rise_low: 1.0,
        max_gradient_sq: 0.93,
        spacing: 0,
        rotations: &[0, 1, 2, 3],
        variants: &[],
        tops: 1.0,
    };
    let biome = Biome {
        placements: &["test_wide_base"],
        ..BIOMES[0]
    };
    generator(f).with_biome(&biome).with_placements(&[rule])
}

/// Chunks generated on demand (face-local coordinates).
struct Chunks<'a> {
    g: &'a TerrainGenerator,
    got: HashMap<ChunkCoord, Chunk>,
}

impl<'a> Chunks<'a> {
    fn new(g: &'a TerrainGenerator) -> Self {
        Self {
            g,
            got: HashMap::new(),
        }
    }

    fn chunk(&mut self, c: ChunkCoord) -> &Chunk {
        let g = self.g;
        self.got.entry(c).or_insert_with(|| {
            let mut chunk = Chunk::default();
            g.generate(c, &mut chunk);
            chunk
        })
    }

    fn at(&mut self, [x, y, z]: [i32; 3]) -> StateId {
        let c = ChunkCoord::new(floor_div(x, S), floor_div(y, S), floor_div(z, S));
        self.chunk(c).get(x - c.x * S, y - c.y * S, z - c.z * S)
    }
}

fn floor_div(a: i32, b: i32) -> i32 {
    a.div_euclid(b)
}

/// The ground's slope at (x, z) in degrees, as the placements measure it (±2 m central differences).
fn slope_deg(g: &TerrainGenerator, x: i32, z: i32) -> f32 {
    let h = |x, z| g.column_at(x, z).height;
    let gx = (h(x + 2, z) - h(x - 2, z)) * 0.25;
    let gz = (h(x, z + 2) - h(x, z - 2)) * 0.25;
    (gx * gx + gz * gz).sqrt().atan().to_degrees()
}

/// Trees anchored in squares spread over the island, with their slopes.
fn trees(g: &TerrainGenerator) -> Vec<(Tree, f32)> {
    let mut out = Vec::new();
    for j in -2..=2 {
        for i in -2..=2 {
            let (x0, z0) = (i * 2_400 + 300, j * 2_400 - 700);
            for t in g.trees_in((x0, z0), (x0 + 255, z0 + 255)) {
                if (x0..x0 + 256).contains(&t.anchor[0]) && (z0..z0 + 256).contains(&t.anchor[2]) {
                    out.push((t, slope_deg(g, t.anchor[0], t.anchor[2])));
                }
            }
        }
    }
    out
}

fn is_full(m: StateId) -> bool {
    m != M.air
        && m != M.water
        && dwell_blocks::info(m).shape_index == dwell_blocks::info(M.stone).shape_index
}

/// Checks the pads of up to 40 trees of `g` on 30–44° slopes (at least `least` of them): every
/// trunk column's ground is full cells up to the row under the trunk's base (no air under a trunk
/// cell, no ramp under it), its top the column's surface block, and the trunk's base cell the
/// tree's own. Returns how many trunk columns were raised above their own ground.
fn check_steep_pads(g: &TerrainGenerator, least: usize) -> usize {
    let mut chunks = Chunks::new(g);
    let steep: Vec<Tree> = trees(g)
        .into_iter()
        .filter(|(_, s)| (30.0..=44.0).contains(s))
        .map(|(t, _)| t)
        .take(40)
        .collect();
    assert!(steep.len() >= least, "only {} steep trees", steep.len());
    let mut raised = 0;
    for t in &steep {
        let p = &g.planters()[t.planter as usize];
        let (cols, base) = p.pad(t);
        assert!(!cols.is_empty(), "the placement pads its trunks");
        let placed = p.place(t);
        for &(x, z) in &cols {
            let raw = g.ground_y(x, z).expect("inside");
            if raw < base - 1 {
                raised += 1;
            }
            // The ground from the column's own top (or the base, uphill) to under the base.
            for y in raw.min(base - 1)..base {
                let m = chunks.at([x, y, z]);
                assert!(
                    is_full(m),
                    "tree {:?} column ({x}, {z}) row {y} (base {base}): {:?}",
                    t.anchor,
                    dwell_blocks::info(m).name
                );
            }
            // Uphill the ground already rises past the base (the trunk stands in it): only a
            // column levelled up to the base shows its surface block there.
            if raw < base {
                assert_eq!(
                    chunks.at([x, base - 1, z]),
                    g.top_state(&g.column_at(x, z)),
                    "the pad's top is the column's surface block"
                );
            }
            if let Some(s) = placed.at([x, base, z])
                && raw < base
            {
                assert_eq!(chunks.at([x, base, z]), s, "the trunk's base");
            }
        }
    }
    eprintln!("{} steep trees, {raised} trunk columns raised", steep.len());
    raised
}

dwell_world::both_faces!(
    /// The build's trees on 30–44° slopes stand on flat, full pads.
    fn a_tree_on_a_steep_slope_stands_on_a_flat_full_pad(f: FaceFrame) {
        check_steep_pads(&generator(f), 10);
    }
);

dwell_world::both_faces!(
    /// A wide base by data alone: a fixture placement of the proto rock with `pad_trunk` (its nine
    /// lowest cells standing in for a thick trunk) on slopes up to 44°: every one of its base's
    /// columns downhill of the anchor is raised to the base on full ground.
    fn a_wide_base_is_levelled_across_all_its_columns(f: FaceFrame) {
        let g = wide_base(f);
        assert!(check_steep_pads(&g, 10) > 0, "some base column was raised");
    }
);

dwell_world::both_faces!(
    /// Only the trunk's columns change: every other voxel of the chunks round a tree, sloped or
    /// flat, is the unpadded generator's; a tree on flat ground changes nothing at all.
    fn the_ground_outside_the_trunk_and_on_the_flat_is_unchanged(f: FaceFrame) {
        let g = generator(f);
        let old = unpadded(f);
        let all = trees(&g);
        let pick: Vec<&(Tree, f32)> = all
            .iter()
            .filter(|(_, s)| *s >= 20.0)
            .take(12)
            .chain(all.iter().filter(|(_, s)| *s == 0.0).take(12))
            .collect();
        assert!(pick.len() >= 12, "{}", pick.len());
        for (t, slope) in pick {
            let p = &g.planters()[t.planter as usize];
            let (_, base) = p.pad(t);
            let [x, _, z] = t.anchor;
            let c = ChunkCoord::new(floor_div(x, S), floor_div(base - 1, S), floor_div(z, S));
            let (mut a, mut b) = (Chunk::default(), Chunk::default());
            g.generate(c, &mut a);
            old.generate(c, &mut b);
            // Every trunk column of every tree reaching the chunk.
            let (x0, z0) = (c.x * S, c.z * S);
            let trunks: Vec<(i32, i32)> = g
                .trees_in((x0, z0), (x0 + S - 1, z0 + S - 1))
                .iter()
                .flat_map(|t| g.planters()[t.planter as usize].pad(t).0)
                .collect();
            let mut changed = 0;
            for lz in 0..S {
                for lx in 0..S {
                    let trunk = trunks.contains(&(x0 + lx, z0 + lz));
                    for ly in 0..S {
                        let (m, n) = (a.get(lx, ly, lz), b.get(lx, ly, lz));
                        if trunk {
                            changed += usize::from(m != n);
                        } else {
                            assert_eq!(m, n, "outside the trunks at ({lx}, {ly}, {lz}) of {c:?}");
                        }
                    }
                }
            }
            // On the flat a trunk column's ground is already its pad.
            let flat = trunks.iter().all(|&(x, z)| slope_deg(&g, x, z) == 0.0);
            if *slope == 0.0 && flat {
                assert_eq!(changed, 0, "a tree on the flat at {:?}", t.anchor);
            }
        }
    }
);

dwell_world::both_faces!(
    /// The pads are a function of the seed and position: the chunks under a padded tree generated
    /// in either order, by a fresh generator or a used one, are the same.
    fn chunks_agree_on_the_pads_in_any_order(f: FaceFrame) {
        let g = generator(f);
        let steep: Vec<Tree> = trees(&g)
            .into_iter()
            .filter(|(_, s)| *s >= 25.0)
            .map(|(t, _)| t)
            .take(10)
            .collect();
        assert!(!steep.is_empty());
        let mut coords = Vec::new();
        for t in &steep {
            let (cols, base) = g.planters()[t.planter as usize].pad(t);
            for (x, z) in cols {
                let raw = g.ground_y(x, z).expect("inside");
                for y in [raw, base - 1, base] {
                    let c = ChunkCoord::new(floor_div(x, S), floor_div(y, S), floor_div(z, S));
                    for dx in -1..=1 {
                        for dz in -1..=1 {
                            coords.push(ChunkCoord::new(c.x + dx, c.y, c.z + dz));
                        }
                    }
                }
            }
        }
        coords.sort_unstable_by_key(|c| (c.x, c.y, c.z));
        coords.dedup();
        let mut forward = Chunks::new(&g);
        let fresh = generator(f);
        let mut backward = Chunks::new(&fresh);
        for c in &coords {
            forward.chunk(*c);
        }
        for c in coords.iter().rev() {
            backward.chunk(*c);
        }
        for c in &coords {
            assert_eq!(
                forward.chunk(*c).voxels(),
                backward.chunk(*c).voxels(),
                "{c:?}"
            );
        }
    }
);

#[test]
fn the_lod_fills_under_a_far_trees_trunk() {
    // The wide-base fixture: its base's downhill columns are raised (a single trunk's column is
    // its own ground already).
    // face-a-only: `generate_lod` takes world sections, whose rows are face A's here; the chunk
    // tests above cover both faces of the pad itself.
    use dwell_world::lod::{
        LodCells, LodCoord, ORIGIN_X, ORIGIN_Y, ORIGIN_Z, cell_index, section_origin, section_size,
    };
    let g = wide_base(FaceFrame::A);
    let section_at = |level: i32, x: i64, y: i64, z: i64| {
        let s = section_size(level);
        let d = |v: i64, o: i64| (v - o).div_euclid(s) as i32;
        LodCoord::new(level, d(x, ORIGIN_X), d(y, ORIGIN_Y), d(z, ORIGIN_Z))
    };
    let mut cells = LodCells::new();
    let mut checked = 0;
    for (t, slope) in trees(&g) {
        if slope < 25.0 {
            continue;
        }
        let (cols, base) = g.planters()[t.planter as usize].pad(&t);
        for (x, z) in cols {
            let c = section_at(1, x.into(), i64::from(base - 1), z.into());
            let o = section_origin(c);
            let (dx, dz, dy) = (
                i64::from(x) - o.x,
                i64::from(z) - o.z,
                i64::from(base - 1) - o.y,
            );
            // Only a column the level samples (its cell's centre).
            if dx.rem_euclid(2) != 1 || dz.rem_euclid(2) != 1 {
                continue;
            }
            g.generate_lod(c, &mut cells, None);
            let i = cell_index(
                dx.div_euclid(2) as i32,
                dy.div_euclid(2) as i32,
                dz.div_euclid(2) as i32,
            );
            assert_ne!(
                cells[i], M.air,
                "the LOD cell under the trunk of {:?} at ({x}, {z})",
                t.anchor
            );
            checked += 1;
        }
        if checked >= 20 {
            break;
        }
    }
    assert!(checked >= 5, "only {checked} trunk columns sampled");
}
