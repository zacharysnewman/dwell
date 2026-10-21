//! The flood light (docs/DAY_NIGHT.md §11): tunnels and caves, edits undone, load order, chunk
//! borders, emitters, both faces; every incremental update checked against a relight from
//! scratch.

use super::*;
use crate::{FaceFrame, MIDPLANE_Y, VoxelWorld, empty_generator};

const NC: i32 = CHUNK_SIZE;
/// The face-local chunk row the fixtures stand in, and its first row.
const ROW: i32 = MIDPLANE_Y.div_euclid(NC) + 2;
const BASE: i32 = ROW * NC;
/// The ground's top (its first air row).
const GROUND: i32 = BASE + 8;

fn rock() -> StateId {
    dwell_blocks::require("dwell:sandbox_floor")
}

/// The fixture's emitter (`shared/blocks/fixtures/test.json` `test:lamp`), registered from its
/// data as a server mod would (ADR 0038): no build block emits light yet.
fn lamp() -> StateId {
    static ID: std::sync::OnceLock<StateId> = std::sync::OnceLock::new();
    *ID.get_or_init(|| {
        dwell_blocks::state("test:lamp").unwrap_or_else(|| {
            let mut row = *dwell_blocks::fixture::STATES
                .iter()
                .find(|s| s.name == "test:lamp")
                .expect("the fixture has test:lamp");
            row.shape_index = dwell_blocks::info(rock()).shape_index;
            dwell_blocks::register(
                dwell_blocks::Origin::Server,
                dwell_blocks::NewBlock {
                    name: "test:lamp".into(),
                    properties: Vec::new(),
                    states: vec![row],
                },
            )
            .expect("registers")
        })
    })
}

/// Face-local cells built on face `f`, then held by a lit world chunk by chunk.
struct Fixture {
    f: FaceFrame,
    src: VoxelWorld,
}

impl Fixture {
    fn new(f: FaceFrame) -> Self {
        Self {
            f,
            src: VoxelWorld::new(empty_generator(), None),
        }
    }

    fn fill(&mut self, lo: [i32; 3], hi: [i32; 3], id: StateId) {
        for y in lo[1]..hi[1] {
            for z in lo[2]..hi[2] {
                for x in lo[0]..hi[0] {
                    let [wx, wy, wz] = self.f.cell([x, y, z]);
                    self.src.set_voxel(wx, wy, wz, id);
                }
            }
        }
    }

    /// The world chunks of face-local chunks x in `xs`, rows ROW + `dys`, z in `zs`.
    fn chunks(
        &self,
        xs: std::ops::RangeInclusive<i32>,
        dys: std::ops::RangeInclusive<i32>,
        zs: std::ops::RangeInclusive<i32>,
    ) -> Vec<ChunkCoord> {
        let mut out = Vec::new();
        for dy in dys {
            for z in zs.clone() {
                for x in xs.clone() {
                    out.push(self.f.chunk(ChunkCoord::new(x, ROW + dy, z)));
                }
            }
        }
        out
    }

    /// A lit world holding `order`'s chunks, loaded in that order.
    fn lit(&mut self, order: &[ChunkCoord]) -> VoxelWorld {
        let mut w = VoxelWorld::streamed();
        w.enable_light();
        for &c in order {
            self.load(&mut w, c);
        }
        w
    }

    fn load(&mut self, w: &mut VoxelWorld, c: ChunkCoord) {
        if let Some(chunk) = self.src.find(c) {
            w.put(c, chunk.clone());
        }
        w.light_load(c);
    }

    /// The sky and block light of face-local cell `p`.
    fn at(&self, w: &VoxelWorld, p: [i32; 3]) -> (u8, u8) {
        let v = w.light().expect("lit").get(self.f.cell(p)).expect("held");
        (sky_of(v), block_of(v))
    }
}

/// Every held cell's light, by world cell.
fn snapshot(w: &VoxelWorld, chunks: &[ChunkCoord]) -> Vec<u8> {
    snapshot_of(w.light().expect("lit"), chunks)
}

fn snapshot_of(l: &LightMap, chunks: &[ChunkCoord]) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunks.len() * CHUNK_VOLUME);
    let mut sorted = chunks.to_vec();
    sorted.sort_by_key(|c| (c.x, c.y, c.z));
    for c in sorted {
        for i in 0..CHUNK_VOLUME as u16 {
            let [x, y, z] = coords(i);
            out.push(
                l.get([c.x * NC + x, c.y * NC + y, c.z * NC + z])
                    .expect("held"),
            );
        }
    }
    out
}

