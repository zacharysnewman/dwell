//! Arches (ADR 0074 B8): openings carved through narrow fins, a biome's `arches` by data alone.

use std::collections::HashMap;

use dwell_blocks::StateId;
use dwell_world::{CELL_H, CHUNK_SIZE, Chunk, ChunkCoord, Face, M, chunk_of};
use dwell_worldgen::arches::Site;
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::terrain::{BIOMES, Biome, FIXTURE_BIOMES, TerrainGenerator};

fn fixture() -> Biome {
    *FIXTURE_BIOMES
        .iter()
        .find(|b| b.name == "test:arches")
        .expect("the arches fixture")
}

/// The fixture's sites over a 1 km square of seed 7's scarps.
fn sites(g: &TerrainGenerator) -> Vec<Site> {
    g.arches_near((18_500, 1_500), (19_500, 2_500))
}

/// World cells, chunks generated on demand.
struct Cells<'a> {
    t: &'a BifacialTerrain,
    chunks: HashMap<ChunkCoord, Chunk>,
}

impl Cells<'_> {
    fn get(&mut self, [x, y, z]: [i32; 3]) -> StateId {
        let c = chunk_of(x, y, z);
        let t = self.t;
        let chunk = self.chunks.entry(c).or_insert_with(|| {
            let mut k = Chunk::default();
            t.generate(c, &mut k);
            k
        });
        let s = CHUNK_SIZE;
        chunk.get(x - c.x * s, y - c.y * s, z - c.z * s)
    }
}

fn rows_of(m: f64) -> i32 {
    (m / CELL_H).floor() as i32
}

#[test]
fn the_default_world_has_no_arches() {
    // The build's biomes ask for none, so the default world's chunks are as before (the goldens).
    assert!(BIOMES.iter().all(|b| b.arches.is_none()));
    let t = BifacialTerrain::height_field(7);
    assert!(sites(t.local(Face::A)).is_empty());
}

dwell_world::both_faces!(
    /// With the fixture biome an arch stands: a column with rock above air above rock, the opening
    /// at least 2 m tall and the span over it at least its lintel; face B's is face A's mirror
    /// (the same cells through the frame).
    fn a_fixture_biome_carves_arches(f: FaceFrame) {
        let t = BifacialTerrain::mirrored_with_biomes(7, &[fixture()]);
        let g = t.local(f.face);
        let found = sites(g);
        assert!(found.len() >= 5, "arch sites in 1 km²: {}", found.len());
        assert!(
            found.iter().any(|s| s.through),
            "an arch through a fin, beside the scarps' overhangs"
        );
        let mut cells = Cells {
            t: &t,
            chunks: HashMap::new(),
        };
        let mut arches = 0;
        for s in found.iter().take(8) {
            let (x, z) = (s.centre[0].floor() as i32, s.centre[1].floor() as i32);
            let top = g.ground_y(x, z).expect("on the disc");
            let floor = rows_of(s.floor);
            let solid = |c: &mut Cells, y: i32| {
                let v = c.get([x, f.row(y), z]);
                v != M.air && v != M.water
            };
            let air: Vec<i32> = (floor..top).filter(|&y| !solid(&mut cells, y)).collect();
            let rock_over = solid(&mut cells, top) && air.last().is_some_and(|&a| a < top);
            if air.len() as f64 * CELL_H >= 2.0 && rock_over && solid(&mut cells, floor - 1) {
                arches += 1;
            }
        }
        assert!(
            arches >= 3,
            "{arches} of 8 sites show rock over air over rock"
        );
    }
);

/// An opening that crosses a chunk border is carved the same from both chunks: every column of
/// its footprint holds exactly the air its roof predicts, whichever chunk generated it.
#[test]
fn arches_agree_across_chunk_seams() {
    // face-a-only: the carve is face-local (both faces' parity is the test above).
    let t = BifacialTerrain::mirrored_with_biomes(7, &[fixture()]);
    let g = t.local(Face::A);
    let all = sites(g);
    let mut cells = Cells {
        t: &t,
        chunks: HashMap::new(),
    };
    let mut crossing = 0;
    for s in &all {
        let r = s.reach().ceil() as i32;
        let (cx, cz) = (s.centre[0].floor() as i32, s.centre[1].floor() as i32);
        let mut chunk_cols = std::collections::HashSet::new();
        let mut carved = 0;
        for z in cz - r..=cz + r {
            for x in cx - r..=cx + r {
                let (mx, mz) = (f64::from(x) + 0.5, f64::from(z) + 0.5);
                if s.roof(mx, mz).is_none() {
                    continue;
                }
                let col = g.column_at(x, z);
                let top = g.ground_y(x, z).unwrap();
                // Every site's predicted air in this column (sites may overlap).
                let predicted = |y: i32| {
                    let mid = (f64::from(y) + 0.5) * CELL_H;
                    all.iter()
                        .any(|o| o.carves(mx, mz, f64::from(col.height), mid))
                };
                if col.water != dwell_worldgen::terrain::Water::None {
                    continue;
                }
                for y in rows_of(s.floor) - 1..=top {
                    let air = cells.get([x, y, z]) == M.air;
                    assert_eq!(air, predicted(y), "column ({x}, {z}) row {y}");
                    if air {
                        carved += 1;
                        chunk_cols.insert((x.div_euclid(CHUNK_SIZE), z.div_euclid(CHUNK_SIZE)));
                    }
                }
            }
        }
        if carved > 0 && chunk_cols.len() > 1 {
            crossing += 1;
        }
        if crossing >= 2 {
            break;
        }
    }
    assert!(crossing >= 2, "arches crossing a chunk border: {crossing}");
}

