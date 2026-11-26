//! BUGS.md #197's invariant (owner, 2026-11-23: "We likely need better (non-visual) testing around
//! this"): every listed tree is drawn exactly once, its real cells (leaves and logs) or its
//! impostor, never both and never neither once a switch's fade is done, for every species, growth
//! stage (each world day's: `FarTree::grown_by` picks one), seed variant and distance, coming and
//! going, on both faces. Decided by the renderer's own rules over the same data: the chunk
//! meshers' ownership (`tree_tint::owner` over the generator's box, `terrain::grown_bounds`), the
//! fade map (`TreeFades`, the CPU twin of the shaders' `tree_near_at`), a chunk's cutout draw
//! (`impostors::cutout_draw`), the leaves' vertex drop past their reach (`card_dropped`), the logs'
//! rule (`impostors::tree_logs_drawn`) and the impostor's own lookup (`imp_vs`).

use std::collections::HashMap;

use dwell_world::FaceFrame;
use dwell_world::tree_tint::{Tint, TreeTint, owner};
use dwell_worldgen::impostor::{Config, Far, FarTree};
use dwell_worldgen::species::{Part, SPECIES, grow};

use super::impostors::{self, CutoutDraw};
use super::tree_fade::{self, TreeFades};
use dwell_world::{CHUNK_EXTENT_M, chunk_origin_m};

/// The anchor's row: a tree on high ground, face-local.
const ROW: i32 = 1200;

fn far() -> Far {
    Config::get().far("on").expect("the on tier")
}

/// A tree's cells as the server places them (face-local cells) and whether the meshers own each
/// to it, its fade-map tree, and its far tree.
struct Fixture {
    cells: Vec<([i32; 3], Part, bool)>,
    tree: tree_fade::Tree,
    /// The billboard's height as drawn (m): its baked tree's, scaled by its instance.
    billboard_h: f32,
}

fn fixture(
    config: &Config,
    layers: &mut HashMap<u32, ([f32; 4], f32)>,
    s: u16,
    seed: u32,
    stage: u32,
) -> Fixture {
    let anchor = [0, ROW, 0];
    let sp = &SPECIES[usize::from(s)];
    let grown = grow(sp, seed, stage);
    let (gmin, gmax) = dwell_worldgen::terrain::grown_bounds(s, seed).expect("a species tree");
    let (mut min, mut max) = (gmin, gmax);
    // The placed cells' bounds too, as `tree_box` widens them.
    for c in grown.cells.keys() {
        for a in 0..3 {
            min[a] = min[a].min(c[a]);
            max[a] = max[a].max(c[a]);
        }
    }
    let abs = |c: [i32; 3]| std::array::from_fn(|a| anchor[a] + c[a]);
    let tint = TreeTint {
        anchor,
        min: abs(min),
        max: abs(max),
        tint: Tint::NONE,
    };
    let far_tree = FarTree {
        anchor,
        species: s,
        seed,
        stage,
        stem: anchor,
        stem_cells: 1,
        columns: [tint.min[0], tint.min[2], tint.max[0], tint.max[2]],
        rows: [tint.min[1], tint.max[1]],
    };
    let list = [tint];
    let cells = grown
        .cells
        .iter()
        .map(|(&c, &p)| {
            let at = abs(c);
            (at, p, owner(&list, at).is_some())
        })
        .collect();
    let inst = config.instances(&[far_tree])[0];
    let (layer, baked_h) = *layers.entry(inst.layer).or_insert_with(|| {
        let b = config.bake_layer(inst.layer, 8).expect("a layer");
        // The baked tree's height: its cells' rows (bake_cells).
        let (sp, stage, seed) = config.layer_tree(inst.layer).expect("a layer");
        let g = grow(&SPECIES[usize::from(sp)], seed, stage);
        let (lo, hi) = g.cells.keys().fold((i32::MAX, i32::MIN), |(lo, hi), c| {
            (lo.min(c[1]), hi.max(c[1]))
        });
        (
            [b.centre[1], b.radius, 1.0, 0.0],
            (hi - lo + 1) as f32 * dwell_world::CELL_H as f32,
        )
    });
    Fixture {
        cells,
        tree: impostors::fade_tree(&inst, far_tree.columns, far_tree.rows, layer),
        billboard_h: baked_h * inst.scale,
    }
}