/// The world relit from nothing agrees with what the updates left.
fn assert_matches_scratch(w: &mut VoxelWorld, chunks: &[ChunkCoord], what: &str) {
    let before = snapshot(w, chunks);
    let voxels = w.chunk_table();
    let mut fresh = LightMap::new();
    for &c in chunks {
        fresh.load(voxels, c);
    }
    fresh.relit_from_scratch(voxels);
    let after = snapshot_of(&fresh, chunks);
    let differ = before.iter().zip(&after).filter(|(x, y)| x != y).count();
    assert_eq!(
        differ, 0,
        "{what}: {differ} cells differ from a relight from scratch"
    );
}

/// Ground, and a hill from x = 4 on, 10 m over the ground, with a 2 m × 2 m tunnel 20 m into it.
fn tunnel(f: FaceFrame) -> Fixture {
    let mut fx = Fixture::new(f);
    fx.fill([-40, BASE, -40], [72, GROUND, 72], rock());
    fx.fill([4, GROUND, -40], [72, GROUND + 20, 72], rock());
    fx.fill([4, GROUND, 15], [24, GROUND + 4, 17], 0);
    fx
}

crate::both_faces!(
    /// Daylight falls off into a carved tunnel: a level a metre from its mouth, dark 15 m in.
    fn a_tunnel_darkens_with_depth(f: FaceFrame) {
        let mut fx = tunnel(f);
        let chunks = fx.chunks(-1..=1, -1..=1, -1..=1);
        let w = fx.lit(&chunks);
        let floor = |x| fx.at(&w, [x, GROUND, 16]).0;
        assert_eq!(
            fx.at(&w, [0, GROUND, 16]).0,
            LIGHT_MAX,
            "open ground outside"
        );
        assert_eq!(floor(4), LIGHT_MAX - 1, "under the mouth's roof");
        for x in 5..=14 {
            assert!(floor(x) < floor(x - 1), "falls off at {x} m: {}", floor(x));
        }
        assert!(floor(9) <= 10, "5 m in: {}", floor(9));
        assert!(floor(14) <= 5, "10 m in: {}", floor(14));
        assert_eq!(floor(19), 0, "15 m in");
        assert!(
            LIGHT.sky(f64::from(floor(14))) < 0.15,
            "10 m in is near the floor"
        );
        assert_eq!(LIGHT.direct(f64::from(floor(14))), 0.0, "no sun 10 m in");
        // Its walls and ceiling fall off with it.
        assert_eq!(fx.at(&w, [19, GROUND + 3, 15]).0, 0);
    }
);

crate::both_faces!(
    /// A tunnel under a hill, across chunks and longer than any mesh job's data, is dark along
    /// its length but near its mouth.
    fn a_long_tunnel_under_a_hill_is_dark(f: FaceFrame) {
        let mut fx = Fixture::new(f);
        fx.fill([-40, BASE, -40], [104, GROUND, 72], rock());
        // A hill 30 m tall (rows into the chunk row above).
        fx.fill([4, GROUND, -40], [104, GROUND + 60, 72], rock());
        fx.fill([4, GROUND, 15], [96, GROUND + 4, 17], 0);
        let chunks = fx.chunks(-1..=3, -1..=2, -1..=1);
        let w = fx.lit(&chunks);
        for x in 19..96 {
            assert_eq!(fx.at(&w, [x, GROUND, 16]).0, 0, "{x} m");
        }
    }
);

crate::both_faces!(
    /// A cave roofed 10 m over its floor (no sky ray reaches the roof) is dark, and a shaft to the
    /// sky lights the floor under it.
    fn a_tall_cave_is_dark_and_a_shaft_lights_it(f: FaceFrame) {
        let mut fx = Fixture::new(f);
        fx.fill([-40, BASE, -40], [72, GROUND + 24, 72], rock());
        fx.fill([4, GROUND, 4], [28, GROUND + 20, 28], 0);
        let chunks = fx.chunks(-1..=1, -1..=1, -1..=1);
        let mut w = fx.lit(&chunks);
        assert_eq!(fx.at(&w, [16, GROUND, 16]).0, 0);
        // A 1 m shaft from the cave's roof to the hill's top.
        let shaft: Vec<[i32; 3]> = (GROUND + 20..GROUND + 24)
            .map(|y| f.cell([16, y, 16]))
            .collect();
        for &[x, y, z] in &shaft {
            w.set_voxel(x, y, z, 0);
        }
        w.light_edited(&shaft);
        assert_eq!(fx.at(&w, [16, GROUND, 16]).0, LIGHT_MAX, "under the shaft");
        assert_eq!(fx.at(&w, [18, GROUND, 16]).0, LIGHT_MAX - 2, "beside it");
        assert_matches_scratch(&mut w, &chunks, "after the shaft");
    }
);