#[test]
fn arches_are_validated() {
    use dwell_worldgen::config::biome_from_json;
    let text = include_str!("../../../shared/worldgen/biomes/fixtures/arches.json");
    let good: serde_json::Value = serde_json::from_str(text).unwrap();
    let b = biome_from_json(&good).expect("the fixture parses");
    assert_eq!(
        b.arches,
        fixture().arches,
        "the parser reads what gen.mjs emits"
    );
    for (k, v) in [
        ("span", serde_json::json!([18, 6])),
        ("rise", serde_json::json!([0, 4])),
        ("fin", serde_json::json!(200)),
        ("density", serde_json::json!(-1)),
    ] {
        let mut bad = good.clone();
        bad["arches"][k] = v;
        let e = biome_from_json(&bad).expect_err(k);
        assert!(e.contains(&format!("arches.{k}")), "{e}");
    }
    // The fin rule and the rock filter (0.52.66), by data alone.
    let mut fins = good.clone();
    fins["arches"]["fins"] = serde_json::json!({ "approach": 150, "width": 12 });
    fins["arches"]["rock"] = serde_json::json!("dwell:sandstone");
    let a = biome_from_json(&fins).unwrap().arches.unwrap();
    assert_eq!(
        a.fins,
        Some(dwell_worldgen::arches::Fins {
            approach: 150.0,
            width: 12.0,
            overrun: 0.0
        })
    );
    assert_eq!(a.rock, Some("dwell:sandstone"));
    let mut bad = fins.clone();
    bad["arches"]["fins"]["width"] = serde_json::json!(1);
    let e = biome_from_json(&bad).expect_err("a 1 m fin");
    assert!(e.contains("arches.fins.width"), "{e}");
    let mut bad = fins.clone();
    bad["arches"]["rock"] = serde_json::json!(3);
    assert!(biome_from_json(&bad).is_err(), "a rock is a name");
    let mut off = good.clone();
    off["arches"]["density"] = serde_json::json!(0);
    assert_eq!(biome_from_json(&off).unwrap().arches, None);
}

/// Writes vertical slices through the fixture's first arch and first overhang as PPM images to
/// `DWELL_ARCH_SLICES` (a directory): across the opening (the half ellipse under its lintel) and
/// along it (the way through). A look at the carve without a browser; nothing is asserted.
#[test]
#[ignore = "writes images: DWELL_ARCH_SLICES=<dir> cargo test --test arches -- --ignored"]
fn arch_slices() {
    let Ok(out_dir) = std::env::var("DWELL_ARCH_SLICES") else {
        return;
    };
    let t = BifacialTerrain::mirrored_with_biomes(7, &[fixture()]);
    let g = t.local(Face::A);
    let all = sites(g);
    // The game's world has the terrain volume (the cliffs) over the ground: its sites, for a look
    // in game (`?worldgen=` an edit of the fixture, `at=` near one).
    if let Some(v) = dwell_worldgen::volume::terrain_volume() {
        let gv = g.clone().with_volume(v);
        for s in sites(&gv).iter().filter(|s| s.through).take(3) {
            println!("in game (volume): {s:?}");
        }
    }
    let mut cells = Cells {
        t: &t,
        chunks: HashMap::new(),
    };
    for (name, s) in [
        ("arch", all.iter().find(|s| s.through)),
        ("overhang", all.iter().find(|s| !s.through)),
    ] {
        let Some(s) = s else { continue };
        let across = [-s.axis[1], s.axis[0]];
        for (view, dir) in [("across", across), ("along", s.axis)] {
            let (w, lo, hi) = (96_i32, rows_of(s.floor) - 24, rows_of(s.floor) + 72);
            let scale = 4;
            let mut img = Vec::new();
            for y in (lo..hi).rev() {
                for _ in 0..scale {
                    for i in 0..w * 2 {
                        let d = f64::from(i - w) * 0.5;
                        let x = (s.centre[0] + dir[0] * d).floor() as i32;
                        let z = (s.centre[1] + dir[1] * d).floor() as i32;
                        let v = cells.get([x, y, z]);
                        let rgb: [u8; 3] = if v == M.air {
                            [170, 205, 235]
                        } else if v == M.water {
                            [40, 90, 200]
                        } else {
                            let b = if (y + x + z) % 2 == 0 { 0 } else { 12 };
                            [150 + b, 85 + b, 55 + b]
                        };
                        for _ in 0..scale {
                            img.extend_from_slice(&rgb);
                        }
                    }
                }
            }
            let mut out =
                format!("P6\n{} {}\n255\n", w * 2 * scale, (hi - lo) * scale).into_bytes();
            out.extend_from_slice(&img);
            std::fs::write(format!("{out_dir}/{name}_{view}.ppm"), out).unwrap();
        }
        println!("{name}: {s:?}");
    }
}