/// The nearest point (m) of the chunk holding face-local cell `c` from the eye `e` (world).
fn chunk_distance(face: FaceFrame, c: [i32; 3], e: [f64; 3]) -> f32 {
    let w = face.cell(c);
    let o = chunk_origin_m(dwell_world::chunk_of(w[0], w[1], w[2]));
    let d2: f64 = (0..3)
        .map(|k| {
            let lo = o[k] - e[k];
            let d = (-lo).clamp(0.0, CHUNK_EXTENT_M[k]) + lo;
            d * d
        })
        .sum();
    d2.sqrt() as f32
}

/// What of a tree draws with the eye at `e` (world), its fade map settled: its real cells drawn,
/// those not drawn that lie within the leaves' reach, and its impostor's share.
fn drawn(
    face: FaceFrame,
    far: Far,
    fades: &TreeFades,
    x: &Fixture,
    e: [f64; 3],
) -> (usize, usize, f32) {
    let t = &x.tree;
    let share = |c: [i32; 3]| fades.share_at(f64::from(c[0]) + 0.5, f64::from(c[2]) + 0.5);
    let tree_share = share(t.key);
    let cell_h = dwell_world::CELL_H;
    let mut real = 0;
    let mut missing_near = 0;
    for &(c, part, owned) in &x.cells {
        let d_chunk = chunk_distance(face, c, e);
        let centre = face.point([
            f64::from(c[0]) + 0.5,
            (f64::from(c[1]) + 0.5) * cell_h,
            f64::from(c[2]) + 0.5,
        ]);
        let d = (0..3)
            .map(|k| (centre[k] - e[k]).powi(2))
            .sum::<f64>()
            .sqrt() as f32;
        let shown = match part {
            Part::Leaf => {
                let chunk = impostors::cutout_draw(Some(far), d_chunk) == CutoutDraw::All;
                if !owned {
                    chunk
                } else {
                    chunk && d <= tree_fade::leaves_reach(&far) + 2.0 && share(c) > 0.0
                }
            }
            Part::Log(..) => !owned || impostors::tree_logs_drawn(Some(far), tree_share),
        };
        real += usize::from(shown);
        // Past the leaves' reach a leaf is dropped whole: a giant's top may be, while it is real
        // (owner, 2026-11-25: "Cap + true-size billboards").
        let reach = tree_fade::leaves_reach(&far) + 2.0;
        missing_near += usize::from(!shown && (d <= reach || !matches!(part, Part::Leaf)));
    }
    let impostor = if t.baked && tree_share < 0.999 {
        1.0 - tree_share
    } else {
        0.0
    };
    (real, missing_near, impostor)
}