crate::both_faces!(
    /// Carving a tunnel and filling it back restores the light exactly (removal and addition).
    fn placing_a_block_back_restores_the_light(f: FaceFrame) {
        let mut fx = tunnel(f);
        let chunks = fx.chunks(-1..=1, -1..=1, -1..=1);
        let mut w = fx.lit(&chunks);
        let before = snapshot(&w, &chunks);
        // Dig 3 m on, then put it back.
        let dig: Vec<[i32; 3]> = (24..27)
            .flat_map(|x| (GROUND..GROUND + 4).flat_map(move |y| (15..17).map(move |z| [x, y, z])))
            .map(|p| f.cell(p))
            .collect();
        for &[x, y, z] in &dig {
            w.set_voxel(x, y, z, 0);
        }
        w.light_edited(&dig);
        assert!(fx.at(&w, [26, GROUND, 16]).0 < fx.at(&w, [23, GROUND, 16]).0.max(1));
        assert_matches_scratch(&mut w, &chunks, "dug");
        for &[x, y, z] in &dig {
            w.set_voxel(x, y, z, rock());
        }
        w.light_edited(&dig);
        assert_eq!(snapshot(&w, &chunks), before, "filled back");
        // A block placed at the mouth darkens the tunnel; taken away, it lights it again.
        let mouth = [f.cell([4, GROUND + 1, 16])];
        let [x, y, z] = mouth[0];
        w.set_voxel(x, y, z, rock());
        w.light_edited(&mouth);
        assert_matches_scratch(&mut w, &chunks, "a block at the mouth");
        w.set_voxel(x, y, z, 0);
        w.light_edited(&mouth);
        assert_eq!(snapshot(&w, &chunks), before, "taken away");
    }
);

crate::both_faces!(
    /// The light is the same whatever order the chunks arrive in, and the same as a relight from
    /// scratch, across chunk borders (the tunnel crosses into the next chunk and the hill into the
    /// row above).
    fn light_is_the_same_whatever_the_load_order(f: FaceFrame) {
        let mut fx = tunnel(f);
        // Leaves over open ground and an emitter in the tunnel, across borders.
        fx.fill(
            [-6, GROUND + 4, 28],
            [2, GROUND + 8, 36],
            dwell_blocks::require("dwell:leaves"),
        );
        fx.fill([22, GROUND, 16], [23, GROUND + 1, 17], lamp());
        let chunks = fx.chunks(-1..=1, -1..=1, -1..=1);
        let reference = {
            let mut w = fx.lit(&chunks);
            assert_matches_scratch(&mut w, &chunks, "in order");
            snapshot(&w, &chunks)
        };
        let mut seed = 12345u64;
        for round in 0..4 {
            let mut order = chunks.clone();
            for i in (1..order.len()).rev() {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                order.swap(i, (seed >> 33) as usize % (i + 1));
            }
            if round == 0 {
                order.reverse();
            }
            let w = fx.lit(&order);
            assert_eq!(snapshot(&w, &chunks), reference, "order {round}");
        }
        // Unloading the chunks above and loading them again, too.
        let mut w = fx.lit(&chunks);
        let above = fx.chunks(-1..=1, 1..=1, -1..=1);
        for &c in &above {
            w.remove(c);
            w.light_unload(c);
        }
        let rest: Vec<ChunkCoord> = chunks
            .iter()
            .copied()
            .filter(|c| !above.contains(c))
            .collect();
        assert_matches_scratch(&mut w, &rest, "the row above unloaded");
        for &c in &above {
            fx.load(&mut w, c);
        }
        assert_eq!(snapshot(&w, &chunks), reference, "loaded again");
    }
);

crate::both_faces!(
    /// Unloading relights only the light that came from the chunks that go (the 0.22.9 stalls:
    /// each dropped chunk cleared and refilled every open-air column beside and below it, a
    /// 1–2 s long task when a fast flight dropped a batch). The open air over a hill goes one
    /// chunk at a time from the top (flying down), and a column five chunks tall goes at once
    /// (flying past): the light agrees with a relight from scratch, and the chunks left, whose
    /// light doesn't change but where the column's lamp reached, are hardly written.
    fn unloading_relights_only_what_came_from_the_chunks(f: FaceFrame) {
        let mut fx = tunnel(f);
        // A lamp on the hill in the column that goes, its light reaching over the border.
        fx.fill([33, GROUND + 20, 16], [34, GROUND + 21, 17], lamp());
        let chunks = fx.chunks(-1..=1, -1..=3, -1..=1);
        let column = fx.chunks(1..=1, -1..=3, 0..=0);
        let rest: Vec<ChunkCoord> = chunks
            .iter()
            .copied()
            .filter(|c| !column.contains(c))
            .collect();
        // What the lamp lit outside the column, cleared and refilled dark: two writes a cell.
        let lamp_lit = {
            let w = fx.lit(&chunks);
            let l = w.light().expect("lit");
            snapshot_of(l, &rest)
                .iter()
                .filter(|&&v| block_of(v) > 0)
                .count() as u64
        };
        assert!(lamp_lit > 0, "the lamp lights across the border");
        // Generous: two writes for each cell of the column's four side faces, besides the lamp's.
        let bound = 2 * lamp_lit + 2 * 4 * 4 * (NC * NC) as u64;

        let mut w = fx.lit(&chunks);
        let before = w.light().expect("lit").writes();
        let air = fx.chunks(1..=1, 1..=3, 0..=0);
        for &c in air.iter().rev() {
            w.remove(c);
            w.light_unload(c);
        }
        let one_by_one = w.light().expect("lit").writes() - before;
        let left: Vec<ChunkCoord> = chunks
            .iter()
            .copied()
            .filter(|c| !air.contains(c))
            .collect();
        assert_matches_scratch(
            &mut w,
            &left,
            "the air unloaded from the top, one at a time",
        );

        let mut w = fx.lit(&chunks);
        let before = w.light().expect("lit").writes();
        for &c in &column {
            w.remove(c);
        }
        w.light_unload_many(&column);
        let together = w.light().expect("lit").writes() - before;
        assert_matches_scratch(&mut w, &rest, "the column unloaded at once");

        assert!(
            one_by_one <= bound && together <= bound,
            "unloading wrote {one_by_one} cells one at a time and {together} at once \
             (bound {bound}): the relight cascaded through chunks whose light didn't change"
        );
        // And loading the column back gives the light it had.
        let reference = snapshot(&fx.lit(&chunks), &chunks);
        for &c in &column {
            fx.load(&mut w, c);
        }
        assert_eq!(snapshot(&w, &chunks), reference, "loaded again");
    }
);

crate::both_faces!(
    /// Random batches of chunks unloaded and loaded again, in random order, over the tunnel, the
    /// hill, leaves and a lamp: the light after each agrees with a relight from scratch (the
    /// unload's and the load's shortcuts change nothing).
    fn random_loads_and_unloads_agree_with_a_relight_from_scratch(f: FaceFrame) {
        let mut fx = tunnel(f);
        fx.fill(
            [-6, GROUND + 4, 28],
            [2, GROUND + 8, 36],
            dwell_blocks::require("dwell:leaves"),
        );
        fx.fill([33, GROUND + 20, 16], [34, GROUND + 21, 17], lamp());
        fx.fill([22, GROUND, 16], [23, GROUND + 1, 17], lamp());
        let all = fx.chunks(-1..=1, -1..=2, -1..=1);
        let mut w = fx.lit(&all);
        let mut held = all.clone();
        let mut out: Vec<ChunkCoord> = Vec::new();
        let mut seed = 99u64;
        let mut next = |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize % n.max(1)
        };
        for round in 0..6 {
            if round % 2 == 0 || out.is_empty() {
                let k = 1 + next(6);
                let batch: Vec<ChunkCoord> = (0..k).map(|_| held[next(held.len())]).collect();
                for &c in &batch {
                    w.remove(c);
                }
                w.light_unload_many(&batch);
                held.retain(|c| !batch.contains(c));
                for c in batch {
                    if !out.contains(&c) {
                        out.push(c);
                    }
                }
            } else {
                for _ in 0..1 + next(out.len()) {
                    let c = out.swap_remove(next(out.len()));
                    fx.load(&mut w, c);
                    held.push(c);
                }
            }
            assert_matches_scratch(&mut w, &held, &format!("round {round}"));
        }
    }
);