dwell_world::both_faces! {
    fn every_far_tree_is_drawn_once_at_every_distance_stage_and_variant(face: FaceFrame) {
        let config = Config::get();
        let far = far();
        let mut layers = HashMap::new();
        let mut failures: Vec<String> = Vec::new();
        let mut sizes: Vec<String> = Vec::new();
        let mut checked = 0usize;
        for s in 0..SPECIES.len() as u16 {
            for stage in 0..SPECIES[usize::from(s)].stages {
                for seed in [1u32, 77, 4242, 0xdead_beef] {
                    let x = fixture(config, &mut layers, s, seed, stage);
                    let t = x.tree;
                    // The billboard is its real tree's size (owner, 2026-11-25): its height within
                    // a quarter of the real tree's.
                    let (lo, hi) = x.cells.iter().fold((i32::MAX, i32::MIN), |(lo, hi), (c, _, _)| {
                        (lo.min(c[1]), hi.max(c[1]))
                    });
                    let real_h = (hi - lo + 1) as f32 * dwell_world::CELL_H as f32;
                    let ratio = x.billboard_h / real_h;
                    let off = (x.billboard_h - real_h).abs() > 1.0 && !(0.75..=1.33).contains(&ratio);
                    if off && sizes.len() < 6 {
                        sizes.push(format!(
                            "{} stage {stage} seed {seed}: billboard {:.1} m tall, the tree {real_h:.1} m",
                            SPECIES[usize::from(s)].name,
                            x.billboard_h
                        ));
                    }
                    let centre = [t.foot[0], t.foot[1] + t.centre_y, t.foot[2]].map(f64::from);
                    let mut fades = TreeFades::new(tree_fade::window());
                    let mut now = 0.0;
                    // In from 110 m to 6 m, then back out: both sides of the hysteresis.
                    let path = (6..=110).rev().chain(6..=110);
                    for (step, d) in path.enumerate() {
                        let coming = step < 105;
                        let e = face.point([centre[0] - f64::from(d), centre[1], centre[2]]);
                        for dt in [0.0, 5.0] {
                            now += dt;
                            fades.update(now, face, e, &far, || Box::new(std::iter::once(t)), |_| true);
                        }
                        let (real, missing_near, impostor) = drawn(face, far, &fades, &x, e);
                        checked += 1;
                        let all = x.cells.len();
                        // Within `inM` less the early switch's cap a tree is its real self (BUGS.md
                        // #197: 0.76.20 kept a giant its billboard at every distance).
                        let near = (d as f32) < far.in_m - tree_fade::EARLY_SWITCH_M;
                        let ok = if impostor > 0.0 {
                            real == 0 && !near
                        } else {
                            missing_near == 0
                        };
                        if !ok && failures.len() < 12 {
                            failures.push(format!(
                                "{} stage {stage} seed {seed} at {d} m {}: {real} of {all} real cells drawn, impostor {impostor}",
                                SPECIES[usize::from(s)].name,
                                if coming { "coming" } else { "going" },
                            ));
                        }
                    }
                }
            }
        }
        assert!(checked > 0);
        failures.extend(sizes);
        assert!(failures.is_empty(), "{} checks; drawn other than once:\n{}", checked, failures.join("\n"));
    }
}

/// The forest world's generator on face `f` (`dwell-worldgen` tests/forest_world.rs `generator`):
/// old pines and oaks standing close, giants among them.
fn forest(f: FaceFrame) -> dwell_worldgen::terrain::TerrainGenerator {
    use dwell_worldgen::bifacial::face_seed;
    let name = "test:forest";
    let e = dwell_worldgen::config::world_config(name)
        .unwrap()
        .expect("the forest world's edit");
    let map = dwell_worldgen::worldmap::WorldMap::for_height_with_worldgen(
        face_seed(0, f.face),
        e.height.expect("its height field"),
        name,
    )
    .unwrap();
    dwell_worldgen::terrain::TerrainGenerator::with_map(
        face_seed(0, f.face),
        std::sync::Arc::new(map),
    )
    .with_biomes_and_geology(&e.biomes, e.map, e.geology)
    .with_vegetation(e.vegetation)
}