crate::both_faces!(
    /// A chunk held over open air the sky lit (flying up: the chunks above arrive after the ones
    /// below) relights only the columns it covers: under its overhang, not the open air beside it,
    /// which kept the sky (0.22.9 cleared and refilled every column under it, 100–150 thousand
    /// cell writes, 5–9 ms native a chunk). Leaves in it filter the sky under them.
    fn a_chunk_over_open_air_relights_only_the_columns_it_covers(f: FaceFrame) {
        let mut fx = Fixture::new(f);
        fx.fill([-40, BASE, -40], [72, GROUND, 72], rock());
        // In the row above: an 8 × 8 overhang 4 m thick, and leaves beside it.
        fx.fill([4, BASE + NC + 2, 4], [12, BASE + NC + 6, 12], rock());
        fx.fill(
            [16, BASE + NC + 2, 4],
            [20, BASE + NC + 4, 8],
            dwell_blocks::require("dwell:leaves"),
        );
        let ground = fx.chunks(-1..=1, 0..=0, -1..=1);
        let top = f.chunk(ChunkCoord::new(0, ROW + 1, 0));
        let mut w = fx.lit(&ground);
        let before = w.light().expect("lit").writes();
        fx.load(&mut w, top);
        let writes = w.light().expect("lit").writes() - before;
        let mut all = ground.clone();
        all.push(top);
        assert_matches_scratch(&mut w, &all, "the chunk above loaded last");
        assert!(
            fx.at(&w, [8, GROUND, 8]).0 < LIGHT_MAX,
            "shaded under the overhang"
        );
        assert_eq!(fx.at(&w, [24, GROUND, 24]).0, LIGHT_MAX, "open ground");
        // Its own cells lit once, and the columns it shades (with what spreads beside them).
        let bound = CHUNK_VOLUME as u64 + 2 * 24 * 24 * (NC as u64);
        assert!(
            writes <= bound,
            "loading the chunk wrote {writes} cells (bound {bound}): the relight cascaded down \
             the open air under it"
        );
    }
);

crate::both_faces!(
    /// An emitter (the fixture lamp, `emits` by data alone) lights the cells around it, falling
    /// off with distance, through a dark tunnel and across a chunk border.
    fn the_fixture_lamp_lights_its_surroundings(f: FaceFrame) {
        let lamp = lamp();
        assert_eq!(dwell_blocks::info(lamp).emits, 14);
        let mut fx = tunnel(f);
        let chunks = fx.chunks(-1..=1, -1..=1, -1..=1);
        let mut w = fx.lit(&chunks);
        assert_eq!(fx.at(&w, [20, GROUND, 16]).1, 0, "no block light before");
        let at = f.cell([22, GROUND + 3, 16]);
        w.set_voxel(at[0], at[1], at[2], lamp);
        w.light_edited(&[at]);
        assert_eq!(fx.at(&w, [22, GROUND + 3, 16]).1, 14, "its own cell");
        assert_eq!(fx.at(&w, [22, GROUND + 2, 16]).1, 13, "under it");
        assert_eq!(fx.at(&w, [20, GROUND + 3, 16]).1, 12, "2 m along");
        assert_eq!(
            fx.at(&w, [22, GROUND, 16]).1,
            11,
            "the floor under it, 1.5 m down"
        );
        assert_eq!(fx.at(&w, [22, GROUND, 16]).0, 0, "no sky there");
        assert_matches_scratch(&mut w, &chunks, "lamp placed");
        w.set_voxel(at[0], at[1], at[2], 0);
        w.light_edited(&[at]);
        assert_eq!(fx.at(&w, [20, GROUND, 16]).1, 0, "gone with it");
    }
);

crate::both_faces!(
    /// Random edits across chunk borders, each followed by the incremental update, always agree
    /// with a relight from scratch.
    fn random_edits_agree_with_a_relight_from_scratch(f: FaceFrame) {
        let mut fx = tunnel(f);
        let chunks = fx.chunks(-1..=0, -1..=1, -1..=0);
        let mut w = fx.lit(&chunks);
        let leaves = dwell_blocks::require("dwell:leaves");
        let lamp = lamp();
        let ids = [0, rock(), leaves, lamp, 0, 0];
        let mut seed = 99u64;
        let mut rnd = |n: i32| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) % n as u64) as i32
        };
        for step in 0..60 {
            let mut cells = Vec::new();
            let id = ids[rnd(ids.len() as i32) as usize];
            let (x, y, z) = (rnd(16) - 2, GROUND - 2 + rnd(30), rnd(16) - 2);
            for k in 0..1 + rnd(6) {
                let c = f.cell([x + k, y, z]);
                w.set_voxel(c[0], c[1], c[2], id);
                cells.push(c);
            }
            w.light_edited(&cells);
            if step % 6 == 5 {
                assert_matches_scratch(&mut w, &chunks, &format!("step {step}"));
            }
        }
    }
);

crate::both_faces!(
    /// Under leaves the sky is dimmed, not shut out; under water it fades with depth.
    fn leaves_and_water_filter_the_sky(f: FaceFrame) {
        let mut fx = Fixture::new(f);
        fx.fill([-40, BASE, -40], [72, GROUND, 72], rock());
        fx.fill(
            [0, GROUND + 6, 0],
            [8, GROUND + 7, 8],
            dwell_blocks::require("dwell:leaves"),
        );
        fx.fill(
            [16, BASE + 2, 0],
            [24, GROUND, 8],
            dwell_blocks::require("dwell:water"),
        );
        let chunks = fx.chunks(-1..=1, -1..=1, -1..=1);
        let w = fx.lit(&chunks);
        let under = fx.at(&w, [4, GROUND, 4]).0;
        assert!(under > 0 && under < LIGHT_MAX, "under the canopy: {under}");
        let shallow = fx.at(&w, [20, GROUND - 1, 4]).0;
        let deep = fx.at(&w, [20, BASE + 2, 4]).0;
        assert!(shallow < LIGHT_MAX && deep < shallow, "{shallow} → {deep}");
    }
);

#[test]
fn the_brightness_curves_and_the_direct_ramp() {
    // face-a-only: functions of a level.
    let t = LIGHT;
    assert_eq!(t.sky(15.0), 1.0);
    assert_eq!(t.sky(0.0), 0.0);
    assert!(t.sky(14.0) < 1.0 && t.sky(7.0) < 0.5 * 7.0 / 15.0 + 0.1);
    assert_eq!(t.direct(15.0), 1.0);
    assert_eq!(t.direct(f64::from(t.direct_from)), 0.0);
    assert!(t.direct(13.0) > 0.0 && t.direct(13.0) < 1.0);
}

crate::both_faces!(
    /// A character's light (your arms, a player's body) follows the cells around it, smoothly as
    /// it moves, and eases over time: walking into the tunnel fades it rather than popping.
    fn a_characters_light_follows_the_cells_and_eases(f: FaceFrame) {
        let mut fx = tunnel(f);
        let chunks = fx.chunks(-1..=1, -1..=1, -1..=1);
        let w = fx.lit(&chunks);
        let l = w.light().expect("lit");
        // An eye 1.6 m over the tunnel's floor (face-local x metres along it), world metres.
        let eye = |x: f64| f.point([x, f64::from(GROUND) * crate::CELL_H + 1.6, 16.0]);
        let outside = l.sample(eye(-4.0));
        let mouth = l.sample(eye(4.5));
        let five = l.sample(eye(9.0));
        let deep = l.sample(eye(19.0));
        assert_eq!(outside, [1.0, 0.0], "open sky");
        assert!(mouth[0] < 1.0 && mouth[0] > five[0], "{mouth:?} {five:?}");
        assert_eq!(deep, [0.0, 0.0], "15 m in");
        // Smooth: a small step moves it a little.
        let (a, b) = (l.sample(eye(6.0))[0], l.sample(eye(6.1))[0]);
        assert!((a - b).abs() < 0.02 && a > b, "{a} {b}");
        // Eased: from the mouth into the dark, a frame moves it a little, a second most of the way.
        let mut e = EasedLight::default();
        assert_eq!(
            e.update(outside, 1.0 / 60.0, LIGHT.character_ease_s),
            outside
        );
        let one = e.update(deep, 1.0 / 60.0, LIGHT.character_ease_s)[0];
        assert!(one < 1.0 && one > 0.9, "after a frame: {one}");
        let mut v = one;
        for _ in 0..59 {
            v = e.update(deep, 1.0 / 60.0, LIGHT.character_ease_s)[0];
        }
        assert!(v < 0.05, "after a second: {v}");
        // An emitter's glow reaches it too.
        let mut w = fx.lit(&chunks);
        let at = f.cell([19, GROUND + 3, 16]);
        w.set_voxel(at[0], at[1], at[2], lamp());
        w.light_edited(&[at]);
        assert!(w.light().expect("lit").sample(eye(19.0))[1] > 0.7);
    }
);