dwell_world::both_faces! {
    /// The owner, 2026-11-25, on the before/after flight: "I'm seeing giants with their billboards
    /// at the same time in the after?" A giant's crown reaches across its neighbours' trunks, and
    /// its leaves followed the nearest trunk's state. A stand of the forest world, the camera
    /// flying through it at the canopy both ways: no tree's cell is drawn while its billboard is,
    /// and a real tree's cell is missing only where another tree that grew it is a billboard (whose
    /// billboard draws it) or past the leaves' reach. Decided by the claims the mesh workers make
    /// (`TerrainGenerator::tree_claims`) and the renderer's rules (`tree_set_drawn`, the leaf's
    /// fade tree's share at its anchor).
    fn every_tree_in_a_stand_is_drawn_once_beside_its_neighbours(f: FaceFrame) {
        let g = forest(f);
        let config = Config::get();
        let far = far();
        let (lo, hi) = ((0, -400), (63, -337));
        let m = 16;
        let listed = g.far_trees((lo.0 - m, lo.1 - m), (hi.0 + m, hi.1 + m));
        let placed = g.trees_in((lo.0 - m, lo.1 - m), (hi.0 + m, hi.1 + m));
        let rows = listed.iter().fold((i32::MAX, i32::MIN), |(a, b), t| (a.min(t.rows[0]), b.max(t.rows[1])));
        let claims = g.tree_claims(lo, hi, rows, 0.0);
        // Every patch tree's cells (as the growers place them) and whether each is a leaf.
        let mut cells: Vec<([i32; 3], bool)> = Vec::new();
        for t in listed.iter().filter(|t| {
            (lo.0..=hi.0).contains(&t.anchor[0]) && (lo.1..=hi.1).contains(&t.anchor[2])
        }) {
            let turns = placed.iter().find(|p| p.anchor == t.anchor).expect("placed").quarter_turns;
            let p = dwell_worldgen::growth::stage_cells(t.species, t.seed, t.stage as u8, t.anchor, turns)
                .expect("a species tree");
            cells.extend(p.cells().map(|(c, s)| (c, dwell_blocks::block_of(s).name.contains("leaves"))));
        }
        cells.sort_unstable();
        cells.dedup_by_key(|c| c.0);
        let mut layers: HashMap<u32, [f32; 4]> = HashMap::new();
        let trees: Vec<tree_fade::Tree> = listed
            .iter()
            .zip(config.instances(&listed))
            .map(|(t, inst)| {
                let l = *layers.entry(inst.layer).or_insert_with(|| {
                    let b = config.bake_layer(inst.layer, 8).expect("a layer");
                    [b.centre[1], b.radius, 1.0, 0.0]
                });
                impostors::fade_tree(&inst, t.columns, t.rows, l)
            })
            .collect();
        let ground = trees.iter().map(|t| t.foot[1]).fold(f32::MAX, f32::min);
        let eye_y = f64::from(ground) + 60.0;
        let x = f64::from(lo.0 + hi.0) * 0.5;
        let mut fades = TreeFades::new(tree_fade::window());
        let mut now = 0.0;
        let mut failures: Vec<String> = Vec::new();
        let reach = tree_fade::leaves_reach(&far) + 2.0;
        let cell_h = dwell_world::CELL_H;
        let steps: Vec<i32> = {
            let path: Vec<i32> = (-520..=-220).step_by(3).collect();
            path.iter().chain(path.iter().rev()).copied().collect()
        };
        for z in steps {
            let e = f.point([x, eye_y, f64::from(z)]);
            for dt in [0.0, 5.0] {
                now += dt;
                fades.update(now, f, e, &far, || Box::new(trees.clone().into_iter()), |_| true);
            }
            let share = |a: [i32; 3]| fades.share_at(f64::from(a[0]) + 0.5, f64::from(a[2]) + 0.5);
            let billboard = |a: [i32; 3]| share(a) < 0.999;
            for &(c, leaf) in &cells {
                let Some(set) = claims.of(c) else { continue };
                let w = f.point([f64::from(c[0]) + 0.5, (f64::from(c[1]) + 0.5) * cell_h, f64::from(c[2]) + 0.5]);
                let d = (0..3).map(|k| (w[k] - e[k]).powi(2)).sum::<f64>().sqrt() as f32;
                let gated = impostors::tree_set_drawn(Some(far), set.iter().map(|&a| share(a)));
                let drawn = gated
                    && (!leaf
                        || (d <= reach
                            && impostors::cutout_draw(Some(far), chunk_distance(f, c, e)) == CutoutDraw::All
                            && share(set[0]) > 0.0));
                // Settled shares only (0 or 1): a fade is the one time both may draw.
                if set.iter().any(|&a| { let s = share(a); s > 0.0 && s < 1.0 }) {
                    continue;
                }
                let any_billboard = set.iter().any(|&a| billboard(a));
                if drawn && any_billboard && failures.len() < 12 {
                    failures.push(format!("eye z {z}: cell {c:?} ({}) drawn under the billboard of one of {set:?}", if leaf { "leaf" } else { "log" }));
                }
                if !drawn && !any_billboard && (!leaf || d <= reach) && failures.len() < 12 {
                    failures.push(format!("eye z {z}: cell {c:?} ({}) of real trees {set:?} missing at {d:.0} m", if leaf { "leaf" } else { "log" }));
                }
            }
        }
        assert!(!cells.is_empty());
        assert!(failures.is_empty(), "drawn other than once:\n{}", failures.join("\n"));
    }
}
